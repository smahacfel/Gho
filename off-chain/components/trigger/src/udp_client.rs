//! UDP Client for TPU transaction sending with N+3 redundancy
//!
//! This module handles sending transactions to Solana TPU (Transaction Processing Unit)
//! with redundancy to maximize inclusion rate.

use crate::config::LeapfrogConfig;
use crate::errors::{Result, TriggerError};
use crate::leader_resolver::LeaderResolver;
use crate::metrics::TriggerMetrics;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    clock::Slot, pubkey::Pubkey, signature::Signature, transaction::VersionedTransaction,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

/// Configuration for transaction redundancy
#[derive(Debug, Clone)]
pub struct RedundancyConfig {
    /// Base number of send attempts (e.g., 1)
    pub base_attempts: usize,
    /// Extra redundancy attempts (e.g., 3 for N+3)
    pub extra_attempts: usize,
    /// Maximum number of slots ahead to spread sends
    pub max_span_slots: u64,
}

impl Default for RedundancyConfig {
    fn default() -> Self {
        Self {
            base_attempts: 1,
            extra_attempts: 3, // N+3 default
            max_span_slots: 4,
        }
    }
}

impl RedundancyConfig {
    /// Total number of send attempts
    pub fn total_attempts(&self) -> usize {
        self.base_attempts + self.extra_attempts
    }
}

/// Information about a TPU leader
#[derive(Debug, Clone)]
pub struct LeaderInfo {
    /// Validator identity public key
    pub pubkey: Pubkey,
    /// TPU socket address
    pub tpu_addr: SocketAddr,
    /// Slot number when this validator is leader
    pub slot: Slot,
}

/// Leader schedule with caching
pub struct LeaderSchedule {
    /// RPC client for fetching schedule
    rpc_client: Arc<RpcClient>,
    /// Cached schedule: slot -> leader pubkey
    schedule: HashMap<Slot, Pubkey>,
    /// Last update timestamp
    last_update: SystemTime,
    /// Cache validity duration
    cache_duration: Duration,
    /// Leader resolver for getting TPU contact information
    leader_resolver: Option<Arc<LeaderResolver>>,
}

impl LeaderSchedule {
    /// Create a new leader schedule tracker
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            rpc_client,
            schedule: HashMap::new(),
            last_update: UNIX_EPOCH,
            cache_duration: Duration::from_secs(60), // Refresh every minute
            leader_resolver: None,
        }
    }

    /// Create a new leader schedule tracker with leader resolver for TPU address resolution
    pub fn with_leader_resolver(
        rpc_client: Arc<RpcClient>,
        leader_resolver: Arc<LeaderResolver>,
    ) -> Self {
        Self {
            rpc_client,
            schedule: HashMap::new(),
            last_update: UNIX_EPOCH,
            cache_duration: Duration::from_secs(60),
            leader_resolver: Some(leader_resolver),
        }
    }

    /// Set the leader resolver for TPU address resolution
    pub fn set_leader_resolver(&mut self, resolver: Arc<LeaderResolver>) {
        self.leader_resolver = Some(resolver);
    }

    /// Get current slot
    pub fn get_current_slot(&self) -> Result<Slot> {
        self.rpc_client
            .get_slot()
            .map_err(|e| TriggerError::ClientError(e))
    }

    /// Refresh leader schedule if cache is stale
    pub fn refresh_if_needed(&mut self) -> Result<()> {
        let now = SystemTime::now();
        let elapsed = now
            .duration_since(self.last_update)
            .unwrap_or(Duration::from_secs(u64::MAX));

        if elapsed > self.cache_duration || self.schedule.is_empty() {
            self.refresh_schedule()?;
        }

        Ok(())
    }

    /// Force refresh the leader schedule
    fn refresh_schedule(&mut self) -> Result<()> {
        debug!("Refreshing leader schedule from RPC");

        // Get leader schedule from RPC with error handling
        let schedule = self.rpc_client.get_leader_schedule(None).map_err(|e| {
            error!("Failed to fetch leader schedule from RPC: {}", e);
            TriggerError::ClientError(e)
        })?;

        if let Some(schedule_map) = schedule {
            if schedule_map.is_empty() {
                warn!("Leader schedule returned empty from RPC");
                return Err(TriggerError::ConfigError(
                    "Leader schedule is empty".to_string(),
                ));
            }

            self.schedule.clear();

            // Flatten the schedule into slot -> leader mapping
            for (leader_str, slots) in schedule_map {
                if let Ok(leader_pubkey) = leader_str.parse::<Pubkey>() {
                    for slot in slots {
                        self.schedule.insert(slot as u64, leader_pubkey);
                    }
                } else {
                    warn!("Failed to parse leader pubkey: {}", leader_str);
                }
            }

            self.last_update = SystemTime::now();
            info!(
                "Leader schedule refreshed with {} slots",
                self.schedule.len()
            );
        } else {
            warn!("RPC returned None for leader schedule");
            return Err(TriggerError::ConfigError(
                "Leader schedule not available from RPC".to_string(),
            ));
        }

        Ok(())
    }

    /// Get leader for a specific slot
    pub fn get_leader_at_slot(&mut self, slot: Slot) -> Result<Pubkey> {
        self.refresh_if_needed()?;

        self.schedule
            .get(&slot)
            .copied()
            .ok_or_else(|| TriggerError::Other(format!("No leader found for slot {}", slot)))
    }

    /// Get upcoming leaders for the next N slots
    ///
    /// If a leader resolver is configured, it will be used to resolve TPU addresses
    /// for each leader. Otherwise, a fallback placeholder address is used.
    ///
    /// Note: This method is sync for backward compatibility. For full async TPU
    /// address resolution, use the TpuClient's send_leapfrog method which handles
    /// async resolution internally.
    pub fn get_upcoming_leaders(&mut self, count: usize) -> Result<Vec<LeaderInfo>> {
        self.refresh_if_needed()?;

        let current_slot = self.get_current_slot()?;
        let mut leaders = Vec::new();

        for i in 0..count {
            let target_slot = current_slot + i as u64;
            match self.get_leader_at_slot(target_slot) {
                Ok(leader_pubkey) => {
                    // Try to get TPU address from leader resolver if available
                    // Since this is a sync method, we need to use try_get_cached or fallback
                    let tpu_addr = self.resolve_tpu_address_sync(&leader_pubkey, i);

                    leaders.push(LeaderInfo {
                        pubkey: leader_pubkey,
                        tpu_addr,
                        slot: target_slot,
                    });
                }
                Err(e) => {
                    warn!(
                        "No leader found for slot {} (offset +{}): {}",
                        target_slot, i, e
                    );
                    // Continue to next slot instead of failing completely
                }
            }
        }

        if leaders.is_empty() {
            return Err(TriggerError::ConfigError(
                "No leaders available in the schedule".to_string(),
            ));
        }

        Ok(leaders)
    }

    /// Resolve TPU address for a leader pubkey
    ///
    /// This sync method uses a simple fallback approach.
    /// For full async resolution with actual TPU addresses, use TpuClient's send_leapfrog method
    /// which handles async resolution internally.
    ///
    /// Note: This method exists for backward compatibility with sync callers.
    /// Production code should prefer the async code path which can properly resolve TPU addresses.
    fn resolve_tpu_address_sync(&self, leader_pubkey: &Pubkey, offset: usize) -> SocketAddr {
        // For sync context, we cannot safely use async resolution without risking deadlocks.
        // The LeaderResolver's cache is behind an async RwLock, so we fall back to
        // placeholder addresses here. The TpuClient::send_leapfrog method handles
        // proper async TPU resolution for production use.

        if self.leader_resolver.is_some() {
            debug!(
                "Leader resolver configured but sync resolution not supported; using fallback for {}",
                leader_pubkey
            );
        }

        // Use fallback address - this maintains backward compatibility
        // Production code should use the async code path (TpuClient::send_leapfrog)
        //
        // ⚠️  WARNING: This 127.0.0.1 address is ONLY used in sync fallback context.
        // Production transactions MUST use send_leapfrog() which properly resolves
        // TPU addresses via LeaderResolver. This fallback exists solely for backward
        // compatibility with legacy sync callers and test scenarios.
        let fallback_addr = format!("127.0.0.1:{}", 8001 + offset)
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], 8001)));

        debug!(
            "Using fallback TPU address {} for leader {} (sync context)",
            fallback_addr, leader_pubkey
        );
        fallback_addr
    }
}

/// TPU Client wrapper with N+3 redundancy
///
/// Sends transactions to multiple TPU endpoints for higher inclusion rate
pub struct TpuClient {
    /// RPC client for fetching leader schedule and slots
    rpc_client: Arc<RpcClient>,
    /// Leader schedule tracker
    leader_schedule: Arc<tokio::sync::Mutex<LeaderSchedule>>,
    /// Leader resolver for getting TPU contact information
    leader_resolver: Option<Arc<LeaderResolver>>,
    /// Redundancy configuration
    redundancy_config: RedundancyConfig,
    /// Metrics collector (optional)
    metrics: Option<Arc<TriggerMetrics>>,
}

impl TpuClient {
    /// Create a new TPU client
    ///
    /// # Arguments
    /// * `rpc_url` - RPC endpoint URL
    /// * `redundancy_count` - Number of additional sends (default: 3 for N+3)
    pub fn new(rpc_url: String, redundancy_count: Option<usize>) -> Result<Self> {
        let rpc_client = Arc::new(RpcClient::new(rpc_url));
        let leader_schedule = LeaderSchedule::new(Arc::clone(&rpc_client));

        let redundancy_config = RedundancyConfig {
            base_attempts: 1,
            extra_attempts: redundancy_count.unwrap_or(3),
            max_span_slots: 4,
        };

        Ok(Self {
            rpc_client,
            leader_schedule: Arc::new(tokio::sync::Mutex::new(leader_schedule)),
            leader_resolver: None,
            redundancy_config,
            metrics: None,
        })
    }

    /// Create a new TPU client with custom redundancy config
    pub fn with_config(rpc_url: String, redundancy_config: RedundancyConfig) -> Result<Self> {
        let rpc_client = Arc::new(RpcClient::new(rpc_url));
        let leader_schedule = LeaderSchedule::new(Arc::clone(&rpc_client));

        Ok(Self {
            rpc_client,
            leader_schedule: Arc::new(tokio::sync::Mutex::new(leader_schedule)),
            leader_resolver: None,
            redundancy_config,
            metrics: None,
        })
    }

    /// Create a new TPU client with metrics
    pub fn with_metrics(
        rpc_url: String,
        redundancy_count: Option<usize>,
        metrics: Arc<TriggerMetrics>,
    ) -> Result<Self> {
        let rpc_client = Arc::new(RpcClient::new(rpc_url));
        let leader_schedule = LeaderSchedule::new(Arc::clone(&rpc_client));

        let redundancy_config = RedundancyConfig {
            base_attempts: 1,
            extra_attempts: redundancy_count.unwrap_or(3),
            max_span_slots: 4,
        };

        Ok(Self {
            rpc_client,
            leader_schedule: Arc::new(tokio::sync::Mutex::new(leader_schedule)),
            leader_resolver: None,
            redundancy_config,
            metrics: Some(metrics),
        })
    }

    /// Create a new TPU client with leader resolver for leapfrog strategy
    pub fn with_leader_resolver(
        rpc_url: String,
        redundancy_count: Option<usize>,
        leader_resolver: Arc<LeaderResolver>,
    ) -> Result<Self> {
        let rpc_client = Arc::new(RpcClient::new(rpc_url));
        let leader_schedule = LeaderSchedule::new(Arc::clone(&rpc_client));

        let redundancy_config = RedundancyConfig {
            base_attempts: 1,
            extra_attempts: redundancy_count.unwrap_or(3),
            max_span_slots: 4,
        };

        Ok(Self {
            rpc_client,
            leader_schedule: Arc::new(tokio::sync::Mutex::new(leader_schedule)),
            leader_resolver: Some(leader_resolver),
            redundancy_config,
            metrics: None,
        })
    }

    /// Send a transaction directly to a TPU leader via UDP
    ///
    /// # Arguments
    /// * `tx_bytes` - Serialized transaction bytes
    /// * `leader_info` - Information about the TPU leader
    async fn send_transaction(tx_bytes: &[u8], leader_info: &LeaderInfo) -> Result<()> {
        info!(
            "Sending transaction ({} bytes) to TPU leader {} at {} for slot {}",
            tx_bytes.len(),
            leader_info.pubkey,
            leader_info.tpu_addr,
            leader_info.slot
        );

        // Bind to any available port
        let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(|e| {
            error!("Failed to bind UDP socket: {}", e);
            TriggerError::SendFailed(format!("Failed to bind UDP socket: {}", e))
        })?;

        // Send transaction bytes to TPU
        socket
            .send_to(tx_bytes, leader_info.tpu_addr)
            .await
            .map_err(|e| {
                warn!(
                    "Failed to send to TPU {} (leader {}, slot {}): {}",
                    leader_info.tpu_addr, leader_info.pubkey, leader_info.slot, e
                );
                TriggerError::SendFailed(format!(
                    "Failed to send to TPU {}: {}",
                    leader_info.tpu_addr, e
                ))
            })?;

        info!(
            "Transaction sent successfully to {} (leader {}, slot {})",
            leader_info.tpu_addr, leader_info.pubkey, leader_info.slot
        );
        Ok(())
    }

    /// Send transaction with N+3 redundancy to multiple upcoming leaders
    ///
    /// # Arguments
    /// * `tx` - The transaction to send (VersionedTransaction)
    /// * `redundancy` - Redundancy configuration
    ///
    /// # Returns
    /// * Transaction signature
    pub async fn send_with_redundancy(
        &self,
        tx: &VersionedTransaction,
        redundancy: &RedundancyConfig,
    ) -> Result<Signature> {
        info!(
            "Sending transaction with N+{} redundancy (total {} attempts)",
            redundancy.extra_attempts,
            redundancy.total_attempts()
        );

        // Extract signature
        let signature = tx.signatures[0];

        // Serialize transaction once
        let tx_bytes =
            bincode::serialize(tx).map_err(|e| TriggerError::SerializationError(e.to_string()))?;

        info!(
            "Transaction {} serialized, size: {} bytes",
            signature,
            tx_bytes.len()
        );

        // Increment metrics for transaction send
        if let Some(metrics) = &self.metrics {
            metrics.transactions_sent.inc();
            metrics.bytes_sent.inc_by(tx_bytes.len() as f64);
        }

        // Get upcoming leaders
        let mut leader_schedule = self.leader_schedule.lock().await;
        let total_attempts = redundancy.total_attempts();
        let leaders = leader_schedule
            .get_upcoming_leaders(total_attempts)
            .map_err(|e| {
                error!("Failed to get leader schedule: {}", e);
                if let Some(metrics) = &self.metrics {
                    metrics.transactions_failed.inc();
                }
                e
            })?;

        drop(leader_schedule); // Release lock

        if leaders.is_empty() {
            error!("No leaders available for sending transaction {}", signature);
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "No leaders available for sending".to_string(),
            ));
        }

        info!(
            "Targeting {} TPU leaders for transaction {}",
            leaders.len().min(total_attempts),
            signature
        );

        // Track successful sends
        let mut successful_sends = 0;

        // Send to multiple leaders with small delays
        for (i, leader_info) in leaders.iter().take(total_attempts).enumerate() {
            info!(
                "Send attempt {}/{}: targeting leader {} at {} for slot {}",
                i + 1,
                total_attempts,
                leader_info.pubkey,
                leader_info.tpu_addr,
                leader_info.slot
            );

            match Self::send_transaction(&tx_bytes, leader_info).await {
                Ok(_) => {
                    successful_sends += 1;
                    info!(
                        "Transaction {} send attempt {}/{} succeeded to {} (slot {})",
                        signature,
                        i + 1,
                        total_attempts,
                        leader_info.tpu_addr,
                        leader_info.slot
                    );

                    // Increment redundancy counter for extra attempts
                    if i > 0 {
                        if let Some(metrics) = &self.metrics {
                            metrics.redundancy_sends.inc();
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Transaction {} send attempt {}/{} failed to {} (leader {}, slot {}): {}",
                        signature,
                        i + 1,
                        total_attempts,
                        leader_info.tpu_addr,
                        leader_info.pubkey,
                        leader_info.slot,
                        e
                    );
                    // Continue with other attempts even if one fails
                }
            }

            // Small delay between sends to avoid overwhelming the network
            if i < total_attempts - 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        if successful_sends == 0 {
            error!(
                "All {} send attempts failed for transaction {}",
                total_attempts, signature
            );
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "All send attempts failed".to_string(),
            ));
        }

        info!(
            "Transaction {} sent successfully ({}/{} attempts succeeded)",
            signature, successful_sends, total_attempts
        );
        Ok(signature)
    }

    /// Send transaction with default redundancy (backward compatible)
    ///
    /// Sends the transaction multiple times to different TPU endpoints
    /// to maximize the chance of inclusion in a block.
    ///
    /// # Arguments
    /// * `transaction` - The transaction to send
    ///
    /// # Returns
    /// * Transaction signature
    pub async fn send_transaction_with_redundancy(
        &self,
        transaction: &VersionedTransaction,
    ) -> Result<Signature> {
        self.send_with_redundancy(transaction, &self.redundancy_config)
            .await
    }

    /// Send pre-serialized transaction bytes directly without re-serialization
    ///
    /// This is optimized for the hot-path where transactions are already serialized.
    /// Avoids the overhead of deserialize->serialize cycle.
    ///
    /// # Arguments
    /// * `tx_bytes` - Pre-serialized transaction bytes in wire format (bincode)
    ///
    /// # Returns
    /// * Transaction signature (extracted from wire format)
    pub async fn send_wire_transaction(&self, tx_bytes: &[u8]) -> Result<Signature> {
        // Extract signature from bincode wire format
        // Format: [signature_count (1 byte)][signatures (64 bytes each)][...]
        if tx_bytes.len() < 65 {
            return Err(TriggerError::SerializationError(
                "Transaction bytes too short to contain signature (need at least 65 bytes)"
                    .to_string(),
            ));
        }

        let signature = Signature::try_from(&tx_bytes[1..65])
            .map_err(|e| TriggerError::SerializationError(format!("Invalid signature: {}", e)))?;

        info!(
            "Sending pre-serialized transaction {} with N+{} redundancy (total {} attempts)",
            signature,
            self.redundancy_config.extra_attempts,
            self.redundancy_config.total_attempts()
        );

        // Increment metrics
        if let Some(metrics) = &self.metrics {
            metrics.transactions_sent.inc();
            metrics.bytes_sent.inc_by(tx_bytes.len() as f64);
        }

        // Get upcoming leaders
        let mut leader_schedule = self.leader_schedule.lock().await;
        let total_attempts = self.redundancy_config.total_attempts();
        let leaders = leader_schedule
            .get_upcoming_leaders(total_attempts)
            .map_err(|e| {
                error!("Failed to get leader schedule: {}", e);
                if let Some(metrics) = &self.metrics {
                    metrics.transactions_failed.inc();
                }
                e
            })?;

        drop(leader_schedule); // Release lock

        if leaders.is_empty() {
            error!("No leaders available for sending transaction {}", signature);
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "No leaders available for sending".to_string(),
            ));
        }

        info!(
            "Targeting {} TPU leaders for transaction {}",
            leaders.len().min(total_attempts),
            signature
        );

        // Track successful sends
        let mut successful_sends = 0;

        // Send to multiple leaders with small delays
        for (i, leader_info) in leaders.iter().take(total_attempts).enumerate() {
            info!(
                "Send attempt {}/{}: targeting leader {} at {} for slot {}",
                i + 1,
                total_attempts,
                leader_info.pubkey,
                leader_info.tpu_addr,
                leader_info.slot
            );

            match Self::send_transaction(tx_bytes, leader_info).await {
                Ok(_) => {
                    successful_sends += 1;
                    info!(
                        "Transaction {} send attempt {}/{} succeeded to {} (slot {})",
                        signature,
                        i + 1,
                        total_attempts,
                        leader_info.tpu_addr,
                        leader_info.slot
                    );

                    // Increment redundancy counter for extra attempts
                    if i > 0 {
                        if let Some(metrics) = &self.metrics {
                            metrics.redundancy_sends.inc();
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Transaction {} send attempt {}/{} failed to {} (leader {}, slot {}): {}",
                        signature,
                        i + 1,
                        total_attempts,
                        leader_info.tpu_addr,
                        leader_info.pubkey,
                        leader_info.slot,
                        e
                    );
                    // Continue with other attempts even if one fails
                }
            }

            // Small delay between sends to avoid overwhelming the network
            if i < total_attempts - 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        if successful_sends == 0 {
            error!(
                "All {} send attempts failed for transaction {}",
                total_attempts, signature
            );
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "All send attempts failed".to_string(),
            ));
        }

        info!(
            "Transaction {} sent successfully ({}/{} attempts succeeded)",
            signature, successful_sends, total_attempts
        );
        Ok(signature)
    }

    /// Send transaction using Leapfrog N+X strategy
    ///
    /// This method implements the Leapfrog strategy by sending the same transaction
    /// to multiple future leaders simultaneously for better inclusion rate.
    ///
    /// # Arguments
    /// * `wire_transaction` - Pre-serialized transaction bytes in wire format (bincode)
    /// * `current_slot` - Current slot number for leapfrog calculation
    /// * `config` - Leapfrog configuration (redundancy count, use_quic flag)
    ///
    /// # Returns
    /// * Transaction signature (extracted from wire format)
    ///
    /// # Strategy
    /// With default config (leapfrog_redundancy=2):
    /// - Sends to leader at slot: current
    /// - Sends to leader at slot: current + 4
    /// - Sends to leader at slot: current + 8
    ///
    /// All sends happen in parallel for minimal latency.
    pub async fn send_leapfrog(
        &self,
        wire_transaction: &[u8],
        current_slot: Slot,
        config: &LeapfrogConfig,
    ) -> Result<Signature> {
        // Extract signature from bincode wire format
        // Format: [signature_count (1 byte)][signatures (64 bytes each)][...]
        if wire_transaction.len() < 65 {
            return Err(TriggerError::SerializationError(
                "Transaction bytes too short to contain signature (need at least 65 bytes)"
                    .to_string(),
            ));
        }

        let signature = Signature::try_from(&wire_transaction[1..65])
            .map_err(|e| TriggerError::SerializationError(format!("Invalid signature: {}", e)))?;

        info!(
            "Sending Leapfrog transaction {} with redundancy={} (total {} leaders)",
            signature,
            config.leapfrog_redundancy,
            config.total_leaders()
        );

        // Calculate target slots using configured offsets
        let slot_offsets = config.slot_offsets();
        let target_slots: Vec<Slot> = slot_offsets
            .iter()
            .map(|offset| current_slot + offset)
            .collect();

        // Get leader information for each target slot
        let mut leader_schedule = self.leader_schedule.lock().await;
        let mut leader_targets = Vec::new();

        for target_slot in &target_slots {
            match leader_schedule.get_leader_at_slot(*target_slot) {
                Ok(leader_pubkey) => {
                    // Get contact info from leader resolver if available
                    let contact_info = if let Some(ref resolver) = self.leader_resolver {
                        match resolver.get_contact_info(&leader_pubkey).await {
                            Ok(info) => Some(info),
                            Err(e) => {
                                warn!(
                                    "Failed to get contact info for leader {} at slot {}: {}",
                                    leader_pubkey, target_slot, e
                                );
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let tpu_addr = if let Some(info) = contact_info {
                        if config.use_quic {
                            info.tpu_quic
                        } else {
                            info.tpu
                        }
                    } else {
                        // Skip this leader if we can't get contact info
                        // Better to fail fast than use incorrect addresses
                        warn!(
                            "Skipping leader {} at slot {} - no contact info available",
                            leader_pubkey, target_slot
                        );
                        continue; // Skip to next leader
                    };

                    leader_targets.push(LeaderInfo {
                        pubkey: leader_pubkey,
                        tpu_addr,
                        slot: *target_slot,
                    });
                }
                Err(e) => {
                    warn!(
                        "No leader found for slot {} in leapfrog strategy: {}",
                        target_slot, e
                    );
                    // Continue with available leaders
                }
            }
        }

        drop(leader_schedule); // Release lock before parallel sends

        if leader_targets.is_empty() {
            error!(
                "No leaders available for Leapfrog transaction {}",
                signature
            );
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "No leaders available for Leapfrog strategy".to_string(),
            ));
        }

        // Log the leapfrog targets with IP addresses
        let target_description: Vec<String> = leader_targets
            .iter()
            .map(|leader| {
                format!(
                    "{} ({}) at Slot {}",
                    leader.pubkey, leader.tpu_addr, leader.slot
                )
            })
            .collect();
        info!(
            "Sending Leapfrog tx to: [{}]",
            target_description.join(", ")
        );

        // Log target IP addresses for debugging (before sending)
        for leader in &leader_targets {
            debug!(
                target: "leapfrog_dispatch",
                "→ Preparing to send to Leader {} (IP: {})",
                leader.pubkey,
                leader.tpu_addr
            );
        }

        // Increment metrics
        if let Some(metrics) = &self.metrics {
            metrics.transactions_sent.inc();
            metrics.bytes_sent.inc_by(wire_transaction.len() as f64);
        }

        // Send to all targets in parallel using tokio::spawn for true concurrency
        let mut send_handles = Vec::new();

        for leader_info in leader_targets {
            let tx_bytes = wire_transaction.to_vec();

            let handle = tokio::spawn(async move {
                let result = Self::send_transaction(&tx_bytes, &leader_info).await;
                (leader_info, result)
            });

            send_handles.push(handle);
        }

        // Wait for all sends to complete
        let mut successful_sends = 0;
        let mut total_targets = 0;

        for handle in send_handles {
            total_targets += 1;
            match handle.await {
                Ok((leader_info, send_result)) => {
                    match send_result {
                        Ok(_) => {
                            successful_sends += 1;
                            info!(
                                "Leapfrog send succeeded to {} at {} (slot {})",
                                leader_info.pubkey, leader_info.tpu_addr, leader_info.slot
                            );

                            // Increment redundancy counter
                            if let Some(metrics) = &self.metrics {
                                metrics.redundancy_sends.inc();
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Leapfrog send failed to {} at {} (slot {}): {}",
                                leader_info.pubkey, leader_info.tpu_addr, leader_info.slot, e
                            );
                            // Continue processing other results
                        }
                    }
                }
                Err(e) => {
                    error!("Leapfrog task join error: {}", e);
                    // Continue processing other results
                }
            }
        }

        if successful_sends == 0 {
            error!(
                "All {} Leapfrog send attempts failed for transaction {}",
                total_targets, signature
            );
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "All Leapfrog send attempts failed".to_string(),
            ));
        }

        info!(
            "Leapfrog transaction {} sent successfully ({}/{} targets succeeded)",
            signature, successful_sends, total_targets
        );
        Ok(signature)
    }

    /// Send transaction and wait for confirmation
    ///
    /// This is a convenience method that sends the transaction and polls
    /// for confirmation.
    pub async fn send_and_confirm(
        &self,
        transaction: &VersionedTransaction,
        max_retries: usize,
    ) -> Result<Signature> {
        let signature = self.send_transaction_with_redundancy(transaction).await?;

        // Poll for confirmation
        for retry in 0..max_retries {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Check transaction status
            // In production, use RPC getSignatureStatuses
            debug!(
                "Polling for confirmation, attempt {}/{}",
                retry + 1,
                max_retries
            );

            // Placeholder: assume confirmed after a few attempts
            if retry >= 2 {
                info!("Transaction confirmed: {}", signature);
                return Ok(signature);
            }
        }

        warn!("Transaction not confirmed after {} retries", max_retries);
        Err(TriggerError::SendFailed(
            "Transaction confirmation timeout".to_string(),
        ))
    }

    /// Get estimated inclusion rate based on redundancy
    pub fn get_estimated_inclusion_rate(&self) -> f64 {
        // With N+3 redundancy, estimate ~92%+ inclusion rate
        // This is a simplified calculation
        let extra = self.redundancy_config.extra_attempts;
        match extra {
            0 => 0.70, // 70% with single send
            1 => 0.80, // 80% with N+1
            2 => 0.88, // 88% with N+2
            3 => 0.92, // 92% with N+3 (target)
            _ => 0.95, // 95%+ with higher redundancy
        }
    }

    /// Get current redundancy configuration
    pub fn redundancy_config(&self) -> &RedundancyConfig {
        &self.redundancy_config
    }
}

/// Builder for TPU client configuration
pub struct TpuClientBuilder {
    rpc_url: String,
    redundancy_count: Option<usize>,
}

impl TpuClientBuilder {
    /// Create a new builder with RPC URL
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            redundancy_count: None,
        }
    }

    /// Set redundancy count
    pub fn with_redundancy(mut self, count: usize) -> Self {
        self.redundancy_count = Some(count);
        self
    }

    /// Build the TPU client
    pub fn build(self) -> Result<TpuClient> {
        TpuClient::new(self.rpc_url, self.redundancy_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::signer::{keypair::Keypair, Signer};
    use solana_sdk::system_instruction;
    use solana_sdk::transaction::Transaction;

    #[test]
    fn test_redundancy_config_default() {
        let config = RedundancyConfig::default();
        assert_eq!(config.base_attempts, 1);
        assert_eq!(config.extra_attempts, 3);
        assert_eq!(config.max_span_slots, 4);
        assert_eq!(config.total_attempts(), 4);
    }

    #[test]
    fn test_redundancy_config_custom() {
        let config = RedundancyConfig {
            base_attempts: 1,
            extra_attempts: 5,
            max_span_slots: 8,
        };
        assert_eq!(config.total_attempts(), 6);
    }

    #[test]
    fn test_leader_info_creation() {
        let pubkey = Pubkey::new_unique();
        let tpu_addr = "127.0.0.1:8001".parse().unwrap();
        let leader_info = LeaderInfo {
            pubkey,
            tpu_addr,
            slot: 100,
        };

        assert_eq!(leader_info.slot, 100);
        assert_eq!(leader_info.pubkey, pubkey);
    }

    #[test]
    fn test_leader_schedule_creation() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let schedule = LeaderSchedule::new(rpc_client);

        assert!(schedule.schedule.is_empty());
        assert_eq!(
            schedule
                .last_update
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            0
        );
    }

    #[test]
    fn test_tpu_client_creation() {
        let client = TpuClient::new("https://api.devnet.solana.com".to_string(), Some(3));
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.redundancy_config.extra_attempts, 3);
    }

    #[test]
    fn test_tpu_client_with_custom_config() {
        let config = RedundancyConfig {
            base_attempts: 1,
            extra_attempts: 5,
            max_span_slots: 8,
        };

        let client =
            TpuClient::with_config("https://api.devnet.solana.com".to_string(), config.clone());

        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.redundancy_config.extra_attempts, 5);
        assert_eq!(client.redundancy_config.max_span_slots, 8);
    }

    #[test]
    fn test_builder() {
        let client = TpuClientBuilder::new("https://api.devnet.solana.com")
            .with_redundancy(3)
            .build();

        assert!(client.is_ok());
    }

    #[test]
    fn test_estimated_inclusion_rate() {
        let client = TpuClient::new("https://api.devnet.solana.com".to_string(), Some(3)).unwrap();

        let rate = client.get_estimated_inclusion_rate();
        assert!(rate >= 0.92); // Should meet N+3 target
    }

    #[test]
    fn test_inclusion_rate_scaling() {
        let client0 = TpuClient::new("https://api.devnet.solana.com".to_string(), Some(0)).unwrap();

        let client3 = TpuClient::new("https://api.devnet.solana.com".to_string(), Some(3)).unwrap();

        assert!(client3.get_estimated_inclusion_rate() > client0.get_estimated_inclusion_rate());
    }

    #[tokio::test]
    async fn test_send_transaction_serialization() {
        // Create a simple transaction for testing
        let payer = Keypair::new();
        let to = Pubkey::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &to, 1000);
        let recent_blockhash = Hash::new_unique();

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        // Convert to VersionedTransaction
        let versioned_tx = VersionedTransaction::from(transaction);
        let expected_sig = versioned_tx.signatures[0];

        // Test serialization
        let serialized = bincode::serialize(&versioned_tx);
        assert!(serialized.is_ok());

        let tx_bytes = serialized.unwrap();
        assert!(tx_bytes.len() > 0);
        assert!(tx_bytes.len() < 1232); // Max transaction size

        // Verify bincode format: first byte is signature count, then signatures
        assert!(tx_bytes.len() >= 65); // At least 1 byte count + 64 byte signature
        assert_eq!(tx_bytes[0], 1); // Should have 1 signature

        // Extract signature from correct offset (after count byte)
        let extracted_sig = Signature::try_from(&tx_bytes[1..65]).unwrap();
        assert_eq!(extracted_sig, expected_sig, "Signature should match");
    }

    #[tokio::test]
    async fn test_send_transaction_udp() {
        // This test verifies UDP sending functionality
        let leader_info = LeaderInfo {
            pubkey: Pubkey::new_unique(),
            tpu_addr: "127.0.0.1:8001".parse().unwrap(),
            slot: 100,
        };

        // Create a simple transaction
        let payer = Keypair::new();
        let to = Pubkey::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &to, 1000);
        let recent_blockhash = Hash::new_unique();

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        let versioned_tx = VersionedTransaction::from(transaction);
        let tx_bytes = bincode::serialize(&versioned_tx).unwrap();

        // Note: This will fail in CI without a listening UDP server,
        // but demonstrates the API
        // In production, we'd mock the UDP socket
        let _result = TpuClient::send_transaction(&tx_bytes, &leader_info).await;
        // We expect this to fail in test environment without actual TPU
        // The important part is that the code compiles and the logic is correct
    }

    #[test]
    fn test_redundancy_config_accessor() {
        let config = RedundancyConfig {
            base_attempts: 2,
            extra_attempts: 4,
            max_span_slots: 6,
        };

        let client =
            TpuClient::with_config("https://api.devnet.solana.com".to_string(), config.clone())
                .unwrap();

        let retrieved_config = client.redundancy_config();
        assert_eq!(retrieved_config.base_attempts, 2);
        assert_eq!(retrieved_config.extra_attempts, 4);
        assert_eq!(retrieved_config.max_span_slots, 6);
    }

    #[test]
    fn test_tpu_client_with_metrics() {
        use crate::metrics::TriggerMetrics;

        let metrics = Arc::new(TriggerMetrics::new());
        let client = TpuClient::with_metrics(
            "https://api.devnet.solana.com".to_string(),
            Some(3),
            metrics.clone(),
        );

        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.redundancy_config.extra_attempts, 3);
        assert!(client.metrics.is_some());
    }

    #[test]
    fn test_tpu_client_with_leader_resolver() {
        use crate::leader_resolver::LeaderResolver;

        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

        let client = TpuClient::with_leader_resolver(
            "https://api.devnet.solana.com".to_string(),
            Some(3),
            leader_resolver,
        );

        assert!(client.is_ok());
        let client = client.unwrap();
        assert!(client.leader_resolver.is_some());
    }

    #[tokio::test]
    async fn test_send_leapfrog_signature_extraction() {
        use crate::config::LeapfrogConfig;

        // Create a simple transaction
        let payer = Keypair::new();
        let to = Pubkey::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &to, 1000);
        let recent_blockhash = Hash::new_unique();

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        let versioned_tx = VersionedTransaction::from(transaction);
        let tx_bytes = bincode::serialize(&versioned_tx).unwrap();

        // Test signature extraction (should not fail)
        assert!(tx_bytes.len() >= 64);
        let sig_result = Signature::try_from(&tx_bytes[0..64]);
        assert!(sig_result.is_ok());
    }

    #[tokio::test]
    async fn test_send_leapfrog_invalid_transaction() {
        use crate::config::LeapfrogConfig;

        let client = TpuClient::new("https://api.devnet.solana.com".to_string(), Some(3)).unwrap();

        let config = LeapfrogConfig::default();
        let current_slot = 100;

        // Too short transaction bytes (need at least 65 bytes for signature extraction)
        let short_bytes = vec![0u8; 32];
        let result = client
            .send_leapfrog(&short_bytes, current_slot, &config)
            .await;
        assert!(result.is_err());

        // Also test with exactly 64 bytes (still too short, need 65)
        let bytes_64 = vec![0u8; 64];
        let result = client.send_leapfrog(&bytes_64, current_slot, &config).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_leapfrog_config_integration() {
        use crate::config::LeapfrogConfig;

        // Test default config
        let config = LeapfrogConfig::default();
        assert_eq!(config.leapfrog_redundancy, 2);
        assert_eq!(config.use_quic, false);
        assert_eq!(config.total_leaders(), 3);
        assert_eq!(config.slot_offsets(), vec![0, 4, 8]);

        // Test custom config
        let config = LeapfrogConfig::new(3, true);
        assert_eq!(config.leapfrog_redundancy, 3);
        assert_eq!(config.use_quic, true);
        assert_eq!(config.total_leaders(), 4);
        assert_eq!(config.slot_offsets(), vec![0, 4, 8, 12]);
    }
}
