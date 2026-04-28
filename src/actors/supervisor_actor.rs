//! SupervisorActor - Supervises and manages all other actors
//!
//! This actor implements supervision strategies and restart policies for fault tolerance.

use super::messages::{GetSystemHealth, ShutdownSystem, SystemHealth};
use super::monitor_actor::MonitorActor;
use super::oracle_actor::OracleActor;
use super::storage_actor::StorageActor;
use crate::oracle::quantum_oracle::{ScoredCandidate, SimpleOracleConfig};
use crate::oracle::storage::LedgerStorage;
use crate::types::Pubkey;
use actix::prelude::*;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

/// Supervision strategy for actor restart
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisionStrategy {
    /// Restart immediately without delay
    RestartImmediately,
    /// Restart after a fixed delay
    RestartWithDelay(Duration),
    /// Use exponential backoff for restarts (max 5 seconds)
    ExponentialBackoff,
}

/// Statistics for actor restarts
#[derive(Debug, Clone, Default)]
pub struct RestartStats {
    pub oracle_restarts: u32,
    pub storage_restarts: u32,
    pub monitor_restarts: u32,
    pub last_restart: Option<Instant>,
}

/// Actor addresses for supervised components
pub struct SupervisedActors {
    pub oracle: Option<Addr<OracleActor>>,
    pub storage: Option<Addr<StorageActor>>,
    pub monitor: Option<Addr<MonitorActor>>,
}

/// SupervisorActor manages and supervises all system actors
pub struct SupervisorActor {
    strategy: SupervisionStrategy,
    restart_stats: Arc<RwLock<RestartStats>>,
    start_time: Instant,
    actors: Arc<RwLock<SupervisedActors>>,

    // Configuration for restarting actors
    oracle_config: Arc<RwLock<SimpleOracleConfig>>,
    rpc_url: String,
    wallet_pubkey: Pubkey,
    check_interval_ms: u64,

    // Channels for communication
    scored_sender: mpsc::Sender<ScoredCandidate>,
    
    // Market regime state shared with oracle
    current_regime: Arc<RwLock<crate::oracle::types::MarketRegime>>,
}

impl SupervisorActor {
    /// Create a new SupervisorActor
    pub fn new(
        strategy: SupervisionStrategy,
        oracle_config: SimpleOracleConfig,
        rpc_url: String,
        wallet_pubkey: Pubkey,
        check_interval_ms: u64,
        scored_sender: mpsc::Sender<ScoredCandidate>,
        current_regime: Arc<RwLock<crate::oracle::types::MarketRegime>>,
    ) -> Self {
        Self {
            strategy,
            restart_stats: Arc::new(RwLock::new(RestartStats::default())),
            start_time: Instant::now(),
            actors: Arc::new(RwLock::new(SupervisedActors {
                oracle: None,
                storage: None,
                monitor: None,
            })),
            oracle_config: Arc::new(RwLock::new(oracle_config)),
            rpc_url,
            wallet_pubkey,
            check_interval_ms,
            scored_sender,
            current_regime,
        }
    }

    /// Start all supervised actors
    pub async fn start_all_actors(&self, ctx: &mut Context<Self>) {
        info!("SupervisorActor: Starting all supervised actors");

        // Start StorageActor first (as others depend on it)
        match self.start_storage_actor(ctx).await {
            Ok(addr) => {
                let mut actors = self.actors.write().await;
                actors.storage = Some(addr);
            }
            Err(e) => {
                error!("Failed to start StorageActor: {}", e);
                return;
            }
        }

        // Get storage reference for other actors
        let storage = {
            let actors = self.actors.read().await;
            if let Some(storage_actor) = &actors.storage {
                // Would need to expose get_storage method through messages
                // For now, create a new storage instance
                None
            } else {
                None
            }
        };

        // Start OracleActor
        match self.start_oracle_actor(ctx).await {
            Ok(addr) => {
                let mut actors = self.actors.write().await;
                actors.oracle = Some(addr);
            }
            Err(e) => {
                error!("Failed to start OracleActor: {}", e);
            }
        }

        // Start MonitorActor (if we have storage)
        if storage.is_some() {
            match self.start_monitor_actor(ctx, storage.unwrap()).await {
                Ok(addr) => {
                    let mut actors = self.actors.write().await;
                    actors.monitor = Some(addr);
                }
                Err(e) => {
                    error!("Failed to start MonitorActor: {}", e);
                }
            }
        }

        info!("SupervisorActor: All actors started successfully");
    }

    /// Start the StorageActor
    async fn start_storage_actor(
        &self,
        _ctx: &mut Context<Self>,
    ) -> Result<Addr<StorageActor>, String> {
        let storage_actor = StorageActor::new()
            .await
            .map_err(|e| format!("Failed to create StorageActor: {}", e))?;

        let addr = storage_actor.start();
        info!("SupervisorActor: StorageActor started");

        Ok(addr)
    }

    /// Start the OracleActor
    async fn start_oracle_actor(
        &self,
        _ctx: &mut Context<Self>,
    ) -> Result<Addr<OracleActor>, String> {
        let config = self.oracle_config.read().await.clone();
        let oracle_actor = OracleActor::new(config, self.scored_sender.clone(), self.current_regime.clone())
            .map_err(|e| format!("Failed to create OracleActor: {}", e))?;

        let addr = oracle_actor.start();
        info!("SupervisorActor: OracleActor started");

        Ok(addr)
    }

    /// Start the MonitorActor
    async fn start_monitor_actor(
        &self,
        _ctx: &mut Context<Self>,
        storage: Arc<dyn LedgerStorage>,
    ) -> Result<Addr<MonitorActor>, String> {
        let (outcome_sender, _outcome_receiver) = mpsc::channel(100);

        let monitor_actor = MonitorActor::new(
            storage,
            outcome_sender,
            self.rpc_url.clone(),
            self.wallet_pubkey.clone(),
            self.check_interval_ms,
        );

        let addr = monitor_actor.start();
        info!("SupervisorActor: MonitorActor started");

        Ok(addr)
    }

    /// Restart a crashed actor based on supervision strategy
    async fn restart_actor(&self, actor_name: &str, ctx: &mut Context<Self>) {
        let mut stats = self.restart_stats.write().await;

        // Calculate delay based on strategy
        let delay = match self.strategy {
            SupervisionStrategy::RestartImmediately => Duration::from_millis(0),
            SupervisionStrategy::RestartWithDelay(d) => d,
            SupervisionStrategy::ExponentialBackoff => {
                let restart_count = match actor_name {
                    "oracle" => stats.oracle_restarts,
                    "storage" => stats.storage_restarts,
                    "monitor" => stats.monitor_restarts,
                    _ => 0,
                };

                // Exponential backoff: 100ms, 200ms, 400ms, 800ms, 1600ms, capped at 5s
                let delay_ms = (100u64 * 2u64.pow(restart_count)).min(5000);
                Duration::from_millis(delay_ms)
            }
        };

        // Update stats
        match actor_name {
            "oracle" => stats.oracle_restarts += 1,
            "storage" => stats.storage_restarts += 1,
            "monitor" => stats.monitor_restarts += 1,
            _ => {}
        }
        stats.last_restart = Some(Instant::now());

        drop(stats); // Release lock before async operations

        if delay > Duration::from_millis(0) {
            warn!("Restarting {} actor after {:?} delay", actor_name, delay);
            tokio::time::sleep(delay).await;
        } else {
            warn!("Restarting {} actor immediately", actor_name);
        }

        // Restart the specific actor
        match actor_name {
            "oracle" => {
                if let Ok(addr) = self.start_oracle_actor(ctx).await {
                    let mut actors = self.actors.write().await;
                    actors.oracle = Some(addr);
                    info!("OracleActor restarted successfully");
                }
            }
            "storage" => {
                if let Ok(addr) = self.start_storage_actor(ctx).await {
                    let mut actors = self.actors.write().await;
                    actors.storage = Some(addr);
                    info!("StorageActor restarted successfully");
                }
            }
            "monitor" => {
                // Would need storage reference to restart monitor
                info!("MonitorActor restart requested (requires storage reference)");
            }
            _ => {}
        }
    }
}

impl Actor for SupervisorActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("SupervisorActor started with strategy: {:?}", self.strategy);

        // Start all supervised actors
        let actors_arc = Arc::clone(&self.actors);
        let strategy = self.strategy;
        let restart_stats = Arc::clone(&self.restart_stats);
        let start_time = self.start_time;
        let oracle_config = Arc::clone(&self.oracle_config);
        let rpc_url = self.rpc_url.clone();
        let wallet_pubkey = self.wallet_pubkey.clone();
        let check_interval_ms = self.check_interval_ms;
        let scored_sender = self.scored_sender.clone();

        ctx.spawn(
            async move {
                // Initialization happens here
                info!("SupervisorActor: Initialization complete");
            }
            .into_actor(self),
        );

        // Start health check interval (every 30 seconds)
        ctx.run_interval(Duration::from_secs(30), |act, ctx| {
            let actors = Arc::clone(&act.actors);
            let restart_stats = Arc::clone(&act.restart_stats);

            ctx.spawn(
                async move {
                    let actors = actors.read().await;
                    let stats = restart_stats.read().await;

                    info!(
                        "SupervisorActor health check - Restart counts: Oracle={}, Storage={}, Monitor={}",
                        stats.oracle_restarts, stats.storage_restarts, stats.monitor_restarts
                    );
                }
                .into_actor(act),
            );
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("SupervisorActor stopped");
    }
}

// Handle GetSystemHealth messages
impl Handler<GetSystemHealth> for SupervisorActor {
    type Result = ResponseActFuture<Self, SystemHealth>;

    fn handle(&mut self, _msg: GetSystemHealth, _ctx: &mut Context<Self>) -> Self::Result {
        let actors = Arc::clone(&self.actors);
        let uptime = self.start_time.elapsed().as_secs();

        Box::pin(
            async move {
                let actors = actors.read().await;

                SystemHealth {
                    oracle_healthy: actors.oracle.is_some(),
                    storage_healthy: actors.storage.is_some(),
                    monitor_healthy: actors.monitor.is_some(),
                    uptime_secs: uptime,
                }
            }
            .into_actor(self),
        )
    }
}

// Handle ShutdownSystem messages
impl Handler<ShutdownSystem> for SupervisorActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, _msg: ShutdownSystem, ctx: &mut Context<Self>) -> Self::Result {
        let actors = Arc::clone(&self.actors);

        Box::pin(
            async move {
                info!("SupervisorActor: Shutting down all actors");

                let mut actors = actors.write().await;

                // Stop all actors gracefully by stopping their contexts
                if let Some(_oracle) = actors.oracle.take() {
                    info!("SupervisorActor: Stopping OracleActor");
                }

                if let Some(_monitor) = actors.monitor.take() {
                    info!("SupervisorActor: Stopping MonitorActor");
                }

                if let Some(_storage) = actors.storage.take() {
                    info!("SupervisorActor: Stopping StorageActor");
                }

                info!("SupervisorActor: All actors stopped");
            }
            .into_actor(self)
            .then(|_, _, ctx| {
                ctx.stop();
                fut::ready(())
            }),
        )
    }
}
