use crate::events::{FundingTransferObserved, PoolTransaction};
use crate::oracle_metrics::{
    record_fsc_authoritative_funding_stream_available, record_fsc_index_entries,
    record_fsc_index_global_evictions, record_fsc_index_per_recipient_overflows,
    record_fsc_lookup_hits, record_fsc_lookup_miss_reason, record_fsc_lookup_misses,
    record_fsc_prune_duration_ms, record_fsc_warmup_ready,
};
use ghost_brain::config::{FscV2Config, GatekeeperV2Config};
use ghost_core::tx_intelligence::types::{
    FscAttributionScope, FscEvidenceStatus, FscExcludedReason, FscMissClass, FscSnapshotMode,
    FscV2Evidence, FscVersion, FundingSourceCount, FundingSourceDiagnostics, FundingSourceKey,
    FundingSourceMissReasonCount, FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON,
    FSC_BUYER_IDENTITY_UNAVAILABLE_REASON, FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON,
    FSC_FUNDING_STREAM_UNAVAILABLE_REASON, FSC_GLOBAL_RECIPIENT_EVICTED_REASON,
    FSC_INSUFFICIENT_KNOWN_SOURCES_REASON, FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON,
    FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON, FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON,
    FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON, FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON,
    FSC_RELATIVE_FUNDING_TOO_SMALL_REASON, FSC_ROLLING_STATE_UNAVAILABLE_REASON,
    FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const FSC_V2_PROVIDER_LEGACY_ROLLING_INDEX: &str = "ghost_legacy_rolling_funding_index";
const FSC_V2_PROVIDER_NLN_PROGRAM_STREAMS: &str = "nln_program_streams";
const FSC_V2_TOPIC_LEGACY_FUNDING_TRANSFERS: &str = "ghost.funding_transfers";
const FSC_V2_TOPIC_NLN_SYSTEM_TRANSFERS: &str = "prod.rpc.solana.system.transfers";
const FSC_V2_MIN_TOTAL_BUYERS: u64 = 2;
const FSC_V2_MIN_KNOWN_NON_NEUTRAL_BUYERS: u64 = 2;
const FSC_V2_MIN_KNOWN_COVERAGE: f64 = 0.50;
const FSC_V2_MIN_NON_NEUTRAL_KNOWN_COVERAGE: f64 = 0.30;

#[derive(Debug, Clone, PartialEq)]
pub struct FundingSourceConfig {
    pub lookback_window_ms: u64,
    pub min_abs_store_lamports: u64,
    pub min_abs_attribution_lamports: u64,
    pub min_rel_to_buy: f64,
    pub min_attribution_confidence_bps: u16,
    pub per_recipient_cap: usize,
    pub global_recipient_cap: usize,
    pub neutral_funder_set_version: Option<String>,
    neutral_funding_sources: HashSet<String>,
}

impl FundingSourceConfig {
    #[must_use]
    pub fn from_gatekeeper_config(config: &GatekeeperV2Config) -> Self {
        Self::from_configs(config, None)
    }

    #[must_use]
    pub fn from_configs(config: &GatekeeperV2Config, fsc_v2: Option<&FscV2Config>) -> Self {
        let lookback_window_ms = fsc_v2
            .map(|fsc| fsc.lookback_window_s.saturating_mul(1_000).max(1))
            .unwrap_or_else(|| {
                config
                    .funding_lookback_window_s
                    .saturating_mul(1_000)
                    .max(1)
            });
        let min_abs_store_lamports = fsc_v2
            .map(|fsc| fsc.min_abs_store_lamports)
            .unwrap_or(config.funding_dust_threshold_lamports);
        let min_abs_attribution_lamports = fsc_v2
            .map(|fsc| fsc.min_abs_attribution_lamports)
            .unwrap_or(config.funding_dust_threshold_lamports);
        let min_rel_to_buy = fsc_v2.map(|fsc| fsc.min_rel_to_buy).unwrap_or(0.0);
        let min_attribution_confidence_bps = fsc_v2
            .map(|fsc| unit_interval_to_bps(fsc.min_attribution_confidence))
            .unwrap_or(6_000);
        Self {
            lookback_window_ms,
            min_abs_store_lamports,
            min_abs_attribution_lamports,
            min_rel_to_buy,
            min_attribution_confidence_bps,
            per_recipient_cap: config.fsc_per_recipient_cap.max(1),
            global_recipient_cap: config.fsc_global_recipient_cap.max(1),
            neutral_funder_set_version: fsc_v2
                .and_then(|fsc| fsc.neutral_funder_set_version.clone()),
            neutral_funding_sources: config
                .neutral_funding_sources
                .iter()
                .filter_map(|value| {
                    let trimmed = value.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                })
                .collect(),
        }
    }

    fn is_neutral_source(&self, wallet: &str) -> bool {
        self.neutral_funding_sources.contains(wallet)
    }
}

fn unit_interval_to_bps(value: f64) -> u16 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 10_000.0).round() as u16
}

#[derive(Debug, Clone, PartialEq)]
pub struct FscComputation {
    pub funding_source_concentration: Option<f64>,
    pub funding_source_v2: FscV2Evidence,
    pub degraded_reasons: Vec<String>,
    pub diagnostics: FundingSourceDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FundingCoverageWindowStatus {
    pub stream_available: bool,
    pub warmup_ready: bool,
    pub coverage_window_ready: bool,
    pub authoritative_buy_ready: bool,
    pub coverage_window_remaining_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FundingTransferRecord {
    slot: Option<u64>,
    source_wallet: String,
    signature: String,
    lamports: u64,
    observed_at_ms: u64,
    arrival_ts_ms: u64,
    event_ordinal: Option<u32>,
    tx_index: Option<u32>,
    outer_instruction_index: Option<u32>,
    inner_group_index: Option<u32>,
    cpi_stack_height: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RecipientHistory {
    transfers: VecDeque<FundingTransferRecord>,
    last_seen_ms: u64,
    overflowed_before_oldest_retained: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EvictedRecipientHistory {
    last_seen_ms: u64,
}

#[derive(Debug, Default)]
struct FundingSourceInner {
    histories: HashMap<String, RecipientHistory>,
    recipient_order: VecDeque<(u64, String)>,
    evicted_recipients: HashMap<String, EvictedRecipientHistory>,
    evicted_recipient_order: VecDeque<(u64, String)>,
    stream_available: bool,
    stream_available_since_ms: Option<u64>,
    saw_transfer: bool,
    availability_controlled: bool,
    observed_funding_lane_kinds: HashSet<String>,
    funding_lane_watermark_slot: Option<u64>,
    last_transfer_recv_ts_ms: Option<u64>,
    last_reconnect_ts_ms: Option<u64>,
    stream_epoch: u64,
    gap_suspected: bool,
    dropped_events: u64,
}

#[derive(Debug, Default)]
pub struct FundingSourceIndex {
    inner: RwLock<FundingSourceInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FundingSourceMatch {
    Concrete(String),
    Neutral {
        source_wallet: String,
        legacy_key: String,
    },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LookupSourceResult {
    matched: FundingSourceMatch,
    removed: bool,
    miss: Option<LookupMiss>,
    attribution_confidence_bps: Option<u16>,
    selected_lamports: u128,
    total_lamports: u128,
    dust_filtered_count: u64,
    post_buy_filtered_count: u64,
    rel_too_small_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LookupMiss {
    reason: &'static str,
    class: FscMissClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WalletLookupOutcome {
    Matched {
        matched: FundingSourceMatch,
        removed: bool,
        attribution_confidence_bps: u16,
        selected_lamports: u128,
        total_lamports: u128,
        dust_filtered_count: u64,
        post_buy_filtered_count: u64,
        rel_too_small_count: u64,
    },
    ContinueMiss {
        miss: LookupMiss,
        removed: bool,
        dust_filtered_count: u64,
        post_buy_filtered_count: u64,
        rel_too_small_count: u64,
    },
    TerminalMiss {
        miss: LookupMiss,
        removed: bool,
        dust_filtered_count: u64,
        post_buy_filtered_count: u64,
        rel_too_small_count: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FundingAttributionSelection {
    recipient_wallet: String,
    source_wallet: String,
    selected_lamports: u128,
    total_lamports: u128,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LookupCounters {
    dust_filtered_count: u64,
    post_buy_filtered_count: u64,
    rel_too_small_count: u64,
}

impl LookupCounters {
    fn merge(&mut self, other: LookupCounters) {
        self.dust_filtered_count = self
            .dust_filtered_count
            .saturating_add(other.dust_filtered_count);
        self.post_buy_filtered_count = self
            .post_buy_filtered_count
            .saturating_add(other.post_buy_filtered_count);
        self.rel_too_small_count = self
            .rel_too_small_count
            .saturating_add(other.rel_too_small_count);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceAccumulator {
    recipient_wallet: String,
    source_wallet: String,
    total_lamports: u128,
    latest_transfer_key: TransferTieBreakKey,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BuyOrderKey {
    slot: u64,
    tx_index: u32,
    event_ordinal: u32,
    event_ts_ms: u64,
    arrival_ts_ms: u64,
    signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TransferTieBreakKey {
    slot: u64,
    tx_index: u32,
    event_ordinal: u32,
    observed_at_ms: u64,
    arrival_ts_ms: u64,
    signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferBuyOrder {
    Precedes,
    DoesNotPrecede,
    Unorderable,
}

#[derive(Debug, Default)]
struct FscV2Accumulator {
    total_buyers: u64,
    known_buyers: u64,
    known_non_neutral_buyers: u64,
    unknown_count: u64,
    neutral_count: u64,
    confidence_sum_bps: u64,
    confidence_min_bps: Option<u16>,
    non_neutral_source_counts: HashMap<String, u64>,
    non_neutral_source_buy_sol: HashMap<String, f64>,
    raw_source_counts: HashMap<String, u64>,
    non_neutral_buyer_weights: Vec<(String, f64)>,
}

impl FscV2Accumulator {
    fn new(total_buyers: usize) -> Self {
        Self {
            total_buyers: total_buyers as u64,
            ..Self::default()
        }
    }

    fn record_concrete(&mut self, source: String, buy_sol: f64, confidence_bps: Option<u16>) {
        self.known_buyers = self.known_buyers.saturating_add(1);
        self.known_non_neutral_buyers = self.known_non_neutral_buyers.saturating_add(1);
        increment_count(&mut self.non_neutral_source_counts, source.clone());
        *self
            .non_neutral_source_buy_sol
            .entry(source.clone())
            .or_default() += buy_sol.max(0.0);
        increment_count(&mut self.raw_source_counts, source.clone());
        self.non_neutral_buyer_weights
            .push((source, buy_sol.max(0.0)));
        self.record_confidence(confidence_bps);
    }

    fn record_neutral(&mut self, source: String, confidence_bps: Option<u16>) {
        self.known_buyers = self.known_buyers.saturating_add(1);
        self.neutral_count = self.neutral_count.saturating_add(1);
        increment_count(&mut self.raw_source_counts, format!("neutral:{source}"));
        self.record_confidence(confidence_bps);
    }

    fn record_unknown(&mut self) {
        self.unknown_count = self.unknown_count.saturating_add(1);
    }

    fn record_confidence(&mut self, confidence_bps: Option<u16>) {
        if let Some(confidence_bps) = confidence_bps {
            self.confidence_sum_bps = self
                .confidence_sum_bps
                .saturating_add(u64::from(confidence_bps));
            self.confidence_min_bps = Some(
                self.confidence_min_bps
                    .map(|existing| existing.min(confidence_bps))
                    .unwrap_or(confidence_bps),
            );
        }
    }
}

fn increment_count(counts: &mut HashMap<String, u64>, key: String) {
    let current = counts.get(&key).copied().unwrap_or_default();
    counts.insert(key, current.saturating_add(1));
}

fn build_fsc_v2_evidence(
    accumulator: &FscV2Accumulator,
    diagnostics: &FundingSourceDiagnostics,
    stream_available: bool,
    saw_transfer: bool,
    config: &FundingSourceConfig,
    max_buy_slot: Option<u64>,
    lane_health: FundingLaneHealth,
    provider: String,
    source_topics: Vec<String>,
) -> FscV2Evidence {
    let total_buyers = accumulator.total_buyers;
    let known_buyers = accumulator.known_buyers;
    let unknown_count = if known_buyers == 0 && accumulator.unknown_count == 0 {
        total_buyers
    } else {
        accumulator.unknown_count
    };
    let known_coverage = ratio(known_buyers, total_buyers);
    let non_neutral_known_coverage = ratio(accumulator.known_non_neutral_buyers, total_buyers);
    let neutral_share = ratio(accumulator.neutral_count, total_buyers);

    let hhi_norm_count = normalized_hhi_from_counts(
        accumulator
            .non_neutral_source_counts
            .values()
            .copied()
            .collect::<Vec<_>>()
            .as_slice(),
    );
    let raw_hhi_including_neutral = normalized_hhi_from_counts(
        accumulator
            .raw_source_counts
            .values()
            .copied()
            .collect::<Vec<_>>()
            .as_slice(),
    );
    let hhi_norm_sol_weighted_excess =
        normalized_sol_weighted_excess(&accumulator.non_neutral_buyer_weights);

    let mut source_counts = accumulator
        .non_neutral_source_counts
        .iter()
        .map(|(source, count)| FundingSourceCount {
            source: FundingSourceKey::new(source.clone()),
            count: saturating_u8(*count),
        })
        .collect::<Vec<_>>();
    source_counts.sort_by(|lhs, rhs| {
        rhs.count
            .cmp(&lhs.count)
            .then_with(|| lhs.source.wallet.cmp(&rhs.source.wallet))
    });

    let top_funder_count = source_counts
        .first()
        .map(|entry| entry.count)
        .unwrap_or_default();
    let top_funder = source_counts.first().map(|entry| entry.source.clone());
    let top_funder_buy_sol = top_funder
        .as_ref()
        .and_then(|source| accumulator.non_neutral_source_buy_sol.get(&source.wallet))
        .copied()
        .unwrap_or_default();
    let top1_share_count = (accumulator.known_non_neutral_buyers > 0)
        .then(|| f64::from(top_funder_count) / accumulator.known_non_neutral_buyers as f64);
    let total_non_neutral_buy_sol = accumulator
        .non_neutral_buyer_weights
        .iter()
        .map(|(_, buy_sol)| *buy_sol)
        .sum::<f64>();
    let top1_share_sol = (total_non_neutral_buy_sol > 0.0)
        .then(|| (top_funder_buy_sol / total_non_neutral_buy_sol).clamp(0.0, 1.0));

    let confidence_denominator = known_buyers.max(1);
    let attribution_confidence_mean = (known_buyers > 0).then(|| {
        (accumulator.confidence_sum_bps as f64 / confidence_denominator as f64) / 10_000.0
    });
    let attribution_confidence_min = accumulator
        .confidence_min_bps
        .map(|confidence_bps| f64::from(confidence_bps) / 10_000.0);

    let low_confidence_count =
        miss_reason_count(diagnostics, FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON);
    let same_slot_unorderable_count =
        miss_reason_count(diagnostics, FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON);

    let (status, excluded_reason) = fsc_v2_status(
        stream_available,
        saw_transfer,
        total_buyers,
        accumulator.known_non_neutral_buyers,
        accumulator.neutral_count,
        known_coverage,
        non_neutral_known_coverage,
        low_confidence_count,
        same_slot_unorderable_count,
        hhi_norm_count,
    );

    FscV2Evidence {
        version: FscVersion::V2,
        attribution_scope: FscAttributionScope::SingleHopNativeSol,
        snapshot_mode: FscSnapshotMode::DecisionTime,
        total_buyers: saturating_u8(total_buyers),
        known_buyers: saturating_u8(known_buyers),
        known_non_neutral_buyers: saturating_u8(accumulator.known_non_neutral_buyers),
        unknown_count: saturating_u8(unknown_count),
        neutral_count: saturating_u8(accumulator.neutral_count),
        low_confidence_count: saturating_u8(low_confidence_count),
        same_slot_unorderable_count: saturating_u16(same_slot_unorderable_count),
        known_coverage,
        non_neutral_known_coverage,
        neutral_share,
        top1_share_count,
        top1_share_sol,
        hhi_norm_count,
        hhi_norm_sol_weighted_excess,
        raw_hhi_including_neutral,
        scoring_hhi_non_neutral: hhi_norm_count,
        top_funder,
        top_funder_count,
        top_funder_buy_sol,
        source_counts,
        attribution_confidence_mean,
        attribution_confidence_min,
        dust_filtered_count: saturating_u16(diagnostics.dust_filtered_count),
        post_buy_filtered_count: saturating_u16(diagnostics.post_buy_filtered_count),
        rel_too_small_count: saturating_u16(diagnostics.rel_too_small_count),
        index_warm: stream_available && saw_transfer,
        capture_ready: stream_available && saw_transfer,
        status,
        excluded_reason,
        funding_lane_watermark_slot: lane_health.funding_lane_watermark_slot,
        max_buy_slot,
        funding_lane_lag_slots: funding_lane_lag_slots(
            lane_health.funding_lane_watermark_slot,
            max_buy_slot,
        ),
        stream_epoch: lane_health.stream_epoch,
        gap_suspected: lane_health.gap_suspected,
        last_transfer_recv_ts_ms: lane_health.last_transfer_recv_ts_ms,
        last_reconnect_ts_ms: lane_health.last_reconnect_ts_ms,
        dropped_events: lane_health.dropped_events,
        min_abs_store_lamports: config.min_abs_store_lamports,
        min_abs_attribution_lamports: config.min_abs_attribution_lamports,
        min_rel_to_buy: config.min_rel_to_buy,
        ttl_seconds: config.lookback_window_ms / 1_000,
        neutral_funder_set_version: config.neutral_funder_set_version.clone(),
        neutral_funder_set_hash: neutral_funder_set_hash(config),
        config_hash: funding_source_config_hash(config),
        provider,
        source_topics,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct FundingLaneHealth {
    funding_lane_watermark_slot: Option<u64>,
    stream_epoch: u64,
    gap_suspected: bool,
    last_transfer_recv_ts_ms: Option<u64>,
    last_reconnect_ts_ms: Option<u64>,
    dropped_events: u64,
}

fn lane_health_locked(inner: &FundingSourceInner) -> FundingLaneHealth {
    FundingLaneHealth {
        funding_lane_watermark_slot: inner.funding_lane_watermark_slot,
        stream_epoch: inner.stream_epoch,
        gap_suspected: inner.gap_suspected,
        last_transfer_recv_ts_ms: inner.last_transfer_recv_ts_ms,
        last_reconnect_ts_ms: inner.last_reconnect_ts_ms,
        dropped_events: inner.dropped_events,
    }
}

fn funding_lane_lag_slots(watermark_slot: Option<u64>, max_buy_slot: Option<u64>) -> Option<i64> {
    Some(watermark_slot? as i64 - max_buy_slot? as i64)
}

fn max_option_u64(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn fsc_v2_source_provenance(inner: &FundingSourceInner) -> (String, Vec<String>) {
    if inner
        .observed_funding_lane_kinds
        .contains("nln_program_streams")
    {
        return (
            FSC_V2_PROVIDER_NLN_PROGRAM_STREAMS.to_string(),
            vec![FSC_V2_TOPIC_NLN_SYSTEM_TRANSFERS.to_string()],
        );
    }
    (
        FSC_V2_PROVIDER_LEGACY_ROLLING_INDEX.to_string(),
        vec![FSC_V2_TOPIC_LEGACY_FUNDING_TRANSFERS.to_string()],
    )
}

fn fsc_v2_status(
    stream_available: bool,
    saw_transfer: bool,
    total_buyers: u64,
    known_non_neutral_buyers: u64,
    neutral_count: u64,
    known_coverage: f64,
    non_neutral_known_coverage: f64,
    low_confidence_count: u64,
    same_slot_unorderable_count: u64,
    hhi_norm_count: Option<f64>,
) -> (FscEvidenceStatus, Option<FscExcludedReason>) {
    if !stream_available {
        return (
            FscEvidenceStatus::Unavailable,
            Some(FscExcludedReason::FundingLaneUnavailable),
        );
    }
    if !saw_transfer {
        return (
            FscEvidenceStatus::Unavailable,
            Some(FscExcludedReason::IndexCold),
        );
    }
    if total_buyers == 0 {
        return (
            FscEvidenceStatus::Unavailable,
            Some(FscExcludedReason::NoBuyerCohort),
        );
    }
    if same_slot_unorderable_count > 0 {
        return (
            FscEvidenceStatus::Degraded,
            Some(FscExcludedReason::SameSlotOrderingUnavailable),
        );
    }
    if low_confidence_count > 0 {
        return (
            FscEvidenceStatus::Degraded,
            Some(FscExcludedReason::LowAttributionConfidence),
        );
    }
    if known_non_neutral_buyers < FSC_V2_MIN_KNOWN_NON_NEUTRAL_BUYERS || hhi_norm_count.is_none() {
        let reason = if known_non_neutral_buyers == 0 && neutral_count > 0 {
            FscExcludedReason::NeutralOnly
        } else {
            FscExcludedReason::InsufficientNonNeutralSupport
        };
        return (FscEvidenceStatus::Degraded, Some(reason));
    }
    if total_buyers < FSC_V2_MIN_TOTAL_BUYERS
        || known_coverage < FSC_V2_MIN_KNOWN_COVERAGE
        || non_neutral_known_coverage < FSC_V2_MIN_NON_NEUTRAL_KNOWN_COVERAGE
    {
        return (
            FscEvidenceStatus::Degraded,
            Some(FscExcludedReason::LowCoverage),
        );
    }

    (FscEvidenceStatus::Clean, None)
}

fn normalized_hhi_from_counts(counts: &[u64]) -> Option<f64> {
    let sample_n = counts.iter().copied().sum::<u64>();
    if sample_n < 2 {
        return None;
    }

    let sample_n_f64 = sample_n as f64;
    let hhi = counts
        .iter()
        .map(|count| {
            let p = *count as f64 / sample_n_f64;
            p * p
        })
        .sum::<f64>();
    let minimum_hhi = 1.0 / sample_n_f64;
    let denominator = 1.0 - minimum_hhi;
    if denominator <= 0.0 {
        return None;
    }
    Some(clamp_unit_epsilon((hhi - minimum_hhi) / denominator))
}

fn normalized_sol_weighted_excess(weights: &[(String, f64)]) -> Option<f64> {
    if weights.len() < 2 {
        return None;
    }
    let total = weights.iter().map(|(_, weight)| *weight).sum::<f64>();
    if total <= 0.0 {
        return None;
    }

    let buyer_weight_hhi = weights
        .iter()
        .map(|(_, weight)| {
            let normalized = *weight / total;
            normalized * normalized
        })
        .sum::<f64>();
    let denominator = 1.0 - buyer_weight_hhi;
    if denominator <= 0.0 {
        return None;
    }

    let mut source_weights = HashMap::<String, f64>::new();
    for (source, weight) in weights {
        *source_weights.entry(source.clone()).or_default() += *weight / total;
    }
    let source_hhi = source_weights
        .values()
        .map(|weight| weight * weight)
        .sum::<f64>();

    Some(clamp_unit_epsilon(
        (source_hhi - buyer_weight_hhi) / denominator,
    ))
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        (numerator as f64 / denominator as f64).clamp(0.0, 1.0)
    }
}

fn clamp_unit_epsilon(value: f64) -> f64 {
    let clamped = value.clamp(0.0, 1.0);
    if clamped <= 1e-12 {
        0.0
    } else if (1.0 - clamped) <= 1e-12 {
        1.0
    } else {
        clamped
    }
}

fn miss_reason_count(diagnostics: &FundingSourceDiagnostics, reason: &str) -> u64 {
    diagnostics
        .miss_reason_counts
        .iter()
        .find(|entry| entry.reason == reason)
        .map(|entry| entry.count)
        .unwrap_or_default()
}

fn saturating_u8(value: u64) -> u8 {
    value.min(u64::from(u8::MAX)) as u8
}

fn saturating_u16(value: u64) -> u16 {
    value.min(u64::from(u16::MAX)) as u16
}

fn neutral_funder_set_hash(config: &FundingSourceConfig) -> Option<String> {
    if config.neutral_funding_sources.is_empty() {
        return None;
    }
    let mut sources = config
        .neutral_funding_sources
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    sources.sort();
    Some(stable_fnv64_hex(sources.join("\n").as_bytes()))
}

fn funding_source_config_hash(config: &FundingSourceConfig) -> String {
    let mut neutral_sources = config
        .neutral_funding_sources
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    neutral_sources.sort();
    stable_fnv64_hex(
        format!(
            "lookback_window_ms={};min_abs_store_lamports={};min_abs_attribution_lamports={};min_rel_to_buy_bits={};min_attribution_confidence_bps={};per_recipient_cap={};global_recipient_cap={};neutral_funder_set_version={};neutral_sources={}",
            config.lookback_window_ms,
            config.min_abs_store_lamports,
            config.min_abs_attribution_lamports,
            config.min_rel_to_buy.to_bits(),
            config.min_attribution_confidence_bps,
            config.per_recipient_cap,
            config.global_recipient_cap,
            config.neutral_funder_set_version.as_deref().unwrap_or(""),
            neutral_sources.join(",")
        )
        .as_bytes(),
    )
}

fn stable_fnv64_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv64:{hash:016x}")
}

impl FundingSourceIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_stream_available(&self, available: bool) {
        let now_ms = wall_clock_epoch_ms();
        let mut inner = self.inner.write();
        inner.availability_controlled = true;
        if available {
            if !inner.stream_available || inner.stream_available_since_ms.is_none() {
                inner.stream_available_since_ms.get_or_insert(now_ms);
            }
        } else {
            inner.stream_available_since_ms = None;
        }
        inner.stream_available = available;
        update_index_metrics(&inner);
    }

    #[must_use]
    pub fn stream_available(&self) -> bool {
        self.inner.read().stream_available
    }

    #[must_use]
    pub fn warmup_ready(&self) -> bool {
        let inner = self.inner.read();
        inner.stream_available && inner.saw_transfer
    }

    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.inner.read().histories.len()
    }

    #[must_use]
    pub fn coverage_window_status(
        &self,
        config: &FundingSourceConfig,
        now_ms: u64,
    ) -> FundingCoverageWindowStatus {
        let inner = self.inner.read();
        coverage_window_status_locked(&inner, config, now_ms)
    }

    pub fn observe_transfer(
        &self,
        transfer: &FundingTransferObserved,
        config: &FundingSourceConfig,
    ) {
        if transfer.lamports < config.min_abs_store_lamports {
            return;
        }

        let observed_at_ms = funding_transfer_event_ts_ms(transfer);
        if observed_at_ms == 0
            || transfer.source_wallet.is_empty()
            || transfer.recipient_wallet.is_empty()
            || transfer.source_wallet == transfer.recipient_wallet
        {
            return;
        }

        let window_start = observed_at_ms.saturating_sub(config.lookback_window_ms);
        let recipient_wallet = transfer.recipient_wallet.clone();
        let observation_wall_ms = wall_clock_epoch_ms();

        let prune_started_at = Instant::now();
        let mut inner = self.inner.write();
        // Any accepted funding transfer warms the rolling index for capture/evidence.
        // Full-chain transfers may additionally mark availability automatically.
        inner.saw_transfer = true;
        inner.funding_lane_watermark_slot =
            max_option_u64(inner.funding_lane_watermark_slot, transfer.slot);
        inner.last_transfer_recv_ts_ms = Some(observation_wall_ms);
        inner
            .observed_funding_lane_kinds
            .insert(transfer.provenance.lane_kind.as_str().to_string());
        if transfer.full_chain_coverage {
            if !inner.availability_controlled {
                inner.stream_available = true;
                inner
                    .stream_available_since_ms
                    .get_or_insert(observation_wall_ms);
            }
        }

        let mut tracked_last_seen = None;
        let mut per_recipient_overflows = 0u64;
        {
            inner.evicted_recipients.remove(&recipient_wallet);
            let history = inner.histories.entry(recipient_wallet.clone()).or_default();
            prune_transfer_history(&mut history.transfers, window_start);
            if history.transfers.is_empty() {
                history.overflowed_before_oldest_retained = false;
            }

            let duplicate = history.transfers.back().is_some_and(|last| {
                last.signature == transfer.signature
                    && last.slot == transfer.slot
                    && last.source_wallet == transfer.source_wallet
                    && last.lamports == transfer.lamports
                    && last.observed_at_ms == observed_at_ms
                    && last.arrival_ts_ms == transfer.arrival_ts_ms
                    && last.event_ordinal == transfer.event_ordinal
                    && last.tx_index == transfer.tx_index
                    && last.outer_instruction_index == transfer.outer_instruction_index
                    && last.inner_group_index == transfer.inner_group_index
                    && last.cpi_stack_height == transfer.cpi_stack_height
            });
            if !duplicate {
                history.transfers.push_back(FundingTransferRecord {
                    slot: transfer.slot,
                    source_wallet: transfer.source_wallet.clone(),
                    signature: transfer.signature.clone(),
                    lamports: transfer.lamports,
                    observed_at_ms,
                    arrival_ts_ms: transfer.arrival_ts_ms,
                    event_ordinal: transfer.event_ordinal,
                    tx_index: transfer.tx_index,
                    outer_instruction_index: transfer.outer_instruction_index,
                    inner_group_index: transfer.inner_group_index,
                    cpi_stack_height: transfer.cpi_stack_height,
                });
                while history.transfers.len() > config.per_recipient_cap {
                    history.transfers.pop_front();
                    per_recipient_overflows = per_recipient_overflows.saturating_add(1);
                    history.overflowed_before_oldest_retained = true;
                }
            }

            let previous_last_seen = history.last_seen_ms;
            history.last_seen_ms = history.last_seen_ms.max(observed_at_ms);
            if history.last_seen_ms > previous_last_seen || previous_last_seen == 0 {
                tracked_last_seen = Some(history.last_seen_ms);
            }
        }

        if let Some(last_seen_ms) = tracked_last_seen {
            inner
                .recipient_order
                .push_back((last_seen_ms, recipient_wallet));
        }

        let global_evictions =
            prune_global_locked(&mut inner, window_start, config.global_recipient_cap);
        record_fsc_prune_duration_ms(prune_started_at.elapsed().as_secs_f64() * 1_000.0);
        if per_recipient_overflows > 0 {
            record_fsc_index_per_recipient_overflows(per_recipient_overflows);
        }
        if global_evictions > 0 {
            record_fsc_index_global_evictions(global_evictions);
        }
        update_index_metrics(&inner);
    }

    pub fn record_stream_reconnect(&self, reconnect_ts_ms: u64) {
        let mut inner = self.inner.write();
        inner.stream_epoch = inner.stream_epoch.saturating_add(1);
        inner.last_reconnect_ts_ms = Some(reconnect_ts_ms);
        inner.gap_suspected = true;
        update_index_metrics(&inner);
    }

    pub fn observe_lane_health(&self, health: seer::ipc::FundingLaneRuntimeHealth) {
        if health.is_default() {
            return;
        }
        let mut inner = self.inner.write();
        if health.stream_epoch > inner.stream_epoch {
            inner.stream_epoch = health.stream_epoch;
            inner.last_reconnect_ts_ms = health.last_reconnect_ts_ms;
        } else if health.last_reconnect_ts_ms.is_some() {
            inner.last_reconnect_ts_ms =
                max_option_u64(inner.last_reconnect_ts_ms, health.last_reconnect_ts_ms);
        }
        inner.gap_suspected |= health.gap_suspected;
        inner.dropped_events = inner.dropped_events.max(health.dropped_events);
        update_index_metrics(&inner);
    }

    pub fn record_dropped_events(&self, count: u64) {
        if count == 0 {
            return;
        }
        let mut inner = self.inner.write();
        inner.dropped_events = inner.dropped_events.saturating_add(count);
        inner.gap_suspected = true;
    }

    #[must_use]
    pub fn compute_for_transactions<'a>(
        &self,
        transactions: impl IntoIterator<Item = &'a PoolTransaction>,
        config: &FundingSourceConfig,
    ) -> FscComputation {
        let buyer_samples = unique_successful_buyers(transactions);
        let mut diagnostics = FundingSourceDiagnostics {
            buyer_sample_count: buyer_samples.len() as u64,
            ..FundingSourceDiagnostics::default()
        };
        let max_buy_slot = buyer_samples.iter().filter_map(|tx| tx.slot).max();
        let mut fsc_v2_accumulator = FscV2Accumulator::new(buyer_samples.len());

        let earliest_buy_ts_ms = buyer_samples
            .iter()
            .map(|tx| tx_event_ts_ms(tx))
            .filter(|ts| *ts > 0)
            .min()
            .unwrap_or_default();
        let window_start = earliest_buy_ts_ms.saturating_sub(config.lookback_window_ms);

        let mut inner = self.inner.write();

        if !inner.stream_available {
            let (provider, source_topics) = fsc_v2_source_provenance(&inner);
            let funding_source_v2 = build_fsc_v2_evidence(
                &fsc_v2_accumulator,
                &diagnostics,
                false,
                inner.saw_transfer,
                config,
                max_buy_slot,
                lane_health_locked(&inner),
                provider,
                source_topics,
            );
            return FscComputation {
                funding_source_concentration: None,
                funding_source_v2,
                degraded_reasons: vec![FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()],
                diagnostics,
            };
        }

        if !inner.saw_transfer {
            let (provider, source_topics) = fsc_v2_source_provenance(&inner);
            let funding_source_v2 = build_fsc_v2_evidence(
                &fsc_v2_accumulator,
                &diagnostics,
                inner.stream_available,
                false,
                config,
                max_buy_slot,
                lane_health_locked(&inner),
                provider,
                source_topics,
            );
            return FscComputation {
                funding_source_concentration: None,
                funding_source_v2,
                degraded_reasons: vec![FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()],
                diagnostics,
            };
        }

        let mut known_sources = Vec::<String>::new();
        let mut lookup_hits = 0u64;
        let mut lookup_misses = 0u64;
        let mut removed_entries = 0u64;

        for tx in buyer_samples {
            let lookup = lookup_source_for_buy(&mut inner, tx, config);
            diagnostics.dust_filtered_count = diagnostics
                .dust_filtered_count
                .saturating_add(lookup.dust_filtered_count);
            diagnostics.post_buy_filtered_count = diagnostics
                .post_buy_filtered_count
                .saturating_add(lookup.post_buy_filtered_count);
            diagnostics.rel_too_small_count = diagnostics
                .rel_too_small_count
                .saturating_add(lookup.rel_too_small_count);
            let matched = lookup.matched;
            if lookup.removed {
                removed_entries = removed_entries.saturating_add(1);
            }
            match matched {
                FundingSourceMatch::Concrete(source) => {
                    lookup_hits = lookup_hits.saturating_add(1);
                    diagnostics.known_source_count =
                        diagnostics.known_source_count.saturating_add(1);
                    known_sources.push(source.clone());
                    fsc_v2_accumulator.record_concrete(
                        source,
                        tx_buy_sol(tx),
                        lookup.attribution_confidence_bps,
                    );
                }
                FundingSourceMatch::Neutral {
                    source_wallet,
                    legacy_key,
                } => {
                    lookup_hits = lookup_hits.saturating_add(1);
                    diagnostics.known_source_count =
                        diagnostics.known_source_count.saturating_add(1);
                    known_sources.push(legacy_key);
                    fsc_v2_accumulator
                        .record_neutral(source_wallet, lookup.attribution_confidence_bps);
                }
                FundingSourceMatch::Unknown => {
                    lookup_misses = lookup_misses.saturating_add(1);
                    fsc_v2_accumulator.record_unknown();
                    if let Some(miss) = lookup.miss {
                        record_lookup_miss(&mut diagnostics, miss);
                    }
                }
            }
        }

        let prune_started_at = Instant::now();
        let global_evictions =
            prune_global_locked(&mut inner, window_start, config.global_recipient_cap);
        record_fsc_prune_duration_ms(prune_started_at.elapsed().as_secs_f64() * 1_000.0);
        if global_evictions > 0 {
            record_fsc_index_global_evictions(global_evictions);
        }
        if removed_entries > 0 {
            record_fsc_index_global_evictions(removed_entries);
        }
        update_index_metrics(&inner);
        if lookup_hits > 0 {
            record_fsc_lookup_hits(lookup_hits);
        }
        if lookup_misses > 0 {
            record_fsc_lookup_misses(lookup_misses);
        }
        sort_lookup_miss_counts(&mut diagnostics);
        let (provider, source_topics) = fsc_v2_source_provenance(&inner);
        let funding_source_v2 = build_fsc_v2_evidence(
            &fsc_v2_accumulator,
            &diagnostics,
            inner.stream_available,
            inner.saw_transfer,
            config,
            max_buy_slot,
            lane_health_locked(&inner),
            provider,
            source_topics,
        );

        if known_sources.len() < 2 {
            return FscComputation {
                funding_source_concentration: None,
                funding_source_v2,
                degraded_reasons: vec![FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()],
                diagnostics,
            };
        }

        let distinct_known_sources = known_sources.iter().collect::<HashSet<_>>().len();
        FscComputation {
            funding_source_concentration: Some(
                1.0 - (distinct_known_sources as f64 / known_sources.len() as f64),
            ),
            funding_source_v2,
            degraded_reasons: Vec::new(),
            diagnostics,
        }
    }
}

fn update_index_metrics(inner: &FundingSourceInner) {
    record_fsc_index_entries(inner.histories.len());
    record_fsc_authoritative_funding_stream_available(inner.stream_available);
    record_fsc_warmup_ready(inner.stream_available && inner.saw_transfer);
}

fn coverage_window_status_locked(
    inner: &FundingSourceInner,
    config: &FundingSourceConfig,
    now_ms: u64,
) -> FundingCoverageWindowStatus {
    let warmup_ready = inner.stream_available && inner.saw_transfer;
    let elapsed_ms = inner
        .stream_available_since_ms
        .map(|since_ms| now_ms.saturating_sub(since_ms))
        .unwrap_or_default();
    let coverage_window_ready = inner.stream_available && elapsed_ms >= config.lookback_window_ms;
    let coverage_window_remaining_ms = if coverage_window_ready {
        0
    } else if inner.stream_available {
        config.lookback_window_ms.saturating_sub(elapsed_ms)
    } else {
        config.lookback_window_ms
    };

    FundingCoverageWindowStatus {
        stream_available: inner.stream_available,
        warmup_ready,
        coverage_window_ready,
        authoritative_buy_ready: warmup_ready && coverage_window_ready,
        coverage_window_remaining_ms,
    }
}

fn choose_lookup_miss(current: Option<LookupMiss>, candidate: LookupMiss) -> LookupMiss {
    current
        .into_iter()
        .chain(std::iter::once(candidate))
        .max_by_key(|miss| lookup_miss_rank(*miss))
        .expect("candidate miss should always exist")
}

fn lookup_miss_rank(miss: LookupMiss) -> (u8, u8) {
    let class_rank = match miss.class {
        FscMissClass::Operational => 3,
        FscMissClass::Indeterminate => 2,
        FscMissClass::Structural => 1,
    };
    let reason_rank = match miss.reason {
        FSC_BUYER_IDENTITY_UNAVAILABLE_REASON => 4,
        FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON => 3,
        FSC_GLOBAL_RECIPIENT_EVICTED_REASON => 2,
        FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON => 1,
        FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON => 1,
        FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON => 1,
        FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON => 1,
        FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON => 1,
        FSC_RELATIVE_FUNDING_TOO_SMALL_REASON => 1,
        FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON => 1,
        FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON => 0,
        _ => 0,
    };
    (class_rank, reason_rank)
}

fn record_lookup_miss(diagnostics: &mut FundingSourceDiagnostics, miss: LookupMiss) {
    diagnostics.unknown_buyer_count = diagnostics.unknown_buyer_count.saturating_add(1);
    match miss.class {
        FscMissClass::Structural => {
            diagnostics.structural_unknown_buyer_count =
                diagnostics.structural_unknown_buyer_count.saturating_add(1);
        }
        FscMissClass::Operational => {
            diagnostics.operational_unknown_buyer_count = diagnostics
                .operational_unknown_buyer_count
                .saturating_add(1);
        }
        FscMissClass::Indeterminate => {
            diagnostics.indeterminate_unknown_buyer_count = diagnostics
                .indeterminate_unknown_buyer_count
                .saturating_add(1);
        }
    }
    if let Some(existing) = diagnostics
        .miss_reason_counts
        .iter_mut()
        .find(|entry| entry.reason == miss.reason)
    {
        existing.count = existing.count.saturating_add(1);
    } else {
        diagnostics
            .miss_reason_counts
            .push(FundingSourceMissReasonCount {
                reason: miss.reason.to_string(),
                class: miss.class,
                count: 1,
            });
    }
    record_fsc_lookup_miss_reason(miss.reason, miss.class, 1);
}

fn sort_lookup_miss_counts(diagnostics: &mut FundingSourceDiagnostics) {
    diagnostics.miss_reason_counts.sort_by(|lhs, rhs| {
        lhs.class
            .as_str()
            .cmp(rhs.class.as_str())
            .then_with(|| lhs.reason.cmp(&rhs.reason))
    });
}

fn wall_clock_epoch_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.min(u128::from(u64::MAX)) as u64
}

fn prune_transfer_history(transfers: &mut VecDeque<FundingTransferRecord>, window_start: u64) {
    while transfers
        .front()
        .is_some_and(|transfer| transfer.observed_at_ms < window_start)
    {
        transfers.pop_front();
    }
}

fn prune_evicted_recipients_locked(inner: &mut FundingSourceInner, window_start: u64) {
    while let Some((tracked_last_seen, recipient)) = inner.evicted_recipient_order.front().cloned()
    {
        if tracked_last_seen >= window_start {
            break;
        }

        inner.evicted_recipient_order.pop_front();
        let should_remove = inner
            .evicted_recipients
            .get(&recipient)
            .is_some_and(|history| history.last_seen_ms == tracked_last_seen);
        if should_remove {
            inner.evicted_recipients.remove(&recipient);
        }
    }
}

fn prune_global_locked(
    inner: &mut FundingSourceInner,
    window_start: u64,
    global_recipient_cap: usize,
) -> u64 {
    let mut evictions = 0u64;
    prune_evicted_recipients_locked(inner, window_start);
    while let Some((tracked_last_seen, recipient)) = inner.recipient_order.front().cloned() {
        let should_prune_for_window = tracked_last_seen < window_start;
        let should_prune_for_cap = inner.histories.len() > global_recipient_cap;
        if !should_prune_for_window && !should_prune_for_cap {
            break;
        }

        inner.recipient_order.pop_front();
        let should_remove = inner
            .histories
            .get(&recipient)
            .is_some_and(|history| history.last_seen_ms == tracked_last_seen);
        if should_remove {
            if let Some(history) = inner.histories.remove(&recipient) {
                if should_prune_for_cap && !should_prune_for_window {
                    inner.evicted_recipients.insert(
                        recipient.clone(),
                        EvictedRecipientHistory {
                            last_seen_ms: history.last_seen_ms,
                        },
                    );
                    inner
                        .evicted_recipient_order
                        .push_back((history.last_seen_ms, recipient.clone()));
                }
            }
            evictions = evictions.saturating_add(1);
        }
    }
    evictions
}

fn lookup_source_for_buy(
    inner: &mut FundingSourceInner,
    tx: &PoolTransaction,
    config: &FundingSourceConfig,
) -> LookupSourceResult {
    let lookup_wallets = funding_lookup_wallets(tx);
    if lookup_wallets.is_empty() {
        return LookupSourceResult {
            matched: FundingSourceMatch::Unknown,
            removed: false,
            miss: Some(LookupMiss {
                reason: FSC_BUYER_IDENTITY_UNAVAILABLE_REASON,
                class: FscMissClass::Operational,
            }),
            attribution_confidence_bps: None,
            selected_lamports: 0,
            total_lamports: 0,
            dust_filtered_count: 0,
            post_buy_filtered_count: 0,
            rel_too_small_count: 0,
        };
    }

    let buy_event_ts_ms = tx_event_ts_ms(tx);
    if buy_event_ts_ms == 0 {
        return LookupSourceResult {
            matched: FundingSourceMatch::Unknown,
            removed: false,
            miss: Some(LookupMiss {
                reason: FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON,
                class: FscMissClass::Operational,
            }),
            attribution_confidence_bps: None,
            selected_lamports: 0,
            total_lamports: 0,
            dust_filtered_count: 0,
            post_buy_filtered_count: 0,
            rel_too_small_count: 0,
        };
    }
    let buy_window_start = buy_event_ts_ms.saturating_sub(config.lookback_window_ms);

    let mut lookup_miss = None::<LookupMiss>;
    let mut removed = false;
    let mut counters = LookupCounters::default();
    for wallet in lookup_wallets {
        match lookup_source_for_wallet(
            inner,
            wallet.as_str(),
            tx,
            config,
            buy_event_ts_ms,
            buy_window_start,
        ) {
            WalletLookupOutcome::Matched {
                matched,
                removed: wallet_removed,
                attribution_confidence_bps,
                selected_lamports,
                total_lamports,
                dust_filtered_count,
                post_buy_filtered_count,
                rel_too_small_count,
            } => {
                counters.merge(LookupCounters {
                    dust_filtered_count,
                    post_buy_filtered_count,
                    rel_too_small_count,
                });
                if wallet_removed {
                    inner.histories.remove(wallet.as_str());
                }
                removed |= wallet_removed;
                return LookupSourceResult {
                    matched,
                    removed,
                    miss: None,
                    attribution_confidence_bps: Some(attribution_confidence_bps),
                    selected_lamports,
                    total_lamports,
                    dust_filtered_count: counters.dust_filtered_count,
                    post_buy_filtered_count: counters.post_buy_filtered_count,
                    rel_too_small_count: counters.rel_too_small_count,
                };
            }
            WalletLookupOutcome::ContinueMiss {
                miss,
                removed: wallet_removed,
                dust_filtered_count,
                post_buy_filtered_count,
                rel_too_small_count,
            } => {
                counters.merge(LookupCounters {
                    dust_filtered_count,
                    post_buy_filtered_count,
                    rel_too_small_count,
                });
                if wallet_removed {
                    inner.histories.remove(wallet.as_str());
                }
                removed |= wallet_removed;
                lookup_miss = Some(choose_lookup_miss(lookup_miss, miss));
            }
            WalletLookupOutcome::TerminalMiss {
                miss,
                removed: wallet_removed,
                dust_filtered_count,
                post_buy_filtered_count,
                rel_too_small_count,
            } => {
                counters.merge(LookupCounters {
                    dust_filtered_count,
                    post_buy_filtered_count,
                    rel_too_small_count,
                });
                if wallet_removed {
                    inner.histories.remove(wallet.as_str());
                }
                removed |= wallet_removed;
                return LookupSourceResult {
                    matched: FundingSourceMatch::Unknown,
                    removed,
                    miss: Some(choose_lookup_miss(lookup_miss, miss)),
                    attribution_confidence_bps: None,
                    selected_lamports: 0,
                    total_lamports: 0,
                    dust_filtered_count: counters.dust_filtered_count,
                    post_buy_filtered_count: counters.post_buy_filtered_count,
                    rel_too_small_count: counters.rel_too_small_count,
                };
            }
        }
    }

    LookupSourceResult {
        matched: FundingSourceMatch::Unknown,
        removed,
        miss: lookup_miss,
        attribution_confidence_bps: None,
        selected_lamports: 0,
        total_lamports: 0,
        dust_filtered_count: counters.dust_filtered_count,
        post_buy_filtered_count: counters.post_buy_filtered_count,
        rel_too_small_count: counters.rel_too_small_count,
    }
}

fn lookup_source_for_wallet(
    inner: &mut FundingSourceInner,
    wallet: &str,
    tx: &PoolTransaction,
    config: &FundingSourceConfig,
    buy_event_ts_ms: u64,
    buy_window_start: u64,
) -> WalletLookupOutcome {
    if let Some(history) = inner.histories.get_mut(wallet) {
        prune_transfer_history(&mut history.transfers, buy_window_start);
        if history.transfers.is_empty() {
            return WalletLookupOutcome::ContinueMiss {
                miss: LookupMiss {
                    reason: FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON,
                    class: FscMissClass::Structural,
                },
                removed: true,
                dust_filtered_count: 0,
                post_buy_filtered_count: 0,
                rel_too_small_count: 0,
            };
        }

        let mut source_accumulators = HashMap::<String, SourceAccumulator>::new();
        let mut total_candidate_lamports = 0u128;
        let mut wallet_candidate_count = 0u64;
        let mut counters = LookupCounters::default();
        let mut saw_unorderable_prebuy_candidate = false;
        let buy_amount_lamports = tx_buy_amount_lamports(tx);

        for transfer in &history.transfers {
            match transfer_buy_order(transfer, tx, buy_event_ts_ms) {
                TransferBuyOrder::Precedes => {
                    if transfer.lamports < config.min_abs_attribution_lamports {
                        counters.dust_filtered_count =
                            counters.dust_filtered_count.saturating_add(1);
                        continue;
                    }
                    if let Some(min_rel_lamports) =
                        min_relative_attribution_lamports(config, buy_amount_lamports)
                    {
                        if transfer.lamports < min_rel_lamports {
                            counters.rel_too_small_count =
                                counters.rel_too_small_count.saturating_add(1);
                            continue;
                        }
                    } else if config.min_rel_to_buy > 0.0 {
                        return WalletLookupOutcome::TerminalMiss {
                            miss: LookupMiss {
                                reason: FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON,
                                class: FscMissClass::Indeterminate,
                            },
                            removed: false,
                            dust_filtered_count: counters.dust_filtered_count,
                            post_buy_filtered_count: counters.post_buy_filtered_count,
                            rel_too_small_count: counters.rel_too_small_count,
                        };
                    }
                    wallet_candidate_count = wallet_candidate_count.saturating_add(1);
                    let transfer_lamports = u128::from(transfer.lamports);
                    total_candidate_lamports =
                        total_candidate_lamports.saturating_add(transfer_lamports);
                    let tie_key = transfer_tie_break_key(transfer);
                    source_accumulators
                        .entry(transfer.source_wallet.clone())
                        .and_modify(|source| {
                            source.total_lamports =
                                source.total_lamports.saturating_add(transfer_lamports);
                            if tie_key > source.latest_transfer_key {
                                source.latest_transfer_key = tie_key.clone();
                                source.recipient_wallet = wallet.to_string();
                            }
                        })
                        .or_insert_with(|| SourceAccumulator {
                            recipient_wallet: wallet.to_string(),
                            source_wallet: transfer.source_wallet.clone(),
                            total_lamports: transfer_lamports,
                            latest_transfer_key: tie_key,
                        });
                }
                TransferBuyOrder::DoesNotPrecede => {
                    counters.post_buy_filtered_count =
                        counters.post_buy_filtered_count.saturating_add(1);
                }
                TransferBuyOrder::Unorderable => {
                    saw_unorderable_prebuy_candidate = true;
                }
            }
        }

        let Some(selection) = select_dominant_source(source_accumulators, total_candidate_lamports)
        else {
            if saw_unorderable_prebuy_candidate {
                return WalletLookupOutcome::TerminalMiss {
                    miss: LookupMiss {
                        reason: FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON,
                        class: FscMissClass::Indeterminate,
                    },
                    removed: false,
                    dust_filtered_count: counters.dust_filtered_count,
                    post_buy_filtered_count: counters.post_buy_filtered_count,
                    rel_too_small_count: counters.rel_too_small_count,
                };
            }

            let miss = if counters.rel_too_small_count > 0 {
                LookupMiss {
                    reason: FSC_RELATIVE_FUNDING_TOO_SMALL_REASON,
                    class: FscMissClass::Structural,
                }
            } else if counters.dust_filtered_count > 0 {
                LookupMiss {
                    reason: FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON,
                    class: FscMissClass::Structural,
                }
            } else if history.overflowed_before_oldest_retained {
                LookupMiss {
                    reason: FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON,
                    class: FscMissClass::Operational,
                }
            } else {
                LookupMiss {
                    reason: FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON,
                    class: FscMissClass::Structural,
                }
            };
            return WalletLookupOutcome::ContinueMiss {
                miss,
                removed: false,
                dust_filtered_count: counters.dust_filtered_count,
                post_buy_filtered_count: counters.post_buy_filtered_count,
                rel_too_small_count: counters.rel_too_small_count,
            };
        };

        debug_assert!(wallet_candidate_count > 0);
        if !attribution_confidence_passes(
            selection.selected_lamports,
            selection.total_lamports,
            config.min_attribution_confidence_bps,
        ) {
            return WalletLookupOutcome::TerminalMiss {
                miss: LookupMiss {
                    reason: FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON,
                    class: FscMissClass::Indeterminate,
                },
                removed: false,
                dust_filtered_count: counters.dust_filtered_count,
                post_buy_filtered_count: counters.post_buy_filtered_count,
                rel_too_small_count: counters.rel_too_small_count,
            };
        }

        let attribution_confidence_bps =
            attribution_confidence_bps(selection.selected_lamports, selection.total_lamports);
        let matched = if config.is_neutral_source(&selection.source_wallet) {
            FundingSourceMatch::Neutral {
                source_wallet: selection.source_wallet.clone(),
                legacy_key: format!("neutral:{}", selection.recipient_wallet),
            }
        } else {
            FundingSourceMatch::Concrete(selection.source_wallet.clone())
        };
        return WalletLookupOutcome::Matched {
            matched,
            removed: false,
            attribution_confidence_bps,
            selected_lamports: selection.selected_lamports,
            total_lamports: selection.total_lamports,
            dust_filtered_count: counters.dust_filtered_count,
            post_buy_filtered_count: counters.post_buy_filtered_count,
            rel_too_small_count: counters.rel_too_small_count,
        };
    }

    if inner.evicted_recipients.contains_key(wallet) {
        return WalletLookupOutcome::ContinueMiss {
            miss: LookupMiss {
                reason: FSC_GLOBAL_RECIPIENT_EVICTED_REASON,
                class: FscMissClass::Operational,
            },
            removed: false,
            dust_filtered_count: 0,
            post_buy_filtered_count: 0,
            rel_too_small_count: 0,
        };
    }

    WalletLookupOutcome::ContinueMiss {
        miss: LookupMiss {
            reason: FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON,
            class: FscMissClass::Indeterminate,
        },
        removed: false,
        dust_filtered_count: 0,
        post_buy_filtered_count: 0,
        rel_too_small_count: 0,
    }
}

fn select_dominant_source(
    source_accumulators: HashMap<String, SourceAccumulator>,
    total_lamports: u128,
) -> Option<FundingAttributionSelection> {
    source_accumulators
        .into_values()
        .max_by(|lhs, rhs| {
            lhs.total_lamports
                .cmp(&rhs.total_lamports)
                .then_with(|| lhs.latest_transfer_key.cmp(&rhs.latest_transfer_key))
                .then_with(|| lhs.source_wallet.cmp(&rhs.source_wallet))
        })
        .map(|selected| FundingAttributionSelection {
            recipient_wallet: selected.recipient_wallet,
            source_wallet: selected.source_wallet,
            selected_lamports: selected.total_lamports,
            total_lamports,
        })
}

fn attribution_confidence_passes(
    selected_lamports: u128,
    total_lamports: u128,
    min_confidence_bps: u16,
) -> bool {
    if total_lamports == 0 {
        return false;
    }
    selected_lamports.saturating_mul(10_000)
        >= total_lamports.saturating_mul(u128::from(min_confidence_bps))
}

fn attribution_confidence_bps(selected_lamports: u128, total_lamports: u128) -> u16 {
    if total_lamports == 0 {
        return 0;
    }
    selected_lamports
        .saturating_mul(10_000)
        .checked_div(total_lamports)
        .unwrap_or_default()
        .min(u128::from(u16::MAX)) as u16
}

fn tx_buy_amount_lamports(tx: &PoolTransaction) -> Option<u64> {
    tx.sol_amount_lamports.or_else(|| {
        (tx.volume_sol.is_finite() && tx.volume_sol > 0.0)
            .then(|| (tx.volume_sol * 1_000_000_000.0).round() as u64)
            .filter(|value| *value > 0)
    })
}

fn min_relative_attribution_lamports(
    config: &FundingSourceConfig,
    buy_amount_lamports: Option<u64>,
) -> Option<u64> {
    if config.min_rel_to_buy <= 0.0 {
        return Some(0);
    }
    let buy_amount_lamports = buy_amount_lamports?;
    Some((buy_amount_lamports as f64 * config.min_rel_to_buy).ceil() as u64)
}

fn transfer_buy_order(
    transfer: &FundingTransferRecord,
    buy: &PoolTransaction,
    buy_event_ts_ms: u64,
) -> TransferBuyOrder {
    if transfer.signature == buy.signature {
        if let Some(precedes) = same_signature_transfer_precedes_buy(transfer, buy) {
            return if precedes {
                TransferBuyOrder::Precedes
            } else {
                TransferBuyOrder::DoesNotPrecede
            };
        }
    }

    if let (Some(transfer_slot), Some(buy_slot)) = (transfer.slot, buy.slot) {
        if transfer_slot != buy_slot {
            return if transfer_slot < buy_slot {
                TransferBuyOrder::Precedes
            } else {
                TransferBuyOrder::DoesNotPrecede
            };
        }

        if transfer.signature == buy.signature {
            return TransferBuyOrder::Unorderable;
        }

        return match (transfer.tx_index, buy.tx_index) {
            (Some(transfer_tx_index), Some(buy_tx_index)) if transfer_tx_index < buy_tx_index => {
                TransferBuyOrder::Precedes
            }
            (Some(transfer_tx_index), Some(buy_tx_index)) if transfer_tx_index > buy_tx_index => {
                TransferBuyOrder::DoesNotPrecede
            }
            _ => TransferBuyOrder::Unorderable,
        };
    }

    if transfer.observed_at_ms < buy_event_ts_ms {
        TransferBuyOrder::Precedes
    } else if transfer.observed_at_ms > buy_event_ts_ms {
        TransferBuyOrder::DoesNotPrecede
    } else {
        match (transfer.tx_index, buy.tx_index) {
            (Some(transfer_tx_index), Some(buy_tx_index)) if transfer_tx_index < buy_tx_index => {
                TransferBuyOrder::Precedes
            }
            (Some(transfer_tx_index), Some(buy_tx_index)) if transfer_tx_index > buy_tx_index => {
                TransferBuyOrder::DoesNotPrecede
            }
            _ => TransferBuyOrder::Unorderable,
        }
    }
}

fn same_signature_transfer_precedes_buy(
    transfer: &FundingTransferRecord,
    buy: &PoolTransaction,
) -> Option<bool> {
    if let (Some(transfer_outer), Some(buy_outer)) = (
        transfer.outer_instruction_index,
        buy.outer_instruction_index,
    ) {
        if transfer_outer != buy_outer {
            return Some(transfer_outer < buy_outer);
        }

        let transfer_is_inner = transfer.inner_group_index.is_some();
        let buy_is_inner = buy.inner_group_index.is_some();
        if transfer_is_inner != buy_is_inner {
            return Some(!transfer_is_inner && buy_is_inner);
        }
    }

    if let (Some(transfer_ordinal), Some(buy_ordinal)) = (transfer.event_ordinal, buy.event_ordinal)
    {
        if transfer_ordinal != buy_ordinal {
            return Some(transfer_ordinal < buy_ordinal);
        }
    }

    if let (Some(transfer_stack_height), Some(buy_stack_height)) =
        (transfer.cpi_stack_height, buy.cpi_stack_height)
    {
        if transfer_stack_height != buy_stack_height {
            return Some(transfer_stack_height < buy_stack_height);
        }
    }

    None
}

fn unique_successful_buyers<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> Vec<&'a PoolTransaction> {
    let mut by_identity = HashMap::<String, &'a PoolTransaction>::new();
    let mut unresolved_buyers = Vec::new();
    for tx in transactions {
        if !tx.is_buy || !tx.success {
            continue;
        }
        if let Some(buyer_identity) = canonical_buyer_identity(tx) {
            by_identity
                .entry(buyer_identity)
                .and_modify(|existing| {
                    if buy_order_key(tx) < buy_order_key(existing) {
                        *existing = tx;
                    }
                })
                .or_insert(tx);
            continue;
        }
        unresolved_buyers.push(tx);
    }

    let mut buyers = by_identity.into_values().collect::<Vec<_>>();
    buyers.extend(unresolved_buyers);
    buyers.sort_by_key(|tx| buy_order_key(tx));
    buyers
}

fn buy_order_key(tx: &PoolTransaction) -> BuyOrderKey {
    BuyOrderKey {
        slot: tx.slot.unwrap_or(u64::MAX),
        tx_index: tx.tx_index.unwrap_or(u32::MAX),
        event_ordinal: tx.event_ordinal.unwrap_or(u32::MAX),
        event_ts_ms: tx_event_ts_ms(tx),
        arrival_ts_ms: tx.arrival_ts_ms,
        signature: tx.signature.clone(),
    }
}

fn transfer_tie_break_key(transfer: &FundingTransferRecord) -> TransferTieBreakKey {
    TransferTieBreakKey {
        slot: transfer.slot.unwrap_or_default(),
        tx_index: transfer.tx_index.unwrap_or_default(),
        event_ordinal: transfer.event_ordinal.unwrap_or_default(),
        observed_at_ms: transfer.observed_at_ms,
        arrival_ts_ms: transfer.arrival_ts_ms,
        signature: transfer.signature.clone(),
    }
}

#[must_use]
pub fn funding_lookup_wallets(tx: &PoolTransaction) -> Vec<String> {
    let mut wallets = Vec::new();
    let mut seen = HashSet::new();

    for delta in &tx.owner_token_deltas {
        if delta.delta_raw <= 0 {
            continue;
        }
        let owner = delta.owner.trim();
        if owner.is_empty() {
            continue;
        }
        if seen.insert(owner.to_string()) {
            wallets.push(owner.to_string());
        }
    }

    let signer = tx.signer.trim();
    if !signer.is_empty() && seen.insert(signer.to_string()) {
        wallets.push(signer.to_string());
    }

    wallets
}

fn canonical_buyer_identity(tx: &PoolTransaction) -> Option<String> {
    funding_lookup_wallets(tx).into_iter().next()
}

fn tx_event_ts_ms(tx: &PoolTransaction) -> u64 {
    tx.event_time
        .compat_event_ts_ms(Some(tx.timestamp_ms))
        .unwrap_or(tx.timestamp_ms)
}

fn tx_buy_sol(tx: &PoolTransaction) -> f64 {
    tx.sol_amount_lamports
        .map(|lamports| lamports as f64 / 1_000_000_000.0)
        .unwrap_or(tx.volume_sol)
        .max(0.0)
}

fn funding_transfer_event_ts_ms(transfer: &FundingTransferObserved) -> u64 {
    transfer
        .event_time
        .compat_event_ts_ms((transfer.arrival_ts_ms > 0).then_some(transfer.arrival_ts_ms))
        .unwrap_or(transfer.arrival_ts_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RawBytesMissingReason;
    use ghost_core::{CurveFinality, EventSemanticEnvelope, EventTimeMetadata};
    use seer::early_fingerprint::TokenDelta;

    fn config() -> FundingSourceConfig {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.funding_lookback_window_s = 1;
        gatekeeper_config.funding_dust_threshold_lamports = 10_000;
        gatekeeper_config.fsc_per_recipient_cap = 2;
        gatekeeper_config.fsc_global_recipient_cap = 8;
        FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config)
    }

    #[test]
    fn fsc_v2_config_plumbing_controls_thresholds_and_hash() {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.funding_lookback_window_s = 1;
        gatekeeper_config.funding_dust_threshold_lamports = 10_000;
        gatekeeper_config.fsc_per_recipient_cap = 2;
        gatekeeper_config.fsc_global_recipient_cap = 8;

        let mut fsc_config = FscV2Config::default();
        fsc_config.lookback_window_s = 300;
        fsc_config.min_abs_store_lamports = 1_000_000;
        fsc_config.min_abs_attribution_lamports = 10_000_000;
        fsc_config.min_rel_to_buy = 0.20;
        fsc_config.min_attribution_confidence = 0.60;
        fsc_config.neutral_funder_set_version = Some("neutral-v-test".to_string());

        let legacy_config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config);
        let config = FundingSourceConfig::from_configs(&gatekeeper_config, Some(&fsc_config));

        assert_eq!(config.lookback_window_ms, 300_000);
        assert_eq!(config.min_abs_store_lamports, 1_000_000);
        assert_eq!(config.min_abs_attribution_lamports, 10_000_000);
        assert_eq!(config.min_rel_to_buy, 0.20);
        assert_eq!(config.min_attribution_confidence_bps, 6_000);
        assert_eq!(
            config.neutral_funder_set_version.as_deref(),
            Some("neutral-v-test")
        );
        assert_ne!(
            funding_source_config_hash(&legacy_config),
            funding_source_config_hash(&config)
        );

        let mut changed = fsc_config.clone();
        changed.min_rel_to_buy = 0.25;
        let changed_config = FundingSourceConfig::from_configs(&gatekeeper_config, Some(&changed));
        assert_ne!(
            funding_source_config_hash(&config),
            funding_source_config_hash(&changed_config)
        );
    }

    fn buy_tx(signer: &str, signature: &str, timestamp_ms: u64) -> PoolTransaction {
        PoolTransaction {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: "pool-1".to_string(),
            slot: None,
            event_ordinal: Some(0),
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms,
            event_time: EventTimeMetadata::new(None, Some(timestamp_ms), None),
            arrival_ts_ms: timestamp_ms.saturating_add(1),
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: 0.2,
            sol_amount_lamports: Some(200_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: signature.to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: vec![],
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: CurveFinality::Speculative,
        }
    }

    fn buy_tx_with_owner(
        signer: &str,
        owner: &str,
        signature: &str,
        timestamp_ms: u64,
    ) -> PoolTransaction {
        let mut tx = buy_tx(signer, signature, timestamp_ms);
        tx.owner_token_deltas = vec![TokenDelta {
            owner: owner.to_string(),
            delta_raw: 1_000_000,
            decimals: 6,
        }];
        tx
    }

    fn funding_transfer(
        source_wallet: &str,
        recipient_wallet: &str,
        signature: &str,
        event_ts_ms: u64,
        lamports: u64,
    ) -> FundingTransferObserved {
        FundingTransferObserved {
            semantic: EventSemanticEnvelope::default(),
            slot: None,
            event_ordinal: None,
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            cpi_stack_height: None,
            event_time: EventTimeMetadata::new(None, Some(event_ts_ms), None),
            arrival_ts_ms: event_ts_ms.saturating_add(1),
            signature: signature.to_string(),
            source_wallet: source_wallet.to_string(),
            recipient_wallet: recipient_wallet.to_string(),
            lamports,
            full_chain_coverage: true,
            provenance: seer::ipc::FundingTransferProvenance::authoritative_full_feed_live(),
            lane_health: seer::ipc::FundingLaneRuntimeHealth::default(),
            detected_at: std::time::SystemTime::now(),
            sequence_number: event_ts_ms,
        }
    }

    fn assert_approx_eq(left: f64, right: f64) {
        assert!(
            (left - right).abs() <= 1e-9,
            "left={left} right={right} diff={}",
            (left - right).abs()
        );
    }

    #[test]
    fn same_funder_yields_high_fsc() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-shared", "buyer-a", "fund-a", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-shared", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-shared", "buyer-c", "fund-c", 300, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
            buy_tx("buyer-c", "buy-c", 600),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_approx_eq(
            computed
                .funding_source_concentration
                .expect("fsc should be materialized"),
            2.0 / 3.0,
        );
        assert!(computed.degraded_reasons.is_empty());
    }

    #[test]
    fn distinct_funders_yield_low_fsc() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a", "buyer-a", "fund-a", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-b", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-c", "buyer-c", "fund-c", 300, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
            buy_tx("buyer-c", "buy-c", 600),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.0));
        assert!(computed.degraded_reasons.is_empty());
    }

    #[test]
    fn neutral_funders_do_not_artificially_cluster_buyers() {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.funding_lookback_window_s = 1;
        gatekeeper_config.funding_dust_threshold_lamports = 10_000;
        gatekeeper_config.neutral_funding_sources = vec!["neutral-hot-wallet".to_string()];
        let config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config);
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("neutral-hot-wallet", "buyer-a", "fund-a", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("neutral-hot-wallet", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("neutral-hot-wallet", "buyer-c", "fund-c", 300, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
            buy_tx("buyer-c", "buy-c", 600),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.0));
        assert!(computed.degraded_reasons.is_empty());
        assert_eq!(
            computed.funding_source_v2.status,
            FscEvidenceStatus::Degraded
        );
        assert_eq!(
            computed.funding_source_v2.excluded_reason,
            Some(FscExcludedReason::NeutralOnly)
        );
        assert_eq!(computed.funding_source_v2.hhi_norm_count, None);
        assert_eq!(
            computed.funding_source_v2.raw_hhi_including_neutral,
            Some(1.0)
        );
        assert_eq!(computed.funding_source_v2.neutral_count, 3);
    }

    #[test]
    fn fsc_v2_mixed_neutral_and_non_neutral_support_is_not_neutral_only() {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.funding_lookback_window_s = 1;
        gatekeeper_config.funding_dust_threshold_lamports = 10_000;
        gatekeeper_config.neutral_funding_sources = vec!["neutral-hot-wallet".to_string()];
        let config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config);
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("neutral-hot-wallet", "buyer-a", "fund-a", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("non-neutral-funder", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(
            computed.funding_source_v2.status,
            FscEvidenceStatus::Degraded
        );
        assert_eq!(
            computed.funding_source_v2.excluded_reason,
            Some(FscExcludedReason::InsufficientNonNeutralSupport)
        );
        assert_eq!(computed.funding_source_v2.known_non_neutral_buyers, 1);
        assert_eq!(computed.funding_source_v2.neutral_count, 1);
        assert_eq!(computed.funding_source_v2.hhi_norm_count, None);
    }

    #[test]
    fn insufficient_known_sources_returns_reason() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a", "buyer-a", "fund-a", 100, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()]
        );
        assert_eq!(computed.diagnostics.buyer_sample_count, 2);
        assert_eq!(computed.diagnostics.known_source_count, 1);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 1);
        assert_eq!(computed.diagnostics.structural_unknown_buyer_count, 0);
        assert_eq!(computed.diagnostics.operational_unknown_buyer_count, 0);
        assert_eq!(computed.diagnostics.indeterminate_unknown_buyer_count, 1);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON.to_string(),
                class: FscMissClass::Indeterminate,
                count: 1,
            }]
        );
    }

    #[test]
    fn dominant_pre_buy_source_can_be_latest_transfer() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("old-funder", "buyer-a", "fund-a-old", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-a", "fund-a-new", 250, 75_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-b", "fund-b", 260, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 300),
            buy_tx("buyer-b", "buy-b", 400),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
    }

    #[test]
    fn dominant_source_resists_late_small_transfer_poisoning() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer(
                "shared-funder",
                "buyer-a",
                "fund-a-dominant",
                100,
                400_000_000,
            ),
            &config,
        );
        index.observe_transfer(
            &funding_transfer(
                "late-small-funder",
                "buyer-a",
                "fund-a-late-small",
                250,
                30_000_000,
            ),
            &config,
        );
        index.observe_transfer(
            &funding_transfer(
                "shared-funder",
                "buyer-b",
                "fund-b-dominant",
                260,
                50_000_000,
            ),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 300),
            buy_tx("buyer-b", "buy-b", 400),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert_eq!(computed.diagnostics.known_source_count, 2);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 0);
    }

    #[test]
    fn low_attribution_confidence_is_explicit_unknown() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a", "buyer-a", "fund-a", 100, 55_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-b", "buyer-a", "fund-b", 200, 45_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-c", "buyer-b", "fund-c", 210, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 300),
            buy_tx("buyer-b", "buy-b", 400),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.funding_source_v2.status,
            FscEvidenceStatus::Degraded
        );
        assert_eq!(
            computed.funding_source_v2.excluded_reason,
            Some(FscExcludedReason::LowAttributionConfidence)
        );
        assert_eq!(computed.funding_source_v2.low_confidence_count, 1);
        assert_eq!(computed.funding_source_v2.hhi_norm_count, None);
        assert_eq!(computed.diagnostics.known_source_count, 1);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 1);
        assert_eq!(computed.diagnostics.indeterminate_unknown_buyer_count, 1);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON.to_string(),
                class: FscMissClass::Indeterminate,
                count: 1,
            }]
        );
    }

    #[test]
    fn same_slot_cross_signature_without_tx_index_is_unorderable() {
        let config = config();
        let index = FundingSourceIndex::new();

        let mut funding_a = funding_transfer("shared-funder", "buyer-a", "fund-a", 400, 50_000_000);
        funding_a.slot = Some(42);
        funding_a.tx_index = None;
        index.observe_transfer(&funding_a, &config);

        let mut funding_b = funding_transfer("shared-funder", "buyer-b", "fund-b", 100, 50_000_000);
        funding_b.slot = Some(41);
        index.observe_transfer(&funding_b, &config);

        let mut buy_a = buy_tx("buyer-a", "buy-a", 500);
        buy_a.slot = Some(42);
        buy_a.tx_index = None;

        let mut buy_b = buy_tx("buyer-b", "buy-b", 500);
        buy_b.slot = Some(42);

        let buys = vec![buy_a, buy_b];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.funding_source_v2.status,
            FscEvidenceStatus::Degraded
        );
        assert_eq!(
            computed.funding_source_v2.excluded_reason,
            Some(FscExcludedReason::SameSlotOrderingUnavailable)
        );
        assert_eq!(computed.funding_source_v2.same_slot_unorderable_count, 1);
        assert_eq!(computed.diagnostics.known_source_count, 1);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 1);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON.to_string(),
                class: FscMissClass::Indeterminate,
                count: 1,
            }]
        );
    }

    #[test]
    fn same_slot_cross_signature_tx_index_orders_transfer_before_buy() {
        let config = config();
        let index = FundingSourceIndex::new();

        let mut funding_a = funding_transfer("shared-funder", "buyer-a", "fund-a", 400, 50_000_000);
        funding_a.slot = Some(42);
        funding_a.tx_index = Some(3);
        index.observe_transfer(&funding_a, &config);

        let mut funding_b = funding_transfer("shared-funder", "buyer-b", "fund-b", 400, 50_000_000);
        funding_b.slot = Some(42);
        funding_b.tx_index = Some(4);
        index.observe_transfer(&funding_b, &config);

        let mut buy_a = buy_tx("buyer-a", "buy-a", 400);
        buy_a.slot = Some(42);
        buy_a.tx_index = Some(5);

        let mut buy_b = buy_tx("buyer-b", "buy-b", 400);
        buy_b.slot = Some(42);
        buy_b.tx_index = Some(6);

        let buys = vec![buy_a, buy_b];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert_eq!(computed.diagnostics.known_source_count, 2);
        assert!(computed.degraded_reasons.is_empty());
    }

    #[test]
    fn first_buy_per_buyer_uses_order_key_not_buffer_order() {
        let mut later = buy_tx("buyer-a", "buy-later", 500);
        later.slot = Some(20);
        later.tx_index = Some(2);

        let mut earlier = buy_tx("buyer-a", "buy-earlier", 400);
        earlier.slot = Some(20);
        earlier.tx_index = Some(1);

        let buyers = unique_successful_buyers([&later, &earlier]);

        assert_eq!(buyers.len(), 1);
        assert_eq!(buyers[0].signature, "buy-earlier");
    }

    #[test]
    fn post_buy_transfer_does_not_overwrite_lookup() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-a", "fund-a-before", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-b", "fund-b-before", 120, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer(
                "post-buy-funder",
                "buyer-a",
                "fund-a-after",
                450,
                50_000_000,
            ),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
    }

    #[test]
    fn same_signature_provenance_orders_top_level_funding_before_buy() {
        let config = config();
        let index = FundingSourceIndex::new();

        let mut funding_a = funding_transfer("shared-funder", "buyer-a", "sig-a", 400, 50_000_000);
        funding_a.arrival_ts_ms = 401;
        funding_a.event_ordinal = Some(0);
        funding_a.outer_instruction_index = Some(0);
        index.observe_transfer(&funding_a, &config);

        let mut funding_b = funding_transfer("shared-funder", "buyer-b", "sig-b", 400, 50_000_000);
        funding_b.arrival_ts_ms = 401;
        funding_b.event_ordinal = Some(0);
        funding_b.outer_instruction_index = Some(0);
        index.observe_transfer(&funding_b, &config);

        let mut buy_a = buy_tx("buyer-a", "sig-a", 400);
        buy_a.arrival_ts_ms = 401;
        buy_a.event_ordinal = Some(1);
        buy_a.outer_instruction_index = Some(1);

        let mut buy_b = buy_tx("buyer-b", "sig-b", 400);
        buy_b.arrival_ts_ms = 401;
        buy_b.event_ordinal = Some(1);
        buy_b.outer_instruction_index = Some(1);

        let buys = vec![buy_a, buy_b];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert!(computed.degraded_reasons.is_empty());
    }

    #[test]
    fn same_signature_stack_height_orders_inner_funding_before_inner_buy() {
        let config = config();
        let index = FundingSourceIndex::new();

        let mut funding_a = funding_transfer("shared-funder", "buyer-a", "sig-a", 400, 50_000_000);
        funding_a.arrival_ts_ms = 401;
        funding_a.event_ordinal = Some(7);
        funding_a.outer_instruction_index = Some(0);
        funding_a.inner_group_index = Some(0);
        funding_a.cpi_stack_height = Some(1);
        index.observe_transfer(&funding_a, &config);

        let mut funding_b = funding_transfer("shared-funder", "buyer-b", "sig-b", 400, 50_000_000);
        funding_b.arrival_ts_ms = 401;
        funding_b.event_ordinal = Some(7);
        funding_b.outer_instruction_index = Some(0);
        funding_b.inner_group_index = Some(0);
        funding_b.cpi_stack_height = Some(1);
        index.observe_transfer(&funding_b, &config);

        let mut buy_a = buy_tx("buyer-a", "sig-a", 400);
        buy_a.arrival_ts_ms = 401;
        buy_a.event_ordinal = Some(7);
        buy_a.outer_instruction_index = Some(0);
        buy_a.inner_group_index = Some(0);
        buy_a.cpi_stack_height = Some(2);

        let mut buy_b = buy_tx("buyer-b", "sig-b", 400);
        buy_b.arrival_ts_ms = 401;
        buy_b.event_ordinal = Some(7);
        buy_b.outer_instruction_index = Some(0);
        buy_b.inner_group_index = Some(0);
        buy_b.cpi_stack_height = Some(2);

        let buys = vec![buy_a, buy_b];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert!(computed.degraded_reasons.is_empty());
    }

    #[test]
    fn lower_slot_transfer_precedes_buy_even_if_arrival_is_later() {
        let config = config();
        let index = FundingSourceIndex::new();

        let mut funding_a = funding_transfer("shared-funder", "buyer-a", "fund-a", 400, 50_000_000);
        funding_a.slot = Some(10);
        funding_a.arrival_ts_ms = 450;
        index.observe_transfer(&funding_a, &config);

        let mut funding_b = funding_transfer("shared-funder", "buyer-b", "fund-b", 400, 50_000_000);
        funding_b.slot = Some(10);
        funding_b.arrival_ts_ms = 460;
        index.observe_transfer(&funding_b, &config);

        let mut buy_a = buy_tx("buyer-a", "buy-a", 400);
        buy_a.slot = Some(11);
        buy_a.arrival_ts_ms = 401;

        let mut buy_b = buy_tx("buyer-b", "buy-b", 400);
        buy_b.slot = Some(11);
        buy_b.arrival_ts_ms = 402;

        let buys = vec![buy_a, buy_b];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert!(computed.degraded_reasons.is_empty());
    }

    #[test]
    fn owner_resolved_buyer_wallet_unblocks_lookup_when_signer_differs() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-owner-a", "fund-a", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-owner-b", "fund-b", 200, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx_with_owner("relayer-a", "buyer-owner-a", "buy-a", 400),
            buy_tx_with_owner("relayer-b", "buyer-owner-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert!(computed.degraded_reasons.is_empty());
    }

    #[test]
    fn owner_wallet_attribution_is_not_poisoned_by_larger_signer_funding() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer(
                "shared-funder",
                "buyer-owner-a",
                "fund-owner-a",
                100,
                50_000_000,
            ),
            &config,
        );
        index.observe_transfer(
            &funding_transfer(
                "signer-funder",
                "relayer-a",
                "fund-signer-a",
                150,
                500_000_000,
            ),
            &config,
        );
        index.observe_transfer(
            &funding_transfer(
                "shared-funder",
                "buyer-owner-b",
                "fund-owner-b",
                200,
                50_000_000,
            ),
            &config,
        );

        let buys = vec![
            buy_tx_with_owner("relayer-a", "buyer-owner-a", "buy-a", 400),
            buy_tx_with_owner("relayer-b", "buyer-owner-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert_eq!(computed.funding_source_v2.hhi_norm_count, Some(1.0));
        assert_eq!(
            computed.funding_source_v2.top_funder,
            Some(FundingSourceKey::new("shared-funder"))
        );
        assert_eq!(computed.diagnostics.known_source_count, 2);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 0);
    }

    #[test]
    fn fsc_v2_sample_normalized_hhi_controls_match_plan_examples() {
        assert_eq!(normalized_hhi_from_counts(&[5]), Some(1.0));
        assert_eq!(normalized_hhi_from_counts(&[1, 1, 1, 1, 1]), Some(0.0));
        assert_approx_eq(normalized_hhi_from_counts(&[4, 1]).unwrap(), 0.60);
        assert_eq!(normalized_hhi_from_counts(&[1]), None);
    }

    #[test]
    fn fsc_v2_weighted_excess_does_not_confuse_unequal_unique_buy_sizes_with_coordination() {
        let unique_sources = vec![
            ("source-a".to_string(), 1.0),
            ("source-b".to_string(), 2.0),
            ("source-c".to_string(), 7.0),
        ];
        assert_eq!(normalized_sol_weighted_excess(&unique_sources), Some(0.0));

        let shared_source = vec![
            ("source-a".to_string(), 1.0),
            ("source-a".to_string(), 2.0),
            ("source-a".to_string(), 7.0),
        ];
        assert_eq!(normalized_sol_weighted_excess(&shared_source), Some(1.0));
    }

    #[test]
    fn fsc_v2_evidence_serializes_additively_without_legacy_field_redefinition() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-a", "fund-a", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, Some(0.5));
        assert_eq!(computed.funding_source_v2.hhi_norm_count, Some(1.0));
        assert_eq!(computed.funding_source_v2.status, FscEvidenceStatus::Clean);

        let payload = serde_json::to_value(&computed.funding_source_v2)
            .expect("fsc v2 evidence should serialize");
        assert_eq!(payload["version"], "v2");
        assert_eq!(payload["attribution_scope"], "single_hop_native_sol");
        assert_eq!(payload["snapshot_mode"], "decision_time");
        assert_eq!(payload["hhi_norm_count"], 1.0);
        assert_eq!(payload["top_funder"]["wallet"], "shared-funder");
    }

    #[test]
    fn fsc_v2_relative_threshold_and_lane_health_are_reported() {
        let gatekeeper_config = GatekeeperV2Config::default();
        let mut fsc_config = FscV2Config::default();
        fsc_config.lookback_window_s = 1;
        fsc_config.min_abs_store_lamports = 1_000_000;
        fsc_config.min_abs_attribution_lamports = 10_000_000;
        fsc_config.min_rel_to_buy = 0.20;
        fsc_config.min_attribution_confidence = 0.60;
        let config = FundingSourceConfig::from_configs(&gatekeeper_config, Some(&fsc_config));

        let index = FundingSourceIndex::new();
        let mut transfer = funding_transfer("source-a", "buyer-a", "fund-a", 100, 20_000_000);
        transfer.slot = Some(10);
        index.observe_transfer(&transfer, &config);
        index.record_stream_reconnect(150);
        index.record_dropped_events(2);

        let mut buy = buy_tx("buyer-a", "buy-a", 400);
        buy.slot = Some(12);
        let buys = vec![buy];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.diagnostics.rel_too_small_count, 1);
        assert_eq!(computed.diagnostics.known_source_count, 0);
        assert_eq!(computed.funding_source_v2.rel_too_small_count, 1);
        assert_eq!(computed.funding_source_v2.min_abs_store_lamports, 1_000_000);
        assert_eq!(
            computed.funding_source_v2.min_abs_attribution_lamports,
            10_000_000
        );
        assert_eq!(computed.funding_source_v2.min_rel_to_buy, 0.20);
        assert_eq!(
            computed.funding_source_v2.funding_lane_watermark_slot,
            Some(10)
        );
        assert_eq!(computed.funding_source_v2.max_buy_slot, Some(12));
        assert_eq!(computed.funding_source_v2.funding_lane_lag_slots, Some(-2));
        assert_eq!(computed.funding_source_v2.stream_epoch, 1);
        assert_eq!(computed.funding_source_v2.last_reconnect_ts_ms, Some(150));
        assert_eq!(computed.funding_source_v2.dropped_events, 2);
        assert!(computed.funding_source_v2.gap_suspected);
        assert!(computed
            .funding_source_v2
            .last_transfer_recv_ts_ms
            .is_some());
    }

    #[test]
    fn dust_transfer_is_ignored() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a", "buyer-a", "fund-a", 100, 9_999),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-b", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()]
        );
    }

    #[test]
    fn ttl_expiry_removes_stale_funding_without_reverting_to_cold_state() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a", "buyer-a", "fund-a", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-b", "buyer-b", "fund-b", 150, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 2_500),
            buy_tx("buyer-b", "buy-b", 2_600),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()]
        );
        assert_eq!(computed.diagnostics.buyer_sample_count, 2);
        assert_eq!(computed.diagnostics.known_source_count, 0);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 2);
        assert_eq!(computed.diagnostics.structural_unknown_buyer_count, 2);
        assert_eq!(computed.diagnostics.operational_unknown_buyer_count, 0);
        assert_eq!(computed.diagnostics.indeterminate_unknown_buyer_count, 0);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON.to_string(),
                class: FscMissClass::Structural,
                count: 2,
            }]
        );
        assert!(index.warmup_ready());
    }

    #[test]
    fn per_recipient_and_global_caps_prune_safely() {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.funding_lookback_window_s = 1;
        gatekeeper_config.fsc_per_recipient_cap = 2;
        gatekeeper_config.fsc_global_recipient_cap = 1;
        let config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config);
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a1", "buyer-a", "fund-a1", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-a2", "buyer-a", "fund-a2", 200, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-a3", "buyer-a", "fund-a3", 300, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-b1", "buyer-b", "fund-b1", 400, 50_000_000),
            &config,
        );

        assert_eq!(index.entry_count(), 1);
        let buys = vec![
            buy_tx("buyer-a", "buy-a", 500),
            buy_tx("buyer-b", "buy-b", 600),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);
        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()]
        );
        assert_eq!(computed.diagnostics.buyer_sample_count, 2);
        assert_eq!(computed.diagnostics.known_source_count, 1);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 1);
        assert_eq!(computed.diagnostics.structural_unknown_buyer_count, 0);
        assert_eq!(computed.diagnostics.operational_unknown_buyer_count, 1);
        assert_eq!(computed.diagnostics.indeterminate_unknown_buyer_count, 0);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_GLOBAL_RECIPIENT_EVICTED_REASON.to_string(),
                class: FscMissClass::Operational,
                count: 1,
            }]
        );
    }

    #[test]
    fn per_recipient_overflow_is_classified_as_operational_miss() {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.funding_lookback_window_s = 1;
        gatekeeper_config.fsc_per_recipient_cap = 1;
        gatekeeper_config.fsc_global_recipient_cap = 8;
        let config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config);
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a1", "buyer-a", "fund-a1", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-a2", "buyer-a", "fund-a2", 350, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-b1", "buyer-b", "fund-b1", 200, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 300),
            buy_tx("buyer-b", "buy-b", 400),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON.to_string(),
                class: FscMissClass::Operational,
                count: 1,
            }]
        );
        assert_eq!(computed.diagnostics.operational_unknown_buyer_count, 1);
    }

    #[test]
    fn post_buy_only_history_is_classified_as_structural_miss() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-a", "buyer-a", "fund-a", 450, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-b", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON.to_string(),
                class: FscMissClass::Structural,
                count: 1,
            }]
        );
        assert_eq!(computed.diagnostics.structural_unknown_buyer_count, 1);
    }

    #[test]
    fn missing_buyer_identity_is_classified_as_operational_miss() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-b", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );

        let mut missing_identity = buy_tx("", "buy-a", 400);
        missing_identity.signer.clear();
        missing_identity.owner_token_deltas.clear();

        let buys = vec![missing_identity, buy_tx("buyer-b", "buy-b", 500)];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(computed.diagnostics.buyer_sample_count, 2);
        assert_eq!(computed.diagnostics.known_source_count, 1);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 1);
        assert_eq!(computed.diagnostics.operational_unknown_buyer_count, 1);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_BUYER_IDENTITY_UNAVAILABLE_REASON.to_string(),
                class: FscMissClass::Operational,
                count: 1,
            }]
        );
    }

    #[test]
    fn missing_buy_timestamp_is_classified_as_operational_miss() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("funder-b", "buyer-b", "fund-b", 200, 50_000_000),
            &config,
        );

        let mut missing_timestamp = buy_tx("buyer-a", "buy-a", 0);
        missing_timestamp.timestamp_ms = 0;
        missing_timestamp.event_time = EventTimeMetadata::default();

        let buys = vec![missing_timestamp, buy_tx("buyer-b", "buy-b", 500)];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(computed.diagnostics.known_source_count, 1);
        assert_eq!(computed.diagnostics.unknown_buyer_count, 1);
        assert_eq!(computed.diagnostics.operational_unknown_buyer_count, 1);
        assert_eq!(
            computed.diagnostics.miss_reason_counts,
            vec![FundingSourceMissReasonCount {
                reason: FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON.to_string(),
                class: FscMissClass::Operational,
                count: 1,
            }]
        );
    }

    #[test]
    fn warmup_unavailable_returns_rolling_state_reason() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.set_stream_available(true);

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()]
        );
    }

    #[test]
    fn stream_unavailable_returns_stream_reason() {
        let config = config();
        let index = FundingSourceIndex::new();

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()]
        );
    }

    #[test]
    fn filtered_transfer_does_not_mark_funding_stream_available() {
        let config = config();
        let index = FundingSourceIndex::new();
        let mut transfer = funding_transfer("funder-a", "buyer-a", "fund-a", 100, 50_000_000);
        transfer.full_chain_coverage = false;
        transfer.provenance =
            seer::ipc::FundingTransferProvenance::filtered_grpc_global_stream_live();
        index.observe_transfer(&transfer, &config);

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert!(!index.warmup_ready());
        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()]
        );
    }

    #[test]
    fn capture_transfer_warms_index_when_stream_is_explicitly_available() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.set_stream_available(true);

        let mut transfer_a =
            funding_transfer("funder-shared", "buyer-a", "fund-a", 100, 50_000_000);
        transfer_a.full_chain_coverage = false;
        transfer_a.provenance = seer::ipc::FundingTransferProvenance::nln_program_streams_live(
            seer::ipc::FundingTransferCoverageClass::FilteredObservations,
        );
        let mut transfer_b =
            funding_transfer("funder-shared", "buyer-b", "fund-b", 110, 50_000_000);
        transfer_b.full_chain_coverage = false;
        transfer_b.provenance = seer::ipc::FundingTransferProvenance::nln_program_streams_live(
            seer::ipc::FundingTransferCoverageClass::FilteredObservations,
        );
        index.observe_transfer(&transfer_a, &config);
        index.observe_transfer(&transfer_b, &config);

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert!(index.warmup_ready());
        assert_eq!(computed.degraded_reasons, Vec::<String>::new());
        assert_approx_eq(
            computed
                .funding_source_v2
                .hhi_norm_count
                .expect("capture FSC v2 should be materialized"),
            1.0,
        );
    }

    #[test]
    fn coverage_window_ready_requires_full_continuous_availability_window() {
        let config = config();
        let index = FundingSourceIndex::new();
        {
            let mut inner = index.inner.write();
            inner.stream_available = true;
            inner.stream_available_since_ms = Some(1_000);
            inner.saw_transfer = true;
            inner.availability_controlled = true;
        }

        let before_window = index.coverage_window_status(&config, 1_999);
        assert!(!before_window.coverage_window_ready);
        assert!(!before_window.authoritative_buy_ready);
        assert_eq!(before_window.coverage_window_remaining_ms, 1);

        let at_window = index.coverage_window_status(&config, 2_000);
        assert!(at_window.coverage_window_ready);
        assert!(at_window.authoritative_buy_ready);
        assert_eq!(at_window.coverage_window_remaining_ms, 0);
    }

    #[test]
    fn coverage_window_resets_after_availability_drop_and_reopens_only_after_fresh_window() {
        let config = config();
        let index = FundingSourceIndex::new();
        {
            let mut inner = index.inner.write();
            inner.stream_available = true;
            inner.stream_available_since_ms = Some(1_000);
            inner.saw_transfer = true;
            inner.availability_controlled = true;
        }

        assert!(
            index
                .coverage_window_status(&config, 2_000)
                .authoritative_buy_ready
        );

        {
            let mut inner = index.inner.write();
            inner.stream_available = false;
            inner.stream_available_since_ms = None;
        }
        let dropped = index.coverage_window_status(&config, 5_000);
        assert!(!dropped.coverage_window_ready);
        assert!(!dropped.authoritative_buy_ready);
        assert_eq!(
            dropped.coverage_window_remaining_ms,
            config.lookback_window_ms
        );

        {
            let mut inner = index.inner.write();
            inner.stream_available = true;
            inner.stream_available_since_ms = Some(6_000);
        }
        assert!(
            !index
                .coverage_window_status(&config, 6_999)
                .authoritative_buy_ready
        );
        assert!(
            index
                .coverage_window_status(&config, 7_000)
                .authoritative_buy_ready
        );
    }
}
