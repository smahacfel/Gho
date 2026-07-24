pub mod engine;
pub mod feature_builder;
pub mod traits;
pub mod types;

pub use crate::tx_intelligence::types::SybilResistanceFeatures;
pub use engine::{CheckpointConfig, CheckpointEngine, EventCheckpointTrigger};
pub use feature_builder::ObservationFeatureBuilder;
pub use traits::{CheckpointProducer, FeatureMaterializer};
pub use types::{
    organic_continuity_experimental_score_contract_hash, AlphaFingerprintFeatures,
    CheckpointDerivedFeatures, CheckpointTrigger, CpvEvidenceContext, CpvMetricSource,
    CurveReadinessFeatures, DecisionTimeSeriesFeatures, DecisionTimeSeriesPriceSource,
    DecisionTimeSeriesRetentionPolicy, DecisionTimeSeriesRetentionStatus,
    DecisionTimeSeriesSourceCounts, EvidenceDegradedReason, EvidenceStatus,
    EvidenceUnavailableReason, FeatureEvidenceStatus, ManipulationContradictionFeatures,
    MaterializedEvidenceStatus, MaterializedFeatureSet, MaterializedTrajectoryAssessment,
    MetricEvidenceQuality, OrganicBroadeningFeatures, OrganicContinuityAvailabilityV1,
    OrganicContinuityBucketReasonV1, OrganicContinuityClaimBoundariesV1,
    OrganicContinuityContextFieldsV1, OrganicContinuityEvidenceV1,
    OrganicContinuityExperimentalScoreStatusV1, OrganicContinuityExperimentalScoreV1,
    OrganicContinuityMissingReasonV1, OrganicContinuityRawOrganicFieldsV1,
    OrganicContinuitySourceV1, PreEntryPathSummaryV1, SessionCheckpoint, SessionRegimeSnapshotV1,
    TemporalAnchorReachedBy, TemporalAnchorSnapshot, TemporalDeltaFeatures,
    TemporalMetricEvidenceContext, TemporalMetricSource, TrajectorySegmentSnapshot, TrendDirection,
    TxSegmentSequence,
};
