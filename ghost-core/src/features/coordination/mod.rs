pub mod config;
pub mod evidence;
pub mod metrics;
pub mod samples;
pub mod stats;
pub mod types;

pub use config::CoordinationRiskConfig;
pub use evidence::{
    severity_high, severity_low, CoordinationMetricBreakdowns, CoordinationMetricName,
    CoordinationRiskEvidenceUnit, CoordinationRiskFeatures, CoordinationSnapshotMode,
    DegradedReason, FundingVisibility, MetricBadDirection, MetricEvidenceRecord,
    MetricEvidenceStatus, MetricPolicyMode, MetricValue, SkippedMetric,
};
pub use metrics::{
    build_coordination_risk_evidence_unit, build_coordination_risk_evidence_unit_from_snapshot,
    compute_bse_v2, compute_cpv_v2, compute_cucd_v2, compute_dbia_v2, compute_des_v2,
    compute_ftdi_v2, compute_sfd_v2, funding_source_concentration_from_fsc_v2,
    sample_summary_for_evidence, skipped_phase06_breakdowns, CoordinationRiskEvidenceInput,
    FrozenCoordinationDecisionSnapshot, MetricComputation,
};
pub use samples::{
    build_observed_buy_txs_from_fixture, sequence_buys, summarize_observed_buy_txs,
    unique_first_buys_by_signer, SequenceBuildError,
};
pub use stats::{
    cv, diversity_from_hhi_norm, kendall_tau_b, mad, median, normalized_hhi_from_counts, robust_cv,
    weighted_mad, weighted_median,
};
pub use types::{
    BseBreakdown, CapitalTemplateFingerprint, CoordinationSampleFixture, CoordinationSampleSummary,
    CpvBreakdown, CpvSignerIntensity, CucdBreakdown, CucdBucketCount, DbiaBreakdown, DesBreakdown,
    DevFingerprintEvidence, DevFingerprintMode, EconomicSpend, EconomicSpendSource,
    ExecutionTemplateFingerprint, FeeTopologyCount, FeeTopologyFingerprint, FtdiBreakdown,
    InfraFingerprint, ObservedBuyTx, SfdBreakdown, SfdSourceCount, SignerCrossPoolActivity,
    T0Source, TxTimeSource,
};
