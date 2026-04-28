//! Example: QMAN Capital Flow Prediction
//!
//! Demonstrates how to use QMAN Part 2 (Unitary Evolution & Matrix Engine)
//! to predict capital flow between tokens.
//!
//! Run with: cargo run --example qman_prediction

use ghost_brain::oracle::{TransitionMatrix, UnitaryEvolution, WalletEnergyTracker};
use solana_sdk::pubkey::Pubkey;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("=== QMAN Capital Flow Prediction Example ===\n");

    // Create the QMAN components
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new()
        .with_horizon(5000) // Predict 5 seconds ahead
        .with_min_confidence(0.3); // Minimum 30% confidence

    // Simulate some pools and tokens
    let pool_pump = Pubkey::new_unique();
    let pool_ray = Pubkey::new_unique();

    let token_doge = Pubkey::new_unique();
    let token_pepe = Pubkey::new_unique();

    println!("Pools:");
    println!("  Pump.fun: {}", pool_pump);
    println!("  Raydium:  {}", pool_ray);

    println!("\nTokens:");
    println!("  DOGE: {}", token_doge);
    println!("  PEPE: {}", token_pepe);

    // Simulate wallet activity
    println!("\n=== Simulating Wallet Activity ===\n");

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Phase 1: Wallets buying DOGE
    println!("Phase 1: 10 wallets buy DOGE (5 SOL each)");
    for i in 0..10 {
        let wallet = Pubkey::new_unique();
        let timestamp = current_time + (i * 100);

        west_tracker.process_transaction(
            pool_pump, wallet, token_doge, true, // is_buy
            5.0,  // 5 SOL
            timestamp,
        );

        transition_matrix.observe_transition(
            wallet,
            None,             // from: free liquidity
            Some(token_doge), // to: DOGE
            5.0,
        );
    }

    // Phase 2: Some wallets switch from DOGE to PEPE
    println!("Phase 2: 5 wallets sell DOGE and buy PEPE");
    for i in 0..5 {
        let wallet = Pubkey::new_unique();
        let timestamp = current_time + 1000 + (i * 100);

        // Sell DOGE
        west_tracker.process_transaction(
            pool_pump, wallet, token_doge, false, // is_sell
            4.0, timestamp,
        );

        // Buy PEPE
        west_tracker.process_transaction(
            pool_ray,
            wallet,
            token_pepe,
            true, // is_buy
            4.0,
            timestamp + 10,
        );

        // Record transitions
        transition_matrix.observe_transition(
            wallet,
            Some(token_doge), // from: DOGE
            None,             // to: free liquidity
            4.0,
        );

        transition_matrix.observe_transition(
            wallet,
            None,             // from: free liquidity
            Some(token_pepe), // to: PEPE
            4.0,
        );
    }

    // Phase 3: New wallets buying PEPE directly
    println!("Phase 3: 8 wallets buy PEPE directly (6 SOL each)");
    for i in 0..8 {
        let wallet = Pubkey::new_unique();
        let timestamp = current_time + 2000 + (i * 100);

        west_tracker.process_transaction(pool_ray, wallet, token_pepe, true, 6.0, timestamp);

        transition_matrix.observe_transition(
            wallet,
            None,             // from: free liquidity
            Some(token_pepe), // to: PEPE
            6.0,
        );
    }

    // Update the transition matrix
    transition_matrix.update();

    // Get current state from WEST
    let current_state = west_tracker.get_state_vector();

    println!("\n=== Current Market State ===\n");
    println!("Total Energy: {:.2} SOL", current_state.total_energy);
    println!("Free Energy: {:.2} SOL", current_state.free_energy);
    println!("Active Wallets: {}", current_state.active_wallets);

    println!("\nEnergy Distribution:");
    for (token, energy) in &current_state.token_energies {
        let token_name = if *token == token_doge {
            "DOGE"
        } else if *token == token_pepe {
            "PEPE"
        } else {
            "OTHER"
        };
        println!(
            "  {}: {:.2} SOL ({:.1}%)",
            token_name,
            energy,
            (energy / current_state.total_energy) * 100.0
        );
    }

    // Get transition matrix info
    let matrix = transition_matrix.get_matrix();
    println!("\n=== Transition Matrix ===\n");
    println!("States Tracked: {}", matrix.num_states());
    println!(
        "Transitions Observed: {}",
        transition_matrix.transition_count()
    );

    // Make prediction
    println!("\n=== Predicting Future State (T+5s) ===\n");

    match unitary_evolution.predict(&current_state, &matrix) {
        Some(prediction) => {
            println!("✓ Prediction Generated");
            println!("\nConfidence: {:.1}%", prediction.confidence * 100.0);
            println!("Prediction Horizon: {}ms", prediction.prediction_horizon_ms);
            println!("Total Predicted Energy: {:.2} SOL", prediction.total_energy);

            let energy_change_pct = ((prediction.total_energy - current_state.total_energy)
                / current_state.total_energy)
                * 100.0;
            println!("Energy Change: {:+.2}%", energy_change_pct);

            println!("\n--- Top Predicted Flows ---");
            for (i, (token_opt, predicted_energy, change)) in
                prediction.top_flows.iter().enumerate()
            {
                let token_name = match token_opt {
                    None => "Free Liquidity".to_string(),
                    Some(t) if *t == token_doge => "DOGE".to_string(),
                    Some(t) if *t == token_pepe => "PEPE".to_string(),
                    Some(t) => format!("Token {:.8}...", t.to_string()),
                };

                let flow_direction = if *change > 0.0 {
                    "↑ INFLOW"
                } else {
                    "↓ OUTFLOW"
                };

                println!(
                    "{}. {} - {:.2} SOL ({:+.2} SOL) {}",
                    i + 1,
                    token_name,
                    predicted_energy,
                    change,
                    flow_direction
                );
            }

            // Find token with highest predicted energy
            if let Some((winner, winner_energy)) = prediction.highest_energy_token() {
                println!("\n--- Prediction Summary ---");
                let winner_name = match winner {
                    None => "Free Liquidity".to_string(),
                    Some(t) if t == token_doge => "DOGE".to_string(),
                    Some(t) if t == token_pepe => "PEPE".to_string(),
                    Some(t) => format!("Token {:.8}...", t.to_string()),
                };

                println!(
                    "🎯 Highest Energy Token: {} ({:.2} SOL)",
                    winner_name, winner_energy
                );

                // Trading recommendation
                if winner == Some(token_pepe) {
                    println!("💡 Recommendation: Consider entering PEPE position");
                } else if winner == Some(token_doge) {
                    println!("💡 Recommendation: DOGE still has momentum");
                } else if winner.is_none() {
                    println!(
                        "⚠️  Warning: Capital flowing back to free liquidity (potential sell-off)"
                    );
                }
            }

            // Check for tokens above threshold
            let threshold = 20.0;
            let hot_tokens = prediction.tokens_above_threshold(threshold);

            if !hot_tokens.is_empty() {
                println!("\n--- Tokens Above {} SOL Threshold ---", threshold);
                for (token_opt, energy) in hot_tokens {
                    let token_name = match token_opt {
                        None => "Free Liquidity".to_string(),
                        Some(t) if t == token_doge => "DOGE".to_string(),
                        Some(t) if t == token_pepe => "PEPE".to_string(),
                        Some(t) => format!("Token {:.8}...", t.to_string()),
                    };
                    println!("  {} - {:.2} SOL", token_name, energy);
                }
            }
        }
        None => {
            println!("✗ Cannot generate prediction - insufficient data");
            println!("  Need more wallet activity and transitions");
        }
    }

    println!("\n=== Example Complete ===");
}
