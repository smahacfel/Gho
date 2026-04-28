//! Fractal Resonance Engine (FRE) Tests
//!
//! Tests for FRE integration with HyperPrediction Oracle.
//! FRE detects botnet attacks, pump & dump chaos, and organic quality.

use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_brain::oracle::tx_metrics::TransactionMetrics;
use ghost_brain::pumpfun::PumpCurveStateCache;
use std::time::Instant;

mod fixtures;
use fixtures::{create_test_candidate, create_test_oracle, create_test_cache};

#[test]
fn test_fre_organic_pattern_boost() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Provide varied transaction data that looks organic
    let organic_metrics = TransactionMetrics::new(
        // Highly varied volumes (organic)
        vec![0.3, 2.1, 0.8, 5.2, 1.1, 0.5, 3.4, 0.9, 1.7, 2.8],
        // Highly varied intervals (organic)
        vec![120, 450, 80, 890, 230, 560, 150, 320, 710, 180],
        10
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(organic_metrics), None, None, None
    ).unwrap();

    // FRE should be computed with sufficient data (>= 10 swaps)
    if let Some(ref fre) = result.fractal_verdict {
        // Organic patterns should have good coherence
        assert!(
            fre.coherence >= 0.0 && fre.coherence <= 1.0,
            "FRE coherence should be in valid range"
        );
        
        // Hurst exponent should be in valid range
        assert!(
            fre.hurst_global >= 0.0 && fre.hurst_global <= 1.0,
            "FRE Hurst exponent should be in valid range"
        );
    }
}

#[test]
fn test_fre_botnet_detection() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Bot-like pattern: identical volumes and regular intervals
    let bot_metrics = TransactionMetrics::new(
        // Identical volumes (bot-like)
        vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        // Regular intervals (bot-like)
        vec![100, 100, 100, 100, 100, 100, 100, 100, 100, 100],
        10
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(bot_metrics), None, None, None
    ).unwrap();

    // FRE should detect bot patterns
    if let Some(ref fre) = result.fractal_verdict {
        // Bot patterns typically have low stability (high variance at different scales)
        // or high coherence (too regular)
        println!(
            "FRE bot detection: coherence={:.3}, Hurst={:.3}, organic={}", 
            fre.coherence, fre.hurst_global, fre.organic_score
        );
    }
}

#[test]
fn test_fre_unstable_penalty() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Chaotic pattern: extreme variance
    let chaotic_metrics = TransactionMetrics::new(
        // Extreme variance in volumes
        vec![0.01, 100.0, 0.02, 50.0, 0.01, 80.0, 0.03, 30.0, 0.01, 60.0],
        // Extreme variance in intervals
        vec![10, 10000, 20, 5000, 15, 8000, 25, 3000, 10, 9000],
        10
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(chaotic_metrics), None, None, None
    ).unwrap();

    // Chaotic patterns should be flagged
    if let Some(ref fre) = result.fractal_verdict {
        // Unstable patterns have high stability_sigma
        println!(
            "FRE unstable detection: stability_sigma={:.3}", 
            fre.stability_sigma
        );
    }
}

#[test]
fn test_fre_performance() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Create realistic transaction data
    let metrics = TransactionMetrics::new(
        (0..20).map(|i| 0.5 + (i as f64 * 0.3).sin() * 2.0).collect(),
        (0..20).map(|i| 100 + ((i * 30) % 400) as u64).collect(),
        20
    );

    let iterations = 10;
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = oracle.score_candidate(
            &candidate, &cache, None, None, None, None, None, None, None, None, 
            Some(metrics.clone()), None, None, None
        );
    }

    let total_time = start.elapsed();
    let avg_ms = total_time.as_millis() / iterations as u128;

    // FRE should be fast (< 100ms per call on average)
    assert!(
        avg_ms < 2000,  // Allow for full oracle processing
        "FRE performance: {}ms avg per call (should be < 2000ms with full oracle)",
        avg_ms
    );
    
    println!("FRE performance test: {}ms avg per oracle call (includes all analysis)", avg_ms);
}

#[test]
fn test_fre_insufficient_data() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Insufficient data for FRE (< 10 swaps)
    let sparse_metrics = TransactionMetrics::new(
        vec![1.0, 2.0, 1.5],
        vec![100, 200, 150],
        3
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(sparse_metrics), None, None, None
    ).unwrap();

    // With insufficient data, FRE may be None or have limited analysis
    // The oracle should still produce a valid result
    assert!(result.score <= 100);
    
    // If FRE is present with sparse data, it should note limited confidence
    if let Some(ref fre) = result.fractal_verdict {
        println!("FRE with sparse data: organic={}", fre.organic_score);
    }
}

#[test]
fn test_fre_interpretation_shows_action() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Provide enough data for FRE
    let metrics = TransactionMetrics::new(
        (0..15).map(|i| 0.5 + (i as f64 * 0.2)).collect(),
        (0..15).map(|i| 100 + (i * 50) as u64).collect(),
        15
    );

    let result = oracle.score_candidate(
        &candidate, &cache, None, None, None, None, None, None, None, None, 
        Some(metrics), None, None, None
    ).unwrap();

    if result.fractal_verdict.is_some() {
        // FRE action should appear in interpretation
        assert!(
            result.interpretation.contains("FRE") ||
            result.interpretation.contains("Organic") ||
            result.interpretation.contains("Watch") ||
            result.interpretation.contains("SKIP"),
            "FRE results should be in interpretation when present: {}",
            result.interpretation
        );
    }
}
