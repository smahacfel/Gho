//! RPC Provider Module - Live Blockhash Management
//!
//! This module provides infrastructure for maintaining fresh blockhashes
//! required for transaction signing. The BlockhashProvider runs a background
//! task that periodically fetches the latest blockhash from the Solana network.
//!
//! # Architecture
//!
//! ```text
//! BlockhashProvider
//!     ├── current_blockhash: Arc<RwLock<Hash>>  (thread-safe shared state)
//!     ├── rpc_client: RpcClient                 (Solana RPC connection)
//!     └── refresh_task: JoinHandle              (background refresh task)
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use trigger::BlockhashProvider;
//!
//! let provider = BlockhashProvider::new("https://api.devnet.solana.com").await?;
//! let blockhash = provider.get_blockhash();
//! ```

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Default interval for blockhash refresh (in milliseconds)
/// Solana blockhashes are valid for ~60-90 seconds, so refreshing every 2 seconds
/// ensures we always have a fresh blockhash while not overwhelming the RPC.
const DEFAULT_REFRESH_INTERVAL_MS: u64 = 2000;

/// Maximum age of blockhash before it's considered stale (in milliseconds)
/// If a blockhash is older than this, transactions may fail.
const MAX_BLOCKHASH_AGE_MS: u64 = 30000;

/// Live Blockhash Provider
///
/// Maintains a fresh blockhash by periodically fetching from the Solana network.
/// The blockhash is stored in an `Arc<RwLock<Hash>>` for thread-safe access.
pub struct BlockhashProvider {
    /// Current blockhash (thread-safe shared state)
    current_blockhash: Arc<RwLock<Hash>>,
    /// Timestamp of last successful blockhash update
    last_update: Arc<RwLock<std::time::Instant>>,
    /// RPC client for fetching blockhash
    rpc_client: Arc<RpcClient>,
    /// Handle to the background refresh task
    refresh_task: Option<JoinHandle<()>>,
    /// Refresh interval in milliseconds
    refresh_interval_ms: u64,
}

impl BlockhashProvider {
    /// Create a new BlockhashProvider and start the background refresh task
    ///
    /// # Arguments
    /// * `rpc_url` - URL of the Solana RPC endpoint
    ///
    /// # Returns
    /// * `Ok(BlockhashProvider)` - Successfully initialized provider
    /// * `Err` - Failed to connect to RPC or fetch initial blockhash
    pub async fn new(rpc_url: &str) -> anyhow::Result<Self> {
        Self::with_refresh_interval(rpc_url, DEFAULT_REFRESH_INTERVAL_MS).await
    }

    /// Create a new BlockhashProvider with a custom refresh interval
    ///
    /// # Arguments
    /// * `rpc_url` - URL of the Solana RPC endpoint
    /// * `refresh_interval_ms` - Interval between blockhash refreshes in milliseconds
    ///
    /// # Returns
    /// * `Ok(BlockhashProvider)` - Successfully initialized provider
    /// * `Err` - Failed to connect to RPC or fetch initial blockhash
    pub async fn with_refresh_interval(
        rpc_url: &str,
        refresh_interval_ms: u64,
    ) -> anyhow::Result<Self> {
        info!("Initializing BlockhashProvider with RPC: {}", rpc_url);

        let rpc_client = Arc::new(RpcClient::new(rpc_url.to_string()));

        // Fetch initial blockhash
        let initial_blockhash = rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch initial blockhash: {}", e))?;

        info!("Initial blockhash: {}", initial_blockhash);

        let current_blockhash = Arc::new(RwLock::new(initial_blockhash));
        let last_update = Arc::new(RwLock::new(std::time::Instant::now()));

        let mut provider = Self {
            current_blockhash,
            last_update,
            rpc_client,
            refresh_task: None,
            refresh_interval_ms,
        };

        // Start background refresh task
        provider.start_refresh_task();

        Ok(provider)
    }

    /// Start the background task that periodically refreshes the blockhash
    fn start_refresh_task(&mut self) {
        let blockhash = Arc::clone(&self.current_blockhash);
        let last_update = Arc::clone(&self.last_update);
        let rpc_client = Arc::clone(&self.rpc_client);
        let interval = Duration::from_millis(self.refresh_interval_ms);

        let task = tokio::spawn(async move {
            info!(
                "BlockhashProvider refresh task started (interval: {}ms)",
                interval.as_millis()
            );

            loop {
                tokio::time::sleep(interval).await;

                match rpc_client.get_latest_blockhash().await {
                    Ok(new_blockhash) => {
                        let mut hash = blockhash.write().await;
                        let old_hash = *hash;
                        *hash = new_blockhash;
                        drop(hash);

                        let mut update_time = last_update.write().await;
                        *update_time = std::time::Instant::now();
                        drop(update_time);

                        if old_hash != new_blockhash {
                            debug!("Blockhash updated: {} -> {}", old_hash, new_blockhash);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to refresh blockhash: {}", e);
                        // Continue with the existing blockhash, but log warning
                    }
                }
            }
        });

        self.refresh_task = Some(task);
    }

    /// Get the current blockhash
    ///
    /// # Returns
    /// The most recently fetched blockhash
    pub async fn get_blockhash(&self) -> Hash {
        *self.current_blockhash.read().await
    }

    /// Get the current blockhash if it's fresh enough
    ///
    /// # Returns
    /// * `Some(Hash)` - Blockhash if it was updated within MAX_BLOCKHASH_AGE_MS
    /// * `None` - Blockhash is stale
    pub async fn get_blockhash_if_fresh(&self) -> Option<Hash> {
        let last_update = *self.last_update.read().await;
        let age = last_update.elapsed();

        if age.as_millis() > MAX_BLOCKHASH_AGE_MS as u128 {
            warn!(
                "Blockhash is stale (age: {}ms, max: {}ms)",
                age.as_millis(),
                MAX_BLOCKHASH_AGE_MS
            );
            return None;
        }

        Some(self.get_blockhash().await)
    }

    /// Force an immediate blockhash refresh
    ///
    /// # Returns
    /// * `Ok(Hash)` - New blockhash after refresh
    /// * `Err` - Failed to fetch blockhash
    pub async fn force_refresh(&self) -> anyhow::Result<Hash> {
        let new_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to refresh blockhash: {}", e))?;

        {
            let mut hash = self.current_blockhash.write().await;
            *hash = new_blockhash;
        }

        {
            let mut update_time = self.last_update.write().await;
            *update_time = std::time::Instant::now();
        }

        debug!("Blockhash force refreshed: {}", new_blockhash);
        Ok(new_blockhash)
    }

    /// Get the age of the current blockhash in milliseconds
    pub async fn blockhash_age_ms(&self) -> u64 {
        let last_update = *self.last_update.read().await;
        last_update.elapsed().as_millis() as u64
    }

    /// Check if the blockhash is fresh (within acceptable age)
    pub async fn is_blockhash_fresh(&self) -> bool {
        self.blockhash_age_ms().await < MAX_BLOCKHASH_AGE_MS
    }

    /// Stop the background refresh task
    pub fn stop(&mut self) {
        if let Some(task) = self.refresh_task.take() {
            task.abort();
            info!("BlockhashProvider refresh task stopped");
        }
    }

    /// Get a clone of the shared blockhash reference
    /// Useful for sharing the blockhash across multiple components
    pub fn get_blockhash_ref(&self) -> Arc<RwLock<Hash>> {
        Arc::clone(&self.current_blockhash)
    }
}

impl Drop for BlockhashProvider {
    fn drop(&mut self) {
        if let Some(task) = self.refresh_task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_constants() {
        assert_eq!(DEFAULT_REFRESH_INTERVAL_MS, 2000);
        assert_eq!(MAX_BLOCKHASH_AGE_MS, 30000);
    }

    #[tokio::test]
    async fn test_blockhash_provider_mock() {
        // Test with a mock hash since we can't connect to RPC in tests
        let blockhash = Arc::new(RwLock::new(Hash::new_unique()));
        let last_update = Arc::new(RwLock::new(std::time::Instant::now()));

        // Verify we can read the blockhash
        let hash = *blockhash.read().await;
        assert!(!hash.to_string().is_empty());

        // Verify age calculation
        let age = last_update.read().await.elapsed();
        assert!(age.as_millis() < 1000); // Should be very fresh
    }

    #[tokio::test]
    async fn test_blockhash_freshness_check() {
        let last_update = Arc::new(RwLock::new(std::time::Instant::now()));

        // Fresh blockhash
        let age = last_update.read().await.elapsed();
        assert!(age.as_millis() < MAX_BLOCKHASH_AGE_MS as u128);

        // Simulate stale blockhash by creating an old timestamp
        let old_time =
            std::time::Instant::now() - Duration::from_millis(MAX_BLOCKHASH_AGE_MS + 1000);
        *last_update.write().await = old_time;

        let age = last_update.read().await.elapsed();
        assert!(age.as_millis() > MAX_BLOCKHASH_AGE_MS as u128);
    }

    #[test]
    fn test_blockhash_provider_drop() {
        // Test that Drop doesn't panic when refresh_task is None
        let provider = BlockhashProvider {
            current_blockhash: Arc::new(RwLock::new(Hash::default())),
            last_update: Arc::new(RwLock::new(std::time::Instant::now())),
            rpc_client: Arc::new(RpcClient::new("http://localhost:8899".to_string())),
            refresh_task: None,
            refresh_interval_ms: DEFAULT_REFRESH_INTERVAL_MS,
        };
        drop(provider);
        // Test passes if no panic
    }
}
