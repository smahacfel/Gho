use crate::events::{FundingTransferObserved, PoolTransaction};
use crate::oracle_metrics::{
    record_fsc_authoritative_funding_stream_available, record_fsc_evidence_status,
    record_fsc_index_entries, record_fsc_index_estimated_memory_bytes,
    record_fsc_index_evicted_recipient_entries, record_fsc_index_global_cap_evictions,
    record_fsc_index_global_evictions, record_fsc_index_lookup_empty_prunes,
    record_fsc_index_per_recipient_overflows, record_fsc_index_window_prunes,
    record_fsc_lookup_hits, record_fsc_lookup_miss_reason, record_fsc_lookup_misses,
    record_fsc_prune_duration_ms, record_fsc_retention_config, record_fsc_warmup_ready,
};
use ghost_brain::config::{FscV2Config, GatekeeperV2Config};
use ghost_core::tx_intelligence::types::{
    FscAttributionScope, FscEvidenceStatus, FscExcludedReason, FscLookupDiagnostic,
    FscLookupWalletCandidate, FscMissClass, FscSnapshotMode, FscV2Evidence, FscVersion,
    FundingSourceCount, FundingSourceDiagnostics, FundingSourceKey, FundingSourceMissReasonCount,
    FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON, FSC_BUYER_IDENTITY_UNAVAILABLE_REASON,
    FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON, FSC_FUNDING_STREAM_UNAVAILABLE_REASON,
    FSC_GLOBAL_RECIPIENT_EVICTED_REASON, FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
    FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON, FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON,
    FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON, FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON,
    FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON, FSC_RELATIVE_FUNDING_TOO_SMALL_REASON,
    FSC_ROLLING_STATE_UNAVAILABLE_REASON, FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const FSC_V2_PROVIDER_LEGACY_ROLLING_INDEX: &str = "ghost_legacy_rolling_funding_index";
const FSC_V2_PROVIDER_NLN_PROGRAM_STREAMS: &str = "nln_program_streams";
const FSC_V2_PROVIDER_GRPC_FULL_CHAIN: &str = "grpc_funding_lane_full_chain";
const FSC_V2_TOPIC_LEGACY_FUNDING_TRANSFERS: &str = "ghost.funding_transfers";
const FSC_V2_TOPIC_NLN_SYSTEM_TRANSFERS: &str = "prod.rpc.solana.system.transfers";
const FSC_V2_TOPIC_GRPC_FULL_CHAIN: &str = "grpc_funding_lane_full_chain";
const FSC_V2_LANE_AUTHORITATIVE_FULL_FEED: &str = "authoritative_full_feed";
const FSC_V2_LANE_NLN_PROGRAM_STREAMS: &str = "nln_program_streams";
const FSC_LOOKUP_WALLET_SOURCE_OWNER_TOKEN_DELTA_POSITIVE: &str = "owner_token_delta_positive";
const FSC_LOOKUP_WALLET_SOURCE_SIGNER_FALLBACK: &str = "signer_fallback";
const FSC_LOOKUP_RESULT_HIT: &str = "hit";
const FSC_LOOKUP_RESULT_MISS: &str = "miss";
const FSC_LOOKUP_RESULT_NO_CANDIDATE: &str = "no_candidate";
const FSC_DIAG_NO_INBOUND_TRANSFER_OBSERVED: &str = "NO_INBOUND_TRANSFER_OBSERVED";
const FSC_DIAG_INBOUND_EXISTS_BUT_OLDER_THAN_LOOKBACK: &str =
    "INBOUND_EXISTS_BUT_OLDER_THAN_LOOKBACK";
const FSC_DIAG_INBOUND_EXISTS_BUT_PRUNED_BY_WINDOW: &str = "INBOUND_EXISTS_BUT_PRUNED_BY_WINDOW";
const FSC_DIAG_INBOUND_EXISTS_BUT_BELOW_ABS_STORE_THRESHOLD: &str =
    "INBOUND_EXISTS_BUT_BELOW_ABS_STORE_THRESHOLD";
const FSC_DIAG_INBOUND_EXISTS_BUT_BELOW_ABS_ATTRIBUTION_THRESHOLD: &str =
    "INBOUND_EXISTS_BUT_BELOW_ABS_ATTRIBUTION_THRESHOLD";
const FSC_DIAG_INBOUND_EXISTS_BUT_BELOW_REL_THRESHOLD: &str =
    "INBOUND_EXISTS_BUT_BELOW_REL_THRESHOLD";
const FSC_DIAG_ADDRESS_KEY_MISMATCH: &str = "ADDRESS_KEY_MISMATCH";
const FSC_DIAG_SAME_SLOT_ORDERING: &str = "SAME_SLOT_ORDERING";
const FSC_DIAG_TRANSFER_KIND_NOT_STORED: &str = "TRANSFER_KIND_NOT_STORED";
const FSC_DIAG_UNKNOWN: &str = "UNKNOWN";

fn normalize_wallet_key(raw: &str) -> Option<String> {
    let wallet = raw.trim();
    (!wallet.is_empty()).then(|| wallet.to_string())
}

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
    pub min_total_buyers: u64,
    pub min_known_non_neutral_buyers: u64,
    pub min_known_coverage: f64,
    pub min_non_neutral_known_coverage: f64,
    pub require_coverage_window_for_actionability: bool,
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
        let min_rel_to_buy = fsc_v2
            .map(|fsc| fsc.min_rel_to_buy)
            .unwrap_or_else(|| FscV2Config::default().min_rel_to_buy);
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
            min_total_buyers: fsc_v2
                .map(|fsc| u64::from(fsc.min_total_buyers))
                .unwrap_or(2),
            min_known_non_neutral_buyers: fsc_v2
                .map(|fsc| u64::from(fsc.min_known_non_neutral_buyers))
                .unwrap_or(2),
            min_known_coverage: fsc_v2.map_or(0.50, |fsc| fsc.min_known_coverage),
            min_non_neutral_known_coverage: fsc_v2
                .map_or(0.30, |fsc| fsc.min_non_neutral_known_coverage),
            require_coverage_window_for_actionability: config
                .fsc_require_coverage_window_for_actionability,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DroppedTransferSummary {
    last_seen_ms: u64,
    latest_lamports: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EvictedRecipientHistory {
    last_seen_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PruneStats {
    removed_recipients: u64,
    cap_evictions: u64,
    window_prunes: u64,
}

#[derive(Debug, Default)]
struct FundingSourceInner {
    histories: HashMap<String, RecipientHistory>,
    recipient_order: VecDeque<(u64, String)>,
    evicted_recipients: HashMap<String, EvictedRecipientHistory>,
    evicted_recipient_order: VecDeque<(u64, String)>,
    below_store_transfers: HashMap<String, DroppedTransferSummary>,
    below_store_order: VecDeque<(u64, String)>,
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
    diagnostic: FscLookupDiagnostic,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct WalletLookupSummary {
    history_entries_found: u64,
    latest_funding_age_ms: Option<u64>,
    below_store_recent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WalletLookupOutcome {
    Matched {
        matched: FundingSourceMatch,
        removed: bool,
        summary: WalletLookupSummary,
        source_wallets_count: u64,
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
        summary: WalletLookupSummary,
        dust_filtered_count: u64,
        post_buy_filtered_count: u64,
        rel_too_small_count: u64,
    },
    TerminalMiss {
        miss: LookupMiss,
        removed: bool,
        summary: WalletLookupSummary,
        dust_filtered_count: u64,
        post_buy_filtered_count: u64,
        rel_too_small_count: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FundingAttributionSelection {
    recipient_wallet: String,
    source_wallet: String,
    source_wallets_count: u64,
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
    coverage_window_status: FundingCoverageWindowStatus,
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
        config,
    );
    ::metrics::counter!(
        "fsc_evidence_emitted",
        1,
        "status" => fsc_evidence_status_label(status),
        "reason" => excluded_reason
            .map(fsc_excluded_reason_label)
            .unwrap_or("none")
    );
    if let Some(reason) = excluded_reason {
        ::metrics::counter!("fsc_degraded_reason", 1, "reason" => fsc_excluded_reason_label(reason));
    }

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
        coverage_window_ready: coverage_window_status.coverage_window_ready,
        coverage_window_remaining_ms: coverage_window_status.coverage_window_remaining_ms,
        authoritative_buy_ready: coverage_window_status.authoritative_buy_ready,
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

fn record_observed_transfer_lane_state_locked(
    inner: &mut FundingSourceInner,
    transfer: &FundingTransferObserved,
    observation_wall_ms: u64,
) {
    inner.saw_transfer = true;
    inner.funding_lane_watermark_slot =
        max_option_u64(inner.funding_lane_watermark_slot, transfer.slot);
    inner.last_transfer_recv_ts_ms = Some(observation_wall_ms);
    inner
        .observed_funding_lane_kinds
        .insert(transfer.provenance.lane_kind.as_str().to_string());
    if transfer.full_chain_coverage && !inner.availability_controlled {
        inner.stream_available = true;
        inner
            .stream_available_since_ms
            .get_or_insert(observation_wall_ms);
    }
}

fn fsc_v2_source_provenance(inner: &FundingSourceInner) -> (String, Vec<String>) {
    if inner
        .observed_funding_lane_kinds
        .contains(FSC_V2_LANE_AUTHORITATIVE_FULL_FEED)
    {
        return (
            FSC_V2_PROVIDER_GRPC_FULL_CHAIN.to_string(),
            vec![FSC_V2_TOPIC_GRPC_FULL_CHAIN.to_string()],
        );
    }
    if inner
        .observed_funding_lane_kinds
        .contains(FSC_V2_LANE_NLN_PROGRAM_STREAMS)
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
    config: &FundingSourceConfig,
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
    if known_non_neutral_buyers < config.min_known_non_neutral_buyers || hhi_norm_count.is_none() {
        let reason = if known_non_neutral_buyers == 0 && neutral_count > 0 {
            FscExcludedReason::NeutralOnly
        } else {
            FscExcludedReason::InsufficientNonNeutralSupport
        };
        return (FscEvidenceStatus::Degraded, Some(reason));
    }
    if total_buyers < config.min_total_buyers
        || known_coverage < config.min_known_coverage
        || non_neutral_known_coverage < config.min_non_neutral_known_coverage
    {
        return (
            FscEvidenceStatus::Degraded,
            Some(FscExcludedReason::LowCoverage),
        );
    }

    (FscEvidenceStatus::Clean, None)
}

fn fsc_primary_score(evidence: &FscV2Evidence) -> Option<f64> {
    (evidence.status == FscEvidenceStatus::Clean)
        .then_some(evidence.scoring_hhi_non_neutral)
        .flatten()
}

fn fsc_degraded_reasons_for_primary_score(evidence: &FscV2Evidence) -> Vec<String> {
    if fsc_primary_score(evidence).is_some() {
        return Vec::new();
    }

    let reason = match evidence.excluded_reason {
        Some(FscExcludedReason::FundingLaneUnavailable) => FSC_FUNDING_STREAM_UNAVAILABLE_REASON,
        Some(FscExcludedReason::IndexCold) => FSC_ROLLING_STATE_UNAVAILABLE_REASON,
        Some(FscExcludedReason::SameSlotOrderingUnavailable) => {
            FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON
        }
        Some(FscExcludedReason::LowAttributionConfidence) => FSC_LOW_ATTRIBUTION_CONFIDENCE_REASON,
        Some(
            FscExcludedReason::NoBuyerCohort
            | FscExcludedReason::InsufficientNonNeutralSupport
            | FscExcludedReason::LowCoverage
            | FscExcludedReason::NeutralOnly,
        )
        | None => FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
    };

    vec![reason.to_string()]
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
            "lookback_window_ms={};min_abs_store_lamports={};min_abs_attribution_lamports={};min_rel_to_buy_bits={};min_attribution_confidence_bps={};per_recipient_cap={};global_recipient_cap={};min_total_buyers={};min_known_non_neutral_buyers={};min_known_coverage_bits={};min_non_neutral_known_coverage_bits={};require_coverage_window_for_actionability={};neutral_funder_set_version={};neutral_sources={}",
            config.lookback_window_ms,
            config.min_abs_store_lamports,
            config.min_abs_attribution_lamports,
            config.min_rel_to_buy.to_bits(),
            config.min_attribution_confidence_bps,
            config.per_recipient_cap,
            config.global_recipient_cap,
            config.min_total_buyers,
            config.min_known_non_neutral_buyers,
            config.min_known_coverage.to_bits(),
            config.min_non_neutral_known_coverage.to_bits(),
            config.require_coverage_window_for_actionability,
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
        self.set_stream_available_at(available, wall_clock_epoch_ms());
    }

    pub fn set_stream_available_at(&self, available: bool, now_ms: u64) {
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
        let observed_at_ms = funding_transfer_event_ts_ms(transfer);
        let Some(source_wallet) = normalize_wallet_key(&transfer.source_wallet) else {
            return;
        };
        let Some(recipient_wallet) = normalize_wallet_key(&transfer.recipient_wallet) else {
            return;
        };
        if observed_at_ms == 0 || source_wallet == recipient_wallet {
            return;
        }

        let window_start = observed_at_ms.saturating_sub(config.lookback_window_ms);
        let observation_wall_ms = wall_clock_epoch_ms();
        let prune_started_at = Instant::now();

        if transfer.lamports < config.min_abs_store_lamports {
            let mut inner = self.inner.write();
            record_observed_transfer_lane_state_locked(&mut inner, transfer, observation_wall_ms);
            record_below_store_transfer_locked(
                &mut inner,
                recipient_wallet,
                observed_at_ms,
                transfer.lamports,
                window_start,
                config.global_recipient_cap,
            );
            record_fsc_prune_duration_ms(prune_started_at.elapsed().as_secs_f64() * 1_000.0);
            update_retention_metrics(&inner, config);
            return;
        }

        let mut inner = self.inner.write();
        record_observed_transfer_lane_state_locked(&mut inner, transfer, observation_wall_ms);

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
                    && last.source_wallet == source_wallet
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
                    source_wallet: source_wallet.clone(),
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

        let prune_stats =
            prune_global_locked(&mut inner, window_start, config.global_recipient_cap);
        record_fsc_prune_duration_ms(prune_started_at.elapsed().as_secs_f64() * 1_000.0);
        if per_recipient_overflows > 0 {
            record_fsc_index_per_recipient_overflows(per_recipient_overflows);
        }
        record_prune_stats(prune_stats);
        update_retention_metrics(&inner, config);
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
        self.compute_for_transactions_at(transactions, config, wall_clock_epoch_ms())
    }

    #[must_use]
    pub fn compute_for_transactions_at<'a>(
        &self,
        transactions: impl IntoIterator<Item = &'a PoolTransaction>,
        config: &FundingSourceConfig,
        decision_wall_ms: u64,
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
        let coverage_window_status =
            coverage_window_status_locked(&inner, config, decision_wall_ms);

        if !inner.stream_available {
            diagnostics.lookup_diagnostics = buyer_samples
                .iter()
                .map(|tx| {
                    build_unavailable_lookup_diagnostic(
                        tx,
                        FSC_FUNDING_STREAM_UNAVAILABLE_REASON,
                        FSC_DIAG_TRANSFER_KIND_NOT_STORED,
                    )
                })
                .collect();
            let (provider, source_topics) = fsc_v2_source_provenance(&inner);
            let funding_source_v2 = build_fsc_v2_evidence(
                &fsc_v2_accumulator,
                &diagnostics,
                false,
                inner.saw_transfer,
                config,
                max_buy_slot,
                lane_health_locked(&inner),
                coverage_window_status,
                provider,
                source_topics,
            );
            record_fsc_evidence_status(fsc_evidence_status_label(funding_source_v2.status));
            update_retention_metrics(&inner, config);
            return FscComputation {
                funding_source_concentration: None,
                funding_source_v2,
                degraded_reasons: vec![FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()],
                diagnostics,
            };
        }

        if !inner.saw_transfer {
            diagnostics.lookup_diagnostics = buyer_samples
                .iter()
                .map(|tx| {
                    build_unavailable_lookup_diagnostic(
                        tx,
                        FSC_ROLLING_STATE_UNAVAILABLE_REASON,
                        FSC_DIAG_NO_INBOUND_TRANSFER_OBSERVED,
                    )
                })
                .collect();
            let (provider, source_topics) = fsc_v2_source_provenance(&inner);
            let funding_source_v2 = build_fsc_v2_evidence(
                &fsc_v2_accumulator,
                &diagnostics,
                inner.stream_available,
                false,
                config,
                max_buy_slot,
                lane_health_locked(&inner),
                coverage_window_status,
                provider,
                source_topics,
            );
            record_fsc_evidence_status(fsc_evidence_status_label(funding_source_v2.status));
            update_retention_metrics(&inner, config);
            return FscComputation {
                funding_source_concentration: None,
                funding_source_v2,
                degraded_reasons: vec![FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()],
                diagnostics,
            };
        }

        let mut lookup_hits = 0u64;
        let mut lookup_misses = 0u64;
        let mut removed_entries = 0u64;

        for tx in buyer_samples {
            let lookup = lookup_source_for_buy(&mut inner, tx, config);
            diagnostics
                .lookup_diagnostics
                .push(lookup.diagnostic.clone());
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
                    fsc_v2_accumulator.record_concrete(
                        source,
                        tx_buy_sol(tx),
                        lookup.attribution_confidence_bps,
                    );
                }
                FundingSourceMatch::Neutral { source_wallet, .. } => {
                    lookup_hits = lookup_hits.saturating_add(1);
                    diagnostics.known_source_count =
                        diagnostics.known_source_count.saturating_add(1);
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
        let prune_stats =
            prune_global_locked(&mut inner, window_start, config.global_recipient_cap);
        record_fsc_prune_duration_ms(prune_started_at.elapsed().as_secs_f64() * 1_000.0);
        record_prune_stats(prune_stats);
        if removed_entries > 0 {
            record_fsc_index_global_evictions(removed_entries);
            record_fsc_index_lookup_empty_prunes(removed_entries);
        }
        update_retention_metrics(&inner, config);
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
            coverage_window_status,
            provider,
            source_topics,
        );
        record_fsc_evidence_status(fsc_evidence_status_label(funding_source_v2.status));

        let funding_source_concentration = fsc_primary_score(&funding_source_v2);
        let degraded_reasons = fsc_degraded_reasons_for_primary_score(&funding_source_v2);
        FscComputation {
            funding_source_concentration,
            funding_source_v2,
            degraded_reasons,
            diagnostics,
        }
    }
}

fn update_index_metrics(inner: &FundingSourceInner) {
    record_fsc_index_entries(inner.histories.len());
    record_fsc_index_evicted_recipient_entries(inner.evicted_recipients.len());
    record_fsc_authoritative_funding_stream_available(inner.stream_available);
    record_fsc_warmup_ready(inner.stream_available && inner.saw_transfer);
    ::metrics::gauge!("funding_index_size", inner.histories.len() as f64);
    ::metrics::gauge!(
        "funding_index_warm",
        if inner.stream_available && inner.saw_transfer {
            1.0
        } else {
            0.0
        }
    );
}

fn update_retention_metrics(inner: &FundingSourceInner, config: &FundingSourceConfig) {
    update_index_metrics(inner);
    record_fsc_retention_config(
        config.global_recipient_cap,
        config.per_recipient_cap,
        config.lookback_window_ms,
    );
    record_fsc_index_estimated_memory_bytes(
        inner.histories.len(),
        inner.evicted_recipients.len(),
        config.per_recipient_cap,
    );
}

fn record_prune_stats(stats: PruneStats) {
    if stats.removed_recipients > 0 {
        record_fsc_index_global_evictions(stats.removed_recipients);
    }
    if stats.cap_evictions > 0 {
        record_fsc_index_global_cap_evictions(stats.cap_evictions);
    }
    if stats.window_prunes > 0 {
        record_fsc_index_window_prunes(stats.window_prunes);
    }
}

fn fsc_evidence_status_label(status: FscEvidenceStatus) -> &'static str {
    match status {
        FscEvidenceStatus::Clean => "clean",
        FscEvidenceStatus::Degraded => "degraded",
        FscEvidenceStatus::Unavailable => "unavailable",
    }
}

fn fsc_excluded_reason_label(reason: FscExcludedReason) -> &'static str {
    match reason {
        FscExcludedReason::FundingLaneUnavailable => "funding_lane_unavailable",
        FscExcludedReason::IndexCold => "index_cold",
        FscExcludedReason::NoBuyerCohort => "no_buyer_cohort",
        FscExcludedReason::InsufficientNonNeutralSupport => "insufficient_non_neutral_support",
        FscExcludedReason::LowCoverage => "low_coverage",
        FscExcludedReason::NeutralOnly => "neutral_only",
        FscExcludedReason::SameSlotOrderingUnavailable => "same_slot_ordering_unavailable",
        FscExcludedReason::LowAttributionConfidence => "low_attribution_confidence",
    }
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

fn build_unavailable_lookup_diagnostic(
    tx: &PoolTransaction,
    miss_reason: &'static str,
    diagnostic_miss_reason: &'static str,
) -> FscLookupDiagnostic {
    let candidates = funding_lookup_wallet_candidates(tx);
    let selected = candidates.first();
    let miss = LookupMiss {
        reason: miss_reason,
        class: FscMissClass::Operational,
    };
    let mut diagnostic = build_lookup_diagnostic(
        tx,
        &candidates,
        selected,
        if selected.is_some() {
            FSC_LOOKUP_RESULT_MISS
        } else {
            FSC_LOOKUP_RESULT_NO_CANDIDATE
        },
        WalletLookupSummary::default(),
        Some(miss),
        None,
        0,
        0,
        0,
    );
    diagnostic.diagnostic_miss_reason = Some(diagnostic_miss_reason.to_string());
    diagnostic
}

fn build_lookup_diagnostic(
    tx: &PoolTransaction,
    candidates: &[FscLookupWalletCandidate],
    selected: Option<&FscLookupWalletCandidate>,
    lookup_result: &'static str,
    summary: WalletLookupSummary,
    miss: Option<LookupMiss>,
    source_wallet: Option<String>,
    source_wallets_count: u64,
    selected_lamports: u128,
    total_lamports: u128,
) -> FscLookupDiagnostic {
    let miss_reason = miss.map(|miss| miss.reason.to_string());
    let diagnostic_miss_reason = miss.map(|miss| {
        diagnostic_lookup_miss_reason(miss, summary)
            .unwrap_or(FSC_DIAG_UNKNOWN)
            .to_string()
    });
    let buy_event_ts_ms = tx_event_ts_ms(tx);
    FscLookupDiagnostic {
        lookup_wallet: selected.map(|candidate| candidate.wallet.clone()),
        candidate_wallets: candidates.to_vec(),
        selected_lookup_wallet: selected.map(|candidate| candidate.wallet.clone()),
        lookup_wallet_source: selected.map(|candidate| candidate.source.clone()),
        fallback_used: selected
            .is_some_and(|candidate| candidate.source == FSC_LOOKUP_WALLET_SOURCE_SIGNER_FALLBACK),
        slot: tx.slot,
        signature: (!tx.signature.trim().is_empty()).then(|| tx.signature.clone()),
        buy_event_ts_ms: (buy_event_ts_ms > 0).then_some(buy_event_ts_ms),
        lookup_result: lookup_result.to_string(),
        history_entries_found: summary.history_entries_found,
        latest_funding_age_ms: summary.latest_funding_age_ms,
        matched_source_wallets_count: source_wallets_count,
        matched_total_lamports: saturating_u128_to_u64(total_lamports),
        funding_amount_lamports: (selected_lamports > 0)
            .then(|| saturating_u128_to_u64(selected_lamports)),
        source_wallet,
        miss_reason,
        diagnostic_miss_reason,
    }
}

fn diagnostic_lookup_miss_reason(
    miss: LookupMiss,
    summary: WalletLookupSummary,
) -> Option<&'static str> {
    match miss.reason {
        FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON if summary.below_store_recent => {
            Some(FSC_DIAG_INBOUND_EXISTS_BUT_BELOW_ABS_STORE_THRESHOLD)
        }
        FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON => Some(FSC_DIAG_NO_INBOUND_TRANSFER_OBSERVED),
        FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON => {
            Some(FSC_DIAG_INBOUND_EXISTS_BUT_OLDER_THAN_LOOKBACK)
        }
        FSC_GLOBAL_RECIPIENT_EVICTED_REASON | FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON => {
            Some(FSC_DIAG_INBOUND_EXISTS_BUT_PRUNED_BY_WINDOW)
        }
        FSC_ABS_ATTRIBUTION_TOO_SMALL_REASON => {
            Some(FSC_DIAG_INBOUND_EXISTS_BUT_BELOW_ABS_ATTRIBUTION_THRESHOLD)
        }
        FSC_RELATIVE_FUNDING_TOO_SMALL_REASON => {
            Some(FSC_DIAG_INBOUND_EXISTS_BUT_BELOW_REL_THRESHOLD)
        }
        FSC_BUYER_IDENTITY_UNAVAILABLE_REASON => Some(FSC_DIAG_ADDRESS_KEY_MISMATCH),
        FSC_SAME_SLOT_ORDERING_UNAVAILABLE_REASON => Some(FSC_DIAG_SAME_SLOT_ORDERING),
        FSC_FUNDING_STREAM_UNAVAILABLE_REASON | FSC_ROLLING_STATE_UNAVAILABLE_REASON => {
            Some(FSC_DIAG_TRANSFER_KIND_NOT_STORED)
        }
        _ => None,
    }
}

fn wallet_lookup_summary(
    history: &RecipientHistory,
    buy_event_ts_ms: u64,
    below_store_recent: bool,
) -> WalletLookupSummary {
    let latest_funding_age_ms = history
        .transfers
        .iter()
        .map(|transfer| transfer.observed_at_ms)
        .max()
        .map(|latest_ts_ms| buy_event_ts_ms.saturating_sub(latest_ts_ms));
    WalletLookupSummary {
        history_entries_found: history.transfers.len() as u64,
        latest_funding_age_ms,
        below_store_recent,
    }
}

fn funding_match_source_wallet(matched: &FundingSourceMatch) -> Option<String> {
    match matched {
        FundingSourceMatch::Concrete(source) => Some(source.clone()),
        FundingSourceMatch::Neutral { source_wallet, .. } => Some(source_wallet.clone()),
        FundingSourceMatch::Unknown => None,
    }
}

fn saturating_u128_to_u64(value: u128) -> u64 {
    value.min(u128::from(u64::MAX)) as u64
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

fn record_below_store_transfer_locked(
    inner: &mut FundingSourceInner,
    recipient_wallet: String,
    observed_at_ms: u64,
    lamports: u64,
    window_start: u64,
    global_recipient_cap: usize,
) {
    let summary = inner
        .below_store_transfers
        .entry(recipient_wallet.clone())
        .or_default();
    if observed_at_ms >= summary.last_seen_ms {
        summary.last_seen_ms = observed_at_ms;
        summary.latest_lamports = lamports;
        inner
            .below_store_order
            .push_back((observed_at_ms, recipient_wallet));
    }
    prune_below_store_transfers_locked(inner, window_start, global_recipient_cap);
}

fn prune_below_store_transfers_locked(
    inner: &mut FundingSourceInner,
    window_start: u64,
    global_recipient_cap: usize,
) {
    while let Some((tracked_last_seen, recipient)) = inner.below_store_order.front().cloned() {
        let should_prune_for_window = tracked_last_seen < window_start;
        let should_prune_for_cap = inner.below_store_transfers.len() > global_recipient_cap;
        if !should_prune_for_window && !should_prune_for_cap {
            break;
        }

        inner.below_store_order.pop_front();
        let should_remove = inner
            .below_store_transfers
            .get(&recipient)
            .is_some_and(|summary| summary.last_seen_ms == tracked_last_seen);
        if should_remove {
            inner.below_store_transfers.remove(&recipient);
        }
    }
}

fn prune_global_locked(
    inner: &mut FundingSourceInner,
    window_start: u64,
    global_recipient_cap: usize,
) -> PruneStats {
    let mut stats = PruneStats::default();
    prune_evicted_recipients_locked(inner, window_start);
    prune_below_store_transfers_locked(inner, window_start, global_recipient_cap);
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
            stats.removed_recipients = stats.removed_recipients.saturating_add(1);
            if should_prune_for_window {
                stats.window_prunes = stats.window_prunes.saturating_add(1);
            } else if should_prune_for_cap {
                stats.cap_evictions = stats.cap_evictions.saturating_add(1);
            }
        }
    }
    stats
}

fn lookup_source_for_buy(
    inner: &mut FundingSourceInner,
    tx: &PoolTransaction,
    config: &FundingSourceConfig,
) -> LookupSourceResult {
    let lookup_candidates = funding_lookup_wallet_candidates(tx);
    if lookup_candidates.is_empty() {
        let miss = LookupMiss {
            reason: FSC_BUYER_IDENTITY_UNAVAILABLE_REASON,
            class: FscMissClass::Operational,
        };
        return LookupSourceResult {
            matched: FundingSourceMatch::Unknown,
            removed: false,
            miss: Some(miss),
            diagnostic: build_lookup_diagnostic(
                tx,
                &lookup_candidates,
                None,
                FSC_LOOKUP_RESULT_NO_CANDIDATE,
                WalletLookupSummary::default(),
                Some(miss),
                None,
                0,
                0,
                0,
            ),
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
        let miss = LookupMiss {
            reason: FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON,
            class: FscMissClass::Operational,
        };
        return LookupSourceResult {
            matched: FundingSourceMatch::Unknown,
            removed: false,
            miss: Some(miss),
            diagnostic: build_lookup_diagnostic(
                tx,
                &lookup_candidates,
                lookup_candidates.first(),
                FSC_LOOKUP_RESULT_MISS,
                WalletLookupSummary::default(),
                Some(miss),
                None,
                0,
                0,
                0,
            ),
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
    let mut lookup_miss_context =
        None::<(LookupMiss, FscLookupWalletCandidate, WalletLookupSummary)>;
    let mut removed = false;
    let mut counters = LookupCounters::default();
    for candidate in &lookup_candidates {
        match lookup_source_for_wallet(
            inner,
            candidate.wallet.as_str(),
            tx,
            config,
            buy_event_ts_ms,
            buy_window_start,
        ) {
            WalletLookupOutcome::Matched {
                matched,
                removed: wallet_removed,
                summary,
                source_wallets_count,
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
                    inner.histories.remove(candidate.wallet.as_str());
                }
                removed |= wallet_removed;
                let source_wallet = funding_match_source_wallet(&matched);
                return LookupSourceResult {
                    matched,
                    removed,
                    miss: None,
                    diagnostic: build_lookup_diagnostic(
                        tx,
                        &lookup_candidates,
                        Some(candidate),
                        FSC_LOOKUP_RESULT_HIT,
                        summary,
                        None,
                        source_wallet,
                        source_wallets_count,
                        selected_lamports,
                        total_lamports,
                    ),
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
                summary,
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
                    inner.histories.remove(candidate.wallet.as_str());
                }
                removed |= wallet_removed;
                if lookup_miss
                    .map(lookup_miss_rank)
                    .is_none_or(|rank| lookup_miss_rank(miss) >= rank)
                {
                    lookup_miss_context = Some((miss, candidate.clone(), summary));
                }
                lookup_miss = Some(choose_lookup_miss(lookup_miss, miss));
            }
            WalletLookupOutcome::TerminalMiss {
                miss,
                removed: wallet_removed,
                summary,
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
                    inner.histories.remove(candidate.wallet.as_str());
                }
                removed |= wallet_removed;
                let selected_miss = choose_lookup_miss(lookup_miss, miss);
                let selected_candidate = if lookup_miss
                    .map(lookup_miss_rank)
                    .is_none_or(|rank| lookup_miss_rank(miss) >= rank)
                {
                    candidate
                } else {
                    lookup_miss_context
                        .as_ref()
                        .map(|(_, candidate, _)| candidate)
                        .unwrap_or(candidate)
                };
                let selected_summary = if selected_candidate.wallet == candidate.wallet {
                    summary
                } else {
                    lookup_miss_context
                        .as_ref()
                        .map(|(_, _, summary)| *summary)
                        .unwrap_or(summary)
                };
                return LookupSourceResult {
                    matched: FundingSourceMatch::Unknown,
                    removed,
                    miss: Some(selected_miss),
                    diagnostic: build_lookup_diagnostic(
                        tx,
                        &lookup_candidates,
                        Some(selected_candidate),
                        FSC_LOOKUP_RESULT_MISS,
                        selected_summary,
                        Some(selected_miss),
                        None,
                        0,
                        0,
                        0,
                    ),
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

    let selected_context = lookup_miss_context.as_ref();
    let selected_candidate = selected_context
        .map(|(_, candidate, _)| candidate)
        .or_else(|| lookup_candidates.first());
    let selected_summary = selected_context
        .map(|(_, _, summary)| *summary)
        .unwrap_or_default();
    LookupSourceResult {
        matched: FundingSourceMatch::Unknown,
        removed,
        miss: lookup_miss,
        diagnostic: build_lookup_diagnostic(
            tx,
            &lookup_candidates,
            selected_candidate,
            FSC_LOOKUP_RESULT_MISS,
            selected_summary,
            lookup_miss,
            None,
            0,
            0,
            0,
        ),
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
    let below_store_recent = inner
        .below_store_transfers
        .get(wallet)
        .is_some_and(|summary| summary.last_seen_ms >= buy_window_start);
    if let Some(history) = inner.histories.get_mut(wallet) {
        prune_transfer_history(&mut history.transfers, buy_window_start);
        let summary = wallet_lookup_summary(history, buy_event_ts_ms, below_store_recent);
        if history.transfers.is_empty() {
            return WalletLookupOutcome::ContinueMiss {
                miss: LookupMiss {
                    reason: FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON,
                    class: FscMissClass::Structural,
                },
                removed: true,
                summary,
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
                            summary,
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
                    summary,
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
                summary,
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
                summary,
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
            summary,
            source_wallets_count: selection.source_wallets_count,
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
            summary: WalletLookupSummary {
                below_store_recent,
                ..WalletLookupSummary::default()
            },
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
        summary: WalletLookupSummary {
            below_store_recent,
            ..WalletLookupSummary::default()
        },
        dust_filtered_count: 0,
        post_buy_filtered_count: 0,
        rel_too_small_count: 0,
    }
}

fn select_dominant_source(
    source_accumulators: HashMap<String, SourceAccumulator>,
    total_lamports: u128,
) -> Option<FundingAttributionSelection> {
    let source_wallets_count = source_accumulators.len() as u64;
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
            source_wallets_count,
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
    funding_lookup_wallet_candidates(tx)
        .into_iter()
        .map(|candidate| candidate.wallet)
        .collect()
}

#[must_use]
pub fn funding_lookup_wallet_candidates(tx: &PoolTransaction) -> Vec<FscLookupWalletCandidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for delta in &tx.owner_token_deltas {
        if delta.delta_raw <= 0 {
            continue;
        }
        let Some(owner) = normalize_wallet_key(&delta.owner) else {
            continue;
        };
        if seen.insert(owner.clone()) {
            candidates.push(FscLookupWalletCandidate {
                wallet: owner,
                source: FSC_LOOKUP_WALLET_SOURCE_OWNER_TOKEN_DELTA_POSITIVE.to_string(),
            });
        }
    }

    if let Some(signer) = normalize_wallet_key(&tx.signer) {
        if !seen.insert(signer.clone()) {
            return candidates;
        }
        candidates.push(FscLookupWalletCandidate {
            wallet: signer,
            source: FSC_LOOKUP_WALLET_SOURCE_SIGNER_FALLBACK.to_string(),
        });
    }

    candidates
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
        assert_eq!(config.min_total_buyers, 2);
        assert_eq!(config.min_known_non_neutral_buyers, 2);
        assert_eq!(config.min_known_coverage, 0.50);
        assert_eq!(config.min_non_neutral_known_coverage, 0.30);
        assert!(config.require_coverage_window_for_actionability);
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

        let mut changed_status_threshold = fsc_config.clone();
        changed_status_threshold.min_known_coverage = 0.75;
        let changed_status_config =
            FundingSourceConfig::from_configs(&gatekeeper_config, Some(&changed_status_threshold));
        assert_ne!(
            funding_source_config_hash(&config),
            funding_source_config_hash(&changed_status_config)
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
            creator_vault: None,
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

    #[test]
    fn lookup_candidate_prefers_positive_owner_token_delta_before_signer_fallback() {
        let tx = buy_tx_with_owner("signer-wallet", "owner-wallet", "buy-owner", 400);

        let candidates = funding_lookup_wallet_candidates(&tx);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].wallet, "owner-wallet");
        assert_eq!(
            candidates[0].source,
            FSC_LOOKUP_WALLET_SOURCE_OWNER_TOKEN_DELTA_POSITIVE
        );
        assert_eq!(candidates[1].wallet, "signer-wallet");
        assert_eq!(
            candidates[1].source,
            FSC_LOOKUP_WALLET_SOURCE_SIGNER_FALLBACK
        );
        assert_eq!(
            funding_lookup_wallets(&tx),
            vec!["owner-wallet".to_string(), "signer-wallet".to_string()]
        );
    }

    #[test]
    fn lookup_candidate_uses_signer_fallback_when_owner_delta_absent() {
        let tx = buy_tx("signer-wallet", "buy-signer", 400);

        let candidates = funding_lookup_wallet_candidates(&tx);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].wallet, "signer-wallet");
        assert_eq!(
            candidates[0].source,
            FSC_LOOKUP_WALLET_SOURCE_SIGNER_FALLBACK
        );
    }

    #[test]
    fn lookup_candidate_does_not_use_mint_pool_or_ata_accounts() {
        let mut tx = buy_tx_with_owner("signer-wallet", "owner-wallet", "buy-accounts", 400);
        tx.pool_amm_id = "pool-account".to_string();
        tx.token_mint = Some("mint-account".to_string());
        tx.associated_bonding_curve = Some("ata-account".to_string());
        tx.creator_vault = Some("creator-vault-account".to_string());
        tx.buy_remaining_accounts = vec![
            "remaining-ata-account".to_string(),
            "remaining-pool-account".to_string(),
        ];

        let wallets = funding_lookup_wallets(&tx);

        assert!(wallets.contains(&"owner-wallet".to_string()));
        assert!(wallets.contains(&"signer-wallet".to_string()));
        assert!(!wallets.contains(&"pool-account".to_string()));
        assert!(!wallets.contains(&"mint-account".to_string()));
        assert!(!wallets.contains(&"ata-account".to_string()));
        assert!(!wallets.contains(&"creator-vault-account".to_string()));
        assert!(!wallets.contains(&"remaining-ata-account".to_string()));
        assert!(!wallets.contains(&"remaining-pool-account".to_string()));
    }

    #[test]
    fn missing_funding_event_is_not_treated_as_clean_fsc_evidence() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.set_stream_available(true);

        let buys = vec![buy_tx("buyer-a", "buy-a", 400)];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.funding_source_v2.status,
            FscEvidenceStatus::Unavailable
        );
        assert_eq!(
            computed.funding_source_v2.excluded_reason,
            Some(FscExcludedReason::IndexCold)
        );
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()]
        );
        assert_eq!(computed.diagnostics.lookup_diagnostics.len(), 1);
        assert_eq!(
            computed.diagnostics.lookup_diagnostics[0].lookup_result,
            FSC_LOOKUP_RESULT_MISS
        );
    }

    #[test]
    fn store_and_lookup_use_same_wallet_key_normalization() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.set_stream_available(true);
        index.observe_transfer(
            &funding_transfer(
                " source-wallet ",
                " buyer-wallet ",
                "funding-normalized",
                200,
                100_000_000,
            ),
            &config,
        );

        let buys = vec![buy_tx(" buyer-wallet ", "buy-normalized", 400)];
        let computed = index.compute_for_transactions(buys.iter(), &config);
        let lookup = computed
            .diagnostics
            .lookup_diagnostics
            .first()
            .expect("lookup diagnostic should be emitted");

        assert_eq!(lookup.lookup_result, FSC_LOOKUP_RESULT_HIT);
        assert_eq!(lookup.lookup_wallet.as_deref(), Some("buyer-wallet"));
        assert_eq!(lookup.source_wallet.as_deref(), Some("source-wallet"));
    }

    #[test]
    fn observe_transfer_indexes_history_by_recipient_wallet_not_source_wallet() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer(
                "funding-source",
                "buyer-wallet",
                "funding-recipient",
                200,
                100_000_000,
            ),
            &config,
        );

        let buyer_buys = vec![buy_tx("buyer-wallet", "buy-recipient", 400)];
        let buyer_computed = index.compute_for_transactions(buyer_buys.iter(), &config);
        let buyer_lookup = buyer_computed
            .diagnostics
            .lookup_diagnostics
            .first()
            .expect("buyer lookup diagnostic should be emitted");
        assert_eq!(buyer_lookup.lookup_result, FSC_LOOKUP_RESULT_HIT);
        assert_eq!(buyer_lookup.lookup_wallet.as_deref(), Some("buyer-wallet"));
        assert_eq!(
            buyer_lookup.source_wallet.as_deref(),
            Some("funding-source")
        );

        let source_buys = vec![buy_tx("funding-source", "buy-source", 400)];
        let source_computed = index.compute_for_transactions(source_buys.iter(), &config);
        let source_lookup = source_computed
            .diagnostics
            .lookup_diagnostics
            .first()
            .expect("source lookup diagnostic should be emitted");
        assert_eq!(source_lookup.lookup_result, FSC_LOOKUP_RESULT_MISS);
        assert_eq!(
            source_lookup.lookup_wallet.as_deref(),
            Some("funding-source")
        );
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
            1.0,
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

        assert_eq!(computed.funding_source_concentration, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()]
        );
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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
        assert_eq!(normalized_hhi_from_counts(&[2]), Some(1.0));
        assert_approx_eq(normalized_hhi_from_counts(&[2, 2]).unwrap(), 1.0 / 3.0);
        assert_eq!(normalized_hhi_from_counts(&[3, 1]), Some(0.5));
        assert_eq!(normalized_hhi_from_counts(&[1, 1, 1]), Some(0.0));
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

        assert_eq!(computed.funding_source_concentration, Some(1.0));
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
    fn below_store_full_chain_transfer_marks_lane_warm_without_retained_history() {
        let config = config();
        let index = FundingSourceIndex::new();
        let mut transfer = funding_transfer("funder-a", "buyer-a", "fund-a", 100, 9_999);
        transfer.slot = Some(7);

        index.observe_transfer(&transfer, &config);

        assert!(index.stream_available());
        assert!(index.warmup_ready());
        assert_eq!(index.entry_count(), 0);

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 400),
            buy_tx("buyer-b", "buy-b", 500),
        ];
        let computed = index.compute_for_transactions(buys.iter(), &config);

        assert_eq!(
            computed.funding_source_v2.provider,
            FSC_V2_PROVIDER_GRPC_FULL_CHAIN
        );
        assert_eq!(
            computed.funding_source_v2.source_topics,
            vec![FSC_V2_TOPIC_GRPC_FULL_CHAIN.to_string()]
        );
        assert_eq!(
            computed.funding_source_v2.status,
            FscEvidenceStatus::Degraded
        );
        assert_eq!(
            computed.funding_source_v2.excluded_reason,
            Some(FscExcludedReason::InsufficientNonNeutralSupport)
        );
        assert_eq!(computed.diagnostics.lookup_diagnostics.len(), 2);
        let diagnostic = &computed.diagnostics.lookup_diagnostics[0];
        assert_eq!(
            diagnostic.selected_lookup_wallet.as_deref(),
            Some("buyer-a")
        );
        assert_eq!(diagnostic.buy_event_ts_ms, Some(400));
        assert_eq!(
            diagnostic.diagnostic_miss_reason.as_deref(),
            Some(FSC_DIAG_INBOUND_EXISTS_BUT_BELOW_ABS_STORE_THRESHOLD)
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
    fn compute_for_transactions_at_populates_fsc_v2_coverage_readiness() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.set_stream_available_at(true, 1_000);
        index.observe_transfer(
            &funding_transfer("funder-shared", "buyer-a", "fund-a", 1_100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("funder-shared", "buyer-b", "fund-b", 1_110, 50_000_000),
            &config,
        );

        let buys = vec![
            buy_tx("buyer-a", "buy-a", 1_200),
            buy_tx("buyer-b", "buy-b", 1_250),
        ];

        let before_window = index.compute_for_transactions_at(buys.iter(), &config, 1_999);
        assert!(!before_window.funding_source_v2.coverage_window_ready);
        assert!(!before_window.funding_source_v2.authoritative_buy_ready);
        assert_eq!(
            before_window.funding_source_v2.coverage_window_remaining_ms,
            1
        );

        let at_window = index.compute_for_transactions_at(buys.iter(), &config, 2_000);
        assert!(at_window.funding_source_v2.coverage_window_ready);
        assert!(at_window.funding_source_v2.authoritative_buy_ready);
        assert_eq!(at_window.funding_source_v2.coverage_window_remaining_ms, 0);
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
