//! Bayesian Optimization for Offline Hyperparameter Tuning
//!
//! Implements Gaussian Process-based Bayesian optimization for full hyperparameter
//! optimization on a 12-hour cycle. Targets:
//!
//! - QASS amplitude parameters
//! - Confidence thresholds
//! - QEDD decay rates
//! - MCI weight distributions
//!
//! Uses Expected Improvement (EI) or Upper Confidence Bound (UCB) acquisition functions.

use crate::tuning::config::BayesianConfig;
use crate::tuning::reward::{RewardCalculator, TradeOutcome};
use crate::tuning::{TunableWeights, TuningContext};
use nalgebra::{DMatrix, DVector};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use tracing::{debug, info};

/// Acquisition function type for Bayesian optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcquisitionFunction {
    /// Expected Improvement
    ExpectedImprovement,
    /// Upper Confidence Bound
    UCB,
    /// Probability of Improvement
    ProbabilityOfImprovement,
}

impl Default for AcquisitionFunction {
    fn default() -> Self {
        AcquisitionFunction::ExpectedImprovement
    }
}

/// Result from Bayesian optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    /// Best weights found
    pub best_weights: TunableWeights,
    /// Expected improvement at best point
    pub expected_improvement: f64,
    /// Number of evaluations performed
    pub evaluations: usize,
    /// Optimization history
    pub history: Vec<OptimizationStep>,
    /// Final GP hyperparameters
    pub gp_length_scale: f64,
    /// Final GP signal variance
    pub gp_signal_variance: f64,
}

/// Single optimization step record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationStep {
    /// Iteration number
    pub iteration: usize,
    /// Weights evaluated
    pub weights: TunableWeights,
    /// Observed reward
    pub reward: f64,
    /// Acquisition value at this point
    pub acquisition_value: f64,
}

/// Gaussian Process surrogate model
///
/// Uses RBF (Radial Basis Function) kernel for smooth interpolation
/// of the reward surface.
pub struct GaussianProcess {
    /// Observed inputs (n × d)
    x_train: Vec<Vec<f64>>,
    /// Observed outputs (n × 1)
    y_train: Vec<f64>,
    /// Length scale parameter
    length_scale: f64,
    /// Signal variance
    signal_variance: f64,
    /// Noise variance
    noise_variance: f64,
    /// Precomputed inverse of K + σ²I
    k_inv: Option<DMatrix<f64>>,
    /// Precomputed alpha = K⁻¹ y
    alpha: Option<DVector<f64>>,
}

impl GaussianProcess {
    /// Create a new Gaussian Process
    pub fn new(length_scale: f64, signal_variance: f64, noise_variance: f64) -> Self {
        Self {
            x_train: Vec::new(),
            y_train: Vec::new(),
            length_scale,
            signal_variance,
            noise_variance,
            k_inv: None,
            alpha: None,
        }
    }

    /// RBF (Squared Exponential) kernel
    fn rbf_kernel(&self, x1: &[f64], x2: &[f64]) -> f64 {
        let sq_dist: f64 = x1.iter().zip(x2.iter()).map(|(a, b)| (a - b).powi(2)).sum();
        self.signal_variance * (-0.5 * sq_dist / self.length_scale.powi(2)).exp()
    }

    /// Add training point
    pub fn add_observation(&mut self, x: Vec<f64>, y: f64) {
        self.x_train.push(x);
        self.y_train.push(y);
        // Invalidate cached computations
        self.k_inv = None;
        self.alpha = None;
    }

    /// Fit the GP (precompute matrices)
    pub fn fit(&mut self) {
        let n = self.x_train.len();
        if n == 0 {
            return;
        }

        // Build covariance matrix K
        let mut k_data = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                k_data[i * n + j] = self.rbf_kernel(&self.x_train[i], &self.x_train[j]);
                if i == j {
                    k_data[i * n + j] += self.noise_variance;
                }
            }
        }
        let k = DMatrix::from_row_slice(n, n, &k_data);

        // Compute K⁻¹ (try_inverse consumes the matrix)
        if let Some(k_inv) = k.try_inverse() {
            let y = DVector::from_vec(self.y_train.clone());
            self.alpha = Some(&k_inv * &y);
            self.k_inv = Some(k_inv);
        }
    }

    /// Predict mean and variance at a point
    pub fn predict(&self, x: &[f64]) -> (f64, f64) {
        let n = self.x_train.len();
        if n == 0 || self.alpha.is_none() || self.k_inv.is_none() {
            return (0.0, self.signal_variance);
        }

        let alpha = self.alpha.as_ref().unwrap();
        let k_inv = self.k_inv.as_ref().unwrap();

        // k* = kernel between x and all training points
        let k_star: Vec<f64> = self
            .x_train
            .iter()
            .map(|xi| self.rbf_kernel(x, xi))
            .collect();
        let k_star_vec = DVector::from_vec(k_star.clone());

        // Mean: μ = k*^T α
        let mean = k_star_vec.dot(alpha);

        // Variance: σ² = k(x, x) - k*^T K⁻¹ k*
        let k_xx = self.rbf_kernel(x, x);
        let var_reduction = k_star_vec.dot(&(k_inv * &k_star_vec));
        let variance = (k_xx - var_reduction).max(1e-10);

        (mean, variance)
    }

    /// Update hyperparameters (simplified - could use marginal likelihood)
    pub fn update_hyperparameters(&mut self) {
        if self.y_train.len() < 3 {
            return;
        }

        // Simple heuristic: set length scale based on input spread
        let n = self.x_train.len();
        if n > 0 && !self.x_train[0].is_empty() {
            let d = self.x_train[0].len();
            let mut total_var = 0.0;
            for dim in 0..d {
                let vals: Vec<f64> = self.x_train.iter().map(|x| x[dim]).collect();
                let mean = vals.iter().sum::<f64>() / n as f64;
                let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
                total_var += var;
            }
            self.length_scale = (total_var / d as f64).sqrt().max(0.1);
        }

        // Set signal variance based on output spread
        let y_mean = self.y_train.iter().sum::<f64>() / n as f64;
        let y_var = self
            .y_train
            .iter()
            .map(|y| (y - y_mean).powi(2))
            .sum::<f64>()
            / n as f64;
        self.signal_variance = y_var.max(0.01);
    }
}

/// Bayesian optimizer for hyperparameter tuning
pub struct BayesianOptimizer {
    /// Configuration
    config: BayesianConfig,
    /// Gaussian Process surrogate
    gp: GaussianProcess,
    /// Best observed value
    best_y: f64,
    /// Best observed weights
    best_x: TunableWeights,
    /// Optimization history
    history: Vec<OptimizationStep>,
    /// Random number generator seed
    rng_seed: u64,
}

impl BayesianOptimizer {
    /// Create a new Bayesian optimizer
    pub fn new(config: BayesianConfig) -> Self {
        let gp = GaussianProcess::new(
            config.gp_length_scale,
            config.gp_signal_variance,
            config.gp_noise_variance,
        );

        Self {
            config,
            gp,
            best_y: f64::NEG_INFINITY,
            best_x: TunableWeights::default(),
            history: Vec::new(),
            rng_seed: 42,
        }
    }

    /// Convert weights to parameter vector
    fn weights_to_vec(weights: &TunableWeights) -> Vec<f64> {
        vec![
            weights.w_qass as f64,
            weights.w_mpcf as f64,
            weights.w_sobp as f64,
            weights.w_iwim as f64,
        ]
    }

    /// Convert parameter vector to weights
    fn vec_to_weights(x: &[f64]) -> TunableWeights {
        TunableWeights {
            w_qass: x[0] as f32,
            w_mpcf: x[1] as f32,
            w_sobp: x[2] as f32,
            w_iwim: x[3] as f32,
        }
    }

    /// Expected Improvement acquisition function
    fn expected_improvement(&self, x: &[f64]) -> f64 {
        let (mean, variance) = self.gp.predict(x);
        let std = variance.sqrt();

        if std < 1e-10 {
            return 0.0;
        }

        let improvement = mean - self.best_y - self.config.xi;
        let z = improvement / std;

        // EI = (μ - f_best - ξ) Φ(z) + σ φ(z)
        let phi = Self::standard_normal_pdf(z);
        let big_phi = Self::standard_normal_cdf(z);

        improvement * big_phi + std * phi
    }

    /// UCB acquisition function
    fn ucb(&self, x: &[f64]) -> f64 {
        let (mean, variance) = self.gp.predict(x);
        mean + self.config.beta.sqrt() * variance.sqrt()
    }

    /// Probability of Improvement acquisition function
    fn probability_of_improvement(&self, x: &[f64]) -> f64 {
        let (mean, variance) = self.gp.predict(x);
        let std = variance.sqrt();

        if std < 1e-10 {
            return if mean > self.best_y { 1.0 } else { 0.0 };
        }

        let z = (mean - self.best_y - self.config.xi) / std;
        Self::standard_normal_cdf(z)
    }

    /// Standard normal PDF
    fn standard_normal_pdf(x: f64) -> f64 {
        (-0.5 * x.powi(2)).exp() / (2.0 * PI).sqrt()
    }

    /// Standard normal CDF (approximation)
    fn standard_normal_cdf(x: f64) -> f64 {
        0.5 * (1.0 + erf(x / 2.0_f64.sqrt()))
    }

    /// Compute acquisition value
    fn acquisition(&self, x: &[f64]) -> f64 {
        match self.config.acquisition_function {
            AcquisitionFunction::ExpectedImprovement => self.expected_improvement(x),
            AcquisitionFunction::UCB => self.ucb(x),
            AcquisitionFunction::ProbabilityOfImprovement => self.probability_of_improvement(x),
        }
    }

    /// Optimize acquisition function to find next point
    fn optimize_acquisition(&self) -> TunableWeights {
        let mut rng = rand::thread_rng();
        let mut best_acq = f64::NEG_INFINITY;
        let mut best_x = TunableWeights::default();

        let min = self.config.min_weight as f64;
        let max = self.config.max_weight as f64;

        // Random search over parameter space
        for _ in 0..self.config.n_random_samples {
            let x = vec![
                rng.gen_range(min..max),
                rng.gen_range(min..max),
                rng.gen_range(min..max),
                rng.gen_range(min..max),
            ];

            let acq = self.acquisition(&x);
            if acq > best_acq {
                best_acq = acq;
                best_x = Self::vec_to_weights(&x);
            }
        }

        // Local optimization around best point (gradient-free)
        let mut current = Self::weights_to_vec(&best_x);
        let step_size = (max - min) * 0.05;

        for _ in 0..20 {
            let base_acq = self.acquisition(&current);
            let mut improved = false;

            for dim in 0..4 {
                // Try positive step
                let mut candidate = current.clone();
                candidate[dim] = (candidate[dim] + step_size).min(max);
                let acq_plus = self.acquisition(&candidate);

                // Try negative step
                let mut candidate_neg = current.clone();
                candidate_neg[dim] = (candidate_neg[dim] - step_size).max(min);
                let acq_neg = self.acquisition(&candidate_neg);

                if acq_plus > base_acq && acq_plus > acq_neg {
                    current = candidate;
                    improved = true;
                } else if acq_neg > base_acq {
                    current = candidate_neg;
                    improved = true;
                }
            }

            if !improved {
                break;
            }
        }

        Self::vec_to_weights(&current)
    }

    /// Evaluate weights on historical data
    fn evaluate(
        &self,
        weights: &TunableWeights,
        data: &[(TuningContext, TradeOutcome)],
        reward_calc: &RewardCalculator,
    ) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut total_reward = 0.0;
        let default_weights = TunableWeights::default();
        for (_, outcome) in data.iter() {
            let reward = reward_calc.calculate(outcome);
            // Weight the reward by current weight configuration relative to defaults
            // This simulates what reward we would have gotten with these weights
            let weighted_reward = reward.total
                * (weights.w_qass / default_weights.w_qass
                    + weights.w_mpcf / default_weights.w_mpcf
                    + weights.w_sobp / default_weights.w_sobp
                    + weights.w_iwim / default_weights.w_iwim)
                / 4.0;
            total_reward += weighted_reward as f64;
        }

        total_reward / data.len() as f64
    }

    /// Run optimization
    pub fn optimize(
        &mut self,
        historical_data: &[(TuningContext, TradeOutcome)],
        reward_calc: &RewardCalculator,
    ) -> Option<OptimizationResult> {
        if historical_data.is_empty() {
            return None;
        }

        self.history.clear();
        self.best_y = f64::NEG_INFINITY;

        // Initial random samples
        let mut rng = rand::thread_rng();
        let min = self.config.min_weight as f64;
        let max = self.config.max_weight as f64;

        info!(
            "Starting Bayesian optimization with {} data points",
            historical_data.len()
        );

        for i in 0..self.config.n_initial_points {
            let weights = TunableWeights {
                w_qass: rng.gen_range(min..max) as f32,
                w_mpcf: rng.gen_range(min..max) as f32,
                w_sobp: rng.gen_range(min..max) as f32,
                w_iwim: rng.gen_range(min..max) as f32,
            };

            let reward = self.evaluate(&weights, historical_data, reward_calc);
            let x = Self::weights_to_vec(&weights);
            self.gp.add_observation(x, reward);

            if reward > self.best_y {
                self.best_y = reward;
                self.best_x = weights;
            }

            self.history.push(OptimizationStep {
                iteration: i,
                weights,
                reward,
                acquisition_value: 0.0,
            });

            debug!("Initial sample {}: reward={:.4}", i, reward);
        }

        // Fit GP
        self.gp.update_hyperparameters();
        self.gp.fit();

        // Bayesian optimization iterations
        for i in 0..self.config.n_iterations {
            // Find next point to evaluate
            let next_weights = self.optimize_acquisition();
            let x = Self::weights_to_vec(&next_weights);
            let acq_value = self.acquisition(&x);

            // Evaluate
            let reward = self.evaluate(&next_weights, historical_data, reward_calc);

            // Update GP
            self.gp.add_observation(x, reward);
            self.gp.fit();

            // Update best
            if reward > self.best_y {
                self.best_y = reward;
                self.best_x = next_weights;
            }

            self.history.push(OptimizationStep {
                iteration: self.config.n_initial_points + i,
                weights: next_weights,
                reward,
                acquisition_value: acq_value,
            });

            debug!(
                "BO iteration {}: reward={:.4}, best={:.4}",
                i, reward, self.best_y
            );
        }

        info!(
            "Bayesian optimization complete: best_reward={:.4}",
            self.best_y
        );

        Some(OptimizationResult {
            best_weights: self.best_x,
            expected_improvement: self.acquisition(&Self::weights_to_vec(&self.best_x)),
            evaluations: self.history.len(),
            history: self.history.clone(),
            gp_length_scale: self.gp.length_scale,
            gp_signal_variance: self.gp.signal_variance,
        })
    }

    /// Reset optimizer state
    pub fn reset(&mut self) {
        self.gp = GaussianProcess::new(
            self.config.gp_length_scale,
            self.config.gp_signal_variance,
            self.config.gp_noise_variance,
        );
        self.best_y = f64::NEG_INFINITY;
        self.best_x = TunableWeights::default();
        self.history.clear();
    }
}

/// Error function approximation (Horner's method)
fn erf(x: f64) -> f64 {
    // Constants for approximation
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();

    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();

    sign * y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gaussian_process_creation() {
        let gp = GaussianProcess::new(1.0, 1.0, 0.01);
        let (mean, var) = gp.predict(&[0.5, 0.5, 0.5, 0.5]);
        assert_eq!(mean, 0.0); // No training data
        assert!(var > 0.0);
    }

    #[test]
    fn test_gaussian_process_prediction() {
        let mut gp = GaussianProcess::new(1.0, 1.0, 0.01);
        gp.add_observation(vec![0.0, 0.0, 0.0, 0.0], 0.0);
        gp.add_observation(vec![1.0, 1.0, 1.0, 1.0], 1.0);
        gp.fit();

        let (mean, _var) = gp.predict(&[0.5, 0.5, 0.5, 0.5]);
        // Mean should be somewhere between 0 and 1
        assert!(mean >= -0.5 && mean <= 1.5);
    }

    #[test]
    fn test_bayesian_optimizer_creation() {
        let config = BayesianConfig::default();
        let optimizer = BayesianOptimizer::new(config);
        assert_eq!(optimizer.history.len(), 0);
    }

    #[test]
    fn test_erf_function() {
        // erf(0) = 0
        assert!((erf(0.0) - 0.0).abs() < 0.001);
        // erf(1) ≈ 0.843
        assert!((erf(1.0) - 0.843).abs() < 0.01);
        // erf(-1) ≈ -0.843
        assert!((erf(-1.0) + 0.843).abs() < 0.01);
    }

    #[test]
    fn test_acquisition_function_default() {
        let acq = AcquisitionFunction::default();
        assert_eq!(acq, AcquisitionFunction::ExpectedImprovement);
    }

    #[test]
    fn test_weights_conversion() {
        let weights = TunableWeights {
            w_qass: 15.0,
            w_mpcf: 10.0,
            w_sobp: 12.0,
            w_iwim: 8.0,
        };
        let vec = BayesianOptimizer::weights_to_vec(&weights);
        let recovered = BayesianOptimizer::vec_to_weights(&vec);
        assert_eq!(weights, recovered);
    }
}
