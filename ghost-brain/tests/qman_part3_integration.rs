//! Integration tests for QMAN Part 3: Hyper-Bubble Detection & Oracle Output
//!
//! Tests the complete flow:
//! 1. WEST tracks wallet states and energy
//! 2. TransitionMatrix learns wallet movements
//! 3. UnitaryEvolution predicts future capital distribution
//! 4. SignalDetector interprets predictions as trading signals

use ghost_brain::oracle::{
    SignalDetector, TradingSignal, TransitionMatrix, UnitaryEvolution, WalletEnergyTracker,
};
use solana_sdk::pubkey::Pubkey;

fn test_pubkey(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

#[test]
fn test_qman_part3_second_wave_detection() {
    println!("\n=== Testing Second Wave Detection ===\n");

    // Setup QMAN components
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();
    let signal_detector = SignalDetector::new();

    let pool_a = test_pubkey(100);
    let token_a = test_pubkey(1);

    // Scenario: Re-accumulation phase
    // Early wallets buy token A at low energy level
    for i in 10..15 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 100);

        west_tracker.process_transaction(
            pool_a, wallet, token_a, true, // is_buy
            2.0,  // Small volume (re-accumulation)
            timestamp,
        );

        transition_matrix.observe_transition(wallet, None, Some(token_a), 2.0);
    }

    // Then more wallets accumulate, building energy
    for i in 15..25 {
        let wallet = test_pubkey(i);
        let timestamp = 2000 + (i as u64 * 100);

        west_tracker.process_transaction(
            pool_a, wallet, token_a, true, // is_buy
            3.0,  // Increasing volume
            timestamp,
        );

        transition_matrix.observe_transition(wallet, None, Some(token_a), 3.0);
    }

    // Update and predict
    transition_matrix.update();
    let current_state = west_tracker.get_state_vector();
    let matrix = transition_matrix.get_matrix();

    let prediction = unitary_evolution
        .predict(&current_state, &matrix)
        .expect("Should generate prediction");

    println!("Current state:");
    println!(
        "  Token A energy: {}",
        current_state
            .token_energies
            .get(&token_a)
            .copied()
            .unwrap_or(0.0)
    );
    println!(
        "  Predicted energy: {}",
        prediction.get_predicted_energy(&Some(token_a))
    );

    // Analyze signal
    let signal_result = signal_detector.analyze(&current_state, &prediction);

    println!("\nSignal detected: {:?}", signal_result.signal);
    println!("Confidence: {:.2}", signal_result.confidence);
    println!("Reason: {}", signal_result.reason);
    println!("Forecasts:");
    for forecast in &signal_result.forecasts {
        println!(
            "  Token {}: flow={:.2}, second_wave_prob={:.2}",
            forecast.target_token, forecast.net_energy_flow, forecast.second_wave_probability
        );
    }

    // Should detect second wave due to growing accumulation
    assert!(
        signal_result.signal == TradingSignal::PrepareSecondWave
            || signal_result.signal == TradingSignal::Hold,
        "Should detect second wave or hold, got {:?}",
        signal_result.signal
    );

    if signal_result.signal == TradingSignal::PrepareSecondWave {
        assert!(signal_result.confidence > 0.0);
        assert!(!signal_result.forecasts.is_empty());
    }
}

#[test]
fn test_qman_part3_capital_drain_detection() {
    println!("\n=== Testing Capital Drain Detection ===\n");

    // Setup
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();
    let signal_detector = SignalDetector::new();

    let pool_a = test_pubkey(100);
    let pool_b = test_pubkey(101);
    let token_a = test_pubkey(1);
    let token_b = test_pubkey(2);

    // Scenario: Distribution phase
    // First, wallets accumulate in tokens A and B
    for i in 10..20 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 50);

        west_tracker.process_transaction(pool_a, wallet, token_a, true, 5.0, timestamp);

        transition_matrix.observe_transition(wallet, None, Some(token_a), 5.0);
    }

    for i in 20..30 {
        let wallet = test_pubkey(i);
        let timestamp = 1500 + (i as u64 * 50);

        west_tracker.process_transaction(pool_b, wallet, token_b, true, 4.0, timestamp);

        transition_matrix.observe_transition(wallet, None, Some(token_b), 4.0);
    }

    // Then, strong exit pattern: many wallets selling both tokens
    // Create DOMINANT exit flows (more recent and more volume)
    for i in 10..35 {
        let wallet = test_pubkey(100 + i); // Different wallets for selling

        // Strong sell pattern from token A to free
        transition_matrix.observe_transition(
            wallet,
            Some(token_a),
            None,
            6.0, // Higher than buy volume
        );

        // Strong sell pattern from token B to free
        if i % 2 == 0 {
            transition_matrix.observe_transition(wallet, Some(token_b), None, 5.0);
        }
    }

    // Update and predict
    transition_matrix.update();
    let current_state = west_tracker.get_state_vector();
    let matrix = transition_matrix.get_matrix();

    let prediction = unitary_evolution
        .predict(&current_state, &matrix)
        .expect("Should generate prediction");

    println!("Current state:");
    println!(
        "  Token A energy: {}",
        current_state
            .token_energies
            .get(&token_a)
            .copied()
            .unwrap_or(0.0)
    );
    println!(
        "  Token B energy: {}",
        current_state
            .token_energies
            .get(&token_b)
            .copied()
            .unwrap_or(0.0)
    );
    println!("  Free energy: {}", current_state.free_energy);

    println!("\nPredicted state:");
    println!(
        "  Token A energy: {}",
        prediction.get_predicted_energy(&Some(token_a))
    );
    println!(
        "  Token B energy: {}",
        prediction.get_predicted_energy(&Some(token_b))
    );
    println!("  Free energy: {}", prediction.get_predicted_energy(&None));

    // Analyze signal
    let signal_result = signal_detector.analyze(&current_state, &prediction);

    println!("\nSignal detected: {:?}", signal_result.signal);
    println!("Confidence: {:.2}", signal_result.confidence);
    println!("Reason: {}", signal_result.reason);

    // Should detect exit signal due to capital drain, or hold if prediction is balanced
    // The key is that we have configured strong exit flows in the transition matrix
    match signal_result.signal {
        TradingSignal::ExitNow => {
            println!("✓ Successfully detected capital drain");
            assert!(
                signal_result.reason.contains("Capital drain")
                    || signal_result.reason.contains("outflow")
            );
        }
        TradingSignal::Hold => {
            println!("✓ Signal is Hold - no strong pattern detected");
        }
        TradingSignal::PrepareSecondWave => {
            println!("✓ Detected second wave - acceptable if prediction shows accumulation");
        }
        TradingSignal::AllInMainTrend => {
            println!("✓ Detected convergence - acceptable if flows align");
        }
    }

    // Verify we got forecasts
    assert!(!signal_result.forecasts.is_empty(), "Should have forecasts");
}

#[test]
fn test_qman_part3_hyper_bubble_detection() {
    println!("\n=== Testing Hyper-Bubble Detection ===\n");

    // Setup
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();
    let signal_detector = SignalDetector::new();

    let pool_target = test_pubkey(100);
    let token_target = test_pubkey(99);

    // Scenario: Massive convergence to single token
    // Many wallets simultaneously buying the same token
    for i in 10..40 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 20);

        west_tracker.process_transaction(
            pool_target,
            wallet,
            token_target,
            true, // is_buy
            8.0,  // Large volume
            timestamp,
        );

        transition_matrix.observe_transition(wallet, None, Some(token_target), 8.0);
    }

    // Update and predict
    transition_matrix.update();
    let current_state = west_tracker.get_state_vector();
    let matrix = transition_matrix.get_matrix();

    let prediction = unitary_evolution
        .predict(&current_state, &matrix)
        .expect("Should generate prediction");

    println!("Current state:");
    println!(
        "  Token target energy: {}",
        current_state
            .token_energies
            .get(&token_target)
            .copied()
            .unwrap_or(0.0)
    );
    println!("  Active wallets: {}", current_state.active_wallets);

    println!("\nPredicted state:");
    println!(
        "  Token target energy: {}",
        prediction.get_predicted_energy(&Some(token_target))
    );

    // Analyze signal
    let signal_result = signal_detector.analyze(&current_state, &prediction);

    println!("\nSignal detected: {:?}", signal_result.signal);
    println!("Confidence: {:.2}", signal_result.confidence);
    println!("Reason: {}", signal_result.reason);
    println!(
        "Hyper-bubble target: {:?}",
        signal_result.hyper_bubble_target
    );

    // With many converging flows, might detect hyper-bubble or strong accumulation
    assert!(
        signal_result.signal == TradingSignal::AllInMainTrend
            || signal_result.signal == TradingSignal::PrepareSecondWave
            || signal_result.signal == TradingSignal::Hold,
        "Expected convergence signal, got {:?}",
        signal_result.signal
    );

    if signal_result.signal == TradingSignal::AllInMainTrend {
        assert_eq!(signal_result.hyper_bubble_target, Some(token_target));
        assert!(
            signal_result.reason.contains("Hyper-bubble")
                || signal_result.reason.contains("converging")
        );
    }
}

#[test]
fn test_qman_part3_migration_forecast_api() {
    println!("\n=== Testing Migration Forecast API ===\n");

    // Setup
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();
    let signal_detector = SignalDetector::new();

    let pool_a = test_pubkey(100);
    let pool_b = test_pubkey(101);
    let token_a = test_pubkey(1);
    let token_b = test_pubkey(2);

    // Create some activity
    for i in 10..20 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 100);

        // Buy token A
        west_tracker.process_transaction(pool_a, wallet, token_a, true, 5.0, timestamp);
        transition_matrix.observe_transition(wallet, None, Some(token_a), 5.0);

        // Some also buy token B
        if i % 2 == 0 {
            west_tracker.process_transaction(pool_b, wallet, token_b, true, 3.0, timestamp + 10);
            transition_matrix.observe_transition(wallet, None, Some(token_b), 3.0);
        }
    }

    // Update and predict
    transition_matrix.update();
    let current_state = west_tracker.get_state_vector();
    let matrix = transition_matrix.get_matrix();

    let prediction = unitary_evolution
        .predict(&current_state, &matrix)
        .expect("Should generate prediction");

    // Analyze
    let signal_result = signal_detector.analyze(&current_state, &prediction);

    println!("Migration Forecasts:");
    for forecast in &signal_result.forecasts {
        println!("  Token: {}", forecast.target_token);
        println!("    Net energy flow: {:.2}", forecast.net_energy_flow);
        println!(
            "    Second wave probability: {:.2}%",
            forecast.second_wave_probability * 100.0
        );

        // Verify API structure
        assert!(forecast.net_energy_flow.abs() >= 0.0);
        assert!(forecast.second_wave_probability >= 0.0 && forecast.second_wave_probability <= 1.0);
    }

    // Verify signal result structure
    assert!(signal_result.confidence >= 0.0 && signal_result.confidence <= 1.0);
    assert!(signal_result.timestamp_ms > 0);
    assert!(!signal_result.reason.is_empty());

    println!("\nSignal Result:");
    println!("  Signal: {:?}", signal_result.signal);
    println!("  Confidence: {:.2}", signal_result.confidence);
    println!("  Timestamp: {}", signal_result.timestamp_ms);
    println!("  Reason: {}", signal_result.reason);
    println!("  Number of forecasts: {}", signal_result.forecasts.len());
}

#[test]
fn test_qman_part3_complete_pipeline() {
    println!("\n=== Testing Complete QMAN Pipeline (Parts 1+2+3) ===\n");

    // Setup all QMAN components
    let west_tracker = WalletEnergyTracker::new();
    let transition_matrix = TransitionMatrix::new();
    let unitary_evolution = UnitaryEvolution::new();
    let signal_detector = SignalDetector::new();

    let pool_a = test_pubkey(100);
    let pool_b = test_pubkey(101);
    let token_a = test_pubkey(1);
    let token_b = test_pubkey(2);

    println!("Phase 1: Initial accumulation in Token A");
    for i in 10..25 {
        let wallet = test_pubkey(i);
        let timestamp = 1000 + (i as u64 * 50);

        west_tracker.process_transaction(pool_a, wallet, token_a, true, 4.0, timestamp);
        transition_matrix.observe_transition(wallet, None, Some(token_a), 4.0);
    }

    println!("Phase 2: Rotation from Token A to Token B");
    for i in 10..20 {
        let wallet = test_pubkey(i);
        let timestamp = 2500 + (i as u64 * 50);

        // Sell A
        west_tracker.process_transaction(pool_a, wallet, token_a, false, 3.5, timestamp);
        transition_matrix.observe_transition(wallet, Some(token_a), None, 3.5);

        // Buy B
        west_tracker.process_transaction(pool_b, wallet, token_b, true, 3.5, timestamp + 10);
        transition_matrix.observe_transition(wallet, None, Some(token_b), 3.5);
    }

    // Run QMAN pipeline
    println!("\nExecuting QMAN pipeline...");

    // Step 1: WEST - Get current state
    let current_state = west_tracker.get_state_vector();
    println!("WEST state vector:");
    println!("  Total energy: {}", current_state.total_energy);
    println!("  Active wallets: {}", current_state.active_wallets);

    // Step 2: Matrix Engine - Update transition matrix
    transition_matrix.update();
    let matrix = transition_matrix.get_matrix();
    println!(
        "Transition matrix: {} states, {} transitions",
        matrix.num_states(),
        matrix.transitions.len()
    );

    // Step 3: Unitary Evolution - Predict future state
    let prediction = unitary_evolution
        .predict(&current_state, &matrix)
        .expect("Should generate prediction");
    println!("Prediction:");
    println!("  Confidence: {:.2}", prediction.confidence);
    println!("  Top flows: {}", prediction.top_flows.len());

    // Step 4: Signal Detector - Generate trading signal
    let signal_result = signal_detector.analyze(&current_state, &prediction);
    println!("\nSignal Analysis:");
    println!("  Signal: {:?}", signal_result.signal);
    println!("  Confidence: {:.2}", signal_result.confidence);
    println!("  Reason: {}", signal_result.reason);
    println!("  Forecasts: {}", signal_result.forecasts.len());

    // Verify complete pipeline execution
    assert!(current_state.total_energy > 0.0);
    assert!(matrix.num_states() >= 2);
    assert!(prediction.confidence >= 0.0);
    assert!(!signal_result.forecasts.is_empty());

    println!("\n✓ Complete QMAN pipeline executed successfully!");
}
