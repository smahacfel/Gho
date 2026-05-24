pub mod config;
pub mod evidence;
pub mod samples;
pub mod stats;
pub mod types;

pub use config::CoordinationRiskConfig;
pub use evidence::{
    severity_high, severity_low, CoordinationRiskFeatures, DegradedReason, FundingVisibility,
    MetricBadDirection, MetricEvidenceStatus, MetricValue,
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
    CapitalTemplateFingerprint, CoordinationSampleFixture, CoordinationSampleSummary,
    EconomicSpend, EconomicSpendSource, ExecutionTemplateFingerprint, FeeTopologyFingerprint,
    ObservedBuyTx, T0Source, TxTimeSource,
};
