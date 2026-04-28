//! TPU Connection Manager - Manages QUIC connections to TPU leaders
//!
//! This module handles pre-warming and pooling of QUIC connections to Solana
//! validators for optimal transaction submission latency.

use crate::errors::{Result, TriggerError};
use crate::leader_resolver::LeaderResolver;
use quinn::{ClientConfig, Connection, Endpoint};
use rustls::{Certificate, ClientConfig as RustlsConfig};
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Default TPU QUIC port offset from TPU port
const TPU_QUIC_PORT_OFFSET: u16 = 6;

/// Configuration for connection pre-warming
#[derive(Debug, Clone)]
pub struct PrewarmConfig {
    /// Number of slots ahead to start warming connections (default: 2)
    pub slots_ahead_min: u64,
    /// Maximum slots ahead to warm connections (default: 4)
    pub slots_ahead_max: u64,
    /// Maximum number of connections to maintain in pool
    pub max_connections: usize,
}

impl Default for PrewarmConfig {
    fn default() -> Self {
        Self {
            slots_ahead_min: 2,
            slots_ahead_max: 4,
            max_connections: 20,
        }
    }
}

/// Represents a managed TPU QUIC connection
#[allow(dead_code)]
struct ManagedConnection {
    /// The QUIC connection
    connection: Connection,
    /// The validator pubkey this connection is for
    pubkey: Pubkey,
    /// The slot this connection was created for
    slot: Slot,
    /// Target TPU QUIC address
    address: SocketAddr,
}

/// TPU Connection Manager with pre-warming and pooling
pub struct TpuConnectionManager {
    /// Leader resolver for getting validator addresses
    leader_resolver: Arc<LeaderResolver>,
    /// QUIC endpoint for creating connections
    endpoint: Endpoint,
    /// Pool of active connections: pubkey -> connection
    connections: Arc<RwLock<HashMap<Pubkey, ManagedConnection>>>,
    /// Pre-warming configuration
    prewarm_config: PrewarmConfig,
}

impl TpuConnectionManager {
    /// Create a new TPU connection manager
    pub async fn new(leader_resolver: Arc<LeaderResolver>) -> Result<Self> {
        let prewarm_config = PrewarmConfig::default();
        Self::with_config(leader_resolver, prewarm_config).await
    }

    /// Create a new TPU connection manager with custom configuration
    pub async fn with_config(
        leader_resolver: Arc<LeaderResolver>,
        prewarm_config: PrewarmConfig,
    ) -> Result<Self> {
        // Create QUIC endpoint
        let endpoint = Self::create_endpoint().await?;

        Ok(Self {
            leader_resolver,
            endpoint,
            connections: Arc::new(RwLock::new(HashMap::new())),
            prewarm_config,
        })
    }

    /// Create a QUIC endpoint with appropriate configuration
    async fn create_endpoint() -> Result<Endpoint> {
        // Configure QUIC client
        let crypto_config = RustlsConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        let client_config = ClientConfig::new(Arc::new(crypto_config));

        // Bind to any available address
        let bind_addr: SocketAddr = "0.0.0.0:0".parse().expect("Valid hardcoded bind address");
        let mut endpoint = Endpoint::client(bind_addr).map_err(|e| {
            error!("Failed to create QUIC endpoint: {}", e);
            TriggerError::NetworkError(format!("Failed to create QUIC endpoint: {}", e))
        })?;

        endpoint.set_default_client_config(client_config);

        info!("QUIC endpoint created successfully");
        Ok(endpoint)
    }

    /// Get or create a connection to a specific leader
    pub async fn get_connection(&self, leader_pubkey: &Pubkey) -> Result<Connection> {
        // Check if we already have a connection
        {
            let connections = self.connections.read().await;
            if let Some(managed_conn) = connections.get(leader_pubkey) {
                // Verify connection is still valid
                if managed_conn.connection.close_reason().is_none() {
                    debug!("Reusing existing connection to {}", leader_pubkey);
                    return Ok(managed_conn.connection.clone());
                } else {
                    debug!(
                        "Existing connection to {} is closed, will reconnect",
                        leader_pubkey
                    );
                }
            }
        }

        // Need to establish new connection
        self.establish_connection(leader_pubkey).await
    }

    /// Establish a new connection to a validator
    async fn establish_connection(&self, leader_pubkey: &Pubkey) -> Result<Connection> {
        // Get contact info for the validator
        let contact_info = self.leader_resolver.get_contact_info(leader_pubkey).await?;

        info!(
            "Establishing QUIC connection to {} at {}",
            leader_pubkey, contact_info.tpu_quic
        );

        // Attempt to connect
        let connection = self
            .endpoint
            .connect(contact_info.tpu_quic, "solana-tpu")
            .map_err(|e| {
                error!(
                    "Failed to initiate QUIC connection to {}: {}",
                    leader_pubkey, e
                );
                TriggerError::NetworkError(format!("Failed to initiate connection: {}", e))
            })?
            .await
            .map_err(|e| {
                warn!("QUIC connection to {} failed: {}", leader_pubkey, e);
                TriggerError::NetworkError(format!("Connection failed: {}", e))
            })?;

        info!("QUIC connection established to {}", leader_pubkey);

        // Store in connection pool
        let managed_conn = ManagedConnection {
            connection: connection.clone(),
            pubkey: *leader_pubkey,
            slot: 0, // Will be updated during pre-warming
            address: contact_info.tpu_quic,
        };

        let mut connections = self.connections.write().await;
        connections.insert(*leader_pubkey, managed_conn);

        // Clean up old connections if pool is too large
        if connections.len() > self.prewarm_config.max_connections {
            self.cleanup_old_connections(&mut connections).await;
        }

        Ok(connection)
    }

    /// Pre-warm connections to upcoming leaders
    ///
    /// # Arguments
    /// * `upcoming_leaders` - List of (pubkey, slot) pairs for upcoming leaders
    pub async fn prewarm_connections(&self, upcoming_leaders: &[(Pubkey, Slot)]) {
        debug!(
            "Pre-warming connections for {} leaders",
            upcoming_leaders.len()
        );

        for (leader_pubkey, slot) in upcoming_leaders {
            // Check if connection already exists
            let needs_connection = {
                let connections = self.connections.read().await;
                !connections.contains_key(leader_pubkey)
            };

            if needs_connection {
                match self.establish_connection(leader_pubkey).await {
                    Ok(_) => {
                        info!(
                            "Pre-warmed connection to {} for slot {}",
                            leader_pubkey, slot
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to pre-warm connection to {} for slot {}: {}",
                            leader_pubkey, slot, e
                        );
                    }
                }
            } else {
                debug!(
                    "Connection to {} already exists, skipping pre-warm",
                    leader_pubkey
                );
            }
        }
    }

    /// Clean up old or closed connections
    async fn cleanup_old_connections(&self, connections: &mut HashMap<Pubkey, ManagedConnection>) {
        let initial_count = connections.len();

        // Remove closed connections
        connections.retain(|pubkey, managed_conn| {
            if managed_conn.connection.close_reason().is_some() {
                debug!("Removing closed connection to {}", pubkey);
                false
            } else {
                true
            }
        });

        // If still too many, remove oldest (simple FIFO)
        while connections.len() > self.prewarm_config.max_connections {
            if let Some(oldest_key) = connections.keys().next().copied() {
                connections.remove(&oldest_key);
                debug!("Removed connection to {} to maintain pool size", oldest_key);
            } else {
                break;
            }
        }

        let removed_count = initial_count - connections.len();
        if removed_count > 0 {
            info!("Cleaned up {} old connections", removed_count);
        }
    }

    /// Get the number of active connections
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Close a specific connection
    pub async fn close_connection(&self, leader_pubkey: &Pubkey) {
        let mut connections = self.connections.write().await;
        if let Some(managed_conn) = connections.remove(leader_pubkey) {
            managed_conn.connection.close(0u32.into(), b"Manual close");
            info!("Closed connection to {}", leader_pubkey);
        }
    }

    /// Close all connections
    pub async fn close_all_connections(&self) {
        let mut connections = self.connections.write().await;
        let count = connections.len();

        for (pubkey, managed_conn) in connections.drain() {
            managed_conn.connection.close(0u32.into(), b"Shutdown");
            debug!("Closed connection to {}", pubkey);
        }

        info!("Closed all {} connections", count);
    }

    /// Get prewarm configuration
    pub fn prewarm_config(&self) -> &PrewarmConfig {
        &self.prewarm_config
    }
}

/// Certificate verifier that skips validation
///
/// ⚠️ WARNING: This is for development/testing only! ⚠️
///
/// In production, implement proper certificate verification to prevent
/// man-in-the-middle attacks. Use a verifier that validates:
/// - Certificate chain of trust
/// - Certificate expiration
/// - Hostname matching
/// - Certificate revocation status
///
/// Example production implementation:
/// ```ignore
/// use rustls::client::{ServerCertVerifier, ServerCertVerified};
///
/// struct ProductionVerifier {
///     roots: rustls::RootCertStore,
/// }
///
/// impl ServerCertVerifier for ProductionVerifier {
///     fn verify_server_cert(&self, ...) -> Result<ServerCertVerified, Error> {
///         // Implement proper certificate verification
///         webpki::verify_server_cert(...)
///     }
/// }
/// ```
struct SkipServerVerification;

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        // Skip verification for testing/development
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leader_resolver::LeaderResolver;
    use solana_client::rpc_client::RpcClient;

    #[test]
    fn test_prewarm_config_default() {
        let config = PrewarmConfig::default();
        assert_eq!(config.slots_ahead_min, 2);
        assert_eq!(config.slots_ahead_max, 4);
        assert_eq!(config.max_connections, 20);
    }

    #[test]
    fn test_prewarm_config_custom() {
        let config = PrewarmConfig {
            slots_ahead_min: 3,
            slots_ahead_max: 6,
            max_connections: 50,
        };
        assert_eq!(config.slots_ahead_min, 3);
        assert_eq!(config.slots_ahead_max, 6);
        assert_eq!(config.max_connections, 50);
    }

    #[tokio::test]
    async fn test_connection_manager_creation() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

        let manager = TpuConnectionManager::new(leader_resolver).await;
        assert!(manager.is_ok());

        let manager = manager.unwrap();
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_connection_manager_with_custom_config() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

        let prewarm_config = PrewarmConfig {
            slots_ahead_min: 3,
            slots_ahead_max: 5,
            max_connections: 30,
        };

        let manager =
            TpuConnectionManager::with_config(leader_resolver, prewarm_config.clone()).await;
        assert!(manager.is_ok());

        let manager = manager.unwrap();
        assert_eq!(manager.prewarm_config().slots_ahead_min, 3);
        assert_eq!(manager.prewarm_config().slots_ahead_max, 5);
        assert_eq!(manager.prewarm_config().max_connections, 30);
    }

    #[tokio::test]
    async fn test_connection_count_initially_zero() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

        let manager = TpuConnectionManager::new(leader_resolver).await.unwrap();
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_close_all_connections() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

        let manager = TpuConnectionManager::new(leader_resolver).await.unwrap();

        // Close all (should handle empty case)
        manager.close_all_connections().await;
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_prewarm_connections_empty_list() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

        let manager = TpuConnectionManager::new(leader_resolver).await.unwrap();

        // Pre-warm with empty list should not error
        manager.prewarm_connections(&[]).await;
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_connection_for_unknown_validator() {
        let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
        let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

        let manager = TpuConnectionManager::new(leader_resolver).await.unwrap();
        let unknown_pubkey = Pubkey::new_unique();

        // Should fail because validator is not in cluster nodes
        let result = manager.get_connection(&unknown_pubkey).await;
        assert!(result.is_err());
    }
}
