//! Resonance Detector - Bot Activity Detection via Trading Pattern Analysis
//!
//! This module implements a real-time detector for identifying bot networks and
//! coordinated trading patterns by analyzing the statistical properties of trade
//! timing intervals.
//!
//! ## Core Concept
//!
//! Bot networks and automated trading systems tend to execute trades at regular,
//! predictable intervals, while human traders exhibit more random, organic timing.
//! By measuring the variance (entropy) of inter-trade intervals, we can distinguish
//! between bot activity (low variance) and human activity (high variance).
//!
//! ## Algorithm
//!
//! 1. **Circular Buffer**: Maintains the last N trade timestamps (default 64)
//! 2. **Interval Calculation**: Computes time deltas between consecutive trades
//! 3. **Statistical Analysis**: Calculates variance and coefficient of variation
//! 4. **Bot Detection**: Low variance/CV indicates periodic bot behavior
//!
//! ## Usage Example
//!
//! ```rust
//! use ghost_brain::signals::{ResonanceDetector, ResonanceConfig};
//!
//! // Create detector with default configuration
//! let mut detector = ResonanceDetector::new();
//!
//! // Process incoming trade timestamps
//! detector.add_timestamp(1000);
//! detector.add_timestamp(1500);
//! detector.add_timestamp(2000);
//!
//! // Analyze pattern
//! let result = detector.analyze();
//! if result.is_bot_likely() {
//!     println!("Bot activity detected! Score: {}", result.resonance_score);
//! }
//! ```
//!
//! ## Performance
//!
//! - **Time Complexity**: O(n) where n is buffer size (typically 64)
//! - **Space Complexity**: O(n) for timestamp storage
//! - **Latency**: < 100μs for typical analysis

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Default buffer size for timestamp storage
pub const DEFAULT_BUFFER_SIZE: usize = 64;

/// Minimum samples required for meaningful analysis
pub const MIN_SAMPLES_FOR_ANALYSIS: usize = 8;

/// Default variance threshold for bot detection (coefficient of variation)
/// CV < 0.3 indicates highly periodic behavior (bot-like)
pub const DEFAULT_BOT_THRESHOLD_CV: f64 = 0.3;

/// Default variance threshold for human behavior
/// CV > 0.8 indicates random/organic behavior (human-like)
pub const DEFAULT_HUMAN_THRESHOLD_CV: f64 = 0.8;

/// Generic circular buffer implementation for efficient FIFO operations
///
/// This structure maintains a fixed-size buffer that automatically overwrites
/// the oldest entries when capacity is reached.
#[derive(Debug, Clone)]
pub struct CircularBuffer<T> {
    /// Internal storage using VecDeque for efficient push/pop
    buffer: VecDeque<T>,
    /// Maximum capacity
    capacity: usize,
}

impl<T: Clone> CircularBuffer<T> {
    /// Create a new circular buffer with specified capacity
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of elements to store
    ///
    /// # Example
    ///
    /// ```
    /// use ghost_brain::signals::CircularBuffer;
    ///
    /// let buffer: CircularBuffer<u64> = CircularBuffer::new(64);
    /// ```
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Add an element to the buffer
    ///
    /// If the buffer is at capacity, the oldest element is removed.
    ///
    /// # Arguments
    ///
    /// * `value` - Element to add
    pub fn push(&mut self, value: T) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(value);
    }

    /// Get the number of elements currently in the buffer
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get a reference to the underlying data
    pub fn as_slice(&self) -> &VecDeque<T> {
        &self.buffer
    }

    /// Clear all elements from the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Get iterator over buffer elements
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.buffer.iter()
    }
}

impl<T: Clone> Default for CircularBuffer<T> {
    fn default() -> Self {
        Self::new(DEFAULT_BUFFER_SIZE)
    }
}

/// Classification of trading activity based on pattern analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityClassification {
    /// Highly periodic pattern indicative of bot activity
    BotLikely,
    /// Suspicious pattern that may be bot or coordinated trading
    Suspicious,
    /// Normal organic trading pattern
    HumanLikely,
    /// Insufficient data for classification
    Insufficient,
}

/// Configuration for resonance detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceConfig {
    /// Maximum number of timestamps to keep in buffer
    pub buffer_size: usize,

    /// Coefficient of variation threshold below which pattern is bot-like
    /// Default: 0.3 (30% variation)
    pub bot_threshold_cv: f64,

    /// Coefficient of variation threshold above which pattern is human-like
    /// Default: 0.8 (80% variation)
    pub human_threshold_cv: f64,

    /// Minimum number of samples required for analysis
    /// Default: 8
    pub min_samples: usize,
}

impl Default for ResonanceConfig {
    fn default() -> Self {
        Self {
            buffer_size: DEFAULT_BUFFER_SIZE,
            bot_threshold_cv: DEFAULT_BOT_THRESHOLD_CV,
            human_threshold_cv: DEFAULT_HUMAN_THRESHOLD_CV,
            min_samples: MIN_SAMPLES_FOR_ANALYSIS,
        }
    }
}

/// Result of resonance analysis
///
/// Contains detailed metrics about the detected trading pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceResult {
    /// Resonance score: 0.0 = highly random, 1.0 = perfectly periodic
    /// This is inversely related to coefficient of variation
    pub resonance_score: f64,

    /// Coefficient of variation (std_dev / mean) of intervals
    /// Lower values indicate more periodic behavior
    pub coefficient_variation: f64,

    /// Mean interval between trades (milliseconds)
    pub mean_interval_ms: f64,

    /// Standard deviation of intervals (milliseconds)
    pub std_dev_ms: f64,

    /// Activity classification
    pub classification: ActivityClassification,

    /// Number of samples used in analysis
    pub sample_count: usize,

    /// Timestamp of analysis (milliseconds since epoch)
    pub timestamp_ms: u64,
}

impl ResonanceResult {
    /// Check if the pattern indicates likely bot activity
    pub fn is_bot_likely(&self) -> bool {
        matches!(self.classification, ActivityClassification::BotLikely)
    }

    /// Check if the pattern is suspicious
    pub fn is_suspicious(&self) -> bool {
        matches!(
            self.classification,
            ActivityClassification::Suspicious | ActivityClassification::BotLikely
        )
    }

    /// Check if the pattern indicates likely human activity
    pub fn is_human_likely(&self) -> bool {
        matches!(self.classification, ActivityClassification::HumanLikely)
    }
}

/// Main resonance detector for identifying bot trading patterns
///
/// This detector analyzes trade timing to identify periodic patterns
/// characteristic of automated trading systems.
pub struct ResonanceDetector {
    /// Circular buffer of trade timestamps (milliseconds)
    timestamps: CircularBuffer<u64>,

    /// Configuration parameters
    config: ResonanceConfig,
}

impl ResonanceDetector {
    /// Create a new resonance detector with default configuration
    ///
    /// # Example
    ///
    /// ```
    /// use ghost_brain::signals::ResonanceDetector;
    ///
    /// let detector = ResonanceDetector::new();
    /// ```
    pub fn new() -> Self {
        Self::with_config(ResonanceConfig::default())
    }

    /// Create a new resonance detector with custom configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Custom configuration parameters
    ///
    /// # Example
    ///
    /// ```
    /// use ghost_brain::signals::{ResonanceDetector, ResonanceConfig};
    ///
    /// let config = ResonanceConfig {
    ///     buffer_size: 128,
    ///     bot_threshold_cv: 0.25,
    ///     ..Default::default()
    /// };
    /// let detector = ResonanceDetector::with_config(config);
    /// ```
    pub fn with_config(config: ResonanceConfig) -> Self {
        Self {
            timestamps: CircularBuffer::new(config.buffer_size),
            config,
        }
    }

    /// Add a new trade timestamp to the detector
    ///
    /// # Arguments
    ///
    /// * `timestamp_ms` - Timestamp in milliseconds since epoch
    ///
    /// # Example
    ///
    /// ```
    /// use ghost_brain::signals::ResonanceDetector;
    ///
    /// let mut detector = ResonanceDetector::new();
    /// detector.add_timestamp(1000);
    /// detector.add_timestamp(2000);
    /// ```
    pub fn add_timestamp(&mut self, timestamp_ms: u64) {
        self.timestamps.push(timestamp_ms);
    }

    /// Add multiple timestamps at once
    ///
    /// # Arguments
    ///
    /// * `timestamps` - Slice of timestamps to add
    pub fn add_timestamps(&mut self, timestamps: &[u64]) {
        for &ts in timestamps {
            self.add_timestamp(ts);
        }
    }

    /// Clear all stored timestamps
    pub fn clear(&mut self) {
        self.timestamps.clear();
    }

    /// Get the number of timestamps currently stored
    pub fn sample_count(&self) -> usize {
        self.timestamps.len()
    }

    /// Calculate intervals between consecutive timestamps
    ///
    /// Returns a vector of time deltas in milliseconds.
    fn calculate_intervals(&self) -> Vec<f64> {
        let timestamps: Vec<_> = self.timestamps.iter().copied().collect();

        if timestamps.len() < 2 {
            return Vec::new();
        }

        let mut intervals = Vec::with_capacity(timestamps.len() - 1);
        for i in 1..timestamps.len() {
            let interval = timestamps[i].saturating_sub(timestamps[i - 1]) as f64;
            if interval > 0.0 {
                intervals.push(interval);
            }
        }

        intervals
    }

    /// Calculate the variance of a set of values
    ///
    /// Returns (mean, variance, std_dev)
    fn calculate_variance(values: &[f64]) -> (f64, f64, f64) {
        if values.is_empty() {
            return (0.0, 0.0, 0.0);
        }

        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;

        if values.len() == 1 {
            return (mean, 0.0, 0.0);
        }

        let variance = values
            .iter()
            .map(|x| {
                let diff = x - mean;
                diff * diff
            })
            .sum::<f64>()
            / n;

        let std_dev = variance.sqrt();

        (mean, variance, std_dev)
    }

    /// Analyze the current timestamp buffer and detect trading patterns
    ///
    /// Returns a `ResonanceResult` containing detailed metrics and classification.
    ///
    /// # Example
    ///
    /// ```
    /// use ghost_brain::signals::ResonanceDetector;
    ///
    /// let mut detector = ResonanceDetector::new();
    ///
    /// // Add periodic bot-like timestamps (every 500ms)
    /// for i in 0..20 {
    ///     detector.add_timestamp(i * 500);
    /// }
    ///
    /// let result = detector.analyze();
    /// assert!(result.is_bot_likely());
    /// ```
    pub fn analyze(&self) -> ResonanceResult {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let sample_count = self.timestamps.len();

        // Check if we have enough samples
        if sample_count < self.config.min_samples {
            return ResonanceResult {
                resonance_score: 0.0,
                coefficient_variation: 0.0,
                mean_interval_ms: 0.0,
                std_dev_ms: 0.0,
                classification: ActivityClassification::Insufficient,
                sample_count,
                timestamp_ms: current_time,
            };
        }

        // Calculate intervals
        let intervals = self.calculate_intervals();

        if intervals.is_empty() {
            return ResonanceResult {
                resonance_score: 0.0,
                coefficient_variation: 0.0,
                mean_interval_ms: 0.0,
                std_dev_ms: 0.0,
                classification: ActivityClassification::Insufficient,
                sample_count,
                timestamp_ms: current_time,
            };
        }

        // Calculate statistical properties
        let (mean, _variance, std_dev) = Self::calculate_variance(&intervals);

        // Calculate coefficient of variation (CV)
        let cv = if mean > 0.0 { std_dev / mean } else { 0.0 };

        // Calculate resonance score (inverse of CV, normalized)
        // CV near 0 = high resonance (periodic)
        // CV > 1.5 = low resonance (random)
        let resonance_score = if cv < 0.001 {
            1.0 // Perfect periodicity
        } else if cv > 1.5 {
            0.0 // Highly random
        } else {
            (1.5 - cv) / 1.5 // Linear mapping [0, 1.5] -> [1.0, 0.0]
        };

        // Classify activity based on CV thresholds
        let classification = if cv < self.config.bot_threshold_cv {
            ActivityClassification::BotLikely
        } else if cv < self.config.human_threshold_cv {
            ActivityClassification::Suspicious
        } else {
            ActivityClassification::HumanLikely
        };

        ResonanceResult {
            resonance_score,
            coefficient_variation: cv,
            mean_interval_ms: mean,
            std_dev_ms: std_dev,
            classification,
            sample_count,
            timestamp_ms: current_time,
        }
    }

    /// Quick check if current pattern indicates bot activity
    ///
    /// This is a convenience method that performs analysis and returns
    /// a simple boolean result.
    ///
    /// # Example
    ///
    /// ```
    /// use ghost_brain::signals::ResonanceDetector;
    ///
    /// let mut detector = ResonanceDetector::new();
    /// // ... add timestamps ...
    /// if detector.is_bot_detected() {
    ///     println!("Bot activity detected!");
    /// }
    /// ```
    pub fn is_bot_detected(&self) -> bool {
        self.analyze().is_bot_likely()
    }
}

impl Default for ResonanceDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circular_buffer_basic() {
        let mut buffer: CircularBuffer<u64> = CircularBuffer::new(4);

        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.len(), 3);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_circular_buffer_overflow() {
        let mut buffer: CircularBuffer<u64> = CircularBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4); // Should evict 1
        buffer.push(5); // Should evict 2

        assert_eq!(buffer.len(), 3);
        let values: Vec<_> = buffer.iter().copied().collect();
        assert_eq!(values, vec![3, 4, 5]);
    }

    #[test]
    fn test_circular_buffer_clear() {
        let mut buffer: CircularBuffer<u64> = CircularBuffer::new(10);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.len(), 3);

        buffer.clear();

        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_resonance_detector_empty() {
        let detector = ResonanceDetector::new();
        let result = detector.analyze();

        assert_eq!(result.classification, ActivityClassification::Insufficient);
        assert_eq!(result.sample_count, 0);
        assert_eq!(result.resonance_score, 0.0);
    }

    #[test]
    fn test_resonance_detector_insufficient_samples() {
        let mut detector = ResonanceDetector::new();

        // Add only 3 timestamps (less than MIN_SAMPLES_FOR_ANALYSIS)
        detector.add_timestamp(1000);
        detector.add_timestamp(2000);
        detector.add_timestamp(3000);

        let result = detector.analyze();

        assert_eq!(result.classification, ActivityClassification::Insufficient);
        assert_eq!(result.sample_count, 3);
    }

    #[test]
    fn test_resonance_detector_periodic_bot_pattern() {
        let mut detector = ResonanceDetector::new();

        // Add highly periodic timestamps (every 500ms) - bot-like behavior
        for i in 0..20 {
            detector.add_timestamp(i * 500);
        }

        let result = detector.analyze();

        // Perfect periodicity should yield CV near 0 and high resonance
        assert!(
            result.coefficient_variation < 0.01,
            "CV should be near 0 for periodic pattern, got {}",
            result.coefficient_variation
        );
        assert!(
            result.resonance_score > 0.9,
            "Resonance should be high for periodic pattern, got {}",
            result.resonance_score
        );
        assert_eq!(result.classification, ActivityClassification::BotLikely);
        assert!(result.is_bot_likely());
    }

    #[test]
    fn test_resonance_detector_random_human_pattern() {
        let mut detector = ResonanceDetector::new();

        // Add random timestamps - human-like behavior
        let random_intervals = vec![
            100, 500, 1200, 300, 2000, 450, 3500, 800, 150, 1800, 600, 250, 4000, 350, 900, 1500,
            200, 2500, 700, 400,
        ];

        let mut timestamp = 0;
        for interval in random_intervals {
            timestamp += interval;
            detector.add_timestamp(timestamp);
        }

        let result = detector.analyze();

        // Random pattern should have high CV and low resonance
        assert!(
            result.coefficient_variation > 0.6,
            "CV should be high for random pattern, got {}",
            result.coefficient_variation
        );
        assert!(
            result.resonance_score < 0.5,
            "Resonance should be low for random pattern, got {}",
            result.resonance_score
        );
        assert!(
            result.is_human_likely()
                || matches!(result.classification, ActivityClassification::Suspicious)
        );
    }

    #[test]
    fn test_resonance_detector_suspicious_pattern() {
        let mut detector = ResonanceDetector::new();

        // Add semi-periodic timestamps with some variation - suspicious pattern
        let base_interval = 1000;
        for i in 0..20 {
            let variation = (i % 3) * 100; // Small variation
            detector.add_timestamp(i * base_interval + variation);
        }

        let result = detector.analyze();

        // Should be classified as suspicious or bot-like
        assert!(result.is_suspicious());
        assert!(result.coefficient_variation < DEFAULT_HUMAN_THRESHOLD_CV);
    }

    #[test]
    fn test_resonance_detector_add_multiple_timestamps() {
        let mut detector = ResonanceDetector::new();

        let timestamps = vec![1000, 1500, 2000, 2500, 3000, 3500, 4000, 4500, 5000, 5500];
        detector.add_timestamps(&timestamps);

        assert_eq!(detector.sample_count(), 10);

        let result = detector.analyze();
        assert!(result.is_bot_likely()); // Perfectly spaced 500ms intervals
    }

    #[test]
    fn test_resonance_detector_clear() {
        let mut detector = ResonanceDetector::new();

        for i in 0..20 {
            detector.add_timestamp(i * 500);
        }

        assert_eq!(detector.sample_count(), 20);

        detector.clear();

        assert_eq!(detector.sample_count(), 0);

        let result = detector.analyze();
        assert_eq!(result.classification, ActivityClassification::Insufficient);
    }

    #[test]
    fn test_resonance_detector_custom_config() {
        let config = ResonanceConfig {
            buffer_size: 32,
            bot_threshold_cv: 0.2,
            human_threshold_cv: 0.9,
            min_samples: 5,
        };

        let mut detector = ResonanceDetector::with_config(config);

        // Add periodic pattern
        for i in 0..10 {
            detector.add_timestamp(i * 1000);
        }

        let result = detector.analyze();

        assert!(result.is_bot_likely());
        assert!(result.coefficient_variation < 0.2);
    }

    #[test]
    fn test_resonance_detector_buffer_overflow() {
        let config = ResonanceConfig {
            buffer_size: 10, // Small buffer
            ..Default::default()
        };

        let mut detector = ResonanceDetector::with_config(config);

        // Add more timestamps than buffer size
        for i in 0..20 {
            detector.add_timestamp(i * 500);
        }

        // Should only keep last 10
        assert_eq!(detector.sample_count(), 10);

        let result = detector.analyze();
        // Should still detect periodic pattern in the last 10 samples
        assert!(result.is_bot_likely());
    }

    #[test]
    fn test_calculate_variance() {
        let values = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let (mean, _variance, std_dev) = ResonanceDetector::calculate_variance(&values);

        // Mean should be 5.0
        assert!((mean - 5.0).abs() < 0.01);

        // Standard deviation should be ~2.0
        assert!((std_dev - 2.0).abs() < 0.1);
    }

    #[test]
    fn test_resonance_result_helper_methods() {
        let bot_result = ResonanceResult {
            resonance_score: 0.95,
            coefficient_variation: 0.1,
            mean_interval_ms: 500.0,
            std_dev_ms: 50.0,
            classification: ActivityClassification::BotLikely,
            sample_count: 20,
            timestamp_ms: 1000000,
        };

        assert!(bot_result.is_bot_likely());
        assert!(bot_result.is_suspicious());
        assert!(!bot_result.is_human_likely());

        let human_result = ResonanceResult {
            resonance_score: 0.2,
            coefficient_variation: 1.0,
            mean_interval_ms: 1500.0,
            std_dev_ms: 1500.0,
            classification: ActivityClassification::HumanLikely,
            sample_count: 20,
            timestamp_ms: 1000000,
        };

        assert!(!human_result.is_bot_likely());
        assert!(!human_result.is_suspicious());
        assert!(human_result.is_human_likely());
    }

    #[test]
    fn test_resonance_detector_is_bot_detected() {
        let mut detector = ResonanceDetector::new();

        // Periodic pattern
        for i in 0..15 {
            detector.add_timestamp(i * 1000);
        }

        assert!(detector.is_bot_detected());

        // Clear and add random pattern
        detector.clear();

        let random_intervals = vec![
            100, 1500, 300, 2000, 450, 800, 150, 600, 250, 350, 900, 200, 700, 400, 1200,
        ];
        let mut timestamp = 0;
        for interval in random_intervals {
            timestamp += interval;
            detector.add_timestamp(timestamp);
        }

        assert!(!detector.is_bot_detected());
    }

    #[test]
    fn test_resonance_score_calculation() {
        let mut detector = ResonanceDetector::new();

        // Perfect periodicity (CV = 0)
        for i in 0..20 {
            detector.add_timestamp(i * 1000);
        }

        let result = detector.analyze();
        assert!(
            result.resonance_score > 0.99,
            "Perfect periodicity should have resonance ~1.0, got {}",
            result.resonance_score
        );
        assert!(
            result.coefficient_variation < 0.01,
            "Perfect periodicity should have CV ~0, got {}",
            result.coefficient_variation
        );

        // High randomness - extremely varied intervals
        detector.clear();
        let highly_random = vec![
            100, 5000, 50, 10000, 200, 15000, 100, 8000, 150, 20000, 300, 12000, 100, 18000, 250,
        ];
        let mut timestamp = 0;
        for interval in highly_random {
            timestamp += interval;
            detector.add_timestamp(timestamp);
        }

        let result2 = detector.analyze();
        // With these extreme variations, CV should be high and resonance low
        assert!(
            result2.coefficient_variation > 1.0,
            "Highly random pattern should have CV > 1.0, got {}",
            result2.coefficient_variation
        );
        assert!(
            result2.resonance_score < 0.4,
            "Highly random pattern should have low resonance, got {}",
            result2.resonance_score
        );
    }
}
