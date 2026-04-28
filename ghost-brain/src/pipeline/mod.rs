//! End-to-End Pipeline Runner
//!
//! This module implements the complete pipeline that connects:
//! Seer → Oracle → Features → DirectBuyBuilder → Trigger
//!
//! The pipeline is split into focused modules:
//! - `builder`: Pipeline initialization and configuration
//! - `stages`: Seer and Oracle/Features processing
//! - `execution`: DirectBuyBuilder/Trigger with Gatekeeper logic
//! - `jito_processor`: Jito batch submission and Revolver integration

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::E2EConfig;
use crate::events::EventEmitter;
use crate::guardian::post_buy::MonitoringEngine;
use crate::jito_bundle::JitoBundleExecutor;
use crate::leader_predictor::LeaderPredictor;
use crate::metrics::E2EMetrics;
use crate::oracle::{ClusterHunter, DevProfiler, HyperOracle, SnapshotEngine, VisionCritic};

use ghost_core::swap_plan::SwapPlan;
use gui_backend::{AppState, GuiBackend, GuiBackendConfig as BackendConfig};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use trigger::{PanicExecutor, Revolver};

// Module declarations
mod builder;
pub mod execution;
// Removed pub use since E2EPipeline is defined below
mod jito_processor;
mod stages;

/// Main E2E pipeline runner
pub struct E2EPipeline {
    /// Configuration
    config: E2EConfig,

    /// Metrics collector
    metrics: Arc<E2EMetrics>,

    /// Authority keypair
    authority: Keypair,

    /// Payer keypair
    payer: Keypair,

    /// GUI backend state (if enabled)
    gui_state: Option<Arc<AppState>>,

    /// Revolver for managing SELL positions (TP/SL)
    revolver: Arc<RwLock<Revolver>>,

    /// Leader Predictor for optimizing transaction timing (if enabled)
    leader_predictor: Option<Arc<LeaderPredictor>>,

    /// Jito Bundle Executor for high-throughput batch submission (if enabled)
    jito_executor: Option<Arc<JitoBundleExecutor>>,

    /// Shadow Ledger for zero-latency bonding curve state
    shadow_ledger: Arc<ghost_core::shadow_ledger::ShadowLedger>,

    /// TPU Connection Manager for Leapfrog strategy (if enabled)
    tpu_connection_manager: Option<Arc<trigger::TpuConnectionManager>>,

    /// Leader Resolver for getting validator TPU contact information
    leader_resolver: Option<Arc<trigger::LeaderResolver>>,

    /// Ghost Intelligence: DevProfiler for creator behavioral analysis
    profiler: Arc<DevProfiler>,

    /// Ghost Intelligence: ClusterHunter for cabal detection
    cluster_hunter: Arc<ClusterHunter>,

    /// Ghost Intelligence: VisionCritic for meme quality assessment
    vision_critic: Arc<VisionCritic>,

    /// HyperOracle: Advanced signal processing for early market analysis (T+2s window)
    hyper_oracle: Arc<HyperOracle>,

    /// SnapshotEngine: Real-time market snapshot system for accurate metrics
    snapshot_engine: Arc<SnapshotEngine>,

    /// Panic Executor: Isolated emergency sell path (UDP/Leapfrog)
    panic_executor: Option<Arc<PanicExecutor>>,

    /// Panic Bus: Critical signal channels for emergency situations
    /// These channels are monitored continuously by execution loops
    panic_signals: PanicSignals,

    /// PostBuy Guardian: Real-time position monitoring engine
    /// When enabled, monitors all active positions and routes signals to Revolver
    post_buy_guardian: Option<Arc<MonitoringEngine>>,

    /// Shared execution event emitter (single source for live hook instrumentation).
    execution_event_emitter: Option<Arc<EventEmitter>>,
    /// Optional secondary emitter for dual mode paper lane (shared writer/run_id).
    execution_event_emitter_paper: Option<Arc<EventEmitter>>,
}

/// Panic signal channels for the "Nervous System"
///
/// These channels carry critical signals that trigger immediate emergency sells
/// followed by process termination (Dead-Man Switch).
#[derive(Clone)]
pub struct PanicSignals {
    /// LIGMA veto signal: Liquidity trap or PSI imbalance detected
    pub ligma_veto_tx: mpsc::Sender<(Pubkey, u64)>, // (mint, amount)
    pub ligma_veto_rx: Arc<RwLock<mpsc::Receiver<(Pubkey, u64)>>>,

    /// QEDD survival signal: Survival probability < 0.5
    pub qedd_survival_tx: mpsc::Sender<(Pubkey, u64)>,
    pub qedd_survival_rx: Arc<RwLock<mpsc::Receiver<(Pubkey, u64)>>>,

    /// PARADOX anomaly signal: HFT manipulation detected
    pub paradox_anomaly_tx: mpsc::Sender<(Pubkey, u64)>,
    pub paradox_anomaly_rx: Arc<RwLock<mpsc::Receiver<(Pubkey, u64)>>>,

    /// CLUSTER cabal signal: Cabal distribution detected
    pub cluster_cabal_tx: mpsc::Sender<(Pubkey, u64)>,
    pub cluster_cabal_rx: Arc<RwLock<mpsc::Receiver<(Pubkey, u64)>>>,
}

impl PanicSignals {
    /// Create new panic signal channels
    pub fn new() -> Self {
        let (ligma_veto_tx, ligma_veto_rx) = mpsc::channel(10);
        let (qedd_survival_tx, qedd_survival_rx) = mpsc::channel(10);
        let (paradox_anomaly_tx, paradox_anomaly_rx) = mpsc::channel(10);
        let (cluster_cabal_tx, cluster_cabal_rx) = mpsc::channel(10);

        Self {
            ligma_veto_tx,
            ligma_veto_rx: Arc::new(RwLock::new(ligma_veto_rx)),
            qedd_survival_tx,
            qedd_survival_rx: Arc::new(RwLock::new(qedd_survival_rx)),
            paradox_anomaly_tx,
            paradox_anomaly_rx: Arc::new(RwLock::new(paradox_anomaly_rx)),
            cluster_cabal_tx,
            cluster_cabal_rx: Arc::new(RwLock::new(cluster_cabal_rx)),
        }
    }
}

impl E2EPipeline {
    /// Run the E2E pipeline
    pub async fn run(&self) -> Result<()> {
        info!("Starting E2E Pipeline (Zero-Cost Mode)");
        info!("Authority: {}", self.authority.pubkey());
        info!("Payer: {}", self.payer.pubkey());
        info!("Mode: Direct AMM Interaction (no on-chain program)");

        // Start Leader Predictor monitoring if enabled
        if let Some(ref predictor) = self.leader_predictor {
            info!("Starting LeaderPredictor background monitoring");
            predictor
                .start_monitoring()
                .await
                .context("Failed to start LeaderPredictor monitoring")?;
            info!("LeaderPredictor monitoring task spawned successfully");
        }

        // Create channels for component communication
        // NOTE: candidate_tx/rx removed - using fast_pipeline instead (zero-copy)
        let (swap_plan_tx, swap_plan_rx) = mpsc::channel::<SwapPlan>(100);

        // Start Seer component (now pushes directly to fast_pipeline)
        let seer_handle = self.start_seer().await?;

        // Start Oracle/Features processing (now using fast_pipeline batch consumer)
        let oracle_handle = self.start_oracle_features(swap_plan_tx).await?;

        // Start Direct Trigger execution (Zero-Cost mode)
        let trigger_handle = self.start_trigger(swap_plan_rx).await?;

        // Start metrics server if enabled
        if self.config.metrics.enable_prometheus {
            self.start_metrics_server().await?;
        }

        // Start SLA monitoring
        let sla_handle = self.start_sla_monitoring();

        // Start Panic Monitor (The Nervous System)
        let panic_handle = self.start_panic_monitor();

        // Start PostBuy Guardian monitoring (if enabled)
        let guardian_handle = if let Some(ref guardian) = self.post_buy_guardian {
            info!("Starting PostBuy Guardian monitoring engine");
            let engine = Arc::clone(guardian);
            Some(tokio::spawn(async move {
                engine.start().await;
            }))
        } else {
            info!("PostBuy Guardian is disabled");
            None
        };

        // Start GUI backend if enabled
        let gui_handle = if self.config.gui_backend.enabled {
            Some(self.start_gui_backend().await?)
        } else {
            None
        };

        info!("E2E Pipeline is now running");

        // Wait for all tasks
        if let Some(gui_handle) = gui_handle {
            tokio::select! {
                result = seer_handle => {
                    error!("Seer task ended: {:?}", result);
                }
                result = oracle_handle => {
                    error!("Oracle task ended: {:?}", result);
                }
                result = trigger_handle => {
                    error!("Trigger task ended: {:?}", result);
                }
                _ = sla_handle => {
                    error!("SLA monitoring task ended");
                }
                _ = panic_handle => {
                    error!("Panic monitor task ended");
                }
                result = gui_handle => {
                    error!("GUI backend task ended: {:?}", result);
                }
                _ = async {
                    match guardian_handle {
                        Some(h) => { let _ = h.await; }
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    error!("PostBuy Guardian task ended");
                }
            }
        } else {
            tokio::select! {
                result = seer_handle => {
                    error!("Seer task ended: {:?}", result);
                }
                result = oracle_handle => {
                    error!("Oracle task ended: {:?}", result);
                }
                result = trigger_handle => {
                    error!("Trigger task ended: {:?}", result);
                }
                _ = sla_handle => {
                    error!("SLA monitoring task ended");
                }
                _ = panic_handle => {
                    error!("Panic monitor task ended");
                }
                _ = async {
                    match guardian_handle {
                        Some(h) => { let _ = h.await; }
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    error!("PostBuy Guardian task ended");
                }
            }
        }

        Ok(())
    }

    /// Start metrics server
    async fn start_metrics_server(&self) -> Result<()> {
        info!(
            "Starting Prometheus metrics server on port {}",
            self.config.metrics.prometheus_port
        );

        // TODO: Implement Prometheus HTTP server
        // For now, just log that it would be started
        // In production, use prometheus_exporter or similar

        Ok(())
    }

    /// Start SLA monitoring task
    fn start_sla_monitoring(&self) -> tokio::task::JoinHandle<()> {
        let metrics: Arc<E2EMetrics> = Arc::clone(&self.metrics);
        let target_land_rate = self.config.metrics.target_land_rate;
        let target_inclusion_rate = self.config.metrics.target_inclusion_rate;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

            loop {
                interval.tick().await;

                // Check Land Rate
                let land_rate_pumpfun = metrics.update_land_rate("pumpfun");
                let land_rate_bonkfun = metrics.update_land_rate("bonkfun");

                info!(
                    "Land Rate - Pump.fun: {:.2}%, Bonk.fun: {:.2}%",
                    land_rate_pumpfun, land_rate_bonkfun
                );

                if land_rate_pumpfun < target_land_rate {
                    warn!(
                        "Land Rate for Pump.fun ({:.2}%) is below target ({:.2}%)",
                        land_rate_pumpfun, target_land_rate
                    );
                    metrics.check_land_rate_sla("pumpfun", target_land_rate);
                }

                if land_rate_bonkfun < target_land_rate {
                    warn!(
                        "Land Rate for Bonk.fun ({:.2}%) is below target ({:.2}%)",
                        land_rate_bonkfun, target_land_rate
                    );
                    metrics.check_land_rate_sla("bonkfun", target_land_rate);
                }

                // Check Inclusion Rate
                let inclusion_rate = metrics.update_inclusion_rate();
                info!("Inclusion Rate: {:.2}%", inclusion_rate);

                if inclusion_rate < target_inclusion_rate {
                    warn!(
                        "Inclusion Rate ({:.2}%) is below target ({:.2}%)",
                        inclusion_rate, target_inclusion_rate
                    );
                    metrics.check_inclusion_rate_sla(target_inclusion_rate);
                }
            }
        })
    }

    /// Start GUI backend server
    async fn start_gui_backend(&self) -> Result<tokio::task::JoinHandle<()>> {
        info!("Starting GUI backend server");

        let gui_config = BackendConfig {
            port: self.config.gui_backend.port,
            enabled: self.config.gui_backend.enabled,
            bind_address: self.config.gui_backend.bind_address.clone(),
        };

        let gui_state = self.gui_state.clone().unwrap();
        let backend = GuiBackend::with_state(gui_config, gui_state);

        let handle = tokio::spawn(async move {
            if let Err(e) = backend.run().await {
                error!("GUI backend error: {}", e);
            }
        });

        Ok(handle)
    }

    /// Get metrics
    pub fn metrics(&self) -> &E2EMetrics {
        &self.metrics
    }
}
