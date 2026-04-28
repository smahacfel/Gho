//! D4: Single-Writer After Commit Integration Test - FIXED (BLOCKER)
//!
//! Full integration test proving that after commit, SnapshotEngine
//! stops writing to ShadowLedger (committed_to_live_pipeline flag).

use ghost_brain::oracle::snapshot_engine::{DataSource, PoolLifecycle, PoolMetrics};
use ghost_brain::oracle::{SnapshotEngine, TxEvent};
use ghost_core::shadow_ledger::{MarketSnapshot, ShadowLedger};
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

fn test_pubkey(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

#[test]
fn test_snapshot_engine_stops_writing_after_commit() {
    // BLOCKER FIX: Real integration test for single-writer with correct API

    let shadow_ledger = Arc::new(ShadowLedger::new());
    let mut engine = SnapshotEngine::new(128, 0);
    engine.set_shadow_ledger(Arc::clone(&shadow_ledger));

    let pool = test_pubkey(1);
    let mint = test_pubkey(2);
    engine.mark_pool_active(pool);

    // ShadowLedger accepts live writes only after history commit.
    shadow_ledger.commit_history(mint, vec![MarketSnapshot::new(1)], None);

    // Phase 1: Process TXs before commit
    // SnapshotEngine emits a snapshot only after the first event establishes a baseline,
    // so we need at least 2 tx events to force a ledger write.
    let tx1 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool,
        base_mint: mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(1000),
        timestamp_ms: 1700000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: test_pubkey(3),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(1000.0),
        reserve_quote: Some(100.0),
        price_quote: Some(0.1),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("sig1".to_string()),
        event_ordinal: None,
        block_time: Some(1700000000),
        arrival_time_ms: Some(1700000000000),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    engine.handle_tx_event(&tx1);

    let tx1b = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool,
        base_mint: mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(1001),
        timestamp_ms: 1700000000001,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: test_pubkey(3),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(1000.0),
        reserve_quote: Some(100.0),
        price_quote: Some(0.1),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("sig1b".to_string()),
        event_ordinal: None,
        block_time: Some(1700000001),
        arrival_time_ms: Some(1700000000001),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    engine.handle_tx_event(&tx1b);

    let snapshots_before_commit = shadow_ledger.get_snapshots(&mint).unwrap_or_default();
    let count_before = snapshots_before_commit.len();
    assert!(
        count_before > 0,
        "SnapshotEngine should write before commit"
    );

    // Phase 2: Simulate commit by setting committed flag for THIS POOL (BLOCKER FIX)
    engine.mark_pool_committed(pool);

    // Phase 3: Process TX after commit (SnapshotEngine should NOT write)
    let tx2 = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id: pool,
        base_mint: mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(1001),
        timestamp_ms: 1700000001000,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: test_pubkey(4),
        is_buy: true,
        volume_sol: 1.0,
        reserve_base: Some(1000.0),
        reserve_quote: Some(100.0),
        price_quote: Some(0.1),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("sig2".to_string()),
        event_ordinal: None,
        block_time: Some(1700000001),
        arrival_time_ms: Some(1700000001000),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None,
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    engine.handle_tx_event(&tx2);

    // Phase 4: Verify ledger count did NOT increase
    let snapshots_after_commit = shadow_ledger.get_snapshots(&mint).unwrap_or_default();
    let count_after = snapshots_after_commit.len();

    assert_eq!(
        count_after, count_before,
        "SnapshotEngine should NOT write to ledger after commit (single-writer enforced)"
    );

    println!("✅ D4 PASS (BLOCKER FIXED): Single-writer verified - SnapshotEngine stopped writing after pool committed");
}
