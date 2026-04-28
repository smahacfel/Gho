//! Validation Module
//!
//! Provides validation logic for SwapPlans against trading constraints.
//!
//! This module ensures off-chain validation matches expected constraints:
//! - MIN_SWAP_AMOUNT: 1000 lamports
//! - MAX_TIMEOUT_DURATION: 7 days
//!
//! All validations here help prevent transaction failures and wasted gas fees.

use crate::swap_plan::SwapPlan;
use crate::trading_constraints::TradingConstraints;
use thiserror::Error;

/// Validation errors that can occur when checking a SwapPlan
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ValidationError {
    /// Amount is below the minimum required
    #[error("Amount {0} is below minimum {1}")]
    AmountTooLow(u64, u64),

    /// Minimum output amount is zero
    #[error("Minimum amount out must be greater than zero")]
    InvalidMinAmountOut,

    /// Timeout is in the past
    #[error("Timeout {0} is in the past (current: {1})")]
    TimeoutInPast(i64, i64),

    /// Timeout is too far in the future
    #[error("Timeout duration {0} exceeds maximum {1}")]
    TimeoutTooLong(i64, i64),

    /// Pool AMM ID is not from a whitelisted program
    #[error("Pool AMM ID {0} is not from a whitelisted program")]
    UnauthorizedPool(String),

    /// Generic validation error
    #[error("Validation failed: {0}")]
    Other(String),
}

/// Result type for validation operations
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Validator for SwapPlans
///
/// Checks that a SwapPlan satisfies all trading constraints
/// before it is sent for execution.
pub struct Validator {
    constraints: TradingConstraints,
}

impl Validator {
    /// Create a new validator with default trading constraints
    pub fn new() -> Self {
        Self {
            constraints: TradingConstraints::default(),
        }
    }

    /// Create a validator with custom constraints (for testing)
    pub fn with_constraints(constraints: TradingConstraints) -> Self {
        Self { constraints }
    }

    /// Validate a SwapPlan against trading constraints
    ///
    /// Checks (in order):
    /// 1. amount_in >= MIN_SWAP_AMOUNT (1000 lamports)
    /// 2. min_amount_out > 0 (prevents zero slippage protection)
    /// 3. timeout is in the future (> current_timestamp)
    /// 4. timeout duration <= MAX_TIMEOUT_DURATION (7 days)
    /// 5. pool_amm_id is from whitelisted AMM (if whitelist is enabled)
    ///
    /// # Arguments
    ///
    /// * `plan` - The SwapPlan to validate
    /// * `current_timestamp` - Current Unix timestamp (for timeout validation)
    ///
    /// # Returns
    ///
    /// * `Ok(())` if validation passes
    /// * `Err(ValidationError)` if validation fails with specific error variant
    pub fn validate(&self, plan: &SwapPlan, current_timestamp: i64) -> ValidationResult<()> {
        // Validation 1: Check minimum amount
        if plan.amount_in < self.constraints.min_swap_amount {
            return Err(ValidationError::AmountTooLow(
                plan.amount_in,
                self.constraints.min_swap_amount,
            ));
        }

        // Validation 2: Check min_amount_out > 0
        if plan.min_amount_out == 0 {
            return Err(ValidationError::InvalidMinAmountOut);
        }

        // Validation 3: Check timeout is in the future
        if plan.timeout <= current_timestamp {
            return Err(ValidationError::TimeoutInPast(
                plan.timeout,
                current_timestamp,
            ));
        }

        // Validation 4: Check timeout duration
        let timeout_duration = plan.timeout - current_timestamp;
        if timeout_duration > self.constraints.max_timeout_duration {
            return Err(ValidationError::TimeoutTooLong(
                timeout_duration,
                self.constraints.max_timeout_duration,
            ));
        }

        // Validation 5: Check pool whitelist if enabled
        #[allow(deprecated)]
        if self.constraints.enable_pool_whitelist
            && !self.constraints.is_whitelisted_pool(&plan.pool_amm_id)
        {
            return Err(ValidationError::UnauthorizedPool(
                plan.pool_amm_id.to_string(),
            ));
        }

        Ok(())
    }

    /// Validate amount_in only
    pub fn validate_amount(&self, amount_in: u64) -> ValidationResult<()> {
        if amount_in < self.constraints.min_swap_amount {
            return Err(ValidationError::AmountTooLow(
                amount_in,
                self.constraints.min_swap_amount,
            ));
        }
        Ok(())
    }

    /// Validate timeout only
    pub fn validate_timeout(&self, timeout: i64, current_timestamp: i64) -> ValidationResult<()> {
        if timeout <= current_timestamp {
            return Err(ValidationError::TimeoutInPast(timeout, current_timestamp));
        }

        let timeout_duration = timeout - current_timestamp;
        if timeout_duration > self.constraints.max_timeout_duration {
            return Err(ValidationError::TimeoutTooLong(
                timeout_duration,
                self.constraints.max_timeout_duration,
            ));
        }

        Ok(())
    }

    /// Get a reference to the constraints being used
    pub fn constraints(&self) -> &TradingConstraints {
        &self.constraints
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to validate a SwapPlan with current timestamp
///
/// Convenience wrapper around Validator::validate that gets the current
/// timestamp automatically.
pub fn validate_swap_plan(plan: &SwapPlan) -> ValidationResult<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let current_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| ValidationError::Other(format!("Failed to get current time: {}", e)))?
        .as_secs() as i64;

    let validator = Validator::new();
    validator.validate(plan, current_timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_valid_swap_plan() {
        let validator = Validator::new();
        let plan = SwapPlan::new(
            Pubkey::new_unique(),
            TradingConstraints::pumpfun_program_id(),
            1_000_000, // Above MIN_SWAP_AMOUNT
            900_000,
            2000, // Future timestamp
        );

        assert!(validator.validate(&plan, 1000).is_ok());
    }

    #[test]
    fn test_amount_too_low() {
        let validator = Validator::new();
        let plan = SwapPlan::new(
            Pubkey::new_unique(),
            TradingConstraints::pumpfun_program_id(),
            500, // Below MIN_SWAP_AMOUNT (1000)
            450,
            2000,
        );

        let result = validator.validate(&plan, 1000);
        assert!(matches!(result, Err(ValidationError::AmountTooLow(_, _))));
    }

    #[test]
    fn test_zero_min_amount_out() {
        let validator = Validator::new();
        let plan = SwapPlan::new(
            Pubkey::new_unique(),
            TradingConstraints::pumpfun_program_id(),
            1_000_000,
            0, // Zero min_amount_out
            2000,
        );

        let result = validator.validate(&plan, 1000);
        assert!(matches!(result, Err(ValidationError::InvalidMinAmountOut)));
    }

    #[test]
    fn test_timeout_in_past() {
        let validator = Validator::new();
        let plan = SwapPlan::new(
            Pubkey::new_unique(),
            TradingConstraints::pumpfun_program_id(),
            1_000_000,
            900_000,
            500, // In the past
        );

        let result = validator.validate(&plan, 1000);
        assert!(matches!(result, Err(ValidationError::TimeoutInPast(_, _))));
    }

    #[test]
    fn test_timeout_too_long() {
        let validator = Validator::new();
        let current = 1000;
        let max_duration = 7 * 24 * 60 * 60; // 7 days
        let plan = SwapPlan::new(
            Pubkey::new_unique(),
            TradingConstraints::pumpfun_program_id(),
            1_000_000,
            900_000,
            current + max_duration + 1000, // Too far in future
        );

        let result = validator.validate(&plan, current);
        assert!(matches!(result, Err(ValidationError::TimeoutTooLong(_, _))));
    }

    #[test]
    fn test_validate_amount() {
        let validator = Validator::new();

        assert!(validator.validate_amount(1000).is_ok());
        assert!(validator.validate_amount(1_000_000).is_ok());
        assert!(validator.validate_amount(500).is_err());
    }

    #[test]
    fn test_validate_timeout() {
        let validator = Validator::new();
        let current = 1000;

        // Valid: 1 hour in future
        assert!(validator.validate_timeout(current + 3600, current).is_ok());

        // Invalid: in the past
        assert!(validator.validate_timeout(current - 1, current).is_err());

        // Invalid: too far in future (> 7 days)
        let too_far = current + (8 * 24 * 60 * 60);
        assert!(validator.validate_timeout(too_far, current).is_err());
    }
}
