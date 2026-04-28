//! Integration tests for Hysteresis Learning Loop
//!
//! These tests verify the complete feedback cycle:
//! - Ultrafast decision → Followup scoring → Outcome → Bandits → Weight update

use ghost_brain::oracle::{DecisionType, FollowupScore, InitialComponents};
use ghost_brain::tuning::{
    BanditAlgorithm, DecisionOutcome, HysteresisConfig, HysteresisLoop, TunableWeights,
    TuningContext,
};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

/// Helper to create test context
fn test_context() -> TuningContext {
    TuningContext {
        volatility: 0.6,
        volume: 0.8,
        bot_activity: 0.3,
        mci: 0.75,
        time_factor: 0.5,
        pool_age_seconds: 120,
    }
}

/// Helper to create test components
fn test_components() -> InitialComponents {
    InitialComponents {
        base_shadow: 65,
        qass_score: 78.0,
        qedd_survival_30s: Some(0.72),
        mci: Some(0.75),
        chaos_loss_prob: Some(0.08),
        gene_match_score: Some(0.02),
        confidence: Some(0.88),
        extras: HashMap::new(),
    }
}

#[test]
fn test_full_learning_cycle_success() {
    // Setup with shortened parameters for faster test execution
    // Note: Production uses 5000ms cooldown, but we use 100ms here for testing speed
    let config = HysteresisConfig {
        enabled: true,
        dry_run: false,
        update_cooldown_ms: 100, // Short cooldown for rapid test execution
        dampening_factor: 0.5,
        max_pending_decisions: 100,
        outcome_timeout_seconds: 60,
        monitor_repeatability: true,
        repeatability_window: 10,
        repeatability_threshold: 0.8,
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Cycle 1: Register ultrafast decision
    loop_instance.register_decision(
        "pool_success_1".to_string(),
        75,
        DecisionType::Buy,
        components.clone(),
        context.clone(),
    );

    let stats = loop_instance.stats();
    assert_eq!(stats.total_decisions, 1);
    assert_eq!(stats.pending_count, 1);

    // Simulate followup scoring and successful outcome
    let outcome = DecisionOutcome {
        candidate_id: "pool_success_1".to_string(),
        final_decision: DecisionType::Hold,
        followup_scores: vec![],
        profit_ratio: Some(1.3), // 30% profit
        total_corrections: 0,
        was_successful: true,
        elapsed_seconds: 65,
    };

    loop_instance.register_outcome(outcome);

    // Wait for cooldown
    thread::sleep(Duration::from_millis(150));

    let stats = loop_instance.stats();
    assert_eq!(stats.total_outcomes, 1);
    assert_eq!(stats.pending_count, 0);
    assert!(stats.cumulative_reward > 0.0, "Should have positive reward");
    assert!(stats.weight_updates >= 1, "Should have updated weights");
}

#[test]
fn test_full_learning_cycle_loss() {
    let config = HysteresisConfig {
        update_cooldown_ms: 100,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Register decision
    loop_instance.register_decision(
        "pool_loss_1".to_string(),
        70,
        DecisionType::Buy,
        components,
        context,
    );

    // Simulate loss
    let outcome = DecisionOutcome {
        candidate_id: "pool_loss_1".to_string(),
        final_decision: DecisionType::Sell,
        followup_scores: vec![],
        profit_ratio: Some(0.7), // 30% loss
        total_corrections: 2,
        was_successful: false,
        elapsed_seconds: 45,
    };

    loop_instance.register_outcome(outcome);

    thread::sleep(Duration::from_millis(150));

    let stats = loop_instance.stats();
    assert!(stats.cumulative_reward < 0.0, "Should have negative reward");
}

#[test]
fn test_multiple_cycles_weight_convergence() {
    let config = HysteresisConfig {
        update_cooldown_ms: 50,
        dampening_factor: 0.3, // Less dampening for faster convergence
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    let initial_stats = loop_instance.stats();
    let initial_w_qass = initial_stats.current_weights.w_qass;

    // Run multiple learning cycles
    for i in 0..10 {
        let pool_id = format!("pool_cycle_{}", i);

        loop_instance.register_decision(
            pool_id.clone(),
            72 + i as u8,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );

        // Alternate between success and small loss
        let profit_ratio = if i % 2 == 0 { 1.15 } else { 0.95 };
        let was_successful = profit_ratio > 1.0;

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
            elapsed_seconds: 50 + i,
        };

        loop_instance.register_outcome(outcome);

        thread::sleep(Duration::from_millis(60));
    }

    let final_stats = loop_instance.stats();
    assert_eq!(final_stats.total_decisions, 10);
    assert_eq!(final_stats.total_outcomes, 10);

    // Weights should have changed
    assert_ne!(
        final_stats.current_weights.w_qass, initial_w_qass,
        "Weights should have updated"
    );

    println!(
        "Weight convergence: initial={:.2}, final={:.2}",
        initial_w_qass, final_stats.current_weights.w_qass
    );
}

#[test]
fn test_dry_run_no_weight_updates() {
    let config = HysteresisConfig {
        dry_run: true,
        update_cooldown_ms: 50,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();
    let initial_stats = loop_instance.stats();

    // Multiple cycles
    for i in 0..5 {
        loop_instance.register_decision(
            format!("pool_dry_{}", i),
            75,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );

        let outcome = DecisionOutcome {
            candidate_id: format!("pool_dry_{}", i),
            final_decision: DecisionType::Hold,
            followup_scores: vec![],
            profit_ratio: Some(1.2),
            total_corrections: 0,
            was_successful: true,
            elapsed_seconds: 60,
        };

        loop_instance.register_outcome(outcome);
        thread::sleep(Duration::from_millis(60));
    }

    let final_stats = loop_instance.stats();
    assert!(final_stats.is_dry_run);
    assert_eq!(final_stats.weight_updates, 0, "No updates in dry-run mode");
    assert_eq!(
        final_stats.current_weights, initial_stats.current_weights,
        "Weights should not change in dry-run"
    );
}

#[test]
fn test_repeatability_monitoring() {
    let config = HysteresisConfig {
        monitor_repeatability: true,
        repeatability_window: 5,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Register consistent decisions
    for i in 0..6 {
        loop_instance.register_decision(
            format!("pool_repeat_{}", i),
            75,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );
    }

    let stats = loop_instance.stats();
    assert!(
        stats.repeatability_score > 0.8,
        "Should have high repeatability for consistent decisions"
    );
}

#[test]
fn test_outcome_timeout_cleanup() {
    let config = HysteresisConfig {
        outcome_timeout_seconds: 1, // Very short timeout
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Register decision
    loop_instance.register_decision(
        "pool_timeout_1".to_string(),
        75,
        DecisionType::Buy,
        components,
        context,
    );

    let stats = loop_instance.stats();
    assert_eq!(stats.pending_count, 1);

    // Wait for timeout
    thread::sleep(Duration::from_secs(2));

    // Try to register outcome (should be rejected)
    let outcome = DecisionOutcome {
        candidate_id: "pool_timeout_1".to_string(),
        final_decision: DecisionType::Hold,
        followup_scores: vec![],
        profit_ratio: Some(1.1),
        total_corrections: 0,
        was_successful: true,
        elapsed_seconds: 70,
    };

    loop_instance.register_outcome(outcome);

    let stats = loop_instance.stats();
    assert_eq!(
        stats.total_outcomes, 0,
        "Expired outcome should be rejected"
    );
}

#[test]
fn test_max_pending_decisions_limit() {
    let config = HysteresisConfig {
        max_pending_decisions: 5,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Register more than max
    for i in 0..10 {
        loop_instance.register_decision(
            format!("pool_limit_{}", i),
            75,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );
    }

    let stats = loop_instance.stats();
    assert!(
        stats.pending_count <= 5,
        "Should not exceed max pending limit"
    );
}

#[test]
fn test_thompson_sampling_algorithm() {
    let config = HysteresisConfig {
        update_cooldown_ms: 50,
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance =
        HysteresisLoop::new(config, BanditAlgorithm::ThompsonSampling, initial_weights)
            .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Run learning cycle
    loop_instance.register_decision(
        "pool_thompson_1".to_string(),
        75,
        DecisionType::Buy,
        components,
        context,
    );

    let outcome = DecisionOutcome {
        candidate_id: "pool_thompson_1".to_string(),
        final_decision: DecisionType::Hold,
        followup_scores: vec![],
        profit_ratio: Some(1.25),
        total_corrections: 0,
        was_successful: true,
        elapsed_seconds: 55,
    };

    loop_instance.register_outcome(outcome);

    thread::sleep(Duration::from_millis(100));

    let stats = loop_instance.stats();
    assert_eq!(stats.total_outcomes, 1);
}

#[test]
fn test_suggest_weights_after_learning() {
    let config = HysteresisConfig {
        update_cooldown_ms: 50,
        dampening_factor: 0.2, // Less dampening
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Before any learning
    let initial_suggested = loop_instance.suggest_weights(&context);
    assert_eq!(initial_suggested, TunableWeights::default());

    // Run learning cycles
    for i in 0..3 {
        loop_instance.register_decision(
            format!("pool_suggest_{}", i),
            78,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );

        let outcome = DecisionOutcome {
            candidate_id: format!("pool_suggest_{}", i),
            final_decision: DecisionType::Hold,
            followup_scores: vec![],
            profit_ratio: Some(1.4),
            total_corrections: 0,
            was_successful: true,
            elapsed_seconds: 50,
        };

        loop_instance.register_outcome(outcome);
        thread::sleep(Duration::from_millis(60));
    }

    // After learning
    let updated_suggested = loop_instance.suggest_weights(&context);
    // Weights should have evolved
    println!(
        "Weights evolved: QASS {:.2} -> {:.2}",
        initial_suggested.w_qass, updated_suggested.w_qass
    );
}

#[test]
fn test_cooldown_prevents_oscillation() {
    let config = HysteresisConfig {
        update_cooldown_ms: 500, // Long cooldown
        ..Default::default()
    };

    let initial_weights = TunableWeights::default();
    let loop_instance = HysteresisLoop::new(config, BanditAlgorithm::LinUCB, initial_weights)
        .expect("Failed to create loop");

    let context = test_context();
    let components = test_components();

    // Rapid fire outcomes
    for i in 0..5 {
        loop_instance.register_decision(
            format!("pool_cooldown_{}", i),
            75,
            DecisionType::Buy,
            components.clone(),
            context.clone(),
        );

        let outcome = DecisionOutcome {
            candidate_id: format!("pool_cooldown_{}", i),
            final_decision: DecisionType::Hold,
            followup_scores: vec![],
            profit_ratio: Some(1.1),
            total_corrections: 0,
            was_successful: true,
            elapsed_seconds: 50,
        };

        loop_instance.register_outcome(outcome);
        // No sleep - fire immediately
    }

    let stats = loop_instance.stats();
    assert!(
        stats.weight_updates < 5,
        "Cooldown should limit update frequency"
    );
}
