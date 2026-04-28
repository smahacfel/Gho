//! Comprehensive tests for SnapshotEngine data reliability features
//!
//! This test suite validates:
//! - Soft truth vs hard truth distinction
//! - Periodic resynchronization (every 10 slots)
//! - Volume sanity checks
//! - Duplicate detection
//! - Reorg handling
//! - Jitter detection and correction
//! - Anomaly injection and handling

use ghost_brain::oracle::snapshot_engine::{DataSource, PoolLifecycle, PoolMetrics};
use ghost_brain::oracle::{
    InitPoolEvent, IntegrityViolation, ResyncConfig, SnapshotEngine, TxEvent,
};
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Helper to get current timestamp in milliseconds
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn metrics_from_values(
    tx_count: u64,
    unique_addrs: u64,
    volume_sol: f64,
    is_buy: bool,
    dev_buy_lamports: u64,
) -> PoolMetrics {
    PoolMetrics {
        tx_count,
        unique_addrs,
        volume_sol,
        buy_volume_sol: if is_buy { volume_sol } else { 0.0 },
        sell_volume_sol: if is_buy { 0.0 } else { volume_sol },
        dev_buy_lamports,
        ..Default::default()
    }
}

/// Test helper to create a pool and bootstrap it
fn create_test_pool(engine: &Arc<SnapshotEngine>, pool_pubkey: Pubkey) -> Pubkey {
    let timestamp_ms = now_ms();
    let base_mint = Pubkey::new_unique();
    engine.mark_pool_active(pool_pubkey);
    let init_event = InitPoolEvent {
        pool_amm_id: pool_pubkey,
        base_mint,
        quote_mint: Pubkey::new_unique(),
        slot: Some(1000),
        timestamp_ms,
        initial_liquidity_sol: 10.0,
        initial_reserve_base: 1000000.0,
        initial_reserve_quote: 10.0,
        initial_price_quote: 0.00001,
    };
    engine.handle_initialize_pool_event(&init_event);
    base_mint
}

#[test]
fn test_data_source_distinction() {
    let engine = Arc::new(SnapshotEngine::new(128, 200));
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let timestamp_ms = now_ms();

    // Create soft truth event
    let soft_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(1, 1, 1.0, true, 0),
        slot: Some(1001),
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(999000.0),
        reserve_quote: Some(11.0),
        price_quote: Some(0.000011),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("soft_truth_sig".to_string()),
        event_ordinal: None,
        block_time: Some((timestamp_ms / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(50)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&soft_event);

    // Create hard truth event
    let hard_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(2, 2, 1.5, true, 0),
        slot: Some(1002),
        timestamp_ms: timestamp_ms.saturating_add(1000),
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.5,
        reserve_base: Some(997000.0),
        reserve_quote: Some(12.5),
        price_quote: Some(0.0000125),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("hard_truth_sig".to_string()),
        event_ordinal: None,
        block_time: Some(((timestamp_ms.saturating_add(1000)) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(1100)),
        data_source: DataSource::HardTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&hard_event);

    // Verify data source statistics
    let (soft_count, hard_count) = engine.get_data_source_stats(&pool_pubkey, 10);
    assert!(soft_count > 0, "Should have soft truth snapshots");
    assert!(hard_count > 0, "Should have hard truth snapshots");

    println!(
        "✓ Data source distinction test passed: soft={}, hard={}",
        soft_count, hard_count
    );
}

#[test]
fn test_periodic_resynchronization() {
    let mut engine = SnapshotEngine::new(128, 200);

    // Configure resync every 5 slots for faster testing
    let mut config = ResyncConfig::default();
    config.resync_interval_slots = 5;
    engine.set_resync_config(config);

    let engine = Arc::new(engine);
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let mut timestamp_ms = now_ms();
    let mut tx_count: u64 = 0;

    // Send transactions across multiple slots
    for slot in 1001..1020 {
        tx_count = tx_count.saturating_add(1);
        let event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: metrics_from_values(tx_count, tx_count, 0.5 * tx_count as f64, true, 0),
            slot: Some(slot),
            timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: 0.5,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(format!("sig_{}", slot)),
            event_ordinal: None,
            block_time: Some((timestamp_ms / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms.saturating_add(50)),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };

        engine.handle_tx_event(&event);
        timestamp_ms = timestamp_ms.saturating_add(300); // Advance time
    }

    // Verify snapshots were created
    let snapshots = engine.last_n(&pool_pubkey, 20);
    assert!(
        snapshots.len() > 5,
        "Should have multiple snapshots after resync"
    );

    println!(
        "✓ Periodic resynchronization test passed: {} snapshots created",
        snapshots.len()
    );
}

#[test]
fn test_duplicate_detection() {
    let mut engine = SnapshotEngine::new(128, 200);

    // Set up integrity violation tracking BEFORE wrapping in Arc
    let violations = Arc::new(Mutex::new(Vec::new()));
    let violations_clone = violations.clone();

    engine.set_integrity_callback(Arc::new(move |violation: IntegrityViolation| {
        violations_clone.lock().unwrap().push(violation);
    }));

    let engine = Arc::new(engine);
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let timestamp_ms = now_ms();
    let duplicate_sig = "duplicate_signature".to_string();

    // First transaction with signature
    let event1 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(1, 1, 1.0, true, 0),
        slot: Some(1001),
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some(duplicate_sig.clone()),
        event_ordinal: None,
        block_time: Some((timestamp_ms / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(50)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&event1);

    // Duplicate transaction with same signature
    let event2 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(2, 2, 1.0, true, 0),
        slot: Some(1002),
        timestamp_ms: timestamp_ms.saturating_add(500),
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some(duplicate_sig.clone()),
        event_ordinal: None,
        block_time: Some(((timestamp_ms.saturating_add(500)) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms.saturating_add(550)),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&event2);

    // SnapshotEngine nie deduplikuje - brak naruszeń
    let violations_list = violations.lock().unwrap();
    let duplicate_violations: Vec<_> = violations_list
        .iter()
        .filter(|v| v.details.contains("Duplicate transaction"))
        .collect();

    assert!(
        duplicate_violations.is_empty(),
        "SnapshotEngine should not flag duplicates"
    );
}

#[test]
fn test_reorg_detection_noop_in_snapshot_engine() {
    let mut engine = SnapshotEngine::new(128, 200);

    // Set up integrity violation tracking BEFORE wrapping in Arc
    let violations = Arc::new(Mutex::new(Vec::new()));
    let violations_clone = violations.clone();

    engine.set_integrity_callback(Arc::new(move |violation: IntegrityViolation| {
        violations_clone.lock().unwrap().push(violation);
    }));

    let engine = Arc::new(engine);
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let timestamp_ms = now_ms();

    // Transaction at slot 1005
    let event1 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(1, 1, 1.0, true, 0),
        slot: Some(1005),
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("sig_1005".to_string()),
        event_ordinal: None,
        block_time: Some((timestamp_ms / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms + 50),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&event1);

    // Transaction at slot 1003 (regression - reorg)
    let event2 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(2, 2, 1.0, true, 0),
        slot: Some(1003),
        timestamp_ms: timestamp_ms + 500,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("sig_1003".to_string()),
        event_ordinal: None,
        block_time: Some(((timestamp_ms + 500) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms + 550),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&event2);

    // SnapshotEngine no longer performs reorg detection
    let violations_list = violations.lock().unwrap();
    assert!(violations_list.is_empty());
}

#[test]
fn test_jitter_detection_noop_in_snapshot_engine() {
    let mut engine = SnapshotEngine::new(128, 200);
    engine.set_max_jitter_ms(100); // Set low threshold for testing

    let pool_pubkey = Pubkey::new_unique();

    // Set up integrity violation tracking
    let violations = Arc::new(Mutex::new(Vec::new()));
    let violations_clone = violations.clone();

    engine.set_integrity_callback(Arc::new(move |violation: IntegrityViolation| {
        violations_clone.lock().unwrap().push(violation);
    }));

    let engine = Arc::new(engine);
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let timestamp_ms = now_ms();
    let block_time = (timestamp_ms / 1000) as i64;

    // Create event with excessive jitter (arrival time much later than block time)
    let event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(1, 1, 1.0, true, 0),
        slot: Some(1001),
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("jitter_sig".to_string()),
        event_ordinal: None,
        block_time: Some(block_time),
        arrival_time_ms: Some((block_time as u64) * 1000 + 500), // 500ms jitter
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&event);

    // SnapshotEngine no longer performs jitter checks
    let violations_list = violations.lock().unwrap();
    assert!(violations_list.is_empty());
}

#[test]
fn test_volume_anomaly_detection_noop_in_snapshot_engine() {
    let mut engine = SnapshotEngine::new(128, 200);

    // Configure strict volume deviation threshold
    let mut config = ResyncConfig::default();
    config.max_volume_deviation = 1.5; // 150% of baseline
    config.resync_interval_slots = 5;
    engine.set_resync_config(config);

    let pool_pubkey = Pubkey::new_unique();

    // Set up integrity violation tracking
    let violations = Arc::new(Mutex::new(Vec::new()));
    let violations_clone = violations.clone();

    engine.set_integrity_callback(Arc::new(move |violation: IntegrityViolation| {
        violations_clone.lock().unwrap().push(violation);
    }));

    let engine = Arc::new(engine);
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let mut timestamp_ms = now_ms();
    let mut tx_count: u64 = 0;

    // Send normal transactions to establish baseline
    for slot in 1001..1010 {
        tx_count += 1;
        let event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_pubkey,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: metrics_from_values(tx_count, tx_count, 0.5 * tx_count as f64, true, 0),
            slot: Some(slot),
            timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: Pubkey::new_unique(),
            is_buy: true,
            volume_sol: 0.5, // Normal volume
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(format!("normal_sig_{}", slot)),
            event_ordinal: None,
            block_time: Some((timestamp_ms / 1000) as i64),
            arrival_time_ms: Some(timestamp_ms + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };

        engine.handle_tx_event(&event);
        timestamp_ms += 300;
    }

    // Send anomalous transaction with very high volume
    let anomaly_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(tx_count + 1, tx_count + 1, 10.0, true, 0),
        slot: Some(1015),
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 10.0, // Anomalously high volume
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("anomaly_sig".to_string()),
        event_ordinal: None,
        block_time: Some((timestamp_ms / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms + 50),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&anomaly_event);

    // SnapshotEngine no longer performs volume anomaly checks
    let violations_list = violations.lock().unwrap();
    assert!(violations_list.is_empty());
}

#[test]
fn test_soft_vs_hard_truth_validation() {
    let engine = Arc::new(SnapshotEngine::new(128, 200));
    let pool_pubkey = Pubkey::new_unique();
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let timestamp_ms = now_ms();
    let slot = 1001;

    // Send soft truth event
    let soft_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(1, 1, 1.0, true, 0),
        slot: Some(slot),
        timestamp_ms,
        event_time: ghost_core::EventTimeMetadata::new(Some(timestamp_ms), None, None),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(999000.0),
        reserve_quote: Some(11.0),
        price_quote: Some(0.000011),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("soft_sig".to_string()),
        event_ordinal: None,
        block_time: Some((timestamp_ms / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms + 50),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&soft_event);

    let soft_event2 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: metrics_from_values(2, 2, 1.0, true, 0),
        slot: Some(slot + 1),
        timestamp_ms: timestamp_ms + 250,
        event_time: ghost_core::EventTimeMetadata::new(Some(timestamp_ms + 250), None, None),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(998500.0),
        reserve_quote: Some(11.5),
        price_quote: Some(0.0000115),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("soft_sig_2".to_string()),
        event_ordinal: None,
        block_time: Some(((timestamp_ms + 250) / 1000) as i64),
        arrival_time_ms: Some(timestamp_ms + 300),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
    };

    engine.handle_tx_event(&soft_event2);

    // Wait for snapshot to be emitted
    std::thread::sleep(std::time::Duration::from_millis(250));

    // Get the soft truth snapshot
    let soft_snapshot = engine.get_latest_snapshot(&pool_pubkey);
    assert!(soft_snapshot.is_some(), "Should have soft truth snapshot");
    let base_snapshot = soft_snapshot.unwrap();

    // Create hard truth snapshot (slightly different volume - within tolerance)
    let mut hard_snapshot = base_snapshot;
    hard_snapshot.set_data_source(DataSource::HardTruth);
    hard_snapshot.cum_volume_sol = base_snapshot.cum_volume_sol * 1.05; // 5% difference

    // Validate with 10% tolerance - should pass
    let is_valid = engine.validate_soft_vs_hard_truth(&pool_pubkey, &hard_snapshot, 0.1);
    assert!(
        is_valid,
        "Should validate with 5% difference and 10% tolerance"
    );

    // Create hard truth snapshot with large difference
    let mut hard_snapshot_bad = base_snapshot;
    hard_snapshot_bad.set_data_source(DataSource::HardTruth);
    hard_snapshot_bad.cum_volume_sol = base_snapshot.cum_volume_sol * 2.0; // 100% difference

    // Validate with 10% tolerance - should fail
    let is_invalid = engine.validate_soft_vs_hard_truth(&pool_pubkey, &hard_snapshot_bad, 0.1);
    assert!(!is_invalid, "Should fail validation with 100% difference");

    println!("✓ Soft vs hard truth validation test passed");
}

#[test]
fn test_comprehensive_anomaly_injection_noop_in_snapshot_engine() {
    let mut engine = SnapshotEngine::new(128, 200);

    // Configure for comprehensive testing
    let mut config = ResyncConfig::default();
    config.resync_interval_slots = 5;
    config.max_volume_deviation = 2.0;
    engine.set_resync_config(config);
    engine.set_max_jitter_ms(200);

    let pool_pubkey = Pubkey::new_unique();

    // Set up comprehensive violation tracking
    let violations = Arc::new(Mutex::new(Vec::new()));
    let violations_clone = violations.clone();

    engine.set_integrity_callback(Arc::new(move |violation: IntegrityViolation| {
        violations_clone.lock().unwrap().push(violation);
    }));

    let engine = Arc::new(engine);
    let base_mint = create_test_pool(&engine, pool_pubkey);

    let mut timestamp_ms = now_ms();
    let mut anomalies_injected = 0;
    let mut tx_count: u64 = 0;

    // Inject various anomalies randomly
    for slot in 1001..1050 {
        let anomaly_type = slot % 5;
        tx_count += 1;

        let event = match anomaly_type {
            0 => {
                // Normal transaction
                TxEvent {
                    semantic: ghost_core::EventSemanticEnvelope::default(),
                    pool_amm_id: pool_pubkey,
                    base_mint,
                    pool_state: PoolLifecycle::Active,
                    metrics: metrics_from_values(tx_count, tx_count, 0.5, true, 0),
                    slot: Some(slot),
                    timestamp_ms,
                    event_time: ghost_core::EventTimeMetadata::default(),
                    signer: Pubkey::new_unique(),
                    is_buy: true,
                    volume_sol: 0.5,
                    reserve_base: None,
                    reserve_quote: None,
                    price_quote: None,
                    is_dev_buy: false,
                    dev_buy_lamports: 0,
                    signature: Some(format!("sig_{}", slot)),
                    event_ordinal: None,
                    block_time: Some((timestamp_ms / 1000) as i64),
                    arrival_time_ms: Some(timestamp_ms + 50),
                    data_source: DataSource::SoftTruth,
                    intra_slot_offset_ms: None,
                    raw_data: None,
                    raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
                }
            }
            1 => {
                // Inject duplicate (reuse signature from 5 slots ago)
                anomalies_injected += 1;
                TxEvent {
                    semantic: ghost_core::EventSemanticEnvelope::default(),
                    pool_amm_id: pool_pubkey,
                    base_mint,
                    pool_state: PoolLifecycle::Active,
                    metrics: metrics_from_values(tx_count, tx_count, 0.5, true, 0),
                    slot: Some(slot),
                    timestamp_ms,
                    event_time: ghost_core::EventTimeMetadata::default(),
                    signer: Pubkey::new_unique(),
                    is_buy: true,
                    volume_sol: 0.5,
                    reserve_base: None,
                    reserve_quote: None,
                    price_quote: None,
                    is_dev_buy: false,
                    dev_buy_lamports: 0,
                    signature: Some(format!("sig_{}", slot - 5)),
                    event_ordinal: None,
                    block_time: Some((timestamp_ms / 1000) as i64),
                    arrival_time_ms: Some(timestamp_ms + 50),
                    data_source: DataSource::SoftTruth,
                    intra_slot_offset_ms: None,
                    raw_data: None,
                    raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
                }
            }
            2 => {
                // Inject excessive jitter
                anomalies_injected += 1;
                TxEvent {
                    semantic: ghost_core::EventSemanticEnvelope::default(),
                    pool_amm_id: pool_pubkey,
                    base_mint,
                    pool_state: PoolLifecycle::Active,
                    metrics: metrics_from_values(tx_count, tx_count, 0.5, true, 0),
                    slot: Some(slot),
                    timestamp_ms,
                    event_time: ghost_core::EventTimeMetadata::default(),
                    signer: Pubkey::new_unique(),
                    is_buy: true,
                    volume_sol: 0.5,
                    reserve_base: None,
                    reserve_quote: None,
                    price_quote: None,
                    is_dev_buy: false,
                    dev_buy_lamports: 0,
                    signature: Some(format!("sig_{}", slot)),
                    event_ordinal: None,
                    block_time: Some((timestamp_ms / 1000) as i64),
                    arrival_time_ms: Some(timestamp_ms + 500), // Excessive jitter
                    data_source: DataSource::SoftTruth,
                    intra_slot_offset_ms: None,
                    raw_data: None,
                    raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
                }
            }
            3 => {
                // Inject volume anomaly
                anomalies_injected += 1;
                TxEvent {
                    semantic: ghost_core::EventSemanticEnvelope::default(),
                    pool_amm_id: pool_pubkey,
                    base_mint,
                    pool_state: PoolLifecycle::Active,
                    metrics: metrics_from_values(tx_count, tx_count, 5.0, true, 0),
                    slot: Some(slot),
                    timestamp_ms,
                    event_time: ghost_core::EventTimeMetadata::default(),
                    signer: Pubkey::new_unique(),
                    is_buy: true,
                    volume_sol: 5.0, // High volume
                    reserve_base: None,
                    reserve_quote: None,
                    price_quote: None,
                    is_dev_buy: false,
                    dev_buy_lamports: 0,
                    signature: Some(format!("sig_{}", slot)),
                    event_ordinal: None,
                    block_time: Some((timestamp_ms / 1000) as i64),
                    arrival_time_ms: Some(timestamp_ms + 50),
                    data_source: DataSource::SoftTruth,
                    intra_slot_offset_ms: None,
                    raw_data: None,
                    raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
                }
            }
            _ => {
                // Normal transaction
                TxEvent {
                    semantic: ghost_core::EventSemanticEnvelope::default(),
                    pool_amm_id: pool_pubkey,
                    base_mint,
                    pool_state: PoolLifecycle::Active,
                    metrics: metrics_from_values(tx_count, tx_count, 0.5, true, 0),
                    slot: Some(slot),
                    timestamp_ms,
                    event_time: ghost_core::EventTimeMetadata::default(),
                    signer: Pubkey::new_unique(),
                    is_buy: true,
                    volume_sol: 0.5,
                    reserve_base: None,
                    reserve_quote: None,
                    price_quote: None,
                    is_dev_buy: false,
                    dev_buy_lamports: 0,
                    signature: Some(format!("sig_{}", slot)),
                    event_ordinal: None,
                    block_time: Some((timestamp_ms / 1000) as i64),
                    arrival_time_ms: Some(timestamp_ms + 50),
                    data_source: DataSource::SoftTruth,
                    intra_slot_offset_ms: None,
                    raw_data: None,
                    raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
                }
            }
        };

        engine.handle_tx_event(&event);
        timestamp_ms += 300;
    }

    // SnapshotEngine no longer performs anomaly detection
    let violations_list = violations.lock().unwrap();
    assert!(violations_list.is_empty());

    println!("✓ Comprehensive anomaly injection test completed (no violations expected)");
    println!("  - Anomalies injected: {}", anomalies_injected);
}
