//! Price Logic for TP/SL Calculations
//!
//! This module defines the business logic for calculating target prices
//! for Take Profit (TP) and Stop Loss (panic) levels based on entry price
//! and configured multipliers.
//!
//! # Usage
//!
//! ```ignore
//! let position = PositionPriceTargets::new(entry_price, tp_config);
//! let tp1_price = position.tp1_target_price;
//! let tp2_price = position.tp2_target_price;
//! let panic_price = position.panic_target_price;
//! ```

use crate::errors::{Result, TriggerError};
use serde::{Deserialize, Serialize};

/// Configuration for Take Profit and Stop Loss levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpPanicConfig {
    /// TP1 multiplier (e.g., 1.2 = 20% profit)
    pub tp1_mult: f64,
    /// TP2 multiplier (e.g., 2.0 = 100% profit)
    pub tp2_mult: f64,
    /// Panic/Stop Loss multiplier (e.g., 0.8 = 20% loss)
    pub panic_mult: f64,
}

impl Default for TpPanicConfig {
    fn default() -> Self {
        Self {
            tp1_mult: 1.2,   // 20% profit
            tp2_mult: 2.0,   // 100% profit
            panic_mult: 0.8, // 20% loss (stop loss)
        }
    }
}

impl TpPanicConfig {
    /// Create a new TP/Panic configuration
    pub fn new(tp1_mult: f64, tp2_mult: f64, panic_mult: f64) -> Result<Self> {
        // Validate multipliers
        if tp1_mult <= 0.0 || tp2_mult <= 0.0 || panic_mult <= 0.0 {
            return Err(TriggerError::ConfigError(
                "All multipliers must be positive".to_string(),
            ));
        }

        if tp1_mult >= tp2_mult {
            return Err(TriggerError::ConfigError(
                "TP1 multiplier must be less than TP2 multiplier".to_string(),
            ));
        }

        if panic_mult >= 1.0 {
            return Err(TriggerError::ConfigError(
                "Panic multiplier should be less than 1.0 for stop loss".to_string(),
            ));
        }

        Ok(Self {
            tp1_mult,
            tp2_mult,
            panic_mult,
        })
    }

    /// Create conservative TP/SL levels (tighter targets)
    pub fn conservative() -> Self {
        Self {
            tp1_mult: 1.1,   // 10% profit
            tp2_mult: 1.5,   // 50% profit
            panic_mult: 0.9, // 10% loss
        }
    }

    /// Create aggressive TP/SL levels (wider targets)
    pub fn aggressive() -> Self {
        Self {
            tp1_mult: 1.5,   // 50% profit
            tp2_mult: 3.0,   // 200% profit
            panic_mult: 0.7, // 30% loss
        }
    }
}

/// Position price targets calculated from entry price
#[derive(Debug, Clone)]
pub struct PositionPriceTargets {
    /// Entry price in lamports per token
    pub entry_price: u64,
    /// TP1 target price in lamports per token
    pub tp1_target_price: u64,
    /// TP2 target price in lamports per token
    pub tp2_target_price: u64,
    /// Panic/Stop Loss target price in lamports per token
    pub panic_target_price: u64,
    /// Configuration used
    pub config: TpPanicConfig,
}

impl PositionPriceTargets {
    /// Create new position price targets from entry price and config
    pub fn new(entry_price: u64, config: TpPanicConfig) -> Self {
        let tp1_target_price = Self::calculate_target_price(entry_price, config.tp1_mult);
        let tp2_target_price = Self::calculate_target_price(entry_price, config.tp2_mult);
        let panic_target_price = Self::calculate_target_price(entry_price, config.panic_mult);

        Self {
            entry_price,
            tp1_target_price,
            tp2_target_price,
            panic_target_price,
            config,
        }
    }

    /// Create with default TP/SL configuration
    pub fn with_default_config(entry_price: u64) -> Self {
        Self::new(entry_price, TpPanicConfig::default())
    }

    /// Calculate target price given entry price and multiplier
    ///
    /// Note: Uses f64 multiplication for simplicity. For very large entry_price values
    /// (> 2^53), precision loss may occur. In practice, lamport prices are well within
    /// safe range. Result is truncated (not rounded) when converting back to u64.
    fn calculate_target_price(entry_price: u64, multiplier: f64) -> u64 {
        let result = (entry_price as f64) * multiplier;
        // Clamp to u64::MAX to handle potential overflow
        if result > u64::MAX as f64 {
            u64::MAX
        } else {
            result as u64
        }
    }

    /// Check if current price has reached TP1
    pub fn has_reached_tp1(&self, current_price: u64) -> bool {
        current_price >= self.tp1_target_price
    }

    /// Check if current price has reached TP2
    pub fn has_reached_tp2(&self, current_price: u64) -> bool {
        current_price >= self.tp2_target_price
    }

    /// Check if current price has hit panic/stop loss
    pub fn has_hit_panic(&self, current_price: u64) -> bool {
        current_price <= self.panic_target_price
    }

    /// Get profit/loss percentage at current price
    pub fn get_pnl_percentage(&self, current_price: u64) -> f64 {
        if self.entry_price == 0 {
            return 0.0;
        }
        ((current_price as f64 - self.entry_price as f64) / self.entry_price as f64) * 100.0
    }

    /// Get the next target price that should trigger
    /// Returns None if all targets have been hit or panic has been hit
    pub fn get_next_target(&self, current_price: u64) -> Option<TargetLevel> {
        // Check if panic was hit
        if self.has_hit_panic(current_price) {
            return Some(TargetLevel::Panic);
        }

        // Check if we haven't reached TP1 yet
        if current_price < self.tp1_target_price {
            return Some(TargetLevel::Tp1);
        }

        // Check if we've reached TP1 but not TP2
        if current_price >= self.tp1_target_price && current_price < self.tp2_target_price {
            return Some(TargetLevel::Tp2);
        }

        // All targets hit
        None
    }
}

/// Target level enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLevel {
    /// Take Profit 1
    Tp1,
    /// Take Profit 2
    Tp2,
    /// Panic/Stop Loss
    Panic,
}

impl TargetLevel {
    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            TargetLevel::Tp1 => "TP1",
            TargetLevel::Tp2 => "TP2",
            TargetLevel::Panic => "PANIC",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tp_panic_config_default() {
        let config = TpPanicConfig::default();
        assert_eq!(config.tp1_mult, 1.2);
        assert_eq!(config.tp2_mult, 2.0);
        assert_eq!(config.panic_mult, 0.8);
    }

    #[test]
    fn test_tp_panic_config_validation() {
        // Valid config
        let result = TpPanicConfig::new(1.2, 2.0, 0.8);
        assert!(result.is_ok());

        // Invalid: TP1 >= TP2
        let result = TpPanicConfig::new(2.0, 1.2, 0.8);
        assert!(result.is_err());

        // Invalid: panic >= 1.0
        let result = TpPanicConfig::new(1.2, 2.0, 1.5);
        assert!(result.is_err());

        // Invalid: negative multiplier
        let result = TpPanicConfig::new(-1.0, 2.0, 0.8);
        assert!(result.is_err());
    }

    #[test]
    fn test_tp_panic_config_presets() {
        let conservative = TpPanicConfig::conservative();
        assert_eq!(conservative.tp1_mult, 1.1);
        assert_eq!(conservative.panic_mult, 0.9);

        let aggressive = TpPanicConfig::aggressive();
        assert_eq!(aggressive.tp1_mult, 1.5);
        assert_eq!(aggressive.tp2_mult, 3.0);
        assert_eq!(aggressive.panic_mult, 0.7);
    }

    #[test]
    fn test_position_price_targets_calculation() {
        let entry_price = 1000; // lamports
        let config = TpPanicConfig::default();
        let targets = PositionPriceTargets::new(entry_price, config);

        assert_eq!(targets.entry_price, 1000);
        assert_eq!(targets.tp1_target_price, 1200); // 1000 * 1.2
        assert_eq!(targets.tp2_target_price, 2000); // 1000 * 2.0
        assert_eq!(targets.panic_target_price, 800); // 1000 * 0.8
    }

    #[test]
    fn test_position_price_targets_default_config() {
        let targets = PositionPriceTargets::with_default_config(1000);
        assert_eq!(targets.tp1_target_price, 1200);
        assert_eq!(targets.tp2_target_price, 2000);
        assert_eq!(targets.panic_target_price, 800);
    }

    #[test]
    fn test_has_reached_tp1() {
        let targets = PositionPriceTargets::with_default_config(1000);

        assert!(!targets.has_reached_tp1(1100)); // Below TP1
        assert!(targets.has_reached_tp1(1200)); // At TP1
        assert!(targets.has_reached_tp1(1300)); // Above TP1
    }

    #[test]
    fn test_has_reached_tp2() {
        let targets = PositionPriceTargets::with_default_config(1000);

        assert!(!targets.has_reached_tp2(1500)); // Below TP2
        assert!(targets.has_reached_tp2(2000)); // At TP2
        assert!(targets.has_reached_tp2(2500)); // Above TP2
    }

    #[test]
    fn test_has_hit_panic() {
        let targets = PositionPriceTargets::with_default_config(1000);

        assert!(!targets.has_hit_panic(900)); // Above panic
        assert!(targets.has_hit_panic(800)); // At panic
        assert!(targets.has_hit_panic(700)); // Below panic (worse)
    }

    #[test]
    fn test_get_pnl_percentage() {
        let targets = PositionPriceTargets::with_default_config(1000);

        // At entry
        assert_eq!(targets.get_pnl_percentage(1000), 0.0);

        // 20% profit
        assert_eq!(targets.get_pnl_percentage(1200), 20.0);

        // 100% profit
        assert_eq!(targets.get_pnl_percentage(2000), 100.0);

        // 20% loss
        assert_eq!(targets.get_pnl_percentage(800), -20.0);
    }

    #[test]
    fn test_get_next_target() {
        let targets = PositionPriceTargets::with_default_config(1000);

        // Below TP1
        assert_eq!(targets.get_next_target(1100), Some(TargetLevel::Tp1));

        // At TP1
        assert_eq!(targets.get_next_target(1200), Some(TargetLevel::Tp2));

        // Between TP1 and TP2
        assert_eq!(targets.get_next_target(1500), Some(TargetLevel::Tp2));

        // At TP2
        assert_eq!(targets.get_next_target(2000), None);

        // Above TP2
        assert_eq!(targets.get_next_target(2500), None);

        // Panic hit
        assert_eq!(targets.get_next_target(700), Some(TargetLevel::Panic));
    }

    #[test]
    fn test_target_level_name() {
        assert_eq!(TargetLevel::Tp1.name(), "TP1");
        assert_eq!(TargetLevel::Tp2.name(), "TP2");
        assert_eq!(TargetLevel::Panic.name(), "PANIC");
    }
}
