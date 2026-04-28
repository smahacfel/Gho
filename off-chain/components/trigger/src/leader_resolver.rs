//! Leader Resolver - Fetches and caches TPU leader contact information
//!
//! This module resolves Solana validator leaders to their TPU addresses
//! by querying cluster nodes and caching the results.

use crate::errors::{Result, TriggerError};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Default TPU QUIC port offset from TPU port
/// TPU QUIC typically runs on TPU port + 6
const TPU_QUIC_PORT_OFFSET: u16 = 6;

/// Contact information for a TPU leader
#[derive(Debug, Clone)]
pub struct TpuContactInfo {
    /// Validator identity public key
    pub pubkey: Pubkey,
    /// TPU address for UDP transactions
    pub tpu: SocketAddr,
    /// TPU address for QUIC transactions
    pub tpu_quic: SocketAddr,
    /// Gossip address
    pub gossip: Option<SocketAddr>,
}

/// Leader resolver that caches validator contact information
pub struct LeaderResolver {
    /// RPC client for fetching cluster nodes
    rpc_client: Arc<RpcClient>,
    /// Cache mapping validator pubkey to contact info
    cache: Arc<RwLock<HashMap<Pubkey, TpuContactInfo>>>,
    /// Last cache update timestamp
    last_update: Arc<RwLock<SystemTime>>,
    /// Cache validity duration
    cache_duration: Duration,
}

impl LeaderResolver {
    /// Create a new leader resolver
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            rpc_client,
            cache: Arc::new(RwLock::new(HashMap::new())),
            last_update: Arc::new(RwLock::new(UNIX_EPOCH)),
            cache_duration: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Create a new leader resolver with custom cache duration
    pub fn with_cache_duration(rpc_client: Arc<RpcClient>, cache_duration: Duration) -> Self {
        Self {
            rpc_client,
            cache: Arc::new(RwLock::new(HashMap::new())),
            last_update: Arc::new(RwLock::new(UNIX_EPOCH)),
            cache_duration,
        }
    }

    /// Refresh the cache if it's stale
    pub async fn refresh_if_needed(&self) -> Result<()> {
        let now = SystemTime::now();
        let last_update = *self.last_update.read().await;
        let elapsed = now
            .duration_since(last_update)
            .unwrap_or(Duration::from_secs(u64::MAX));

        if elapsed > self.cache_duration || self.cache.read().await.is_empty() {
            self.refresh_cache().await?;
        }

        Ok(())
    }

    /// Force refresh the cluster nodes cache
    async fn refresh_cache(&self) -> Result<()> {
        debug!("Refreshing cluster nodes cache");

        // Fetch cluster nodes from RPC
        let cluster_nodes = self.rpc_client.get_cluster_nodes().map_err(|e| {
            warn!("Failed to fetch cluster nodes: {}", e);
            TriggerError::ClientError(e)
        })?;

        if cluster_nodes.is_empty() {
            warn!("Cluster nodes returned empty");
            return Err(TriggerError::ConfigError(
                "No cluster nodes available".to_string(),
            ));
        }

        let mut cache = self.cache.write().await;
        cache.clear();

        // Parse and cache contact information
        for node in cluster_nodes {
            // Parse the pubkey
            let pubkey = match node.pubkey.parse::<Pubkey>() {
                Ok(pk) => pk,
                Err(e) => {
                    warn!("Failed to parse pubkey {}: {}", node.pubkey, e);
                    continue;
                }
            };

            // Get TPU address
            let tpu = match node.tpu {
                Some(addr) => addr,
                None => {
                    debug!("No TPU address for validator {}", pubkey);
                    continue;
                }
            };

            // Get TPU QUIC address (if available, otherwise derive from TPU)
            let tpu_quic = match node.tpu_quic {
                Some(addr) => addr,
                None => {
                    // Derive TPU QUIC from TPU using standard offset
                    SocketAddr::new(tpu.ip(), tpu.port() + TPU_QUIC_PORT_OFFSET)
                }
            };

            // Get gossip address (optional)
            let gossip = node.gossip;

            let contact_info = TpuContactInfo {
                pubkey,
                tpu,
                tpu_quic,
                gossip,
            };

            cache.insert(pubkey, contact_info);
        }

        *self.last_update.write().await = SystemTime::now();
        info!(
            "Cluster nodes cache refreshed with {} validators",
            cache.len()
        );

        Ok(())
    }

    /// Get TPU contact info for a specific validator
    pub async fn get_contact_info(&self, pubkey: &Pubkey) -> Result<TpuContactInfo> {
        // Refresh cache if needed
        self.refresh_if_needed().await?;

        // Get from cache
        let cache = self.cache.read().await;
        cache.get(pubkey).cloned().ok_or_else(|| {
            TriggerError::ConfigError(format!("No contact info found for validator {}", pubkey))
        })
    }

    /// Get TPU QUIC address for a specific validator
    pub async fn get_tpu_quic_address(&self, pubkey: &Pubkey) -> Result<SocketAddr> {
        let contact_info = self.get_contact_info(pubkey).await?;
        Ok(contact_info.tpu_quic)
    }

    /// Get all cached validators
    pub async fn get_all_validators(&self) -> Vec<Pubkey> {
        self.cache.read().await.keys().copied().collect()
    }

    /// Get cache size
    pub async fn cache_size(&self) -> usize {
        self.cache.read().await.len()
    }

    /// Get current slot from RPC (for panic executor)
    ///
    /// Returns None if RPC call fails (panic executor will use 0 as fallback)
    pub fn get_current_slot(&self) -> Option<u64> {
        self.rpc_client.get_slot().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tpu_contact_info_creation() {
        let pubkey = Pubkey::new_unique();
        let tpu = "127.0.0.1:8001".parse().unwrap();
        let tpu_quic = "127.0.0.1:8007".parse().unwrap();

        let contact_info = TpuContactInfo {
            pubkey,
            tpu,
            tpu_quic,
            gossip: None,
        };

        assert_eq!(contact_info.pubkey, pubkey);
        assert_eq!(contact_info.tpu, tpu);
        assert_eq!(contact_info.tpu_quic, tpu_quic);
        assert!(contact_info.gossip.is_none());
    }

    #[test]
    fn test_leader_resolver_creation() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let resolver = LeaderResolver::new(rpc_client);

        assert_eq!(resolver.cache_duration, Duration::from_secs(300));
    }

    #[test]
    fn test_leader_resolver_with_custom_duration() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let custom_duration = Duration::from_secs(600);
        let resolver = LeaderResolver::with_cache_duration(rpc_client, custom_duration);

        assert_eq!(resolver.cache_duration, custom_duration);
    }

    #[tokio::test]
    async fn test_cache_size_initially_empty() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let resolver = LeaderResolver::new(rpc_client);

        assert_eq!(resolver.cache_size().await, 0);
    }

    #[tokio::test]
    async fn test_get_all_validators_initially_empty() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let resolver = LeaderResolver::new(rpc_client);

        let validators = resolver.get_all_validators().await;
        assert!(validators.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_contact_info_missing_validator() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let resolver = LeaderResolver::new(rpc_client);
        let unknown_pubkey = Pubkey::new_unique();

        // Should fail because cache is empty and RPC call will likely fail or return no match
        let result = resolver.get_contact_info(&unknown_pubkey).await;
        // We expect this to error since we can't actually fetch real data in tests
        assert!(result.is_err());
    }
}
