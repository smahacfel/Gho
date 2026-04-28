//! Scenario Revolver: E2E Tests for Revolver Module
//!
//! This scenario tests the complete Revolver workflow:
//! 1. BUY → Creating a Revolver with bullets
//! 2. Worker → Refreshing bullets (blockhash updates)
//! 3. Price signal → Firing bullets when target price is reached
//!
//! Tests validate:
//! - Bullets are created with non-empty tx_bytes
//! - Bullets are refreshed by the worker
//! - Bullets fire when current_price >= target_price
//! - Bullets are removed from magazine after firing

use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::info;

use crate::config::E2EConfig;
use crate::metrics::E2EMetrics;
use crate::scenarios::{LatencyMetrics, ScenarioResult, TestScenario};
use trigger::{Bullet, PriceTarget, Revolver};

/// Mock BlockhashProvider for testing without RPC
pub struct MockBlockhashProvider {
    current_blockhash: String,
}

impl MockBlockhashProvider {
    pub fn new() -> Self {
        Self {
            current_blockhash: "MockBlockhash1111111111111111111111111111".to_string(),
        }
    }

    pub fn get_blockhash(&self) -> String {
        self.current_blockhash.clone()
    }

    pub fn update_blockhash(&mut self, new_hash: String) {
        self.current_blockhash = new_hash;
    }
}

/// Mock SellTxBuilder for testing without actual transaction signing
pub struct MockSellTxBuilder {
    mint: Pubkey,
    position_size: u64,
}

impl MockSellTxBuilder {
    pub fn new(mint: Pubkey, position_size: u64) -> Self {
        Self {
            mint,
            position_size,
        }
    }

    /// Build a mock SELL transaction for the given target price and fraction
    pub fn build_sell_tx(
        &self,
        target_price: u64,
        position_fraction_bps: u16,
        blockhash: &str,
    ) -> Result<Vec<u8>> {
        // Create mock transaction bytes
        // In real implementation, this would create a VersionedTransaction
        let tx_data = format!(
            "MOCK_TX:mint={},target={},fraction={},hash={}",
            self.mint, target_price, position_fraction_bps, blockhash
        );

        Ok(tx_data.into_bytes())
    }
}

/// Mock TPU client for testing without network I/O
pub struct MockTpuClient {
    sent_transactions: Arc<RwLock<Vec<String>>>,
}

impl MockTpuClient {
    pub fn new() -> Self {
        Self {
            sent_transactions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn send_transaction(&self, tx_bytes: &[u8]) -> Result<String> {
        let tx_str = String::from_utf8_lossy(tx_bytes).to_string();
        let signature = format!("MockSig{}", self.sent_transactions.read().await.len());

        self.sent_transactions.write().await.push(tx_str);

        Ok(signature)
    }

    pub async fn get_sent_count(&self) -> usize {
        self.sent_transactions.read().await.len()
    }
}

/// Scenario Revolver: Test Revolver E2E flow
pub struct ScenarioRevolver {
    config: E2EConfig,
}

impl ScenarioRevolver {
    pub fn new(config: E2EConfig) -> Self {
        Self { config }
    }

    /// Create a magazine with test bullets
    fn create_test_magazine(
        &self,
        mint: Pubkey,
        entry_price: u64,
        position_size: u64,
        blockhash_provider: &MockBlockhashProvider,
    ) -> Result<Vec<Bullet>> {
        let tx_builder = MockSellTxBuilder::new(mint, position_size);
        let blockhash = blockhash_provider.get_blockhash();

        // Create bullets at different price targets
        let targets = vec![
            PriceTarget::new(2.0, 2500)?, // 25% at 2x
            PriceTarget::new(3.0, 2500)?, // 25% at 3x
            PriceTarget::new(5.0, 5000)?, // 50% at 5x
        ];

        let mut bullets = Vec::new();
        for target in targets {
            let target_price = target.calculate_target_price(entry_price);
            let tx_bytes =
                tx_builder.build_sell_tx(target_price, target.position_fraction_bps, &blockhash)?;

            let bullet = Bullet::new(tx_bytes, target_price, target.position_fraction_bps)?;

            bullets.push(bullet);
        }

        Ok(bullets)
    }

    /// Simulate worker refreshing bullets
    async fn simulate_worker_refresh(
        &self,
        revolver: &mut Revolver,
        mint: Pubkey,
        position_size: u64,
        blockhash_provider: &mut MockBlockhashProvider,
    ) -> Result<usize> {
        // Update blockhash to simulate passage of time
        blockhash_provider
            .update_blockhash("MockBlockhash2222222222222222222222222222".to_string());

        let tx_builder = MockSellTxBuilder::new(mint, position_size);
        let new_blockhash = blockhash_provider.get_blockhash();

        let token_revolver = revolver
            .get_revolver_mut(&mint)
            .ok_or_else(|| anyhow::anyhow!("No revolver for mint"))?;

        let mut refreshed_count = 0;
        for bullet in &mut token_revolver.bullets {
            // Refresh each bullet with new blockhash
            let new_tx_bytes = tx_builder.build_sell_tx(
                bullet.target_price,
                bullet.position_fraction_bps,
                &new_blockhash,
            )?;

            bullet.update_tx(new_tx_bytes);
            refreshed_count += 1;
        }

        Ok(refreshed_count)
    }

    /// Simulate firing bullets at target price using mock TPU client
    async fn simulate_fire_bullets(
        &self,
        revolver: &mut Revolver,
        mint: Pubkey,
        current_price: u64,
        tpu_client: &MockTpuClient,
    ) -> Result<Vec<MockShotResult>> {
        let token_revolver = revolver
            .get_revolver_mut(&mint)
            .ok_or_else(|| anyhow::anyhow!("No revolver for mint"))?;

        // Find bullets that should fire
        let bullet_indices = token_revolver.check_targets(current_price);

        if bullet_indices.is_empty() {
            return Ok(vec![]);
        }

        // Take bullets to fire
        let bullets = token_revolver.take_bullets(&bullet_indices);

        // Fire each bullet
        let mut results = Vec::new();
        for bullet in bullets {
            let signature = tpu_client.send_transaction(&bullet.tx_bytes).await?;
            results.push(MockShotResult { bullet, signature });
        }

        Ok(results)
    }
}

/// Mock shot result for testing
pub struct MockShotResult {
    pub bullet: Bullet,
    pub signature: String,
}

impl TestScenario for ScenarioRevolver {
    fn name(&self) -> &str {
        "Scenario Revolver: E2E Revolver Tests"
    }

    async fn run(&self, _config: &E2EConfig, metrics: Arc<E2EMetrics>) -> Result<ScenarioResult> {
        info!("Running Scenario Revolver: E2E Tests");

        let mut result = ScenarioResult::new(self.name());
        let start_time = Instant::now();

        // === TEST 1: BUY → Create Revolver ===
        info!("TEST 1: Creating Revolver with magazine");

        let mint = Pubkey::new_unique();
        let entry_price = 1000_u64; // 1000 lamports per token
        let position_size = 10_000_000_u64; // 10M tokens

        let mut blockhash_provider = MockBlockhashProvider::new();

        // Create magazine
        let bullets =
            self.create_test_magazine(mint, entry_price, position_size, &blockhash_provider)?;

        // Verify bullets are created with non-empty tx_bytes
        assert_eq!(bullets.len(), 3, "Should create 3 bullets");
        for (i, bullet) in bullets.iter().enumerate() {
            assert!(
                !bullet.tx_bytes.is_empty(),
                "Bullet {} should have non-empty tx_bytes",
                i
            );
            info!(
                "Bullet {}: target_price={}, fraction_bps={}, tx_bytes_len={}",
                i,
                bullet.target_price,
                bullet.position_fraction_bps,
                bullet.tx_bytes.len()
            );
        }

        result.add_observation(format!(
            "Created {} bullets with non-empty tx_bytes",
            bullets.len()
        ));

        // Load into revolver
        let mut revolver = Revolver::new();
        revolver.load_magazine(mint, bullets);

        assert_eq!(
            revolver.total_bullet_count(),
            3,
            "Revolver should have 3 bullets"
        );
        result.add_observation("Revolver loaded with 3 bullets".to_string());

        // === TEST 2: Worker → Refresh Bullets ===
        info!("TEST 2: Worker refreshing bullets");

        let refresh_start = Instant::now();
        let refreshed_count = self
            .simulate_worker_refresh(&mut revolver, mint, position_size, &mut blockhash_provider)
            .await?;
        let refresh_latency = refresh_start.elapsed().as_millis() as f64;

        assert_eq!(refreshed_count, 3, "Should refresh all 3 bullets");

        // Verify bullets still have non-empty tx_bytes after refresh
        let token_revolver = revolver.get_revolver(&mint).unwrap();
        for (i, bullet) in token_revolver.bullets.iter().enumerate() {
            assert!(
                !bullet.tx_bytes.is_empty(),
                "Bullet {} should still have non-empty tx_bytes after refresh",
                i
            );
            // Verify the tx_bytes contain the new blockhash
            let tx_str = String::from_utf8_lossy(&bullet.tx_bytes);
            assert!(
                tx_str.contains("MockBlockhash2222"),
                "Bullet {} should have updated blockhash",
                i
            );
        }

        result.add_observation(format!(
            "Worker refreshed {} bullets in {:.2}ms",
            refreshed_count, refresh_latency
        ));

        // === TEST 3: Price Signal → Fire Bullets ===
        info!("TEST 3: Firing bullets at target price");

        let tpu_client = MockTpuClient::new();

        // Scenario 3a: Price below all targets - no bullets should fire
        info!("TEST 3a: Price below targets");
        let current_price = 1500_u64; // 1.5x (below 2x target)
        let fired = self
            .simulate_fire_bullets(&mut revolver, mint, current_price, &tpu_client)
            .await?;

        assert_eq!(fired.len(), 0, "No bullets should fire at 1.5x");
        assert_eq!(
            revolver.total_bullet_count(),
            3,
            "All bullets should remain"
        );
        result.add_observation("No bullets fired at price 1.5x (below 2x target)".to_string());

        // Scenario 3b: Price at 2.5x - first bullet should fire
        info!("TEST 3b: Price reaches 2.5x");
        let current_price = 2500_u64; // 2.5x
        let fire_start = Instant::now();
        let fired = self
            .simulate_fire_bullets(&mut revolver, mint, current_price, &tpu_client)
            .await?;
        let fire_latency = fire_start.elapsed().as_millis() as f64;

        assert_eq!(fired.len(), 1, "One bullet should fire at 2.5x");
        assert_eq!(
            fired[0].bullet.target_price, 2000,
            "Should fire 2x target bullet"
        );
        assert_eq!(
            revolver.total_bullet_count(),
            2,
            "Two bullets should remain"
        );

        let sent_count = tpu_client.get_sent_count().await;
        assert_eq!(sent_count, 1, "One transaction should be sent");

        result.add_observation(format!(
            "Fired 1 bullet at price 2.5x in {:.2}ms, signature: {}",
            fire_latency, fired[0].signature
        ));

        // Scenario 3c: Price at 10x - remaining bullets should fire
        info!("TEST 3c: Price reaches 10x");
        let current_price = 10000_u64; // 10x
        let fired = self
            .simulate_fire_bullets(&mut revolver, mint, current_price, &tpu_client)
            .await?;

        assert_eq!(fired.len(), 2, "Two bullets should fire at 10x");
        assert_eq!(revolver.total_bullet_count(), 0, "Magazine should be empty");

        let sent_count = tpu_client.get_sent_count().await;
        assert_eq!(sent_count, 3, "Three transactions total should be sent");

        result.add_observation(format!(
            "Fired 2 more bullets at price 10x, total transactions: {}",
            sent_count
        ));

        // === Update metrics ===
        metrics
            .seer_pools_detected
            .with_label_values(&["revolver_test"])
            .inc();

        // Calculate overall results
        let e2e_latency = start_time.elapsed().as_millis() as f64;
        result.avg_latencies = LatencyMetrics {
            seer_to_oracle: None,
            oracle_scoring: Some(refresh_latency),
            trigger_send: Some(fire_latency),
            trigger_confirm: None,
            e2e_total: Some(e2e_latency),
        };

        result.passed = true;
        result.land_rate = 100.0; // All bullets were created successfully
        result.inclusion_rate = 100.0; // All bullets fired successfully

        info!(
            "Scenario Revolver completed successfully in {:.2}ms",
            e2e_latency
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_blockhash_provider() {
        let mut provider = MockBlockhashProvider::new();
        let hash1 = provider.get_blockhash();
        assert!(hash1.contains("MockBlockhash1111"));

        provider.update_blockhash("NewHash".to_string());
        let hash2 = provider.get_blockhash();
        assert_eq!(hash2, "NewHash");
    }

    #[test]
    fn test_mock_sell_tx_builder() {
        let mint = Pubkey::new_unique();
        let builder = MockSellTxBuilder::new(mint, 1000000);

        let tx_bytes = builder.build_sell_tx(2000, 2500, "TestHash").unwrap();

        assert!(!tx_bytes.is_empty());
        let tx_str = String::from_utf8(tx_bytes).unwrap();
        assert!(tx_str.contains("MOCK_TX"));
        assert!(tx_str.contains("target=2000"));
        assert!(tx_str.contains("fraction=2500"));
        assert!(tx_str.contains("hash=TestHash"));
    }

    #[tokio::test]
    async fn test_mock_tpu_client() {
        let client = MockTpuClient::new();

        let tx_bytes = b"test_transaction";
        let sig = client.send_transaction(tx_bytes).await.unwrap();

        assert!(sig.starts_with("MockSig"));
        assert_eq!(client.get_sent_count().await, 1);
    }

    #[tokio::test]
    async fn test_scenario_revolver_integration() {
        // Setup - create a minimal test config
        use solana_sdk::pubkey::Pubkey;

        let config = E2EConfig {
            rpc_url: "http://localhost:8899".to_string(),
            websocket_url: "ws://localhost:8900".to_string(),
            authority_keypair_path: "test".to_string(),
            payer_keypair_path: "test".to_string(),
            seer: crate::config::SeerConfig {
                enable_pumpfun: true,
                enable_bonkfun: true,
                min_liquidity_sol: Some(5.0),
                max_reconnect_attempts: 3,
                reconnect_delay_secs: 5,
                verbose: false,
            },
            oracle: crate::config::OracleConfig {
                min_score_threshold: 70,
                enable_anomaly_detection: false,
                rpc_endpoints: vec![],
            },
            features: crate::config::FeaturesConfig {
                default_strategy: "test".to_string(),
                max_position_size_lamports: 1000000,
                max_slippage: 0.05,
                intent_timeout_secs: 3600,
            },
            trigger: crate::config::TriggerConfig {
                redundancy_factor: 3,
                max_span_slots: 4,
                enable_jito: false,
                jito_block_engine_url: None,
                dry_run: true,
                max_concurrent_positions: Some(3),
                enable_leapfrog: false,
                leapfrog_redundancy: 2,
                leapfrog_use_quic: false,
            },
            metrics: crate::config::MetricsConfig {
                enable_prometheus: false,
                prometheus_port: 9090,
                target_land_rate: 100.0,
                target_inclusion_rate: 92.0,
            },
            gui_backend: crate::config::GuiBackendConfig {
                enabled: false,
                port: 8080,
                bind_address: "localhost".to_string(),
            },
            leader_predictor: crate::config::LeaderPredictorConfig {
                enabled: false,
                grpc_endpoint: "test".to_string(),
                our_leaders: vec![],
                verbose: false,
            },
            intelligence: crate::config::IntelligenceConfig {
                enable_vision: false,
                vision_provider: "openai".to_string(),
                vision_api_key: None,
                openai_model: "gpt-4o-mini".to_string(),
                anthropic_model: "claude-3-haiku-20240307".to_string(),
                max_cluster_size: 20,
                min_cluster_size: 3,
                high_risk_threshold_pct: 30.0,
                max_signatures: 10,
                serial_minter_threshold: 5,
                serial_minter_window_hours: 24,
                rpc_timeout_secs: 10,
                vision_api_timeout_secs: 30,
            },
            execution: crate::config::ExecutionConfig::default(),
        };

        let metrics = Arc::new(E2EMetrics::new());
        let scenario = ScenarioRevolver::new(config.clone());

        // Run scenario
        let result = scenario.run(&config, metrics).await;

        // Verify
        assert!(result.is_ok(), "Scenario should complete successfully");
        let result = result.unwrap();
        assert!(result.passed, "Scenario should pass all tests");
        assert_eq!(result.land_rate, 100.0);
        assert_eq!(result.inclusion_rate, 100.0);
        assert!(!result.observations.is_empty());
    }
}
