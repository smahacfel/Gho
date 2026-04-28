//! Orchestrator configuration and coordination for the H-5N1P3R system
//!
//! This module provides configuration management, channel factories, and graceful shutdown
//! coordination for all system components.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::Level;

use crate::oracle::{
    DecisionLedger, DecisionRecordSender, FeatureWeights, LedgerStorage, MarketRegime,
    MonitoredTransaction, OracleConfig, OracleDataSources, PerformanceMonitor, PredictiveOracle,
    ScoreThresholds, StrategyOptimizer, TransactionRecord,
};
use crate::types::PremintCandidate;

/// Main configuration for the entire H-5N1P3R system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    pub database_path: String,
    pub rpc_url: String,
    pub wallet_pubkey: String,
    pub rpc_timeout_secs: u64,
    pub channel_buffer_size: usize,
    pub perf_monitor: PerformanceMonitorConfig,
    pub tx_monitor: TransactionMonitorConfig,
    pub regime_detector: RegimeDetectorConfig,
    pub feature_worker: FeatureWorkerConfigOrchestrator,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMonitorConfig {
    pub analysis_interval_minutes: u64,
    pub lookback_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionMonitorConfig {
    pub check_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeDetectorConfig {
    pub analysis_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureWorkerConfigOrchestrator {
    pub max_concurrent_fetches: usize,
    pub fetch_timeout_secs: u64,
    pub retry_attempts: usize,
    pub queue_capacity: usize,
}

impl FeatureWorkerConfigOrchestrator {
    /// Convert to the actual worker configuration
    pub fn to_worker_config(&self) -> crate::features::FeatureWorkerConfig {
        crate::features::FeatureWorkerConfig {
            max_concurrent_fetches: self.max_concurrent_fetches,
            fetch_timeout: Duration::from_secs(self.fetch_timeout_secs),
            retry_attempts: self.retry_attempts,
            queue_capacity: self.queue_capacity,
        }
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            database_path: "decisions.db".to_string(),
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            wallet_pubkey: "11111111111111111111111111111112".to_string(),
            rpc_timeout_secs: 30,
            channel_buffer_size: 100,
            perf_monitor: PerformanceMonitorConfig {
                analysis_interval_minutes: 15,
                lookback_hours: 24,
            },
            tx_monitor: TransactionMonitorConfig {
                check_interval_ms: 1000,
            },
            regime_detector: RegimeDetectorConfig {
                analysis_interval_secs: 60,
            },
            feature_worker: FeatureWorkerConfigOrchestrator {
                max_concurrent_fetches: 10,
                fetch_timeout_secs: 30,
                retry_attempts: 3,
                queue_capacity: 1000,
            },
            log_level: "info".to_string(),
        }
    }
}

impl OrchestratorConfig {
    pub fn from_toml_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let mut config: OrchestratorConfig = toml::from_str(&contents)?;

        // Override with environment variables if present (for secrets)
        if let Ok(rpc_url) = std::env::var("RPC_URL") {
            config.rpc_url = rpc_url;
        }
        if let Ok(wallet_pubkey) = std::env::var("WALLET_PUBKEY") {
            config.wallet_pubkey = wallet_pubkey;
        }

        // Validate configuration using security utilities
        crate::security::validate_rpc_url(&config.rpc_url)?;
        crate::security::validate_solana_pubkey(&config.wallet_pubkey)?;

        Ok(config)
    }

    pub fn to_toml_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    pub fn init_logging(&self) {
        let level = match self.log_level.to_lowercase().as_str() {
            "trace" => Level::TRACE,
            "debug" => Level::DEBUG,
            "warn" => Level::WARN,
            "error" => Level::ERROR,
            _ => Level::INFO,
        };
        tracing_subscriber::fmt().with_max_level(level).init();
    }

    /// Get the metrics authentication token from environment variable
    pub fn get_metrics_auth_token() -> Option<String> {
        std::env::var("METRICS_AUTH_TOKEN").ok().and_then(|token| {
            if token.is_empty() {
                None
            } else {
                // Validate token format
                match crate::security::validate_auth_token(&token) {
                    Ok(_) => Some(token),
                    Err(e) => {
                        tracing::warn!("Invalid METRICS_AUTH_TOKEN format: {}", e);
                        None
                    }
                }
            }
        })
    }
}

/// Factory for creating all system communication channels
pub struct ChannelFactory {
    buffer_size: usize,
}

impl ChannelFactory {
    pub fn new(buffer_size: usize) -> Self {
        Self { buffer_size }
    }

    pub fn create_decision_ledger_channels(&self) -> DecisionLedgerChannels {
        let (decision_record_sender, decision_record_receiver) =
            mpsc::channel::<TransactionRecord>(self.buffer_size);
        let (outcome_update_sender, outcome_update_receiver) = mpsc::channel(self.buffer_size);
        let (monitor_tx_sender, monitor_tx_receiver) =
            mpsc::channel::<MonitoredTransaction>(self.buffer_size);

        DecisionLedgerChannels {
            decision_record_sender,
            decision_record_receiver,
            outcome_update_sender,
            outcome_update_receiver,
            monitor_tx_sender,
            monitor_tx_receiver,
        }
    }

    pub fn create_ooda_channels(&self) -> OodaChannels {
        let (perf_report_sender, perf_report_receiver) = mpsc::channel(16);
        let (opt_params_sender, opt_params_receiver) = mpsc::channel(16);

        OodaChannels {
            perf_report_sender,
            perf_report_receiver,
            opt_params_sender,
            opt_params_receiver,
        }
    }

    pub fn create_oracle_channels(&self) -> OracleChannels {
        let (candidate_sender, candidate_receiver) =
            mpsc::channel::<PremintCandidate>(self.buffer_size);
        let (oracle_scored_sender, oracle_scored_receiver) =
            mpsc::channel::<crate::oracle::quantum_oracle::ScoredCandidate>(self.buffer_size);

        OracleChannels {
            candidate_sender,
            candidate_receiver,
            oracle_scored_sender,
            oracle_scored_receiver,
        }
    }
}

pub struct DecisionLedgerChannels {
    pub decision_record_sender: DecisionRecordSender,
    pub decision_record_receiver: mpsc::Receiver<TransactionRecord>,
    pub outcome_update_sender: mpsc::Sender<(
        String,
        crate::oracle::Outcome,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<u64>,
        bool,
    )>,
    pub outcome_update_receiver: mpsc::Receiver<(
        String,
        crate::oracle::Outcome,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<u64>,
        bool,
    )>,
    pub monitor_tx_sender: mpsc::Sender<MonitoredTransaction>,
    pub monitor_tx_receiver: mpsc::Receiver<MonitoredTransaction>,
}

pub struct OodaChannels {
    pub perf_report_sender: mpsc::Sender<crate::oracle::PerformanceReport>,
    pub perf_report_receiver: mpsc::Receiver<crate::oracle::PerformanceReport>,
    pub opt_params_sender: mpsc::Sender<crate::oracle::OptimizedParameters>,
    pub opt_params_receiver: mpsc::Receiver<crate::oracle::OptimizedParameters>,
}

pub struct OracleChannels {
    pub candidate_sender: mpsc::Sender<PremintCandidate>,
    pub candidate_receiver: mpsc::Receiver<PremintCandidate>,
    pub oracle_scored_sender: mpsc::Sender<crate::oracle::quantum_oracle::ScoredCandidate>,
    pub oracle_scored_receiver: mpsc::Receiver<crate::oracle::quantum_oracle::ScoredCandidate>,
}

/// Coordinates graceful shutdown of all system components
pub struct ShutdownCoordinator {
    handles: Vec<JoinHandle<()>>,
}

impl ShutdownCoordinator {
    pub fn new() -> Self {
        Self {
            handles: Vec::new(),
        }
    }

    pub fn add_handle(&mut self, handle: JoinHandle<()>) {
        self.handles.push(handle);
    }

    pub async fn shutdown(self) {
        tracing::info!(
            "Initiating graceful shutdown of {} components",
            self.handles.len()
        );
        for handle in self.handles {
            handle.abort();
        }
        tracing::info!("All components shut down successfully");
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// System initializer helper
pub struct SystemInitializer;

impl SystemInitializer {
    pub async fn init_pillar_one(
        decision_rx: mpsc::Receiver<TransactionRecord>,
        outcome_rx: mpsc::Receiver<(
            String,
            crate::oracle::Outcome,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
            Option<u64>,
            bool,
        )>,
    ) -> Result<(
        DecisionLedger,
        sqlx::Pool<sqlx::Sqlite>,
        Arc<dyn LedgerStorage>,
    )> {
        let ledger = DecisionLedger::new(decision_rx, outcome_rx).await?;
        let pool = ledger
            .get_db_pool()
            .expect("Expected SQLite storage")
            .clone();
        let storage = ledger.get_storage();
        Ok((ledger, pool, storage))
    }

    pub fn init_pillar_two(
        db_pool: sqlx::Pool<sqlx::Sqlite>,
        perf_tx: mpsc::Sender<crate::oracle::PerformanceReport>,
        perf_rx: mpsc::Receiver<crate::oracle::PerformanceReport>,
        opt_tx: mpsc::Sender<crate::oracle::OptimizedParameters>,
        config: &OrchestratorConfig,
    ) -> (PerformanceMonitor, StrategyOptimizer) {
        let monitor = PerformanceMonitor::new(
            db_pool.clone(),
            perf_tx,
            config.perf_monitor.analysis_interval_minutes,
            config.perf_monitor.lookback_hours,
        );
        let optimizer = StrategyOptimizer::new(
            db_pool,
            perf_rx,
            opt_tx,
            FeatureWeights::default(),
            ScoreThresholds::default(),
        );
        (monitor, optimizer)
    }

    pub fn init_oracle(
        candidate_rx: mpsc::Receiver<PremintCandidate>,
        scored_tx: mpsc::Sender<crate::oracle::quantum_oracle::ScoredCandidate>,
        current_regime: Arc<RwLock<MarketRegime>>,
    ) -> Result<Arc<PredictiveOracle>> {
        use crate::features::store::FeatureStore;
        use std::time::Duration;
        
        let config = Arc::new(RwLock::new(
            crate::oracle::quantum_oracle::SimpleOracleConfig::default(),
        ));
        
        // Create FeatureStore with default configuration
        let feature_store = Arc::new(FeatureStore::new(1000, Duration::from_secs(300)));
        
        Ok(Arc::new(PredictiveOracle::new(
            candidate_rx,
            scored_tx,
            config,
            feature_store,
            current_regime,
        )?))
    }

    pub fn init_pillar_three(
        config: &OrchestratorConfig,
    ) -> (
        crate::oracle::MarketRegimeDetector,
        Arc<RwLock<MarketRegime>>,
    ) {
        let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
        let data_sources = Arc::new(OracleDataSources::new(
            vec![],
            reqwest::Client::new(),
            OracleConfig::default(),
        ));
        let detector = crate::oracle::MarketRegimeDetector::new(
            data_sources,
            regime.clone(),
            config.regime_detector.analysis_interval_secs,
        );
        (detector, regime)
    }

    /// Initialize the asynchronous feature worker
    ///
    /// This worker fetches TokenData via OracleDataSources, extracts features,
    /// and stores them in the FeatureStore with TTL caching.
    pub async fn init_feature_worker(
        data_sources: Arc<OracleDataSources>,
        feature_store: Arc<crate::features::FeatureStore>,
        worker_config: crate::features::FeatureWorkerConfig,
    ) -> Result<crate::features::FeatureWorkerHandle> {
        use crate::features::FeatureWorker;

        let worker = FeatureWorker::new(data_sources, feature_store, worker_config);
        worker.start().await
    }
}
