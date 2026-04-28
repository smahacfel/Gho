//! MCI Configuration
//!
//! Configuration for the Market Coherence Index (MCI) engine.
//! This module defines parameters for computing directional and structural coherence.

use serde::{Deserialize, Serialize};

/// Configuration for MCI engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MciConfig {
    /// Configuration version for API compatibility
    pub version: u8,

    /// Weight for Directional Coherence component
    #[serde(alias = "w_dc")]
    pub weight_dc: f32,

    /// Weight for Structural Coherence component
    #[serde(alias = "w_sc")]
    pub weight_sc: f32,

    /// Coherence abort threshold - if coherence drops below this, trading is vetoed
    pub coherence_abort_threshold: f32,

    /// Optional initial state to warm-start sentiment
    #[serde(default)]
    pub initial_state: Option<MciInitialState>,
}

/// Initial state configuration for the MCI engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MciInitialState {
    /// Base sentiment to seed short-term memory [0.0, 1.0]
    pub base_sentiment: f64,

    /// Volatility index [0.0, 1.0] controlling blending strength
    pub volatility_index: f64,

    /// Whether to override computed MCI with the base sentiment
    #[serde(default)]
    pub force_override: bool,
}

impl Default for MciConfig {
    fn default() -> Self {
        Self {
            version: 1,
            weight_dc: 0.6,
            weight_sc: 0.4,
            coherence_abort_threshold: 0.3,
            initial_state: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MciConfig::default();
        assert_eq!(config.version, 1);
        assert_eq!(config.weight_dc, 0.6);
        assert_eq!(config.weight_sc, 0.4);
        assert!(config.initial_state.is_none());
    }

    #[test]
    fn test_serialization() {
        let config = MciConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: MciConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config.version, deserialized.version);
        assert_eq!(config.weight_dc, deserialized.weight_dc);
    }

    #[test]
    fn test_weights_sum() {
        let config = MciConfig::default();
        let sum = config.weight_dc + config.weight_sc;
        assert!(
            (sum - 1.0).abs() < 0.001,
            "Weights should sum to approximately 1.0"
        );
    }

    #[test]
    fn test_initial_state_roundtrip() {
        let config = MciConfig {
            initial_state: Some(MciInitialState {
                base_sentiment: 0.85,
                volatility_index: 0.25,
                force_override: true,
            }),
            ..Default::default()
        };
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: MciConfig = toml::from_str(&serialized).unwrap();
        let init = deserialized.initial_state.unwrap();
        assert!((init.base_sentiment - 0.85).abs() < f64::EPSILON);
        assert!((init.volatility_index - 0.25).abs() < f64::EPSILON);
        assert!(init.force_override);
    }
}
