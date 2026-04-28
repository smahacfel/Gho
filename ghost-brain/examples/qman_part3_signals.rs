//! Example: QMAN Part 3 - Signal Detection & Trading Signals
//!
//! Demonstrates the complete QMAN pipeline:
//! - Part 1 (WEST): Track wallet energy states
//! - Part 2 (Matrix Engine): Learn transition patterns and predict flows
//! - Part 3 (Signal Detector): Generate trading signals
//!
//! Run with: cargo run --example qman_part3_signals

use ghost_brain::oracle::{
    SignalDetector, TransitionMatrix, UnitaryEvolution, WalletEnergyTracker,
};
use solana_sdk::pubkey::Pubkey;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("=== QMAN Part 3: Signal Detection Example ===\n");

    // Initialize all QMAN components
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new()
        .with_horizon(5000) // Predict 5 seconds ahead
        .with_min_confidence(0.3);
    let signal_detector = SignalDetector::new();

    // Simulate some tokens and pools
    let pool_doge = Pubkey::new_unique();
    let pool_pepe = Pubkey::new_unique();
    let pool_bonk = Pubkey::new_unique();

    let token_doge = Pubkey::new_unique();
    let token_pepe = Pubkey::new_unique();
    let token_bonk = Pubkey::new_unique();

    println!("Tokens:");
    println!("  DOGE: {}", token_doge);
    println!("  PEPE: {}", token_pepe);
    println!("  BONK: {}", token_bonk);

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Scenario 1: Initial accumulation in DOGE
    println!("\n=== Phase 1: Accumulation in DOGE ===");
    for i in 0..15 {
        let wallet = Pubkey::new_unique();
        let timestamp = current_time + (i * 100);

        west_tracker.process_transaction(
            pool_doge, wallet, token_doge, true, // buy
            5.0, timestamp,
        );

        transition_matrix.observe_transition(wallet, None, Some(token_doge), 5.0);
    }

    // Check signal after Phase 1
    transition_matrix.update();
    let state_1 = west_tracker.get_state_vector();
    let matrix_1 = transition_matrix.get_matrix();

    if let Some(prediction_1) = unitary_evolution.predict(&state_1, &matrix_1) {
        let signal_1 = signal_detector.analyze(&state_1, &prediction_1);

        println!("Signal: {:?}", signal_1.signal);
        println!("Confidence: {:.2}", signal_1.confidence);
        println!("Reason: {}", signal_1.reason);

        if !signal_1.forecasts.is_empty() {
            println!("Top forecast:");
            let top = &signal_1.forecasts[0];
            println!("  Token: {}", top.target_token);
            println!("  Net flow: {:.2}", top.net_energy_flow);
            println!(
                "  Second wave probability: {:.0}%",
                top.second_wave_probability * 100.0
            );
        }
    }

    // Scenario 2: Rotation from DOGE to PEPE
    println!("\n=== Phase 2: Rotation DOGE → PEPE ===");
    for i in 0..10 {
        let wallet = Pubkey::new_unique();
        let timestamp = current_time + 2000 + (i * 100);

        // Sell DOGE
        transition_matrix.observe_transition(wallet, Some(token_doge), None, 4.0);

        // Buy PEPE
        west_tracker.process_transaction(pool_pepe, wallet, token_pepe, true, 4.0, timestamp + 10);

        transition_matrix.observe_transition(wallet, None, Some(token_pepe), 4.0);
    }

    // Check signal after Phase 2
    transition_matrix.update();
    let state_2 = west_tracker.get_state_vector();
    let matrix_2 = transition_matrix.get_matrix();

    if let Some(prediction_2) = unitary_evolution.predict(&state_2, &matrix_2) {
        let signal_2 = signal_detector.analyze(&state_2, &prediction_2);

        println!("Signal: {:?}", signal_2.signal);
        println!("Confidence: {:.2}", signal_2.confidence);
        println!("Reason: {}", signal_2.reason);

        println!("\nMigration Forecasts:");
        for (idx, forecast) in signal_2.forecasts.iter().take(3).enumerate() {
            println!("  {}. Token {}", idx + 1, forecast.target_token);
            println!("     Net flow: {:+.2}", forecast.net_energy_flow);
            println!(
                "     Second wave prob: {:.0}%",
                forecast.second_wave_probability * 100.0
            );
        }
    }

    // Scenario 3: Hyper-bubble - convergence on BONK
    println!("\n=== Phase 3: Hyper-Bubble on BONK ===");
    for i in 0..25 {
        let wallet = Pubkey::new_unique();
        let timestamp = current_time + 4000 + (i * 50);

        west_tracker.process_transaction(pool_bonk, wallet, token_bonk, true, 6.0, timestamp);

        transition_matrix.observe_transition(wallet, None, Some(token_bonk), 6.0);
    }

    // Check signal after Phase 3
    transition_matrix.update();
    let state_3 = west_tracker.get_state_vector();
    let matrix_3 = transition_matrix.get_matrix();

    if let Some(prediction_3) = unitary_evolution.predict(&state_3, &matrix_3) {
        let signal_3 = signal_detector.analyze(&state_3, &prediction_3);

        println!("Signal: {:?}", signal_3.signal);
        println!("Confidence: {:.2}", signal_3.confidence);
        println!("Reason: {}", signal_3.reason);

        if let Some(target) = signal_3.hyper_bubble_target {
            println!("Hyper-bubble target: {}", target);
        }

        println!("\nAll Migration Forecasts:");
        for forecast in &signal_3.forecasts {
            println!(
                "  Token {}: flow={:+.2}, second_wave={:.0}%",
                forecast.target_token,
                forecast.net_energy_flow,
                forecast.second_wave_probability * 100.0
            );
        }
    }

    // Summary
    println!("\n=== Summary ===");
    println!("QMAN Part 3 demonstrates:");
    println!("  ✓ Second Wave Detection - re-accumulation signals");
    println!("  ✓ Capital Drain Detection - exit signals");
    println!("  ✓ Hyper-Bubble Detection - convergence signals");
    println!("  ✓ Migration Forecasts - per-token flow predictions");

    println!("\nSignal Types:");
    println!("  - PrepareSecondWave: Re-accumulation phase detected");
    println!("  - ExitNow: Capital drain/distribution detected");
    println!("  - AllInMainTrend: Hyper-bubble convergence detected");
    println!("  - Hold: No clear signal - normal conditions");

    println!("\nIntegration:");
    println!("  1. Use WEST to track wallet energy states");
    println!("  2. Use TransitionMatrix to learn capital flow patterns");
    println!("  3. Use UnitaryEvolution to predict future distribution");
    println!("  4. Use SignalDetector to generate actionable trading signals");
}
