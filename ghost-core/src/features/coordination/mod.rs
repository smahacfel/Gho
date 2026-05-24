pub mod config;
pub mod evidence;
pub mod samples;
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
pub use types::{
    CapitalTemplateFingerprint, CoordinationSampleFixture, CoordinationSampleSummary,
    EconomicSpend, EconomicSpendSource, ExecutionTemplateFingerprint, FeeTopologyFingerprint,
    ObservedBuyTx, T0Source, TxTimeSource,
};
