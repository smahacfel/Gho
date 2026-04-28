//! MAR Engine.
//!
//! Orchestrates the Mechanism-Aware Rug Guard logic.
//! Maintains supply snapshots, tracks execution costs, and determines market exploitability state.

use solana_sdk::pubkey::Pubkey;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use crate::mar::execution_cost::{calculate_k, ExecutionCostAnalyzer};
use crate::mar::impact::choose_x_effective;
use crate::mar::perturbation::PerturbationTester;
use crate::mar::supply_snapshot::SupplySnapshotEngine;
use crate::mar::types::{
    MarConfig, MarMetricsSnapshot, MarPoolReserves, MarketExploitabilityState,
};

/// The central engine for MAR.
///
/// It processes stream events (token account updates, reserves updates, slot updates)
/// and maintains the current exploitability state.
pub struct MarEngine {
    /// Configuration parameters.
    config: MarConfig,

    /// Engine for tracking token distribution.
    supply_snapshot: SupplySnapshotEngine,

    /// Analyzer for K(t) stability.
    cost_analyzer: ExecutionCostAnalyzer,

    /// Tester for robustness against perturbations.
    perturbation_tester: PerturbationTester,

    /// Latest known pool reserves (if any).
    latest_reserves: Option<MarPoolReserves>,

    /// Current state of the market.
    current_state: MarketExploitabilityState,

    /// Latest computed metrics.
    latest_metrics: Option<MarMetricsSnapshot>,
}

impl MarEngine {
    /// Create a new MAR Engine with the given configuration.
    pub fn new(config: MarConfig) -> Self {
        let supply_snapshot = SupplySnapshotEngine::new(config.top_holders_limit);
        let cost_analyzer = ExecutionCostAnalyzer::new(config.stability_window_seconds);
        let perturbation_tester = PerturbationTester::new(
            config.perturbation_iterations,
            None, // Use random seed
        );

        Self {
            config,
            supply_snapshot,
            cost_analyzer,
            perturbation_tester,
            latest_reserves: None,
            current_state: MarketExploitabilityState::InvalidCoverage, // Start as invalid until data arrives
            latest_metrics: None,
        }
    }

    /// Updates the latest known pool reserves.
    pub fn update_reserves(&mut self, reserves: MarPoolReserves) {
        self.latest_reserves = Some(reserves);
    }

    /// Updates the total supply (e.g. from mint account update).
    pub fn update_total_supply(&mut self, total_supply: u64) {
        self.supply_snapshot.set_total_supply(total_supply);
    }

    /// Process a token account update.
    pub fn update_token_account(
        &mut self,
        slot: u64,
        token_account_pubkey: Pubkey,
        owner_pubkey: Pubkey,
        mint_pubkey: Pubkey,
        amount: u64,
    ) {
        self.supply_snapshot.apply_token_account_update(
            slot,
            token_account_pubkey,
            owner_pubkey,
            mint_pubkey,
            amount,
        );
    }

    /// Returns the latest metrics snapshot.
    pub fn get_latest_snapshot(&self) -> Option<MarMetricsSnapshot> {
        self.latest_metrics.clone()
    }

    /// Returns the current state.
    pub fn get_state(&self) -> MarketExploitabilityState {
        self.current_state
    }

    /// Triggers a tick update (e.g. on new slot).
    pub fn on_slot(&mut self, _slot: u64) {
        // We trigger recalculation on slot updates to ensure freshness
        self.recalculate_state();
    }

    /// Recalculates the market state based on current data.
    pub fn recalculate_state(&mut self) {
        let total_supply = match self.supply_snapshot.total_supply() {
            Some(s) if s > 0 => s,
            _ => {
                self.set_state(MarketExploitabilityState::InvalidCoverage, None);
                return;
            }
        };

        let observed_supply = self.supply_snapshot.observed_supply();
        let coverage = observed_supply as f64 / total_supply as f64;

        if coverage < self.config.coverage_min {
            self.set_state(
                MarketExploitabilityState::InvalidCoverage,
                Some(self.create_partial_snapshot(coverage)),
            );
            return;
        }

        let reserves = match self.latest_reserves {
            Some(r) => r,
            None => {
                // Missing reserves implies we can't calculate impact -> Invalid Coverage
                self.set_state(
                    MarketExploitabilityState::InvalidCoverage,
                    Some(self.create_partial_snapshot(coverage)),
                );
                return;
            }
        };

        let x_effective = choose_x_effective(
            &reserves,
            total_supply as u128,
            &self.config.x_effective_candidates,
            self.config.impact_threshold,
        );

        let x_eff_val = match x_effective {
            Some(x) => x,
            None => {
                // Impact never reached threshold even at max candidate?
                // Means huge liquidity. Safe.
                self.set_state(
                    MarketExploitabilityState::Safe,
                    Some(self.create_partial_snapshot(coverage)),
                );
                return;
            }
        };

        let holders = self.supply_snapshot.get_top_holders_desc();
        let balances: Vec<u64> = holders.iter().map(|(_, b)| *b).collect();
        let target_amount = (total_supply as f64 * x_eff_val) as u64;

        let k_now = calculate_k(&balances, target_amount);
        self.cost_analyzer.add_sample(k_now);

        let sigma_k = self.cost_analyzer.sigma_k();
        let delta_k = self.cost_analyzer.delta_k();

        let perturb_k_median = self
            .perturbation_tester
            .run_test(&holders, total_supply, x_eff_val);

        // --- Decision Logic ---

        let is_cheap = k_now <= self.config.k_max;

        // Stability check
        let stable_sigma = sigma_k.map_or(false, |s| s <= self.config.sigma_max);
        let stable_delta = delta_k.map_or(false, |d| d <= self.config.delta_max);

        // Perturbation check (robustness)
        let robust = match perturb_k_median {
            Some(median) => median <= (k_now as f64 + self.config.perturbation_epsilon as f64),
            None => false, // Cannot run test -> assume not robust
        };

        let new_state = if is_cheap {
            if stable_sigma && stable_delta && robust {
                MarketExploitabilityState::ExecutionReady
            } else {
                MarketExploitabilityState::Fragile
            }
        } else {
            MarketExploitabilityState::Safe
        };

        // Construct metrics
        let top_balance_sum: u64 = balances.iter().sum();
        let coverage_top = if observed_supply > 0 {
            Some(top_balance_sum as f64 / observed_supply as f64)
        } else {
            None
        };

        let metrics = MarMetricsSnapshot {
            coverage_supply: coverage,
            coverage_top,
            x_effective: Some(x_eff_val),
            k_now: Some(k_now),
            sigma_k,
            delta_k,
            perturb_k_median,
            impact_threshold: self.config.impact_threshold,
            pool_reserves: Some(reserves),
        };

        self.set_state(new_state, Some(metrics));
    }

    fn set_state(&mut self, state: MarketExploitabilityState, metrics: Option<MarMetricsSnapshot>) {
        if self.current_state != state {
            info!(
                "MAR State Transition: {:?} -> {:?}",
                self.current_state, state
            );
        }
        self.current_state = state;
        self.latest_metrics = metrics;
    }

    fn create_partial_snapshot(&self, coverage: f64) -> MarMetricsSnapshot {
        MarMetricsSnapshot {
            coverage_supply: coverage,
            coverage_top: None,
            x_effective: None,
            k_now: None,
            sigma_k: None,
            delta_k: None,
            perturb_k_median: None,
            impact_threshold: self.config.impact_threshold,
            pool_reserves: self.latest_reserves,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> MarConfig {
        MarConfig {
            coverage_min: 0.7,
            impact_threshold: 0.6,
            k_max: 25,
            sigma_max: 0.5,
            delta_max: 2.0,
            perturbation_epsilon: 2,
            perturbation_iterations: 10,
            stability_window_seconds: 60,
            top_holders_limit: 200,
            x_effective_candidates: vec![0.1, 0.2, 0.3],
            fail_closed_on_invalid_coverage: true,
        }
    }

    #[test]
    fn test_initial_state() {
        let engine = MarEngine::new(default_config());
        assert_eq!(
            engine.get_state(),
            MarketExploitabilityState::InvalidCoverage
        );
        assert!(engine.get_latest_snapshot().is_none());
    }

    #[test]
    fn test_missing_data_invalid_coverage() {
        let mut engine = MarEngine::new(default_config());

        // No total supply -> InvalidCoverage
        engine.recalculate_state();
        assert_eq!(
            engine.get_state(),
            MarketExploitabilityState::InvalidCoverage
        );

        // Set total supply, but observed is 0 -> coverage 0 -> InvalidCoverage
        engine.update_total_supply(1000);
        engine.recalculate_state();
        assert_eq!(
            engine.get_state(),
            MarketExploitabilityState::InvalidCoverage
        );

        // Set observed supply to 100% -> coverage 1.0 -> But no reserves -> InvalidCoverage
        let mint = Pubkey::new_unique();
        engine.update_token_account(1, Pubkey::new_unique(), Pubkey::new_unique(), mint, 1000);
        engine.recalculate_state();
        assert_eq!(
            engine.get_state(),
            MarketExploitabilityState::InvalidCoverage
        );
    }

    #[test]
    fn test_transition_to_execution_ready() {
        let mut engine = MarEngine::new(default_config());
        let mint = Pubkey::new_unique();
        let total_supply = 1_000_000;

        engine.update_total_supply(total_supply);

        // 1. Setup reserves for high impact
        // Pool: 1M tokens, 100 SOL.
        // Selling 100k (10%) -> 1M / 1.1M = 0.909 -> drop ~17% (not enough for 0.6)
        // Selling 300k (30%) -> 1M / 1.3M = 0.769 -> drop ~40%
        // Selling 500k (50%) -> 1M / 1.5M = 0.666 -> drop ~55%
        // Selling 1M (100%) -> 1M / 2M = 0.5 -> drop 75%
        // So X_effective should be picked around something higher than 0.3.
        // Candidates: 0.1, 0.2, 0.3. Max is 0.3.
        // With 0.3 (300k), drop is ~40% < 60%.
        // Wait, if max candidate doesn't reach threshold -> Safe.
        // Let's adjust reserves to be very sensitive (low liquidity vs sold amount)
        // Or adjust candidates.

        let mut config = default_config();
        config.x_effective_candidates = vec![0.1, 0.5]; // 0.5 gives 55% drop, still < 60%.
                                                        // Let's change threshold to 0.4.
        config.impact_threshold = 0.4;
        let mut engine = MarEngine::new(config);
        engine.update_total_supply(total_supply);

        // Reserves: 1M tokens.
        engine.update_reserves(MarPoolReserves {
            reserve_token: 1_000_000,
            reserve_sol: 1000,
        });

        // 2. Setup holders to be concentrated (Low K)
        // We need X_effective.
        // Try 0.1: 1M/1.1M -> 0.909^2 = 0.82 -> drop 0.18.
        // Try 0.5: 1M/1.5M -> 0.666^2 = 0.44 -> drop 0.56. (> 0.4).
        // So X_effective = 0.5. Target = 500k.

        // One holder has 600k. K=1.
        let whale = Pubkey::new_unique();
        engine.update_token_account(1, Pubkey::new_unique(), whale, mint, 600_000);

        // Add some small holders to reach coverage min (0.7)
        // Total observed needs to be 700k.
        engine.update_token_account(1, Pubkey::new_unique(), Pubkey::new_unique(), mint, 100_000);

        // Coverage = 700k/1M = 0.7. Valid.

        // 3. First tick
        engine.recalculate_state();
        // K=1. Cheap.
        // Sigma/Delta undefined or 0 (single sample).
        // If single sample, sigma/delta returns None or 0?
        // Analyzer logic: < 2 samples returns None.
        // Engine logic: map_or(false) -> so None means NOT stable.
        // So first tick -> Fragile (because cheap but not stable).

        assert_eq!(engine.get_state(), MarketExploitabilityState::Fragile);

        // 4. Second tick (stable K)
        engine.recalculate_state();
        // Now 2 samples: 1, 1. Delta=0, Sigma=0.
        // Should be stable.

        assert_eq!(
            engine.get_state(),
            MarketExploitabilityState::ExecutionReady
        );
    }

    #[test]
    fn test_safe_state_high_k() {
        let mut engine = MarEngine::new(default_config());
        let mint = Pubkey::new_unique();
        let total_supply = 1_000_000;
        engine.update_total_supply(total_supply);

        // Low threshold, so X_effective is small (e.g. 0.1)
        let mut config = default_config();
        config.impact_threshold = 0.1;
        config.x_effective_candidates = vec![0.1];
        config.k_max = 2; // Strict K max
        let mut engine = MarEngine::new(config);
        engine.update_total_supply(total_supply);

        engine.update_reserves(MarPoolReserves {
            reserve_token: 1_000_000,
            reserve_sol: 1000,
        });

        // X_effective = 0.1 (100k).

        // Distribute 100k among many holders (each 10k).
        // We need 10 holders to reach 100k.
        // K = 10.
        // K_max = 2.
        // So K > K_max -> Safe.

        for _ in 0..15 {
            engine.update_token_account(
                1,
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                mint,
                10_000,
            );
        }
        // Total 150k. Coverage 0.15. Too low? default min is 0.7.
        // Need to add more supply to reach 700k.
        engine.update_token_account(1, Pubkey::new_unique(), Pubkey::new_unique(), mint, 600_000); // Whale but not top?
                                                                                                   // Wait, whale will be top holder. 600k > 100k target. K=1.
                                                                                                   // That would be unsafe.

        // We need distributed supply.
        // Let's create 80 holders with 10k each -> 800k total.
        // Target 100k.
        // Sorted: all 10k.
        // Need 10 holders. K=10.
        // Safe.

        // Reset engine to clear whale
        let mut config = default_config();
        config.impact_threshold = 0.1;
        config.x_effective_candidates = vec![0.1];
        config.k_max = 5;
        let mut engine = MarEngine::new(config);
        engine.update_total_supply(total_supply);
        engine.update_reserves(MarPoolReserves {
            reserve_token: 1_000_000,
            reserve_sol: 1000,
        });

        for _ in 0..80 {
            engine.update_token_account(
                1,
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                mint,
                10_000,
            );
        }

        engine.recalculate_state();
        assert_eq!(engine.get_state(), MarketExploitabilityState::Safe);
    }
}
