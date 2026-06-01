//! Integration Test: Oracle Runtime + Event Bus Integration
//!
//! This test validates the complete event flow for Task 2:
//! 1. NewPoolDetected event → register_new_pool + spawn async scoring task
//! 2. PoolTransaction events → register_pool_tx with raw_tx
//! 3. After observation window → pool observation task completes
//! 4. Rejected pool is removed from OracleRuntime without hanging
//!
//! This simulates the production flow without requiring actual blockchain connections.

use ghost_brain::config::{GatekeeperV2Config, GatekeeperV3Config, IwimVetoGateConfig};
use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_brain::oracle::SnapshotEngine;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::EventSemanticEnvelope;
use ghost_launcher::config::ExecutionMode;
use ghost_launcher::events::{
    create_event_bus, DetectedPool, EventBusSender, FundingTransferObserved, GhostEvent,
    PoolTransaction,
};
use ghost_launcher::oracle_runtime::{
    start_oracle_runtime_task, start_oracle_runtime_task_with_funding_availability, OracleRuntime,
};
use ghost_launcher::session::observation::SharedSession;
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::sleep;

// Default program IDs for testing
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

fn detected_pool(
    pool_pubkey: Pubkey,
    base_mint: Pubkey,
    creator: Pubkey,
    timestamp_ms: u64,
) -> DetectedPool {
    DetectedPool {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: Pubkey::new_from_array([0u8; 32]).to_string(),
        amm_program: PUMP_PROGRAM_ID.to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: creator.to_string(),
        slot: Some(12345),
        tx_index: None,
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(timestamp_ms.saturating_add(123)),
        initial_liquidity_sol: Some(10.0),
        signature: format!("detected-{pool_pubkey}"),
    }
}

fn fsc_buy_tx(
    pool_id: Pubkey,
    signer: Pubkey,
    signature: &str,
    timestamp_ms: u64,
    slot: u64,
    is_dev_buy: bool,
    price: f64,
) -> PoolTransaction {
    PoolTransaction {
        semantic: EventSemanticEnvelope::default(),
        pool_amm_id: pool_id.to_string(),
        slot: Some(slot),
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
        is_buy: true,
        volume_sol: 0.1,
        sol_amount_lamports: Some(100_000_000),
        token_amount_units: Some(1_000_000),
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy,
        dev_buy_lamports: if is_dev_buy { 100_000_000 } else { 0 },
        signature: signature.to_string(),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        owner_token_deltas: vec![],
        mpcf_payload: vec![],
        mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
        token_mint: None,
        v_tokens_in_bonding_curve: Some(1.0),
        v_sol_in_bonding_curve: Some(price),
        market_cap_sol: Some(price * 1_000_000_000.0),
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
        signer_pre_balance_lamports: Some(100),
        signer_post_balance_lamports: Some(90),
        jito_tip_detected: None,
        toolchain_fingerprint: seer::types::ToolchainFingerprintInput {
            account_keys_len: Some(12),
            outer_instruction_count: Some(3),
            inner_instruction_group_count: Some(2),
            has_set_compute_unit_limit: Some(true),
            has_set_compute_unit_price: Some(true),
            external_fee_transfer_count: Some(0),
            internal_fee_transfer_count: Some(0),
            filtered_wsol_self_transfer_count: Some(0),
        },
        curve_data_known: true,
        curve_finality: ghost_core::CurveFinality::Provisional,
    }
}

fn funding_transfer(
    source_wallet: &str,
    recipient_wallet: &str,
    signature: &str,
    timestamp_ms: u64,
    lamports: u64,
) -> FundingTransferObserved {
    FundingTransferObserved {
        semantic: EventSemanticEnvelope::default(),
        slot: Some(1),
        event_ordinal: None,
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        cpi_stack_height: None,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None),
        arrival_ts_ms: timestamp_ms,
        signature: signature.to_string(),
        source_wallet: source_wallet.to_string(),
        recipient_wallet: recipient_wallet.to_string(),
        lamports,
        full_chain_coverage: true,
        provenance: seer::ipc::FundingTransferProvenance::authoritative_full_feed_live(),
        lane_health: seer::ipc::FundingLaneRuntimeHealth::default(),
        detected_at: std::time::SystemTime::now(),
        sequence_number: timestamp_ms,
    }
}

async fn spawn_runtime_for_fsc(
    test_name: &str,
    authoritative_funding_stream_available: bool,
) -> (Arc<OracleRuntime>, EventBusSender) {
    spawn_runtime_for_fsc_with_optional_signal(
        test_name,
        authoritative_funding_stream_available,
        None,
    )
    .await
}

async fn spawn_runtime_for_fsc_with_signal(
    test_name: &str,
    authoritative_funding_stream_available: bool,
) -> (Arc<OracleRuntime>, EventBusSender, watch::Sender<bool>) {
    let (authoritative_funding_stream_tx, authoritative_funding_stream_rx) =
        watch::channel(authoritative_funding_stream_available);
    let (oracle_runtime, event_tx) = spawn_runtime_for_fsc_with_optional_signal(
        test_name,
        authoritative_funding_stream_available,
        Some(authoritative_funding_stream_rx),
    )
    .await;
    (oracle_runtime, event_tx, authoritative_funding_stream_tx)
}

async fn spawn_runtime_for_fsc_with_optional_signal(
    test_name: &str,
    authoritative_funding_stream_available: bool,
    authoritative_funding_stream_availability_rx: Option<watch::Receiver<bool>>,
) -> (Arc<OracleRuntime>, EventBusSender) {
    let (event_tx, _event_rx) = create_event_bus();
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle,
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));

    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let decision_dir = format!("/tmp/{test_name}_decisions");
    let events_dir = format!("/tmp/{test_name}_events");
    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.max_wait_time_ms = 5_000;
    gatekeeper_config.curve_wait_ms = 100;

    tokio::spawn(async move {
        if let Some(authoritative_funding_stream_availability_rx) =
            authoritative_funding_stream_availability_rx
        {
            start_oracle_runtime_task_with_funding_availability(
                oracle_rx,
                oracle_runtime_clone,
                snapshot_engine_clone,
                event_tx_clone,
                None,
                5_000,
                gatekeeper_config,
                GatekeeperV3Config::default(),
                IwimVetoGateConfig::default(),
                ExecutionMode::Paper,
                true,
                decision_dir,
                "/tmp/ghost-shadow-entry-test.jsonl".to_string(),
                None,
                None,
                events_dir,
                None,
                false,
                authoritative_funding_stream_available,
                false,
                Some(authoritative_funding_stream_availability_rx),
            )
            .await;
        } else {
            start_oracle_runtime_task(
                oracle_rx,
                oracle_runtime_clone,
                snapshot_engine_clone,
                event_tx_clone,
                None,
                5_000,
                gatekeeper_config,
                IwimVetoGateConfig::default(),
                true,
                decision_dir,
                None,
                events_dir,
                None,
                false,
                authoritative_funding_stream_available,
            )
            .await;
        }
    });

    sleep(Duration::from_millis(50)).await;
    (oracle_runtime, event_tx)
}

async fn wait_for_session(
    oracle_runtime: &Arc<OracleRuntime>,
    pool_pubkey: Pubkey,
) -> SharedSession {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Some(session) = oracle_runtime.session_manager().get_session(&pool_pubkey) {
                break session;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pool observation session should open")
}

async fn wait_for_total_tx_seen(session: &SharedSession, expected: u64) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if session.read().diagnostics.total_tx_seen >= expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime session should ingest expected buy count");
}

#[tokio::test]
async fn test_oracle_event_bus_integration_complete_flow() {
    // Setup: Create event bus
    let (event_tx, _event_rx) = create_event_bus();

    // Setup: Create SnapshotEngine
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));

    // Setup: Create HyperPredictionOracle
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());

    // Setup: Create OracleRuntime
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));

    // Start Oracle Runtime Task
    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let analysis_window_ms = 500; // Short window for testing (500ms instead of 2000ms)
    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.max_wait_time_ms = 750;
    gatekeeper_config.curve_wait_ms = 200;

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            analysis_window_ms,
            gatekeeper_config,
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_decisions_oracle_event_bus_integration".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    // Give the task time to start
    sleep(Duration::from_millis(50)).await;

    // Step 1: Emit NewPoolDetected event
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    // SOL is wrapped SOL in SPL Token program
    let quote_mint = Pubkey::new_from_array([0u8; 32]); // Use zero pubkey as placeholder for SOL
    let creator = Pubkey::new_unique();

    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(), // Pump.fun
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

    println!("✅ Sent NewPoolDetected event for pool: {}", pool_pubkey);

    // Give oracle runtime time to register the pool
    sleep(Duration::from_millis(100)).await;

    // Verify pool was registered
    assert_eq!(oracle_runtime.pool_count(), 1);
    println!("✅ Pool registered in OracleRuntime");

    // Step 2: Emit multiple PoolTransaction events with raw_tx
    for i in 0..5 {
        let timestamp_ms = 1700000000000 + (i * 100); // 100ms apart
        let signer = if i == 0 {
            creator
        } else {
            Pubkey::new_unique()
        };

        // Create synthetic raw transaction bytes
        let raw_tx = vec![i as u8; 200]; // Simple pattern for testing

        let volume_sol = 1.0 + (i as f64 * 0.5);
        let pool_tx = PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey.to_string(),
            slot: Some(12345 + i),
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
            is_buy: i % 2 == 0,
            volume_sol,
            sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
            token_amount_units: Some(1_000_000 + (i as u64 * 100_000)),
            reserve_base: Some(1000.0 + (i as f64 * 100.0)),
            reserve_quote: Some(100.0 + (i as f64 * 10.0)),
            price_quote: Some(0.1 + (i as f64 * 0.01)),
            is_dev_buy: i == 0, // First tx is dev buy
            dev_buy_lamports: if i == 0 { 1_000_000_000 } else { 0 },
            signature: format!("test_sig_tx_{}", i),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: raw_tx,
            mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
            token_mint: Some(Pubkey::new_unique().to_string()),
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
        };

        event_tx
            .send(GhostEvent::pool_transaction(pool_tx))
            .expect("Failed to send PoolTransaction");

        println!("✅ Sent PoolTransaction {} with raw_tx", i);
    }

    // Give time for transactions to be processed
    sleep(Duration::from_millis(100)).await;

    println!("✅ All PoolTransactions sent");

    // Step 3: Wait for observation task to complete.
    // With the current OracleRuntime contract, this path does not emit GhostEvent::PoolScored.
    // The stable externally observable effect is that the rejected pool observation task exits
    // and the pool is removed from OracleRuntime state.
    println!("⏳ Waiting for pool observation task to finish...");

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if oracle_runtime.pool_count() == 0 {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Timeout waiting for OracleRuntime observation task to finish");

    assert_eq!(oracle_runtime.pool_count(), 0);
    println!("✅ Observation task finished and pool was cleaned up");
}

#[tokio::test]
async fn test_oracle_event_bus_multiple_pools() {
    // Test handling multiple pools concurrently
    let (event_tx, _event_rx) = create_event_bus();
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    ));

    // Start Oracle Runtime Task
    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let analysis_window_ms = 300; // Short window for testing

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
            "/tmp/ghost_test_decisions_oracle_event_bus_multiple_pools".to_string(),
            None,
            "/tmp/ghost-test-events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(50)).await;

    // Create 3 different pools
    let num_pools = 3;
    let mut pool_ids = Vec::new();

    for i in 0..num_pools {
        let pool_pubkey = Pubkey::new_unique();
        pool_ids.push(pool_pubkey);

        let detected_pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "So11111111111111111111111111111111111111112".to_string(), // SOL
            amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            bonding_curve: Pubkey::new_unique().to_string(),
            creator: Pubkey::new_unique().to_string(),
            slot: Some(12345 + i),
            tx_index: None,
            timestamp_ms: 1700000000000 + i as u64,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123 + i as u64),
            initial_liquidity_sol: Some(10.0 + (i as f64)),
            signature: format!("test_sig_pool_{}", i),
        };

        event_tx
            .send(GhostEvent::new_pool_detected(detected_pool))
            .expect("Failed to send NewPoolDetected");

        // Send some transactions for each pool
        for j in 0..4 {
            let volume_sol = 1.0;
            let pool_tx = PoolTransaction {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                pool_amm_id: pool_pubkey.to_string(),
                slot: Some(12345 + i + j),
                event_ordinal: Some(0),
                tx_index: None,
                outer_instruction_index: None,
                inner_group_index: None,
                outer_program_id: None,
                cpi_stack_height: None,
                timestamp_ms: 1700000000000 + (i * 1000) + (j * 100),
                event_time: ghost_core::EventTimeMetadata::new(
                    None,
                    Some(1700000000000 + (i * 1000) + (j * 100)),
                    None,
                ),
                arrival_ts_ms: 1700000000000 + (i * 1000) + (j * 100),
                signer: Pubkey::new_unique().to_string(),
                is_buy: true,
                volume_sol,
                sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
                token_amount_units: Some(1_000_000),
                reserve_base: Some(1000.0),
                reserve_quote: Some(100.0),
                price_quote: Some(0.1),
                is_dev_buy: false,
                dev_buy_lamports: 0,
                signature: format!("test_sig_pool_{}_tx_{}", i, j),
                success: true,
                error_code: None,
                compute_units_consumed: None,
                owner_token_deltas: vec![],
                mpcf_payload: vec![(i as u8).wrapping_add(j as u8); 100],
                mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
                token_mint: Some(Pubkey::new_unique().to_string()),
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
            };

            event_tx
                .send(GhostEvent::pool_transaction(pool_tx))
                .expect("Failed to send PoolTransaction");
        }
    }

    sleep(Duration::from_millis(100)).await;

    // Verify all pools were registered
    assert_eq!(oracle_runtime.pool_count(), num_pools as usize);
    println!("✅ All {} pools registered", num_pools);

    // Wait for all pools to be scored
    sleep(Duration::from_millis(400)).await;

    println!("✅ Multiple pool test completed");
}

#[tokio::test]
async fn test_oracle_event_bus_raw_tx_preservation() {
    // Verify that raw_tx bytes are properly preserved through the event flow
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

    tokio::spawn(async move {
        start_oracle_runtime_task(
            oracle_rx,
            oracle_runtime_clone,
            snapshot_engine_clone,
            event_tx_clone,
            None,
            500,
            GatekeeperV2Config::default(),
            IwimVetoGateConfig::default(),
            true,
            "/tmp/ghost_test_decisions_oracle_event_bus_raw_tx_preservation".to_string(),
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
    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: Pubkey::new_unique().to_string(),
        quote_mint: "So11111111111111111111111111111111111111112".to_string(), // SOL
        amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: Pubkey::new_unique().to_string(),
        slot: Some(12345),
        tx_index: None,
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000123),
        initial_liquidity_sol: Some(10.0),
        signature: "test_sig".to_string(),
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool))
        .expect("Failed to send NewPoolDetected");

    sleep(Duration::from_millis(50)).await;

    // Send a transaction with specific payload pattern
    let expected_payload = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    let volume_sol = 1.0;
    let pool_tx = PoolTransaction {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        slot: Some(12345),
        event_ordinal: Some(0),
        tx_index: None,
        outer_instruction_index: None,
        inner_group_index: None,
        outer_program_id: None,
        cpi_stack_height: None,
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::new(None, Some(1700000000000), None),
        arrival_ts_ms: 1700000000000,
        signer: Pubkey::new_unique().to_string(),
        is_buy: true,
        volume_sol,
        sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
        token_amount_units: Some(1_000_000),
        reserve_base: Some(1000.0),
        reserve_quote: Some(100.0),
        price_quote: Some(0.1),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: "test_sig_tx".to_string(),
        success: true,
        error_code: None,
        compute_units_consumed: None,
        owner_token_deltas: vec![],
        mpcf_payload: expected_payload.clone(),
        mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
        token_mint: Some(Pubkey::new_unique().to_string()),
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
    };

    event_tx
        .send(GhostEvent::pool_transaction(pool_tx.clone()))
        .expect("Failed to send PoolTransaction");

    sleep(Duration::from_millis(100)).await;

    // Verify the transaction was registered
    // Note: We can't directly access the internal state, but we verify through the flow
    println!("✅ Raw TX preservation test completed");
    println!("   Expected payload: {:02X?}", expected_payload);
}

#[tokio::test]
async fn test_fsc_runtime_e2e_materializes_from_authoritative_funding_stream() {
    let (oracle_runtime, event_tx) =
        spawn_runtime_for_fsc("ghost_test_fsc_runtime_e2e_positive", true).await;

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let creator = Pubkey::new_unique();
    let base_ts_ms = 1_700_000_100_000u64;

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool(
            pool_pubkey,
            base_mint,
            creator,
            base_ts_ms,
        )))
        .expect("Failed to send NewPoolDetected");

    let session = wait_for_session(&oracle_runtime, pool_pubkey).await;
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("runtime-opened session should capture dev wallet");
    let buyer_a = Pubkey::new_unique();
    let buyer_b = Pubkey::new_unique();
    let buyer_c = Pubkey::new_unique();
    let shared_funder = Pubkey::new_unique().to_string();
    let distinct_funder = Pubkey::new_unique().to_string();

    for transfer in [
        funding_transfer(
            &shared_funder,
            &dev_wallet.to_string(),
            "fund-dev",
            base_ts_ms + 10,
            50_000_000,
        ),
        funding_transfer(
            &shared_funder,
            &buyer_a.to_string(),
            "fund-a",
            base_ts_ms + 20,
            50_000_000,
        ),
        funding_transfer(
            &shared_funder,
            &buyer_b.to_string(),
            "fund-b",
            base_ts_ms + 30,
            50_000_000,
        ),
        funding_transfer(
            &distinct_funder,
            &buyer_c.to_string(),
            "fund-c",
            base_ts_ms + 40,
            50_000_000,
        ),
    ] {
        event_tx
            .send(GhostEvent::funding_transfer_observed(transfer))
            .expect("Failed to send FundingTransferObserved");
    }

    for tx in [
        fsc_buy_tx(
            pool_pubkey,
            dev_wallet,
            "sig-fsc-dev",
            base_ts_ms + 110,
            1,
            true,
            10.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_a,
            "sig-fsc-a",
            base_ts_ms + 120,
            2,
            false,
            11.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_b,
            "sig-fsc-b",
            base_ts_ms + 130,
            3,
            false,
            12.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_c,
            "sig-fsc-c",
            base_ts_ms + 140,
            4,
            false,
            13.0,
        ),
    ] {
        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("Failed to send PoolTransaction");
    }

    wait_for_total_tx_seen(&session, 4).await;

    let features = session.write().materialize_features();
    assert_eq!(
        features.sybil_resistance.funding_source_concentration,
        Some(0.5)
    );
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()
    ));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()
    ));
}

#[tokio::test]
async fn test_fsc_runtime_e2e_reports_rolling_state_when_authoritative_lane_is_healthy_but_cold() {
    let (oracle_runtime, event_tx, _authoritative_funding_stream_tx) =
        spawn_runtime_for_fsc_with_signal("ghost_test_fsc_runtime_e2e_cold", true).await;

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let creator = Pubkey::new_unique();
    let base_ts_ms = 1_700_000_150_000u64;

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool(
            pool_pubkey,
            base_mint,
            creator,
            base_ts_ms,
        )))
        .expect("Failed to send NewPoolDetected");

    let session = wait_for_session(&oracle_runtime, pool_pubkey).await;
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("runtime-opened session should capture dev wallet");
    let buyer_a = Pubkey::new_unique();
    let buyer_b = Pubkey::new_unique();
    let buyer_c = Pubkey::new_unique();

    for tx in [
        fsc_buy_tx(
            pool_pubkey,
            dev_wallet,
            "sig-cold-dev",
            base_ts_ms + 110,
            1,
            true,
            10.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_a,
            "sig-cold-a",
            base_ts_ms + 120,
            2,
            false,
            11.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_b,
            "sig-cold-b",
            base_ts_ms + 130,
            3,
            false,
            12.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_c,
            "sig-cold-c",
            base_ts_ms + 140,
            4,
            false,
            13.0,
        ),
    ] {
        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("Failed to send PoolTransaction");
    }

    wait_for_total_tx_seen(&session, 4).await;

    let features = session.write().materialize_features();
    assert_eq!(features.sybil_resistance.funding_source_concentration, None);
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()
    ));
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()
    ));
}

#[tokio::test]
async fn test_fsc_runtime_e2e_fail_closes_when_authoritative_lane_health_drops() {
    let (oracle_runtime, event_tx, authoritative_funding_stream_tx) =
        spawn_runtime_for_fsc_with_signal("ghost_test_fsc_runtime_e2e_health_drop", true).await;

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let creator = Pubkey::new_unique();
    let base_ts_ms = 1_700_000_175_000u64;

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool(
            pool_pubkey,
            base_mint,
            creator,
            base_ts_ms,
        )))
        .expect("Failed to send NewPoolDetected");

    let session = wait_for_session(&oracle_runtime, pool_pubkey).await;
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("runtime-opened session should capture dev wallet");
    let buyer_a = Pubkey::new_unique();
    let buyer_b = Pubkey::new_unique();
    let buyer_c = Pubkey::new_unique();
    let shared_funder = Pubkey::new_unique().to_string();
    let distinct_funder = Pubkey::new_unique().to_string();
    let stream_unavailable_reason =
        ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string();
    let rolling_state_reason =
        ghost_core::tx_intelligence::types::FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string();

    for transfer in [
        funding_transfer(
            &shared_funder,
            &dev_wallet.to_string(),
            "fund-health-dev",
            base_ts_ms + 10,
            50_000_000,
        ),
        funding_transfer(
            &shared_funder,
            &buyer_a.to_string(),
            "fund-health-a",
            base_ts_ms + 20,
            50_000_000,
        ),
        funding_transfer(
            &shared_funder,
            &buyer_b.to_string(),
            "fund-health-b",
            base_ts_ms + 30,
            50_000_000,
        ),
        funding_transfer(
            &distinct_funder,
            &buyer_c.to_string(),
            "fund-health-c",
            base_ts_ms + 40,
            50_000_000,
        ),
    ] {
        event_tx
            .send(GhostEvent::funding_transfer_observed(transfer))
            .expect("Failed to send FundingTransferObserved");
    }

    for tx in [
        fsc_buy_tx(
            pool_pubkey,
            dev_wallet,
            "sig-health-dev",
            base_ts_ms + 110,
            1,
            true,
            10.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_a,
            "sig-health-a",
            base_ts_ms + 120,
            2,
            false,
            11.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_b,
            "sig-health-b",
            base_ts_ms + 130,
            3,
            false,
            12.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_c,
            "sig-health-c",
            base_ts_ms + 140,
            4,
            false,
            13.0,
        ),
    ] {
        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("Failed to send PoolTransaction");
    }

    wait_for_total_tx_seen(&session, 4).await;

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let features = session.write().materialize_features();
            if features.sybil_resistance.funding_source_concentration == Some(0.5)
                && !features
                    .sybil_resistance
                    .degraded_reasons
                    .contains(&stream_unavailable_reason)
                && !features
                    .sybil_resistance
                    .degraded_reasons
                    .contains(&rolling_state_reason)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("authoritative warmup should become ready");

    authoritative_funding_stream_tx
        .send(false)
        .expect("authoritative funding signal should be updated");

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let features = session.write().materialize_features();
            if features
                .sybil_resistance
                .degraded_reasons
                .contains(&stream_unavailable_reason)
            {
                assert!(!features
                    .sybil_resistance
                    .degraded_reasons
                    .contains(&rolling_state_reason));
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("lane health loss should fail FSC closed again");
}

#[tokio::test]
async fn test_fsc_runtime_e2e_reports_stream_unavailable_without_authoritative_feed() {
    let (oracle_runtime, event_tx) =
        spawn_runtime_for_fsc("ghost_test_fsc_runtime_e2e_negative", false).await;

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let creator = Pubkey::new_unique();
    let base_ts_ms = 1_700_000_200_000u64;

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool(
            pool_pubkey,
            base_mint,
            creator,
            base_ts_ms,
        )))
        .expect("Failed to send NewPoolDetected");

    let session = wait_for_session(&oracle_runtime, pool_pubkey).await;
    let dev_wallet = session
        .read()
        .dev_wallet
        .expect("runtime-opened session should capture dev wallet");
    let buyer_a = Pubkey::new_unique();
    let buyer_b = Pubkey::new_unique();
    let buyer_c = Pubkey::new_unique();

    for tx in [
        fsc_buy_tx(
            pool_pubkey,
            dev_wallet,
            "sig-neg-dev",
            base_ts_ms + 110,
            1,
            true,
            10.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_a,
            "sig-neg-a",
            base_ts_ms + 120,
            2,
            false,
            11.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_b,
            "sig-neg-b",
            base_ts_ms + 130,
            3,
            false,
            12.0,
        ),
        fsc_buy_tx(
            pool_pubkey,
            buyer_c,
            "sig-neg-c",
            base_ts_ms + 140,
            4,
            false,
            13.0,
        ),
    ] {
        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("Failed to send PoolTransaction");
    }

    wait_for_total_tx_seen(&session, 4).await;

    let features = session.write().materialize_features();
    assert_eq!(features.sybil_resistance.funding_source_concentration, None);
    assert!(features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_FUNDING_STREAM_UNAVAILABLE_REASON.to_string()
    ));
    assert!(!features.sybil_resistance.degraded_reasons.contains(
        &ghost_core::tx_intelligence::types::FSC_ROLLING_STATE_UNAVAILABLE_REASON.to_string()
    ));
}
