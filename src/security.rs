//! Security utilities for input validation, authentication, and rate limiting.
//!
//! This module provides comprehensive security validation functions including:
//! - Solana public key validation
//! - RPC URL validation with HTTPS enforcement
//! - Authentication token validation
//! - Input sanitization for logging
//! - Score range validation

use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use url::Url;

/// Validate a Solana public key string.
///
/// # Arguments
/// * `pubkey_str` - The public key string to validate
///
/// # Returns
/// * `Ok(Pubkey)` if valid
/// * `Err` if invalid format
pub fn validate_solana_pubkey(pubkey_str: &str) -> Result<Pubkey> {
    Pubkey::from_str(pubkey_str).map_err(|e| anyhow!("Invalid Solana public key: {}", e))
}

/// Validate an RPC URL.
///
/// # Arguments
/// * `url_str` - The URL string to validate
///
/// # Returns
/// * `Ok(String)` if valid HTTPS URL
/// * `Err` if invalid or not HTTPS
pub fn validate_rpc_url(url_str: &str) -> Result<String> {
    let url = Url::parse(url_str).map_err(|e| anyhow!("Invalid URL format: {}", e))?;

    // Require HTTPS in production (allow HTTP for localhost/testing)
    if url.scheme() != "https"
        && !url.host_str().unwrap_or("").contains("localhost")
        && !url.host_str().unwrap_or("").starts_with("127.0.0.1")
    {
        return Err(anyhow!("RPC URL must use HTTPS for security"));
    }

    Ok(url.to_string())
}

/// Validate an authentication token.
///
/// # Arguments
/// * `token` - The token string to validate
///
/// # Returns
/// * `Ok(())` if valid (minimum length, printable ASCII)
/// * `Err` if invalid
pub fn validate_auth_token(token: &str) -> Result<()> {
    const MIN_TOKEN_LENGTH: usize = 32;

    if token.len() < MIN_TOKEN_LENGTH {
        return Err(anyhow!(
            "Authentication token must be at least {} characters",
            MIN_TOKEN_LENGTH
        ));
    }

    if !token.chars().all(|c| c.is_ascii_graphic()) {
        return Err(anyhow!(
            "Authentication token must contain only printable ASCII characters"
        ));
    }

    Ok(())
}

/// Sanitize log output to prevent sensitive data leakage.
///
/// # Arguments
/// * `input` - The string to sanitize
///
/// # Returns
/// * Sanitized string with potential secrets redacted
pub fn sanitize_for_logging(input: &str) -> String {
    // Redact patterns that look like private keys, tokens, or API keys
    let patterns = [
        (r"[a-zA-Z0-9]{64,}", "[REDACTED-KEY]"),
        (r"Bearer [a-zA-Z0-9\-._~+/]+=*", "Bearer [REDACTED]"),
        (r"token[=:]\s*[a-zA-Z0-9\-._~+/]+=*", "token=[REDACTED]"),
        (r"key[=:]\s*[a-zA-Z0-9\-._~+/]+=*", "key=[REDACTED]"),
    ];

    let mut output = input.to_string();
    for (pattern, replacement) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            output = re.replace_all(&output, replacement).to_string();
        }
    }
    output
}

/// Validate a score value is within expected bounds.
///
/// # Arguments
/// * `score` - The score to validate
/// * `min` - Minimum acceptable value
/// * `max` - Maximum acceptable value
///
/// # Returns
/// * `Ok(())` if valid
/// * `Err` if out of bounds
pub fn validate_score_range(score: i64, min: i64, max: i64) -> Result<()> {
    if score < min || score > max {
        return Err(anyhow!(
            "Score {} is out of valid range [{}, {}]",
            score,
            min,
            max
        ));
    }
    Ok(())
}

/// Validate a mint address format.
///
/// # Arguments
/// * `mint` - The mint address string to validate
///
/// # Returns
/// * `Ok(())` if valid Solana address format
/// * `Err` if invalid
pub fn validate_mint_address(mint: &str) -> Result<()> {
    validate_solana_pubkey(mint)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_solana_pubkey_valid() {
        // Valid Solana pubkey (base58 encoded)
        let result = validate_solana_pubkey("11111111111111111111111111111111");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_solana_pubkey_invalid() {
        let result = validate_solana_pubkey("invalid_pubkey");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_rpc_url_https() {
        let result = validate_rpc_url("https://api.mainnet-beta.solana.com");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rpc_url_http_localhost() {
        let result = validate_rpc_url("http://localhost:8899");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rpc_url_http_production() {
        let result = validate_rpc_url("http://api.mainnet-beta.solana.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_auth_token_valid() {
        let token = "a".repeat(32);
        let result = validate_auth_token(&token);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_auth_token_too_short() {
        let token = "short";
        let result = validate_auth_token(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_score_range_valid() {
        assert!(validate_score_range(50, 0, 100).is_ok());
    }

    #[test]
    fn test_validate_score_range_below_min() {
        assert!(validate_score_range(-10, 0, 100).is_err());
    }

    #[test]
    fn test_validate_score_range_above_max() {
        assert!(validate_score_range(150, 0, 100).is_err());
    }

    #[test]
    fn test_sanitize_for_logging() {
        let input = "Bearer abc123token456 key=secret789";
        let output = sanitize_for_logging(input);
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("abc123token456"));
    }
}
