//! QEDD Result Models
//!
//! Data structures for QEDD engine computation results.

use serde::{Deserialize, Serialize};

// Feature flags for QEDD computation (zero-allocation alternative to Vec<String>)
pub const FEATURE_LAMBDA_BASE: u8 = 1 << 0;
pub const FEATURE_SOBP_DROP: u8 = 1 << 1;
pub const FEATURE_OUTFLOW: u8 = 1 << 2;
pub const FEATURE_RESONANCE_RISK: u8 = 1 << 3;
pub const FEATURE_DEVIATION_RISK: u8 = 1 << 4;

/// Result from QEDD computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QeddResult {
    /// Current lambda (decay rate) value
    pub lambda_now: f32,

    /// Survival probability at 1 second horizon
    pub survival_1s: f32,

    /// Survival probability at 5 second horizon
    pub survival_5s: f32,

    /// Survival probability at 30 second horizon
    pub survival_30s: f32,

    /// Survival probability at 60 second horizon
    pub survival_60s: f32,

    /// List of feature names used in computation
    ///
    /// **Deprecated**: This field is kept for backward compatibility with serialization.
    /// Use `features_flags` for zero-allocation access.
    #[deprecated(since = "1.1.0", note = "Use features_flags instead")]
    pub features_used: Vec<String>,

    /// Bitflags indicating which features were used (zero-allocation)
    #[serde(default)]
    pub features_flags: u8,

    /// Computation time in milliseconds
    pub computation_ms: u64,
}

impl QeddResult {
    /// Create a placeholder result with zero values
    /// Used for initial implementation before actual computation logic
    pub fn placeholder() -> Self {
        Self {
            lambda_now: 0.0,
            survival_1s: 0.0,
            survival_5s: 0.0,
            survival_30s: 0.0,
            survival_60s: 0.0,
            #[allow(deprecated)]
            features_used: vec![],
            features_flags: 0,
            computation_ms: 0,
        }
    }

    /// Check if the result represents an abort condition
    pub fn should_abort(&self, threshold: f32) -> bool {
        self.lambda_now > threshold
    }

    /// Convert feature flags to string vec (for backward compatibility)
    pub fn features_to_vec(&self) -> Vec<String> {
        let mut features = Vec::new();
        if self.features_flags & FEATURE_LAMBDA_BASE != 0 {
            features.push("lambda_base".to_string());
        }
        if self.features_flags & FEATURE_SOBP_DROP != 0 {
            features.push("sobp_drop".to_string());
        }
        if self.features_flags & FEATURE_OUTFLOW != 0 {
            features.push("outflow".to_string());
        }
        if self.features_flags & FEATURE_RESONANCE_RISK != 0 {
            features.push("resonance_risk".to_string());
        }
        if self.features_flags & FEATURE_DEVIATION_RISK != 0 {
            features.push("deviation_risk".to_string());
        }
        features
    }
}

/// Survival probability data for a specific horizon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QeddHorizonSurvival {
    /// Time horizon in milliseconds
    pub horizon_ms: u32,

    /// Survival probability [0.0, 1.0]
    pub survival: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder() {
        let result = QeddResult::placeholder();
        assert_eq!(result.lambda_now, 0.0);
        assert_eq!(result.survival_1s, 0.0);
        assert!(result.features_used.is_empty());
    }

    #[test]
    fn test_serialization() {
        let result = QeddResult::placeholder();
        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: QeddResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(result.lambda_now, deserialized.lambda_now);
    }

    #[test]
    fn test_should_abort() {
        let mut result = QeddResult::placeholder();
        result.lambda_now = 0.96;
        assert!(result.should_abort(0.95));

        result.lambda_now = 0.90;
        assert!(!result.should_abort(0.95));
    }
}
