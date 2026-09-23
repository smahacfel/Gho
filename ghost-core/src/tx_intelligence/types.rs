use crate::checkpoint::CpvEvidenceContext;
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
    /// Preferred signer-volume concentration metric in ratio scale `0.0..1.0`.
    ///
    /// `top3_volume_pct` is retained as a compatibility alias for older payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top3_signer_volume_ratio: Option<f64>,
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

impl TxIntelFeatures {
    /// Preferred top3 signer-volume concentration read path.
    ///
    /// New payloads should set `top3_signer_volume_ratio`; old payloads fallback
    /// to the legacy `top3_volume_pct` alias, which already carried ratio-scale
    /// values despite its name.
    #[must_use]
    pub fn effective_top3_signer_volume_ratio(&self) -> f64 {
        self.top3_signer_volume_ratio
            .unwrap_or(self.top3_volume_pct)
    }
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
    /// Transfer candidates below the configured absolute attribution floor.
    #[serde(default)]
    pub dust_filtered_count: u64,
    /// Transfer candidates observed after the buyer's first buy.
    #[serde(default)]
    pub post_buy_filtered_count: u64,
    /// Transfer candidates below the configured transfer-to-buy ratio.
    #[serde(default)]
    pub rel_too_small_count: u64,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FscVersion {
    #[default]
    V2,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FscAttributionScope {
    #[default]
    SingleHopNativeSol,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FscSnapshotMode {
    #[default]
    DecisionTime,
    EventualPostfill,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FscEvidenceStatus {
    Clean,
    Degraded,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FscExcludedReason {
    FundingLaneUnavailable,
    IndexCold,
    NoBuyerCohort,
    InsufficientNonNeutralSupport,
    LowCoverage,
    NeutralOnly,
    SameSlotOrderingUnavailable,
    LowAttributionConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingSourceKey {
    pub wallet: String,
}

impl FundingSourceKey {
    #[must_use]
    pub fn new(wallet: impl Into<String>) -> Self {
        Self {
            wallet: wallet.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingSourceCount {
    pub source: FundingSourceKey,
    pub count: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FscV2Evidence {
    pub version: FscVersion,
    pub attribution_scope: FscAttributionScope,
    pub snapshot_mode: FscSnapshotMode,

    pub total_buyers: u8,
    pub known_buyers: u8,
    pub known_non_neutral_buyers: u8,
    pub unknown_count: u8,
    pub neutral_count: u8,
    pub low_confidence_count: u8,
    pub same_slot_unorderable_count: u16,

    pub known_coverage: f64,
    pub non_neutral_known_coverage: f64,
    pub neutral_share: f64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top1_share_count: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top1_share_sol: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hhi_norm_count: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hhi_norm_sol_weighted_excess: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_hhi_including_neutral: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoring_hhi_non_neutral: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_funder: Option<FundingSourceKey>,
    pub top_funder_count: u8,
    pub top_funder_buy_sol: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_counts: Vec<FundingSourceCount>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution_confidence_mean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution_confidence_min: Option<f64>,

    pub dust_filtered_count: u16,
    pub post_buy_filtered_count: u16,
    pub rel_too_small_count: u16,

    pub index_warm: bool,
    pub capture_ready: bool,
    pub status: FscEvidenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded_reason: Option<FscExcludedReason>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_lane_watermark_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_buy_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_lane_lag_slots: Option<i64>,
    pub stream_epoch: u64,
    pub gap_suspected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transfer_recv_ts_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reconnect_ts_ms: Option<u64>,
    #[serde(default)]
    pub dropped_events: u64,

    pub min_abs_store_lamports: u64,
    pub min_abs_attribution_lamports: u64,
    pub min_rel_to_buy: f64,
    pub ttl_seconds: u64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neutral_funder_set_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neutral_funder_set_hash: Option<String>,
    pub config_hash: String,
    pub provider: String,
    pub source_topics: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SybilResistanceFeatures {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Historyczny FTDI K/N; nowa definicja ma osobny rekord V2.
    pub fee_topology_diversity_index: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_buyer_infrastructure_affinity: Option<f64>,
    /// MAD frakcji netto natywnego SOL ubywającego signerom w transakcjach BUY.
    /// Zawiera opłaty i inne przepływy w transakcji; nie mierzy całego majątku
    /// ani wyizolowanego kosztu konkretnego tokena. Zakres poprawnego MAD: [0, 0.5].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_fraction_divergence: Option<f64>,
    /// Historyczny DES V1. Nie wolno zapisywać tu definicji next-BUY tau-b.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_elasticity_score: Option<f64>,
    /// DES V2: zmiana zaobserwowanej ceny a odstęp do następnego BUY.
    /// Osobny klucz zachowuje znaczenie starych rekordów i progów V1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_elasticity_v2: Option<DesEvidenceV2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_cross_pool_velocity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpv_other_pool_activity: Option<f64>,
    /// Shared evidence context for CPV-family metrics:
    /// `signer_cross_pool_velocity` and `cpv_other_pool_activity`.
    #[serde(default)]
    pub cpv_evidence: CpvEvidenceContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_concentration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_diagnostics: Option<FundingSourceDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_v2: Option<FscV2Evidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<String>,
    #[serde(default)]
    pub buy_sample_count: u64,
    #[serde(default)]
    pub signer_sample_count: u64,
    /// Wersjonowane pomiary z tego samego producenta i cutoff, nie nowe kalkulatory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_topology_diversity_v2: Option<FtdiEvidenceV2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dbia_evidence_v1: Option<DbiaEvidenceV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sfd_evidence_v1: Option<SfdEvidenceV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_cutoff_received_ms: Option<u64>,
}

/// Wersja wzoru, niezależna od historycznego FtdiUniqueBuyerActionabilityV2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FtdiDefinitionV2 {
    GiniSimpson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FtdiEvidenceV2 {
    pub definition: FtdiDefinitionV2,
    pub fee_topology_diversity_index: Option<f64>,
    pub coordination_hhi: Option<f64>,
    pub unique_topology_count: u64,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
    pub represented_signer_count: u64,
    pub degraded_reasons: Vec<String>,
}

impl FtdiEvidenceV2 {
    /// Kontrola kontraktu 1-HHI; nigdy nie przyjmuje K/N jako nowej definicji.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.unique_topology_count > self.represented_signer_count
            || self.represented_signer_count > self.signer_sample_count
            || self.signer_sample_count > self.buy_sample_count
        {
            return Err("ftdi_v2.counts");
        }
        match (self.fee_topology_diversity_index, self.coordination_hhi) {
            (Some(value), Some(hhi))
                if value.is_finite()
                    && hhi.is_finite()
                    && (0.0..=1.0).contains(&value)
                    && (0.0..=1.0).contains(&hhi)
                    && hhi > 0.0
                    && self.unique_topology_count > 0
                    && self.represented_signer_count == self.signer_sample_count
                    && value.to_bits() == (1.0 - hhi).to_bits() =>
            {
                // Granice każdego rozkładu N reprezentantów na K niepustych klas.
                // Nie odtwarzamy histogramu ani pomiaru po stronie odbiorcy.
                let n = u128::from(self.represented_signer_count);
                let k = u128::from(self.unique_topology_count);
                let q = n / k;
                let remainder = n % k;
                let min_squares = (k - remainder) * q * q + remainder * (q + 1) * (q + 1);
                let max_squares = (n - k + 1) * (n - k + 1) + k - 1;
                let denominator = (n * n) as f64;
                if hhi < min_squares as f64 / denominator || hhi > max_squares as f64 / denominator
                {
                    return Err("ftdi_v2.hhi_population_bounds");
                }
                Ok(())
            }
            (None, None) => Ok(()),
            _ => Err("ftdi_v2.gini_simpson"),
        }
    }

    pub fn has_full_quality(&self) -> bool {
        self.validate().is_ok()
            && self.fee_topology_diversity_index.is_some()
            && self.represented_signer_count >= 3
            && self.represented_signer_count == self.signer_sample_count
            && self.degraded_reasons.is_empty()
    }
}

/// Podobieństwo struktury do referencji deva; nie dowód wspólnego właściciela.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbiaEvidenceV1 {
    pub dev_buyer_infrastructure_affinity: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    /// Liczność obejmuje deva, który nie należy do średniej buyerów.
    pub signer_sample_count: u64,
    pub represented_signer_count: u64,
}

impl DbiaEvidenceV1 {
    pub fn has_full_quality(&self) -> bool {
        self.dev_buyer_infrastructure_affinity
            .is_some_and(|v| v.is_finite() && (0.0..=1.0).contains(&v))
            && self.represented_signer_count >= 3
            && self.represented_signer_count == self.signer_sample_count
            && self.signer_sample_count <= self.buy_sample_count
            && self.degraded_reasons.is_empty()
    }
}

/// MAD frakcji netto natywnego SOL; pełna populacja i użyta próba są odrębne.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SfdEvidenceV1 {
    pub spend_fraction_divergence: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
    pub represented_signer_count: u64,
}

impl SfdEvidenceV1 {
    pub fn has_full_quality(&self) -> bool {
        self.spend_fraction_divergence
            .is_some_and(|v| v.is_finite() && (0.0..=0.5).contains(&v))
            && self.represented_signer_count >= 3
            && self.represented_signer_count == self.signer_sample_count
            && self.signer_sample_count <= self.buy_sample_count
            && self.degraded_reasons.is_empty()
    }
}

/// Definicja jest częścią zapisu; nieznanej wersji nie odczytujemy jako V2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesDefinitionV2 {
    NextBuySlotTauB,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesIntervalUnitV2 {
    Slots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesPriceSourceV2 {
    /// Surowe rezerwy post-trade Pump: lamporty / bazowe jednostki tokena.
    /// Nie obejmuje znormalizowanych rezerw PumpSwap ani price_quote.
    PumpVirtualPostTradeReserves,
}

/// Jeden wynik producenta DES, przenoszony bez przeliczania do MFS.
/// Zmiana ceny obejmuje także SELL pomiędzy BUY; nie oznacza wpływu samego BUY.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesEvidenceV2 {
    pub definition: DesDefinitionV2,
    pub interval_unit: DesIntervalUnitV2,
    pub price_source: DesPriceSourceV2,
    pub demand_elasticity_score: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub buy_sample_count: u64,
    pub signer_sample_count: u64,
    pub priced_buy_count: u64,
    /// Liczba potencjalnych zamkniętych trójek w uporządkowanym oknie BUY.
    pub candidate_triple_count: u64,
    /// Liczba faktycznie użytych, kompletnych i porównywalnych trójek.
    pub closed_triple_count: u64,
}

impl DesEvidenceV2 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.signer_sample_count > self.buy_sample_count
            || self.priced_buy_count > self.buy_sample_count
            || self.closed_triple_count > self.candidate_triple_count
            || self.candidate_triple_count > self.buy_sample_count.saturating_sub(2)
            || self.demand_elasticity_score.is_some_and(|v| {
                !v.is_finite() || !(-1.0..=1.0).contains(&v) || self.closed_triple_count < 2
            })
        {
            return Err("des_v2.value_or_counts");
        }
        Ok(())
    }

    /// Jakość pomiaru, niezależna od dostępności progu strategii dla V2.
    pub fn has_full_quality(&self) -> bool {
        self.validate().is_ok()
            && self
                .demand_elasticity_score
                .is_some_and(|value| value.is_finite() && (-1.0..=1.0).contains(&value))
            && self.closed_triple_count >= 3
            && self.closed_triple_count == self.candidate_triple_count
            && self.candidate_triple_count == self.buy_sample_count.saturating_sub(2)
            && self.priced_buy_count == self.buy_sample_count
            && self.degraded_reasons.is_empty()
    }
}

pub const FTDI_COMPARISON_DEFINITION_MISMATCH_REASON: &str = "FTDI_COMPARISON_DEFINITION_MISMATCH";
pub const FTDI_INSUFFICIENT_BUYS_REASON: &str = "FTDI_INSUFFICIENT_BUYS";
pub const FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON: &str = "FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE";
pub const DBIA_NO_DEV_BUY_REASON: &str = "DBIA_NO_DEV_BUY";
pub const DBIA_INSUFFICIENT_BUYERS_REASON: &str = "DBIA_INSUFFICIENT_BUYERS";
pub const DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON: &str = "DBIA_RAW_FINGERPRINT_UNAVAILABLE";
pub const SFD_INSUFFICIENT_BUYS_REASON: &str = "SFD_INSUFFICIENT_BUYS";
pub const SFD_ZERO_PREBALANCE_SKIPPED_REASON: &str = "SFD_ZERO_PREBALANCE_SKIPPED";
/// Historyczna nazwa zachowana dla zgodności: brak któregokolwiek salda signera.
/// Może towarzyszyć diagnostycznemu MAD z pozostałych reprezentantów;
/// nie oznacza pełnego pokrycia ani używalności takiej próbki w policy.
pub const SFD_POSTBALANCE_UNAVAILABLE_REASON: &str = "SFD_POSTBALANCE_UNAVAILABLE";
pub const SFD_PARTIAL_BALANCE_COVERAGE_REASON: &str = "SFD_PARTIAL_BALANCE_COVERAGE";
pub const SFD_INVALID_BALANCE_PAIR_REASON: &str = "SFD_INPUT_INVALID_BALANCE_PAIR";
pub const DES_INSUFFICIENT_BUYS_REASON: &str = "DES_INSUFFICIENT_BUYS";
pub const DES_CURVE_DATA_UNAVAILABLE_REASON: &str = "DES_CURVE_DATA_UNAVAILABLE";
pub const DES_SLOT_ORDER_UNAVAILABLE_REASON: &str = "DES_SLOT_ORDER_UNAVAILABLE";
/// Minimum oceniamy po poprawnych trójkach, nie po surowej liczbie BUY.
pub const DES_INSUFFICIENT_TRIPLES_REASON: &str = "DES_INSUFFICIENT_CLOSED_TRIPLES";
pub const DES_NO_COMPARABLE_PAIRS_REASON: &str = "DES_NO_COMPARABLE_PAIRS";
pub const DES_PRICE_DOMAIN_MISMATCH_REASON: &str = "DES_INPUT_PRICE_DOMAIN_MISMATCH";
/// Nie jest błędem pomiaru V2: istniejący próg policy ma definicję V1.
pub const DES_COMPARISON_DEFINITION_MISMATCH_REASON: &str = "DES_COMPARISON_DEFINITION_MISMATCH";
pub const CPV_ROLLING_STATE_UNAVAILABLE_REASON: &str = "CPV_ROLLING_STATE_UNAVAILABLE";
pub const CPV_INSUFFICIENT_SUCCESSFUL_BUY_SIGNERS_REASON: &str =
    "CPV_INSUFFICIENT_SUCCESSFUL_BUY_SIGNERS";
pub const CPV_INSUFFICIENT_SIGNERS_REASON: &str = CPV_INSUFFICIENT_SUCCESSFUL_BUY_SIGNERS_REASON;
pub const CPV_LOW_SAMPLE_DEGRADED_REASON: &str = "CPV_LOW_SAMPLE_DEGRADED";
pub const CPV_DISABLED_BY_CONFIG_REASON: &str = "CPV_DISABLED_BY_CONFIG";
pub const FSC_ROLLING_STATE_UNAVAILABLE_REASON: &str = "FSC_ROLLING_STATE_UNAVAILABLE";
pub const FSC_INSUFFICIENT_KNOWN_SOURCES_REASON: &str = "FSC_INSUFFICIENT_KNOWN_SOURCES";
pub const FSC_FUNDING_STREAM_UNAVAILABLE_REASON: &str = "FSC_FUNDING_STREAM_UNAVAILABLE";
pub const FSC_BUYER_IDENTITY_UNAVAILABLE_REASON: &str = "FSC_BUYER_IDENTITY_UNAVAILABLE";
pub const FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON: &str = "FSC_BUY_TIMESTAMP_UNAVAILABLE";
pub const FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON: &str = "FSC_NO_RETAINED_RECIPIENT_HISTORY";
pub const FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON: &str = "FSC_LOOKBACK_WINDOW_EXHAUSTED";
pub const FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON: &str = "FSC_NO_PREBUY_TRANSFER_IN_WINDOW";
pub const FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON: &str = "FSC_SAME_SLOT_ORDERING_UNAVAILABLE";
pub const FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON: &str = "FSC_LOW_ATTRIBUTION_CONFIDENCE";
pub const FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON: &str = "FSC_ABS_ATTRIBUTION_TOO_SMALL";
pub const FSC_RELATIVE_FUNDING_TOO_SMALL_REASON: &str = "FSC_RELATIVE_FUNDING_TOO_SMALL";
pub const FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON: &str = "FSC_PER_RECIPIENT_HISTORY_OVERFLOW";
pub const FSC_GLOBAL_RECIPIENT_EVICTED_REASON: &str = "FSC_GLOBAL_RECIPIENT_EVICTED";
