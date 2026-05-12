//! Oracle module for ghost-e2e
//!
//! This module contains oracle-related functionality including:
//! - Scoring system (SimpleOracle, ScoringWeights, RiskLevel)
//! - Anomaly detection (AnomalyDetector, RingBuffer)
//! - Ghost Intelligence modules (DevProfiler, ClusterHunter, VisionCritic)
//! - SCR 2.0 (Harmonic Detection & Spectral Pattern Matching)
//! - ULVF 2.0 (Momentum Classification & Multi-Snapshot Trend Analysis)
//! - Snapshot Engine (High-performance market snapshot system)
//! - WEST (Wallet Energy & State Tracker - QMAN Part 1)
//! - QMAN (Quantum Market Analysis Network - Parts 1 & 2)
//! - Cyclic Engine (S1-S13 Heartbeat Loop)

pub mod anomaly;
pub mod block_metrics;
pub mod bva;
pub mod cluster_hunter;
pub mod confidence_model;
pub mod decision_logger;
pub mod engine;
pub mod followup_scoring;
pub mod hyper_oracle;
pub mod hyper_prediction;
pub mod outcome_tracker;
pub mod predator_strategy;
pub mod profiler;
pub mod qman;
pub mod reason_code;
pub mod score_history;
pub mod scoring;
pub mod scoring_phase;
pub mod scr_extended;
pub mod second_wave_detector;
pub mod snapshot_engine;
pub mod snapshot_metrics;
pub mod survivor_score;
#[cfg(test)]
mod survivor_score_test_issue49;
pub mod tcf;
pub mod tx_metrics;
pub mod ultrafast;
pub mod ulvf_extended;
pub mod vision_critic;
pub mod wallet_energy_tracker;
pub mod window_spec;

// Re-export scoring types
// POPRAWKA: Importujemy z lokalnego modułu `scoring`, a nie z nieistniejącego `crate::oracle_scoring`
pub use scoring::{
    AggregatedRiskScore, RiskAggregator, ScoredCandidate, ScoringWeights, SimpleOracle,
};

// Re-export anomaly detection types
pub use anomaly::{AnomalyConfig, AnomalyDetector, PremintCandidateWithAnomaly, RingBuffer};

// Re-export Ghost Intelligence types
pub use cluster_hunter::{
    ClusterAnalysis, ClusterHunter, ClusterHunterConfig, ClusterMetric, HolderFunding,
};
pub use profiler::{DevProfile, DevProfiler, DevProfilerConfig, FundingSource};
pub use vision_critic::{
    LlmProvider, SignalStrength, VisionCritic, VisionCriticConfig, VisionCriticResult,
};

// Re-export HyperOracle types
pub use hyper_oracle::{HyperOracle, MarketSnapshot};

// Re-export HyperPrediction Oracle types
pub use hyper_prediction::{HyperPredictionOracle, HyperPredictionResult, TcfResult};

// Re-export HyperPrediction TCF integration helpers
pub use hyper_prediction::{
    apply_tcf_modulation, build_tcf_observation, compute_tcf_result, interpret_tcf_result,
};

// Re-export HyperPrediction Verdict types (decision-related types)
pub use hyper_prediction::verdict::{FinalVerdict, OracleDecision, RiskLevel, RiskThresholds};

// Re-export SCR 2.0 types (Harmonic Detection & Spectral Pattern Matching)
pub use scr_extended::{
    ActivityType, HarmonicPeak, PatternMatch, SCRAnalysis, SCRExtended, SpectralSignature,
};

// Re-export ULVF 2.0 types (Momentum Classification & Multi-Snapshot Trend Analysis)
pub use ulvf_extended::{MomentumType, TrendAnalysis, ULVFConfig, ULVFExtended};

// Re-export Ultrafast SSMI types
pub use ultrafast::{SourceType, SsmiResult, SubSlotMicroentropy};

// Re-export deprecated QASS types (for backward compatibility)
pub use ultrafast::{
    build_cluster_wave, build_povc_wave, build_profiler_wave, build_scr_wave, build_shadow_wave,
    build_ssmi_wave, build_ulvf_wave, build_vision_wave, HeuristicWave, QASSResult,
    QuantumAmplitudeScorer, WaveContribution, MAX_WAVES,
};

// Re-export Snapshot Engine types
pub use snapshot_engine::{
    ApprovedPools, DataSource, InitPoolEvent, IntegritySeverity, IntegrityViolation, LiveCounters,
    MarketSnapshot as ExtendedMarketSnapshot, PoolAccumulators, PoolState, ResyncConfig,
    RingSnapshots, SnapshotEngine, TxEvent,
};

// Re-export Snapshot Metrics types
pub use snapshot_metrics::SnapshotMetrics;

// Re-export WEST (Wallet Energy & State Tracker) types
pub use wallet_energy_tracker::{
    Action, ObservedToken, StateVector, WalletEnergyTracker, WalletParticle, WestStats,
};

// Re-export QMAN (Quantum Market Analysis Network) types
pub use qman::{
    MigrationForecast, PredictionResult, SignalDetector, SignalDetectorConfig, SignalResult,
    SparseTransitionMatrix, TradingSignal, TransitionMatrix, TransitionMatrixBuilder,
    UnitaryEvolution,
};

// Re-export Transaction Metrics types
pub use tx_metrics::TransactionMetrics;

// Re-export Decision Logger types
pub use decision_logger::{
    CorrectionReason, DecisionLogger, DecisionLoggerConfig, DecisionType, FollowupScore,
    GatekeeperBuyLog, InitialComponents, OracleDecisionLog, VetoType, DEFAULT_DECISION_LOG_DIR,
    GATEKEEPER_BUY_LOG_SCHEMA_VERSION, GATEKEEPER_VERSION,
};

// Re-export WindowSpec types (A/B Boundary Equalization)
pub use window_spec::{
    ensure_epoch_ms, EndKind, StartKind, WindowCloseReason, WindowSpec, WindowState,
};

// Re-export OutcomeTracker types
pub use outcome_tracker::{
    build_join_key, OutcomeRecord, OutcomeStatus, OutcomeThresholds, OutcomeTracker, TrackedPool,
};

// Re-export Follow-up Scoring types
pub use followup_scoring::{FollowupConfig, FollowupContext, FollowupScoringManager};

// Re-export Confidence Model types
pub use confidence_model::{
    ConfidenceInputs, ConfidenceMetadata, ConfidenceModel, ConfidenceScore, ConfidenceWeights,
    ModuleContributions,
};

// Re-export Second Wave Detector types
pub use second_wave_detector::{
    SecondWaveAction, SecondWaveComponents, SecondWaveConfig, SecondWaveDetector, SecondWaveResult,
};

// Re-export Block Metrics types
pub use block_metrics::{BlockMetricsBuffer, BlockSnapshot};

// Re-export SurvivorScore types
pub use survivor_score::{
    DeferReason, SurvivorScoreBreakdown, SurvivorScoreCalculator, SurvivorScoreConfig,
    SurvivorScoreInput, SurvivorScoreResult, VetoReason,
};

// Re-export BVA types
pub use bva::{BvaAnalyzer, BvaClassification, BvaMetrics, BvaOutput, BvaState};

// Re-export ScoreHistory types (Patient Observer)
pub use score_history::{
    CycleScore, ObservationAction, ObservationDecision, ScoreHistory, ScoreTrend,
    TcfObservationData,
};

// Re-export Ghost Predator Strategy types
pub use predator_strategy::{
    calculate_quality_early_stage_with_config,
    calculate_quality_for_cycle_with_config,
    calculate_quality_full_analysis_with_config,
    calculate_weighted_average,
    calculate_weighted_average_with_config,
    calculate_weighted_geometric_mean_with_config,
    get_cycle_weight,
    // Config-aware functions
    get_cycle_weight_from_config,
    get_gunshot_threshold,
    get_gunshot_threshold_from_config,
    CYCLE_WEIGHTS,
    GUNSHOT_THRESHOLDS,
};

// Re-export TCF (Trend Cohesion Field) types
pub use tcf::{
    cohesion, cohesion_simple, observation_from_ghost_signals, CohesionBreakdown, CohesionConfig,
    CohesionResult, ExpectedTransition, ExpectedTransitionModel, MarketObservation, TcfDiagnostics,
    TcfPhase, TcfUpdateResult, Transition, TrendCohesionField, OBSERVATION_DIM,
};

// Re-export Cyclic Engine / HyperPrediction types
pub use engine::{EngineAction, PredictionSession, PredictionSessionConfig};
pub use hyper_prediction::CycleResult;

// Re-export ScoringPhase types
pub use scoring_phase::ScoringPhase;
