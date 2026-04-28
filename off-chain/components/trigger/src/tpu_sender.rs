//! TPU Sender - Dynamic TPU Transaction Sending with Leapfrog Strategy
//!
//! This module integrates the LeaderTracker with transaction sending,
//! replacing static TPU lists with dynamic leader resolution. It implements
//! the "leapfrog" fan-out strategy for higher transaction inclusion rates.
//!
//! # Features
//!
//! - Dynamic leader TPU resolution (no more hardcoded addresses)
//! - Leapfrog fan-out strategy (current leader + N future leaders)
//! - Support for both UDP and QUIC protocols
//! - Localhost address filtering
//! - Metrics integration for monitoring
//!
//! # Usage
//!
//! ```ignore
//! use trigger::{TpuSender, TpuSenderConfig, LeaderTracker};
//!
//! let tracker = LeaderTracker::new(rpc_client.clone());
//! let sender = TpuSender::new(tracker, TpuSenderConfig::default());
//!
//! // Send with leapfrog strategy
//! let result = sender.send_leapfrog(&wire_transaction, current_slot).await?;
//! ```

use crate::config::LeapfrogConfig;
use crate::errors::{Result, TriggerError};
use crate::leader_tracker::{LeaderTpuInfo, LeaderTracker, TpuProtocol};
use crate::metrics::TriggerMetrics;
use solana_sdk::{clock::Slot, signature::Signature};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

/// Configuration for the TPU sender
#[derive(Debug, Clone)]
pub struct TpuSenderConfig {
    /// Leapfrog configuration (redundancy count, slot offsets)
    pub leapfrog_config: LeapfrogConfig,
    /// Delay between sends to different leaders (milliseconds)
    pub inter_send_delay_ms: u64,
    /// Whether to send in parallel vs sequential
    pub parallel_sends: bool,
    /// Timeout for UDP sends (milliseconds)
    pub send_timeout_ms: u64,
}

impl Default for TpuSenderConfig {
    fn default() -> Self {
        Self {
            leapfrog_config: LeapfrogConfig::default(),
            inter_send_delay_ms: 10,
            parallel_sends: true,
            send_timeout_ms: 500,
        }
    }
}

impl TpuSenderConfig {
    /// Create a new config with custom leapfrog redundancy
    pub fn with_redundancy(redundancy: usize) -> Self {
        Self {
            leapfrog_config: LeapfrogConfig::new(redundancy, false),
            ..Default::default()
        }
    }

    /// Create a new config with QUIC protocol
    pub fn with_quic(redundancy: usize) -> Self {
        Self {
            leapfrog_config: LeapfrogConfig::new(redundancy, true),
            ..Default::default()
        }
    }
}

/// Result of a TPU send operation
#[derive(Debug, Clone)]
pub struct SendResult {
    /// Transaction signature
    pub signature: Signature,
    /// Number of successful sends
    pub successful_sends: usize,
    /// Total number of attempted sends
    pub total_attempts: usize,
    /// Leaders that were targeted
    pub targeted_leaders: Vec<LeaderTpuInfo>,
    /// Total time for all sends
    pub total_duration: Duration,
}

impl SendResult {
    /// Check if at least one send succeeded
    pub fn is_success(&self) -> bool {
        self.successful_sends > 0
    }

    /// Get the success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_attempts == 0 {
            0.0
        } else {
            (self.successful_sends as f64 / self.total_attempts as f64) * 100.0
        }
    }
}

/// TPU Sender with dynamic leader resolution and leapfrog strategy
pub struct TpuSender {
    /// Leader tracker for resolving TPU addresses
    leader_tracker: Arc<LeaderTracker>,
    /// Configuration
    config: TpuSenderConfig,
    /// Optional metrics collector
    metrics: Option<Arc<TriggerMetrics>>,
}

impl TpuSender {
    /// Create a new TPU sender with default configuration
    pub fn new(leader_tracker: Arc<LeaderTracker>) -> Self {
        Self::with_config(leader_tracker, TpuSenderConfig::default())
    }

    /// Create a new TPU sender with custom configuration
    pub fn with_config(leader_tracker: Arc<LeaderTracker>, config: TpuSenderConfig) -> Self {
        Self {
            leader_tracker,
            config,
            metrics: None,
        }
    }

    /// Create a new TPU sender with metrics
    pub fn with_metrics(
        leader_tracker: Arc<LeaderTracker>,
        config: TpuSenderConfig,
        metrics: Arc<TriggerMetrics>,
    ) -> Self {
        Self {
            leader_tracker,
            config,
            metrics: Some(metrics),
        }
    }

    /// Send a transaction using the leapfrog strategy
    ///
    /// # Arguments
    /// * `wire_transaction` - Pre-serialized transaction bytes
    /// * `current_slot` - Current slot number for calculating leapfrog targets
    ///
    /// # Returns
    /// SendResult with details about the send operation
    pub async fn send_leapfrog(
        &self,
        wire_transaction: &[u8],
        current_slot: Slot,
    ) -> Result<SendResult> {
        let start = Instant::now();

        // Extract signature from wire format
        let signature = Self::extract_signature(wire_transaction)?;

        // Get slot offsets from config
        let slot_offsets = self.config.leapfrog_config.slot_offsets();
        let protocol = if self.config.leapfrog_config.use_quic {
            TpuProtocol::Quic
        } else {
            TpuProtocol::Udp
        };

        info!(
            "Sending leapfrog transaction {} with {} targets (protocol: {:?})",
            signature,
            slot_offsets.len(),
            protocol
        );

        // Resolve TPU addresses for leapfrog slots
        let leaders = self
            .leader_tracker
            .get_leapfrog_tpu_sockets(current_slot, &slot_offsets, protocol)
            .await?;

        if leaders.is_empty() {
            error!(
                "No leaders available for leapfrog transaction {}",
                signature
            );
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "No leaders available for leapfrog strategy".to_string(),
            ));
        }

        // Log targeted leaders
        let target_info: Vec<String> = leaders
            .iter()
            .map(|l| format!("{}@{}", l.leader_pubkey, l.tpu_socket))
            .collect();
        info!(
            "Leapfrog targets for tx {}: [{}]",
            signature,
            target_info.join(", ")
        );

        // Record metrics
        if let Some(metrics) = &self.metrics {
            metrics.transactions_sent.inc();
            metrics.bytes_sent.inc_by(wire_transaction.len() as f64);
        }

        // Send to all leaders
        let (successful_sends, total_attempts) = if self.config.parallel_sends {
            self.send_parallel(wire_transaction, &leaders).await
        } else {
            self.send_sequential(wire_transaction, &leaders).await
        };

        let total_duration = start.elapsed();

        // Record success/failure metrics
        if successful_sends == 0 {
            error!(
                "All {} leapfrog sends failed for transaction {}",
                total_attempts, signature
            );
            if let Some(metrics) = &self.metrics {
                metrics.transactions_failed.inc();
            }
            return Err(TriggerError::SendFailed(
                "All leapfrog send attempts failed".to_string(),
            ));
        }

        // Record redundancy metrics (only count extra sends beyond the first successful one)
        if let Some(metrics) = &self.metrics {
            if successful_sends > 1 {
                metrics
                    .redundancy_sends
                    .inc_by((successful_sends - 1) as u64);
            }
        }

        info!(
            "Leapfrog transaction {} sent successfully ({}/{} attempts, {:?})",
            signature, successful_sends, total_attempts, total_duration
        );

        Ok(SendResult {
            signature,
            successful_sends,
            total_attempts,
            targeted_leaders: leaders,
            total_duration,
        })
    }

    /// Send to multiple leaders in parallel
    async fn send_parallel(
        &self,
        wire_transaction: &[u8],
        leaders: &[LeaderTpuInfo],
    ) -> (usize, usize) {
        let mut handles = Vec::with_capacity(leaders.len());

        for leader in leaders {
            let tx_bytes = wire_transaction.to_vec();
            let tpu_socket = leader.tpu_socket;
            let leader_pubkey = leader.leader_pubkey;
            let slot = leader.slot;
            let timeout_ms = self.config.send_timeout_ms;

            let handle = tokio::spawn(async move {
                let result = Self::send_udp(&tx_bytes, tpu_socket, timeout_ms).await;
                (leader_pubkey, slot, tpu_socket, result)
            });

            handles.push(handle);
        }

        let mut successful = 0;
        let mut total = 0;

        for handle in handles {
            total += 1;
            match handle.await {
                Ok((pubkey, slot, addr, send_result)) => match send_result {
                    Ok(_) => {
                        successful += 1;
                        debug!(
                            "Parallel send succeeded to {} ({}) at slot {}",
                            pubkey, addr, slot
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Parallel send failed to {} ({}) at slot {}: {}",
                            pubkey, addr, slot, e
                        );
                    }
                },
                Err(e) => {
                    error!("Parallel send task join error: {}", e);
                }
            }
        }

        (successful, total)
    }

    /// Send to multiple leaders sequentially with delay
    async fn send_sequential(
        &self,
        wire_transaction: &[u8],
        leaders: &[LeaderTpuInfo],
    ) -> (usize, usize) {
        let mut successful = 0;
        let total = leaders.len();
        let delay = Duration::from_millis(self.config.inter_send_delay_ms);

        for (i, leader) in leaders.iter().enumerate() {
            match Self::send_udp(
                wire_transaction,
                leader.tpu_socket,
                self.config.send_timeout_ms,
            )
            .await
            {
                Ok(_) => {
                    successful += 1;
                    debug!(
                        "Sequential send {}/{} succeeded to {} ({}) at slot {}",
                        i + 1,
                        total,
                        leader.leader_pubkey,
                        leader.tpu_socket,
                        leader.slot
                    );
                }
                Err(e) => {
                    warn!(
                        "Sequential send {}/{} failed to {} ({}) at slot {}: {}",
                        i + 1,
                        total,
                        leader.leader_pubkey,
                        leader.tpu_socket,
                        leader.slot,
                        e
                    );
                }
            }

            // Delay before next send (except for last)
            if i < leaders.len() - 1 && delay.as_millis() > 0 {
                tokio::time::sleep(delay).await;
            }
        }

        (successful, total)
    }

    /// Send transaction bytes via UDP with timeout
    async fn send_udp(tx_bytes: &[u8], addr: SocketAddr, timeout_ms: u64) -> Result<()> {
        // Bind to any available port
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| TriggerError::SendFailed(format!("Failed to bind UDP socket: {}", e)))?;

        // Send transaction bytes with timeout
        let send_future = socket.send_to(tx_bytes, addr);
        let timeout_duration = std::time::Duration::from_millis(timeout_ms);

        match tokio::time::timeout(timeout_duration, send_future).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(TriggerError::SendFailed(format!(
                "Failed to send to {}: {}",
                addr, e
            ))),
            Err(_) => Err(TriggerError::SendFailed(format!(
                "Timeout sending to {} after {}ms",
                addr, timeout_ms
            ))),
        }
    }

    /// Extract signature from wire format transaction
    fn extract_signature(wire_transaction: &[u8]) -> Result<Signature> {
        // Bincode wire format: [signature_count (1 byte)][signatures (64 bytes each)][...]
        if wire_transaction.len() < 65 {
            return Err(TriggerError::SerializationError(
                "Transaction bytes too short to contain signature (need at least 65 bytes)"
                    .to_string(),
            ));
        }

        Signature::try_from(&wire_transaction[1..65])
            .map_err(|e| TriggerError::SerializationError(format!("Invalid signature: {}", e)))
    }

    /// Send a transaction to specific TPU addresses (fallback method)
    ///
    /// # Arguments
    /// * `wire_transaction` - Pre-serialized transaction bytes
    /// * `addresses` - Specific TPU addresses to send to
    ///
    /// # Returns
    /// SendResult with details about the send operation
    pub async fn send_to_addresses(
        &self,
        wire_transaction: &[u8],
        addresses: &[SocketAddr],
    ) -> Result<SendResult> {
        let start = Instant::now();
        let signature = Self::extract_signature(wire_transaction)?;

        if addresses.is_empty() {
            return Err(TriggerError::SendFailed(
                "No addresses provided for sending".to_string(),
            ));
        }

        info!(
            "Sending transaction {} to {} specific addresses",
            signature,
            addresses.len()
        );

        // Create placeholder LeaderTpuInfo for the addresses (used for fallback sending)
        // Note: Using sequential slot numbers and derived pubkeys since we don't have actual leader info
        let leaders: Vec<LeaderTpuInfo> = addresses
            .iter()
            .enumerate()
            .map(|(i, addr)| {
                // Create a deterministic but unique pubkey based on address
                let mut seed_bytes = [0u8; 32];
                let addr_str = addr.to_string();
                let addr_bytes = addr_str.as_bytes();
                let copy_len = addr_bytes.len().min(32);
                seed_bytes[..copy_len].copy_from_slice(&addr_bytes[..copy_len]);

                LeaderTpuInfo {
                    leader_pubkey: solana_sdk::pubkey::Pubkey::new_from_array(seed_bytes),
                    slot: i as u64,
                    tpu_socket: *addr,
                    protocol: TpuProtocol::Udp,
                }
            })
            .collect();

        let (successful_sends, total_attempts) = if self.config.parallel_sends {
            self.send_parallel(wire_transaction, &leaders).await
        } else {
            self.send_sequential(wire_transaction, &leaders).await
        };

        let total_duration = start.elapsed();

        if successful_sends == 0 {
            return Err(TriggerError::SendFailed(
                "All send attempts to specified addresses failed".to_string(),
            ));
        }

        Ok(SendResult {
            signature,
            successful_sends,
            total_attempts,
            targeted_leaders: leaders,
            total_duration,
        })
    }

    /// Get the current slot from the leader tracker
    pub fn get_current_slot(&self) -> Result<Slot> {
        self.leader_tracker.get_current_slot()
    }

    /// Get configuration
    pub fn config(&self) -> &TpuSenderConfig {
        &self.config
    }

    /// Get reference to leader tracker
    pub fn leader_tracker(&self) -> &Arc<LeaderTracker> {
        &self.leader_tracker
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_client::rpc_client::RpcClient;
    use solana_sdk::hash::Hash;
    use solana_sdk::signer::{keypair::Keypair, Signer};
    use solana_sdk::system_instruction;
    use solana_sdk::transaction::{Transaction, VersionedTransaction};

    #[test]
    fn test_tpu_sender_config_default() {
        let config = TpuSenderConfig::default();
        assert_eq!(config.leapfrog_config.leapfrog_redundancy, 2);
        assert!(!config.leapfrog_config.use_quic);
        assert!(config.parallel_sends);
        assert_eq!(config.inter_send_delay_ms, 10);
    }

    #[test]
    fn test_tpu_sender_config_with_redundancy() {
        let config = TpuSenderConfig::with_redundancy(4);
        assert_eq!(config.leapfrog_config.leapfrog_redundancy, 4);
        assert!(!config.leapfrog_config.use_quic);
    }

    #[test]
    fn test_tpu_sender_config_with_quic() {
        let config = TpuSenderConfig::with_quic(3);
        assert_eq!(config.leapfrog_config.leapfrog_redundancy, 3);
        assert!(config.leapfrog_config.use_quic);
    }

    #[test]
    fn test_send_result_is_success() {
        let result = SendResult {
            signature: Signature::default(),
            successful_sends: 2,
            total_attempts: 3,
            targeted_leaders: vec![],
            total_duration: Duration::from_millis(100),
        };
        assert!(result.is_success());

        let failed_result = SendResult {
            signature: Signature::default(),
            successful_sends: 0,
            total_attempts: 3,
            targeted_leaders: vec![],
            total_duration: Duration::from_millis(100),
        };
        assert!(!failed_result.is_success());
    }

    #[test]
    fn test_send_result_success_rate() {
        let result = SendResult {
            signature: Signature::default(),
            successful_sends: 3,
            total_attempts: 4,
            targeted_leaders: vec![],
            total_duration: Duration::from_millis(100),
        };
        assert!((result.success_rate() - 75.0).abs() < 0.01);

        let empty_result = SendResult {
            signature: Signature::default(),
            successful_sends: 0,
            total_attempts: 0,
            targeted_leaders: vec![],
            total_duration: Duration::from_millis(100),
        };
        assert_eq!(empty_result.success_rate(), 0.0);
    }

    #[test]
    fn test_extract_signature_valid() {
        // Create a valid transaction
        let payer = Keypair::new();
        let to = solana_sdk::pubkey::Pubkey::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &to, 1000);
        let recent_blockhash = Hash::new_unique();

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        let versioned_tx = VersionedTransaction::from(transaction);
        let expected_sig = versioned_tx.signatures[0];
        let wire_tx = bincode::serialize(&versioned_tx).unwrap();

        let extracted = TpuSender::extract_signature(&wire_tx).unwrap();
        assert_eq!(extracted, expected_sig);
    }

    #[test]
    fn test_extract_signature_too_short() {
        let short_bytes = vec![0u8; 32];
        let result = TpuSender::extract_signature(&short_bytes);
        assert!(result.is_err());

        let bytes_64 = vec![0u8; 64];
        let result = TpuSender::extract_signature(&bytes_64);
        assert!(result.is_err());
    }

    #[test]
    fn test_tpu_sender_creation() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = Arc::new(LeaderTracker::new(rpc_client));
        let sender = TpuSender::new(tracker);

        assert!(sender.config.parallel_sends);
        assert_eq!(sender.config.leapfrog_config.leapfrog_redundancy, 2);
    }

    #[test]
    fn test_tpu_sender_with_custom_config() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = Arc::new(LeaderTracker::new(rpc_client));

        let config = TpuSenderConfig {
            leapfrog_config: LeapfrogConfig::new(5, true),
            inter_send_delay_ms: 20,
            parallel_sends: false,
            send_timeout_ms: 1000,
        };

        let sender = TpuSender::with_config(tracker, config);

        assert!(!sender.config.parallel_sends);
        assert_eq!(sender.config.leapfrog_config.leapfrog_redundancy, 5);
        assert!(sender.config.leapfrog_config.use_quic);
        assert_eq!(sender.config.inter_send_delay_ms, 20);
    }

    #[test]
    fn test_tpu_sender_with_metrics() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = Arc::new(LeaderTracker::new(rpc_client));
        let metrics = Arc::new(TriggerMetrics::new());

        let sender = TpuSender::with_metrics(tracker, TpuSenderConfig::default(), metrics);

        assert!(sender.metrics.is_some());
    }

    #[tokio::test]
    async fn test_send_to_addresses_empty() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let tracker = Arc::new(LeaderTracker::new(rpc_client));
        let sender = TpuSender::new(tracker);

        // Create a valid transaction
        let payer = Keypair::new();
        let to = solana_sdk::pubkey::Pubkey::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &to, 1000);
        let recent_blockhash = Hash::new_unique();

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        let versioned_tx = VersionedTransaction::from(transaction);
        let wire_tx = bincode::serialize(&versioned_tx).unwrap();

        // Empty addresses should fail
        let result = sender.send_to_addresses(&wire_tx, &[]).await;
        assert!(result.is_err());
    }
}
