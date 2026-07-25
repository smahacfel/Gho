use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::{
    AccountStateUpdate, AccountUpdateRejectReason, AccountUpdateResult, BootstrapHints, StatePhase,
    UpdateSource,
};
use ghost_core::account_state_core::{
    AccountObservationClassificationV1, AccountObservationOutcomeV1,
};
use ghost_core::{CurveFinality, RawProviderRoleV1};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

fn pk(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn account_update(
    pool_amm_id: Pubkey,
    base_mint: Pubkey,
    bonding_curve: Pubkey,
    sol_reserves: u64,
    token_reserves: u64,
    slot: u64,
    receive_ts_ms: u64,
    receive_seq: u64,
) -> AccountStateUpdate {
    AccountStateUpdate {
        provider_id: Some("test-primary".to_owned()),
        provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
        pool_amm_id,
        base_mint,
        bonding_curve,
        sol_reserves,
        token_reserves,
        is_complete: 0,
        slot,
        write_version: Some(slot),
        txn_signature: None,
        source_account_pubkey: None,
        source_account_owner_or_program: Some(pk(250)),
        account_data_len: Some(56),
        account_data_hash: Some(format!(
            "{slot:016x}{sol_reserves:016x}{token_reserves:016x}{:016x}",
            0_u64
        )),
        receive_ts_ms,
        receive_seq,
        curve_finality: CurveFinality::Finalized,
        source: UpdateSource::GeyserAccountUpdate,
    }
}

#[test]
fn missing_provider_provenance_fails_closed_without_creating_canonical_state() {
    let pool_amm_id = pk(31);
    let base_mint = pk(32);
    let bonding_curve = pk(33);
    let mut without_provenance = account_update(
        pool_amm_id,
        base_mint,
        bonding_curve,
        42_500_000_000,
        1_000_000,
        10,
        1_000,
        1,
    );
    without_provenance.provider_id = None;
    without_provenance.provider_role = None;
    let primary = account_update(
        pool_amm_id,
        base_mint,
        bonding_curve,
        42_500_000_000,
        1_000_000,
        10,
        1_000,
        1,
    );
    let reducer = AccountStateReducer::new();
    assert_eq!(
        reducer.apply_account_update(without_provenance),
        AccountUpdateResult::Rejected(AccountUpdateRejectReason::MissingProviderProvenance)
    );
    assert!(reducer.get_canonical_state(&base_mint).is_none());
    assert_eq!(
        reducer.apply_account_update(primary),
        AccountUpdateResult::Applied
    );
    assert!(reducer.get_canonical_state(&base_mint).is_some());
}

#[test]
fn transaction_observed_bootstrap_cannot_bypass_raw_account_arbitration() {
    let pool_amm_id = pk(34);
    let base_mint = pk(35);
    let bonding_curve = pk(36);
    let mut tx_derived = account_update(
        pool_amm_id,
        base_mint,
        bonding_curve,
        42_500_000_000,
        1_000_000,
        10,
        1_000,
        1,
    );
    tx_derived.source = UpdateSource::TxObservedBootstrap;

    let reducer = AccountStateReducer::new();
    let result = reducer.apply_account_observation(tx_derived);
    assert_eq!(
        result.decision.classification,
        AccountObservationClassificationV1::UnsupportedUpdateSource
    );
    assert_eq!(
        result.decision.outcome,
        AccountObservationOutcomeV1::RejectedInvalidObservation
    );
    assert!(!result.did_apply());
    assert!(
        reducer.get_canonical_state(&base_mint).is_none(),
        "parsed transaction data must not become canonical AccountStateCore authority"
    );
}

#[test]
fn old_account_state_update_json_defaults_new_metadata_to_none() {
    let mut update = account_update(
        pk(41),
        pk(42),
        pk(43),
        42_500_000_000,
        1_000_000,
        10,
        1_000,
        1,
    );
    update.provider_id = None;
    update.provider_role = None;
    update.txn_signature = None;
    let value = serde_json::to_value(&update).expect("serialize baseline account update");
    let object = value.as_object().expect("account update object");
    assert!(!object.contains_key("provider_id"));
    assert!(!object.contains_key("provider_role"));
    assert!(!object.contains_key("txn_signature"));

    let decoded: AccountStateUpdate =
        serde_json::from_value(value).expect("deserialize old account update shape");
    assert_eq!(decoded.provider_id, None);
    assert_eq!(decoded.provider_role, None);
    assert_eq!(decoded.txn_signature, None);
}

#[test]
fn bootstrap_state_stays_non_canonical_until_first_account_update() {
    let reducer = AccountStateReducer::new();
    let pool_amm_id = pk(1);
    let base_mint = pk(2);
    let bonding_curve = pk(3);

    reducer.register_pool_from_bootstrap(
        pool_amm_id,
        base_mint,
        bonding_curve,
        BootstrapHints {
            speculative_reserves: Some((111, 222)),
            token_total_supply: Some(1_000_000),
            bonding_curve_progress: Some(0.25),
            initial_liquidity_sol: Some(12.0),
        },
    );

    assert_eq!(reducer.bootstrap_pool_count(), 1);
    assert_eq!(reducer.canonical_pool_count(), 0);
    assert!(reducer.get_bootstrap_state(&base_mint).is_some());
    assert!(reducer.get_canonical_state(&base_mint).is_none());
    assert!(!reducer.is_canonical(&base_mint));

    let result = reducer.apply_account_update(account_update(
        pk(99),
        base_mint,
        pk(77),
        42_500_000_000,
        1_000_000,
        10,
        1_000,
        1,
    ));

    assert_eq!(result, AccountUpdateResult::PromotedFromBootstrap);
    assert_eq!(reducer.bootstrap_pool_count(), 0);
    assert_eq!(reducer.canonical_pool_count(), 1);
    assert!(reducer.is_canonical(&base_mint));

    let state = reducer
        .get_canonical_state(&base_mint)
        .expect("canonical state after promotion");
    assert_eq!(state.pool_amm_id, pool_amm_id);
    assert_eq!(state.bonding_curve, bonding_curve);
    assert_eq!(state.state_phase, StatePhase::Canonical);
    assert_eq!(state.update_count, 1);
    assert_eq!(state.initial_price_sol, state.price_sol);
    assert_eq!(state.price_change_since_t0_pct, 0.0);

    let features = reducer.get_features(&base_mint).expect("features");
    assert_eq!(features.state_phase, StatePhase::Canonical);
    assert!(!features.is_bootstrap);
    assert_eq!(features.bonding_progress, 0.5);
}

#[test]
fn reducer_classifies_same_version_same_hash_as_duplicate_without_mutating_state() {
    let reducer = AccountStateReducer::new();
    let base_mint = pk(4);

    assert_eq!(
        reducer.apply_account_update(account_update(
            pk(5),
            base_mint,
            pk(6),
            10,
            20,
            100,
            1_000,
            2,
        )),
        AccountUpdateResult::Applied
    );

    assert_eq!(
        reducer.apply_account_update(account_update(
            pk(5),
            base_mint,
            pk(6),
            10,
            20,
            100,
            1_100,
            99,
        )),
        AccountUpdateResult::Rejected(AccountUpdateRejectReason::DuplicateObservation)
    );

    let state = reducer
        .get_canonical_state(&base_mint)
        .expect("canonical state remains intact");
    assert_eq!(state.virtual_sol_reserves, 10);
    assert_eq!(state.update_count, 1);
    assert_eq!(state.last_update_slot, 100);
    assert_eq!(state.last_update_ts_ms, 1_000);
}

#[test]
fn raw_primary_pumpfun_completion_allows_controlled_pumpswap_account_identity_transition() {
    const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";

    let reducer = AccountStateReducer::new();
    let mint = pk(70);
    let pumpfun_curve = pk(71);
    let pumpswap_pool = pk(72);
    let pumpfun_owner = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("valid pump.fun ID");
    let pumpswap_owner = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).expect("valid PumpSwap ID");

    let mut canonical = account_update(pk(73), mint, pumpfun_curve, 10_000, 20_000, 100, 1_000, 1);
    canonical.source_account_pubkey = Some(pumpfun_curve);
    canonical.source_account_owner_or_program = Some(pumpfun_owner);
    assert_eq!(
        reducer.apply_account_update(canonical.clone()),
        AccountUpdateResult::Applied
    );

    let mut completion = canonical.clone();
    completion.slot = 101;
    completion.write_version = Some(2);
    completion.receive_ts_ms = 2_000;
    completion.receive_seq = 2;
    completion.is_complete = 1;
    completion.account_data_hash =
        Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned());
    assert_eq!(
        reducer.apply_account_update(completion),
        AccountUpdateResult::Applied
    );
    assert_eq!(
        reducer
            .get_canonical_state(&mint)
            .expect("completion state")
            .state_phase,
        StatePhase::Migrated
    );

    let mut pumpswap = canonical;
    pumpswap.pool_amm_id = pk(74);
    pumpswap.bonding_curve = pumpswap_pool;
    pumpswap.source_account_pubkey = Some(pumpswap_pool);
    pumpswap.source_account_owner_or_program = Some(pumpswap_owner);
    pumpswap.slot = 102;
    pumpswap.write_version = Some(3);
    pumpswap.receive_ts_ms = 3_000;
    pumpswap.receive_seq = 3;
    // PumpSwap's local layout does not need to reuse Pump.fun's completion
    // bit. The canonical phase must stay migrated after the proven transition.
    pumpswap.is_complete = 0;
    pumpswap.account_data_hash =
        Some("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_owned());
    pumpswap.sol_reserves = 30_000;
    assert_eq!(
        reducer.apply_account_update(pumpswap),
        AccountUpdateResult::Applied
    );

    let state = reducer
        .get_canonical_state(&mint)
        .expect("PumpSwap state must be canonical after controlled transition");
    assert_eq!(state.bonding_curve, pumpswap_pool);
    assert_eq!(state.source_account_pubkey, Some(pumpswap_pool));
    assert_eq!(state.source_account_owner_or_program, Some(pumpswap_owner));
    assert_eq!(state.state_phase, StatePhase::Migrated);
    let snapshot = reducer
        .account_observation_arbiter_snapshot(&mint)
        .expect("arbiter snapshot");
    assert_eq!(snapshot.bound_account_pubkey, Some(pumpswap_pool));
    assert_eq!(snapshot.identity_transitions.len(), 1);
}

#[test]
fn reducer_computes_price_change_and_velocity_from_previous_canonical_state() {
    let reducer = AccountStateReducer::new();
    let base_mint = pk(7);
    let pool_amm_id = pk(8);
    let bonding_curve = pk(9);

    assert_eq!(
        reducer.apply_account_update(account_update(
            pool_amm_id,
            base_mint,
            bonding_curve,
            10,
            20,
            1,
            1_000,
            1,
        )),
        AccountUpdateResult::Applied
    );

    assert_eq!(
        reducer.apply_account_update(account_update(
            pool_amm_id,
            base_mint,
            bonding_curve,
            30,
            20,
            2,
            3_000,
            2,
        )),
        AccountUpdateResult::Applied
    );

    let state = reducer
        .get_canonical_state(&base_mint)
        .expect("canonical state after second update");
    assert_eq!(state.update_count, 2);
    assert_eq!(state.virtual_sol_reserves, 30);
    assert_eq!(state.virtual_token_reserves, 20);
    assert!((state.price_sol - 0.0015).abs() < 1e-12);
    assert!((state.initial_price_sol - 0.0005).abs() < 1e-12);
    assert!((state.price_change_since_t0_pct - 200.0).abs() < 1e-9);
    assert!((state.market_cap_sol - 1_500_000.0).abs() < 1e-6);
    assert!((state.reserve_velocity_sol_per_sec - (10.0 / 1_000_000_000.0)).abs() < 1e-18);
}

#[test]
fn reducer_preserves_raw_reserves_but_exposes_normalized_feature_units() {
    let reducer = AccountStateReducer::new();
    let base_mint = pk(11);
    let pool_amm_id = pk(12);
    let bonding_curve = pk(13);

    assert_eq!(
        reducer.apply_account_update(account_update(
            pool_amm_id,
            base_mint,
            bonding_curve,
            30_000_000_000,
            20_000_000,
            5,
            2_000,
            1,
        )),
        AccountUpdateResult::Applied
    );

    let state = reducer
        .get_canonical_state(&base_mint)
        .expect("canonical state after first update");
    assert_eq!(state.virtual_sol_reserves, 30_000_000_000);
    assert_eq!(state.virtual_token_reserves, 20_000_000);
    assert!((state.price_sol - 1.5).abs() < 1e-12);
    assert!((state.market_cap_sol - 1_500_000_000.0).abs() < 1e-3);
    assert_eq!(state.reserve_velocity_sol_per_sec, 0.0);

    let features = reducer.get_features(&base_mint).expect("features");
    assert_eq!(features.current_reserves, (30_000_000_000, 20_000_000));
    assert!((features.price_sol - 1.5).abs() < 1e-12);
    assert!((features.market_cap_sol - 1_500_000_000.0).abs() < 1e-3);
}

#[test]
fn reducer_rejects_older_slot_even_when_receive_seq_is_newer() {
    let reducer = AccountStateReducer::new();
    let base_mint = pk(14);
    let pool_amm_id = pk(15);
    let bonding_curve = pk(16);

    assert_eq!(
        reducer.apply_account_update(account_update(
            pool_amm_id,
            base_mint,
            bonding_curve,
            20,
            10,
            50,
            1_000,
            1,
        )),
        AccountUpdateResult::Applied
    );

    assert_eq!(
        reducer.apply_account_update(account_update(
            pool_amm_id,
            base_mint,
            bonding_curve,
            40,
            10,
            49,
            1_100,
            99,
        )),
        AccountUpdateResult::Rejected(AccountUpdateRejectReason::StaleObservation)
    );

    let state = reducer
        .get_canonical_state(&base_mint)
        .expect("canonical state remains intact");
    assert_eq!(state.last_update_slot, 50);
    assert_eq!(state.update_count, 1);
}
