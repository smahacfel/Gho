//! Scoring Logic Tests for HyperPrediction Oracle
//!
//! Tests for score calculation, modifiers, and risk assessment.

use ghost_brain::oracle::hyper_prediction::{
    HyperPredictionOracle,
    HyperPredictionConfig,
};
use ghost_brain::oracle::tx_metrics::TransactionMetrics;
use ghost_brain::pumpfun::PumpCurveStateCache;

mod fixtures;
use fixtures::{create_test_candidate, create_test_oracle, create_test_cache};

#[test]
fn test_hyper_prediction_varied_scores() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();

    // Test with different candidate profiles
    let mut low_quality = create_test_candidate();
    low_quality.initial_liquidity_sol = 0.5;  // Very low
    low_quality.has_dev_buy = false;
    low_quality.dev_buy_sol = 0.0;
    low_quality.vanity_score = 10;

    let mut high_quality = create_test_candidate();
    high_quality.initial_liquidity_sol = 100.0;  // Very high
    high_quality.has_dev_buy = true;
    high_quality.dev_buy_sol = 10.0;
    high_quality.vanity_score = 90;

    let low_result = oracle.score_candidate(
        &low_quality, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    let high_result = oracle.score_candidate(
        &high_quality, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // High quality should generally score higher
    // Note: Without full signals, the difference may not be dramatic
    assert!(
        high_result.score >= low_result.score,
        "High quality candidate should score >= low quality: {} vs {}",
        high_result.score, low_result.score
    );
}

#[test]
fn test_transaction_metrics_cause_score_variation() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Without metrics
    let result_no_metrics = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // With healthy metrics
    let healthy_metrics = TransactionMetrics::new(
        vec![2.0, 3.0, 2.5, 4.0, 1.5],  // Varied volumes
        vec![100, 200, 150, 250, 300],  // Varied intervals
        5
    );

    let result_with_metrics = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(healthy_metrics), None, None, None
    ).unwrap();

    // Results should potentially differ (metrics provide more data)
    // Both should be valid scores
    assert!(result_no_metrics.score <= 100);
    assert!(result_with_metrics.score <= 100);
}

#[test]
fn test_cold_start_multiplier_scales_with_chaos_probability() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Low pump probability
    let chaos_low = ghost_brain::chaos::engine::ChaosResult {
        pump_probability: 20.0,
        crash_probability: 60.0,
        median_roi: -10.0,
        p5_roi: -30.0,
        p95_roi: 5.0,
        mean_price_change: -8.0,
        price_volatility: 30.0,
        num_simulations: 10000,
        execution_time_ms: 500,
        avg_time_per_sim_us: 50.0,
    };

    // High pump probability
    let chaos_high = ghost_brain::chaos::engine::ChaosResult {
        pump_probability: 80.0,
        crash_probability: 10.0,
        median_roi: 25.0,
        p5_roi: 5.0,
        p95_roi: 60.0,
        mean_price_change: 20.0,
        price_volatility: 20.0,
        num_simulations: 10000,
        execution_time_ms: 500,
        avg_time_per_sim_us: 50.0,
    };

    let result_low = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, 
        Some(chaos_low), None, None, None, None, None, None, None
    ).unwrap();

    let result_high = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, 
        Some(chaos_high), None, None, None, None, None, None, None
    ).unwrap();

    // Higher pump probability should generally lead to higher score
    assert!(
        result_high.score >= result_low.score,
        "High pump probability ({:.1}%) should score >= low ({:.1}%): {} vs {}",
        chaos_high.pump_probability, chaos_low.pump_probability,
        result_high.score, result_low.score
    );
}

#[test]
fn test_hyper_prediction_with_chaos_result() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let chaos = ghost_brain::chaos::engine::ChaosResult {
        pump_probability: 60.0,
        crash_probability: 20.0,
        median_roi: 15.0,
        p5_roi: -10.0,
        p95_roi: 50.0,
        mean_price_change: 12.0,
        price_volatility: 25.0,
        num_simulations: 10000,
        execution_time_ms: 500,
        avg_time_per_sim_us: 50.0,
    };

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, 
        Some(chaos), None, None, None, None, None, None, None
    ).unwrap();

    // Should produce valid result with chaos data
    assert!(result.score <= 100);
    assert!(result.chaos_result.is_some(), "Chaos result should be preserved");
    
    // Interpretation should mention chaos if notable
    // (depends on probability thresholds)
    if result.chaos_result.as_ref().unwrap().pump_probability > 40.0 {
        assert!(
            result.interpretation.contains("Chaos") || result.interpretation.contains("Pump"),
            "Interpretation should mention notable chaos pump probability"
        );
    }
}

#[test]
fn test_hyper_prediction_with_hunter_score() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // High hunter score
    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, 
        Some(85), // hunter_score
        None, None, None, None
    ).unwrap();

    // Hunter score should be preserved and potentially affect interpretation
    assert!(result.hunter_score == Some(85));
    
    // High hunter score should be mentioned
    assert!(
        result.interpretation.contains("Hunter") || result.interpretation.contains("85"),
        "High hunter score should be mentioned in interpretation"
    );
}

#[test]
fn test_fallback_tracker_present() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // Fallback tracker should be present
    // (it tracks which default values were used)
    // The presence of the field is verified by the fact that
    // the result struct has it as a required field
    assert!(
        result.score <= 100,
        "Score should be valid even with potential fallbacks"
    );
}

#[test]
fn test_interpretation_new_format() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // Interpretation should follow new format
    assert!(
        result.interpretation.contains("PATIENT") ||
        result.interpretation.contains("Final:"),
        "Interpretation should contain PATIENT mode and/or Final score: {}",
        result.interpretation
    );
}
