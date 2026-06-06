//! Demonstration Test: Oracle Runtime Event Pipeline Logging
//!
//! This test demonstrates the complete event flow and logging for the Oracle Runtime:
//! 1. Seer emits NewPoolDetected → logged with full details
//! 2. OracleRuntime receives event → logged
//! 3. Pool registered → logged
//! 4. Async scoring task spawned → logged
//! 5. PoolTransaction events received → logged
//! 6. Analysis window completes → logged
//! 7. HyperPrediction Oracle scoring triggered → logged
//! 8. PoolScored event published → logged
//!
//! Run with: cargo test -p ghost-launcher oracle_logging_demo -- --nocapture

use ghost_brain::config::{GatekeeperV2Config, IwimVetoGateConfig};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_brain::oracle::ultrafast::SourceType;
use ghost_brain::oracle::SnapshotEngine;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_launcher::events::{create_event_bus, DetectedPool, GhostEvent, PoolTransaction};
use ghost_launcher::oracle_runtime::{start_oracle_runtime_task, OracleRuntime};

// Default program IDs for testing
const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
use seer::types::{CandidatePool, RawBytesMissingReason, TradeEvent};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const BASELINE_PRICE_LAMPORTS: u64 = 1_000_000; // ~0.001 SOL baseline
                                                // Numerical Recipes 64-bit LCG parameters for decent period/dispersion
const LCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const LCG_INCREMENT: u64 = 1_442_695_040_888_963_407;
const BASE_INTERVAL_MS: u64 = 300;
const INTERVAL_JITTER_MS: u64 = 240;
const SYNTHETIC_TX_SIZE: usize = 96;
const SEED_SHIFT_WIDTH: usize = 8;
const SEED_SHIFT_MASK: usize = SEED_SHIFT_WIDTH - 1;
const HISTORY_PADDING_MS: u64 = 600;

fn generate_synthetic_history(
    pool_amm_id: &Pubkey,
    mint: &Pubkey,
    start_timestamp_ms: u64,
    slots: usize,
) -> Vec<TradeEvent> {
    let mut history = Vec::with_capacity(slots);
    let mut price_lamports: u64 = BASELINE_PRICE_LAMPORTS;
    let mut seed: u64 =
        pool_amm_id.to_bytes()[0] as u64 ^ mint.to_bytes()[1] as u64 ^ (slots as u64);
    let mut timestamp_ms = start_timestamp_ms.saturating_sub((slots as u64) * HISTORY_PADDING_MS);

    for i in 0..slots {
        seed = seed
            .wrapping_mul(LCG_MULTIPLIER)
            .wrapping_add(LCG_INCREMENT);
        let rand_fraction = (seed >> 33) as f64 / ((1u64 << 31) as f64);
        let jitter = rand_fraction - 0.5;
        let delta = (jitter * 0.25 * price_lamports as f64) as i64;
        let new_price = (price_lamports as i64 + delta).max(500);
        price_lamports = new_price as u64;

        let interval_ms = BASE_INTERVAL_MS + (seed % INTERVAL_JITTER_MS); // 300-539ms to build entropy
        timestamp_ms = timestamp_ms.saturating_add(interval_ms);
        let raw_tx: Vec<u8> = (0..SYNTHETIC_TX_SIZE)
            .map(|b| {
                let shift = b & SEED_SHIFT_MASK;
                (b as u8) ^ ((seed >> shift) as u8)
            })
            .collect();

        let is_buy = (seed & 1) == 0;

        history.push(TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(10_000 + i as u64),
            signature: Signature::new_unique(),
            event_ordinal: Some(i as u32),
            tx_index: None,
            provenance: None,
            timestamp_ms,
            arrival_ts_ms: timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id: *pool_amm_id,
            mint: *mint,
            signer: Pubkey::new_unique(),
            is_buy,
            amount: price_lamports,
            max_sol_cost: price_lamports * 2,
            min_sol_output: price_lamports / 2,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            mpcf_payload: raw_tx,
            mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
            is_dev_buy: false,
            owner_token_deltas: vec![],
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            creator_vault: None,
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
            is_pumpswap: false,
        });
    }

    history
}

#[tokio::test]
async fn test_oracle_logging_demo() {
    // Initialize logging for this test
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .try_init();

    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║       Oracle Runtime Event Pipeline - Complete Logging Demo           ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    // Setup: Create event bus
    let (event_tx, _event_rx) = create_event_bus();
    let decision_log_dir =
        std::env::temp_dir().join(format!("ghost_test_decisions_{}", Pubkey::new_unique()));
    let event_log_dir =
        std::env::temp_dir().join(format!("ghost_test_events_{}", Pubkey::new_unique()));
    let _ = fs::remove_dir_all(&decision_log_dir);
    let _ = fs::remove_dir_all(&event_log_dir);

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
    println!("📋 Step 1: Starting Oracle Runtime Task...");
    println!();
    let oracle_rx = event_tx.subscribe();
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let snapshot_engine_clone = Arc::clone(&snapshot_engine);
    let event_tx_clone = event_tx.clone();
    let analysis_window_ms = 1000; // 1 second for demo
    let decision_log_dir_string = decision_log_dir.to_string_lossy().into_owned();
    let event_log_dir_string = event_log_dir.to_string_lossy().into_owned();
    let mut gatekeeper_config = GatekeeperV2Config::default();
    gatekeeper_config.min_tx_count = 4;
    gatekeeper_config.min_unique_signers = 3;
    gatekeeper_config.min_buy_count = 3;
    gatekeeper_config.min_interval_cv = 0.0;
    gatekeeper_config.min_timing_entropy = 0.0;
    gatekeeper_config.curve_require_for_buy = false;
    gatekeeper_config.hard_fail_hhi = 1.0;
    gatekeeper_config.hard_fail_top3_volume_pct = 1.0;
    gatekeeper_config.hard_fail_same_ms_tx_ratio = 1.0;

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
            true, // dry_run
            decision_log_dir_string,
            None, // trigger
            event_log_dir_string,
            None,
            true, // account_updates_enabled
            false,
        )
        .await;
    });

    sleep(Duration::from_millis(100)).await;

    // Step 2: Emit NewPoolDetected event (simulating Seer)
    println!("📋 Step 2: Emitting NewPoolDetected event (simulating Seer)...");
    println!();

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_from_array([0u8; 32]);
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
        signature: "demo_sig_pool_init".to_string(),
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool.clone()))
        .expect("Failed to send NewPoolDetected");

    sleep(Duration::from_millis(200)).await;

    // Step 3: Emit PoolTransaction events
    println!();
    println!("📋 Step 3: Emitting PoolTransaction events...");
    println!();

    for i in 0..6 {
        let timestamp_ms = 1700000000000 + (i * 150);
        let signer = if i == 0 {
            creator
        } else {
            Pubkey::new_unique()
        };
        let raw_tx = vec![i as u8; 250];

        let volume_sol = 1.0 + (i as f64 * 0.5);
        let v_tokens = 1_000_000.0 - (i as f64 * 20_000.0);
        let v_sol = 10.0 + (i as f64 * 0.75);
        let market_cap_sol = (v_sol / v_tokens) * 1_000_000_000.0;

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
            owner_token_deltas: vec![],
            is_buy: i % 2 == 0,
            volume_sol,
            sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
            token_amount_units: Some(1_000_000 + (i as u64 * 100_000)),
            reserve_base: Some(1000.0 + (i as f64 * 100.0)),
            reserve_quote: Some(100.0 + (i as f64 * 10.0)),
            price_quote: Some(0.1 + (i as f64 * 0.01)),
            is_dev_buy: i == 0,
            dev_buy_lamports: if i == 0 { 1_000_000_000 } else { 0 },
            signature: format!("demo_sig_tx_{}", i),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            mpcf_payload: raw_tx,
            mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
            token_mint: Some(base_mint.to_string()),
            v_tokens_in_bonding_curve: Some(v_tokens),
            v_sol_in_bonding_curve: Some(v_sol),
            market_cap_sol: Some(market_cap_sol),
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            creator_vault: None,
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
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Finalized,
        };

        event_tx
            .send(GhostEvent::pool_transaction(pool_tx))
            .expect("Failed to send PoolTransaction");

        sleep(Duration::from_millis(50)).await;
    }

    println!();
    println!("📋 Step 4: Waiting for analysis window to complete (1000ms)...");
    println!();

    sleep(Duration::from_millis(600)).await;

    println!();
    println!("📋 Step 5: Waiting for BUY decision log and lifecycle cleanup...");
    println!();

    let buys_log_path = decision_log_dir.join("gatekeeper_v2_buys.jsonl");
    let buy_log = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if oracle_runtime.pool_count() == 0 {
                if let Ok(contents) = fs::read_to_string(&buys_log_path) {
                    if contents.contains(&pool_pubkey.to_string()) {
                        return contents;
                    }
                }
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("OracleRuntime should emit a BUY decision log and close the pool lifecycle");

    println!();
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                   FINAL RESULT - BUY Decision Logged                   ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  Pool AMM ID:       {}", pool_pubkey);
    println!("  Base Mint:         {}", base_mint);
    println!("  BUY Log Path:      {}", buys_log_path.display());
    println!("  Lifecycle Closed:  {}", oracle_runtime.pool_count() == 0);
    println!(
        "  BUY Verdict Seen:  {}",
        buy_log.contains("\"decision_verdict_buy\":true")
            || buy_log.contains("\"verdict_type\":\"BUY\"")
    );
    println!();
    assert!(
        buy_log.contains("\"decision_verdict_buy\":true")
            || buy_log.contains("\"verdict_type\":\"BUY\""),
        "BUY log should record a positive gatekeeper decision for the demo pool"
    );
    assert_eq!(
        oracle_runtime.pool_count(),
        0,
        "demo pool should complete its lifecycle and be removed after BUY"
    );

    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                          ✅ SUCCESS                                     ║");
    println!("║                                                                        ║");
    println!("║  Oracle Runtime Integration is WORKING!                               ║");
    println!("║  - Seer emits NewPoolDetected ✓                                       ║");
    println!("║  - OracleRuntime receives and processes events ✓                      ║");
    println!("║  - PoolTransaction events accumulate data ✓                           ║");
    println!("║  - Gatekeeper reaches BUY verdict ✓                                   ║");
    println!("║  - BUY decision logs are flushed and lifecycle closes ✓               ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    let _ = fs::remove_dir_all(&decision_log_dir);
    let _ = fs::remove_dir_all(&event_log_dir);
}

#[tokio::test]
async fn test_synthetic_history_prevents_data_starvation() {
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .try_init();

    let snapshot_engine = SnapshotEngine::new(32, 100);
    let hyper_oracle = Arc::new(HyperPredictionOracle::default());
    let oracle_runtime = OracleRuntime::new(
        hyper_oracle.clone(),
        PUMP_PROGRAM_ID.to_string(),
        BONK_PROGRAM_ID.to_string(),
        Arc::new(ShadowLedger::new()),
    );

    let pool_amm_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();
    let creator = Pubkey::new_unique();

    let candidate_pool = CandidatePool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        slot: Some(123_456),
        tx_index: None,
        event_ts_ms: Some(1_700_000_100_000),
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: "synthetic_sig".to_string(),
        amm_program_id: PUMP_PROGRAM_ID.parse().unwrap(),
        pool_amm_id,
        base_mint,
        quote_mint,
        bonding_curve,
        creator,
        timestamp: 1_700_000_100,
        bonding_curve_progress: None,
        initial_liquidity_sol: Some(12.0),
        token_total_supply: Some(1_000_000_000),
        block_time: Some(1_700_000_000),
    };

    let enhanced: EnhancedCandidate = candidate_pool.into();
    let registered = oracle_runtime.register_new_pool(pool_amm_id, base_mint, enhanced, None);
    assert!(registered, "Pool should register with synthetic candidate");

    let history = generate_synthetic_history(&pool_amm_id, &base_mint, 1_700_000_500_000, 64);

    let result = oracle_runtime
        .score_pool(pool_amm_id, &snapshot_engine, Some(&history), true)
        .expect("Scoring should succeed with synthetic history");

    assert!(
        !result.interpretation.is_empty(),
        "synthetic history should still yield a concrete interpretation"
    );
    assert!(
        result.processing_time_us > 0,
        "synthetic history scoring should record processing time"
    );
    if let Some(ssmi) = result.ssmi_result {
        assert_ne!(
            ssmi.source_type,
            SourceType::Unknown,
            "SSMI must not be Unknown when present"
        );
        assert!(ssmi.ssmi_score > 0.0, "SSMI score should be non-zero");
    }
}
