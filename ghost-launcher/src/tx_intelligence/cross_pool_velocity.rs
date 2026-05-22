use crate::events::PoolTransaction;
use crate::oracle_metrics::{
    record_cpv_index_entries, record_cpv_index_evictions, record_cpv_lookup_hits,
    record_cpv_lookup_misses,
};
use ghost_brain::config::GatekeeperV2Config;
use ghost_core::tx_intelligence::types::{
    CPV_INSUFFICIENT_SIGNERS_REASON, CPV_ROLLING_STATE_UNAVAILABLE_REASON,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrossPoolVelocityConfig {
    pub lookback_window_ms: u64,
    pub per_signer_cap: usize,
    pub global_signer_cap: usize,
}

impl CrossPoolVelocityConfig {
    #[must_use]
    pub fn from_gatekeeper_config(config: &GatekeeperV2Config) -> Self {
        Self {
            lookback_window_ms: config.cpv_lookback_window_s.saturating_mul(1_000),
            per_signer_cap: config.cpv_per_signer_cap.max(1),
            global_signer_cap: config.cpv_global_signer_cap.max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpvComputation {
    pub signer_cross_pool_velocity: Option<f64>,
    pub degraded_reasons: Vec<String>,
    pub signer_sample_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignerActivity {
    pool_id: String,
    observed_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SignerHistory {
    activities: VecDeque<SignerActivity>,
    last_seen_ms: u64,
}

#[derive(Debug, Default)]
struct CrossPoolVelocityInner {
    histories: HashMap<String, SignerHistory>,
    signer_order: VecDeque<(u64, String)>,
    saw_activity: bool,
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

    pub fn observe_transaction(
        &self,
        current_pool_id: &str,
        tx: &PoolTransaction,
        config: &CrossPoolVelocityConfig,
    ) {
        if !tx.is_buy || !tx.success {
            return;
        }

        let observed_at_ms = tx_event_ts_ms(tx);
        if observed_at_ms == 0 {
            return;
        }

        self.observe_buy(current_pool_id, tx.signer.as_str(), observed_at_ms, config);
    }

    pub fn observe_buy(
        &self,
        current_pool_id: &str,
        signer: &str,
        observed_at_ms: u64,
        config: &CrossPoolVelocityConfig,
    ) {
        if current_pool_id.is_empty() || signer.is_empty() || observed_at_ms == 0 {
            return;
        }

        let lookback_window_ms = config.lookback_window_ms.max(1);
        let window_start = observed_at_ms.saturating_sub(lookback_window_ms);
        let signer = signer.to_string();
        let pool_id = current_pool_id.to_string();

        let mut inner = self.inner.write();
        inner.saw_activity = true;

        let mut tracked_last_seen = None;
        {
            let history = inner.histories.entry(signer.clone()).or_default();
            prune_history(&mut history.activities, window_start);
            history.activities.push_back(SignerActivity {
                pool_id,
                observed_at_ms,
            });
            while history.activities.len() > config.per_signer_cap.max(1) {
                history.activities.pop_front();
            }

            let previous_last_seen = history.last_seen_ms;
            history.last_seen_ms = history.last_seen_ms.max(observed_at_ms);
            if history.last_seen_ms > previous_last_seen || previous_last_seen == 0 {
                tracked_last_seen = Some(history.last_seen_ms);
            }
        }

        if let Some(last_seen_ms) = tracked_last_seen {
            inner.signer_order.push_back((last_seen_ms, signer));
        }

        let evictions =
            prune_global_locked(&mut inner, window_start, config.global_signer_cap.max(1));
        if evictions > 0 {
            record_cpv_index_evictions(evictions);
        }
        record_cpv_index_entries(inner.histories.len());
    }

    #[must_use]
    pub fn compute_for_transactions<'a>(
        &self,
        current_pool_id: &str,
        transactions: impl IntoIterator<Item = &'a PoolTransaction>,
        anchor_ts_ms: Option<u64>,
        config: &CrossPoolVelocityConfig,
    ) -> CpvComputation {
        let unique_signers = unique_successful_signers(transactions);
        let signer_sample_count = unique_signers.len() as u64;
        let lookback_window_ms = config.lookback_window_ms.max(1);
        let anchor_ts_ms = anchor_ts_ms.unwrap_or_default();
        let window_start = anchor_ts_ms.saturating_sub(lookback_window_ms);

        let mut inner = self.inner.write();
        let evictions =
            prune_global_locked(&mut inner, window_start, config.global_signer_cap.max(1));
        if evictions > 0 {
            record_cpv_index_evictions(evictions);
        }
        record_cpv_index_entries(inner.histories.len());

        let ready = inner.saw_activity && !inner.histories.is_empty();
        let mut degraded_reasons = Vec::new();
        if !ready {
            degraded_reasons.push(CPV_ROLLING_STATE_UNAVAILABLE_REASON.to_string());
        }
        if signer_sample_count < 3 {
            degraded_reasons.push(CPV_INSUFFICIENT_SIGNERS_REASON.to_string());
        }
        if !degraded_reasons.is_empty() {
            return CpvComputation {
                signer_cross_pool_velocity: None,
                degraded_reasons,
                signer_sample_count,
            };
        }

        let mut cross_pool_signers = 0u64;
        let mut lookup_hits = 0u64;
        let mut lookup_misses = 0u64;
        let mut removed_entries = 0u64;

        for signer in unique_signers {
            let mut remove_signer = false;
            let mut is_cross_pool = false;
            let mut has_history = false;

            if let Some(history) = inner.histories.get_mut(&signer) {
                prune_history(&mut history.activities, window_start);
                if history.activities.is_empty() {
                    remove_signer = true;
                } else {
                    has_history = true;
                    is_cross_pool = history
                        .activities
                        .iter()
                        .any(|activity| activity.pool_id != current_pool_id);
                }
            }

            if remove_signer {
                inner.histories.remove(&signer);
                removed_entries = removed_entries.saturating_add(1);
            }

            if has_history {
                lookup_hits = lookup_hits.saturating_add(1);
                if is_cross_pool {
                    cross_pool_signers = cross_pool_signers.saturating_add(1);
                }
            } else {
                lookup_misses = lookup_misses.saturating_add(1);
            }
        }

        if removed_entries > 0 {
            record_cpv_index_entries(inner.histories.len());
        }
        if lookup_hits > 0 {
            record_cpv_lookup_hits(lookup_hits);
        }
        if lookup_misses > 0 {
            record_cpv_lookup_misses(lookup_misses);
        }

        CpvComputation {
            signer_cross_pool_velocity: Some(
                cross_pool_signers as f64 / signer_sample_count as f64,
            ),
            degraded_reasons: Vec::new(),
            signer_sample_count,
        }
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        let inner = self.inner.read();
        inner.saw_activity && !inner.histories.is_empty()
    }

    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.inner.read().histories.len()
    }
}

fn prune_history(activities: &mut VecDeque<SignerActivity>, window_start: u64) {
    while activities
        .front()
        .is_some_and(|activity| activity.observed_at_ms < window_start)
    {
        activities.pop_front();
    }
}

fn prune_global_locked(
    inner: &mut CrossPoolVelocityInner,
    window_start: u64,
    global_signer_cap: usize,
) -> u64 {
    let mut evictions = 0u64;
    while let Some((tracked_last_seen, signer)) = inner.signer_order.front().cloned() {
        let should_prune_for_window = tracked_last_seen < window_start;
        let should_prune_for_cap = inner.histories.len() > global_signer_cap;
        if !should_prune_for_window && !should_prune_for_cap {
            break;
        }

        inner.signer_order.pop_front();
        let should_remove = inner
            .histories
            .get(&signer)
            .is_some_and(|history| history.last_seen_ms == tracked_last_seen);
        if should_remove {
            inner.histories.remove(&signer);
            evictions = evictions.saturating_add(1);
        }
    }
    evictions
}

fn unique_successful_signers<'a>(
    transactions: impl IntoIterator<Item = &'a PoolTransaction>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut signers = Vec::new();
    for tx in transactions {
        if !tx.is_buy || !tx.success || tx.signer.is_empty() {
            continue;
        }
        if seen.insert(tx.signer.as_str()) {
            signers.push(tx.signer.clone());
        }
    }
    signers
}

fn tx_event_ts_ms(tx: &PoolTransaction) -> u64 {
    tx.event_time
        .compat_event_ts_ms(Some(tx.timestamp_ms))
        .unwrap_or(tx.timestamp_ms)
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
        PoolTransaction {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
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
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            bonding_curve_v2: None,
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
        let computed = index.compute_for_transactions("pool-b", current.iter(), Some(240), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(1.0 / 3.0));
        assert!(computed.degraded_reasons.is_empty());
        assert_eq!(computed.signer_sample_count, 3);
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
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(330), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(0.0));
        assert!(computed.degraded_reasons.is_empty());
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
        let computed =
            index.compute_for_transactions("pool-a", current.iter(), Some(1_200), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(0.0));
        assert!(computed.degraded_reasons.is_empty());
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
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(120), &config);

        assert_eq!(computed.signer_cross_pool_velocity, None);
        assert_eq!(
            computed.degraded_reasons,
            vec![CPV_INSUFFICIENT_SIGNERS_REASON.to_string()]
        );
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
        let computed = index.compute_for_transactions("pool-a", current.iter(), Some(170), &config);

        assert_eq!(computed.signer_cross_pool_velocity, Some(1.0 / 3.0));
        assert_eq!(computed.signer_sample_count, 3);
    }
}
