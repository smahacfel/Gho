//! Safety module - The Bulkhead (Waterproof Compartments)
//!
//! This module implements hard financial limits to prevent portfolio depletion
//! through logic errors, transaction loops, or validator-tip bidding wars.
//!
//! ## Features
//! - Emergency Floor Check: Ensures minimum balance for exit fees
//! - Position Size Hard Cap: Dynamically limits trade amounts based on available balance
//!
//! ## Safety Philosophy
//! Better to miss a trade opportunity than to deplete the entire portfolio.

use anyhow::{bail, Result};
use solana_sdk::hash::hashv;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PositionSlotId([u8; 32]);

impl PositionSlotId {
    pub fn derive(owner: &Pubkey, mint: &Pubkey) -> Self {
        Self(hashv(&[b"position_slot_v1", owner.as_ref(), mint.as_ref()]).to_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PositionSlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

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

    #[error("No safe trade capacity: balance={current_balance} SOL, required_reserve={required_reserve} SOL")]
    NoSafeTradeCapacity {
        current_balance: f64,
        required_reserve: f64,
    },

    #[error("Max concurrent positions reached: active={active_positions}, max={max_concurrent_positions}")]
    MaxConcurrentPositionsReached {
        active_positions: usize,
        max_concurrent_positions: usize,
    },

    #[error("Position slot already active")]
    PositionSlotAlreadyActive { slot_id: PositionSlotId },
}

impl SafetyViolation {
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::BalanceCritical { .. } => "emergency_floor",
            Self::InsufficientSafeBalance { .. } => "position_buffer",
            Self::TradeAmountExceedsMax { .. } => "safe_trade_size",
            Self::NoSafeTradeCapacity { .. } => "no_safe_trade_capacity",
            Self::MaxConcurrentPositionsReached { .. } => "max_concurrent_positions",
            Self::PositionSlotAlreadyActive { .. } => "position_slot_already_active",
        }
    }
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

#[derive(Debug, Clone)]
struct ActivePositionRecord {
    pool_amm_id: String,
    base_mint: String,
}

#[derive(Debug)]
struct PositionLimitState {
    active: HashMap<PositionSlotId, ActivePositionRecord>,
}

#[derive(Debug, Clone)]
pub struct PositionLimitTracker {
    max_positions: usize,
    state: Arc<Mutex<PositionLimitState>>,
}

impl PositionLimitTracker {
    pub fn new(max_positions: usize) -> Self {
        Self {
            max_positions: max_positions.max(1),
            state: Arc::new(Mutex::new(PositionLimitState {
                active: HashMap::new(),
            })),
        }
    }

    pub fn max_positions(&self) -> usize {
        self.max_positions
    }

    pub fn active_positions(&self) -> usize {
        self.state
            .lock()
            .expect("position limit state")
            .active
            .len()
    }

    pub fn available_slots(&self) -> usize {
        self.max_positions.saturating_sub(self.active_positions())
    }

    pub fn contains(&self, slot_id: PositionSlotId) -> bool {
        self.state
            .lock()
            .expect("position limit state")
            .active
            .contains_key(&slot_id)
    }

    pub fn try_acquire(
        &self,
        owner: &Pubkey,
        mint: &Pubkey,
        pool_amm_id: impl Into<String>,
    ) -> Result<ActivePositionLease> {
        self.try_acquire_with_slot_id(
            PositionSlotId::derive(owner, mint),
            pool_amm_id,
            mint.to_string(),
        )
    }

    pub fn try_acquire_with_slot_id(
        &self,
        slot_id: PositionSlotId,
        pool_amm_id: impl Into<String>,
        base_mint: impl Into<String>,
    ) -> Result<ActivePositionLease> {
        let mut state = self.state.lock().expect("position limit state");
        let active_positions = state.active.len();
        if state.active.contains_key(&slot_id) {
            bail!(SafetyViolation::PositionSlotAlreadyActive { slot_id });
        }
        if active_positions >= self.max_positions {
            bail!(SafetyViolation::MaxConcurrentPositionsReached {
                active_positions,
                max_concurrent_positions: self.max_positions,
            });
        }

        state.active.insert(
            slot_id,
            ActivePositionRecord {
                pool_amm_id: pool_amm_id.into(),
                base_mint: base_mint.into(),
            },
        );
        drop(state);

        metrics::gauge!("trigger_active_positions", self.active_positions() as f64);

        Ok(ActivePositionLease {
            tracker: self.clone(),
            slot_id,
            active: true,
        })
    }

    pub fn register_existing(
        &self,
        slot_id: PositionSlotId,
        pool_amm_id: impl Into<String>,
        base_mint: impl Into<String>,
    ) -> Result<()> {
        self.try_acquire_with_slot_id(slot_id, pool_amm_id, base_mint)?
            .retain();
        Ok(())
    }

    pub fn release(&self, slot_id: PositionSlotId) -> bool {
        let removed = self
            .state
            .lock()
            .expect("position limit state")
            .active
            .remove(&slot_id)
            .is_some();

        if removed {
            metrics::gauge!("trigger_active_positions", self.active_positions() as f64);
        }

        removed
    }

    pub fn snapshot(&self) -> Vec<(PositionSlotId, String, String)> {
        let state = self.state.lock().expect("position limit state");
        state
            .active
            .iter()
            .map(|(slot_id, record)| {
                (
                    *slot_id,
                    record.pool_amm_id.clone(),
                    record.base_mint.clone(),
                )
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct ActivePositionLease {
    tracker: PositionLimitTracker,
    pub slot_id: PositionSlotId,
    active: bool,
}

impl ActivePositionLease {
    pub fn retain(mut self) {
        self.active = false;
    }
}

impl Drop for ActivePositionLease {
    fn drop(&mut self) {
        if self.active {
            let _ = self.tracker.release(self.slot_id);
        }
    }
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
///     config.max_position_size_sol,
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
/// // Result: min(0.1, 1.0 - 0.05 - 0.02) = min(0.1, 0.93) = 0.1 SOL
/// ```
pub fn calculate_safe_trade_amount(current_balance: f64, config: &SafetyConfig) -> f64 {
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

    let safe_amount = available_after_reserves.min(config.max_position_size_sol);

    info!(
        "Safety: Calculated safe trade amount: {} SOL (balance: {} SOL, available: {} SOL, max: {} SOL)",
        safe_amount, current_balance, available_after_reserves, config.max_position_size_sol
    );

    safe_amount
}

pub fn resolve_safe_trade_amount(current_balance: f64, config: &SafetyConfig) -> Result<f64> {
    check_emergency_floor(current_balance, config)?;

    let safe_amount = calculate_safe_trade_amount(current_balance, config);
    if safe_amount <= 0.0 {
        bail!(SafetyViolation::NoSafeTradeCapacity {
            current_balance,
            required_reserve: config.emergency_floor_sol + config.position_size_buffer_sol,
        });
    }

    validate_trade(safe_amount, current_balance, config)?;
    Ok(safe_amount)
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
    let max_safe_amount = calculate_safe_trade_amount(current_balance, config);

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
        let safe_amount = calculate_safe_trade_amount(1.0, &config);
        assert!((safe_amount - 0.1).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_trade_amount_limited_by_balance() {
        let config = default_config();
        // Balance: 0.08 SOL, Emergency: 0.05, Buffer: 0.02
        // Available: 0.08 - 0.05 - 0.02 = 0.01
        // Max: 0.1
        // Result: min(0.1, 0.01) = 0.01
        let safe_amount = calculate_safe_trade_amount(0.08, &config);
        assert!((safe_amount - 0.01).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_trade_amount_insufficient() {
        let config = default_config();
        // Balance: 0.05 SOL (at emergency floor)
        // Available: 0.05 - 0.05 - 0.02 = -0.02 (negative!)
        // Result: 0.0
        let safe_amount = calculate_safe_trade_amount(0.05, &config);
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

    #[test]
    fn test_resolve_safe_trade_amount_rejects_when_no_capacity() {
        let config = default_config();
        let err = resolve_safe_trade_amount(0.05, &config)
            .expect_err("balance at reserve threshold must reject");

        assert!(matches!(
            err.downcast_ref::<SafetyViolation>(),
            Some(SafetyViolation::NoSafeTradeCapacity { .. })
        ));
    }

    #[test]
    fn test_position_limit_tracker_rejects_when_full() {
        let tracker = PositionLimitTracker::new(1);
        let owner = Pubkey::new_unique();
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        let lease = tracker
            .try_acquire(&owner, &mint_a, "pool-a")
            .expect("first slot should be acquired");
        let err = tracker
            .try_acquire(&owner, &mint_b, "pool-b")
            .expect_err("second slot should be rejected");

        assert!(matches!(
            err.downcast_ref::<SafetyViolation>(),
            Some(SafetyViolation::MaxConcurrentPositionsReached { .. })
        ));
        drop(lease);
        assert_eq!(tracker.active_positions(), 0);
    }

    #[test]
    fn slot_id_is_stable_between_buy_and_hydration() {
        let owner = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let from_buy = PositionSlotId::derive(&owner, &mint);
        let from_hydration = PositionSlotId::derive(&owner, &mint);

        assert_eq!(from_buy, from_hydration);
    }

    #[test]
    fn different_mint_produces_different_slot() {
        let owner = Pubkey::new_unique();
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();

        assert_ne!(
            PositionSlotId::derive(&owner, &mint_a),
            PositionSlotId::derive(&owner, &mint_b),
        );
    }
}
