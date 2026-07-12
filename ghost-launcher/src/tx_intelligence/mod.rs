pub mod analysis;
pub mod config;
pub mod cross_pool_velocity;
pub mod engine;
pub mod funding_source;
pub mod sybil_metrics;

pub use analysis::{
    compute_dev_behavior, compute_gini, compute_signer_diversity, compute_velocity_profile,
    compute_volume_sanity, DevBehaviorProfile, SignerDiversityProfile, SignerStats,
    VelocityProfile, VolumeSanityProfile,
};
pub use config::{TxIntelligenceConfig, DEFAULT_SESSION_TX_RING_CAPACITY};
pub use cross_pool_velocity::{CpvComputation, CrossPoolVelocityConfig, CrossPoolVelocityIndex};
pub(crate) use engine::{tx_has_stable_timing_order_identity, BUNDLE_CLUSTER_THRESHOLD_MS};
pub use engine::{
    DevBuyProducerSnapshotV1, Top3ProducerSnapshotV1, TxIntelligenceEngine,
    TxIntelligenceMetricContractSnapshotV1, TxTimingProducerSnapshotV1,
};
pub(crate) use funding_source::FSC_LEGACY_MIN_KNOWN_SOURCE_SAMPLES_V1;
pub use funding_source::{
    funding_lookup_wallets, FscComputation, FundingSourceConfig, FundingSourceIndex,
};
pub use sybil_metrics::{
    compute_dbia, compute_des, compute_ftdi, compute_sfd, compute_sybil_resistance,
    DbiaComputation, DesComputation, FtdiComputation, SfdComputation,
};
pub(crate) use sybil_metrics::{
    MIN_CLEAN_BUY_SAMPLE_COUNT, MIN_CLEAN_UNIQUE_BUYER_SAMPLE_COUNT_V2, MIN_DIAGNOSTIC_SAMPLE_COUNT,
};
