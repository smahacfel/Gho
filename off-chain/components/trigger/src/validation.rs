//! Pool Validation Module - Off-Chain Security Layer
//!
//! This module implements rigorous off-chain validation for Pump.fun and Bonk.fun pools
//! before building buy instructions. It is critical for preventing loss of funds to
//! honeypot or malicious pools.
//!
//! ## Security Features
//!
//! 1. **Account Owner Verification**: Ensures pool account is owned by legitimate AMM program
//! 2. **PDA Derivation Verification**: Mathematically verifies pool address matches expected PDA
//! 3. **Associated Token Account Verification**: Confirms bonding curve token account exists
//! 4. **BondingCurve Structure Validation**: Deserializes and validates account data structure
//! 5. **TTL-based Caching**: Prevents stale validation results from being used
//!
//! ## Usage
//!
//! ```ignore
//! let validator = PoolValidator::new(60); // 60 second TTL
//! let result = validator.verify_pool_integrity(&pool_id, &mint, &rpc_client).await?;
//! if !result {
//!     return Err(TriggerError::InvalidPool("Pool validation failed".into()));
//! }
//! ```

use crate::errors::{Result, TriggerError};
use ghost_core::market_state::BondingCurve;
use lru::LruCache;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

// ============================================================================
// Program ID Constants
// ============================================================================

/// Pump.fun Program ID (Mainnet)
pub const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Bonk.fun Program ID (Mainnet)
pub const BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

/// Token Program ID
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Associated Token Program ID
pub const ASSOC_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

// ============================================================================
// PDA Seeds
// ============================================================================

/// Seed for Pump.fun bonding curve PDA
const BONDING_CURVE_SEED: &[u8] = b"bonding-curve";

// ============================================================================
// Cache Configuration
// ============================================================================

/// Default TTL for cached validation results (60 seconds)
pub const DEFAULT_CACHE_TTL_SECS: u64 = 60;

/// Default cache capacity (number of pools to cache)
pub const DEFAULT_CACHE_CAPACITY: usize = 1000;

// ============================================================================
// BondingCurve Structure Constants
// ============================================================================

/// Minimum size in bytes for a valid BondingCurve structure
/// Layout: discriminator (8) + virtual_token_reserves (8) + virtual_sol_reserves (8) +
///         real_token_reserves (8) + real_sol_reserves (8) + token_total_supply (8) +
///         complete (1) + padding (7) = 56 bytes
pub const BONDING_CURVE_SIZE: usize = 56;

// ============================================================================
// Validation Result Types
// ============================================================================

/// Result of pool validation with detailed information
#[derive(Debug, Clone)]
pub struct PoolValidationResult {
    /// Whether the pool is valid
    pub is_valid: bool,
    /// Program that owns the pool (if valid)
    pub program_owner: Option<Pubkey>,
    /// Reason for validation failure (if invalid)
    pub failure_reason: Option<String>,
    /// Timestamp of validation
    pub validated_at: Instant,
}

impl PoolValidationResult {
    /// Create a valid result
    pub fn valid(program_owner: Pubkey) -> Self {
        Self {
            is_valid: true,
            program_owner: Some(program_owner),
            failure_reason: None,
            validated_at: Instant::now(),
        }
    }

    /// Create an invalid result with reason
    pub fn invalid(reason: impl Into<String>) -> Self {
        Self {
            is_valid: false,
            program_owner: None,
            failure_reason: Some(reason.into()),
            validated_at: Instant::now(),
        }
    }

    /// Check if this result has expired based on TTL
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.validated_at.elapsed() > ttl
    }
}

/// Cached validation entry with TTL tracking
#[derive(Debug, Clone)]
struct CachedValidation {
    /// Validation result
    result: bool,
    /// Time when validation was performed
    validated_at: Instant,
}

impl CachedValidation {
    fn new(result: bool) -> Self {
        Self {
            result,
            validated_at: Instant::now(),
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.validated_at.elapsed() > ttl
    }
}

// ============================================================================
// Pool Validator
// ============================================================================

/// Pool Validator with LRU caching and TTL support
///
/// This validator performs comprehensive off-chain verification of pool integrity
/// before allowing buy instructions to be built. It caches validation results
/// to minimize RPC calls while ensuring cache entries expire after TTL.
pub struct PoolValidator {
    /// LRU cache for validation results
    cache: Mutex<LruCache<Pubkey, CachedValidation>>,
    /// Time-to-live for cached entries
    ttl: Duration,
    /// Pump.fun program ID
    pump_program_id: Pubkey,
    /// Bonk.fun program ID
    bonk_program_id: Pubkey,
    /// Token program ID
    token_program_id: Pubkey,
    /// Associated token program ID
    assoc_token_program_id: Pubkey,
    /// Allow trading on graduated (completed) pools
    allow_graduated_pools: bool,
    /// Allow trading on pools where associated token account doesn't exist yet (new pools)
    allow_new_pools: bool,
}

impl PoolValidator {
    /// Create a new PoolValidator with specified TTL in seconds
    ///
    /// By default, graduated pools are rejected and new pools are rejected.
    /// Use `with_config()` for more control over these settings.
    pub fn new(ttl_secs: u64) -> Self {
        let capacity = NonZeroUsize::new(DEFAULT_CACHE_CAPACITY).unwrap();
        Self {
            cache: Mutex::new(LruCache::new(capacity)),
            ttl: Duration::from_secs(ttl_secs),
            pump_program_id: Pubkey::from_str(PUMP_PROGRAM_ID).expect("Invalid PUMP_PROGRAM_ID"),
            bonk_program_id: Pubkey::from_str(BONK_PROGRAM_ID).expect("Invalid BONK_PROGRAM_ID"),
            token_program_id: Pubkey::from_str(TOKEN_PROGRAM_ID).expect("Invalid TOKEN_PROGRAM_ID"),
            assoc_token_program_id: Pubkey::from_str(ASSOC_TOKEN_PROGRAM_ID)
                .expect("Invalid ASSOC_TOKEN_PROGRAM_ID"),
            allow_graduated_pools: false,
            allow_new_pools: false,
        }
    }

    /// Create a new PoolValidator with custom configuration
    ///
    /// # Arguments
    /// * `ttl_secs` - Time-to-live for cached validation results
    /// * `allow_graduated_pools` - If true, allows trading on graduated/completed pools
    /// * `allow_new_pools` - If true, allows trading on pools where ATA doesn't exist yet
    pub fn with_config(ttl_secs: u64, allow_graduated_pools: bool, allow_new_pools: bool) -> Self {
        let mut validator = Self::new(ttl_secs);
        validator.allow_graduated_pools = allow_graduated_pools;
        validator.allow_new_pools = allow_new_pools;
        validator
    }

    /// Create a new PoolValidator with default TTL (60 seconds)
    pub fn with_default_ttl() -> Self {
        Self::new(DEFAULT_CACHE_TTL_SECS)
    }

    /// Verify the integrity of a pool before building buy instructions
    ///
    /// This function performs the following checks:
    /// 1. Verifies the pool account exists and is owned by Pump.fun or Bonk.fun
    /// 2. Verifies the pool_id matches the expected PDA derived from mint
    /// 3. Verifies the associated bonding curve token account exists
    /// 4. Deserializes and validates the BondingCurve structure
    ///
    /// # Arguments
    /// * `pool_id` - The pool/bonding curve address to validate
    /// * `mint` - The token mint address
    /// * `rpc_client` - RPC client for blockchain queries
    ///
    /// # Returns
    /// * `Ok(true)` if pool is valid and safe to trade
    /// * `Ok(false)` if pool validation fails (honeypot/invalid)
    /// * `Err(TriggerError)` if RPC or other errors occur
    ///
    /// # Security Note
    /// This function MUST be called before building any buy instruction.
    /// Skipping this check exposes the system to honeypot attacks.
    pub async fn verify_pool_integrity(
        &self,
        pool_id: &Pubkey,
        mint: &Pubkey,
        rpc_client: &RpcClient,
    ) -> Result<bool> {
        // Check cache first
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| TriggerError::Other(format!("Cache lock poisoned: {}", e)))?;

            if let Some(cached) = cache.get(pool_id) {
                if !cached.is_expired(self.ttl) {
                    debug!(
                        pool_id = %pool_id,
                        cached_result = cached.result,
                        "Pool validation cache hit"
                    );
                    return Ok(cached.result);
                }
                // Cache entry expired, will re-validate
                debug!(
                    pool_id = %pool_id,
                    "Pool validation cache expired, re-validating"
                );
            }
        }

        // Perform full validation
        let result = self.validate_pool_full(pool_id, mint, rpc_client).await;

        // Cache the result
        let is_valid = result.as_ref().map(|r| *r).unwrap_or(false);
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| TriggerError::Other(format!("Cache lock poisoned: {}", e)))?;
            cache.put(*pool_id, CachedValidation::new(is_valid));
        }

        result
    }

    /// Perform full pool validation (internal method)
    async fn validate_pool_full(
        &self,
        pool_id: &Pubkey,
        mint: &Pubkey,
        rpc_client: &RpcClient,
    ) -> Result<bool> {
        info!(
            pool_id = %pool_id,
            mint = %mint,
            "Performing full pool validation"
        );

        // ====================================================================
        // Check 1: PDA Verification
        // Verify that the pool_id is correctly derived from mint
        // ====================================================================
        let (expected_pda_pump, _) = Pubkey::find_program_address(
            &[BONDING_CURVE_SEED, mint.as_ref()],
            &self.pump_program_id,
        );

        let (expected_pda_bonk, _) = Pubkey::find_program_address(
            &[BONDING_CURVE_SEED, mint.as_ref()],
            &self.bonk_program_id,
        );

        let is_pump_pda = *pool_id == expected_pda_pump;
        let is_bonk_pda = *pool_id == expected_pda_bonk;

        if !is_pump_pda && !is_bonk_pda {
            error!(
                pool_id = %pool_id,
                expected_pump_pda = %expected_pda_pump,
                expected_bonk_pda = %expected_pda_bonk,
                "PDA MISMATCH: Pool ID does not match expected PDA derivation! POTENTIAL HONEYPOT!"
            );
            return Ok(false);
        }

        debug!(
            pool_id = %pool_id,
            is_pump = is_pump_pda,
            is_bonk = is_bonk_pda,
            "PDA verification passed"
        );

        // ====================================================================
        // Check 2: Account Owner Verification
        // Fetch account info and verify owner is legitimate AMM program
        // ====================================================================
        let account_info = match rpc_client.get_account(pool_id).await {
            Ok(account) => account,
            Err(e) => {
                warn!(
                    pool_id = %pool_id,
                    error = %e,
                    "Failed to fetch pool account - account may not exist"
                );
                return Ok(false);
            }
        };

        let expected_owner = if is_pump_pda {
            self.pump_program_id
        } else {
            self.bonk_program_id
        };

        if account_info.owner != expected_owner {
            error!(
                pool_id = %pool_id,
                actual_owner = %account_info.owner,
                expected_owner = %expected_owner,
                "OWNER MISMATCH: Pool account owner does not match expected program! POTENTIAL HONEYPOT!"
            );
            return Ok(false);
        }

        debug!(
            pool_id = %pool_id,
            owner = %account_info.owner,
            "Account owner verification passed"
        );

        // ====================================================================
        // Check 3: BondingCurve Structure Validation
        // Deserialize account data and validate structure
        // ====================================================================
        if account_info.data.len() < BONDING_CURVE_SIZE {
            error!(
                pool_id = %pool_id,
                data_len = account_info.data.len(),
                expected_min = BONDING_CURVE_SIZE,
                "Account data too small for BondingCurve structure! INVALID POOL!"
            );
            return Ok(false);
        }

        let bonding_curve = match BondingCurve::from_bytes(&account_info.data[..BONDING_CURVE_SIZE])
        {
            Some(curve) => curve,
            None => {
                error!(
                    pool_id = %pool_id,
                    "Failed to deserialize BondingCurve structure! INVALID POOL DATA!"
                );
                return Ok(false);
            }
        };

        // Validate bonding curve is active (not graduated)
        if !bonding_curve.is_active() {
            if !self.allow_graduated_pools {
                warn!(
                    pool_id = %pool_id,
                    "BondingCurve is complete/graduated - rejecting. Set allow_graduated_pools=true to allow."
                );
                return Ok(false);
            }
            warn!(
                pool_id = %pool_id,
                "BondingCurve is complete/graduated - allowing due to configuration"
            );
        }

        // Basic sanity checks on reserves
        if bonding_curve.virtual_token_reserves == 0 || bonding_curve.virtual_sol_reserves == 0 {
            error!(
                pool_id = %pool_id,
                token_reserves = bonding_curve.virtual_token_reserves,
                sol_reserves = bonding_curve.virtual_sol_reserves,
                "BondingCurve has zero reserves! INVALID POOL!"
            );
            return Ok(false);
        }

        debug!(
            pool_id = %pool_id,
            token_reserves = bonding_curve.virtual_token_reserves,
            sol_reserves = bonding_curve.virtual_sol_reserves,
            complete = bonding_curve.complete,
            "BondingCurve structure validation passed"
        );

        // ====================================================================
        // Check 4: Associated Token Account Verification
        // Verify that the bonding curve's token account exists
        // ====================================================================
        let associated_bonding_curve = self.derive_associated_token_address(pool_id, mint);

        match rpc_client.get_account(&associated_bonding_curve).await {
            Ok(ata_account) => {
                // Verify the ATA is owned by the Token Program
                if ata_account.owner != self.token_program_id {
                    error!(
                        ata = %associated_bonding_curve,
                        actual_owner = %ata_account.owner,
                        expected_owner = %self.token_program_id,
                        "Associated token account has wrong owner! POTENTIAL HONEYPOT!"
                    );
                    return Ok(false);
                }
                debug!(
                    ata = %associated_bonding_curve,
                    "Associated token account verification passed"
                );
            }
            Err(e) => {
                if !self.allow_new_pools {
                    warn!(
                        ata = %associated_bonding_curve,
                        error = %e,
                        "Associated token account does not exist - rejecting. Set allow_new_pools=true to allow."
                    );
                    return Ok(false);
                }
                warn!(
                    ata = %associated_bonding_curve,
                    error = %e,
                    "Associated token account does not exist - allowing due to configuration (new pool)"
                );
            }
        }

        info!(
            pool_id = %pool_id,
            mint = %mint,
            program = if is_pump_pda { "Pump.fun" } else { "Bonk.fun" },
            "Pool validation PASSED - safe to trade"
        );

        Ok(true)
    }

    /// Derive the associated token address for a wallet and mint
    fn derive_associated_token_address(&self, wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(
            &[
                &wallet.to_bytes(),
                &self.token_program_id.to_bytes(),
                &mint.to_bytes(),
            ],
            &self.assoc_token_program_id,
        )
        .0
    }

    /// Invalidate a cached entry for a specific pool
    pub fn invalidate_cache(&self, pool_id: &Pubkey) -> Result<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|e| TriggerError::Other(format!("Cache lock poisoned: {}", e)))?;
        cache.pop(pool_id);
        debug!(pool_id = %pool_id, "Cache entry invalidated");
        Ok(())
    }

    /// Clear all cached entries
    pub fn clear_cache(&self) -> Result<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|e| TriggerError::Other(format!("Cache lock poisoned: {}", e)))?;
        cache.clear();
        debug!("All cache entries cleared");
        Ok(())
    }

    /// Get current cache statistics
    pub fn cache_stats(&self) -> Result<(usize, usize)> {
        let cache = self
            .cache
            .lock()
            .map_err(|e| TriggerError::Other(format!("Cache lock poisoned: {}", e)))?;
        Ok((cache.len(), cache.cap().get()))
    }
}

// ============================================================================
// Standalone Verification Functions
// ============================================================================

/// Verify that a discriminator matches the expected Pump.fun "buy" instruction
///
/// The discriminator should be the first 8 bytes of SHA256("global:buy")
/// Expected value: 0x66063d1201daebea
///
/// # Returns
/// `true` if the discriminator is valid, `false` otherwise
pub fn verify_buy_discriminator(discriminator: &[u8; 8]) -> bool {
    const EXPECTED_DISCRIMINATOR: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
    discriminator == &EXPECTED_DISCRIMINATOR
}

/// Verify that a discriminator matches the expected Pump.fun "sell" instruction
///
/// The discriminator should be the first 8 bytes of SHA256("global:sell")
/// Expected value: 0x33e685a4017f83ad
///
/// # Returns
/// `true` if the discriminator is valid, `false` otherwise
pub fn verify_sell_discriminator(discriminator: &[u8; 8]) -> bool {
    const EXPECTED_DISCRIMINATOR: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
    discriminator == &EXPECTED_DISCRIMINATOR
}

/// Derive the expected bonding curve PDA for a given mint
///
/// # Arguments
/// * `mint` - The token mint address
/// * `program_id` - The AMM program ID (Pump.fun or Bonk.fun)
///
/// # Returns
/// The derived PDA and bump seed
pub fn derive_bonding_curve_pda(mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[BONDING_CURVE_SEED, mint.as_ref()], program_id)
}

/// Check if a program ID is a whitelisted AMM program
pub fn is_whitelisted_amm_program(program_id: &Pubkey) -> bool {
    let pump_id = Pubkey::from_str(PUMP_PROGRAM_ID).expect("Invalid PUMP_PROGRAM_ID");
    let bonk_id = Pubkey::from_str(BONK_PROGRAM_ID).expect("Invalid BONK_PROGRAM_ID");
    *program_id == pump_id || *program_id == bonk_id
}

// ============================================================================
// Default Implementation
// ============================================================================

impl Default for PoolValidator {
    fn default() -> Self {
        Self::with_default_ttl()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn test_verify_buy_discriminator_valid() {
        let valid_discriminator: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
        assert!(verify_buy_discriminator(&valid_discriminator));
    }

    #[test]
    fn test_verify_buy_discriminator_invalid() {
        let invalid_discriminator: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(!verify_buy_discriminator(&invalid_discriminator));
    }

    #[test]
    fn test_verify_sell_discriminator_valid() {
        let valid_discriminator: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
        assert!(verify_sell_discriminator(&valid_discriminator));
    }

    #[test]
    fn test_verify_sell_discriminator_invalid() {
        let invalid_discriminator: [u8; 8] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(!verify_sell_discriminator(&invalid_discriminator));
    }

    /// Test that the discriminator matches SHA256("global:buy")[..8]
    /// This mathematically verifies the discriminator against Anchor's IDL convention
    #[test]
    fn test_discriminator_matches_sha256_hash() {
        let expected_discriminator: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];

        // Compute SHA256("global:buy") and take first 8 bytes
        let mut hasher = Sha256::new();
        hasher.update(b"global:buy");
        let hash = hasher.finalize();
        let computed_discriminator: [u8; 8] = hash[0..8].try_into().unwrap();

        assert_eq!(
            expected_discriminator, computed_discriminator,
            "Discriminator mismatch! Expected {:02x?}, got {:02x?}. \
             This indicates the BUY_DISCRIMINATOR constant needs to be updated.",
            expected_discriminator, computed_discriminator
        );
    }

    /// Test that the sell discriminator matches SHA256("global:sell")[..8]
    #[test]
    fn test_sell_discriminator_matches_sha256_hash() {
        let expected_discriminator: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

        // Compute SHA256("global:sell") and take first 8 bytes
        let mut hasher = Sha256::new();
        hasher.update(b"global:sell");
        let hash = hasher.finalize();
        let computed_discriminator: [u8; 8] = hash[0..8].try_into().unwrap();

        assert_eq!(
            expected_discriminator, computed_discriminator,
            "Discriminator mismatch! Expected {:02x?}, got {:02x?}. \
             This indicates the SELL_DISCRIMINATOR constant needs to be updated.",
            expected_discriminator, computed_discriminator
        );
    }

    #[test]
    fn test_derive_bonding_curve_pda() {
        let mint = Pubkey::new_unique();
        let pump_id = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();

        let (pda, bump) = derive_bonding_curve_pda(&mint, &pump_id);

        // PDA should be different from mint
        assert_ne!(pda, mint);
        // Bump should be valid (0-255)
        assert!(bump <= 255);

        // Same mint should always derive same PDA
        let (pda2, bump2) = derive_bonding_curve_pda(&mint, &pump_id);
        assert_eq!(pda, pda2);
        assert_eq!(bump, bump2);
    }

    #[test]
    fn test_is_whitelisted_amm_program() {
        let pump_id = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let bonk_id = Pubkey::from_str(BONK_PROGRAM_ID).unwrap();
        let random_id = Pubkey::new_unique();

        assert!(is_whitelisted_amm_program(&pump_id));
        assert!(is_whitelisted_amm_program(&bonk_id));
        assert!(!is_whitelisted_amm_program(&random_id));
    }

    #[test]
    fn test_pool_validator_creation() {
        let validator = PoolValidator::new(30);
        assert_eq!(validator.ttl, Duration::from_secs(30));

        let default_validator = PoolValidator::with_default_ttl();
        assert_eq!(
            default_validator.ttl,
            Duration::from_secs(DEFAULT_CACHE_TTL_SECS)
        );
    }

    #[test]
    fn test_cached_validation_expiry() {
        let cached = CachedValidation::new(true);

        // Should not be expired immediately
        assert!(!cached.is_expired(Duration::from_secs(60)));

        // Would be expired with 0 TTL
        assert!(cached.is_expired(Duration::from_secs(0)));
    }

    #[test]
    fn test_pool_validation_result_creation() {
        let pump_id = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();

        let valid = PoolValidationResult::valid(pump_id);
        assert!(valid.is_valid);
        assert_eq!(valid.program_owner, Some(pump_id));
        assert!(valid.failure_reason.is_none());

        let invalid = PoolValidationResult::invalid("Test failure");
        assert!(!invalid.is_valid);
        assert!(invalid.program_owner.is_none());
        assert_eq!(invalid.failure_reason, Some("Test failure".to_string()));
    }

    #[test]
    fn test_pool_validation_result_expiry() {
        let pump_id = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();
        let result = PoolValidationResult::valid(pump_id);

        // Should not be expired immediately
        assert!(!result.is_expired(Duration::from_secs(60)));

        // Would be expired with 0 TTL
        assert!(result.is_expired(Duration::from_secs(0)));
    }

    #[test]
    fn test_cache_stats() {
        let validator = PoolValidator::new(60);
        let (len, cap) = validator.cache_stats().unwrap();

        assert_eq!(len, 0);
        assert_eq!(cap, DEFAULT_CACHE_CAPACITY);
    }

    #[test]
    fn test_program_id_constants() {
        // Verify program ID constants are valid Pubkeys
        let pump_id = Pubkey::from_str(PUMP_PROGRAM_ID);
        assert!(pump_id.is_ok(), "PUMP_PROGRAM_ID should be valid");

        let bonk_id = Pubkey::from_str(BONK_PROGRAM_ID);
        assert!(bonk_id.is_ok(), "BONK_PROGRAM_ID should be valid");

        let token_id = Pubkey::from_str(TOKEN_PROGRAM_ID);
        assert!(token_id.is_ok(), "TOKEN_PROGRAM_ID should be valid");

        let assoc_id = Pubkey::from_str(ASSOC_TOKEN_PROGRAM_ID);
        assert!(assoc_id.is_ok(), "ASSOC_TOKEN_PROGRAM_ID should be valid");
    }

    #[test]
    fn test_derive_associated_token_address() {
        let validator = PoolValidator::new(60);
        let wallet = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let ata = validator.derive_associated_token_address(&wallet, &mint);

        // ATA should be different from both wallet and mint
        assert_ne!(ata, wallet);
        assert_ne!(ata, mint);

        // Same inputs should always derive same ATA
        let ata2 = validator.derive_associated_token_address(&wallet, &mint);
        assert_eq!(ata, ata2);
    }

    #[test]
    fn test_pool_validator_with_config() {
        // Default configuration rejects graduated and new pools
        let default_validator = PoolValidator::new(60);
        assert!(!default_validator.allow_graduated_pools);
        assert!(!default_validator.allow_new_pools);

        // Custom configuration allows both
        let permissive_validator = PoolValidator::with_config(30, true, true);
        assert_eq!(permissive_validator.ttl, Duration::from_secs(30));
        assert!(permissive_validator.allow_graduated_pools);
        assert!(permissive_validator.allow_new_pools);

        // Custom configuration allows only graduated pools
        let graduated_only = PoolValidator::with_config(45, true, false);
        assert!(graduated_only.allow_graduated_pools);
        assert!(!graduated_only.allow_new_pools);

        // Custom configuration allows only new pools
        let new_only = PoolValidator::with_config(45, false, true);
        assert!(!new_only.allow_graduated_pools);
        assert!(new_only.allow_new_pools);
    }

    #[test]
    fn test_bonding_curve_size_constant() {
        // Verify the BONDING_CURVE_SIZE constant matches the expected structure size
        assert_eq!(BONDING_CURVE_SIZE, 56);

        // Verify it matches the size of BondingCurve struct (if accessible)
        // This ensures consistency between the constant and the actual struct
        assert_eq!(
            BONDING_CURVE_SIZE,
            std::mem::size_of::<ghost_core::market_state::BondingCurve>()
        );
    }
}
