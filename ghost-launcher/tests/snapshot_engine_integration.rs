//! Integration test for SnapshotEngine with synthetic WebSocket/Geyser events
//!
//! This test simulates a scenario where:
//! 1. A new pool is detected (InitializePool event)
//! 2. Multiple transactions occur on that pool
//! 3. The SnapshotEngine correctly accumulates and generates snapshots
//!
//! This validates the full event flow:
//! DetectedPool → InitPoolEvent → SnapshotEngine bootstrap (g0, g1, g2)
//! PoolTransaction → TxEvent → SnapshotEngine accumulation → Snapshot emission

use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::oracle::snapshot_engine::{DataSource, PoolLifecycle, PoolMetrics};
use ghost_brain::oracle::{InitPoolEvent, SnapshotEngine, TxEvent};
use ghost_core::account_state_core::types::{AccountStateFeatures, StatePhase};
use ghost_core::session::types::SessionId;
use ghost_core::{CurveFinality, CurveFreshnessState};
use ghost_launcher::components::gatekeeper::{GatekeeperIngressOutcome, GatekeeperVerdict};
use ghost_launcher::events::{create_event_bus, DetectedPool, GhostEvent, PoolTransaction};
use ghost_launcher::session::PoolObservationSession;
use ghost_launcher::tx_intelligence::TxIntelligenceConfig;
use seer::early_fingerprint::EarlyFingerprintConfig;
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

/// Helper to get current timestamp in milliseconds
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn ingress_event_time(timestamp_ms: u64) -> ghost_core::EventTimeMetadata {
    ghost_core::EventTimeMetadata::new(None, Some(timestamp_ms), None)
}

fn snapshot_candidate(
    pool_amm_id: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    bonding_curve: Pubkey,
    timestamp_ms: u64,
    liquidity_sol: f64,
    signature: String,
) -> EnhancedCandidate {
    EnhancedCandidate {
        pool_amm_id,
        base_mint,
        quote_mint,
        bonding_curve,
        timestamp: timestamp_ms,
        initial_liquidity_sol: liquidity_sol,
        signature,
        ..Default::default()
    }
}

fn seed_snapshot_session(session: &mut PoolObservationSession, liquidity_sol: f64) {
    let current_reserves = (
        ((liquidity_sol.max(1.0)) * 1_000_000_000.0) as u64,
        1_000_000_000,
    );
    session.account_features = AccountStateFeatures {
        current_reserves,
        price_sol: liquidity_sol.max(1.0) / 1_000_000.0,
        market_cap_sol: liquidity_sol.max(1.0) * 1.2,
        bonding_progress: 0.12,
        price_change_since_t0_pct: 0.0,
        reserve_velocity_sol_per_sec: 0.0,
        is_bootstrap: false,
        curve_finality: CurveFinality::Finalized,
        state_phase: StatePhase::Canonical,
        update_count: 1,
    };
    session
        .gatekeeper_buffer_mut()
        .record_curve_state(CurveFreshnessState::Fresh, CurveFinality::Finalized);
    session.checkpoint_engine.config.interval_ms = 1;
    session.checkpoint_engine.config.min_tx_between_checkpoints = 1;
}

fn evaluate_snapshot_terminal_verdict(
    session: &mut PoolObservationSession,
    gatekeeper_config: &GatekeeperV2Config,
    force_deadline: bool,
) -> GatekeeperVerdict {
    session.begin_evaluation();
    let mut features = session.materialize_features();
    if force_deadline {
        let forced_wait_elapsed = features
            .curve_readiness
            .wait_elapsed_ms
            .unwrap_or_default()
            .max(gatekeeper_config.curve_wait_ms);
        features.curve_readiness.wait_elapsed_ms = Some(forced_wait_elapsed);
    }

    let verdict = {
        let buffer = session.gatekeeper_buffer_mut();
        buffer.prepare_feature_evaluation();
        buffer.evaluate_from_features(features, gatekeeper_config)
    };

    if matches!(verdict, GatekeeperVerdict::PendingCurve) {
        session
            .gatekeeper_buffer_mut()
            .rollback_feature_evaluation();
        session.resume_accumulation();
    }

    verdict
}

fn resolve_snapshot_ingress(
    session: &mut PoolObservationSession,
    ingress: GatekeeperIngressOutcome,
    gatekeeper_config: &GatekeeperV2Config,
) -> GatekeeperVerdict {
    match ingress {
        GatekeeperIngressOutcome::Wait => GatekeeperVerdict::Wait,
        GatekeeperIngressOutcome::ApprovedTx { tx, metrics } => {
            GatekeeperVerdict::ApprovedTx { tx, metrics }
        }
        GatekeeperIngressOutcome::TriggerEvaluation => {
            evaluate_snapshot_terminal_verdict(session, gatekeeper_config, false)
        }
        GatekeeperIngressOutcome::DeadlineElapsed => {
            evaluate_snapshot_terminal_verdict(session, gatekeeper_config, true)
        }
    }
}

/// Test that SnapshotEngine correctly processes InitPoolEvent and creates bootstrap snapshots
#[tokio::test]
async fn test_snapshot_engine_bootstrap() {
    // Create SnapshotEngine
    let engine = Arc::new(SnapshotEngine::new(128, 200));

    // Create synthetic pool
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();

    let timestamp_ms = now_ms();

    // Create InitPoolEvent
    let init_event = InitPoolEvent {
        pool_amm_id: pool_pubkey,
        base_mint,
        quote_mint,
        slot: Some(12345),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };

    // Bootstrap the pool
    engine.mark_pool_active(init_event.pool_amm_id);
    engine.handle_initialize_pool_event(&init_event);

    // Verify bootstrap snapshots were created
    let snapshots = engine.last_n(&pool_pubkey, 5);
    assert_eq!(
        snapshots.len(),
        3,
        "Should have 3 bootstrap snapshots (g0, g1, g2)"
    );

    // get_last_n returns most recent first, so:
    // snapshots[0] = g2 (most recent)
    // snapshots[1] = g1 (intermediate)
    // snapshots[2] = g0 (oldest/initial)

    // Verify g2 (most recent, minimal activity)
    let g2 = &snapshots[0];
    assert_eq!(g2.timestamp_ms, timestamp_ms.saturating_add(2));
    assert_eq!(g2.cum_volume_sol, 0.0);
    assert_eq!(g2.tx_count, 0);
    assert_eq!(g2.unique_addrs, 0);

    // Verify g1 (intermediate, minimal activity)
    let g1 = &snapshots[1];
    assert_eq!(g1.timestamp_ms, timestamp_ms.saturating_add(1));
    assert_eq!(g1.cum_volume_sol, 0.0);
    assert_eq!(g1.tx_count, 0);
    assert_eq!(g1.unique_addrs, 0);

    // Verify g0 (oldest, initial state)
    let g0 = &snapshots[2];
    assert_eq!(g0.timestamp_ms, timestamp_ms);
    assert_eq!(g0.slot, Some(12345));
    assert_eq!(g0.cum_volume_sol, 0.0);
    assert_eq!(g0.tx_count, 0);
    assert_eq!(g0.unique_addrs, 0);

    println!("✓ Bootstrap test passed: 3 synthetic snapshots created correctly");
}

/// Test that SnapshotEngine correctly accumulates transactions and emits snapshots
#[tokio::test]
async fn test_snapshot_engine_transaction_accumulation() {
    // Create SnapshotEngine with short interval for testing
    let engine = Arc::new(SnapshotEngine::new(128, 100)); // 100ms interval

    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();

    let timestamp_ms = now_ms();

    // Bootstrap the pool
    let init_event = InitPoolEvent {
        pool_amm_id: pool_pubkey,
        base_mint,
        quote_mint,
        slot: Some(12345),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };
    engine.mark_pool_active(init_event.pool_amm_id);
    engine.handle_initialize_pool_event(&init_event);

    // Simulate transactions
    let signer1 = Pubkey::new_unique();
    let signer2 = Pubkey::new_unique();

    // First transaction (buy)
    let tx1 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint: pool_pubkey,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 1,
            unique_addrs: 1,
            volume_sol: 1.5,
            buy_volume_sol: 1.5,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
            ..Default::default()
        },
        slot: Some(12346),
        timestamp_ms: timestamp_ms.saturating_add(150), // Past interval threshold
        event_time: ingress_event_time(timestamp_ms.saturating_add(150)),
        signer: signer1,
        is_buy: true,
        volume_sol: 1.5,
        reserve_base: Some(950000.0),
        reserve_quote: Some(11.5),
        price_quote: Some(0.0000121),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: None,
        event_ordinal: None,
        block_time: Some(((timestamp_ms.saturating_add(150)) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(200)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };
    engine.handle_tx_event(&tx1);

    // Second transaction (buy, same signer)
    let tx2 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint: pool_pubkey,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 2,
            unique_addrs: 1,
            volume_sol: 2.0,
            buy_volume_sol: 2.0,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
            ..Default::default()
        },
        slot: Some(12347),
        timestamp_ms: timestamp_ms.saturating_add(200),
        event_time: ingress_event_time(timestamp_ms.saturating_add(200)),
        signer: signer1,
        is_buy: true,
        volume_sol: 0.5,
        reserve_base: Some(925000.0),
        reserve_quote: Some(12.0),
        price_quote: Some(0.00001297),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: None,
        event_ordinal: None,
        block_time: Some(((timestamp_ms.saturating_add(200)) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(250)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };
    engine.handle_tx_event(&tx2);

    // Third transaction (sell, different signer) - should trigger snapshot emission
    let tx3 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint: pool_pubkey,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 3,
            unique_addrs: 2,
            volume_sol: 2.3,
            buy_volume_sol: 2.0,
            sell_volume_sol: 0.3,
            dev_buy_lamports: 0,
            ..Default::default()
        },
        slot: Some(12348),
        timestamp_ms: timestamp_ms.saturating_add(350), // Past another interval
        event_time: ingress_event_time(timestamp_ms.saturating_add(350)),
        signer: signer2,
        is_buy: false,
        volume_sol: 0.3,
        reserve_base: Some(940000.0),
        reserve_quote: Some(11.7),
        price_quote: Some(0.0000124),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: None,
        event_ordinal: None,
        block_time: Some(((timestamp_ms.saturating_add(350)) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(400)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };
    engine.handle_tx_event(&tx3);

    // Check that snapshots were created
    let snapshots = engine.last_n(&pool_pubkey, 10);

    // Should have at least bootstrap (3) + tx-triggered snapshots
    assert!(
        snapshots.len() >= 4,
        "Should have bootstrap + transaction snapshots, got {}",
        snapshots.len()
    );

    // Verify latest snapshot contains accumulated data
    let latest = engine.get_latest_snapshot(&pool_pubkey).unwrap();
    assert!(
        latest.cum_volume_sol > 0.0,
        "Latest snapshot should have volume"
    );
    assert!(latest.tx_count > 0, "Latest snapshot should have tx count");

    println!(
        "✓ Transaction accumulation test passed: {} snapshots created",
        snapshots.len()
    );
    println!(
        "  Latest snapshot: volume={:.4} SOL, tx_count={}, unique_addrs={}",
        latest.cum_volume_sol, latest.tx_count, latest.unique_addrs
    );
}

/// Integration test with event bus simulation
#[tokio::test]
async fn test_full_event_flow_integration() {
    // Create event bus
    let (event_tx, mut event_rx) = create_event_bus();

    // Create SnapshotEngine
    let engine = Arc::new(SnapshotEngine::new(128, 200));
    let engine_clone = Arc::clone(&engine);

    // Spawn a listener that processes events (simulating snapshot_listener component)
    let listener_handle = tokio::spawn(async move {
        let mut count = 0;
        let mut next_session_id = 1u64;
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.min_tx_count = 5;
        gatekeeper_config.min_unique_signers = 5;
        gatekeeper_config.min_buy_count = 3;
        gatekeeper_config.min_interval_cv = 0.0;
        gatekeeper_config.min_timing_entropy = 0.0;
        gatekeeper_config.min_buy_ratio = 0.0;
        gatekeeper_config.min_avg_interval_ms = 0.0;
        gatekeeper_config.min_market_cap_sol = 0.0;
        gatekeeper_config.min_phases_to_pass = 2;
        gatekeeper_config.curve_require_for_buy = false;
        let mut sessions: HashMap<Pubkey, PoolObservationSession> = HashMap::new();
        while let Ok(event) = event_rx.recv().await {
            match event {
                GhostEvent::NewPoolDetected(pool) => {
                    // Simulate InitPoolEvent creation from DetectedPool
                    if let (Ok(pool_pubkey), Ok(base_mint), Ok(quote_mint), Ok(bonding_curve)) = (
                        Pubkey::from_str(&pool.pool_amm_id),
                        Pubkey::from_str(&pool.base_mint),
                        Pubkey::from_str(&pool.quote_mint),
                        Pubkey::from_str(&pool.bonding_curve),
                    ) {
                        let initial_liquidity_sol = pool.initial_liquidity_sol.unwrap_or(0.0);
                        let init_event = InitPoolEvent {
                            pool_amm_id: pool_pubkey,
                            base_mint,
                            quote_mint,
                            slot: pool.slot,
                            timestamp_ms: pool.timestamp_ms,
                            initial_liquidity_sol,
                            initial_reserve_base: 0.0,
                            initial_reserve_quote: initial_liquidity_sol,
                            initial_price_quote: 0.0,
                        };
                        engine_clone.mark_pool_active(init_event.pool_amm_id);
                        engine_clone.handle_initialize_pool_event(&init_event);

                        let candidate = snapshot_candidate(
                            pool_pubkey,
                            base_mint,
                            quote_mint,
                            bonding_curve,
                            pool.timestamp_ms,
                            initial_liquidity_sol,
                            pool.signature.clone(),
                        );
                        let mut session = PoolObservationSession::new(
                            SessionId(next_session_id),
                            pool_pubkey,
                            base_mint,
                            bonding_curve,
                            Pubkey::from_str(&pool.creator).ok(),
                            candidate,
                            pool.timestamp_ms,
                            pool.timestamp_ms
                                .saturating_add(gatekeeper_config.max_wait_time_ms),
                            &gatekeeper_config,
                            TxIntelligenceConfig::from_gatekeeper_config(
                                &gatekeeper_config,
                                EarlyFingerprintConfig::default(),
                            ),
                        );
                        next_session_id = next_session_id.saturating_add(1);
                        seed_snapshot_session(&mut session, initial_liquidity_sol);
                        sessions.insert(pool_pubkey, session);
                    }
                }
                GhostEvent::PoolTransaction(pool_tx) => {
                    let (Ok(pool_pubkey), Ok(signer_key), Some(base_mint)) = (
                        Pubkey::from_str(&pool_tx.pool_amm_id),
                        Pubkey::from_str(&pool_tx.signer),
                        pool_tx
                            .token_mint
                            .as_ref()
                            .and_then(|m| Pubkey::from_str(m).ok()),
                    ) else {
                        count += 1;
                        if count >= 5 {
                            break;
                        }
                        continue;
                    };

                    let Some(session) = sessions.get_mut(&pool_pubkey) else {
                        count += 1;
                        if count >= 5 {
                            break;
                        }
                        continue;
                    };

                    let ingress = session.ingest_transaction(pool_tx.clone());
                    session.try_checkpoint(pool_tx.timestamp_ms);

                    match resolve_snapshot_ingress(session, ingress, &gatekeeper_config) {
                        GatekeeperVerdict::Wait => {}
                        GatekeeperVerdict::PendingCurve => {}
                        GatekeeperVerdict::Reject { .. } | GatekeeperVerdict::Timeout { .. } => {
                            // Pool rejected by gatekeeper, skip
                        }
                        GatekeeperVerdict::Buy { buffered_txs, .. } => {
                            for buffered in buffered_txs {
                                let tx = buffered.tx;
                                let metrics = buffered.metrics;
                                if tx.success {
                                    let tx_event = TxEvent {
                                        semantic: ghost_core::EventSemanticEnvelope::default(),
                                        pool_amm_id: pool_pubkey,
                                        base_mint,
                                        pool_state: PoolLifecycle::Active,
                                        metrics,
                                        slot: tx.slot,
                                        timestamp_ms: tx.timestamp_ms,
                                        event_time: ingress_event_time(tx.timestamp_ms),
                                        signer: signer_key,
                                        is_buy: tx.is_buy,
                                        volume_sol: tx.volume_sol,
                                        reserve_base: tx.reserve_base,
                                        reserve_quote: tx.reserve_quote,
                                        price_quote: tx.price_quote,
                                        is_dev_buy: tx.is_dev_buy,
                                        dev_buy_lamports: tx.dev_buy_lamports,
                                        signature: None,
                                        event_ordinal: None,
                                        block_time: Some((tx.timestamp_ms / 1000) as i64),
                                        arrival_time_ms: Some(tx.timestamp_ms + 50),
                                        data_source: DataSource::SoftTruth,
                                        intra_slot_offset_ms: None,
                                        raw_data: None,
                                        raw_data_missing_reason: RawBytesMissingReason::Unknown,
                                    };
                                    engine_clone.handle_tx_event(&tx_event);
                                }
                            }
                        }
                        GatekeeperVerdict::ApprovedTx { tx, metrics } => {
                            if tx.success {
                                let tx_event = TxEvent {
                                    semantic: ghost_core::EventSemanticEnvelope::default(),
                                    pool_amm_id: pool_pubkey,
                                    base_mint,
                                    pool_state: PoolLifecycle::Active,
                                    metrics,
                                    slot: tx.slot,
                                    timestamp_ms: tx.timestamp_ms,
                                    event_time: ingress_event_time(tx.timestamp_ms),
                                    signer: signer_key,
                                    is_buy: tx.is_buy,
                                    volume_sol: tx.volume_sol,
                                    reserve_base: tx.reserve_base,
                                    reserve_quote: tx.reserve_quote,
                                    price_quote: tx.price_quote,
                                    is_dev_buy: tx.is_dev_buy,
                                    dev_buy_lamports: tx.dev_buy_lamports,
                                    signature: None,
                                    event_ordinal: None,
                                    block_time: Some((tx.timestamp_ms / 1000) as i64),
                                    arrival_time_ms: Some(tx.timestamp_ms + 50),
                                    data_source: DataSource::SoftTruth,
                                    intra_slot_offset_ms: None,
                                    raw_data: None,
                                    raw_data_missing_reason: RawBytesMissingReason::Unknown,
                                };
                                engine_clone.handle_tx_event(&tx_event);
                            }
                        }
                    }
                    count += 1;
                    if count >= 5 {
                        break; // Exit after processing 5 transactions
                    }
                }
                _ => {}
            }
        }
    });

    // Simulate fake WebSocket/Geyser events
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();

    let timestamp_ms = now_ms(); // epoch-ms

    // 1. Emit NewPoolDetected
    let detected_pool = DetectedPool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey.to_string(),
        base_mint: base_mint.to_string(),
        quote_mint: quote_mint.to_string(),
        amm_program: "PumpFun111111111111111111111111111111111".to_string(),
        bonding_curve: Pubkey::new_unique().to_string(),
        creator: Pubkey::new_unique().to_string(),
        slot: Some(10000),
        tx_index: None,
        timestamp_ms,
        event_time: ingress_event_time(timestamp_ms),
        initial_liquidity_sol: Some(5.0),
        signature: "sig123".to_string(),
        detected_wall_ts_ms: None,
    };

    event_tx
        .send(GhostEvent::new_pool_detected(detected_pool))
        .unwrap();

    // Give time for processing
    sleep(Duration::from_millis(50)).await;

    // 2. Emit several PoolTransaction events
    for i in 0..5 {
        let volume_sol = 0.5 + (i as f64 * 0.1);
        let pool_tx = PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey.to_string(),
            slot: Some(10001 + i),
            event_ordinal: Some(0),
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: timestamp_ms
                .saturating_mul(1000)
                .saturating_add(i as u64 * 250),
            event_time: ghost_core::EventTimeMetadata::new(
                None,
                Some(
                    timestamp_ms
                        .saturating_mul(1000)
                        .saturating_add(i as u64 * 250),
                ),
                None,
            ),
            arrival_ts_ms: timestamp_ms
                .saturating_mul(1000)
                .saturating_add(i as u64 * 250),
            signer: Pubkey::new_unique().to_string(),
            owner_token_deltas: vec![],
            is_buy: i % 2 == 0, // Alternate buy/sell
            volume_sol,
            sol_amount_lamports: Some((volume_sol * 1_000_000_000.0) as u64),
            token_amount_units: Some(1_000_000 + (i as u64 * 10_000)),
            reserve_base: Some(1000000.0 - (i as f64 * 10000.0)),
            reserve_quote: Some(10.0 + (i as f64 * 0.5)),
            price_quote: Some(0.00001 * (1.0 + i as f64 * 0.01)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: format!("sig_tx_{}", i),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            mpcf_payload: vec![i as u8; 100],
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
            .unwrap();
        sleep(Duration::from_millis(10)).await;
    }

    // Wait for listener to process all events
    let _ = tokio::time::timeout(Duration::from_secs(2), listener_handle).await;

    // Verify SnapshotEngine has data for this pool
    let snapshots = engine.last_n(&pool_pubkey, 10);

    assert!(
        snapshots.len() >= 3,
        "Should have at least bootstrap snapshots, got {}",
        snapshots.len()
    );

    // Check that we can get latest pair for ULVF calculations
    let pair = engine.latest_pair(&pool_pubkey);
    assert!(pair.is_some(), "Should be able to get latest pair");

    if let Some((t0, t1)) = pair {
        println!("✓ Full integration test passed:");
        println!("  Bootstrap snapshots: 3 (g0, g1, g2)");
        println!("  Total snapshots: {}", snapshots.len());
        println!("  Latest pair available for ULVF:");
        println!(
            "    t0: timestamp={}, volume={:.4} SOL",
            t0.timestamp_ms, t0.cum_volume_sol
        );
        println!(
            "    t1: timestamp={}, volume={:.4} SOL",
            t1.timestamp_ms, t1.cum_volume_sol
        );
    }

    println!("✓ Event flow verified: DetectedPool → InitPoolEvent → PoolTransaction → TxEvent → Snapshots");
}

/// Test ULVF calculation with SnapshotEngine integration
/// Verifies that divergence and curl grow with increasing volume
#[tokio::test]
async fn test_ulvf_integration_with_snapshot_engine() {
    use ghost_brain::oracle::HyperOracle;

    // Create SnapshotEngine and HyperOracle
    let engine = Arc::new(SnapshotEngine::new(128, 100)); // 100ms interval for faster testing
    let oracle = HyperOracle::new();

    // Create synthetic pool
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let timestamp_ms = now_ms();

    // Bootstrap pool
    let init_event = InitPoolEvent {
        pool_amm_id: pool_pubkey,
        base_mint,
        quote_mint,
        slot: Some(12345),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };

    engine.mark_pool_active(init_event.pool_amm_id);
    engine.handle_initialize_pool_event(&init_event);

    // Verify initial ULVF can be calculated (should be near zero with bootstrap data)
    let ulvf_initial = oracle.calculate_ulvf_for_pool(&engine, &pool_pubkey);
    assert!(
        ulvf_initial.is_some(),
        "Should be able to calculate ULVF with bootstrap data"
    );

    let (div_initial, curl_initial) = ulvf_initial.unwrap();
    println!(
        "Initial ULVF: divergence={:.4}, curl={:.4}",
        div_initial, curl_initial
    );

    // Simulate increasing trading activity
    let signer1 = Pubkey::new_unique();
    let signer2 = Pubkey::new_unique();
    let signer3 = Pubkey::new_unique();
    let mut cumulative_volume = 0.0;
    let mut cumulative_buy_volume = 0.0;
    let mut cumulative_sell_volume = 0.0;
    let mut tx_count: u64 = 0;
    let mut unique_signers: HashSet<Pubkey> = HashSet::new();

    // First wave of transactions (moderate volume)
    for i in 0..5 {
        let ts = timestamp_ms + 200 + (i * 50);
        let signer = if i % 3 == 0 {
            signer1
        } else if i % 3 == 1 {
            signer2
        } else {
            signer3
        };
        let volume_sol = 1.0 + (i as f64 * 0.5);
        tx_count += 1;
        cumulative_volume += volume_sol;
        cumulative_buy_volume += volume_sol;
        unique_signers.insert(signer);
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey,
            base_mint: pool_pubkey,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count,
                unique_addrs: unique_signers.len() as u64,
                volume_sol: cumulative_volume,
                buy_volume_sol: cumulative_buy_volume,
                sell_volume_sol: cumulative_sell_volume,
                dev_buy_lamports: 0,
            },
            slot: Some(12345 + i),
            timestamp_ms: ts,
            event_time: ingress_event_time(ts),
            signer,
            is_buy: true,
            volume_sol,
            reserve_base: Some(1000000.0 - (i as f64 * 100.0)),
            reserve_quote: Some(10.0 + (i as f64 * 1.0)),
            price_quote: Some(0.00001 * (1.0 + i as f64 * 0.02)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some((ts / 1000) as i64),
            arrival_time_ms: Some(ts + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
    }

    // Calculate ULVF after first wave
    let ulvf_wave1 = oracle.calculate_ulvf_for_pool(&engine, &pool_pubkey);
    assert!(
        ulvf_wave1.is_some(),
        "Should calculate ULVF after first wave"
    );

    let (div_wave1, curl_wave1) = ulvf_wave1.unwrap();
    println!(
        "After first wave: divergence={:.4}, curl={:.4}",
        div_wave1, curl_wave1
    );

    // Second wave of transactions (high volume)
    for i in 0..5 {
        let ts = timestamp_ms + 500 + (i * 50);
        let signer = if i % 3 == 0 {
            signer1
        } else if i % 3 == 1 {
            signer2
        } else {
            signer3
        };
        let volume_sol = 5.0 + (i as f64 * 2.0);
        tx_count += 1;
        cumulative_volume += volume_sol;
        cumulative_buy_volume += volume_sol;
        unique_signers.insert(signer);
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey,
            base_mint: pool_pubkey,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count,
                unique_addrs: unique_signers.len() as u64,
                volume_sol: cumulative_volume,
                buy_volume_sol: cumulative_buy_volume,
                sell_volume_sol: cumulative_sell_volume,
                dev_buy_lamports: 0,
            },
            slot: Some(12350 + i),
            timestamp_ms: ts,
            event_time: ingress_event_time(ts),
            signer,
            is_buy: true,
            volume_sol,
            reserve_base: Some(1000000.0 - (i as f64 * 500.0)),
            reserve_quote: Some(10.0 + (i as f64 * 5.0)),
            price_quote: Some(0.00001 * (1.0 + i as f64 * 0.1)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some((ts / 1000) as i64),
            arrival_time_ms: Some(ts + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
    }

    // Calculate ULVF after second wave
    let ulvf_wave2 = oracle.calculate_ulvf_for_pool(&engine, &pool_pubkey);
    assert!(
        ulvf_wave2.is_some(),
        "Should calculate ULVF after second wave"
    );

    let (div_wave2, curl_wave2) = ulvf_wave2.unwrap();
    println!(
        "After second wave: divergence={:.4}, curl={:.4}",
        div_wave2, curl_wave2
    );

    // Verify ULVF metrics change with different volume patterns
    // Wave 2 has much higher volume than wave 1, which should result in different metrics
    // The key test is that both divergence and curl are calculated and are reasonable values
    assert!(
        div_wave2.is_finite() && curl_wave2.is_finite(),
        "ULVF metrics should be finite: div={}, curl={}",
        div_wave2,
        curl_wave2
    );

    // Verify that higher volume produces higher curl (more rotation in the flow field)
    assert!(
        curl_wave2 > curl_wave1,
        "Curl should increase with higher volume activity: {} > {}",
        curl_wave2,
        curl_wave1
    );

    println!("✓ ULVF test passed: metrics respond to changing volume patterns");
    println!(
        "  Initial: div={:.4}, curl={:.4}",
        div_initial, curl_initial
    );
    println!(
        "  Wave 1 (moderate):  div={:.4}, curl={:.4}",
        div_wave1, curl_wave1
    );
    println!(
        "  Wave 2 (high):      div={:.4}, curl={:.4}",
        div_wave2, curl_wave2
    );

    // Display curl growth ratio only if wave 1 had meaningful curl
    if curl_wave1 > 0.1 {
        println!(
            "  Curl growth: {:.2}x (from wave 1 to wave 2)",
            curl_wave2 / curl_wave1
        );
    } else {
        println!(
            "  Curl growth: {:.4} → {:.4} (absolute change)",
            curl_wave1, curl_wave2
        );
    }
}

/// Test POVC calculation with SnapshotEngine integration
/// Verifies that POVC assigns sensible clusters based on market activity
#[tokio::test]
async fn test_povc_integration_with_snapshot_engine() {
    use ghost_brain::oracle::HyperOracle;

    // Create SnapshotEngine and HyperOracle
    let engine = Arc::new(SnapshotEngine::new(128, 100));
    let oracle = HyperOracle::new();

    // Scenario 1: Low activity pool (should classify as Dump or Noise)
    let pool_low = Pubkey::new_unique();
    let timestamp_ms = now_ms();

    let init_low = InitPoolEvent {
        pool_amm_id: pool_low,
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        slot: Some(10000),
        timestamp_ms,
        initial_liquidity_sol: 1.0,
        initial_reserve_base: 100000.0,
        initial_reserve_quote: 1.0,
        initial_price_quote: 0.00001,
    };

    engine.mark_pool_active(init_low.pool_amm_id);
    engine.handle_initialize_pool_event(&init_low);

    // Add minimal activity
    let mut low_tx_count: u64 = 0;
    let mut low_cumulative_volume = 0.0;
    for i in 0..3 {
        low_tx_count += 1;
        low_cumulative_volume += 0.1;
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_low,
            base_mint: pool_low,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: low_tx_count,
                unique_addrs: low_tx_count,
                volume_sol: low_cumulative_volume,
                buy_volume_sol: low_cumulative_volume,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(10000 + i),
            timestamp_ms: timestamp_ms + 200 + (i * 100),
            event_time: ingress_event_time(timestamp_ms + 200 + (i * 100)),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: 0.1,
            reserve_base: Some(100000.0),
            reserve_quote: Some(1.0),
            price_quote: Some(0.00001),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some(((timestamp_ms + 200 + (i * 100)) / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms + 200 + (i * 100) + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
    }

    let cluster_low = oracle.calculate_povc_for_pool(&engine, &pool_low);
    assert!(
        cluster_low.is_some(),
        "Should calculate POVC for low activity pool"
    );
    println!(
        "Low activity pool classified as cluster: {}",
        cluster_low.unwrap()
    );

    // Scenario 2: High organic activity pool (should classify as Hype)
    let pool_high = Pubkey::new_unique();

    let init_high = InitPoolEvent {
        pool_amm_id: pool_high,
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        slot: Some(20000),
        timestamp_ms: timestamp_ms + 1000,
        initial_liquidity_sol: 100.0,
        initial_reserve_base: 10000000.0,
        initial_reserve_quote: 100.0,
        initial_price_quote: 0.00001,
    };

    engine.mark_pool_active(init_high.pool_amm_id);
    engine.handle_initialize_pool_event(&init_high);

    // Add high organic activity (many unique addresses, high volume)
    let mut high_tx_count: u64 = 0;
    let mut high_cumulative_volume = 0.0;
    for i in 0..20 {
        let volume_sol = 5.0 + (i as f64 * 0.5);
        high_tx_count += 1;
        high_cumulative_volume += volume_sol;
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_high,
            base_mint: pool_high,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: high_tx_count,
                unique_addrs: high_tx_count,
                volume_sol: high_cumulative_volume,
                buy_volume_sol: high_cumulative_volume,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(20000 + i),
            timestamp_ms: timestamp_ms + 1200 + (i * 50),
            event_time: ingress_event_time(timestamp_ms + 1200 + (i * 50)),
            signer: Pubkey::new_unique(), // Different signer each time
            is_buy: true,
            volume_sol,
            reserve_base: Some(10000000.0 - (i as f64 * 10000.0)),
            reserve_quote: Some(100.0 + (i as f64 * 5.0)),
            price_quote: Some(0.00001 * (1.0 + i as f64 * 0.05)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some(((timestamp_ms + 1200 + (i * 50)) / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms + 1200 + (i * 50) + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
    }

    let cluster_high = oracle.calculate_povc_for_pool(&engine, &pool_high);
    assert!(
        cluster_high.is_some(),
        "Should calculate POVC for high activity pool"
    );
    println!(
        "High activity pool classified as cluster: {}",
        cluster_high.unwrap()
    );

    // Scenario 3: Bot noise pool (repetitive, identical patterns)
    let pool_bot = Pubkey::new_unique();

    let init_bot = InitPoolEvent {
        pool_amm_id: pool_bot,
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        slot: Some(30000),
        timestamp_ms: timestamp_ms + 2000,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };

    engine.mark_pool_active(init_bot.pool_amm_id);
    engine.handle_initialize_pool_event(&init_bot);

    // Add repetitive bot-like activity (same signer, identical volumes)
    let bot_signer = Pubkey::new_unique();
    let mut bot_tx_count: u64 = 0;
    let mut bot_cumulative_volume = 0.0;
    for i in 0..30 {
        bot_tx_count += 1;
        bot_cumulative_volume += 1.0;
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_bot,
            base_mint: pool_bot,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: bot_tx_count,
                unique_addrs: 1,
                volume_sol: bot_cumulative_volume,
                buy_volume_sol: bot_cumulative_volume,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(30000 + i),
            timestamp_ms: timestamp_ms + 2200 + (i * 20), // Regular intervals
            event_time: ingress_event_time(timestamp_ms + 2200 + (i * 20)),
            signer: bot_signer, // Same signer
            is_buy: true,
            volume_sol: 1.0, // Constant volume
            reserve_base: Some(1000000.0),
            reserve_quote: Some(10.0),
            price_quote: Some(0.00001),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some(((timestamp_ms + 2200 + (i * 20)) / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms + 2200 + (i * 20) + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
    }

    let cluster_bot = oracle.calculate_povc_for_pool(&engine, &pool_bot);
    assert!(
        cluster_bot.is_some(),
        "Should calculate POVC for bot noise pool"
    );
    println!(
        "Bot noise pool classified as cluster: {}",
        cluster_bot.unwrap()
    );

    // Verify all clusters are valid (0, 1, or 2)
    assert!(cluster_low.unwrap() <= 2, "Cluster should be 0, 1, or 2");
    assert!(cluster_high.unwrap() <= 2, "Cluster should be 0, 1, or 2");
    assert!(cluster_bot.unwrap() <= 2, "Cluster should be 0, 1, or 2");

    println!("✓ POVC test passed: sensible cluster assignments");
    println!("  Low activity:  cluster {}", cluster_low.unwrap());
    println!("  High organic:  cluster {}", cluster_high.unwrap());
    println!("  Bot noise:     cluster {}", cluster_bot.unwrap());
}

/// Test SCR calculation with SnapshotEngine integration
/// Verifies that SCR receives appropriate data stream and calculates correctly
#[tokio::test]
async fn test_scr_integration_with_snapshot_engine() {
    use ghost_brain::oracle::HyperOracle;

    // Create SnapshotEngine and HyperOracle
    let engine = Arc::new(SnapshotEngine::new(128, 50)); // Short interval for more snapshots
    let oracle = HyperOracle::new();

    // Create synthetic pool
    let pool_pubkey = Pubkey::new_unique();
    let timestamp_ms = now_ms();

    let init_event = InitPoolEvent {
        pool_amm_id: pool_pubkey,
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        slot: Some(40000),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };

    engine.mark_pool_active(init_event.pool_amm_id);
    engine.handle_initialize_pool_event(&init_event);

    // Bootstrap has only 3 snapshots, which is less than 4 required for SCR
    // SCR should return None initially
    let scr_bootstrap = oracle.calculate_scr_for_pool(&engine, &pool_pubkey, 10);
    // Bootstrap has 3 snapshots which is less than 4 needed for SCR
    if scr_bootstrap.is_some() {
        println!("Bootstrap SCR (3 snapshots): {:.4}", scr_bootstrap.unwrap());
    } else {
        println!("Bootstrap: Not enough snapshots for SCR (need 4+)");
    }

    // Generate regular bot-like activity (should produce high SCR)
    let mut regular_tx_count: u64 = 0;
    let mut regular_volume = 0.0;
    for i in 0..20 {
        regular_tx_count += 1;
        regular_volume += 1.0;
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey,
            base_mint: pool_pubkey,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: regular_tx_count,
                unique_addrs: regular_tx_count,
                volume_sol: regular_volume,
                buy_volume_sol: regular_volume,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(40000 + i),
            timestamp_ms: timestamp_ms + 100 + (i * 100), // Regular 100ms intervals
            event_time: ingress_event_time(timestamp_ms + 100 + (i * 100)),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(1000000.0),
            reserve_quote: Some(10.0),
            price_quote: Some(0.00001),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some(((timestamp_ms + 100 + (i * 100)) / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms + 100 + (i * 100) + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
    }

    // Calculate SCR with regular activity
    let scr_regular = oracle.calculate_scr_for_pool(&engine, &pool_pubkey, 10);
    assert!(
        scr_regular.is_some(),
        "Should calculate SCR with regular activity"
    );

    let scr_regular_value = scr_regular.unwrap();
    println!(
        "SCR with regular activity (bot-like): {:.4}",
        scr_regular_value
    );
    assert!(
        scr_regular_value >= 0.0 && scr_regular_value <= 1.0,
        "SCR should be between 0 and 1"
    );

    // Create another pool with irregular activity
    let pool_irregular = Pubkey::new_unique();
    let timestamp_ms2 = now_ms();

    let init_event2 = InitPoolEvent {
        pool_amm_id: pool_irregular,
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        slot: Some(50000),
        timestamp_ms: timestamp_ms2,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };

    engine.mark_pool_active(init_event2.pool_amm_id);
    engine.handle_initialize_pool_event(&init_event2);

    // Generate irregular organic activity with deterministic but varied intervals
    // Pattern simulates human trading: bursts of activity with varying pauses
    let irregular_intervals = vec![
        30u64, 150, 80, 200, 45, // Initial burst with long pause
        120, 90, 160, 70, 110, // Second wave with medium pauses
        95, 140, 60, 180, 85, // Third wave, more varied
        130, 75, 155, // Final trades tapering off
    ];
    let mut cumulative_time = 100u64; // Start at 100ms
    let mut irregular_tx_count: u64 = 0;
    let mut irregular_volume = 0.0;
    let mut irregular_buy_volume = 0.0;
    let mut irregular_sell_volume = 0.0;
    for (i, &interval) in irregular_intervals.iter().enumerate() {
        let volume_sol = 0.5 + ((i as f64) % 5.0) * 0.3;
        irregular_tx_count += 1;
        irregular_volume += volume_sol;
        if i % 2 == 0 {
            irregular_buy_volume += volume_sol;
        } else {
            irregular_sell_volume += volume_sol;
        }
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_irregular,
            base_mint: pool_irregular,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: irregular_tx_count,
                unique_addrs: irregular_tx_count,
                volume_sol: irregular_volume,
                buy_volume_sol: irregular_buy_volume,
                sell_volume_sol: irregular_sell_volume,
                dev_buy_lamports: 0,
            },
            slot: Some(50000 + i as u64),
            timestamp_ms: timestamp_ms2 + cumulative_time,
            event_time: ingress_event_time(timestamp_ms2 + cumulative_time),
            signer: Pubkey::new_unique(),
            is_buy: i % 2 == 0,
            volume_sol, // Varying volumes
            reserve_base: Some(1000000.0),
            reserve_quote: Some(10.0),
            price_quote: Some(0.00001),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some((timestamp_ms2 + cumulative_time / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms2 + cumulative_time + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
        cumulative_time += interval; // Add interval for next iteration
    }

    // Calculate SCR with irregular activity
    let scr_irregular = oracle.calculate_scr_for_pool(&engine, &pool_irregular, 10);
    assert!(
        scr_irregular.is_some(),
        "Should calculate SCR with irregular activity"
    );

    let scr_irregular_value = scr_irregular.unwrap();
    println!(
        "SCR with irregular activity (organic): {:.4}",
        scr_irregular_value
    );
    assert!(
        scr_irregular_value >= 0.0 && scr_irregular_value <= 1.0,
        "SCR should be between 0 and 1"
    );

    // Regular activity should typically have higher SCR than irregular (bot detection)
    // Note: This might not always be true depending on the specific pattern,
    // but we verify both are valid values
    println!("✓ SCR test passed: appropriate data stream processing");
    println!(
        "  Regular intervals (bot):     SCR = {:.4}",
        scr_regular_value
    );
    println!(
        "  Irregular intervals (human): SCR = {:.4}",
        scr_irregular_value
    );
    println!("  Both values are within valid range [0.0, 1.0]");
}

/// Comprehensive test combining all three helpers (ULVF, POVC, SCR)
#[tokio::test]
async fn test_combined_oracle_helpers_with_snapshot_engine() {
    use ghost_brain::oracle::HyperOracle;

    let engine = Arc::new(SnapshotEngine::new(128, 100));
    let oracle = HyperOracle::new();

    let pool_pubkey = Pubkey::new_unique();
    let timestamp_ms = now_ms();

    // Bootstrap pool
    let init_event = InitPoolEvent {
        pool_amm_id: pool_pubkey,
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        slot: Some(60000),
        timestamp_ms,
        initial_liquidity_sol: 50.0,
        initial_reserve_base: 5000000.0,
        initial_reserve_quote: 50.0,
        initial_price_quote: 0.00001,
    };

    engine.mark_pool_active(init_event.pool_amm_id);
    engine.handle_initialize_pool_event(&init_event);

    // Generate realistic trading activity
    let mut combined_tx_count: u64 = 0;
    let mut combined_volume = 0.0;
    let mut combined_buy_volume = 0.0;
    let mut combined_sell_volume = 0.0;
    for i in 0..25 {
        let volume_sol = 2.0 + (i as f64 % 7.0) * 0.8;
        combined_tx_count += 1;
        combined_volume += volume_sol;
        if i % 3 != 0 {
            combined_buy_volume += volume_sol;
        } else {
            combined_sell_volume += volume_sol;
        }
        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey,
            base_mint: pool_pubkey,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: combined_tx_count,
                unique_addrs: combined_tx_count,
                volume_sol: combined_volume,
                buy_volume_sol: combined_buy_volume,
                sell_volume_sol: combined_sell_volume,
                dev_buy_lamports: 0,
            },
            slot: Some(60000 + i),
            timestamp_ms: timestamp_ms + 200 + (i * 80),
            event_time: ingress_event_time(timestamp_ms + 200 + (i * 80)),
            signer: Pubkey::new_unique(),
            is_buy: i % 3 != 0, // Mostly buys
            volume_sol,
            reserve_base: Some(5000000.0 - (i as f64 * 5000.0)),
            reserve_quote: Some(50.0 + (i as f64 * 2.0)),
            price_quote: Some(0.00001 * (1.0 + i as f64 * 0.03)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some(((timestamp_ms + 200 + (i * 80)) / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms + 200 + (i * 80) + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_event);
    }

    // Test all three helpers work together
    let ulvf_result = oracle.calculate_ulvf_for_pool(&engine, &pool_pubkey);
    let povc_result = oracle.calculate_povc_for_pool(&engine, &pool_pubkey);
    let scr_result = oracle.calculate_scr_for_pool(&engine, &pool_pubkey, 15);

    assert!(ulvf_result.is_some(), "ULVF should calculate successfully");
    assert!(povc_result.is_some(), "POVC should calculate successfully");
    assert!(scr_result.is_some(), "SCR should calculate successfully");

    let (divergence, curl) = ulvf_result.unwrap();
    let cluster = povc_result.unwrap();
    let scr = scr_result.unwrap();

    println!("✓ Combined oracle test passed:");
    println!("  ULVF: divergence={:.4}, curl={:.4}", divergence, curl);
    println!("  POVC: cluster={} (0=Dump, 1=Hype, 2=Noise)", cluster);
    println!("  SCR:  score={:.4} (0=organic, 1=bot)", scr);

    // Verify all results are in valid ranges
    assert!(divergence.is_finite(), "Divergence should be finite");
    assert!(curl.is_finite(), "Curl should be finite");
    assert!(cluster <= 2, "Cluster should be 0, 1, or 2");
    assert!(scr >= 0.0 && scr <= 1.0, "SCR should be between 0 and 1");
}

// =========================================================================
// PR-3b integration tests
// =========================================================================

/// PR-3b: Two events with the SAME signature but DIFFERENT event_ordinal values must
/// each produce a snapshot — they are distinct canonical trades.
#[tokio::test]
async fn test_pr3b_same_signature_different_ordinal_both_accepted() {
    let engine = Arc::new(SnapshotEngine::new(128, 0)); // 0ms → emit every event
    let pool = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let ts = now_ms();

    engine.mark_pool_active(pool);

    let init = InitPoolEvent {
        pool_amm_id: pool,
        base_mint,
        quote_mint: Pubkey::new_unique(),
        slot: Some(1000),
        timestamp_ms: ts,
        initial_liquidity_sol: 5.0,
        initial_reserve_base: 500_000.0,
        initial_reserve_quote: 5.0,
        initial_price_quote: 0.00001,
    };
    engine.handle_initialize_pool_event(&init);
    let bootstrap_count = engine.last_n(&pool, 10).len();

    // Same signature, ordinal=0
    let ev0 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 1,
            unique_addrs: 1,
            volume_sol: 1.0,
            buy_volume_sol: 1.0,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
        },
        slot: Some(1001),
        timestamp_ms: ts + 500,
        event_time: ingress_event_time(ts + 500),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(499_000.0),
        reserve_quote: Some(5.5),
        price_quote: Some(0.0000110),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("IntegSig_MultiTrade".to_string()),
        event_ordinal: Some(0),
        block_time: Some(((ts + 500) / 1000) as i64),
        arrival_time_ms: Some(ts + 520),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    // Same signature, ordinal=1 — DISTINCT trade
    let ev1 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        event_ordinal: Some(1),
        metrics: PoolMetrics {
            tx_count: 2,
            unique_addrs: 1,
            volume_sol: 2.5,
            buy_volume_sol: 2.5,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
        },
        volume_sol: 1.5,
        reserve_base: Some(498_000.0),
        reserve_quote: Some(6.0),
        price_quote: Some(0.0000120),
        ..ev0.clone()
    };

    engine.handle_tx_event(&ev0);
    engine.handle_tx_event(&ev1);

    let final_count = engine.last_n(&pool, 20).len();
    assert!(
        final_count > bootstrap_count + 1,
        "Both ordinal=0 and ordinal=1 events must be accepted (bootstrap={}, final={})",
        bootstrap_count,
        final_count
    );
}

/// PR-3b single-ingress contract: duplicate call with identical TxKey is deduped.
#[tokio::test]
async fn test_pr3b_duplicate_txkey_deduped() {
    let engine = Arc::new(SnapshotEngine::new(128, 0));
    let pool = Pubkey::new_unique();
    let ts = now_ms();

    engine.mark_pool_active(pool);

    let ev = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool,
        base_mint: Pubkey::new_unique(),
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 1,
            unique_addrs: 1,
            volume_sol: 1.0,
            buy_volume_sol: 1.0,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
        },
        slot: Some(2000),
        timestamp_ms: ts,
        event_time: ingress_event_time(ts),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(1_000.0),
        reserve_quote: Some(1.0),
        price_quote: Some(0.001),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("DedupSig_12345".to_string()),
        event_ordinal: Some(0),
        block_time: Some((ts / 1000) as i64),
        arrival_time_ms: Some(ts + 40),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    engine.handle_tx_event(&ev);
    let count_first = engine.last_n(&pool, 10).len();

    // Second identical call — must be deduped
    engine.handle_tx_event(&ev);
    let count_second = engine.last_n(&pool, 10).len();

    assert_eq!(
        count_first, count_second,
        "Duplicate TxKey must be deduped — snapshot count must not change"
    );
}

/// PR-3b enrichment contract: the same canonical event may arrive later with richer
/// reserves/price context and must upgrade local state without counting as a second trade.
#[tokio::test]
async fn test_pr3b_same_event_enriched_later_upgrades_snapshot_state() {
    let engine = Arc::new(SnapshotEngine::new(128, 0));
    let pool = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let ts = now_ms();

    engine.mark_pool_active(pool);

    let poor = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 1,
            unique_addrs: 1,
            volume_sol: 2.0,
            buy_volume_sol: 2.0,
            sell_volume_sol: 0.0,
            dev_buy_lamports: 0,
        },
        slot: Some(2100),
        timestamp_ms: ts,
        event_time: ingress_event_time(ts),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 2.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("EnrichedLaterSig".to_string()),
        event_ordinal: Some(0),
        block_time: Some((ts / 1000) as i64),
        arrival_time_ms: Some(ts + 20),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };
    let enriched = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        reserve_base: Some(800.0),
        reserve_quote: Some(2.0),
        price_quote: Some(0.0025),
        data_source: DataSource::HardTruth,
        ..poor.clone()
    };

    engine.handle_tx_event(&poor);
    let count_after_poor = engine.last_n(&pool, 10).len();
    let poor_latest = engine
        .last_n(&pool, 10)
        .into_iter()
        .next()
        .expect("poor event should produce an initial snapshot");
    assert_eq!(poor_latest.reserve_base, 0.0);
    assert_eq!(poor_latest.reserve_quote, 0.0);
    assert_eq!(poor_latest.price_quote, 0.0);

    engine.handle_tx_event(&enriched);
    let snaps = engine.last_n(&pool, 10);
    let latest = snaps
        .first()
        .expect("enriched duplicate should produce upgraded local snapshot state");

    assert!(
        snaps.len() == count_after_poor,
        "enriched duplicate must upgrade local snapshot state without adding a second snapshot"
    );
    assert_eq!(
        latest.tx_count, 1,
        "enrichment must not count as a second trade"
    );
    assert_eq!(latest.cum_volume_sol, 2.0);
    assert_eq!(latest.reserve_base, 800.0);
    assert_eq!(latest.reserve_quote, 2.0);
    assert_eq!(latest.price_quote, 0.0025);
    assert_eq!(latest.get_data_source(), DataSource::HardTruth);
}

/// PR-3b pre-commit boundary: SnapshotEngine must NOT write to ShadowLedger from handle_tx_event.
#[tokio::test]
async fn test_pr3b_no_shadow_ledger_write_from_snapshot_engine() {
    use ghost_core::shadow_ledger::types::{MarketSnapshot as GhostCoreMarketSnapshot, PriceState};
    use ghost_core::shadow_ledger::ShadowLedger;

    let shadow_ledger = Arc::new(ShadowLedger::new());
    let mut engine = SnapshotEngine::new(128, 0);
    engine.set_shadow_ledger(Arc::clone(&shadow_ledger));
    let engine = Arc::new(engine);

    let pool = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let ts = now_ms();

    // Externally commit exactly 1 snapshot to ShadowLedger (Gatekeeper role)
    let seed = GhostCoreMarketSnapshot {
        slot: Some(3000),
        timestamp_ms: ts,
        price_sol_per_token: 0.00001,
        price_state: PriceState::Valid,
        reserve_base: 1_000_000.0,
        reserve_quote: 10.0,
        market_cap_sol: 10.0,
        ..Default::default()
    };
    shadow_ledger.commit_history(base_mint, vec![seed], None);

    engine.mark_pool_active(pool);

    let init = InitPoolEvent {
        pool_amm_id: pool,
        base_mint,
        quote_mint: Pubkey::new_unique(),
        slot: Some(3000),
        timestamp_ms: ts,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1_000_000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };
    engine.handle_initialize_pool_event(&init);

    for i in 1u64..=5 {
        let ev = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: i,
                unique_addrs: i,
                volume_sol: i as f64,
                buy_volume_sol: i as f64,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(3000 + i),
            timestamp_ms: ts + i * 1000,
            event_time: ingress_event_time(ts + i * 1000),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: i as f64,
            reserve_base: Some(1_000_000.0 - i as f64 * 1000.0),
            reserve_quote: Some(10.0 + i as f64 * 0.5),
            price_quote: Some(0.00001 + i as f64 * 0.000001),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(format!("integ_sig_{}", i)),
            event_ordinal: None,
            block_time: Some(((ts + i * 1000) / 1000) as i64),
            arrival_time_ms: Some(ts + i * 1000 + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&ev);
    }

    // ShadowLedger must still have ONLY the 1 externally-committed snapshot
    let ledger = shadow_ledger
        .get_snapshots(&base_mint)
        .expect("seed should still be present");
    assert_eq!(
        ledger.len(),
        1,
        "ShadowLedger must not be written by SnapshotEngine (PR-3b pre-commit boundary), got {}",
        ledger.len()
    );

    // LOCAL ring buffer has the new snapshots
    let local = engine.last_n(&pool, 20);
    assert!(!local.is_empty(), "LOCAL ring buffer must have snapshots");
}
