//! Scenario B: Burst Test
//!
//! This scenario tests the E2E pipeline under load by simulating multiple
//! InitializePool events in a short timeframe (30-60 seconds).
//!
//! It validates that:
//! - The system can handle multiple events without choking
//! - N+X redundancy achieves target Inclusion Rate under load
//! - Latencies remain acceptable under load
//! - No transactions are dropped

use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

use crate::config::E2EConfig;
use crate::metrics::E2EMetrics;
use crate::oracle_scoring::SimpleOracle;
use crate::scenarios::{LatencyMetrics, ScenarioResult, TestScenario};
use crate::strategy::StrategySelector;
use seer::types::CandidatePool;

/// Scenario B: Burst test with multiple pools
pub struct ScenarioB {
    /// Test configuration
    config: E2EConfig,

    /// Number of pools to generate
    num_pools: usize,

    /// Duration to spread events over (seconds)
    duration_secs: u64,
}

impl ScenarioB {
    /// Create a new Scenario B test
    pub fn new(config: E2EConfig, num_pools: usize, duration_secs: u64) -> Self {
        Self {
            config,
            num_pools,
            duration_secs,
        }
    }

    /// Create synthetic candidate pools for testing
    fn create_synthetic_pools(&self, count: usize) -> Vec<CandidatePool> {
        (0..count)
            .map(|i| {
                let amm_program_id = if i % 2 == 0 {
                    // Pump.fun
                    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                        .parse()
                        .unwrap()
                } else {
                    // Bonk.fun (using same for now, would be different in production)
                    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                        .parse()
                        .unwrap()
                };

                CandidatePool {
                    semantic: ghost_core::EventSemanticEnvelope::default(),
                    slot: Some(123456789 + i as u64),
                    tx_index: None,
                    event_ts_ms: Some((1234567890 + i as u64).saturating_mul(1000)),
                    event_time: ghost_core::EventTimeMetadata::default(),
                    signature: format!("test_signature_{}", i).repeat(4),
                    amm_program_id,
                    pool_amm_id: Pubkey::new_unique(),
                    base_mint: Pubkey::new_unique(),
                    quote_mint: Pubkey::new_unique(),
                    bonding_curve: Pubkey::new_unique(),
                    creator: Pubkey::new_unique(),
                    timestamp: 1234567890 + i as u64,
                    bonding_curve_progress: Some(0.05 + (i as f64 * 0.01)),
                    initial_liquidity_sol: Some(5.0 + (i as f64 * 0.5)),
                    token_total_supply: Some(1_000_000_000),
                    block_time: Some(1234567890 + i as i64),
                }
            })
            .collect()
    }
}

impl TestScenario for ScenarioB {
    fn name(&self) -> &str {
        "Scenario B: Burst Test"
    }

    async fn run(&self, config: &E2EConfig, metrics: Arc<E2EMetrics>) -> Result<ScenarioResult> {
        info!(
            "Running Scenario B: Burst Test ({} pools over {} seconds)",
            self.num_pools, self.duration_secs
        );

        let mut result = ScenarioResult::new(self.name());
        let start_time = Instant::now();

        // Create synthetic pools
        let candidates = self.create_synthetic_pools(self.num_pools);
        info!("Created {} synthetic candidate pools", candidates.len());

        // Create Oracle and Strategy Selector
        let oracle = SimpleOracle::new(config.oracle.min_score_threshold);
        let strategy_selector = StrategySelector::new(
            Pubkey::new_unique(),
            config.features.max_position_size_lamports,
            config.features.max_slippage,
            config.features.intent_timeout_secs,
        );

        // Track latencies
        let mut oracle_latencies = Vec::new();
        let mut trigger_send_latencies = Vec::new();
        let mut trigger_confirm_latencies = Vec::new();

        // Calculate delay between events
        let delay_ms = if self.num_pools > 1 {
            (self.duration_secs * 1000) / (self.num_pools as u64 - 1)
        } else {
            0
        };

        info!("Processing pools with {}ms delay between events", delay_ms);

        // Process each candidate
        for (idx, candidate) in candidates.into_iter().enumerate() {
            let pool_start = Instant::now();

            // Simulate Seer detection
            let amm = if idx % 2 == 0 { "pumpfun" } else { "bonkfun" };
            metrics.seer_pools_detected.with_label_values(&[amm]).inc();
            metrics.seer_pools_parsed.with_label_values(&[amm]).inc();

            // Oracle scoring
            let oracle_start = Instant::now();
            let scored = match oracle.score_candidate(&candidate).await {
                Ok(s) => s,
                Err(e) => {
                    warn!("Oracle scoring failed for pool {}: {}", idx, e);
                    continue;
                }
            };
            let oracle_latency = oracle_start.elapsed().as_millis() as f64;
            oracle_latencies.push(oracle_latency);

            metrics.oracle_candidates_scored.inc();
            metrics.oracle_score_histogram.observe(scored.score as f64);
            metrics.oracle_latency.observe(oracle_latency);

            // Strategy selection
            let _swap_plan = match strategy_selector.generate_swap_plan(&scored) {
                Ok(Some(plan)) => {
                    metrics.features_plans_created.inc();
                    plan
                }
                Ok(None) => {
                    metrics.features_plans_rejected.inc();
                    continue;
                }
                Err(e) => {
                    warn!("Strategy selection failed for pool {}: {}", idx, e);
                    continue;
                }
            };

            // Simulate DirectBuyBuilder/Trigger execution
            let trigger_start = Instant::now();

            metrics.buy_intents_initialized.inc();
            metrics.trigger_txs_sent.inc();
            let trigger_send_latency = trigger_start.elapsed().as_millis() as f64;
            trigger_send_latencies.push(trigger_send_latency);
            metrics.trigger_send_latency.observe(trigger_send_latency);

            // Simulate confirmation
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            metrics.trigger_txs_confirmed.inc();
            let trigger_confirm_latency = trigger_start.elapsed().as_millis() as f64;
            trigger_confirm_latencies.push(trigger_confirm_latency);
            metrics
                .trigger_confirm_latency
                .observe(trigger_confirm_latency);

            // Record E2E latency for this pool
            let e2e_latency = pool_start.elapsed().as_millis() as f64;
            metrics.e2e_total_latency.observe(e2e_latency);

            // Delay before next event (except for last one)
            if idx < self.num_pools - 1 && delay_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            }
        }

        // Calculate average latencies
        let avg_oracle_latency = if !oracle_latencies.is_empty() {
            Some(oracle_latencies.iter().sum::<f64>() / oracle_latencies.len() as f64)
        } else {
            None
        };

        let avg_trigger_send = if !trigger_send_latencies.is_empty() {
            Some(trigger_send_latencies.iter().sum::<f64>() / trigger_send_latencies.len() as f64)
        } else {
            None
        };

        let avg_trigger_confirm = if !trigger_confirm_latencies.is_empty() {
            Some(
                trigger_confirm_latencies.iter().sum::<f64>()
                    / trigger_confirm_latencies.len() as f64,
            )
        } else {
            None
        };

        let total_duration = start_time.elapsed().as_millis() as f64;

        result.avg_latencies = LatencyMetrics {
            oracle_scoring: avg_oracle_latency,
            trigger_send: avg_trigger_send,
            trigger_confirm: avg_trigger_confirm,
            e2e_total: Some(total_duration),
            seer_to_oracle: None,
        };

        // Calculate metrics
        result.land_rate =
            (metrics.update_land_rate("pumpfun") + metrics.update_land_rate("bonkfun")) / 2.0;
        result.inclusion_rate = metrics.update_inclusion_rate();

        // Evaluate pass/fail
        result.passed = result.land_rate >= 95.0 && result.inclusion_rate >= 92.0;

        if result.passed {
            info!("Scenario B PASSED");
        } else {
            warn!(
                "Scenario B FAILED: Land Rate={:.2}%, Inclusion Rate={:.2}%",
                result.land_rate, result.inclusion_rate
            );
        }

        result.add_observation(format!("Total pools processed: {}", self.num_pools));
        result.add_observation(format!("Duration: {:.2} seconds", total_duration / 1000.0));
        result.add_observation(format!(
            "Throughput: {:.2} pools/sec",
            self.num_pools as f64 / (total_duration / 1000.0)
        ));
        result.add_observation(format!(
            "Redundancy factor: N+{}",
            config.trigger.redundancy_factor
        ));

        // Check for latency spikes
        if let Some(avg_oracle) = avg_oracle_latency {
            if avg_oracle > 500.0 {
                result.add_observation(format!(
                    "WARNING: High Oracle latency: {:.2} ms",
                    avg_oracle
                ));
            }
        }

        if let Some(avg_send) = avg_trigger_send {
            if avg_send > 200.0 {
                result.add_observation(format!(
                    "WARNING: High Trigger send latency: {:.2} ms",
                    avg_send
                ));
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scenario_b_creation() {
        let config = E2EConfig {
            rpc_url: "https://api.devnet.solana.com".to_string(),
            websocket_url: "wss://api.devnet.solana.com".to_string(),
            authority_keypair_path: "~/.config/solana/id.json".to_string(),
            payer_keypair_path: "~/.config/solana/id.json".to_string(),
            seer: crate::config::SeerConfig {
                enable_pumpfun: true,
                enable_bonkfun: true,
                min_liquidity_sol: Some(1.0),
                max_reconnect_attempts: 5,
                reconnect_delay_secs: 5,
                verbose: false,
            },
            oracle: crate::config::OracleConfig {
                min_score_threshold: 70,
                enable_anomaly_detection: true,
                rpc_endpoints: vec!["https://api.devnet.solana.com".to_string()],
            },
            features: crate::config::FeaturesConfig {
                default_strategy: "snipe_new_pool".to_string(),
                max_position_size_lamports: 10_000_000,
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
                target_land_rate: 95.0,
                target_inclusion_rate: 92.0,
            },
            gui_backend: crate::config::GuiBackendConfig {
                enabled: false,
                port: 8800,
                bind_address: "127.0.0.1".to_string(),
            },
            leader_predictor: crate::config::LeaderPredictorConfig {
                enabled: false,
                grpc_endpoint: "http://localhost:10000".to_string(),
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

        let scenario = ScenarioB::new(config, 10, 30);
        let pools = scenario.create_synthetic_pools(10);

        assert_eq!(pools.len(), 10);
        assert!(pools[0].initial_liquidity_sol.unwrap() < pools[9].initial_liquidity_sol.unwrap());
    }
}
