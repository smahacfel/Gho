//! Integration tests for Pump.fun CurveStateCache and EarlySwapRingBuffer
//!
//! Tests validate:
//! - Real-time snapshot updates from gRPC-like events
//! - Ring buffer behavior with TTL enforcement
//! - Cache hit rates and telemetry
//! - Thread-safe concurrent operations
//! - Performance characteristics

use ghost_brain::pumpfun::{
    CurveSnapshot, EarlySwapEvent, PumpCurveStateCache, EARLY_SWAP_BUFFER_SIZE, GENESIS_FEE_BPS,
    GENESIS_VIRTUAL_SOL_LAMPORTS, GENESIS_VIRTUAL_TOKEN_AMOUNT,
};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Test that cache can store and retrieve snapshots
#[test]
fn test_cache_basic_snapshot_operations() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Create and update snapshot
    let snapshot = CurveSnapshot::new(
        1_000_000_000,     // 1 SOL virtual
        1_000_000_000_000, // 1M tokens virtual
        100,               // 1% fee
        Some(12345),
    );

    let is_new = cache.update_snapshot(curve, snapshot.clone());
    assert!(is_new, "First update should report as new curve");

    // Retrieve and verify
    let retrieved = cache.get_snapshot(&curve).expect("Snapshot should exist");

    assert_eq!(
        retrieved.virtual_sol_reserves_lamports, 1_000_000_000,
        "SOL reserves should match"
    );
    assert_eq!(
        retrieved.virtual_token_reserves, 1_000_000_000_000,
        "Token reserves should match"
    );
    assert_eq!(retrieved.fee_bps, 100, "Fee should match");
    assert_eq!(retrieved.last_update_slot, Some(12345), "Slot should match");
    assert!(retrieved.has_valid_reserves(), "Reserves should be valid");
}

/// Test that snapshots with real reserves are correctly stored
#[test]
fn test_cache_snapshot_with_real_reserves() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345))
        .with_real_reserves(2_000_000_000, 2_000_000_000_000);

    cache.update_snapshot(curve, snapshot);

    let retrieved = cache.get_snapshot(&curve).expect("Snapshot should exist");
    assert_eq!(
        retrieved.real_sol_reserves_lamports,
        Some(2_000_000_000),
        "Real SOL reserves should match"
    );
    assert_eq!(
        retrieved.real_token_reserves,
        Some(2_000_000_000_000),
        "Real token reserves should match"
    );
}

/// Test genesis injection creates snapshot and does not override existing data
#[test]
fn test_cache_inject_genesis() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();
    let slot = 42;

    cache.inject_genesis(&curve, slot);

    let snapshot = cache
        .get_snapshot(&curve)
        .expect("Genesis snapshot should exist");
    assert_eq!(
        snapshot.virtual_sol_reserves_lamports,
        GENESIS_VIRTUAL_SOL_LAMPORTS
    );
    assert_eq!(
        snapshot.virtual_token_reserves,
        GENESIS_VIRTUAL_TOKEN_AMOUNT
    );
    assert_eq!(
        snapshot.real_sol_reserves_lamports,
        Some(GENESIS_VIRTUAL_SOL_LAMPORTS)
    );
    assert_eq!(
        snapshot.real_token_reserves,
        Some(GENESIS_VIRTUAL_TOKEN_AMOUNT)
    );
    assert_eq!(snapshot.fee_bps, GENESIS_FEE_BPS);
    assert_eq!(snapshot.last_update_slot, Some(slot));

    let manual_snapshot = CurveSnapshot::new(5, 10, 25, Some(99));
    cache.update_snapshot(curve, manual_snapshot.clone());

    cache.inject_genesis(&curve, slot + 1);

    let final_snapshot = cache
        .get_snapshot(&curve)
        .expect("Snapshot should persist after second inject");
    assert_eq!(
        final_snapshot.virtual_sol_reserves_lamports,
        manual_snapshot.virtual_sol_reserves_lamports
    );
    assert_eq!(
        final_snapshot.virtual_token_reserves,
        manual_snapshot.virtual_token_reserves
    );
    assert_eq!(final_snapshot.fee_bps, manual_snapshot.fee_bps);
    assert_eq!(
        final_snapshot.last_update_slot,
        manual_snapshot.last_update_slot
    );
}

/// Test that swap events are correctly buffered
#[test]
fn test_cache_swap_event_buffering() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Add multiple swap events
    for i in 0..5 {
        let event = EarlySwapEvent::new((i + 1) * 100_000_000, i % 2 == 0, i as u64);
        cache.update_swap(curve, event);
    }

    let swaps = cache.get_early_swaps(&curve);
    assert_eq!(swaps.len as usize, 5, "All swaps should be present");

    // Verify order and data using iterator
    let mut iter = swaps.iter();
    let first = iter.next().unwrap();
    assert_eq!(first.amount_in, 100_000_000);
    assert!(first.is_buy);
    let second = iter.next().unwrap();
    assert_eq!(second.amount_in, 200_000_000);
    assert!(!second.is_buy);
}

/// Test that swap buffer correctly handles overflow (FIFO)
#[test]
fn test_cache_swap_buffer_overflow() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Fill buffer beyond capacity
    let overflow_count = EARLY_SWAP_BUFFER_SIZE + 10;
    for i in 0..overflow_count {
        let event = EarlySwapEvent::new(i as u64, true, i as u64);
        cache.update_swap(curve, event);
    }

    let swaps = cache.get_early_swaps(&curve);
    assert!(
        swaps.len as usize <= EARLY_SWAP_BUFFER_SIZE,
        "Should not exceed buffer size"
    );

    // Newest events should be present (FIFO behavior)
    // Due to TTL, all recent events should still be valid
    assert!(
        !swaps.is_empty(),
        "Should have some valid events after overflow"
    );
}

/// Test that expired swap events are filtered out
#[test]
fn test_cache_swap_event_ttl_filtering() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Add events that will be expired
    for i in 0..3 {
        let mut event = EarlySwapEvent::new(i * 100_000_000, true, i);
        // Manually set timestamp to 3 seconds ago (beyond TTL)
        event.timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 3000;
        cache.update_swap(curve, event);
    }

    // Add fresh events
    for i in 3..6 {
        let event = EarlySwapEvent::new(i * 100_000_000, false, i);
        cache.update_swap(curve, event);
    }

    let valid_swaps = cache.get_early_swaps(&curve);

    // Only fresh events (last 3) should be valid
    assert_eq!(
        valid_swaps.len as usize, 3,
        "Only events within TTL should be returned"
    );

    // Verify they are the fresh ones
    for swap in valid_swaps.iter() {
        assert!(!swap.is_buy, "Fresh events should be sells");
    }
}

/// Test that cache correctly tracks hit/miss rates
#[test]
fn test_cache_metrics_tracking() {
    let cache = PumpCurveStateCache::new();
    let curve1 = Pubkey::new_unique();
    let curve2 = Pubkey::new_unique();

    // Add snapshot for curve1
    let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
    cache.update_snapshot(curve1, snapshot);

    // 3 hits on curve1
    cache.get_snapshot(&curve1);
    cache.get_snapshot(&curve1);
    cache.get_snapshot(&curve1);

    // 2 misses on curve2
    cache.get_snapshot(&curve2);
    cache.get_snapshot(&curve2);

    let metrics = cache.metrics();

    // Note: First update_snapshot doesn't count as hit/miss, only get_snapshot calls
    assert_eq!(
        metrics
            .cache_hits
            .load(std::sync::atomic::Ordering::Relaxed),
        3,
        "Should have 3 cache hits"
    );
    assert_eq!(
        metrics
            .cache_misses
            .load(std::sync::atomic::Ordering::Relaxed),
        2,
        "Should have 2 cache misses"
    );

    let hit_rate = metrics.hit_rate();
    assert!(
        (hit_rate - 60.0).abs() < 1.0,
        "Hit rate should be ~60% (3/5)"
    );
}

/// Test that get_state returns both snapshot and swaps atomically
#[test]
fn test_cache_get_state_atomic() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Setup snapshot
    let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
    cache.update_snapshot(curve, snapshot);

    // Add swaps
    cache.update_swap(curve, EarlySwapEvent::new(100_000_000, true, 1));
    cache.update_swap(curve, EarlySwapEvent::new(200_000_000, false, 2));

    // Get both atomically
    let (snap, swaps) = cache.get_state(&curve).expect("State should exist");

    assert_eq!(snap.virtual_sol_reserves_lamports, 1_000_000_000);
    assert_eq!(swaps.len as usize, 2);
}

/// Test that cache handles missing curves gracefully
#[test]
fn test_cache_missing_curve_handling() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    assert!(
        cache.get_snapshot(&curve).is_none(),
        "Should return None for missing curve"
    );

    let swaps = cache.get_early_swaps(&curve);
    assert!(
        swaps.is_empty(),
        "Should return empty vec for missing curve"
    );

    assert!(
        cache.get_state(&curve).is_none(),
        "get_state should return None for missing curve"
    );
}

/// Test concurrent access from multiple threads
#[test]
fn test_cache_concurrent_access() {
    let cache = Arc::new(PumpCurveStateCache::new());
    let curve = Pubkey::new_unique();

    // Initialize snapshot
    let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
    cache.update_snapshot(curve, snapshot);

    let mut handles = vec![];

    // Spawn 10 threads doing concurrent reads
    for _ in 0..10 {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                cache_clone.get_snapshot(&curve);
            }
        });
        handles.push(handle);
    }

    // Spawn 5 threads doing concurrent writes
    for thread_id in 0..5 {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            for i in 0..50 {
                let event =
                    EarlySwapEvent::new((thread_id * 1000 + i) as u64, i % 2 == 0, i as u64);
                cache_clone.update_swap(curve, event);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().expect("Thread should complete");
    }

    // Verify cache is still consistent
    let swaps = cache.get_early_swaps(&curve);
    assert!(
        !swaps.is_empty(),
        "Should have some swaps after concurrent writes"
    );
    assert!(
        swaps.len as usize <= EARLY_SWAP_BUFFER_SIZE,
        "Should not exceed buffer size"
    );
}

/// Test that telemetry correctly counts snapshot updates
#[test]
fn test_cache_telemetry_snapshot_counts() {
    let cache = PumpCurveStateCache::new();

    for i in 0..10 {
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(
            (i + 1) * 1_000_000_000,
            ((i + 1) * 1_000_000_000_000) as u128,
            100,
            Some(i),
        );
        cache.update_snapshot(curve, snapshot);
    }

    let metrics = cache.metrics();
    assert_eq!(
        metrics
            .snapshot_updates
            .load(std::sync::atomic::Ordering::Relaxed),
        10,
        "Should track all snapshot updates"
    );
}

/// Test that telemetry correctly counts swap events
#[test]
fn test_cache_telemetry_swap_counts() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    for i in 0..20 {
        let event = EarlySwapEvent::new(i * 100_000_000, i % 2 == 0, i);
        cache.update_swap(curve, event);
    }

    let metrics = cache.metrics();
    assert_eq!(metrics.total_swaps(), 20, "Should track all swap events");
}

/// Test that cache reports percentage of curves with valid reserves
#[test]
fn test_cache_telemetry_valid_reserves() {
    let cache = PumpCurveStateCache::new();

    // Add 3 curves with valid reserves
    for i in 0..3 {
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(i));
        cache.update_snapshot(curve, snapshot);
    }

    // Add 2 curves with zero reserves (invalid)
    for i in 0..2 {
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(0, 0, 100, Some(i));
        cache.update_snapshot(curve, snapshot);
    }

    let metrics = cache.metrics();
    assert_eq!(
        metrics
            .valid_reserves_updates
            .load(std::sync::atomic::Ordering::Relaxed),
        3,
        "Should track snapshot updates with valid reserves"
    );
}

/// Test that cache size matches actual entries
#[test]
fn test_cache_size_tracking() {
    let cache = PumpCurveStateCache::new();

    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());

    // Add 5 different curves
    for _ in 0..5 {
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(1));
        cache.update_snapshot(curve, snapshot);
    }

    assert_eq!(cache.len(), 5);
    assert!(!cache.is_empty());
}

/// Test that cache clear removes all entries
#[test]
fn test_cache_clear() {
    let cache = PumpCurveStateCache::new();

    // Add multiple curves
    for _ in 0..10 {
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(1));
        cache.update_snapshot(curve, snapshot);
    }

    assert_eq!(cache.len(), 10);

    cache.clear();

    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

/// Test that configurable fee_bps is respected (no magic numbers)
#[test]
fn test_cache_configurable_fee_bps() {
    let cache = PumpCurveStateCache::new();

    // Test various fee configurations
    let test_fees = vec![50, 100, 150, 200, 300];

    for (i, &fee) in test_fees.iter().enumerate() {
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, fee, Some(i as u64));
        cache.update_snapshot(curve, snapshot);

        let retrieved = cache.get_snapshot(&curve).unwrap();
        assert_eq!(
            retrieved.fee_bps, fee,
            "Fee should be configurable, not hardcoded"
        );
    }
}

/// Benchmark-style test: verify insert performance target <50μs
/// Marked as ignored for CI stability - use criterion bench instead
#[test]
#[ignore]
fn test_cache_insert_performance() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    let start = std::time::Instant::now();
    let iterations = 1000;

    for i in 0..iterations {
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(i));
        cache.update_snapshot(curve, snapshot.clone());
    }

    let elapsed = start.elapsed();
    let avg_micros = elapsed.as_micros() / iterations as u128;

    println!("Average insert time: {}μs", avg_micros);

    // Should be well under 50μs per operation
    assert!(
        avg_micros < 50,
        "Insert should be <50μs, got {}μs",
        avg_micros
    );
}

/// Test that swaps can be added before snapshot arrives (critical for gRPC events)
#[test]
fn test_swap_before_snapshot_integration() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Simulate gRPC swap events arriving before snapshot
    cache.update_swap(curve, EarlySwapEvent::new(100_000_000, true, 1));
    cache.update_swap(curve, EarlySwapEvent::new(200_000_000, false, 2));
    cache.update_swap(curve, EarlySwapEvent::new(300_000_000, true, 3));

    // Snapshot should NOT exist yet
    assert!(
        cache.get_snapshot(&curve).is_none(),
        "Snapshot must be None before update_snapshot is called"
    );

    // Swaps should still be available
    let swaps = cache.get_early_swaps(&curve);
    assert_eq!(
        swaps.len as usize, 3,
        "Swaps must be available even without snapshot"
    );

    // get_state should return None (no snapshot)
    assert!(
        cache.get_state(&curve).is_none(),
        "get_state must return None when snapshot not yet received"
    );

    // Now snapshot arrives from gRPC
    let snapshot = CurveSnapshot::new(5_000_000_000, 10_000_000_000_000, 100, Some(10));
    cache.update_snapshot(curve, snapshot);

    // Now everything should work
    assert!(
        cache.get_snapshot(&curve).is_some(),
        "Snapshot should exist"
    );
    let (snap, swaps) = cache.get_state(&curve).expect("get_state should work");
    assert_eq!(snap.virtual_sol_reserves_lamports, 5_000_000_000);
    assert_eq!(swaps.len as usize, 3);
}

/// Test that NO magic number 100 appears without real snapshot
#[test]
fn test_no_magic_fee_without_snapshot_integration() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Add swap events only
    cache.update_swap(curve, EarlySwapEvent::new(100_000_000, true, 1));

    // get_snapshot MUST return None - not a dummy with hardcoded fee=100
    let result = cache.get_snapshot(&curve);
    assert!(
        result.is_none(),
        "No dummy snapshot with magic constants should be created"
    );

    // Even after multiple swaps
    cache.update_swap(curve, EarlySwapEvent::new(200_000_000, false, 2));
    cache.update_swap(curve, EarlySwapEvent::new(300_000_000, true, 3));

    assert!(
        cache.get_snapshot(&curve).is_none(),
        "Still no dummy snapshot after multiple swaps"
    );
}

/// Test that swap events within 2-second window are retrievable
#[test]
fn test_swap_event_2s_window_retrievability() {
    let cache = PumpCurveStateCache::new();
    let curve = Pubkey::new_unique();

    // Add event at t=0
    let event1 = EarlySwapEvent::new(100_000_000, true, 1);
    cache.update_swap(curve, event1);

    // Wait 1 second
    thread::sleep(Duration::from_secs(1));

    // Add another event at t=1s
    let event2 = EarlySwapEvent::new(200_000_000, false, 2);
    cache.update_swap(curve, event2);

    // Both should still be valid (within 2s window)
    let swaps = cache.get_early_swaps(&curve);
    assert_eq!(
        swaps.len as usize, 2,
        "Both events should be valid within 2s window"
    );
}
