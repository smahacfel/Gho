use super::sybil_metrics::sybil_event_key;
use crate::events::PoolTransaction;
use crate::oracle_metrics::{
    record_cpv_index_entries, record_cpv_index_evictions, record_cpv_lookup_hits,
    record_cpv_lookup_misses,
};
use ghost_brain::config::GatekeeperV2Config;
use ghost_core::checkpoint::{CpvEvidenceContext, CpvMetricSource, MetricEvidenceQuality};
use ghost_core::tx_intelligence::types::{
    CPV_INSUFFICIENT_SIGNERS_REASON, CPV_LOW_SAMPLE_DEGRADED_REASON,
    CPV_ROLLING_STATE_UNAVAILABLE_REASON,
};
use parking_lot::RwLock;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrossPoolVelocityConfig {
    pub lookback_window_ms: u64,
    pub per_signer_cap: usize,
    pub global_signer_cap: usize,
    pub min_successful_buy_signers_clean: u64,
    pub min_successful_buy_signers_degraded: u64,
    pub emit_degraded_low_sample: bool,
}

impl CrossPoolVelocityConfig {
    #[must_use]
    pub fn from_gatekeeper_config(config: &GatekeeperV2Config) -> Self {
        Self {
            lookback_window_ms: config.cpv_lookback_window_s.saturating_mul(1_000),
            per_signer_cap: config.cpv_per_signer_cap.max(1),
            global_signer_cap: config.cpv_global_signer_cap.max(1),
            min_successful_buy_signers_clean: config.cpv_min_successful_buy_signers_clean.max(1),
            min_successful_buy_signers_degraded: config
                .cpv_min_successful_buy_signers_degraded
                .max(1)
                .min(config.cpv_min_successful_buy_signers_clean.max(1)),
            emit_degraded_low_sample: config.cpv_emit_degraded_low_sample,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpvComputation {
    pub signer_cross_pool_velocity: Option<f64>,
    pub cpv_other_pool_activity: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub signer_sample_count: u64,
    pub required_clean_sample_count: u64,
    pub required_degraded_sample_count: u64,
    pub value_source: CpvMetricSource,
    pub status: MetricEvidenceQuality,
    pub rolling_state_available: bool,
}

impl CpvComputation {
    #[must_use]
    pub fn evidence_context(&self) -> CpvEvidenceContext {
        CpvEvidenceContext {
            quality: self.status,
            source: self.value_source,
            signer_cross_pool_velocity: self.signer_cross_pool_velocity,
            cpv_other_pool_activity: self.cpv_other_pool_activity,
            sample_count: Some(self.signer_sample_count),
            required_clean_sample_count: Some(self.required_clean_sample_count),
            required_degraded_sample_count: Some(self.required_degraded_sample_count),
            rolling_state_available: Some(self.rolling_state_available),
            degraded_reasons: self.degraded_reasons.clone(),
        }
    }
}

/// Dwa jawne okna: historia innych pooli ma [anchor-lookback, anchor],
/// signerzy badanego poola [signer_window_start, anchor]. Dostępność ma osobny cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpvQueryWindow {
    pub signer_window_start_ms: u64,
    pub anchor_ms: u64,
    pub cutoff_received_ms: u64,
}

type EventKey = (ghost_core::metric_contracts::StableEventIdentityV1, u32);

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignerActivity {
    key: EventKey,
    pool_id: String,
    event_ms: u64,
    received_ms: u64,
    chain_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LossRange {
    first: Option<u64>,
    through: u64,
}
impl LossRange {
    fn record(&mut self, event_ms: u64) {
        self.first = Some(self.first.map_or(event_ms, |old| old.min(event_ms)));
        self.through = self.through.max(event_ms);
    }
    fn intersects(self, start: u64, end: u64) -> bool {
        self.first
            .is_some_and(|first| first <= end && self.through >= start)
    }
    fn expire(&mut self, floor: u64) {
        if self.first.is_some() && self.through < floor {
            *self = Self::default();
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SignerHistory {
    activities: VecDeque<SignerActivity>,
    last_seen_ms: u64,
    loss: LossRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProgressPoint {
    event_ms: u64,
    received_ms: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CrossPoolVelocityInner {
    histories: HashMap<String, SignerHistory>,
    // Jeden aktualny wpis na signera, również po wielokrotnych aktualizacjach.
    signer_order: BTreeSet<(u64, String)>,
    // Dokładnie jeden klucz na zachowaną activity; limit wynika z istniejących capów.
    event_index: HashMap<EventKey, String>,
    config: Option<CrossPoolVelocityConfig>,
    epoch: u64,
    continuous_since: Option<ProgressPoint>,
    progress: VecDeque<ProgressPoint>,
    progress_high_water_ms: u64,
    retained_from_ms: u64,
    gap_received_floor_ms: Option<u64>,
    global_loss: LossRange,
}

#[derive(Debug, Default)]
pub struct CrossPoolVelocityIndex {
    inner: RwLock<CrossPoolVelocityInner>,
}

impl CrossPoolVelocityIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Jedyne produkcyjne zasilanie, wywoływane przed wyborem pooli w Oracle.
    pub(crate) fn observe_feed_event(
        &self,
        event: &crate::events::CpvFeedEvent,
        available_ms: u64,
        config: &CrossPoolVelocityConfig,
    ) {
        use crate::events::CpvFeedEventKind;
        match &event.0 {
            CpvFeedEventKind::Trade(tx) => {
                self.observe_transaction_at(&tx.pool_amm_id, tx, available_ms, config)
            }
            CpvFeedEventKind::Progress(progress) if !progress.gap => self.observe_source_progress(
                progress.epoch,
                progress.event_ms,
                progress.received_ms,
                available_ms,
                config,
            ),
            CpvFeedEventKind::Progress(progress) => self.mark_stream_gap(progress.received_ms),
            CpvFeedEventKind::Gap { received_ms } => self.mark_stream_gap(*received_ms),
        }
    }

    /// Wyłącznie jawny postęp obsługiwanego feedu, po przetworzeniu wcześniejszych
    /// rekordów. Zegar ani pojedynczy BUY nie otwierają pełnego pokrycia.
    pub fn observe_source_progress(
        &self,
        epoch: u64,
        event_ms: u64,
        source_received_ms: u64,
        available_received_ms: u64,
        config: &CrossPoolVelocityConfig,
    ) {
        let mut inner = self.inner.write();
        configure(&mut inner, config);
        if epoch == 0
            || source_received_ms == 0
            || available_received_ms < source_received_ms
            || event_ms > available_received_ms
        {
            return;
        }
        if epoch < inner.epoch {
            return;
        }
        if epoch > inner.epoch {
            inner.epoch = epoch;
            inner.continuous_since = None;
            inner.progress.clear();
        }
        if inner
            .gap_received_floor_ms
            .is_some_and(|floor| source_received_ms <= floor)
        {
            return;
        }
        if inner
            .progress
            .back()
            .is_some_and(|p| event_ms < p.event_ms || available_received_ms < p.received_ms)
        {
            return;
        }
        let point = ProgressPoint {
            event_ms,
            received_ms: available_received_ms,
        };
        inner.continuous_since.get_or_insert(point);
        inner.progress_high_water_ms = inner.progress_high_water_ms.max(event_ms);
        if !inner.progress.back().is_some_and(|last| *last == point) {
            inner.progress.push_back(point);
        }
        while inner.progress.len() > config.per_signer_cap.max(1) {
            inner.progress.pop_front();
        }
        // Jawnie wspierana historia: ostatnie dwa lookbacki. Nie zwiększa to
        // capów pamięci; older anchors bez danych otrzymują niedostępność.
        inner.retained_from_ms = inner
            .progress_high_water_ms
            .saturating_sub(config.lookback_window_ms.max(1).saturating_mul(2));
        prune_locked(&mut inner, config);
    }

    /// Luka/lag/restart nie korzysta z lane FSC. Nie usuwa zapisanych MFS.
    pub fn mark_stream_gap(&self, received_ms: u64) {
        let mut inner = self.inner.write();
        inner.continuous_since = None;
        inner.progress.clear();
        inner.gap_received_floor_ms =
            Some(inner.gap_received_floor_ms.unwrap_or(0).max(received_ms));
    }

    pub fn observe_transaction(
        &self,
        current_pool_id: &str,
        tx: &PoolTransaction,
        config: &CrossPoolVelocityConfig,
    ) {
        self.observe_transaction_at(
            current_pool_id,
            tx,
            tx.event_time.ingress_wall_ts_ms.unwrap_or_default(),
            config,
        );
    }

    /// Odbiorca runtime podaje czas dotarcia do CPV, nie wcześniejszy czas raw
    /// pakietu. Pozwala to odciąć również opóźnienie IPC/Event Bus.
    pub fn observe_transaction_at(
        &self,
        current_pool_id: &str,
        tx: &PoolTransaction,
        available_received_ms: u64,
        config: &CrossPoolVelocityConfig,
    ) {
        if !tx.is_buy || (tx.metadata_availability.status_known && !tx.success) {
            return;
        }
        let (Some(key), Some(event_ms), Some(raw_received)) = (
            sybil_event_key(tx),
            tx_event_ts_ms(tx),
            tx.event_time.ingress_wall_ts_ms,
        ) else {
            self.mark_stream_gap(available_received_ms);
            return;
        };
        if !tx.is_confirmed_success()
            || current_pool_id != tx.pool_amm_id
            || current_pool_id.is_empty()
            || tx.signer.is_empty()
            || raw_received == 0
            || available_received_ms < raw_received
        {
            self.mark_stream_gap(available_received_ms);
            return;
        }
        self.observe_activity(
            key,
            current_pool_id,
            &tx.signer,
            event_ms,
            available_received_ms,
            tx.event_time.chain_event_ts_ms,
            config,
        );
    }

    /// Kontrolowane zasilenie historyczne/fixture bez raw tożsamości.
    /// Nie ustanawia pokrycia źródła i nie jest wywoływane przez live runtime.
    #[doc(hidden)]
    pub fn observe_buy(
        &self,
        pool: &str,
        signer: &str,
        event_ms: u64,
        config: &CrossPoolVelocityConfig,
    ) {
        use solana_sdk::{hash::hashv, signature::Signature};
        let hash = hashv(&[pool.as_bytes(), signer.as_bytes(), &event_ms.to_le_bytes()]).to_bytes();
        let mut signature = [0u8; 64];
        signature[..32].copy_from_slice(&hash);
        signature[32..].copy_from_slice(&hash);
        let identity = ghost_core::metric_contracts::StableEventIdentityV1::try_from_signature(
            "cpv_explicit_fixture",
            &Signature::from(signature).to_string(),
        )
        .expect("valid signature");
        self.observe_activity(
            (identity, 0),
            pool,
            signer,
            event_ms,
            event_ms,
            Some(event_ms),
            config,
        );
    }

    fn observe_activity(
        &self,
        key: EventKey,
        pool: &str,
        signer: &str,
        event_ms: u64,
        received_ms: u64,
        chain_ms: Option<u64>,
        config: &CrossPoolVelocityConfig,
    ) {
        if pool.is_empty() || signer.is_empty() || event_ms == 0 || received_ms == 0 {
            return;
        }
        let mut inner = self.inner.write();
        configure(&mut inner, config);
        if event_ms < inner.retained_from_ms {
            return;
        }
        if let Some(existing_signer) = inner.event_index.get(&key).cloned() {
            if let Some(existing) = inner
                .histories
                .get_mut(&existing_signer)
                .and_then(|h| h.activities.iter_mut().find(|a| a.key == key))
            {
                if existing_signer != signer
                    || existing.pool_id != pool
                    || existing.chain_ms.zip(chain_ms).is_some_and(|(a, b)| a != b)
                {
                    let old_time = existing.event_ms;
                    inner.global_loss.record(old_time);
                    inner.global_loss.record(event_ms);
                } else if received_ms < existing.received_ms {
                    // Ta sama tożsamość: najwcześniejsza dostępna obserwacja,
                    // nigdy odświeżanie TTL kolejną dostawą.
                    existing.received_ms = received_ms;
                    existing.event_ms = event_ms;
                    existing.chain_ms = chain_ms;
                }
            }
            return;
        }
        let mut history = inner.histories.remove(signer).unwrap_or_default();
        inner
            .signer_order
            .remove(&(history.last_seen_ms, signer.to_string()));
        history.activities.push_back(SignerActivity {
            key: key.clone(),
            pool_id: pool.to_string(),
            event_ms,
            received_ms,
            chain_ms,
        });
        inner.event_index.insert(key, signer.to_string());
        while history.activities.len() > config.per_signer_cap.max(1) {
            let oldest = history
                .activities
                .iter()
                .enumerate()
                .min_by_key(|(_, a)| (a.event_ms, a.received_ms, a.pool_id.as_str()))
                .map(|(i, _)| i)
                .expect("nonempty activity");
            let lost = history
                .activities
                .remove(oldest)
                .expect("existing activity");
            history.loss.record(lost.event_ms);
            inner.event_index.remove(&lost.key);
        }
        history.last_seen_ms = history.last_seen_ms.max(event_ms);
        inner
            .signer_order
            .insert((history.last_seen_ms, signer.to_string()));
        inner.histories.insert(signer.to_string(), history);
        enforce_global_cap(&mut inner, config);
    }

    /// Kompatybilny jawny odczyt historyczny: cutoff=anchor, signer window=lookback.
    #[must_use]
    pub fn compute_for_transactions<'a>(
        &self,
        current_pool_id: &str,
        transactions: impl IntoIterator<Item = &'a PoolTransaction>,
        anchor_ts_ms: Option<u64>,
        config: &CrossPoolVelocityConfig,
    ) -> CpvComputation {
        let anchor_ms = anchor_ts_ms.unwrap_or_default();
        self.compute_for_transactions_at(
            current_pool_id,
            transactions,
            CpvQueryWindow {
                signer_window_start_ms: anchor_ms.saturating_sub(config.lookback_window_ms.max(1)),
                anchor_ms,
                cutoff_received_ms: anchor_ms,
            },
            config,
        )
    }

    #[must_use]
    pub fn compute_for_transactions_at<'a>(
        &self,
        pool: &str,
        transactions: impl IntoIterator<Item = &'a PoolTransaction>,
        window: CpvQueryWindow,
        config: &CrossPoolVelocityConfig,
    ) -> CpvComputation {
        let (signers, input_incomplete) = qualified_signers(pool, transactions, window);
        let count = signers.len() as u64;
        let clean_min = config.min_successful_buy_signers_clean.max(1);
        let degraded_min = config
            .min_successful_buy_signers_degraded
            .max(1)
            .min(clean_min);
        let start = window
            .anchor_ms
            .saturating_sub(config.lookback_window_ms.max(1));
        let mut reasons = Vec::new();
        let inner = self.inner.read(); // Żadnego prune, zapisu ani usuwania w odczycie.
        let proof = inner
            .progress
            .iter()
            .rev()
            .find(|p| p.received_ms <= window.cutoff_received_ms);
        let ready = window.anchor_ms > 0
            && window.signer_window_start_ms <= window.anchor_ms
            && inner
                .config
                .as_ref()
                .is_some_and(|stored| history_config_matches(stored, config))
            && inner.continuous_since.is_some_and(|p| {
                p.event_ms <= start.min(window.signer_window_start_ms)
                    && p.received_ms <= window.cutoff_received_ms
            })
            && proof.is_some_and(|p| p.event_ms >= window.anchor_ms)
            && start >= inner.retained_from_ms;
        if !ready {
            reasons.push(CPV_ROLLING_STATE_UNAVAILABLE_REASON.to_string());
        }
        if start < inner.retained_from_ms {
            reasons.push("CPV_HISTORY_NOT_RETAINED".to_string());
        }
        if input_incomplete {
            reasons.push("CPV_QUERY_INPUT_UNAVAILABLE".to_string());
        }
        let loss = inner.global_loss.intersects(start, window.anchor_ms)
            || signers.iter().any(|signer| {
                inner
                    .histories
                    .get(signer)
                    .is_some_and(|h| h.loss.intersects(start, window.anchor_ms))
            });
        if loss {
            reasons.push("CPV_HISTORY_LOST".to_string());
        }
        let low_sample =
            count < clean_min && config.emit_degraded_low_sample && count >= degraded_min;
        if count < clean_min {
            reasons.push(
                if ready && !loss && !input_incomplete && low_sample {
                    CPV_LOW_SAMPLE_DEGRADED_REASON
                } else {
                    CPV_INSUFFICIENT_SIGNERS_REASON
                }
                .to_string(),
            );
        }
        let usable =
            ready && !loss && !input_incomplete && (count >= clean_min || low_sample) && count > 0;
        let mut cross = 0u64;
        let mut other_activity = 0u64;
        let mut hits = 0u64;
        if usable {
            for signer in &signers {
                let pools: HashSet<_> = inner
                    .histories
                    .get(signer)
                    .into_iter()
                    .flat_map(|h| h.activities.iter())
                    .filter(|a| {
                        a.event_ms >= start
                            && a.event_ms <= window.anchor_ms
                            && a.received_ms <= window.cutoff_received_ms
                            && a.pool_id != pool
                    })
                    .map(|a| a.pool_id.as_str())
                    .collect();
                if inner.histories.contains_key(signer) {
                    hits += 1;
                }
                if !pools.is_empty() {
                    cross += 1;
                }
                other_activity += pools.len() as u64;
            }
        }
        if hits > 0 {
            record_cpv_lookup_hits(hits);
        }
        if usable && count > hits {
            record_cpv_lookup_misses(count - hits);
        }
        CpvComputation {
            signer_cross_pool_velocity: usable.then(|| cross as f64 / count as f64),
            cpv_other_pool_activity: usable.then(|| other_activity as f64 / count as f64),
            degraded_reasons: reasons,
            signer_sample_count: count,
            required_clean_sample_count: clean_min,
            required_degraded_sample_count: degraded_min,
            value_source: if usable {
                CpvMetricSource::SuccessfulBuyRollingIndex
            } else {
                CpvMetricSource::Unavailable
            },
            status: if !ready || loss || input_incomplete {
                MetricEvidenceQuality::UnavailableSource
            } else if !usable {
                MetricEvidenceQuality::InsufficientSample
            } else if low_sample {
                MetricEvidenceQuality::DegradedLowSample
            } else {
                MetricEvidenceQuality::Clean
            },
            rolling_state_available: ready && !loss,
        }
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        let inner = self.inner.read();
        inner.config.as_ref().is_some_and(|config| {
            inner
                .continuous_since
                .zip(inner.progress.back().copied())
                .is_some_and(|(start, last)| {
                    last.event_ms.saturating_sub(start.event_ms) >= config.lookback_window_ms.max(1)
                })
        })
    }
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.inner.read().histories.len()
    }
}

fn history_config_matches(a: &CrossPoolVelocityConfig, b: &CrossPoolVelocityConfig) -> bool {
    // Minimalna liczność jest polityką odczytu, nie inną historią feedu.
    a.lookback_window_ms == b.lookback_window_ms
        && a.per_signer_cap == b.per_signer_cap
        && a.global_signer_cap == b.global_signer_cap
}

fn configure(inner: &mut CrossPoolVelocityInner, config: &CrossPoolVelocityConfig) {
    if inner
        .config
        .as_ref()
        .is_some_and(|old| !history_config_matches(old, config))
    {
        *inner = CrossPoolVelocityInner::default();
    }
    inner.config = Some(*config);
}

fn prune_locked(inner: &mut CrossPoolVelocityInner, config: &CrossPoolVelocityConfig) {
    let floor = inner.retained_from_ms;
    // Retain sprawdza każdy element, również dostawę poza kolejnością na końcu.
    let mut remove = Vec::new();
    for (signer, history) in &mut inner.histories {
        history.activities.retain(|a| {
            if a.event_ms < floor {
                inner.event_index.remove(&a.key);
                false
            } else {
                true
            }
        });
        history.loss.expire(floor);
        if history.activities.is_empty() && history.loss.first.is_none() {
            remove.push(signer.clone());
        }
    }
    for signer in remove {
        if let Some(history) = inner.histories.remove(&signer) {
            inner.signer_order.remove(&(history.last_seen_ms, signer));
        }
    }
    inner.global_loss.expire(floor);
    enforce_global_cap(inner, config);
}

fn enforce_global_cap(inner: &mut CrossPoolVelocityInner, config: &CrossPoolVelocityConfig) {
    let mut evictions = 0;
    while inner.histories.len() > config.global_signer_cap.max(1) {
        let Some((_, signer)) = inner.signer_order.pop_first() else {
            break;
        };
        if let Some(history) = inner.histories.remove(&signer) {
            for activity in history.activities {
                inner.event_index.remove(&activity.key);
                inner.global_loss.record(activity.event_ms);
            }
            if let Some(first) = history.loss.first {
                inner.global_loss.record(first);
                inner.global_loss.record(history.loss.through);
            }
            evictions += 1;
        }
    }
    if evictions > 0 {
        record_cpv_index_evictions(evictions);
    }
    record_cpv_index_entries(inner.histories.len());
}

fn qualified_signers<'a>(
    pool: &str,
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
    window: CpvQueryWindow,
) -> (BTreeSet<String>, bool) {
    let mut groups = HashMap::<EventKey, Vec<&PoolTransaction>>::new();
    let mut incomplete = false;
    for tx in transactions {
        if tx.pool_amm_id != pool {
            continue;
        }
        let could_be_buy = tx.is_buy && (!tx.metadata_availability.status_known || tx.success);
        match tx.event_time.ingress_wall_ts_ms {
            Some(received) if received > window.cutoff_received_ms => continue,
            Some(_) => {}
            None => {
                incomplete |= could_be_buy;
                continue;
            }
        }
        let Some(time) = tx_event_ts_ms(tx) else {
            incomplete |= could_be_buy;
            continue;
        };
        if time < window.signer_window_start_ms || time > window.anchor_ms {
            continue;
        }
        let Some(key) = sybil_event_key(tx) else {
            incomplete |= could_be_buy;
            continue;
        };
        groups.entry(key).or_default().push(tx);
    }
    let mut signers = BTreeSet::new();
    for views in groups.values() {
        let Some(first) = views.iter().find(|v| v.is_buy && v.is_confirmed_success()) else {
            incomplete |= views
                .iter()
                .any(|v| v.is_buy && !v.metadata_availability.status_known);
            continue;
        };
        if first.signer.is_empty()
            || views.iter().any(|v| {
                v.signer != first.signer
                    || !v.is_buy
                    || (v.metadata_availability.status_known && !v.success)
            })
        {
            incomplete = true;
            continue;
        }
        signers.insert(first.signer.clone());
    }
    (signers, incomplete)
}

fn tx_event_ts_ms(tx: &PoolTransaction) -> Option<u64> {
    tx.event_time
        .compat_event_ts_ms(None)
        .filter(|time| *time > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RawBytesMissingReason;
    use ghost_core::{CurveFinality, EventSemanticEnvelope};

    fn test_config() -> CrossPoolVelocityConfig {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.cpv_lookback_window_s = 1;
        gatekeeper_config.cpv_per_signer_cap = 8;
        gatekeeper_config.cpv_global_signer_cap = 8;
        CrossPoolVelocityConfig::from_gatekeeper_config(&gatekeeper_config)
    }

    fn tx(pool_id: &str, signer: &str, signature: &str, timestamp_ms: u64) -> PoolTransaction {
        let digest = solana_sdk::hash::hash(signature.as_bytes()).to_bytes();
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(&digest);
        bytes[32..].copy_from_slice(&digest);
        let signature = solana_sdk::signature::Signature::from(bytes).to_string();
        PoolTransaction {
            metadata_availability: seer::types::TransactionMetadataAvailability {
                status_known: true,
                inner_instructions_known: true,
            },
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            tx_index: None,
            outer_instruction_index: None,
            inner_instruction_path: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
            arrival_ts_ms: timestamp_ms,
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
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
            virtual_sol_reserves: None,
            virtual_token_reserves: None,
            real_sol_reserves: None,
            real_token_reserves: None,
            complete: None,
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
            signer_pre_balance_lamports: Some(1_000_000_000),
            signer_post_balance_lamports: Some(900_000_000),
            jito_tip_detected: None,
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: CurveFinality::Speculative,
        }
    }

    fn tx_with_flags(
        pool_id: &str,
        signer: &str,
        signature: &str,
        timestamp_ms: u64,
        is_buy: bool,
        success: bool,
    ) -> PoolTransaction {
        let mut tx = tx(pool_id, signer, signature, timestamp_ms);
        tx.is_buy = is_buy;
        tx.success = success;
        tx
    }

    #[test]
    fn from_gatekeeper_config_maps_successful_buy_sample_policy() {
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.cpv_min_successful_buy_signers_clean = 4;
        gatekeeper_config.cpv_min_successful_buy_signers_degraded = 2;
        gatekeeper_config.cpv_emit_degraded_low_sample = true;

        let config = CrossPoolVelocityConfig::from_gatekeeper_config(&gatekeeper_config);

        assert_eq!(config.min_successful_buy_signers_clean, 4);
        assert_eq!(config.min_successful_buy_signers_degraded, 2);
        assert!(config.emit_degraded_low_sample);
    }

    fn warm_source(index: &CrossPoolVelocityIndex, config: &CrossPoolVelocityConfig, anchor: u64) {
        index.observe_source_progress(1, 0, 1, 1, config);
        index.observe_source_progress(1, anchor, anchor, anchor, config);
    }

    #[test]
    fn cross_pool_signer_raises_cpv() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-a", "shared", 100, &config);
        index.observe_buy("pool-b", "shared", 200, &config);
        index.observe_buy("pool-b", "local-a", 210, &config);
        index.observe_buy("pool-b", "local-b", 220, &config);

        let current = vec![
            tx("pool-b", "shared", "sig-shared", 220),
            tx("pool-b", "local-a", "sig-local-a", 230),
            tx("pool-b", "local-b", "sig-local-b", 240),
        ];
        warm_source(&index, &config, 240);
        let computed = index.compute_for_transactions("pool-b", current.iter(), Some(240), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(1.0 / 3.0));
        assert_eq!(computed.cpv_other_pool_activity, Some(1.0 / 3.0));
        assert!(computed.degraded_reasons.is_empty());
        assert_eq!(computed.signer_sample_count, 3);
        assert_eq!(computed.status, MetricEvidenceQuality::Clean);
        assert_eq!(computed.required_clean_sample_count, 3);
    }

    #[test]
    fn local_only_signers_keep_cpv_at_zero() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-a", "signer-a", 100, &config);
        index.observe_buy("pool-a", "signer-b", 200, &config);
        index.observe_buy("pool-a", "signer-c", 300, &config);

        let current = vec![
            tx("pool-a", "signer-a", "sig-a", 310),
            tx("pool-a", "signer-b", "sig-b", 320),
            tx("pool-a", "signer-c", "sig-c", 330),
        ];
        warm_source(&index, &config, 330);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(330), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(0.0));
        assert_eq!(computed.cpv_other_pool_activity, Some(0.0));
        assert!(computed.degraded_reasons.is_empty());
        assert_eq!(computed.status, MetricEvidenceQuality::Clean);
    }

    #[test]
    fn stale_history_expires_out_of_lookback_window() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-z", "shared", 100, &config);
        index.observe_buy("pool-a", "shared", 1_150, &config);
        index.observe_buy("pool-a", "signer-b", 1_160, &config);
        index.observe_buy("pool-a", "signer-c", 1_170, &config);

        let current = vec![
            tx("pool-a", "shared", "sig-shared", 1_180),
            tx("pool-a", "signer-b", "sig-b", 1_190),
            tx("pool-a", "signer-c", "sig-c", 1_200),
        ];
        warm_source(&index, &config, 1_200);
        let computed =
            index.compute_for_transactions("pool-a", current.iter(), Some(1_200), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(0.0));
        assert!(computed.degraded_reasons.is_empty());
        assert_eq!(computed.status, MetricEvidenceQuality::Clean);
    }

    #[test]
    fn global_signer_cap_evicts_oldest_entries() {
        let index = CrossPoolVelocityIndex::new();
        let mut config = test_config();
        config.global_signer_cap = 2;

        index.observe_buy("pool-a", "signer-a", 100, &config);
        index.observe_buy("pool-a", "signer-b", 200, &config);
        index.observe_buy("pool-a", "signer-c", 300, &config);

        let inner = index.inner.read();
        assert_eq!(inner.histories.len(), 2);
        assert!(!inner.histories.contains_key("signer-a"));
        assert!(inner.histories.contains_key("signer-b"));
        assert!(inner.histories.contains_key("signer-c"));
    }

    #[test]
    fn cold_index_reports_rolling_state_unavailable() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();
        let current = vec![
            tx("pool-a", "signer-a", "sig-a", 100),
            tx("pool-a", "signer-b", "sig-b", 110),
            tx("pool-a", "signer-c", "sig-c", 120),
        ];

        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(120), &config);

        assert_eq!(computed.signer_cross_pool_velocity, None);
        assert_eq!(computed.status, MetricEvidenceQuality::UnavailableSource);
        assert_eq!(computed.rolling_state_available, false);
        assert_eq!(
            computed.degraded_reasons,
            vec![CPV_ROLLING_STATE_UNAVAILABLE_REASON.to_string()]
        );
    }

    #[test]
    fn insufficient_signers_degrades_even_when_index_is_warm() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-z", "shared", 100, &config);

        let current = vec![
            tx("pool-a", "shared", "sig-shared", 110),
            tx("pool-a", "local-a", "sig-local-a", 120),
        ];
        warm_source(&index, &config, 120);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(120), &config);

        assert_eq!(computed.signer_cross_pool_velocity, None);
        assert_eq!(computed.status, MetricEvidenceQuality::InsufficientSample);
        assert_eq!(computed.signer_sample_count, 2);
        assert_eq!(
            computed.degraded_reasons,
            vec![CPV_INSUFFICIENT_SIGNERS_REASON.to_string()]
        );
    }

    #[test]
    fn one_successful_buy_signer_is_insufficient_sample() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-z", "shared", 100, &config);
        index.observe_buy("pool-a", "shared", 110, &config);

        let current = vec![tx("pool-a", "shared", "sig-shared", 120)];
        warm_source(&index, &config, 120);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(120), &config);

        assert_eq!(computed.signer_cross_pool_velocity, None);
        assert_eq!(computed.cpv_other_pool_activity, None);
        assert_eq!(computed.signer_sample_count, 1);
        assert_eq!(computed.status, MetricEvidenceQuality::InsufficientSample);
        assert_eq!(
            computed.degraded_reasons,
            vec![CPV_INSUFFICIENT_SIGNERS_REASON.to_string()]
        );
    }

    #[test]
    fn two_successful_buy_signers_do_not_emit_degraded_by_default() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-z", "shared", 100, &config);
        index.observe_buy("pool-a", "shared", 110, &config);
        index.observe_buy("pool-a", "local", 115, &config);

        let current = vec![
            tx("pool-a", "shared", "sig-shared", 120),
            tx("pool-a", "local", "sig-local", 121),
        ];
        warm_source(&index, &config, 121);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(121), &config);

        assert_eq!(computed.signer_cross_pool_velocity, None);
        assert_eq!(computed.cpv_other_pool_activity, None);
        assert_eq!(computed.signer_sample_count, 2);
        assert_eq!(computed.status, MetricEvidenceQuality::InsufficientSample);
        assert_eq!(computed.required_clean_sample_count, 3);
        assert_eq!(computed.required_degraded_sample_count, 2);
        assert_eq!(
            computed.degraded_reasons,
            vec![CPV_INSUFFICIENT_SIGNERS_REASON.to_string()]
        );
    }

    #[test]
    fn two_successful_buy_signers_emit_degraded_when_config_allows() {
        let index = CrossPoolVelocityIndex::new();
        let mut config = test_config();
        config.emit_degraded_low_sample = true;

        index.observe_buy("pool-z", "shared", 100, &config);
        index.observe_buy("pool-a", "shared", 110, &config);
        index.observe_buy("pool-a", "local", 115, &config);

        let current = vec![
            tx("pool-a", "shared", "sig-shared", 120),
            tx("pool-a", "local", "sig-local", 121),
        ];
        warm_source(&index, &config, 121);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(121), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(0.5));
        assert_eq!(computed.cpv_other_pool_activity, Some(0.5));
        assert_eq!(computed.signer_sample_count, 2);
        assert_eq!(computed.status, MetricEvidenceQuality::DegradedLowSample);
        assert_eq!(
            computed.value_source,
            CpvMetricSource::SuccessfulBuyRollingIndex
        );
        assert_eq!(
            computed.degraded_reasons,
            vec![CPV_LOW_SAMPLE_DEGRADED_REASON.to_string()]
        );

        let evidence = computed.evidence_context();
        assert_eq!(evidence.quality, MetricEvidenceQuality::DegradedLowSample);
        assert_eq!(evidence.sample_count, Some(2));
        assert_eq!(evidence.required_clean_sample_count, Some(3));
        assert_eq!(evidence.required_degraded_sample_count, Some(2));
        assert_eq!(evidence.rolling_state_available, Some(true));
    }

    #[test]
    fn failed_and_sell_only_transactions_do_not_increase_cpv_sample() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-z", "successful", 100, &config);
        index.observe_buy("pool-a", "successful", 110, &config);
        let failed_buy = tx_with_flags("pool-a", "failed-buy", "sig-failed-buy", 121, true, false);
        let sell_only = tx_with_flags("pool-a", "sell-only", "sig-sell", 122, false, true);
        let failed_sell = tx_with_flags(
            "pool-a",
            "failed-sell",
            "sig-failed-sell",
            123,
            false,
            false,
        );
        index.observe_transaction("pool-a", &failed_buy, &config);
        index.observe_transaction("pool-a", &sell_only, &config);
        index.observe_transaction("pool-a", &failed_sell, &config);

        let current = vec![
            tx("pool-a", "successful", "sig-success", 120),
            failed_buy,
            sell_only,
            failed_sell,
        ];
        warm_source(&index, &config, 123);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(123), &config);

        assert_eq!(computed.signer_sample_count, 1);
        assert_eq!(computed.signer_cross_pool_velocity, None);
        assert_eq!(computed.status, MetricEvidenceQuality::InsufficientSample);
        assert_eq!(
            computed.degraded_reasons,
            vec![CPV_INSUFFICIENT_SIGNERS_REASON.to_string()]
        );
    }

    #[test]
    fn future_other_pool_activity_does_not_leak_into_anchor_cpv() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-a", "shared", 100, &config);
        index.observe_buy("pool-a", "local-a", 110, &config);
        index.observe_buy("pool-a", "local-b", 120, &config);
        index.observe_buy("pool-z", "shared", 200, &config);

        let current = vec![
            tx("pool-a", "shared", "sig-shared", 100),
            tx("pool-a", "local-a", "sig-local-a", 110),
            tx("pool-a", "local-b", "sig-local-b", 120),
        ];
        warm_source(&index, &config, 120);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(120), &config);

        assert_eq!(computed.signer_sample_count, 3);
        assert_eq!(computed.signer_cross_pool_velocity, Some(0.0));
        assert_eq!(computed.cpv_other_pool_activity, Some(0.0));
        assert_eq!(computed.status, MetricEvidenceQuality::Clean);
    }

    #[test]
    fn repeated_buys_count_unique_signers_once() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();

        index.observe_buy("pool-z", "shared", 100, &config);
        index.observe_buy("pool-a", "shared", 110, &config);
        index.observe_buy("pool-a", "local-a", 120, &config);
        index.observe_buy("pool-a", "local-b", 130, &config);

        let current = vec![
            tx("pool-a", "shared", "sig-shared-1", 140),
            tx("pool-a", "shared", "sig-shared-2", 150),
            tx("pool-a", "local-a", "sig-local-a", 160),
            tx("pool-a", "local-b", "sig-local-b", 170),
        ];
        warm_source(&index, &config, 170);
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(170), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(1.0 / 3.0));
        assert_eq!(computed.cpv_other_pool_activity, Some(1.0 / 3.0));
        assert_eq!(computed.signer_sample_count, 3);
        assert_eq!(computed.status, MetricEvidenceQuality::Clean);
    }
    mod m4_audit {
        use super::*;
        fn three_buyers() -> Vec<PoolTransaction> {
            ["a", "b", "c"]
                .iter()
                .map(|s| tx("local", s, s, 2000))
                .collect()
        }
        #[test]
        fn cpv_late_expired_activity_must_not_count() {
            let index = CrossPoolVelocityIndex::new();
            let config = test_config();
            for s in ["a", "b", "c"] {
                index.observe_buy("local", s, 1900, &config);
            }
            index.observe_buy("other", "a", 100, &config);
            warm_source(&index, &config, 2000);
            let result =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2000), &config);
            println!("TTL: {result:?}");
            assert_eq!(result.signer_cross_pool_velocity, Some(0.0));
        }
        #[test]
        fn cpv_history_overflow_must_not_certify_false_zero() {
            let index = CrossPoolVelocityIndex::new();
            let mut config = test_config();
            config.per_signer_cap = 2;
            index.observe_buy("other", "a", 1500, &config);
            for s in ["a", "b", "c"] {
                index.observe_buy("local", s, 1600, &config);
            }
            warm_source(&index, &config, 2000);
            let before =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2000), &config);
            index.observe_buy("local", "a", 1700, &config);
            let after =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2000), &config);
            println!("CAP: before={before:?}; after={after:?}");
            assert_eq!(before.signer_cross_pool_velocity, Some(1.0 / 3.0));
            assert_eq!(before.status, MetricEvidenceQuality::Clean);
            assert_eq!(after.signer_cross_pool_velocity, None);
            assert_eq!(after.status, MetricEvidenceQuality::UnavailableSource);
            assert!(after
                .degraded_reasons
                .contains(&"CPV_HISTORY_LOST".to_string()));
        }
        #[test]
        fn cpv_global_eviction_must_not_certify_false_zero() {
            let index = CrossPoolVelocityIndex::new();
            let mut config = test_config();
            config.global_signer_cap = 1;
            index.observe_buy("other", "a", 1500, &config);
            index.observe_buy("local", "b", 1600, &config);
            index.observe_buy("local", "c", 1700, &config);
            warm_source(&index, &config, 2000);
            let result =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2000), &config);
            println!("GLOBAL CAP: {result:?}");
            assert_eq!(result.signer_cross_pool_velocity, None);
            assert_eq!(result.status, MetricEvidenceQuality::UnavailableSource);
            assert!(result
                .degraded_reasons
                .contains(&"CPV_HISTORY_LOST".to_string()));
        }
        #[test]
        fn cpv_just_started_index_does_not_prove_full_window_coverage() {
            let index = CrossPoolVelocityIndex::new();
            let config = test_config();
            for observed in three_buyers() {
                index.observe_transaction("local", &observed, &config);
            }
            let result =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2000), &config);
            println!("COVERAGE: {result:?}");
            assert_eq!(result.signer_cross_pool_velocity, None);
            assert_eq!(result.status, MetricEvidenceQuality::UnavailableSource);
            assert!(result
                .degraded_reasons
                .contains(&CPV_ROLLING_STATE_UNAVAILABLE_REASON.to_string()));
        }
        #[test]
        fn cpv_metadata_queue_must_be_bounded_by_state_budget() {
            let index = CrossPoolVelocityIndex::new();
            let mut config = test_config();
            config.lookback_window_ms = 300_000;
            config.per_signer_cap = 1;
            config.global_signer_cap = 1;
            for t in 1..=1000 {
                index.observe_buy("local", "a", t, &config);
            }
            let inner = index.inner.read();
            println!(
                "MEMORY: histories={} activities={} queue={}",
                inner.histories.len(),
                inner.histories["a"].activities.len(),
                inner.signer_order.len()
            );
            assert_eq!(inner.histories.len(), 1);
            assert_eq!(inner.histories["a"].activities.len(), 1);
            assert_eq!(inner.signer_order.len(), 1);
            assert_eq!(inner.event_index.len(), 1);
        }
        #[test]
        fn cpv_later_query_must_not_silently_change_older_snapshot() {
            let index = CrossPoolVelocityIndex::new();
            let config = test_config();
            index.observe_buy("other", "a", 1500, &config);
            for s in ["a", "b", "c"] {
                index.observe_buy("local", s, 1900, &config);
            }
            warm_source(&index, &config, 2000);
            let before =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2000), &config);
            for s in ["a", "b", "c"] {
                index.observe_buy("local", s, 2100, &config);
            }
            index.observe_source_progress(1, 2600, 2600, 2600, &config);
            let _later =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2600), &config);
            let after =
                index.compute_for_transactions("local", three_buyers().iter(), Some(2000), &config);
            println!("SNAPSHOT: before={before:?}; after={after:?}");
            assert_eq!(before.signer_cross_pool_velocity, Some(1.0 / 3.0));
            assert_eq!(before.status, MetricEvidenceQuality::Clean);
            assert_eq!(after, before);
        }
    }

    fn m4_current(at: u64) -> Vec<PoolTransaction> {
        ["a", "b", "c"]
            .into_iter()
            .map(|signer| tx("p", signer, &format!("{signer}-{at}"), at))
            .collect()
    }

    #[test]
    fn m4_filters_both_event_bounds_and_receiver_cutoff_for_history_and_signers() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();
        let mut late = tx("q", "a", "late", 1900);
        late.event_time.chain_event_ts_ms = Some(1900);
        late.event_time.ingress_wall_ts_ms = Some(2001);
        index.observe_transaction_at("q", &late, 2001, &config);
        index.observe_buy("q", "a", 999, &config); // za początkiem lookback
        index.observe_buy("q", "c", 2001, &config); // po anchorze
        index.observe_buy("q", "b", 1000, &config); // włączona dolna granica
        warm_source(&index, &config, 2000);
        let mut current = m4_current(1900);
        current.push(tx("p", "old", "old", 1499));
        current.push(tx("p", "future", "future", 2001));
        current.push(tx("other-pool", "wrong", "wrong", 1900));
        let mut late_buyer = tx("p", "late-buyer", "late-buyer", 1900);
        late_buyer.event_time.chain_event_ts_ms = Some(1900);
        late_buyer.event_time.ingress_wall_ts_ms = Some(2001);
        current.push(late_buyer);
        let window = CpvQueryWindow {
            signer_window_start_ms: 1500,
            anchor_ms: 2000,
            cutoff_received_ms: 2000,
        };
        let before = index.compute_for_transactions_at("p", &current, window, &config);
        assert_eq!(before.signer_sample_count, 3);
        assert_eq!(before.signer_cross_pool_velocity, Some(1.0 / 3.0));
        let after = index.compute_for_transactions_at(
            "p",
            &current[..3],
            CpvQueryWindow {
                cutoff_received_ms: 2001,
                ..window
            },
            &config,
        );
        assert_eq!(after.signer_cross_pool_velocity, Some(2.0 / 3.0));
        assert_eq!(
            index.compute_for_transactions_at("p", current.iter().rev(), window, &config),
            before
        );
    }

    #[test]
    fn m4_source_progress_is_not_wall_clock_or_a_single_buy_and_recovers_after_gap() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();
        let current = m4_current(2000);
        for tx in &current {
            index.observe_transaction("p", tx, &config);
        }
        assert!(!index.is_ready());
        assert_eq!(
            index
                .compute_for_transactions("p", &current, Some(2000), &config)
                .signer_cross_pool_velocity,
            None
        );
        warm_source(&index, &config, 2000);
        assert_eq!(
            index
                .compute_for_transactions("p", &current, Some(2000), &config)
                .signer_cross_pool_velocity,
            Some(0.0)
        );
        index.mark_stream_gap(2001);
        index.observe_source_progress(1, 3000, 2000, 3001, &config); // marker sprzed luki nie leczy historii
        assert!(!index.is_ready());
        index.observe_source_progress(1, 2100, 2100, 3100, &config);
        index.observe_source_progress(1, 3000, 3000, 3100, &config);
        let current = m4_current(3000);
        let window = CpvQueryWindow {
            signer_window_start_ms: 3000,
            anchor_ms: 3100,
            cutoff_received_ms: 3200,
        };
        assert_eq!(
            index
                .compute_for_transactions_at("p", &current, window, &config)
                .signer_cross_pool_velocity,
            None
        );
        index.observe_source_progress(1, 3100, 3100, 3100, &config);
        assert_eq!(
            index
                .compute_for_transactions_at("p", &current, window, &config)
                .signer_cross_pool_velocity,
            Some(0.0)
        );
        index.observe_source_progress(2, 3200, 3200, 3200, &config);
        assert!(!index.is_ready()); // nowy epoch nie dziedziczy pełnego okna
        index.observe_source_progress(1, 5000, 5000, 5000, &config); // stary epoch ignorowany
        assert!(!index.is_ready());
        index.observe_source_progress(2, 4200, 4200, 4200, &config);
        assert!(index.is_ready());
    }

    #[test]
    fn m4_forced_loss_expires_without_a_permanent_tombstone() {
        let index = CrossPoolVelocityIndex::new();
        let mut config = test_config();
        config.per_signer_cap = 2;
        index.observe_buy("other", "a", 1500, &config);
        index.observe_buy("p", "a", 1600, &config);
        index.observe_buy("p", "a", 1700, &config);
        warm_source(&index, &config, 2000);
        let at2000 =
            index.compute_for_transactions("p", m4_current(2000).iter(), Some(2000), &config);
        assert_eq!(at2000.signer_cross_pool_velocity, None);
        assert!(at2000
            .degraded_reasons
            .contains(&"CPV_HISTORY_LOST".to_string()));
        index.observe_source_progress(1, 3000, 3000, 3000, &config);
        let at3000 =
            index.compute_for_transactions("p", m4_current(3000).iter(), Some(3000), &config);
        assert_eq!(at3000.signer_cross_pool_velocity, Some(0.0));
        assert_eq!(at3000.status, MetricEvidenceQuality::Clean);
        index.observe_source_progress(1, 4000, 4000, 4000, &config);
        assert_eq!(index.entry_count(), 0); // regular expiry usuwa też wygasły znacznik
    }

    #[test]
    fn m4_global_eviction_loss_expires_after_the_affected_window() {
        let index = CrossPoolVelocityIndex::new();
        let mut config = test_config();
        config.global_signer_cap = 1;
        index.observe_buy("other", "lost-a", 1500, &config);
        index.observe_buy("other", "lost-b", 1600, &config); // global eviction of lost-a
        warm_source(&index, &config, 2000);
        let affected =
            index.compute_for_transactions("p", m4_current(2000).iter(), Some(2000), &config);
        assert_eq!(affected.signer_cross_pool_velocity, None);
        assert!(affected
            .degraded_reasons
            .contains(&"CPV_HISTORY_LOST".to_string()));

        // Monotonic source progress moves the retained floor past the lost event.
        // No new history is inserted: with complete coverage, absence is now a real zero.
        index.observe_source_progress(1, 4000, 4000, 4000, &config);
        let recovered =
            index.compute_for_transactions("p", m4_current(4000).iter(), Some(4000), &config);
        assert_eq!(recovered.signer_cross_pool_velocity, Some(0.0));
        assert_eq!(recovered.status, MetricEvidenceQuality::Clean);
        assert!(recovered.degraded_reasons.is_empty());
    }

    #[test]
    fn m4_pruning_removes_expired_out_of_order_records_from_all_indexes() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();
        // Celowo odwrotna kolejność event-time w tej samej historii.
        index.observe_buy("q", "a", 2_500, &config);
        index.observe_buy("q", "a", 500, &config);
        assert_eq!(index.inner.read().histories["a"].activities.len(), 2);

        // high-water=4000, retention=2*lookback => floor=2000.
        index.observe_source_progress(1, 4_000, 4_000, 4_000, &config);
        {
            let inner = index.inner.read();
            assert_eq!(inner.histories["a"].activities.len(), 1);
            assert_eq!(inner.histories["a"].activities[0].event_ms, 2_500);
            assert_eq!(inner.event_index.len(), 1);
            assert_eq!(inner.signer_order.len(), 1);
        }

        // Rekord dostarczony po przesunięciu floor nie może ponownie wejść.
        index.observe_buy("q", "a", 1_000, &config);
        let inner = index.inner.read();
        assert_eq!(inner.histories["a"].activities.len(), 1);
        assert_eq!(inner.histories["a"].activities[0].event_ms, 2_500);
        assert_eq!(inner.event_index.len(), 1);
        assert_eq!(inner.signer_order.len(), 1);
    }

    #[test]
    fn m4_queries_are_read_only_and_unretained_anchor_is_not_new_zero() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();
        index.observe_buy("q", "a", 1500, &config);
        warm_source(&index, &config, 2000);
        index.observe_source_progress(1, 2600, 2600, 2600, &config);
        let frozen = index.inner.read().clone();
        let old = index.compute_for_transactions("p", m4_current(2000).iter(), Some(2000), &config);
        let newer =
            index.compute_for_transactions("p", m4_current(2600).iter(), Some(2600), &config);
        assert_eq!(old.signer_cross_pool_velocity, Some(1.0 / 3.0));
        assert_eq!(newer.signer_cross_pool_velocity, Some(0.0));
        assert_eq!(
            index.compute_for_transactions("p", m4_current(2000).iter(), Some(2000), &config),
            old
        );
        assert_eq!(*index.inner.read(), frozen);
        index.observe_source_progress(1, 8000, 8000, 8000, &config);
        let unavailable =
            index.compute_for_transactions("p", m4_current(2000).iter(), Some(2000), &config);
        assert_eq!(unavailable.signer_cross_pool_velocity, None);
        assert!(unavailable
            .degraded_reasons
            .contains(&"CPV_HISTORY_NOT_RETAINED".to_string()));
    }

    #[test]
    fn m4_duplicate_never_refreshes_history_and_all_structures_have_hard_bounds() {
        let index = CrossPoolVelocityIndex::new();
        let mut config = test_config();
        config.per_signer_cap = 2;
        config.global_signer_cap = 2;
        let old = tx("q", "a", "same", 1000);
        index.observe_transaction("q", &old, &config);
        let mut again = old.clone();
        again.timestamp_ms = 2000;
        again.event_time = ghost_core::EventTimeMetadata::new(None, Some(2000), Some(2000));
        index.observe_transaction("q", &again, &config);
        assert_eq!(index.inner.read().histories["a"].activities.len(), 1);
        assert_eq!(index.inner.read().histories["a"].last_seen_ms, 1000);
        for i in 0..5000u64 {
            index.observe_buy("p", &format!("signer-{}", i % 3), 2001 + i, &config);
            if i % 10 == 0 {
                index.observe_source_progress(1, 2001 + i, 2001 + i, 2001 + i, &config);
            }
            let inner = index.inner.read();
            assert!(inner.histories.len() <= 2);
            assert_eq!(inner.signer_order.len(), inner.histories.len());
            assert!(inner.progress.len() <= 2);
            assert!(inner.event_index.len() <= 4);
            assert_eq!(
                inner.event_index.len(),
                inner
                    .histories
                    .values()
                    .map(|h| h.activities.len())
                    .sum::<usize>()
            );
            assert!(inner.histories.values().all(|h| h.activities.len() <= 2));
        }
    }

    #[test]
    fn m4_progress_itself_must_be_available_before_the_query_cutoff() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();
        index.observe_source_progress(1, 0, 1, 1, &config);
        index.observe_source_progress(1, 2000, 2000, 2500, &config);
        let current = m4_current(2000);
        let window = CpvQueryWindow {
            signer_window_start_ms: 1500,
            anchor_ms: 2000,
            cutoff_received_ms: 2499,
        };
        assert_eq!(
            index
                .compute_for_transactions_at("p", &current, window, &config)
                .signer_cross_pool_velocity,
            None
        );
        assert_eq!(
            index
                .compute_for_transactions_at(
                    "p",
                    &current,
                    CpvQueryWindow {
                        cutoff_received_ms: 2500,
                        ..window
                    },
                    &config
                )
                .signer_cross_pool_velocity,
            Some(0.0)
        );
    }
    #[test]
    fn m4_conflicting_event_views_never_resolve_by_delivery_order() {
        let config = test_config();
        let index = CrossPoolVelocityIndex::new();
        let mut first = tx("q", "a", "same", 1500);
        first.event_time.chain_event_ts_ms = Some(1500);
        index.observe_transaction("q", &first, &config);
        let mut conflict = first.clone();
        conflict.event_time.chain_event_ts_ms = Some(1600);
        index.observe_transaction("q", &conflict, &config);
        warm_source(&index, &config, 2000);
        let current = m4_current(2000);
        let history_conflict = index.compute_for_transactions("p", &current, Some(2000), &config);
        assert_eq!(history_conflict.signer_cross_pool_velocity, None);
        assert!(history_conflict
            .degraded_reasons
            .contains(&"CPV_HISTORY_LOST".to_string()));
        let clean = CrossPoolVelocityIndex::new();
        warm_source(&clean, &config, 2000);
        let mut bad = current[0].clone();
        bad.success = false;
        for views in [
            [&bad, &current[0], &current[1], &current[2]],
            [&current[2], &current[1], &current[0], &bad],
        ] {
            let result = clean.compute_for_transactions("p", views, Some(2000), &config);
            assert_eq!(result.signer_cross_pool_velocity, None);
            assert!(result
                .degraded_reasons
                .contains(&"CPV_QUERY_INPUT_UNAVAILABLE".to_string()));
        }
    }
    #[test]
    fn m4_ready_other_pool_window_does_not_hide_a_gap_in_the_signer_window() {
        let index = CrossPoolVelocityIndex::new();
        let config = test_config();
        index.observe_source_progress(1, 2000, 2000, 2000, &config);
        index.observe_source_progress(1, 4000, 4000, 4000, &config);
        let txs = m4_current(3900);
        let old_start = CpvQueryWindow {
            signer_window_start_ms: 1500,
            anchor_ms: 4000,
            cutoff_received_ms: 4000,
        };
        assert_eq!(
            index
                .compute_for_transactions_at("p", &txs, old_start, &config)
                .signer_cross_pool_velocity,
            None
        );
        assert_eq!(
            index
                .compute_for_transactions_at(
                    "p",
                    &txs,
                    CpvQueryWindow {
                        signer_window_start_ms: 3000,
                        ..old_start
                    },
                    &config
                )
                .signer_cross_pool_velocity,
            Some(0.0)
        );
        index.mark_stream_gap(4001);
        index.observe_source_progress(2, 9000, 5000, 5000, &config); // future proof rejected
        assert!(!index.is_ready());
    }
}
