//! Test server that runs indefinitely for UI testing
//!
//! Run with: cargo run --example test_server

use gui_backend::{GuiBackend, GuiBackendConfig, Portfolio, Position, Settings};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    println!("Starting GUI Backend Test Server");
    println!("=================================\n");

    // Create backend configuration
    let config = GuiBackendConfig {
        port: 8800,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    // Create backend
    let backend = GuiBackend::new(config);
    let state = backend.state();

    println!("Server available at:");
    println!("  Dashboard: http://127.0.0.1:8800");
    println!("  REST API: http://127.0.0.1:8800");
    println!("  WebSocket: ws://127.0.0.1:8800/ws");
    println!("\nPress Ctrl+C to stop\n");

    // Start server in background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = backend.run().await {
            eprintln!("Server error: {}", e);
        }
    });

    // Simulate portfolio updates continuously
    let state_clone = Arc::clone(&state);
    let update_handle = tokio::spawn(async move {
        sleep(Duration::from_secs(2)).await;

        println!("Initializing with test data...");

        // Set initial settings
        let settings = Settings {
            position_size_lamports: 100_000_000, // 0.1 SOL
            jito_tip_lamports: 10_000,
            max_slippage: 0.01,
            enable_jito: false,
            auto_jito_tip: true,
        };
        state_clone.update_settings(settings);
        println!("✓ Settings initialized");

        // Set initial portfolio
        let portfolio = Portfolio {
            sol_balance: 10_000_000_000, // 10 SOL
            positions: vec![Position {
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
                amount: 1_000_000,
                entry_price: 50_000_000,
                current_price: Some(52_000_000),
                pnl: 2_000_000,
                opened_at: 1700000000,
            }],
            total_value: 10_052_000_000,
            total_pnl: 52_000_000,
        };
        state_clone.update_portfolio(portfolio);
        println!("✓ Portfolio initialized");

        // Update transaction stats
        state_clone.update_transaction_stats(50, 48);
        println!("✓ Transaction stats initialized");

        println!("\n✅ Test server ready! Open http://127.0.0.1:8800 in your browser\n");

        // Keep updating periodically
        loop {
            sleep(Duration::from_secs(10)).await;

            // Simulate price changes
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let price = 50_000_000 + (current_time % 10_000_000) as u64;
            let pnl = (price as i64) - 50_000_000;

            let portfolio = Portfolio {
                sol_balance: 10_000_000_000,
                positions: vec![Position {
                    mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                    amount: 1_000_000,
                    entry_price: 50_000_000,
                    current_price: Some(price),
                    pnl,
                    opened_at: 1700000000,
                }],
                total_value: 10_000_000_000 + pnl as u64,
                total_pnl: pnl,
            };
            state_clone.update_portfolio(portfolio);
        }
    });

    // Wait for Ctrl+C
    tokio::select! {
        _ = server_handle => {
            println!("Server stopped");
        }
        _ = update_handle => {
            println!("Updates stopped");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down...");
        }
    }

    Ok(())
}
