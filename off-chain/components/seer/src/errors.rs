//! Error types for the Seer module
//!
//! This module defines all error types that can occur during Seer operation.

use thiserror::Error;

/// Main error type for Seer operations
#[derive(Error, Debug)]
pub enum SeerError {
    /// WebSocket connection error
    #[error("WebSocket connection error: {0}")]
    WebSocketError(String),

    /// gRPC connection error
    #[error("gRPC connection error: {0}")]
    GrpcError(String),

    /// Failed to parse binary data
    #[error("Binary parsing error: {0}")]
    BinaryParseError(String),

    /// Invalid program ID
    #[error("Invalid program ID: expected {expected}, got {actual}")]
    InvalidProgramId { expected: String, actual: String },

    /// Failed to deserialize instruction data
    #[error("Failed to deserialize instruction: {0}")]
    DeserializationError(String),

    /// Missing required account
    #[error("Missing required account: {0}")]
    MissingAccount(String),

    /// Invalid discriminator
    #[error("Invalid discriminator: expected {expected:?}, got {actual:?}")]
    InvalidDiscriminator { expected: Vec<u8>, actual: Vec<u8> },

    /// Failed to extract field from instruction
    #[error("Failed to extract field '{field}': {reason}")]
    FieldExtractionError { field: String, reason: String },

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Channel send error
    #[error("Failed to send on channel: {0}")]
    ChannelSendError(String),

    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),

    /// RPC error
    #[error("RPC error: {0}")]
    RpcError(String),

    /// Generic error
    #[error("Seer error: {0}")]
    Generic(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Anyhow error wrapper
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result type alias for Seer operations
pub type SeerResult<T> = Result<T, SeerError>;
