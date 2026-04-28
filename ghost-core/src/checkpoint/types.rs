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
}
