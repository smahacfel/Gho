//! Comprehensive tests for SnapshotEngine diagnostics and monitoring
//!
//! This test file validates:
//! 1. Ring-buffer diagnostics and logging
//! 2. Metrics integration and accuracy
//! 3. Stagnation detection
//! 4. Behavior under various scenarios (tx inflow, long absence, overflow)

use ghost_brain::oracle::{
    snapshot_engine::{DataSource, PoolLifecycle, PoolMetrics},
    InitPoolEvent, SnapshotEngine, SnapshotMetrics, TxEvent,
};
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

// Test constants
const TEST_STAGNATION_THRESHOLD_MS: u64 = 100;

/// Helper to get current timestamp in milliseconds
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Helper to create a test pubkey
fn test_pubkey(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

#[tokio::test]
async fn test_snapshot_metrics_integration() {
    // Create metrics and engine
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(128, 200, Some(metrics.clone()), None);

    // Verify initial metrics state
    assert_eq!(metrics.snapshot_len.get(), 0);
    assert_eq!(metrics.snapshots_pushed_total.get(), 0);
    assert_eq!(metrics.pools_initialized_total.get(), 0);

    // Initialize a pool
    let pool_key = test_pubkey(1);
    engine.mark_pool_active(pool_key);
    let init_event = InitPoolEvent {
        pool_amm_id: pool_key,
        base_mint: test_pubkey(2),
        quote_mint: test_pubkey(3),
        slot: Some(1000),
        timestamp_ms: now_ms(),
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };

    engine.handle_initialize_pool_event(&init_event);

    // Verify metrics were updated
    assert_eq!(metrics.pools_initialized_total.get(), 1);
    assert_eq!(metrics.snapshots_pushed_total.get(), 3); // g0, g1, g2
    assert_eq!(metrics.snapshot_len.get(), 3);
    assert_eq!(metrics.ring_buffer_wraps_total.get(), 0);

    println!("✓ Metrics integration test passed: pool initialization recorded");
}

#[tokio::test]
async fn test_tx_event_metrics_tracking() {
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(128, 100, Some(metrics.clone()), None);

    let pool_key = test_pubkey(1);
    let base_ts = now_ms();
    engine.mark_pool_active(pool_key);

    // Send transaction events
    for i in 0..5 {
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100 + i),
            timestamp_ms: base_ts + (i * 150), // Exceeds 100ms interval
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10 + i as u8),
            is_buy: i % 2 == 0,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx);
    }

    // Verify tx event tracking
    assert_eq!(metrics.tx_events_processed_total.get(), 5);

    // Should have emitted 4 snapshots (first event sets baseline, next 4 emit)
    assert!(metrics.snapshots_pushed_total.get() >= 3); // At least a few snapshots

    println!(
        "✓ TX event metrics test passed: {} tx events tracked",
        metrics.tx_events_processed_total.get()
    );
}

#[tokio::test]
async fn test_ring_buffer_wrap_detection() {
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(4, 10, Some(metrics.clone()), None); // Small capacity

    let pool_key = test_pubkey(1);
    let base_ts = now_ms();
    engine.mark_pool_active(pool_key);

    // Fill buffer beyond capacity to trigger wraps
    for i in 0..10 {
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100 + i),
            timestamp_ms: base_ts + (i * 20), // Exceeds 10ms interval
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10 + i as u8),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx);
    }

    // Buffer size should be capped at capacity
    assert_eq!(metrics.snapshot_len.get(), 4);

    // Should have detected wraps
    assert!(metrics.ring_buffer_wraps_total.get() > 0);

    println!(
        "✓ Ring buffer wrap test passed: {} wraps detected",
        metrics.ring_buffer_wraps_total.get()
    );
}

#[tokio::test]
async fn test_stagnation_detection_empty_buffer() {
    let metrics = Arc::new(SnapshotMetrics::new(None));
    // Use test constant for stagnation threshold
    let engine = SnapshotEngine::with_metrics(
        128,
        200,
        Some(metrics.clone()),
        Some(TEST_STAGNATION_THRESHOLD_MS),
    );

    let pool_key = test_pubkey(1);

    // Create pool state without initializing it (so buffer stays empty)
    let _ = engine.get_or_create_pool_state(pool_key);

    // Wait longer than stagnation threshold
    sleep(Duration::from_millis(TEST_STAGNATION_THRESHOLD_MS + 50)).await;

    // Check for stagnation
    let stagnant_count = engine.check_stagnation();

    // Should detect stagnation
    assert!(stagnant_count > 0);
    assert!(metrics.stagnation_detected_total.get() > 0);

    println!(
        "✓ Stagnation detection test passed: detected {} stagnant pools",
        stagnant_count
    );
}

#[tokio::test]
async fn test_no_stagnation_with_recent_activity() {
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(
        128,
        200,
        Some(metrics.clone()),
        Some(TEST_STAGNATION_THRESHOLD_MS),
    );

    let pool_key = test_pubkey(1);

    // Initialize pool (this creates activity)
    let init_event = InitPoolEvent {
        pool_amm_id: pool_key,
        base_mint: test_pubkey(2),
        quote_mint: test_pubkey(3),
        slot: Some(1000),
        timestamp_ms: now_ms(),
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };
    engine.mark_pool_active(pool_key);
    engine.handle_initialize_pool_event(&init_event);

    // Wait a bit but not past threshold
    sleep(Duration::from_millis(50)).await;

    // Check for stagnation
    let stagnant_count = engine.check_stagnation();

    // Should NOT detect stagnation (buffer has snapshots)
    assert_eq!(stagnant_count, 0);
    assert_eq!(metrics.stagnation_detected_total.get(), 0);

    println!("✓ No false stagnation test passed");
}

#[tokio::test]
async fn test_normal_operation_with_tx_inflow() {
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(128, 50, Some(metrics.clone()), None);

    let pool_key = test_pubkey(1);
    let base_ts = now_ms();
    engine.mark_pool_active(pool_key);

    // Simulate continuous transaction flow
    for i in 0..20 {
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100 + i),
            timestamp_ms: base_ts + (i * 60), // Regular intervals
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey((10 + i % 5) as u8),
            is_buy: i % 3 != 0,
            volume_sol: 5.0 + (i as f64 * 0.5),
            reserve_base: Some(1000.0 + (i as f64 * 10.0)),
            reserve_quote: Some(100.0 - (i as f64 * 0.5)),
            price_quote: Some(0.1 + (i as f64 * 0.001)),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx);
    }

    // Verify normal operation
    let snapshots = engine.last_n(&pool_key, 10);
    assert!(snapshots.len() > 5, "Should have multiple snapshots");

    // Verify metrics show activity
    assert_eq!(metrics.tx_events_processed_total.get(), 20);
    assert!(metrics.snapshots_pushed_total.get() > 10);

    // No stagnation with continuous activity
    let stagnant_count = engine.check_stagnation();
    assert_eq!(stagnant_count, 0);

    println!(
        "✓ Normal operation test passed: {} snapshots created from {} tx events",
        snapshots.len(),
        metrics.tx_events_processed_total.get()
    );
}

#[tokio::test]
async fn test_long_absence_then_recovery() {
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(
        128,
        50,
        Some(metrics.clone()),
        Some(TEST_STAGNATION_THRESHOLD_MS),
    );

    let pool_key = test_pubkey(1);
    engine.mark_pool_active(pool_key);

    // Create pool with no snapshots (empty buffer)
    let _ = engine.get_or_create_pool_state(pool_key);

    // Long absence
    sleep(Duration::from_millis(TEST_STAGNATION_THRESHOLD_MS + 50)).await;

    // Check stagnation
    let stagnant_before = engine.check_stagnation();
    assert!(
        stagnant_before > 0,
        "Should detect stagnation during absence"
    );

    // Recovery: send transaction
    let tx = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_key,
        base_mint: pool_key,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(1000),
        timestamp_ms: now_ms(),
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: test_pubkey(10),
        is_buy: true,
        volume_sol: 10.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: None,
        event_ordinal: None,
        block_time: None,
        arrival_time_ms: None,
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };
    engine.handle_tx_event(&tx);

    // Send another to trigger snapshot
    sleep(Duration::from_millis(60)).await;
    let tx2 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_key,
        base_mint: pool_key,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(1001),
        timestamp_ms: now_ms(),
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: test_pubkey(11),
        is_buy: false,
        volume_sol: 5.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: None,
        event_ordinal: None,
        block_time: None,
        arrival_time_ms: None,
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };
    engine.handle_tx_event(&tx2);

    // Buffer should now have activity
    let snapshots = engine.last_n(&pool_key, 5);
    assert!(snapshots.len() > 0, "Should have snapshots after recovery");

    println!("✓ Long absence and recovery test passed");
}

#[tokio::test]
async fn test_multiple_pools_independent_tracking() {
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(
        128,
        50,
        Some(metrics.clone()),
        Some(TEST_STAGNATION_THRESHOLD_MS),
    );

    let pool1 = test_pubkey(1);
    let pool2 = test_pubkey(2);
    let pool3 = test_pubkey(3);
    let base_ts = now_ms();
    engine.mark_pool_active(pool1);
    engine.mark_pool_active(pool3);

    // Pool 1: Active
    for i in 0..5 {
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool1,
            base_mint: pool1,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100 + i),
            timestamp_ms: base_ts + (i * 60),
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx);
    }

    // Pool 2: Initialized but inactive
    let _ = engine.get_or_create_pool_state(pool2);

    // Pool 3: Active
    for i in 0..3 {
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool3,
            base_mint: pool3,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(200 + i),
            timestamp_ms: base_ts + (i * 60),
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(20),
            is_buy: false,
            volume_sol: 10.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx);
    }

    // Wait for stagnation threshold
    sleep(Duration::from_millis(TEST_STAGNATION_THRESHOLD_MS + 50)).await;

    // Check stagnation - should detect pool2
    let stagnant_count = engine.check_stagnation();
    assert_eq!(stagnant_count, 1, "Only pool2 should be stagnant");

    // Verify pool1 and pool3 have snapshots
    assert!(engine.last_n(&pool1, 5).len() > 0);
    assert!(engine.last_n(&pool3, 5).len() > 0);
    assert_eq!(engine.last_n(&pool2, 5).len(), 0);

    println!("✓ Multiple pools independent tracking test passed");
}

#[tokio::test]
async fn test_downstream_drop_scenario() {
    // Simulate scenario where snapshots are created but never consumed
    let metrics = Arc::new(SnapshotMetrics::new(None));
    let engine = SnapshotEngine::with_metrics(16, 50, Some(metrics.clone()), None);

    let pool_key = test_pubkey(1);
    let base_ts = now_ms();
    engine.mark_pool_active(pool_key);

    // Generate many transactions to overflow buffer
    for i in 0..30 {
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100 + i),
            timestamp_ms: base_ts + (i * 60),
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey((10 + i % 5) as u8),
            is_buy: i % 2 == 0,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };
        engine.handle_tx_event(&tx);
    }

    // Buffer should be full and have wrapped
    let snapshots = engine.last_n(&pool_key, 20);
    assert_eq!(snapshots.len(), 16, "Buffer should be at capacity");
    assert!(
        metrics.ring_buffer_wraps_total.get() > 0,
        "Should have wrapped"
    );

    // Most recent snapshot should be latest
    let latest = engine.get_latest_snapshot(&pool_key).unwrap();
    assert_eq!(latest.timestamp_ms, snapshots[0].timestamp_ms);

    println!(
        "✓ Downstream drop scenario test passed: buffer wrapped {} times",
        metrics.ring_buffer_wraps_total.get()
    );
}
