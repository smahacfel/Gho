//! Execution Guard - Safe Buy Instruction Builder with Pool Validation
//!
//! This module provides a wrapper around DirectBuyBuilder that ALWAYS validates
//! pools before building buy instructions. This is a critical security layer
//! to prevent honeypot attacks and trading on malicious pools.
//!
//! ## Security Features
//!
//! - **Mandatory Validation**: Pool validation is NOT optional - it happens on every build
//! - **Cached Validation**: Uses PoolValidator's TTL-based caching for performance
//! - **Clear Error Types**: Distinguishes between validation failures and pool rejection
//!
//! ## Usage
//!
//! ```ignore
//! use trigger::SafeBuyBuilder;
//! use solana_client::nonblocking::rpc_client::RpcClient;
//! use std::sync::Arc;
//!
//! let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
//! let safe_builder = SafeBuyBuilder::new(rpc_client, 60);
//!
//! // This will ALWAYS validate the pool first
//! let ix = safe_builder.build_validated_buy_ix(
//!     &payer,
//!     &mint,
//!     &pool_id,
//!     amount_sol_in,
//!     min_tokens_out,
//! ).await?;
//! ```

use crate::direct_buy_builder::DirectBuyBuilder;
use crate::validation::PoolValidator;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during safe buy execution
#[derive(Error, Debug)]
pub enum ExecutionGuardError {
    /// Pool validation RPC/parsing failed
    #[error("Pool validation failed: {0}")]
    ValidationFailed(String),

    /// Pool failed validation checks (potential honeypot)
    #[error("Pool not whitelisted: {0}")]
    PoolNotWhitelisted(String),
}

/// Safe buy instruction builder that ALWAYS validates pools first
///
/// This wrapper ensures that no buy instruction can be built without
/// first verifying the pool's integrity through `PoolValidator`.
///
/// Unlike `DirectBuyBuilder::build_buy_ix()`, this builder:
/// - Requires an RPC client for validation
/// - Caches validation results based on TTL
/// - Returns descriptive errors for validation failures
pub struct SafeBuyBuilder {
    /// Pool validator with caching
    validator: Arc<PoolValidator>,
    /// RPC client for blockchain queries
    rpc_client: Arc<RpcClient>,
}

impl SafeBuyBuilder {
    /// Create a new SafeBuyBuilder
    ///
    /// # Arguments
    /// * `rpc_client` - RPC client for pool validation queries
    /// * `cache_ttl_secs` - Time-to-live for cached validation results
    ///
    /// # Example
    /// ```ignore
    /// let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    /// let builder = SafeBuyBuilder::new(rpc_client, 60); // 60 second cache TTL
    /// ```
    pub fn new(rpc_client: Arc<RpcClient>, cache_ttl_secs: u64) -> Self {
        Self {
            validator: Arc::new(PoolValidator::new(cache_ttl_secs)),
            rpc_client,
        }
    }

    /// Create a new SafeBuyBuilder with custom validator configuration
    ///
    /// # Arguments
    /// * `rpc_client` - RPC client for pool validation queries
    /// * `cache_ttl_secs` - Time-to-live for cached validation results
    /// * `allow_graduated_pools` - If true, allows trading on graduated/completed pools
    /// * `allow_new_pools` - If true, allows trading on pools where ATA doesn't exist yet
    pub fn with_config(
        rpc_client: Arc<RpcClient>,
        cache_ttl_secs: u64,
        allow_graduated_pools: bool,
        allow_new_pools: bool,
    ) -> Self {
        Self {
            validator: Arc::new(PoolValidator::with_config(
                cache_ttl_secs,
                allow_graduated_pools,
                allow_new_pools,
            )),
            rpc_client,
        }
    }

    /// Build buy instruction ONLY after successful pool validation
    ///
    /// This method performs the following steps:
    /// 1. Validates pool integrity via PoolValidator
    /// 2. Returns error if validation fails
    /// 3. Builds the buy instruction only on success
    ///
    /// # Arguments
    /// * `payer` - The wallet paying for the transaction (signer)
    /// * `mint` - The token mint address being purchased
    /// * `pool_id` - The pool/bonding curve address (explicit parameter for clarity)
    /// * `amount_sol_in` - Maximum SOL to spend (in lamports)
    /// * `min_tokens_out` - Minimum tokens to receive (slippage protection)
    ///
    /// # Returns
    /// * `Ok(Instruction)` - Buy instruction if validation passes
    /// * `Err(ExecutionGuardError)` - If validation fails or pool is rejected
    ///
    /// # Security Note
    /// This is the RECOMMENDED way to build buy instructions in production.
    /// Using `DirectBuyBuilder::build_buy_ix()` directly skips validation.
    pub async fn build_validated_buy_ix(
        &self,
        payer: &Pubkey,
        mint: &Pubkey,
        pool_id: &Pubkey,
        amount_sol_in: u64,
        min_tokens_out: u64,
    ) -> Result<Instruction, ExecutionGuardError> {
        // ALWAYS validate first - this is non-negotiable
        let is_valid = self
            .validator
            .verify_pool_integrity(pool_id, mint, &self.rpc_client)
            .await
            .map_err(|e| ExecutionGuardError::ValidationFailed(e.to_string()))?;

        if !is_valid {
            return Err(ExecutionGuardError::PoolNotWhitelisted(format!(
                "Pool {} failed validation for mint {}",
                pool_id, mint
            )));
        }

        // Pool is valid, build the instruction
        Ok(DirectBuyBuilder::build_buy_ix(
            payer,
            mint,
            amount_sol_in,
            min_tokens_out,
        ))
    }

    /// Build buy instruction using derived pool_id from mint
    ///
    /// This is a convenience method that derives the pool_id from the mint
    /// using Pump.fun's bonding curve PDA derivation.
    ///
    /// # Arguments
    /// * `payer` - The wallet paying for the transaction (signer)
    /// * `mint` - The token mint address being purchased
    /// * `amount_sol_in` - Maximum SOL to spend (in lamports)
    /// * `min_tokens_out` - Minimum tokens to receive (slippage protection)
    ///
    /// # Returns
    /// * `Ok(Instruction)` - Buy instruction if validation passes
    /// * `Err(ExecutionGuardError)` - If validation fails or pool is rejected
    pub async fn build_validated_buy_ix_from_mint(
        &self,
        payer: &Pubkey,
        mint: &Pubkey,
        amount_sol_in: u64,
        min_tokens_out: u64,
    ) -> Result<Instruction, ExecutionGuardError> {
        // Derive the bonding curve PDA for this mint
        // Note: bump seed is ignored since we only need the derived address for validation
        let (bonding_curve, _bump) = DirectBuyBuilder::derive_bonding_curve(mint);

        self.build_validated_buy_ix(payer, mint, &bonding_curve, amount_sol_in, min_tokens_out)
            .await
    }

    /// Get access to the underlying validator for cache operations
    ///
    /// This can be used to invalidate cache entries or get statistics.
    pub fn validator(&self) -> &PoolValidator {
        &self.validator
    }

    /// Invalidate a cached validation result for a specific pool
    ///
    /// Use this when you have reason to believe the pool state has changed
    /// and cached validation results may be stale.
    pub fn invalidate_pool_cache(&self, pool_id: &Pubkey) -> Result<(), ExecutionGuardError> {
        self.validator
            .invalidate_cache(pool_id)
            .map_err(|e| ExecutionGuardError::ValidationFailed(e.to_string()))
    }

    /// Clear all cached validation results
    pub fn clear_cache(&self) -> Result<(), ExecutionGuardError> {
        self.validator
            .clear_cache()
            .map_err(|e| ExecutionGuardError::ValidationFailed(e.to_string()))
    }

    /// Get cache statistics (current size, capacity)
    pub fn cache_stats(&self) -> Result<(usize, usize), ExecutionGuardError> {
        self.validator
            .cache_stats()
            .map_err(|e| ExecutionGuardError::ValidationFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_guard_error_display() {
        let validation_err = ExecutionGuardError::ValidationFailed("RPC timeout".to_string());
        assert!(validation_err
            .to_string()
            .contains("Pool validation failed"));
        assert!(validation_err.to_string().contains("RPC timeout"));

        let whitelist_err = ExecutionGuardError::PoolNotWhitelisted("ABC123".to_string());
        assert!(whitelist_err.to_string().contains("Pool not whitelisted"));
        assert!(whitelist_err.to_string().contains("ABC123"));
    }

    #[test]
    fn test_safe_buy_builder_creation() {
        let rpc_client = Arc::new(RpcClient::new("http://localhost:8899".to_string()));
        let builder = SafeBuyBuilder::new(rpc_client.clone(), 60);

        // Verify cache is initialized
        let (len, cap) = builder.cache_stats().unwrap();
        assert_eq!(len, 0);
        assert!(cap > 0);
    }

    #[test]
    fn test_safe_buy_builder_with_config() {
        let rpc_client = Arc::new(RpcClient::new("http://localhost:8899".to_string()));
        let builder = SafeBuyBuilder::with_config(rpc_client.clone(), 30, true, true);

        // Verify builder is created with custom config
        let (len, _) = builder.cache_stats().unwrap();
        assert_eq!(len, 0);
    }

    #[test]
    fn test_cache_operations() {
        let rpc_client = Arc::new(RpcClient::new("http://localhost:8899".to_string()));
        let builder = SafeBuyBuilder::new(rpc_client.clone(), 60);

        // Test cache operations don't panic
        let pool_id = Pubkey::new_unique();
        assert!(builder.invalidate_pool_cache(&pool_id).is_ok());
        assert!(builder.clear_cache().is_ok());

        let (len, cap) = builder.cache_stats().unwrap();
        assert_eq!(len, 0);
        assert!(cap > 0);
    }

    #[test]
    fn test_validator_access() {
        let rpc_client = Arc::new(RpcClient::new("http://localhost:8899".to_string()));
        let builder = SafeBuyBuilder::new(rpc_client.clone(), 45);

        // Verify we can access the validator
        let validator = builder.validator();
        let (_, cap) = validator.cache_stats().unwrap();
        assert!(cap > 0);
    }

    // Note: Async tests for build_validated_buy_ix would require a mock RPC client
    // or integration test setup with a local validator.
    //
    // The actual validation logic is tested in validation.rs tests.
    // Here we primarily test the wrapper construction and error handling.
}
