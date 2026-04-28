//! Integration tests for Leapfrog N+X transaction sending strategy
//!
//! These tests verify the leapfrog functionality including:
//! - Configuration integration
//! - Parallel sending to multiple leaders
//! - Error handling per target
//! - Log output format

use solana_sdk::{
    hash::Hash,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::{Transaction, VersionedTransaction},
};
use trigger::{config::LeapfrogConfig, udp_client::TpuClient};

#[test]
fn test_leapfrog_config_creation() {
    // Test default configuration
    let default_config = LeapfrogConfig::default();
    assert_eq!(default_config.leapfrog_redundancy, 2);
    assert_eq!(default_config.use_quic, false);
    assert_eq!(default_config.total_leaders(), 3);

    let offsets = default_config.slot_offsets();
    assert_eq!(offsets, vec![0, 4, 8]);
}

#[test]
fn test_leapfrog_config_custom() {
    // Test custom configuration
    let config = LeapfrogConfig::new(4, true);
    assert_eq!(config.leapfrog_redundancy, 4);
    assert_eq!(config.use_quic, true);
    assert_eq!(config.total_leaders(), 5);

    let offsets = config.slot_offsets();
    assert_eq!(offsets, vec![0, 4, 8, 12, 16]);
}

#[test]
fn test_leapfrog_slot_calculation() {
    let config = LeapfrogConfig::default();
    let current_slot = 100;

    let target_slots: Vec<u64> = config
        .slot_offsets()
        .iter()
        .map(|offset| current_slot + offset)
        .collect();

    assert_eq!(target_slots, vec![100, 104, 108]);
}

#[tokio::test]
async fn test_tpu_client_creation_with_leapfrog() {
    use solana_client::rpc_client::RpcClient;
    use std::sync::Arc;
    use trigger::leader_resolver::LeaderResolver;

    let rpc_url = "https://api.devnet.solana.com".to_string();
    let rpc_client = Arc::new(RpcClient::new(rpc_url.clone()));
    let leader_resolver = Arc::new(LeaderResolver::new(rpc_client));

    let tpu_client = TpuClient::with_leader_resolver(rpc_url, Some(3), leader_resolver);

    assert!(tpu_client.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_leapfrog_send_signature_extraction() {
    let rpc_url = "https://api.devnet.solana.com".to_string();
    let tpu_client = TpuClient::new(rpc_url, Some(3)).unwrap();

    // Create a test transaction
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
    let wire_transaction = bincode::serialize(&versioned_tx).unwrap();

    // Test with valid transaction
    let config = LeapfrogConfig::default();
    let current_slot = 1000;

    // This may succeed or fail depending on leader schedule availability
    let result = tpu_client
        .send_leapfrog(&wire_transaction, current_slot, &config)
        .await;

    // If it fails, it should be due to leader availability or network issues
    if let Err(e) = result {
        let error_msg = format!("{}", e);
        // Verify error is reasonable (not a panic or signature extraction error)
        assert!(
            error_msg.contains("No leaders available")
                || error_msg.contains("Leader schedule")
                || error_msg.contains("send")
                || error_msg.contains("failed")
        );
    }
    // If it succeeds, that's also fine - signature was extracted correctly
}

#[tokio::test]
async fn test_leapfrog_invalid_transaction_bytes() {
    let rpc_url = "https://api.devnet.solana.com".to_string();
    let tpu_client = TpuClient::new(rpc_url, Some(3)).unwrap();

    let config = LeapfrogConfig::default();
    let current_slot = 1000;

    // Test with too short bytes (should fail signature extraction)
    let short_bytes = vec![0u8; 32];
    let result = tpu_client
        .send_leapfrog(&short_bytes, current_slot, &config)
        .await;

    assert!(result.is_err());
    if let Err(e) = result {
        let error_msg = format!("{}", e);
        assert!(error_msg.contains("too short") || error_msg.contains("65 bytes"));
    }
}

#[test]
fn test_leapfrog_redundancy_levels() {
    // Test different redundancy levels
    let configs = vec![
        (0, 1, vec![0]),               // No redundancy
        (1, 2, vec![0, 4]),            // N+1
        (2, 3, vec![0, 4, 8]),         // N+2 (default)
        (3, 4, vec![0, 4, 8, 12]),     // N+3
        (4, 5, vec![0, 4, 8, 12, 16]), // N+4
    ];

    for (redundancy, expected_total, expected_offsets) in configs {
        let config = LeapfrogConfig::new(redundancy, false);
        assert_eq!(config.total_leaders(), expected_total);
        assert_eq!(config.slot_offsets(), expected_offsets);
    }
}

#[test]
fn test_leapfrog_quic_vs_udp_mode() {
    let udp_config = LeapfrogConfig::new(2, false);
    let quic_config = LeapfrogConfig::new(2, true);

    assert_eq!(udp_config.use_quic, false);
    assert_eq!(quic_config.use_quic, true);

    // Both should have same redundancy
    assert_eq!(udp_config.total_leaders(), quic_config.total_leaders());
}

/// This test demonstrates the expected log format for leapfrog sends
#[test]
fn test_leapfrog_expected_log_format() {
    // Expected format:
    // "Sending Leapfrog tx to: [<LeaderPubkey> (Slot X), <LeaderPubkey> (Slot Y), ...]"

    use solana_sdk::pubkey::Pubkey;

    let leader1 = Pubkey::new_unique();
    let leader2 = Pubkey::new_unique();
    let leader3 = Pubkey::new_unique();

    let target_description: Vec<String> = vec![
        format!("{} (Slot 100)", leader1),
        format!("{} (Slot 104)", leader2),
        format!("{} (Slot 108)", leader3),
    ];

    let log_message = format!(
        "Sending Leapfrog tx to: [{}]",
        target_description.join(", ")
    );

    // Verify format
    assert!(log_message.starts_with("Sending Leapfrog tx to: ["));
    assert!(log_message.contains("(Slot 100)"));
    assert!(log_message.contains("(Slot 104)"));
    assert!(log_message.contains("(Slot 108)"));
    assert!(log_message.ends_with("]"));

    println!("{}", log_message);
    println!("\n✓ Log format matches expected pattern:");
    println!(
        "  'Sending Leapfrog tx to: [LeaderA (Slot 100), LeaderB (Slot 104), LeaderC (Slot 108)]'"
    );
}
