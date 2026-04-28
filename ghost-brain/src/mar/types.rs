//! MAR domain types shared across PRE-BUY and POST-BUY integrations.

use serde::{Deserialize, Serialize};

/// High-level exploitability state used by Gatekeeper (PRE-BUY) and Guardian (POST-BUY).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarketExploitabilityState {
    /// Market is stable and not cheaply executable.
    Safe,
    /// Market shows weakening structure but does not meet execution-ready criteria.
    Fragile,
    /// Execution is feasible and cheap; must block entry or force exit.
    ExecutionReady,
    /// Coverage is insufficient for reliable evaluation.
    InvalidCoverage,
}

/// Snapshot of pool reserves when available.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarPoolReserves {
    /// Token-side reserves (raw units).
    pub reserve_token: u128,
    /// SOL-side reserves (lamports).
    pub reserve_sol: u128,
}

/// MAR telemetry snapshot emitted per mint/pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarMetricsSnapshot {
    /// Coverage ratio of observed supply to total supply (0.0-1.0).
    pub coverage_supply: f64,
    /// Coverage ratio of top holder balances to observed supply (0.0-1.0).
    pub coverage_top: Option<f64>,
    /// Effective supply share required for the configured impact threshold.
    pub x_effective: Option<f64>,
    /// Current K(t) value for the chosen X_effective.
    pub k_now: Option<u32>,
    /// Rolling standard deviation of K(t).
    pub sigma_k: Option<f64>,
    /// Absolute delta between current K and window baseline.
    pub delta_k: Option<f64>,
    /// Median K' value from the perturbation test.
    pub perturb_k_median: Option<f64>,
    /// Impact threshold required to trigger execution readiness (fractional drop).
    pub impact_threshold: f64,
    /// Pool reserves snapshot when available.
    pub pool_reserves: Option<MarPoolReserves>,
}

/// Configuration for MAR thresholds and policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarConfig {
    /// Minimum coverage ratio required for valid evaluation (e.g., 0.7).
    pub coverage_min: f64,
    /// Impact threshold for defining X_effective (e.g., 0.6 for 60% drop).
    pub impact_threshold: f64,
    /// Maximum K(t) allowed for ExecutionReady.
    pub k_max: u32,
    /// Maximum allowed standard deviation of K(t).
    pub sigma_max: f64,
    /// Maximum allowed absolute delta of K(t) in the rolling window.
    pub delta_max: f64,
    /// Perturbation tolerance epsilon (K' median <= K + epsilon).
    pub perturbation_epsilon: u32,
    /// Number of perturbation iterations per tick.
    pub perturbation_iterations: usize,
    /// Rolling window duration in seconds for stability metrics.
    pub stability_window_seconds: u64,
    /// Maximum number of top holders tracked.
    pub top_holders_limit: usize,
    /// Candidate X_effective values to test (fractions of total supply).
    pub x_effective_candidates: Vec<f64>,
    /// Policy for InvalidCoverage after BUY (fail-closed when true).
    pub fail_closed_on_invalid_coverage: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_state_roundtrip() {
        let state = MarketExploitabilityState::ExecutionReady;
        let serialized = serde_json::to_string(&state).unwrap();
        let deserialized: MarketExploitabilityState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn metrics_snapshot_roundtrip() {
        let metrics = MarMetricsSnapshot {
            coverage_supply: 0.7,
            coverage_top: None,
            x_effective: Some(0.3),
            k_now: Some(25),
            sigma_k: None,
            delta_k: None,
            perturb_k_median: Some(25.0),
            impact_threshold: 0.6,
            pool_reserves: None,
        };
        let serialized = serde_json::to_string(&metrics).unwrap();
        let deserialized: MarMetricsSnapshot = serde_json::from_str(&serialized).unwrap();
        assert_eq!(metrics, deserialized);
    }

    #[test]
    fn config_roundtrip() {
        let coverage_min = 0.7;
        let config = MarConfig {
            coverage_min,
            impact_threshold: 0.6,
            k_max: 25,
            sigma_max: coverage_min,
            delta_max: coverage_min,
            perturbation_epsilon: 2,
            perturbation_iterations: 20,
            stability_window_seconds: 60,
            top_holders_limit: 200,
            x_effective_candidates: vec![0.1, 0.15, 0.2, 0.3, 0.4],
            fail_closed_on_invalid_coverage: true,
        };
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: MarConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }
}
