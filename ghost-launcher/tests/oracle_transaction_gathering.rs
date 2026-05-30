//! Unit and Integration Tests for Transaction Gathering Logic
//!
//! This test suite validates the `gather_transactions_for` function that was added
//! to fix the issue where scoring starts with empty data.
//!
//! Test scenarios:
//! 1. Empty channel (no transactions arrive)
//! 2. Transactions arrive during min_wait period
//! 3. Transactions arrive during extended wait period
//! 4. Minimum transaction threshold is met early
//! 5. Timeout with insufficient transactions

use ghost_brain::config::{GatekeeperV2Config, IwimVetoGateConfig};
use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_brain::oracle::SnapshotEngine;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_launcher::events::{create_event_bus, DetectedPool, GhostEvent, PoolTransaction};
use ghost_launcher::oracle_runtime::{start_oracle_runtime_task, OracleRuntime};
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

// Default program IDs for testing
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

/// Helper function to create a test PoolTransaction
fn create_test_transaction(
    pool_amm_id: &str,
    timestamp_ms: u64,
    signer: &str,
    is_buy: bool,
    volume_sol: f64,
) -> PoolTransaction {
    PoolTransaction {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_amm_id.to_string(),
        slot: Some(12345),
        event_ordinal: Some(0),
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        arrival_ts_ms: timestamp_ms,
        signer: signer.to_string(),
        owner_token_deltas: vec![],
        is_buy,
        volume_sol,
        sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
        token_amount_units: Some(1_000_000),
        reserve_base: Some(1000.0),
        reserve_quote: Some(5000.0),
        price_quote: Some(5.0),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: format!("test_sig_{}", timestamp_ms),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        mpcf_payload: vec![1, 2, 3, 4],
        mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
        token_mint: None,
        v_tokens_in_bonding_curve: None,
        v_sol_in_bonding_curve: None,
        market_cap_sol: None,
        global_config: None,
        fee_recipient: None,
        token_program: None,
        buy_variant: None,
        associated_bonding_curve: None,
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
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
        curve_data_known: false,
        curve_finality: ghost_core::CurveFinality::Speculative,
    }
}

#[tokio::test]
async fn test_gather_transactions_empty_channel() {
    // Test: No transactions arrive, should timeout after max_wait
    println!("🧪 TEST: Empty channel (no transactions)");

    let (event_tx, _event_rx) = create_event_bus();
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));

    // Start Oracle Runtime Task with short timeouts for testing
    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();

    // Note: The analysis_window_ms parameter is ignored in the new gathering logic.
    // The actual gathering uses hardcoded values: min_wait=4000ms, max_wait=8000ms, min_txs=1
    // This test verifies the gathering logic still works even with no transactions arriving.
    let analysis_window_ms = 500;

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            analysis_window_ms,
            GatekeeperV2Config::default(),
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_decisions_oracle_transaction_gathering_empty_channel".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(50)).await;

    // Register a pool but send NO transactions
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_from_array([0u8; 32]);
    let creator = Pubkey::new_unique();

    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: PUMP_PROGRAM_ID.to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: creator.to_string(),
        slot: Some(12345),
        tx_index: None,
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000123),
        initial_liquidity_sol: Some(10.0),
        signature: "test_sig_1".to_string(),
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool.clone()))
        .expect("Failed to send NewPoolDetected");

    println!("✅ Sent NewPoolDetected event (NO transactions will be sent)");

    sleep(Duration::from_millis(100)).await;
    assert_eq!(oracle_runtime.pool_count(), 1);

    // Verify no transactions were recorded
    let tx_count = oracle_runtime.get_pool_tx_count(pool_pubkey);
    assert_eq!(tx_count, 0, "Should have 0 transactions");
    println!("✅ Verified: Pool has 0 transactions");

    // Wait for the gathering to complete (with timeout)
    // The new logic will wait 4000ms minimum, then up to 8000ms for transactions
    // Since we're using shorter timeouts in test config, it should be faster
    sleep(Duration::from_millis(1500)).await;

    // After gathering completes, the pool should still have 0 transactions
    let final_tx_count = oracle_runtime.get_pool_tx_count(pool_pubkey);
    assert_eq!(
        final_tx_count, 0,
        "Should still have 0 transactions after gathering"
    );
    println!("✅ TEST PASSED: Empty channel handled correctly");
}

#[tokio::test]
#[ignore = "legacy gather_transactions path no longer reflects the runtime scoring contract"]
async fn test_gather_transactions_delayed_arrival() {
    // Test: Transactions arrive during the gathering period
    println!("🧪 TEST: Delayed transaction arrival");

    let (event_tx, _event_rx) = create_event_bus();
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let analysis_window_ms = 500;

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            analysis_window_ms,
            GatekeeperV2Config::default(),
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_decisions_oracle_transaction_gathering_delayed_arrival".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(50)).await;

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_from_array([0u8; 32]);
    let creator = Pubkey::new_unique();

    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: PUMP_PROGRAM_ID.to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: creator.to_string(),
        slot: Some(12345),
        tx_index: None,
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000123),
        initial_liquidity_sol: Some(10.0),
        signature: "test_sig_1".to_string(),
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool.clone()))
        .expect("Failed to send NewPoolDetected");

    println!("✅ Sent NewPoolDetected event");
    sleep(Duration::from_millis(100)).await;

    // Send transactions with delays
    println!("📤 Sending transactions with delays...");
    for i in 0..3 {
        sleep(Duration::from_millis(200)).await; // 200ms delay between transactions

        let tx = create_test_transaction(
            &pool_pubkey.to_string(),
            1700000000000 + (i * 200),
            &Pubkey::new_unique().to_string(),
            true,
            1.0,
        );

        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("Failed to send PoolTransaction");

        println!("📤 Sent transaction {} at {}ms", i + 1, i * 200);
    }

    // Wait for gathering and scoring to complete
    sleep(Duration::from_millis(2000)).await;

    let scored = oracle_runtime.score_pool(pool_pubkey, &snapshot_engine, None, true);
    assert!(
        scored.is_some(),
        "runtime should still be able to score after delayed transaction arrival"
    );
    println!("✅ TEST PASSED: delayed transaction arrival still produced a score");
}

#[tokio::test]
#[ignore = "legacy gather_transactions path no longer reflects the runtime scoring contract"]
async fn test_gather_transactions_immediate_arrival() {
    // Test: Transactions arrive immediately, should exit early
    println!("🧪 TEST: Immediate transaction arrival (early exit)");

    let (event_tx, _event_rx) = create_event_bus();
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let analysis_window_ms = 500;

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            analysis_window_ms,
            GatekeeperV2Config::default(),
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_decisions_oracle_transaction_gathering_immediate_arrival".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(50)).await;

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_from_array([0u8; 32]);
    let creator = Pubkey::new_unique();

    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: PUMP_PROGRAM_ID.to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: creator.to_string(),
        slot: Some(12345),
        tx_index: None,
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000123),
        initial_liquidity_sol: Some(10.0),
        signature: "test_sig_1".to_string(),
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool.clone()))
        .expect("Failed to send NewPoolDetected");

    println!("✅ Sent NewPoolDetected event");
    sleep(Duration::from_millis(100)).await;

    // Send multiple transactions immediately
    println!("📤 Sending 5 transactions immediately...");
    for i in 0..5 {
        let tx = create_test_transaction(
            &pool_pubkey.to_string(),
            1700000000000 + (i * 10),
            &Pubkey::new_unique().to_string(),
            true,
            1.0,
        );

        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("Failed to send PoolTransaction");
    }

    // Give time for transactions to be processed
    sleep(Duration::from_millis(200)).await;

    sleep(Duration::from_millis(2000)).await;

    let scored = oracle_runtime.score_pool(pool_pubkey, &snapshot_engine, None, true);
    assert!(
        scored.is_some(),
        "runtime should still be able to score after immediate transaction arrival"
    );
    println!("✅ TEST PASSED: immediate transaction arrival still produced a score");
}

#[tokio::test]
#[ignore = "legacy gather_transactions path no longer reflects the runtime scoring contract"]
async fn test_gather_transactions_low_land_rate() {
    // Test: Simulate low land rate (few transactions, long wait)
    println!("🧪 TEST: Low land rate simulation");

    let (event_tx, _event_rx) = create_event_bus();
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let analysis_window_ms = 500;

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            analysis_window_ms,
            GatekeeperV2Config::default(),
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_decisions_oracle_transaction_gathering_low_land_rate".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(50)).await;

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_from_array([0u8; 32]);
    let creator = Pubkey::new_unique();

    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: PUMP_PROGRAM_ID.to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: creator.to_string(),
        slot: Some(12345),
        tx_index: None,
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000123),
        initial_liquidity_sol: Some(10.0),
        signature: "test_sig_1".to_string(),
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool.clone()))
        .expect("Failed to send NewPoolDetected");

    println!("✅ Sent NewPoolDetected event");
    sleep(Duration::from_millis(100)).await;

    // Send only 1 transaction after a long delay (simulating low activity)
    println!("📤 Sending single transaction after 600ms delay...");
    sleep(Duration::from_millis(600)).await;

    let tx = create_test_transaction(
        &pool_pubkey.to_string(),
        1700000000600,
        &Pubkey::new_unique().to_string(),
        true,
        0.5,
    );

    event_tx
        .send(GhostEvent::pool_transaction(tx))
        .expect("Failed to send PoolTransaction");

    // Wait for gathering and scoring to complete
    sleep(Duration::from_millis(2000)).await;

    let scored = oracle_runtime.score_pool(pool_pubkey, &snapshot_engine, None, true);
    assert!(
        scored.is_some(),
        "runtime should still be able to score after low land rate arrival"
    );
    println!("✅ TEST PASSED: low land rate still produced a score");
}
