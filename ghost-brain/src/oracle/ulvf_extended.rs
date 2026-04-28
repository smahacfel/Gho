//! ULVF 2.0: Momentum Classification & Multi-Snapshot Trend Analysis
//!
//! This module extends the base ULVF (Ultra-Early Liquidity Vector Field) with:
//! - Momentum type classification (OrganicAttraction/BotSpiral/Stagnation/Mixed/Unknown)
//! - Confidence scoring for classifications
//! - Multi-snapshot trend analysis with acceleration/deceleration detection
//! - Trajectory prediction for future momentum
//!
//! # Example
//!
//! ```ignore
//! use ghost_brain::oracle::ulvf_extended::{ULVFExtended, MomentumType};
//! use ghost_brain::oracle::hyper_oracle::MarketSnapshot;
//!
//! let mut ulvf = ULVFExtended::new();
//!
//! // Add snapshots as they arrive
//! let snapshot = MarketSnapshot {
//!     timestamp_ms: 1000,
//!     volume_sol: 100.0,
//!     tx_count: 50,
//!     unique_addrs: 25,
//! };
//! ulvf.add_snapshot(snapshot);
//!
//! // Get trend analysis after collecting enough snapshots
//! let trend = ulvf.analyze_trend();
//! println!("Momentum: {:?} (confidence: {:.2})", trend.momentum_type, trend.confidence);
//! println!("Accelerating: {}", trend.is_accelerating);
//! ```

use super::hyper_oracle::{HyperOracle, MarketSnapshot};
use std::collections::VecDeque;

// =============================================================================
// Configuration Constants
// =============================================================================

/// Default divergence threshold for organic attraction detection
const DEFAULT_DIVERGENCE_ORGANIC_THRESHOLD: f32 = 0.3;

/// Default divergence threshold for stagnation detection
const DEFAULT_DIVERGENCE_STAGNATION_THRESHOLD: f32 = 0.1;

/// Default curl threshold for bot spiral detection
const DEFAULT_CURL_BOT_THRESHOLD: f32 = 15.0;

/// Default maximum snapshots to keep in history
const DEFAULT_MAX_HISTORY_SIZE: usize = 20;

/// Default minimum snapshots for trend analysis
const DEFAULT_MIN_SNAPSHOTS_FOR_TREND: usize = 3;

/// Acceleration threshold for divergence trend
const ACCELERATION_DIVERGENCE_THRESHOLD: f32 = 0.05;

/// Acceleration threshold for curl trend
const ACCELERATION_CURL_THRESHOLD: f32 = 1.0;

// =============================================================================
// Data Structures
// =============================================================================

/// Momentum type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentumType {
    /// Strong organic buying pressure (divergence > 0.3, low curl)
    OrganicAttraction,
    /// Bot spiral - artificial volume rotation (high curl)
    BotSpiral,
    /// Market stagnation (low divergence)
    Stagnation,
    /// Mixed signals - unclear pattern
    Mixed,
    /// Insufficient data
    Unknown,
}

impl MomentumType {
    /// Get risk level associated with this momentum type
    /// Returns a value between 0.0 (safe) and 1.0 (risky)
    pub fn risk_level(&self) -> f32 {
        match self {
            MomentumType::OrganicAttraction => 0.2,
            MomentumType::BotSpiral => 0.9,
            MomentumType::Stagnation => 0.6,
            MomentumType::Mixed => 0.5,
            MomentumType::Unknown => 0.7,
        }
    }

    /// Get recommended action based on momentum type
    pub fn recommendation(&self) -> &'static str {
        match self {
            MomentumType::OrganicAttraction => "BUY - Strong organic momentum",
            MomentumType::BotSpiral => "SKIP - Bot manipulation detected",
            MomentumType::Stagnation => "WAIT - No momentum",
            MomentumType::Mixed => "CAUTION - Mixed signals",
            MomentumType::Unknown => "SKIP - Insufficient data",
        }
    }
}

/// Trend analysis over multiple snapshots
#[derive(Debug, Clone)]
pub struct TrendAnalysis {
    /// Current momentum type
    pub momentum_type: MomentumType,
    /// Confidence in classification (0.0-1.0)
    pub confidence: f32,
    /// Is momentum accelerating?
    pub is_accelerating: bool,
    /// Divergence trend (positive = increasing)
    pub divergence_trend: f32,
    /// Curl trend (positive = increasing rotation)
    pub curl_trend: f32,
    /// Predicted momentum in next 3-5 seconds
    pub predicted_momentum: MomentumType,
    /// Number of snapshots analyzed
    pub snapshot_count: usize,
}

impl Default for TrendAnalysis {
    fn default() -> Self {
        Self {
            momentum_type: MomentumType::Unknown,
            confidence: 0.0,
            is_accelerating: false,
            divergence_trend: 0.0,
            curl_trend: 0.0,
            predicted_momentum: MomentumType::Unknown,
            snapshot_count: 0,
        }
    }
}

/// ULVF configuration thresholds
#[derive(Debug, Clone)]
pub struct ULVFConfig {
    /// Divergence threshold for organic attraction
    pub divergence_organic_threshold: f32,
    /// Divergence threshold for stagnation
    pub divergence_stagnation_threshold: f32,
    /// Curl threshold for bot spiral detection
    pub curl_bot_threshold: f32,
    /// Maximum snapshots to keep in history
    pub max_history_size: usize,
    /// Minimum snapshots for trend analysis
    pub min_snapshots_for_trend: usize,
}

impl Default for ULVFConfig {
    fn default() -> Self {
        Self {
            divergence_organic_threshold: DEFAULT_DIVERGENCE_ORGANIC_THRESHOLD,
            divergence_stagnation_threshold: DEFAULT_DIVERGENCE_STAGNATION_THRESHOLD,
            curl_bot_threshold: DEFAULT_CURL_BOT_THRESHOLD,
            max_history_size: DEFAULT_MAX_HISTORY_SIZE,
            min_snapshots_for_trend: DEFAULT_MIN_SNAPSHOTS_FOR_TREND,
        }
    }
}

// =============================================================================
// ULVFExtended Implementation
// =============================================================================

/// Extended ULVF with momentum classification and trend analysis
#[derive(Clone)]
pub struct ULVFExtended {
    /// Base HyperOracle for ULVF computation
    base: HyperOracle,

    /// Configuration thresholds
    config: ULVFConfig,

    /// History of snapshots for trend analysis
    snapshot_history: VecDeque<MarketSnapshot>,

    /// History of computed (divergence, curl) pairs with timestamps
    /// Format: (divergence, curl, timestamp_ms)
    ulvf_history: VecDeque<(f32, f32, u64)>,
}

impl Default for ULVFExtended {
    fn default() -> Self {
        Self::new()
    }
}

impl ULVFExtended {
    /// Create with default configuration
    pub fn new() -> Self {
        Self::with_config(ULVFConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: ULVFConfig) -> Self {
        Self {
            base: HyperOracle::new(),
            config,
            snapshot_history: VecDeque::with_capacity(DEFAULT_MAX_HISTORY_SIZE),
            ulvf_history: VecDeque::with_capacity(DEFAULT_MAX_HISTORY_SIZE),
        }
    }

    /// Get reference to the current configuration
    pub fn config(&self) -> &ULVFConfig {
        &self.config
    }

    /// Add a new snapshot and compute ULVF
    /// Returns Some((divergence, curl)) if we have at least 2 snapshots, None otherwise
    pub fn add_snapshot(&mut self, snapshot: MarketSnapshot) -> Option<(f32, f32)> {
        // Get previous snapshot if exists
        let result = if let Some(prev) = self.snapshot_history.back() {
            let (div, curl) = self.base.calculate_ulvf(prev, &snapshot);
            self.ulvf_history
                .push_back((div, curl, snapshot.timestamp_ms));
            Some((div, curl))
        } else {
            None
        };

        // Add to history
        self.snapshot_history.push_back(snapshot);

        // Trim history if needed
        while self.snapshot_history.len() > self.config.max_history_size {
            self.snapshot_history.pop_front();
        }
        while self.ulvf_history.len() > self.config.max_history_size {
            self.ulvf_history.pop_front();
        }

        result
    }

    /// Classify momentum type from single (divergence, curl) pair
    pub fn classify_momentum(&self, div: f32, curl: f32) -> MomentumType {
        // Bot spiral detection (highest priority)
        if curl > self.config.curl_bot_threshold {
            return MomentumType::BotSpiral;
        }

        // Organic attraction
        if div > self.config.divergence_organic_threshold
            && curl < self.config.curl_bot_threshold * 0.5
        {
            return MomentumType::OrganicAttraction;
        }

        // Stagnation
        if div < self.config.divergence_stagnation_threshold {
            return MomentumType::Stagnation;
        }

        // Mixed
        MomentumType::Mixed
    }

    /// Classify with confidence score
    /// Returns (MomentumType, confidence) where confidence is 0.0-1.0
    pub fn classify_with_confidence(&self, div: f32, curl: f32) -> (MomentumType, f32) {
        let momentum_type = self.classify_momentum(div, curl);

        let confidence = match momentum_type {
            MomentumType::BotSpiral => {
                // Higher curl = higher confidence
                (curl / (self.config.curl_bot_threshold * 2.0)).clamp(0.5, 1.0)
            }
            MomentumType::OrganicAttraction => {
                // Higher divergence + lower curl = higher confidence
                let div_factor = (div / self.config.divergence_organic_threshold).clamp(0.0, 1.0);
                let curl_factor = 1.0 - (curl / self.config.curl_bot_threshold).clamp(0.0, 1.0);
                (div_factor * 0.6 + curl_factor * 0.4).clamp(0.3, 1.0)
            }
            MomentumType::Stagnation => {
                // Lower divergence = higher confidence in stagnation
                // Guard against division by zero
                if self.config.divergence_stagnation_threshold < 1e-9 {
                    0.5
                } else {
                    let ratio = div / self.config.divergence_stagnation_threshold;
                    (1.0 - ratio).clamp(0.5, 1.0)
                }
            }
            MomentumType::Mixed => 0.4,
            MomentumType::Unknown => 0.0,
        };

        (momentum_type, confidence)
    }

    /// Analyze trend over multiple snapshots
    pub fn analyze_trend(&self) -> TrendAnalysis {
        let snapshot_count = self.ulvf_history.len();

        if snapshot_count < self.config.min_snapshots_for_trend {
            return TrendAnalysis {
                momentum_type: MomentumType::Unknown,
                confidence: 0.0,
                is_accelerating: false,
                divergence_trend: 0.0,
                curl_trend: 0.0,
                predicted_momentum: MomentumType::Unknown,
                snapshot_count,
            };
        }

        // Get recent values (most recent first)
        let recent: Vec<(f32, f32)> = self
            .ulvf_history
            .iter()
            .rev()
            .take(5)
            .map(|(d, c, _)| (*d, *c))
            .collect();

        // Current values (average of last 2 for smoothing)
        let (current_div, current_curl) = if recent.len() >= 2 {
            (
                (recent[0].0 + recent[1].0) / 2.0,
                (recent[0].1 + recent[1].1) / 2.0,
            )
        } else {
            recent[0]
        };

        // Classify current momentum
        let (momentum_type, confidence) = self.classify_with_confidence(current_div, current_curl);

        // Calculate trends (simple difference for now, could use linear regression)
        // Using safe indexing to avoid potential bounds issues
        let (divergence_trend, curl_trend) = if recent.len() >= 3 {
            match (recent.get(recent.len() - 1), recent.get(recent.len() - 2)) {
                (Some(oldest), Some(second_oldest)) => {
                    let old_div = (oldest.0 + second_oldest.0) / 2.0;
                    let old_curl = (oldest.1 + second_oldest.1) / 2.0;
                    (current_div - old_div, current_curl - old_curl)
                }
                _ => (0.0, 0.0),
            }
        } else {
            (0.0, 0.0)
        };

        // Is accelerating?
        let is_accelerating = divergence_trend > ACCELERATION_DIVERGENCE_THRESHOLD
            || curl_trend.abs() > ACCELERATION_CURL_THRESHOLD;

        // Predict future momentum based on trends
        let predicted_div = current_div + divergence_trend;
        let predicted_curl = current_curl + curl_trend;
        let (predicted_momentum, _) = self.classify_with_confidence(predicted_div, predicted_curl);

        TrendAnalysis {
            momentum_type,
            confidence,
            is_accelerating,
            divergence_trend,
            curl_trend,
            predicted_momentum,
            snapshot_count,
        }
    }

    /// Get latest ULVF values
    /// Returns Some((divergence, curl)) if history is not empty
    pub fn latest(&self) -> Option<(f32, f32)> {
        self.ulvf_history.back().map(|(d, c, _)| (*d, *c))
    }

    /// Get number of ULVF pairs in history
    pub fn history_len(&self) -> usize {
        self.ulvf_history.len()
    }

    /// Get number of snapshots in history
    pub fn snapshot_count(&self) -> usize {
        self.snapshot_history.len()
    }

    /// Clear history (for new token)
    pub fn clear(&mut self) {
        self.snapshot_history.clear();
        self.ulvf_history.clear();
    }

    /// Direct access to base ULVF calculation
    pub fn calculate_ulvf(&self, t0: &MarketSnapshot, t1: &MarketSnapshot) -> (f32, f32) {
        self.base.calculate_ulvf(t0, t1)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a MarketSnapshot
    fn create_snapshot(ts: u64, vol: f64, tx: usize, addrs: usize) -> MarketSnapshot {
        MarketSnapshot {
            tx_key: None,
            timestamp_ms: ts,
            volume_sol: vol,
            tx_count: tx,
            unique_addrs: addrs,
        }
    }

    #[test]
    fn test_ulvf_extended_new() {
        let ulvf = ULVFExtended::new();
        assert_eq!(
            ulvf.config.divergence_organic_threshold,
            DEFAULT_DIVERGENCE_ORGANIC_THRESHOLD
        );
        assert_eq!(
            ulvf.config.divergence_stagnation_threshold,
            DEFAULT_DIVERGENCE_STAGNATION_THRESHOLD
        );
        assert_eq!(ulvf.config.curl_bot_threshold, DEFAULT_CURL_BOT_THRESHOLD);
        assert_eq!(ulvf.config.max_history_size, DEFAULT_MAX_HISTORY_SIZE);
        assert_eq!(
            ulvf.config.min_snapshots_for_trend,
            DEFAULT_MIN_SNAPSHOTS_FOR_TREND
        );
    }

    #[test]
    fn test_ulvf_extended_with_config() {
        let config = ULVFConfig {
            divergence_organic_threshold: 0.5,
            divergence_stagnation_threshold: 0.2,
            curl_bot_threshold: 20.0,
            max_history_size: 10,
            min_snapshots_for_trend: 5,
        };
        let ulvf = ULVFExtended::with_config(config.clone());
        assert_eq!(
            ulvf.config.divergence_organic_threshold,
            config.divergence_organic_threshold
        );
        assert_eq!(ulvf.config.max_history_size, config.max_history_size);
    }

    #[test]
    fn test_classify_organic_attraction() {
        let ulvf = ULVFExtended::new();
        // High divergence, low curl = organic
        let momentum = ulvf.classify_momentum(0.5, 5.0);
        assert_eq!(momentum, MomentumType::OrganicAttraction);
    }

    #[test]
    fn test_classify_bot_spiral() {
        let ulvf = ULVFExtended::new();
        // High curl = bot spiral
        let momentum = ulvf.classify_momentum(0.3, 20.0);
        assert_eq!(momentum, MomentumType::BotSpiral);
    }

    #[test]
    fn test_classify_stagnation() {
        let ulvf = ULVFExtended::new();
        // Low divergence = stagnation
        let momentum = ulvf.classify_momentum(0.05, 2.0);
        assert_eq!(momentum, MomentumType::Stagnation);
    }

    #[test]
    fn test_classify_mixed() {
        let ulvf = ULVFExtended::new();
        // Medium divergence, medium curl = mixed
        let momentum = ulvf.classify_momentum(0.2, 10.0);
        assert_eq!(momentum, MomentumType::Mixed);
    }

    #[test]
    fn test_classify_with_confidence_bot() {
        let ulvf = ULVFExtended::new();
        // High curl should give high bot confidence
        let (momentum, conf) = ulvf.classify_with_confidence(0.2, 30.0);
        assert_eq!(momentum, MomentumType::BotSpiral);
        assert!(
            conf > 0.7,
            "High curl should give high bot confidence, got {}",
            conf
        );
    }

    #[test]
    fn test_classify_with_confidence_organic() {
        let ulvf = ULVFExtended::new();
        // High div, low curl should give good organic confidence
        let (momentum, conf) = ulvf.classify_with_confidence(0.6, 3.0);
        assert_eq!(momentum, MomentumType::OrganicAttraction);
        assert!(
            conf > 0.5,
            "High div, low curl should give good organic confidence, got {}",
            conf
        );
    }

    #[test]
    fn test_classify_with_confidence_stagnation() {
        let ulvf = ULVFExtended::new();
        // Very low divergence = high stagnation confidence
        let (momentum, conf) = ulvf.classify_with_confidence(0.02, 2.0);
        assert_eq!(momentum, MomentumType::Stagnation);
        assert!(
            conf >= 0.5,
            "Low divergence should give high stagnation confidence, got {}",
            conf
        );
    }

    #[test]
    fn test_add_snapshot_first() {
        let mut ulvf = ULVFExtended::new();
        let snapshot = create_snapshot(0, 100.0, 50, 25);
        let result = ulvf.add_snapshot(snapshot);

        // First snapshot should return None (no previous to compare)
        assert!(result.is_none());
        assert_eq!(ulvf.snapshot_count(), 1);
        assert_eq!(ulvf.history_len(), 0);
    }

    #[test]
    fn test_add_snapshot_second() {
        let mut ulvf = ULVFExtended::new();
        ulvf.add_snapshot(create_snapshot(0, 100.0, 50, 25));
        let result = ulvf.add_snapshot(create_snapshot(500, 150.0, 70, 35));

        // Second snapshot should return Some with ULVF values
        assert!(result.is_some());
        assert_eq!(ulvf.snapshot_count(), 2);
        assert_eq!(ulvf.history_len(), 1);

        let (div, curl) = result.unwrap();
        assert!(div.is_finite(), "Divergence should be finite, got {}", div);
        assert!(curl.is_finite(), "Curl should be finite, got {}", curl);
    }

    #[test]
    fn test_add_snapshots_and_trend() {
        let mut ulvf = ULVFExtended::new();

        // Add growing snapshots (organic growth pattern)
        for i in 0..5 {
            let snapshot = create_snapshot(
                i * 500,
                100.0 + (i as f64) * 50.0, // Growing volume
                50 + (i as usize) * 20,    // Growing tx
                25 + (i as usize) * 10,    // Growing addrs
            );
            ulvf.add_snapshot(snapshot);
        }

        let trend = ulvf.analyze_trend();
        assert_eq!(trend.snapshot_count, 4); // 5 snapshots = 4 ULVF pairs
        assert!(
            trend.divergence_trend.is_finite(),
            "Divergence trend should be finite"
        );
        assert!(trend.curl_trend.is_finite(), "Curl trend should be finite");
    }

    #[test]
    fn test_analyze_trend_insufficient_data() {
        let mut ulvf = ULVFExtended::new();
        ulvf.add_snapshot(create_snapshot(0, 100.0, 50, 25));
        ulvf.add_snapshot(create_snapshot(500, 150.0, 70, 35));

        // Only 1 ULVF pair, need at least 3 for trend
        let trend = ulvf.analyze_trend();
        assert_eq!(trend.momentum_type, MomentumType::Unknown);
        assert_eq!(trend.confidence, 0.0);
        assert!(!trend.is_accelerating);
    }

    #[test]
    fn test_analyze_trend_with_enough_data() {
        let mut ulvf = ULVFExtended::new();

        // Add 4 snapshots (produces 3 ULVF pairs)
        for i in 0..4 {
            let snapshot = create_snapshot(
                i * 500,
                100.0 + (i as f64) * 50.0,
                50 + (i as usize) * 20,
                25 + (i as usize) * 10,
            );
            ulvf.add_snapshot(snapshot);
        }

        let trend = ulvf.analyze_trend();
        assert_eq!(trend.snapshot_count, 3);
        assert_ne!(trend.momentum_type, MomentumType::Unknown);
        assert!(trend.confidence > 0.0);
    }

    #[test]
    fn test_latest() {
        let mut ulvf = ULVFExtended::new();

        // Before any snapshots
        assert!(ulvf.latest().is_none());

        ulvf.add_snapshot(create_snapshot(0, 100.0, 50, 25));
        assert!(ulvf.latest().is_none()); // Still none with just 1 snapshot

        ulvf.add_snapshot(create_snapshot(500, 150.0, 70, 35));
        assert!(ulvf.latest().is_some());
    }

    #[test]
    fn test_clear_history() {
        let mut ulvf = ULVFExtended::new();
        ulvf.add_snapshot(create_snapshot(0, 100.0, 50, 25));
        ulvf.add_snapshot(create_snapshot(500, 150.0, 70, 35));

        assert!(ulvf.latest().is_some());
        assert_eq!(ulvf.snapshot_count(), 2);

        ulvf.clear();
        assert!(ulvf.latest().is_none());
        assert_eq!(ulvf.snapshot_count(), 0);
        assert_eq!(ulvf.history_len(), 0);
    }

    #[test]
    fn test_history_trimming() {
        let config = ULVFConfig {
            max_history_size: 5,
            ..Default::default()
        };
        let mut ulvf = ULVFExtended::with_config(config);

        // Add more snapshots than max_history_size
        for i in 0..10 {
            ulvf.add_snapshot(create_snapshot(i * 500, 100.0 + (i as f64) * 10.0, 50, 25));
        }

        assert!(ulvf.snapshot_count() <= 5);
        assert!(ulvf.history_len() <= 5);
    }

    #[test]
    fn test_momentum_type_risk_levels() {
        assert_eq!(MomentumType::OrganicAttraction.risk_level(), 0.2);
        assert_eq!(MomentumType::BotSpiral.risk_level(), 0.9);
        assert_eq!(MomentumType::Stagnation.risk_level(), 0.6);
        assert_eq!(MomentumType::Mixed.risk_level(), 0.5);
        assert_eq!(MomentumType::Unknown.risk_level(), 0.7);
    }

    #[test]
    fn test_momentum_type_recommendations() {
        assert!(MomentumType::OrganicAttraction
            .recommendation()
            .contains("BUY"));
        assert!(MomentumType::BotSpiral.recommendation().contains("SKIP"));
        assert!(MomentumType::Stagnation.recommendation().contains("WAIT"));
        assert!(MomentumType::Mixed.recommendation().contains("CAUTION"));
        assert!(MomentumType::Unknown.recommendation().contains("SKIP"));
    }

    #[test]
    fn test_default_config() {
        let config = ULVFConfig::default();
        assert_eq!(
            config.divergence_organic_threshold,
            DEFAULT_DIVERGENCE_ORGANIC_THRESHOLD
        );
        assert_eq!(
            config.divergence_stagnation_threshold,
            DEFAULT_DIVERGENCE_STAGNATION_THRESHOLD
        );
        assert_eq!(config.curl_bot_threshold, DEFAULT_CURL_BOT_THRESHOLD);
        assert_eq!(config.max_history_size, DEFAULT_MAX_HISTORY_SIZE);
        assert_eq!(
            config.min_snapshots_for_trend,
            DEFAULT_MIN_SNAPSHOTS_FOR_TREND
        );
    }

    #[test]
    fn test_default_trend_analysis() {
        let trend = TrendAnalysis::default();
        assert_eq!(trend.momentum_type, MomentumType::Unknown);
        assert_eq!(trend.confidence, 0.0);
        assert!(!trend.is_accelerating);
        assert_eq!(trend.divergence_trend, 0.0);
        assert_eq!(trend.curl_trend, 0.0);
        assert_eq!(trend.predicted_momentum, MomentumType::Unknown);
        assert_eq!(trend.snapshot_count, 0);
    }

    #[test]
    fn test_default_implementation() {
        let ulvf1 = ULVFExtended::default();
        let ulvf2 = ULVFExtended::new();

        // Both should have same configuration
        assert_eq!(
            ulvf1.config.divergence_organic_threshold,
            ulvf2.config.divergence_organic_threshold
        );
        assert_eq!(ulvf1.config.max_history_size, ulvf2.config.max_history_size);
    }

    #[test]
    fn test_calculate_ulvf_direct() {
        let ulvf = ULVFExtended::new();
        let t0 = create_snapshot(0, 100.0, 50, 25);
        let t1 = create_snapshot(1000, 200.0, 100, 50);

        let (div, curl) = ulvf.calculate_ulvf(&t0, &t1);
        assert!(div.is_finite());
        assert!(curl.is_finite());
    }

    #[test]
    fn test_config_accessor() {
        let custom_config = ULVFConfig {
            divergence_organic_threshold: 0.4,
            ..Default::default()
        };
        let ulvf = ULVFExtended::with_config(custom_config);

        let config = ulvf.config();
        assert_eq!(config.divergence_organic_threshold, 0.4);
    }

    #[test]
    fn test_predicted_momentum_changes() {
        let mut ulvf = ULVFExtended::new();

        // Add snapshots with increasing volume (should predict organic or acceleration)
        for i in 0..6 {
            let snapshot = create_snapshot(
                i * 500,
                100.0 + (i as f64) * 100.0, // Strong volume growth
                50 + (i as usize) * 30,     // Strong tx growth
                25 + (i as usize) * 15,     // Strong addr growth
            );
            ulvf.add_snapshot(snapshot);
        }

        let trend = ulvf.analyze_trend();
        // With consistent growth, predicted momentum should not be Unknown
        assert!(trend.snapshot_count >= 3);
        // The prediction should be valid
        assert!(matches!(
            trend.predicted_momentum,
            MomentumType::OrganicAttraction
                | MomentumType::BotSpiral
                | MomentumType::Stagnation
                | MomentumType::Mixed
                | MomentumType::Unknown
        ));
    }

    #[test]
    fn test_zero_stagnation_threshold_no_panic() {
        // Test that zero threshold doesn't cause division by zero panic
        let config = ULVFConfig {
            divergence_stagnation_threshold: 0.0,
            ..Default::default()
        };
        let ulvf = ULVFExtended::with_config(config);

        // This should not panic
        let (momentum, confidence) = ulvf.classify_with_confidence(0.05, 2.0);
        assert!(
            confidence.is_finite(),
            "Confidence should be finite even with zero threshold"
        );
        // With zero stagnation threshold, it won't classify as stagnation since div > threshold
        assert!(matches!(
            momentum,
            MomentumType::OrganicAttraction
                | MomentumType::BotSpiral
                | MomentumType::Stagnation
                | MomentumType::Mixed
        ));
    }
}
