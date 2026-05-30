//! Utility Functions for HyperPrediction Oracle
//!
//! This module contains standalone utility functions that are used by the
//! HyperPrediction Oracle but don't require access to internal state.
//!
//! ## Functions
//!
//! - `is_pool_genesis()`: Check if pool is at initial state
//! - `calculate_recommended_delay()`: Calculate HFT delay from ParadoxState
//! - `convert_enhanced_to_candidate_pool()`: Type conversion helper
//!
//! ## Design Philosophy
//!
//! These functions are extracted from `mod.rs` to reduce file size and improve
//! modularity. They are pure functions that can be tested in isolation.

use crate::chaos::amm_math::AmmPool;
use crate::fast_pipeline::EnhancedCandidate;
use crate::pumpfun::{GENESIS_FEE_BPS, GENESIS_VIRTUAL_SOL_LAMPORTS, GENESIS_VIRTUAL_TOKEN_AMOUNT};
use seer::paradox_sensor::ParadoxState;
use solana_sdk::pubkey::Pubkey;

// =============================================================================
// Pool Genesis Detection
// =============================================================================

/// Check if pool state represents genesis (initial) values
///
/// Genesis pool is the initial state of a Pump.fun bonding curve:
/// - SOL reserve: 30 SOL (30_000_000_000 lamports)
/// - Token reserve: 1.073 trillion tokens (1_073_000_000_000_000)
/// - Fee: 1% (100 bps)
///
/// Uses integer arithmetic with 0.1% tolerance to detect genesis state.
///
/// # Arguments
/// * `pool` - The AMM pool to check
///
/// # Returns
/// `true` if pool reserves match genesis values (within 0.1% tolerance)
///
/// # Example
///
/// ```ignore
/// let pool = AmmPool::new(30_000_000_000, 1_073_000_000_000_000, 100).unwrap();
/// assert!(is_pool_genesis(&pool)); // True - matches genesis values
///
/// let post_trade_pool = AmmPool::new(35_000_000_000, 900_000_000_000_000, 100).unwrap();
/// assert!(!is_pool_genesis(&post_trade_pool)); // False - reserves changed
/// ```
pub fn is_pool_genesis(pool: &AmmPool) -> bool {
    // Use integer arithmetic for precision
    // Allow 0.1% tolerance (1/1000)
    let genesis_sol = GENESIS_VIRTUAL_SOL_LAMPORTS as u128;
    let genesis_token = GENESIS_VIRTUAL_TOKEN_AMOUNT;

    // Calculate absolute differences using saturating_sub to avoid underflow
    let sol_diff = if pool.reserve_a > genesis_sol {
        pool.reserve_a - genesis_sol
    } else {
        genesis_sol - pool.reserve_a
    };

    let token_diff = if pool.reserve_b > genesis_token {
        pool.reserve_b - genesis_token
    } else {
        genesis_token - pool.reserve_b
    };

    // 0.1% tolerance = diff < value / 1000
    let sol_match = sol_diff < genesis_sol / 1000;
    let token_match = token_diff < genesis_token / 1000;
    let fee_match = pool.fee_bps == GENESIS_FEE_BPS;

    sol_match && token_match && fee_match
}

// =============================================================================
// ParadoxSensor Integration
// =============================================================================

/// Calculate recommended delay based on ParadoxState metrics
///
/// This function determines how long to wait before entering a trade when
/// HFT activity is detected. Higher phase_sync and tension values indicate
/// more aggressive bot activity, requiring longer delays.
///
/// # Formula
/// - base_delay = 5000ms (5 seconds)
/// - sync_factor = phase_sync * 2000ms (0-2000ms additional)
/// - tension_factor = (tension / 100) * 3000ms (0-3000ms additional)
///
/// Total delay = base_delay + sync_factor + tension_factor
/// Clamped to [3000ms, 15000ms] range
///
/// # Arguments
/// * `paradox` - ParadoxState containing HFT metrics
///
/// # Returns
/// Recommended delay in milliseconds
///
/// # Example
///
/// ```ignore
/// let high_hft = ParadoxState {
///     phase_sync: 0.85,
///     tension: 80.0,
///     ..Default::default()
/// };
/// let delay = calculate_recommended_delay(&high_hft);
/// assert!(delay >= 5000); // At least 5 seconds due to high HFT activity
/// ```
pub fn calculate_recommended_delay(paradox: &ParadoxState) -> u64 {
    const BASE_DELAY_MS: u64 = 5000;
    const MAX_DELAY_MS: u64 = 15000;
    const MIN_DELAY_MS: u64 = 3000;

    let sync_factor = (paradox.phase_sync * 2000.0) as u64;
    let tension_factor = ((paradox.tension / 100.0) * 3000.0) as u64;

    let total_delay = BASE_DELAY_MS + sync_factor + tension_factor;
    total_delay.clamp(MIN_DELAY_MS, MAX_DELAY_MS)
}

// =============================================================================
// Type Conversion Helpers
// =============================================================================

/// Convert EnhancedCandidate to CandidatePool
///
/// This function converts the internal `EnhancedCandidate` type used by the
/// fast pipeline to the external `CandidatePool` type used by the seer crate.
///
/// # Arguments
/// * `candidate` - The enhanced candidate to convert
///
/// # Returns
/// A `CandidatePool` with fields mapped from the enhanced candidate
///
/// # Note
/// The `creator` field is set to `Pubkey::new_unique()` because the
/// `EnhancedCandidate` type doesn't store creator information.
/// This is acceptable for internal use where creator is not needed
/// for scoring or analysis decisions.
pub fn convert_enhanced_to_candidate_pool(
    candidate: &EnhancedCandidate,
) -> seer::types::CandidatePool {
    seer::types::CandidatePool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        slot: candidate.slot,
        tx_index: None,
        event_ts_ms: Some(candidate.timestamp.saturating_mul(1000)),
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: candidate.signature.clone(),
        amm_program_id: candidate.amm_program_id,
        pool_amm_id: candidate.pool_amm_id,
        base_mint: candidate.base_mint,
        quote_mint: candidate.quote_mint,
        bonding_curve: candidate.bonding_curve,
        // Note: EnhancedCandidate doesn't store creator, using placeholder
        // This field is not used in scoring/analysis decisions
        creator: Pubkey::new_unique(),
        timestamp: candidate.timestamp,
        bonding_curve_progress: candidate.bonding_curve_progress,
        initial_liquidity_sol: Some(candidate.initial_liquidity_sol),
        token_total_supply: candidate.token_total_supply,
        block_time: None,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pumpfun::{
        GENESIS_FEE_BPS, GENESIS_VIRTUAL_SOL_LAMPORTS, GENESIS_VIRTUAL_TOKEN_AMOUNT,
    };

    #[test]
    fn test_is_pool_genesis_matches_genesis() {
        let genesis_pool = AmmPool::new(
            GENESIS_VIRTUAL_SOL_LAMPORTS as u128,
            GENESIS_VIRTUAL_TOKEN_AMOUNT,
            GENESIS_FEE_BPS,
        )
        .unwrap();
        assert!(
            is_pool_genesis(&genesis_pool),
            "Genesis pool should be detected as genesis"
        );
    }

    #[test]
    fn test_is_pool_genesis_post_genesis() {
        // Post-genesis pool with different reserves
        let post_genesis_pool = AmmPool::new(
            35_000_000_000,      // 35 SOL (different from genesis 30 SOL)
            900_000_000_000_000, // Different token amount
            100,
        )
        .unwrap();
        assert!(
            !is_pool_genesis(&post_genesis_pool),
            "Post-genesis pool should NOT be detected as genesis"
        );
    }

    #[test]
    fn test_is_pool_genesis_different_fee() {
        // Pool with genesis reserves but different fee
        let different_fee_pool = AmmPool::new(
            GENESIS_VIRTUAL_SOL_LAMPORTS as u128,
            GENESIS_VIRTUAL_TOKEN_AMOUNT,
            50, // Different fee (0.5%)
        )
        .unwrap();
        assert!(
            !is_pool_genesis(&different_fee_pool),
            "Pool with different fee should NOT be genesis"
        );
    }

    #[test]
    fn test_calculate_recommended_delay_high_hft() {
        let high_hft = ParadoxState {
            tension: 80.0,
            jitter_ms: 2.0,
            density_bps: 150.0,
            anomaly_detected: true,
            derivative: 0.5,
            phase_sync: 0.85,
            pds_score: 85.0,
            is_echo_spike: false,
        };

        let delay = calculate_recommended_delay(&high_hft);
        assert!(
            delay >= 5000,
            "High HFT activity should trigger at least 5s delay"
        );
        assert!(delay <= 15000, "Delay should not exceed 15s max");
    }

    #[test]
    fn test_calculate_recommended_delay_low_hft() {
        let low_hft = ParadoxState {
            tension: 30.0,
            jitter_ms: 15.0,
            density_bps: 20.0,
            anomaly_detected: false,
            derivative: 0.0,
            phase_sync: 0.3,
            pds_score: 25.0,
            is_echo_spike: false,
        };

        let delay = calculate_recommended_delay(&low_hft);
        // With low activity, delay should be near minimum
        assert!(delay >= 3000, "Delay should be at least MIN_DELAY_MS");
    }

    #[test]
    fn test_calculate_recommended_delay_extreme_values() {
        // Maximum values
        let extreme = ParadoxState {
            tension: 100.0,
            jitter_ms: 1.0,
            density_bps: 200.0,
            anomaly_detected: true,
            derivative: 1.0,
            phase_sync: 1.0,
            pds_score: 100.0,
            is_echo_spike: true,
        };

        let delay = calculate_recommended_delay(&extreme);
        assert_eq!(delay, 15000, "Extreme values should hit max delay");
    }
}
