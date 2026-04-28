//! IRONCLAD Protocol Example
//!
//! This example demonstrates how to use the IRONCLAD Transaction Protocol
//! for secure and time-bounded transaction submission to Jito bundles.
//!
//! The IRONCLAD protocol enforces:
//! - Blockhash freshness (max 200ms fetch time)
//! - Pre-flight simulation (honeypot detection)
//! - TTL enforcement (max 1500ms bundle confirmation)
//! - Strict retry limits (0 for transactions, 2 for bundle submission)
//!
//! Run with:
//! ```bash
//! cargo run --example ironclad_example
//! ```

use anyhow::Result;
use ghost_core::SwapPlan;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{signature::Keypair, signer::Signer};
use std::sync::Arc;
use tracing::{error, info, warn};
use trigger::{
    config::{AmmType, LutConfig},
    config::{BundleConfig, RedundancyPolicy, TipConfig},
    jito_client::JitoClientBuilder,
    transaction_builder::{AmmAccounts, GhostTransactionBuilder},
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("🛡️ IRONCLAD Protocol Example");
    info!("================================");

    // Step 1: Configure RPC client for simulation
    // Important: Set RPC_URL environment variable to avoid using public endpoints
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| {
        warn!("⚠️  RPC_URL not set! Using devnet for safety.");
        warn!("    For mainnet: export RPC_URL=https://your-dedicated-rpc.com");
        warn!("    For devnet:  export RPC_URL=https://api.devnet.solana.com");
        "https://api.devnet.solana.com".to_string()
    });

    info!("Setting up RPC client: {}", rpc_url);
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    // Step 2: Configure Jito client with IRONCLAD protocol
    let jito_endpoint = std::env::var("JITO_ENDPOINT")
        .unwrap_or_else(|_| "https://mainnet.block-engine.jito.wtf/api/v1".to_string());

    let bundle_config = BundleConfig {
        tip_config: TipConfig::default(),
        redundancy_policy: RedundancyPolicy::NPlusThree,
        enable_diagnostics: true,
        stagger_nonce: true,
    };

    info!("Building Jito client with IRONCLAD protocol");
    let jito_client = JitoClientBuilder::new()
        .with_endpoint(jito_endpoint)
        .with_rpc_client(rpc_client.clone()) // 🛡️ Enable IRONCLAD simulation
        .with_bundle_config(bundle_config)
        .with_diagnostics(true)
        .build()?;

    info!("✅ Jito client configured with IRONCLAD protocol");
    info!("");

    // Step 3: IRONCLAD Blockhash Fetch (with freshness check)
    info!("🛡️ PART A: TTL Guard - Fetching fresh blockhash");

    match jito_client.get_fresh_blockhash().await {
        Ok(blockhash) => {
            info!("✅ Fresh blockhash obtained: {}", blockhash);
            info!("   (Fetch time < 200ms requirement met)");
        }
        Err(e) => {
            error!("❌ Blockhash fetch failed: {}", e);
            warn!("   This would abort the transaction in production");
            return Ok(());
        }
    }
    info!("");

    // Step 4: Build example transaction
    info!("Building example swap transaction");

    let payer = Keypair::new();
    let tip_payer = Keypair::new();

    // Create swap plan
    let lut_config = LutConfig::new();
    let swap_plan = SwapPlan::new(
        payer.pubkey(),
        lut_config.pump_fun.program_id,
        1_000_000_000, // 1 SOL
        900_000,       // min amount out
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64
            + 3600,
    );

    let amm_accounts = AmmAccounts {
        pool: solana_sdk::pubkey::Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: None,
        additional_accounts: vec![],
    };

    let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

    // Get fresh blockhash for transaction building
    let blockhash = jito_client.get_fresh_blockhash().await?;
    let tx = builder.build_initialize_intent_tx(&payer, blockhash)?;

    info!("✅ Transaction built");
    info!("");

    // Step 5: IRONCLAD Simulation (honeypot detection)
    info!("🛡️ PART B: Pre-Flight Simulation");

    match jito_client.simulate_transaction_preflight(&tx).await {
        Ok(_) => {
            info!("✅ Simulation passed all checks:");
            info!("   - No errors in simulation result");
            info!("   - No 'insufficient funds' in logs");
            info!("   - No custom errors detected");
            info!("   - Compute units < 400k");
            info!("   - Simulation time < 100ms");
        }
        Err(e) => {
            error!("❌ Simulation failed: {}", e);
            warn!("   Transaction would be ABORTED in production");
            warn!("   Possible honeypot or invalid transaction");
            return Ok(());
        }
    }
    info!("");

    // Step 6: Build bundle
    info!("Building Jito bundle");

    let init_pool_tx = tx.clone();
    let ghost_txs = vec![tx];

    let bundle = jito_client.build_bundle(
        init_pool_tx,
        ghost_txs,
        1_000_000_000, // 1 SOL value
        0.5,           // Medium priority
        blockhash,
        Some(&tip_payer),
    )?;

    info!("✅ Bundle built:");
    info!("   - Bundle ID: {}", bundle.bundle_id);
    info!("   - Transaction count: {}", bundle.transactions.len());
    info!("   - Tip: {} lamports", bundle.tip_lamports);
    info!("");

    // Step 7: IRONCLAD Bundle Submission (with TTL enforcement)
    info!("🛡️ IRONCLAD Bundle Submission");
    info!("   - TTL: 1500ms maximum");
    info!("   - Max retries: 2 (network errors only)");
    info!("   - All transactions pre-simulated");
    info!("");

    // In a real scenario, you would submit with:
    // let signature = jito_client.submit_bundle_ironclad(bundle).await?;

    // For this example, we'll just demonstrate the call structure
    info!("Example call:");
    info!("   jito_client.submit_bundle_ironclad(bundle).await");
    info!("");

    info!("Expected outcomes:");
    info!("   ✅ Success: Bundle lands within 1500ms");
    info!("   ⚠️  TTL Violation: Bundle took > 1500ms (aborted)");
    info!("   ❌ Simulation Failed: Honeypot detected (aborted)");
    info!("   ⚠️  Stale Blockhash: Fetch > 200ms (aborted)");
    info!("");

    // Step 8: Summary
    info!("================================");
    info!("🛡️ IRONCLAD Protocol Summary");
    info!("================================");
    info!("");
    info!("Protection Layers:");
    info!("  1. Blockhash freshness check (< 200ms)");
    info!("  2. Pre-flight simulation (honeypot detection)");
    info!("  3. TTL enforcement (< 1500ms)");
    info!("  4. Reduced retries (max 2 for network errors)");
    info!("");
    info!("Benefits:");
    info!("  ✅ Zero risk of delayed inclusion");
    info!("  ✅ Honeypot detection before submission");
    info!("  ✅ No wasted fees on failed transactions");
    info!("  ✅ Predictable behavior under all conditions");
    info!("");
    info!("See IRONCLAD_PROTOCOL.md for full documentation");

    Ok(())
}
