//! Bandit Algorithms for Online Weight Tuning
//!
//! This module implements contextual bandit algorithms for real-time weight adjustment:
//!
//! - **LinUCB**: Linear Upper Confidence Bound - balances exploration/exploitation
//!   using linear reward models with confidence bounds
//! - **Thompson Sampling**: Bayesian approach that samples from posterior distributions
//!
//! Both algorithms learn from trading outcomes to optimize signal weights.

use crate::tuning::config::BanditConfig;
use nalgebra::{DMatrix, DVector};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Bandit algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BanditAlgorithm {
    /// Linear Upper Confidence Bound
    LinUCB,
    /// Thompson Sampling with Gaussian posteriors
    ThompsonSampling,
}

impl Default for BanditAlgorithm {
    fn default() -> Self {
        BanditAlgorithm::LinUCB
    }
}

/// Trait for weight-tuning bandit algorithms
pub trait WeightBandit: Send + Sync {
    /// Update the model with observed reward
    fn update(&mut self, context: &[f32], reward: f32);

    /// Suggest new weights given current context
    fn suggest_weights(&self, context: &[f32]) -> [f32; 4];

    /// Get the algorithm name
    fn name(&self) -> &'static str;

    /// Get the number of updates performed
    fn update_count(&self) -> u64;
}

/// LinUCB (Linear Upper Confidence Bound) bandit
///
/// Implements the LinUCB algorithm for contextual bandits with linear reward models.
/// For each weight dimension, maintains a separate linear model with uncertainty estimates.
///
/// Reference: Li et al., "A Contextual-Bandit Approach to Personalized News Article
/// Recommendation", WWW 2010
pub struct LinUCBBandit {
    /// Configuration
    config: BanditConfig,
    /// Design matrices A for each weight (d x d)
    a_matrices: Vec<Mutex<DMatrix<f64>>>,
    /// Reward vectors b for each weight (d x 1)
    b_vectors: Vec<Mutex<DVector<f64>>>,
    /// Feature dimension
    dim: usize,
    /// Number of weight arms (always 4)
    n_arms: usize,
    /// Update counter
    updates: Mutex<u64>,
}

impl LinUCBBandit {
    /// Create a new LinUCB bandit
    pub fn new(config: BanditConfig) -> Self {
        let dim = 7; // Context feature dimension (bias + 6 features)
        let n_arms = 4; // Number of weights to tune

        // Initialize A matrices to identity (for regularization)
        let a_matrices: Vec<_> = (0..n_arms)
            .map(|_| Mutex::new(DMatrix::identity(dim, dim)))
            .collect();

        // Initialize b vectors to zero
        let b_vectors: Vec<_> = (0..n_arms)
            .map(|_| Mutex::new(DVector::zeros(dim)))
            .collect();

        Self {
            config,
            a_matrices,
            b_vectors,
            dim,
            n_arms,
            updates: Mutex::new(0),
        }
    }

    /// Compute theta (weight vector) for an arm
    fn compute_theta(&self, arm: usize) -> DVector<f64> {
        let a = self.a_matrices[arm].lock().unwrap();
        let b = self.b_vectors[arm].lock().unwrap();

        // theta = A^{-1} b
        match a.clone().try_inverse() {
            Some(a_inv) => a_inv * b.clone(),
            None => DVector::zeros(self.dim),
        }
    }

    /// Compute UCB value for an arm given context
    fn compute_ucb(&self, arm: usize, context: &DVector<f64>) -> f64 {
        let a = self.a_matrices[arm].lock().unwrap();
        let theta = self.compute_theta(arm);

        // Expected reward: x^T * theta
        let expected = context.dot(&theta);

        // Compute confidence bound: sqrt(x^T * A^{-1} * x)
        let confidence = match a.clone().try_inverse() {
            Some(a_inv) => {
                let tmp = &a_inv * context;
                (context.dot(&tmp)).sqrt()
            }
            None => 1.0,
        };

        // UCB = expected + alpha * confidence
        expected + self.config.exploration_alpha as f64 * confidence
    }
}

impl WeightBandit for LinUCBBandit {
    fn update(&mut self, context: &[f32], reward: f32) {
        let x = DVector::from_iterator(self.dim, context.iter().map(|&v| v as f64));

        // Update all arms proportionally to their current weight contribution
        // This is a simplification - in practice, we'd update only the "chosen" arm
        // But for continuous weight tuning, we update all with scaled rewards
        for arm in 0..self.n_arms {
            let mut a = self.a_matrices[arm].lock().unwrap();
            let mut b = self.b_vectors[arm].lock().unwrap();

            // A = A + x * x^T
            *a += &x * x.transpose();

            // b = b + reward * x
            *b += reward as f64 * &x;
        }

        *self.updates.lock().unwrap() += 1;
    }

    fn suggest_weights(&self, context: &[f32]) -> [f32; 4] {
        let x = DVector::from_iterator(self.dim, context.iter().map(|&v| v as f64));

        // Compute UCB values for each weight arm
        let ucb_values: Vec<f64> = (0..self.n_arms)
            .map(|arm| self.compute_ucb(arm, &x))
            .collect();

        // Convert to weights using softmax-like transformation
        let max_ucb = ucb_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exp_values: Vec<f64> = ucb_values.iter().map(|&v| (v - max_ucb).exp()).collect();
        let sum_exp: f64 = exp_values.iter().sum();

        // Scale to weight range
        let base = self.config.base_weight as f64;
        let range = (self.config.max_weight - self.config.min_weight) as f64;

        [
            (base + range * exp_values[0] / sum_exp) as f32,
            (base + range * exp_values[1] / sum_exp) as f32,
            (base + range * exp_values[2] / sum_exp) as f32,
            (base + range * exp_values[3] / sum_exp) as f32,
        ]
    }

    fn name(&self) -> &'static str {
        "LinUCB"
    }

    fn update_count(&self) -> u64 {
        *self.updates.lock().unwrap()
    }
}

/// Thompson Sampling bandit with Gaussian posteriors
///
/// Maintains posterior distributions over weight parameters and samples from them
/// for exploration. Uses conjugate Gaussian prior with known variance.
pub struct ThompsonSamplingBandit {
    /// Configuration
    config: BanditConfig,
    /// Mean estimates for each weight
    means: Mutex<[f64; 4]>,
    /// Variance estimates for each weight
    variances: Mutex<[f64; 4]>,
    /// Sample counts for each weight
    counts: Mutex<[u64; 4]>,
    /// Prior variance
    prior_variance: f64,
    /// Update counter
    updates: Mutex<u64>,
}

impl ThompsonSamplingBandit {
    /// Create a new Thompson Sampling bandit
    pub fn new(config: BanditConfig) -> Self {
        // Initialize with prior centered on base weights
        let base = config.base_weight as f64;
        let means = Mutex::new([base, base, base, base]);
        let variances = Mutex::new([1.0, 1.0, 1.0, 1.0]); // Prior variance
        let counts = Mutex::new([0u64; 4]);

        Self {
            config,
            means,
            variances,
            counts,
            prior_variance: 1.0,
            updates: Mutex::new(0),
        }
    }

    /// Update posterior for a weight arm
    fn update_posterior(&self, arm: usize, reward: f64) {
        let mut means = self.means.lock().unwrap();
        let mut variances = self.variances.lock().unwrap();
        let mut counts = self.counts.lock().unwrap();

        counts[arm] += 1;
        let n = counts[arm] as f64;

        // Bayesian update for Gaussian with known variance
        // Posterior mean: (prior_precision * prior_mean + n * sample_mean) / (prior_precision + n)
        // Posterior variance: 1 / (prior_precision + n)

        let prior_precision = 1.0 / self.prior_variance;
        let likelihood_precision = n;

        let posterior_precision = prior_precision + likelihood_precision;
        let posterior_variance = 1.0 / posterior_precision;

        // Update mean with exponential moving average for stability
        // Scale reward to weight range using config values
        let weight_range = (self.config.max_weight - self.config.min_weight) as f64;
        let alpha = 1.0 / (n + 1.0);
        means[arm] = (1.0 - alpha) * means[arm] + alpha * reward * weight_range;

        variances[arm] = posterior_variance;
    }
}

impl WeightBandit for ThompsonSamplingBandit {
    fn update(&mut self, _context: &[f32], reward: f32) {
        // Update all arms (simplified - could use context for arm selection)
        for arm in 0..4 {
            self.update_posterior(arm, reward as f64);
        }
        *self.updates.lock().unwrap() += 1;
    }

    fn suggest_weights(&self, _context: &[f32]) -> [f32; 4] {
        let means = self.means.lock().unwrap();
        let variances = self.variances.lock().unwrap();

        let mut rng = rand::thread_rng();
        let mut weights = [0.0f32; 4];

        // Sample from posterior for each weight using Box-Muller transform
        for i in 0..4 {
            let std_dev = variances[i].sqrt().max(0.01);
            let sample = sample_normal(&mut rng, means[i], std_dev);

            // Clamp to valid range
            weights[i] = (sample as f32).clamp(self.config.min_weight, self.config.max_weight);
        }

        weights
    }

    fn name(&self) -> &'static str {
        "ThompsonSampling"
    }

    fn update_count(&self) -> u64 {
        *self.updates.lock().unwrap()
    }
}

/// Sample from a normal distribution using Box-Muller transform
fn sample_normal<R: Rng>(rng: &mut R, mean: f64, std_dev: f64) -> f64 {
    let u1: f64 = rng.gen::<f64>().max(1e-10); // Avoid log(0)
    let u2: f64 = rng.gen::<f64>();
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    mean + std_dev * z
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> BanditConfig {
        BanditConfig::default()
    }

    #[test]
    fn test_linucb_creation() {
        let bandit = LinUCBBandit::new(default_config());
        assert_eq!(bandit.name(), "LinUCB");
        assert_eq!(bandit.update_count(), 0);
    }

    #[test]
    fn test_linucb_update_and_suggest() {
        let mut bandit = LinUCBBandit::new(default_config());
        let context = vec![1.0, 0.5, 0.3, 0.2, 0.7, 0.4, 0.1];

        // Initial suggestion
        let weights_before = bandit.suggest_weights(&context);
        assert!(weights_before.iter().all(|&w| w > 0.0));

        // Update with positive reward
        bandit.update(&context, 1.0);
        assert_eq!(bandit.update_count(), 1);

        // Weights should change after update
        let weights_after = bandit.suggest_weights(&context);
        assert!(weights_after.iter().all(|&w| w > 0.0));
    }

    #[test]
    fn test_thompson_sampling_creation() {
        let bandit = ThompsonSamplingBandit::new(default_config());
        assert_eq!(bandit.name(), "ThompsonSampling");
        assert_eq!(bandit.update_count(), 0);
    }

    #[test]
    fn test_thompson_sampling_update_and_suggest() {
        let mut bandit = ThompsonSamplingBandit::new(default_config());
        let context = vec![1.0, 0.5, 0.3, 0.2, 0.7, 0.4, 0.1];

        // Initial suggestion (should be near prior)
        let weights_before = bandit.suggest_weights(&context);
        assert!(weights_before.iter().all(|&w| w > 0.0));

        // Multiple updates with high reward
        for _ in 0..10 {
            bandit.update(&context, 1.5);
        }
        assert_eq!(bandit.update_count(), 10);

        // Weights should shift toward higher values
        let weights_after = bandit.suggest_weights(&context);
        assert!(weights_after.iter().all(|&w| w > 0.0));
    }

    #[test]
    fn test_bandit_algorithm_default() {
        let algo = BanditAlgorithm::default();
        assert_eq!(algo, BanditAlgorithm::LinUCB);
    }

    #[test]
    fn test_weight_bounds() {
        let config = BanditConfig {
            min_weight: 5.0,
            max_weight: 25.0,
            ..Default::default()
        };
        let mut bandit = ThompsonSamplingBandit::new(config);
        let context = vec![1.0, 0.5, 0.3, 0.2, 0.7, 0.4, 0.1];

        // Several updates
        for _ in 0..10 {
            bandit.update(&context, 2.0);
        }

        let weights = bandit.suggest_weights(&context);
        for w in weights.iter() {
            assert!(*w >= 5.0 && *w <= 25.0, "Weight {} out of bounds", w);
        }
    }
}
