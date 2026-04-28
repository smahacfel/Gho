//! Integration Test: WHF Part 2 with WEST
//!
//! This test demonstrates the complete integration between WEST (Wallet Energy & State Tracker)
//! and WHF Part 2 (Harmonic & Field Analysis).

use ghost_brain::chaos::HarmonicFieldAnalyzer;
use ghost_brain::oracle::wallet_energy_tracker::WalletEnergyTracker;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

/// Test buffer size (smaller than default for faster tests)
const TEST_BUFFER_SIZE: usize = 64;

#[test]
fn test_whf_integration_with_west() {
    // Create WEST tracker
    let tracker = WalletEnergyTracker::new();

    // Create WHF analyzer
    let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);

    // Simulate trading activity
    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let wallet1 = Pubkey::new_unique();
    let wallet2 = Pubkey::new_unique();

    let mut time = 1000000u64;

    // First snapshot
    tracker.process_transaction(pool, wallet1, token, true, 100.0, time);
    time += 1000;
    tracker.process_transaction(pool, wallet2, token, true, 150.0, time);
    time += 1000;

    // Extract wallet data
    let cache = tracker.get_wallet_cache_snapshot();
    let mut wallet_data = HashMap::new();
    for (pk, particle) in cache.iter() {
        wallet_data.insert(
            *pk,
            (
                particle.energy as f64,
                particle.current_token,
                particle.last_update_ms,
            ),
        );
    }

    // First analysis
    let analysis1 = analyzer.analyze(&wallet_data, time);

    // Verify analysis structure
    assert!(analysis1.curl >= 0.0 && analysis1.curl <= 1.0);
    assert!(analysis1.divergence >= -1.0 && analysis1.divergence <= 1.0);
    assert!(analysis1.resonance_score >= 0.0 && analysis1.resonance_score <= 1.0);
    assert_eq!(analysis1.timestamp_ms, time);

    // Second snapshot with more activity
    tracker.process_transaction(pool, wallet1, token, false, 80.0, time);
    time += 1000;
    tracker.process_transaction(pool, wallet2, token, true, 200.0, time);
    time += 1000;

    let cache2 = tracker.get_wallet_cache_snapshot();
    let mut wallet_data2 = HashMap::new();
    for (pk, particle) in cache2.iter() {
        wallet_data2.insert(
            *pk,
            (
                particle.energy as f64,
                particle.current_token,
                particle.last_update_ms,
            ),
        );
    }

    // Second analysis
    let analysis2 = analyzer.analyze(&wallet_data2, time);

    // Divergence should reflect the net flow (buying pressure)
    assert!(analysis2.divergence > -1.0 && analysis2.divergence < 1.0);
}

#[test]
fn test_whf_accumulation_detection() {
    let tracker = WalletEnergyTracker::new();
    let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();

    let mut time = 1000000u64;

    // Create multiple wallets accumulating
    for _i in 0..5 {
        let wallet = Pubkey::new_unique();
        // Initial state
        tracker.process_transaction(pool, wallet, token, false, 50.0, time);
        time += 1000;
    }

    let cache1 = tracker.get_wallet_cache_snapshot();
    let mut data1 = HashMap::new();
    for (pk, p) in cache1.iter() {
        data1.insert(*pk, (p.energy as f64, p.current_token, p.last_update_ms));
    }

    analyzer.analyze(&data1, time);

    // All wallets buy (accumulation)
    for (wallet, _) in cache1.iter() {
        tracker.process_transaction(pool, *wallet, token, true, 100.0, time);
        time += 1000;
    }

    let cache2 = tracker.get_wallet_cache_snapshot();
    let mut data2 = HashMap::new();
    for (pk, p) in cache2.iter() {
        data2.insert(*pk, (p.energy as f64, p.current_token, p.last_update_ms));
    }

    let result = analyzer.analyze(&data2, time);

    // Should show positive divergence (accumulation)
    assert!(
        result.divergence > 0.0,
        "Expected positive divergence for accumulation"
    );
}

#[test]
fn test_whf_periodic_trading_detection() {
    let tracker = WalletEnergyTracker::new();
    let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let bot = Pubkey::new_unique();

    let mut time = 1000000u64;
    let mut all_timestamps = Vec::new();

    // Simulate bot trading with perfect periodicity (every 100ms)
    // Need multiple analyze calls to build up timestamp buffer
    for i in 0..32 {
        let is_buy = i % 2 == 0;
        tracker.process_transaction(pool, bot, token, is_buy, 10.0, time);
        all_timestamps.push(time);
        time += 100; // Perfect 100ms intervals

        // Analyze periodically to build up buffer
        if i % 4 == 3 {
            let cache = tracker.get_wallet_cache_snapshot();
            let mut data = HashMap::new();
            for (pk, p) in cache.iter() {
                data.insert(*pk, (p.energy as f64, p.current_token, p.last_update_ms));
            }
            analyzer.analyze(&data, time);
        }
    }

    // Manually update timestamp buffer with all transaction times
    analyzer.update_timestamp_buffer(all_timestamps);

    let result = analyzer.calculate_resonance();

    // Should detect high resonance (periodic pattern)
    assert!(
        result > 0.5,
        "Expected high resonance for periodic pattern, got {}",
        result
    );
}

#[test]
fn test_whf_empty_data_handling() {
    let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);
    let data = HashMap::new();

    let result = analyzer.analyze(&data, 1000000);

    assert_eq!(result.curl, 0.0);
    assert_eq!(result.divergence, 0.0);
    assert_eq!(result.resonance_score, 0.0);
}
