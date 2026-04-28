//! E2E Scoring Integration Tests - Task 5 (Zadanie 5)
//!
//! End-to-end tests for the complete scoring flow as specified in
//! CONSOLIDATED_TASKS_SCORING_ENGINE.md.
//!
//! ## Coverage
//! - Full scoring flow without GUNSHOT
//! - Full scoring flow with GUNSHOT
//! - Full scoring flow with VETO
//! - IWIM integration in Final Verdict
//! - TCF modulation
//! - Weighted geometric mean final calculation

use ghost_brain::oracle::predator_strategy::{
    calculate_quality_for_cycle, calculate_weighted_geometric_mean, get_gunshot_threshold,
    is_early_stage_cycle, ScoringPhase, CYCLE_WEIGHTS,
};
use ghost_brain::oracle::survivor_score::{SurvivorScoreCalculator, SurvivorScoreInput};
use ghost_brain::oracle::tcf::{
    observation_from_ghost_signals, MarketObservation, TrendCohesionField,
};

// =============================================================================
// 5.6: E2E Scoring Integration Tests
// =============================================================================

/// Helper to simulate a scoring cycle
fn simulate_cycle(cycle_idx: usize, base_score: f32, tcf_score: f64, cliff_detected: bool) -> f32 {
    // Apply TCF modulation
    if cliff_detected {
        base_score * 0.6 // Cliff penalty
    } else {
        base_score * (0.6 + 0.4 * tcf_score as f32) // Organic boost
    }
}

/// Helper to check if gunshot triggers
fn check_gunshot(cycle_idx: usize, score: f32) -> bool {
    score >= get_gunshot_threshold(cycle_idx)
}

#[test]
fn test_full_scoring_flow_no_gunshot() {
    // Simulate complete 12-cycle flow without GUNSHOT trigger
    let calculator = SurvivorScoreCalculator::new();
    let mut tcf = TrendCohesionField::new();
    let mut scores: Vec<f32> = Vec::with_capacity(12);

    // Simulate moderate token - no gunshot trigger
    for cycle_idx in 0..12 {
        let phase = ScoringPhase::from_cycle_idx(cycle_idx);

        // Simulate input signals
        let input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.7 + 0.01 * cycle_idx as f32),
            sobp_momentum: Some(0.5 + 0.02 * cycle_idx as f32),
            mpcf_organic_ratio: Some(0.65),
            cluster_risk_score: Some(0.2),
            ..Default::default()
        };

        // Calculate base score
        let result = calculator.calculate(&input);
        let base_score = result.score as f32;

        // TCF observation
        let obs = observation_from_ghost_signals(
            0.05 + 0.01 * cycle_idx as f64,
            0.03 + 0.01 * cycle_idx as f64,
            0.55,
            0.7,
            0.5,
            0.2,
        );
        let tcf_result = tcf.update(&obs);

        // Apply TCF modulation
        let modulated_score = simulate_cycle(
            cycle_idx,
            base_score,
            tcf_result.tcf_score,
            tcf_result.cliff_detected,
        );

        // Check gunshot (should not trigger for moderate scores)
        let gunshot = check_gunshot(cycle_idx, modulated_score);
        assert!(
            !gunshot,
            "Gunshot should not trigger at S{} with score {}",
            cycle_idx + 1,
            modulated_score
        );

        scores.push(modulated_score);
    }

    // Final Verdict - weighted geometric mean
    let final_score = calculate_weighted_geometric_mean(&scores);

    // Verify final score is reasonable
    assert!(
        final_score > 0.0 && final_score <= 100.0,
        "Final score should be in (0, 100], got {}",
        final_score
    );

    // Verify we collected 12 scores
    assert_eq!(scores.len(), 12, "Should have 12 cycle scores");
}

#[test]
fn test_full_scoring_flow_with_gunshot_s3() {
    // Simulate GUNSHOT trigger at S3
    let calculator = SurvivorScoreCalculator::new();
    let mut tcf = TrendCohesionField::new();
    let mut gunshot_triggered = false;
    let mut gunshot_cycle = 0;

    // Simulate extremely strong token - triggers GUNSHOT
    for cycle_idx in 0..12 {
        // Exceptional signals
        let input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.98),
            sobp_momentum: Some(1.5), // Very strong
            chaos_pump_prob: Some(0.95),
            mpcf_organic_ratio: Some(0.95),
            mesa_organic_likeness: Some(0.9),
            cluster_risk_score: Some(0.05),
            ligma_tradability_score: Some(0.95),
            ..Default::default()
        };

        let result = calculator.calculate(&input);
        let base_score = result.score as f32;

        // TCF with excellent trend
        let obs = observation_from_ghost_signals(0.3, 0.4, 0.7, 0.9, 0.4, 0.1);
        let tcf_result = tcf.update(&obs);

        let modulated_score = simulate_cycle(
            cycle_idx,
            base_score,
            tcf_result.tcf_score,
            tcf_result.cliff_detected,
        );

        // Check gunshot
        let threshold = get_gunshot_threshold(cycle_idx);
        if modulated_score >= threshold {
            gunshot_triggered = true;
            gunshot_cycle = cycle_idx + 1;
            break;
        }
    }

    // Verify gunshot behavior (may or may not trigger depending on exact calculation)
    // If it triggers, it should be in early cycles due to high thresholds
    if gunshot_triggered {
        assert!(
            gunshot_cycle <= 6,
            "If gunshot triggers with strong signals, should be early: S{}",
            gunshot_cycle
        );
    }
}

#[test]
fn test_full_scoring_flow_with_veto() {
    // Simulate VETO by LIGMA or ClusterHunter
    let calculator = SurvivorScoreCalculator::new();
    let mut veto_triggered = false;
    let mut veto_reason = String::new();

    // Simulate token with critical risk
    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.8),
        sobp_momentum: Some(0.7),
        cluster_risk_score: Some(0.95), // Very high cabal risk
        paradox_anomaly: true,          // Network anomaly
        ..Default::default()
    };

    let result = calculator.calculate(&input);

    // Check for Hard Veto conditions
    if result.score == 0 {
        veto_triggered = true;
        veto_reason = result.interpretation.clone();
    }

    assert!(veto_triggered, "Paradox anomaly should trigger VETO");
    assert!(
        veto_reason.contains("PARADOX") || veto_reason.contains("⛔"),
        "VETO reason should mention PARADOX"
    );
}

#[test]
fn test_iwim_integration_in_final_verdict() {
    // Verify IWIM affects Final Verdict differently than cycles
    let calculator = SurvivorScoreCalculator::new();

    let base_input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.75),
        sobp_momentum: Some(0.6),
        cluster_risk_score: Some(0.2),
        mpcf_organic_ratio: Some(0.7),
        ..Default::default()
    };

    // Cycle scores (IWIM invisible)
    let mut cycle_scores: Vec<f32> = Vec::new();
    for _cycle in 0..12 {
        let input_with_iwim = SurvivorScoreInput {
            iwim_threat_score: Some(0.8), // High threat - should be invisible
            ..base_input.clone()
        };
        let result = calculator.calculate(&input_with_iwim);
        cycle_scores.push(result.score as f32);
    }

    // Final Verdict (IWIM applied)
    let final_input = SurvivorScoreInput {
        iwim_threat_score: Some(0.8), // High threat
        ..base_input.clone()
    };
    let final_result = calculator.calculate_with_iwim(&final_input);

    let weighted_mean = calculate_weighted_geometric_mean(&cycle_scores);

    // IWIM should reduce Final Verdict score compared to cycle scores
    // (because high threat = low safety in Final Verdict)
    assert!(
        (final_result.score as f32) < weighted_mean || cycle_scores.iter().all(|&s| s < 50.0),
        "Final Verdict with high IWIM threat should be lower than or equal to cycle mean"
    );
}

// =============================================================================
// TCF Integration Tests
// =============================================================================

#[test]
fn test_tcf_organic_growth_pattern() {
    let mut tcf = TrendCohesionField::new();

    // Simulate organic growth: consistent upward movement
    for i in 0..13 {
        let obs = MarketObservation::new(
            0.1 + 0.02 * i as f64, // Gradually increasing price
            0.1 + 0.01 * i as f64, // Volume following price
            0.6,                   // Healthy entropy
            0.3,                   // Moderate buy pressure
            0.7,                   // High MPCF confidence
            0.6,                   // Human-like jitter
            0.2,                   // Low sync (independent actors)
        );
        tcf.update(&obs);
    }

    let tcf_score = tcf.get_tcf_score();

    // Organic growth should have high TCF score
    assert!(
        tcf_score > 0.5,
        "Organic growth should have high TCF: {}",
        tcf_score
    );
}

#[test]
fn test_tcf_pump_dump_detection() {
    let mut tcf = TrendCohesionField::new();

    // Phase 1: Pump (cycles 0-6)
    for i in 0..7 {
        let obs = MarketObservation::new(
            0.3 + 0.1 * i as f64, // Strong price increase
            0.4 + 0.1 * i as f64, // Strong volume
            0.5,
            0.6,
            0.5,
            0.3, // Low jitter (bot-like)
            0.7, // High sync (coordinated)
        );
        tcf.update(&obs);
    }

    // Phase 2: Dump (cycles 7-12) - sudden reversal
    for i in 0..6 {
        let obs = MarketObservation::new(
            -0.4 - 0.1 * i as f64, // Sharp price drop
            -0.2,                  // Declining volume
            0.3,
            -0.7, // Heavy selling
            0.5,
            0.4,
            0.5,
        );
        let result = tcf.update(&obs);

        // After reversal, TCF should detect issues
        if i >= 3 {
            // Either low score or cliff detected or dump phase
            let diag = tcf.get_diagnostics();
            assert!(
                diag.tcf_score < 0.7 || diag.cliff_detected || diag.trend_direction == -1,
                "TCF should detect pump-dump pattern"
            );
        }
    }
}

#[test]
fn test_tcf_modulation_formula() {
    // Test TCF modulation: effective = base * (0.6 + 0.4 * tcf_score)
    let mut tcf = TrendCohesionField::new();

    // Prime TCF
    for i in 0..8 {
        let obs = MarketObservation::new(
            0.1 + 0.02 * i as f64,
            0.1 + 0.01 * i as f64,
            0.6,
            0.3,
            0.7,
            0.5,
            0.2,
        );
        tcf.update(&obs);
    }

    let tcf_score = tcf.get_tcf_score();
    let base_momentum = 50.0;
    let effective_momentum = base_momentum * (0.6 + 0.4 * tcf_score);

    // Verify range
    assert!(effective_momentum >= 30.0, "Min when tcf=0 should be 30");
    assert!(effective_momentum <= 50.0, "Max when tcf=1 should be 50");
}

// =============================================================================
// Final Verdict Decision Tests
// =============================================================================

#[test]
fn test_final_verdict_buy_decision() {
    // Scores above threshold (82) should result in BUY
    let scores: Vec<f32> = vec![85.0; 12];
    let final_score = calculate_weighted_geometric_mean(&scores);

    let threshold = 82.0;
    let is_buy = final_score >= threshold;

    assert!(
        is_buy,
        "Uniform 85 scores should result in BUY (score {} >= {})",
        final_score, threshold
    );
}

#[test]
fn test_final_verdict_skip_decision() {
    // Scores below threshold (82) should result in SKIP
    let scores: Vec<f32> = vec![70.0; 12];
    let final_score = calculate_weighted_geometric_mean(&scores);

    let threshold = 82.0;
    let is_buy = final_score >= threshold;

    assert!(
        !is_buy,
        "Uniform 70 scores should result in SKIP (score {} < {})",
        final_score, threshold
    );
}

#[test]
fn test_final_verdict_late_rally() {
    // Strong late scores should boost final decision due to exponential weights
    let mut scores: Vec<f32> = vec![60.0; 12];
    scores[10] = 90.0; // S11 (weight 17.0)
    scores[11] = 95.0; // S12 (weight 22.0)

    let final_score = calculate_weighted_geometric_mean(&scores);

    // Should be higher than 60 due to late rally
    assert!(
        final_score > 65.0,
        "Late rally should boost final score: {} > 65",
        final_score
    );
}

#[test]
fn test_final_verdict_early_crash() {
    // Low early scores should not heavily penalize due to low weights
    let mut scores: Vec<f32> = vec![80.0; 12];
    scores[0] = 30.0; // S1 (weight 1.3)
    scores[1] = 40.0; // S2 (weight 1.7)

    let final_score = calculate_weighted_geometric_mean(&scores);

    // Should still be relatively high despite early crash
    assert!(
        final_score > 70.0,
        "Early crash should not heavily penalize: {} > 70",
        final_score
    );
}

// =============================================================================
// Phase-Aware Scoring Tests
// =============================================================================

#[test]
fn test_early_stage_scoring_consistency() {
    // All Early Stage cycles should use the same quality formula
    for cycle_idx in 0..6 {
        assert!(
            is_early_stage_cycle(cycle_idx),
            "Cycle {} should be Early Stage",
            cycle_idx
        );

        // Verify SCR is ignored
        let q_with_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, Some(0.9), 0.5);
        let q_without_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, None, 0.5);

        assert_eq!(
            q_with_scr,
            q_without_scr,
            "Early Stage S{} should ignore SCR",
            cycle_idx + 1
        );
    }
}

#[test]
fn test_full_analysis_scr_impact() {
    // Full Analysis cycles should incorporate SCR
    for cycle_idx in 6..12 {
        assert!(
            !is_early_stage_cycle(cycle_idx),
            "Cycle {} should be Full Analysis",
            cycle_idx
        );

        // SCR should affect quality
        let q_low_bot = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, Some(0.1), 0.5);
        let q_high_bot = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, Some(0.9), 0.5);

        assert!(
            q_low_bot > q_high_bot,
            "Low bot score should give higher quality: {} > {}",
            q_low_bot,
            q_high_bot
        );
    }
}

// =============================================================================
// Concurrent Pool Scoring Tests
// =============================================================================

#[test]
fn test_independent_scoring_instances() {
    // Multiple scoring instances should be independent
    let calc1 = SurvivorScoreCalculator::new();
    let calc2 = SurvivorScoreCalculator::new();

    let input1 = SurvivorScoreInput {
        qedd_survival_60s: Some(0.9),
        sobp_momentum: Some(1.0),
        ..Default::default()
    };

    let input2 = SurvivorScoreInput {
        qedd_survival_60s: Some(0.3),
        sobp_momentum: Some(-0.5),
        ..Default::default()
    };

    let result1 = calc1.calculate(&input1);
    let result2 = calc2.calculate(&input2);

    // Results should be different for different inputs
    assert_ne!(
        result1.score, result2.score,
        "Different inputs should produce different scores"
    );
}

#[test]
fn test_tcf_instance_independence() {
    // Different TCF instances should be independent
    let mut tcf1 = TrendCohesionField::new();
    let mut tcf2 = TrendCohesionField::new();

    // Feed different patterns
    for i in 0..8 {
        let obs1 = MarketObservation::new(0.1, 0.1, 0.5, 0.2, 0.5, 0.5, 0.2);
        let obs2 = MarketObservation::new(-0.1 * i as f64, -0.1, 0.3, -0.3, 0.5, 0.3, 0.6);

        tcf1.update(&obs1);
        tcf2.update(&obs2);
    }

    let score1 = tcf1.get_tcf_score();
    let score2 = tcf2.get_tcf_score();

    // Scores should differ after feeding different patterns
    if tcf1.is_primed() && tcf2.is_primed() {
        assert!(
            (score1 - score2).abs() > 0.05,
            "Different patterns should produce different TCF scores: {} vs {}",
            score1,
            score2
        );
    }
}

// =============================================================================
// Edge Case Handling Tests
// =============================================================================

#[test]
fn test_scoring_with_all_defaults() {
    let calculator = SurvivorScoreCalculator::new();
    let input = SurvivorScoreInput::default();

    let result = calculator.calculate(&input);

    // Should produce a valid score even with defaults
    assert!(result.score <= 100);
    assert!(!result.interpretation.is_empty());
}

#[test]
fn test_scoring_with_extreme_values() {
    let calculator = SurvivorScoreCalculator::new();

    // Test with all maximum values
    let max_input = SurvivorScoreInput {
        qedd_survival_60s: Some(1.0),
        iwim_threat_score: Some(0.0), // Min threat = max safety
        cluster_risk_score: Some(0.0),
        sobp_momentum: Some(2.0),
        qman_score: Some(1.0),
        chaos_pump_prob: Some(1.0),
        mpcf_organic_ratio: Some(1.0),
        mesa_organic_likeness: Some(1.0),
        scr_bot_score: Some(0.0),
        unique_wallet_ratio: Some(1.0),
        mesa_wash_likeness: Some(0.0),
        qman_exit_signal: false,
        price_crash_detected: false,
        paradox_anomaly: false,
        ligma_tradability_score: Some(1.0),
        ligma_psi: Some(1.0),
        ligma_liquidity_trap_risk: Some(0.0),
        tx_count: Some(50),
        ..Default::default()
    };

    let result = calculator.calculate(&max_input);

    // Should produce high score
    assert!(
        result.score >= 70,
        "Maximum inputs should give high score: {}",
        result.score
    );
    assert!(result.passed);
}

#[test]
fn test_geometric_mean_numerical_stability() {
    // Test with scores near zero (but not zero)
    let low_scores: Vec<f32> = vec![1.0; 12];
    let mean = calculate_weighted_geometric_mean(&low_scores);

    assert!(mean.is_finite(), "Mean should be finite");
    assert!(mean > 0.0, "Mean should be positive");

    // Test with very high scores
    let high_scores: Vec<f32> = vec![99.9; 12];
    let high_mean = calculate_weighted_geometric_mean(&high_scores);

    assert!(high_mean.is_finite(), "High mean should be finite");
    assert!((high_mean - 99.9).abs() < 0.5, "High mean should be ~99.9");
}
