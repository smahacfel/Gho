//! Price Oracle Module
//!
//! Fetches current token prices from AMM pools (Pump.fun and Bonk.fun)
//! by reading on-chain pool reserves and calculating prices using the
//! constant product formula.

use anyhow::{Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{account::Account, pubkey::Pubkey};
use std::str::FromStr;
use std::sync::Arc;
use tracing::debug;

/// Known AMM program IDs
pub mod amm_programs {
    use super::*;

    pub fn pump_fun() -> Pubkey {
        Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap()
    }

    pub fn bonk_fun() -> Pubkey {
        Pubkey::from_str("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj").unwrap()
    }
}

/// Pool information for price calculation
#[derive(Debug, Clone)]
pub struct PoolInfo {
    /// Virtual token reserves (used for bonding curve)
    pub virtual_token_reserves: u64,

    /// Virtual SOL reserves (used for bonding curve)
    pub virtual_sol_reserves: u64,

    /// Real token reserves (actual tokens in pool)
    pub real_token_reserves: Option<u64>,

    /// Real SOL reserves (actual SOL in pool)
    pub real_sol_reserves: Option<u64>,
}

impl PoolInfo {
    /// Calculate current price in lamports per token
    ///
    /// Price = (virtual_sol_reserves * 10^9) / virtual_token_reserves
    /// This represents how many lamports (10^-9 SOL) you get per base token unit
    pub fn calculate_price(&self) -> u64 {
        if self.virtual_token_reserves == 0 {
            return 0;
        }

        // Use 128-bit arithmetic to prevent overflow
        let price = (self.virtual_sol_reserves as u128 * 1_000_000_000)
            / self.virtual_token_reserves as u128;

        // Clamp to u64 max if overflow
        price.min(u64::MAX as u128) as u64
    }
}

/// Price Oracle for fetching token prices from AMM pools
pub struct PriceOracle {
    rpc_client: std::sync::Arc<RpcClient>,
}

impl PriceOracle {
    /// Create new price oracle
    pub fn new(rpc_client: std::sync::Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Fetch token price from bonding curve pool
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    /// * `pool_address` - Optional pool address (if known)
    ///
    /// # Returns
    /// Price in lamports per token
    pub async fn fetch_price(&self, mint: &Pubkey, pool_address: Option<&Pubkey>) -> Result<u64> {
        // If pool address is provided, use it directly
        if let Some(pool) = pool_address {
            return self.fetch_price_from_pool(pool).await;
        }

        // Otherwise, try to find the pool by deriving bonding curve PDA
        self.fetch_price_by_mint(mint).await
    }

    /// Fetch price from a specific pool address
    async fn fetch_price_from_pool(&self, pool_address: &Pubkey) -> Result<u64> {
        let pool_address = *pool_address;
        let rpc_client = Arc::clone(&self.rpc_client);

        // Fetch the pool account data in a blocking task
        let account = tokio::task::spawn_blocking(move || rpc_client.get_account(&pool_address))
            .await
            .context("Task join error")?
            .context("Failed to fetch pool account")?;

        // Parse the pool data to extract reserves
        let pool_info = self.parse_pool_account(&account)?;

        // Calculate and return price
        let price = pool_info.calculate_price();
        debug!(
            "Pool {}: virtual_sol={}, virtual_token={}, price={} lamports",
            pool_address, pool_info.virtual_sol_reserves, pool_info.virtual_token_reserves, price
        );

        Ok(price)
    }

    /// Fetch price by deriving pool address from mint
    async fn fetch_price_by_mint(&self, mint: &Pubkey) -> Result<u64> {
        // Try Pump.fun bonding curve PDA
        if let Ok(price) = self.try_pumpfun_pool(mint).await {
            return Ok(price);
        }

        // Try Bonk.fun bonding curve PDA
        if let Ok(price) = self.try_bonkfun_pool(mint).await {
            return Ok(price);
        }

        anyhow::bail!("Could not find pool for mint {}", mint)
    }

    /// Try to fetch price from Pump.fun pool
    async fn try_pumpfun_pool(&self, mint: &Pubkey) -> Result<u64> {
        let bonding_curve_pda = self.derive_bonding_curve_pda(mint, &amm_programs::pump_fun())?;

        debug!("Trying Pump.fun bonding curve: {}", bonding_curve_pda);
        self.fetch_price_from_pool(&bonding_curve_pda).await
    }

    /// Try to fetch price from Bonk.fun pool
    async fn try_bonkfun_pool(&self, mint: &Pubkey) -> Result<u64> {
        let bonding_curve_pda = self.derive_bonding_curve_pda(mint, &amm_programs::bonk_fun())?;

        debug!("Trying Bonk.fun bonding curve: {}", bonding_curve_pda);
        self.fetch_price_from_pool(&bonding_curve_pda).await
    }

    /// Derive bonding curve PDA for a mint
    ///
    /// Seeds: ["bonding-curve", mint]
    fn derive_bonding_curve_pda(&self, mint: &Pubkey, program_id: &Pubkey) -> Result<Pubkey> {
        let (pda, _bump) =
            Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], program_id);
        Ok(pda)
    }

    /// Parse pool account data to extract reserve information
    ///
    /// Expected data layout (typical bonding curve account):
    /// - Discriminator: 8 bytes
    /// - virtual_token_reserves: u64 (8 bytes) at offset 8
    /// - virtual_sol_reserves: u64 (8 bytes) at offset 16
    /// - real_token_reserves: u64 (8 bytes) at offset 24
    /// - real_sol_reserves: u64 (8 bytes) at offset 32
    fn parse_pool_account(&self, account: &Account) -> Result<PoolInfo> {
        let data = &account.data;

        if data.len() < 40 {
            anyhow::bail!("Pool account data too short: {} bytes", data.len());
        }

        // Skip discriminator (8 bytes) and parse reserves
        let virtual_token_reserves = u64::from_le_bytes(
            data[8..16]
                .try_into()
                .context("Failed to parse virtual_token_reserves")?,
        );

        let virtual_sol_reserves = u64::from_le_bytes(
            data[16..24]
                .try_into()
                .context("Failed to parse virtual_sol_reserves")?,
        );

        let real_token_reserves = if data.len() >= 32 {
            Some(u64::from_le_bytes(
                data[24..32]
                    .try_into()
                    .context("Failed to parse real_token_reserves")?,
            ))
        } else {
            None
        };

        let real_sol_reserves = if data.len() >= 40 {
            Some(u64::from_le_bytes(
                data[32..40]
                    .try_into()
                    .context("Failed to parse real_sol_reserves")?,
            ))
        } else {
            None
        };

        Ok(PoolInfo {
            virtual_token_reserves,
            virtual_sol_reserves,
            real_token_reserves,
            real_sol_reserves,
        })
    }
}

/// Cache for pool addresses to avoid repeated PDA derivations
pub struct PoolAddressCache {
    cache: std::sync::RwLock<std::collections::HashMap<Pubkey, Pubkey>>,
}

impl PoolAddressCache {
    /// Create new pool address cache
    pub fn new() -> Self {
        Self {
            cache: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Get pool address from cache
    pub fn get(&self, mint: &Pubkey) -> Option<Pubkey> {
        self.cache.read().unwrap().get(mint).copied()
    }

    /// Insert pool address into cache
    pub fn insert(&self, mint: Pubkey, pool: Pubkey) {
        self.cache.write().unwrap().insert(mint, pool);
    }

    /// Clear the cache
    pub fn clear(&self) {
        self.cache.write().unwrap().clear();
    }
}

impl Default for PoolAddressCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_info_calculate_price() {
        let pool = PoolInfo {
            virtual_token_reserves: 1_000_000_000, // 1 billion tokens
            virtual_sol_reserves: 100_000_000,     // 0.1 SOL
            real_token_reserves: None,
            real_sol_reserves: None,
        };

        // Price = (100_000_000 * 1_000_000_000) / 1_000_000_000 = 100_000_000 lamports
        // This means 0.1 SOL per token
        assert_eq!(pool.calculate_price(), 100_000_000);
    }

    #[test]
    fn test_pool_info_calculate_price_high_ratio() {
        let pool = PoolInfo {
            virtual_token_reserves: 1_000_000,   // 1 million tokens
            virtual_sol_reserves: 1_000_000_000, // 1 SOL
            real_token_reserves: None,
            real_sol_reserves: None,
        };

        // Price = (1_000_000_000 * 1_000_000_000) / 1_000_000 = 1_000_000_000_000 lamports per token
        // This means 1000 SOL per token
        assert_eq!(pool.calculate_price(), 1_000_000_000_000);
    }

    #[test]
    fn test_pool_info_calculate_price_zero_reserves() {
        let pool = PoolInfo {
            virtual_token_reserves: 0,
            virtual_sol_reserves: 1_000_000_000,
            real_token_reserves: None,
            real_sol_reserves: None,
        };

        // Should return 0 to avoid division by zero
        assert_eq!(pool.calculate_price(), 0);
    }

    #[test]
    fn test_derive_bonding_curve_pda() {
        let mint = Pubkey::new_unique();
        let program_id = amm_programs::pump_fun();

        let (pda, _bump) =
            Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program_id);

        // Verify PDA derivation is deterministic
        let (pda2, _bump2) =
            Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program_id);

        assert_eq!(pda, pda2);
    }

    #[test]
    fn test_pool_address_cache() {
        let cache = PoolAddressCache::new();
        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();

        // Initially empty
        assert!(cache.get(&mint).is_none());

        // Insert and retrieve
        cache.insert(mint, pool);
        assert_eq!(cache.get(&mint), Some(pool));

        // Clear
        cache.clear();
        assert!(cache.get(&mint).is_none());
    }
}
