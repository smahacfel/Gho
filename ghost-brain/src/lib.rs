//! Ghost Brain Pipeline Library
//!
//! This library provides an end-to-end integration pipeline that connects
//! all Ghost components from detection to execution:
//!
//! ```text
//! Seer (InitializePool Detection)
//!   ↓
//! Oracle (Candidate Scoring)
//!   ↓
//! Features (Strategy Selection)
//!   ↓
//! SwapPlan Generation
//!   ↓
//! DirectBuyBuilder (Direct AMM Interaction)
//!   ↓
//! Trigger (Transaction Building & Sending)
//!   ↓
//! Inclusion on Solana
//! ```
//!
//! ## Metrics
//!
//! The pipeline tracks:
//! - **Land Rate**: (Seer) Percentage of detected pools successfully parsed (target: ≥95%)
//! - **Inclusion Rate**: (Trigger) Percentage of sent transactions confirmed (target: ≥92%)
//! - **End-to-end Latency**: Time from detection to confirmation
//!
//! ## Metrics Server
//!
//! The library includes a Prometheus metrics HTTP server:
//! - `GET /metrics` - Prometheus metrics in text format
//! - `GET /readyz` - Health check endpoint
//!
//! ## Usage
//!
//! ```rust,no_run
//! use ghost_brain::{E2EConfig, E2EPipeline};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Load configuration from environment
//!     let config = E2EConfig::from_env()?;
//!     config.validate()?;
//!
//!     // Create and run pipeline
//!     let pipeline = E2EPipeline::new(config)?;
//!     pipeline.run().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod aem;
pub mod analyzers;
pub mod calibration;
pub mod chaos;
pub mod config;
pub mod events;
pub mod execution;
pub mod fast_pipeline;
pub mod guardian;
pub mod jito_bundle;
pub mod leader_predictor;
pub mod mar;
pub mod mci;
pub mod metrics;
pub mod metrics_server;
pub mod models;
pub mod oracle;
pub mod pipeline;
pub mod pool_state_ssot;
pub mod pumpfun;
pub mod qedd;
pub mod quotes;
pub mod scenarios;
pub mod security;
pub mod signals;
pub mod strategy;
pub mod telemetry;
pub mod tuning;

// Re-export oracle_scoring for backward compatibility
pub use oracle::scoring as oracle_scoring;

// Re-export main types
pub use config::E2EConfig;
pub use metrics::E2EMetrics;
pub use oracle_scoring::SimpleOracle;
pub use pipeline::E2EPipeline;
pub use scenarios::{ScenarioResult, TestScenario};
pub use strategy::StrategySelector;

// Re-export execution backend types
pub use events::{
    ComparisonReport, EventEmitter, EventEnvelope, EventKind, EventWriter, EventWriterConfig,
    ExecutionEvent,
};
pub use execution::dual::{DualBackend, DualBackendConfig};
pub use execution::live::LiveBackend;
pub use execution::paper::{PaperBackend, PaperBroker, PaperBrokerConfig, StressTransition};
pub use execution::paper_lifecycle::{PaperLifecycleConfig, PaperPositionLifecycle};
pub use execution::{
    CandidateId, CandidateRef, CommandId, ExecutionBackend, ExecutionError, ExecutionMode,
    ExecutionStressSnapshot, FillEvent, FillStatus, Lane, OrderId, OrderSide, PositionId, QuoteId,
    StressBucket,
};
pub use quotes::{ExecutableQuote, ExecutableQuoteProvider, QuoteProviderConfig, QuoteSource};

// Re-export metrics server types
pub use mar::{MarConfig, MarMetricsSnapshot, MarPoolReserves, MarketExploitabilityState};
pub use metrics_server::{
    start_metrics_server, GhostMetrics, MetricsServer, MetricsServerConfig, DEFAULT_METRICS_PORT,
};

// Re-export specific scenarios
pub use scenarios::scenario_a::ScenarioA;
pub use scenarios::scenario_b::ScenarioB;
pub use scenarios::scenario_e2e_full::ScenarioE2EFull;

// Re-export jito_bundle types
pub use jito_bundle::{
    get_swap_intent_from_pool, BatchExecutionStats, BundleSubmissionResult, JitoBundleExecutor,
    SwapIntent, YellowstoneConfirmationTracker,
};

// Re-export leader_predictor types
pub use leader_predictor::{LeaderPredictor, LeaderStats};

// Re-export chaos types
pub use chaos::{
    action_to_amount_multiplier,
    is_buy,
    is_sell,
    // AMM Math
    AmmMathError,
    AmmPool,
    BatchSwapInput,
    BatchSwapOutput,
    BuyerProfile,
    // Engine
    ChaosEngine,
    ChaosResult,
    CompactSwapResult,
    // Distributions
    MarketAction,
    MarketScenario,
    SimulationConfig,
    SimulationRun,
    SwapResult,
};

// Re-export signals types
pub use signals::{
    analyze_resonance,
    // LIGMA types
    compute_ligma,
    extract_bands,
    ActivityClassification,
    BandConfig,
    BandExtractor,
    // FRB types
    BandProfile,
    BandRange,
    BandTransaction,
    CircularBuffer,
    FrbResult,
    FrbSignal,
    LigmaDiagnostics,
    LigmaResult,
    ResonanceAnalyzer,
    ResonanceConfig,
    ResonanceConfig as FrbResonanceConfig,
    ResonanceDetector,
    ResonanceResult,
};

// Re-export security types
pub use security::{DetectedPattern, GeneAnalysisResult, GeneMapper, GeneMapperConfig, RiskLevel};

// Re-export telemetry types
pub use telemetry::{TelemetryConfig, TelemetryEvent, TelemetryRecorder};

// Re-export QEDD and MCI types
pub use config::mci_config::MciInitialState;
pub use config::{MciConfig, QeddConfig};
pub use mci::MciEngine;
pub use models::{MciResult, QeddHorizonSurvival, QeddResult};
pub use qedd::QeddEngine;
pub use signals::MarketSignals;

// Re-export tuning types
pub use tuning::{
    BanditAlgorithm, BayesianOptimizer, DecisionOutcome, HysteresisConfig, HysteresisLoop,
    LinUCBBandit, LoopStats, OptimizationResult, RewardCalculator, RewardSignal,
    ThompsonSamplingBandit, TradeOutcome, TunableWeights, TuningConfig, TuningContext, TuningStats,
    WeightBandit, WeightTuner,
};

// Re-export HysteresisLoop integration helpers
pub use tuning::integration::{
    cleanup_expired, get_current_weights, get_loop_stats, is_enabled, register_decision,
    register_outcome, HYSTERESIS_LOOP,
};

// Re-export pumpfun types
pub use pumpfun::{
    CacheMetrics, CurveSnapshot, EarlySwapEvent, EarlySwapEvents, EarlySwapRingBuffer,
    PumpCurveStateCache, EARLY_SWAP_BUFFER_SIZE, SWAP_EVENT_TTL_MS,
};

// Re-export pool_state_ssot types
pub use pool_state_ssot::{
    PoolPhase, PoolSnapshot, Quote, QuoteEngine, QuoteSide, SnapshotSource, SnapshotStore,
    SsotConfig, SsotMetrics, SubscriberDiagnostics, TokenParseError, YellowstoneSubscriber,
};

// Test modules
#[cfg(test)]
mod ligma_tests;
