//! MCI Result Models
//!
//! Data structures for MCI engine computation results.

use serde::{Deserialize, Serialize};

// Feature flags for MCI computation (zero-allocation alternative to Vec<String>)
pub const FEATURE_QASS_ALIGNMENT: u8 = 1 << 0;
pub const FEATURE_FLOW_MAGNITUDE: u8 = 1 << 1;
pub const FEATURE_MPCF_ENTROPY: u8 = 1 << 2;
pub const FEATURE_SOBP_STABILITY: u8 = 1 << 3;
pub const FEATURE_COMBINED_ENTROPY: u8 = 1 << 4;
pub const FEATURE_DEVIATION_RISK: u8 = 1 << 5;

/// Result from MCI computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MciResult {
    /// Market Coherence Index value [0.0, 1.0]
    pub mci: f32,

    /// Directional Coherence component
    pub dc: f32,

    /// Structural Coherence component
    pub sc: f32,

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

impl MciResult {
    /// Create a placeholder result with zero values
    /// Used for initial implementation before actual computation logic
    pub fn placeholder() -> Self {
        Self {
            mci: 0.0,
            dc: 0.0,
            sc: 0.0,
            #[allow(deprecated)]
            features_used: vec![],
            features_flags: 0,
            computation_ms: 0,
        }
    }

    /// Check if the result represents an abort condition
    pub fn should_abort(&self, threshold: f32) -> bool {
        self.mci < threshold
    }

    /// Convert feature flags to string vec (for backward compatibility)
    pub fn features_to_vec(&self) -> Vec<String> {
        let mut features = Vec::new();
        if self.features_flags & FEATURE_QASS_ALIGNMENT != 0 {
            features.push("qass_alignment".to_string());
        }
        if self.features_flags & FEATURE_FLOW_MAGNITUDE != 0 {
            features.push("flow_magnitude".to_string());
        }
        if self.features_flags & FEATURE_MPCF_ENTROPY != 0 {
            features.push("mpcf_entropy".to_string());
        }
        if self.features_flags & FEATURE_SOBP_STABILITY != 0 {
            features.push("sobp_stability".to_string());
        }
        if self.features_flags & FEATURE_COMBINED_ENTROPY != 0 {
            features.push("combined_entropy".to_string());
        }
        if self.features_flags & FEATURE_DEVIATION_RISK != 0 {
            features.push("deviation_risk".to_string());
        }
        features
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder() {
        let result = MciResult::placeholder();
        assert_eq!(result.mci, 0.0);
        assert_eq!(result.dc, 0.0);
        assert_eq!(result.sc, 0.0);
        assert!(result.features_used.is_empty());
    }

    #[test]
    fn test_serialization() {
        let result = MciResult::placeholder();
        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: MciResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(result.mci, deserialized.mci);
    }

    #[test]
    fn test_should_abort() {
        let mut result = MciResult::placeholder();
        result.mci = 0.25;
        assert!(result.should_abort(0.3));

        result.mci = 0.35;
        assert!(!result.should_abort(0.3));
    }
}
