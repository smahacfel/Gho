//! End-to-End test for raw_bytes pipeline
//!
//! Validates that raw transaction bytes flow from event creation
//! through to MPCF actor classification.
//!
//! Issue #158: Smoke test for raw_bytes capture and MPCF integration

use ghost_brain::oracle::snapshot_engine::{
    DataSource, PoolLifecycle, PoolMetrics, SnapshotEngine, TransactionRecord, TxEvent,
};
use ghost_brain::oracle::ultrafast::mpcf::{self, ActorType};
use seer::types::RawBytesMissingReason;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Create a mock transaction with raw bytes
fn create_mock_transaction_bytes() -> Vec<u8> {
    // Minimal valid Solana transaction structure
    // This is a simplified mock - in production would be actual transaction bytes
    // Note: Real Solana transactions have 64-byte signatures, but we use 16 bytes
    // here for simplicity since we're testing the pipeline, not signature validity
    vec![
        0x01, // Version (1 signature)
        // Signature bytes (16 bytes - simplified, real signatures are 64 bytes)
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
        0x00, // Message header (4 bytes)
        0x01, // 1 required signature
        0x00, // 0 readonly signed accounts
        0x01, // 1 readonly unsigned account
        0x02, // 2 accounts
        // Additional instruction data (10 bytes)
        // Total: 1 + 16 + 4 + 10 = 31 bytes
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
        // One more byte to reach exactly MIN_PAYLOAD_SIZE (32 bytes)
        0x0A,
    ]
}

#[test]
fn test_raw_bytes_end_to_end_pipeline() {
    // ========================================
    // SETUP: SnapshotEngine
    // ========================================

    let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));

    let pool_amm_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    snapshot_engine.mark_pool_active(pool_amm_id);

    // ========================================
    // STEP 1: Create event with raw bytes
    // ========================================

    let raw_bytes = create_mock_transaction_bytes();
    println!("📦 Created mock transaction: {} bytes", raw_bytes.len());

    let tx_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(12345),
        timestamp_ms: 1640000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 1.5,
        reserve_base: Some(1000000.0),
        reserve_quote: Some(50.0),
        price_quote: Some(0.00005),
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("test_sig_12345".to_string()),
        event_ordinal: None,
        block_time: None,
        arrival_time_ms: Some(1640000000000),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: Some(raw_bytes.clone()), // ← Raw bytes attached
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    // ========================================
    // STEP 2: Process through SnapshotEngine
    // ========================================

    snapshot_engine.handle_tx_event(&tx_event);
    println!("✅ Transaction processed by SnapshotEngine");

    // ========================================
    // STEP 3: Verify raw_bytes in SnapshotEngine
    // ========================================

    let transactions = snapshot_engine.get_transactions(&pool_amm_id);

    assert!(
        !transactions.is_empty(),
        "SnapshotEngine should have transactions"
    );

    let first_tx = &transactions[0];

    assert!(
        first_tx.raw_bytes.is_some(),
        "Transaction should have raw_bytes"
    );

    let captured_bytes = first_tx.raw_bytes.as_ref().unwrap();

    assert_eq!(
        captured_bytes, &raw_bytes,
        "Captured bytes should match original"
    );

    println!(
        "✅ raw_bytes verified in SnapshotEngine: {} bytes",
        captured_bytes.len()
    );

    // ========================================
    // STEP 4: Test MPCF inference
    // ========================================

    let inference = mpcf::mpcf_infer(captured_bytes);

    println!("✅ MPCF inference completed:");
    println!("   Actor: {:?}", inference.actor);
    println!("   Confidence: {:.2}", inference.confidence);
    println!("   Entropy: {:.3}", inference.entropy);

    // Verify inference produced valid results
    assert!(
        inference.confidence >= 0.0 && inference.confidence <= 1.0,
        "Confidence should be in range [0.0, 1.0]"
    );

    assert!(inference.entropy >= 0.0, "Entropy should be non-negative");

    // With small mock payload, actor might be Unknown due to size
    // That's acceptable - we're testing the pipeline, not classification accuracy
    println!("   Note: Small mock payload may result in Unknown classification");

    println!("✅ E2E pipeline test PASSED");
}

#[test]
fn test_mpcf_fallback_when_no_raw_bytes() {
    // ========================================
    // Test fallback behavior (negative case)
    // ========================================

    let pool_amm_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();

    let tx_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(12345),
        timestamp_ms: 1640000000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: false,
        volume_sol: 0.5,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("test_sig_no_bytes".to_string()),
        event_ordinal: None,
        block_time: None,
        arrival_time_ms: Some(1640000000000),
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: None, // ← No raw bytes
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    let tx_record = TransactionRecord::from_tx_event(&tx_event);

    assert!(
        tx_record.raw_bytes.is_none(),
        "TransactionRecord should have None for raw_bytes"
    );

    println!("✅ Fallback test: Confirmed raw_bytes is None when not provided");
}

#[test]
fn test_mpcf_entropy_calculation() {
    // ========================================
    // Verify MPCF entropy calculation
    // ========================================

    // High entropy (diverse bytes)
    let high_entropy_bytes: Vec<u8> = (0..64).map(|i| (i * 37) as u8).collect();
    let inference_high = mpcf::mpcf_infer(&high_entropy_bytes);

    // Low entropy (repetitive bytes)
    let low_entropy_bytes = vec![0x42; 64];
    let inference_low = mpcf::mpcf_infer(&low_entropy_bytes);

    println!("High entropy bytes: {:.3}", inference_high.entropy);
    println!("Low entropy bytes: {:.3}", inference_low.entropy);

    assert!(
        inference_high.entropy > inference_low.entropy,
        "High entropy bytes should have higher entropy score"
    );

    println!("✅ MPCF entropy calculation working correctly");
}

#[test]
fn test_transaction_record_from_event_with_raw_bytes() {
    // Test that TransactionRecord correctly extracts raw_bytes from TxEvent

    let raw_bytes = vec![0x01, 0x02, 0x03, 0x04, 0x05];
    let pool_amm_id = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();

    let tx_event = TxEvent {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        pool_amm_id,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics::default(),
        slot: Some(100),
        timestamp_ms: 1000000,
        event_time: ghost_core::EventTimeMetadata::default(),
        signer: Pubkey::new_unique(),
        is_buy: true,
        volume_sol: 2.0,
        reserve_base: None,
        reserve_quote: None,
        price_quote: None,
        is_dev_buy: false,
        dev_buy_lamports: 0,
        signature: Some("test_sig".to_string()),
        event_ordinal: None,
        block_time: None,
        arrival_time_ms: None,
        data_source: DataSource::SoftTruth,
        intra_slot_offset_ms: None,
        raw_data: Some(raw_bytes.clone()),
        raw_data_missing_reason: RawBytesMissingReason::Unknown,
    };

    let tx_record = TransactionRecord::from_tx_event(&tx_event);

    assert!(tx_record.raw_bytes.is_some());
    assert_eq!(tx_record.raw_bytes.unwrap(), raw_bytes);
    assert_eq!(tx_record.slot, Some(100));
    assert_eq!(tx_record.sol_amount, 2.0);
    assert!(tx_record.is_buy);

    println!("✅ TransactionRecord correctly extracts raw_bytes from TxEvent");
}

#[test]
fn test_mpcf_with_realistic_payload_sizes() {
    // Test MPCF with various realistic payload sizes

    // Small payload (below min threshold)
    let small = vec![0x42; 20];
    let result_small = mpcf::mpcf_infer(&small);
    assert_eq!(
        result_small.actor,
        ActorType::Unknown,
        "Small payload should return Unknown"
    );

    // Medium payload (typical bot transaction)
    let medium = vec![0x42; 256];
    let result_medium = mpcf::mpcf_infer(&medium);
    assert!(
        result_medium.confidence > 0.0,
        "Medium payload should have some confidence"
    );

    // Large payload (typical human transaction)
    let large: Vec<u8> = (0..1024).map(|i| (i * 7) as u8).collect();
    let result_large = mpcf::mpcf_infer(&large);
    assert!(
        result_large.entropy > 0.0,
        "Large payload should have entropy > 0"
    );

    println!("✅ MPCF handles various payload sizes correctly");
}

#[test]
fn test_snapshot_engine_transaction_buffer() {
    // Verify SnapshotEngine properly stores transactions with raw_bytes

    let snapshot_engine = Arc::new(SnapshotEngine::new(64, 100));
    let base_mint = Pubkey::new_unique();
    let pool_amm_id = Pubkey::new_unique();
    snapshot_engine.mark_pool_active(pool_amm_id);

    // Add multiple transactions
    for i in 0..5 {
        let raw_bytes = vec![i as u8; 40];

        let tx_event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(1000 + i),
            timestamp_ms: 1640000000000 + (i * 1000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: Pubkey::new_unique(),
            is_buy: i % 2 == 0,
            volume_sol: 1.0 + i as f64,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(format!("sig_{}", i)),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: Some(raw_bytes),
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        snapshot_engine.handle_tx_event(&tx_event);
    }

    let transactions = snapshot_engine.get_transactions(&pool_amm_id);

    assert_eq!(transactions.len(), 5, "Should have 5 transactions");

    // Verify all have raw_bytes
    for (i, tx) in transactions.iter().enumerate() {
        assert!(
            tx.raw_bytes.is_some(),
            "Transaction {} should have raw_bytes",
            i
        );
        assert_eq!(
            tx.raw_bytes.as_ref().unwrap().len(),
            40,
            "Transaction {} raw_bytes size mismatch",
            i
        );
    }

    println!("✅ SnapshotEngine correctly stores multiple transactions with raw_bytes");
}
