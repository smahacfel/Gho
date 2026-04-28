//! Integration Tests for HyperPrediction Oracle
//!
//! These tests verify end-to-end functionality and veto conditions.

use ghost_brain::oracle::hyper_prediction::{
    HyperPredictionOracle,
    AnalysisPhase,
};
use ghost_brain::oracle::cluster_hunter::{ClusterAnalysis, ClusterMetric};
use ghost_brain::pumpfun::PumpCurveStateCache;
use std::time::Instant;

mod fixtures;
use fixtures::{create_test_candidate, create_test_oracle, create_test_cache};

#[test]
fn test_end_to_end_scoring() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate,
        &cache,
        None, // explicit_pool_state
        None, // tx_timestamps
        None, // tx_data
        None, // iwim_result
        None, // chaos_result
        None, // resonance_result
        None, // gene_safety_result
        None, // hunter_score
        None, // tx_metrics
        None, // cluster_result
        None, // paradox_state
        None, // tuned_weights
    ).unwrap();

    // Basic assertions
    assert!(result.score <= 100, "Score should not exceed 100");
    assert!(result.processing_time_us < 2_000_000, "Processing time should be < 2 seconds ({}μs)", result.processing_time_us);
    assert!(!result.interpretation.is_empty(), "Interpretation should not be empty");
    
    // Phase tracking
    assert!(
        result.analysis_phase == AnalysisPhase::EarlyStage,
        "Should be early stage with no tx_metrics"
    );
}

#[test]
fn test_veto_conditions_short_circuit() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Create high-risk cluster analysis that should trigger veto
    let cabal_cluster = ClusterAnalysis {
        risk_score: 0.8, // Above CABAL_RISK_THRESHOLD (0.65)
        holders: vec![],
        metrics: ClusterMetric {
            max_cluster_size: 5,
            controlled_supply_pct: 45.0,
            cluster_count: 2,
            total_clustered_holders: 10,
            ..Default::default()
        },
        ..Default::default()
    };

    let result = oracle.score_candidate(
        &candidate,
        &cache,
        None, None, None, None, None, None, None, None, None,
        Some(cabal_cluster), // cluster_result with high risk
        None,
        None,
    ).unwrap();

    // Should be vetoed
    assert_eq!(result.score, 0, "Cabal veto should result in score 0");
    assert!(!result.passed, "Cabal veto should fail");
    assert!(result.interpretation.contains("CLUSTER") || result.interpretation.contains("VETO"),
        "Interpretation should mention cluster veto: {}", result.interpretation);
    
    // Veto should be fast
    assert!(result.processing_time_us < 500_000, 
        "Veto should be fast (< 500ms), got {}μs", result.processing_time_us);
}

#[test]
fn test_performance_regression() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let iterations = 10; // Reduced for faster test
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = oracle.score_candidate(
            &candidate,
            &cache,
            None, None, None, None, None, None, None, None, None,
            None, None, None,
        );
    }

    let total_time = start.elapsed();
    let avg_time = total_time.as_micros() / iterations as u128;
    
    assert!(
        avg_time < 1_500_000,
        "Performance regression: average {}μs per call (limit: 1.5s)",
        avg_time
    );
    
    println!("Performance test: {} iterations, avg {}μs/call", iterations, avg_time);
}

#[test]
fn test_hyper_prediction_oracle_creation() {
    let oracle = HyperPredictionOracle::new(70);
    assert_eq!(oracle.threshold(), 70);
}

#[test]
fn test_hyper_prediction_basic_scoring() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // Basic validation
    assert!(result.score <= 100);
    assert!(!result.interpretation.is_empty());
}

#[test]
fn test_identical_candidates_produce_identical_scores() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    
    // First run
    let candidate1 = create_test_candidate();
    let result1 = oracle.score_candidate(
        &candidate1, &cache, None, None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // Second run with identical candidate
    let candidate2 = create_test_candidate();
    let result2 = oracle.score_candidate(
        &candidate2, &cache, None, None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // Scores should be identical
    assert_eq!(
        result1.score, result2.score,
        "Identical candidates should produce identical scores: {} vs {}",
        result1.score, result2.score
    );
}
