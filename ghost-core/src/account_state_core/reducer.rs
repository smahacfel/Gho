use super::monotonic_guard::MonotonicUpdateGuard;
use super::types::{
    AccountStateFeatures, AccountStateReserveVelocitySnapshotV1, AccountStateUpdate,
    AccountUpdateRejectReason, AccountUpdateResult, BootstrapHints, BootstrapPoolState,
    CanonicalPoolState, RpcRefreshResult, StatePhase, UpdateSource,
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
    reserve_velocity_evidence: DashMap<Pubkey, AccountStateReserveVelocitySnapshotV1>,
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
            reserve_velocity_previous_real_sol_reserves_lamports,
            reserve_velocity_interval_ms,
            reserve_velocity_status,
        ) = if let Some(previous) = previous_state.as_ref() {
            let initial_price_sol =
                normalize_initial_price(previous.initial_price_sol, previous.price_sol);
            let previous_data_change_ts_ms = previous
                .last_data_change_ts_ms
                .max(previous.last_update_ts_ms);
            let reserve_velocity_sol_per_sec = compute_reserve_velocity_sol_per_sec(
                previous.real_sol_reserves,
                curve.real_sol_reserves,
                previous_data_change_ts_ms,
                update.receive_ts_ms,
            );
            let interval_ms = update.receive_ts_ms.checked_sub(previous_data_change_ts_ms);
            let status = match interval_ms {
                Some(0) => crate::metric_contracts::ReserveVelocityStatusV1::ZeroDeltaTime,
                Some(_) => crate::metric_contracts::ReserveVelocityStatusV1::Measured,
                None => crate::metric_contracts::ReserveVelocityStatusV1::Unavailable,
            };
            let previous_data_change_count = previous.data_change_count.max(previous.update_count);
            let (update_count, status) = match previous_data_change_count.checked_add(1) {
                Some(update_count) => (update_count, status),
                None => (
                    previous.update_count,
                    crate::metric_contracts::ReserveVelocityStatusV1::Unavailable,
                ),
            };
            (
                initial_price_sol,
                compute_price_change_pct(initial_price_sol, price_sol),
                reserve_velocity_sol_per_sec,
                update_count,
                Some(previous.real_sol_reserves),
                interval_ms,
                status,
            )
        } else {
            (
                price_sol,
                0.0,
                0.0,
                1,
                None,
                None,
                crate::metric_contracts::ReserveVelocityStatusV1::FirstUpdate,
            )
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
                last_observed_slot: update.slot,
                last_observed_ts_ms: update.receive_ts_ms,
                last_observation_source: update.source,
                observation_count: previous_state
                    .as_ref()
                    .map(|state| {
                        state
                            .observation_count
                            .max(state.update_count)
                            .saturating_add(1)
                    })
                    .unwrap_or(1),
                last_data_change_ts_ms: update.receive_ts_ms,
                last_data_change_source: update.source,
                data_change_count: update_count,
                source_write_version: update.write_version,
                source_account_pubkey: update.source_account_pubkey,
                source_account_owner_or_program: update.source_account_owner_or_program,
                account_data_len: update.account_data_len,
                account_data_hash: update.account_data_hash,
                curve_finality: update.curve_finality,
                state_phase,
                update_count,
                initial_price_sol,
                price_change_since_t0_pct,
                reserve_velocity_sol_per_sec,
            },
        );
        self.reserve_velocity_evidence.insert(
            update.base_mint,
            AccountStateReserveVelocitySnapshotV1 {
                legacy_velocity_sol_per_sec: reserve_velocity_sol_per_sec,
                previous_real_sol_reserves_lamports:
                    reserve_velocity_previous_real_sol_reserves_lamports,
                current_real_sol_reserves_lamports: Some(curve.real_sol_reserves),
                interval_ms: reserve_velocity_interval_ms,
                accepted_update_count: update_count,
                status: reserve_velocity_status,
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

    /// Apply a processed-RPC point observation for an already canonical pool.
    ///
    /// The RPC context slot says only that a node observed the account at that
    /// slot.  It is intentionally excluded from `update_guards` and from the
    /// reducer-wide Geyser ordering watermark: allowing it to advance either
    /// would let polling reject a later-delivered real account write.
    #[must_use]
    pub fn apply_rpc_refresh(&self, update: AccountStateUpdate) -> RpcRefreshResult {
        if !matches!(update.source, UpdateSource::RpcRefresh) {
            return RpcRefreshResult::Rejected(AccountUpdateRejectReason::RpcRefreshInvalidSource);
        }
        let Some(account_data_hash) = update.account_data_hash.as_deref() else {
            return RpcRefreshResult::Rejected(
                AccountUpdateRejectReason::RpcRefreshMissingAccountDataHash,
            );
        };

        let mut state = match self.states.get_mut(&update.base_mint) {
            Some(state) => state,
            None => {
                return RpcRefreshResult::Rejected(
                    AccountUpdateRejectReason::RpcRefreshWithoutCanonicalState,
                );
            }
        };
        if state.pool_amm_id != update.pool_amm_id || state.bonding_curve != update.bonding_curve {
            return RpcRefreshResult::Rejected(
                AccountUpdateRejectReason::RpcRefreshIdentityMismatch,
            );
        }

        let next_observation_ts_ms = state.last_observed_ts_ms.max(update.receive_ts_ms);
        let next_observation_slot = state.last_observed_slot.max(update.slot);
        let next_observation_count = state
            .observation_count
            .max(state.update_count)
            .saturating_add(1);
        let same_account_data = state.account_data_hash.as_deref() == Some(account_data_hash);
        if same_account_data {
            state.last_observed_slot = next_observation_slot;
            state.last_observed_ts_ms = next_observation_ts_ms;
            state.last_observation_source = UpdateSource::RpcRefresh;
            state.observation_count = next_observation_count;
            return RpcRefreshResult::ObservationRefreshed;
        }

        let token_total_supply = state.token_total_supply;
        let curve = bonding_curve_from_update(&update, token_total_supply);
        let price_sol = normalized_price_sol(&curve);
        let market_cap_sol = normalized_market_cap_sol(&curve);
        let bonding_curve_progress = curve.get_bonding_progress() as f64 / 100.0;
        let is_complete = update.is_complete != 0;
        let next_phase = if is_complete {
            StatePhase::Migrated
        } else {
            StatePhase::Canonical
        };
        if !state.state_phase.can_transition_to(next_phase) {
            return RpcRefreshResult::Rejected(
                AccountUpdateRejectReason::RpcRefreshPhaseRegression,
            );
        }

        let previous_real_sol_reserves = state.real_sol_reserves;
        let previous_data_change_ts_ms = state.last_data_change_ts_ms.max(state.last_update_ts_ms);
        let reserve_velocity_sol_per_sec = compute_reserve_velocity_sol_per_sec(
            previous_real_sol_reserves,
            curve.real_sol_reserves,
            previous_data_change_ts_ms,
            update.receive_ts_ms,
        );
        let interval_ms = update.receive_ts_ms.checked_sub(previous_data_change_ts_ms);
        let reserve_velocity_status = match interval_ms {
            Some(0) => crate::metric_contracts::ReserveVelocityStatusV1::ZeroDeltaTime,
            Some(_) => crate::metric_contracts::ReserveVelocityStatusV1::Measured,
            None => crate::metric_contracts::ReserveVelocityStatusV1::Unavailable,
        };
        let previous_data_change_count = state.data_change_count.max(state.update_count);
        let data_change_count = previous_data_change_count.saturating_add(1);
        let initial_price_sol = normalize_initial_price(state.initial_price_sol, state.price_sol);

        state.virtual_sol_reserves = curve.virtual_sol_reserves;
        state.virtual_token_reserves = curve.virtual_token_reserves;
        state.real_sol_reserves = curve.real_sol_reserves;
        state.real_token_reserves = curve.real_token_reserves;
        state.bonding_curve_progress = bonding_curve_progress;
        state.price_sol = price_sol;
        state.market_cap_sol = market_cap_sol;
        state.is_complete = is_complete;
        state.source_account_pubkey = update.source_account_pubkey;
        state.source_account_owner_or_program = update.source_account_owner_or_program;
        state.account_data_len = update.account_data_len;
        state.account_data_hash = update.account_data_hash;
        state.curve_finality = update.curve_finality;
        state.state_phase = next_phase;
        state.update_count = data_change_count;
        state.initial_price_sol = initial_price_sol;
        state.price_change_since_t0_pct = compute_price_change_pct(initial_price_sol, price_sol);
        state.reserve_velocity_sol_per_sec = reserve_velocity_sol_per_sec;
        state.last_observed_slot = next_observation_slot;
        state.last_observed_ts_ms = next_observation_ts_ms;
        state.last_observation_source = UpdateSource::RpcRefresh;
        state.observation_count = next_observation_count;
        state.last_data_change_ts_ms = update.receive_ts_ms;
        state.last_data_change_source = UpdateSource::RpcRefresh;
        state.data_change_count = data_change_count;

        self.reserve_velocity_evidence.insert(
            update.base_mint,
            AccountStateReserveVelocitySnapshotV1 {
                legacy_velocity_sol_per_sec: reserve_velocity_sol_per_sec,
                previous_real_sol_reserves_lamports: Some(previous_real_sol_reserves),
                current_real_sol_reserves_lamports: Some(curve.real_sol_reserves),
                interval_ms,
                accepted_update_count: data_change_count,
                status: reserve_velocity_status,
            },
        );
        RpcRefreshResult::DataChanged
    }

    #[must_use]
    pub fn get_canonical_state(&self, mint: &Pubkey) -> Option<CanonicalPoolState> {
        self.states.get(mint).map(|entry| entry.clone())
    }

    #[must_use]
    pub fn get_reserve_velocity_snapshot(
        &self,
        mint: &Pubkey,
    ) -> Option<crate::account_state_core::types::AccountStateReserveVelocitySnapshotV1> {
        self.reserve_velocity_evidence
            .get(mint)
            .map(|entry| entry.clone())
    }

    /// Frozen metric-contract view owned by AccountStateCore. Bootstrap and
    /// absent states are represented explicitly so downstream materialization
    /// never fabricates reserve evidence from an MFS compatibility scalar.
    #[must_use]
    pub fn metric_contract_reserve_velocity_snapshot(
        &self,
        mint: &Pubkey,
    ) -> AccountStateReserveVelocitySnapshotV1 {
        if let Some(snapshot) = self.get_reserve_velocity_snapshot(mint) {
            return snapshot;
        }
        if self.bootstrap_states.contains_key(mint) {
            return AccountStateReserveVelocitySnapshotV1 {
                legacy_velocity_sol_per_sec: 0.0,
                previous_real_sol_reserves_lamports: None,
                current_real_sol_reserves_lamports: None,
                interval_ms: None,
                accepted_update_count: 0,
                status: crate::metric_contracts::ReserveVelocityStatusV1::BootstrapFallback,
            };
        }
        AccountStateReserveVelocitySnapshotV1 {
            legacy_velocity_sol_per_sec: 0.0,
            previous_real_sol_reserves_lamports: None,
            current_real_sol_reserves_lamports: None,
            interval_ms: None,
            accepted_update_count: 0,
            status: crate::metric_contracts::ReserveVelocityStatusV1::Unavailable,
        }
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
            source_account_pubkey: None,
            source_account_owner_or_program: None,
            account_data_len: None,
            account_data_hash: None,
            receive_ts_ms: slot.saturating_mul(1000),
            receive_seq,
            curve_finality: CurveFinality::Provisional,
            source: UpdateSource::GeyserAccountUpdate,
        }
    }

    fn rpc_refresh_from(
        canonical: &AccountStateUpdate,
        slot: u64,
        ts_ms: u64,
    ) -> AccountStateUpdate {
        AccountStateUpdate {
            slot,
            write_version: Some(0),
            receive_ts_ms: ts_ms,
            receive_seq: 999,
            source: UpdateSource::RpcRefresh,
            account_data_hash: Some("rpc-account-data".to_string()),
            account_data_len: Some(16),
            source_account_pubkey: Some(canonical.bonding_curve),
            ..canonical.clone()
        }
    }

    #[test]
    fn identical_rpc_refresh_updates_observation_without_activity_or_geyser_ordering() {
        let reducer = AccountStateReducer::new();
        let mut canonical = sample_update(100, 1);
        canonical.account_data_hash = Some("rpc-account-data".to_string());
        canonical.account_data_len = Some(16);
        canonical.source_account_pubkey = Some(canonical.bonding_curve);
        assert_eq!(
            reducer.apply_account_update(canonical.clone()),
            AccountUpdateResult::Applied
        );
        let before = reducer
            .get_canonical_state(&canonical.base_mint)
            .expect("canonical state");
        let velocity_before = reducer
            .get_reserve_velocity_snapshot(&canonical.base_mint)
            .expect("velocity evidence");

        let refresh = rpc_refresh_from(&canonical, 500, 500_000);
        assert_eq!(
            reducer.apply_rpc_refresh(refresh),
            RpcRefreshResult::ObservationRefreshed
        );
        let after = reducer
            .get_canonical_state(&canonical.base_mint)
            .expect("canonical state after refresh");
        assert_eq!(after.last_observed_slot, 500);
        assert_eq!(after.last_observed_ts_ms, 500_000);
        assert_eq!(after.last_observation_source, UpdateSource::RpcRefresh);
        assert_eq!(after.last_data_change_ts_ms, before.last_data_change_ts_ms);
        assert_eq!(after.data_change_count, before.data_change_count);
        assert_eq!(after.update_count, before.update_count);
        assert_eq!(
            after.reserve_velocity_sol_per_sec,
            before.reserve_velocity_sol_per_sec
        );
        assert_eq!(
            reducer.get_reserve_velocity_snapshot(&canonical.base_mint),
            Some(velocity_before)
        );
        assert_eq!(reducer.latest_observed_slot(), Some(100));

        let mut delayed_geyser = canonical;
        delayed_geyser.slot = 101;
        delayed_geyser.write_version = Some(101);
        delayed_geyser.receive_seq = 2;
        delayed_geyser.receive_ts_ms = 101_000;
        assert_eq!(
            reducer.apply_account_update(delayed_geyser),
            AccountUpdateResult::Applied,
            "a high RPC context slot must not reject a later-delivered Geyser write"
        );
    }

    #[test]
    fn changed_rpc_refresh_updates_data_change_without_advancing_geyser_guard() {
        let reducer = AccountStateReducer::new();
        let mut canonical = sample_update(100, 1);
        canonical.account_data_hash = Some("canonical-account-data".to_string());
        canonical.account_data_len = Some(16);
        canonical.source_account_pubkey = Some(canonical.bonding_curve);
        assert_eq!(
            reducer.apply_account_update(canonical.clone()),
            AccountUpdateResult::Applied
        );

        let mut refresh = rpc_refresh_from(&canonical, 500, 500_000);
        refresh.account_data_hash = Some("changed-rpc-account-data".to_string());
        refresh.sol_reserves = refresh.sol_reserves.saturating_add(1_000);
        assert_eq!(
            reducer.apply_rpc_refresh(refresh),
            RpcRefreshResult::DataChanged
        );
        let state = reducer
            .get_canonical_state(&canonical.base_mint)
            .expect("canonical state after changed refresh");
        assert_eq!(state.last_update_slot, 100);
        assert_eq!(state.last_observed_slot, 500);
        assert_eq!(state.last_data_change_source, UpdateSource::RpcRefresh);
        assert_eq!(state.data_change_count, 2);
        assert_eq!(reducer.latest_observed_slot(), Some(100));
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

    #[test]
    fn account_data_hash_metadata_propagates_to_canonical_state() {
        let reducer = AccountStateReducer::new();
        let source_account_pubkey = Pubkey::new_unique();
        let source_owner = Pubkey::new_unique();
        let mut update = sample_update(123, 1);
        update.source_account_pubkey = Some(source_account_pubkey);
        update.source_account_owner_or_program = Some(source_owner);
        update.account_data_len = Some(56);
        update.account_data_hash = Some("blake3-raw-account-bytes".to_string());

        assert_eq!(
            reducer.apply_account_update(update.clone()),
            AccountUpdateResult::Applied
        );

        let state = reducer
            .get_canonical_state(&update.base_mint)
            .expect("canonical state should exist after update");
        assert_eq!(state.source_write_version, update.write_version);
        assert_eq!(state.source_account_pubkey, Some(source_account_pubkey));
        assert_eq!(state.source_account_owner_or_program, Some(source_owner));
        assert_eq!(state.account_data_len, Some(56));
        assert_eq!(
            state.account_data_hash.as_deref(),
            Some("blake3-raw-account-bytes")
        );
    }

    #[test]
    fn account_data_hash_metadata_tracks_latest_applied_update() {
        let reducer = AccountStateReducer::new();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let pool_amm_id = Pubkey::new_unique();
        let source_account_pubkey_a = Pubkey::new_unique();
        let source_owner_a = Pubkey::new_unique();
        let source_account_pubkey_b = Pubkey::new_unique();
        let source_owner_b = Pubkey::new_unique();

        let mut first = sample_update(200, 1);
        first.base_mint = mint;
        first.bonding_curve = bonding_curve;
        first.pool_amm_id = pool_amm_id;
        first.write_version = Some(7);
        first.source_account_pubkey = Some(source_account_pubkey_a);
        first.source_account_owner_or_program = Some(source_owner_a);
        first.account_data_len = Some(111);
        first.account_data_hash = Some("blake3-hash-a".to_string());
        assert_eq!(
            reducer.apply_account_update(first),
            AccountUpdateResult::Applied
        );

        let mut second = sample_update(201, 2);
        second.base_mint = mint;
        second.bonding_curve = bonding_curve;
        second.pool_amm_id = pool_amm_id;
        second.write_version = Some(9);
        second.source_account_pubkey = Some(source_account_pubkey_b);
        second.source_account_owner_or_program = Some(source_owner_b);
        second.account_data_len = Some(222);
        second.account_data_hash = Some("blake3-hash-b".to_string());
        assert_eq!(
            reducer.apply_account_update(second),
            AccountUpdateResult::Applied
        );

        let state = reducer
            .get_canonical_state(&mint)
            .expect("canonical state should exist after latest update");
        assert_eq!(state.last_update_slot, 201);
        assert_eq!(state.source_write_version, Some(9));
        assert_eq!(state.source_account_pubkey, Some(source_account_pubkey_b));
        assert_eq!(state.source_account_owner_or_program, Some(source_owner_b));
        assert_eq!(state.account_data_len, Some(222));
        assert_eq!(state.account_data_hash.as_deref(), Some("blake3-hash-b"));
    }

    #[test]
    fn metric_contract_reserve_velocity_distinguishes_first_measured_zero_and_zero_delta() {
        let reducer = AccountStateReducer::new();
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut first = sample_update(1, 1);
        first.base_mint = mint;
        first.pool_amm_id = pool;
        first.bonding_curve = curve;
        first.receive_ts_ms = 1_000;
        first.sol_reserves = 1_000_000_000;
        let _ = reducer.apply_account_update(first);
        let first = reducer.metric_contract_reserve_velocity_snapshot(&mint);
        assert_eq!(
            first.status,
            crate::metric_contracts::ReserveVelocityStatusV1::FirstUpdate
        );
        assert_eq!(first.accepted_update_count, 1);
        assert_eq!(
            first.current_real_sol_reserves_lamports,
            Some(1_000_000_000)
        );
        assert_eq!(first.interval_ms, None);

        let mut measured = sample_update(2, 2);
        measured.base_mint = mint;
        measured.pool_amm_id = pool;
        measured.bonding_curve = curve;
        measured.receive_ts_ms = 2_000;
        measured.sol_reserves = 2_000_000_000;
        let _ = reducer.apply_account_update(measured);
        let measured = reducer.metric_contract_reserve_velocity_snapshot(&mint);
        assert_eq!(
            measured.status,
            crate::metric_contracts::ReserveVelocityStatusV1::Measured
        );
        assert_eq!(
            measured.legacy_velocity_sol_per_sec.to_bits(),
            1.0_f64.to_bits()
        );

        let mut measured_zero = sample_update(3, 3);
        measured_zero.base_mint = mint;
        measured_zero.pool_amm_id = pool;
        measured_zero.bonding_curve = curve;
        measured_zero.receive_ts_ms = 3_000;
        measured_zero.sol_reserves = 2_000_000_000;
        let _ = reducer.apply_account_update(measured_zero);
        let measured_zero = reducer.metric_contract_reserve_velocity_snapshot(&mint);
        assert_eq!(
            measured_zero.status,
            crate::metric_contracts::ReserveVelocityStatusV1::Measured
        );
        assert_eq!(
            measured_zero.legacy_velocity_sol_per_sec.to_bits(),
            0.0_f64.to_bits()
        );

        let mut zero_delta = sample_update(4, 4);
        zero_delta.base_mint = mint;
        zero_delta.pool_amm_id = pool;
        zero_delta.bonding_curve = curve;
        zero_delta.receive_ts_ms = 3_000;
        zero_delta.sol_reserves = 3_000_000_000;
        let _ = reducer.apply_account_update(zero_delta);
        let zero_delta = reducer.metric_contract_reserve_velocity_snapshot(&mint);
        assert_eq!(
            zero_delta.status,
            crate::metric_contracts::ReserveVelocityStatusV1::ZeroDeltaTime
        );
        assert_eq!(zero_delta.interval_ms, Some(0));
    }

    #[test]
    fn metric_contract_reserve_velocity_keeps_bootstrap_and_absent_unavailable_nonmeasured() {
        let reducer = AccountStateReducer::new();
        let bootstrap_mint = Pubkey::new_unique();
        reducer.register_pool_from_bootstrap(
            Pubkey::new_unique(),
            bootstrap_mint,
            Pubkey::new_unique(),
            BootstrapHints::default(),
        );
        let bootstrap = reducer.metric_contract_reserve_velocity_snapshot(&bootstrap_mint);
        assert_eq!(
            bootstrap.status,
            crate::metric_contracts::ReserveVelocityStatusV1::BootstrapFallback
        );
        assert_eq!(bootstrap.accepted_update_count, 0);

        let absent = reducer.metric_contract_reserve_velocity_snapshot(&Pubkey::new_unique());
        assert_eq!(
            absent.status,
            crate::metric_contracts::ReserveVelocityStatusV1::Unavailable
        );
        assert_eq!(absent.accepted_update_count, 0);
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
