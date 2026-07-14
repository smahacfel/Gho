use crate::events::PoolTransaction;
use crate::tx_intelligence::config::TxIntelligenceConfig;
use crate::tx_intelligence::{
    compute_dev_behavior, compute_signer_diversity, compute_velocity_profile,
    compute_volume_sanity, FlipV2ProducerSnapshotV1, FlipV2StateMachineV1, SignerStats,
};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::metric_contracts::DevBuySelectionModeV1;
use ghost_core::shadow_ledger::TxKey;
use ghost_core::tx_intelligence::types::{
    BurstWindow, RiskFlag, RiskSeverity, TxIntelFeatures, TxIntelligenceState,
};
use ghost_core::{EventTruthKind, SlotQuality};
use seer::early_fingerprint::{
    EarlyFingerprintMetrics, FingerprintAggregator, FingerprintTxEvent, TokenDelta,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const PUMPFUN_TOKEN_DECIMALS: u8 = 6;
const GENESIS_TOKEN_RESERVES_RAW: u128 = 1_073_000_000_000_000;
const FINGERPRINT_REPLAY_HISTORY_TRUNCATED_REASON: &str = "FINGERPRINT_REPLAY_HISTORY_TRUNCATED";
pub(crate) const BUNDLE_CLUSTER_THRESHOLD_MS: u64 = 50;

#[derive(Debug, Clone, Default)]
struct SignerBehaviorStats {
    tx_count: usize,
    buy_count: usize,
    sell_count: usize,
    total_volume_sol: f64,
    buy_volume_sol: f64,
    sell_volume_sol: f64,
    first_buy_volume_sol: Option<f64>,
    first_buy_tokens: Option<f64>,
    first_buy_record: Option<DevBuyCandidateV1>,
}

#[derive(Debug, Clone)]
struct DevBuyCandidateV1 {
    tx_key: Option<TxKey>,
    signer: String,
    signature: String,
    slot: Option<u64>,
    transaction_index: Option<u32>,
    amount_sol: f64,
    success: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DevBuyProducerSnapshotV1 {
    pub amount_sol: Option<f64>,
    pub creator_known: bool,
    pub create_signature: Option<String>,
    pub create_signature_matched: bool,
    pub selection_mode: DevBuySelectionModeV1,
    pub selected_signature: Option<String>,
    pub selected_slot: Option<u64>,
    pub selected_transaction_index: Option<u32>,
    pub eligible_buy_count: u64,
    pub selected_success: Option<bool>,
    pub selection_complete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TxTimingProducerSnapshotV1 {
    pub numerator: u64,
    pub denominator: u64,
    pub ratio: Option<f64>,
    pub canonical_dedupe_applied: bool,
    pub dust_filter_sol: Option<f64>,
    pub window_ms: Option<u64>,
    pub fallback_timestamp_count: u64,
    pub fallback_ordering_count: u64,
    pub source_complete: bool,
    pub source_state_capacity: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Top3ProducerSnapshotV1 {
    pub preferred_ratio: Option<f64>,
    pub compatibility_alias_ratio: Option<f64>,
    pub effective_ratio: Option<f64>,
    pub preferred_alias_bitwise_equal: Option<bool>,
    pub used_compatibility_fallback: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TxIntelligenceMetricContractSnapshotV1 {
    pub producer_dust_filter_sol: f64,
    pub producer_dedupe_capacity: u64,
    pub dev_first_observed: DevBuyProducerSnapshotV1,
    pub dev_primary_v1: DevBuyProducerSnapshotV1,
    pub exact_same_ms: TxTimingProducerSnapshotV1,
    pub cluster_lt_50ms: TxTimingProducerSnapshotV1,
    pub top3: Top3ProducerSnapshotV1,
}

impl SignerBehaviorStats {
    fn to_gatekeeper_stats(&self) -> SignerStats {
        SignerStats {
            tx_count: self.tx_count,
            buy_count: self.buy_count,
            sell_count: self.sell_count,
            total_volume_sol: self.total_volume_sol,
        }
    }
}

#[derive(Debug)]
pub struct TxIntelligenceEngine {
    pub state: TxIntelligenceState,
    pub config: TxIntelligenceConfig,
    signer_stats: HashMap<String, SignerBehaviorStats>,
    tx_timestamps_sorted: Vec<u64>,
    tx_volumes: Vec<f64>,
    total_volume_sol: f64,
    current_consecutive_buys: usize,
    max_consecutive_buys: usize,
    dev_wallet: Option<String>,
    pool_create_signature: Option<String>,
    first_signer: Option<String>,
    dev_buy_total_sol: f64,
    dev_buy_volume_total_sol: f64,
    dev_sell_total_sol: f64,
    dev_initial_buy_tokens: Option<f64>,
    tx_keys_seen: HashSet<TxKey>,
    tx_keys_fifo: VecDeque<TxKey>,
    tx_key_history_truncated: bool,
    dev_primary_candidates: VecDeque<DevBuyCandidateV1>,
    dev_primary_candidates_truncated: bool,
    timing_fallback_timestamp_count: u64,
    timing_fallback_ordering_count: u64,
    fingerprint_agg: Option<FingerprintAggregator>,
    fingerprint_replay_events: VecDeque<FingerprintTxEvent>,
    fingerprint_replay_history_truncated: bool,
    fingerprint_rebuild_skipped_due_to_truncated_history: bool,
    fingerprint_slot: Option<u64>,
    fingerprint_t0_ms: u64,
    flip_v2: FlipV2StateMachineV1,
}

impl TxIntelligenceEngine {
    #[must_use]
    pub fn new(
        config: TxIntelligenceConfig,
        candidate_snapshot: &EnhancedCandidate,
        dev_wallet: Option<Pubkey>,
    ) -> Self {
        let flip_v2 = FlipV2StateMachineV1::new(
            &config.fingerprint,
            config.min_sol_threshold,
            config.tx_key_capacity,
            candidate_snapshot.timestamp,
        );
        let mut engine = Self {
            state: TxIntelligenceState::default(),
            config,
            signer_stats: HashMap::new(),
            tx_timestamps_sorted: Vec::new(),
            tx_volumes: Vec::new(),
            total_volume_sol: 0.0,
            current_consecutive_buys: 0,
            max_consecutive_buys: 0,
            dev_wallet: dev_wallet.map(|value| value.to_string()),
            pool_create_signature: non_blank(candidate_snapshot.signature.as_str()),
            first_signer: None,
            dev_buy_total_sol: 0.0,
            dev_buy_volume_total_sol: 0.0,
            dev_sell_total_sol: 0.0,
            dev_initial_buy_tokens: None,
            tx_keys_seen: HashSet::new(),
            tx_keys_fifo: VecDeque::new(),
            tx_key_history_truncated: false,
            dev_primary_candidates: VecDeque::new(),
            dev_primary_candidates_truncated: false,
            timing_fallback_timestamp_count: 0,
            timing_fallback_ordering_count: 0,
            fingerprint_agg: None,
            fingerprint_replay_events: VecDeque::new(),
            fingerprint_replay_history_truncated: false,
            fingerprint_rebuild_skipped_due_to_truncated_history: false,
            fingerprint_slot: candidate_snapshot.slot,
            fingerprint_t0_ms: candidate_snapshot.timestamp,
            flip_v2,
        };
        engine.rebuild_fingerprint_aggregator();
        engine
    }

    #[must_use]
    pub const fn state(&self) -> &TxIntelligenceState {
        &self.state
    }

    #[must_use]
    pub const fn total_tx_count(&self) -> u64 {
        self.state.total_tx
    }

    #[must_use]
    pub fn unique_signer_count(&self) -> usize {
        self.state.unique_signers.len()
    }

    #[must_use]
    pub const fn dev_has_sold(&self) -> bool {
        self.state.dev_has_sold
    }

    pub fn set_dev_wallet(&mut self, dev_wallet: Option<Pubkey>) {
        self.dev_wallet = dev_wallet.map(|value| value.to_string());
        self.refresh_dev_metrics_from_signer_stats();
        self.rebuild_fingerprint_aggregator();
    }

    pub fn set_dev_identity(&mut self, dev_wallet: Option<Pubkey>, create_signature: Option<&str>) {
        self.dev_wallet = dev_wallet.map(|value| value.to_string());
        self.pool_create_signature = create_signature.and_then(non_blank);
        self.refresh_dev_metrics_from_signer_stats();
        self.rebuild_fingerprint_aggregator();
    }

    pub fn update_fingerprint_anchor(
        &mut self,
        slot: Option<u64>,
        timestamp_ms: Option<u64>,
        dev_wallet: Option<Pubkey>,
    ) {
        self.fingerprint_slot = slot.or(self.fingerprint_slot);
        if let Some(timestamp_ms) = timestamp_ms {
            self.fingerprint_t0_ms = timestamp_ms;
        }
        if let Some(dev_wallet) = dev_wallet {
            self.dev_wallet = Some(dev_wallet.to_string());
            self.refresh_dev_metrics_from_signer_stats();
        }
        self.rebuild_fingerprint_aggregator();
    }

    /// Atomically apply late pool identity and fingerprint anchor metadata.
    ///
    /// When replay history is complete, the rebuild replays every retained
    /// fingerprint-eligible event. If bounded history was truncated, the current complete
    /// aggregate is preserved and exposed as degraded instead of being replaced by a partial
    /// replay. In either case, metadata arriving after tx-first ingestion cannot silently erase
    /// earlier evidence or leave identity and anchor in two separate rebuild states.
    pub fn update_pool_identity_and_fingerprint_anchor(
        &mut self,
        dev_wallet: Option<Pubkey>,
        create_signature: Option<&str>,
        slot: Option<u64>,
        timestamp_ms: Option<u64>,
    ) {
        self.dev_wallet = dev_wallet.map(|value| value.to_string());
        self.pool_create_signature = create_signature.and_then(non_blank);
        self.fingerprint_slot = slot.or(self.fingerprint_slot);
        if let Some(timestamp_ms) = timestamp_ms {
            self.fingerprint_t0_ms = timestamp_ms;
        }
        self.refresh_dev_metrics_from_signer_stats();
        self.rebuild_fingerprint_aggregator();
    }

    pub fn on_transaction(&mut self, tx: &PoolTransaction) {
        self.flip_v2.on_transaction(tx);

        let tx_key = tx_key_for(tx);
        if let Some(ref tx_key) = tx_key {
            if self.tx_keys_seen.contains(tx_key) {
                return;
            }
        }

        if tx.volume_sol < self.config.min_sol_threshold {
            self.state.dust_tx_count = self.state.dust_tx_count.saturating_add(1);
            return;
        }

        if let Some(tx_key) = tx_key.clone() {
            self.track_tx_key(tx_key);
        }

        // The canonical fingerprint universe contains unique, successful, non-dust events only.
        // Failed attempts remain part of the V2-compatible TxIntelligence counters below, but
        // they cannot become positive/organic fingerprint evidence.
        if tx.success {
            self.ingest_fingerprint(tx);
        }

        let event_ts_ms = tx_epoch_like_event_ts_ms(tx);
        if tx.compat_event_ts_ms().is_none() {
            self.timing_fallback_timestamp_count =
                self.timing_fallback_timestamp_count.saturating_add(1);
        }
        if !tx_has_stable_timing_order_identity(tx) {
            self.timing_fallback_ordering_count =
                self.timing_fallback_ordering_count.saturating_add(1);
        }
        if self.first_signer.is_none() {
            self.first_signer = Some(tx.signer.clone());
        }

        self.state.total_tx = self.state.total_tx.saturating_add(1);
        if !tx.success {
            self.state.failed_tx_count = self.state.failed_tx_count.saturating_add(1);
        }
        if tx.is_buy {
            self.state.total_buys = self.state.total_buys.saturating_add(1);
            self.state.buy_volume_sol += tx.volume_sol;
        } else {
            self.state.total_sells = self.state.total_sells.saturating_add(1);
            self.state.sell_volume_sol += tx.volume_sol;
        }
        self.total_volume_sol += tx.volume_sol;
        self.tx_volumes.push(tx.volume_sol);

        let ts_insert_pos = self
            .tx_timestamps_sorted
            .partition_point(|timestamp| *timestamp <= event_ts_ms);
        self.tx_timestamps_sorted.insert(ts_insert_pos, event_ts_ms);
        self.recompute_timing_state();

        let signer_key = Pubkey::try_from(tx.signer.as_str()).ok();
        if let Some(signer_key) = signer_key {
            self.state.unique_signers.insert(signer_key);
            *self
                .state
                .signer_volume_map
                .entry(signer_key)
                .or_insert(0.0) += tx.volume_sol;
        }

        let signer_stats = self.signer_stats.entry(tx.signer.clone()).or_default();
        signer_stats.tx_count += 1;
        signer_stats.total_volume_sol += tx.volume_sol;
        if tx.is_buy {
            signer_stats.buy_count += 1;
            signer_stats.buy_volume_sol += tx.volume_sol;
            if signer_stats.first_buy_volume_sol.is_none() {
                signer_stats.first_buy_volume_sol = Some(tx.volume_sol);
            }
            if signer_stats.first_buy_tokens.is_none() {
                signer_stats.first_buy_tokens = tx.token_amount_units.map(|value| value as f64);
            }
            if signer_stats.first_buy_record.is_none() {
                signer_stats.first_buy_record = Some(dev_buy_candidate(tx, tx_key.clone()));
            }
        } else {
            signer_stats.sell_count += 1;
            signer_stats.sell_volume_sol += tx.volume_sol;
        }

        if tx.is_buy {
            self.current_consecutive_buys += 1;
            self.max_consecutive_buys =
                self.max_consecutive_buys.max(self.current_consecutive_buys);
        } else {
            self.current_consecutive_buys = 0;
        }

        if tx.is_dev_buy {
            self.state.dev_buy_lamports = self
                .state
                .dev_buy_lamports
                .saturating_add(tx.dev_buy_lamports);
            if self.dev_wallet.is_none() {
                self.dev_wallet = Some(tx.signer.clone());
            }
        }

        if tx.is_buy && tx.success {
            if let Some(tx_key) = tx_key {
                self.dev_primary_candidates
                    .push_back(dev_buy_candidate(tx, Some(tx_key)));
                while self.dev_primary_candidates.len() > self.config.tx_key_capacity {
                    self.dev_primary_candidates.pop_front();
                    self.dev_primary_candidates_truncated = true;
                }
            }
        }

        self.refresh_dev_metrics_from_signer_stats();
    }

    #[must_use]
    pub fn compute_features(&self) -> TxIntelFeatures {
        let total_tx = self.state.total_tx as usize;
        let gatekeeper_signer_stats: HashMap<String, SignerStats> = self
            .signer_stats
            .iter()
            .map(|(signer, stats)| (signer.clone(), stats.to_gatekeeper_stats()))
            .collect();

        let velocity = compute_velocity_profile(
            &self.tx_timestamps_sorted,
            self.config.observation_window_ms,
        );
        let diversity = compute_signer_diversity(
            &gatekeeper_signer_stats,
            total_tx,
            self.total_volume_sol,
            &self.tx_timestamps_sorted,
        );
        let volume = compute_volume_sanity(
            &self.tx_volumes,
            self.state.total_buys as usize,
            self.state.total_sells as usize,
            self.total_volume_sol,
            self.state.buy_volume_sol,
            self.max_consecutive_buys,
        );
        let dev = compute_dev_behavior(
            &self.dev_wallet,
            &self.first_signer,
            self.dev_buy_total_sol,
            self.dev_buy_volume_total_sol,
            self.dev_sell_total_sol,
            self.state.dev_tx_count as usize,
            self.state.dev_has_sold,
            self.dev_initial_buy_tokens,
            total_tx,
            self.total_volume_sol,
        );

        let tx_count = self.state.total_tx;
        let unique_signers = self.state.unique_signers.len() as u64;
        let total_tx_f64 = tx_count.max(1) as f64;
        let dust_denominator = tx_count.saturating_add(self.state.dust_tx_count).max(1) as f64;

        TxIntelFeatures {
            tx_count,
            buy_count: self.state.total_buys,
            sell_count: self.state.total_sells,
            unique_signers,
            buy_ratio: if tx_count > 0 {
                self.state.total_buys as f64 / tx_count as f64
            } else {
                0.0
            },
            sol_buy_ratio: volume.sol_buy_ratio,
            avg_tx_sol: volume.avg_tx_sol,
            volume_cv: volume.volume_cv,
            hhi: diversity.hhi,
            volume_gini: diversity.volume_gini,
            unique_signer_ratio: unique_signers as f64 / total_tx_f64,
            avg_tx_per_signer: if unique_signers > 0 {
                tx_count as f64 / unique_signers as f64
            } else {
                0.0
            },
            same_ms_tx_ratio: self.state.same_ms_tx_count as f64 / total_tx_f64,
            bundle_suspicion_ratio: self.state.bundle_suspicion_count as f64 / total_tx_f64,
            top3_signer_volume_ratio: diversity.top3_signer_volume_ratio,
            top3_volume_pct: diversity.effective_top3_signer_volume_ratio(),
            dev_buy_sol: dev.dev_buy_total_sol,
            dev_volume_ratio: dev.dev_volume_ratio,
            dev_tx_ratio: dev.dev_tx_ratio,
            dev_has_sold: dev.dev_has_sold,
            interval_cv: velocity.interval_cv,
            timing_entropy: velocity.timing_entropy,
            avg_interval_ms: velocity.avg_interval_ms,
            burst_ratio: velocity.burst_ratio,
            dust_ratio: self.state.dust_tx_count as f64 / dust_denominator,
            max_tx_per_signer: diversity.max_tx_per_signer as u64,
            total_volume_sol: volume.total_volume_sol,
            min_tx_sol: volume.min_tx_sol,
            max_tx_sol: volume.max_tx_sol,
            max_consecutive_buys: volume.max_consecutive_buys as u64,
            dev_wallet_known: dev.dev_wallet_known,
            dev_initial_buy_tokens: dev.dev_initial_buy_tokens,
            dev_tx_count: dev.dev_tx_count as u64,
            dev_is_first_buyer: dev.dev_is_first_buyer,
            dust_tx_count: self.state.dust_tx_count,
            failed_tx_count: self.state.failed_tx_count,
        }
    }

    #[must_use]
    pub fn get_risk_flags(&self) -> Vec<RiskFlag> {
        let features = self.compute_features();
        self.risk_flags_for_features(&features)
    }

    #[must_use]
    pub fn snapshot(&self) -> (TxIntelFeatures, Vec<RiskFlag>) {
        let features = self.compute_features();
        let flags = self.risk_flags_for_features(&features);
        (features, flags)
    }

    #[must_use]
    pub fn fingerprint_metrics(&self) -> Option<EarlyFingerprintMetrics> {
        self.fingerprint_agg.as_ref().map(|aggregator| {
            let mut metrics = aggregator.finalize();
            if self.fingerprint_rebuild_skipped_due_to_truncated_history {
                metrics.fingerprint_degraded = true;
                match metrics.fingerprint_reason.as_mut() {
                    Some(reasons)
                        if !reasons.split(',').any(|reason| {
                            reason == FINGERPRINT_REPLAY_HISTORY_TRUNCATED_REASON
                        }) =>
                    {
                        reasons.push(',');
                        reasons.push_str(FINGERPRINT_REPLAY_HISTORY_TRUNCATED_REASON);
                    }
                    None => {
                        metrics.fingerprint_reason =
                            Some(FINGERPRINT_REPLAY_HISTORY_TRUNCATED_REASON.to_string());
                    }
                    Some(_) => {}
                }
            }
            metrics
        })
    }

    #[must_use]
    pub fn flip_v2_snapshot(
        &self,
        decision_timestamp_ms: u64,
        decision_slot: Option<u64>,
    ) -> FlipV2ProducerSnapshotV1 {
        self.flip_v2.snapshot(decision_timestamp_ms, decision_slot)
    }

    pub fn mark_flip_v2_reconnect_gap(&mut self) {
        self.flip_v2.mark_reconnect_gap();
    }

    #[must_use]
    pub fn metric_contract_snapshot(
        &self,
        features: &TxIntelFeatures,
    ) -> TxIntelligenceMetricContractSnapshotV1 {
        let dev_first_observed = self.dev_first_observed_snapshot();
        let dev_primary_v1 = self.dev_primary_snapshot();
        let denominator = self.state.total_tx;
        let exact_ratio = (denominator > 0).then_some(features.same_ms_tx_ratio);
        let cluster_ratio = (denominator > 0).then_some(features.bundle_suspicion_ratio);
        let preferred = features.top3_signer_volume_ratio;
        let alias = (features.tx_count > 0 && features.total_volume_sol > 0.0)
            .then_some(features.top3_volume_pct);
        let effective = preferred.or(alias);

        TxIntelligenceMetricContractSnapshotV1 {
            producer_dust_filter_sol: self.config.min_sol_threshold,
            producer_dedupe_capacity: u64::try_from(self.config.tx_key_capacity)
                .unwrap_or(u64::MAX),
            dev_first_observed,
            dev_primary_v1,
            exact_same_ms: TxTimingProducerSnapshotV1 {
                numerator: self.state.same_ms_tx_count,
                denominator,
                ratio: exact_ratio,
                canonical_dedupe_applied: true,
                dust_filter_sol: Some(self.config.min_sol_threshold),
                window_ms: None,
                fallback_timestamp_count: self.timing_fallback_timestamp_count,
                fallback_ordering_count: self.timing_fallback_ordering_count,
                source_complete: !self.tx_key_history_truncated,
                source_state_capacity: Some(
                    u64::try_from(self.config.tx_key_capacity).unwrap_or(u64::MAX),
                ),
            },
            cluster_lt_50ms: TxTimingProducerSnapshotV1 {
                numerator: self.state.bundle_suspicion_count,
                denominator,
                ratio: cluster_ratio,
                canonical_dedupe_applied: true,
                dust_filter_sol: Some(self.config.min_sol_threshold),
                window_ms: None,
                fallback_timestamp_count: self.timing_fallback_timestamp_count,
                fallback_ordering_count: self.timing_fallback_ordering_count,
                source_complete: !self.tx_key_history_truncated,
                source_state_capacity: Some(
                    u64::try_from(self.config.tx_key_capacity).unwrap_or(u64::MAX),
                ),
            },
            top3: Top3ProducerSnapshotV1 {
                preferred_ratio: preferred,
                compatibility_alias_ratio: alias,
                effective_ratio: effective,
                preferred_alias_bitwise_equal: match (preferred, alias) {
                    (Some(left), Some(right)) => Some(left.to_bits() == right.to_bits()),
                    _ => None,
                },
                used_compatibility_fallback: preferred.is_none() && alias.is_some(),
            },
        }
    }

    fn dev_first_observed_snapshot(&self) -> DevBuyProducerSnapshotV1 {
        let creator_known = self.dev_wallet.is_some();
        let selected = self
            .dev_wallet
            .as_ref()
            .and_then(|creator| self.signer_stats.get(creator))
            .and_then(|stats| stats.first_buy_record.as_ref());
        dev_snapshot_from_candidate(
            selected,
            creator_known,
            self.pool_create_signature.clone(),
            false,
            if selected.is_some() {
                DevBuySelectionModeV1::LegacyFirstObserved
            } else {
                DevBuySelectionModeV1::NoEligibleBuy
            },
            u64::from(selected.is_some()),
            true,
        )
    }

    fn dev_primary_snapshot(&self) -> DevBuyProducerSnapshotV1 {
        let Some(creator) = self.dev_wallet.as_deref() else {
            return dev_snapshot_from_candidate(
                None,
                false,
                self.pool_create_signature.clone(),
                false,
                DevBuySelectionModeV1::NoEligibleBuy,
                0,
                !self.dev_primary_candidates_truncated,
            );
        };
        let eligible = self
            .dev_primary_candidates
            .iter()
            .filter(|candidate| candidate.signer == creator)
            .collect::<Vec<_>>();
        let signature_match = self
            .pool_create_signature
            .as_deref()
            .and_then(|create_signature| {
                eligible
                    .iter()
                    .copied()
                    .filter(|candidate| candidate.signature == create_signature)
                    .min_by(|left, right| left.tx_key.cmp(&right.tx_key))
            });
        let selected = signature_match.or_else(|| {
            eligible
                .iter()
                .copied()
                .min_by(|left, right| left.tx_key.cmp(&right.tx_key))
        });
        let selection_mode = if signature_match.is_some() {
            DevBuySelectionModeV1::CreateSignatureMatch
        } else if selected.is_some() {
            DevBuySelectionModeV1::EarliestEligibleCreatorBuy
        } else {
            DevBuySelectionModeV1::NoEligibleBuy
        };
        dev_snapshot_from_candidate(
            selected,
            true,
            self.pool_create_signature.clone(),
            signature_match.is_some(),
            selection_mode,
            eligible.len() as u64,
            !self.dev_primary_candidates_truncated,
        )
    }

    fn risk_flags_for_features(&self, features: &TxIntelFeatures) -> Vec<RiskFlag> {
        let mut flags = Vec::new();
        let detected_at_ms = self
            .tx_timestamps_sorted
            .last()
            .copied()
            .unwrap_or_default();
        let dev_known = self.dev_wallet.is_some();
        let max_tx_per_signer = self
            .signer_stats
            .values()
            .map(|stats| stats.tx_count)
            .max()
            .unwrap_or_default();

        if self.config.reject_on_dev_sell && features.dev_has_sold {
            flags.push(risk_flag(
                "dev_has_sold",
                RiskSeverity::Hard,
                detected_at_ms,
                "Developer wallet sold during the observation window".to_string(),
            ));
        }

        if features.tx_count >= 2 && features.interval_cv < 0.08 && features.avg_interval_ms < 30.0
        {
            flags.push(risk_flag(
                "extreme_bot_timing",
                RiskSeverity::Hard,
                detected_at_ms,
                format!(
                    "interval_cv={:.3} avg_interval_ms={:.1}",
                    features.interval_cv, features.avg_interval_ms
                ),
            ));
        }

        if features.hhi > 0.5 {
            flags.push(risk_flag(
                "extreme_signer_concentration",
                RiskSeverity::Hard,
                detected_at_ms,
                format!("hhi={:.3}", features.hhi),
            ));
        }

        if features.interval_cv < self.config.min_interval_cv {
            flags.push(risk_flag(
                "low_interval_cv",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "interval_cv={:.3} < {:.3}",
                    features.interval_cv, self.config.min_interval_cv
                ),
            ));
        }

        if features.interval_cv > self.config.max_interval_cv {
            flags.push(risk_flag(
                "high_interval_cv",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "interval_cv={:.3} > {:.3}",
                    features.interval_cv, self.config.max_interval_cv
                ),
            ));
        }

        if features.timing_entropy < self.config.min_timing_entropy {
            flags.push(risk_flag(
                "low_timing_entropy",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "timing_entropy={:.3} < {:.3}",
                    features.timing_entropy, self.config.min_timing_entropy
                ),
            ));
        }

        if features.timing_entropy > self.config.max_timing_entropy {
            flags.push(risk_flag(
                "high_timing_entropy",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "timing_entropy={:.3} > {:.3}",
                    features.timing_entropy, self.config.max_timing_entropy
                ),
            ));
        }

        if features.avg_interval_ms < self.config.min_avg_interval_ms
            || features.avg_interval_ms > self.config.max_avg_interval_ms
        {
            flags.push(risk_flag(
                "avg_interval_out_of_range",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "avg_interval_ms={:.1} not in [{:.1}, {:.1}]",
                    features.avg_interval_ms,
                    self.config.min_avg_interval_ms,
                    self.config.max_avg_interval_ms
                ),
            ));
        }

        if features.burst_ratio > self.config.max_burst_ratio {
            flags.push(risk_flag(
                "high_burst_ratio",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "burst_ratio={:.3} > {:.3}",
                    features.burst_ratio, self.config.max_burst_ratio
                ),
            ));
        }

        if features.bundle_suspicion_ratio > self.config.max_same_ms_tx_ratio {
            flags.push(risk_flag(
                "bundle_suspicion",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "bundle_ratio={:.3} > {:.3}",
                    features.bundle_suspicion_ratio, self.config.max_same_ms_tx_ratio
                ),
            ));
        }

        if features.unique_signer_ratio < self.config.min_unique_ratio
            || features.unique_signer_ratio > self.config.max_unique_ratio
        {
            flags.push(risk_flag(
                "unique_ratio_out_of_range",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "unique_signer_ratio={:.3} not in [{:.3}, {:.3}]",
                    features.unique_signer_ratio,
                    self.config.min_unique_ratio,
                    self.config.max_unique_ratio
                ),
            ));
        }

        if features.hhi > self.config.max_hhi {
            flags.push(risk_flag(
                "high_hhi",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!("hhi={:.3} > {:.3}", features.hhi, self.config.max_hhi),
            ));
        }

        if max_tx_per_signer > self.config.max_tx_per_signer {
            flags.push(risk_flag(
                "high_tx_per_signer",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "max_tx_per_signer={} > {}",
                    max_tx_per_signer, self.config.max_tx_per_signer
                ),
            ));
        }

        if features.volume_gini > self.config.max_volume_gini {
            flags.push(risk_flag(
                "high_volume_gini",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "volume_gini={:.3} > {:.3}",
                    features.volume_gini, self.config.max_volume_gini
                ),
            ));
        }

        let top3_signer_volume_ratio = features.effective_top3_signer_volume_ratio();
        if top3_signer_volume_ratio > self.config.max_top3_volume_pct {
            flags.push(risk_flag(
                "top3_volume_dominance",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "top3_signer_volume_ratio={:.3} > {:.3}",
                    top3_signer_volume_ratio, self.config.max_top3_volume_pct
                ),
            ));
        }

        if self.state.dust_tx_count < self.config.min_dust_filtered_count {
            flags.push(risk_flag(
                "low_dust_count",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dust_tx_count={} < {}",
                    self.state.dust_tx_count, self.config.min_dust_filtered_count
                ),
            ));
        }

        if dev_known && features.dev_buy_sol < self.config.min_dev_buy_sol {
            flags.push(risk_flag(
                "dev_buy_too_small",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dev_buy_sol={:.3} < {:.3}",
                    features.dev_buy_sol, self.config.min_dev_buy_sol
                ),
            ));
        }

        if features.dev_buy_sol > self.config.max_dev_buy_sol {
            flags.push(risk_flag(
                "dev_buy_too_large",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "dev_buy_sol={:.3} > {:.3}",
                    features.dev_buy_sol, self.config.max_dev_buy_sol
                ),
            ));
        }

        if features.dev_tx_ratio > self.config.max_dev_tx_ratio {
            flags.push(risk_flag(
                "high_dev_tx_ratio",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "dev_tx_ratio={:.3} > {:.3}",
                    features.dev_tx_ratio, self.config.max_dev_tx_ratio
                ),
            ));
        }

        if dev_known && features.dev_tx_ratio < self.config.min_dev_tx_ratio {
            flags.push(risk_flag(
                "low_dev_tx_ratio",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dev_tx_ratio={:.3} < {:.3}",
                    features.dev_tx_ratio, self.config.min_dev_tx_ratio
                ),
            ));
        }

        if features.dev_volume_ratio > self.config.max_dev_volume_ratio {
            flags.push(risk_flag(
                "high_dev_volume_ratio",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "dev_volume_ratio={:.3} > {:.3}",
                    features.dev_volume_ratio, self.config.max_dev_volume_ratio
                ),
            ));
        }

        if dev_known
            && self.config.min_dev_volume_ratio > 0.0
            && features.dev_volume_ratio < self.config.min_dev_volume_ratio
        {
            flags.push(risk_flag(
                "low_dev_volume_ratio",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dev_volume_ratio={:.3} < {:.3}",
                    features.dev_volume_ratio, self.config.min_dev_volume_ratio
                ),
            ));
        }

        flags
    }

    fn ingest_fingerprint(&mut self, tx: &PoolTransaction) {
        let Some(event) = pool_tx_to_fingerprint_event(tx) else {
            return;
        };
        if let Some(ref mut fingerprint_agg) = self.fingerprint_agg {
            if fingerprint_agg.in_window(&event) {
                fingerprint_agg.ingest(&event);
            }
        }
        self.retain_fingerprint_replay_event(event);
    }

    fn retain_fingerprint_replay_event(&mut self, event: FingerprintTxEvent) {
        self.fingerprint_replay_events.push_back(event);
        let capacity = self.config.tx_key_capacity.max(1);
        while self.fingerprint_replay_events.len() > capacity {
            self.fingerprint_replay_events.pop_front();
            self.fingerprint_replay_history_truncated = true;
        }
    }

    fn refresh_dev_metrics_from_signer_stats(&mut self) {
        self.dev_buy_total_sol = 0.0;
        self.dev_buy_volume_total_sol = 0.0;
        self.dev_sell_total_sol = 0.0;
        self.dev_initial_buy_tokens = None;
        self.state.dev_tx_count = 0;
        self.state.dev_has_sold = false;

        let Some(dev_wallet) = self.dev_wallet.as_ref() else {
            return;
        };
        let Some(stats) = self.signer_stats.get(dev_wallet) else {
            return;
        };

        self.dev_buy_total_sol = stats.first_buy_volume_sol.unwrap_or(0.0);
        self.dev_buy_volume_total_sol = stats.buy_volume_sol;
        self.dev_sell_total_sol = stats.sell_volume_sol;
        self.dev_initial_buy_tokens = stats.first_buy_tokens;
        self.state.dev_tx_count = stats.tx_count as u64;
        self.state.dev_has_sold = stats.sell_count > 0;
    }

    fn rebuild_fingerprint_aggregator(&mut self) {
        if self.fingerprint_replay_history_truncated {
            // A partial replay would silently replace a complete current aggregate with metrics
            // computed from only the retained suffix. Preserve the current evidence and expose
            // that late metadata could not be applied safely instead.
            self.fingerprint_rebuild_skipped_due_to_truncated_history = true;
            return;
        }

        let mut rebuilt = FingerprintAggregator::new(
            self.config.fingerprint.clone(),
            self.fingerprint_slot.unwrap_or(u64::MAX),
            self.fingerprint_slot.is_some(),
            self.fingerprint_t0_ms,
            self.fingerprint_slot.map(|_| GENESIS_TOKEN_RESERVES_RAW),
            PUMPFUN_TOKEN_DECIMALS,
            self.dev_wallet.clone(),
        );
        for event in &self.fingerprint_replay_events {
            if rebuilt.in_window(event) {
                rebuilt.ingest(event);
            }
        }
        self.fingerprint_agg = Some(rebuilt);
    }

    fn recompute_timing_state(&mut self) {
        self.state.tx_intervals_ms = self
            .tx_timestamps_sorted
            .windows(2)
            .map(|window| window[1].saturating_sub(window[0]))
            .filter(|interval| *interval > 0)
            .collect();

        self.state.same_ms_tx_count = self
            .tx_timestamps_sorted
            .windows(2)
            .filter(|window| window[1].saturating_sub(window[0]) == 0)
            .count() as u64;

        self.state.bundle_suspicion_count = self
            .tx_timestamps_sorted
            .windows(2)
            .filter(|window| window[1].saturating_sub(window[0]) < BUNDLE_CLUSTER_THRESHOLD_MS)
            .count() as u64;

        self.state.burst_windows.clear();
        if let Some(first_ts_ms) = self.tx_timestamps_sorted.first().copied() {
            let burst_end_ms = first_ts_ms.saturating_add(self.config.burst_window_ms.max(1));
            let tx_count = self
                .tx_timestamps_sorted
                .iter()
                .take_while(|timestamp| **timestamp <= burst_end_ms)
                .count() as u64;
            if tx_count > 0 {
                self.state.burst_windows.push(BurstWindow {
                    start_ts_ms: first_ts_ms,
                    end_ts_ms: burst_end_ms,
                    tx_count,
                });
            }
        }
    }

    fn track_tx_key(&mut self, tx_key: TxKey) {
        self.tx_keys_seen.insert(tx_key.clone());
        self.tx_keys_fifo.push_back(tx_key);
        while self.tx_keys_fifo.len() > self.config.tx_key_capacity {
            if let Some(oldest) = self.tx_keys_fifo.pop_front() {
                self.tx_keys_seen.remove(&oldest);
                self.tx_key_history_truncated = true;
            }
        }
    }
}

fn risk_flag(
    flag_id: &'static str,
    severity: RiskSeverity,
    detected_at_ms: u64,
    detail: String,
) -> RiskFlag {
    RiskFlag {
        flag_id: Cow::Borrowed(flag_id),
        severity,
        detected_at_ms,
        detail,
    }
}

fn non_blank(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "unknown").then(|| value.to_string())
}

fn dev_buy_candidate(tx: &PoolTransaction, tx_key: Option<TxKey>) -> DevBuyCandidateV1 {
    DevBuyCandidateV1 {
        tx_key,
        signer: tx.signer.clone(),
        signature: tx.signature.clone(),
        slot: tx.slot,
        transaction_index: tx.tx_index.or(tx.event_ordinal),
        amount_sol: tx.volume_sol,
        success: tx.success,
    }
}

#[allow(clippy::too_many_arguments)]
fn dev_snapshot_from_candidate(
    candidate: Option<&DevBuyCandidateV1>,
    creator_known: bool,
    create_signature: Option<String>,
    create_signature_matched: bool,
    selection_mode: DevBuySelectionModeV1,
    eligible_buy_count: u64,
    selection_complete: bool,
) -> DevBuyProducerSnapshotV1 {
    DevBuyProducerSnapshotV1 {
        amount_sol: candidate.map(|value| value.amount_sol),
        creator_known,
        create_signature,
        create_signature_matched,
        selection_mode,
        selected_signature: candidate.map(|value| value.signature.clone()),
        selected_slot: candidate.and_then(|value| value.slot),
        selected_transaction_index: candidate.and_then(|value| value.transaction_index),
        eligible_buy_count,
        selected_success: candidate.map(|value| value.success),
        selection_complete,
    }
}

fn tx_epoch_like_event_ts_ms(tx: &PoolTransaction) -> u64 {
    if let Some(explicit_event_ts_ms) = tx.effective_event_ts_ms() {
        explicit_event_ts_ms
    } else {
        wallclock_epoch_ms()
    }
}

fn tx_ordering_ts_ms(tx: &PoolTransaction) -> u64 {
    if let Some(explicit_event_ts_ms) = tx.compat_event_ts_ms() {
        explicit_event_ts_ms
    } else if tx.arrival_ts_ms > 0 {
        tx.arrival_ts_ms
    } else {
        wallclock_epoch_ms()
    }
}

fn wallclock_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn tx_key_for(tx: &PoolTransaction) -> Option<TxKey> {
    let event_ts_ms = tx_ordering_ts_ms(tx);
    if event_ts_ms == 0 {
        return None;
    }
    let signature = if tx.signature.is_empty() {
        None
    } else {
        Signature::from_str(&tx.signature).ok()
    };
    let has_ordering_info = signature.is_some() || tx.event_ordinal.is_some();
    let fallback_counter = if has_ordering_info {
        0
    } else {
        fallback_counter_for_tx(tx, event_ts_ms)
    };
    TxKey::new(
        event_ts_ms,
        tx.slot,
        tx.event_ordinal,
        signature,
        fallback_counter,
    )
    .ok()
}

pub(crate) fn tx_has_stable_timing_order_identity(tx: &PoolTransaction) -> bool {
    (!tx.signature.trim().is_empty() && Signature::from_str(&tx.signature).is_ok())
        || tx.event_ordinal.is_some()
        || tx.tx_index.is_some()
}

fn fallback_counter_for_tx(tx: &PoolTransaction, event_ts_ms: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    event_ts_ms.hash(&mut hasher);
    tx.signer.hash(&mut hasher);
    tx.is_buy.hash(&mut hasher);
    tx.volume_sol.to_bits().hash(&mut hasher);
    tx.event_ordinal.hash(&mut hasher);
    if let Some(price) = tx.price_quote {
        price.to_bits().hash(&mut hasher);
    }
    if let Some(lamports) = tx.sol_amount_lamports {
        lamports.hash(&mut hasher);
    }
    hasher.finish()
}

fn pool_tx_to_fingerprint_event(tx: &PoolTransaction) -> Option<FingerprintTxEvent> {
    if matches!(tx.semantic.event_truth_kind, EventTruthKind::Synthetic) {
        return None;
    }

    if !matches!(tx.semantic.slot_quality, SlotQuality::Present) {
        return None;
    }

    let slot = tx.slot?;
    let signer = tx.signer.clone();

    let mut token_deltas = Vec::new();
    if let Some(token_units) = tx.token_amount_units {
        let delta_raw = if tx.is_buy {
            token_units as i128
        } else {
            -(token_units as i128)
        };
        token_deltas.push(TokenDelta {
            owner: signer.clone(),
            delta_raw,
            decimals: PUMPFUN_TOKEN_DECIMALS,
        });
    }

    let mut sol_pre_balances = HashMap::new();
    if let Some(pre_balance_lamports) = tx.signer_pre_balance_lamports {
        sol_pre_balances.insert(signer.clone(), pre_balance_lamports);
    }

    Some(FingerprintTxEvent {
        slot,
        tx_index: 0,
        signature: tx.signature.clone(),
        timestamp_ms: tx_epoch_like_event_ts_ms(tx),
        is_buy: tx.is_buy,
        sol_amount_sol: tx
            .sol_amount_lamports
            .map(|lamports| lamports as f64 / LAMPORTS_PER_SOL)
            .or(Some(tx.volume_sol)),
        resolved_owner_deltas: tx.owner_token_deltas.clone(),
        token_deltas,
        sol_pre_balances,
        cu_price_micro_lamports: tx.cu_price_micro_lamports,
        compute_unit_limit: tx.compute_unit_limit,
        compute_units_consumed: tx.compute_units_consumed,
        inner_ix_count: tx.inner_ix_count,
        cpi_depth: tx.cpi_depth,
        ata_create_count: tx.ata_create_count,
        jito_tip_detected: tx.jito_tip_detected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx() -> PoolTransaction {
        PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope {
                slot_quality: SlotQuality::Present,
                ..ghost_core::EventSemanticEnvelope::default()
            },
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 0,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 77,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::Unknown,
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
            curve_finality: ghost_core::CurveFinality::Speculative,
        }
    }

    #[test]
    fn tx_epoch_like_event_ts_ms_prefers_explicit_event_time_over_arrival() {
        let mut tx = make_tx();
        tx.event_time.ingress_wall_ts_ms = Some(5_000);
        tx.arrival_ts_ms = 12;

        assert_eq!(tx_epoch_like_event_ts_ms(&tx), 5_000);
    }

    #[test]
    fn tx_epoch_like_event_ts_ms_does_not_use_monotonic_arrival_as_epoch() {
        let tx = make_tx();
        let before = wallclock_epoch_ms();
        let actual = tx_epoch_like_event_ts_ms(&tx);
        let after = wallclock_epoch_ms();

        assert!(
            actual >= before && actual <= after,
            "expected wallclock fallback, got {actual} outside [{before}, {after}]"
        );
    }

    #[test]
    fn tx_epoch_like_event_ts_ms_ignores_legacy_only_timestamp() {
        let mut tx = make_tx();
        tx.timestamp_ms = 5_000;

        let before = wallclock_epoch_ms();
        let actual = tx_epoch_like_event_ts_ms(&tx);
        let after = wallclock_epoch_ms();

        assert_ne!(actual, 5_000);
        assert!(
            actual >= before && actual <= after,
            "expected wallclock fallback, got {actual} outside [{before}, {after}]"
        );
    }

    #[test]
    fn tx_ordering_ts_ms_uses_arrival_for_internal_tie_breaks() {
        let tx = make_tx();

        assert_eq!(tx_ordering_ts_ms(&tx), 77);
    }

    #[test]
    fn fingerprint_event_uses_normalized_event_time() {
        let mut tx = make_tx();
        tx.event_time.ingress_wall_ts_ms = Some(9_000);
        tx.arrival_ts_ms = 15;

        let event = pool_tx_to_fingerprint_event(&tx).expect("fingerprint event");

        assert_eq!(event.timestamp_ms, 9_000);
    }

    #[test]
    fn fingerprint_anchor_preserves_existing_t0_when_timestamp_missing() {
        let mut candidate = EnhancedCandidate::default();
        candidate.timestamp = 1_000;
        let mut engine =
            TxIntelligenceEngine::new(TxIntelligenceConfig::default(), &candidate, None);

        engine.update_fingerprint_anchor(Some(7), None, None);

        assert_eq!(engine.fingerprint_t0_ms, 1_000);
        assert_eq!(engine.fingerprint_slot, Some(7));
    }

    #[test]
    fn fingerprint_replay_history_overflow_remains_explicit_after_rebuild() {
        let mut candidate = EnhancedCandidate::default();
        candidate.timestamp = 1_000;
        candidate.slot = Some(7);

        let mut config = TxIntelligenceConfig::default();
        config.tx_key_capacity = 1;
        let mut engine = TxIntelligenceEngine::new(config, &candidate, None);

        let mut first = make_tx();
        first.slot = Some(7);
        first.signature = "fingerprint-replay-first".to_string();
        first.timestamp_ms = 1_010;
        first.event_time = ghost_core::EventTimeMetadata::new(None, Some(1_010), None);

        let mut second = make_tx();
        second.slot = Some(7);
        second.event_ordinal = Some(1);
        second.signature = "fingerprint-replay-second".to_string();
        second.timestamp_ms = 1_020;
        second.event_time = ghost_core::EventTimeMetadata::new(None, Some(1_020), None);

        engine.on_transaction(&first);
        engine.on_transaction(&second);

        assert_eq!(engine.fingerprint_replay_events.len(), 1);
        assert!(engine.fingerprint_replay_history_truncated);
        let before_rebuild = engine.fingerprint_metrics().expect("fingerprint metrics");
        assert!(!before_rebuild
            .fingerprint_reason
            .as_deref()
            .is_some_and(|reason| reason.contains(FINGERPRINT_REPLAY_HISTORY_TRUNCATED_REASON)));

        engine.update_pool_identity_and_fingerprint_anchor(
            Some(Pubkey::new_unique()),
            Some("late-create-signature"),
            Some(7),
            Some(1_000),
        );

        let after_rebuild = engine.fingerprint_metrics().expect("fingerprint metrics");
        assert!(after_rebuild.fingerprint_degraded);
        assert_eq!(after_rebuild.sell_buy_ratio, before_rebuild.sell_buy_ratio);
        assert_eq!(
            after_rebuild.block0_sniped_supply_pct,
            before_rebuild.block0_sniped_supply_pct
        );
        assert!(after_rebuild
            .fingerprint_reason
            .as_deref()
            .is_some_and(|reason| reason.contains(FINGERPRINT_REPLAY_HISTORY_TRUNCATED_REASON)));
    }
}
