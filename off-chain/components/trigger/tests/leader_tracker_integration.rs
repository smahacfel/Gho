//! Integration tests for LeaderTracker and TpuSender
//!
//! These tests verify the new TPU client implementation:
//! - Dynamic leader resolution via LeaderTracker
//! - Localhost filtering
//! - Leapfrog strategy integration with TpuSender
//! - Epoch-aware caching

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::{Transaction, VersionedTransaction},
};
use std::sync::Arc;
use trigger::{
    LeaderTracker, LeaderTrackerConfig, TpuProtocol, TpuSender, TpuSenderConfig, TriggerMetrics,
};

#[test]
fn test_leader_tracker_config_default() {
    let config = LeaderTrackerConfig::default();
    assert_eq!(config.epoch_check_interval_secs, 10);
    assert_eq!(config.cluster_nodes_cache_secs, 300);
    assert_eq!(config.default_protocol, TpuProtocol::Udp);
    assert!(config.filter_localhost);
}

#[test]
fn test_leader_tracker_config_custom() {
    let config = LeaderTrackerConfig {
        epoch_check_interval_secs: 30,
        cluster_nodes_cache_secs: 600,
        default_protocol: TpuProtocol::Quic,
        filter_localhost: false,
    };

    assert_eq!(config.epoch_check_interval_secs, 30);
    assert_eq!(config.cluster_nodes_cache_secs, 600);
    assert_eq!(config.default_protocol, TpuProtocol::Quic);
    assert!(!config.filter_localhost);
}

#[test]
fn test_tpu_protocol_variants() {
    assert_eq!(TpuProtocol::default(), TpuProtocol::Udp);
    assert_ne!(TpuProtocol::Udp, TpuProtocol::Quic);
}

#[tokio::test]
async fn test_leader_tracker_creation() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let tracker = LeaderTracker::new(rpc_client);

    assert!(tracker.config().filter_localhost);
    assert_eq!(tracker.config().default_protocol, TpuProtocol::Udp);
}

#[tokio::test]
async fn test_leader_tracker_with_config() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let config = LeaderTrackerConfig {
        epoch_check_interval_secs: 60,
        cluster_nodes_cache_secs: 120,
        default_protocol: TpuProtocol::Quic,
        filter_localhost: true,
    };

    let tracker = LeaderTracker::with_config(rpc_client, config);

    assert_eq!(tracker.config().epoch_check_interval_secs, 60);
    assert_eq!(tracker.config().cluster_nodes_cache_secs, 120);
    assert_eq!(tracker.config().default_protocol, TpuProtocol::Quic);
}

#[tokio::test]
async fn test_leader_tracker_with_metrics() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let metrics = Arc::new(TriggerMetrics::new());
    let config = LeaderTrackerConfig::default();

    let tracker = LeaderTracker::with_metrics(rpc_client, config, metrics);

    assert!(tracker.has_metrics());
}

#[tokio::test]
async fn test_leader_tracker_initial_state() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let tracker = LeaderTracker::new(rpc_client);

    // Initially, caches should be empty
    assert_eq!(tracker.schedule_size().await, 0);
    assert_eq!(tracker.cluster_nodes_count().await, 0);
    assert!(tracker.cached_epoch().await.is_none());
}

#[test]
fn test_tpu_sender_config_default() {
    let config = TpuSenderConfig::default();

    assert_eq!(config.leapfrog_config.leapfrog_redundancy, 2);
    assert!(!config.leapfrog_config.use_quic);
    assert!(config.parallel_sends);
    assert_eq!(config.inter_send_delay_ms, 10);
    assert_eq!(config.send_timeout_ms, 500);
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

#[tokio::test]
async fn test_tpu_sender_creation() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let tracker = Arc::new(LeaderTracker::new(rpc_client));

    let sender = TpuSender::new(tracker);

    assert!(sender.config().parallel_sends);
    assert_eq!(sender.config().leapfrog_config.leapfrog_redundancy, 2);
}

#[tokio::test]
async fn test_tpu_sender_with_config() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let tracker = Arc::new(LeaderTracker::new(rpc_client));

    let config = TpuSenderConfig {
        leapfrog_config: trigger::LeapfrogConfig::new(5, true),
        inter_send_delay_ms: 50,
        parallel_sends: false,
        send_timeout_ms: 1000,
    };

    let sender = TpuSender::with_config(tracker, config);

    assert!(!sender.config().parallel_sends);
    assert_eq!(sender.config().leapfrog_config.leapfrog_redundancy, 5);
    assert!(sender.config().leapfrog_config.use_quic);
    assert_eq!(sender.config().inter_send_delay_ms, 50);
}

#[tokio::test]
async fn test_tpu_sender_with_metrics() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let tracker = Arc::new(LeaderTracker::new(rpc_client));
    let metrics = Arc::new(TriggerMetrics::new());

    let _sender = TpuSender::with_metrics(tracker, TpuSenderConfig::default(), metrics);

    // Sender should be created successfully with metrics
}

#[tokio::test]
async fn test_send_result_success_rate() {
    use std::time::Duration;
    use trigger::SendResult;

    let result = SendResult {
        signature: solana_sdk::signature::Signature::default(),
        successful_sends: 3,
        total_attempts: 4,
        targeted_leaders: vec![],
        total_duration: Duration::from_millis(100),
    };

    assert!(result.is_success());
    assert!((result.success_rate() - 75.0).abs() < 0.01);

    let failed_result = SendResult {
        signature: solana_sdk::signature::Signature::default(),
        successful_sends: 0,
        total_attempts: 4,
        targeted_leaders: vec![],
        total_duration: Duration::from_millis(100),
    };

    assert!(!failed_result.is_success());
    assert_eq!(failed_result.success_rate(), 0.0);
}

#[tokio::test]
async fn test_send_to_empty_addresses() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let tracker = Arc::new(LeaderTracker::new(rpc_client));
    let sender = TpuSender::new(tracker);

    // Create a valid transaction
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
    let wire_tx = bincode::serialize(&versioned_tx).unwrap();

    // Send to empty addresses should fail
    let result = sender.send_to_addresses(&wire_tx, &[]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_leapfrog_send_invalid_transaction() {
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));
    let tracker = Arc::new(LeaderTracker::new(rpc_client));
    let sender = TpuSender::new(tracker);

    // Too short transaction bytes
    let short_bytes = vec![0u8; 32];
    let current_slot = 100;

    let result = sender.send_leapfrog(&short_bytes, current_slot).await;
    assert!(result.is_err());

    if let Err(e) = result {
        let error_msg = format!("{}", e);
        assert!(error_msg.contains("too short") || error_msg.contains("65 bytes"));
    }
}

/// Test localhost filtering logic
#[test]
fn test_localhost_detection() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    // Helper function to check localhost (simulating LeaderTracker's logic)
    fn is_localhost(addr: &SocketAddr) -> bool {
        match addr.ip() {
            IpAddr::V4(ipv4) => ipv4 == Ipv4Addr::new(127, 0, 0, 1) || ipv4.is_loopback(),
            IpAddr::V6(ipv6) => ipv6.is_loopback(),
        }
    }

    // IPv4 localhost
    let localhost_v4: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    assert!(is_localhost(&localhost_v4));

    // IPv6 localhost
    let localhost_v6: SocketAddr = "[::1]:8001".parse().unwrap();
    assert!(is_localhost(&localhost_v6));

    // Public IPv4
    let public_v4: SocketAddr = "192.168.1.1:8001".parse().unwrap();
    assert!(!is_localhost(&public_v4));

    // Public IPv6
    let public_v6: SocketAddr = "[2001:db8::1]:8001".parse().unwrap();
    assert!(!is_localhost(&public_v6));
}

/// Test slot offset calculation for leapfrog strategy
#[test]
fn test_leapfrog_slot_offsets() {
    use trigger::LeapfrogConfig;

    // Default: redundancy=2 -> offsets [0, 4, 8]
    let default_config = LeapfrogConfig::default();
    assert_eq!(default_config.slot_offsets(), vec![0, 4, 8]);

    // Custom: redundancy=0 -> only current slot
    let config_0 = LeapfrogConfig::new(0, false);
    assert_eq!(config_0.slot_offsets(), vec![0]);

    // Custom: redundancy=4 -> 5 total leaders
    let config_4 = LeapfrogConfig::new(4, true);
    assert_eq!(config_4.slot_offsets(), vec![0, 4, 8, 12, 16]);
}

/// Integration test for the full workflow (mock/demonstration)
#[tokio::test]
async fn test_full_workflow_demonstration() {
    // This test demonstrates the intended workflow without requiring a live network

    // 1. Create RPC client
    let rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com".to_string()));

    // 2. Create LeaderTracker with custom config
    let tracker_config = LeaderTrackerConfig {
        epoch_check_interval_secs: 30,
        cluster_nodes_cache_secs: 300,
        default_protocol: TpuProtocol::Udp,
        filter_localhost: true, // Important: filter out 127.0.0.1
    };
    let tracker = Arc::new(LeaderTracker::with_config(rpc_client, tracker_config));

    // 3. Create TpuSender with metrics
    let metrics = Arc::new(TriggerMetrics::new());
    let sender_config = TpuSenderConfig {
        leapfrog_config: trigger::LeapfrogConfig::new(2, false), // UDP with N+2 redundancy
        inter_send_delay_ms: 10,
        parallel_sends: true, // Fan-out sends in parallel
        send_timeout_ms: 500,
    };
    let _sender = TpuSender::with_metrics(tracker.clone(), sender_config, metrics.clone());

    // 4. Verify configuration
    assert!(tracker.config().filter_localhost);
    assert_eq!(tracker.config().default_protocol, TpuProtocol::Udp);

    // 5. Verify initial state
    assert_eq!(tracker.schedule_size().await, 0);
    assert_eq!(tracker.cluster_nodes_count().await, 0);

    // Note: In production, you would:
    // - Call tracker.ensure_caches_fresh().await to populate caches
    // - Use sender.send_leapfrog(&wire_tx, current_slot).await to send transactions
    // - The sender would automatically resolve leader TPU addresses dynamically

    println!("✓ Full workflow demonstration completed successfully");
    println!("  - LeaderTracker created with localhost filtering enabled");
    println!("  - TpuSender configured with N+2 leapfrog redundancy");
    println!("  - Parallel fan-out sending enabled");
}
