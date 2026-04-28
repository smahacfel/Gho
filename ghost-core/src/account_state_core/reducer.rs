use super::monotonic_guard::MonotonicUpdateGuard;
use super::types::{
    AccountStateFeatures, AccountStateUpdate, AccountUpdateRejectReason, AccountUpdateResult,
    BootstrapHints, BootstrapPoolState, CanonicalPoolState, StatePhase,
};
use crate::market_state::BondingCurve;
use crate::PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY;
use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;
const PUMP_TOKEN_DECIMAL_FACTOR_F64: f64 = 1_000_000.0;

#[derive(Debug, Default)]
pub struct AccountStateReducer {
    states: DashMap<Pubkey, CanonicalPoolState>,
    update_guards: DashMap<Pubkey, MonotonicUpdateGuard>,
    bootstrap_states: DashMap<Pubkey, BootstrapPoolState>,
    recv_seq_counter: AtomicU64,
    latest_observed_slot: AtomicU64,
}

impl AccountStateReducer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_pool_from_bootstrap(
        &self,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        hints: BootstrapHints,
    ) {
        self.bootstrap_states.insert(
            base_mint,
            BootstrapPoolState {
                pool_amm_id,
                base_mint,
                bonding_curve,
                speculative_reserves: hints.speculative_reserves,
                token_total_supply: hints.token_total_supply,
                bonding_curve_progress: hints.bonding_curve_progress,
                initial_liquidity_sol: hints.initial_liquidity_sol,
                created_at_ms: current_time_ms(),
            },
        );
    }

    #[must_use]
    pub fn apply_account_update(&self, update: AccountStateUpdate) -> AccountUpdateResult {
        let mut guard = self.update_guards.entry(update.base_mint).or_default();
        let last_slot = guard.last_accepted_slot;
        let last_recv_seq = guard.last_accepted_recv_seq;
        if !guard.accept(update.slot, update.write_version, update.receive_seq) {
            return AccountUpdateResult::Rejected(rejection_reason(
                last_slot,
                last_recv_seq,
                update.slot,
                update.receive_seq,
            ));
        }
        drop(guard);

        let bootstrap = self
            .bootstrap_states
            .get(&update.base_mint)
            .map(|entry| entry.clone());

        let previous_state = self
            .states
            .get(&update.base_mint)
            .map(|entry| entry.clone());
        let token_total_supply = previous_state
            .as_ref()
            .map(|state| state.token_total_supply)
            .or_else(|| {
                bootstrap
                    .as_ref()
                    .and_then(|state| state.token_total_supply)
            })
            .unwrap_or(PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY);

        let curve = bonding_curve_from_update(&update, token_total_supply);
        let price_sol = normalized_price_sol(&curve);
        let market_cap_sol = normalized_market_cap_sol(&curve);
        let bonding_curve_progress = curve.get_bonding_progress() as f64 / 100.0;
        let is_complete = update.is_complete != 0;
        let state_phase = if is_complete {
            StatePhase::Migrated
        } else {
            StatePhase::Canonical
        };

        let (
            initial_price_sol,
            price_change_since_t0_pct,
            reserve_velocity_sol_per_sec,
            update_count,
        ) = if let Some(previous) = previous_state.as_ref() {
            let initial_price_sol =
                normalize_initial_price(previous.initial_price_sol, previous.price_sol);
            let reserve_velocity_sol_per_sec = compute_reserve_velocity_sol_per_sec(
                previous.real_sol_reserves,
                curve.real_sol_reserves,
                previous.last_update_ts_ms,
                update.receive_ts_ms,
            );
            (
                initial_price_sol,
                compute_price_change_pct(initial_price_sol, price_sol),
                reserve_velocity_sol_per_sec,
                previous.update_count.saturating_add(1),
            )
        } else {
            (price_sol, 0.0, 0.0, 1)
        };

        let pool_amm_id = bootstrap
            .as_ref()
            .map(|state| state.pool_amm_id)
            .unwrap_or(update.pool_amm_id);
        let bonding_curve = bootstrap
            .as_ref()
            .map(|state| state.bonding_curve)
            .unwrap_or(update.bonding_curve);

        self.states.insert(
            update.base_mint,
            CanonicalPoolState {
                pool_amm_id,
                base_mint: update.base_mint,
                bonding_curve,
                virtual_sol_reserves: curve.virtual_sol_reserves,
                virtual_token_reserves: curve.virtual_token_reserves,
                real_sol_reserves: curve.real_sol_reserves,
                real_token_reserves: curve.real_token_reserves,
                bonding_curve_progress,
                price_sol,
                market_cap_sol,
                token_total_supply,
                is_complete,
                last_update_slot: update.slot,
                last_update_ts_ms: update.receive_ts_ms,
                curve_finality: update.curve_finality,
                state_phase,
                update_count,
                initial_price_sol,
                price_change_since_t0_pct,
                reserve_velocity_sol_per_sec,
            },
        );
        self.latest_observed_slot
            .fetch_max(update.slot, Ordering::Relaxed);

        if bootstrap.is_some() {
            self.bootstrap_states.remove(&update.base_mint);
            AccountUpdateResult::PromotedFromBootstrap
        } else {
            AccountUpdateResult::Applied
        }
    }

    #[must_use]
    pub fn get_canonical_state(&self, mint: &Pubkey) -> Option<CanonicalPoolState> {
        self.states.get(mint).map(|entry| entry.clone())
    }

    #[must_use]
    pub fn bonding_curve(&self, mint: &Pubkey) -> Option<BondingCurve> {
        self.states
            .get(mint)
            .map(|entry| bonding_curve_from_canonical_state(&entry))
    }

    #[must_use]
    pub fn get_bootstrap_state(&self, mint: &Pubkey) -> Option<BootstrapPoolState> {
        self.bootstrap_states.get(mint).map(|entry| entry.clone())
    }

    #[must_use]
    pub fn get_features(&self, mint: &Pubkey) -> Option<AccountStateFeatures> {
        let state = self.states.get(mint)?;
        Some(AccountStateFeatures {
            current_reserves: (state.virtual_sol_reserves, state.virtual_token_reserves),
            price_sol: state.price_sol,
            market_cap_sol: state.market_cap_sol,
            bonding_progress: state.bonding_curve_progress,
            price_change_since_t0_pct: state.price_change_since_t0_pct,
            reserve_velocity_sol_per_sec: state.reserve_velocity_sol_per_sec,
            is_bootstrap: state.state_phase.is_bootstrap_like(),
            curve_finality: state.curve_finality,
            state_phase: state.state_phase,
            update_count: state.update_count,
        })
    }

    #[must_use]
    pub fn is_canonical(&self, mint: &Pubkey) -> bool {
        self.states
            .get(mint)
            .map(|entry| {
                matches!(
                    entry.state_phase,
                    StatePhase::Canonical | StatePhase::Migrated
                )
            })
            .unwrap_or(false)
    }

    #[must_use]
    pub fn next_recv_seq(&self) -> u64 {
        self.recv_seq_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[must_use]
    pub fn latest_observed_slot(&self) -> Option<u64> {
        match self.latest_observed_slot.load(Ordering::Relaxed) {
            0 => None,
            slot => Some(slot),
        }
    }

    pub fn remove_pool(&self, mint: &Pubkey) {
        self.states.remove(mint);
        self.bootstrap_states.remove(mint);
        self.update_guards.remove(mint);
    }

    #[must_use]
    pub fn canonical_pool_count(&self) -> usize {
        self.states.len()
    }

    #[must_use]
    pub fn bootstrap_pool_count(&self) -> usize {
        self.bootstrap_states.len()
    }
}

fn rejection_reason(
    last_slot: u64,
    last_recv_seq: u64,
    slot: u64,
    recv_seq: u64,
) -> AccountUpdateRejectReason {
    if slot < last_slot {
        AccountUpdateRejectReason::OlderSlot
    } else {
        let _ = last_recv_seq;
        let _ = recv_seq;
        AccountUpdateRejectReason::OlderOrDuplicateReceiveSeq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_state_core::types::UpdateSource;
    use crate::CurveFinality;
    use solana_sdk::pubkey::Pubkey;

    fn sample_update(slot: u64, receive_seq: u64) -> AccountStateUpdate {
        AccountStateUpdate {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            sol_reserves: 1_000_000_000,
            token_reserves: 500_000_000_000,
            is_complete: 0,
            slot,
            write_version: Some(slot),
            receive_ts_ms: slot.saturating_mul(1000),
            receive_seq,
            curve_finality: CurveFinality::Provisional,
            source: UpdateSource::GeyserAccountUpdate,
        }
    }

    #[test]
    fn latest_observed_slot_tracks_latest_applied_update() {
        let reducer = AccountStateReducer::new();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let pool_amm_id = Pubkey::new_unique();

        let mut first = sample_update(100, 1);
        first.base_mint = mint;
        first.bonding_curve = bonding_curve;
        first.pool_amm_id = pool_amm_id;
        assert_eq!(
            reducer.apply_account_update(first),
            AccountUpdateResult::Applied
        );
        assert_eq!(reducer.latest_observed_slot(), Some(100));

        let mut stale = sample_update(99, 2);
        stale.base_mint = mint;
        stale.bonding_curve = bonding_curve;
        stale.pool_amm_id = pool_amm_id;
        assert_eq!(
            reducer.apply_account_update(stale),
            AccountUpdateResult::Rejected(AccountUpdateRejectReason::OlderSlot)
        );
        assert_eq!(reducer.latest_observed_slot(), Some(100));

        let mut fresh = sample_update(105, 3);
        fresh.base_mint = mint;
        fresh.bonding_curve = bonding_curve;
        fresh.pool_amm_id = pool_amm_id;
        assert_eq!(
            reducer.apply_account_update(fresh),
            AccountUpdateResult::Applied
        );
        assert_eq!(reducer.latest_observed_slot(), Some(105));
    }
}

fn bonding_curve_from_update(update: &AccountStateUpdate, token_total_supply: u64) -> BondingCurve {
    bonding_curve_from_reserves(
        update.token_reserves,
        update.sol_reserves,
        token_total_supply,
        update.is_complete != 0,
    )
}

fn bonding_curve_from_canonical_state(state: &CanonicalPoolState) -> BondingCurve {
    bonding_curve_from_reserves(
        state.virtual_token_reserves,
        state.virtual_sol_reserves,
        state.token_total_supply,
        state.is_complete,
    )
}

fn bonding_curve_from_reserves(
    virtual_token_reserves: u64,
    virtual_sol_reserves: u64,
    token_total_supply: u64,
    is_complete: bool,
) -> BondingCurve {
    BondingCurve {
        discriminator: 0,
        virtual_token_reserves,
        virtual_sol_reserves,
        real_token_reserves: virtual_token_reserves.min(token_total_supply),
        real_sol_reserves: virtual_sol_reserves,
        token_total_supply,
        complete: u8::from(is_complete),
        _padding: [0; 7],
    }
}

fn normalize_initial_price(initial_price_sol: f64, fallback_price_sol: f64) -> f64 {
    if initial_price_sol.is_finite() && initial_price_sol > 0.0 {
        initial_price_sol
    } else {
        fallback_price_sol
    }
}

fn compute_price_change_pct(initial_price_sol: f64, current_price_sol: f64) -> f64 {
    if !initial_price_sol.is_finite() || initial_price_sol <= 0.0 {
        0.0
    } else {
        ((current_price_sol - initial_price_sol) / initial_price_sol) * 100.0
    }
}

/// Convert lamport deltas into a normalized human `SOL/sec` reserve velocity.
fn compute_reserve_velocity_sol_per_sec(
    previous_real_sol_reserves: u64,
    current_real_sol_reserves: u64,
    previous_ts_ms: u64,
    current_ts_ms: u64,
) -> f64 {
    let delta_ms = current_ts_ms.saturating_sub(previous_ts_ms);
    if delta_ms == 0 {
        return 0.0;
    }

    let delta_sol = (current_real_sol_reserves as f64 - previous_real_sol_reserves as f64)
        / LAMPORTS_PER_SOL_F64;
    delta_sol / (delta_ms as f64 / 1000.0)
}

/// Normalize raw on-chain reserves into human `SOL/token`.
fn normalized_price_sol(curve: &BondingCurve) -> f64 {
    if curve.virtual_token_reserves == 0 {
        return 0.0;
    }

    let virtual_sol_sol = curve.virtual_sol_reserves as f64 / LAMPORTS_PER_SOL_F64;
    let virtual_tokens = curve.virtual_token_reserves as f64 / PUMP_TOKEN_DECIMAL_FACTOR_F64;

    if virtual_tokens <= 0.0 {
        0.0
    } else {
        virtual_sol_sol / virtual_tokens
    }
}

/// Normalize market cap into human `SOL` while preserving raw reserve inputs.
fn normalized_market_cap_sol(curve: &BondingCurve) -> f64 {
    if curve.virtual_token_reserves == 0 {
        return 0.0;
    }

    ((curve.virtual_sol_reserves as u128).saturating_mul(curve.token_total_supply as u128)
        / curve.virtual_token_reserves as u128) as f64
        / LAMPORTS_PER_SOL_F64
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
