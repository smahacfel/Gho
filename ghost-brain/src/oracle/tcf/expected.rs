//! TCF Expected Transition Module - Adaptive Learning Model
//!
//! Implements an adaptive model that learns the "expected" transition behavior
//! online during the scoring cycles. The model starts fresh at S1 and builds
//! expectations based on observed transitions.
//!
//! ## Design Philosophy
//!
//! The Expected Transition Model does NOT use:
//! - Machine Learning / Neural Networks
//! - Full historical storage
//! - Pre-trained parameters
//!
//! Instead, it uses:
//! - Sliding window with exponential forgetting
//! - Online Welford algorithm for variance estimation
//! - Adaptive learning rate based on data quality
//!
//! ## Key Properties
//!
//! 1. **Slower than market**: The model deliberately reacts slower than raw
//!    transitions. This is crucial - we want to detect DEVIATIONS from the
//!    established pattern, not follow every twitch.
//!
//! 2. **Anomaly-resistant**: Single outlier transitions are downweighted.
//!    The model requires sustained pattern changes to update expectations.
//!
//! 3. **Memory-bounded**: Uses fixed-size accumulators, no growing vectors.
//!
//! ## Mathematical Model
//!
//! Expected transition at cycle i:
//! ```text
//! E[T_i] = α * T_{i-1} + (1-α) * E[T_{i-1}]
//! ```
//! where α is the adaptive learning rate based on transition consistency.
//!
//! ## Performance
//!
//! - O(1) update per cycle
//! - O(1) memory (fixed-size accumulators)
//! - No heap allocation in steady state

use super::observation::OBSERVATION_DIM;
use super::transition::Transition;

/// Default forgetting factor (weight decay per cycle).
/// Higher = faster forgetting, lower = longer memory.
const DEFAULT_ALPHA_BASE: f64 = 0.15;

/// Minimum learning rate (prevents model freeze).
const MIN_ALPHA: f64 = 0.05;

/// Maximum learning rate (prevents overreaction).
const MAX_ALPHA: f64 = 0.40;

/// Minimum transitions before model is considered "primed".
const MIN_TRANSITIONS_FOR_PRIME: usize = 3;

/// Outlier detection threshold (number of std deviations).
const OUTLIER_THRESHOLD_SIGMA: f64 = 2.5;

/// Stability threshold for considering pattern "established".
const STABILITY_THRESHOLD: f64 = 0.15;

/// Expected transition model that learns online.
///
/// Maintains running estimates of:
/// - Expected delta direction
/// - Expected volatility
/// - Expected directional consistency
/// - Variance estimates for anomaly detection
///
/// # Thread Safety
///
/// This struct is NOT thread-safe by design. Each scoring pipeline
/// should have its own instance.
#[derive(Debug, Clone)]
pub struct ExpectedTransitionModel {
    /// Running estimate of expected delta vector.
    expected_delta: [f64; OBSERVATION_DIM],

    /// Running estimate of expected volatility.
    expected_volatility: f64,

    /// Running estimate of expected directional consistency.
    expected_consistency: f64,

    /// Online variance estimation for delta components.
    delta_variance: WelfordState,

    /// Online variance estimation for volatility.
    volatility_variance: WelfordOnline,

    /// Number of transitions observed.
    transition_count: usize,

    /// Current adaptive learning rate.
    current_alpha: f64,

    /// Stability score [0, 1] indicating how established the pattern is.
    stability: f64,

    /// Recent transitions for trend analysis (circular buffer).
    recent_transitions: [Transition; 4],
    recent_idx: usize,
}

impl Default for ExpectedTransitionModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpectedTransitionModel {
    /// Create a new model in cold start state.
    pub fn new() -> Self {
        Self {
            expected_delta: [0.0; OBSERVATION_DIM],
            expected_volatility: 0.1,  // Small non-zero default
            expected_consistency: 0.5, // Neutral default
            delta_variance: WelfordState::new(),
            volatility_variance: WelfordOnline::new(),
            transition_count: 0,
            current_alpha: DEFAULT_ALPHA_BASE,
            stability: 0.0,
            recent_transitions: [Transition::zero(); 4],
            recent_idx: 0,
        }
    }

    /// Reset the model to cold start state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Check if the model has enough data to make predictions.
    pub fn is_primed(&self) -> bool {
        self.transition_count >= MIN_TRANSITIONS_FOR_PRIME
    }

    /// Get the current stability score [0, 1].
    ///
    /// High stability means the pattern is well-established and
    /// we can confidently detect deviations.
    pub fn stability(&self) -> f64 {
        self.stability
    }

    /// Get the number of transitions observed.
    pub fn transition_count(&self) -> usize {
        self.transition_count
    }

    /// Update the model with a new observed transition.
    ///
    /// # Arguments
    ///
    /// * `observed` - The transition that just occurred
    ///
    /// # Returns
    ///
    /// The previous expected transition (before this update).
    pub fn update(&mut self, observed: &Transition) -> ExpectedTransition {
        // Capture current expectations before updating
        let previous_expected = self.get_expected();

        // Check for outlier
        let is_outlier = self.is_outlier(observed);

        // Calculate adaptive alpha
        let alpha = self.calculate_adaptive_alpha(observed, is_outlier);
        self.current_alpha = alpha;

        // Update running estimates with exponential smoothing
        // Lower alpha for outliers to resist jumps
        let effective_alpha = if is_outlier { alpha * 0.3 } else { alpha };

        // Update delta vector
        for i in 0..OBSERVATION_DIM {
            self.expected_delta[i] = self.expected_delta[i] * (1.0 - effective_alpha)
                + observed.delta_vector[i] * effective_alpha;
        }

        // Update volatility.
        // Anchor to first real transition so cold-start scale does not dominate rhythm scoring.
        if self.transition_count == 0 {
            self.expected_volatility = observed.volatility;
        } else {
            // Speed up downward adaptation when volatility contracts after a shock.
            // This prevents long-lived overestimation that collapses rhythm_score.
            let expected_scale = self.expected_volatility.max(1e-6);
            let contraction = (1.0 - observed.volatility / expected_scale).max(0.0);
            let volatility_alpha =
                (effective_alpha * (1.0 + contraction)).clamp(MIN_ALPHA, MAX_ALPHA);
            self.expected_volatility = self.expected_volatility * (1.0 - volatility_alpha)
                + observed.volatility * volatility_alpha;
        }

        // Update consistency
        self.expected_consistency = self.expected_consistency * (1.0 - effective_alpha)
            + observed.directional_consistency * effective_alpha;

        // Update variance estimates
        self.delta_variance.update(&observed.delta_vector);
        self.volatility_variance.update(observed.volatility);

        // Store in recent buffer
        self.recent_transitions[self.recent_idx] = *observed;
        self.recent_idx = (self.recent_idx + 1) % 4;

        // Update stability
        self.update_stability();

        self.transition_count += 1;

        previous_expected
    }

    /// Get the current expected transition.
    pub fn get_expected(&self) -> ExpectedTransition {
        if self.transition_count == 0 {
            return ExpectedTransition::cold_start();
        }

        ExpectedTransition {
            delta_vector: self.expected_delta,
            volatility: self.expected_volatility,
            directional_consistency: self.expected_consistency,
            delta_std: self.delta_variance.std_dev(),
            volatility_std: self.volatility_variance.std_dev(),
            confidence: if self.is_primed() {
                self.calculate_confidence()
            } else {
                0.0
            },
        }
    }

    /// Predict the next expected transition based on trend analysis.
    ///
    /// Uses recent transitions to extrapolate where the pattern is heading.
    pub fn predict_next(&self) -> ExpectedTransition {
        if self.transition_count < 2 {
            return ExpectedTransition::cold_start();
        }

        // Simple momentum-based prediction
        // Look at the direction of change in expected values
        let expected = self.get_expected();

        // Extrapolate using recent trend
        if self.transition_count >= 3 {
            let trend = self.calculate_trend();

            let mut predicted_delta = expected.delta_vector;
            for i in 0..OBSERVATION_DIM {
                // Small extrapolation in trend direction
                predicted_delta[i] += trend[i] * 0.3;
            }

            ExpectedTransition {
                delta_vector: predicted_delta,
                volatility: expected.volatility,
                directional_consistency: expected.directional_consistency,
                delta_std: expected.delta_std,
                volatility_std: expected.volatility_std,
                confidence: expected.confidence * 0.8, // Reduce confidence for predictions
            }
        } else {
            expected
        }
    }

    /// Check if an observed transition is an outlier.
    fn is_outlier(&self, observed: &Transition) -> bool {
        if !self.is_primed() {
            return false; // Can't detect outliers without baseline
        }

        let vol_std = self.volatility_variance.std_dev();
        if vol_std < 1e-10 {
            return false;
        }

        let vol_diff = (observed.volatility - self.expected_volatility).abs();
        vol_diff > OUTLIER_THRESHOLD_SIGMA * vol_std
    }

    /// Calculate adaptive learning rate based on recent behavior.
    fn calculate_adaptive_alpha(&self, observed: &Transition, is_outlier: bool) -> f64 {
        let mut alpha = DEFAULT_ALPHA_BASE;

        // Reduce alpha if we're well-established (slow adaptation)
        if self.stability > STABILITY_THRESHOLD {
            alpha *= 0.7;
        }

        // Reduce alpha for outliers
        if is_outlier {
            alpha *= 0.3;
        }

        // Increase alpha if recent transitions are very consistent
        // (market is stable, can learn faster)
        if self.transition_count > 3 {
            let recent_consistency = self.calculate_recent_consistency();
            if recent_consistency > 0.8 {
                alpha *= 1.3;
            }
        }

        // Increase alpha early on to prime faster
        if self.transition_count < 5 {
            alpha *= 1.5;
        }

        alpha.clamp(MIN_ALPHA, MAX_ALPHA)
    }

    /// Calculate consistency among recent transitions.
    fn calculate_recent_consistency(&self) -> f64 {
        if self.transition_count < 2 {
            return 0.5;
        }

        let count = self.transition_count.min(4);
        let mut total_sim = 0.0;
        let mut pairs = 0;

        for i in 0..count {
            for j in (i + 1)..count {
                total_sim += self.recent_transitions[i].similarity(&self.recent_transitions[j]);
                pairs += 1;
            }
        }

        if pairs == 0 {
            0.5
        } else {
            total_sim / pairs as f64
        }
    }

    /// Calculate confidence in current expectations.
    fn calculate_confidence(&self) -> f64 {
        if !self.is_primed() {
            return 0.0;
        }

        let mut confidence = 0.3; // Base confidence

        // More transitions = higher confidence (up to +0.3)
        let data_factor = (self.transition_count as f64 / 10.0).min(1.0) * 0.3;
        confidence += data_factor;

        // Higher stability = higher confidence (up to +0.25)
        confidence += self.stability * 0.25;

        // Lower variance = higher confidence (up to +0.15)
        let vol_cv = if self.expected_volatility > 1e-10 {
            self.volatility_variance.std_dev() / self.expected_volatility
        } else {
            1.0
        };
        let variance_factor = (1.0 - vol_cv.clamp(0.0, 1.0)) * 0.15;
        confidence += variance_factor;

        confidence.clamp(0.0, 1.0)
    }

    /// Update stability score based on recent behavior.
    fn update_stability(&mut self) {
        if self.transition_count < 3 {
            self.stability = 0.0;
            return;
        }

        // Stability based on variance of recent transitions
        let vol_cv = if self.expected_volatility > 1e-10 {
            self.volatility_variance.std_dev() / self.expected_volatility
        } else {
            1.0
        };

        let recent_consistency = self.calculate_recent_consistency();

        // Combine: low CV and high consistency = high stability
        self.stability =
            ((1.0 - vol_cv.clamp(0.0, 1.0)) * 0.6 + recent_consistency * 0.4).clamp(0.0, 1.0);
    }

    /// Calculate trend in delta values.
    fn calculate_trend(&self) -> [f64; OBSERVATION_DIM] {
        if self.transition_count < 3 {
            return [0.0; OBSERVATION_DIM];
        }

        // Simple finite difference on recent values
        let count = self.transition_count.min(4);
        let mut trend = [0.0; OBSERVATION_DIM];

        if count >= 2 {
            let newest_idx = (self.recent_idx + 3) % 4;
            let oldest_idx = (self.recent_idx + 4 - count) % 4;

            for i in 0..OBSERVATION_DIM {
                trend[i] = (self.recent_transitions[newest_idx].delta_vector[i]
                    - self.recent_transitions[oldest_idx].delta_vector[i])
                    / (count - 1) as f64;
            }
        }

        trend
    }
}

/// Expected transition with uncertainty bounds.
///
/// Contains the model's prediction of what the next transition should look like,
/// along with uncertainty estimates that can be used for anomaly detection.
#[derive(Debug, Clone, Copy)]
pub struct ExpectedTransition {
    /// Expected delta vector.
    pub delta_vector: [f64; OBSERVATION_DIM],

    /// Expected volatility (magnitude of change).
    pub volatility: f64,

    /// Expected directional consistency.
    pub directional_consistency: f64,

    /// Standard deviation of delta components.
    pub delta_std: [f64; OBSERVATION_DIM],

    /// Standard deviation of volatility.
    pub volatility_std: f64,

    /// Confidence in this expectation [0, 1].
    pub confidence: f64,
}

impl ExpectedTransition {
    /// Create a cold start expected transition.
    ///
    /// Used when the model hasn't seen enough data.
    pub fn cold_start() -> Self {
        Self {
            delta_vector: [0.0; OBSERVATION_DIM],
            volatility: 0.1,
            directional_consistency: 0.5,
            delta_std: [0.1; OBSERVATION_DIM],
            volatility_std: 0.1,
            confidence: 0.0,
        }
    }

    /// Convert to a Transition struct for comparison.
    pub fn to_transition(&self) -> Transition {
        Transition {
            delta_vector: self.delta_vector,
            volatility: self.volatility,
            directional_consistency: self.directional_consistency,
        }
    }

    /// Check if an observed transition is within expected bounds.
    ///
    /// Returns true if the observation is within `sigma` standard deviations.
    pub fn is_within_bounds(&self, observed: &Transition, sigma: f64) -> bool {
        // Check volatility
        let vol_diff = (observed.volatility - self.volatility).abs();
        if vol_diff > sigma * self.volatility_std && self.volatility_std > 1e-10 {
            return false;
        }

        // Check delta vector components
        for i in 0..OBSERVATION_DIM {
            let diff = (observed.delta_vector[i] - self.delta_vector[i]).abs();
            if diff > sigma * self.delta_std[i] && self.delta_std[i] > 1e-10 {
                return false;
            }
        }

        true
    }
}

/// Welford online algorithm state for vector variance.
#[derive(Debug, Clone)]
struct WelfordState {
    count: usize,
    mean: [f64; OBSERVATION_DIM],
    m2: [f64; OBSERVATION_DIM],
}

impl WelfordState {
    fn new() -> Self {
        Self {
            count: 0,
            mean: [0.0; OBSERVATION_DIM],
            m2: [0.0; OBSERVATION_DIM],
        }
    }

    fn update(&mut self, value: &[f64; OBSERVATION_DIM]) {
        self.count += 1;
        let n = self.count as f64;

        for i in 0..OBSERVATION_DIM {
            let delta = value[i] - self.mean[i];
            self.mean[i] += delta / n;
            let delta2 = value[i] - self.mean[i];
            self.m2[i] += delta * delta2;
        }
    }

    fn std_dev(&self) -> [f64; OBSERVATION_DIM] {
        if self.count < 2 {
            return [0.1; OBSERVATION_DIM];
        }

        std::array::from_fn(|i| (self.m2[i] / (self.count - 1) as f64).sqrt())
    }
}

/// Welford online algorithm for scalar variance.
#[derive(Debug, Clone)]
struct WelfordOnline {
    count: usize,
    mean: f64,
    m2: f64,
}

impl WelfordOnline {
    fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            m2: 0.0,
        }
    }

    fn update(&mut self, value: f64) {
        self.count += 1;
        let n = self.count as f64;

        let delta = value - self.mean;
        self.mean += delta / n;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
    }

    fn std_dev(&self) -> f64 {
        if self.count < 2 {
            return 0.1;
        }

        (self.m2 / (self.count - 1) as f64).sqrt()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_transition(delta: [f64; OBSERVATION_DIM], vol: f64) -> Transition {
        Transition {
            delta_vector: delta,
            volatility: vol,
            directional_consistency: 0.8,
        }
    }

    #[test]
    fn test_model_cold_start() {
        let model = ExpectedTransitionModel::new();

        assert!(!model.is_primed());
        assert_eq!(model.transition_count(), 0);
        assert!(model.stability() < 0.01);
    }

    #[test]
    fn test_model_priming() {
        let mut model = ExpectedTransitionModel::new();

        // Feed MIN_TRANSITIONS_FOR_PRIME transitions
        for i in 0..MIN_TRANSITIONS_FOR_PRIME {
            let trans = make_transition([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15);
            model.update(&trans);
        }

        assert!(model.is_primed());
        assert_eq!(model.transition_count(), MIN_TRANSITIONS_FOR_PRIME);
    }

    #[test]
    fn test_model_learns_pattern() {
        let mut model = ExpectedTransitionModel::new();

        // Feed consistent transitions
        for _ in 0..10 {
            let trans = make_transition([0.2, 0.15, 0.05, 0.0, 0.0, 0.0, 0.0], 0.25);
            model.update(&trans);
        }

        let expected = model.get_expected();

        // Should learn the pattern
        assert!((expected.delta_vector[0] - 0.2).abs() < 0.1);
        assert!((expected.volatility - 0.25).abs() < 0.1);
        assert!(expected.confidence > 0.5);
    }

    #[test]
    fn test_model_resists_outliers() {
        let mut model = ExpectedTransitionModel::new();

        // Establish baseline
        for _ in 0..5 {
            let trans = make_transition([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15);
            model.update(&trans);
        }

        let before_outlier = model.get_expected();

        // Inject outlier
        let outlier = make_transition([1.0, -0.9, 0.5, 0.0, 0.0, 0.0, 0.0], 1.5);
        model.update(&outlier);

        let after_outlier = model.get_expected();

        // Model should resist the outlier
        let delta_delta = (after_outlier.delta_vector[0] - before_outlier.delta_vector[0]).abs();
        assert!(delta_delta < 0.3); // Should not jump to 1.0
    }

    #[test]
    fn test_model_stability_increases() {
        let mut model = ExpectedTransitionModel::new();

        // Feed very consistent transitions
        for _ in 0..15 {
            let trans = make_transition([0.1, 0.1, 0.05, 0.0, 0.0, 0.0, 0.0], 0.15);
            model.update(&trans);
        }

        // Stability should be high
        assert!(model.stability() > 0.3);
    }

    #[test]
    fn test_expected_transition_bounds() {
        let expected = ExpectedTransition {
            delta_vector: [0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0],
            volatility: 0.15,
            directional_consistency: 0.8,
            delta_std: [0.05; OBSERVATION_DIM],
            volatility_std: 0.05,
            confidence: 0.7,
        };

        // Within bounds
        let within = make_transition([0.12, 0.08, 0.02, 0.0, 0.0, 0.0, 0.0], 0.14);
        assert!(expected.is_within_bounds(&within, 2.0));

        // Outside bounds
        let outside = make_transition([0.5, -0.5, 0.3, 0.0, 0.0, 0.0, 0.0], 0.8);
        assert!(!expected.is_within_bounds(&outside, 2.0));
    }

    #[test]
    fn test_model_reset() {
        let mut model = ExpectedTransitionModel::new();

        // Build up state
        for _ in 0..10 {
            let trans = make_transition([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15);
            model.update(&trans);
        }

        assert!(model.is_primed());

        // Reset
        model.reset();

        assert!(!model.is_primed());
        assert_eq!(model.transition_count(), 0);
    }

    #[test]
    fn test_predict_next() {
        let mut model = ExpectedTransitionModel::new();

        // Feed trending transitions
        for i in 0..8 {
            let delta = 0.1 + 0.02 * i as f64;
            let trans = make_transition([delta, delta, 0.0, 0.0, 0.0, 0.0, 0.0], 0.2);
            model.update(&trans);
        }

        let prediction = model.predict_next();

        // Prediction should extrapolate the trend slightly
        assert!(prediction.delta_vector[0] > 0.1);
        assert!(prediction.confidence > 0.0);
        assert!(prediction.confidence < 1.0); // Reduced confidence for prediction
    }

    #[test]
    fn test_welford_online() {
        let mut welford = WelfordOnline::new();

        let values = [1.0, 2.0, 3.0, 4.0, 5.0];
        for v in values {
            welford.update(v);
        }

        // Mean should be 3.0
        assert!((welford.mean - 3.0).abs() < 1e-10);

        // Std dev should be sqrt(2.5) ≈ 1.58
        let expected_std = (2.5_f64).sqrt();
        assert!((welford.std_dev() - expected_std).abs() < 0.1);
    }
}
