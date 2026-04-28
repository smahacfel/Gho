//! Safety module for capital preservation
//!
//! This module contains the Bulkhead (balance safety) and TipGuard (tip limiting) components
//! that prevent portfolio depletion.

pub mod bulkhead;
pub mod tip_guard;

// Re-export key types for convenience
pub use bulkhead::{
    calculate_safe_trade_amount, check_emergency_floor, validate_trade, SafetyConfig,
    SafetyViolation, EMERGENCY_FLOOR_SOL, POSITION_SIZE_BUFFER_SOL,
};

pub use tip_guard::{
    calculate_safe_tip, get_fallback_tip, validate_tip, TipGuardConfig,
    DEFAULT_MAX_TIP_ABSOLUTE_SOL, DEFAULT_MAX_TIP_RATIO_PERCENT, FALLBACK_TIP_SOL,
};
