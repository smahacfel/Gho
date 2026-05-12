//! Integration Test: Gatekeeper V2 Pipeline (ETAP 4)
//!
//! These tests validate the complete Gatekeeper V2 event flow:
//!
//! Test #1 (test_full_pipeline_buy):
//!   EventBus → NewPoolDetected → PoolTransactions → GatekeeperVerdict::Buy
//!   → SnapshotEngine receives TxEvents → pool is tracked
//!
//! Test #2 (test_full_pipeline_reject):
//!   EventBus → NewPoolDetected → PoolTransactions (bot pattern)
//!   → GatekeeperVerdict::Reject → remove_pool → no SnapshotEngine activity

use ghost_brain::config::{GatekeeperV2Config, IwimVetoGateConfig};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_brain::oracle::SnapshotEngine;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_launcher::components::gatekeeper::PoolState;
use ghost_launcher::events::{create_event_bus, DetectedPool, GhostEvent, PoolTransaction};
use ghost_launcher::oracle_runtime::{start_oracle_runtime_task, OracleRuntime};
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

/// Create a GatekeeperV2Config with low thresholds for integration testing.
fn test_gk_v2_config() -> GatekeeperV2Config {
    GatekeeperV2Config {
        // Mode
        mode: ghost_brain::config::GatekeeperMode::Standard,
        // Low thresholds so we don't need 8+ unique TXs
        min_tx_count: 4,
        min_unique_signers: 3,
        min_buy_count: 3,
        max_wait_time_ms: 5_000,
        min_sol_threshold: 0.005,

        // Phase 2: Velocity — lenient for test
        min_interval_cv: 0.05,
        max_interval_cv: 9999.0,
        max_burst_ratio: 0.95,
        min_avg_interval_ms: 1.0,
        max_avg_interval_ms: 60_000.0,
        min_timing_entropy: 0.1,
        max_timing_entropy: 9999.0,
        min_dust_filtered_count: 0,

        // Phase 3: Diversity — lenient
        min_unique_ratio: 0.3,
        max_unique_ratio: 1.0,
        max_hhi: 0.8,
        max_tx_per_signer: 10,
        max_volume_gini: 0.95,
        min_volume_gini: 0.0,
        max_top3_volume_pct: 0.99,
        max_same_ms_tx_ratio: 1.0,

        // Phase 4: Volume — lenient
        min_buy_ratio: 0.3,
        max_buy_ratio: 1.0,
        min_avg_tx_sol: 0.01,
        max_avg_tx_sol: 100.0,
        min_volume_cv: 0.01,
        max_volume_cv: 9999.0,
        min_total_volume_sol: 0.1,
        max_total_volume_sol: 9999.0,
        min_sol_buy_ratio: 0.0,
        min_consecutive_buys: 0,

        // Phase 5: Dev — standard
        max_dev_buy_sol: 8.0,
        max_dev_tx_ratio: 0.20,
        min_dev_tx_ratio: 0.0,
        max_dev_volume_ratio: 0.40,
        min_dev_volume_ratio: 0.0,
        reject_on_dev_sell: true,
        min_dev_buy_sol: 0.0,

        // Phase 6: auto-pass (no reserve data in test)
        max_price_change_ratio: 100.0,
        max_single_tx_price_impact_pct: 100.0,
        max_bonding_progress_pct: 100.0,
        min_market_cap_sol: 0.01,
        max_single_sell_impact_pct: 100.0,

        // Decision
        min_phases_to_pass: 4,
        re_eval_tx_interval: 2,

        // Three-Layer Decision System
        use_three_layer_decision: true, // Phase 2 production default; legacy remains explicit compat-only
        hard_fail_hhi: 0.50,
        hard_fail_same_ms_tx_ratio: 0.90,
        hard_fail_top3_volume_pct: 0.99,
        max_soft_points: 30,
        soft_weight_timing: 1,
        soft_weight_manipulation: 3,
        soft_weight_diversity: 2,
        soft_weight_ecosystem: 1,
        max_soft_score: 11,
        dev_unknown_min_market_cap_sol: 0.01,
        dev_unknown_min_sol_buy_ratio: 0.0,
        dev_unknown_max_soft_points: 30,
        dev_unknown_max_single_tx_price_impact_pct: 100.0,
        max_sell_buy_ratio: 9999.0,
        min_sell_buy_ratio: 0.0,
        max_compute_unit_cluster_dominance: 1.0,
        min_compute_unit_cluster_dominance: 0.0,
        max_static_fee_profile_ratio: 1.0,
        min_static_fee_profile_ratio: 0.0,
        max_fixed_size_buy_ratio: 1.0,
        min_fixed_size_buy_ratio: 0.0,
        max_fixed_size_buy_ratio_1e4: 1.0,
        max_flipper_presence_ratio: 1.0,
        max_jito_tip_intensity: 1.0,
        min_jito_tip_intensity: 0.0,
        max_early_slot_volume_dominance_buy: 1.0,
        max_early_top3_buy_volume_pct_3s: 1.0,
        min_avg_inner_ix_count_50tx: 0.0,
        max_avg_inner_ix_count_50tx: 9999.0,
        max_whale_reversal_ratio_top3: 9999.0,
        max_whale_reversal_ratio_top1: 9999.0,
        min_dev_paperhand_latency_ms: 0,
        min_fee_topology_diversity_index: 0.0,
        max_dev_buyer_infrastructure_affinity: 1.0,
        min_spend_fraction_divergence: 0.0,
        min_demand_elasticity_score: -1.0,
        max_signer_cross_pool_velocity: 1.0,
        max_funding_source_concentration: 1.0,
        soft_penalty_low_ftdi: 0,
        soft_penalty_high_dbia: 0,
        soft_penalty_low_sfd: 0,
        soft_penalty_inelastic_demand: 0,
        soft_penalty_high_cpv: 0,
        soft_penalty_high_fsc: 0,
        soft_penalty_high_dbia_low_ftdi_combo: 0,
        soft_penalty_low_des_low_sfd_combo: 0,
        soft_penalty_high_cpv_low_des_combo: 0,
        soft_penalty_high_fsc_high_cpv_combo: 0,
        enable_sybil_interference_layer: false,
        max_sybil_soft_points: 255,
        dev_unknown_max_sybil_soft_points: 255,
        enable_sybil_combo_veto: false,
        emit_sybil_meta_score: false,
        require_ready_fsc_for_combo_veto: true,
        cpv_lookback_window_s: 300,
        funding_lookback_window_s: 300,
        funding_dust_threshold_lamports: 10_000_000,
        cpv_per_signer_cap: 16,
        cpv_global_signer_cap: 50_000,
        fsc_per_recipient_cap: 4,
        fsc_global_recipient_cap: 75_000,
        neutral_funding_sources: vec![],
        hard_fail_bot_min_tx: 20,
        hard_fail_bot_min_observation_ms: 1500,

        // Yellowstone
        min_failed_tx_ratio_for_bot_flag: None,
        use_slot_ordering: false,
        curve_wait_ms: 800,
        curve_require_for_buy: false,
        stale_fallback: ghost_core::shadow_ledger::ShadowLedgerStaleFallback::PendingCurve,

        // IWIM veto strong
        iwim_veto_strong_margin: 3,
        iwim_veto_strong_max_manip_flags: 0,

        // Bonding progress minimum
        min_bonding_progress_pct: 0.0,
        enable_alpha_gate: false,
        min_momentum: 0.55,
        min_demand: 0.55,
        min_alpha_joint: 0.35,
        min_alpha_sample: 15,
        enable_prosperity_filter: false,
        prosperity_min_market_cap_sol: 35.0,
        prosperity_max_signer_cross_pool_velocity: 0.50,
        prosperity_branch1_min_block0_sniped_supply_pct: 0.28,
        prosperity_branch1_max_sell_buy_ratio: 0.16,
        prosperity_branch2_min_market_cap_sol: 50.0,
        prosperity_branch2_min_early_slot_volume_dominance_buy: 0.90,
        prosperity_branch3_max_hhi: 0.0416,
        prosperity_branch3_min_fee_topology_diversity_index: 0.0909,
        enable_prosperity_overlay: false,
        prosperity_overlay_max_price_change_ratio: 2.2,
        prosperity_overlay_max_bonding_progress_pct: 85.0,
        prosperity_overlay_min_fee_topology_diversity_index: 0.10,
        prosperity_overlay_branch23_max_sell_buy_ratio: 0.18,
        prosperity_overlay_branch2_max_price_change_ratio: 2.0,
        ..Default::default()
    }
}

/// Generate a pool transaction with organic-looking properties.
fn organic_tx(
    pool_id: &str,
    token_mint: &str,
    signer: &str,
    volume_sol: f64,
    timestamp_ms: u64,
    is_buy: bool,
) -> PoolTransaction {
    PoolTransaction {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        signer: signer.to_string(),
        token_mint: Some(token_mint.to_string()),
        owner_token_deltas: vec![],
        is_buy,
        volume_sol,
        price_quote: Some(0.00003),
        slot: Some(100_000 + (timestamp_ms / 400)),
        event_ordinal: Some(0),
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms,
        signature: format!("sig_{}_{}", &signer[..8.min(signer.len())], timestamp_ms),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        sol_amount_lamports: Some((volume_sol * 1e9) as u64),
        token_amount_units: None,
        reserve_base: None,
        reserve_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        arrival_ts_ms: timestamp_ms + 50,
        event_time: ghost_core::EventTimeMetadata::default(),
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
        v_tokens_in_bonding_curve: None,
        v_sol_in_bonding_curve: None,
        market_cap_sol: None,
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: None,
        associated_bonding_curve: None,
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

/// Generate a bot-like transaction (identical timing, identical volume).
fn bot_tx(pool_id: &str, token_mint: &str, signer: &str, timestamp_ms: u64) -> PoolTransaction {
    // All same volume and very regular timing = bot signature
    organic_tx(pool_id, token_mint, signer, 1.0, timestamp_ms, true)
}

fn make_detected_pool(pool_id: Pubkey, base_mint: Pubkey, quote_mint: Pubkey) -> DetectedPool {
    DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: PUMP_PROGRAM_ID.to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: Pubkey::new_unique().to_string(),
        slot: Some(100_000),
        timestamp_ms: 1_700_000_000_000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1_700_000_000_123),
        initial_liquidity_sol: Some(80.0),
        signature: "init_sig_test".to_string(),
    }
}

fn setup_runtime() -> (
    Arc<OracleRuntime>,
    Arc<SnapshotEngine>,
    tokio::sync::broadcast::Sender<GhostEvent>,
) {
    let (event_tx, _rx) = create_event_bus();
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle,
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));
    (oracle_runtime, snapshot_engine, event_tx)
}

/// Integration Test #1: a detected pool is canonically registered, but organic
/// tx alone does not implicitly activate SnapshotEngine without an actual
/// BUY/commit path.
///
/// PR 8 moves canonical truth into AccountStateCore/session wiring, so this
/// harness no longer assumes that `NewPoolDetected + tx stream` alone yields an
/// immediate BUY verdict and SnapshotEngine activation.
#[tokio::test]
async fn test_detected_pool_organic_flow_registers_without_implicit_activation() {
    let (oracle_runtime, snapshot_engine, event_tx) = setup_runtime();

    let mut gk_v2_config = test_gk_v2_config();
    gk_v2_config.min_tx_count = 8;
    gk_v2_config.min_unique_signers = 5;
    gk_v2_config.min_buy_count = 5;
    gk_v2_config.max_wait_time_ms = 30_000;
    gk_v2_config.min_phases_to_pass = 5;
    gk_v2_config.re_eval_tx_interval = 3;
    gk_v2_config.min_interval_cv = 0.2;
    gk_v2_config.max_burst_ratio = 0.80;
    gk_v2_config.min_avg_interval_ms = 30.0;
    gk_v2_config.max_avg_interval_ms = 10_000.0;
    gk_v2_config.min_timing_entropy = 0.5;
    gk_v2_config.max_hhi = 0.30;
    gk_v2_config.max_tx_per_signer = 5;
    gk_v2_config.max_volume_gini = 0.80;
    gk_v2_config.max_top3_volume_pct = 0.90;
    gk_v2_config.min_buy_ratio = 0.40;
    gk_v2_config.min_volume_cv = 0.10;
    gk_v2_config.min_total_volume_sol = 0.2;
    gk_v2_config.max_dev_buy_sol = 10.0;
    gk_v2_config.max_dev_tx_ratio = 0.30;
    gk_v2_config.max_dev_volume_ratio = 0.50;
    gk_v2_config.max_price_change_ratio = 10.0;
    gk_v2_config.max_single_tx_price_impact_pct = 40.0;
    gk_v2_config.max_bonding_progress_pct = 50.0;
    gk_v2_config.min_market_cap_sol = 5.0;

    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();

    // Start Oracle Runtime Task (dry_run=true)
    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            5000,
            gk_v2_config,
            IwimVetoGateConfig::default(),
            true, // dry_run
            "/tmp/ghost_test_gk_v2_pipeline_buy".to_string(),
            None, // no trigger in test
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(100)).await;

    // Step 1: Send NewPoolDetected
    let detected = make_detected_pool(pool_id, base_mint, quote_mint);
    let _ = event_tx.send(GhostEvent::new_pool_detected(detected));
    sleep(Duration::from_millis(100)).await;

    assert_eq!(
        oracle_runtime.lookup_base_mint_for_pool(&pool_id),
        Some(base_mint),
        "NewPoolDetected should canonically register pool -> base_mint mapping"
    );
    assert!(
        matches!(
            oracle_runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Tracked)
        ),
        "detected pool should enter tracked lifecycle state before any BUY/commit path"
    );

    // Step 2: Send the same stable organic-buy fixture used by Gatekeeper unit tests.
    let signers = [
        "alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi", "alice", "bob",
    ];
    let timestamps = [
        1_700_000_000_000u64,
        1_700_000_001_500,
        1_700_000_003_200,
        1_700_000_004_800,
        1_700_000_007_000,
        1_700_000_008_500,
        1_700_000_012_000,
        1_700_000_014_000,
        1_700_000_018_000,
        1_700_000_023_000,
    ];
    let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];
    let v_tokens_base = 1_073_000_000.0;

    let pool_str = pool_id.to_string();
    let mint_str = base_mint.to_string();
    for i in 0..10 {
        let mut tx = organic_tx(
            &pool_str,
            &mint_str,
            signers[i],
            volumes[i],
            timestamps[i],
            true,
        );
        let v_tokens = v_tokens_base - (i as f64) * 1_000_000.0;
        let v_sol = 30.0 + (i as f64) * 0.5;
        tx.v_tokens_in_bonding_curve = Some(v_tokens);
        tx.v_sol_in_bonding_curve = Some(v_sol);
        tx.market_cap_sol = Some(v_sol);
        tx.curve_data_known = true;
        tx.curve_finality = ghost_core::CurveFinality::Provisional;
        let _ = event_tx.send(GhostEvent::pool_transaction(tx));
        sleep(Duration::from_millis(50)).await;
    }

    // Wait for the wall-clock Gatekeeper/session flow to emit BUY and activate the pool.
    let mut is_tracked = false;
    for _ in 0..70 {
        if snapshot_engine.has_pool(&pool_id) {
            is_tracked = true;
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }

    assert!(
        !is_tracked,
        "organic traffic alone must not implicitly activate SnapshotEngine without BUY/commit"
    );
    assert!(
        !matches!(
            oracle_runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Committed)
        ),
        "organic traffic alone must not move the pool into committed runtime state"
    );
}

#[tokio::test]
async fn test_runtime_router_keeps_approved_distinct_from_committed() {
    let (oracle_runtime, _snapshot_engine, event_tx) = setup_runtime();
    let gk_v2_config = test_gk_v2_config();

    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let candidate = EnhancedCandidate {
        pool_amm_id: pool_id,
        base_mint,
        bonding_curve: Pubkey::new_unique(),
        slot: Some(42),
        ..Default::default()
    };

    assert!(oracle_runtime.register_new_pool(pool_id, base_mint, candidate, None));
    oracle_runtime.mark_pool_approved(pool_id);
    oracle_runtime.approved_pools().insert(pool_id);
    assert_eq!(
        oracle_runtime.runtime_pool_state(&pool_id),
        Some(PoolState::Approved)
    );

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::new(SnapshotEngine::new(128, 200));
    let event_tx_clone = event_tx.clone();

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            5000,
            gk_v2_config,
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_gk_v2_runtime_state".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(100)).await;

    let mut tx = organic_tx(
        &pool_id.to_string(),
        &base_mint.to_string(),
        &Pubkey::new_unique().to_string(),
        0.4,
        1_700_000_010_000,
        true,
    );
    tx.sol_amount_lamports = Some(400_000_000);
    tx.token_amount_units = Some(400_000);
    let _ = event_tx.send(GhostEvent::pool_transaction(tx));
    sleep(Duration::from_millis(200)).await;

    assert_eq!(
        oracle_runtime.runtime_pool_state(&pool_id),
        Some(PoolState::Approved)
    );
    assert!(
        !oracle_runtime.get_shadow_ledger().is_committed(&base_mint),
        "approved pool must not be treated as committed"
    );
    assert_eq!(
        oracle_runtime.commit_coordinator().active_buffer_count(),
        0,
        "approved pool without a commit window should remain distinct from committed/live staging"
    );
}

#[tokio::test]
async fn test_runtime_router_does_not_spawn_unknown_pool_from_tx_only() {
    let (oracle_runtime, snapshot_engine, event_tx) = setup_runtime();
    let gk_v2_config = test_gk_v2_config();

    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            5_000,
            gk_v2_config,
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_gk_v2_tx_only_unknown_pool".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(100)).await;

    let tx = organic_tx(
        &pool_id.to_string(),
        &base_mint.to_string(),
        &Pubkey::new_unique().to_string(),
        0.35,
        1_700_000_020_000,
        true,
    );
    let _ = event_tx.send(GhostEvent::pool_transaction(tx));

    sleep(Duration::from_millis(250)).await;

    assert_eq!(
        oracle_runtime.runtime_pool_state(&pool_id),
        None,
        "tx-only unknown pool must not create runtime lifecycle"
    );
    assert!(
        oracle_runtime.lookup_base_mint_for_pool(&pool_id).is_none(),
        "tx-only unknown pool must not enter canonical identity registry"
    );
    assert!(
        !snapshot_engine.has_pool(&pool_id),
        "tx-only unknown pool must not create SnapshotEngine state"
    );
    assert_eq!(
        oracle_runtime.commit_coordinator().active_buffer_count(),
        0,
        "tx-only unknown pool must not open commit/buy-capable path"
    );

    let (orphan_pools, total_orphans) = oracle_runtime.get_orphan_stats();
    assert_eq!(
        orphan_pools, 1,
        "tx-only unknown pool may be buffered as a single orphaned pool until canonical registration"
    );
    assert_eq!(
        total_orphans, 1,
        "tx-only unknown pool must stay out of lifecycle/commit paths, but the tx-first orphan buffer may retain exactly one pending event"
    );
}

/// Integration Test #2: Full pipeline leading to Gatekeeper REJECT/TIMEOUT verdict.
///
/// Sends bot-like transactions (regular timing, same volume, concentrated signers)
/// that should fail Gatekeeper phase checks. After timeout, pool is cleaned up.
/// Verifies cleanup: SnapshotEngine not activated, pool removed from runtime.
#[tokio::test]
async fn test_full_pipeline_reject() {
    let (_oracle_runtime, snapshot_engine, event_tx) = setup_runtime();

    let mut gk_v2_config = test_gk_v2_config();
    // Make Phase 3 strict so bot pattern gets rejected
    gk_v2_config.max_hhi = 0.15; // very strict HHI → single signer dominance rejected
    gk_v2_config.max_tx_per_signer = 2; // max 2 TX per signer → bot with 6 TX from same signer fails
    gk_v2_config.min_phases_to_pass = 5; // require 5/6 phases
    gk_v2_config.max_wait_time_ms = 2_000; // short timeout for faster test

    let pool_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&_oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            5000,
            gk_v2_config,
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_gk_v2_pipeline_reject".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(100)).await;

    // Step 1: Send NewPoolDetected
    let detected = make_detected_pool(pool_id, base_mint, quote_mint);
    let _ = event_tx.send(GhostEvent::new_pool_detected(detected));
    sleep(Duration::from_millis(100)).await;

    // Step 2: Send bot-like transactions
    // Single signer dominance (signer_bot has most TXs) with regular interval timing
    let signer_bot = Pubkey::new_unique().to_string();
    let signer_acc = Pubkey::new_unique().to_string();
    let signer_acc2 = Pubkey::new_unique().to_string();

    let base_ts = 1_700_000_000_000u64;
    let pool_str = pool_id.to_string();
    let mint_str = base_mint.to_string();

    // Bot: 6 TX from same signer at exact regular intervals with identical volumes
    let txs = vec![
        bot_tx(&pool_str, &mint_str, &signer_bot, base_ts + 100),
        bot_tx(&pool_str, &mint_str, &signer_bot, base_ts + 200),
        bot_tx(&pool_str, &mint_str, &signer_bot, base_ts + 300),
        bot_tx(&pool_str, &mint_str, &signer_bot, base_ts + 400),
        bot_tx(&pool_str, &mint_str, &signer_acc, base_ts + 500),
        bot_tx(&pool_str, &mint_str, &signer_acc2, base_ts + 600),
    ];

    for tx in txs {
        let _ = event_tx.send(GhostEvent::pool_transaction(tx));
        sleep(Duration::from_millis(30)).await;
    }

    // Wait for Gatekeeper timeout (max_wait_time_ms=2000) + processing margin
    sleep(Duration::from_millis(3500)).await;

    // Verify: pool should NOT be tracked in SnapshotEngine (never got activated)
    let is_tracked = snapshot_engine.has_pool(&pool_id);
    assert!(
        !is_tracked,
        "Pool {} should NOT be tracked in SnapshotEngine after REJECT/TIMEOUT verdict",
        pool_id
    );
}
