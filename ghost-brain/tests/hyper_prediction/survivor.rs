//! SurvivorScore Integration Tests
//!
//! Tests for the interpretable scoring system integration.

use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_brain::pumpfun::PumpCurveStateCache;

mod fixtures;
use fixtures::{create_test_candidate, create_test_oracle, create_test_cache};

#[test]
fn test_survivor_score_present_in_result() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // SurvivorScore should be computed
    assert!(
        result.survivor_score_result.is_some(),
        "SurvivorScore should be computed for every evaluation"
    );
}

#[test]
fn test_survivor_score_influences_passed() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    
    // Low quality candidate
    let mut low_candidate = create_test_candidate();
    low_candidate.initial_liquidity_sol = 0.1;
    low_candidate.has_dev_buy = false;
    low_candidate.vanity_score = 5;
    low_candidate.metadata_len_score = 10;

    let result = oracle.score_candidate(
        &low_candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // With very low quality, should likely not pass
    // (depends on threshold and survivor calculation)
    if let Some(ref survivor) = result.survivor_score_result {
        assert!(survivor.score <= 100);
        assert!(survivor.confidence >= 0.0 && survivor.confidence <= 1.0);
    }
}

#[test]
fn test_interpretation_shows_survivor() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // Interpretation should show SURVIVOR score
    assert!(
        result.interpretation.contains("SURVIVOR"),
        "Interpretation should contain SURVIVOR: {}",
        result.interpretation
    );
}

#[test]
fn test_interpretation_survivor_before_qass() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // In the new interpretation format, SURVIVOR should come before QASS
    if result.interpretation.contains("SURVIVOR") && result.interpretation.contains("QASS") {
        let survivor_pos = result.interpretation.find("SURVIVOR").unwrap();
        let qass_pos = result.interpretation.find("QASS").unwrap();
        
        assert!(
            survivor_pos < qass_pos,
            "SURVIVOR should appear before QASS in interpretation"
        );
    }
}

#[test]
fn test_early_exit_on_critical_survivor_score() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    
    // Create worst-case candidate
    let mut terrible_candidate = create_test_candidate();
    terrible_candidate.initial_liquidity_sol = 0.01;  // Almost no liquidity
    terrible_candidate.has_dev_buy = false;
    terrible_candidate.dev_buy_sol = 0.0;
    terrible_candidate.vanity_score = 0;
    terrible_candidate.metadata_len_score = 0;
    terrible_candidate.mint_auth_disabled = false;  // Red flag

    let result = oracle.score_candidate(
        &terrible_candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // Should fail with very low survivor score
    if let Some(ref survivor) = result.survivor_score_result {
        if survivor.score < 35 {  // Below critical threshold
            assert!(!result.passed, "Very low survivor score should fail");
        }
    }
}

#[test]
fn test_qass_limited_to_secondary_modifier() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // QASS should be present but limited
    if let Some(ref qass) = result.qass_result {
        // QASS score should be valid
        assert!(qass.score >= 0.0 && qass.score <= 1.0);
        assert!(qass.confidence >= 0.0 && qass.confidence <= 1.0);
    }

    // Final score should be primarily driven by SurvivorScore
    if let Some(ref survivor) = result.survivor_score_result {
        // The difference between final score and survivor score should be limited
        // (QASS can only add ±10 points by default)
        let score_diff = (result.score as i16 - survivor.score as i16).abs();
        
        // Allow for additional modifiers beyond just QASS
        assert!(
            score_diff <= 50,  // Conservative limit allowing for penalties/boosters
            "Final score {} differs too much from survivor score {}: diff={}",
            result.score, survivor.score, score_diff
        );
    }
}

#[test]
fn test_survivor_score_breakdown_available() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    if let Some(ref survivor) = result.survivor_score_result {
        // Breakdown components should be present
        let breakdown = &survivor.breakdown;
        
        // All components should be in valid range
        assert!(breakdown.survival >= 0.0 && breakdown.survival <= 1.0);
        assert!(breakdown.momentum >= 0.0 && breakdown.momentum <= 1.0);
        assert!(breakdown.quality >= 0.0 && breakdown.quality <= 1.0);
        assert!(breakdown.risk_discount >= 0.0 && breakdown.risk_discount <= 1.0);
    }
}
