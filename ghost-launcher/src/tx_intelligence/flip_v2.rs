use crate::events::PoolTransaction;
use ghost_core::metric_contracts::{
    CanonicalU128StringV1, CanonicalU64StringV1, FlipEvidenceReasonV1, FlipOwnerEvidenceV2,
    FlipOwnerStatusV2, StableEventIdentityV1, StableEventKeyV1,
};
use ghost_core::{EventTruthKind, SlotQuality};
use seer::early_fingerprint::EarlyFingerprintConfig;
use std::collections::{BTreeMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub struct FlipV2ProducerConfigSnapshotV1 {
    pub wall_clock_window_ms: Option<u64>,
    pub max_slot_gap: u64,
    pub dump_ratio: f64,
    pub dust_threshold_sol: f64,
    pub dedupe_capacity: usize,
    pub max_wallets: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlipV2ProducerSnapshotV1 {
    pub ratio: Option<f64>,
    pub eligible_buyer_count: u64,
    pub flipper_count: u64,
    pub owners: Vec<FlipOwnerEvidenceV2>,
    pub config: FlipV2ProducerConfigSnapshotV1,
    pub evaluable: bool,
    pub reasons: Vec<FlipEvidenceReasonV1>,
    pub dedupe_eviction_count: u64,
    pub wallet_eviction_count: u64,
}

#[derive(Debug, Clone)]
struct EligibleFlipEventV1 {
    identity: StableEventIdentityV1,
    timestamp_ms: u64,
    slot: u64,
    owners: Vec<(String, i128)>,
}

#[derive(Debug, Clone, Default)]
struct OwnerStateV1 {
    status: Option<FlipOwnerStatusV2>,
    anchor_identity: Option<StableEventIdentityV1>,
    anchor_timestamp_ms: Option<u64>,
    anchor_slot: Option<u64>,
    pre_anchor_sell_count: u32,
    cumulative_buy_tokens: u128,
    cumulative_sell_tokens: u128,
    qualifying_identity: Option<StableEventIdentityV1>,
    qualifying_timestamp_ms: Option<u64>,
    qualifying_slot: Option<u64>,
}

impl OwnerStateV1 {
    fn into_evidence(self, owner_id: String) -> FlipOwnerEvidenceV2 {
        FlipOwnerEvidenceV2 {
            owner_id,
            status: self.status.unwrap_or(FlipOwnerStatusV2::NoAnchor),
            anchor_event_identity: self.anchor_identity.into(),
            anchor_slot: self.anchor_slot.map(CanonicalU64StringV1::new).into(),
            anchor_timestamp_ms: self
                .anchor_timestamp_ms
                .map(CanonicalU64StringV1::new)
                .into(),
            pre_anchor_sell_count: self.pre_anchor_sell_count,
            cumulative_eligible_buy_tokens: CanonicalU128StringV1::new(self.cumulative_buy_tokens),
            cumulative_eligible_sell_tokens: CanonicalU128StringV1::new(
                self.cumulative_sell_tokens,
            ),
            qualifying_sell_event_identity: self.qualifying_identity.into(),
            qualifying_sell_slot: self.qualifying_slot.map(CanonicalU64StringV1::new).into(),
            qualifying_sell_timestamp_ms: self
                .qualifying_timestamp_ms
                .map(CanonicalU64StringV1::new)
                .into(),
        }
    }
}

#[derive(Debug)]
pub struct FlipV2StateMachineV1 {
    config: FlipV2ProducerConfigSnapshotV1,
    pool_t0_ms: u64,
    events: Vec<EligibleFlipEventV1>,
    seen: HashSet<StableEventIdentityV1>,
    seen_fifo: VecDeque<StableEventIdentityV1>,
    seen_owners: HashSet<String>,
    reasons: Vec<FlipEvidenceReasonV1>,
    dedupe_eviction_count: u64,
    wallet_eviction_count: u64,
    reconnect_gap: bool,
}

impl FlipV2StateMachineV1 {
    #[must_use]
    pub fn new(
        fingerprint: &EarlyFingerprintConfig,
        dust_threshold_sol: f64,
        dedupe_capacity: usize,
        pool_t0_ms: u64,
    ) -> Self {
        let wall_clock_window_ms = fingerprint.window_secs.checked_mul(1_000);
        Self {
            config: FlipV2ProducerConfigSnapshotV1 {
                wall_clock_window_ms,
                max_slot_gap: fingerprint.max_flip_slots,
                dump_ratio: fingerprint.flip_dump_pct,
                dust_threshold_sol,
                dedupe_capacity,
                max_wallets: fingerprint.max_wallets,
            },
            pool_t0_ms,
            events: Vec::new(),
            seen: HashSet::new(),
            seen_fifo: VecDeque::new(),
            seen_owners: HashSet::new(),
            reasons: Vec::new(),
            dedupe_eviction_count: 0,
            wallet_eviction_count: 0,
            reconnect_gap: false,
        }
    }

    fn record_reason(&mut self, reason: FlipEvidenceReasonV1) {
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }

    pub fn mark_reconnect_gap(&mut self) {
        self.reconnect_gap = true;
        self.record_reason(FlipEvidenceReasonV1::ReconnectGap);
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.seen.clear();
        self.seen_fifo.clear();
        self.seen_owners.clear();
        self.reasons.clear();
        self.dedupe_eviction_count = 0;
        self.wallet_eviction_count = 0;
        self.reconnect_gap = false;
    }

    pub fn on_transaction(&mut self, tx: &PoolTransaction) {
        if !tx.success {
            self.record_reason(FlipEvidenceReasonV1::FailedTransactionExcluded);
            return;
        }
        if !tx.volume_sol.is_finite() || tx.volume_sol < self.config.dust_threshold_sol {
            self.record_reason(FlipEvidenceReasonV1::DustExcluded);
            return;
        }
        let Some(timestamp_ms) = tx.event_time.effective_event_ts_ms() else {
            self.record_reason(FlipEvidenceReasonV1::MissingStableOrder);
            return;
        };
        let Some(window_end) = self
            .config
            .wall_clock_window_ms
            .and_then(|window_ms| self.pool_t0_ms.checked_add(window_ms))
        else {
            self.record_reason(FlipEvidenceReasonV1::ArithmeticOverflow);
            return;
        };
        if timestamp_ms < self.pool_t0_ms || timestamp_ms > window_end {
            return;
        }
        let Some(slot) = tx
            .slot
            .filter(|_| tx.semantic.slot_quality == SlotQuality::Present)
        else {
            self.record_reason(FlipEvidenceReasonV1::MissingStableOrder);
            return;
        };
        if tx.semantic.event_truth_kind == EventTruthKind::Synthetic {
            self.record_reason(FlipEvidenceReasonV1::MissingStableOrder);
            return;
        }
        let source = tx.semantic.source_kind.as_str();
        let identity = if !tx.signature.trim().is_empty() {
            StableEventIdentityV1::try_from_signature(source, tx.signature.clone()).ok()
        } else if let Some(transaction_index) = tx.tx_index {
            StableEventIdentityV1::try_from_transaction_index(source, slot, transaction_index).ok()
        } else if let Some(event_ordinal) = tx.event_ordinal {
            StableEventIdentityV1::try_from_event_ordinal(source, slot, event_ordinal).ok()
        } else {
            None
        };
        let Some(identity) = identity else {
            self.record_reason(FlipEvidenceReasonV1::MissingStableIdentity);
            return;
        };
        if self.seen.contains(&identity) {
            self.record_reason(FlipEvidenceReasonV1::DuplicateEvent);
            return;
        }
        if self.config.dedupe_capacity == 0 {
            self.record_reason(FlipEvidenceReasonV1::WalletCapReached);
            return;
        }
        if self.seen.len() >= self.config.dedupe_capacity {
            if let Some(evicted) = self.seen_fifo.pop_front() {
                self.seen.remove(&evicted);
                self.events.retain(|event| event.identity != evicted);
                self.dedupe_eviction_count = match self.dedupe_eviction_count.checked_add(1) {
                    Some(value) => value,
                    None => {
                        self.record_reason(FlipEvidenceReasonV1::ArithmeticOverflow);
                        return;
                    }
                };
                ::metrics::counter!("metric_contract_flip_v2_dedupe_evictions_total", 1);
            }
            self.record_reason(FlipEvidenceReasonV1::WalletCapReached);
        }
        let owners = tx
            .owner_token_deltas
            .iter()
            .filter(|delta| delta.delta_raw != 0 && !delta.owner.trim().is_empty())
            .map(|delta| (delta.owner.clone(), delta.delta_raw))
            .collect::<Vec<_>>();
        if owners.is_empty() {
            self.record_reason(FlipEvidenceReasonV1::MissingResolvedOwner);
            return;
        }
        let new_owner_count = owners
            .iter()
            .map(|(owner, _)| owner)
            .filter(|owner| !self.seen_owners.contains(owner.as_str()))
            .collect::<HashSet<_>>()
            .len();
        let Some(total_owner_count) = self.seen_owners.len().checked_add(new_owner_count) else {
            self.record_reason(FlipEvidenceReasonV1::ArithmeticOverflow);
            return;
        };
        if total_owner_count > self.config.max_wallets {
            self.wallet_eviction_count = match self.wallet_eviction_count.checked_add(1) {
                Some(value) => value,
                None => {
                    self.record_reason(FlipEvidenceReasonV1::ArithmeticOverflow);
                    return;
                }
            };
            self.record_reason(FlipEvidenceReasonV1::WalletCapReached);
            ::metrics::counter!("metric_contract_flip_v2_wallet_evictions_total", 1);
            return;
        }
        self.seen_owners
            .extend(owners.iter().map(|(owner, _)| owner.clone()));
        self.seen.insert(identity.clone());
        self.seen_fifo.push_back(identity.clone());
        self.events.push(EligibleFlipEventV1 {
            identity,
            timestamp_ms,
            slot,
            owners,
        });
    }

    #[must_use]
    pub fn snapshot(
        &self,
        decision_timestamp_ms: u64,
        decision_slot: Option<u64>,
    ) -> FlipV2ProducerSnapshotV1 {
        let mut reasons = self.reasons.clone();
        let Some(wall_clock_window_ms) = self.config.wall_clock_window_ms else {
            if !reasons.contains(&FlipEvidenceReasonV1::ArithmeticOverflow) {
                reasons.push(FlipEvidenceReasonV1::ArithmeticOverflow);
            }
            return FlipV2ProducerSnapshotV1 {
                ratio: None,
                eligible_buyer_count: 0,
                flipper_count: 0,
                owners: Vec::new(),
                config: self.config.clone(),
                evaluable: false,
                reasons,
                dedupe_eviction_count: self.dedupe_eviction_count,
                wallet_eviction_count: self.wallet_eviction_count,
            };
        };
        let mut events = self.events.clone();
        events
            .sort_by(|left, right| canonical_event_order(left).cmp(&canonical_event_order(right)));
        let order_conflict = events
            .windows(2)
            .any(|pair| pair[1].timestamp_ms > pair[0].timestamp_ms && pair[1].slot < pair[0].slot);
        if order_conflict && !reasons.contains(&FlipEvidenceReasonV1::OutOfOrderEvent) {
            reasons.push(FlipEvidenceReasonV1::OutOfOrderEvent);
        }
        let capacity_lost = self.dedupe_eviction_count > 0 || self.wallet_eviction_count > 0;
        let evaluable = !self.reconnect_gap
            && !capacity_lost
            && !order_conflict
            && !reasons.iter().any(|reason| {
                matches!(
                    reason,
                    FlipEvidenceReasonV1::MissingStableIdentity
                        | FlipEvidenceReasonV1::MissingStableOrder
                        | FlipEvidenceReasonV1::MissingResolvedOwner
                        | FlipEvidenceReasonV1::ArithmeticOverflow
                )
            });
        if !evaluable {
            return FlipV2ProducerSnapshotV1 {
                ratio: None,
                eligible_buyer_count: 0,
                flipper_count: 0,
                owners: Vec::new(),
                config: self.config.clone(),
                evaluable: false,
                reasons,
                dedupe_eviction_count: self.dedupe_eviction_count,
                wallet_eviction_count: self.wallet_eviction_count,
            };
        }

        let mut owners = BTreeMap::<String, OwnerStateV1>::new();
        let mut overflow = false;
        for event in events {
            for (owner_id, delta) in event.owners {
                let owner = owners.entry(owner_id).or_default();
                if owner.status == Some(FlipOwnerStatusV2::Flipper) {
                    continue;
                }
                if delta > 0 {
                    let amount = delta as u128;
                    if owner.anchor_identity.is_none() {
                        owner.status = Some(FlipOwnerStatusV2::Tracking);
                        owner.anchor_identity = Some(event.identity.clone());
                        owner.anchor_timestamp_ms = Some(event.timestamp_ms);
                        owner.anchor_slot = Some(event.slot);
                    }
                    match owner.cumulative_buy_tokens.checked_add(amount) {
                        Some(value) => owner.cumulative_buy_tokens = value,
                        None => overflow = true,
                    }
                } else {
                    let sold = delta.unsigned_abs();
                    let (Some(anchor_ts), Some(anchor_slot)) =
                        (owner.anchor_timestamp_ms, owner.anchor_slot)
                    else {
                        owner.status = Some(FlipOwnerStatusV2::NoAnchor);
                        owner.pre_anchor_sell_count =
                            match owner.pre_anchor_sell_count.checked_add(1) {
                                Some(value) => value,
                                None => {
                                    overflow = true;
                                    owner.pre_anchor_sell_count
                                }
                            };
                        continue;
                    };
                    match owner.cumulative_sell_tokens.checked_add(sold) {
                        Some(value) => owner.cumulative_sell_tokens = value,
                        None => overflow = true,
                    }
                    if owner.cumulative_buy_tokens > (1_u128 << 53)
                        || owner.cumulative_sell_tokens > (1_u128 << 53)
                    {
                        overflow = true;
                        continue;
                    }
                    let within_time = event
                        .timestamp_ms
                        .checked_sub(anchor_ts)
                        .is_some_and(|gap| gap <= wall_clock_window_ms);
                    let within_slot = event
                        .slot
                        .checked_sub(anchor_slot)
                        .is_some_and(|gap| gap <= self.config.max_slot_gap);
                    let threshold_met = (owner.cumulative_sell_tokens as f64)
                        >= (owner.cumulative_buy_tokens as f64 * self.config.dump_ratio);
                    if within_time && within_slot && threshold_met {
                        owner.status = Some(FlipOwnerStatusV2::Flipper);
                        owner.qualifying_identity = Some(event.identity.clone());
                        owner.qualifying_timestamp_ms = Some(event.timestamp_ms);
                        owner.qualifying_slot = Some(event.slot);
                    }
                }
            }
        }
        if overflow {
            if !reasons.contains(&FlipEvidenceReasonV1::ArithmeticOverflow) {
                reasons.push(FlipEvidenceReasonV1::ArithmeticOverflow);
            }
            return FlipV2ProducerSnapshotV1 {
                ratio: None,
                eligible_buyer_count: 0,
                flipper_count: 0,
                owners: Vec::new(),
                config: self.config.clone(),
                evaluable: false,
                reasons,
                dedupe_eviction_count: self.dedupe_eviction_count,
                wallet_eviction_count: self.wallet_eviction_count,
            };
        }
        for owner in owners.values_mut() {
            if owner.status != Some(FlipOwnerStatusV2::Tracking) {
                continue;
            }
            let time_closed = owner.anchor_timestamp_ms.is_some_and(|anchor| {
                decision_timestamp_ms
                    .checked_sub(anchor)
                    .is_some_and(|gap| gap > wall_clock_window_ms)
            });
            let slot_closed = match (owner.anchor_slot, decision_slot) {
                (Some(anchor), Some(current)) => current
                    .checked_sub(anchor)
                    .is_some_and(|gap| gap > self.config.max_slot_gap),
                _ => false,
            };
            if time_closed || slot_closed {
                owner.status = Some(FlipOwnerStatusV2::ClosedNonFlipper);
            }
        }
        let owners = owners
            .into_iter()
            .map(|(owner, state)| state.into_evidence(owner))
            .collect::<Vec<_>>();
        let eligible_buyer_count = owners
            .iter()
            .filter(|owner| owner.status != FlipOwnerStatusV2::NoAnchor)
            .count();
        let flipper_count = owners
            .iter()
            .filter(|owner| owner.status == FlipOwnerStatusV2::Flipper)
            .count();
        let (Ok(eligible_buyer_count), Ok(flipper_count)) = (
            u64::try_from(eligible_buyer_count),
            u64::try_from(flipper_count),
        ) else {
            reasons.push(FlipEvidenceReasonV1::ArithmeticOverflow);
            return FlipV2ProducerSnapshotV1 {
                ratio: None,
                eligible_buyer_count: 0,
                flipper_count: 0,
                owners: Vec::new(),
                config: self.config.clone(),
                evaluable: false,
                reasons,
                dedupe_eviction_count: self.dedupe_eviction_count,
                wallet_eviction_count: self.wallet_eviction_count,
            };
        };
        let ratio =
            (eligible_buyer_count > 0).then(|| flipper_count as f64 / eligible_buyer_count as f64);
        if eligible_buyer_count == 0 && !reasons.contains(&FlipEvidenceReasonV1::NoEligibleBuyers) {
            reasons.push(FlipEvidenceReasonV1::NoEligibleBuyers);
        }
        FlipV2ProducerSnapshotV1 {
            ratio,
            eligible_buyer_count,
            flipper_count,
            owners,
            config: self.config.clone(),
            evaluable: true,
            reasons,
            dedupe_eviction_count: self.dedupe_eviction_count,
            wallet_eviction_count: self.wallet_eviction_count,
        }
    }
}

fn canonical_event_order(event: &EligibleFlipEventV1) -> (u64, u64, u8, u32, &str) {
    match &event.identity.key {
        StableEventKeyV1::Signature { signature } => {
            (event.timestamp_ms, event.slot, 0, 0, signature.as_str())
        }
        StableEventKeyV1::SlotTransactionIndex {
            transaction_index, ..
        } => (event.timestamp_ms, event.slot, 1, *transaction_index, ""),
        StableEventKeyV1::SlotEventOrdinal { event_ordinal, .. } => {
            (event.timestamp_ms, event.slot, 2, *event_ordinal, "")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RawBytesMissingReason;
    use ghost_core::metric_contracts::CanonicalNullableV1;
    use ghost_core::{CurveFinality, EventSemanticEnvelope, EventTimeMetadata};
    use seer::early_fingerprint::TokenDelta;
    use seer::types::ToolchainFingerprintInput;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Signature;

    fn machine(max_wallets: usize, dedupe_capacity: usize) -> FlipV2StateMachineV1 {
        let config = EarlyFingerprintConfig {
            max_wallets,
            ..EarlyFingerprintConfig::default()
        };
        FlipV2StateMachineV1::new(&config, 0.01, dedupe_capacity, 1_000)
    }

    fn tx(
        owner: &str,
        delta_raw: i128,
        timestamp_ms: u64,
        slot: u64,
        ordinal: u32,
    ) -> PoolTransaction {
        PoolTransaction {
            semantic: EventSemanticEnvelope {
                slot_quality: SlotQuality::Present,
                ..EventSemanticEnvelope::default()
            },
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(slot),
            event_ordinal: Some(ordinal),
            tx_index: Some(ordinal),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms,
            event_time: EventTimeMetadata {
                ingress_wall_ts_ms: Some(timestamp_ms),
                ..EventTimeMetadata::default()
            },
            arrival_ts_ms: timestamp_ms,
            signer: owner.to_string(),
            is_buy: delta_raw > 0,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(delta_raw.unsigned_abs() as u64),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Signature::new_unique().to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![TokenDelta {
                owner: owner.to_string(),
                delta_raw,
                decimals: 6,
            }],
            mpcf_payload: Vec::new(),
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
            buy_remaining_accounts: Vec::new(),
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: CurveFinality::Speculative,
        }
    }

    fn owner<'a>(snapshot: &'a FlipV2ProducerSnapshotV1, id: &str) -> &'a FlipOwnerEvidenceV2 {
        snapshot
            .owners
            .iter()
            .find(|owner| owner.owner_id == id)
            .expect("owner evidence")
    }

    #[test]
    fn multiple_buys_nonqualifying_sell_then_first_qualifying_sell_freezes_owner() {
        let mut machine = machine(8, 32);
        for event in [
            tx("alice", 100, 1_000, 10, 0),
            tx("alice", 100, 1_100, 11, 1),
            tx("alice", -50, 1_200, 12, 2),
            tx("alice", 100, 1_300, 13, 3),
            tx("alice", -100, 1_400, 14, 4),
            tx("alice", -200, 1_500, 15, 5),
        ] {
            machine.on_transaction(&event);
        }
        let snapshot = machine.snapshot(2_000, Some(20));
        assert_eq!(snapshot.eligible_buyer_count, 1);
        assert_eq!(snapshot.flipper_count, 1);
        assert_eq!(snapshot.ratio, Some(1.0));
        let alice = owner(&snapshot, "alice");
        assert_eq!(alice.status, FlipOwnerStatusV2::Flipper);
        assert_eq!(alice.cumulative_eligible_buy_tokens.get(), 300);
        assert_eq!(alice.cumulative_eligible_sell_tokens.get(), 150);
    }

    #[test]
    fn pre_anchor_sell_is_not_retroactive_and_first_buy_is_never_reanchored() {
        let mut machine = machine(8, 32);
        for event in [
            tx("alice", -100, 1_000, 10, 0),
            tx("alice", 100, 1_100, 11, 1),
            tx("alice", 100, 1_200, 12, 2),
            tx("alice", -100, 1_300, 13, 3),
        ] {
            machine.on_transaction(&event);
        }
        let snapshot = machine.snapshot(2_000, Some(20));
        let alice = owner(&snapshot, "alice");
        assert_eq!(alice.pre_anchor_sell_count, 1);
        assert_eq!(
            alice.anchor_timestamp_ms,
            CanonicalNullableV1::Value(CanonicalU64StringV1::new(1_100))
        );
        assert_eq!(alice.cumulative_eligible_buy_tokens.get(), 200);
        assert_eq!(alice.cumulative_eligible_sell_tokens.get(), 100);
        assert_eq!(alice.status, FlipOwnerStatusV2::Flipper);
    }

    #[test]
    fn exact_time_and_slot_boundaries_are_inclusive_but_slot_21_is_not() {
        let mut exact = machine(8, 32);
        exact.on_transaction(&tx("alice", 100, 1_000, 10, 0));
        exact.on_transaction(&tx("alice", -50, 11_000, 30, 1));
        let snapshot = exact.snapshot(11_000, Some(30));
        assert_eq!(snapshot.flipper_count, 1);

        let mut late = machine(8, 32);
        late.on_transaction(&tx("alice", 100, 1_000, 10, 0));
        late.on_transaction(&tx("alice", -50, 10_999, 31, 1));
        let snapshot = late.snapshot(12_000, Some(31));
        assert_eq!(snapshot.flipper_count, 0);
        assert_eq!(
            owner(&snapshot, "alice").status,
            FlipOwnerStatusV2::ClosedNonFlipper
        );
    }

    #[test]
    fn duplicate_failed_dust_missing_owner_and_missing_identity_are_excluded() {
        let mut machine = machine(8, 32);
        let valid = tx("alice", 100, 1_000, 10, 0);
        machine.on_transaction(&valid);
        machine.on_transaction(&valid);
        let mut failed = tx("bob", 100, 1_100, 11, 1);
        failed.success = false;
        machine.on_transaction(&failed);
        let mut dust = tx("carol", 100, 1_200, 12, 2);
        dust.volume_sol = 0.001;
        machine.on_transaction(&dust);
        let mut no_owner = tx("dave", 100, 1_300, 13, 3);
        no_owner.owner_token_deltas.clear();
        machine.on_transaction(&no_owner);
        let mut no_identity = tx("eve", 100, 1_400, 14, 4);
        no_identity.signature.clear();
        no_identity.tx_index = None;
        no_identity.event_ordinal = None;
        machine.on_transaction(&no_identity);
        let snapshot = machine.snapshot(2_000, Some(20));
        assert!(!snapshot.evaluable);
        assert!(snapshot
            .reasons
            .contains(&FlipEvidenceReasonV1::DuplicateEvent));
        assert!(snapshot
            .reasons
            .contains(&FlipEvidenceReasonV1::FailedTransactionExcluded));
        assert!(snapshot
            .reasons
            .contains(&FlipEvidenceReasonV1::DustExcluded));
        assert!(snapshot
            .reasons
            .contains(&FlipEvidenceReasonV1::MissingResolvedOwner));
        assert!(snapshot
            .reasons
            .contains(&FlipEvidenceReasonV1::MissingStableIdentity));
    }

    #[test]
    fn reconnect_wallet_and_dedupe_capacity_degrade_fail_closed_and_clear_resets() {
        let mut reconnect = machine(8, 32);
        reconnect.on_transaction(&tx("alice", 100, 1_000, 10, 0));
        reconnect.mark_reconnect_gap();
        assert!(!reconnect.snapshot(2_000, Some(20)).evaluable);
        reconnect.clear();
        assert!(reconnect.snapshot(2_000, Some(20)).evaluable);

        let mut wallets = machine(1, 32);
        wallets.on_transaction(&tx("alice", 100, 1_000, 10, 0));
        wallets.on_transaction(&tx("bob", 100, 1_100, 11, 1));
        let snapshot = wallets.snapshot(2_000, Some(20));
        assert!(!snapshot.evaluable);
        assert_eq!(snapshot.wallet_eviction_count, 1);

        let mut dedupe = machine(8, 1);
        dedupe.on_transaction(&tx("alice", 100, 1_000, 10, 0));
        dedupe.on_transaction(&tx("alice", 100, 1_100, 11, 1));
        let snapshot = dedupe.snapshot(2_000, Some(20));
        assert!(!snapshot.evaluable);
        assert_eq!(snapshot.dedupe_eviction_count, 1);
        assert!(dedupe.events.len() <= 1);
    }

    #[test]
    fn canonical_permutations_produce_identical_ratio_and_ratio_is_bounded() {
        let events = vec![
            tx("alice", 100, 1_000, 10, 0),
            tx("bob", 100, 1_010, 10, 1),
            tx("alice", -50, 1_020, 11, 2),
        ];
        let mut forward = machine(8, 32);
        for event in &events {
            forward.on_transaction(event);
        }
        let mut reverse = machine(8, 32);
        for event in events.iter().rev() {
            reverse.on_transaction(event);
        }
        let forward = forward.snapshot(2_000, Some(20));
        let reverse = reverse.snapshot(2_000, Some(20));
        assert_eq!(forward.ratio, reverse.ratio);
        assert!(forward
            .ratio
            .is_some_and(|ratio| (0.0..=1.0).contains(&ratio)));
    }

    #[test]
    fn timestamp_slot_conflict_is_non_evaluable() {
        let mut machine = machine(8, 32);
        machine.on_transaction(&tx("alice", 100, 1_000, 20, 0));
        machine.on_transaction(&tx("alice", -50, 1_100, 19, 1));
        let snapshot = machine.snapshot(2_000, Some(30));
        assert!(!snapshot.evaluable);
        assert!(snapshot
            .reasons
            .contains(&FlipEvidenceReasonV1::OutOfOrderEvent));
    }
}
