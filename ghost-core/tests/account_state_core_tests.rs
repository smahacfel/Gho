use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::{
    AccountStateUpdate, AccountUpdateRejectReason, AccountUpdateResult, BootstrapHints, StatePhase,
    UpdateSource,
};
use ghost_core::CurveFinality;
use solana_sdk::pubkey::Pubkey;

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
        pool_amm_id,
        base_mint,
        bonding_curve,
        sol_reserves,
        token_reserves,
        is_complete: 0,
        slot,
        write_version: None,
        receive_ts_ms,
        receive_seq,
        curve_finality: CurveFinality::Finalized,
        source: UpdateSource::GeyserAccountUpdate,
    }
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
fn reducer_rejects_out_of_order_updates_without_mutating_state() {
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
            999,
            20,
            100,
            1_100,
            1,
        )),
        AccountUpdateResult::Rejected(AccountUpdateRejectReason::OlderOrDuplicateReceiveSeq)
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
        AccountUpdateResult::Rejected(AccountUpdateRejectReason::OlderSlot)
    );

    let state = reducer
        .get_canonical_state(&base_mint)
        .expect("canonical state remains intact");
    assert_eq!(state.last_update_slot, 50);
    assert_eq!(state.update_count, 1);
}
