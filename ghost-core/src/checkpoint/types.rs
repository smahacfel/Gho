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
