use crate::account_state_core::types::AccountStateFeatures;
use crate::session::types::SessionMetadata;
use crate::tx_intelligence::types::{RiskFlag, SybilResistanceFeatures, TxIntelFeatures};
use crate::{CurveFinality, CurveFreshnessState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    Rising,
    Falling,
    Stable,
    #[default]
    Insufficient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointTrigger {
    TimeBased(u64),
    EventBased(String),
}

impl Default for CheckpointTrigger {
    fn default() -> Self {
        Self::TimeBased(0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionCheckpoint {
    pub checkpoint_id: u32,
    pub timestamp_ms: u64,
    pub trigger: CheckpointTrigger,
    pub account_state_snapshot: AccountStateFeatures,
    pub tx_intel_snapshot: TxIntelFeatures,
    pub risk_flags: Vec<RiskFlag>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckpointDerivedFeatures {
    pub price_trajectory: Vec<f64>,
    pub reserve_trajectory: Vec<(u64, u64)>,
    pub buy_pressure_trend: TrendDirection,
    pub signer_diversity_trend: TrendDirection,
    pub risk_flag_count_trend: TrendDirection,
    pub trajectory_checkpoint_count: u32,
    #[serde(default)]
    pub price_change_from_first_checkpoint_pct: f64,
    #[serde(default)]
    pub single_tx_max_price_impact_pct: f64,
    #[serde(default)]
    pub max_single_sell_impact_pct: f64,
    #[serde(default)]
    pub bonding_progress: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trajectory_assessment: Option<MaterializedTrajectoryAssessment>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MaterializedTrajectoryAssessment {
    pub overall_tas_score: f64,
    pub momentum_score: f64,
    pub hhi_score: f64,
    pub volume_score: f64,
    pub interval_score: f64,
    pub buy_ratio_score: f64,
    pub segment_count: usize,
    pub t0_tx_count: usize,
    pub t1_tx_count: usize,
    pub t2_tx_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveReadinessFeatures {
    pub is_ready: bool,
    pub freshness: CurveFreshnessState,
    pub finality: CurveFinality,
    #[serde(default)]
    pub curve_data_known: bool,
    #[serde(default)]
    pub price_sample_count: u32,
    #[serde(default)]
    pub t0_event_ts_ms: Option<u64>,
    #[serde(default)]
    pub wait_elapsed_ms: Option<u64>,
}

impl Default for CurveReadinessFeatures {
    fn default() -> Self {
        Self {
            is_ready: false,
            freshness: CurveFreshnessState::Unknown,
            finality: CurveFinality::Speculative,
            curve_data_known: false,
            price_sample_count: 0,
            t0_event_ts_ms: None,
            wait_elapsed_ms: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlphaFingerprintFeatures {
    pub avg_inner_ix_count_50tx: Option<f64>,
    pub sell_buy_ratio: Option<f64>,
    pub compute_unit_cluster_dominance: Option<f64>,
    pub static_fee_profile_ratio: Option<f64>,
    pub jito_tip_intensity: Option<f64>,
    pub early_slot_volume_dominance_buy: Option<f64>,
    pub early_top3_buy_volume_pct_3s: Option<f64>,
    pub fixed_size_buy_ratio: Option<f64>,
    pub flipper_presence_ratio: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Clean,
    Degraded,
    Unavailable,
    InsufficientSample,
    Stale,
    Fallback,
    ShadowOnly,
    NotConfigured,
}

impl Default for EvidenceStatus {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceDegradedReason {
    SegmentSequencePartial,
    SegmentSignerCoveragePartial,
    TxIntelLowSample,
    AccountStateFallback,
    CheckpointHistorySparse,
    CurveEvidencePartial,
    SybilEvidencePartial,
    AlphaEvidencePartial,
    ManipulationEvidencePartial,
    IdentityEvidenceFallback,
    TrajectoryEvidenceSparse,
    PddSequencePartial,
    CpvEvidencePartial,
    FscEvidencePartial,
    OrganicBroadeningInsufficient,
    ManipulationContradictionPartial,
    EvidenceStale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceUnavailableReason {
    NotMaterialized,
    IdentityMissing,
    SegmentSequenceMissing,
    SegmentSignerDataMissing,
    TxIntelMissing,
    AccountStateMissing,
    CheckpointHistoryMissing,
    CurveDataMissing,
    TrajectoryMissing,
    PddSequenceMissing,
    SybilMetricsMissing,
    AlphaFingerprintMissing,
    CpvMetricsMissing,
    FscMetricsMissing,
    OrganicBroadeningMissing,
    ManipulationContradictionMissing,
    ExecutionNotRun,
    NotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureEvidenceStatus {
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<EvidenceDegradedReason>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_reasons: Vec<EvidenceUnavailableReason>,
}

impl Default for FeatureEvidenceStatus {
    fn default() -> Self {
        Self {
            status: EvidenceStatus::Unavailable,
            degraded_reasons: Vec::new(),
            unavailable_reasons: vec![EvidenceUnavailableReason::NotMaterialized],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializedEvidenceStatus {
    #[serde(default)]
    pub identity: FeatureEvidenceStatus,
    #[serde(default)]
    pub account_state: FeatureEvidenceStatus,
    #[serde(default)]
    pub tx_intel: FeatureEvidenceStatus,
    #[serde(default)]
    pub tx_segments: FeatureEvidenceStatus,
    #[serde(default)]
    pub checkpoints: FeatureEvidenceStatus,
    #[serde(default)]
    pub trajectory: FeatureEvidenceStatus,
    #[serde(default)]
    pub pdd_sequence: FeatureEvidenceStatus,
    #[serde(default)]
    pub curve: FeatureEvidenceStatus,
    #[serde(default)]
    pub sybil: FeatureEvidenceStatus,
    #[serde(default)]
    pub cpv: FeatureEvidenceStatus,
    #[serde(default)]
    pub fsc: FeatureEvidenceStatus,
    #[serde(default)]
    pub alpha: FeatureEvidenceStatus,
    #[serde(default)]
    pub manipulation: FeatureEvidenceStatus,
    #[serde(default)]
    pub organic_broadening: FeatureEvidenceStatus,
    #[serde(default)]
    pub manipulation_contradiction: FeatureEvidenceStatus,
    #[serde(default)]
    pub execution: FeatureEvidenceStatus,
}

impl Default for MaterializedEvidenceStatus {
    fn default() -> Self {
        Self {
            identity: FeatureEvidenceStatus::default(),
            account_state: FeatureEvidenceStatus::default(),
            tx_intel: FeatureEvidenceStatus::default(),
            tx_segments: FeatureEvidenceStatus::default(),
            checkpoints: FeatureEvidenceStatus::default(),
            trajectory: FeatureEvidenceStatus::default(),
            pdd_sequence: FeatureEvidenceStatus::default(),
            curve: FeatureEvidenceStatus::default(),
            sybil: FeatureEvidenceStatus::default(),
            cpv: FeatureEvidenceStatus::default(),
            fsc: FeatureEvidenceStatus::default(),
            alpha: FeatureEvidenceStatus::default(),
            manipulation: FeatureEvidenceStatus::default(),
            organic_broadening: FeatureEvidenceStatus::default(),
            manipulation_contradiction: FeatureEvidenceStatus::default(),
            execution: FeatureEvidenceStatus {
                status: EvidenceStatus::Unavailable,
                degraded_reasons: Vec::new(),
                unavailable_reasons: vec![EvidenceUnavailableReason::ExecutionNotRun],
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OrganicBroadeningFeatures {
    #[serde(default)]
    pub sequence_available: bool,
    #[serde(default)]
    pub total_tx_count: u64,
    #[serde(default)]
    pub total_unique_signers: u64,
    #[serde(default)]
    pub t0_tx_count: u64,
    #[serde(default)]
    pub t1_tx_count: u64,
    #[serde(default)]
    pub t2_tx_count: u64,
    #[serde(default)]
    pub t0_unique_signers: u64,
    #[serde(default)]
    pub t1_unique_signers: u64,
    #[serde(default)]
    pub t2_unique_signers: u64,
    #[serde(default)]
    pub t1_vs_t0_unique_signer_delta: i64,
    #[serde(default)]
    pub t2_vs_t1_unique_signer_delta: i64,
    #[serde(default)]
    pub tx_count_growth_ratio: f64,
    #[serde(default)]
    pub unique_signer_growth_ratio: f64,
    #[serde(default)]
    pub buy_ratio_mean: f64,
    #[serde(default)]
    pub buy_ratio_min: f64,
    #[serde(default)]
    pub buy_ratio_max: f64,
    #[serde(default)]
    pub max_segment_hhi: f64,
    #[serde(default)]
    pub min_segment_hhi: f64,
    #[serde(default)]
    pub signer_growth_t2_t0: i64,
    #[serde(default)]
    pub hhi_delta_t2_t0: f64,
    #[serde(default)]
    pub tx_count_growth_vs_signer_growth: f64,
    #[serde(default)]
    pub new_signer_ratio_t2: f64,
    #[serde(default)]
    pub broadening_score: f64,
    #[serde(default)]
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<EvidenceDegradedReason>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ManipulationContradictionFeatures {
    #[serde(default)]
    pub same_ms_tx_ratio: f64,
    #[serde(default)]
    pub bundle_suspicion_ratio: f64,
    #[serde(default)]
    pub top3_volume_pct: f64,
    #[serde(default)]
    pub hhi: f64,
    #[serde(default)]
    pub max_tx_per_signer: u64,
    #[serde(default)]
    pub dev_volume_ratio: f64,
    #[serde(default)]
    pub dev_has_sold: bool,
    #[serde(default)]
    pub fee_topology_diversity_index: Option<f64>,
    #[serde(default)]
    pub spend_fraction_divergence: Option<f64>,
    #[serde(default)]
    pub signer_cross_pool_velocity: Option<f64>,
    #[serde(default)]
    pub funding_source_concentration: Option<f64>,
    #[serde(default)]
    pub high_same_ms_tx_ratio: bool,
    #[serde(default)]
    pub high_bundle_suspicion_ratio: bool,
    #[serde(default)]
    pub high_top3_volume_pct: bool,
    #[serde(default)]
    pub high_hhi: bool,
    #[serde(default)]
    pub high_signer_concentration: bool,
    #[serde(default)]
    pub high_dev_concentration: bool,
    #[serde(default)]
    pub sybil_evidence_degraded: bool,
    #[serde(default)]
    pub momentum_without_broadening: bool,
    #[serde(default)]
    pub volume_spike_without_new_signers: bool,
    #[serde(default)]
    pub high_buy_pressure_with_high_top3: bool,
    #[serde(default)]
    pub fixed_size_or_ramping_pattern: bool,
    #[serde(default)]
    pub timing_bundle_concentration: bool,
    #[serde(default)]
    pub early_top3_concentration: bool,
    #[serde(default)]
    pub contradiction_score: f64,
    #[serde(default)]
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MaterializedFeatureSet {
    pub account_features: AccountStateFeatures,
    pub tx_intel_features: TxIntelFeatures,
    pub checkpoint_features: CheckpointDerivedFeatures,
    pub risk_flags: Vec<RiskFlag>,
    pub session_metadata: SessionMetadata,
    #[serde(default)]
    pub curve_readiness: CurveReadinessFeatures,
    #[serde(default)]
    pub sybil_resistance: SybilResistanceFeatures,
    #[serde(default)]
    pub alpha_fingerprint: AlphaFingerprintFeatures,
    /// V2.5: Per-segment trajectory snapshots (T0/T1/T2) for Path B TAS and
    /// PDD sequence signal computation. `None` when the buffer hasn't
    /// accumulated enough data for segment division (min TX per segment,
    /// min total duration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_segment_sequence: Option<TxSegmentSequence>,
    /// V3 P0: conservative evidence-plane status. Missing evidence defaults to
    /// unavailable and is never interpreted as clean.
    #[serde(default)]
    pub evidence_status: MaterializedEvidenceStatus,
    /// V3 P0: materialized organic broadening signals for shadow evaluation.
    #[serde(default)]
    pub organic_broadening: OrganicBroadeningFeatures,
    /// V3 P0: materialized manipulation/risk contradictions for shadow
    /// evaluation.
    #[serde(default)]
    pub manipulation_contradictions: ManipulationContradictionFeatures,
}

/// Per-segment trajectory snapshot used by Path B to compute TAS and PDD
/// sequence signals (spike, ramping, flash crash) without access to the
/// raw buffered transaction stream.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TxSegmentSequence {
    pub t0_segment: TrajectorySegmentSnapshot,
    pub t1_segment: TrajectorySegmentSnapshot,
    pub t2_segment: TrajectorySegmentSnapshot,
    /// Total observation duration across all segments.
    pub total_duration_ms: u64,
    /// Whether every segment met `tas_min_tx_per_segment`.
    pub min_tx_per_segment_satisfied: bool,
}

/// Metrics for a single time segment within the trajectory window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrajectorySegmentSnapshot {
    pub tx_count: u64,
    pub buy_ratio: f64,
    pub avg_interval_ms: f64,
    pub total_volume_sol: f64,
    pub hhi: f64,
    /// Largest single-TX SOL amount in this segment (NOT a price impact %).
    /// For actual price impact, use `CheckpointDerivedFeatures::single_tx_max_price_impact_pct`.
    pub max_single_tx_sol: f64,
    pub same_size_streak: u32,
}
