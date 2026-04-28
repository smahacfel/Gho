//! Example: Hysteresis Learning Loop Demo
//!
//! This example demonstrates the complete hysteresis learning loop workflow:
//! 1. Register ultrafast decisions
//! 2. Simulate followup scoring
//! 3. Register outcomes
//! 4. Observe weight evolution
//!
//! Run with: cargo run --example hysteresis_loop_demo

use ghost_brain::oracle::{DecisionType, FollowupScore, InitialComponents};
use ghost_brain::tuning::{
    BanditAlgorithm, DecisionOutcome, HysteresisConfig, HysteresisLoop, TunableWeights,
    TuningContext,
};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== Hysteresis Learning Loop Demo ===\n");

    // Configure the learning loop
    let config = HysteresisConfig {
        enabled: true,
        dry_run: false,
        update_cooldown_ms: 1000, // 1s for demo
        dampening_factor: 0.6,
        max_pending_decisions: 100,
        outcome_timeout_seconds: 300,
        monitor_repeatability: true,
        repeatability_window: 10,
        repeatability_threshold: 0.85,
    };

    println!("Configuration:");
    println!("  Update cooldown: {} ms", config.update_cooldown_ms);
    println!("  Dampening factor: {}", config.dampening_factor);
    println!("  Dry run: {}\n", config.dry_run);

    // Create loop with LinUCB algorithm
    let initial_weights = TunableWeights::default();
    println!("Initial weights:");
    println!("  QASS: {:.2}", initial_weights.w_qass);
    println!("  MPCF: {:.2}", initial_weights.w_mpcf);
    println!("  SOBP: {:.2}", initial_weights.w_sobp);
    println!("  IWIM: {:.2}\n", initial_weights.w_iwim);

    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create hysteresis loop");

    // Simulate 20 trading cycles
    println!("Simulating 20 trading cycles...\n");

    for cycle in 1..=20 {
        println!("--- Cycle {} ---", cycle);

        // Create realistic context
        let context = TuningContext {
            volatility: 0.5 + (cycle as f32 * 0.01) % 0.3,
            volume: 0.7,
            bot_activity: 0.3,
            mci: 0.75 - (cycle as f32 * 0.01) % 0.2,
            time_factor: 0.5,
            pool_age_seconds: 60 + cycle as u64 * 5,
        };

        // Simulate ultrafast decision
        let candidate_id = format!("demo_pool_{}", cycle);
        let initial_score = 70 + (cycle % 15) as u8;

        let components = InitialComponents {
            base_shadow: 65,
            qass_score: 75.0 + (cycle as f32 % 10.0),
            qedd_survival_30s: Some(0.72),
            mci: Some(context.mci),
            chaos_loss_prob: Some(0.08),
            gene_match_score: Some(0.02),
            confidence: Some(0.85),
            extras: HashMap::new(),
        };

        // Register ultrafast decision
        loop_instance.register_decision(
            candidate_id.clone(),
            initial_score,
            DecisionType::Buy,
            components,
            context,
        );

        println!("  Registered BUY decision (score: {})", initial_score);

        // Simulate followup scoring delay
        thread::sleep(Duration::from_millis(100));

        // Simulate outcome
        // Cycles 1-10: mostly successful
        // Cycles 11-15: mixed results
        // Cycles 16-20: improving again
        let success_factor = match cycle {
            1..=10 => 0.8,
            11..=15 => 0.4,
            _ => 0.7,
        };

        let was_successful = (cycle % 3) != 0 || cycle <= 10 || cycle >= 16;
        let profit_ratio = if was_successful {
            1.0 + success_factor * 0.25 // 10-20% profit
        } else {
            1.0 - (1.0 - success_factor) * 0.15 // 5-9% loss
        };

        let outcome = DecisionOutcome {
            candidate_id: candidate_id.clone(),
            final_decision: if was_successful {
                DecisionType::Hold
            } else {
                DecisionType::Sell
            },
            followup_scores: vec![],
            profit_ratio: Some(profit_ratio),
            total_corrections: if was_successful { 0 } else { 2 },
            was_successful,
            elapsed_seconds: 55,
        };

        // Register outcome
        loop_instance.register_outcome(outcome);

        println!(
            "  Outcome: {} ({}% {})",
            if was_successful { "SUCCESS" } else { "LOSS" },
            ((profit_ratio - 1.0) * 100.0).abs() as i32,
            if profit_ratio > 1.0 { "profit" } else { "loss" }
        );

        // Show statistics every 5 cycles
        if cycle % 5 == 0 {
            let stats = loop_instance.stats();
            println!("\n  Current Statistics:");
            println!("    Total outcomes: {}", stats.total_outcomes);
            println!("    Cumulative reward: {:.4}", stats.cumulative_reward);
            println!("    Average reward: {:.4}", stats.average_reward);
            println!("    Weight updates: {}", stats.weight_updates);
            println!("    Repeatability: {:.3}", stats.repeatability_score);
            println!("    Current weights:");
            println!("      QASS: {:.2}", stats.current_weights.w_qass);
            println!("      MPCF: {:.2}", stats.current_weights.w_mpcf);
            println!("      SOBP: {:.2}", stats.current_weights.w_sobp);
            println!("      IWIM: {:.2}", stats.current_weights.w_iwim);
        }

        println!();

        // Allow weight update to process
        thread::sleep(Duration::from_millis(1100));
    }

    // Final statistics
    let final_stats = loop_instance.stats();
    println!("\n=== Final Results ===");
    println!("Total decisions: {}", final_stats.total_decisions);
    println!("Total outcomes: {}", final_stats.total_outcomes);
    println!("Cumulative reward: {:.4}", final_stats.cumulative_reward);
    println!("Average reward: {:.4}", final_stats.average_reward);
    println!("Weight updates: {}", final_stats.weight_updates);
    println!(
        "Repeatability score: {:.3}",
        final_stats.repeatability_score
    );
    println!("\nWeight Evolution:");
    println!("  Initial → Final");
    println!(
        "  QASS: {:.2} → {:.2}",
        initial_weights.w_qass, final_stats.current_weights.w_qass
    );
    println!(
        "  MPCF: {:.2} → {:.2}",
        initial_weights.w_mpcf, final_stats.current_weights.w_mpcf
    );
    println!(
        "  SOBP: {:.2} → {:.2}",
        initial_weights.w_sobp, final_stats.current_weights.w_sobp
    );
    println!(
        "  IWIM: {:.2} → {:.2}",
        initial_weights.w_iwim, final_stats.current_weights.w_iwim
    );

    // Weight change analysis
    let qass_change = final_stats.current_weights.w_qass - initial_weights.w_qass;
    let mpcf_change = final_stats.current_weights.w_mpcf - initial_weights.w_mpcf;
    let sobp_change = final_stats.current_weights.w_sobp - initial_weights.w_sobp;
    let iwim_change = final_stats.current_weights.w_iwim - initial_weights.w_iwim;

    println!("\nWeight Changes:");
    println!(
        "  QASS: {:+.2} ({:.1}%)",
        qass_change,
        (qass_change / initial_weights.w_qass) * 100.0
    );
    println!(
        "  MPCF: {:+.2} ({:.1}%)",
        mpcf_change,
        (mpcf_change / initial_weights.w_mpcf) * 100.0
    );
    println!(
        "  SOBP: {:+.2} ({:.1}%)",
        sobp_change,
        (sobp_change / initial_weights.w_sobp) * 100.0
    );
    println!(
        "  IWIM: {:+.2} ({:.1}%)",
        iwim_change,
        (iwim_change / initial_weights.w_iwim) * 100.0
    );

    println!("\n=== Demo Complete ===");
}
