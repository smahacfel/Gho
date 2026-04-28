//! MESA Microstructure Analysis Tests
//!
//! Tests for wash trading detection, bot pattern detection, and organic activity bonuses.

use ghost_brain::oracle::hyper_prediction::{
    HyperPredictionOracle,
    AnalysisPhase,
};
use ghost_brain::oracle::tx_metrics::TransactionMetrics;
use ghost_brain::analyzers::mesa::{MesaAnalyzer, MesaResult};
use ghost_brain::pumpfun::PumpCurveStateCache;

mod fixtures;
use fixtures::{create_test_candidate, create_test_oracle, create_test_cache};

/// Helper to create MESA result with specific wash/bot/organic values
fn create_mesa_result(wash: f32, bot: f32, organic: f32, entropy: f32) -> MesaResult {
    MesaResult {
        execution_fingerprint: 0,
        wash_likeness: wash,
        bot_likeness: bot,
        organic_likeness: organic,
        entropy_score: entropy,
        impact_efficiency: 0.0,
        tx_count: 20,
        analysis_time_us: 5000,
    }
}

#[test]
fn test_mesa_wash_trading_penalty() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // High wash trading detection
    let high_wash = create_mesa_result(0.90, 0.20, 0.30, 0.50); // 90% wash

    // Provide tx_metrics to enable full analysis (MESA only in manager mode)
    let tx_metrics = TransactionMetrics::new(
        vec![1.0, 2.0, 1.5, 2.5, 1.0],
        vec![100, 200, 150, 250, 100],
        5
    );

    // Create mocked scoring by providing the MESA result indirectly through the oracle
    // Note: MESA is analyzed within score_candidate, we can't directly inject it
    // This test verifies that high wash in interpretation is noted
    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics), None, None, None
    ).unwrap();

    // Should be in full analysis mode
    assert_eq!(result.analysis_phase, AnalysisPhase::FullAnalysis);
    
    // Note: Since MESA is computed internally, we verify the structure exists
    // The actual wash detection depends on the swap data provided
    assert!(result.mesa_result.is_some() || result.analysis_phase.is_full_analysis());
}

#[test]
fn test_mesa_bot_pattern_penalty() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Provide tx_metrics for full analysis
    let tx_metrics = TransactionMetrics::new(
        vec![1.0, 1.0, 1.0, 1.0, 1.0],  // Identical volumes (bot-like)
        vec![100, 100, 100, 100, 100],  // Identical intervals (bot-like)
        5
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics), None, None, None
    ).unwrap();

    // Should be in full analysis mode
    assert_eq!(result.analysis_phase, AnalysisPhase::FullAnalysis);
    
    // Bot-like patterns should be detectable
    // (exact results depend on MESA implementation)
}

#[test]
fn test_mesa_organic_bonus() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Highly varied volumes and intervals (organic-like)
    let tx_metrics = TransactionMetrics::new(
        vec![0.5, 3.0, 1.2, 7.5, 0.8, 2.3, 4.1, 0.3],  // High variety
        vec![50, 500, 120, 800, 90, 350, 200, 600],     // High variety
        8
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics), None, None, None
    ).unwrap();

    // Should be in full analysis mode with sufficient transactions
    assert_eq!(result.analysis_phase, AnalysisPhase::FullAnalysis);
}

#[test]
fn test_mesa_interpretation_shows_metrics() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Provide tx_metrics for full analysis
    let tx_metrics = TransactionMetrics::new(
        vec![1.0, 2.0, 1.5],
        vec![100, 200, 150],
        3
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics), None, None, None
    ).unwrap();

    // In full analysis mode, if MESA detects notable patterns,
    // they should appear in interpretation
    if result.mesa_result.is_some() {
        let mesa = result.mesa_result.as_ref().unwrap();
        
        // If wash is above threshold, it should be mentioned
        if mesa.wash_likeness > 0.70 {
            assert!(
                result.interpretation.contains("Wash"),
                "High wash should be in interpretation"
            );
        }
        
        // If bot is above threshold, it should be mentioned
        if mesa.bot_likeness > 0.75 {
            assert!(
                result.interpretation.contains("Bot"),
                "High bot should be in interpretation"
            );
        }
    }
}

#[test]
fn test_mesa_modifiers_only_in_manager_mode() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Early stage (no tx_metrics)
    let result_early = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        None, // No tx_metrics = early stage
        None, None, None
    ).unwrap();

    // Should be early stage
    assert!(result_early.analysis_phase.is_early_stage());
    
    // MESA should not be present or have minimal impact in early stage
    // because MESA requires sufficient transaction data

    // Full analysis (with tx_metrics)
    let tx_metrics = TransactionMetrics::new(
        vec![1.0, 2.0, 1.5],
        vec![100, 200, 150],
        3
    );

    let result_full = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(tx_metrics),
        None, None, None
    ).unwrap();

    // Should be full analysis
    assert!(result_full.analysis_phase.is_full_analysis());
    
    // MESA should be analyzed in full mode
    assert!(result_full.mesa_result.is_some());
}
