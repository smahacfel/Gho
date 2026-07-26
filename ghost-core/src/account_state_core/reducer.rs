use super::observation_arbiter::{
    AccountMutationVersionV1, AccountObservationApplyResultV1, AccountObservationArbiter,
    AccountObservationArbiterSnapshotV1, AccountObservationClassificationV1,
    AccountObservationDecisionV1, AccountObservationOutcomeV1, AccountProviderAgreementV1,
};
use super::types::{
    AccountStateFeatures, AccountStateReserveVelocitySnapshotV1, AccountStateUpdate,
    AccountUpdateRejectReason, AccountUpdateResult, BootstrapHints, BootstrapPoolState,
    CanonicalPoolState, RpcRefreshResult, StatePhase, UpdateSource,
};
use crate::market_state::BondingCurve;
use crate::PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY;
use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

const LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;
const PUMP_TOKEN_DECIMAL_FACTOR_F64: f64 = 1_000_000.0;

#[derive(Debug, Default)]
pub struct AccountStateReducer {
    states: DashMap<Pubkey, CanonicalPoolState>,
    account_observation_arbiters: DashMap<Pubkey, Arc<Mutex<AccountObservationArbiter>>>,
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

    /// Compatibility wrapper for existing callers that only need the legacy
    /// applied/rejected shape.  New active ingest code should use
    /// [`Self::apply_account_observation`] to retain the typed decision.
    #[must_use]
    pub fn apply_account_update(&self, update: AccountStateUpdate) -> AccountUpdateResult {
        self.apply_account_observation(update)
            .into_account_update_result()
    }

    /// Classify one raw account observation exactly once and apply it to
    /// canonical state only when the arbiter returns `AppliedNewMutation`.
    ///
    /// The per-mint mutex is intentionally held through the infallible reducer
    /// mutation.  This prevents two concurrently delivered observations of the
    /// same account from both passing arbitration and then reordering their
    /// canonical writes.  It is never held across an async await point.
    #[must_use]
    pub fn apply_account_observation(
        &self,
        update: AccountStateUpdate,
    ) -> AccountObservationApplyResultV1 {
        let arbiter = self
            .account_observation_arbiters
            .entry(update.base_mint)
            .or_insert_with(|| Arc::new(Mutex::new(AccountObservationArbiter::default())))
            .clone();
        let mut arbiter = match arbiter.lock() {
            Ok(guard) => guard,
            // Do not recover a potentially partial version/hash watermark.
            // Creating a fresh arbiter, or applying through poisoned state,
            // could both double-apply a canonical mutation.  Fail closed
            // instead and retain the poisoned map entry for diagnosis.
            Err(_) => {
                let decision = AccountObservationDecisionV1 {
                    classification: AccountObservationClassificationV1::ArbiterStateUnavailable,
                    outcome: AccountObservationOutcomeV1::RejectedInvalidObservation,
                    canonical_apply: false,
                    provider_agreement: AccountProviderAgreementV1::NotObserved,
                    mutation_version: Some(AccountMutationVersionV1 {
                        pubkey: update.source_account_pubkey.unwrap_or(update.bonding_curve),
                        slot: update.slot,
                        write_version: update.write_version,
                    }),
                    data_hash_blake3: None,
                };
                self.record_account_observation_decision_metric(&decision);
                return AccountObservationApplyResultV1 {
                    decision,
                    canonical_result: None,
                };
            }
        };
        let decision = arbiter.arbitrate(&update);
        self.record_account_observation_decision_metric(&decision);
        if !decision.canonical_apply {
            return AccountObservationApplyResultV1 {
                decision,
                canonical_result: None,
            };
        }

        let canonical_result = self.apply_canonical_account_mutation(update);
        AccountObservationApplyResultV1 {
            decision,
            canonical_result: Some(canonical_result),
        }
    }

    /// Retain and classify a raw provider observation after the candidate has
    /// reached a terminal runtime state, without mutating canonical reserves.
    ///
    /// The arbiter watermark/evidence is intentionally preserved so a late
    /// primary/secondary agreement or conflict can still produce an immutable
    /// CandidateIntegrity audit marker. `AccountStateCore`, velocity counters,
    /// bootstrap state and reconciliation inputs are never changed here.
    #[must_use]
    pub fn observe_account_evidence_only(
        &self,
        update: AccountStateUpdate,
    ) -> AccountObservationDecisionV1 {
        let arbiter = self
            .account_observation_arbiters
            .entry(update.base_mint)
            .or_insert_with(|| Arc::new(Mutex::new(AccountObservationArbiter::default())))
            .clone();
        let mut arbiter = match arbiter.lock() {
            Ok(guard) => guard,
            Err(_) => {
                let decision = AccountObservationDecisionV1 {
                    classification: AccountObservationClassificationV1::ArbiterStateUnavailable,
                    outcome: AccountObservationOutcomeV1::RejectedInvalidObservation,
                    canonical_apply: false,
                    provider_agreement: AccountProviderAgreementV1::NotObserved,
                    mutation_version: Some(AccountMutationVersionV1 {
                        pubkey: update.source_account_pubkey.unwrap_or(update.bonding_curve),
                        slot: update.slot,
                        write_version: update.write_version,
                    }),
                    data_hash_blake3: None,
                };
                self.record_account_observation_decision_metric(&decision);
                return decision;
            }
        };
        let mut decision = arbiter.arbitrate(&update);
        // The arbiter may advance its bounded evidence watermark, but this
        // terminal-audit API never grants reducer mutation authority.
        decision.canonical_apply = false;
        self.record_account_observation_decision_metric(&decision);
        decision
    }

    fn apply_canonical_account_mutation(&self, update: AccountStateUpdate) -> AccountUpdateResult {
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
        // A Pump.fun completion promotes the pool to `Migrated`. Later raw
        // PumpSwap observations may legitimately carry a layout-local
        // `is_complete = 0`; they must never demote the canonical lifecycle
        // back to `Canonical` merely because they originate from the new
        // source account.
        let state_phase = if is_complete
            || previous_state
                .as_ref()
                .is_some_and(|state| matches!(state.state_phase, StatePhase::Migrated))
        {
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

    /// Observe a processed-RPC point for an already canonical pool.
    ///
    /// This method intentionally does not mutate [`CanonicalPoolState`]. A
    /// processed RPC reply has neither a raw provider role nor a Yellowstone
    /// account-write version, so allowing it to alter reserves, account hash,
    /// phase, counters, velocity, or freshness would create a second canonical
    /// authority beside the raw-primary arbiter. The result only tells the
    /// polling caller whether the captured payload agrees with the current
    /// raw-primary state.
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

        let state = match self.states.get(&update.base_mint) {
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

        let same_account_data = state.account_data_hash.as_deref() == Some(account_data_hash);
        if same_account_data {
            metrics::counter!(
                "account_rpc_refresh_observation_total",
                1_u64,
                "agreement" => "matches_canonical"
            );
            return RpcRefreshResult::ObservationMatchesCanonical;
        }
        metrics::counter!(
            "account_rpc_refresh_observation_total",
            1_u64,
            "agreement" => "diverges_from_canonical"
        );
        RpcRefreshResult::ObservationDivergesFromCanonical
    }

    #[must_use]
    pub fn get_canonical_state(&self, mint: &Pubkey) -> Option<CanonicalPoolState> {
        self.states.get(mint).map(|entry| entry.clone())
    }

    /// Read-only arbitration evidence for one pool/mint.  This diagnostic
    /// snapshot is intentionally outside `CanonicalPoolState` and cannot
    /// affect feature materialization or policy evaluation.
    #[must_use]
    pub fn account_observation_arbiter_snapshot(
        &self,
        mint: &Pubkey,
    ) -> Option<AccountObservationArbiterSnapshotV1> {
        let arbiter = self.account_observation_arbiters.get(mint)?.clone();
        let guard = arbiter.lock().ok()?;
        Some(guard.snapshot())
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

    /// Allocates local transport metadata.  It is deliberately not part of
    /// account chain ordering, duplicate identity, or canonical state
    /// authority.
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
        // Preserve the bounded in-process arbiter after terminal cleanup.
        // Late provider agreement/conflict must remain auditable even though
        // canonical runtime state and the observation session are gone.
    }

    #[must_use]
    pub fn canonical_pool_count(&self) -> usize {
        self.states.len()
    }

    #[must_use]
    pub fn bootstrap_pool_count(&self) -> usize {
        self.bootstrap_states.len()
    }

    fn record_account_observation_decision_metric(&self, decision: &AccountObservationDecisionV1) {
        metrics::counter!(
            "account_observation_arbiter_decision_total",
            1u64,
            "classification" => decision.classification.as_str(),
            "outcome" => decision.outcome.as_str(),
            "provider_agreement" => decision.provider_agreement.as_str(),
        );
        if decision.canonical_apply {
            metrics::counter!("account_observation_arbiter_canonical_mutation_total", 1u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_state_core::types::UpdateSource;
    use crate::{CurveFinality, RawProviderRoleV1};
    use solana_sdk::pubkey::Pubkey;

    fn sample_update(slot: u64, receive_seq: u64) -> AccountStateUpdate {
        AccountStateUpdate {
            provider_id: Some("test-primary".to_owned()),
            provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            sol_reserves: 1_000_000_000,
            token_reserves: 500_000_000_000,
            is_complete: 0,
            slot,
            write_version: Some(slot),
            txn_signature: None,
            // Tests which overwrite `bonding_curve` intentionally leave this
            // absent so the arbiter exercises its deterministic curve-key
            // compatibility fallback instead of inventing a second account.
            source_account_pubkey: None,
            source_account_owner_or_program: Some(Pubkey::new_unique()),
            account_data_len: Some(56),
            account_data_hash: Some(format!("{slot:064x}")),
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
            account_data_hash: Some(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ),
            account_data_len: Some(16),
            source_account_pubkey: Some(canonical.bonding_curve),
            ..canonical.clone()
        }
    }

    #[test]
    fn matching_rpc_refresh_is_observation_only_and_does_not_change_canonical_state() {
        let reducer = AccountStateReducer::new();
        let mut canonical = sample_update(100, 1);
        canonical.account_data_hash =
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
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
            RpcRefreshResult::ObservationMatchesCanonical
        );
        let after = reducer
            .get_canonical_state(&canonical.base_mint)
            .expect("canonical state after refresh");
        assert_eq!(after, before);
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
    fn diverging_rpc_refresh_cannot_overwrite_raw_primary_or_break_duplicate_suppression() {
        let reducer = AccountStateReducer::new();
        let mut canonical = sample_update(100, 1);
        canonical.account_data_hash =
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string());
        canonical.account_data_len = Some(16);
        canonical.source_account_pubkey = Some(canonical.bonding_curve);
        assert_eq!(
            reducer.apply_account_update(canonical.clone()),
            AccountUpdateResult::Applied
        );

        let mut refresh = rpc_refresh_from(&canonical, 500, 500_000);
        refresh.account_data_hash =
            Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string());
        refresh.sol_reserves = refresh.sol_reserves.saturating_add(1_000);
        assert_eq!(
            reducer.apply_rpc_refresh(refresh),
            RpcRefreshResult::ObservationDivergesFromCanonical
        );
        let state = reducer
            .get_canonical_state(&canonical.base_mint)
            .expect("canonical state after changed refresh");
        assert_eq!(state.account_data_hash, canonical.account_data_hash);
        assert_eq!(state.virtual_sol_reserves, canonical.sol_reserves);
        assert_eq!(state.update_count, 1);
        assert_eq!(state.data_change_count, 1);
        assert_eq!(reducer.latest_observed_slot(), Some(100));
        assert_eq!(
            reducer.apply_account_update(canonical),
            AccountUpdateResult::Rejected(AccountUpdateRejectReason::DuplicateObservation),
            "a raw-primary replay must remain a duplicate after a divergent RPC observation"
        );
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
            AccountUpdateResult::Rejected(AccountUpdateRejectReason::StaleObservation)
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
        update.account_data_hash =
            Some("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string());

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
            Some("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd")
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

        let mut first = sample_update(200, 1);
        first.base_mint = mint;
        first.bonding_curve = bonding_curve;
        first.pool_amm_id = pool_amm_id;
        first.write_version = Some(7);
        first.source_account_pubkey = Some(source_account_pubkey_a);
        first.source_account_owner_or_program = Some(source_owner_a);
        first.account_data_len = Some(111);
        first.account_data_hash =
            Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string());
        assert_eq!(
            reducer.apply_account_update(first),
            AccountUpdateResult::Applied
        );

        let mut second = sample_update(201, 2);
        second.base_mint = mint;
        second.bonding_curve = bonding_curve;
        second.pool_amm_id = pool_amm_id;
        second.write_version = Some(9);
        // A pool/mint is bound to one observed account pubkey.  A changed
        // source account is an identity conflict, not a newer mutation.
        second.source_account_pubkey = Some(source_account_pubkey_a);
        second.source_account_owner_or_program = Some(source_owner_a);
        second.account_data_len = Some(222);
        second.account_data_hash =
            Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string());
        assert_eq!(
            reducer.apply_account_update(second),
            AccountUpdateResult::Applied
        );

        let state = reducer
            .get_canonical_state(&mint)
            .expect("canonical state should exist after latest update");
        assert_eq!(state.last_update_slot, 201);
        assert_eq!(state.source_write_version, Some(9));
        assert_eq!(state.source_account_pubkey, Some(source_account_pubkey_a));
        assert_eq!(state.source_account_owner_or_program, Some(source_owner_a));
        assert_eq!(state.account_data_len, Some(222));
        assert_eq!(
            state.account_data_hash.as_deref(),
            Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
        );
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
