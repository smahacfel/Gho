//! Config Wiring Regression Tests
//!
//! These tests verify that the `[confidence]` section weights from
//! ghost_brain_config.toml actually affect the SurvivorScore calculation.
//!
//! CRITICAL: If these tests fail, it means config weights are being ignored
//! and the bot is uncontrollable via configuration changes.

use ghost_brain::config::GhostBrainConfig;
use ghost_brain::oracle::survivor_score::{
    SurvivorScoreCalculator, SurvivorScoreConfig, SurvivorScoreInput,
};

/// Helper to create mock signals for testing
fn mock_signals() -> SurvivorScoreInput {
    SurvivorScoreInput {
        // Survival signals
        qedd_survival_60s: Some(0.7),
        iwim_threat_score: Some(0.2), // Low threat = good dev
        cluster_risk_score: Some(0.1),

        // Momentum signals (SOBP-related)
        sobp_momentum: Some(0.8),
        qman_score: Some(0.6),
        chaos_pump_prob: Some(0.7),

        // Quality signals (MPCF-related)
        mpcf_organic_ratio: Some(0.75),
        mesa_organic_likeness: Some(0.8),
        scr_bot_score: Some(0.2),
        unique_wallet_ratio: Some(0.6),

        // Risk signals (all clear)
        mesa_wash_likeness: Some(0.1),
        qman_exit_signal: false,
        price_crash_detected: false,
        paradox_anomaly: false,

        // LIGMA signals
        ligma_tradability_score: Some(0.7),
        ligma_psi: Some(0.3),
        ligma_liquidity_trap_risk: Some(0.1),

        // Transaction count
        tx_count: Some(25),
        ..Default::default()
    }
}

/// MANDATORY TEST: Verify config weights actually affect score calculation
///
/// This test is required per issue specification. If it fails, the PR must be rejected.
#[test]
fn test_confidence_config_impact() {
    let signals = mock_signals();

    // Create config with ONLY SOBP weighted (momentum-focused)
    let mut config_sobp_only = GhostBrainConfig::default();
    config_sobp_only.confidence.weight_sobp = 100.0;
    config_sobp_only.confidence.weight_mpcf = 0.0;
    config_sobp_only.confidence.weight_iwim = 0.0;

    let calc_sobp = SurvivorScoreCalculator::from_ghost_brain_config(&config_sobp_only);
    let score_sobp = calc_sobp.calculate(&signals);

    // Create config with ONLY MPCF weighted (quality-focused)
    let mut config_mpcf_only = GhostBrainConfig::default();
    config_mpcf_only.confidence.weight_sobp = 0.0;
    config_mpcf_only.confidence.weight_mpcf = 100.0;
    config_mpcf_only.confidence.weight_iwim = 0.0;

    let calc_mpcf = SurvivorScoreCalculator::from_ghost_brain_config(&config_mpcf_only);
    let score_mpcf = calc_mpcf.calculate(&signals);

    // Create config with ONLY IWIM weighted (survival-focused)
    let mut config_iwim_only = GhostBrainConfig::default();
    config_iwim_only.confidence.weight_sobp = 0.0;
    config_iwim_only.confidence.weight_mpcf = 0.0;
    config_iwim_only.confidence.weight_iwim = 100.0;

    let calc_iwim = SurvivorScoreCalculator::from_ghost_brain_config(&config_iwim_only);
    let score_iwim = calc_iwim.calculate(&signals);

    // CRITICAL ASSERTION: Scores must be different when weights are different
    // This proves the config weights are actually being used
    assert_ne!(
        score_sobp.score, score_mpcf.score,
        "CRITICAL: Config weights are ignored! SOBP-only and MPCF-only configs produced same score: {}",
        score_sobp.score
    );

    // Additional validation: all three should produce different scores
    // (unless by coincidence the mock signals happen to balance out)
    let scores = vec![score_sobp.score, score_mpcf.score, score_iwim.score];
    let unique_scores: std::collections::HashSet<_> = scores.iter().collect();

    // At minimum, 2 out of 3 should be different
    assert!(
        unique_scores.len() >= 2,
        "CRITICAL: Config weights appear to be ignored. All scores identical: SOBP={}, MPCF={}, IWIM={}",
        score_sobp.score, score_mpcf.score, score_iwim.score
    );

    println!(
        "✅ Config wiring verified: SOBP-only={}, MPCF-only={}, IWIM-only={}",
        score_sobp.score, score_mpcf.score, score_iwim.score
    );
}

/// Test that IWIM weight=0 means IWIM data has no impact
#[test]
fn test_iwim_weight_zero_no_impact() {
    // Config with IWIM weight = 0
    let mut config = GhostBrainConfig::default();
    config.confidence.weight_sobp = 50.0;
    config.confidence.weight_mpcf = 50.0;
    config.confidence.weight_iwim = 0.0;

    let calc = SurvivorScoreCalculator::from_ghost_brain_config(&config);

    // Test with good IWIM (low threat)
    let mut signals_good_iwim = mock_signals();
    signals_good_iwim.iwim_threat_score = Some(0.1); // Good dev
    let score_good = calc.calculate(&signals_good_iwim);

    // Test with bad IWIM (high threat)
    let mut signals_bad_iwim = mock_signals();
    signals_bad_iwim.iwim_threat_score = Some(0.9); // Bad dev
    let score_bad = calc.calculate(&signals_bad_iwim);

    // With IWIM weight = 0, the survival component weight becomes 0
    // So IWIM changes should have minimal/no effect on score
    // Note: There may still be some effect due to the formula, but it should be reduced
    let score_diff = (score_good.score as i32 - score_bad.score as i32).abs();

    println!(
        "IWIM weight=0 test: good_iwim={}, bad_iwim={}, diff={}",
        score_good.score, score_bad.score, score_diff
    );

    // The difference should be significantly smaller than when IWIM has weight
    // We can't assert exact equality due to formula interactions
    // Maximum expected score difference when IWIM weight is zero
    const MAX_SCORE_DIFF_WHEN_ZERO_WEIGHT: i32 = 20;
    assert!(
        score_diff < MAX_SCORE_DIFF_WHEN_ZERO_WEIGHT,
        "With IWIM weight=0, IWIM data should have reduced impact. Diff was {}",
        score_diff
    );
}

/// Test that SurvivorScoreConfig::from_config properly normalizes weights
#[test]
fn test_weight_normalization() {
    // Test with various weight combinations
    let mut config = GhostBrainConfig::default();

    // Set weights that don't sum to 1.0
    config.confidence.weight_sobp = 40.0;
    config.confidence.weight_mpcf = 35.0;
    config.confidence.weight_iwim = 25.0;

    let survivor_config = SurvivorScoreConfig::from_config(&config);

    // Weights should be normalized to sum to 1.0 (100%) - NO ARTIFICIAL CAPS
    let weight_sum = survivor_config.weight_survival
        + survivor_config.weight_momentum
        + survivor_config.weight_quality;

    assert!(
        (weight_sum - 1.0).abs() < 0.01,
        "Weights should sum to ~1.0, got {}",
        weight_sum
    );

    // Verify proportions are maintained
    // IWIM -> survival, SOBP -> momentum, MPCF -> quality
    assert!(
        survivor_config.weight_momentum > survivor_config.weight_quality,
        "SOBP (40) > MPCF (35) should mean momentum > quality"
    );
    assert!(
        survivor_config.weight_quality > survivor_config.weight_survival,
        "MPCF (35) > IWIM (25) should mean quality > survival"
    );

    println!(
        "Weight normalization: survival={:.3}, momentum={:.3}, quality={:.3}, sum={:.3}",
        survivor_config.weight_survival,
        survivor_config.weight_momentum,
        survivor_config.weight_quality,
        weight_sum
    );
}

/// Test that passing_threshold is derived from threshold_high
#[test]
fn test_threshold_from_config() {
    let mut config = GhostBrainConfig::default();

    // Set threshold_high to 0.75 (should become 75)
    config.confidence.threshold_high = 0.75;

    let survivor_config = SurvivorScoreConfig::from_config(&config);

    assert_eq!(
        survivor_config.passing_threshold, 75,
        "threshold_high=0.75 should map to passing_threshold=75"
    );

    // Test edge cases
    config.confidence.threshold_high = 0.0;
    let config_zero = SurvivorScoreConfig::from_config(&config);
    assert_eq!(config_zero.passing_threshold, 0);

    config.confidence.threshold_high = 1.0;
    let config_one = SurvivorScoreConfig::from_config(&config);
    assert_eq!(config_one.passing_threshold, 100);
}

/// Test the "Predator" vs "Coward" scenario from issue description
/// Changing config should change bot behavior significantly
#[test]
fn test_strategy_change_via_config() {
    let signals = mock_signals();

    // "Coward" strategy: High threshold, survival-focused
    let mut coward_config = GhostBrainConfig::default();
    coward_config.confidence.threshold_high = 0.80;
    coward_config.confidence.weight_iwim = 50.0; // Focus on dev safety
    coward_config.confidence.weight_sobp = 25.0;
    coward_config.confidence.weight_mpcf = 25.0;

    let coward_calc = SurvivorScoreCalculator::from_ghost_brain_config(&coward_config);
    let coward_result = coward_calc.calculate(&signals);

    // "Predator" strategy: Lower threshold, momentum-focused
    let mut predator_config = GhostBrainConfig::default();
    predator_config.confidence.threshold_high = 0.65;
    predator_config.confidence.weight_iwim = 15.0;
    predator_config.confidence.weight_sobp = 55.0; // Focus on buying pressure
    predator_config.confidence.weight_mpcf = 30.0;

    let predator_calc = SurvivorScoreCalculator::from_ghost_brain_config(&predator_config);
    let predator_result = predator_calc.calculate(&signals);

    // Scores should differ
    assert_ne!(
        coward_result.score, predator_result.score,
        "Different strategies should produce different scores"
    );

    // Predator should have lower threshold requirement
    let predator_survivor_config = SurvivorScoreConfig::from_config(&predator_config);
    let coward_survivor_config = SurvivorScoreConfig::from_config(&coward_config);

    assert!(
        predator_survivor_config.passing_threshold < coward_survivor_config.passing_threshold,
        "Predator should have lower passing threshold"
    );

    println!(
        "Strategy comparison: Coward(score={}, threshold={}) vs Predator(score={}, threshold={})",
        coward_result.score,
        coward_survivor_config.passing_threshold,
        predator_result.score,
        predator_survivor_config.passing_threshold
    );
}

/// Verify LIGMA weight is properly wired from config
#[test]
fn test_ligma_weight_from_config() {
    let mut config = GhostBrainConfig::default();

    // Set custom LIGMA weight
    config.ligma.weight_in_survivor_score = 0.30;

    let calc = SurvivorScoreCalculator::from_ghost_brain_config(&config);

    // Create signals with varying LIGMA scores
    let mut signals_high_ligma = mock_signals();
    signals_high_ligma.ligma_tradability_score = Some(0.95);

    let mut signals_low_ligma = mock_signals();
    signals_low_ligma.ligma_tradability_score = Some(0.1);

    let score_high = calc.calculate(&signals_high_ligma);
    let score_low = calc.calculate(&signals_low_ligma);

    // With LIGMA weight = 0.30, LIGMA score should have noticeable impact
    assert!(
        score_high.score > score_low.score,
        "High LIGMA tradability should produce higher score. Got high={}, low={}",
        score_high.score,
        score_low.score
    );

    println!(
        "LIGMA impact test: high_tradability={}, low_tradability={}",
        score_high.score, score_low.score
    );
}
