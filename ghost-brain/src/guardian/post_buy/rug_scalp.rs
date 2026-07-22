//! Typed market-fact ingress and pure exit lattice for `rug_scalp_exit_v1`.
//!
//! This module deliberately does **not** receive `TradeEvent`/`PoolTransaction`.
//! The launcher-side reducer/adapter must materialize the small canonical fact
//! contract below.  Position Manager is the only mutable owner after a
//! position has been registered.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// The PM accepts this profile only for the strategy that owns the isolated
/// RUG V2 adapter.  Keeping both identifiers here makes the join contract
/// enforceable without importing launcher policy into `ghost-brain`.
pub const RUG_SCALP_V2_STRATEGY_ID: &str = "rug_scalp_v2";
pub const RUG_SCALP_EXIT_PROFILE_ID: &str = "rug_scalp_exit_v1";
pub const RUG_SCALP_EXIT_PROFILE_VERSION: u16 = 1;
const MAX_RECENT_FACT_IDS: usize = 256;

/// Complete, typed market evidence sent from the RUG adapter to Position
/// Manager.  Numeric values are lamports.  `None` means the adapter could not
/// prove that field, never zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RugScalpMarketFactV1 {
    pub position_id: String,
    pub mint: Pubkey,
    pub slot: u64,
    pub tx_index: Option<u32>,
    pub event_ordinal: Option<u32>,
    pub fact_kind: RugScalpMarketFactKindV1,
    pub successful_buy_count_in_slot: u32,
    pub sell_quote_lamports: Option<u64>,
    pub reserve_before: Option<u64>,
    pub reserve_after: Option<u64>,
    pub executable_position_value_before: Option<u64>,
    pub executable_position_value_after: Option<u64>,
    pub data_completeness: RugScalpDataCompletenessV1,
}

/// Immutable canonical boundary at which the RUG primary position becomes
/// eligible to consume market facts.  A modelled fill may prove only its RPC
/// slot; a confirmed canonical fill can additionally prove tx/event order.
/// The latter is required to distinguish earlier and later facts in one slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RugScalpEntryWatermarkV1 {
    pub slot: u64,
    pub tx_index: Option<u32>,
    pub event_ordinal: Option<u32>,
}

impl RugScalpEntryWatermarkV1 {
    pub const fn modelled(slot: u64) -> Self {
        Self {
            slot,
            tx_index: None,
            event_ordinal: None,
        }
    }

    pub const fn canonical(slot: u64, tx_index: u32, event_ordinal: u32) -> Self {
        Self {
            slot,
            tx_index: Some(tx_index),
            event_ordinal: Some(event_ordinal),
        }
    }

    fn classify_ordered_fact(&self, fact: &RugScalpMarketFactV1) -> EntryFactOrderingV1 {
        if fact.slot < self.slot {
            return EntryFactOrderingV1::BeforeEntry;
        }
        if fact.slot > self.slot {
            return EntryFactOrderingV1::AfterEntry;
        }
        match (
            self.tx_index,
            self.event_ordinal,
            fact.tx_index,
            fact.event_ordinal,
        ) {
            (Some(entry_tx), Some(entry_ordinal), Some(fact_tx), Some(fact_ordinal)) => {
                if (fact_tx, fact_ordinal) <= (entry_tx, entry_ordinal) {
                    EntryFactOrderingV1::BeforeEntry
                } else {
                    EntryFactOrderingV1::AfterEntry
                }
            }
            _ => EntryFactOrderingV1::AmbiguousSameSlot,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryFactOrderingV1 {
    BeforeEntry,
    AfterEntry,
    AmbiguousSameSlot,
}

impl RugScalpMarketFactV1 {
    /// Stable idempotency key.  It includes the explicit source ordering
    /// coordinates and cannot be derived from processing time.
    pub fn dedupe_key(&self) -> String {
        format!(
            "{}:{}:{}:{:?}:{:?}:{:?}",
            self.position_id,
            self.mint,
            self.slot,
            self.fact_kind,
            self.tx_index,
            self.event_ordinal
        )
    }

    pub fn validates_shape(&self) -> bool {
        if self.position_id.trim().is_empty() {
            return false;
        }
        match self.fact_kind {
            RugScalpMarketFactKindV1::SlotComplete => {
                matches!(self.data_completeness, RugScalpDataCompletenessV1::Complete)
            }
            RugScalpMarketFactKindV1::DataGap => {
                !matches!(self.data_completeness, RugScalpDataCompletenessV1::Complete)
            }
            RugScalpMarketFactKindV1::SuccessfulBuy | RugScalpMarketFactKindV1::SuccessfulSell => {
                self.tx_index.is_some() && self.event_ordinal.is_some()
            }
            RugScalpMarketFactKindV1::RouteStateChanged => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpMarketFactKindV1 {
    SuccessfulBuy,
    SuccessfulSell,
    SlotComplete,
    DataGap,
    RouteStateChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpDataCompletenessV1 {
    Complete,
    Incomplete,
    Gap,
    Unknown,
}

impl RugScalpDataCompletenessV1 {
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// The profile is intentionally self-contained.  It does not reuse the
/// similarly shaped CrashGuard or TimeStop thresholds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RugScalpExitProfileConfigV1 {
    pub enabled: bool,
    pub profile_id: String,
    pub material_sell_reserve_drain_bps: u32,
    pub material_sell_position_value_drop_bps: u32,
    pub profit_min_net_bps: i32,
    pub hard_stop_net_bps: i32,
    pub max_hold_slots: u64,
    pub max_hold_ms: u64,
    pub flow_stop_empty_slots: u8,
    /// Frozen PRIMARY exit delay.  `0` is a valid empirically measured
    /// baseline; STRESS_1/2 stay outside the primary PM lifecycle.
    pub primary_exit_latency_slots: u64,
    /// Costs excluded from the Pump program quote and charged exactly once.
    pub entry_fixed_cost_lamports: u64,
    pub exit_fixed_cost_lamports: u64,
}

impl Default for RugScalpExitProfileConfigV1 {
    fn default() -> Self {
        Self {
            enabled: false,
            profile_id: RUG_SCALP_EXIT_PROFILE_ID.to_string(),
            material_sell_reserve_drain_bps: 500,
            material_sell_position_value_drop_bps: 1_500,
            profit_min_net_bps: 1_000,
            hard_stop_net_bps: -500,
            max_hold_slots: 8,
            max_hold_ms: 5_000,
            flow_stop_empty_slots: 2,
            primary_exit_latency_slots: 0,
            entry_fixed_cost_lamports: 0,
            exit_fixed_cost_lamports: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RugScalpExitProfileConfigErrorV1 {
    #[error("rug_scalp_exit_v1 profile_id mismatch")]
    ProfileIdMismatch,
    #[error("rug_scalp_exit_v1 material sell thresholds must be positive")]
    InvalidMaterialSellThreshold,
    #[error("rug_scalp_exit_v1 net thresholds must be +10%/-5%")]
    InvalidNetThreshold,
    #[error("rug_scalp_exit_v1 max hold must be 8 slots / 5000ms")]
    InvalidMaxHold,
    #[error("rug_scalp_exit_v1 flow stop must require two complete empty slots")]
    InvalidFlowStop,
}

impl RugScalpExitProfileConfigV1 {
    pub fn validate(&self) -> Result<(), RugScalpExitProfileConfigErrorV1> {
        if self.profile_id != RUG_SCALP_EXIT_PROFILE_ID {
            return Err(RugScalpExitProfileConfigErrorV1::ProfileIdMismatch);
        }
        if self.material_sell_reserve_drain_bps != 500
            || self.material_sell_position_value_drop_bps != 1_500
        {
            return Err(RugScalpExitProfileConfigErrorV1::InvalidMaterialSellThreshold);
        }
        if self.profit_min_net_bps != 1_000 || self.hard_stop_net_bps != -500 {
            return Err(RugScalpExitProfileConfigErrorV1::InvalidNetThreshold);
        }
        if self.max_hold_slots != 8 || self.max_hold_ms != 5_000 {
            return Err(RugScalpExitProfileConfigErrorV1::InvalidMaxHold);
        }
        if self.flow_stop_empty_slots != 2 {
            return Err(RugScalpExitProfileConfigErrorV1::InvalidFlowStop);
        }
        Ok(())
    }
}

/// Typed reason produced by the RUG profile.  These names are intentionally
/// distinct from legacy manager reasons so logs/replay cannot imply a
/// CrashGuard or TimeStop decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpExitReasonV1 {
    PendingReconciliation,
    DataIdentityRouteBlocked,
    MaterialSellEmergency,
    TargetReached10PctNet,
    BaselineHardLoss5PctNet,
    FlowExhausted,
    MaxHold,
    Hold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RugScalpFactIngressResultV1 {
    Applied,
    Duplicate,
    /// The fact is chain-canonically earlier than the registered entry fill.
    /// It is deliberately ignored rather than becoming post-entry evidence.
    IgnoredPreEntry,
    RejectedInvalidFact,
    RejectedUnknownPosition,
    RejectedProfileMismatch,
}

#[derive(Debug, Clone, Default)]
struct RugScalpSlotAccumulatorV1 {
    successful_buy_count: u32,
    reserve_before: Option<u64>,
    reserve_after: Option<u64>,
    executable_before: Option<u64>,
    executable_after: Option<u64>,
    saw_sell: bool,
}

impl RugScalpSlotAccumulatorV1 {
    fn observe(&mut self, fact: &RugScalpMarketFactV1) {
        self.successful_buy_count = self
            .successful_buy_count
            .max(fact.successful_buy_count_in_slot);
        if matches!(fact.fact_kind, RugScalpMarketFactKindV1::SuccessfulBuy) {
            self.successful_buy_count = self.successful_buy_count.max(1);
        }
        if matches!(fact.fact_kind, RugScalpMarketFactKindV1::SuccessfulSell) {
            self.saw_sell = true;
            if self.reserve_before.is_none() {
                self.reserve_before = fact.reserve_before;
            }
            if self.executable_before.is_none() {
                self.executable_before = fact.executable_position_value_before;
            }
            if fact.reserve_after.is_some() {
                self.reserve_after = fact.reserve_after;
            }
            if fact.executable_position_value_after.is_some() {
                self.executable_after = fact.executable_position_value_after;
            }
        }
    }

    fn material_sell(&self, profile: &RugScalpExitProfileConfigV1) -> bool {
        let reserve_drain = self
            .reserve_before
            .zip(self.reserve_after)
            .filter(|(before, after)| *before > 0 && after < before)
            .is_some_and(|(before, after)| {
                (before.saturating_sub(after) as u128).saturating_mul(10_000)
                    >= (before as u128)
                        .saturating_mul(profile.material_sell_reserve_drain_bps as u128)
            });
        let value_drop = self
            .executable_before
            .zip(self.executable_after)
            .filter(|(before, after)| *before > 0 && after < before)
            .is_some_and(|(before, after)| {
                (before.saturating_sub(after) as u128).saturating_mul(10_000)
                    >= (before as u128)
                        .saturating_mul(profile.material_sell_position_value_drop_bps as u128)
            });
        reserve_drain || value_drop
    }
}

/// Position-owned state.  The manager owns it and mutates it only through
/// [`Self::apply_fact`].  It retains a bounded duplicate cache and no raw
/// trades, transactions, or unbounded historical path.
#[derive(Debug, Clone)]
pub struct RugScalpMarketFactStateV1 {
    position_id: String,
    mint: Pubkey,
    recent_fact_ids: VecDeque<String>,
    slots: BTreeMap<u64, RugScalpSlotAccumulatorV1>,
    last_completed_slot: Option<u64>,
    /// Last canonical trade order consumed by this position.  Facts are a
    /// lifecycle evidence stream, so a late lower-order trade is a data
    /// integrity failure rather than an opportunity to rewrite valuation.
    last_accepted_trade_order: Option<(u64, u32, u32)>,
    material_sell_due: bool,
    consecutive_empty_complete_slots: u8,
    data_or_route_blocked: bool,
    entry_watermark: Option<RugScalpEntryWatermarkV1>,
    /// Latest adapter-materialised *net executable* full-position value.
    /// It is produced by the typed Pump route contract, not reconstructed by
    /// Position Manager from a mark or the historical one-percent simulator.
    latest_executable_position_value_lamports: Option<u64>,
    /// Canonical slot of the fact that produced the current executable value.
    /// It lets the PM audit the typed `LegacySell` quote without claiming that
    /// a generic `MarketSnapshot` was the pricing authority.
    latest_executable_position_value_slot: Option<u64>,
    /// Highest accepted canonical fact slot.  This is separate from the
    /// executable-value slot: a later `SLOT_COMPLETE` can advance lifecycle
    /// latency without fabricating a fresh executable quote.
    latest_observed_slot: Option<u64>,
}

impl RugScalpMarketFactStateV1 {
    pub fn new(position_id: String, mint: Pubkey) -> Self {
        Self {
            position_id,
            mint,
            recent_fact_ids: VecDeque::with_capacity(MAX_RECENT_FACT_IDS),
            slots: BTreeMap::new(),
            last_completed_slot: None,
            last_accepted_trade_order: None,
            material_sell_due: false,
            consecutive_empty_complete_slots: 0,
            data_or_route_blocked: false,
            entry_watermark: None,
            latest_executable_position_value_lamports: None,
            latest_executable_position_value_slot: None,
            latest_observed_slot: None,
        }
    }

    pub fn with_entry_watermark(
        position_id: String,
        mint: Pubkey,
        entry_watermark: RugScalpEntryWatermarkV1,
    ) -> Self {
        let mut state = Self::new(position_id, mint);
        state.entry_watermark = Some(entry_watermark);
        state
    }

    pub fn apply_fact(
        &mut self,
        fact: RugScalpMarketFactV1,
        profile: &RugScalpExitProfileConfigV1,
    ) -> RugScalpFactIngressResultV1 {
        if fact.position_id != self.position_id || fact.mint != self.mint || !fact.validates_shape()
        {
            return RugScalpFactIngressResultV1::RejectedInvalidFact;
        }
        // SLOT_COMPLETE represents the end of the slot and DATA/ROUTE facts
        // are blockers, so both are safe to consume at the entry slot.  Only
        // ordered trade facts require an entry-watermark comparison.
        if matches!(
            fact.fact_kind,
            RugScalpMarketFactKindV1::SuccessfulBuy | RugScalpMarketFactKindV1::SuccessfulSell
        ) {
            if let Some(watermark) = self.entry_watermark {
                match watermark.classify_ordered_fact(&fact) {
                    EntryFactOrderingV1::BeforeEntry => {
                        return RugScalpFactIngressResultV1::IgnoredPreEntry;
                    }
                    EntryFactOrderingV1::AmbiguousSameSlot => {
                        // Do not guess whether a same-slot dump occurred
                        // before or after a modelled fill.  This is a typed
                        // evidence failure, never an optimistic omission.
                        self.data_or_route_blocked = true;
                        self.consecutive_empty_complete_slots = 0;
                        return RugScalpFactIngressResultV1::Applied;
                    }
                    EntryFactOrderingV1::AfterEntry => {}
                }
            }
        }
        let fact_id = fact.dedupe_key();
        if self.recent_fact_ids.iter().any(|seen| seen == &fact_id) {
            return RugScalpFactIngressResultV1::Duplicate;
        }
        if matches!(
            fact.fact_kind,
            RugScalpMarketFactKindV1::SuccessfulBuy | RugScalpMarketFactKindV1::SuccessfulSell
        ) {
            let order = (
                fact.slot,
                fact.tx_index.unwrap_or_default(),
                fact.event_ordinal.unwrap_or_default(),
            );
            let behind_completed_slot = self
                .last_completed_slot
                .is_some_and(|slot| fact.slot <= slot);
            let non_monotonic_trade = self
                .last_accepted_trade_order
                .is_some_and(|last| order <= last);
            if behind_completed_slot || non_monotonic_trade {
                // There is no safe replay buffer inside PM for a fact whose
                // canonical order has already been passed.  Preserve a typed
                // evidence failure instead of letting a late fact overwrite
                // the latest executable value.
                self.data_or_route_blocked = true;
                self.consecutive_empty_complete_slots = 0;
                return RugScalpFactIngressResultV1::RejectedInvalidFact;
            }
        }
        self.recent_fact_ids.push_back(fact_id);
        if self.recent_fact_ids.len() > MAX_RECENT_FACT_IDS {
            self.recent_fact_ids.pop_front();
        }
        match fact.fact_kind {
            RugScalpMarketFactKindV1::DataGap => {
                // A data gap never creates a synthetic empty slot.  The
                // blocker is sticky for this position; a later complete slot
                // cannot prove that the lost interval was harmless.
                self.data_or_route_blocked = true;
                self.consecutive_empty_complete_slots = 0;
            }
            RugScalpMarketFactKindV1::RouteStateChanged => {
                if !fact.data_completeness.is_complete() {
                    self.data_or_route_blocked = true;
                }
            }
            RugScalpMarketFactKindV1::SuccessfulBuy | RugScalpMarketFactKindV1::SuccessfulSell => {
                if let Some(value) = fact.executable_position_value_after {
                    self.latest_executable_position_value_lamports = Some(value);
                    self.latest_executable_position_value_slot = Some(fact.slot);
                }
                self.last_accepted_trade_order = Some((
                    fact.slot,
                    fact.tx_index.unwrap_or_default(),
                    fact.event_ordinal.unwrap_or_default(),
                ));
                self.slots.entry(fact.slot).or_default().observe(&fact);
            }
            RugScalpMarketFactKindV1::SlotComplete => {
                let last_completed = self.last_completed_slot;
                if last_completed.is_some_and(|last| fact.slot <= last) {
                    return RugScalpFactIngressResultV1::RejectedInvalidFact;
                }
                if last_completed.is_some_and(|last| fact.slot > last.saturating_add(1)) {
                    self.data_or_route_blocked = true;
                    self.consecutive_empty_complete_slots = 0;
                }
                if self
                    .last_accepted_trade_order
                    .is_some_and(|(slot, _, _)| slot > fact.slot)
                {
                    self.data_or_route_blocked = true;
                    self.consecutive_empty_complete_slots = 0;
                    return RugScalpFactIngressResultV1::RejectedInvalidFact;
                }
                let mut slot = self.slots.remove(&fact.slot).unwrap_or_default();
                // `SLOT_COMPLETE` carries the adapter's canonical aggregate as
                // well.  This keeps the PM robust if individual buy facts were
                // coalesced upstream while preserving the same slot semantics.
                slot.successful_buy_count = slot
                    .successful_buy_count
                    .max(fact.successful_buy_count_in_slot);
                // Empty completed slots may still carry a canonical
                // full-position `LegacySell` valuation.  This is essential
                // for a FLOW_EXHAUSTED close: no buy occurred in either slot,
                // but PM must not fall back to a mark-price estimate.
                if let Some(value) = fact.executable_position_value_after {
                    self.latest_executable_position_value_lamports = Some(value);
                    self.latest_executable_position_value_slot = Some(fact.slot);
                }
                self.last_completed_slot = Some(fact.slot);
                if slot.material_sell(profile) {
                    self.material_sell_due = true;
                }
                if slot.successful_buy_count == 0 {
                    self.consecutive_empty_complete_slots =
                        self.consecutive_empty_complete_slots.saturating_add(1);
                } else {
                    self.consecutive_empty_complete_slots = 0;
                }
                // We retain at most the current open slot.  Older incomplete
                // accumulators are no longer trustworthy after a completion.
                self.slots.retain(|slot_id, _| *slot_id > fact.slot);
            }
        }
        // Only a fact that fully passed its variant-specific ordering checks
        // is allowed to advance the lifecycle clock.  In particular, a
        // rejected stale `SLOT_COMPLETE` must not shorten the frozen latency.
        self.latest_observed_slot = Some(
            self.latest_observed_slot
                .map_or(fact.slot, |slot| slot.max(fact.slot)),
        );
        RugScalpFactIngressResultV1::Applied
    }

    pub fn blocker_active(&self) -> bool {
        self.data_or_route_blocked
    }

    pub fn material_sell_due(&self) -> bool {
        self.material_sell_due
    }

    pub fn flow_exhausted(&self, profile: &RugScalpExitProfileConfigV1) -> bool {
        self.consecutive_empty_complete_slots >= profile.flow_stop_empty_slots
    }

    /// The value includes the typed route's program settlement and explicit
    /// transaction-envelope debit exactly once.  `None` is an evidence gap,
    /// never a zero-valued executable position.
    pub fn latest_executable_position_value_lamports(&self) -> Option<u64> {
        self.latest_executable_position_value_lamports
    }

    /// Slot in which the current exact executable value was materialised.
    pub fn latest_executable_position_value_slot(&self) -> Option<u64> {
        self.latest_executable_position_value_slot
    }

    /// Last accepted fact slot, used solely to advance the RUG lifecycle's
    /// frozen slot-latency boundary.
    pub fn latest_observed_slot(&self) -> Option<u64> {
        self.latest_observed_slot
    }
}

/// Pure precedence lattice.  `net_return_bps` must come from an exact
/// executable quote at the frozen exit latency, never a mark price.
pub fn evaluate_rug_scalp_exit_v1(
    facts: &RugScalpMarketFactStateV1,
    profile: &RugScalpExitProfileConfigV1,
    pending_or_reconciling: bool,
    identity_data_or_route_blocked: bool,
    net_return_bps: Option<i32>,
    entry_slot: Option<u64>,
    observed_slot: Option<u64>,
    age_ms: u64,
) -> RugScalpExitReasonV1 {
    if pending_or_reconciling {
        return RugScalpExitReasonV1::PendingReconciliation;
    }
    if identity_data_or_route_blocked || facts.blocker_active() {
        return RugScalpExitReasonV1::DataIdentityRouteBlocked;
    }
    // Completion is the evidence boundary: `material_sell_due` is set only
    // when SLOT_COMPLETE is applied.  This deliberately outranks a target
    // seen in the same slot (DUMP_WINS).
    if facts.material_sell_due() {
        return RugScalpExitReasonV1::MaterialSellEmergency;
    }
    if net_return_bps.is_some_and(|value| value >= profile.profit_min_net_bps) {
        return RugScalpExitReasonV1::TargetReached10PctNet;
    }
    if net_return_bps.is_some_and(|value| value <= profile.hard_stop_net_bps) {
        return RugScalpExitReasonV1::BaselineHardLoss5PctNet;
    }
    if facts.flow_exhausted(profile) {
        return RugScalpExitReasonV1::FlowExhausted;
    }
    let slot_age_due = entry_slot
        .zip(observed_slot)
        .is_some_and(|(entry, observed)| observed >= entry.saturating_add(profile.max_hold_slots));
    if slot_age_due || age_ms >= profile.max_hold_ms {
        return RugScalpExitReasonV1::MaxHold;
    }
    RugScalpExitReasonV1::Hold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position() -> (String, Pubkey) {
        ("rug-position-1".to_string(), Pubkey::new_unique())
    }

    fn fact(
        position_id: &str,
        mint: Pubkey,
        slot: u64,
        fact_kind: RugScalpMarketFactKindV1,
    ) -> RugScalpMarketFactV1 {
        RugScalpMarketFactV1 {
            position_id: position_id.to_string(),
            mint,
            slot,
            tx_index: Some(1),
            event_ordinal: Some(0),
            fact_kind,
            successful_buy_count_in_slot: 0,
            sell_quote_lamports: None,
            reserve_before: None,
            reserve_after: None,
            executable_position_value_before: None,
            executable_position_value_after: None,
            data_completeness: RugScalpDataCompletenessV1::Complete,
        }
    }

    fn complete(position_id: &str, mint: Pubkey, slot: u64, buys: u32) -> RugScalpMarketFactV1 {
        let mut value = fact(
            position_id,
            mint,
            slot,
            RugScalpMarketFactKindV1::SlotComplete,
        );
        value.tx_index = None;
        value.event_ordinal = None;
        value.successful_buy_count_in_slot = buys;
        value
    }

    #[test]
    fn material_sell_waits_for_complete_slot_and_beats_target() {
        let (position_id, mint) = position();
        let profile = RugScalpExitProfileConfigV1::default();
        let mut state = RugScalpMarketFactStateV1::new(position_id.clone(), mint);
        let mut sell = fact(
            &position_id,
            mint,
            99,
            RugScalpMarketFactKindV1::SuccessfulSell,
        );
        sell.reserve_before = Some(1_000);
        sell.reserve_after = Some(940);
        assert_eq!(
            state.apply_fact(sell, &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert!(!state.material_sell_due());
        assert_eq!(
            evaluate_rug_scalp_exit_v1(
                &state,
                &profile,
                false,
                false,
                Some(2_000),
                Some(90),
                Some(99),
                100
            ),
            RugScalpExitReasonV1::TargetReached10PctNet
        );
        assert_eq!(
            state.apply_fact(complete(&position_id, mint, 99, 1), &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert_eq!(
            evaluate_rug_scalp_exit_v1(
                &state,
                &profile,
                false,
                false,
                Some(2_000),
                Some(90),
                Some(99),
                100
            ),
            RugScalpExitReasonV1::MaterialSellEmergency
        );
    }

    #[test]
    fn two_complete_empty_slots_trigger_flow_and_gap_does_not_count() {
        let (position_id, mint) = position();
        let profile = RugScalpExitProfileConfigV1::default();
        let mut state = RugScalpMarketFactStateV1::new(position_id.clone(), mint);
        state.apply_fact(complete(&position_id, mint, 11, 0), &profile);
        assert!(!state.flow_exhausted(&profile));
        let mut gap = fact(&position_id, mint, 12, RugScalpMarketFactKindV1::DataGap);
        gap.tx_index = None;
        gap.event_ordinal = None;
        gap.data_completeness = RugScalpDataCompletenessV1::Gap;
        state.apply_fact(gap, &profile);
        assert!(!state.flow_exhausted(&profile));
        assert!(state.blocker_active());
        state.apply_fact(complete(&position_id, mint, 12, 0), &profile);
        assert!(!state.flow_exhausted(&profile));
        assert_eq!(
            evaluate_rug_scalp_exit_v1(&state, &profile, false, false, None, Some(10), Some(12), 1),
            RugScalpExitReasonV1::DataIdentityRouteBlocked
        );
    }

    #[test]
    fn duplicate_fact_cannot_create_second_exit_condition() {
        let (position_id, mint) = position();
        let profile = RugScalpExitProfileConfigV1::default();
        let mut state = RugScalpMarketFactStateV1::new(position_id.clone(), mint);
        let first = complete(&position_id, mint, 20, 0);
        assert_eq!(
            state.apply_fact(first.clone(), &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert_eq!(
            state.apply_fact(first, &profile),
            RugScalpFactIngressResultV1::Duplicate
        );
        assert!(!state.flow_exhausted(&profile));
        state.apply_fact(complete(&position_id, mint, 21, 0), &profile);
        assert!(state.flow_exhausted(&profile));
    }

    #[test]
    fn entry_watermark_ignores_earlier_facts_and_invalidates_ambiguous_same_slot() {
        let (position_id, mint) = position();
        let profile = RugScalpExitProfileConfigV1::default();
        let mut state = RugScalpMarketFactStateV1::with_entry_watermark(
            position_id.clone(),
            mint,
            RugScalpEntryWatermarkV1::canonical(50, 5, 1),
        );
        let mut earlier = fact(
            &position_id,
            mint,
            50,
            RugScalpMarketFactKindV1::SuccessfulSell,
        );
        earlier.tx_index = Some(5);
        earlier.event_ordinal = Some(0);
        earlier.reserve_before = Some(1_000);
        earlier.reserve_after = Some(800);
        assert_eq!(
            state.apply_fact(earlier, &profile),
            RugScalpFactIngressResultV1::IgnoredPreEntry
        );
        assert!(!state.material_sell_due());

        let mut later = fact(
            &position_id,
            mint,
            50,
            RugScalpMarketFactKindV1::SuccessfulSell,
        );
        later.tx_index = Some(5);
        later.event_ordinal = Some(2);
        later.reserve_before = Some(1_000);
        later.reserve_after = Some(800);
        assert_eq!(
            state.apply_fact(later, &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert_eq!(
            state.apply_fact(complete(&position_id, mint, 50, 0), &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert!(state.material_sell_due());

        let mut modelled_state = RugScalpMarketFactStateV1::with_entry_watermark(
            position_id.clone(),
            mint,
            RugScalpEntryWatermarkV1::modelled(60),
        );
        let ambiguous = fact(
            &position_id,
            mint,
            60,
            RugScalpMarketFactKindV1::SuccessfulSell,
        );
        assert_eq!(
            modelled_state.apply_fact(ambiguous, &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert!(
            modelled_state.blocker_active(),
            "same-slot modelled-fill tie must be DATA_INVALIDATED, never guessed"
        );
    }

    #[test]
    fn canonical_slot_complete_aggregate_prevents_false_empty_flow() {
        let (position_id, mint) = position();
        let profile = RugScalpExitProfileConfigV1::default();
        let mut state = RugScalpMarketFactStateV1::new(position_id.clone(), mint);

        // The adapter is allowed to coalesce individual BUY facts but must
        // carry its canonical aggregate on SLOT_COMPLETE.  PM must not turn
        // that observed buy into a synthetic empty slot.
        state.apply_fact(complete(&position_id, mint, 30, 1), &profile);
        state.apply_fact(complete(&position_id, mint, 31, 0), &profile);
        assert!(!state.flow_exhausted(&profile));
        assert_eq!(
            evaluate_rug_scalp_exit_v1(
                &state,
                &profile,
                false,
                false,
                Some(0),
                Some(29),
                Some(31),
                100,
            ),
            RugScalpExitReasonV1::Hold
        );
    }

    #[test]
    fn slot_complete_carries_the_only_valid_empty_slot_exit_valuation() {
        let (position_id, mint) = position();
        let profile = RugScalpExitProfileConfigV1::default();
        let mut state = RugScalpMarketFactStateV1::new(position_id.clone(), mint);
        let mut empty_complete = complete(&position_id, mint, 41, 0);
        empty_complete.executable_position_value_after = Some(987_654);

        assert_eq!(
            state.apply_fact(empty_complete, &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert_eq!(
            state.latest_executable_position_value_lamports(),
            Some(987_654)
        );
        assert_eq!(state.latest_executable_position_value_slot(), Some(41));
        assert_eq!(state.latest_observed_slot(), Some(41));
    }

    #[test]
    fn late_trade_after_slot_complete_invalidates_instead_of_repricing_position() {
        let (position_id, mint) = position();
        let profile = RugScalpExitProfileConfigV1::default();
        let mut state = RugScalpMarketFactStateV1::new(position_id.clone(), mint);
        let mut accepted = fact(
            &position_id,
            mint,
            51,
            RugScalpMarketFactKindV1::SuccessfulSell,
        );
        accepted.executable_position_value_after = Some(900);
        assert_eq!(
            state.apply_fact(accepted, &profile),
            RugScalpFactIngressResultV1::Applied
        );
        assert_eq!(
            state.apply_fact(complete(&position_id, mint, 51, 0), &profile),
            RugScalpFactIngressResultV1::Applied
        );

        let mut late = fact(
            &position_id,
            mint,
            51,
            RugScalpMarketFactKindV1::SuccessfulSell,
        );
        late.tx_index = Some(2);
        late.executable_position_value_after = Some(1_500);
        assert_eq!(
            state.apply_fact(late, &profile),
            RugScalpFactIngressResultV1::RejectedInvalidFact
        );
        assert!(state.blocker_active());
        assert_eq!(
            state.latest_executable_position_value_lamports(),
            Some(900),
            "a late higher valuation must not rewrite the completed canonical slot"
        );
    }

    #[test]
    fn profile_rejects_semantically_similar_legacy_values() {
        let mut profile = RugScalpExitProfileConfigV1::default();
        profile.material_sell_reserve_drain_bps = 2_500;
        assert_eq!(
            profile.validate(),
            Err(RugScalpExitProfileConfigErrorV1::InvalidMaterialSellThreshold)
        );
    }
}
