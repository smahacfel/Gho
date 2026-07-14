use crate::account_state_core::types::AccountStateFeatures;
use crate::metric_contracts::MetricContractDecisionEvidenceProjectionV1;
use crate::session::types::SessionMetadata;
use crate::tx_intelligence::types::{RiskFlag, SybilResistanceFeatures, TxIntelFeatures};
use crate::{CurveFinality, CurveFreshnessState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_cpi_depth_50tx: Option<f64>,
    pub sell_buy_ratio: Option<f64>,
    pub compute_unit_cluster_dominance: Option<f64>,
    pub static_fee_profile_ratio: Option<f64>,
    pub jito_tip_intensity: Option<f64>,
    pub early_slot_volume_dominance_buy: Option<f64>,
    pub early_top3_buy_volume_pct_3s: Option<f64>,
    pub fixed_size_buy_ratio: Option<f64>,
    pub flipper_presence_ratio: Option<f64>,
    /// Source-level quality emitted by the canonical fingerprint owner. Numeric field
    /// completeness must never upgrade a degraded source to clean evidence.
    #[serde(default)]
    pub fingerprint_degraded: bool,
    /// Exact source diagnostic retained for replay/audit across the MFS boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint_reason: Option<String>,
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
    DecisionTimeSeriesPricePartial,
    DecisionTimeSeriesTruncated,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricEvidenceQuality {
    Clean,
    DegradedLowSample,
    CarriedForward,
    InsufficientSample,
    Stale,
    NotAllowed,
    UnavailableSource,
    Unavailable,
    NotConfigured,
}

impl Default for MetricEvidenceQuality {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpvMetricSource {
    SuccessfulBuyRollingIndex,
    Unavailable,
    NotConfigured,
}

impl Default for CpvMetricSource {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalMetricSource {
    Observed,
    CarriedForwardNoEvent,
    PartialCarriedForward,
    Stale,
    NotAllowed,
    Unavailable,
    NotConfigured,
}

impl Default for TemporalMetricSource {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalAnchorReachedBy {
    Event,
    ObservationElapsed,
    Deadline,
    NotReached,
}

impl Default for TemporalAnchorReachedBy {
    fn default() -> Self {
        Self::NotReached
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTimeSeriesPriceSource {
    Reserve,
    Quote,
    MarketCap,
    AccountState,
    History,
    CarryForward,
    Missing,
}

impl Default for DecisionTimeSeriesPriceSource {
    fn default() -> Self {
        Self::Missing
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTimeSeriesRetentionStatus {
    Clean,
    Truncated,
    Unavailable,
}

impl Default for DecisionTimeSeriesRetentionStatus {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTimeSeriesRetentionPolicy {
    TruncateWithStatus,
}

impl Default for DecisionTimeSeriesRetentionPolicy {
    fn default() -> Self {
        Self::TruncateWithStatus
    }
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionTimeSeriesSourceCounts {
    #[serde(default)]
    pub reserve: u64,
    #[serde(default)]
    pub quote: u64,
    #[serde(default)]
    pub market_cap: u64,
    #[serde(default)]
    pub account_state: u64,
    #[serde(default)]
    pub history: u64,
    #[serde(default)]
    pub carry_forward: u64,
    #[serde(default)]
    pub missing: u64,
}

impl DecisionTimeSeriesSourceCounts {
    pub fn increment(&mut self, source: DecisionTimeSeriesPriceSource) {
        match source {
            DecisionTimeSeriesPriceSource::Reserve => self.reserve = self.reserve.saturating_add(1),
            DecisionTimeSeriesPriceSource::Quote => self.quote = self.quote.saturating_add(1),
            DecisionTimeSeriesPriceSource::MarketCap => {
                self.market_cap = self.market_cap.saturating_add(1)
            }
            DecisionTimeSeriesPriceSource::AccountState => {
                self.account_state = self.account_state.saturating_add(1)
            }
            DecisionTimeSeriesPriceSource::History => self.history = self.history.saturating_add(1),
            DecisionTimeSeriesPriceSource::CarryForward => {
                self.carry_forward = self.carry_forward.saturating_add(1)
            }
            DecisionTimeSeriesPriceSource::Missing => self.missing = self.missing.saturating_add(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionTimeSeriesFeatures {
    #[serde(default)]
    pub status: EvidenceStatus,
    #[serde(default)]
    pub retention_status: DecisionTimeSeriesRetentionStatus,
    #[serde(default)]
    pub retention_policy: DecisionTimeSeriesRetentionPolicy,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub retention_capacity: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub retained_sample_count: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub total_tx_count: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub dropped_oldest_count: u64,
    #[serde(default)]
    pub sample_count: u64,
    #[serde(default)]
    pub finite_price_count: u64,
    #[serde(default)]
    pub missing_price_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_coverage_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ts_offsets_ms: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sol_amounts: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prices: Vec<Option<f64>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub price_sources: Vec<DecisionTimeSeriesPriceSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interval_ms: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub d_price: Vec<Option<f64>>,
    #[serde(default)]
    pub source_counts: DecisionTimeSeriesSourceCounts,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<EvidenceDegradedReason>,
}

impl Default for DecisionTimeSeriesFeatures {
    fn default() -> Self {
        Self {
            status: EvidenceStatus::Unavailable,
            retention_status: DecisionTimeSeriesRetentionStatus::Unavailable,
            retention_policy: DecisionTimeSeriesRetentionPolicy::TruncateWithStatus,
            retention_capacity: 0,
            retained_sample_count: 0,
            total_tx_count: 0,
            dropped_oldest_count: 0,
            sample_count: 0,
            finite_price_count: 0,
            missing_price_count: 0,
            price_coverage_ratio: None,
            ts_offsets_ms: Vec::new(),
            sol_amounts: Vec::new(),
            prices: Vec::new(),
            price_sources: Vec::new(),
            interval_ms: Vec::new(),
            d_price: Vec::new(),
            source_counts: DecisionTimeSeriesSourceCounts::default(),
            degraded_reasons: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CpvEvidenceContext {
    #[serde(default)]
    pub quality: MetricEvidenceQuality,
    #[serde(default)]
    pub source: CpvMetricSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_cross_pool_velocity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpv_other_pool_activity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_clean_sample_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_degraded_sample_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling_state_available: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalMetricEvidenceContext {
    #[serde(default)]
    pub quality: MetricEvidenceQuality,
    #[serde(default)]
    pub source: TemporalMetricSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carried_from_anchor_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staleness_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TemporalAnchorSnapshot {
    #[serde(default)]
    pub anchor_ms: u64,
    #[serde(default)]
    pub reached: bool,
    #[serde(default)]
    pub reached_by: TemporalAnchorReachedBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_observation_elapsed_ms: Option<u64>,
    #[serde(default)]
    pub status: MetricEvidenceQuality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buy_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_signers: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_quote_sol: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_volume_sol: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_cap_sol: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jito_tip_intensity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_cross_pool_velocity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flipper_presence_ratio: Option<f64>,
    #[serde(default)]
    pub event_counters_evidence: TemporalMetricEvidenceContext,
    #[serde(default)]
    pub state_metrics_evidence: TemporalMetricEvidenceContext,
    #[serde(default)]
    pub ratio_metrics_evidence: TemporalMetricEvidenceContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemporalDeltaFeatures {
    #[serde(default)]
    pub status: EvidenceStatus,
    #[serde(default)]
    pub anchor_1s: TemporalAnchorSnapshot,
    #[serde(default)]
    pub anchor_2s: TemporalAnchorSnapshot,
    #[serde(default)]
    pub anchor_3s: TemporalAnchorSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_mcap_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_mcap_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_mcap_2s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_price_pct_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_price_pct_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_price_pct_2s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_burstratio_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_burstratio_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_burstratio_2s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_buy_count_1s_to_2s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_buy_count_1s_to_3s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_buy_count_2s_to_3s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_unique_signers_1s_to_2s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_unique_signers_1s_to_3s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_unique_signers_2s_to_3s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_tx_count_1s_to_2s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_tx_count_1s_to_3s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_tx_count_2s_to_3s: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_net_quote_sol_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_net_quote_sol_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_net_quote_sol_2s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_jito_tip_intensity_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_jito_tip_intensity_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_signer_cross_pool_velocity_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_signer_cross_pool_velocity_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_flipper_presence_ratio_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_flipper_presence_ratio_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_mcap_sol_per_s_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_mcap_sol_per_s_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_mcap_sol_per_s_2s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_buy_count_per_s_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_buy_count_per_s_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_unique_signers_per_s_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_unique_signers_per_s_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_net_quote_sol_per_s_1s_to_2s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_net_quote_sol_per_s_1s_to_3s: Option<f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub delta_evidence: BTreeMap<String, TemporalMetricEvidenceContext>,
}

impl Default for TemporalDeltaFeatures {
    fn default() -> Self {
        Self {
            status: EvidenceStatus::Unavailable,
            anchor_1s: TemporalAnchorSnapshot::default(),
            anchor_2s: TemporalAnchorSnapshot::default(),
            anchor_3s: TemporalAnchorSnapshot::default(),
            delta_mcap_1s_to_2s: None,
            delta_mcap_1s_to_3s: None,
            delta_mcap_2s_to_3s: None,
            delta_price_pct_1s_to_2s: None,
            delta_price_pct_1s_to_3s: None,
            delta_price_pct_2s_to_3s: None,
            delta_burstratio_1s_to_2s: None,
            delta_burstratio_1s_to_3s: None,
            delta_burstratio_2s_to_3s: None,
            delta_buy_count_1s_to_2s: None,
            delta_buy_count_1s_to_3s: None,
            delta_buy_count_2s_to_3s: None,
            delta_unique_signers_1s_to_2s: None,
            delta_unique_signers_1s_to_3s: None,
            delta_unique_signers_2s_to_3s: None,
            delta_tx_count_1s_to_2s: None,
            delta_tx_count_1s_to_3s: None,
            delta_tx_count_2s_to_3s: None,
            delta_net_quote_sol_1s_to_2s: None,
            delta_net_quote_sol_1s_to_3s: None,
            delta_net_quote_sol_2s_to_3s: None,
            delta_jito_tip_intensity_1s_to_2s: None,
            delta_jito_tip_intensity_1s_to_3s: None,
            delta_signer_cross_pool_velocity_1s_to_2s: None,
            delta_signer_cross_pool_velocity_1s_to_3s: None,
            delta_flipper_presence_ratio_1s_to_2s: None,
            delta_flipper_presence_ratio_1s_to_3s: None,
            rate_mcap_sol_per_s_1s_to_2s: None,
            rate_mcap_sol_per_s_1s_to_3s: None,
            rate_mcap_sol_per_s_2s_to_3s: None,
            rate_buy_count_per_s_1s_to_2s: None,
            rate_buy_count_per_s_1s_to_3s: None,
            rate_unique_signers_per_s_1s_to_2s: None,
            rate_unique_signers_per_s_1s_to_3s: None,
            rate_net_quote_sol_per_s_1s_to_2s: None,
            rate_net_quote_sol_per_s_1s_to_3s: None,
            delta_evidence: BTreeMap::new(),
        }
    }
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
    /// Legacy FSC scalar availability. `fsc` remains its compatibility alias.
    #[serde(default)]
    pub fsc_legacy: FeatureEvidenceStatus,
    /// FSC v2 readiness/coverage status. Evidence-only in PR2A.
    #[serde(default)]
    pub fsc_v2: FeatureEvidenceStatus,
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
            fsc_legacy: FeatureEvidenceStatus::default(),
            fsc_v2: FeatureEvidenceStatus::default(),
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
pub struct PreEntryPathSummaryV1 {
    #[serde(default)]
    pub pre_entry_ret_5s: Option<f64>,
    #[serde(default)]
    pub pre_entry_ret_10s: Option<f64>,
    #[serde(default)]
    pub pre_entry_ret_20s: Option<f64>,
    #[serde(default)]
    pub pre_entry_ret_30s: Option<f64>,
    #[serde(default)]
    pub pre_entry_ret_45s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mfe_10s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mfe_20s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mfe_30s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mfe_45s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mae_10s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mae_20s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mae_30s: Option<f64>,
    #[serde(default)]
    pub pre_entry_mae_45s: Option<f64>,
    #[serde(default)]
    pub pullback_depth_bps: Option<f64>,
    #[serde(default)]
    pub reclaim_bps: Option<f64>,
    #[serde(default)]
    pub reclaim_fraction: Option<f64>,
    #[serde(default)]
    pub higher_low_count: Option<u64>,
    #[serde(default)]
    pub above_0bps_dwell_ms: Option<u64>,
    #[serde(default)]
    pub above_300bps_dwell_ms: Option<u64>,
    #[serde(default)]
    pub above_600bps_dwell_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionRegimeSnapshotV1 {
    #[serde(default)]
    pub same_ms_tx_ratio_recent: Option<f64>,
    #[serde(default)]
    pub same_ms_tx_ratio_decay: Option<f64>,
    #[serde(default)]
    pub burst_ratio_recent: Option<f64>,
    #[serde(default)]
    pub burst_ratio_decay: Option<f64>,
    #[serde(default)]
    pub unique_ratio_recent: Option<f64>,
    #[serde(default)]
    pub unique_ratio_drift: Option<f64>,
    #[serde(default)]
    pub top3_signer_volume_ratio_recent: Option<f64>,
    #[serde(default)]
    pub top3_signer_volume_ratio_drift: Option<f64>,
    #[serde(default)]
    pub buy_sell_ratio_recent: Option<f64>,
    #[serde(default)]
    pub session_pool_rate_5m: Option<f64>,
    #[serde(default)]
    pub session_pool_rate_10m: Option<f64>,
    #[serde(default)]
    pub session_followthrough_rate_10m_optional: Option<f64>,
    #[serde(default)]
    pub template_reason_code: Option<String>,
    #[serde(default)]
    pub veto_reason_code: Option<String>,
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
    /// V3 evidence-plane temporal anchor and delta snapshot.
    #[serde(default)]
    pub temporal_deltas: TemporalDeltaFeatures,
    /// V3 evidence-plane full decision-time tick series for DTW/shape audit.
    #[serde(default)]
    pub decision_time_series: DecisionTimeSeriesFeatures,
    /// PR-RCE-A0: decision-time-safe pre-entry path summary for offline
    /// regime-confirmed entry proof. This is logging-only evidence and is not
    /// consumed by Gatekeeper policy.
    #[serde(default)]
    pub pre_entry_path_summary_v1: PreEntryPathSummaryV1,
    /// PR-RCE-A0: session-level regime summary for offline proof. Unknown
    /// global/session context remains nullable rather than imputed.
    #[serde(default)]
    pub session_regime_snapshot_v1: SessionRegimeSnapshotV1,
    /// Metric Contracts V1.1 PR2B: complete, validated ten-family compact
    /// decision-evidence projection. Historical payloads deserialize to
    /// `None`; current successful terminal materialization emits `Some`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::metric_contracts::serialize_optional_projection_wire_v1",
        deserialize_with = "crate::metric_contracts::deserialize_optional_projection_wire_v1"
    )]
    pub metric_contract_decision_projection_v1: Option<MetricContractDecisionEvidenceProjectionV1>,
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
