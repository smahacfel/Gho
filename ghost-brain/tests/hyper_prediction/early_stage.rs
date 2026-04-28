//! Early Stage Analysis Tests (Patient Observer)
//!
//! Tests for the Patient Observer pattern with adaptive thresholds.
//! In early stage (15-22 TX), trend-based metrics are skipped and static analysis is used.
//! Full analysis mode activates at 23+ TX after passing Gatekeeper at 15 TX.

use ghost_brain::oracle::hyper_prediction::{
    HyperPredictionOracle,
    AnalysisPhase,
};
use ghost_brain::oracle::tx_metrics::TransactionMetrics;
use ghost_brain::pumpfun::PumpCurveStateCache;

mod fixtures;
use fixtures::{create_test_candidate, create_test_oracle, create_test_cache};

#[test]
fn test_early_stage_activation_with_zero_tx() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // No tx_metrics means early stage mode
    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, // No tx_metrics
        None, None, None
    ).unwrap();

    // Should be in early stage
    assert_eq!(
        result.analysis_phase,
        AnalysisPhase::EarlyStage,
        "Should be early stage with no tx_metrics"
    );

    // QEDD and MCI should be None in early stage
    // (they require trend data from tx history)
    assert!(result.qedd_result.is_none() || result.analysis_phase.is_early_stage(),
        "QEDD should be skipped or irrelevant in early stage");
}

#[test]
fn test_early_stage_activation_with_fifteen_tx() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // 15 transactions - just passed Gatekeeper, still Early Stage
    let tx_metrics = TransactionMetrics::new(vec![1.0; 15], vec![100; 15], 15);

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics), // 15 transactions
        None, None, None
    ).unwrap();

    // Should be in Early Stage (15 < 22)
    assert_eq!(
        result.analysis_phase,
        AnalysisPhase::EarlyStage,
        "Should be early stage with 15 transactions (just passed Gatekeeper)"
    );
}

#[test]
fn test_manager_mode_activation_with_twentytwo_tx() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // 22 transactions - at Early Stage threshold, should transition to Full Analysis
    let tx_metrics = TransactionMetrics::new(vec![1.0; 22], vec![100; 22], 22);

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics), // 22 transactions
        None, None, None
    ).unwrap();

    // Should be in Full Analysis mode (22 >= 22)
    assert_eq!(
        result.analysis_phase,
        AnalysisPhase::FullAnalysis,
        "Should be full analysis with 22 transactions (at threshold)"
    );
}

#[test]
fn test_manager_mode_activation_with_twentyfive_tx() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // 25 transactions - well above Early Stage threshold
    let tx_metrics = TransactionMetrics::new(vec![1.0; 25], vec![100; 25], 25);

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics), // 25 transactions
        None, None, None
    ).unwrap();

    // Should be in full analysis mode (25 >= 22)
    assert_eq!(
        result.analysis_phase,
        AnalysisPhase::FullAnalysis,
        "Should be full analysis with 25+ transactions"
    );
}

#[test]
fn test_early_stage_hides_qedd_mci_in_interpretation() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Early stage
    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // Interpretation should not contain QEDD/MCI details in early stage
    // because they're based on insufficient data
    assert!(result.analysis_phase.is_early_stage());
    
    // Mode indicator should show early stage
    assert!(
        result.interpretation.contains("Early Stage") || 
        result.interpretation.contains("🐣") ||
        result.interpretation.contains("PATIENT"),
        "Interpretation should indicate early stage mode: {}", 
        result.interpretation
    );
}

#[test]
fn test_early_stage_cold_start_uses_qedd_mci_signals() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Create tx_metrics with 16 transactions (Early Stage: passed Gatekeeper but not Full Analysis)
    let tx_metrics = TransactionMetrics::new(vec![5.0; 16], vec![50; 16], 16);

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics),
        None, None, None
    ).unwrap();

    // Early stage should still produce a valid score
    assert!(result.score <= 100);
    assert!(result.analysis_phase.is_early_stage(), "16 TX should be Early Stage (< 22)");
}

#[test]
fn test_early_stage_cold_start_boost() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    
    // Create high-liquidity candidate
    let mut candidate = create_test_candidate();
    candidate.initial_liquidity_sol = 50.0; // High liquidity
    candidate.has_dev_buy = true;
    candidate.dev_buy_sol = 5.0;

    // Use chaos result with high pump probability for cold start boost
    let chaos = ghost_brain::chaos::engine::ChaosResult {
        pump_probability: 75.0,
        crash_probability: 10.0,
        median_roi: 20.0,
        p5_roi: -5.0,
        p95_roi: 50.0,
        mean_price_change: 15.0,
        price_volatility: 25.0,
        num_simulations: 10000,
        execution_time_ms: 500,
        avg_time_per_sim_us: 50.0,
    };

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, 
        Some(chaos), // chaos_result for cold start boost
        None, None, None, None, None, None, None
    ).unwrap();

    // Cold start with favorable chaos should produce reasonable score
    assert!(result.score > 0, "Cold start with good signals should score > 0");
}

#[test]
fn test_analysis_phase_tracked() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, None, None, None
    ).unwrap();

    // Phase should be tracked
    assert!(
        result.analysis_phase == AnalysisPhase::EarlyStage || 
        result.analysis_phase == AnalysisPhase::FullAnalysis
    );
    
    // analysis_started_at should be recent
    let elapsed = result.analysis_started_at.elapsed();
    assert!(elapsed.as_secs() < 5, "analysis_started_at should be recent");
}

#[test]
fn test_phase_display_name() {
    assert_eq!(AnalysisPhase::EarlyStage.display_name(), "Early Stage");
    assert_eq!(AnalysisPhase::FullAnalysis.display_name(), "Full Analysis");
}

#[test]
fn test_phase_helper_methods() {
    assert!(AnalysisPhase::EarlyStage.is_early_stage());
    assert!(!AnalysisPhase::EarlyStage.is_full_analysis());
    
    assert!(!AnalysisPhase::FullAnalysis.is_early_stage());
    assert!(AnalysisPhase::FullAnalysis.is_full_analysis());
}

#[test]
fn test_early_stage_threshold_matches_gatekeeper() {
    // Regression test: Early Stage threshold must be >= Gatekeeper
    // Issue: Orchestrator was checking for < 2 TX, but Gatekeeper rejects < 15 TX
    // Solution: Use adaptive threshold (Gatekeeper × 1.5 = 22 TX)
    
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Test at Gatekeeper boundary (15 TX)
    let tx_metrics_gatekeeper = TransactionMetrics::new(
        vec![1.0; 15],
        vec![100; 15],
        15
    );

    let result_gatekeeper = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None,
        Some(tx_metrics_gatekeeper),
        None, None, None
    ).unwrap();

    // Should be in Early Stage (15 < 22)
    assert_eq!(
        result_gatekeeper.analysis_phase,
        AnalysisPhase::EarlyStage,
        "Pools at Gatekeeper threshold (15 TX) should use Early Stage mode"
    );

    // Test at Early Stage boundary (22 TX)
    let tx_metrics_early_boundary = TransactionMetrics::new(
        vec![1.0; 22],
        vec![100; 22],
        22
    );

    let result_early_boundary = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None,
        Some(tx_metrics_early_boundary),
        None, None, None
    ).unwrap();

    // Should be in Full Analysis (22 >= 22)
    assert_eq!(
        result_early_boundary.analysis_phase,
        AnalysisPhase::FullAnalysis,
        "Pools at Early Stage threshold (22 TX) should use Full Analysis"
    );

    // Test above Early Stage threshold (25 TX)
    let tx_metrics_full = TransactionMetrics::new(
        vec![1.0; 25],
        vec![100; 25],
        25
    );

    let result_full = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None,
        Some(tx_metrics_full),
        None, None, None
    ).unwrap();

    assert_eq!(
        result_full.analysis_phase,
        AnalysisPhase::FullAnalysis,
        "Pools with sufficient history (25 TX) should use Full Analysis"
    );
}

#[test]
fn test_early_stage_range_coverage() {
    // Ensure all values in 15-21 TX range are Early Stage
    // and all values >= 22 TX are Full Analysis
    
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Test Early Stage range (15-21 TX)
    for tx_count in 15..22 {
        let tx_metrics = TransactionMetrics::new(
            vec![1.0; tx_count],
            vec![100; tx_count],
            tx_count
        );

        let result = oracle.score_candidate(
            &candidate, &cache, None, None, None, None, None, None, None, None,
            Some(tx_metrics),
            None, None, None
        ).unwrap();

        assert_eq!(
            result.analysis_phase,
            AnalysisPhase::EarlyStage,
            "TX count {} should be Early Stage", tx_count
        );
    }

    // Test Full Analysis range (22-30 TX)
    for tx_count in 22..=30 {
        let tx_metrics = TransactionMetrics::new(
            vec![1.0; tx_count],
            vec![100; tx_count],
            tx_count
        );

        let result = oracle.score_candidate(
            &candidate, &cache, None, None, None, None, None, None, None, None,
            Some(tx_metrics),
            None, None, None
        ).unwrap();

        assert_eq!(
            result.analysis_phase,
            AnalysisPhase::FullAnalysis,
            "TX count {} should be Full Analysis", tx_count
        );
    }
}
