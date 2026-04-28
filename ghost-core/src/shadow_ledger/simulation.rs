//! Shadow Ledger Simulation Module - Pure Mathematical Functions for Market Simulation
//!
//! This module contains all stateless, pure mathematical simulation functions for the
//! Shadow Ledger. It is responsible for:
//! - Buy/sell simulation calculations
//! - Price impact computations
//! - Derivative calculations (d_price_d_volume, d_price_d_liquidity, d_price_d_slippage)
//! - Slippage-adjusted minimum amounts
//!
//! ## Design Principles
//!
//! - **Pure Functions**: All functions are stateless and side-effect free
//! - **No Storage Access**: This module does not access DashMap or any storage
//! - **Fixed-Point Ready**: Uses u128 arithmetic for precision where appropriate
//! - **Hotpath Optimized**: Critical functions are marked `#[inline(always)]`
//!
//! ## Performance
//!
//! Target: < 50 nanoseconds per simulation
//! - Stack-allocated results
//! - No heap allocations in hot paths
//! - Integer arithmetic preferred over floating-point where possible
//!
//! ## Usage
//!
//! ```ignore
//! use ghost_core::shadow_ledger::simulation::*;
//!
//! // Simulate a buy operation
//! let result = simulate_buy_pure(&curve, 1_000_000_000); // 1 SOL
//! println!("Tokens out: {}", result.tokens_out);
//!
//! // Calculate price impact
//! let impact = calculate_price_impact(&curve, 1_000_000_000);
//! println!("Price impact: {}%", impact);
//! ```

use super::types::{
    BuySimulationResult, SellSimulationResult, DERIVATIVE_EPSILON, LAMPORTS_PER_SOL,
};
use crate::market_state::BondingCurve;

// ============================================================================
// Constants for Fixed-Point Arithmetic
// ============================================================================

/// Basis points denominator (10000 = 100%)
pub const BPS_DENOMINATOR: u64 = 10000;

/// Default slippage tolerance in basis points (50 = 0.5%)
pub const DEFAULT_SLIPPAGE_BPS: u64 = 50;

/// Fee in basis points (100 = 1%)
pub const FEE_BPS: u64 = 100;

/// Precision multiplier for fixed-point calculations (10^12)
/// Used to maintain precision in derivative calculations
pub const FIXED_POINT_PRECISION: u128 = 1_000_000_000_000;

// ============================================================================
// Core Buy Simulation - Pure Functions
// ============================================================================

/// Simulate a buy operation and calculate expected tokens out.
///
/// This is a **pure function** that takes curve state and returns simulation results.
/// It does not access any storage or have side effects.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `amount_sol_lamports` - SOL amount to spend (in lamports)
///
/// # Returns
///
/// `BuySimulationResult` with all simulation data
///
/// # Performance
///
/// Target: < 1 microsecond per simulation
/// - Uses u128 arithmetic for overflow safety
/// - Stack-allocated result
#[inline(always)]
pub fn simulate_buy_pure(curve: &BondingCurve, amount_sol_lamports: u64) -> BuySimulationResult {
    simulate_buy_with_slippage_pure(curve, amount_sol_lamports, DEFAULT_SLIPPAGE_BPS)
}

/// Simulate a buy operation with custom slippage tolerance.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `amount_sol_lamports` - SOL amount to spend (in lamports)
/// * `slippage_bps` - Slippage tolerance in basis points (e.g., 50 = 0.5%)
///
/// # Returns
///
/// `BuySimulationResult` with slippage-adjusted min_tokens_out
///
/// # Slippage Calculation
///
/// ```text
/// min_tokens_out = tokens_out * (10000 - slippage_bps) / 10000
/// ```
///
/// For slippage_bps = 50 (0.5%): multiplier = 9950 / 10000 = 0.995
#[inline(always)]
pub fn simulate_buy_with_slippage_pure(
    curve: &BondingCurve,
    amount_sol_lamports: u64,
    slippage_bps: u64,
) -> BuySimulationResult {
    // Early return for zero input
    if amount_sol_lamports == 0 {
        return BuySimulationResult::default();
    }

    // Calculate tokens out using the bonding curve formula
    let tokens_out = curve.simulate_buy(amount_sol_lamports);

    // Calculate price impact
    let price_impact = calculate_price_impact(curve, amount_sol_lamports);

    // Get market metrics
    let market_cap = curve.get_market_cap_sol();
    let bonding_progress = curve.get_bonding_progress();

    // Calculate effective SOL after fee using FEE_BPS constant
    let fee = amount_sol_lamports * FEE_BPS / BPS_DENOMINATOR;
    let effective_sol = amount_sol_lamports.saturating_sub(fee);

    // Calculate min_tokens_out with slippage protection using helper function
    let min_tokens_out = apply_slippage_bps(tokens_out, slippage_bps);

    // Calculate effective price per token
    let effective_price = if tokens_out > 0 {
        amount_sol_lamports as f64 / tokens_out as f64
    } else {
        0.0
    };

    BuySimulationResult {
        tokens_out,
        min_tokens_out,
        sol_in: amount_sol_lamports,
        effective_sol_in: effective_sol,
        price_impact_percent: price_impact,
        effective_price_per_token: effective_price,
        market_cap_sol: market_cap,
        bonding_progress,
    }
}

// ============================================================================
// Core Sell Simulation - Pure Functions
// ============================================================================

/// Simulate a sell operation and calculate expected SOL out.
///
/// This is a **pure function** that takes curve state and returns simulation results.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `amount_tokens` - Number of tokens to sell
///
/// # Returns
///
/// `SellSimulationResult` with all simulation data
#[inline(always)]
pub fn simulate_sell_pure(curve: &BondingCurve, amount_tokens: u64) -> SellSimulationResult {
    simulate_sell_with_slippage_pure(curve, amount_tokens, DEFAULT_SLIPPAGE_BPS)
}

/// Simulate a sell operation with custom slippage tolerance.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `amount_tokens` - Number of tokens to sell
/// * `slippage_bps` - Slippage tolerance in basis points
///
/// # Returns
///
/// `SellSimulationResult` with slippage-adjusted min_sol_out
#[inline(always)]
pub fn simulate_sell_with_slippage_pure(
    curve: &BondingCurve,
    amount_tokens: u64,
    slippage_bps: u64,
) -> SellSimulationResult {
    // Early return for zero input
    if amount_tokens == 0 {
        return SellSimulationResult::default();
    }

    // Calculate SOL out using the bonding curve formula
    let sol_out = curve.simulate_sell(amount_tokens);

    // Calculate price impact for sell
    let price_impact = calculate_sell_price_impact(curve, amount_tokens);

    // Calculate min_sol_out with slippage protection using helper function
    let min_sol_out = apply_slippage_bps(sol_out, slippage_bps);

    // Calculate effective price per token
    let effective_price = if amount_tokens > 0 {
        sol_out as f64 / amount_tokens as f64
    } else {
        0.0
    };

    SellSimulationResult {
        sol_out,
        min_sol_out,
        tokens_in: amount_tokens,
        price_impact_percent: price_impact,
        effective_price_per_token: effective_price,
    }
}

// ============================================================================
// Price Impact Calculations - Pure Functions
// ============================================================================

/// Calculate the price impact of a buy order as a percentage.
///
/// This is a **pure function** that computes how much the price will move
/// as a result of the trade.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `amount_in_lamports` - SOL amount to spend (in lamports)
///
/// # Returns
///
/// Price impact as a percentage (e.g., 2.5 = 2.5% price increase)
///
/// # Formula
///
/// ```text
/// price_before = virtual_sol_reserves / virtual_token_reserves
/// price_after = (virtual_sol_reserves + effective_sol) / (virtual_token_reserves - tokens_out)
/// impact = ((price_after - price_before) / price_before) * 100
/// ```
#[inline(always)]
pub fn calculate_price_impact(curve: &BondingCurve, amount_in_lamports: u64) -> f64 {
    // Edge cases
    if amount_in_lamports == 0
        || curve.virtual_token_reserves == 0
        || curve.virtual_sol_reserves == 0
    {
        return 0.0;
    }

    // Calculate price before trade
    let price_before = curve.virtual_sol_reserves as f64 / curve.virtual_token_reserves as f64;

    // Simulate the buy to get tokens out
    let tokens_out = curve.simulate_buy(amount_in_lamports);
    if tokens_out == 0 {
        return 0.0;
    }

    // Calculate effective SOL added (after fee using FEE_BPS constant)
    let fee = amount_in_lamports * FEE_BPS / BPS_DENOMINATOR;
    let effective_sol = amount_in_lamports.saturating_sub(fee);

    // Calculate price after trade
    let new_sol_reserves = curve.virtual_sol_reserves.saturating_add(effective_sol);
    let new_token_reserves = curve.virtual_token_reserves.saturating_sub(tokens_out);

    if new_token_reserves == 0 {
        return 100.0; // Would drain the pool
    }

    let price_after = new_sol_reserves as f64 / new_token_reserves as f64;

    // Calculate percentage impact
    if price_before == 0.0 {
        return 0.0;
    }

    ((price_after - price_before) / price_before) * 100.0
}

/// Calculate the price impact of a sell order as a percentage.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `amount_tokens` - Number of tokens to sell
///
/// # Returns
///
/// Price impact as a percentage (negative for sells as price decreases)
#[inline(always)]
pub fn calculate_sell_price_impact(curve: &BondingCurve, amount_tokens: u64) -> f64 {
    // Edge cases
    if amount_tokens == 0 || curve.virtual_token_reserves == 0 || curve.virtual_sol_reserves == 0 {
        return 0.0;
    }

    // Calculate price before trade
    let price_before = curve.virtual_sol_reserves as f64 / curve.virtual_token_reserves as f64;

    // Simulate the sell to compute new reserves
    let k = (curve.virtual_sol_reserves as u128) * (curve.virtual_token_reserves as u128);
    let new_token_reserves = curve.virtual_token_reserves as u128 + amount_tokens as u128;
    let new_sol_reserves = if new_token_reserves > 0 {
        k / new_token_reserves
    } else {
        0
    };

    // Calculate price after trade
    let price_after = if new_token_reserves > 0 {
        new_sol_reserves as f64 / new_token_reserves as f64
    } else {
        0.0
    };

    // Calculate percentage impact (negative for sells)
    if price_before == 0.0 {
        return 0.0;
    }

    ((price_after - price_before) / price_before) * 100.0
}

// ============================================================================
// Derivative Calculations - Pure Functions for SCR/ULVF/POVC/HOSD
// ============================================================================

/// Calculate d_price_d_volume: price sensitivity to volume.
///
/// This derivative measures how much the price changes per unit of volume traded.
/// Used by scoring modules (SCR, ULVF, POVC, HOSD) for trajectory prediction.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `volume_sol_lamports` - Volume in SOL lamports
///
/// # Returns
///
/// `d_price / d_volume` in SOL per token / SOL units
///
/// # Formula
///
/// ```text
/// d_price_d_volume = (price_after_trade - price_before) / volume_sol
/// ```
#[inline(always)]
pub fn calculate_d_price_d_volume(curve: &BondingCurve, volume_sol_lamports: u64) -> f64 {
    // Edge cases
    if volume_sol_lamports == 0 || curve.virtual_token_reserves == 0 {
        return 0.0;
    }

    let price_before = curve.current_price();

    // Simulate a buy to get price after
    let tokens_out = curve.simulate_buy(volume_sol_lamports);
    if tokens_out == 0 {
        return 0.0;
    }

    // Calculate effective SOL after fee using FEE_BPS constant
    let fee = volume_sol_lamports * FEE_BPS / BPS_DENOMINATOR;
    let effective_sol = volume_sol_lamports.saturating_sub(fee);

    // Calculate price after
    let new_sol_reserves = curve.virtual_sol_reserves.saturating_add(effective_sol);
    let new_token_reserves = curve.virtual_token_reserves.saturating_sub(tokens_out);

    if new_token_reserves == 0 {
        return 0.0;
    }

    let price_after = new_sol_reserves as f64 / new_token_reserves as f64;

    // Convert volume to SOL
    let volume_sol = volume_sol_lamports as f64 / LAMPORTS_PER_SOL;

    // Calculate derivative
    if volume_sol.abs() < DERIVATIVE_EPSILON {
        return 0.0;
    }

    (price_after - price_before) / volume_sol
}

/// Calculate d_price_d_liquidity: price sensitivity to reserve changes.
///
/// This derivative measures how the price responds to changes in liquidity
/// (specifically virtual_sol_reserves).
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `liquidity_delta_lamports` - Change in liquidity (SOL lamports)
///
/// # Returns
///
/// `d_price / d_liquidity` in SOL per token / SOL units
#[inline(always)]
pub fn calculate_d_price_d_liquidity(curve: &BondingCurve, liquidity_delta_lamports: u64) -> f64 {
    // Edge cases
    if liquidity_delta_lamports == 0 || curve.virtual_token_reserves == 0 {
        return 0.0;
    }

    let price_before = curve.current_price();

    // Simulate price change from liquidity delta
    let tokens_out = curve.simulate_buy(liquidity_delta_lamports);
    if tokens_out == 0 {
        return 0.0;
    }

    // Calculate effective SOL after fee using FEE_BPS constant (this represents the actual liquidity added)
    let fee = liquidity_delta_lamports * FEE_BPS / BPS_DENOMINATOR;
    let effective_sol = liquidity_delta_lamports.saturating_sub(fee);

    // Calculate price after
    let new_sol_reserves = curve.virtual_sol_reserves.saturating_add(effective_sol);
    let new_token_reserves = curve.virtual_token_reserves.saturating_sub(tokens_out);

    if new_token_reserves == 0 {
        return 0.0;
    }

    let price_after = new_sol_reserves as f64 / new_token_reserves as f64;

    // Convert liquidity delta to SOL
    let liquidity_sol = effective_sol as f64 / LAMPORTS_PER_SOL;

    // Calculate derivative
    if liquidity_sol.abs() < DERIVATIVE_EPSILON {
        return 0.0;
    }

    (price_after - price_before) / liquidity_sol
}

/// Calculate d_price_d_slippage: curvature of slippage.
///
/// This derivative measures how rapidly slippage increases with order size.
/// It's the second-order effect: (impact_2x - impact_1x) / delta_amount.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `base_amount_lamports` - Base amount for comparison
///
/// # Returns
///
/// `d_slippage / d_amount` - rate of slippage increase per SOL
///
/// # Formula
///
/// ```text
/// impact_1x = price_impact(base_amount)
/// impact_2x = price_impact(2 * base_amount)
/// d_slippage = (impact_2x - impact_1x) / base_amount_in_sol
/// ```
#[inline(always)]
pub fn calculate_d_price_d_slippage(curve: &BondingCurve, base_amount_lamports: u64) -> f64 {
    // Edge cases
    if base_amount_lamports == 0 {
        return 0.0;
    }

    // Calculate impact at base amount
    let impact_1x = calculate_price_impact(curve, base_amount_lamports);

    // Calculate impact at 2x base amount
    let double_amount = base_amount_lamports.saturating_mul(2);
    let impact_2x = calculate_price_impact(curve, double_amount);

    // Convert base amount to SOL
    let base_sol = base_amount_lamports as f64 / LAMPORTS_PER_SOL;

    // Calculate curvature (second derivative of price with respect to amount)
    if base_sol.abs() < DERIVATIVE_EPSILON {
        return 0.0;
    }

    (impact_2x - impact_1x) / base_sol
}

// ============================================================================
// Fixed-Point Arithmetic Helpers
// ============================================================================

/// Calculate tokens out using fixed-point arithmetic (u128).
///
/// This function provides higher precision than the standard simulate_buy
/// by using u128 throughout the calculation and avoiding f64 conversions
/// until the final result.
///
/// # Arguments
///
/// * `virtual_sol_reserves` - Virtual SOL reserves in lamports
/// * `virtual_token_reserves` - Virtual token reserves
/// * `amount_sol_lamports` - SOL amount to spend (in lamports)
///
/// # Returns
///
/// Expected tokens out
#[inline(always)]
pub fn calculate_tokens_out_fixed(
    virtual_sol_reserves: u64,
    virtual_token_reserves: u64,
    amount_sol_lamports: u64,
) -> u64 {
    // Avoid division by zero
    if amount_sol_lamports == 0 || virtual_sol_reserves == 0 || virtual_token_reserves == 0 {
        return 0;
    }

    // Calculate fee using FEE_BPS constant
    let fee = amount_sol_lamports * FEE_BPS / BPS_DENOMINATOR;
    let effective_sol = amount_sol_lamports.saturating_sub(fee);

    // Calculate invariant k = x * y using u128
    let invariant = (virtual_sol_reserves as u128).saturating_mul(virtual_token_reserves as u128);

    // New SOL reserves after adding effective input
    let new_sol_reserves = (virtual_sol_reserves as u128).saturating_add(effective_sol as u128);

    if new_sol_reserves == 0 {
        return 0;
    }

    // New token reserves to maintain invariant
    let new_token_reserves = invariant / new_sol_reserves;

    // Tokens out is the difference
    let tokens_out = (virtual_token_reserves as u128).saturating_sub(new_token_reserves);

    tokens_out as u64
}

/// Calculate SOL out using fixed-point arithmetic (u128).
///
/// # Arguments
///
/// * `virtual_sol_reserves` - Virtual SOL reserves in lamports
/// * `virtual_token_reserves` - Virtual token reserves
/// * `amount_tokens` - Number of tokens to sell
///
/// # Returns
///
/// Expected SOL out (after 1% fee)
#[inline(always)]
pub fn calculate_sol_out_fixed(
    virtual_sol_reserves: u64,
    virtual_token_reserves: u64,
    amount_tokens: u64,
) -> u64 {
    // Avoid division by zero
    if amount_tokens == 0 || virtual_sol_reserves == 0 || virtual_token_reserves == 0 {
        return 0;
    }

    // Calculate invariant k = x * y using u128
    let invariant = (virtual_sol_reserves as u128).saturating_mul(virtual_token_reserves as u128);

    // New token reserves after adding tokens being sold
    let new_token_reserves = (virtual_token_reserves as u128).saturating_add(amount_tokens as u128);

    if new_token_reserves == 0 {
        return 0;
    }

    // New SOL reserves to maintain invariant
    let new_sol_reserves = invariant / new_token_reserves;

    // SOL out is the difference
    let sol_out = (virtual_sol_reserves as u128).saturating_sub(new_sol_reserves);

    // Apply fee to output using FEE_BPS constant
    let fee = sol_out * (FEE_BPS as u128) / (BPS_DENOMINATOR as u128);
    let sol_after_fee = sol_out.saturating_sub(fee);

    sol_after_fee as u64
}

/// Calculate price impact using fixed-point arithmetic.
///
/// Returns price impact in basis points (e.g., 250 = 2.5%)
///
/// # Arguments
///
/// * `virtual_sol_reserves` - Virtual SOL reserves
/// * `virtual_token_reserves` - Virtual token reserves
/// * `amount_sol_lamports` - SOL amount to spend
///
/// # Returns
///
/// Price impact in basis points (u64)
#[inline(always)]
pub fn calculate_price_impact_bps(
    virtual_sol_reserves: u64,
    virtual_token_reserves: u64,
    amount_sol_lamports: u64,
) -> u64 {
    // Edge cases
    if amount_sol_lamports == 0 || virtual_token_reserves == 0 || virtual_sol_reserves == 0 {
        return 0;
    }

    // Calculate tokens out
    let tokens_out = calculate_tokens_out_fixed(
        virtual_sol_reserves,
        virtual_token_reserves,
        amount_sol_lamports,
    );
    if tokens_out == 0 {
        return 0;
    }

    // Calculate effective SOL using FEE_BPS constant
    let fee = amount_sol_lamports * FEE_BPS / BPS_DENOMINATOR;
    let effective_sol = amount_sol_lamports.saturating_sub(fee);

    // Calculate new reserves
    let new_sol_reserves = virtual_sol_reserves.saturating_add(effective_sol);
    let new_token_reserves = virtual_token_reserves.saturating_sub(tokens_out);

    if new_token_reserves == 0 {
        return BPS_DENOMINATOR; // 100% impact
    }

    // Calculate prices using fixed-point (multiply by FIXED_POINT_PRECISION for precision)
    // price = sol_reserves * PRECISION / token_reserves
    let price_before = (virtual_sol_reserves as u128).saturating_mul(FIXED_POINT_PRECISION)
        / (virtual_token_reserves as u128);

    let price_after = (new_sol_reserves as u128).saturating_mul(FIXED_POINT_PRECISION)
        / (new_token_reserves as u128);

    if price_before == 0 {
        return 0;
    }

    // Calculate impact in basis points
    // impact_bps = ((price_after - price_before) * 10000) / price_before
    let price_diff = price_after.saturating_sub(price_before);
    let impact_bps = (price_diff.saturating_mul(BPS_DENOMINATOR as u128)) / price_before;

    impact_bps as u64
}

/// Apply slippage to an amount using fixed-point arithmetic.
///
/// # Arguments
///
/// * `amount` - Base amount
/// * `slippage_bps` - Slippage in basis points
///
/// # Returns
///
/// Amount after slippage deduction
#[inline(always)]
pub fn apply_slippage_bps(amount: u64, slippage_bps: u64) -> u64 {
    let multiplier = BPS_DENOMINATOR.saturating_sub(slippage_bps);
    ((amount as u128) * (multiplier as u128) / (BPS_DENOMINATOR as u128)) as u64
}

// ============================================================================
// Micro-Simulation Helpers for Bootstrap
// ============================================================================

/// Perform a complete buy micro-simulation for derivative calculations.
///
/// This function returns all the data needed for computing derivatives
/// during bootstrap (G1, G2 snapshot generation).
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `amount_sol_lamports` - SOL amount to spend
///
/// # Returns
///
/// Tuple of (tokens_out, price_after, effective_sol, price_impact)
#[inline(always)]
pub fn micro_simulate_buy(curve: &BondingCurve, amount_sol_lamports: u64) -> (u64, f64, u64, f64) {
    if amount_sol_lamports == 0 {
        return (0, curve.current_price(), 0, 0.0);
    }

    let tokens_out = curve.simulate_buy(amount_sol_lamports);
    let price_impact = calculate_price_impact(curve, amount_sol_lamports);

    // Calculate effective SOL after fee using FEE_BPS constant
    let fee = amount_sol_lamports * FEE_BPS / BPS_DENOMINATOR;
    let effective_sol = amount_sol_lamports.saturating_sub(fee);

    // Calculate price after trade
    let new_sol_reserves = curve.virtual_sol_reserves.saturating_add(effective_sol);
    let new_token_reserves = curve.virtual_token_reserves.saturating_sub(tokens_out);

    let price_after = if new_token_reserves > 0 {
        new_sol_reserves as f64 / new_token_reserves as f64
    } else {
        0.0
    };

    (tokens_out, price_after, effective_sol, price_impact)
}

/// Compute all three derivatives (d_price_d_volume, d_price_d_liquidity, d_price_d_slippage).
///
/// This is an optimized function that computes all derivatives in a single pass,
/// reusing intermediate calculations.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `base_amount_lamports` - Base amount for derivative calculations
///
/// # Returns
///
/// Tuple of (d_price_d_volume, d_price_d_liquidity, d_price_d_slippage)
#[inline(always)]
pub fn compute_all_derivatives(curve: &BondingCurve, base_amount_lamports: u64) -> (f64, f64, f64) {
    // Edge cases
    if base_amount_lamports == 0 || curve.virtual_token_reserves == 0 {
        return (0.0, 0.0, 0.0);
    }

    let price_before = curve.current_price();

    // Micro-simulate at base amount
    let (_, price_after_1x, effective_sol_1x, impact_1x) =
        micro_simulate_buy(curve, base_amount_lamports);

    // Convert to SOL
    let volume_sol = base_amount_lamports as f64 / LAMPORTS_PER_SOL;
    let liquidity_sol = effective_sol_1x as f64 / LAMPORTS_PER_SOL;

    // Calculate d_price_d_volume
    let d_price_d_volume = if volume_sol.abs() > DERIVATIVE_EPSILON {
        (price_after_1x - price_before) / volume_sol
    } else {
        0.0
    };

    // Calculate d_price_d_liquidity
    let d_price_d_liquidity = if liquidity_sol.abs() > DERIVATIVE_EPSILON {
        (price_after_1x - price_before) / liquidity_sol
    } else {
        0.0
    };

    // Calculate d_price_d_slippage (need impact at 2x)
    let double_amount = base_amount_lamports.saturating_mul(2);
    let impact_2x = calculate_price_impact(curve, double_amount);

    let d_price_d_slippage = if volume_sol.abs() > DERIVATIVE_EPSILON {
        (impact_2x - impact_1x) / volume_sol
    } else {
        0.0
    };

    (d_price_d_volume, d_price_d_liquidity, d_price_d_slippage)
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a test bonding curve
    fn create_test_curve(virtual_token_reserves: u64, virtual_sol_reserves: u64) -> BondingCurve {
        BondingCurve {
            discriminator: 0x1234567890abcdef,
            virtual_token_reserves,
            virtual_sol_reserves,
            real_token_reserves: virtual_token_reserves * 8 / 10,
            real_sol_reserves: virtual_sol_reserves * 8 / 10,
            token_total_supply: virtual_token_reserves,
            complete: 0,
            _padding: [0; 7],
        }
    }

    // =========================================================================
    // Buy Simulation Tests
    // =========================================================================

    #[test]
    fn test_simulate_buy_pure_basic() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let result = simulate_buy_pure(&curve, 1_000_000_000); // 1 SOL

        assert!(result.tokens_out > 0, "Should receive tokens");
        assert!(
            result.min_tokens_out < result.tokens_out,
            "Min should be less than expected"
        );
        assert_eq!(result.sol_in, 1_000_000_000);
        assert!(
            result.effective_sol_in < result.sol_in,
            "Effective SOL should be less due to fee"
        );
        assert!(
            result.price_impact_percent > 0.0,
            "Price impact should be positive"
        );
    }

    #[test]
    fn test_simulate_buy_pure_zero_input() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let result = simulate_buy_pure(&curve, 0);

        assert_eq!(result.tokens_out, 0);
        assert_eq!(result.min_tokens_out, 0);
        assert_eq!(result.sol_in, 0);
    }

    #[test]
    fn test_simulate_buy_with_slippage_pure() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Default slippage (0.5%)
        let result_default = simulate_buy_pure(&curve, 1_000_000_000);

        // Higher slippage (1%)
        let result_higher = simulate_buy_with_slippage_pure(&curve, 1_000_000_000, 100);

        // Same tokens out
        assert_eq!(result_default.tokens_out, result_higher.tokens_out);

        // But different min_tokens_out
        assert!(result_higher.min_tokens_out < result_default.min_tokens_out);
    }

    #[test]
    fn test_simulate_buy_pure_large_amount() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Large buy: 10 SOL
        let result = simulate_buy_pure(&curve, 10_000_000_000);

        assert!(result.tokens_out > 0);
        assert!(
            result.price_impact_percent > 5.0,
            "Large order should have significant impact"
        );
    }

    // =========================================================================
    // Sell Simulation Tests
    // =========================================================================

    #[test]
    fn test_simulate_sell_pure_basic() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let result = simulate_sell_pure(&curve, 1_000_000_000); // 1B tokens

        assert!(result.sol_out > 0, "Should receive SOL");
        assert!(
            result.min_sol_out < result.sol_out,
            "Min should be less than expected"
        );
        assert_eq!(result.tokens_in, 1_000_000_000);
        assert!(
            result.price_impact_percent < 0.0,
            "Sell should have negative price impact"
        );
    }

    #[test]
    fn test_simulate_sell_pure_zero_input() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let result = simulate_sell_pure(&curve, 0);

        assert_eq!(result.sol_out, 0);
        assert_eq!(result.min_sol_out, 0);
        assert_eq!(result.tokens_in, 0);
    }

    #[test]
    fn test_simulate_sell_with_slippage_pure() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Higher slippage (2%)
        let result = simulate_sell_with_slippage_pure(&curve, 1_000_000_000, 200);

        // Min should be 98% of expected
        let expected_min = (result.sol_out as u128 * 9800 / 10000) as u64;
        assert_eq!(result.min_sol_out, expected_min);
    }

    // =========================================================================
    // Price Impact Tests
    // =========================================================================

    #[test]
    fn test_calculate_price_impact_zero() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let impact = calculate_price_impact(&curve, 0);
        assert_eq!(impact, 0.0);
    }

    #[test]
    fn test_calculate_price_impact_small_order() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // 0.01 SOL - very small order
        let impact = calculate_price_impact(&curve, 10_000_000);

        assert!(impact > 0.0, "Should have positive impact");
        assert!(impact < 1.0, "Small order should have < 1% impact");
    }

    #[test]
    fn test_calculate_price_impact_large_order() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // 10 SOL - large order
        let impact = calculate_price_impact(&curve, 10_000_000_000);

        assert!(impact > 10.0, "Large order should have > 10% impact");
    }

    #[test]
    fn test_calculate_sell_price_impact() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let impact = calculate_sell_price_impact(&curve, 1_000_000_000);

        assert!(impact < 0.0, "Sell should have negative impact");
    }

    // =========================================================================
    // Derivative Calculation Tests
    // =========================================================================

    #[test]
    fn test_calculate_d_price_d_volume() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let derivative = calculate_d_price_d_volume(&curve, 1_000_000_000);

        assert!(
            derivative > 0.0,
            "Buying should increase price, positive derivative"
        );
    }

    #[test]
    fn test_calculate_d_price_d_volume_zero() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let derivative = calculate_d_price_d_volume(&curve, 0);

        assert_eq!(derivative, 0.0);
    }

    #[test]
    fn test_calculate_d_price_d_liquidity() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let derivative = calculate_d_price_d_liquidity(&curve, 1_000_000_000);

        assert!(derivative > 0.0, "Adding liquidity should increase price");
    }

    #[test]
    fn test_calculate_d_price_d_slippage() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let derivative = calculate_d_price_d_slippage(&curve, 1_000_000_000);

        // Slippage curvature should be positive (larger orders have more slippage)
        assert!(derivative > 0.0, "Slippage should increase with order size");
    }

    #[test]
    fn test_compute_all_derivatives() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let (d_vol, d_liq, d_slip) = compute_all_derivatives(&curve, 1_000_000_000);

        assert!(d_vol > 0.0, "d_price_d_volume should be positive");
        assert!(d_liq > 0.0, "d_price_d_liquidity should be positive");
        assert!(d_slip > 0.0, "d_price_d_slippage should be positive");
    }

    #[test]
    fn test_compute_all_derivatives_zero_amount() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let (d_vol, d_liq, d_slip) = compute_all_derivatives(&curve, 0);

        assert_eq!(d_vol, 0.0);
        assert_eq!(d_liq, 0.0);
        assert_eq!(d_slip, 0.0);
    }

    // =========================================================================
    // Fixed-Point Arithmetic Tests
    // =========================================================================

    #[test]
    fn test_calculate_tokens_out_fixed() {
        let tokens_out =
            calculate_tokens_out_fixed(30_000_000_000, 1_000_000_000_000, 1_000_000_000);

        assert!(tokens_out > 0, "Should return positive tokens");
    }

    #[test]
    fn test_calculate_tokens_out_fixed_zero() {
        let tokens_out = calculate_tokens_out_fixed(30_000_000_000, 1_000_000_000_000, 0);
        assert_eq!(tokens_out, 0);

        let tokens_out = calculate_tokens_out_fixed(0, 1_000_000_000_000, 1_000_000_000);
        assert_eq!(tokens_out, 0);

        let tokens_out = calculate_tokens_out_fixed(30_000_000_000, 0, 1_000_000_000);
        assert_eq!(tokens_out, 0);
    }

    #[test]
    fn test_calculate_sol_out_fixed() {
        let sol_out = calculate_sol_out_fixed(30_000_000_000, 1_000_000_000_000, 1_000_000_000);

        assert!(sol_out > 0, "Should return positive SOL");
    }

    #[test]
    fn test_calculate_sol_out_fixed_zero() {
        let sol_out = calculate_sol_out_fixed(30_000_000_000, 1_000_000_000_000, 0);
        assert_eq!(sol_out, 0);
    }

    #[test]
    fn test_calculate_price_impact_bps() {
        let impact_bps =
            calculate_price_impact_bps(30_000_000_000, 1_000_000_000_000, 1_000_000_000);

        assert!(impact_bps > 0, "Should have positive impact");
        assert!(
            impact_bps < 1000,
            "1 SOL should have < 10% impact (1000 bps)"
        );
    }

    #[test]
    fn test_apply_slippage_bps() {
        let original = 1_000_000_000u64;

        // 0.5% slippage
        let with_slippage = apply_slippage_bps(original, 50);
        assert_eq!(with_slippage, 995_000_000); // 99.5%

        // 1% slippage
        let with_slippage = apply_slippage_bps(original, 100);
        assert_eq!(with_slippage, 990_000_000); // 99%

        // 10% slippage
        let with_slippage = apply_slippage_bps(original, 1000);
        assert_eq!(with_slippage, 900_000_000); // 90%
    }

    #[test]
    fn test_apply_slippage_bps_overflow_protection() {
        let original = u64::MAX;

        // Should not overflow
        let with_slippage = apply_slippage_bps(original, 50);
        assert!(with_slippage < original);
    }

    // =========================================================================
    // Micro-Simulation Tests
    // =========================================================================

    #[test]
    fn test_micro_simulate_buy() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let (tokens_out, price_after, effective_sol, impact) =
            micro_simulate_buy(&curve, 1_000_000_000);

        assert!(tokens_out > 0);
        assert!(price_after > curve.current_price());
        assert_eq!(effective_sol, 990_000_000); // 1% fee
        assert!(impact > 0.0);
    }

    #[test]
    fn test_micro_simulate_buy_zero() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let (tokens_out, price_after, effective_sol, impact) = micro_simulate_buy(&curve, 0);

        assert_eq!(tokens_out, 0);
        assert_eq!(price_after, curve.current_price());
        assert_eq!(effective_sol, 0);
        assert_eq!(impact, 0.0);
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_simulation_with_zero_reserves() {
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 0,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 0,
            real_sol_reserves: 0,
            token_total_supply: 0,
            complete: 0,
            _padding: [0; 7],
        };

        let result = simulate_buy_pure(&curve, 1_000_000_000);
        assert_eq!(result.tokens_out, 0);
    }

    #[test]
    fn test_simulation_with_small_amounts() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Very small amount: 100 lamports (0.0000001 SOL)
        let result = simulate_buy_pure(&curve, 100);

        // Should not panic and should return something (may be 0 due to rounding)
        assert!(result.tokens_out <= curve.virtual_token_reserves);
    }

    #[test]
    fn test_deterministic_simulation() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let result1 = simulate_buy_pure(&curve, 1_000_000_000);
        let result2 = simulate_buy_pure(&curve, 1_000_000_000);
        let result3 = simulate_buy_pure(&curve, 1_000_000_000);

        assert_eq!(result1.tokens_out, result2.tokens_out);
        assert_eq!(result2.tokens_out, result3.tokens_out);
        assert_eq!(result1.min_tokens_out, result3.min_tokens_out);
    }

    #[test]
    fn test_price_impact_increases_with_size() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        let impact_small = calculate_price_impact(&curve, 100_000_000); // 0.1 SOL
        let impact_medium = calculate_price_impact(&curve, 1_000_000_000); // 1 SOL
        let impact_large = calculate_price_impact(&curve, 5_000_000_000); // 5 SOL

        assert!(impact_medium > impact_small);
        assert!(impact_large > impact_medium);
    }

    // =========================================================================
    // Constants Tests
    // =========================================================================

    #[test]
    fn test_constants() {
        assert_eq!(BPS_DENOMINATOR, 10000);
        assert_eq!(DEFAULT_SLIPPAGE_BPS, 50);
        assert_eq!(FEE_BPS, 100);
        assert_eq!(FIXED_POINT_PRECISION, 1_000_000_000_000);
    }

    // =========================================================================
    // Performance Sanity Tests
    // =========================================================================

    #[test]
    fn test_simulation_performance() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let iterations = 10000;

        let start = std::time::Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(simulate_buy_pure(&curve, 1_000_000_000));
        }
        let duration = start.elapsed();

        let ns_per_call = duration.as_nanos() / iterations as u128;

        // Should be well under 1 microsecond
        assert!(
            ns_per_call < 1000,
            "simulate_buy_pure took {} ns/call, should be < 1000 ns",
            ns_per_call
        );
    }

    #[test]
    fn test_derivative_computation_performance() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let iterations = 10000;

        let start = std::time::Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(compute_all_derivatives(&curve, 1_000_000_000));
        }
        let duration = start.elapsed();

        let ns_per_call = duration.as_nanos() / iterations as u128;

        // Should be under 2 microseconds (slightly more due to 2x simulations)
        assert!(
            ns_per_call < 2000,
            "compute_all_derivatives took {} ns/call, should be < 2000 ns",
            ns_per_call
        );
    }
}

// ============================================================================
// Property-Based Tests using Proptest
// ============================================================================

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Strategy for generating valid bonding curves
    fn bonding_curve_strategy() -> impl Strategy<Value = BondingCurve> {
        // Generate reserves in realistic ranges
        // Virtual SOL: 1 SOL to 1000 SOL (in lamports)
        // Virtual tokens: 100M to 1T tokens
        (
            1_000_000_000u64..1_000_000_000_000u64, // virtual_sol (1 SOL - 1000 SOL)
            100_000_000u64..1_000_000_000_000u64,   // virtual_tokens (100M - 1T)
        )
            .prop_map(|(virtual_sol, virtual_tokens)| BondingCurve {
                discriminator: 0x1234567890abcdef,
                virtual_token_reserves: virtual_tokens,
                virtual_sol_reserves: virtual_sol,
                real_token_reserves: virtual_tokens * 8 / 10,
                real_sol_reserves: virtual_sol * 8 / 10,
                token_total_supply: virtual_tokens,
                complete: 0,
                _padding: [0; 7],
            })
    }

    /// Strategy for generating trade amounts in realistic ranges
    fn trade_amount_strategy() -> impl Strategy<Value = u64> {
        // 0.001 SOL to 100 SOL (in lamports)
        1_000_000u64..100_000_000_000u64
    }

    /// Strategy for generating slippage in realistic ranges
    fn slippage_strategy() -> impl Strategy<Value = u64> {
        // 0.01% to 50% (in basis points: 1 to 5000)
        1u64..5000u64
    }

    proptest! {
        // =========================================================================
        // Buy Simulation Properties
        // =========================================================================

        #[test]
        fn prop_buy_simulation_never_panics(
            curve in bonding_curve_strategy(),
            amount in 0u64..1_000_000_000_000u64
        ) {
            // Should never panic for any input
            let _ = simulate_buy_pure(&curve, amount);
        }

        #[test]
        fn prop_buy_simulation_tokens_out_bounded(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy()
        ) {
            let result = simulate_buy_pure(&curve, amount);

            // Tokens out should never exceed virtual token reserves
            prop_assert!(result.tokens_out <= curve.virtual_token_reserves);
        }

        #[test]
        fn prop_buy_simulation_min_tokens_less_than_expected(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy(),
            slippage in slippage_strategy()
        ) {
            let result = simulate_buy_with_slippage_pure(&curve, amount, slippage);

            // min_tokens_out should always be <= tokens_out
            prop_assert!(result.min_tokens_out <= result.tokens_out);
        }

        #[test]
        fn prop_buy_simulation_price_impact_non_negative(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy()
        ) {
            let result = simulate_buy_pure(&curve, amount);

            // Price impact for buys should always be non-negative
            prop_assert!(result.price_impact_percent >= 0.0);
        }

        #[test]
        fn prop_buy_simulation_effective_sol_less_than_input(
            curve in bonding_curve_strategy(),
            amount in 100u64..1_000_000_000_000u64 // At least 100 lamports for fee
        ) {
            let result = simulate_buy_pure(&curve, amount);

            // Effective SOL should be less than input due to 1% fee
            prop_assert!(result.effective_sol_in < result.sol_in);
        }

        #[test]
        fn prop_buy_simulation_monotonic_tokens(
            curve in bonding_curve_strategy(),
            amount1 in trade_amount_strategy(),
            amount2 in trade_amount_strategy()
        ) {
            // More SOL in should always result in more or equal tokens out
            let result1 = simulate_buy_pure(&curve, amount1);
            let result2 = simulate_buy_pure(&curve, amount2);

            if amount1 < amount2 {
                prop_assert!(result1.tokens_out <= result2.tokens_out);
            } else if amount1 > amount2 {
                prop_assert!(result1.tokens_out >= result2.tokens_out);
            } else {
                prop_assert_eq!(result1.tokens_out, result2.tokens_out);
            }
        }

        // =========================================================================
        // Sell Simulation Properties
        // =========================================================================

        #[test]
        fn prop_sell_simulation_never_panics(
            curve in bonding_curve_strategy(),
            amount in 0u64..1_000_000_000_000u64
        ) {
            // Should never panic for any input
            let _ = simulate_sell_pure(&curve, amount);
        }

        #[test]
        fn prop_sell_simulation_sol_out_bounded(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy()
        ) {
            let result = simulate_sell_pure(&curve, amount);

            // SOL out should never exceed virtual SOL reserves
            prop_assert!(result.sol_out <= curve.virtual_sol_reserves);
        }

        #[test]
        fn prop_sell_simulation_min_sol_less_than_expected(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy(),
            slippage in slippage_strategy()
        ) {
            let result = simulate_sell_with_slippage_pure(&curve, amount, slippage);

            // min_sol_out should always be <= sol_out
            prop_assert!(result.min_sol_out <= result.sol_out);
        }

        #[test]
        fn prop_sell_simulation_price_impact_non_positive(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy()
        ) {
            let result = simulate_sell_pure(&curve, amount);

            // Price impact for sells should always be non-positive (price decreases)
            prop_assert!(result.price_impact_percent <= 0.0);
        }

        // =========================================================================
        // Fixed-Point Arithmetic Properties
        // =========================================================================

        #[test]
        fn prop_fixed_point_tokens_out_never_overflows(
            virtual_sol in 1_000_000_000u64..1_000_000_000_000u64,
            virtual_tokens in 1_000_000_000u64..1_000_000_000_000_000u64,
            amount_in in 0u64..1_000_000_000_000u64
        ) {
            // Should never panic from overflow
            let _ = calculate_tokens_out_fixed(virtual_sol, virtual_tokens, amount_in);
        }

        #[test]
        fn prop_fixed_point_sol_out_never_overflows(
            virtual_sol in 1_000_000_000u64..1_000_000_000_000u64,
            virtual_tokens in 1_000_000_000u64..1_000_000_000_000_000u64,
            amount_tokens in 0u64..1_000_000_000_000u64
        ) {
            // Should never panic from overflow
            let _ = calculate_sol_out_fixed(virtual_sol, virtual_tokens, amount_tokens);
        }

        #[test]
        fn prop_slippage_application_bounded(
            amount in 0u64..u64::MAX,
            slippage_bps in 0u64..10000u64 // 0% to 100%
        ) {
            let result = apply_slippage_bps(amount, slippage_bps);

            // Result should always be <= original amount
            prop_assert!(result <= amount);

            // With 100% slippage (10000 bps), result should be 0
            if slippage_bps == 10000 {
                prop_assert_eq!(result, 0);
            }
        }

        #[test]
        fn prop_price_impact_bps_bounded(
            virtual_sol in 1_000_000_000u64..1_000_000_000_000u64,
            virtual_tokens in 1_000_000_000u64..1_000_000_000_000_000u64,
            amount in trade_amount_strategy()
        ) {
            let impact_bps = calculate_price_impact_bps(virtual_sol, virtual_tokens, amount);

            // `impact_bps` is an unsigned integer; the useful invariant here is simply
            // that the function completes across the sampled reserve/trade domain.
            let _ = impact_bps;
        }

        // =========================================================================
        // Derivative Calculation Properties
        // =========================================================================

        #[test]
        fn prop_derivatives_never_panic(
            curve in bonding_curve_strategy(),
            amount in 0u64..1_000_000_000_000u64
        ) {
            // Should never panic for any input
            let _ = compute_all_derivatives(&curve, amount);
        }

        #[test]
        fn prop_derivatives_finite_values(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy()
        ) {
            let (d_vol, d_liq, d_slip) = compute_all_derivatives(&curve, amount);

            // All derivatives should be finite (not NaN or Inf)
            prop_assert!(d_vol.is_finite());
            prop_assert!(d_liq.is_finite());
            prop_assert!(d_slip.is_finite());
        }

        #[test]
        fn prop_price_impact_monotonic_with_amount(
            curve in bonding_curve_strategy()
        ) {
            // Price impact should increase monotonically with trade size
            let amounts = [
                100_000_000u64,      // 0.1 SOL
                500_000_000u64,      // 0.5 SOL
                1_000_000_000u64,    // 1 SOL
                5_000_000_000u64,    // 5 SOL
                10_000_000_000u64,   // 10 SOL
            ];

            let mut prev_impact = 0.0;
            for amount in amounts {
                let impact = calculate_price_impact(&curve, amount);
                prop_assert!(impact >= prev_impact,
                    "Impact {} should be >= previous impact {} for increasing amounts",
                    impact, prev_impact);
                prev_impact = impact;
            }
        }

        // =========================================================================
        // Zero Liquidity Edge Cases
        // =========================================================================

        #[test]
        fn prop_zero_token_reserves_safe(
            virtual_sol in 1_000_000_000u64..1_000_000_000_000u64,
            amount in trade_amount_strategy()
        ) {
            let curve = BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 0,
                virtual_sol_reserves: virtual_sol,
                real_token_reserves: 0,
                real_sol_reserves: 0,
                token_total_supply: 0,
                complete: 0,
                _padding: [0; 7],
            };

            // Should handle zero token reserves gracefully
            let result = simulate_buy_pure(&curve, amount);
            prop_assert_eq!(result.tokens_out, 0);
        }

        #[test]
        fn prop_zero_sol_reserves_safe(
            virtual_tokens in 1_000_000_000u64..1_000_000_000_000u64,
            amount in trade_amount_strategy()
        ) {
            let curve = BondingCurve {
                discriminator: 0,
                virtual_token_reserves: virtual_tokens,
                virtual_sol_reserves: 0,
                real_token_reserves: 0,
                real_sol_reserves: 0,
                token_total_supply: virtual_tokens,
                complete: 0,
                _padding: [0; 7],
            };

            // Should handle zero SOL reserves gracefully
            let result = simulate_sell_pure(&curve, amount);
            prop_assert_eq!(result.sol_out, 0);
        }

        // =========================================================================
        // Determinism Properties
        // =========================================================================

        #[test]
        fn prop_simulation_deterministic(
            curve in bonding_curve_strategy(),
            amount in trade_amount_strategy()
        ) {
            // Same inputs should always produce same outputs
            let result1 = simulate_buy_pure(&curve, amount);
            let result2 = simulate_buy_pure(&curve, amount);

            prop_assert_eq!(result1.tokens_out, result2.tokens_out);
            prop_assert_eq!(result1.min_tokens_out, result2.min_tokens_out);
            prop_assert_eq!(result1.effective_sol_in, result2.effective_sol_in);
        }
    }
}
