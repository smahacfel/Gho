//! Safety module - The Bulkhead (Waterproof Compartments)
//!
//! This module implements hard financial limits to prevent portfolio depletion
//! through logic errors, transaction loops, or Jito bidding wars.
//!
//! ## Features
//! - Emergency Floor Check: Ensures minimum balance for exit fees
//! - Position Size Hard Cap: Dynamically limits trade amounts based on available balance
//!
//! ## Safety Philosophy
//! Better to miss a trade opportunity than to deplete the entire portfolio.

use anyhow::{bail, Result};
use tracing::{error, info, warn};

/// Emergency floor balance in SOL - minimum reserve for exit fees
/// This is the absolute minimum balance that must be maintained at all times
///
/// Note: This constant is also defined in SafetyConfig::default() for configuration.
/// The constant exists for documentation, examples, and as a reference value.
pub const EMERGENCY_FLOOR_SOL: f64 = 0.05;

/// Position size buffer in SOL - additional safety margin beyond emergency floor
/// This covers transaction fees and tips while keeping the emergency floor untouched
///
/// Note: This constant is also defined in SafetyConfig::default() for configuration.
/// The constant exists for documentation, examples, and as a reference value.
pub const POSITION_SIZE_BUFFER_SOL: f64 = 0.02;

/// Safety violation error types
#[derive(Debug, thiserror::Error)]
pub enum SafetyViolation {
    #[error("Balance critical: {current_balance} SOL < {emergency_floor} SOL emergency floor")]
    BalanceCritical {
        current_balance: f64,
        emergency_floor: f64,
    },

    #[error("Insufficient safe balance: available={available} SOL, required={required} SOL")]
    InsufficientSafeBalance { available: f64, required: f64 },

    #[error("Trade amount {trade_amount} SOL exceeds maximum safe size {max_safe} SOL")]
    TradeAmountExceedsMax { trade_amount: f64, max_safe: f64 },
}

/// Configuration for safety checks
#[derive(Debug, Clone)]
pub struct SafetyConfig {
    /// Emergency floor in SOL (default: 0.05)
    pub emergency_floor_sol: f64,

    /// Position size buffer in SOL (default: 0.02)
    pub position_size_buffer_sol: f64,

    /// Maximum position size in SOL (configured value)
    pub max_position_size_sol: f64,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            emergency_floor_sol: EMERGENCY_FLOOR_SOL,
            position_size_buffer_sol: POSITION_SIZE_BUFFER_SOL,
            max_position_size_sol: 0.1, // Conservative default
        }
    }
}

/// Validates that the current balance is above the emergency floor
///
/// # Arguments
/// * `current_balance` - Current wallet balance in SOL
/// * `config` - Safety configuration
///
/// # Returns
/// * `Ok(())` if balance is safe
/// * `Err(SafetyViolation::BalanceCritical)` if balance is below emergency floor
///
/// # Panics
/// This function will cause a critical error log if balance is below emergency floor.
/// The caller should handle this by shutting down the bot.
pub fn check_emergency_floor(current_balance: f64, config: &SafetyConfig) -> Result<()> {
    if current_balance < config.emergency_floor_sol {
        error!(
            "🚨 CRITICAL SAFETY VIOLATION: Balance {} SOL < {} SOL emergency floor",
            current_balance, config.emergency_floor_sol
        );
        error!("🚨 BOT MUST SHUTDOWN IMMEDIATELY - INSUFFICIENT BALANCE FOR EXIT FEES");

        bail!(SafetyViolation::BalanceCritical {
            current_balance,
            emergency_floor: config.emergency_floor_sol,
        });
    }

    Ok(())
}

/// Calculates the maximum safe trade amount based on current balance
///
/// # Arguments
/// * `current_balance` - Current wallet balance in SOL
/// * `config` - Safety configuration
///
/// # Returns
/// The maximum safe amount that can be used for a trade in SOL
///
/// # Formula
/// ```text
/// safe_trade_amount = min(
///     config.max_position_size_sol * safety_coefficient,
///     current_balance - emergency_floor - buffer
/// )
/// ```
///
/// # Example
/// ```
/// // Balance: 1.0 SOL
/// // Emergency floor: 0.05 SOL
/// // Buffer: 0.02 SOL
/// // Max position: 0.1 SOL
/// // Result: min(0.1 * 1.0, 1.0 - 0.05 - 0.02) = min(0.1, 0.93) = 0.1 SOL
/// ```
pub fn calculate_safe_trade_amount(
    current_balance: f64,
    config: &SafetyConfig,
    safety_coefficient: f64,
) -> f64 {
    let coefficient = safety_coefficient.clamp(0.0, 1.0);
    let available_after_reserves =
        current_balance - config.emergency_floor_sol - config.position_size_buffer_sol;

    // Can't trade with negative available balance
    if available_after_reserves <= 0.0 {
        warn!(
            "No safe balance available: balance={} SOL, reserves={} SOL",
            current_balance,
            config.emergency_floor_sol + config.position_size_buffer_sol
        );
        return 0.0;
    }

    let scaled_cap = config.max_position_size_sol * coefficient;
    let safe_amount = available_after_reserves.min(scaled_cap);

    info!(
        "Safety: Calculated safe trade amount: {} SOL (balance: {} SOL, available: {} SOL, max: {} SOL, coeff: {})",
        safe_amount, current_balance, available_after_reserves, config.max_position_size_sol, coefficient
    );

    safe_amount
}

/// Validates a proposed trade amount against safety limits
///
/// # Arguments
/// * `trade_amount` - Proposed trade amount in SOL
/// * `current_balance` - Current wallet balance in SOL
/// * `config` - Safety configuration
///
/// # Returns
/// * `Ok(())` if trade is safe to execute
/// * `Err(SafetyViolation)` if trade violates safety constraints
pub fn validate_trade(
    trade_amount: f64,
    current_balance: f64,
    config: &SafetyConfig,
) -> Result<()> {
    // First check emergency floor
    check_emergency_floor(current_balance, config)?;

    // Calculate maximum safe amount
    let max_safe_amount = calculate_safe_trade_amount(current_balance, config, 1.0);

    // Check if trade amount is within safe limits
    if trade_amount > max_safe_amount {
        bail!(SafetyViolation::TradeAmountExceedsMax {
            trade_amount,
            max_safe: max_safe_amount,
        });
    }

    // Check if we have sufficient balance after trade
    let balance_after_trade = current_balance - trade_amount;
    let required_reserve = config.emergency_floor_sol + config.position_size_buffer_sol;

    if balance_after_trade < required_reserve {
        bail!(SafetyViolation::InsufficientSafeBalance {
            available: balance_after_trade,
            required: required_reserve,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SafetyConfig {
        SafetyConfig {
            emergency_floor_sol: 0.05,
            position_size_buffer_sol: 0.02,
            max_position_size_sol: 0.1,
        }
    }

    #[test]
    fn test_emergency_floor_check_passes() {
        let config = default_config();
        let result = check_emergency_floor(0.1, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_emergency_floor_check_fails() {
        let config = default_config();
        let result = check_emergency_floor(0.04, &config);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("Balance critical"));
    }

    #[test]
    fn test_calculate_safe_trade_amount_normal() {
        let config = default_config();
        // Balance: 1.0 SOL, Emergency: 0.05, Buffer: 0.02
        // Available: 1.0 - 0.05 - 0.02 = 0.93
        // Max: 0.1
        // Result: min(0.1, 0.93) = 0.1
        let safe_amount = calculate_safe_trade_amount(1.0, &config, 1.0);
        assert!((safe_amount - 0.1).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_trade_amount_scaled_by_coefficient() {
        let config = default_config();
        // Apply a 0.5 safety coefficient to simulate degraded network conditions
        let safe_amount = calculate_safe_trade_amount(1.0, &config, 0.5);
        assert!((safe_amount - 0.05).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_trade_amount_limited_by_balance() {
        let config = default_config();
        // Balance: 0.08 SOL, Emergency: 0.05, Buffer: 0.02
        // Available: 0.08 - 0.05 - 0.02 = 0.01
        // Max: 0.1
        // Result: min(0.1, 0.01) = 0.01
        let safe_amount = calculate_safe_trade_amount(0.08, &config, 1.0);
        assert!((safe_amount - 0.01).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_trade_amount_insufficient() {
        let config = default_config();
        // Balance: 0.05 SOL (at emergency floor)
        // Available: 0.05 - 0.05 - 0.02 = -0.02 (negative!)
        // Result: 0.0
        let safe_amount = calculate_safe_trade_amount(0.05, &config, 1.0);
        assert_eq!(safe_amount, 0.0);
    }

    #[test]
    fn test_validate_trade_success() {
        let config = default_config();
        let result = validate_trade(0.1, 1.0, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_trade_emergency_floor_violation() {
        let config = default_config();
        // Balance at emergency floor should fail
        let result = validate_trade(0.01, 0.04, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Balance critical"));
    }

    #[test]
    fn test_validate_trade_exceeds_safe_amount() {
        let config = default_config();
        // Trying to trade 0.5 SOL with only 0.1 SOL balance
        // Max safe: 0.1 - 0.05 - 0.02 = 0.03
        let result = validate_trade(0.5, 0.1, &config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds maximum safe size"));
    }

    #[test]
    fn test_validate_trade_insufficient_reserve_after() {
        let config = default_config();
        // Balance: 0.08 SOL, trying to trade 0.02 SOL
        // After trade: 0.06 SOL
        // Required reserve: 0.05 + 0.02 = 0.07 SOL
        // 0.06 < 0.07 -> should fail
        let result = validate_trade(0.02, 0.08, &config);
        assert!(result.is_err());
    }
}
