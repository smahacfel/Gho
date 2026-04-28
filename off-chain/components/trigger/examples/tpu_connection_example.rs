//! Example: TPU Connection Manager with Pre-warming
//!
//! This example demonstrates how to use the LeaderResolver and TpuConnectionManager
//! to establish QUIC connections to Solana validators with pre-warming.

use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use trigger::{LeaderResolver, PrewarmConfig, TpuConnectionManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== TPU Connection Manager Example ===\n");

    // Step 1: Create RPC client
    println!("Step 1: Creating RPC client...");
    let rpc_url =
        std::env::var("RPC_URL").unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());
    let rpc_client = Arc::new(RpcClient::new(rpc_url.clone()));
    println!("  Connected to: {}\n", rpc_url);

    // Step 2: Create LeaderResolver
    println!("Step 2: Creating LeaderResolver...");
    let leader_resolver = Arc::new(LeaderResolver::new(rpc_client.clone()));
    println!("  LeaderResolver created with 5-minute cache\n");

    // Step 3: Fetch and display cluster nodes
    println!("Step 3: Fetching cluster nodes...");
    match leader_resolver.refresh_if_needed().await {
        Ok(_) => {
            let cache_size = leader_resolver.cache_size().await;
            println!("  ✓ Successfully fetched {} validators", cache_size);

            // Display some validators
            let validators = leader_resolver.get_all_validators().await;
            if !validators.is_empty() {
                println!("\n  Sample validators:");
                for (i, validator) in validators.iter().take(3).enumerate() {
                    if let Ok(contact_info) = leader_resolver.get_contact_info(validator).await {
                        println!("    {}. {}", i + 1, validator);
                        println!("       TPU:      {}", contact_info.tpu);
                        println!("       TPU QUIC: {}", contact_info.tpu_quic);
                        if let Some(gossip) = contact_info.gossip {
                            println!("       Gossip:   {}", gossip);
                        }
                    }
                }
            }
            println!();
        }
        Err(e) => {
            eprintln!("  ✗ Failed to fetch cluster nodes: {}", e);
            eprintln!("  This is normal if running without network access\n");
            return Ok(());
        }
    }

    // Step 4: Create TpuConnectionManager
    println!("Step 4: Creating TpuConnectionManager...");
    let prewarm_config = PrewarmConfig {
        slots_ahead_min: 2,
        slots_ahead_max: 4,
        max_connections: 20,
    };
    let manager =
        TpuConnectionManager::with_config(leader_resolver.clone(), prewarm_config).await?;
    println!("  ✓ TpuConnectionManager created");
    println!("  Configuration:");
    println!("    - Pre-warm: 2-4 slots ahead");
    println!("    - Max connections: 20\n");

    // Step 5: Simulate getting upcoming leaders
    println!("Step 5: Simulating leader schedule...");
    let validators = leader_resolver.get_all_validators().await;

    if validators.len() >= 3 {
        // Get current slot
        let current_slot = match rpc_client.get_slot() {
            Ok(slot) => slot,
            Err(e) => {
                eprintln!("  ✗ Failed to get current slot: {}", e);
                return Ok(());
            }
        };

        println!("  Current slot: {}", current_slot);

        // Simulate upcoming leaders (in production, get from leader schedule)
        let upcoming_leaders: Vec<(Pubkey, u64)> = validators
            .iter()
            .take(3)
            .enumerate()
            .map(|(i, pubkey)| (*pubkey, current_slot + 2 + i as u64))
            .collect();

        println!("\n  Upcoming leaders (simulated):");
        for (pubkey, slot) in &upcoming_leaders {
            println!("    Slot {}: {}", slot, pubkey);
        }
        println!();

        // Step 6: Pre-warm connections
        println!("Step 6: Pre-warming connections...");
        manager.prewarm_connections(&upcoming_leaders).await;
        let connection_count = manager.connection_count().await;
        println!(
            "  Connection pool: {} connections established\n",
            connection_count
        );

        // Step 7: Get connection for specific leader
        println!("Step 7: Getting connection for first leader...");
        let first_leader = upcoming_leaders[0].0;
        match manager.get_connection(&first_leader).await {
            Ok(connection) => {
                println!("  ✓ Connection established to {}", first_leader);
                if let Ok(contact_info) = leader_resolver.get_contact_info(&first_leader).await {
                    println!("  Remote address: {}", contact_info.tpu_quic);
                }

                // Check connection status
                if connection.close_reason().is_none() {
                    println!("  Connection status: OPEN");
                } else {
                    println!("  Connection status: CLOSED");
                }
                println!();
            }
            Err(e) => {
                eprintln!("  ✗ Failed to establish connection: {}", e);
                eprintln!("  This may happen if validators are unreachable\n");
            }
        }

        // Step 8: Connection pool management
        println!("Step 8: Connection pool management");
        println!("  Total connections: {}", manager.connection_count().await);
        println!("  Closing specific connection...");
        manager.close_connection(&first_leader).await;
        println!(
            "  Remaining connections: {}",
            manager.connection_count().await
        );
        println!();

        // Step 9: Cleanup
        println!("Step 9: Cleanup");
        manager.close_all_connections().await;
        println!(
            "  All connections closed. Pool size: {}",
            manager.connection_count().await
        );
    } else {
        println!("  ✗ Not enough validators found for example");
    }

    println!("\n=== Example Complete ===");
    Ok(())
}
