//! TCF Observation Module - Market State Representation
//!
//! Defines the `MarketObservation` structure that captures the state of the market
//! at a single scoring cycle S(i). All variables are normalized to ensure consistent
//! mathematical operations across the TCF pipeline.
//!
//! ## Design Philosophy
//!
//! The observation represents **local** market state measured within a single cycle.
//! It does NOT represent trend direction - that emerges from analyzing transitions
//! between observations. Each variable is:
//!
//! - **Normalized**: All values in [-1, 1] or [0, 1] range
//! - **Local**: Computed within the current cycle only
//! - **Independent of classics**: No RSI, EMA, MACD, VWAP derivatives
//!
//! ## Variables
//!
//! | Variable | Range | Interpretation |
//! |----------|-------|----------------|
//! | price_delta | [-1, 1] | Price change direction and magnitude |
//! | volume_delta | [-1, 1] | Volume change direction and magnitude |
//! | liquidity_entropy | [0, 1] | Disorder in liquidity distribution |
//! | order_flow_imbalance | [-1, 1] | Buy vs sell pressure asymmetry |
//! | mpcf | [0, 1] | Actor classification confidence |
//! | jitter | [0, 1] | Transaction timing noise level |
//! | phase_sync | [0, 1] | Synchronization of market participants |
//!
//! ## Performance
//!
//! - Zero heap allocation in hot path
//! - All operations are O(1)
//! - Copy semantics for efficient passing

use std::ops::{Add, Sub};

/// Number of dimensions in the observation vector
pub const OBSERVATION_DIM: usize = 7;

/// MarketObservation captures the state of the market at a single scoring cycle.
///
/// Each field is a normalized scalar that represents one aspect of market dynamics.
/// The observation is designed to be computed locally within each cycle without
/// relying on historical data or external indicators.
///
/// # Mathematical Properties
///
/// - All fields normalized to consistent ranges for uniform scaling
/// - Vector arithmetic supported via Add/Sub traits
/// - Euclidean distance and dot product operations available
///
/// # Thread Safety
///
/// Implements `Copy + Clone + Send + Sync` for concurrent usage.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MarketObservation {
    /// Price change direction and magnitude normalized to [-1, 1].
    ///
    /// Computed as: `(price_now - price_prev) / (price_prev * max_expected_change)`
    /// - Positive: Price increased
    /// - Negative: Price decreased
    /// - Magnitude: Relative size of change
    pub price_delta: f64,

    /// Volume change direction and magnitude normalized to [-1, 1].
    ///
    /// Computed as: `(volume_now - volume_prev) / max(volume_prev, volume_now)`
    /// - Positive: Volume increasing (growing activity)
    /// - Negative: Volume decreasing (dying activity)
    pub volume_delta: f64,

    /// Liquidity distribution entropy normalized to [0, 1].
    ///
    /// Measures how evenly liquidity is distributed across price levels.
    /// - 0: All liquidity concentrated at one point (dangerous)
    /// - 1: Uniform distribution (healthy market)
    ///
    /// Computed using Shannon entropy of liquidity buckets.
    pub liquidity_entropy: f64,

    /// Order flow imbalance normalized to [-1, 1].
    ///
    /// Measures asymmetry between buy and sell pressure.
    /// - +1: All buying pressure (extreme bullish)
    /// - -1: All selling pressure (extreme bearish)
    /// - 0: Balanced order flow
    ///
    /// Computed as: `(buy_volume - sell_volume) / (buy_volume + sell_volume)`
    pub order_flow_imbalance: f64,

    /// MPCF (Micro-Payload Cognitive Fingerprint) confidence [0, 1].
    ///
    /// Actor classification confidence from the MPCF module.
    /// Higher values indicate clearer actor classification (human vs bot).
    /// - 0: Unknown/ambiguous actor
    /// - 1: High confidence classification
    pub mpcf: f64,

    /// Transaction timing jitter normalized to [0, 1].
    ///
    /// Measures irregularity in transaction timing intervals.
    /// - 0: Perfect regularity (likely bot)
    /// - 1: High irregularity (likely human or chaotic)
    ///
    /// Computed as: CV (coefficient of variation) of inter-transaction times.
    pub jitter: f64,

    /// Market participant synchronization [0, 1].
    ///
    /// Measures how synchronized market participants are in their actions.
    /// - 0: Completely desynchronized (independent actors)
    /// - 1: Perfect synchronization (coordinated action or bot swarm)
    ///
    /// High sync with low jitter = likely coordinated bot activity
    /// High sync with high jitter = viral organic movement
    pub phase_sync: f64,
}

impl MarketObservation {
    /// Create a new MarketObservation with explicit values.
    ///
    /// # Arguments
    ///
    /// All arguments are clamped to their valid ranges.
    pub fn new(
        price_delta: f64,
        volume_delta: f64,
        liquidity_entropy: f64,
        order_flow_imbalance: f64,
        mpcf: f64,
        jitter: f64,
        phase_sync: f64,
    ) -> Self {
        Self {
            price_delta: price_delta.clamp(-1.0, 1.0),
            volume_delta: volume_delta.clamp(-1.0, 1.0),
            liquidity_entropy: liquidity_entropy.clamp(0.0, 1.0),
            order_flow_imbalance: order_flow_imbalance.clamp(-1.0, 1.0),
            mpcf: mpcf.clamp(0.0, 1.0),
            jitter: jitter.clamp(0.0, 1.0),
            phase_sync: phase_sync.clamp(0.0, 1.0),
        }
    }

    /// Create a neutral observation (market at rest).
    ///
    /// Used as initialization state before any data arrives.
    pub fn neutral() -> Self {
        Self {
            price_delta: 0.0,
            volume_delta: 0.0,
            liquidity_entropy: 0.5,
            order_flow_imbalance: 0.0,
            mpcf: 0.5,
            jitter: 0.5,
            phase_sync: 0.0,
        }
    }

    /// Convert observation to a fixed-size array for mathematical operations.
    ///
    /// Order: [price_delta, volume_delta, liquidity_entropy, order_flow_imbalance, mpcf, jitter, phase_sync]
    #[inline]
    pub fn to_array(&self) -> [f64; OBSERVATION_DIM] {
        [
            self.price_delta,
            self.volume_delta,
            self.liquidity_entropy,
            self.order_flow_imbalance,
            self.mpcf,
            self.jitter,
            self.phase_sync,
        ]
    }

    /// Create observation from a fixed-size array.
    ///
    /// Values are clamped to valid ranges.
    pub fn from_array(arr: [f64; OBSERVATION_DIM]) -> Self {
        Self::new(arr[0], arr[1], arr[2], arr[3], arr[4], arr[5], arr[6])
    }

    /// Compute Euclidean distance to another observation.
    ///
    /// Used for measuring "distance" between market states.
    #[inline]
    pub fn euclidean_distance(&self, other: &Self) -> f64 {
        let a = self.to_array();
        let b = other.to_array();

        let sum_sq: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum();

        sum_sq.sqrt()
    }

    /// Compute dot product with another observation.
    ///
    /// Used for measuring "alignment" between market states.
    #[inline]
    pub fn dot(&self, other: &Self) -> f64 {
        let a = self.to_array();
        let b = other.to_array();

        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// Compute L2 norm (magnitude) of the observation vector.
    #[inline]
    pub fn magnitude(&self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Compute cosine similarity with another observation.
    ///
    /// Returns value in [-1, 1]:
    /// - 1: Perfectly aligned
    /// - 0: Orthogonal
    /// - -1: Opposite directions
    #[inline]
    pub fn cosine_similarity(&self, other: &Self) -> f64 {
        let mag_self = self.magnitude();
        let mag_other = other.magnitude();

        if mag_self < 1e-10 || mag_other < 1e-10 {
            return 0.0;
        }

        self.dot(other) / (mag_self * mag_other)
    }

    /// Scale observation by a scalar factor.
    #[inline]
    pub fn scale(&self, factor: f64) -> Self {
        let arr = self.to_array();
        let scaled: [f64; OBSERVATION_DIM] = std::array::from_fn(|i| arr[i] * factor);
        Self::from_array(scaled)
    }

    /// Check if observation represents bullish market state.
    ///
    /// Bullish indicators:
    /// - Positive price delta
    /// - Positive volume delta
    /// - Positive order flow imbalance (more buying)
    pub fn is_bullish(&self) -> bool {
        self.price_delta > 0.0 && self.volume_delta > 0.0 && self.order_flow_imbalance > 0.0
    }

    /// Check if observation represents bearish market state.
    ///
    /// Bearish indicators:
    /// - Negative price delta
    /// - Negative or declining volume
    /// - Negative order flow imbalance (more selling)
    pub fn is_bearish(&self) -> bool {
        self.price_delta < 0.0 && self.order_flow_imbalance < 0.0
    }

    /// Compute "momentum alignment" - how well price and volume changes align.
    ///
    /// Returns [0, 1]:
    /// - 1: Price and volume move in same direction (strong trend)
    /// - 0.5: One of them is near zero (neutral/inconclusive)
    /// - 0: Price and volume diverge (weak/confused trend)
    #[inline]
    pub fn momentum_alignment(&self) -> f64 {
        const NEAR_ZERO_THRESHOLD: f64 = 0.01;

        // If either delta is near zero, return neutral
        if self.price_delta.abs() < NEAR_ZERO_THRESHOLD
            || self.volume_delta.abs() < NEAR_ZERO_THRESHOLD
        {
            return 0.5;
        }

        let sign_agreement = self.price_delta * self.volume_delta;
        // Positive product = same sign = aligned
        // Negative product = different signs = divergent
        if sign_agreement > 0.0 {
            1.0
        } else {
            0.0
        }
    }
}

impl Add for MarketObservation {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        let a = self.to_array();
        let b = other.to_array();
        let sum: [f64; OBSERVATION_DIM] = std::array::from_fn(|i| a[i] + b[i]);
        Self::from_array(sum)
    }
}

impl Sub for MarketObservation {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        let a = self.to_array();
        let b = other.to_array();
        let diff: [f64; OBSERVATION_DIM] = std::array::from_fn(|i| a[i] - b[i]);
        Self::from_array(diff)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_clamping() {
        let obs = MarketObservation::new(2.0, -2.0, 1.5, -1.5, 2.0, -0.5, 1.1);

        assert_eq!(obs.price_delta, 1.0);
        assert_eq!(obs.volume_delta, -1.0);
        assert_eq!(obs.liquidity_entropy, 1.0);
        assert_eq!(obs.order_flow_imbalance, -1.0);
        assert_eq!(obs.mpcf, 1.0);
        assert_eq!(obs.jitter, 0.0);
        assert_eq!(obs.phase_sync, 1.0);
    }

    #[test]
    fn test_neutral() {
        let neutral = MarketObservation::neutral();

        assert_eq!(neutral.price_delta, 0.0);
        assert_eq!(neutral.volume_delta, 0.0);
        assert_eq!(neutral.liquidity_entropy, 0.5);
        assert_eq!(neutral.order_flow_imbalance, 0.0);
        assert_eq!(neutral.mpcf, 0.5);
        assert_eq!(neutral.jitter, 0.5);
        assert_eq!(neutral.phase_sync, 0.0);
    }

    #[test]
    fn test_to_from_array() {
        let obs = MarketObservation::new(0.5, -0.3, 0.8, 0.2, 0.9, 0.4, 0.1);
        let arr = obs.to_array();
        let restored = MarketObservation::from_array(arr);

        assert_eq!(obs, restored);
    }

    #[test]
    fn test_euclidean_distance() {
        let a = MarketObservation::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let b = MarketObservation::new(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        assert!((a.euclidean_distance(&b) - 1.0).abs() < 1e-10);
        assert!((a.euclidean_distance(&a)).abs() < 1e-10);
    }

    #[test]
    fn test_dot_product() {
        let a = MarketObservation::new(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let b = MarketObservation::new(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        assert!((a.dot(&b) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = MarketObservation::new(1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let b = MarketObservation::new(0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0);

        // Same direction, should be 1.0
        assert!((a.cosine_similarity(&b) - 1.0).abs() < 1e-10);

        // Opposite direction
        let c = MarketObservation::new(-1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert!((a.cosine_similarity(&c) - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn test_add_sub() {
        let a = MarketObservation::new(0.3, 0.2, 0.5, 0.1, 0.4, 0.3, 0.2);
        let b = MarketObservation::new(0.2, 0.1, 0.3, 0.2, 0.3, 0.2, 0.1);

        let sum = a + b;
        assert!((sum.price_delta - 0.5).abs() < 1e-10);
        assert!((sum.volume_delta - 0.3).abs() < 1e-10);

        let diff = a - b;
        assert!((diff.price_delta - 0.1).abs() < 1e-10);
        assert!((diff.volume_delta - 0.1).abs() < 1e-10);
    }

    #[test]
    fn test_is_bullish() {
        let bullish = MarketObservation::new(0.5, 0.3, 0.5, 0.4, 0.5, 0.5, 0.3);
        assert!(bullish.is_bullish());

        let bearish = MarketObservation::new(-0.5, 0.3, 0.5, -0.4, 0.5, 0.5, 0.3);
        assert!(!bearish.is_bullish());
    }

    #[test]
    fn test_is_bearish() {
        let bearish = MarketObservation::new(-0.5, -0.3, 0.5, -0.4, 0.5, 0.5, 0.3);
        assert!(bearish.is_bearish());

        let bullish = MarketObservation::new(0.5, 0.3, 0.5, 0.4, 0.5, 0.5, 0.3);
        assert!(!bullish.is_bearish());
    }

    #[test]
    fn test_momentum_alignment() {
        // Same direction = high alignment
        let aligned = MarketObservation::new(0.5, 0.3, 0.5, 0.0, 0.5, 0.5, 0.0);
        assert!((aligned.momentum_alignment() - 1.0).abs() < 1e-10);

        // Opposite direction = low alignment
        let divergent = MarketObservation::new(0.5, -0.3, 0.5, 0.0, 0.5, 0.5, 0.0);
        assert!((divergent.momentum_alignment() - 0.0).abs() < 1e-10);

        // Near-zero volume = neutral (0.5)
        let neutral = MarketObservation::new(0.5, 0.005, 0.5, 0.0, 0.5, 0.5, 0.0);
        assert!((neutral.momentum_alignment() - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_scale() {
        let obs = MarketObservation::new(0.4, 0.2, 0.5, 0.3, 0.6, 0.4, 0.2);
        let scaled = obs.scale(0.5);

        assert!((scaled.price_delta - 0.2).abs() < 1e-10);
        assert!((scaled.volume_delta - 0.1).abs() < 1e-10);
    }

    #[test]
    fn test_magnitude() {
        let unit = MarketObservation::new(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert!((unit.magnitude() - 1.0).abs() < 1e-10);

        let zero = MarketObservation::default();
        assert!(zero.magnitude() < 1e-10);
    }

    #[test]
    fn test_default_is_zero() {
        let default = MarketObservation::default();
        let zero = MarketObservation::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        assert_eq!(default, zero);
    }
}
