//! Frozen Parameters for Manual Override
//!
//! Provides fallback mechanism to freeze tuning parameters via TOML configuration
//! when drift is detected or manual intervention is required.

use crate::tuning::TunableWeights;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{info, warn};

/// Frozen parameter state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenParameters {
    /// Whether freezing is enabled
    pub enabled: bool,

    /// Frozen weight values
    pub weights: TunableWeights,

    /// Reason for freezing
    pub reason: String,

    /// When parameters were frozen
    pub frozen_at: DateTime<Utc>,
}

impl Default for FrozenParameters {
    fn default() -> Self {
        Self {
            enabled: false,
            weights: TunableWeights::default(),
            reason: String::new(),
            frozen_at: Utc::now(),
        }
    }
}

impl FrozenParameters {
    /// Create new frozen parameters
    pub fn new(weights: TunableWeights, reason: &str) -> Self {
        Self {
            enabled: true,
            weights,
            reason: reason.to_string(),
            frozen_at: Utc::now(),
        }
    }

    /// Load frozen parameters from TOML file
    pub fn load_from_toml(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let params: FrozenParameters = toml::from_str(&content)?;
        info!("Loaded frozen parameters from {}", path);
        Ok(params)
    }

    /// Save frozen parameters to TOML file
    pub fn save_to_toml(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        info!("Saved frozen parameters to {}", path);
        Ok(())
    }

    /// Check if file exists
    pub fn file_exists(path: &str) -> bool {
        Path::new(path).exists()
    }
}

/// Manager for parameter freezing with drift detection
pub struct ParameterFreezer {
    /// Current frozen state
    frozen: Option<FrozenParameters>,

    /// Recent rewards for drift detection
    recent_rewards: Vec<f32>,

    /// Window size for drift detection
    window_size: usize,

    /// Threshold for automatic freezing
    drift_threshold: f32,

    /// Path to frozen parameters file
    file_path: Option<String>,

    /// Whether auto-freeze is enabled
    auto_freeze_enabled: bool,
}

impl ParameterFreezer {
    /// Create a new parameter freezer
    pub fn new(
        window_size: usize,
        drift_threshold: f32,
        file_path: Option<String>,
        auto_freeze_enabled: bool,
    ) -> Self {
        Self {
            frozen: None,
            recent_rewards: Vec::with_capacity(window_size),
            window_size,
            drift_threshold,
            file_path,
            auto_freeze_enabled,
        }
    }

    /// Create with defaults
    pub fn default() -> Self {
        Self::new(100, -2.0, None, true)
    }

    /// Load frozen parameters from file if exists
    pub fn load_from_file(&mut self) -> anyhow::Result<bool> {
        if let Some(ref path) = self.file_path {
            if FrozenParameters::file_exists(path) {
                let params = FrozenParameters::load_from_toml(path)?;
                if params.enabled {
                    self.frozen = Some(params);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Check if parameters are frozen
    pub fn is_frozen(&self) -> bool {
        self.frozen.as_ref().map(|f| f.enabled).unwrap_or(false)
    }

    /// Get frozen weights if available
    pub fn frozen_weights(&self) -> Option<TunableWeights> {
        self.frozen
            .as_ref()
            .filter(|f| f.enabled)
            .map(|f| f.weights)
    }

    /// Manually freeze parameters
    pub fn freeze(&mut self, weights: TunableWeights, reason: &str) {
        let params = FrozenParameters::new(weights, reason);
        warn!("Freezing parameters: {}", reason);

        // Save to file if configured
        if let Some(ref path) = self.file_path {
            if let Err(e) = params.save_to_toml(path) {
                warn!("Failed to save frozen parameters: {}", e);
            }
        }

        self.frozen = Some(params);
    }

    /// Unfreeze parameters
    pub fn unfreeze(&mut self) {
        if let Some(ref mut frozen) = self.frozen {
            frozen.enabled = false;
            info!("Unfreezing parameters");

            // Update file if configured
            if let Some(ref path) = self.file_path {
                if let Err(e) = frozen.save_to_toml(path) {
                    warn!("Failed to save unfrozen state: {}", e);
                }
            }
        }
        self.frozen = None;
    }

    /// Record a reward and check for drift
    pub fn record_reward(&mut self, reward: f32, current_weights: TunableWeights) -> bool {
        self.recent_rewards.push(reward);

        // Keep window size
        while self.recent_rewards.len() > self.window_size {
            self.recent_rewards.remove(0);
        }

        // Check for drift
        if self.auto_freeze_enabled && self.should_freeze() {
            self.freeze(current_weights, "Automatic freeze due to detected drift");
            return true;
        }

        false
    }

    /// Check if we should freeze based on recent performance
    fn should_freeze(&self) -> bool {
        if self.recent_rewards.len() < self.window_size / 2 {
            return false;
        }

        // Calculate cumulative reward over window
        let cumulative: f32 = self.recent_rewards.iter().sum();
        let avg = cumulative / self.recent_rewards.len() as f32;

        // Freeze if average reward is below threshold
        if avg < self.drift_threshold {
            warn!(
                "Drift detected: avg_reward={:.4} < threshold={:.4}",
                avg, self.drift_threshold
            );
            return true;
        }

        // Check for sudden drops
        if self.recent_rewards.len() >= 20 {
            let recent_20: f32 = self.recent_rewards.iter().rev().take(20).sum();
            let recent_avg = recent_20 / 20.0;

            let earlier_20: f32 = self.recent_rewards.iter().take(20).sum();
            let earlier_avg = earlier_20 / 20.0;

            if earlier_avg > 0.0 && recent_avg < earlier_avg * 0.5 {
                warn!(
                    "Sudden performance drop detected: recent={:.4}, earlier={:.4}",
                    recent_avg, earlier_avg
                );
                return true;
            }
        }

        false
    }

    /// Get drift statistics
    pub fn drift_stats(&self) -> DriftStats {
        if self.recent_rewards.is_empty() {
            return DriftStats::default();
        }

        let cumulative: f32 = self.recent_rewards.iter().sum();
        let avg = cumulative / self.recent_rewards.len() as f32;

        let variance: f32 = self
            .recent_rewards
            .iter()
            .map(|r| (r - avg).powi(2))
            .sum::<f32>()
            / self.recent_rewards.len() as f32;

        DriftStats {
            window_size: self.recent_rewards.len(),
            cumulative_reward: cumulative,
            average_reward: avg,
            variance,
            is_frozen: self.is_frozen(),
            freeze_reason: self.frozen.as_ref().map(|f| f.reason.clone()),
        }
    }

    /// Clear reward history
    pub fn clear_history(&mut self) {
        self.recent_rewards.clear();
    }
}

/// Drift detection statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftStats {
    /// Current window size
    pub window_size: usize,
    /// Cumulative reward in window
    pub cumulative_reward: f32,
    /// Average reward in window
    pub average_reward: f32,
    /// Variance in window
    pub variance: f32,
    /// Whether parameters are frozen
    pub is_frozen: bool,
    /// Reason for freeze if frozen
    pub freeze_reason: Option<String>,
}

impl Default for DriftStats {
    fn default() -> Self {
        Self {
            window_size: 0,
            cumulative_reward: 0.0,
            average_reward: 0.0,
            variance: 0.0,
            is_frozen: false,
            freeze_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frozen_parameters_creation() {
        let weights = TunableWeights::default();
        let frozen = FrozenParameters::new(weights, "Test freeze");
        assert!(frozen.enabled);
        assert_eq!(frozen.reason, "Test freeze");
    }

    #[test]
    fn test_frozen_parameters_default() {
        let frozen = FrozenParameters::default();
        assert!(!frozen.enabled);
        assert!(frozen.reason.is_empty());
    }

    #[test]
    fn test_parameter_freezer_creation() {
        let freezer = ParameterFreezer::default();
        assert!(!freezer.is_frozen());
        assert!(freezer.frozen_weights().is_none());
    }

    #[test]
    fn test_manual_freeze() {
        let mut freezer = ParameterFreezer::default();
        let weights = TunableWeights {
            w_qass: 20.0,
            w_mpcf: 15.0,
            w_sobp: 10.0,
            w_iwim: 5.0,
        };

        freezer.freeze(weights, "Manual test");
        assert!(freezer.is_frozen());
        assert_eq!(freezer.frozen_weights().unwrap().w_qass, 20.0);
    }

    #[test]
    fn test_unfreeze() {
        let mut freezer = ParameterFreezer::default();
        let weights = TunableWeights::default();

        freezer.freeze(weights, "Test");
        assert!(freezer.is_frozen());

        freezer.unfreeze();
        assert!(!freezer.is_frozen());
    }

    #[test]
    fn test_drift_detection_negative_rewards() {
        let mut freezer = ParameterFreezer::new(10, -0.5, None, true);
        let weights = TunableWeights::default();

        // Record many negative rewards
        for _ in 0..10 {
            freezer.record_reward(-1.0, weights);
        }

        // Should be frozen due to drift
        assert!(freezer.is_frozen());
    }

    #[test]
    fn test_drift_detection_positive_rewards() {
        let mut freezer = ParameterFreezer::new(10, -0.5, None, true);
        let weights = TunableWeights::default();

        // Record positive rewards
        for _ in 0..10 {
            freezer.record_reward(1.0, weights);
        }

        // Should not be frozen
        assert!(!freezer.is_frozen());
    }

    #[test]
    fn test_drift_stats() {
        let mut freezer = ParameterFreezer::default();
        let weights = TunableWeights::default();

        freezer.record_reward(1.0, weights);
        freezer.record_reward(2.0, weights);
        freezer.record_reward(3.0, weights);

        let stats = freezer.drift_stats();
        assert_eq!(stats.window_size, 3);
        assert!((stats.cumulative_reward - 6.0).abs() < 0.001);
        assert!((stats.average_reward - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_clear_history() {
        let mut freezer = ParameterFreezer::default();
        let weights = TunableWeights::default();

        freezer.record_reward(1.0, weights);
        freezer.record_reward(2.0, weights);

        freezer.clear_history();
        let stats = freezer.drift_stats();
        assert_eq!(stats.window_size, 0);
    }

    #[test]
    fn test_frozen_parameters_serialization() {
        let weights = TunableWeights {
            w_qass: 18.0,
            w_mpcf: 12.0,
            w_sobp: 14.0,
            w_iwim: 6.0,
        };
        let frozen = FrozenParameters::new(weights, "Serialization test");

        let toml_str = toml::to_string(&frozen).unwrap();
        let recovered: FrozenParameters = toml::from_str(&toml_str).unwrap();

        assert_eq!(recovered.enabled, frozen.enabled);
        assert_eq!(recovered.weights.w_qass, frozen.weights.w_qass);
        assert_eq!(recovered.reason, frozen.reason);
    }
}
