//! TipGuard++ module - Protection against Jito bidding wars
//!
//! This module implements strict limits on Jito tips to prevent
//! catastrophic capital depletion during gas wars.
//!
//! ## Features
//! - Absolute Hard Cap: Maximum tip amount regardless of calculation
//! - Smart Ratio Cap: Tips limited to percentage of trade value
//! - Fail-Safe Fallback: Conservative default when API fails
//!
//! ## Protection Philosophy
//! Better to lose transaction priority than to pay 50% of capital to validators.

use tracing::{error, info, warn};

/// Fallback tip in SOL when Jito API fails or times out
/// Conservative value that ensures transaction submission without excessive cost
///
/// Note: This constant is also defined in TipGuardConfig::default() for configuration.
/// The constant exists for documentation, examples, and as a reference value.
pub const FALLBACK_TIP_SOL: f64 = 0.001;

/// Default absolute maximum tip in SOL
/// This is the hard cap that cannot be exceeded under any circumstances
///
/// Note: This constant is also defined in TipGuardConfig::default() for configuration.
/// The constant exists for documentation, examples, and as a reference value.
pub const DEFAULT_MAX_TIP_ABSOLUTE_SOL: f64 = 0.04;

/// Default maximum tip as percentage of trade value
/// Prevents spending more on tips than the trade is worth
///
/// Note: This constant is also defined in TipGuardConfig::default() for configuration.
/// The constant exists for documentation, examples, and as a reference value.
pub const DEFAULT_MAX_TIP_RATIO_PERCENT: f64 = 0.40; // 40%

/// Configuration for Jito tip calculations
#[derive(Debug, Clone)]
pub struct TipGuardConfig {
    /// Absolute maximum tip in SOL (hard cap)
    pub max_tip_absolute_sol: f64,

    /// Maximum tip as ratio of trade value (0.40 = 40%)
    pub max_tip_ratio_percent: f64,

    /// Fallback tip when API fails (SOL)
    pub fallback_tip_sol: f64,
}

impl Default for TipGuardConfig {
    fn default() -> Self {
        Self {
            max_tip_absolute_sol: DEFAULT_MAX_TIP_ABSOLUTE_SOL,
            max_tip_ratio_percent: DEFAULT_MAX_TIP_RATIO_PERCENT,
            fallback_tip_sol: FALLBACK_TIP_SOL,
        }
    }
}

/// Calculates a safe Jito tip amount with multiple layers of protection
///
/// # Arguments
/// * `calculated_tip` - The tip amount calculated by the algorithm (in SOL)
/// * `trade_value_sol` - The value of the trade being executed (in SOL)
/// * `config` - TipGuard configuration
///
/// # Returns
/// The safe tip amount in SOL, guaranteed to be within all limits
///
/// # Safety Layers
/// 1. Absolute cap: Never exceed configured maximum
/// 2. Ratio cap: Never exceed percentage of trade value
/// 3. Sanity check: Never allow negative or zero tips from bad calculations
///
/// # Example
/// ```
/// // Calculated tip: 1.0 SOL (from aggressive algorithm)
/// // Trade value: 0.1 SOL
/// // Max absolute: 0.04 SOL
/// // Max ratio: 40% of trade = 0.04 SOL
/// // Result: min(1.0, 0.04, 0.04) = 0.04 SOL
/// ```
pub fn calculate_safe_tip(
    calculated_tip: f64,
    trade_value_sol: f64,
    config: &TipGuardConfig,
) -> f64 {
    // Sanity check: calculated tip must be positive
    if calculated_tip <= 0.0 {
        warn!(
            "TipGuard: Invalid calculated tip {} SOL, using fallback {} SOL",
            calculated_tip, config.fallback_tip_sol
        );
        return config.fallback_tip_sol;
    }

    // Apply absolute hard cap
    let tip_after_absolute_cap = calculated_tip.min(config.max_tip_absolute_sol);

    // Calculate ratio-based cap
    let max_tip_from_ratio = trade_value_sol * config.max_tip_ratio_percent;

    // Apply ratio cap
    let final_tip = tip_after_absolute_cap.min(max_tip_from_ratio);

    // Log if we had to reduce the tip
    if final_tip < calculated_tip {
        info!(
            "TipGuard: Reduced tip from {} SOL to {} SOL (absolute_cap={}, ratio_cap={}, trade_value={})",
            calculated_tip, final_tip, config.max_tip_absolute_sol, max_tip_from_ratio, trade_value_sol
        );
    }

    // Sanity check: final tip should never be negative or too small
    if final_tip < 0.0001 {
        warn!(
            "TipGuard: Calculated tip {} SOL too small, using fallback {} SOL",
            final_tip, config.fallback_tip_sol
        );
        return config.fallback_tip_sol;
    }

    final_tip
}

/// Returns the fallback tip when Jito API fails
///
/// # Arguments
/// * `config` - TipGuard configuration
///
/// # Returns
/// The fallback tip amount in SOL
///
/// This function should be called when:
/// - Jito API returns an error
/// - Jito API request times out
/// - Jito percentile data is unavailable
pub fn get_fallback_tip(config: &TipGuardConfig) -> f64 {
    error!(
        "TipGuard: Using fallback tip {} SOL due to API failure",
        config.fallback_tip_sol
    );
    config.fallback_tip_sol
}

/// Validates that a tip amount is within acceptable bounds
///
/// # Arguments
/// * `tip_amount` - The tip amount to validate (in SOL)
/// * `trade_value_sol` - The value of the trade (in SOL)
/// * `config` - TipGuard configuration
///
/// # Returns
/// `true` if tip is valid, `false` otherwise
///
/// This is a defensive check that can be used before submitting transactions
pub fn validate_tip(tip_amount: f64, trade_value_sol: f64, config: &TipGuardConfig) -> bool {
    // Check absolute cap
    if tip_amount > config.max_tip_absolute_sol {
        error!(
            "TipGuard: Tip {} SOL exceeds absolute cap {} SOL",
            tip_amount, config.max_tip_absolute_sol
        );
        return false;
    }

    // Check ratio cap
    let max_from_ratio = trade_value_sol * config.max_tip_ratio_percent;
    if tip_amount > max_from_ratio {
        error!(
            "TipGuard: Tip {} SOL exceeds {}% of trade value {} SOL (max: {})",
            tip_amount,
            config.max_tip_ratio_percent * 100.0,
            trade_value_sol,
            max_from_ratio
        );
        return false;
    }

    // Check minimum
    if tip_amount < 0.0 {
        error!("TipGuard: Negative tip amount {} SOL", tip_amount);
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> TipGuardConfig {
        TipGuardConfig {
            max_tip_absolute_sol: 0.04,
            max_tip_ratio_percent: 0.40,
            fallback_tip_sol: 0.001,
        }
    }

    #[test]
    fn test_calculate_safe_tip_normal() {
        let config = default_config();
        // Reasonable tip that doesn't hit any limits
        let safe_tip = calculate_safe_tip(0.01, 0.1, &config);
        assert!((safe_tip - 0.01).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_tip_absolute_cap() {
        let config = default_config();
        // Calculated tip: 1.0 SOL (way too high)
        // Trade value: 0.1 SOL
        // Should be capped at 0.04 SOL (absolute max)
        let safe_tip = calculate_safe_tip(1.0, 0.1, &config);
        assert!((safe_tip - 0.04).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_tip_ratio_cap() {
        let config = default_config();
        // Calculated tip: 0.03 SOL
        // Trade value: 0.05 SOL
        // 40% of trade: 0.02 SOL
        // Should be capped at 0.02 SOL (ratio limit kicks in first)
        let safe_tip = calculate_safe_tip(0.03, 0.05, &config);
        assert!((safe_tip - 0.02).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_tip_both_caps() {
        let config = default_config();
        // Calculated tip: 0.5 SOL (absurdly high)
        // Trade value: 0.1 SOL
        // Absolute cap: 0.04 SOL
        // Ratio cap: 0.04 SOL (40% of 0.1)
        // Should be capped at 0.04 SOL
        let safe_tip = calculate_safe_tip(0.5, 0.1, &config);
        assert!((safe_tip - 0.04).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_tip_negative() {
        let config = default_config();
        // Negative tip should use fallback
        let safe_tip = calculate_safe_tip(-0.1, 0.1, &config);
        assert!((safe_tip - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_calculate_safe_tip_zero() {
        let config = default_config();
        // Zero tip should use fallback
        let safe_tip = calculate_safe_tip(0.0, 0.1, &config);
        assert!((safe_tip - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_calculate_safe_tip_too_small() {
        let config = default_config();
        // Very small tip should use fallback
        let safe_tip = calculate_safe_tip(0.00001, 0.1, &config);
        assert!((safe_tip - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_get_fallback_tip() {
        let config = default_config();
        let fallback = get_fallback_tip(&config);
        assert!((fallback - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_validate_tip_success() {
        let config = default_config();
        // Valid tip: 0.01 SOL for 0.1 SOL trade
        assert!(validate_tip(0.01, 0.1, &config));
    }

    #[test]
    fn test_validate_tip_exceeds_absolute() {
        let config = default_config();
        // Tip exceeds absolute cap
        assert!(!validate_tip(0.05, 1.0, &config));
    }

    #[test]
    fn test_validate_tip_exceeds_ratio() {
        let config = default_config();
        // Tip: 0.03 SOL, Trade: 0.05 SOL
        // 0.03 > (0.05 * 0.40 = 0.02)
        assert!(!validate_tip(0.03, 0.05, &config));
    }

    #[test]
    fn test_validate_tip_negative() {
        let config = default_config();
        // Negative tips are invalid
        assert!(!validate_tip(-0.01, 0.1, &config));
    }

    #[test]
    fn test_validate_tip_at_absolute_limit() {
        let config = default_config();
        // Tip exactly at absolute limit should be valid
        assert!(validate_tip(0.04, 1.0, &config));
    }

    #[test]
    fn test_validate_tip_at_ratio_limit() {
        let config = default_config();
        // Tip at exactly 40% of trade value
        assert!(validate_tip(0.04, 0.1, &config));
    }
}
