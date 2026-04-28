//! Portfolio Tracker Integration Example
//!
//! This example demonstrates how to integrate the portfolio tracker with
//! the Ghost trading system to monitor positions and P&L in real-time.

use gui_backend::{PortfolioConfig, PortfolioTracker};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info,gui_backend=debug")
        .init();

    // Connect to Solana RPC
    let rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());

    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    // Get authority pubkey from environment
    let authority_pubkey =
        std::env::var("WALLET_PUBKEY").expect("WALLET_PUBKEY environment variable not set");

    // Create portfolio tracker configuration
    let config = PortfolioConfig {
        refresh_interval_secs: 10, // Refresh every 10 seconds
        authority_pubkey,
    };

    // Create portfolio tracker
    let tracker = Arc::new(PortfolioTracker::new(config, rpc_client)?);

    println!("Portfolio Tracker Example");
    println!("========================\n");

    // Simulate adding some positions (in a real scenario, these would come from confirmed transactions)
    // For demonstration, we'll add a few example positions
    simulate_trading_activity(&tracker).await;

    // Start the portfolio tracker in the background
    let tracker_clone = Arc::clone(&tracker);
    let tracker_handle = tokio::spawn(async move {
        if let Err(e) = tracker_clone.start().await {
            eprintln!("Portfolio tracker error: {}", e);
        }
    });

    // Monitor portfolio for a period
    println!("\nMonitoring portfolio for 60 seconds...\n");

    for i in 1..=6 {
        sleep(Duration::from_secs(10)).await;

        // Get current portfolio state
        let portfolio = tracker.get_portfolio();

        println!("=== Update #{} ===", i);
        println!(
            "SOL Balance: {} lamports ({:.4} SOL)",
            portfolio.sol_balance,
            portfolio.sol_balance as f64 / 1_000_000_000.0
        );
        println!("Active Positions: {}", portfolio.positions.len());
        println!(
            "Total Value: {} lamports ({:.4} SOL)",
            portfolio.total_value_lamports,
            portfolio.total_value_lamports as f64 / 1_000_000_000.0
        );
        println!(
            "Total P&L: {} lamports ({:.4} SOL)",
            portfolio.total_pnl_lamports,
            portfolio.total_pnl_lamports as f64 / 1_000_000_000.0
        );
        println!(
            "Total P&L %: {:.2}%\n",
            portfolio.total_pnl_percentage * 100.0
        );

        // Print individual positions
        if !portfolio.positions.is_empty() {
            println!("Positions:");
            for (mint, position) in &portfolio.positions {
                println!("  Token: {}", mint);
                println!("    Amount: {}", position.amount);
                println!(
                    "    Entry Price: {} lamports",
                    position.entry_price_lamports
                );
                if let Some(current_price) = position.current_price_lamports {
                    println!("    Current Price: {} lamports", current_price);
                    println!(
                        "    P&L: {} lamports ({:.2}%)",
                        position.pnl_lamports,
                        position.pnl_percentage * 100.0
                    );
                } else {
                    println!("    Current Price: Not available");
                }
                println!();
            }
        }
    }

    println!("Monitoring complete. Press Ctrl+C to exit.");

    // Keep running until interrupted
    tokio::signal::ctrl_c().await?;

    tracker_handle.abort();
    println!("\nShutting down...");

    Ok(())
}

/// Simulate trading activity by adding example positions
async fn simulate_trading_activity(tracker: &Arc<PortfolioTracker>) {
    println!("Simulating trading activity...\n");

    // Example: Buy some tokens
    // In a real scenario, these would be parsed from confirmed swap transactions

    // Example mint addresses (these would be real token mints in production)
    let example_mint1 = Pubkey::new_unique();
    let example_mint2 = Pubkey::new_unique();

    println!("Recording buy transaction:");
    println!("  Token: {}", example_mint1);
    println!("  Amount: 1,000,000,000 (1 token with 9 decimals)");
    println!("  Price: 100,000,000 lamports (0.1 SOL)\n");

    tracker.record_buy(
        example_mint1,
        1_000_000_000, // 1 token with 9 decimals
        100_000_000,   // 0.1 SOL per token
    );

    println!("Recording buy transaction:");
    println!("  Token: {}", example_mint2);
    println!("  Amount: 500,000,000 (0.5 tokens)");
    println!("  Price: 50,000,000 lamports (0.05 SOL)\n");

    tracker.record_buy(
        example_mint2,
        500_000_000, // 0.5 tokens
        50_000_000,  // 0.05 SOL per token
    );

    println!("Positions recorded. Portfolio tracker will fetch current prices periodically.\n");
}
