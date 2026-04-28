//! Shared Test Fixtures for HyperPrediction Tests
//!
//! This module provides common test data and helper functions used across
//! multiple test files.

use ghost_brain::fast_pipeline::{EnhancedCandidate, CacheLinePadding};
use ghost_brain::pumpfun::PumpCurveStateCache;
use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
use solana_sdk::pubkey::Pubkey;

/// Create a standard test candidate with reasonable defaults
pub fn create_test_candidate() -> EnhancedCandidate {
    EnhancedCandidate {
        slot: Some(12345),
        pool_amm_id: Pubkey::new_unique(),
        amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".parse().unwrap(),
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        bonding_curve: Pubkey::new_unique(),
        timestamp: 1234567890,
        bonding_curve_progress: Some(0.05),
        initial_liquidity_sol: 10.0,
        token_total_supply: Some(1_000_000_000),
        signature: "test".to_string(),
        vanity_score: 50,
        has_dev_buy: true,
        dev_buy_sol: 3.0,
        mint_auth_disabled: true,
        metadata_len_score: 70,
        expected_price: None,
        shadow_bonding_progress: Some(5),
        virtual_sol_reserves: Some(35_000_000_000),
        shadow_market_cap: None,
        _hot_padding: [0; 4],
        _cache_barrier_1: CacheLinePadding::default(),
        _cache_barrier_2: CacheLinePadding::default(),
    }
}

/// Create a test oracle with default threshold
pub fn create_test_oracle() -> HyperPredictionOracle {
    HyperPredictionOracle::new(70)
}

/// Create a test oracle with custom threshold
pub fn create_test_oracle_with_threshold(threshold: u8) -> HyperPredictionOracle {
    HyperPredictionOracle::new(threshold)
}

/// Create an empty pumpfun cache for testing
pub fn create_test_cache() -> PumpCurveStateCache {
    PumpCurveStateCache::new()
}
