//! Leader Tracker - Epoch-aware Leader Schedule Cache
//!
//! This module provides a leader schedule cache that tracks slot-to-leader mappings
//! with automatic refresh on epoch changes. It resolves leader pubkeys to TPU socket
//! addresses for direct transaction submission.
//!
//! # Architecture
//!
//! ```text
//! LeaderTracker
//!     ├── epoch_info: EpochInfo                (current epoch state)
//!     ├── leader_schedule: HashMap<Slot, Pubkey>  (slot -> leader mapping)
//!     ├── cluster_nodes: HashMap<Pubkey, ClusterNode>  (validator contact info)
//!     └── rpc_client: RpcClient               (Solana RPC connection)
//! ```
//!
//! # Features
//!
//! - Epoch-aware caching with automatic refresh
//! - Slot-to-leader resolution
//! - TPU socket address resolution (excluding localhost)
//! - Metrics for resolution latency
//!
//! # Usage
//!
//! ```ignore
//! use trigger::LeaderTracker;
//!
//! let tracker = LeaderTracker::new("https://api.mainnet-beta.solana.com").await?;
//! let sockets = tracker.get_leader_tpu_sockets(3).await?;  // Get next 3 leaders' TPU sockets
//! ```

use crate::errors::{Result, TriggerError};
use crate::metrics::TriggerMetrics;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    clock::{Epoch, Slot},
    epoch_info::EpochInfo,
    pubkey::Pubkey,
};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Default cache refresh interval for epoch checks (seconds)
const DEFAULT_EPOCH_CHECK_INTERVAL_SECS: u64 = 10;

/// Default cache duration for cluster nodes (seconds)
const DEFAULT_CLUSTER_NODES_CACHE_SECS: u64 = 300;

/// TPU QUIC port offset from TPU UDP port
const TPU_QUIC_PORT_OFFSET: u16 = 6;

/// Localhost IPv4 address to filter out
const LOCALHOST_IPV4: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

/// TPU transport protocol selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpuProtocol {
    /// UDP transport (faster, less reliable)
    Udp,
    /// QUIC transport (more stable, recommended)
    Quic,
}

impl Default for TpuProtocol {
    fn default() -> Self {
        TpuProtocol::Udp
    }
}

/// Configuration for the leader tracker
#[derive(Debug, Clone)]
pub struct LeaderTrackerConfig {
    /// Interval for checking epoch changes (seconds)
    pub epoch_check_interval_secs: u64,
    /// Cache duration for cluster nodes (seconds)
    pub cluster_nodes_cache_secs: u64,
    /// Default TPU protocol to use
    pub default_protocol: TpuProtocol,
    /// Whether to filter out localhost addresses
    pub filter_localhost: bool,
}

impl Default for LeaderTrackerConfig {
    fn default() -> Self {
        Self {
            epoch_check_interval_secs: DEFAULT_EPOCH_CHECK_INTERVAL_SECS,
            cluster_nodes_cache_secs: DEFAULT_CLUSTER_NODES_CACHE_SECS,
            default_protocol: TpuProtocol::Udp,
            filter_localhost: true,
        }
    }
}

/// Cluster node contact information
#[derive(Debug, Clone)]
pub struct ClusterNodeInfo {
    /// Validator identity pubkey
    pub pubkey: Pubkey,
    /// TPU UDP socket address
    pub tpu_udp: Option<SocketAddr>,
    /// TPU QUIC socket address  
    pub tpu_quic: Option<SocketAddr>,
    /// Gossip address
    pub gossip: Option<SocketAddr>,
}

/// Result of leader TPU socket resolution
#[derive(Debug, Clone)]
pub struct LeaderTpuInfo {
    /// Leader validator pubkey
    pub leader_pubkey: Pubkey,
    /// Slot when this validator is the leader
    pub slot: Slot,
    /// TPU socket address (UDP or QUIC based on protocol)
    pub tpu_socket: SocketAddr,
    /// Protocol used
    pub protocol: TpuProtocol,
}

/// Epoch-aware leader schedule cache with TPU address resolution
pub struct LeaderTracker {
    /// RPC client for fetching data
    rpc_client: Arc<RpcClient>,
    /// Cached epoch info
    epoch_info: Arc<RwLock<Option<EpochInfo>>>,
    /// Cached leader schedule: slot -> leader pubkey
    leader_schedule: Arc<RwLock<HashMap<Slot, Pubkey>>>,
    /// Epoch for which the leader schedule is valid
    schedule_epoch: Arc<RwLock<Option<Epoch>>>,
    /// First slot of the cached epoch
    epoch_start_slot: Arc<RwLock<Option<Slot>>>,
    /// Cached cluster nodes: pubkey -> contact info
    cluster_nodes: Arc<RwLock<HashMap<Pubkey, ClusterNodeInfo>>>,
    /// Last cluster nodes update timestamp
    cluster_nodes_updated: Arc<RwLock<SystemTime>>,
    /// Last epoch check timestamp
    last_epoch_check: Arc<RwLock<Instant>>,
    /// Configuration
    config: LeaderTrackerConfig,
    /// Optional metrics collector
    metrics: Option<Arc<TriggerMetrics>>,
}

impl LeaderTracker {
    /// Create a new leader tracker with default configuration
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self::with_config(rpc_client, LeaderTrackerConfig::default())
    }

    /// Create a new leader tracker with custom configuration
    pub fn with_config(rpc_client: Arc<RpcClient>, config: LeaderTrackerConfig) -> Self {
        Self {
            rpc_client,
            epoch_info: Arc::new(RwLock::new(None)),
            leader_schedule: Arc::new(RwLock::new(HashMap::new())),
            schedule_epoch: Arc::new(RwLock::new(None)),
            epoch_start_slot: Arc::new(RwLock::new(None)),
            cluster_nodes: Arc::new(RwLock::new(HashMap::new())),
            cluster_nodes_updated: Arc::new(RwLock::new(UNIX_EPOCH)),
            last_epoch_check: Arc::new(RwLock::new(Instant::now())),
            config,
            metrics: None,
        }
    }

    /// Create a new leader tracker with metrics
    pub fn with_metrics(
        rpc_client: Arc<RpcClient>,
        config: LeaderTrackerConfig,
        metrics: Arc<TriggerMetrics>,
    ) -> Self {
        Self {
            rpc_client,
            epoch_info: Arc::new(RwLock::new(None)),
            leader_schedule: Arc::new(RwLock::new(HashMap::new())),
            schedule_epoch: Arc::new(RwLock::new(None)),
            epoch_start_slot: Arc::new(RwLock::new(None)),
            cluster_nodes: Arc::new(RwLock::new(HashMap::new())),
            cluster_nodes_updated: Arc::new(RwLock::new(UNIX_EPOCH)),
            last_epoch_check: Arc::new(RwLock::new(Instant::now())),
            config,
            metrics: Some(metrics),
        }
    }

    /// Get the current slot from RPC
    pub fn get_current_slot(&self) -> Result<Slot> {
        self.rpc_client
            .get_slot()
            .map_err(TriggerError::ClientError)
    }

    /// Get epoch info, refreshing if necessary
    pub async fn get_epoch_info(&self) -> Result<EpochInfo> {
        // Check if we need to refresh epoch info
        let should_refresh = {
            let last_check = *self.last_epoch_check.read().await;
            last_check.elapsed().as_secs() > self.config.epoch_check_interval_secs
        };

        if should_refresh || self.epoch_info.read().await.is_none() {
            self.refresh_epoch_info().await?;
        }

        self.epoch_info
            .read()
            .await
            .clone()
            .ok_or_else(|| TriggerError::ConfigError("Epoch info not available".to_string()))
    }

    /// Force refresh epoch info from RPC
    async fn refresh_epoch_info(&self) -> Result<()> {
        let start = Instant::now();

        let epoch_info = self.rpc_client.get_epoch_info().map_err(|e| {
            error!("Failed to fetch epoch info: {}", e);
            TriggerError::ClientError(e)
        })?;

        let elapsed = start.elapsed();
        debug!(
            "Epoch info refreshed in {:?} - epoch: {}, slot: {}, slots_in_epoch: {}",
            elapsed, epoch_info.epoch, epoch_info.absolute_slot, epoch_info.slots_in_epoch
        );

        // Check if epoch changed
        let old_epoch = {
            let info = self.epoch_info.read().await;
            info.as_ref().map(|i| i.epoch)
        };

        let epoch_changed = old_epoch != Some(epoch_info.epoch);

        // Update epoch info
        *self.epoch_info.write().await = Some(epoch_info.clone());
        *self.last_epoch_check.write().await = Instant::now();

        // If epoch changed, invalidate leader schedule
        if epoch_changed {
            info!(
                "Epoch changed: {:?} -> {}, refreshing leader schedule",
                old_epoch, epoch_info.epoch
            );
            self.refresh_leader_schedule(epoch_info.epoch).await?;
        }

        Ok(())
    }

    /// Refresh the leader schedule for the current or specified epoch
    pub async fn refresh_leader_schedule(&self, epoch: Epoch) -> Result<()> {
        let start = Instant::now();

        debug!("Refreshing leader schedule for epoch {}", epoch);

        // Get leader schedule from RPC
        let schedule = self.rpc_client.get_leader_schedule(None).map_err(|e| {
            error!("Failed to fetch leader schedule: {}", e);
            TriggerError::ClientError(e)
        })?;

        let schedule_map = schedule.ok_or_else(|| {
            TriggerError::ConfigError("Leader schedule not available from RPC".to_string())
        })?;

        if schedule_map.is_empty() {
            warn!("Leader schedule returned empty from RPC");
            return Err(TriggerError::ConfigError(
                "Leader schedule is empty".to_string(),
            ));
        }

        // Get epoch start slot from cached epoch info (don't call get_epoch_info to avoid recursion)
        let epoch_start_slot = {
            let info = self.epoch_info.read().await;
            match &*info {
                Some(ei) => ei.absolute_slot - ei.slot_index,
                None => {
                    // If no epoch info is cached, fetch it directly without going through get_epoch_info
                    let ei = self
                        .rpc_client
                        .get_epoch_info()
                        .map_err(TriggerError::ClientError)?;
                    ei.absolute_slot - ei.slot_index
                }
            }
        };

        // Build slot -> leader mapping
        let mut new_schedule = HashMap::new();
        for (leader_str, slot_indices) in schedule_map {
            if let Ok(leader_pubkey) = leader_str.parse::<Pubkey>() {
                for slot_index in slot_indices {
                    let absolute_slot = epoch_start_slot + slot_index as u64;
                    new_schedule.insert(absolute_slot, leader_pubkey);
                }
            } else {
                warn!("Failed to parse leader pubkey: {}", leader_str);
            }
        }

        let elapsed = start.elapsed();
        info!(
            "Leader schedule refreshed in {:?} for epoch {} with {} slot mappings",
            elapsed,
            epoch,
            new_schedule.len()
        );

        // Update cache
        *self.leader_schedule.write().await = new_schedule;
        *self.schedule_epoch.write().await = Some(epoch);
        *self.epoch_start_slot.write().await = Some(epoch_start_slot);

        Ok(())
    }

    /// Refresh cluster nodes cache
    pub async fn refresh_cluster_nodes(&self) -> Result<()> {
        let start = Instant::now();

        debug!("Refreshing cluster nodes cache");

        let cluster_nodes = self.rpc_client.get_cluster_nodes().map_err(|e| {
            error!("Failed to fetch cluster nodes: {}", e);
            TriggerError::ClientError(e)
        })?;

        if cluster_nodes.is_empty() {
            warn!("Cluster nodes returned empty from RPC");
            return Err(TriggerError::ConfigError(
                "No cluster nodes available".to_string(),
            ));
        }

        let mut new_nodes = HashMap::new();
        let mut localhost_filtered = 0;

        for node in cluster_nodes {
            let pubkey = match node.pubkey.parse::<Pubkey>() {
                Ok(pk) => pk,
                Err(e) => {
                    warn!("Failed to parse node pubkey {}: {}", node.pubkey, e);
                    continue;
                }
            };

            // Get TPU UDP address
            let tpu_udp = node.tpu;

            // Check for localhost and filter if configured
            if self.config.filter_localhost {
                if let Some(addr) = tpu_udp {
                    if Self::is_localhost(&addr) {
                        localhost_filtered += 1;
                        // Note: Individual localhost filtering logged at trace level to avoid performance impact
                        continue;
                    }
                }
            }

            // Get TPU QUIC address (derive from TPU UDP if not provided)
            let tpu_quic = node.tpu_quic.or_else(|| {
                tpu_udp.map(|addr| SocketAddr::new(addr.ip(), addr.port() + TPU_QUIC_PORT_OFFSET))
            });

            let gossip = node.gossip;

            let node_info = ClusterNodeInfo {
                pubkey,
                tpu_udp,
                tpu_quic,
                gossip,
            };

            new_nodes.insert(pubkey, node_info);
        }

        let elapsed = start.elapsed();
        info!(
            "Cluster nodes refreshed in {:?} with {} validators ({} localhost filtered)",
            elapsed,
            new_nodes.len(),
            localhost_filtered
        );

        if localhost_filtered > 0 {
            warn!(
                "Filtered {} validators with localhost TPU addresses (127.0.0.1)",
                localhost_filtered
            );
        }

        *self.cluster_nodes.write().await = new_nodes;
        *self.cluster_nodes_updated.write().await = SystemTime::now();

        Ok(())
    }

    /// Check if an address is localhost
    fn is_localhost(addr: &SocketAddr) -> bool {
        match addr.ip() {
            IpAddr::V4(ipv4) => ipv4 == LOCALHOST_IPV4 || ipv4.is_loopback(),
            IpAddr::V6(ipv6) => ipv6.is_loopback(),
        }
    }

    /// Ensure caches are fresh and refresh if needed
    pub async fn ensure_caches_fresh(&self) -> Result<()> {
        // Refresh epoch info (this also triggers leader schedule refresh if epoch changed)
        self.get_epoch_info().await?;

        // Check if leader schedule needs refresh
        let schedule_epoch = *self.schedule_epoch.read().await;
        let current_epoch = self.epoch_info.read().await.as_ref().map(|i| i.epoch);

        if schedule_epoch != current_epoch {
            if let Some(epoch) = current_epoch {
                self.refresh_leader_schedule(epoch).await?;
            }
        }

        // Check if cluster nodes need refresh
        let cluster_age = {
            let updated = *self.cluster_nodes_updated.read().await;
            SystemTime::now()
                .duration_since(updated)
                .unwrap_or(Duration::from_secs(u64::MAX))
        };

        if cluster_age.as_secs() > self.config.cluster_nodes_cache_secs
            || self.cluster_nodes.read().await.is_empty()
        {
            self.refresh_cluster_nodes().await?;
        }

        Ok(())
    }

    /// Get the leader pubkey for a specific slot
    pub async fn get_leader_at_slot(&self, slot: Slot) -> Result<Pubkey> {
        self.ensure_caches_fresh().await?;

        let schedule = self.leader_schedule.read().await;
        schedule
            .get(&slot)
            .copied()
            .ok_or_else(|| TriggerError::Other(format!("No leader found for slot {}", slot)))
    }

    /// Get TPU socket addresses for upcoming leaders
    ///
    /// # Arguments
    /// * `limit` - Maximum number of leader TPU sockets to return
    ///
    /// # Returns
    /// Vector of socket addresses for upcoming leaders, excluding localhost addresses
    pub async fn get_leader_tpu_sockets(&self, limit: usize) -> Result<Vec<SocketAddr>> {
        let leader_info = self
            .get_leader_tpu_sockets_with_info(limit, self.config.default_protocol)
            .await?;

        Ok(leader_info
            .into_iter()
            .map(|info| info.tpu_socket)
            .collect())
    }

    /// Get TPU socket addresses for upcoming leaders with detailed info
    ///
    /// # Arguments
    /// * `limit` - Maximum number of leader TPU sockets to return
    /// * `protocol` - TPU protocol to use (UDP or QUIC)
    ///
    /// # Returns
    /// Vector of LeaderTpuInfo containing socket addresses and metadata
    pub async fn get_leader_tpu_sockets_with_info(
        &self,
        limit: usize,
        protocol: TpuProtocol,
    ) -> Result<Vec<LeaderTpuInfo>> {
        let start = Instant::now();

        self.ensure_caches_fresh().await?;

        let current_slot = self.get_current_slot()?;
        let schedule = self.leader_schedule.read().await;
        let nodes = self.cluster_nodes.read().await;

        let mut results = Vec::with_capacity(limit);
        let mut checked_slots = 0;
        let max_slots_to_check = limit * 10; // Check up to 10x the limit to handle missing entries

        for offset in 0..max_slots_to_check {
            if results.len() >= limit {
                break;
            }

            let target_slot = current_slot + offset as u64;
            checked_slots += 1;

            // Get leader for this slot
            let leader_pubkey = match schedule.get(&target_slot) {
                Some(pk) => *pk,
                None => {
                    debug!("No leader found for slot {} in schedule", target_slot);
                    continue;
                }
            };

            // Get contact info for this leader
            let node_info = match nodes.get(&leader_pubkey) {
                Some(info) => info,
                None => {
                    warn!(
                        "No contact info for leader {} at slot {}",
                        leader_pubkey, target_slot
                    );
                    continue;
                }
            };

            // Get appropriate TPU socket based on protocol
            let tpu_socket = match protocol {
                TpuProtocol::Udp => node_info.tpu_udp,
                TpuProtocol::Quic => node_info.tpu_quic,
            };

            let socket = match tpu_socket {
                Some(addr) => {
                    // Double-check localhost filtering
                    if self.config.filter_localhost && Self::is_localhost(&addr) {
                        warn!(
                            "Skipping localhost address {} for leader {} at slot {}",
                            addr, leader_pubkey, target_slot
                        );
                        continue;
                    }
                    addr
                }
                None => {
                    debug!(
                        "No {:?} TPU address for leader {} at slot {}",
                        protocol, leader_pubkey, target_slot
                    );
                    continue;
                }
            };

            results.push(LeaderTpuInfo {
                leader_pubkey,
                slot: target_slot,
                tpu_socket: socket,
                protocol,
            });
        }

        let elapsed = start.elapsed();
        debug!(
            "Resolved {} leader TPU sockets in {:?} (checked {} slots)",
            results.len(),
            elapsed,
            checked_slots
        );

        if results.is_empty() {
            error!(
                "No leader TPU sockets available for next {} slots (protocol: {:?})",
                max_slots_to_check, protocol
            );
            return Err(TriggerError::ConfigError(
                "No leader TPU sockets available".to_string(),
            ));
        }

        Ok(results)
    }

    /// Get TPU sockets for the leapfrog strategy (current + N future leaders)
    ///
    /// # Arguments
    /// * `current_slot` - Current slot number
    /// * `slot_offsets` - Offsets from current slot (e.g., [0, 4, 8] for current, +4, +8)
    /// * `protocol` - TPU protocol to use
    ///
    /// # Returns
    /// Vector of LeaderTpuInfo for each slot offset that has a valid TPU address
    pub async fn get_leapfrog_tpu_sockets(
        &self,
        current_slot: Slot,
        slot_offsets: &[u64],
        protocol: TpuProtocol,
    ) -> Result<Vec<LeaderTpuInfo>> {
        let start = Instant::now();

        self.ensure_caches_fresh().await?;

        let schedule = self.leader_schedule.read().await;
        let nodes = self.cluster_nodes.read().await;

        let mut results = Vec::with_capacity(slot_offsets.len());

        for &offset in slot_offsets {
            let target_slot = current_slot + offset;

            // Get leader for this slot
            let leader_pubkey = match schedule.get(&target_slot) {
                Some(pk) => *pk,
                None => {
                    warn!(
                        "No leader found for leapfrog slot {} (offset +{})",
                        target_slot, offset
                    );
                    continue;
                }
            };

            // Get contact info for this leader
            let node_info = match nodes.get(&leader_pubkey) {
                Some(info) => info,
                None => {
                    warn!(
                        "No contact info for leapfrog leader {} at slot {} (offset +{})",
                        leader_pubkey, target_slot, offset
                    );
                    continue;
                }
            };

            // Get appropriate TPU socket
            let tpu_socket = match protocol {
                TpuProtocol::Udp => node_info.tpu_udp,
                TpuProtocol::Quic => node_info.tpu_quic,
            };

            let socket = match tpu_socket {
                Some(addr) => {
                    if self.config.filter_localhost && Self::is_localhost(&addr) {
                        warn!(
                            "Skipping localhost {} for leapfrog leader {} at slot {}",
                            addr, leader_pubkey, target_slot
                        );
                        continue;
                    }
                    addr
                }
                None => {
                    debug!(
                        "No {:?} TPU for leapfrog leader {} at slot {}",
                        protocol, leader_pubkey, target_slot
                    );
                    continue;
                }
            };

            results.push(LeaderTpuInfo {
                leader_pubkey,
                slot: target_slot,
                tpu_socket: socket,
                protocol,
            });
        }

        let elapsed = start.elapsed();
        info!(
            "Resolved {} leapfrog TPU sockets in {:?} for offsets {:?}",
            results.len(),
            elapsed,
            slot_offsets
        );

        // Log each resolved leader
        for info in &results {
            debug!(
                target: "leapfrog_resolution",
                "→ Leader {} at slot {} → {}",
                info.leader_pubkey, info.slot, info.tpu_socket
            );
        }

        if results.is_empty() {
            warn!("No leapfrog TPU sockets resolved for any offset");
            return Err(TriggerError::ConfigError(
                "No leapfrog TPU sockets available".to_string(),
            ));
        }

        Ok(results)
    }

    /// Get the number of cached leader schedule entries
    pub async fn schedule_size(&self) -> usize {
        self.leader_schedule.read().await.len()
    }

    /// Get the number of cached cluster nodes
    pub async fn cluster_nodes_count(&self) -> usize {
        self.cluster_nodes.read().await.len()
    }

    /// Get the cached epoch (if any)
    pub async fn cached_epoch(&self) -> Option<Epoch> {
        *self.schedule_epoch.read().await
    }

    /// Get configuration
    pub fn config(&self) -> &LeaderTrackerConfig {
        &self.config
    }

    /// Check if metrics are enabled
    pub fn has_metrics(&self) -> bool {
        self.metrics.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leader_tracker_config_default() {
        let config = LeaderTrackerConfig::default();
        assert_eq!(
            config.epoch_check_interval_secs,
            DEFAULT_EPOCH_CHECK_INTERVAL_SECS
        );
        assert_eq!(
            config.cluster_nodes_cache_secs,
            DEFAULT_CLUSTER_NODES_CACHE_SECS
        );
        assert_eq!(config.default_protocol, TpuProtocol::Udp);
        assert!(config.filter_localhost);
    }

    #[test]
    fn test_tpu_protocol_default() {
        let protocol = TpuProtocol::default();
        assert_eq!(protocol, TpuProtocol::Udp);
    }

    #[test]
    fn test_is_localhost_ipv4() {
        let localhost = "127.0.0.1:8001".parse().unwrap();
        assert!(LeaderTracker::is_localhost(&localhost));

        let public = "192.168.1.1:8001".parse().unwrap();
        assert!(!LeaderTracker::is_localhost(&public));

        let another_public = "8.8.8.8:8001".parse().unwrap();
        assert!(!LeaderTracker::is_localhost(&another_public));
    }

    #[test]
    fn test_is_localhost_ipv6() {
        let localhost_v6 = "[::1]:8001".parse().unwrap();
        assert!(LeaderTracker::is_localhost(&localhost_v6));

        let public_v6 = "[2001:db8::1]:8001".parse().unwrap();
        assert!(!LeaderTracker::is_localhost(&public_v6));
    }

    #[test]
    fn test_cluster_node_info_creation() {
        let pubkey = Pubkey::new_unique();
        let tpu_udp = Some("192.168.1.1:8001".parse().unwrap());
        let tpu_quic = Some("192.168.1.1:8007".parse().unwrap());

        let info = ClusterNodeInfo {
            pubkey,
            tpu_udp,
            tpu_quic,
            gossip: None,
        };

        assert_eq!(info.pubkey, pubkey);
        assert!(info.tpu_udp.is_some());
        assert!(info.tpu_quic.is_some());
        assert!(info.gossip.is_none());
    }

    #[test]
    fn test_leader_tpu_info_creation() {
        let pubkey = Pubkey::new_unique();
        let socket = "192.168.1.1:8001".parse().unwrap();

        let info = LeaderTpuInfo {
            leader_pubkey: pubkey,
            slot: 12345,
            tpu_socket: socket,
            protocol: TpuProtocol::Udp,
        };

        assert_eq!(info.leader_pubkey, pubkey);
        assert_eq!(info.slot, 12345);
        assert_eq!(info.protocol, TpuProtocol::Udp);
    }

    #[test]
    fn test_leader_tracker_creation() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = LeaderTracker::new(rpc_client);

        assert_eq!(
            tracker.config.epoch_check_interval_secs,
            DEFAULT_EPOCH_CHECK_INTERVAL_SECS
        );
        assert!(tracker.config.filter_localhost);
    }

    #[test]
    fn test_leader_tracker_with_custom_config() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let config = LeaderTrackerConfig {
            epoch_check_interval_secs: 30,
            cluster_nodes_cache_secs: 600,
            default_protocol: TpuProtocol::Quic,
            filter_localhost: false,
        };

        let tracker = LeaderTracker::with_config(rpc_client, config);

        assert_eq!(tracker.config.epoch_check_interval_secs, 30);
        assert_eq!(tracker.config.cluster_nodes_cache_secs, 600);
        assert_eq!(tracker.config.default_protocol, TpuProtocol::Quic);
        assert!(!tracker.config.filter_localhost);
    }

    #[tokio::test]
    async fn test_schedule_size_initially_empty() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = LeaderTracker::new(rpc_client);

        assert_eq!(tracker.schedule_size().await, 0);
    }

    #[tokio::test]
    async fn test_cluster_nodes_count_initially_empty() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = LeaderTracker::new(rpc_client);

        assert_eq!(tracker.cluster_nodes_count().await, 0);
    }

    #[tokio::test]
    async fn test_cached_epoch_initially_none() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = LeaderTracker::new(rpc_client);

        assert!(tracker.cached_epoch().await.is_none());
    }

    #[test]
    fn test_tpu_quic_port_offset() {
        // Verify the QUIC port offset constant
        assert_eq!(TPU_QUIC_PORT_OFFSET, 6);

        // Test port calculation
        let udp_port: u16 = 8001;
        let expected_quic_port = udp_port + TPU_QUIC_PORT_OFFSET;
        assert_eq!(expected_quic_port, 8007);
    }
}
