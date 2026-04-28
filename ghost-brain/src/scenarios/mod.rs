//! E2E Test Scenarios
//!
//! This module contains test scenarios for the Ghost E2E pipeline.
//!
//! ## Scenario A: Single Synthetic Pool Test
//! Tests the pipeline with a single simulated InitializePool event.
//!
//! ## Scenario B: Burst Test
//! Tests the pipeline with multiple pool events in a short timeframe.
//!
//! ## Scenario E2E Full: Real Yellowstone→Jito→DirectBuy→On-chain
//! Complete end-to-end test with real Yellowstone detection and Jito bundles.
//!
//! ## Scenario Enhanced InitializePool: Enhanced Candidate Scoring
//! Tests enhanced candidate scoring with contextual analysis signals.
//!
//! ## Scenario Revolver: Revolver E2E Tests
//! Tests the Revolver workflow: BUY → create bullets, worker → refresh, price → fire.

pub mod scenario_a;
pub mod scenario_b;
pub mod scenario_e2e_full;
pub mod scenario_enhanced_initialize_pool;
pub mod scenario_revolver;

use anyhow::Result;
use std::sync::Arc;

use crate::config::E2EConfig;
use crate::metrics::E2EMetrics;

/// Test scenario result
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Scenario name
    pub name: String,

    /// Whether the scenario passed
    pub passed: bool,

    /// Land rate achieved (%)
    pub land_rate: f64,

    /// Inclusion rate achieved (%)
    pub inclusion_rate: f64,

    /// Average latencies in milliseconds
    pub avg_latencies: LatencyMetrics,

    /// Additional observations
    pub observations: Vec<String>,
}

/// Latency metrics summary
#[derive(Debug, Clone, Default)]
pub struct LatencyMetrics {
    /// Seer detection to Oracle scoring (ms)
    pub seer_to_oracle: Option<f64>,

    /// Oracle scoring duration (ms)
    pub oracle_scoring: Option<f64>,

    /// Trigger send duration (ms)
    pub trigger_send: Option<f64>,

    /// Trigger confirmation duration (ms)
    pub trigger_confirm: Option<f64>,

    /// End-to-end latency (ms)
    pub e2e_total: Option<f64>,
}

impl ScenarioResult {
    /// Create a new scenario result
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: false,
            land_rate: 0.0,
            inclusion_rate: 0.0,
            avg_latencies: LatencyMetrics::default(),
            observations: Vec::new(),
        }
    }

    /// Add an observation
    pub fn add_observation(&mut self, observation: impl Into<String>) {
        self.observations.push(observation.into());
    }

    /// Print a formatted summary
    pub fn print_summary(&self) {
        println!("\n=== Scenario: {} ===", self.name);
        println!(
            "Status: {}",
            if self.passed {
                "PASSED ✓"
            } else {
                "FAILED ✗"
            }
        );
        println!("Land Rate: {:.2}%", self.land_rate);
        println!("Inclusion Rate: {:.2}%", self.inclusion_rate);
        println!("\nLatencies:");
        if let Some(latency) = self.avg_latencies.oracle_scoring {
            println!("  Oracle Scoring: {:.2} ms", latency);
        }
        if let Some(latency) = self.avg_latencies.trigger_send {
            println!("  Trigger Send: {:.2} ms", latency);
        }
        if let Some(latency) = self.avg_latencies.trigger_confirm {
            println!("  Trigger Confirm: {:.2} ms", latency);
        }
        if let Some(latency) = self.avg_latencies.e2e_total {
            println!("  E2E Total: {:.2} ms", latency);
        }
        if !self.observations.is_empty() {
            println!("\nObservations:");
            for obs in &self.observations {
                println!("  - {}", obs);
            }
        }
        println!();
    }
}

/// Trait for test scenarios
pub trait TestScenario {
    /// Get the scenario name
    fn name(&self) -> &str;

    /// Run the scenario
    fn run(
        &self,
        config: &E2EConfig,
        metrics: Arc<E2EMetrics>,
    ) -> impl std::future::Future<Output = Result<ScenarioResult>> + Send;
}
