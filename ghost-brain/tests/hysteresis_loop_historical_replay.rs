//! Historical Replay Tests for Hysteresis Learning Loop
//!
//! This test simulates replaying historical trading data through the learning loop
//! to validate effectiveness and tuning parameters.

use ghost_brain::oracle::{DecisionType, InitialComponents};
use ghost_brain::tuning::{
    BanditAlgorithm, DecisionOutcome, HysteresisConfig, HysteresisLoop, TunableWeights,
    TuningContext,
};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

/// Simulated historical trade record
#[derive(Debug, Clone)]
struct HistoricalTrade {
    pool_id: String,
    initial_score: u8,
    initial_decision: DecisionType,
    profit_ratio: f32,
    was_successful: bool,
    elapsed_seconds: u64,
    context: TuningContext,
}

/// Generate synthetic historical data for testing
fn generate_synthetic_history(n_trades: usize) -> Vec<HistoricalTrade> {
    let mut history = Vec::new();

    for i in 0..n_trades {
        // Simulate varying market conditions
        let volatility = 0.3 + (i as f32 * 0.01) % 0.5;
        let volume = 0.5 + (i as f32 * 0.02) % 0.4;
        let mci = 0.6 + (i as f32 * 0.015) % 0.3;

        let context = TuningContext {
            volatility,
            volume,
            bot_activity: 0.3,
            mci,
            time_factor: 0.5,
            pool_age_seconds: 60 + i as u64,
        };

        // Simulate decision quality based on market conditions
        // Higher MCI and lower volatility = better outcomes
        let quality_factor = mci - volatility;
        let was_successful = quality_factor > 0.15;
        let profit_ratio = if was_successful {
            1.0 + quality_factor * 0.5 // 5-25% profit
        } else {
            1.0 - (0.15 - quality_factor) * 0.3 // 0-15% loss
        };

        let initial_decision = if quality_factor > 0.1 {
            DecisionType::Buy
        } else {
            DecisionType::Skip
        };

        history.push(HistoricalTrade {
            pool_id: format!("historical_pool_{}", i),
            initial_score: (60.0 + quality_factor * 100.0).min(90.0) as u8,
            initial_decision,
            profit_ratio,
            was_successful,
            elapsed_seconds: 45 + (i as u64 % 30),
            context,
        });
    }

    history
}

/// Helper to create test components
fn test_components() -> InitialComponents {
    InitialComponents {
        base_shadow: 65,
        qass_score: 75.0,
        qedd_survival_30s: Some(0.72),
        mci: Some(0.75),
        chaos_loss_prob: Some(0.1),
        gene_match_score: Some(0.05),
        confidence: Some(0.85),
        extras: HashMap::new(),
    }
}

#[test]
fn test_historical_replay_basic() {
    let config = HysteresisConfig {
        update_cooldown_ms: 10, // Fast updates for testing
        dampening_factor: 0.5,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let history = generate_synthetic_history(50);
    let components = test_components();

    println!("Replaying {} historical trades...", history.len());

    let initial_stats = loop_instance.stats();
    let initial_weights = initial_stats.current_weights;

    // Replay history
    for trade in history {
        loop_instance.register_decision(
            trade.pool_id.clone(),
            trade.initial_score,
            trade.initial_decision.clone(),
            components.clone(),
            trade.context.clone(),
        );

        let outcome = DecisionOutcome {
            candidate_id: trade.pool_id,
            final_decision: if trade.was_successful {
                DecisionType::Hold
            } else {
                DecisionType::Sell
            },
            followup_scores: vec![],
            profit_ratio: Some(trade.profit_ratio),
            total_corrections: 0,
            was_successful: trade.was_successful,
            elapsed_seconds: trade.elapsed_seconds,
        };

        loop_instance.register_outcome(outcome);
        thread::sleep(Duration::from_millis(15));
    }

    let final_stats = loop_instance.stats();

    println!("\n=== Historical Replay Results ===");
    println!("Total trades: {}", final_stats.total_outcomes);
    println!("Weight updates: {}", final_stats.weight_updates);
    println!("Cumulative reward: {:.4}", final_stats.cumulative_reward);
    println!("Average reward: {:.4}", final_stats.average_reward);
    println!("\nWeight Evolution:");
    println!(
        "  QASS: {:.2} -> {:.2}",
        initial_weights.w_qass, final_stats.current_weights.w_qass
    );
    println!(
        "  MPCF: {:.2} -> {:.2}",
        initial_weights.w_mpcf, final_stats.current_weights.w_mpcf
    );
    println!(
        "  SOBP: {:.2} -> {:.2}",
        initial_weights.w_sobp, final_stats.current_weights.w_sobp
    );
    println!(
        "  IWIM: {:.2} -> {:.2}",
        initial_weights.w_iwim, final_stats.current_weights.w_iwim
    );

    assert_eq!(final_stats.total_outcomes, 50);
    assert!(
        final_stats.weight_updates > 0,
        "Should have updated weights"
    );
}

#[test]
fn test_replay_with_varying_market_regimes() {
    let config = HysteresisConfig {
        update_cooldown_ms: 10,
        dampening_factor: 0.4,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance =
        HysteresisLoop::new(config, BanditAlgorithm::ThompsonSampling, initial_weights)
            .expect("Failed to create loop");

    let components = test_components();

    // Simulate three market regimes: bull, bear, neutral
    let regimes = vec![
        ("Bull Market", 30, 1.2, 0.85), // 30 trades, avg 20% profit, 85% success
        ("Bear Market", 20, 0.88, 0.40), // 20 trades, avg 12% loss, 40% success
        ("Neutral Market", 25, 1.05, 0.65), // 25 trades, avg 5% profit, 65% success
    ];

    for (regime_name, n_trades, avg_profit, success_rate) in regimes {
        println!("\n--- Simulating {} ---", regime_name);

        let regime_start_stats = loop_instance.stats();

        for i in 0..n_trades {
            let was_successful = (i as f32 / n_trades as f32) < success_rate;
            let profit_ratio = if was_successful {
                avg_profit + (i as f32 * 0.001)
            } else {
                1.0 - (1.0 - avg_profit).abs()
            };

            let context = TuningContext {
                volatility: if avg_profit < 1.0 { 0.8 } else { 0.4 },
                volume: 0.7,
                bot_activity: 0.3,
                mci: if avg_profit > 1.1 { 0.8 } else { 0.5 },
                time_factor: 0.5,
                pool_age_seconds: 60,
            };

            let pool_id = format!("regime_{}_{}", regime_name.replace(" ", "_"), i);

            loop_instance.register_decision(
                pool_id.clone(),
                (65.0 + (success_rate * 30.0)) as u8,
                DecisionType::Buy,
                components.clone(),
                context,
            );

            let outcome = DecisionOutcome {
                candidate_id: pool_id,
                final_decision: if was_successful {
                    DecisionType::Hold
                } else {
                    DecisionType::Sell
                },
                followup_scores: vec![],
                profit_ratio: Some(profit_ratio),
                total_corrections: 0,
                was_successful,
                elapsed_seconds: 50,
            };

            loop_instance.register_outcome(outcome);
            thread::sleep(Duration::from_millis(12));
        }

        let regime_end_stats = loop_instance.stats();
        let regime_reward =
            regime_end_stats.cumulative_reward - regime_start_stats.cumulative_reward;

        println!(
            "  Regime reward: {:.4}, Success rate: {:.1}%",
            regime_reward,
            success_rate * 100.0
        );
    }

    let final_stats = loop_instance.stats();
    println!("\n=== Final Stats After All Regimes ===");
    println!("Total outcomes: {}", final_stats.total_outcomes);
    println!("Cumulative reward: {:.4}", final_stats.cumulative_reward);
    println!("Repeatability: {:.3}", final_stats.repeatability_score);

    assert_eq!(final_stats.total_outcomes, 75);
}

#[test]
fn test_replay_convergence_speed() {
    // Test how quickly weights converge to optimal values
    let config = HysteresisConfig {
        update_cooldown_ms: 5,
        dampening_factor: 0.3, // Less dampening = faster convergence
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let components = test_components();
    let context = TuningContext {
        volatility: 0.5,
        volume: 0.7,
        bot_activity: 0.3,
        mci: 0.75,
        time_factor: 0.5,
        pool_age_seconds: 60,
    };

    let initial_stats = loop_instance.stats();
    let mut weight_history = vec![initial_stats.current_weights];

    // Run 100 consistent successful trades
    for i in 0..100 {
        let pool_id = format!("convergence_pool_{}", i);

        loop_instance.register_decision(
            pool_id.clone(),
            75,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );

        let outcome = DecisionOutcome {
            candidate_id: pool_id,
            final_decision: DecisionType::Hold,
            followup_scores: vec![],
            profit_ratio: Some(1.2), // Consistent 20% profit
            total_corrections: 0,
            was_successful: true,
            elapsed_seconds: 50,
        };

        loop_instance.register_outcome(outcome);

        // Record weight every 10 trades
        if i % 10 == 9 {
            let stats = loop_instance.stats();
            weight_history.push(stats.current_weights);
        }

        thread::sleep(Duration::from_millis(8));
    }

    println!("\n=== Weight Convergence Over Time ===");
    for (idx, weights) in weight_history.iter().enumerate() {
        println!(
            "Trade {:3}: QASS={:.2}, MPCF={:.2}, SOBP={:.2}, IWIM={:.2}",
            idx * 10,
            weights.w_qass,
            weights.w_mpcf,
            weights.w_sobp,
            weights.w_iwim
        );
    }

    // Check that weights stabilized (last two entries should be similar)
    let n = weight_history.len();
    let last_change = (weight_history[n - 1].w_qass - weight_history[n - 2].w_qass).abs();
    println!("\nLast weight change: {:.4}", last_change);

    assert!(
        last_change < 2.0,
        "Weights should stabilize after 100 trades"
    );
}

#[test]
fn test_replay_dry_run_mode() {
    let config = HysteresisConfig {
        dry_run: true,
        update_cooldown_ms: 10,
        monitor_repeatability: true,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let history = generate_synthetic_history(30);
    let components = test_components();

    println!("\n=== Dry-Run Replay Test ===");

    // Replay in dry-run mode
    for trade in history {
        loop_instance.register_decision(
            trade.pool_id.clone(),
            trade.initial_score,
            trade.initial_decision.clone(),
            components.clone(),
            trade.context.clone(),
        );

        let outcome = DecisionOutcome {
            candidate_id: trade.pool_id,
            final_decision: if trade.was_successful {
                DecisionType::Hold
            } else {
                DecisionType::Sell
            },
            followup_scores: vec![],
            profit_ratio: Some(trade.profit_ratio),
            total_corrections: 0,
            was_successful: trade.was_successful,
            elapsed_seconds: trade.elapsed_seconds,
        };

        loop_instance.register_outcome(outcome);
        thread::sleep(Duration::from_millis(12));
    }

    let final_stats = loop_instance.stats();

    println!("Outcomes processed: {}", final_stats.total_outcomes);
    println!("Cumulative reward: {:.4}", final_stats.cumulative_reward);
    println!("Weight updates: {}", final_stats.weight_updates);
    println!(
        "Weights: QASS={:.2}, MPCF={:.2}, SOBP={:.2}, IWIM={:.2}",
        final_stats.current_weights.w_qass,
        final_stats.current_weights.w_mpcf,
        final_stats.current_weights.w_sobp,
        final_stats.current_weights.w_iwim
    );

    assert!(final_stats.is_dry_run);
    assert_eq!(
        final_stats.weight_updates, 0,
        "No weight updates in dry-run"
    );
    assert_eq!(
        final_stats.current_weights, initial_weights,
        "Weights unchanged in dry-run"
    );
    assert!(
        final_stats.cumulative_reward != 0.0,
        "Should still calculate rewards"
    );
}

#[test]
fn test_replay_with_corrections() {
    // Test replay that includes followup corrections
    let config = HysteresisConfig {
        update_cooldown_ms: 10,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let components = test_components();
    let context = TuningContext {
        volatility: 0.6,
        volume: 0.7,
        bot_activity: 0.4,
        mci: 0.7,
        time_factor: 0.5,
        pool_age_seconds: 90,
    };

    // Scenario: Initial BUY, but heavy corrections lead to SELL
    for i in 0..20 {
        let pool_id = format!("correction_pool_{}", i);

        loop_instance.register_decision(
            pool_id.clone(),
            70,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );

        // Simulate outcomes with varying correction counts
        let num_corrections = i % 5;
        let was_successful = num_corrections < 2;
        let profit_ratio = if was_successful { 1.1 } else { 0.9 };

        let outcome = DecisionOutcome {
            candidate_id: pool_id,
            final_decision: if was_successful {
                DecisionType::Hold
            } else {
                DecisionType::Sell
            },
            followup_scores: vec![],
            profit_ratio: Some(profit_ratio),
            total_corrections: num_corrections,
            was_successful,
            elapsed_seconds: 55,
        };

        loop_instance.register_outcome(outcome);
        thread::sleep(Duration::from_millis(12));
    }

    let final_stats = loop_instance.stats();

    println!("\n=== Replay with Corrections ===");
    println!("Total outcomes: {}", final_stats.total_outcomes);
    println!("Average reward: {:.4}", final_stats.average_reward);

    assert_eq!(final_stats.total_outcomes, 20);
}
