//! Weight Tuning Module
//!
//! This module provides two key mechanisms for tuning signal weights:
//!
//! ## 1. Online Bandits (LinUCB / Thompson Sampling)
//!
//! Real-time weight adjustment every 1-5 minutes based on trading rewards:
//! - **LinUCB**: Upper Confidence Bound algorithm with linear payoff model
//! - **Thompson Sampling**: Bayesian approach using posterior sampling
//!
//! Targets weights: `wQASS`, `wMPCF`, `wSOBP`, `wIWIM`
//!
//! Reward signal:
//! - Positive reward from profitable transactions
//! - Penalty for false BUY signals (opportunity cost)
//!
//! ## 2. Offline Bayesian Optimization
//!
//! Full hyperparameter optimization on a 12-hour cycle:
//! - QASS amplitude parameters
//! - Confidence thresholds
//! - QEDD decay rates
//! - MCI weight distributions
//!
//! ## 3. Fallback: Manual Parameter Freezing
//!
//! Emergency mechanism to lock parameters via TOML configuration when:
//! - Market regime drift detected
//! - Unexpected optimization divergence
//! - Manual operator intervention required
//!
//! # Usage
//!
//! ```rust,ignore
//! use ghost_brain::tuning::{WeightTuner, TuningConfig, BanditAlgorithm};
//!
//! // Create tuner with LinUCB algorithm
//! let config = TuningConfig::default();
//! let mut tuner = WeightTuner::new(config, BanditAlgorithm::LinUCB);
//!
//! // Update weights based on reward
//! let context = TuningContext::from_market_signals(&signals);
//! let reward = compute_reward(profit, was_false_positive);
//! tuner.update(context, reward);
//!
//! // Get current optimized weights
//! let weights = tuner.current_weights();
//! ```
//!
//! # Integration
//!
//! - **Telemetry**: All weight updates are logged to telemetry
//! - **Dry Run**: Tuning works in simulation mode for validation
//! - **Historical Replay**: Can replay historical data for backtesting

pub mod bandits;
pub mod bayesian;
pub mod config;
pub mod frozen_params;
pub mod hysteresis_loop;
pub mod integration;
pub mod reward;
pub mod service;

pub use bandits::{BanditAlgorithm, LinUCBBandit, ThompsonSamplingBandit, WeightBandit};
pub use bayesian::{AcquisitionFunction, BayesianOptimizer, GaussianProcess, OptimizationResult};
pub use config::{BanditConfig, BayesianConfig, FrozenConfig, RewardConfig, TuningConfig};
pub use frozen_params::{FrozenParameters, ParameterFreezer};
pub use hysteresis_loop::{DecisionOutcome, HysteresisConfig, HysteresisLoop, LoopStats};
pub use integration::{
    cleanup_expired, get_current_weights, get_loop_stats, is_enabled, register_decision,
    register_outcome, HYSTERESIS_LOOP,
};
pub use reward::{RewardCalculator, RewardSignal, TradeOutcome};
pub use service::{TuningMessage, TuningService, TuningServiceConfig, TuningServiceStats};

use crate::config::ConfidenceConfig;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{debug, info, warn};

/// Tunable weights for confidence calculation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TunableWeights {
    /// QASS (Quantum Amplitude Scoring System) weight
    pub w_qass: f32,
    /// MPCF (Micro-Payload Cognitive Fingerprint) weight
    pub w_mpcf: f32,
    /// SOBP (Slot-Over-Slot Buying Pressure) weight
    pub w_sobp: f32,
    /// IWIM (Inter-Wallet Interaction Matrix) weight
    pub w_iwim: f32,
}

impl Default for TunableWeights {
    fn default() -> Self {
        Self {
            w_qass: 15.0,
            w_mpcf: 10.0,
            w_sobp: 12.0,
            w_iwim: 8.0,
        }
    }
}

impl TunableWeights {
    /// Create weights from ConfidenceConfig
    pub fn from_config(config: &ConfidenceConfig) -> Self {
        Self {
            w_qass: config.weight_qass,
            w_mpcf: config.weight_mpcf,
            w_sobp: config.weight_sobp,
            w_iwim: config.weight_iwim,
        }
    }

    /// Convert to array for bandit algorithms
    pub fn to_array(&self) -> [f32; 4] {
        [self.w_qass, self.w_mpcf, self.w_sobp, self.w_iwim]
    }

    /// Create from array
    pub fn from_array(arr: [f32; 4]) -> Self {
        Self {
            w_qass: arr[0],
            w_mpcf: arr[1],
            w_sobp: arr[2],
            w_iwim: arr[3],
        }
    }

    /// Normalize weights to sum to a target value
    pub fn normalize(&mut self, target_sum: f32) {
        let current_sum = self.w_qass + self.w_mpcf + self.w_sobp + self.w_iwim;
        if current_sum > 0.0 {
            let scale = target_sum / current_sum;
            self.w_qass *= scale;
            self.w_mpcf *= scale;
            self.w_sobp *= scale;
            self.w_iwim *= scale;
        }
    }

    /// Clamp weights to valid ranges
    pub fn clamp(&mut self, min: f32, max: f32) {
        self.w_qass = self.w_qass.clamp(min, max);
        self.w_mpcf = self.w_mpcf.clamp(min, max);
        self.w_sobp = self.w_sobp.clamp(min, max);
        self.w_iwim = self.w_iwim.clamp(min, max);
    }
}

/// Context features for bandit decision making
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningContext {
    /// Market volatility indicator [0.0, 1.0]
    pub volatility: f32,
    /// Average trading volume (normalized)
    pub volume: f32,
    /// Bot activity level [0.0, 1.0]
    pub bot_activity: f32,
    /// Current MCI (Market Coherence Index)
    pub mci: f32,
    /// Time of day factor (for time-based patterns)
    pub time_factor: f32,
    /// Pool age in seconds
    pub pool_age_seconds: u64,
}

impl Default for TuningContext {
    fn default() -> Self {
        Self {
            volatility: 0.5,
            volume: 0.5,
            bot_activity: 0.3,
            mci: 0.5,
            time_factor: 0.5,
            pool_age_seconds: 0,
        }
    }
}

impl TuningContext {
    /// Convert to feature vector for linear models
    pub fn to_features(&self) -> Vec<f32> {
        vec![
            1.0, // Bias term
            self.volatility,
            self.volume,
            self.bot_activity,
            self.mci,
            self.time_factor,
            (self.pool_age_seconds as f32 / 600.0).min(1.0), // Normalized pool age
        ]
    }
}

/// Main weight tuner combining all tuning mechanisms
pub struct WeightTuner {
    /// Current weights
    weights: TunableWeights,
    /// Bandit algorithm for online updates
    bandit: Box<dyn WeightBandit + Send + Sync>,
    /// Bayesian optimizer for offline optimization
    bayesian: Option<BayesianOptimizer>,
    /// Frozen parameters (if any)
    frozen: Option<FrozenParameters>,
    /// Configuration
    config: TuningConfig,
    /// Reward calculator
    reward_calc: RewardCalculator,
    /// Last update timestamp
    last_update: Instant,
    /// Last Bayesian optimization timestamp
    last_bayesian_update: Instant,
    /// Update counter
    update_count: u64,
    /// Cumulative reward
    cumulative_reward: f64,
}

impl WeightTuner {
    /// Create a new weight tuner
    pub fn new(config: TuningConfig, algorithm: BanditAlgorithm) -> Self {
        let initial_weights = TunableWeights::default();
        let bandit: Box<dyn WeightBandit + Send + Sync> = match algorithm {
            BanditAlgorithm::LinUCB => Box::new(LinUCBBandit::new(config.bandit.clone())),
            BanditAlgorithm::ThompsonSampling => {
                Box::new(ThompsonSamplingBandit::new(config.bandit.clone()))
            }
        };

        let bayesian = if config.bayesian.enabled {
            Some(BayesianOptimizer::new(config.bayesian.clone()))
        } else {
            None
        };

        let reward_calc = RewardCalculator::new(config.reward.clone());

        Self {
            weights: initial_weights,
            bandit,
            bayesian,
            frozen: None,
            config,
            reward_calc,
            last_update: Instant::now(),
            last_bayesian_update: Instant::now(),
            update_count: 0,
            cumulative_reward: 0.0,
        }
    }

    /// Create with specific initial weights
    pub fn with_initial_weights(mut self, weights: TunableWeights) -> Self {
        self.weights = weights;
        self
    }

    /// Load frozen parameters from TOML file
    pub fn load_frozen_params(&mut self, path: &str) -> anyhow::Result<()> {
        let frozen = FrozenParameters::load_from_toml(path)?;
        if frozen.enabled {
            info!("Loaded frozen parameters from {}", path);
            self.frozen = Some(frozen);
        }
        Ok(())
    }

    /// Freeze parameters manually
    pub fn freeze_parameters(&mut self, params: FrozenParameters) {
        warn!("Manually freezing parameters: {:?}", params);
        self.frozen = Some(params);
    }

    /// Unfreeze parameters
    pub fn unfreeze_parameters(&mut self) {
        info!("Unfreezing parameters");
        self.frozen = None;
    }

    /// Get current weights
    pub fn current_weights(&self) -> TunableWeights {
        // If frozen, return frozen weights
        if let Some(frozen) = &self.frozen {
            if frozen.enabled {
                return frozen.weights;
            }
        }
        self.weights
    }

    /// Update weights based on trade outcome
    pub fn update(&mut self, context: &TuningContext, outcome: &TradeOutcome) {
        // Don't update if frozen
        if self.frozen.as_ref().map(|f| f.enabled).unwrap_or(false) {
            debug!("Skipping update - parameters are frozen");
            return;
        }

        // Calculate reward
        let reward = self.reward_calc.calculate(outcome);
        self.cumulative_reward += reward.total as f64;
        self.update_count += 1;

        // Update bandit with context and reward
        let features = context.to_features();
        self.bandit.update(&features, reward.total);

        // Get new weight suggestions from bandit
        let suggested = self.bandit.suggest_weights(&features);

        // Apply with smoothing
        let alpha = self.config.bandit.learning_rate;
        self.weights.w_qass = (1.0 - alpha) * self.weights.w_qass + alpha * suggested[0];
        self.weights.w_mpcf = (1.0 - alpha) * self.weights.w_mpcf + alpha * suggested[1];
        self.weights.w_sobp = (1.0 - alpha) * self.weights.w_sobp + alpha * suggested[2];
        self.weights.w_iwim = (1.0 - alpha) * self.weights.w_iwim + alpha * suggested[3];

        // Clamp and normalize
        self.weights
            .clamp(self.config.bandit.min_weight, self.config.bandit.max_weight);
        self.weights.normalize(self.config.bandit.target_weight_sum);

        self.last_update = Instant::now();

        debug!(
            "Weight update #{}: QASS={:.2}, MPCF={:.2}, SOBP={:.2}, IWIM={:.2}, reward={:.4}",
            self.update_count,
            self.weights.w_qass,
            self.weights.w_mpcf,
            self.weights.w_sobp,
            self.weights.w_iwim,
            reward.total
        );
    }

    /// Run offline Bayesian optimization (typically on 12h cycle)
    pub fn run_bayesian_optimization(
        &mut self,
        historical_data: &[(TuningContext, TradeOutcome)],
    ) -> Option<OptimizationResult> {
        // Don't run if frozen
        if self.frozen.as_ref().map(|f| f.enabled).unwrap_or(false) {
            warn!("Skipping Bayesian optimization - parameters are frozen");
            return None;
        }

        let optimizer = self.bayesian.as_mut()?;

        // Run optimization
        let result = optimizer.optimize(historical_data, &self.reward_calc);

        if let Some(ref opt_result) = result {
            info!(
                "Bayesian optimization complete: best_weights={:?}, expected_improvement={:.4}",
                opt_result.best_weights, opt_result.expected_improvement
            );

            // Apply optimized weights with conservative blending
            let blend = self.config.bayesian.blend_factor;
            self.weights.w_qass =
                (1.0 - blend) * self.weights.w_qass + blend * opt_result.best_weights.w_qass;
            self.weights.w_mpcf =
                (1.0 - blend) * self.weights.w_mpcf + blend * opt_result.best_weights.w_mpcf;
            self.weights.w_sobp =
                (1.0 - blend) * self.weights.w_sobp + blend * opt_result.best_weights.w_sobp;
            self.weights.w_iwim =
                (1.0 - blend) * self.weights.w_iwim + blend * opt_result.best_weights.w_iwim;

            self.weights
                .clamp(self.config.bandit.min_weight, self.config.bandit.max_weight);
            self.weights.normalize(self.config.bandit.target_weight_sum);
        }

        self.last_bayesian_update = Instant::now();
        result
    }

    /// Check if Bayesian optimization should run
    pub fn should_run_bayesian(&self) -> bool {
        if !self.config.bayesian.enabled {
            return false;
        }
        self.last_bayesian_update.elapsed() >= self.config.bayesian.cycle_duration
    }

    /// Get tuning statistics
    pub fn stats(&self) -> TuningStats {
        TuningStats {
            update_count: self.update_count,
            cumulative_reward: self.cumulative_reward,
            average_reward: if self.update_count > 0 {
                self.cumulative_reward / self.update_count as f64
            } else {
                0.0
            },
            current_weights: self.weights,
            is_frozen: self.frozen.as_ref().map(|f| f.enabled).unwrap_or(false),
            seconds_since_last_update: self.last_update.elapsed().as_secs(),
            seconds_since_bayesian: self.last_bayesian_update.elapsed().as_secs(),
        }
    }
}

/// Statistics about tuning state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningStats {
    /// Total number of weight updates
    pub update_count: u64,
    /// Cumulative reward
    pub cumulative_reward: f64,
    /// Average reward per update
    pub average_reward: f64,
    /// Current weights
    pub current_weights: TunableWeights,
    /// Whether parameters are frozen
    pub is_frozen: bool,
    /// Seconds since last online update
    pub seconds_since_last_update: u64,
    /// Seconds since last Bayesian optimization
    pub seconds_since_bayesian: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunable_weights_default() {
        let weights = TunableWeights::default();
        assert_eq!(weights.w_qass, 15.0);
        assert_eq!(weights.w_mpcf, 10.0);
        assert_eq!(weights.w_sobp, 12.0);
        assert_eq!(weights.w_iwim, 8.0);
    }

    #[test]
    fn test_tunable_weights_normalize() {
        let mut weights = TunableWeights {
            w_qass: 10.0,
            w_mpcf: 10.0,
            w_sobp: 10.0,
            w_iwim: 10.0,
        };
        weights.normalize(100.0);
        let sum = weights.w_qass + weights.w_mpcf + weights.w_sobp + weights.w_iwim;
        assert!((sum - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_tunable_weights_clamp() {
        let mut weights = TunableWeights {
            w_qass: 50.0,
            w_mpcf: -5.0,
            w_sobp: 25.0,
            w_iwim: 0.5,
        };
        weights.clamp(1.0, 30.0);
        assert_eq!(weights.w_qass, 30.0);
        assert_eq!(weights.w_mpcf, 1.0);
        assert_eq!(weights.w_sobp, 25.0);
        assert_eq!(weights.w_iwim, 1.0);
    }

    #[test]
    fn test_tuning_context_to_features() {
        let ctx = TuningContext {
            volatility: 0.7,
            volume: 0.5,
            bot_activity: 0.2,
            mci: 0.8,
            time_factor: 0.3,
            pool_age_seconds: 300,
        };
        let features = ctx.to_features();
        assert_eq!(features.len(), 7);
        assert_eq!(features[0], 1.0); // Bias
        assert_eq!(features[1], 0.7); // Volatility
    }

    #[test]
    fn test_weight_tuner_creation() {
        let config = TuningConfig::default();
        let tuner = WeightTuner::new(config, BanditAlgorithm::LinUCB);
        let weights = tuner.current_weights();
        assert_eq!(weights, TunableWeights::default());
    }

    #[test]
    fn test_weight_tuner_frozen() {
        let config = TuningConfig::default();
        let mut tuner = WeightTuner::new(config, BanditAlgorithm::LinUCB);

        let frozen = FrozenParameters {
            enabled: true,
            weights: TunableWeights {
                w_qass: 20.0,
                w_mpcf: 15.0,
                w_sobp: 10.0,
                w_iwim: 5.0,
            },
            reason: "Test freeze".to_string(),
            frozen_at: chrono::Utc::now(),
        };

        tuner.freeze_parameters(frozen);
        let weights = tuner.current_weights();
        assert_eq!(weights.w_qass, 20.0);
        assert!(tuner.stats().is_frozen);
    }
}
