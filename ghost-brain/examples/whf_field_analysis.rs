//! Integration Example: WHF Part 2 - Harmonic & Field Analysis
//!
//! This example demonstrates how to use the HarmonicFieldAnalyzer with
//! the WalletEnergyTracker to detect market manipulation patterns.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example whf_field_analysis
//! ```

use ghost_brain::chaos::{HarmonicFieldAnalysis, HarmonicFieldAnalyzer};
use ghost_brain::oracle::wallet_energy_tracker::WalletEnergyTracker;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

fn main() {
    println!("=== WHF Part 2: Harmonic & Field Analysis Example ===\n");

    // Create wallet energy tracker
    let tracker = WalletEnergyTracker::new();

    // Create field analyzer
    let mut analyzer = HarmonicFieldAnalyzer::new(128);

    // Simulate some trading activity
    println!("Simulating trading activity...\n");

    let pool_amm_id = Pubkey::new_unique();
    let token_mint = Pubkey::new_unique();

    // Create wallets for simulation
    let whale1 = Pubkey::new_unique();
    let whale2 = Pubkey::new_unique();
    let whale3 = Pubkey::new_unique();
    let bot1 = Pubkey::new_unique();
    let bot2 = Pubkey::new_unique();

    // Scenario 1: Normal trading (random intervals)
    println!("--- Scenario 1: Normal Trading ---");

    let mut time = 1000000u64;

    // Random trading activity
    tracker.process_transaction(pool_amm_id, whale1, token_mint, true, 10.0, time);
    time += 5432;
    tracker.process_transaction(pool_amm_id, whale2, token_mint, true, 15.0, time);
    time += 8123;
    tracker.process_transaction(pool_amm_id, whale3, token_mint, false, 8.0, time);
    time += 2456;
    tracker.process_transaction(pool_amm_id, whale1, token_mint, false, 5.0, time);
    time += 9876;
    tracker.process_transaction(pool_amm_id, whale2, token_mint, true, 20.0, time);

    // Extract wallet data for analysis
    let wallet_cache = tracker.get_wallet_cache_snapshot();
    let mut wallet_data = HashMap::new();

    for (pubkey, particle) in wallet_cache.iter() {
        wallet_data.insert(
            *pubkey,
            (
                particle.energy as f64,
                particle.current_token,
                particle.last_update_ms,
            ),
        );
    }

    let result1 = analyzer.analyze(&wallet_data, time);

    println!("  Curl (wash trading indicator): {:.3}", result1.curl);
    println!("  Divergence (flow direction): {:.3}", result1.divergence);
    println!("  Resonance (bot activity): {:.3}", result1.resonance_score);
    println!();

    // Scenario 2: Wash trading pattern (high curl)
    println!("--- Scenario 2: Wash Trading Pattern ---");

    time += 10000;

    // Create alternating buy-sell pattern between two wallets
    for i in 0..10 {
        if i % 2 == 0 {
            tracker.process_transaction(pool_amm_id, bot1, token_mint, true, 5.0, time);
            tracker.process_transaction(pool_amm_id, bot2, token_mint, false, 5.0, time);
        } else {
            tracker.process_transaction(pool_amm_id, bot1, token_mint, false, 5.0, time);
            tracker.process_transaction(pool_amm_id, bot2, token_mint, true, 5.0, time);
        }
        time += 1000; // Regular intervals
    }

    let wallet_cache2 = tracker.get_wallet_cache_snapshot();
    let mut wallet_data2 = HashMap::new();

    for (pubkey, particle) in wallet_cache2.iter() {
        wallet_data2.insert(
            *pubkey,
            (
                particle.energy as f64,
                particle.current_token,
                particle.last_update_ms,
            ),
        );
    }

    let result2 = analyzer.analyze(&wallet_data2, time);

    println!("  Curl (wash trading indicator): {:.3}", result2.curl);
    println!("  Divergence (flow direction): {:.3}", result2.divergence);
    println!("  Resonance (bot activity): {:.3}", result2.resonance_score);
    println!();

    // Scenario 3: Accumulation phase (positive divergence)
    println!("--- Scenario 3: Accumulation Phase ---");

    time += 10000;

    // Multiple wallets buying
    for _ in 0..5 {
        tracker.process_transaction(pool_amm_id, whale1, token_mint, true, 25.0, time);
        time += 2000;
        tracker.process_transaction(pool_amm_id, whale2, token_mint, true, 30.0, time);
        time += 2000;
        tracker.process_transaction(pool_amm_id, whale3, token_mint, true, 20.0, time);
        time += 2000;
    }

    let wallet_cache3 = tracker.get_wallet_cache_snapshot();
    let mut wallet_data3 = HashMap::new();

    for (pubkey, particle) in wallet_cache3.iter() {
        wallet_data3.insert(
            *pubkey,
            (
                particle.energy as f64,
                particle.current_token,
                particle.last_update_ms,
            ),
        );
    }

    let result3 = analyzer.analyze(&wallet_data3, time);

    println!("  Curl (wash trading indicator): {:.3}", result3.curl);
    println!("  Divergence (flow direction): {:.3}", result3.divergence);
    println!("  Resonance (bot activity): {:.3}", result3.resonance_score);
    println!();

    // Interpretation guide
    println!("=== Interpretation Guide ===");
    println!();
    println!("Curl:");
    println!("  < 0.3: Normal trading");
    println!("  0.3-0.6: Suspicious alternating patterns");
    println!("  > 0.6: High probability of wash trading");
    println!();
    println!("Divergence:");
    println!("  > 0.3: Strong accumulation (buying pressure)");
    println!("  -0.3 to 0.3: Balanced flow");
    println!("  < -0.3: Strong distribution (selling pressure)");
    println!();
    println!("Resonance:");
    println!("  > 0.7: Highly periodic (likely bot activity)");
    println!("  0.3-0.7: Some periodicity");
    println!("  < 0.3: Random trading (likely human)");
    println!();

    // Combined analysis
    println!("=== Combined Analysis ===");
    println!();

    for (scenario, result) in [
        ("Normal Trading", &result1),
        ("Wash Trading", &result2),
        ("Accumulation", &result3),
    ] {
        println!("{}:", scenario);

        let mut flags = Vec::new();

        if result.curl > 0.6 {
            flags.push("⚠️  WASH TRADING DETECTED");
        } else if result.curl > 0.3 {
            flags.push("⚠️  Suspicious alternating patterns");
        }

        if result.divergence > 0.3 {
            flags.push("📈 Strong accumulation");
        } else if result.divergence < -0.3 {
            flags.push("📉 Strong distribution");
        }

        if result.resonance_score > 0.7 {
            flags.push("🤖 High bot activity");
        } else if result.resonance_score > 0.3 {
            flags.push("🤖 Moderate bot activity");
        }

        if flags.is_empty() {
            println!("  ✅ Normal market activity");
        } else {
            for flag in flags {
                println!("  {}", flag);
            }
        }
        println!();
    }
}
