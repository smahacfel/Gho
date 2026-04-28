//! Error types for the Trigger module

use thiserror::Error;

/// Errors that can occur in the Trigger module
#[derive(Error, Debug)]
pub enum TriggerError {
    /// Swap plan validation failed
    #[error("Swap plan validation failed: {0}")]
    InvalidSwapPlan(String),

    /// Invalid pool - validation failed (CRITICAL SECURITY ERROR)
    #[error("Invalid pool - validation failed: {0}")]
    InvalidPool(String),

    /// LUT address not found in configuration
    #[error("LUT address not found: {0}")]
    LutAddressNotFound(String),

    /// Transaction building failed
    #[error("Transaction building failed: {0}")]
    TransactionBuildFailed(String),

    /// Transaction sending failed
    #[error("Transaction sending failed: {0}")]
    SendFailed(String),

    /// Solana SDK error
    #[error("Solana SDK error: {0}")]
    SolanaError(#[from] solana_sdk::transaction::TransactionError),

    /// Solana client error
    #[error("Solana client error: {0}")]
    ClientError(#[from] solana_client::client_error::ClientError),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Jito bundle error
    #[error("Jito bundle error: {0}")]
    JitoBundleError(String),

    /// Jito bundle may still land after a non-accepted status; callers must fail closed
    #[error("Jito bundle landing uncertain: {0}")]
    UncertainBundleLanding(String),

    /// Metrics error
    #[error("Metrics error: {0}")]
    MetricsError(String),

    /// Network error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Generic error
    #[error("Error: {0}")]
    Other(String),

    /// TTL violation - blockhash too stale
    #[error("TTL violation: {0}")]
    TtlViolation(String),

    /// Blockhash fetch took too long
    #[error("Blockhash fetch timeout: {0}")]
    StaleBlockhash(String),

    /// Transaction simulation failed
    #[error("Transaction simulation failed: {0}")]
    SimulationFailed(String),

    /// Transaction aborted due to failed validation
    #[error("Transaction aborted: {0}")]
    TransactionAborted(String),

    /// Safety validation failed (Bulkhead/TipGuard)
    #[error("Safety validation failed: {0}")]
    ValidationFailed(String),
}

/// Result type for Trigger operations
pub type Result<T> = std::result::Result<T, TriggerError>;
