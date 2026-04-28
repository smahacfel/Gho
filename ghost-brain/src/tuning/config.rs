//! Tuning Configuration
//!
//! Configuration structures for weight tuning mechanisms including:
//! - Bandit algorithm parameters
//! - Bayesian optimization settings
//! - Frozen parameter configuration

use crate::tuning::bayesian::AcquisitionFunction;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Main tuning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningConfig {
    /// Whether tuning is enabled
    pub enabled: bool,

    /// Bandit configuration
    pub bandit: BanditConfig,

    /// Bayesian optimization configuration
    pub bayesian: BayesianConfig,

    /// Reward calculation configuration
    pub reward: RewardConfig,

    /// Frozen parameters configuration
    pub frozen: FrozenConfig,
}

impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bandit: BanditConfig::default(),
            bayesian: BayesianConfig::default(),
            reward: RewardConfig::default(),
            frozen: FrozenConfig::default(),
        }
    }
}

impl TuningConfig {
    /// Load configuration from TOML file
    pub fn load_from_toml(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: TuningConfig = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        self.bandit.validate()?;
        self.bayesian.validate()?;
        self.reward.validate()?;
        Ok(())
    }
}

/// Bandit algorithm configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanditConfig {
    /// Update interval in seconds (1-5 minutes typical)
    pub update_interval_seconds: u64,

    /// Learning rate for weight updates (0.0 to 1.0)
    pub learning_rate: f32,

    /// Exploration parameter for LinUCB
    pub exploration_alpha: f32,

    /// Minimum weight value
    pub min_weight: f32,

    /// Maximum weight value
    pub max_weight: f32,

    /// Base weight value (initial/default)
    pub base_weight: f32,

    /// Target sum of all weights for normalization
    pub target_weight_sum: f32,

    /// Number of context features
    pub n_features: usize,

    /// Discount factor for older observations (0.0 to 1.0)
    pub discount_factor: f32,
}

impl Default for BanditConfig {
    fn default() -> Self {
        Self {
            update_interval_seconds: 180, // 3 minutes
            learning_rate: 0.1,
            exploration_alpha: 1.0,
            min_weight: 1.0,
            max_weight: 30.0,
            base_weight: 10.0,
            target_weight_sum: 45.0, // Sum of default weights
            n_features: 7,
            discount_factor: 0.95,
        }
    }
}

impl BanditConfig {
    /// Validate configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.update_interval_seconds < 60 || self.update_interval_seconds > 600 {
            anyhow::bail!(
                "update_interval_seconds must be between 60 and 600, got {}",
                self.update_interval_seconds
            );
        }
        if self.learning_rate <= 0.0 || self.learning_rate > 1.0 {
            anyhow::bail!(
                "learning_rate must be in (0.0, 1.0], got {}",
                self.learning_rate
            );
        }
        if self.exploration_alpha < 0.0 {
            anyhow::bail!(
                "exploration_alpha must be non-negative, got {}",
                self.exploration_alpha
            );
        }
        if self.min_weight < 0.0 || self.max_weight <= self.min_weight {
            anyhow::bail!(
                "Invalid weight range: [{}, {}]",
                self.min_weight,
                self.max_weight
            );
        }
        Ok(())
    }

    /// Get update interval as Duration
    pub fn update_interval(&self) -> Duration {
        Duration::from_secs(self.update_interval_seconds)
    }
}

/// Bayesian optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BayesianConfig {
    /// Whether Bayesian optimization is enabled
    pub enabled: bool,

    /// Optimization cycle duration
    #[serde(with = "duration_serde")]
    pub cycle_duration: Duration,

    /// Number of initial random samples
    pub n_initial_points: usize,

    /// Number of optimization iterations
    pub n_iterations: usize,

    /// Number of random samples for acquisition optimization
    pub n_random_samples: usize,

    /// Acquisition function to use
    pub acquisition_function: AcquisitionFunction,

    /// Exploration-exploitation tradeoff for UCB
    pub beta: f64,

    /// Improvement threshold for EI
    pub xi: f64,

    /// GP length scale
    pub gp_length_scale: f64,

    /// GP signal variance
    pub gp_signal_variance: f64,

    /// GP noise variance
    pub gp_noise_variance: f64,

    /// Blend factor for applying optimized weights (0.0 to 1.0)
    pub blend_factor: f32,

    /// Minimum weight value
    pub min_weight: f32,

    /// Maximum weight value
    pub max_weight: f32,
}

impl Default for BayesianConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cycle_duration: Duration::from_secs(12 * 3600), // 12 hours
            n_initial_points: 10,
            n_iterations: 50,
            n_random_samples: 100,
            acquisition_function: AcquisitionFunction::ExpectedImprovement,
            beta: 2.0,
            xi: 0.01,
            gp_length_scale: 1.0,
            gp_signal_variance: 1.0,
            gp_noise_variance: 0.01,
            blend_factor: 0.3, // Conservative blending
            min_weight: 1.0,
            max_weight: 30.0,
        }
    }
}

impl BayesianConfig {
    /// Validate configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.n_initial_points < 3 {
            anyhow::bail!(
                "n_initial_points must be at least 3, got {}",
                self.n_initial_points
            );
        }
        if self.n_iterations == 0 {
            anyhow::bail!("n_iterations must be positive");
        }
        if self.blend_factor < 0.0 || self.blend_factor > 1.0 {
            anyhow::bail!(
                "blend_factor must be in [0.0, 1.0], got {}",
                self.blend_factor
            );
        }
        if self.gp_length_scale <= 0.0 {
            anyhow::bail!(
                "gp_length_scale must be positive, got {}",
                self.gp_length_scale
            );
        }
        if self.gp_signal_variance <= 0.0 {
            anyhow::bail!(
                "gp_signal_variance must be positive, got {}",
                self.gp_signal_variance
            );
        }
        Ok(())
    }
}

/// Reward calculation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardConfig {
    /// Base reward for successful trade
    pub success_reward: f32,

    /// Penalty for false positive (BUY signal that shouldn't have been)
    pub false_positive_penalty: f32,

    /// Penalty for missed opportunity (should have bought)
    pub false_negative_penalty: f32,

    /// Reward scaling factor for profit
    pub profit_scale: f32,

    /// Penalty scaling factor for loss
    pub loss_scale: f32,

    /// Maximum reward value (clipping)
    pub max_reward: f32,

    /// Minimum reward value (clipping)
    pub min_reward: f32,

    /// Discount factor for time-delayed outcomes
    pub time_discount: f32,
}

impl Default for RewardConfig {
    fn default() -> Self {
        Self {
            success_reward: 1.0,
            false_positive_penalty: -0.5,
            false_negative_penalty: -0.2,
            profit_scale: 2.0,
            loss_scale: 1.5,
            max_reward: 5.0,
            min_reward: -3.0,
            time_discount: 0.99,
        }
    }
}

impl RewardConfig {
    /// Validate configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_reward <= self.min_reward {
            anyhow::bail!(
                "max_reward must be greater than min_reward: {} <= {}",
                self.max_reward,
                self.min_reward
            );
        }
        if self.time_discount <= 0.0 || self.time_discount > 1.0 {
            anyhow::bail!(
                "time_discount must be in (0.0, 1.0], got {}",
                self.time_discount
            );
        }
        Ok(())
    }
}

/// Configuration for frozen parameters fallback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenConfig {
    /// Path to frozen parameters TOML file
    pub file_path: Option<String>,

    /// Auto-freeze on drift detection
    pub auto_freeze_on_drift: bool,

    /// Drift detection threshold (cumulative reward drop)
    pub drift_threshold: f32,

    /// Window size for drift detection (number of updates)
    pub drift_window: usize,
}

impl Default for FrozenConfig {
    fn default() -> Self {
        Self {
            file_path: None,
            auto_freeze_on_drift: true,
            drift_threshold: -2.0,
            drift_window: 100,
        }
    }
}

/// Serde helper for Duration
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tuning_config_default() {
        let config = TuningConfig::default();
        assert!(config.enabled);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_bandit_config_default() {
        let config = BanditConfig::default();
        assert_eq!(config.update_interval_seconds, 180);
        assert_eq!(config.learning_rate, 0.1);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_bandit_config_validation() {
        let mut config = BanditConfig::default();

        // Invalid update interval
        config.update_interval_seconds = 30;
        assert!(config.validate().is_err());
        config.update_interval_seconds = 180;

        // Invalid learning rate
        config.learning_rate = 0.0;
        assert!(config.validate().is_err());
        config.learning_rate = 0.1;

        // Invalid weight range
        config.min_weight = 10.0;
        config.max_weight = 5.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_bayesian_config_default() {
        let config = BayesianConfig::default();
        assert!(config.enabled);
        assert_eq!(config.cycle_duration, Duration::from_secs(12 * 3600));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_bayesian_config_validation() {
        let mut config = BayesianConfig::default();

        // Invalid initial points
        config.n_initial_points = 1;
        assert!(config.validate().is_err());
        config.n_initial_points = 10;

        // Invalid blend factor
        config.blend_factor = 1.5;
        assert!(config.validate().is_err());
        config.blend_factor = 0.3;

        // Invalid GP parameters
        config.gp_length_scale = 0.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_reward_config_default() {
        let config = RewardConfig::default();
        assert_eq!(config.success_reward, 1.0);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_reward_config_validation() {
        let mut config = RewardConfig::default();

        // Invalid reward range
        config.max_reward = -5.0;
        assert!(config.validate().is_err());
        config.max_reward = 5.0;

        // Invalid time discount
        config.time_discount = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = TuningConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let _: TuningConfig = toml::from_str(&toml_str).unwrap();
    }
}
