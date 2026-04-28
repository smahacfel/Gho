//! TCF Cohesion Module - Core Cohesion Function
//!
//! This is the **most important** module in TCF. The cohesion function measures
//! how well an observed transition matches the expected transition pattern.
//!
//! ## Design Philosophy
//!
//! Cohesion is NOT about whether the market went up or down.
//! Cohesion is about whether the WAY the market changed was consistent
//! with the established pattern of change.
//!
//! High cohesion = "The market is doing what it was doing"
//! Low cohesion = "Something changed in how the market behaves"
//!
//! ## Scoring Components
//!
//! | Component | Weight | Description |
//! |-----------|--------|-------------|
//! | Direction | 40% | Are changes in the same direction? |
//! | Rhythm | 30% | Is the magnitude of change similar? |
//! | Stability | 30% | Is the internal consistency maintained? |
//!
//! ## Penalties (reduce cohesion)
//!
//! - Direction contradiction: when expected and observed move opposite ways
//! - Volatility spike: sudden increase in magnitude
//! - Consistency drop: price-volume relationship breaks down
//! - Entropy shock: liquidity distribution suddenly changes
//!
//! ## Rewards (increase cohesion)
//!
//! - Direction alignment: expected and observed move same way
//! - Rhythm maintenance: similar volatility levels
//! - Pattern continuation: internal relationships preserved
//!
//! ## Mathematical Properties
//!
//! - Output: [0, 1] where 1 = perfect cohesion, 0 = complete break
//! - Continuous: small changes produce small cohesion changes
//! - Symmetric in some sense: similar deviations get similar penalties
//!
//! ## Performance
//!
//! - O(1) computation
//! - Zero heap allocation
//! - Pure function (no side effects)

use super::expected::ExpectedTransition;
use super::observation::OBSERVATION_DIM;
use super::transition::Transition;
use tracing::debug;

/// Configuration for cohesion calculation.
///
/// Allows tuning the sensitivity to different types of deviations.
#[derive(Debug, Clone, Copy)]
pub struct CohesionConfig {
    /// Weight for directional alignment component [0, 1].
    pub direction_weight: f64,

    /// Weight for rhythm (magnitude) component [0, 1].
    pub rhythm_weight: f64,

    /// Weight for stability (consistency) component [0, 1].
    pub stability_weight: f64,

    /// Sensitivity to volatility spikes (higher = more penalty).
    pub volatility_sensitivity: f64,

    /// Sensitivity to direction contradiction.
    pub direction_sensitivity: f64,

    /// Penalty multiplier for price-volume divergence.
    pub price_volume_divergence_penalty: f64,

    /// Bonus for perfect alignment (up to this multiplier).
    pub alignment_bonus_max: f64,
}

impl Default for CohesionConfig {
    fn default() -> Self {
        Self {
            direction_weight: 0.40,
            rhythm_weight: 0.30,
            stability_weight: 0.30,
            volatility_sensitivity: 2.0,
            direction_sensitivity: 1.5,
            price_volume_divergence_penalty: 0.15,
            alignment_bonus_max: 0.10,
        }
    }
}

impl CohesionConfig {
    /// Create a config optimized for pump detection.
    ///
    /// More sensitive to sudden volatility spikes.
    pub fn pump_sensitive() -> Self {
        Self {
            direction_weight: 0.35,
            rhythm_weight: 0.40,
            stability_weight: 0.25,
            volatility_sensitivity: 2.5,
            direction_sensitivity: 1.8,
            price_volume_divergence_penalty: 0.20,
            alignment_bonus_max: 0.05,
        }
    }

    /// Create a config optimized for organic growth detection.
    ///
    /// More tolerant of gradual changes.
    pub fn organic_tolerant() -> Self {
        Self {
            direction_weight: 0.45,
            rhythm_weight: 0.25,
            stability_weight: 0.30,
            volatility_sensitivity: 1.5,
            direction_sensitivity: 1.2,
            price_volume_divergence_penalty: 0.10,
            alignment_bonus_max: 0.15,
        }
    }
}

/// Result of cohesion calculation with breakdown.
#[derive(Debug, Clone, Copy)]
pub struct CohesionResult {
    /// Overall cohesion score [0, 1].
    pub cohesion: f64,

    /// Direction alignment component [0, 1].
    pub direction_score: f64,

    /// Rhythm (magnitude) component [0, 1].
    pub rhythm_score: f64,

    /// Stability (consistency) component [0, 1].
    pub stability_score: f64,

    /// Applied penalties (sum of all penalties).
    pub total_penalty: f64,

    /// Applied bonuses (sum of all bonuses).
    pub total_bonus: f64,

    /// Breakdown of what caused the score.
    pub breakdown: CohesionBreakdown,
}

/// Detailed breakdown of cohesion calculation.
#[derive(Debug, Clone, Copy, Default)]
pub struct CohesionBreakdown {
    /// Was there a direction contradiction?
    pub direction_contradiction: bool,

    /// Magnitude of volatility deviation.
    pub volatility_deviation: f64,

    /// Price-volume divergence detected?
    pub price_volume_divergent: bool,

    /// Consistency drop amount.
    pub consistency_drop: f64,

    /// Perfect alignment bonus applied?
    pub alignment_bonus_applied: bool,
}

/// Calculate cohesion between expected and observed transitions.
///
/// This is the core function of TCF. It measures how well the observed
/// transition matches what was expected based on the established pattern.
///
/// # Arguments
///
/// * `expected` - The transition we expected to see
/// * `observed` - The transition that actually occurred
/// * `config` - Configuration for sensitivity tuning
///
/// # Returns
///
/// `CohesionResult` with score in [0, 1] and detailed breakdown.
///
/// # Mathematical Properties
///
/// 1. cohesion(E, E) = 1.0 (identical transitions have perfect cohesion)
/// 2. cohesion is continuous (small changes → small score changes)
/// 3. cohesion penalizes contradiction more than noise
///
/// # Performance
///
/// O(1) computation, zero allocation.
pub fn cohesion(
    expected: &ExpectedTransition,
    observed: &Transition,
    config: &CohesionConfig,
) -> CohesionResult {
    let mut breakdown = CohesionBreakdown::default();

    // === Component 1: Direction Alignment ===
    // Measures whether expected and observed changes point the same way
    let direction_score = calculate_direction_score(expected, observed, &mut breakdown);

    // === Component 2: Rhythm (Magnitude) ===
    // Measures whether the magnitude of change is similar
    let rhythm_score = calculate_rhythm_score(expected, observed, config, &mut breakdown);

    // === Component 3: Stability (Consistency) ===
    // Measures whether internal relationships are maintained
    let stability_score = calculate_stability_score(expected, observed, &mut breakdown);

    // === Combine Base Scores ===
    let base_score = config.direction_weight * direction_score
        + config.rhythm_weight * rhythm_score
        + config.stability_weight * stability_score;

    // === Calculate Penalties ===
    let mut total_penalty = 0.0;

    // Penalty for direction contradiction
    if breakdown.direction_contradiction {
        total_penalty += config.direction_sensitivity * 0.15;
    }

    // Penalty for volatility spike
    if breakdown.volatility_deviation > 1.0 {
        let vol_penalty =
            (breakdown.volatility_deviation - 1.0) * 0.1 * config.volatility_sensitivity;
        total_penalty += vol_penalty.min(0.3);
    }

    // Penalty for price-volume divergence
    if breakdown.price_volume_divergent {
        total_penalty += config.price_volume_divergence_penalty;
    }

    // === Calculate Bonuses ===
    let mut total_bonus = 0.0;

    // Bonus for perfect alignment
    if direction_score > 0.9 && rhythm_score > 0.8 && stability_score > 0.8 {
        let alignment_bonus =
            config.alignment_bonus_max * ((direction_score - 0.9) * 10.0).clamp(0.0, 1.0);
        total_bonus += alignment_bonus;
        breakdown.alignment_bonus_applied = true;
    }

    // === Final Score ===
    let raw_cohesion = base_score - total_penalty + total_bonus;
    let cohesion = raw_cohesion.clamp(0.0, 1.0);

    CohesionResult {
        cohesion,
        direction_score,
        rhythm_score,
        stability_score,
        total_penalty,
        total_bonus,
        breakdown,
    }
}

/// Calculate direction alignment score.
fn calculate_direction_score(
    expected: &ExpectedTransition,
    observed: &Transition,
    breakdown: &mut CohesionBreakdown,
) -> f64 {
    // Cosine similarity of standardized delta vectors.
    // Per-dimension scaling prevents heterogeneous units from biasing direction score.
    let dot: f64 = expected
        .delta_vector
        .iter()
        .zip(observed.delta_vector.iter())
        .zip(expected.delta_std.iter())
        .map(|((a, b), std)| {
            let s = std.max(1e-6);
            (a / s) * (b / s)
        })
        .sum();

    let mag_exp: f64 = expected
        .delta_vector
        .iter()
        .zip(expected.delta_std.iter())
        .map(|(x, std)| {
            let s = std.max(1e-6);
            let v = x / s;
            v * v
        })
        .sum::<f64>()
        .sqrt();
    let mag_obs: f64 = observed
        .delta_vector
        .iter()
        .zip(expected.delta_std.iter())
        .map(|(x, std)| {
            let s = std.max(1e-6);
            let v = x / s;
            v * v
        })
        .sum::<f64>()
        .sqrt();

    if mag_exp < 1e-10 && mag_obs < 1e-10 {
        return 1.0; // Both near zero -> highly aligned quiet regime
    }
    if mag_exp < 1e-10 || mag_obs < 1e-10 {
        return 0.35; // One near zero and one active -> weak alignment
    }

    let cos_sim = (dot / (mag_exp * mag_obs)).clamp(-1.0, 1.0);
    debug!(
        target: "oracle::tcf::cohesion",
        "direction cosine normalized={:.6}",
        cos_sim
    );

    // Check for contradiction
    if cos_sim < -0.3 {
        breakdown.direction_contradiction = true;
    }

    // Map cosine similarity [-1, 1] to score [0, 1]
    // cos_sim = 1 → score = 1
    // cos_sim = 0 → score = 0.5
    // cos_sim = -1 → score = 0
    (cos_sim + 1.0) / 2.0
}

/// Calculate rhythm (magnitude) score.
fn calculate_rhythm_score(
    expected: &ExpectedTransition,
    observed: &Transition,
    config: &CohesionConfig,
    breakdown: &mut CohesionBreakdown,
) -> f64 {
    let exp_vol = expected.volatility.max(0.01); // Avoid division by zero
    let obs_vol = observed.volatility;

    // Ratio-based comparison
    let ratio = obs_vol / exp_vol;
    breakdown.volatility_deviation = (ratio - 1.0).abs();

    // Use uncertainty-aware, asymmetric deviation:
    // - Volatility spikes should be penalized strongly.
    // - Volatility contractions are softer (quiet market is often cohesive).
    let vol_scale = expected.volatility_std.max(exp_vol * 0.15).max(0.02);
    let delta = obs_vol - exp_vol;
    let norm_dev = if delta >= 0.0 {
        delta / vol_scale
    } else {
        // Softer penalty for contractions to avoid deterministic braking in calm phases.
        (-delta) / (vol_scale * 2.0)
    };
    let rhythm_score = (-norm_dev * config.volatility_sensitivity).exp();
    debug!(
        target: "oracle::tcf::cohesion",
        "rhythm exp_vol={:.6} obs_vol={:.6} ratio={:.6} vol_scale={:.6} norm_dev={:.6} rhythm_score={:.6}",
        exp_vol,
        obs_vol,
        ratio,
        vol_scale,
        norm_dev,
        rhythm_score
    );

    rhythm_score.clamp(0.0, 1.0)
}

/// Calculate stability (consistency) score.
fn calculate_stability_score(
    expected: &ExpectedTransition,
    observed: &Transition,
    breakdown: &mut CohesionBreakdown,
) -> f64 {
    // Compare directional consistency
    let cons_diff = (expected.directional_consistency - observed.directional_consistency).abs();
    breakdown.consistency_drop = cons_diff;

    // Score based on consistency difference
    let consistency_score = 1.0 - cons_diff;

    // Check for price-volume divergence
    // In expected: price and volume should move together for organic trend
    // Divergence = one up, one down
    let exp_price_dir = expected.delta_vector[0].signum();
    let obs_price_dir = observed.delta_vector[0].signum();
    let obs_volume_dir = observed.delta_vector[1].signum();

    // If expected shows alignment but observed shows divergence
    if exp_price_dir * expected.delta_vector[1].signum() > 0.0
        && obs_price_dir * obs_volume_dir < 0.0
        && observed.delta_vector[0].abs() > 0.05
        && observed.delta_vector[1].abs() > 0.05
    {
        breakdown.price_volume_divergent = true;
    }

    // Also penalize if consistency dropped significantly
    let stability_score = consistency_score * (1.0 - cons_diff.min(0.3));

    stability_score.clamp(0.0, 1.0)
}

/// Simplified cohesion function for quick calculations.
///
/// Uses default config, returns just the score.
pub fn cohesion_simple(expected: &ExpectedTransition, observed: &Transition) -> f64 {
    cohesion(expected, observed, &CohesionConfig::default()).cohesion
}

/// Batch cohesion calculation for multiple transitions.
///
/// Computes cohesion for a sequence of observed transitions against
/// a single expected pattern. Useful for analyzing trend consistency.
///
/// # Arguments
///
/// * `expected` - The expected transition pattern
/// * `observations` - Slice of observed transitions
/// * `config` - Configuration for cohesion calculation
///
/// # Returns
///
/// Vector of cohesion scores, same length as observations.
pub fn batch_cohesion(
    expected: &ExpectedTransition,
    observations: &[Transition],
    config: &CohesionConfig,
) -> Vec<f64> {
    observations
        .iter()
        .map(|obs| cohesion(expected, obs, config).cohesion)
        .collect()
}

/// Calculate cumulative cohesion with decay.
///
/// More recent cohesions are weighted higher than older ones.
/// Uses exponential decay with configurable half-life.
///
/// # Arguments
///
/// * `cohesions` - Slice of cohesion values (oldest first)
/// * `decay_factor` - Weight multiplier per step (0.8 = 20% decay per step)
///
/// # Returns
///
/// Weighted average cohesion where recent values dominate.
pub fn cumulative_cohesion(cohesions: &[f64], decay_factor: f64) -> f64 {
    if cohesions.is_empty() {
        return 0.0;
    }

    let mut weighted_sum = 0.0;
    let mut weight_sum = 0.0;
    let mut weight = 1.0;

    // Iterate from newest to oldest
    for c in cohesions.iter().rev() {
        weighted_sum += c * weight;
        weight_sum += weight;
        weight *= decay_factor;
    }

    if weight_sum < 1e-10 {
        return 0.0;
    }

    weighted_sum / weight_sum
}

/// Detect cohesion "cliff" - sudden drop in cohesion.
///
/// Returns true if there's a sharp transition from high to low cohesion,
/// indicating the market dynamics have fundamentally changed.
///
/// # Arguments
///
/// * `cohesions` - Slice of recent cohesion values
/// * `cliff_threshold` - Minimum drop to consider as cliff (e.g., 0.3)
///
/// # Returns
///
/// True if a cohesion cliff is detected.
pub fn detect_cohesion_cliff(cohesions: &[f64], cliff_threshold: f64) -> bool {
    if cohesions.len() < 3 {
        return false;
    }

    // Look at recent values
    let len = cohesions.len();
    let recent = &cohesions[len.saturating_sub(5)..];

    if recent.len() < 2 {
        return false;
    }

    // Check for sudden drop
    for i in 1..recent.len() {
        let drop = recent[i - 1] - recent[i];
        if drop > cliff_threshold && recent[i - 1] > 0.6 {
            return true;
        }
    }

    false
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_expected(delta: [f64; OBSERVATION_DIM], vol: f64, cons: f64) -> ExpectedTransition {
        ExpectedTransition {
            delta_vector: delta,
            volatility: vol,
            directional_consistency: cons,
            delta_std: [0.1; OBSERVATION_DIM],
            volatility_std: 0.1,
            confidence: 0.8,
        }
    }

    fn make_transition(delta: [f64; OBSERVATION_DIM], vol: f64, cons: f64) -> Transition {
        Transition {
            delta_vector: delta,
            volatility: vol,
            directional_consistency: cons,
        }
    }

    #[test]
    fn test_cohesion_identical() {
        let expected = make_expected([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8);
        let observed = make_transition([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8);

        let result = cohesion(&expected, &observed, &CohesionConfig::default());

        // Should be very high cohesion
        assert!(result.cohesion > 0.9);
    }

    #[test]
    fn test_cohesion_opposite_direction() {
        let expected = make_expected([0.3, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0], 0.36, 0.8);
        let observed = make_transition([-0.3, -0.2, 0.0, 0.0, 0.0, 0.0, 0.0], 0.36, 0.8);

        let result = cohesion(&expected, &observed, &CohesionConfig::default());

        // Should have low cohesion due to direction contradiction
        assert!(result.cohesion < 0.5);
        assert!(result.breakdown.direction_contradiction);
    }

    #[test]
    fn test_cohesion_volatility_spike() {
        let expected = make_expected([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8);
        let observed = make_transition([0.3, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0], 0.45, 0.8);

        let result = cohesion(&expected, &observed, &CohesionConfig::default());

        // Same direction but different magnitude
        assert!(result.cohesion < 0.8);
        assert!(result.breakdown.volatility_deviation > 0.5);
    }

    #[test]
    fn test_cohesion_consistency_drop() {
        let expected = make_expected([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.9);
        let observed = make_transition([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.3);

        let result = cohesion(&expected, &observed, &CohesionConfig::default());

        // Stability should be penalized
        assert!(result.stability_score < 0.8);
        assert!(result.breakdown.consistency_drop > 0.5);
    }

    #[test]
    fn test_cohesion_price_volume_divergence() {
        let expected = make_expected([0.2, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0], 0.28, 0.9);
        let observed = make_transition([0.2, -0.2, 0.0, 0.0, 0.0, 0.0, 0.0], 0.28, 0.4);

        let result = cohesion(&expected, &observed, &CohesionConfig::default());

        // Should detect divergence
        assert!(result.breakdown.price_volume_divergent);
        assert!(result.total_penalty > 0.0);
    }

    #[test]
    fn test_cohesion_alignment_bonus() {
        let expected = make_expected([0.2, 0.2, 0.1, 0.0, 0.0, 0.0, 0.0], 0.3, 0.9);
        let observed = make_transition([0.2, 0.2, 0.1, 0.0, 0.0, 0.0, 0.0], 0.3, 0.9);

        let result = cohesion(&expected, &observed, &CohesionConfig::default());

        // Perfect alignment should get bonus
        assert!(result.breakdown.alignment_bonus_applied || result.cohesion > 0.95);
    }

    #[test]
    fn test_cohesion_simple() {
        let expected = make_expected([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8);
        let observed = make_transition([0.12, 0.09, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.75);

        let score = cohesion_simple(&expected, &observed);

        assert!(score > 0.5 && score <= 1.0);
    }

    #[test]
    fn test_batch_cohesion() {
        let expected = make_expected([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8);
        let observations = vec![
            make_transition([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8),
            make_transition([0.12, 0.08, 0.0, 0.0, 0.0, 0.0, 0.0], 0.14, 0.75),
            make_transition([-0.1, -0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8),
        ];

        let scores = batch_cohesion(&expected, &observations, &CohesionConfig::default());

        assert_eq!(scores.len(), 3);
        assert!(scores[0] > scores[2]); // First should be higher than reversed
    }

    #[test]
    fn test_cumulative_cohesion() {
        let cohesions = vec![0.9, 0.85, 0.8, 0.75, 0.7]; // Declining

        let cumulative = cumulative_cohesion(&cohesions, 0.8);

        // With decay favoring recent, should be closer to 0.7 than 0.9
        assert!(cumulative < 0.8);
        assert!(cumulative > 0.7);
    }

    #[test]
    fn test_detect_cohesion_cliff() {
        // Normal decline - no cliff
        let normal = vec![0.9, 0.85, 0.8, 0.75, 0.7];
        assert!(!detect_cohesion_cliff(&normal, 0.25));

        // Sharp drop - cliff detected
        let cliff = vec![0.9, 0.85, 0.8, 0.45, 0.4];
        assert!(detect_cohesion_cliff(&cliff, 0.25));
    }

    #[test]
    fn test_config_presets() {
        let expected = make_expected([0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0], 0.15, 0.8);
        let spike = make_transition([0.3, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0], 0.45, 0.8);

        let default_result = cohesion(&expected, &spike, &CohesionConfig::default());
        let pump_result = cohesion(&expected, &spike, &CohesionConfig::pump_sensitive());
        let organic_result = cohesion(&expected, &spike, &CohesionConfig::organic_tolerant());

        // Pump sensitive should penalize more
        assert!(pump_result.cohesion <= default_result.cohesion);
        // Organic tolerant should penalize less
        assert!(organic_result.cohesion >= default_result.cohesion);
    }

    #[test]
    fn test_cohesion_bounds() {
        // Test extreme cases
        let expected = make_expected([1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], 2.6, 1.0);
        let opposite = make_transition([-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0], 0.1, 0.0);

        let result = cohesion(&expected, &opposite, &CohesionConfig::default());

        // Should be bounded to [0, 1]
        assert!(result.cohesion >= 0.0);
        assert!(result.cohesion <= 1.0);
    }
}
