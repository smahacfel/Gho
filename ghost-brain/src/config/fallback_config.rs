//! Fallback Configuration
//!
//! Centralized configuration for fallback values with confidence penalties.
//!
//! This module provides a configurable approach to handling missing data in the
//! HyperPrediction Oracle. Instead of hardcoded fallback values, each module
//! (ULVF, POVC, SCR, SSMI) has configurable defaults with associated confidence penalties.
//!
//! ## Design Rationale
//!
//! - **Neutral Defaults**: Fallback values are designed to be neutral (e.g., 0.5 for scores)
//!   to avoid bias when data is missing.
//! - **Confidence Penalties**: Each fallback incurs a confidence penalty that reduces
//!   the overall confidence in the prediction, signaling data quality issues.
//! - **Cumulative Penalty Cap**: Multiple fallbacks don't compound linearly; instead,
//!   they're capped at `max_cumulative_penalty` to prevent complete score invalidation.

use serde::{Deserialize, Serialize};

/// Configuration for fallback values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    pub ulvf: UlvfFallback,
    pub povc: PovcFallback,
    pub scr: ScrFallback,
    pub ssmi: SsmiFallback,
    /// Maximum cumulative confidence penalty (0.0-1.0)
    pub max_cumulative_penalty: f32,
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            ulvf: UlvfFallback::default(),
            povc: PovcFallback::default(),
            scr: ScrFallback::default(),
            ssmi: SsmiFallback::default(),
            max_cumulative_penalty: 0.50,
        }
    }
}

/// ULVF (Ultra-Low Variance Field) fallback configuration
///
/// ULVF requires transaction count and unique address data from two time windows.
/// When metrics are unavailable, we use conservative defaults that assume minimal activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UlvfFallback {
    /// Default divergence (0.5 = neutral, no clear trend)
    pub default_divergence: f32,
    /// Default curl (0.0 = no wash trading detected)
    pub default_curl: f32,
    /// Default transaction count in time window T0
    pub default_tx_count_t0: usize,
    /// Default transaction count in time window T1
    pub default_tx_count_t1: usize,
    /// Default unique addresses in time window T0
    pub default_unique_t0: usize,
    /// Default unique addresses in time window T1
    pub default_unique_t1: usize,
    /// Confidence penalty when using fallback (0.15 = 15% penalty)
    pub confidence_penalty: f32,
}

impl Default for UlvfFallback {
    fn default() -> Self {
        Self {
            default_divergence: 0.5,  // Neutral - no directional bias
            default_curl: 0.0,        // No wash trading assumption
            default_tx_count_t0: 1,   // Minimal activity (conservative)
            default_tx_count_t1: 2,   // Slight growth (conservative)
            default_unique_t0: 1,     // Minimal unique addresses
            default_unique_t1: 2,     // Slight growth in unique addresses
            confidence_penalty: 0.15, // 15% penalty for missing ULVF data
        }
    }
}

/// POVC (Pattern of Volume Clustering) fallback configuration
///
/// POVC classifies market activity into clusters (0=Dump, 1=Hype, 2=Bot Noise).
/// When metrics are unavailable, we assume cluster 2 (Bot Noise) as the safest default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PovcFallback {
    /// Default cluster (2 = Bot Noise, safest assumption when data is missing)
    pub default_cluster: usize,
    /// Default transaction count for snapshot
    pub default_tx_count: usize,
    /// Default unique addresses for snapshot
    pub default_unique_addrs: usize,
    /// Confidence penalty when using fallback (0.10 = 10% penalty)
    pub confidence_penalty: f32,
}

impl Default for PovcFallback {
    fn default() -> Self {
        Self {
            default_cluster: 2,       // Bot Noise (safest/most neutral)
            default_tx_count: 2,      // Minimal activity
            default_unique_addrs: 2,  // Minimal unique addresses
            confidence_penalty: 0.10, // 10% penalty for missing POVC data
        }
    }
}

/// SCR (Sybil Confidence Rating) fallback configuration
///
/// SCR detects bot activity via FFT analysis of transaction timing.
/// When unavailable, we use 0.5 (unknown) rather than 0.0 (which implies "definitely no bots").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrFallback {
    /// Default SCR score (0.5 = unknown, not 0.0 which implies "no bots")
    pub default_score: f32,
    /// Confidence penalty when using fallback (0.12 = 12% penalty)
    pub confidence_penalty: f32,
}

impl Default for ScrFallback {
    fn default() -> Self {
        Self {
            default_score: 0.5,       // Unknown (neutral position)
            confidence_penalty: 0.12, // 12% penalty for missing SCR data
        }
    }
}

/// SSMI (Sub-Slot Microentropy Index) fallback configuration
///
/// SSMI analyzes micro-timing patterns in transaction timestamps.
/// When unavailable, we use 0.5 (unknown entropy) as a neutral default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsmiFallback {
    /// Default SSMI score (0.5 = unknown entropy)
    pub default_score: f32,
    /// Confidence penalty when using fallback (0.10 = 10% penalty)
    pub confidence_penalty: f32,
}

impl Default for SsmiFallback {
    fn default() -> Self {
        Self {
            default_score: 0.5,       // Unknown entropy (neutral)
            confidence_penalty: 0.10, // 10% penalty for missing SSMI data
        }
    }
}

/// Tracker for fallback usage during a prediction cycle
///
/// Records which fallbacks were used and accumulates confidence penalties.
/// The cumulative penalty is capped at `max_cumulative_penalty` to prevent
/// complete invalidation of predictions when multiple data sources are missing.
#[derive(Debug, Clone, Default)]
pub struct FallbackTracker {
    pub used_fallbacks: Vec<FallbackType>,
    pub cumulative_penalty: f32,
}

/// Types of fallbacks that can be used
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackType {
    UlvfDivergence,
    UlvfTxCounts,
    PovcCluster,
    ScrScore,
    SsmiScore,
}

impl FallbackTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a fallback usage with its associated penalty
    ///
    /// The penalty is added to the cumulative penalty, capped at `max`.
    pub fn record(&mut self, fallback: FallbackType, penalty: f32, max: f32) {
        self.used_fallbacks.push(fallback);
        self.cumulative_penalty = (self.cumulative_penalty + penalty).min(max);
    }

    /// Get the confidence multiplier (1.0 - cumulative_penalty)
    ///
    /// This can be applied to the final score or confidence metric.
    /// Example: cumulative_penalty=0.25 → multiplier=0.75 (score reduced by 25%)
    pub fn confidence_multiplier(&self) -> f32 {
        1.0 - self.cumulative_penalty
    }

    /// Check if any fallbacks were used
    pub fn any_used(&self) -> bool {
        !self.used_fallbacks.is_empty()
    }

    /// Get the number of fallbacks used
    pub fn count(&self) -> usize {
        self.used_fallbacks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_config_defaults() {
        let config = FallbackConfig::default();

        // ULVF defaults
        assert_eq!(config.ulvf.default_divergence, 0.5);
        assert_eq!(config.ulvf.default_curl, 0.0);
        assert_eq!(config.ulvf.default_tx_count_t0, 1);
        assert_eq!(config.ulvf.default_tx_count_t1, 2);
        assert_eq!(config.ulvf.confidence_penalty, 0.15);

        // POVC defaults
        assert_eq!(config.povc.default_cluster, 2);
        assert_eq!(config.povc.confidence_penalty, 0.10);

        // SCR defaults
        assert_eq!(config.scr.default_score, 0.5);
        assert_eq!(config.scr.confidence_penalty, 0.12);

        // SSMI defaults
        assert_eq!(config.ssmi.default_score, 0.5);
        assert_eq!(config.ssmi.confidence_penalty, 0.10);

        // Max cumulative penalty
        assert_eq!(config.max_cumulative_penalty, 0.50);
    }

    #[test]
    fn test_fallback_tracker_single_fallback() {
        let mut tracker = FallbackTracker::new();
        assert!(!tracker.any_used());
        assert_eq!(tracker.confidence_multiplier(), 1.0);

        tracker.record(FallbackType::UlvfTxCounts, 0.15, 0.50);

        assert!(tracker.any_used());
        assert_eq!(tracker.count(), 1);
        assert_eq!(tracker.cumulative_penalty, 0.15);
        assert_eq!(tracker.confidence_multiplier(), 0.85);
    }

    #[test]
    fn test_fallback_tracker_multiple_fallbacks() {
        let mut tracker = FallbackTracker::new();

        tracker.record(FallbackType::UlvfTxCounts, 0.15, 0.50);
        tracker.record(FallbackType::PovcCluster, 0.10, 0.50);
        tracker.record(FallbackType::ScrScore, 0.12, 0.50);

        assert_eq!(tracker.count(), 3);
        assert_eq!(tracker.cumulative_penalty, 0.37);
        assert_eq!(tracker.confidence_multiplier(), 0.63);
    }

    #[test]
    fn test_fallback_tracker_penalty_capped() {
        let mut tracker = FallbackTracker::new();

        // Record penalties that would exceed max if not capped
        tracker.record(FallbackType::UlvfTxCounts, 0.30, 0.50);
        tracker.record(FallbackType::PovcCluster, 0.30, 0.50);
        tracker.record(FallbackType::ScrScore, 0.30, 0.50);

        // Should be capped at 0.50
        assert_eq!(tracker.cumulative_penalty, 0.50);
        assert_eq!(tracker.confidence_multiplier(), 0.50);
    }

    #[test]
    fn test_fallback_types_equality() {
        assert_eq!(FallbackType::UlvfTxCounts, FallbackType::UlvfTxCounts);
        assert_ne!(FallbackType::UlvfTxCounts, FallbackType::PovcCluster);
    }
}
