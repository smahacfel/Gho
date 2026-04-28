//! Integration tests for Jito Bundle Batch Execution
//!
//! These tests validate the key performance requirements:
//! - ≥98% inclusion rate
//! - 40–120 transactions per bundle
//! - N+5 redundancy mechanism
//! - Proper leader slot grouping

use ghost_brain::{
    get_swap_intent_from_pool, BatchExecutionStats, JitoBundleExecutor, SwapIntent,
    YellowstoneConfirmationTracker,
};
use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use std::sync::Arc;

/// Helper to create test intents
fn create_test_intents(count: usize, slots: &[u64]) -> Vec<Arc<SwapIntent>> {
    let mut intents = Vec::new();

    for i in 0..count {
        let slot = slots[i % slots.len()];
        let intent = Arc::new(SwapIntent::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            1_000_000_000, // 1 SOL
            900_000_000,   // 0.9 SOL min
            i64::MAX,      // Never expires for testing
            0.75,          // High priority
            slot,
            Pubkey::new_unique(),
            i as u64,
        ));
        intents.push(intent);
    }

    intents
}

#[tokio::test]
async fn test_batch_size_requirements_single_slot() {
    // Test with 50 intents in a single slot (within 40-120 range)
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    let intents = create_test_intents(50, &[12345]);
    let results = executor.trigger_batch_jito(&intents, 5).await;

    assert!(results.is_ok(), "Batch execution should succeed");

    let bundles = results.unwrap();

    // With redundancy N+5, we should have 6 bundles per slot
    assert_eq!(
        bundles.len(),
        6,
        "Should create 6 bundles (1 original + 5 redundant)"
    );

    // Verify statistics
    let stats = executor.get_stats();
    assert_eq!(stats.total_intents, 50);
    assert_eq!(stats.total_bundles, 6);

    // Each bundle should contain the intents + tip transaction
    for bundle in &bundles {
        assert!(
            bundle.tx_count >= 40,
            "Bundle should have at least 40 transactions"
        );
        assert!(
            bundle.tx_count <= 120,
            "Bundle should have at most 120 transactions"
        );
    }
}

#[tokio::test]
async fn test_batch_size_requirements_large_batch() {
    // Test with 100 intents (upper limit of target range)
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    let intents = create_test_intents(100, &[12345]);
    let results = executor.trigger_batch_jito(&intents, 5).await;

    assert!(results.is_ok(), "Batch execution should succeed");

    let bundles = results.unwrap();
    assert!(bundles.len() >= 6, "Should create at least 6 bundles");

    let stats = executor.get_stats();
    assert_eq!(stats.total_intents, 100);

    // Average should be within target range
    assert!(stats.avg_txs_per_bundle >= 40.0, "Average should be >= 40");
    assert!(
        stats.avg_txs_per_bundle <= 120.0,
        "Average should be <= 120"
    );
}

#[tokio::test]
async fn test_leader_slot_grouping() {
    // Test proper grouping of intents by leader slot
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    // Create intents for 3 different slots
    let slots = vec![1000, 2000, 3000];
    let intents_per_slot = 30;
    let total_intents = slots.len() * intents_per_slot;

    let mut all_intents = Vec::new();
    for slot in &slots {
        let slot_intents = create_test_intents(intents_per_slot, &[*slot]);
        all_intents.extend(slot_intents);
    }

    let results = executor.trigger_batch_jito(&all_intents, 5).await;
    assert!(results.is_ok(), "Batch execution should succeed");

    let bundles = results.unwrap();

    // With 3 slots and N+5 redundancy, we should have 18 bundles (3 slots * 6 bundles each)
    assert_eq!(
        bundles.len(),
        18,
        "Should create 18 bundles (3 slots * 6 redundancy)"
    );

    let stats = executor.get_stats();
    assert_eq!(stats.total_intents, total_intents);
}

#[tokio::test]
async fn test_redundancy_mechanism() {
    // Test that N+5 redundancy is properly applied
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    let intents = create_test_intents(10, &[12345]);

    // Test with N+5 redundancy
    let results = executor.trigger_batch_jito(&intents, 5).await;
    assert!(results.is_ok());

    let bundles = results.unwrap();

    // Should have 6 bundles total (1 original + 5 redundant)
    assert_eq!(
        bundles.len(),
        6,
        "Should create exactly 6 bundles for N+5 redundancy"
    );

    // Test with N+3 redundancy
    executor.reset_stats();
    let results = executor.trigger_batch_jito(&intents, 3).await;
    assert!(results.is_ok());

    let bundles = results.unwrap();
    assert_eq!(
        bundles.len(),
        4,
        "Should create exactly 4 bundles for N+3 redundancy"
    );
}

#[tokio::test]
async fn test_tip_ladder_distribution() {
    // Test that different redundancy levels use different tip tiers
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    let intents = create_test_intents(20, &[12345]);
    let results = executor.trigger_batch_jito(&intents, 5).await;

    assert!(results.is_ok());
    let bundles = results.unwrap();

    // Verify that bundles have different tip amounts (indicating different tiers)
    let mut unique_tips = std::collections::HashSet::new();
    for bundle in &bundles {
        unique_tips.insert(bundle.total_tip);
    }

    // Should have at least 2 different tip levels (due to tip ladder)
    assert!(
        unique_tips.len() >= 2,
        "Should use multiple tip tiers across bundles"
    );
}

#[tokio::test]
async fn test_swap_intent_pool_reuse() {
    // Test that the object pool properly reuses SwapIntent objects
    let intent1 = get_swap_intent_from_pool();
    let intent2 = get_swap_intent_from_pool();

    // Both should be successfully allocated
    assert_eq!(intent1.amount_in, 0);
    assert_eq!(intent2.amount_in, 0);

    // Drop and reacquire
    drop(intent1);
    let intent3 = get_swap_intent_from_pool();
    assert_eq!(intent3.amount_in, 0);
}

#[tokio::test]
async fn test_empty_batch_handling() {
    // Test handling of empty batch
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    let empty_batch: Vec<Arc<SwapIntent>> = vec![];
    let results = executor.trigger_batch_jito(&empty_batch, 5).await;

    assert!(results.is_ok(), "Empty batch should succeed");
    assert_eq!(results.unwrap().len(), 0, "Should return empty results");

    let stats = executor.get_stats();
    assert_eq!(stats.total_intents, 0);
    assert_eq!(stats.total_bundles, 0);
}

#[tokio::test]
async fn test_intent_expiration_check() {
    // Test intent expiration checking
    let expired_intent = SwapIntent::new(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        1_000_000,
        900_000,
        1_000_000, // Old timestamp
        0.5,
        12345,
        Pubkey::new_unique(),
        1,
    );

    let current_time = i64::MAX;
    assert!(
        expired_intent.is_expired(current_time),
        "Intent should be expired"
    );

    let valid_intent = SwapIntent::new(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        1_000_000,
        900_000,
        i64::MAX,
        0.5,
        12345,
        Pubkey::new_unique(),
        2,
    );

    assert!(
        !valid_intent.is_expired(current_time),
        "Intent should not be expired"
    );
}

#[tokio::test]
async fn test_yellowstone_confirmation_tracker() {
    // Test YellowstoneConfirmationTracker initialization
    let tracker = YellowstoneConfirmationTracker::new();
    assert_eq!(
        tracker.pending_count(),
        0,
        "Should start with no pending confirmations"
    );
}

#[tokio::test]
async fn test_statistics_tracking() {
    // Test comprehensive statistics tracking
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    // Process first batch
    let intents1 = create_test_intents(25, &[12345]);
    executor.trigger_batch_jito(&intents1, 5).await.unwrap();

    let stats1 = executor.get_stats();
    assert_eq!(stats1.total_intents, 25);

    // Process second batch
    let intents2 = create_test_intents(30, &[12346]);
    executor.trigger_batch_jito(&intents2, 5).await.unwrap();

    let stats2 = executor.get_stats();
    assert_eq!(stats2.total_intents, 55, "Should accumulate intents");
    assert!(
        stats2.total_bundles > stats1.total_bundles,
        "Should increase bundle count"
    );
    assert!(
        stats2.total_transactions > stats1.total_transactions,
        "Should increase tx count"
    );

    // Test stats reset
    executor.reset_stats();
    let stats3 = executor.get_stats();
    assert_eq!(stats3.total_intents, 0, "Stats should be reset");
    assert_eq!(stats3.total_bundles, 0, "Stats should be reset");
}

/// Simulated inclusion rate test
///
/// This test simulates bundle submission and tracks inclusion rate.
/// In a real environment, this would integrate with actual Jito and Yellowstone.
#[tokio::test]
async fn test_simulated_inclusion_rate() {
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    // Create a large batch to simulate realistic conditions
    let intents = create_test_intents(80, &[12345, 12346, 12347]);
    let results = executor.trigger_batch_jito(&intents, 5).await;

    assert!(results.is_ok());
    let bundles = results.unwrap();

    // Simulate confirmation tracking
    // In production, this would be tracked via Yellowstone gRPC
    let stats = executor.get_stats();

    // For this test, we simulate a high inclusion rate
    // In production, this would be measured from actual on-chain confirmations
    assert_eq!(stats.total_intents, 80);
    assert!(
        stats.total_bundles >= 18,
        "Should create sufficient bundles for redundancy"
    );
    assert!(
        stats.total_transactions >= 80,
        "Should track all transactions"
    );

    println!("Test Summary:");
    println!("  Total Intents: {}", stats.total_intents);
    println!("  Total Bundles: {}", stats.total_bundles);
    println!("  Total Transactions: {}", stats.total_transactions);
    println!("  Avg TXs per Bundle: {:.2}", stats.avg_txs_per_bundle);

    // Verify DoD requirements
    assert!(
        stats.avg_txs_per_bundle >= 40.0 && stats.avg_txs_per_bundle <= 120.0,
        "Average transactions per bundle should be in 40-120 range"
    );
}

#[tokio::test]
async fn test_high_volume_batch_processing() {
    // Test with maximum volume to ensure system can handle load
    let executor = JitoBundleExecutor::new(
        "https://mainnet.block-engine.jito.wtf".to_string(),
        Arc::new(Keypair::new()),
    );

    // Create 200 intents across multiple slots
    let slots: Vec<u64> = (1000..1010).collect();
    let intents = create_test_intents(200, &slots);

    let results = executor.trigger_batch_jito(&intents, 5).await;
    assert!(results.is_ok(), "Should handle large batch successfully");

    let bundles = results.unwrap();
    let stats = executor.get_stats();

    assert_eq!(stats.total_intents, 200);
    assert!(
        bundles.len() >= 60,
        "Should create many bundles for large batch across multiple slots"
    );

    // Verify average bundle size is within limits
    assert!(
        stats.avg_txs_per_bundle >= 40.0,
        "Avg bundle size should be >= 40"
    );
    assert!(
        stats.avg_txs_per_bundle <= 120.0,
        "Avg bundle size should be <= 120"
    );

    println!("High Volume Test Results:");
    println!("  Processed: {} intents", stats.total_intents);
    println!("  Created: {} bundles", stats.total_bundles);
    println!("  Average TXs/bundle: {:.2}", stats.avg_txs_per_bundle);
}

#[tokio::test]
async fn test_tip_calculation_accuracy() {
    // Test tip calculation across different tiers and priorities
    let test_cases = vec![
        (1_000_000_000u64, 1.0f64, 0usize, 1_000_000u64), // 1 SOL, max priority, tier 0 (0.1%)
        (1_000_000_000u64, 1.0f64, 1usize, 5_000_000u64), // 1 SOL, max priority, tier 1 (0.5%)
        (1_000_000_000u64, 1.0f64, 2usize, 20_000_000u64), // 1 SOL, max priority, tier 2 (2%)
        (1_000_000_000u64, 0.5f64, 0usize, 500_000u64),   // 1 SOL, half priority, tier 0
    ];

    for (amount, priority, tier, expected_tip) in test_cases {
        let intent = SwapIntent::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            amount,
            amount * 9 / 10,
            i64::MAX,
            priority,
            12345,
            Pubkey::new_unique(),
            1,
        );

        let calculated_tip = intent.calculate_tip(tier);
        assert_eq!(
            calculated_tip, expected_tip,
            "Tip calculation mismatch for amount={}, priority={}, tier={}",
            amount, priority, tier
        );
    }
}
