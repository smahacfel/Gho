pub mod engine;
pub mod feature_builder;
pub mod traits;
pub mod types;

pub use crate::tx_intelligence::types::SybilResistanceFeatures;
pub use engine::{CheckpointConfig, CheckpointEngine, EventCheckpointTrigger};
pub use feature_builder::ObservationFeatureBuilder;
pub use traits::{CheckpointProducer, FeatureMaterializer};
pub use types::{
    AlphaFingerprintFeatures, CheckpointDerivedFeatures, CheckpointTrigger, CurveReadinessFeatures,
    EvidenceDegradedReason, EvidenceStatus, EvidenceUnavailableReason, FeatureEvidenceStatus,
    ManipulationContradictionFeatures, MaterializedEvidenceStatus, MaterializedFeatureSet,
    MaterializedTrajectoryAssessment, OrganicBroadeningFeatures, SessionCheckpoint,
    TrajectorySegmentSnapshot, TrendDirection, TxSegmentSequence,
};
