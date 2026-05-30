use crate::events::{FundingTransferObserved, PoolTransaction};
use crate::oracle_metrics::{
    record_fsc_authoritative_funding_stream_available, record_fsc_index_entries,
    record_fsc_index_global_evictions, record_fsc_index_per_recipient_overflows,
    record_fsc_lookup_hits, record_fsc_lookup_miss_reason, record_fsc_lookup_misses,
    record_fsc_prune_duration_ms, record_fsc_warmup_ready,
};
use ghost_brain::config::GatekeeperV2Config;
use ghost_core::tx_intelligence::types::{
    FscMissClass, FundingSourceDiagnostics, FundingSourceMissReasonCount,
    FSC_BUYER_IDENTITY_UNAVAILABLE_REASON, FSC_BUY_TIMESTAMP_UNAVAILABLE_REASON,
    FSC_FUNDING_STREAM_UNAVAILABLE_REASON, FSC_GLOBAL_RECIPIENT_EVICTED_REASON,
    FSC_INSUFFICIENT_KNOWN_SOURCES_REASON, FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON,
    FSC_NO_PREBUY_TRANSFER_IN_WINDOW_REASON, FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON,
    FSC_PER_RECIPIENT_HISTORY_OVERFLOW_REASON, FSC_ROLLING_STATE_UNAVAILABLE_REASON,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingSourceConfig {
    pub lookback_window_ms: u64,
    pub dust_threshold_lamports: u64,
    pub per_recipient_cap: usize,
    pub global_recipient_cap: usize,
    neutral_funding_sources: HashSet<String>,
}

impl FundingSourceConfig {
    #[must_use]
    pub fn from_gatekeeper_config(config: &GatekeeperV2Config) -> Self {
        Self {
            lookback_window_ms: config
                .funding_lookback_window_s
                .saturating_mul(1_000)
                .max(1),
            dust_threshold_lamports: config.funding_dust_threshold_lamports,
            per_recipient_cap: config.fsc_per_recipient_cap.max(1),
            global_recipient_cap: config.fsc_global_recipient_cap.max(1),
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

#[derive(Debug, Clone, PartialEq)]
pub struct FscComputation {
    pub funding_source_concentration: Option<f64>,
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
}

#[derive(Debug, Default)]
pub struct FundingSourceIndex {
    inner: RwLock<FundingSourceInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FundingSourceMatch {
    Concrete(String),
    Neutral(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LookupSourceResult {
    matched: FundingSourceMatch,
    removed: bool,
    miss: Option<LookupMiss>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LookupMiss {
    reason: &'static str,
    class: FscMissClass,
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
        if transfer.lamports < config.dust_threshold_lamports {
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
        if transfer.full_chain_coverage {
            if !inner.availability_controlled {
                inner.stream_available = true;
                inner
                    .stream_available_since_ms
                    .get_or_insert(observation_wall_ms);
            }
            inner.saw_transfer = true;
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

    #[must_use]
    pub fn compute_for_transactions<'a>(
        &self,
        transactions: impl IntoIterator<Item = &'a PoolTransaction>,
        config: &FundingSourceConfig,
    ) -> FscComputation {
        let mut buyer_samples = unique_successful_buyers(transactions);
        buyer_samples
            .sort_by_key(|tx| (tx_event_ts_ms(tx), tx.arrival_ts_ms, tx.signature.clone()));
        let mut diagnostics = FundingSourceDiagnostics {
            buyer_sample_count: buyer_samples.len() as u64,
            ..FundingSourceDiagnostics::default()
        };

        let earliest_buy_ts_ms = buyer_samples
            .iter()
            .map(|tx| tx_event_ts_ms(tx))
            .filter(|ts| *ts > 0)
            .min()
            .unwrap_or_default();
        let window_start = earliest_buy_ts_ms.saturating_sub(config.lookback_window_ms);

        let mut inner = self.inner.write();

        if !inner.stream_available {
            return FscComputation {
                funding_source_concentration: None,
                degraded_reasons: vec![FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()],
                diagnostics,
            };
        }

        if !inner.saw_transfer {
            return FscComputation {
                funding_source_concentration: None,
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
            let matched = lookup.matched;
            if lookup.removed {
                removed_entries = removed_entries.saturating_add(1);
            }
            match matched {
                FundingSourceMatch::Concrete(source) | FundingSourceMatch::Neutral(source) => {
                    lookup_hits = lookup_hits.saturating_add(1);
                    diagnostics.known_source_count =
                        diagnostics.known_source_count.saturating_add(1);
                    known_sources.push(source);
                }
                FundingSourceMatch::Unknown => {
                    lookup_misses = lookup_misses.saturating_add(1);
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

        if known_sources.len() < 2 {
            return FscComputation {
                funding_source_concentration: None,
                degraded_reasons: vec![FSC_INSUFFICIENT_KNOWN_SOURCES_REASON.to_string()],
                diagnostics,
            };
        }

        let distinct_known_sources = known_sources.iter().collect::<HashSet<_>>().len();
        FscComputation {
            funding_source_concentration: Some(
                1.0 - (distinct_known_sources as f64 / known_sources.len() as f64),
            ),
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
        };
    }
    let buy_arrival_ts_ms = tx.arrival_ts_ms;
    let buy_window_start = buy_event_ts_ms.saturating_sub(config.lookback_window_ms);

    let mut remove_wallets = Vec::new();
    let mut matched_source = None::<(String, String)>;
    let mut lookup_miss = None::<LookupMiss>;
    for wallet in lookup_wallets {
        if let Some(history) = inner.histories.get_mut(&wallet) {
            prune_transfer_history(&mut history.transfers, buy_window_start);
            if history.transfers.is_empty() {
                lookup_miss = Some(choose_lookup_miss(
                    lookup_miss,
                    LookupMiss {
                        reason: FSC_LOOKBACK_WINDOW_EXHAUSTED_REASON,
                        class: FscMissClass::Structural,
                    },
                ));
                remove_wallets.push(wallet);
                continue;
            }

            matched_source = history
                .transfers
                .iter()
                .rev()
                .find(|transfer| {
                    transfer_precedes_buy(transfer, tx, buy_event_ts_ms, buy_arrival_ts_ms)
                })
                .map(|transfer| (wallet, transfer.source_wallet.clone()));
            if matched_source.is_some() {
                break;
            }

            let miss = if history.overflowed_before_oldest_retained {
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
            lookup_miss = Some(choose_lookup_miss(lookup_miss, miss));
        } else if inner.evicted_recipients.contains_key(&wallet) {
            lookup_miss = Some(choose_lookup_miss(
                lookup_miss,
                LookupMiss {
                    reason: FSC_GLOBAL_RECIPIENT_EVICTED_REASON,
                    class: FscMissClass::Operational,
                },
            ));
        } else {
            lookup_miss = Some(choose_lookup_miss(
                lookup_miss,
                LookupMiss {
                    reason: FSC_NO_RETAINED_RECIPIENT_HISTORY_REASON,
                    class: FscMissClass::Indeterminate,
                },
            ));
        }
    }

    let removed = !remove_wallets.is_empty();
    for wallet in remove_wallets {
        inner.histories.remove(wallet.as_str());
    }

    let Some((matched_wallet, source_wallet)) = matched_source else {
        return LookupSourceResult {
            matched: FundingSourceMatch::Unknown,
            removed,
            miss: lookup_miss,
        };
    };
    if config.is_neutral_source(&source_wallet) {
        return LookupSourceResult {
            matched: FundingSourceMatch::Neutral(format!("neutral:{matched_wallet}")),
            removed,
            miss: None,
        };
    }
    LookupSourceResult {
        matched: FundingSourceMatch::Concrete(source_wallet),
        removed,
        miss: None,
    }
}

fn transfer_precedes_buy(
    transfer: &FundingTransferRecord,
    buy: &PoolTransaction,
    buy_event_ts_ms: u64,
    buy_arrival_ts_ms: u64,
) -> bool {
    if transfer.signature == buy.signature {
        if let Some(precedes) = same_signature_transfer_precedes_buy(transfer, buy) {
            return precedes;
        }
    }

    if let (Some(transfer_slot), Some(buy_slot)) = (transfer.slot, buy.slot) {
        if transfer_slot != buy_slot {
            return transfer_slot < buy_slot;
        }
    }

    transfer.observed_at_ms < buy_event_ts_ms
        || (transfer.observed_at_ms == buy_event_ts_ms
            && transfer.arrival_ts_ms < buy_arrival_ts_ms)
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
    let mut seen = HashSet::new();
    let mut buyers = Vec::new();
    for tx in transactions {
        if !tx.is_buy || !tx.success {
            continue;
        }
        if let Some(buyer_identity) = canonical_buyer_identity(tx) {
            if seen.insert(buyer_identity) {
                buyers.push(tx);
            }
            continue;
        }
        buyers.push(tx);
    }
    buyers
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

    fn buy_tx(signer: &str, signature: &str, timestamp_ms: u64) -> PoolTransaction {
        PoolTransaction {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: "pool-1".to_string(),
            slot: Some(1),
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
            slot: Some(1),
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
    fn latest_eligible_pre_buy_transfer_wins() {
        let config = config();
        let index = FundingSourceIndex::new();
        index.observe_transfer(
            &funding_transfer("old-funder", "buyer-a", "fund-a-old", 100, 50_000_000),
            &config,
        );
        index.observe_transfer(
            &funding_transfer("shared-funder", "buyer-a", "fund-a-new", 250, 50_000_000),
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
