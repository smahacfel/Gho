use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurstWindow {
    pub start_ts_ms: u64,
    pub end_ts_ms: u64,
    pub tx_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskSeverity {
    Hard,
    Soft(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskFlag {
    pub flag_id: Cow<'static, str>,
    pub severity: RiskSeverity,
    pub detected_at_ms: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TxIntelligenceState {
    pub total_buys: u64,
    pub total_sells: u64,
    pub total_tx: u64,
    pub unique_signers: HashSet<Pubkey>,
    pub buy_volume_sol: f64,
    pub sell_volume_sol: f64,
    pub dev_buy_lamports: u64,
    pub dev_has_sold: bool,
    pub dev_tx_count: u64,
    pub signer_volume_map: HashMap<Pubkey, f64>,
    pub tx_intervals_ms: Vec<u64>,
    pub burst_windows: Vec<BurstWindow>,
    pub bundle_suspicion_count: u64,
    pub same_ms_tx_count: u64,
    pub dust_tx_count: u64,
    pub failed_tx_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TxIntelFeatures {
    pub tx_count: u64,
    pub buy_count: u64,
    pub sell_count: u64,
    pub unique_signers: u64,
    pub buy_ratio: f64,
    pub sol_buy_ratio: f64,
    pub avg_tx_sol: f64,
    pub volume_cv: f64,
    pub hhi: f64,
    pub volume_gini: f64,
    pub unique_signer_ratio: f64,
    pub avg_tx_per_signer: f64,
    pub same_ms_tx_ratio: f64,
    pub bundle_suspicion_ratio: f64,
    pub top3_volume_pct: f64,
    pub dev_buy_sol: f64,
    pub dev_volume_ratio: f64,
    pub dev_tx_ratio: f64,
    pub dev_has_sold: bool,
    pub interval_cv: f64,
    pub timing_entropy: f64,
    pub avg_interval_ms: f64,
    pub burst_ratio: f64,
    pub dust_ratio: f64,
    #[serde(default)]
    pub max_tx_per_signer: u64,
    #[serde(default)]
    pub total_volume_sol: f64,
    #[serde(default)]
    pub min_tx_sol: f64,
    #[serde(default)]
    pub max_tx_sol: f64,
    #[serde(default)]
    pub max_consecutive_buys: u64,
    #[serde(default)]
    pub dev_wallet_known: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_initial_buy_tokens: Option<f64>,
    #[serde(default)]
    pub dev_tx_count: u64,
    #[serde(default)]
    pub dev_is_first_buyer: bool,
    #[serde(default)]
    pub dust_tx_count: u64,
    #[serde(default)]
    pub failed_tx_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FundingSourceDiagnostics {
    /// Number of FSC lookup units considered for this BUY.
    ///
    /// This is the canonical buyer sample used by FSC after deduplicating
    /// buyers with a known identity. Successful BUY txs that have no canonical
    /// buyer identity are still counted here as unresolved lookup units.
    #[serde(default)]
    pub buyer_sample_count: u64,
    /// Number of buyer sample units that resolved to a known funding source.
    #[serde(default)]
    pub known_source_count: u64,
    /// Number of buyer sample units that did not resolve to a known funding source.
    #[serde(default)]
    pub unknown_buyer_count: u64,
    /// Unknowns classified as structurally unobservable under the current FSC model.
    #[serde(default)]
    pub structural_unknown_buyer_count: u64,
    /// Unknowns classified as operational / attainable misses.
    #[serde(default)]
    pub operational_unknown_buyer_count: u64,
    /// Unknowns that remain undecidable with current runtime evidence.
    #[serde(default)]
    pub indeterminate_unknown_buyer_count: u64,
    /// Aggregated miss taxonomy counts for the buyer sample.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub miss_reason_counts: Vec<FundingSourceMissReasonCount>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FscMissClass {
    Structural,
    Operational,
    Indeterminate,
}

impl FscMissClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Structural => "structural",
            Self::Operational => "operational",
            Self::Indeterminate => "indeterminate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingSourceMissReasonCount {
    pub reason: String,
    pub class: FscMissClass,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SybilResistanceFeatures {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_topology_diversity_index: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_buyer_infrastructure_affinity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_fraction_divergence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_elasticity_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_cross_pool_velocity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_concentration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_diagnostics: Option<FundingSourceDiagnostics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<String>,
    #[serde(default)]
    pub buy_sample_count: u64,
    #[serde(default)]
    pub signer_sample_count: u64,
}

pub const FTDI_INSUFFICIENT_BUYS_REASON: &str = "FTDI_INSUFFICIENT_BUYS";
pub const FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON: &str = "FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE";
pub const DBIA_NO_DEV_BUY_REASON: &str = "DBIA_NO_DEV_BUY";
pub const DBIA_INSUFFICIENT_BUYERS_REASON: &str = "DBIA_INSUFFICIENT_BUYERS";
pub const DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON: &str = "DBIA_RAW_FINGERPRINT_UNAVAILABLE";
pub const SFD_INSUFFICIENT_BUYS_REASON: &str = "SFD_INSUFFICIENT_BUYS";
pub const SFD_ZERO_PREBALANCE_SKIPPED_REASON: &str = "SFD_ZERO_PREBALANCE_SKIPPED";
/// Legacy hard-failure label retained for compatibility.
///
/// Despite the historical name, this branch is also used when a required
/// signer pre-balance snapshot is missing and SFD cannot be materialized from
/// the remaining sample set.
pub const SFD_POSTBALANCE_UNAVAILABLE_REASON: &str = "SFD_POSTBALANCE_UNAVAILABLE";
pub const SFD_PARTIAL_BALANCE_COVERAGE_REASON: &str = "SFD_PARTIAL_BALANCE_COVERAGE";
pub const DES_INSUFFICIENT_BUYS_REASON: &str = "DES_INSUFFICIENT_BUYS";
pub const DES_CURVE_DATA_UNAVAILABLE_REASON: &str = "DES_CURVE_DATA_UNAVAILABLE";
pub const DES_SLOT_ORDER_UNAVAILABLE_REASON: &str = "DES_SLOT_ORDER_UNAVAILABLE";
pub const CPV_ROLLING_STATE_UNAVAILABLE_REASON: &str = "CPV_ROLLING_STATE_UNAVAILABLE";
pub const CPV_INSUFFICIENT_SIGNERS_REASON: &str = "CPV_INSUFFICIENT_SIGNERS";
pub const FSC_ROLLING_STATE_UNAVAILABLE_REASON: &str = "FSC_ROLLING_STATE_UNAVAILABLE";
pub const FSC_INSUFFICIENT_KNOWN_SOURCES_REASON: &str = "FSC_INSUFFICIENT_KNOWN_SOURCES";
pub const FSC_FUNDING_STREAM_UNAVAILABLE_REASON: &str = "FSC_FUNDING_STREAM_UNAVAILABLE";
pub const FSC_BUYER_IDENTITY_UNAVAILABLE_REASON: &str = "FSC_BUYER_IDENTITY_UNAVAILABLE";
pub const FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON: &str = "FSC_BUY_TIMESTAMP_UNAVAILABLE";
pub const FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON: &str = "FSC_NO_RETAINED_RECIPIENT_HISTORY";
pub const FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON: &str = "FSC_LOOKBACK_WINDOW_EXHAUSTED";
pub const FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON: &str = "FSC_NO_PREBUY_TRANSFER_IN_WINDOW";
pub const FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON: &str = "FSC_PER_RECIPIENT_HISTORY_OVERFLOW";
pub const FSC_GLOBAL_RECIPIENT_EVICTED_REASON: &str = "FSC_GLOBAL_RECIPIENT_EVICTED";
