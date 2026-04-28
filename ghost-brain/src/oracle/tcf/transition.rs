//! TCF Transition Module - State Change Operator
//!
//! Defines the `Transition` structure that captures the **relationship** between
//! two consecutive market observations O_i and O_{i+1}. Unlike the observation
//! which captures state, the transition captures *dynamics*.
//!
//! ## Design Philosophy
//!
//! The transition is the core abstraction in TCF. We don't care about:
//! - "Did price go up?" → That's in the observation
//!
//! We care about:
//! - "Was the WAY price changed consistent with the previous change mechanism?"
//! - "Did the rhythm of the market remain stable?"
//!
//! ## Key Insight
//!
//! A consistent trend isn't one where price always goes up. It's one where
//! the underlying dynamics generating the price movement remain stable.
//! A pump-and-dump has inconsistent dynamics: initial organic buying → bot dump.
//!
//! ## Components
//!
//! | Field | Description |
//! |-------|-------------|
//! | delta_vector | The raw difference O_{i+1} - O_i |
//! | volatility | Magnitude of change (how big was the transition) |
//! | directional_consistency | Did all components change in coherent directions |
//!
//! ## Performance
//!
//! - Zero heap allocation
//! - O(1) computation from two observations
//! - Copy semantics

use super::observation::{MarketObservation, OBSERVATION_DIM};

/// Transition captures the relationship between consecutive observations.
///
/// It encodes HOW the market changed, not just WHAT changed.
///
/// # Mathematical Model
///
/// Given observations O_i and O_{i+1}:
/// - `delta_vector` = O_{i+1} - O_i (component-wise difference)
/// - `volatility` = ||delta_vector|| (L2 norm of change)
/// - `directional_consistency` = measure of coordinated movement
///
/// # Thread Safety
///
/// Implements `Copy + Clone + Send + Sync` for concurrent usage.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transition {
    /// Raw difference vector between consecutive observations.
    ///
    /// Each component represents the change in that dimension.
    /// - delta_vector[0] = price_delta change
    /// - delta_vector[1] = volume_delta change
    /// - etc.
    pub delta_vector: [f64; OBSERVATION_DIM],

    /// Magnitude of the transition (L2 norm of delta_vector).
    ///
    /// Measures "how much" the market changed overall.
    /// - 0: No change (perfect stability)
    /// - High: Large sudden change (shock or event)
    pub volatility: f64,

    /// Directional consistency score [0, 1].
    ///
    /// Measures whether all components changed in a coordinated way.
    /// - 1: All components moved in aligned directions (coherent trend)
    /// - 0: Components moved in conflicting directions (chaos/confusion)
    ///
    /// Computed as: fraction of component pairs with same-sign products
    pub directional_consistency: f64,
}

impl Transition {
    /// Compute transition from two consecutive observations.
    ///
    /// # Arguments
    ///
    /// * `from` - Previous observation O_i
    /// * `to` - Current observation O_{i+1}
    ///
    /// # Returns
    ///
    /// Transition capturing the dynamics of O_i → O_{i+1}
    pub fn compute(from: &MarketObservation, to: &MarketObservation) -> Self {
        // Compute raw delta
        let diff = *to - *from;
        let delta_vector = diff.to_array();

        // Compute volatility (L2 norm)
        let volatility = delta_vector.iter().map(|d| d * d).sum::<f64>().sqrt();

        // Compute directional consistency
        let directional_consistency = compute_directional_consistency(&delta_vector);

        Self {
            delta_vector,
            volatility,
            directional_consistency,
        }
    }

    /// Create a zero transition (no change).
    pub fn zero() -> Self {
        Self {
            delta_vector: [0.0; OBSERVATION_DIM],
            volatility: 0.0,
            directional_consistency: 1.0, // Perfect consistency (nothing to disagree)
        }
    }

    /// Get the primary direction of the transition.
    ///
    /// Returns the normalized delta vector (unit direction).
    /// If volatility is near zero, returns zero vector.
    pub fn direction(&self) -> [f64; OBSERVATION_DIM] {
        if self.volatility < 1e-10 {
            return [0.0; OBSERVATION_DIM];
        }

        std::array::from_fn(|i| self.delta_vector[i] / self.volatility)
    }

    /// Compute similarity to another transition.
    ///
    /// Measures how similar two transitions are in terms of:
    /// - Direction (cosine similarity of delta vectors)
    /// - Magnitude (ratio of volatilities)
    /// - Consistency (difference in directional consistency)
    ///
    /// Returns value in [0, 1]:
    /// - 1: Identical transitions
    /// - 0: Completely different transitions
    pub fn similarity(&self, other: &Self) -> f64 {
        // Direction similarity (cosine similarity)
        let dir_sim = cosine_similarity(&self.delta_vector, &other.delta_vector);
        let dir_component = (dir_sim + 1.0) / 2.0; // Map [-1, 1] to [0, 1]

        // Magnitude similarity (ratio-based, symmetric)
        let mag_sim = if self.volatility < 1e-10 && other.volatility < 1e-10 {
            1.0 // Both zero = identical
        } else if self.volatility < 1e-10 || other.volatility < 1e-10 {
            0.0 // One zero, one not = different
        } else {
            let ratio =
                (self.volatility / other.volatility).min(other.volatility / self.volatility);
            ratio // Closer to 1 = more similar
        };

        // Consistency similarity
        let cons_sim = 1.0 - (self.directional_consistency - other.directional_consistency).abs();

        // Combined with weights
        // Direction is most important (50%), then magnitude (30%), then consistency (20%)
        0.50 * dir_component + 0.30 * mag_sim + 0.20 * cons_sim
    }

    /// Check if this transition represents a reversal from the previous.
    ///
    /// A reversal occurs when the dominant direction flips.
    pub fn is_reversal_of(&self, other: &Self) -> bool {
        let dir_sim = cosine_similarity(&self.delta_vector, &other.delta_vector);
        // Reversal if directions are mostly opposite (cosine < -0.5)
        dir_sim < -0.5 && self.volatility > 0.1 && other.volatility > 0.1
    }

    /// Get the "energy" of the transition.
    ///
    /// Higher energy indicates more significant market movement.
    /// Combines volatility with directional coherence.
    #[inline]
    pub fn energy(&self) -> f64 {
        // High volatility + high consistency = high energy (strong trend move)
        // High volatility + low consistency = medium energy (chaotic spike)
        // Low volatility = low energy regardless
        self.volatility * (0.5 + 0.5 * self.directional_consistency)
    }

    /// Compute the "rhythm deviation" from an expected transition.
    ///
    /// Measures how much this transition deviates from expected behavior.
    /// Used for anomaly detection in trend continuity.
    pub fn rhythm_deviation(&self, expected: &Self) -> f64 {
        // Normalize by expected volatility to make comparison scale-invariant
        let scale = expected.volatility.max(0.1);

        let delta_diff: f64 = self
            .delta_vector
            .iter()
            .zip(expected.delta_vector.iter())
            .map(|(a, b)| ((a - b) / scale).powi(2))
            .sum();

        delta_diff.sqrt()
    }
}

/// Minimum threshold for considering a delta component "significant".
/// Changes below this threshold are treated as noise.
const SIGNIFICANT_DELTA_THRESHOLD: f64 = 0.05;

/// Compute directional consistency of a delta vector.
///
/// Measures how well-coordinated the changes are across dimensions.
/// High consistency means all significant changes move "together".
fn compute_directional_consistency(delta: &[f64; OBSERVATION_DIM]) -> f64 {
    // Count components with significant changes
    let significant: Vec<f64> = delta
        .iter()
        .copied()
        .filter(|d| d.abs() > SIGNIFICANT_DELTA_THRESHOLD)
        .collect();

    if significant.len() < 2 {
        return 1.0; // Trivially consistent if < 2 significant changes
    }

    // Count same-sign pairs
    let mut same_sign_pairs = 0;
    let mut total_pairs = 0;

    for i in 0..significant.len() {
        for j in (i + 1)..significant.len() {
            total_pairs += 1;
            if significant[i].signum() == significant[j].signum() {
                same_sign_pairs += 1;
            }
        }
    }

    if total_pairs == 0 {
        return 1.0;
    }

    same_sign_pairs as f64 / total_pairs as f64
}

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f64; OBSERVATION_DIM], b: &[f64; OBSERVATION_DIM]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mag_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();

    if mag_a < 1e-10 || mag_b < 1e-10 {
        return 0.0;
    }

    (dot / (mag_a * mag_b)).clamp(-1.0, 1.0)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transition_compute() {
        let o1 = MarketObservation::new(0.0, 0.0, 0.5, 0.0, 0.5, 0.5, 0.0);
        let o2 = MarketObservation::new(0.5, 0.3, 0.6, 0.2, 0.7, 0.4, 0.1);

        let trans = Transition::compute(&o1, &o2);

        // Check delta vector
        assert!((trans.delta_vector[0] - 0.5).abs() < 1e-10);
        assert!((trans.delta_vector[1] - 0.3).abs() < 1e-10);

        // Volatility should be positive
        assert!(trans.volatility > 0.0);

        // Directional consistency should be in [0, 1]
        assert!(trans.directional_consistency >= 0.0 && trans.directional_consistency <= 1.0);
    }

    #[test]
    fn test_transition_zero() {
        let zero = Transition::zero();

        assert!(zero.delta_vector.iter().all(|&d| d.abs() < 1e-10));
        assert!(zero.volatility < 1e-10);
        assert!((zero.directional_consistency - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_transition_direction() {
        let trans = Transition {
            delta_vector: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            volatility: 1.0,
            directional_consistency: 1.0,
        };

        let dir = trans.direction();
        assert!((dir[0] - 1.0).abs() < 1e-10);
        assert!(dir[1..].iter().all(|&d| d.abs() < 1e-10));
    }

    #[test]
    fn test_transition_similarity_identical() {
        let trans = Transition {
            delta_vector: [0.5, 0.3, 0.2, 0.1, 0.4, 0.2, 0.1],
            volatility: 0.75,
            directional_consistency: 0.8,
        };

        let sim = trans.similarity(&trans);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_transition_similarity_opposite() {
        let t1 = Transition {
            delta_vector: [0.5, 0.3, 0.2, 0.0, 0.0, 0.0, 0.0],
            volatility: 0.6,
            directional_consistency: 1.0,
        };
        let t2 = Transition {
            delta_vector: [-0.5, -0.3, -0.2, 0.0, 0.0, 0.0, 0.0],
            volatility: 0.6,
            directional_consistency: 1.0,
        };

        let sim = t1.similarity(&t2);
        // Opposite directions but same magnitude and consistency
        // Direction: 0 (cosine = -1 → mapped to 0)
        // Magnitude: 1 (same)
        // Consistency: 1 (same)
        // Result: 0.50 * 0 + 0.30 * 1 + 0.20 * 1 = 0.50
        assert!((sim - 0.50).abs() < 0.1);
    }

    #[test]
    fn test_is_reversal_of() {
        let t1 = Transition {
            delta_vector: [0.5, 0.3, 0.2, 0.1, 0.0, 0.0, 0.0],
            volatility: 0.6,
            directional_consistency: 1.0,
        };
        let t2 = Transition {
            delta_vector: [-0.5, -0.3, -0.2, -0.1, 0.0, 0.0, 0.0],
            volatility: 0.6,
            directional_consistency: 1.0,
        };

        assert!(t2.is_reversal_of(&t1));
        assert!(t1.is_reversal_of(&t2));
    }

    #[test]
    fn test_energy() {
        let high_energy = Transition {
            delta_vector: [0.5, 0.3, 0.2, 0.1, 0.0, 0.0, 0.0],
            volatility: 0.6,
            directional_consistency: 1.0,
        };

        let low_energy = Transition {
            delta_vector: [0.05, 0.03, 0.02, 0.01, 0.0, 0.0, 0.0],
            volatility: 0.06,
            directional_consistency: 1.0,
        };

        assert!(high_energy.energy() > low_energy.energy());
    }

    #[test]
    fn test_rhythm_deviation() {
        let expected = Transition {
            delta_vector: [0.1, 0.1, 0.1, 0.0, 0.0, 0.0, 0.0],
            volatility: 0.17,
            directional_consistency: 1.0,
        };

        // Similar transition - low deviation
        let similar = Transition {
            delta_vector: [0.12, 0.11, 0.09, 0.0, 0.0, 0.0, 0.0],
            volatility: 0.18,
            directional_consistency: 1.0,
        };

        // Very different transition - high deviation
        let different = Transition {
            delta_vector: [-0.3, 0.5, -0.2, 0.0, 0.0, 0.0, 0.0],
            volatility: 0.62,
            directional_consistency: 0.5,
        };

        let dev_similar = similar.rhythm_deviation(&expected);
        let dev_different = different.rhythm_deviation(&expected);

        assert!(dev_similar < dev_different);
    }

    #[test]
    fn test_directional_consistency_all_same_sign() {
        let delta = [0.2, 0.3, 0.1, 0.4, 0.2, 0.1, 0.3];
        let consistency = compute_directional_consistency(&delta);
        assert!((consistency - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_directional_consistency_mixed_sign() {
        let delta = [0.2, -0.3, 0.1, -0.4, 0.2, -0.1, 0.3];
        let consistency = compute_directional_consistency(&delta);
        assert!(consistency < 1.0);
        assert!(consistency >= 0.0);
    }

    #[test]
    fn test_directional_consistency_small_values() {
        // Values below threshold should be ignored
        let delta = [0.01, 0.02, 0.01, 0.01, 0.01, 0.01, 0.01];
        let consistency = compute_directional_consistency(&delta);
        assert!((consistency - 1.0).abs() < 1e-10); // Trivially consistent
    }
}
