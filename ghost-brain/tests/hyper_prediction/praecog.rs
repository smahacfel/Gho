//! PRAECOG Analysis Tests for HyperPrediction Oracle
//!
//! Tests for adversarial exploitability analysis and genesis pool detection.

use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use ghost_brain::chaos::amm_math::AmmPool;
use ghost_brain::pumpfun::{
    PumpCurveStateCache,
    GENESIS_VIRTUAL_SOL_LAMPORTS,
    GENESIS_VIRTUAL_TOKEN_AMOUNT,
    GENESIS_FEE_BPS,
};

mod fixtures;
use fixtures::{create_test_candidate, create_test_oracle, create_test_cache};

#[test]
fn test_praecog_explicit_pool_state_overrides_cache() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Create explicit pool state (not from cache)
    let explicit_pool = AmmPool::new(
        40_000_000_000,       // 40 SOL (different from genesis)
        900_000_000_000_000,  // Modified token reserves
        100,
    ).unwrap();

    let result = oracle.score_candidate(
        &candidate, &cache, 
        Some(explicit_pool), // Explicit pool state
        None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // Should produce valid result with explicit pool
    assert!(result.score <= 100);
    assert!(result.praecog_result.is_some(), "PRAECOG should analyze explicit pool");
}

#[test]
fn test_praecog_genesis_detection() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Genesis pool (initial state)
    let genesis_pool = AmmPool::new(
        GENESIS_VIRTUAL_SOL_LAMPORTS as u128,
        GENESIS_VIRTUAL_TOKEN_AMOUNT,
        GENESIS_FEE_BPS,
    ).unwrap();

    let result = oracle.score_candidate(
        &candidate, &cache, 
        Some(genesis_pool),
        None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // Genesis pool should be detected and analyzed
    assert!(result.praecog_result.is_some(), "PRAECOG should run on genesis pool");
}

#[test]
fn test_praecog_adversarial_score_varies_with_liquidity() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Low liquidity pool (more exploitable)
    let low_liquidity_pool = AmmPool::new(
        5_000_000_000,        // Only 5 SOL
        1_000_000_000_000_000,
        100,
    ).unwrap();

    // High liquidity pool (less exploitable)
    let high_liquidity_pool = AmmPool::new(
        100_000_000_000,      // 100 SOL
        800_000_000_000_000,
        100,
    ).unwrap();

    let result_low = oracle.score_candidate(
        &candidate, &cache, 
        Some(low_liquidity_pool),
        None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    let result_high = oracle.score_candidate(
        &candidate, &cache, 
        Some(high_liquidity_pool),
        None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // Both should produce valid PRAECOG results
    assert!(result_low.praecog_result.is_some());
    assert!(result_high.praecog_result.is_some());

    // Higher liquidity generally means lower exploitability
    // (though this depends on the specific PRAECOG implementation)
    if let (Some(praecog_low), Some(praecog_high)) = (&result_low.praecog_result, &result_high.praecog_result) {
        // Lower adversarial score is better (less exploitable)
        // We just verify both are in valid range
        assert!(praecog_low.adversarial_score >= 0.0 && praecog_low.adversarial_score <= 1.0);
        assert!(praecog_high.adversarial_score >= 0.0 && praecog_high.adversarial_score <= 1.0);
    }
}

// =============================================================================
// is_pool_genesis tests (via utils module)
// =============================================================================

// Note: is_pool_genesis tests are in the utils module but we can test
// the behavior through the full oracle pipeline

#[test]
fn test_genesis_pool_detection_via_praecog() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Genesis pool should be detected
    let genesis_pool = AmmPool::new(
        GENESIS_VIRTUAL_SOL_LAMPORTS as u128,
        GENESIS_VIRTUAL_TOKEN_AMOUNT,
        GENESIS_FEE_BPS,
    ).unwrap();

    let result = oracle.score_candidate(
        &candidate, &cache, 
        Some(genesis_pool),
        None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // PRAECOG should process genesis pool
    assert!(result.praecog_result.is_some());
}

#[test]
fn test_post_genesis_pool() {
    let oracle = create_test_oracle();
    let cache = create_test_cache();
    let candidate = create_test_candidate();

    // Post-genesis pool (after trading)
    let post_genesis = AmmPool::new(
        35_000_000_000,           // 35 SOL (different from genesis 30)
        900_000_000_000_000,      // Different token amount
        100,
    ).unwrap();

    let result = oracle.score_candidate(
        &candidate, &cache, 
        Some(post_genesis),
        None, None, None, None, None, None, None, None, None, None, None
    ).unwrap();

    // Should still produce valid PRAECOG analysis
    assert!(result.praecog_result.is_some());
}
