//! Wallet Loader - Load payer keypair from environment configuration
//!
//! This module provides functionality to load a Solana keypair from either:
//! - A file path specified by `WALLET_KEYPAIR_PATH` environment variable
//! - A base58-encoded private key in `WALLET_PRIVATE_KEY` environment variable
//!
//! The file-based loading uses the existing `load_keypair` function from `config.rs`.

use crate::config::load_keypair;
use anyhow::{Context, Result};
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use tracing::{debug, info};

/// Environment variable name for the keypair file path
const WALLET_KEYPAIR_PATH_ENV: &str = "WALLET_KEYPAIR_PATH";

/// Environment variable name for the base58-encoded private key
const WALLET_PRIVATE_KEY_ENV: &str = "WALLET_PRIVATE_KEY";

/// Load the payer keypair from environment configuration.
///
/// This function tries the following in order:
/// 1. Load from file path specified in `WALLET_KEYPAIR_PATH` environment variable
/// 2. Load from base58-encoded private key in `WALLET_PRIVATE_KEY` environment variable
///
/// # Returns
/// - `Ok(Keypair)` if successfully loaded
/// - `Err` if neither environment variable is set or if loading fails
///
/// # Example
/// ```ignore
/// // Set environment variable
/// std::env::set_var("WALLET_KEYPAIR_PATH", "~/.config/solana/id.json");
///
/// // Load keypair
/// let keypair = load_payer_keypair()?;
/// println!("Loaded wallet: {}", keypair.pubkey());
/// ```
pub fn load_payer_keypair() -> Result<Keypair> {
    // Try loading from file path first
    if let Ok(path) = std::env::var(WALLET_KEYPAIR_PATH_ENV) {
        debug!("Loading keypair from file: {}", path);
        let keypair = load_keypair(&path);
        info!("Loaded payer keypair from file: {}", keypair.pubkey());
        return Ok(keypair);
    }

    // Try loading from base58-encoded private key
    if let Ok(private_key_b58) = std::env::var(WALLET_PRIVATE_KEY_ENV) {
        debug!("Loading keypair from base58-encoded private key");
        let keypair = load_keypair_from_base58(&private_key_b58)
            .context("Failed to decode base58 private key")?;
        info!("Loaded payer keypair from base58: {}", keypair.pubkey());
        return Ok(keypair);
    }

    anyhow::bail!(
        "No wallet keypair configured. Set either {} (file path) or {} (base58 private key) environment variable.",
        WALLET_KEYPAIR_PATH_ENV,
        WALLET_PRIVATE_KEY_ENV
    )
}

/// Load a keypair from a base58-encoded private key string.
///
/// The private key should be a 64-byte keypair (32-byte secret key + 32-byte public key)
/// encoded in base58 format.
///
/// # Arguments
/// * `base58_key` - Base58-encoded keypair bytes (64 bytes)
///
/// # Returns
/// - `Ok(Keypair)` if successfully decoded
/// - `Err` if decoding fails or the key length is invalid
fn load_keypair_from_base58(base58_key: &str) -> Result<Keypair> {
    let bytes = bs58::decode(base58_key.trim())
        .into_vec()
        .context("Invalid private key format")?;

    if bytes.len() != 64 {
        anyhow::bail!(
            "Invalid private key length: expected 64 bytes, got {} bytes. \
             The key should be a full Solana keypair (32-byte secret key + 32-byte public key).",
            bytes.len()
        );
    }

    Keypair::from_bytes(&bytes).context("Invalid private key")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_keypair_from_base58() {
        // Generate a test keypair and encode it
        let original_keypair = Keypair::new();
        let encoded = bs58::encode(original_keypair.to_bytes()).into_string();

        // Load it back
        let loaded = load_keypair_from_base58(&encoded).unwrap();

        assert_eq!(original_keypair.pubkey(), loaded.pubkey());
    }

    #[test]
    fn test_load_keypair_from_base58_invalid_length() {
        // Too short
        let short_key = bs58::encode(vec![0u8; 32]).into_string();
        let result = load_keypair_from_base58(&short_key);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid private key length"));
        assert!(err_msg.contains("expected 64 bytes"));
    }

    #[test]
    fn test_load_keypair_from_base58_invalid_encoding() {
        let result = load_keypair_from_base58("not-valid-base58-!!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_payer_keypair_no_env() {
        // Clear environment variables
        std::env::remove_var(WALLET_KEYPAIR_PATH_ENV);
        std::env::remove_var(WALLET_PRIVATE_KEY_ENV);

        let result = load_payer_keypair();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No wallet keypair configured"));
    }
}
