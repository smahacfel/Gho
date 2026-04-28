//! Guardian module - handles the "2-Second Void" problem
//!
//! This module provides watchdog functionality to prevent execution
//! during critical void periods.

pub mod post_buy;
pub mod types;
pub mod watchdog;

// Re-export public types
pub use types::{
    IntegritySeverity, WatchdogConfig, WatchdogContext, WatchdogDecision, WatchdogSignal,
};
pub use watchdog::{evaluate_network_health, run_watchdog, NetworkHealth};
