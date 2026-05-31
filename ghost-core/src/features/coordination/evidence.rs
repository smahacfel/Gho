use crate::features::coordination::types::{
    BseBreakdown, CoordinationSampleSummary, CpvBreakdown, CucdBreakdown, DbiaBreakdown,
    DesBreakdown, FtdiBreakdown, SfdBreakdown,
};
use crate::tx_intelligence::types::{FscExcludedReason, FscV2Evidence};
use serde::{Deserialize, Serialize};
use smallvec::{smallvec, SmallVec};
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricEvidenceStatus {
    Clean,
    Degraded,
    Unavailable,
    InsufficientSample,
    NotConfigured,
    /// Legacy compatibility only. Prefer `MetricPolicyMode::ExportOnly` for policy state.
    ExportOnly,
}

impl Default for MetricEvidenceStatus {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricPolicyMode {
    ExportOnly,
    ScoreEligible,
    Disabled,
}

impl Default for MetricPolicyMode {
    fn default() -> Self {
        Self::ExportOnly
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradedReason {
    InsufficientBuys,
    InsufficientUniqueSigners,

    MissingMeta,
    MissingSlotIndex,
    MissingComputeUnits,
    MissingCostUnits,
    MissingPrePostBalances,
    MissingDecisionSnapshot,
    MissingFrozenBuffer,
    MissingCurveState,
    MissingEconomicSpend,

    MissingDevBuy,
    DevTxNotComparable,

    RollingStateUnavailable,
    RollingStateNotWarm,
    MissingFeatureCutoff,
    ActivityAfterCutoff,
    CurrentPoolNotExcluded,

    FundingLaneUnavailable,
    InsufficientNonNeutralSupport,
    NeutralOnly,
    UnknownOnly,

    LowCoverage,
    SameSlotDominated,
    SpendFractionOutOfRange,
    DuplicateSlotIndex,

    ZeroOrInvalidMean,
    AllXTies,
    AllYTies,
    DenominatorZero,

    NotConfigured,
    ExportOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingVisibility {
    Available,
    Unavailable,
    Warmup,
    GapSuspected,
}

impl Default for FundingVisibility {
    fn default() -> Self {
        Self::Unavailable
    }
}

impl FundingVisibility {
    #[must_use]
    pub fn from_lane_health(lane_connected: bool, index_warm: bool, gap_suspected: bool) -> Self {
        if gap_suspected {
            return Self::GapSuspected;
        }

        if !lane_connected {
            return Self::Unavailable;
        }

        if !index_warm {
            return Self::Warmup;
        }

        Self::Available
    }

    #[must_use]
    pub fn from_fsc_v2_lane_health(evidence: Option<&FscV2Evidence>) -> Self {
        let Some(evidence) = evidence else {
            return Self::Unavailable;
        };

        if evidence.gap_suspected {
            return Self::GapSuspected;
        }

        match evidence.excluded_reason {
            Some(FscExcludedReason::FundingLaneUnavailable) => Self::Unavailable,
            Some(FscExcludedReason::IndexCold) => Self::Warmup,
            _ if !evidence.capture_ready => Self::Unavailable,
            _ if !evidence.index_warm => Self::Warmup,
            _ => Self::Available,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricBadDirection {
    LowIsBad,
    HighIsBad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationSnapshotMode {
    DecisionTime,
    EventualPostfill,
    ReplayFixture,
}

impl Default for CoordinationSnapshotMode {
    fn default() -> Self {
        Self::DecisionTime
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationMetricName {
    FeeTopologyDiversityIndex,
    DevBuyerInfraAffinity,
    SpendFractionDivergence,
    FundingSourceConcentration,
    SignerCrossPoolVelocity,
    DemandElasticityScore,
    BuySizingElasticity,
    CapitalTemplateConcentration,
    CrossPoolCohortRecurrence,
    ExecutionTemplateConcentration,
    ComputeUnitConsumptionDispersion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricValue {
    pub value: f64,
    pub severity: f64,
    pub confidence: f64,
    pub sample_n: u8,
    pub coverage: f64,
    pub status: MetricEvidenceStatus,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub degraded_reasons: SmallVec<[DegradedReason; 4]>,
}

impl MetricValue {
    #[must_use]
    pub fn new(
        value: f64,
        severity: f64,
        confidence: f64,
        sample_n: u8,
        coverage: f64,
        status: MetricEvidenceStatus,
    ) -> Self {
        Self {
            value,
            severity,
            confidence,
            sample_n,
            coverage,
            status,
            degraded_reasons: SmallVec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricEvidenceRecord<T> {
    pub evidence_status: MetricEvidenceStatus,
    pub policy_mode: MetricPolicyMode,
    pub score_eligible: bool,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub degraded_reasons: SmallVec<[DegradedReason; 4]>,
    pub breakdown: T,
}

impl<T: Default> Default for MetricEvidenceRecord<T> {
    fn default() -> Self {
        Self {
            evidence_status: MetricEvidenceStatus::Unavailable,
            policy_mode: MetricPolicyMode::ExportOnly,
            score_eligible: false,
            degraded_reasons: SmallVec::new(),
            breakdown: T::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedMetric {
    pub metric: CoordinationMetricName,
    pub reason: DegradedReason,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoordinationMetricBreakdowns {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_topology_diversity_index: Option<MetricEvidenceRecord<FtdiBreakdown>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_buyer_infra_affinity: Option<MetricEvidenceRecord<DbiaBreakdown>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_fraction_divergence: Option<MetricEvidenceRecord<SfdBreakdown>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_cross_pool_velocity: Option<MetricEvidenceRecord<CpvBreakdown>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_elasticity_score: Option<MetricEvidenceRecord<DesBreakdown>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buy_sizing_elasticity: Option<MetricEvidenceRecord<BseBreakdown>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_unit_consumption_dispersion: Option<MetricEvidenceRecord<CucdBreakdown>>,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub skipped_metrics: SmallVec<[SkippedMetric; 4]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationRiskEvidenceUnit {
    pub schema_version: u16,
    pub scope_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<String>,
    pub pool_id: Pubkey,
    pub mint: Pubkey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    pub decision_ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_slot: Option<u64>,
    pub snapshot_mode: CoordinationSnapshotMode,
    pub feature_cutoff_ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_cutoff_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_buffer_watermark_slot: Option<u64>,
    pub computed_at_recv_ts_ns: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_snapshot_hash: Option<String>,
    #[serde(default)]
    pub sample_summary: CoordinationSampleSummary,
    #[serde(default)]
    pub funding_visibility: FundingVisibility,
    #[serde(default)]
    pub features: CoordinationRiskFeatures,
    #[serde(default, alias = "breakdowns")]
    pub metric_breakdowns: CoordinationMetricBreakdowns,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub skipped_metrics: SmallVec<[SkippedMetric; 4]>,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub degraded_reasons: SmallVec<[DegradedReason; 4]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoordinationRiskFeatures {
    #[serde(default)]
    pub funding_visibility: FundingVisibility,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_topology_diversity_index: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_buyer_infra_affinity: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_fraction_divergence: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_concentration: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_cross_pool_velocity: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_elasticity_score: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buy_sizing_elasticity: Option<MetricValue>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capital_template_concentration: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_pool_cohort_recurrence: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_template_concentration: Option<MetricValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_unit_consumption_dispersion: Option<MetricValue>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_coordination_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_penalty: Option<f64>,

    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub degraded_reasons: SmallVec<[DegradedReason; 4]>,
}

impl Default for CoordinationRiskFeatures {
    fn default() -> Self {
        Self {
            funding_visibility: FundingVisibility::Unavailable,
            fee_topology_diversity_index: None,
            dev_buyer_infra_affinity: None,
            spend_fraction_divergence: None,
            funding_source_concentration: None,
            signer_cross_pool_velocity: None,
            demand_elasticity_score: None,
            buy_sizing_elasticity: None,
            capital_template_concentration: None,
            cross_pool_cohort_recurrence: None,
            execution_template_concentration: None,
            compute_unit_consumption_dispersion: None,
            total_coordination_penalty: None,
            interaction_penalty: None,
            degraded_reasons: default_funding_unavailable_reasons(),
        }
    }
}

fn default_funding_unavailable_reasons() -> SmallVec<[DegradedReason; 4]> {
    smallvec![DegradedReason::FundingLaneUnavailable]
}

#[must_use]
pub fn severity_low(value: f64, threshold: f64) -> f64 {
    if threshold <= 0.0 {
        return 0.0;
    }

    ((threshold - value) / threshold).clamp(0.0, 1.0)
}

#[must_use]
pub fn severity_high(value: f64, threshold: f64) -> f64 {
    if threshold >= 1.0 {
        return 0.0;
    }

    ((value - threshold) / (1.0 - threshold)).clamp(0.0, 1.0)
}
