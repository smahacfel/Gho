//! Simple example demonstrating the GUI backend
//!
//! This example starts the GUI backend server and demonstrates
//! basic API usage.
//!
//! Run with: cargo run --example simple_server

use gui_backend::{GuiBackend, GuiBackendConfig, Portfolio, Position, Settings};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    println!("Starting GUI Backend Example");
    println!("==============================\n");

    // Create backend configuration
    let config = GuiBackendConfig {
        port: 8800,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    // Create backend
    let backend = GuiBackend::new(config);
    let state = backend.state();

    println!("Server will be available at:");
    println!("  REST API: http://127.0.0.1:8800");
    println!("  WebSocket: ws://127.0.0.1:8800/ws");
    println!("\nAvailable endpoints:");
    println!("  GET  /health");
    println!("  GET  /status");
    println!("  GET  /portfolio");
    println!("  GET  /settings");
    println!("  POST /settings");
    println!("  POST /control/pause");
    println!("  POST /control/resume");
    println!("  POST /control/stop");
    println!("  WS   /ws");
    println!("\nPress Ctrl+C to stop\n");

    // Start server in background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = backend.run().await {
            eprintln!("Server error: {}", e);
        }
    });

    // Simulate portfolio updates
    let state_clone = Arc::clone(&state);
    let update_handle = tokio::spawn(async move {
        sleep(Duration::from_secs(2)).await;

        println!("Simulating portfolio updates...");

        // Update settings
        let new_settings = Settings {
            position_size_lamports: 200_000_000, // 0.2 SOL
            jito_tip_lamports: 20_000,
            max_slippage: 0.02,
            enable_jito: true,
            auto_jito_tip: false,
        };
        state_clone.update_settings(new_settings);
        println!("✓ Updated settings");

        sleep(Duration::from_secs(2)).await;

        // Update portfolio with a position
        let portfolio = Portfolio {
            sol_balance: 5_000_000_000, // 5 SOL
            positions: vec![Position {
                mint: "TokenMint1111111111111111111111111111111".to_string(),
                amount: 1_000_000,
                entry_price: 50_000,
                current_price: Some(55_000),
                pnl: 5_000,
                opened_at: 1700000000,
            }],
            total_value: 5_100_000_000,
            total_pnl: 100_000_000,
        };
        state_clone.update_portfolio(portfolio);
        println!("✓ Updated portfolio with 1 position");

        sleep(Duration::from_secs(2)).await;

        // Update transaction stats
        state_clone.update_transaction_stats(100, 95);
        println!("✓ Updated transaction stats (sent: 100, confirmed: 95)");

        sleep(Duration::from_secs(3)).await;

        // Demonstrate pause/resume
        println!("\nDemonstrating system control...");
        state_clone.set_mode(gui_backend::SystemMode::Paused);
        println!("✓ System PAUSED");

        sleep(Duration::from_secs(2)).await;

        state_clone.set_mode(gui_backend::SystemMode::Running);
        println!("✓ System RESUMED");

        println!("\nExample running. Try these commands in another terminal:");
        println!("\n# Get status");
        println!("curl http://127.0.0.1:8800/status");
        println!("\n# Get portfolio");
        println!("curl http://127.0.0.1:8800/portfolio");
        println!("\n# Update settings");
        println!("curl -X POST http://127.0.0.1:8800/settings \\");
        println!("  -H 'Content-Type: application/json' \\");
        println!("  -d '{{\"position_size_lamports\": 300000000}}'");
        println!("\n# Pause system");
        println!("curl -X POST http://127.0.0.1:8800/control/pause");
        println!("\n# WebSocket (using websocat or similar)");
        println!("websocat ws://127.0.0.1:8800/ws");
        println!("\nPress Ctrl+C to stop");
    });

    // Wait for either task to complete (or Ctrl+C)
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
