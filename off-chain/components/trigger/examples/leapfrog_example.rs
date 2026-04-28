//! Example demonstrating the Leapfrog N+X transaction sending strategy
//!
//! This example shows how to use the send_leapfrog method to send transactions
//! to multiple future leaders for better inclusion rates.
//!
//! Run with: cargo run --example leapfrog_example

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    hash::Hash,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::{Transaction, VersionedTransaction},
};
use std::sync::Arc;
use trigger::{
    config::LeapfrogConfig, errors::Result, leader_resolver::LeaderResolver, udp_client::TpuClient,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("=== Leapfrog N+X Transaction Sending Example ===\n");

    // Configuration
    let rpc_url = "https://api.devnet.solana.com".to_string();

    // Create RPC client and leader resolver
    let rpc_client = Arc::new(RpcClient::new(rpc_url.clone()));
    let leader_resolver = Arc::new(LeaderResolver::new(Arc::clone(&rpc_client)));

    // Create TPU client with leader resolver
    let tpu_client = TpuClient::with_leader_resolver(rpc_url, Some(3), leader_resolver)?;

    println!("✓ TPU Client initialized with leader resolver");

    // Create a sample transaction
    let payer = Keypair::new();
    let to = solana_sdk::pubkey::Pubkey::new_unique();
    let instruction = system_instruction::transfer(&payer.pubkey(), &to, 1_000_000); // 0.001 SOL
    let recent_blockhash = Hash::new_unique(); // In production, fetch real blockhash

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    let versioned_tx = VersionedTransaction::from(transaction);
    let wire_transaction = bincode::serialize(&versioned_tx).unwrap();

    println!(
        "✓ Transaction created and serialized ({} bytes)",
        wire_transaction.len()
    );

    // Get current slot
    let current_slot = rpc_client.get_slot().unwrap_or(1000);
    println!("✓ Current slot: {}", current_slot);

    // Configure Leapfrog strategy
    println!("\n=== Leapfrog Configuration ===");

    // Test 1: Default config (redundancy=2, UDP)
    let config_default = LeapfrogConfig::default();
    println!("\nTest 1: Default Config");
    println!("  - Redundancy: {}", config_default.leapfrog_redundancy);
    println!("  - Use QUIC: {}", config_default.use_quic);
    println!("  - Total Leaders: {}", config_default.total_leaders());
    println!("  - Slot Offsets: {:?}", config_default.slot_offsets());
    println!(
        "  - Target Slots: {:?}",
        config_default
            .slot_offsets()
            .iter()
            .map(|offset| current_slot + offset)
            .collect::<Vec<_>>()
    );

    println!("\n--- Sending with Default Config ---");
    match tpu_client
        .send_leapfrog(&wire_transaction, current_slot, &config_default)
        .await
    {
        Ok(signature) => {
            println!("✓ Transaction sent successfully!");
            println!("  Signature: {}", signature);
        }
        Err(e) => {
            println!("✗ Transaction failed (expected in test environment): {}", e);
            println!("  Note: This is normal if not connected to a real cluster");
        }
    }

    // Test 2: Custom config with higher redundancy
    let config_custom = LeapfrogConfig::new(4, false);
    println!("\nTest 2: Custom Config (redundancy=4)");
    println!("  - Redundancy: {}", config_custom.leapfrog_redundancy);
    println!("  - Total Leaders: {}", config_custom.total_leaders());
    println!("  - Slot Offsets: {:?}", config_custom.slot_offsets());
    println!(
        "  - Target Slots: {:?}",
        config_custom
            .slot_offsets()
            .iter()
            .map(|offset| current_slot + offset)
            .collect::<Vec<_>>()
    );

    println!("\n--- Sending with Custom Config ---");
    match tpu_client
        .send_leapfrog(&wire_transaction, current_slot, &config_custom)
        .await
    {
        Ok(signature) => {
            println!("✓ Transaction sent successfully!");
            println!("  Signature: {}", signature);
        }
        Err(e) => {
            println!("✗ Transaction failed (expected in test environment): {}", e);
            println!("  Note: This is normal if not connected to a real cluster");
        }
    }

    // Test 3: QUIC mode
    let config_quic = LeapfrogConfig::new(2, true);
    println!("\nTest 3: QUIC Mode");
    println!("  - Redundancy: {}", config_quic.leapfrog_redundancy);
    println!("  - Use QUIC: {}", config_quic.use_quic);
    println!("  - Total Leaders: {}", config_quic.total_leaders());

    println!("\n--- Sending with QUIC Mode ---");
    match tpu_client
        .send_leapfrog(&wire_transaction, current_slot, &config_quic)
        .await
    {
        Ok(signature) => {
            println!("✓ Transaction sent successfully!");
            println!("  Signature: {}", signature);
        }
        Err(e) => {
            println!("✗ Transaction failed (expected in test environment): {}", e);
            println!("  Note: This is normal if not connected to a real cluster");
        }
    }

    println!("\n=== Example Complete ===");
    println!("\nKey Features Demonstrated:");
    println!("  1. Parallel sending to multiple future leaders");
    println!("  2. Configurable redundancy (N+X strategy)");
    println!("  3. UDP vs QUIC mode selection");
    println!("  4. Graceful error handling per target");
    println!("  5. Comprehensive logging of leader targets");

    println!("\nExpected Log Format:");
    println!("  'Sending Leapfrog tx to: [<LeaderPubkey> (Slot X), <LeaderPubkey> (Slot Y), ...]'");

    Ok(())
}
