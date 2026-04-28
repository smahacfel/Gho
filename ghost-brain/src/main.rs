//! Ghost E2E Pipeline Runner
//!
//! Main executable for running the end-to-end Ghost pipeline on devnet.
//! Uses Zero-Cost Direct AMM Interaction via DirectBuyBuilder.

use anyhow::Result;
use ghost_brain::{E2EConfig, E2EPipeline};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_e2e=info,seer=info,trigger=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("===========================================");
    info!("   Ghost E2E Pipeline - Zero-Cost Mode");
    info!("   Direct AMM Interaction (Off-chain only)");
    info!("===========================================");

    // Load configuration
    info!("Loading configuration from environment...");
    let config = match E2EConfig::from_env() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            error!("Make sure .env.devnet file exists or environment variables are set");
            error!("Required variables:");
            error!("  - RPC_URL_DEVNET");
            error!("  - WEBSOCKET_URL_DEVNET");
            error!("  - AUTHORITY_KEYPAIR_PATH (optional, defaults to ~/.config/solana/id.json)");
            error!("  - PAYER_KEYPAIR_PATH (optional, defaults to ~/.config/solana/id.json)");
            std::process::exit(1);
        }
    };

    // Validate configuration
    info!("Validating configuration...");
    if let Err(e) = config.validate() {
        error!("Configuration validation failed: {}", e);
        std::process::exit(1);
    }

    info!("Configuration loaded successfully");
    info!("  RPC URL: {}", config.rpc_url);
    info!("  WebSocket URL: {}", config.websocket_url);
    info!("  Mode: Zero-Cost (Direct Pump.fun AMM)");
    info!("  Pump.fun enabled: {}", config.seer.enable_pumpfun);
    info!("  Bonk.fun enabled: {}", config.seer.enable_bonkfun);
    info!("  Oracle min score: {}", config.oracle.min_score_threshold);
    info!(
        "  Max position size: {} lamports",
        config.features.max_position_size_lamports
    );
    info!(
        "  Max slippage: {:.2}%",
        config.features.max_slippage * 100.0
    );
    info!(
        "  Redundancy factor: N+{}",
        config.trigger.redundancy_factor
    );
    info!("  Jito enabled: {}", config.trigger.enable_jito);
    info!(
        "  Target Land Rate: {:.1}%",
        config.metrics.target_land_rate
    );
    info!(
        "  Target Inclusion Rate: {:.1}%",
        config.metrics.target_inclusion_rate
    );

    // Create pipeline
    info!("Creating E2E pipeline...");
    let pipeline = match E2EPipeline::new(config) {
        Ok(pipeline) => pipeline,
        Err(e) => {
            error!("Failed to create pipeline: {}", e);
            std::process::exit(1);
        }
    };

    info!("Pipeline created successfully");
    info!("");
    info!("Starting pipeline...");
    info!("Press Ctrl+C to stop");
    info!("===========================================");
    info!("");

    // Run pipeline
    if let Err(e) = pipeline.run().await {
        error!("Pipeline error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
