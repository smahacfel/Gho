//! Trigger - Ghost Transaction Sender
//!
//! Main entrypoint for the Trigger component that builds and sends
//! Ghost Transactions with N+3 redundancy and LUT optimization.

use anyhow::Result;
use prometheus::Registry;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::{Keypair, Signer};
use std::sync::Arc;
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use trigger::{
    load_payer_keypair, AmmAccounts, AmmType, BundleConfig, GhostTransactionBuilder, JitoClient,
    JitoClientBuilder, LutConfig, TpuClient, TriggerMetrics,
};

/// Trigger configuration
#[derive(Debug, Clone)]
struct TriggerConfig {
    /// RPC endpoint URL
    rpc_url: String,
    /// Whether to use Jito bundles
    use_jito: bool,
    /// Jito endpoint (if using Jito)
    jito_endpoint: Option<String>,
    /// Bundle configuration (if using Jito)
    bundle_config: BundleConfig,
    /// Redundancy count for N+3 (traditional TPU sending)
    redundancy_count: usize,
    /// Metrics port
    metrics_port: u16,
    /// Slippage tolerance used when deriving min_amount_out (0.0 - 1.0)
    slippage_tolerance: f64,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://api.devnet.solana.com".to_string(),
            use_jito: false,
            jito_endpoint: None,
            bundle_config: BundleConfig::default(),
            redundancy_count: 3, // N+3
            metrics_port: 9091,
            slippage_tolerance: 0.15,
        }
    }
}

/// Trigger service
struct TriggerService {
    config: TriggerConfig,
    tpu_client: Option<TpuClient>,
    jito_client: Option<JitoClient>,
    metrics: Arc<TriggerMetrics>,
    rpc_client: Arc<RpcClient>,
    payer: Option<Keypair>,
    #[allow(dead_code)]
    lut_config: LutConfig,
}

impl TriggerService {
    /// Create a new Trigger service
    async fn new(config: TriggerConfig) -> Result<Self> {
        info!("Initializing Trigger service");

        // Initialize metrics
        let metrics = Arc::new(TriggerMetrics::new());

        // Initialize async RPC client for blockhash fetching
        let rpc_client = Arc::new(RpcClient::new(config.rpc_url.clone()));

        // Load payer keypair from environment (optional - may not be configured)
        let payer = match load_payer_keypair() {
            Ok(keypair) => {
                info!("Loaded payer wallet: {}", keypair.pubkey());
                Some(keypair)
            }
            Err(e) => {
                warn!(
                    "Payer keypair not configured: {}. Transaction signing will not be available.",
                    e
                );
                None
            }
        };

        // Initialize TPU client for explicit non-Jito mode.
        let tpu_client = Some(TpuClient::new(
            config.rpc_url.clone(),
            Some(config.redundancy_count),
        )?);

        // Initialize Jito client if configured
        let jito_client = if config.use_jito {
            let endpoint = config
                .jito_endpoint
                .clone()
                .unwrap_or_else(|| "https://mainnet.block-engine.jito.wtf".to_string());
            info!("Initializing Jito client with endpoint: {}", endpoint);
            info!(
                "  Redundancy Policy: {:?}",
                config.bundle_config.redundancy_policy
            );
            info!(
                "  Tip Base: {:.1}%",
                config.bundle_config.tip_config.base_tip_percent * 100.0
            );
            info!(
                "  Tip Dynamic: {:.1}%",
                config.bundle_config.tip_config.dynamic_tip_percent * 100.0
            );
            info!(
                "  Tip Max: {:.1}%",
                config.bundle_config.tip_config.max_tip_percent * 100.0
            );
            info!("  Nonce Staggering: {}", config.bundle_config.stagger_nonce);
            Some(
                JitoClientBuilder::new()
                    .with_endpoint(endpoint)
                    .with_bundle_config(config.bundle_config.clone())
                    .build()?,
            )
        } else {
            None
        };

        // Load LUT configuration
        let lut_config = LutConfig::new();

        info!("Trigger service initialized");
        info!("  RPC: {}", config.rpc_url);
        info!("  Redundancy: N+{}", config.redundancy_count);
        info!(
            "  Jito: {}",
            if config.use_jito {
                "enabled"
            } else {
                "disabled"
            }
        );

        Ok(Self {
            config,
            tpu_client,
            jito_client,
            metrics,
            rpc_client,
            payer,
            lut_config,
        })
    }

    /// Run the Trigger service
    async fn run(&self) -> Result<()> {
        info!("Starting Trigger service");

        // Start metrics server
        let _metrics_server = self.start_metrics_server();

        // Main service loop
        loop {
            // In production, this would:
            // 1. Listen for SwapPlan messages from Oracle/Features
            // 2. Build transactions
            // 3. Send with N+3 redundancy or via Jito
            // 4. Track confirmations
            // 5. Update metrics

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    /// Start Prometheus metrics server
    fn start_metrics_server(&self) -> tokio::task::JoinHandle<Result<()>> {
        let metrics = self.metrics.clone();
        let port = self.config.metrics_port;

        tokio::spawn(async move {
            info!("Starting metrics server on port {}", port);

            // Register metrics
            let registry = Registry::new();
            metrics
                .register(&registry)
                .expect("Failed to register metrics");

            // In production, this would start an HTTP server
            // serving metrics at /metrics endpoint
            // For now, just print metrics periodically
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                let summary = metrics.get_summary();
                info!("Metrics: {}", summary);
            }
        })
    }

    /// Process a swap plan and send transaction
    #[allow(dead_code)]
    async fn process_swap_plan(
        &self,
        swap_plan: ghost_core::SwapPlan,
        amm_type: AmmType,
        amm_accounts: AmmAccounts,
    ) -> Result<solana_sdk::signature::Signature> {
        info!("Processing swap plan: {}", swap_plan.description());

        // Ensure payer is configured
        let payer = self.payer.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Payer keypair not configured. Set WALLET_KEYPAIR_PATH or WALLET_PRIVATE_KEY environment variable."))?;

        // Build transaction
        let builder = GhostTransactionBuilder::new(swap_plan, amm_type, amm_accounts)
            .with_slippage_tolerance(self.config.slippage_tolerance);

        // Get recent blockhash from RPC
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch recent blockhash: {}", e))?;

        // Build transaction
        let tx = builder.build_initialize_intent_tx(payer, recent_blockhash)?;

        // Record send in metrics
        let tx_size = bincode::serialize(&tx)?.len();
        self.metrics
            .record_send(tx_size, self.config.redundancy_count);

        // Send transaction
        let signature = if self.config.use_jito {
            // Use Jito bundle
            if let Some(jito) = &self.jito_client {
                info!("Sending via Jito bundle");
                let receipt = jito.submit_single_transaction(tx).await?;
                self.metrics.record_jito_bundle(true);
                receipt.signature
            } else {
                return Err(anyhow::anyhow!(
                    "Jito enabled but client not initialized; bundle transport is fail-closed and TPU fallback is disabled"
                ));
            }
        } else {
            // Use TPU with N+3 redundancy
            self.send_via_tpu(&tx).await?
        };

        info!("Transaction sent with signature: {}", signature);

        Ok(signature)
    }

    /// Send transaction via TPU with N+3 redundancy
    async fn send_via_tpu(
        &self,
        tx: &solana_sdk::transaction::VersionedTransaction,
    ) -> Result<solana_sdk::signature::Signature> {
        if let Some(tpu) = &self.tpu_client {
            info!(
                "Sending via TPU with N+{} redundancy",
                self.config.redundancy_count
            );
            let signature = tpu.send_transaction_with_redundancy(tx).await?;
            Ok(signature)
        } else {
            anyhow::bail!("TPU client not initialized")
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("=== Ghost Trigger v0.1.0 ===");

    // Load configuration
    let config = TriggerConfig::default();

    // Create and run service
    let service = TriggerService::new(config).await?;
    service.run().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TriggerConfig::default();
        assert_eq!(config.redundancy_count, 3);
        assert!(!config.use_jito);
        assert_eq!(config.metrics_port, 9091);
    }
}
