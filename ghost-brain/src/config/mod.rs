//! Configuration modules for all Ghost Brain components

pub mod e2e_config;
pub mod fallback_config;
pub mod gatekeeper_v25_config;
pub mod ghost_brain_config;
pub mod mci_config;
pub mod qedd_config;

// Re-export QEDD and MCI configs
pub use mci_config::{MciConfig, MciInitialState};
pub use qedd_config::QeddConfig;

// Re-export E2E pipeline configs
pub use e2e_config::*;

// Re-export fallback configuration
pub use fallback_config::{FallbackConfig, FallbackTracker, FallbackType};

// Re-export unified Ghost Brain config
pub use ghost_brain_config::{
    BvaConfig,
    ConfidenceConfig,
    // Cycle Weights & Gunshot Thresholds (Section 10)
    CycleWeightsConfig,
    FrbConfig,
    GatekeeperConfig,
    GatekeeperMode,
    GatekeeperV2Config,
    GhostBrainConfig,
    GunshotThresholdsConfig,
    IwimConfig,
    IwimFeedMode,
    // IWIM Veto Gate
    IwimVetoGateConfig,
    LigmaConfig,
    MomentumConfig,
    MpcfConfig,
    PanicConfig,
    ProfileWeights,
    QassConfig,
    QofsvConfig,
    QualityEarlyStageWeights,
    QualityFullAnalysisWeights,
    ResonanceConfig,
    ScoringWeightsConfig,
    SobpConfig,
    SsmiConfig,
    SurvivalCycleWeights,
    SurvivalFinalVerdictWeights,
    // Survivor Score configuration (Section 9)
    SurvivorScoreComponentConfig,
    TcfConfig,
    TcrPhiConfig,
    WeightProfile,
    WeightProfiles,
};

// Re-export Gatekeeper V2.5 config types
pub use gatekeeper_v25_config::{
    AdaptiveProsperityConfig, DynamicObservationWindowConfig, EntryDriftAnchorQuality,
    GatekeeperV25RolloutConfig, PumpAndDumpDetectorConfig, TrajectoryAwareScoringConfig,
};

// Re-export PostBuy Guardian config
pub use crate::aem::config::AemConfig;
pub use crate::guardian::post_buy::PostBuyGuardianConfig;
