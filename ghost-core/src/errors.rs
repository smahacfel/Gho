//! Error types for Ghost Core
//!
//! This module defines the error types used across the Ghost Core library,
//! particularly for Shadow Ledger operations and state management.

use thiserror::Error;

/// Errors that can occur in Ghost Core operations
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum GhostError {
    /// Shadow Ledger state is stale (too old)
    ///
    /// This error indicates that the cached bonding curve state in the Shadow Ledger
    /// is outdated and should be refreshed via RPC getAccountInfo.
    ///
    /// **Trigger Action**: When this error occurs, the Trigger component must
    /// fall back to fetching fresh state from RPC to ensure accurate pricing.
    #[error("Shadow Ledger state is stale: current_slot={current_slot}, last_updated_slot={last_updated_slot}, max_age={max_age}")]
    StaleState {
        /// Current slot number
        current_slot: u64,
        /// Slot when state was last updated
        last_updated_slot: u64,
        /// Maximum allowed age in slots
        max_age: u64,
    },

    /// Bonding curve not found in Shadow Ledger
    #[error("Bonding curve not found for mint: {0}")]
    CurveNotFound(String),

    /// Invalid bonding curve data
    #[error("Invalid bonding curve data: {0}")]
    InvalidCurveData(String),

    /// RPC error during fallback
    #[error("RPC fallback failed: {0}")]
    RpcFallbackFailed(String),

    /// Price calculation error
    #[error("Price calculation error: {0}")]
    PriceCalculationError(String),

    /// Seed generation timeout
    #[error("Seed generation timeout: {0}")]
    SeedGenerationTimeout(String),
}

/// Result type for Ghost Core operations
pub type GhostResult<T> = std::result::Result<T, GhostError>;
