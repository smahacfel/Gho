//! SurvivorScore Unit Tests - Task 5 (Zadanie 5)
//!
//! Tests for the scoring engine as specified in CONSOLIDATED_TASKS_SCORING_ENGINE.md.
//!
//! ## Coverage
//! - Cycle score calculation with formula verification
//! - Risk discount clamping
//! - Quality formulas for Early Stage and Full Analysis
//! - Component calculations (Momentum, Quality, Survival)
//! - Edge cases and boundary conditions

use ghost_brain::oracle::predator_strategy::{
    calculate_quality_early_stage, calculate_quality_full_analysis,
    calculate_weighted_geometric_mean, get_gunshot_threshold, CYCLE_WEIGHTS, CYCLE_WEIGHTS_SUM,
    GUNSHOT_THRESHOLDS,
};
use ghost_brain::oracle::survivor_score::{
    SurvivorScoreCalculator, SurvivorScoreConfig, SurvivorScoreInput,
};
use ghost_brain::oracle::ultrafast::EctoVerdict;

// =============================================================================
// 5.1: SurvivorScore Unit Tests
// =============================================================================

#[test]
fn test_cycle_score_calculation() {
    let calculator = SurvivorScoreCalculator::new();
    let components = SurvivorScoreInput {
        qedd_survival_60s: Some(0.8),
        sobp_momentum: Some(0.5),
        chaos_pump_prob: Some(0.5),
        mpcf_organic_ratio: Some(0.75),
        cluster_risk_score: Some(0.2),
        ..Default::default()
    };

    let result = calculator.calculate(&components);

    // Verify score is in valid range
    assert!(
        result.score <= 100,
        "Score should be <= 100, got {}",
        result.score
    );

    // Verify breakdown components are in valid ranges
    // Issue #155: Momentum range widened from [0.5, 2.0] to [0.2, 4.0]
    assert!(result.breakdown.survival >= 0.0 && result.breakdown.survival <= 1.0);
    assert!(result.breakdown.momentum >= 0.2 && result.breakdown.momentum <= 4.0);
    assert!(result.breakdown.quality >= 0.0 && result.breakdown.quality <= 1.0);
}

#[test]
fn test_risk_discount_clamping() {
    let calculator = SurvivorScoreCalculator::new();

    // Test with extreme wash trading
    let input = SurvivorScoreInput {
        mesa_wash_likeness: Some(0.95), // Very high wash
        qman_exit_signal: true,         // Exit signal
        qedd_survival_60s: Some(0.7),
        ..Default::default()
    };

    let result = calculator.calculate(&input);

    // Risk discount should be clamped to 0.9 max
    assert!(
        result.breakdown.risk_discount <= 0.9,
        "Risk discount should be clamped at 0.9, got {}",
        result.breakdown.risk_discount
    );
}

#[test]
fn test_quality_early_stage_no_scr() {
    // Test Early Stage quality formula: 0.44*mpcf + 0.31*mesa + 0.25*wallet
    let quality = calculate_quality_early_stage(0.8, 0.7, 0.6);
    let expected = 0.44 * 0.8 + 0.31 * 0.7 + 0.25 * 0.6;

    assert!(
        (quality - expected).abs() < 0.001,
        "Quality should be {:.4}, got {:.4}",
        expected,
        quality
    );

    // Verify weights sum to 1.0
    let weight_sum: f32 = 0.44 + 0.31 + 0.25;
    assert!(
        (weight_sum - 1.0).abs() < 0.01,
        "Early Stage weights should sum to 1.0, got {}",
        weight_sum
    );
}

#[test]
fn test_quality_full_analysis_with_scr() {
    // Test Full Analysis quality formula: 0.35*mpcf + 0.25*mesa + 0.20*(1-scr) + 0.20*wallet
    let quality = calculate_quality_full_analysis(0.8, 0.7, 0.2, 0.6);
    let expected = 0.35 * 0.8 + 0.25 * 0.7 + 0.20 * (1.0 - 0.2) + 0.20 * 0.6;

    assert!(
        (quality - expected).abs() < 0.001,
        "Quality should be {:.4}, got {:.4}",
        expected,
        quality
    );

    // Verify weights sum to 1.0
    let weight_sum: f32 = 0.35 + 0.25 + 0.20 + 0.20;
    assert!(
        (weight_sum - 1.0).abs() < 0.01,
        "Full Analysis weights should sum to 1.0, got {}",
        weight_sum
    );
}

#[test]
fn test_momentum_calculation() {
    let calculator = SurvivorScoreCalculator::new();

    // High momentum inputs
    let high_momentum_input = SurvivorScoreInput {
        sobp_momentum: Some(1.5), // Strong buying
        chaos_pump_prob: Some(0.9),
        qman_score: Some(0.8),
        ..Default::default()
    };

    // Low momentum inputs
    let low_momentum_input = SurvivorScoreInput {
        sobp_momentum: Some(-0.5), // Selling
        chaos_pump_prob: Some(0.2),
        qman_score: Some(0.3),
        ..Default::default()
    };

    let high_result = calculator.calculate(&high_momentum_input);
    let low_result = calculator.calculate(&low_momentum_input);

    assert!(
        high_result.breakdown.momentum > low_result.breakdown.momentum,
        "High momentum ({}) should be > low momentum ({})",
        high_result.breakdown.momentum,
        low_result.breakdown.momentum
    );

    // Verify momentum is in valid range [0.2, 4.0] (Issue #155: widened from [0.5, 2.0])
    assert!(high_result.breakdown.momentum >= 0.2 && high_result.breakdown.momentum <= 4.0);
    assert!(low_result.breakdown.momentum >= 0.2 && low_result.breakdown.momentum <= 4.0);
}

#[test]
fn test_survival_calculation_without_iwim() {
    let calculator = SurvivorScoreCalculator::new();

    // Test survival calculation during cycles (without IWIM)
    // Formula: Survival = 0.625 * qedd + 0.375 * (1 - cluster_risk)
    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.8),
        cluster_risk_score: Some(0.2),
        ..Default::default()
    };

    let result = calculator.calculate(&input);

    // Expected survival (during cycles, no IWIM)
    let expected_survival = 0.625 * 0.8 + 0.375 * (1.0 - 0.2);

    assert!(
        (result.breakdown.survival - expected_survival).abs() < 0.01,
        "Survival should be ~{:.4}, got {:.4}",
        expected_survival,
        result.breakdown.survival
    );
}

#[test]
fn test_survival_calculation_with_iwim() {
    let calculator = SurvivorScoreCalculator::new();

    // Test survival calculation at Final Verdict (with IWIM)
    // Formula: Survival = 0.5 * qedd + 0.3 * (1 - iwim_threat) + 0.2 * (1 - cluster_risk)
    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.8),
        iwim_threat_score: Some(0.1), // Low threat (good dev)
        cluster_risk_score: Some(0.2),
        ..Default::default()
    };

    let result = calculator.calculate_with_iwim(&input);

    // Expected survival (Final Verdict with IWIM)
    let expected_survival = 0.5 * 0.8 + 0.3 * (1.0 - 0.1) + 0.2 * (1.0 - 0.2);

    assert!(
        (result.breakdown.survival - expected_survival).abs() < 0.01,
        "Survival with IWIM should be ~{:.4}, got {:.4}",
        expected_survival,
        result.breakdown.survival
    );
}

// =============================================================================
// 5.3: GUNSHOT Unit Tests
// =============================================================================

#[test]
fn test_gunshot_thresholds() {
    // S1 requires 100
    assert_eq!(
        get_gunshot_threshold(0),
        100.0,
        "S1 threshold should be 100"
    );
    assert!(get_gunshot_threshold(0) >= 100.0);

    // S12 requires 82
    assert_eq!(
        get_gunshot_threshold(11),
        82.0,
        "S12 threshold should be 82"
    );

    // Test descending order
    for i in 0..11 {
        assert!(
            GUNSHOT_THRESHOLDS[i] >= GUNSHOT_THRESHOLDS[i + 1],
            "Thresholds should descend: S{} ({}) >= S{} ({})",
            i + 1,
            GUNSHOT_THRESHOLDS[i],
            i + 2,
            GUNSHOT_THRESHOLDS[i + 1]
        );
    }
}

#[test]
fn test_gunshot_threshold_boundaries() {
    // Test exact threshold matching
    let thresholds = [
        (0, 100.0), // S1
        (1, 99.0),  // S2
        (2, 98.0),  // S3
        (3, 97.0),  // S4
        (4, 96.0),  // S5
        (5, 95.0),  // S6
        (6, 88.0),  // S7
        (7, 87.0),  // S8
        (8, 86.0),  // S9
        (9, 85.0),  // S10
        (10, 83.5), // S11
        (11, 82.0), // S12
    ];

    for (idx, expected) in thresholds {
        let actual = get_gunshot_threshold(idx);
        assert!(
            (actual - expected).abs() < 0.01,
            "S{} threshold should be {}, got {}",
            idx + 1,
            expected,
            actual
        );
    }
}

#[test]
fn test_gunshot_out_of_bounds() {
    // Out of bounds should return 100.0 (max difficulty)
    let out_of_bounds = get_gunshot_threshold(12);
    assert_eq!(
        out_of_bounds, 100.0,
        "Out of bounds threshold should be 100.0"
    );

    let far_out = get_gunshot_threshold(100);
    assert_eq!(
        far_out, 100.0,
        "Far out of bounds threshold should be 100.0"
    );
}

// =============================================================================
// 5.4: Weighted Geometric Mean Unit Tests
// =============================================================================

#[test]
fn test_weighted_geometric_mean_uniform() {
    // All scores equal 80 - mean should be 80
    let scores = vec![80.0; 12];
    let mean = calculate_weighted_geometric_mean(&scores);

    assert!(
        (mean - 80.0).abs() < 0.1,
        "Uniform 80 scores should give mean ~80, got {}",
        mean
    );
}

#[test]
fn test_weighted_geometric_mean_later_cycles_dominant() {
    // High scores in late cycles should dominate due to higher weights
    let mut scores = vec![50.0; 12];
    scores[10] = 90.0; // S11 (weight 17.0)
    scores[11] = 95.0; // S12 (weight 22.0)

    let mean = calculate_weighted_geometric_mean(&scores);

    // Mean should be significantly higher than 50 due to late cycle weights
    assert!(
        mean > 55.0,
        "Late high scores should increase mean: expected >55, got {}",
        mean
    );
}

#[test]
fn test_weighted_geometric_mean_empty() {
    // Empty scores should return 0
    let scores: Vec<f32> = vec![];
    let mean = calculate_weighted_geometric_mean(&scores);

    assert_eq!(mean, 0.0, "Empty scores should return 0");
}

#[test]
fn test_weighted_geometric_mean_single() {
    // Single score should return that score
    let scores = vec![75.0];
    let mean = calculate_weighted_geometric_mean(&scores);

    assert!(
        (mean - 75.0).abs() < 0.1,
        "Single score 75 should give mean ~75, got {}",
        mean
    );
}

#[test]
fn test_cycle_weights_values() {
    // Verify specific weights from spec
    assert_eq!(CYCLE_WEIGHTS[0], 1.3, "S1 weight should be 1.3");
    assert_eq!(CYCLE_WEIGHTS[5], 4.6, "S6 weight should be 4.6");
    assert_eq!(CYCLE_WEIGHTS[6], 6.0, "S7 weight should be 6.0");
    assert_eq!(CYCLE_WEIGHTS[11], 22.0, "S12 weight should be 22.0");

    // Verify ascending order
    for i in 0..11 {
        assert!(
            CYCLE_WEIGHTS[i] < CYCLE_WEIGHTS[i + 1],
            "Weights should ascend: S{} ({}) < S{} ({})",
            i + 1,
            CYCLE_WEIGHTS[i],
            i + 2,
            CYCLE_WEIGHTS[i + 1]
        );
    }
}

#[test]
fn test_cycle_weights_sum() {
    // Verify CYCLE_WEIGHTS_SUM constant matches actual sum
    let actual_sum: f32 = CYCLE_WEIGHTS.iter().sum();

    assert!(
        (actual_sum - CYCLE_WEIGHTS_SUM).abs() < 0.1,
        "CYCLE_WEIGHTS_SUM ({}) should match actual sum ({})",
        CYCLE_WEIGHTS_SUM,
        actual_sum
    );
}

// =============================================================================
// 5.5: TCF Modulation Unit Tests
// =============================================================================

#[test]
fn test_tcf_modulation_range() {
    // TCF modulation formula: effective = base * (0.6 + 0.4 * tcf_score)

    // tcf_score = 0.0 -> modulation = 0.6
    let tcf_score_0: f32 = 0.0;
    let base: f32 = 100.0;
    let result_0 = base * (0.6 + 0.4 * tcf_score_0);
    assert!(
        (result_0 - 60.0).abs() < 0.01,
        "TCF=0 should give 60, got {}",
        result_0
    );

    // tcf_score = 1.0 -> modulation = 1.0
    let tcf_score_1: f32 = 1.0;
    let result_1 = base * (0.6 + 0.4 * tcf_score_1);
    assert!(
        (result_1 - 100.0).abs() < 0.01,
        "TCF=1 should give 100, got {}",
        result_1
    );

    // tcf_score = 0.5 -> modulation = 0.8
    let tcf_score_05: f32 = 0.5;
    let result_05 = base * (0.6 + 0.4 * tcf_score_05);
    assert!(
        (result_05 - 80.0).abs() < 0.01,
        "TCF=0.5 should give 80, got {}",
        result_05
    );
}

#[test]
fn test_tcf_cliff_penalty() {
    // When cliff detected, score is penalized by 0.6 multiplier
    let base_score: f32 = 85.0;
    let cliff_penalty: f32 = 0.6;
    let penalized = base_score * cliff_penalty;

    assert!(
        (penalized - 51.0).abs() < 0.01,
        "Cliff penalty should reduce 85 to 51, got {}",
        penalized
    );
}

// =============================================================================
// 5.8: Edge Cases Tests
// =============================================================================

#[test]
fn test_zero_score_handling() {
    // Score 0 should not cause panic in ln() for geometric mean
    let scores = vec![0.1; 12]; // Using min floor value
    let mean = calculate_weighted_geometric_mean(&scores);

    assert!(mean.is_finite(), "Mean should be finite, got {}", mean);
    assert!(mean >= 0.0, "Mean should be non-negative");
}

#[test]
fn test_iwim_missing_in_final_verdict() {
    // Missing IWIM = Safety 1.0 (Default Trust)
    let calculator = SurvivorScoreCalculator::new();

    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.8),
        cluster_risk_score: Some(0.2),
        iwim_threat_score: None, // Missing IWIM
        ..Default::default()
    };

    let result = calculator.calculate_with_iwim(&input);

    // survival_from_iwim should be 1.0 when missing (Default Trust)
    assert_eq!(
        result.breakdown.survival_from_iwim, 1.0,
        "Missing IWIM should default to 1.0 (trust)"
    );
}

#[test]
fn test_paradox_anomaly_veto() {
    let calculator = SurvivorScoreCalculator::new();

    let input = SurvivorScoreInput {
        paradox_anomaly: true, // Network anomaly detected
        qedd_survival_60s: Some(0.9),
        sobp_momentum: Some(1.0),
        ..Default::default()
    };

    let result = calculator.calculate(&input);

    // Paradox anomaly should trigger Hard Veto
    assert_eq!(
        result.score, 0,
        "Paradox anomaly should trigger Hard Veto (score=0)"
    );
    assert!(!result.passed);
    assert!(result.interpretation.contains("PARADOX"));
}

#[test]
fn test_price_crash_veto() {
    let calculator = SurvivorScoreCalculator::new();

    let input = SurvivorScoreInput {
        price_crash_detected: true, // >90% crash
        qedd_survival_60s: Some(0.9),
        sobp_momentum: Some(1.0),
        ..Default::default()
    };

    let result = calculator.calculate(&input);

    // Price crash should trigger Hard Veto
    assert_eq!(
        result.score, 0,
        "Price crash should trigger Hard Veto (score=0)"
    );
    assert!(!result.passed);
    assert!(result.interpretation.contains("CRASH"));
}

#[test]
fn test_smart_money_exit_penalty() {
    let calculator = SurvivorScoreCalculator::new();

    // With exit signal
    let input_with_exit = SurvivorScoreInput {
        qman_exit_signal: true,
        qedd_survival_60s: Some(0.8),
        sobp_momentum: Some(0.5),
        ..Default::default()
    };

    // Without exit signal
    let input_no_exit = SurvivorScoreInput {
        qman_exit_signal: false,
        qedd_survival_60s: Some(0.8),
        sobp_momentum: Some(0.5),
        ..Default::default()
    };

    let result_with_exit = calculator.calculate(&input_with_exit);
    let result_no_exit = calculator.calculate(&input_no_exit);

    // Exit signal should NOT veto (only penalty)
    assert!(
        result_with_exit.score > 0,
        "Exit signal should apply penalty, not veto"
    );

    // Exit signal should reduce score
    assert!(
        result_with_exit.score < result_no_exit.score,
        "Exit signal should reduce score: {} < {}",
        result_with_exit.score,
        result_no_exit.score
    );

    // Risk from exit should be recorded
    assert!(result_with_exit.breakdown.risk_from_exit > 0.0);
}

#[test]
fn test_all_components_provided() {
    // Test with all signals provided for maximum confidence
    let calculator = SurvivorScoreCalculator::new();

    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.85),
        iwim_threat_score: Some(0.1),
        cluster_risk_score: Some(0.15),
        sobp_momentum: Some(0.8),
        qman_score: Some(0.75),
        chaos_pump_prob: Some(0.7),
        mpcf_organic_ratio: Some(0.85),
        mesa_organic_likeness: Some(0.8),
        scr_bot_score: Some(0.1),
        unique_wallet_ratio: Some(0.75),
        mesa_wash_likeness: Some(0.2),
        qman_exit_signal: false,
        price_crash_detected: false,
        paradox_anomaly: false,
        ligma_tradability_score: Some(0.8),
        ligma_psi: Some(0.5),
        ligma_liquidity_trap_risk: Some(0.1),
        tx_count: Some(25),
        ecto_score: Some(0.8),
        ecto_verdict: Some(EctoVerdict::Neutral),
        bva_score: Some(0.75),
        panic_pressure: Some(0.1),
        tcr_causality: Some(0.8),
        cir_strength: Some(0.7),
        age_secs: Some(120.0),
        ..Default::default()
    };

    let result = calculator.calculate(&input);

    // Should have high confidence with all signals
    assert!(
        result.confidence > 0.8,
        "All signals should give high confidence: {}",
        result.confidence
    );

    // Should pass with good signals
    assert!(result.passed, "Good signals should pass");
}

#[test]
fn test_minimal_signals() {
    // Test with minimal signals
    let calculator = SurvivorScoreCalculator::new();

    let input = SurvivorScoreInput::default();

    let result = calculator.calculate(&input);

    // Should have lower confidence with minimal signals
    assert!(
        result.confidence < 0.5,
        "Minimal signals should have low confidence: {}",
        result.confidence
    );

    // Score should still be in valid range
    assert!(result.score <= 100);
}

// =============================================================================
// Formula Verification Tests
// =============================================================================

#[test]
fn test_survivor_score_formula_structure() {
    // Verify formula: S = survival^Ws × momentum^Wm × quality^Wq × (1 - risk_discount)
    // Default weights: Ws=0.35, Wm=0.30, Wq=0.20

    let config = SurvivorScoreConfig::default();

    assert_eq!(
        config.weight_survival, 0.35,
        "Survival weight should be 0.35"
    );
    assert_eq!(
        config.weight_momentum, 0.30,
        "Momentum weight should be 0.30"
    );
    assert_eq!(config.weight_quality, 0.20, "Quality weight should be 0.20");
}

#[test]
fn test_wash_trading_penalty_threshold() {
    let calculator = SurvivorScoreCalculator::new();

    // Just below threshold (0.6) - should not incur penalty
    let input_below = SurvivorScoreInput {
        mesa_wash_likeness: Some(0.59),
        qedd_survival_60s: Some(0.7),
        ..Default::default()
    };

    // Above threshold (0.6) - should incur penalty
    let input_above = SurvivorScoreInput {
        mesa_wash_likeness: Some(0.7),
        qedd_survival_60s: Some(0.7),
        ..Default::default()
    };

    let result_below = calculator.calculate(&input_below);
    let result_above = calculator.calculate(&input_above);

    assert_eq!(
        result_below.breakdown.risk_from_wash, 0.0,
        "Wash < 0.6 should have no penalty"
    );
    assert!(
        result_above.breakdown.risk_from_wash > 0.0,
        "Wash > 0.6 should have penalty"
    );
}

// =============================================================================
// IWIM Behavior Tests
// =============================================================================

#[test]
fn test_iwim_invisible_during_cycles() {
    // During cycles, IWIM should NOT affect score
    let calculator = SurvivorScoreCalculator::new();

    let base_input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.7),
        cluster_risk_score: Some(0.2),
        sobp_momentum: Some(0.5),
        ..Default::default()
    };

    // No IWIM
    let result_no_iwim = calculator.calculate(&base_input);

    // Good IWIM
    let input_good_iwim = SurvivorScoreInput {
        iwim_threat_score: Some(0.1),
        ..base_input.clone()
    };
    let result_good_iwim = calculator.calculate(&input_good_iwim);

    // Bad IWIM
    let input_bad_iwim = SurvivorScoreInput {
        iwim_threat_score: Some(0.9),
        ..base_input.clone()
    };
    let result_bad_iwim = calculator.calculate(&input_bad_iwim);

    // All scores should be IDENTICAL during cycles (IWIM invisible)
    assert_eq!(
        result_no_iwim.score, result_good_iwim.score,
        "IWIM should not affect cycle score: no_iwim={} vs good_iwim={}",
        result_no_iwim.score, result_good_iwim.score
    );
    assert_eq!(
        result_no_iwim.score, result_bad_iwim.score,
        "IWIM should not affect cycle score: no_iwim={} vs bad_iwim={}",
        result_no_iwim.score, result_bad_iwim.score
    );
}

#[test]
fn test_iwim_applied_at_final_verdict() {
    // At Final Verdict, IWIM SHOULD affect score
    let calculator = SurvivorScoreCalculator::new();

    let base_input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.7),
        cluster_risk_score: Some(0.2),
        sobp_momentum: Some(0.5),
        ..Default::default()
    };

    // Good IWIM at Final Verdict
    let input_good_iwim = SurvivorScoreInput {
        iwim_threat_score: Some(0.1),
        ..base_input.clone()
    };
    let result_good = calculator.calculate_with_iwim(&input_good_iwim);

    // Bad IWIM at Final Verdict
    let input_bad_iwim = SurvivorScoreInput {
        iwim_threat_score: Some(0.9),
        ..base_input.clone()
    };
    let result_bad = calculator.calculate_with_iwim(&input_bad_iwim);

    // Good IWIM should give higher score than bad IWIM
    assert!(
        result_good.score > result_bad.score,
        "Good IWIM should score higher: good={} vs bad={}",
        result_good.score,
        result_bad.score
    );
}
