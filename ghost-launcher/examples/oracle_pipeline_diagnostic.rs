//! Oracle Pipeline Diagnostic Tool
//!
//! This example helps diagnose Oracle Pipeline issues by:
//! 1. Testing the event bus in isolation
//! 2. Simulating the complete pipeline
//! 3. Verifying each component's functionality
//!
//! Run with: cargo run -p ghost-launcher --example oracle_pipeline_diagnostic

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
use tracing::warn;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// Default program IDs
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    println!("\n╔════════════════════════════════════════════════════════════════════════╗");
    println!("║              Oracle Pipeline Diagnostic Tool v1.0                     ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝\n");

    // Test 1: Event Bus Creation
    println!("🔍 Test 1: Event Bus Creation");
    let (event_tx, _event_rx) = create_event_bus();
    println!("   ✅ Event bus created successfully");
    println!(
        "   ℹ️  Initial receiver count: {}\n",
        event_tx.receiver_count()
    );

    // Test 2: Event Bus Subscribe
    println!("🔍 Test 2: Event Bus Subscription");
    let mut test_rx = event_tx.subscribe();
    println!("   ✅ Subscription created");
    println!(
        "   ℹ️  Receiver count after subscribe: {}\n",
        event_tx.receiver_count()
    );

    // Test 3: Event Transmission
    println!("🔍 Test 3: Event Transmission");
    let test_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: Pubkey::new_unique().to_string(),
        base_mint: Pubkey::new_unique().to_string(),
        quote_mint: Pubkey::new_unique().to_string(),
        amm_program: "test".to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: Pubkey::new_unique().to_string(),
        slot: Some(12345),
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000000),
        initial_liquidity_sol: Some(10.0),
        signature: "test_sig".to_string(),
    };

    if let Err(e) = event_tx.send(GhostEvent::new_pool_detected(test_pool.clone())) {
        warn!("   ❌ Failed to send event: {}", e);
        return;
    }
    println!("   ✅ Event sent successfully\n");

    // Test 4: Event Reception
    println!("🔍 Test 4: Event Reception");
    match tokio::time::timeout(Duration::from_secs(1), test_rx.recv()).await {
        Ok(Ok(event)) => {
            println!("   ✅ Event received: {}", event.event_type());
        }
        Ok(Err(e)) => {
            warn!("   ❌ Error receiving event: {}", e);
            return;
        }
        Err(_) => {
            warn!("   ❌ Timeout waiting for event");
            return;
        }
    }
    println!();

    // Test 5: Complete Oracle Pipeline
    println!("🔍 Test 5: Complete Oracle Pipeline Integration");
    println!("   ℹ️  This tests the full NewPoolDetected → Scoring → PoolScored flow\n");

    // Setup components
    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = Arc::new(OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(), // Bonk.fun program ID
        Arc::new(ShadowLedger::new()),
    ));

    // Start Oracle Runtime Task
    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let analysis_window_ms = 1000;

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
            "logs/oracle_decision.log".to_string(),
            None,
            "datasets/events".to_string(),
            None,
            false,
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(100)).await;

    // Subscribe to receive PoolScored events
    let mut scored_rx = event_tx.subscribe();

    // Emit NewPoolDetected event
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();

    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: Pubkey::new_from_array([0u8; 32]).to_string(),
        amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: Pubkey::new_unique().to_string(),
        slot: Some(12345),
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        detected_wall_ts_ms: Some(1700000000000),
        initial_liquidity_sol: Some(10.0),
        signature: "diagnostic_sig".to_string(),
    };

    println!("   📤 Emitting NewPoolDetected event...");
    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool.clone()))
        .expect("Failed to send NewPoolDetected");

    sleep(Duration::from_millis(200)).await;

    // Emit PoolTransaction events
    println!("   📤 Emitting 6 PoolTransaction events...");
    for i in 0..6 {
        let timestamp_ms = 1700000000000 + (i * 150);
        let raw_tx = vec![i as u8; 250];

        let volume_sol = 1.0 + (i as f64 * 0.5);

        let pool_tx = PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey.to_string(),
            slot: Some(12345 + i),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: timestamp_ms,
            signer: Pubkey::new_unique().to_string(),
            owner_token_deltas: vec![],
            is_buy: i % 2 == 0,
            volume_sol,
            sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
            token_amount_units: Some(1_000_000 + (i as u64 * 100_000)),
            reserve_base: Some(1000.0 + (i as f64 * 100.0)),
            reserve_quote: Some(100.0 + (i as f64 * 10.0)),
            price_quote: Some(0.1 + (i as f64 * 0.01)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: format!("diag_tx_{}", i),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            mpcf_payload: raw_tx,
            mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
            token_mint: Some(base_mint.to_string()),
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
        };

        event_tx
            .send(GhostEvent::pool_transaction(pool_tx))
            .expect("Failed to send PoolTransaction");

        sleep(Duration::from_millis(50)).await;
    }

    println!("   ⏳ Waiting for analysis window (1000ms)...");
    sleep(Duration::from_millis(700)).await;

    println!("   📥 Waiting for PoolScored event...");
    match tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match scored_rx.recv().await {
                Ok(GhostEvent::PoolScored(scored)) => {
                    return Ok(scored);
                }
                Ok(_) => continue,
                Err(e) => {
                    return Err(format!("Error receiving event: {}", e));
                }
            }
        }
    })
    .await
    {
        Ok(Ok(scored_event)) => {
            println!("\n   ✅ PIPELINE TEST PASSED!\n");
            println!("   📊 Results:");
            println!("      Pool:           {}", scored_event.pool_amm_id);
            println!("      Score:          {}", scored_event.score);
            println!("      Passed:         {}", scored_event.passed);
            println!("      Risk Level:     {}", scored_event.risk_level);
            println!(
                "      Processing:     {}μs",
                scored_event.processing_time_us
            );
        }
        Ok(Err(e)) => {
            warn!("\n   ❌ PIPELINE TEST FAILED: {}\n", e);
            return;
        }
        Err(_) => {
            warn!("\n   ❌ PIPELINE TEST FAILED: Timeout waiting for PoolScored event\n");
            warn!("   💡 This suggests Oracle Runtime is not processing events properly.");
            warn!("   💡 Check logs for errors in Oracle Runtime task.\n");
            return;
        }
    }

    // Final Summary
    println!("\n╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                       DIAGNOSTIC SUMMARY                               ║");
    println!("╠════════════════════════════════════════════════════════════════════════╣");
    println!("║  ✅ Event Bus: WORKING                                                ║");
    println!("║  ✅ Event Transmission: WORKING                                       ║");
    println!("║  ✅ Event Reception: WORKING                                          ║");
    println!("║  ✅ Oracle Pipeline: WORKING                                          ║");
    println!("║  ✅ HyperPrediction Scoring: WORKING                                  ║");
    println!("╠════════════════════════════════════════════════════════════════════════╣");
    println!("║                                                                        ║");
    println!("║  🎉 All tests passed! The Oracle Pipeline is functioning correctly.   ║");
    println!("║                                                                        ║");
    println!("║  If you're experiencing issues in production:                         ║");
    println!("║  1. Check that Seer component is enabled in config.toml              ║");
    println!("║  2. Verify Geyser/gRPC endpoint is accessible and working            ║");
    println!("║  3. Ensure network connectivity to blockchain                        ║");
    println!("║  4. Check logs for Seer connection errors                            ║");
    println!("║  5. Verify Seer is actually detecting pools (check Seer logs)        ║");
    println!("║                                                                        ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝\n");
}
