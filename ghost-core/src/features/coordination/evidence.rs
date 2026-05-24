use serde::{Deserialize, Serialize};
use smallvec::{smallvec, SmallVec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricEvidenceStatus {
    Clean,
    Degraded,
    Unavailable,
    InsufficientSample,
    NotConfigured,
    ExportOnly,
}

impl Default for MetricEvidenceStatus {
    fn default() -> Self {
        Self::Unavailable
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
    MissingCurveState,
    MissingEconomicSpend,

    MissingDevBuy,
    DevTxNotComparable,

    RollingStateUnavailable,
    RollingStateNotWarm,

    FundingLaneUnavailable,

    LowCoverage,
    SameSlotDominated,

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
}

impl Default for FundingVisibility {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricBadDirection {
    LowIsBad,
    HighIsBad,
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
