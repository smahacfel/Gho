//! Integration tests for QMAN (Quantum Market Analysis Network)
//!
//! Tests the complete flow:
//! 1. WEST tracks wallet states and energy
//! 2. TransitionMatrix learns wallet movements
//! 3. UnitaryEvolution predicts future capital distribution

use ghost_brain::oracle::{TransitionMatrix, UnitaryEvolution, WalletEnergyTracker};
use solana_sdk::pubkey::Pubkey;

fn test_pubkey(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

#[test]
fn test_qman_full_integration() {
    // Setup: Create WEST tracker and QMAN components
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();

    let pool_a = test_pubkey(100);
    let pool_b = test_pubkey(101);
    let token_a = test_pubkey(1);
    let token_b = test_pubkey(2);

    // Simulate wallet activity: wallets buying token A
    for i in 10..20 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 100);

        // Buy token A
        west_tracker.process_transaction(
            pool_a, wallet, token_a, true, // is_buy
            5.0,  // volume_sol
            timestamp,
        );

        // Record transition: free -> token A
        transition_matrix.observe_transition(wallet, None, Some(token_a), 5.0);
    }

    // Simulate some wallets moving from A -> B
    for i in 10..15 {
        let wallet = test_pubkey(i);
        let timestamp = 2000 + (i as u64 * 100);

        // Sell token A
        west_tracker.process_transaction(
            pool_a, wallet, token_a, false, // is_sell
            4.0, timestamp,
        );

        // Buy token B
        west_tracker.process_transaction(
            pool_b,
            wallet,
            token_b,
            true, // is_buy
            4.0,
            timestamp + 10,
        );

        // Record transition: token A -> free -> token B
        transition_matrix.observe_transition(wallet, Some(token_a), None, 4.0);
        transition_matrix.observe_transition(wallet, None, Some(token_b), 4.0);
    }

    // Update transition matrix
    transition_matrix.update();

    // Get current state from WEST
    let current_state = west_tracker.get_state_vector();

    // Verify current state has energy distributed
    assert!(current_state.total_energy > 0.0, "Should have total energy");
    assert!(
        current_state.active_wallets > 0,
        "Should have active wallets"
    );

    println!("Current state:");
    println!("  Total energy: {}", current_state.total_energy);
    println!("  Free energy: {}", current_state.free_energy);
    println!("  Active wallets: {}", current_state.active_wallets);
    for (token, energy) in &current_state.token_energies {
        println!("  Token {:?}: {}", token, energy);
    }

    // Predict future state
    let matrix = transition_matrix.get_matrix();
    let prediction = unitary_evolution.predict(&current_state, &matrix);

    assert!(prediction.is_some(), "Should generate prediction");
    let prediction = prediction.unwrap();

    println!("\nPrediction (T+5s):");
    println!(
        "  Total energy: {} (change: {:.2}%)",
        prediction.total_energy,
        ((prediction.total_energy - current_state.total_energy) / current_state.total_energy)
            * 100.0
    );
    println!("  Confidence: {:.2}", prediction.confidence);
    println!("  Top flows:");
    for (token, predicted_energy, change) in &prediction.top_flows {
        println!(
            "    {:?}: {:.2} (change: {:+.2})",
            token, predicted_energy, change
        );
    }

    // Verify prediction properties
    assert!(
        prediction.confidence >= 0.3,
        "Should have reasonable confidence"
    );
    assert!(
        prediction.total_energy > 0.0,
        "Predicted energy should be positive"
    );

    // Verify energy is roughly conserved (within 20% due to limited data)
    let energy_diff_pct = ((prediction.total_energy - current_state.total_energy).abs()
        / current_state.total_energy)
        * 100.0;
    assert!(energy_diff_pct < 20.0, "Energy should be roughly conserved");
}

#[test]
fn test_qman_capital_flow_prediction() {
    // Setup
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();

    let _pool_a = test_pubkey(100);
    let pool_b = test_pubkey(101);
    let _token_a = test_pubkey(1);
    let token_b = test_pubkey(2);

    // Strong trend: Many wallets moving free -> token B
    for i in 10..30 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 50);

        // Buy token B
        west_tracker.process_transaction(pool_b, wallet, token_b, true, 10.0, timestamp);

        transition_matrix.observe_transition(wallet, None, Some(token_b), 10.0);
    }

    // Update matrix
    transition_matrix.update();

    // Get current state
    let current_state = west_tracker.get_state_vector();

    // Predict
    let matrix = transition_matrix.get_matrix();
    let prediction = unitary_evolution
        .predict(&current_state, &matrix)
        .expect("Should have prediction");

    // Token B should have significant predicted energy
    let token_b_predicted = prediction.get_predicted_energy(&Some(token_b));
    assert!(
        token_b_predicted > 0.0,
        "Token B should have predicted energy"
    );

    println!("Capital flow prediction test:");
    println!(
        "  Token B current energy: {}",
        current_state
            .token_energies
            .get(&token_b)
            .copied()
            .unwrap_or(0.0)
    );
    println!("  Token B predicted energy: {}", token_b_predicted);
    println!("  Confidence: {}", prediction.confidence);
}

#[test]
fn test_qman_threshold_filtering() {
    // Setup
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();

    let pool_a = test_pubkey(100);
    let token_a = test_pubkey(1);

    // Create significant activity
    for i in 10..25 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 50);

        west_tracker.process_transaction(pool_a, wallet, token_a, true, 8.0, timestamp);

        transition_matrix.observe_transition(wallet, None, Some(token_a), 8.0);
    }

    transition_matrix.update();
    let current_state = west_tracker.get_state_vector();
    let matrix = transition_matrix.get_matrix();

    // Test threshold filtering
    let threshold = 50.0;
    let above_threshold =
        unitary_evolution.predict_above_threshold(&current_state, &matrix, threshold);

    if let Some(tokens) = above_threshold {
        println!("Tokens above {} energy threshold:", threshold);
        for (token, energy) in tokens {
            println!("  {:?}: {}", token, energy);
        }
    }
}

#[test]
fn test_qman_performance() {
    use std::time::Instant;

    // Setup with moderate data
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();

    let pool_a = test_pubkey(100);
    let token_a = test_pubkey(1);

    // Add data
    for i in 10..30 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 50);

        west_tracker.process_transaction(pool_a, wallet, token_a, true, 5.0, timestamp);

        transition_matrix.observe_transition(wallet, None, Some(token_a), 5.0);
    }

    transition_matrix.update();
    let current_state = west_tracker.get_state_vector();
    let matrix = transition_matrix.get_matrix();

    // Benchmark prediction time
    let start = Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _ = unitary_evolution.predict(&current_state, &matrix);
    }

    let elapsed = start.elapsed();
    let avg_time = elapsed.as_micros() / iterations;

    println!("Performance test:");
    println!("  {} iterations", iterations);
    println!("  Total time: {:?}", elapsed);
    println!("  Average time per prediction: {}μs", avg_time);

    // Should be fast (target: <1ms = 1000μs)
    assert!(avg_time < 1000, "Prediction should be fast (<1ms)");
}

#[test]
fn test_qman_multi_token_dynamics() {
    // Test with multiple tokens and complex transitions
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();

    let pool_a = test_pubkey(100);
    let pool_b = test_pubkey(101);
    let pool_c = test_pubkey(102);

    let token_a = test_pubkey(1);
    let token_b = test_pubkey(2);
    let token_c = test_pubkey(3);

    // Complex flow: free -> A -> B, free -> C
    for i in 10..20 {
        let wallet = test_pubkey(i);
        let t = 1000 + (i as u64 * 100);

        // Buy A
        west_tracker.process_transaction(pool_a, wallet, token_a, true, 10.0, t);
        transition_matrix.observe_transition(wallet, None, Some(token_a), 10.0);

        // Later: A -> B
        if i < 15 {
            west_tracker.process_transaction(pool_a, wallet, token_a, false, 8.0, t + 50);
            west_tracker.process_transaction(pool_b, wallet, token_b, true, 8.0, t + 60);
            transition_matrix.observe_transition(wallet, Some(token_a), None, 8.0);
            transition_matrix.observe_transition(wallet, None, Some(token_b), 8.0);
        }
    }

    // Another group: free -> C
    for i in 20..25 {
        let wallet = test_pubkey(i);
        let t = 1500 + (i as u64 * 100);

        west_tracker.process_transaction(pool_c, wallet, token_c, true, 6.0, t);
        transition_matrix.observe_transition(wallet, None, Some(token_c), 6.0);
    }

    transition_matrix.update();
    let current_state = west_tracker.get_state_vector();
    let matrix = transition_matrix.get_matrix();

    let prediction = unitary_evolution
        .predict(&current_state, &matrix)
        .expect("Should have prediction");

    println!("Multi-token dynamics:");
    println!("  States tracked: {}", matrix.num_states());
    println!("  Current tokens: {}", current_state.token_energies.len());
    println!("  Predicted flows:");
    for (token, predicted, change) in &prediction.top_flows {
        println!("    {:?}: {:.2} ({:+.2})", token, predicted, change);
    }

    // Should track multiple tokens
    assert!(matrix.num_states() >= 3, "Should track multiple states");
}
