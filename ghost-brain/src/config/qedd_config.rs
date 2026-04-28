//! QEDD Configuration
//!
//! Configuration for the Quantum Entropy-Driven Decay (QEDD) engine.
//! This module defines parameters for computing survival probabilities
//! across multiple time horizons.

use serde::{Deserialize, Serialize};

/// Configuration for QEDD engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QeddConfig {
    /// Configuration version for API compatibility
    pub version: u8,

    /// Base lambda parameter for decay rate calculation
    pub lambda_base: f32,

    /// Sensitivity factor for lambda adjustments
    ///
    /// **Deprecated**: This field is kept for backward compatibility but is no longer used.
    /// Use the specific coefficients (alpha_sobp_drop, beta_outflow, etc.) instead.
    #[deprecated(since = "1.1.0", note = "Use specific coefficients instead")]
    pub lambda_sensitivity: f32,

    /// Abort threshold - if lambda exceeds this, trading is vetoed
    pub lambda_abort_threshold: f32,

    /// Coefficient α for SOBP drop contribution to hazard rate
    pub alpha_sobp_drop: f32,

    /// Coefficient β for outflow contribution to hazard rate
    pub beta_outflow: f32,

    /// Coefficient γ for resonance risk contribution to hazard rate
    pub gamma_resonance: f32,

    /// Coefficient δ for deviation risk contribution to hazard rate
    pub delta_deviation: f32,

    /// Time horizon: 1 second (in milliseconds)
    pub horizon_1s: u32,

    /// Time horizon: 5 seconds (in milliseconds)
    pub horizon_5s: u32,

    /// Time horizon: 30 seconds (in milliseconds)
    pub horizon_30s: u32,

    /// Time horizon: 60 seconds (in milliseconds)
    pub horizon_60s: u32,

    /// Pre-computed decay multipliers for horizons (optimization)
    /// These are computed as -horizon_ms / 1000.0 to avoid runtime division
    #[serde(skip)]
    pub(crate) decay_mult_1s: f32,

    #[serde(skip)]
    pub(crate) decay_mult_5s: f32,

    #[serde(skip)]
    pub(crate) decay_mult_30s: f32,

    #[serde(skip)]
    pub(crate) decay_mult_60s: f32,
}

impl Default for QeddConfig {
    #[allow(deprecated)]
    fn default() -> Self {
        let horizon_1s = 1000;
        let horizon_5s = 5000;
        let horizon_30s = 30000;
        let horizon_60s = 60000;

        Self {
            version: 1,
            lambda_base: 0.5,
            lambda_sensitivity: 0.1, // Kept for backward compatibility
            lambda_abort_threshold: 0.95,
            // Hazard rate coefficients for λ(t) = λ_base + α * SOBP_drop + β * outflow + γ * resonance_risk + δ * dev_risk
            alpha_sobp_drop: 0.3,  // SOBP drop has significant impact
            beta_outflow: 0.25,    // Capital outflow is critical
            gamma_resonance: 0.15, // Bot activity increases risk moderately
            delta_deviation: 0.20, // Market deviation indicates instability
            horizon_1s,
            horizon_5s,
            horizon_30s,
            horizon_60s,
            // Pre-compute decay multipliers (negative, already divided by 1000)
            decay_mult_1s: -(horizon_1s as f32 / 1000.0),
            decay_mult_5s: -(horizon_5s as f32 / 1000.0),
            decay_mult_30s: -(horizon_30s as f32 / 1000.0),
            decay_mult_60s: -(horizon_60s as f32 / 1000.0),
        }
    }
}

impl QeddConfig {
    /// Create a new config with custom horizons, automatically computing decay multipliers
    pub fn with_horizons(
        horizon_1s: u32,
        horizon_5s: u32,
        horizon_30s: u32,
        horizon_60s: u32,
    ) -> Self {
        let mut config = Self::default();
        config.horizon_1s = horizon_1s;
        config.horizon_5s = horizon_5s;
        config.horizon_30s = horizon_30s;
        config.horizon_60s = horizon_60s;
        config.decay_mult_1s = -(horizon_1s as f32 / 1000.0);
        config.decay_mult_5s = -(horizon_5s as f32 / 1000.0);
        config.decay_mult_30s = -(horizon_30s as f32 / 1000.0);
        config.decay_mult_60s = -(horizon_60s as f32 / 1000.0);
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = QeddConfig::default();
        assert_eq!(config.version, 1);
        assert_eq!(config.horizon_1s, 1000);
        assert_eq!(config.horizon_5s, 5000);
        assert_eq!(config.horizon_30s, 30000);
        assert_eq!(config.horizon_60s, 60000);
    }

    #[test]
    fn test_serialization() {
        let config = QeddConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: QeddConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config.version, deserialized.version);
        assert_eq!(config.lambda_base, deserialized.lambda_base);
    }
}
