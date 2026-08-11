//! Minimal bounded detector for owner-to-owner SPL token fan-out.
//!
//! The index is lifecycle-scoped: a mint is tracked only while Oracle observes
//! it pre-buy or while PostBuy owns an open position. It deliberately does not
//! attempt wallet clustering or persist a historical transfer graph.

use crate::events::{TokenRedistributionDetected, TokenRedistributionPhase, TokenTransferObserved};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

const DEFAULT_DISTINCT_RECIPIENT_THRESHOLD: usize = 3;
const MAX_OWNERS_PER_MINT: usize = 32_768;
const MAX_DEDUPE_IDENTITIES_PER_MINT: usize = 65_536;
const HARD_STATE_TTL_MS: u64 = 10 * 60 * 1_000;

fn default_distinct_recipient_threshold() -> usize {
    DEFAULT_DISTINCT_RECIPIENT_THRESHOLD
}

/// The only policy knob for the first deterministic guard iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRedistributionConfig {
    #[serde(default = "default_distinct_recipient_threshold")]
    pub distinct_recipient_threshold: usize,
}

impl Default for TokenRedistributionConfig {
    fn default() -> Self {
        Self {
            distinct_recipient_threshold: DEFAULT_DISTINCT_RECIPIENT_THRESHOLD,
        }
    }
}

impl TokenRedistributionConfig {
    pub fn validate(self) -> Result<Self, String> {
        if self.distinct_recipient_threshold == 0 {
            return Err("token_redistribution.distinct_recipient_threshold must be > 0".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TransferIdentity {
    signature: solana_sdk::signature::Signature,
    event_ordinal: u32,
}

#[derive(Debug, Default)]
struct OutboundState {
    recipients: HashSet<Pubkey>,
    qualified_transfer_count: u64,
    raw_units_sent: u128,
    first_slot: Option<u64>,
    last_slot: Option<u64>,
}

#[derive(Debug)]
struct MintState {
    pool_id: Pubkey,
    phase: TokenRedistributionPhase,
    market_accounts: HashSet<Pubkey>,
    market_owners: HashSet<Pubkey>,
    outbound: HashMap<Pubkey, OutboundState>,
    seen: HashSet<TransferIdentity>,
    seen_fifo: VecDeque<TransferIdentity>,
    created_at_ms: u64,
    last_observed_ms: u64,
    signal: Option<TokenRedistributionDetected>,
    coverage_complete: bool,
}

impl MintState {
    fn new(pool_id: Pubkey, now_ms: u64) -> Self {
        Self {
            pool_id,
            phase: TokenRedistributionPhase::PreBuy,
            market_accounts: HashSet::new(),
            market_owners: HashSet::new(),
            outbound: HashMap::new(),
            seen: HashSet::new(),
            seen_fifo: VecDeque::new(),
            created_at_ms: now_ms,
            last_observed_ms: now_ms,
            signal: None,
            coverage_complete: true,
        }
    }

    fn remember_identity(&mut self, identity: TransferIdentity) -> bool {
        if !self.seen.insert(identity) {
            return false;
        }
        self.seen_fifo.push_back(identity);
        while self.seen_fifo.len() > MAX_DEDUPE_IDENTITIES_PER_MINT {
            if let Some(oldest) = self.seen_fifo.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        true
    }
}

#[derive(Debug)]
struct TokenRedistributionState {
    mints: HashMap<Pubkey, MintState>,
    max_active_mints: usize,
    stream_available: bool,
    coverage_gap: bool,
}

/// Shared bounded state used by Oracle and PostBuyRuntime.
#[derive(Debug, Clone)]
pub struct TokenRedistributionIndex {
    config: TokenRedistributionConfig,
    inner: Arc<Mutex<TokenRedistributionState>>,
}

impl TokenRedistributionIndex {
    pub fn new(config: TokenRedistributionConfig, max_active_mints: usize) -> Self {
        Self {
            config: config
                .validate()
                .expect("validated token redistribution configuration"),
            inner: Arc::new(Mutex::new(TokenRedistributionState {
                mints: HashMap::new(),
                max_active_mints: max_active_mints.max(1),
                stream_available: false,
                coverage_gap: false,
            })),
        }
    }

    pub fn track_pre_buy(
        &self,
        pool_id: Pubkey,
        mint: Pubkey,
        market_accounts: impl IntoIterator<Item = Pubkey>,
        now_ms: u64,
    ) -> bool {
        let mut inner = self.inner.lock();
        Self::prune_locked(&mut inner, now_ms);
        if !inner.mints.contains_key(&mint) && inner.mints.len() >= inner.max_active_mints {
            inner.coverage_gap = true;
            return false;
        }
        let state = inner
            .mints
            .entry(mint)
            .or_insert_with(|| MintState::new(pool_id, now_ms));
        state.pool_id = pool_id;
        state.last_observed_ms = now_ms;
        for account in market_accounts {
            if account != Pubkey::default() {
                state.market_accounts.insert(account);
                state.market_owners.insert(account);
            }
        }
        true
    }

    pub fn add_market_accounts(&self, mint: Pubkey, accounts: impl IntoIterator<Item = Pubkey>) {
        let mut inner = self.inner.lock();
        let Some(state) = inner.mints.get_mut(&mint) else {
            return;
        };
        for account in accounts {
            if account != Pubkey::default() {
                state.market_accounts.insert(account);
                state.market_owners.insert(account);
            }
        }
    }

    pub fn promote_post_buy(&self, mint: &Pubkey) -> bool {
        let mut inner = self.inner.lock();
        let Some(state) = inner.mints.get_mut(mint) else {
            return false;
        };
        state.phase = TokenRedistributionPhase::PostBuy;
        if let Some(signal) = state.signal.as_mut() {
            signal.phase = TokenRedistributionPhase::PostBuy;
        }
        true
    }

    pub fn remove_mint(&self, mint: &Pubkey) -> bool {
        self.inner.lock().mints.remove(mint).is_some()
    }

    pub fn contains_mint(&self, mint: &Pubkey) -> bool {
        self.inner.lock().mints.contains_key(mint)
    }

    pub fn active_mint_count(&self) -> usize {
        self.inner.lock().mints.len()
    }

    pub fn set_stream_available(&self, available: bool) {
        let mut inner = self.inner.lock();
        inner.stream_available = available;
        if !available {
            inner.coverage_gap = true;
            for state in inner.mints.values_mut() {
                state.coverage_complete = false;
            }
        }
    }

    pub fn mark_coverage_gap(&self) {
        let mut inner = self.inner.lock();
        inner.coverage_gap = true;
        for state in inner.mints.values_mut() {
            state.coverage_complete = false;
        }
    }

    pub fn stream_available(&self) -> bool {
        self.inner.lock().stream_available
    }

    pub fn coverage_gap(&self) -> bool {
        self.inner.lock().coverage_gap
    }

    pub fn signal_for_mint(&self, mint: &Pubkey) -> Option<TokenRedistributionDetected> {
        self.inner
            .lock()
            .mints
            .get(mint)
            .and_then(|state| state.signal.clone())
    }

    pub fn prune(&self, now_ms: u64) -> usize {
        let mut inner = self.inner.lock();
        Self::prune_locked(&mut inner, now_ms)
    }

    fn prune_locked(inner: &mut TokenRedistributionState, now_ms: u64) -> usize {
        let before = inner.mints.len();
        inner.mints.retain(|_, state| {
            now_ms.saturating_sub(state.last_observed_ms.max(state.created_at_ms))
                <= HARD_STATE_TTL_MS
        });
        before.saturating_sub(inner.mints.len())
    }

    /// Observe one owner-resolved SPL transfer. Returns a one-shot hard signal
    /// exactly when the configured distinct-recipient threshold is reached.
    pub fn observe(&self, transfer: &TokenTransferObserved) -> Option<TokenRedistributionDetected> {
        if !transfer.full_chain_coverage || transfer.raw_amount == 0 {
            return None;
        }
        if transfer.source_owner == transfer.destination_owner {
            return None;
        }
        let event_ordinal = transfer.event_ordinal?;
        let mut inner = self.inner.lock();
        let state = inner.mints.get_mut(&transfer.mint)?;
        state.last_observed_ms = transfer
            .detected_at
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;

        if state.signal.is_some()
            || state
                .market_accounts
                .contains(&transfer.source_token_account)
            || state
                .market_accounts
                .contains(&transfer.destination_token_account)
            || state.market_owners.contains(&transfer.source_owner)
            || state.market_owners.contains(&transfer.destination_owner)
        {
            return None;
        }

        let identity = TransferIdentity {
            signature: transfer.signature,
            event_ordinal,
        };
        if !state.remember_identity(identity) {
            return None;
        }

        if !state.outbound.contains_key(&transfer.source_owner)
            && state.outbound.len() >= MAX_OWNERS_PER_MINT
        {
            state.coverage_complete = false;
            inner.coverage_gap = true;
            return None;
        }

        let outbound = state.outbound.entry(transfer.source_owner).or_default();
        outbound.qualified_transfer_count = outbound.qualified_transfer_count.saturating_add(1);
        outbound.raw_units_sent = outbound
            .raw_units_sent
            .saturating_add(u128::from(transfer.raw_amount));
        outbound.first_slot = outbound.first_slot.or(transfer.slot);
        outbound.last_slot = transfer.slot.or(outbound.last_slot);
        let is_new_recipient = outbound.recipients.insert(transfer.destination_owner);
        if !is_new_recipient || outbound.recipients.len() < self.config.distinct_recipient_threshold
        {
            return None;
        }

        let signal = TokenRedistributionDetected {
            pool_amm_id: state.pool_id,
            base_mint: transfer.mint,
            source_owner: transfer.source_owner,
            distinct_recipient_count: outbound.recipients.len() as u32,
            phase: state.phase,
            signature: transfer.signature,
            slot: transfer.slot,
            event_ordinal,
            detected_at: transfer.detected_at,
        };
        state.signal = Some(signal.clone());
        Some(signal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::TokenTransferObserved;
    use ghost_core::{EventSemanticEnvelope, EventTimeMetadata};
    use seer::ipc::{FundingLaneRuntimeHealth, FundingTransferProvenance};
    use solana_sdk::signature::Signature;
    use std::time::SystemTime;

    fn transfer(
        mint: Pubkey,
        source_owner: Pubkey,
        destination_owner: Pubkey,
        ordinal: u32,
    ) -> TokenTransferObserved {
        TokenTransferObserved {
            semantic: EventSemanticEnvelope::default(),
            slot: Some(100),
            event_ordinal: Some(ordinal),
            tx_index: Some(0),
            outer_instruction_index: Some(ordinal),
            inner_group_index: None,
            cpi_stack_height: None,
            event_time: EventTimeMetadata::default(),
            arrival_ts_ms: 1_000,
            signature: Signature::new_unique(),
            mint,
            source_token_account: Pubkey::new_unique(),
            destination_token_account: Pubkey::new_unique(),
            source_owner,
            destination_owner,
            raw_amount: 10,
            full_chain_coverage: true,
            provenance: FundingTransferProvenance::authoritative_full_feed_live(),
            lane_health: FundingLaneRuntimeHealth::default(),
            detected_at: SystemTime::now(),
            sequence_number: ordinal as u64,
        }
    }

    #[test]
    fn pre_buy_fan_out_triggers_on_third_distinct_owner() {
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let index = TokenRedistributionIndex::new(TokenRedistributionConfig::default(), 8);
        assert!(index.track_pre_buy(pool, mint, [pool], 1_000));

        assert!(index
            .observe(&transfer(mint, source, Pubkey::new_unique(), 0))
            .is_none());
        assert!(index
            .observe(&transfer(mint, source, Pubkey::new_unique(), 1))
            .is_none());
        let signal = index
            .observe(&transfer(mint, source, Pubkey::new_unique(), 2))
            .expect("third distinct recipient must trigger");
        assert_eq!(signal.phase, TokenRedistributionPhase::PreBuy);
        assert_eq!(signal.distinct_recipient_count, 3);
    }

    #[test]
    fn repeated_recipient_and_duplicate_identity_do_not_fake_fan_out() {
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let index = TokenRedistributionIndex::new(TokenRedistributionConfig::default(), 8);
        index.track_pre_buy(pool, mint, [pool], 1_000);

        let same = transfer(mint, source, recipient, 0);
        assert!(index.observe(&same).is_none());
        assert!(index.observe(&same).is_none());
        assert!(index
            .observe(&transfer(mint, source, recipient, 1))
            .is_none());
        assert!(index
            .observe(&transfer(mint, source, recipient, 2))
            .is_none());
    }

    #[test]
    fn market_legs_self_transfers_and_untracked_mints_are_ignored() {
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let index = TokenRedistributionIndex::new(TokenRedistributionConfig::default(), 8);
        index.track_pre_buy(pool, mint, [pool], 1_000);

        for ordinal in 0..5 {
            assert!(index
                .observe(&transfer(mint, pool, Pubkey::new_unique(), ordinal))
                .is_none());
        }
        assert!(index.observe(&transfer(mint, owner, owner, 10)).is_none());
        assert!(index
            .observe(&transfer(
                Pubkey::new_unique(),
                owner,
                Pubkey::new_unique(),
                11,
            ))
            .is_none());
        assert_eq!(index.active_mint_count(), 1);
    }

    #[test]
    fn post_buy_signal_and_cleanup_follow_lifecycle() {
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let index = TokenRedistributionIndex::new(TokenRedistributionConfig::default(), 8);
        index.track_pre_buy(pool, mint, [pool], 1_000);
        assert!(index.promote_post_buy(&mint));

        for ordinal in 0..2 {
            assert!(index
                .observe(&transfer(mint, source, Pubkey::new_unique(), ordinal))
                .is_none());
        }
        let signal = index
            .observe(&transfer(mint, source, Pubkey::new_unique(), 2))
            .expect("post-buy fan-out must trigger");
        assert_eq!(signal.phase, TokenRedistributionPhase::PostBuy);
        assert!(index.remove_mint(&mint));
        assert!(!index.contains_mint(&mint));
    }

    #[test]
    fn recipient_can_be_followed_as_a_later_source() {
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        let index = TokenRedistributionIndex::new(TokenRedistributionConfig::default(), 8);
        index.track_pre_buy(pool, mint, [pool], 1_000);
        assert!(index.observe(&transfer(mint, a, b, 0)).is_none());
        assert!(index
            .observe(&transfer(mint, b, Pubkey::new_unique(), 1))
            .is_none());
        assert!(index
            .observe(&transfer(mint, b, Pubkey::new_unique(), 2))
            .is_none());
        assert!(index
            .observe(&transfer(mint, b, Pubkey::new_unique(), 3))
            .is_some());
    }
}
