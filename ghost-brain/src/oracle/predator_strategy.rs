//! Ghost Predator Strategy Definitions
//!
//! Implements the mathematical specifications for Issue #51: GHOST PREDATOR.
//! Includes Cycle Weights, Gunshot Thresholds, and Phase-Aware Quality Formulas.
//!
//! # Scoring Phases
//!
//! **Early Stage Mode (S1-S6)**:
//! - Uses static analysis only (no trend-based metrics)
//! - Quality Formula: `0.44 * mpcf + 0.31 * mesa + 0.25 * wallet_ratio`
//! - SCR, ULVF, POVC are skipped (require ~20+ samples)
//!
//! **Full Analysis Mode (S7-S12)**:
//! - All metrics active including trend-based analysis
//! - Quality Formula: `0.35 * mpcf + 0.25 * mesa + 0.20 * (1-scr_bot) + 0.20 * wallet_ratio`
//! - SCR, ULVF, POVC provide additional signals

/// Gatekeeper duration in milliseconds (hard timer)
pub const GATEKEEPER_DURATION_MS: u64 = 1780;

/// Minimum transaction count for scoring (Gatekeeper threshold)
pub const MIN_TX_FOR_SCORING: usize = 15;

/// Early Stage threshold: cycles S1-S6 use static analysis only
pub const EARLY_STAGE_CYCLE_THRESHOLD: usize = 6;

/// Full Analysis threshold: cycles S7+ use full metrics including SCR/ULVF/POVC
pub const FULL_ANALYSIS_TX_THRESHOLD: usize = 23;

/// Exponential weights for the 12 scoring cycles.
/// Last cycles have dominant impact on the final decision.
///
/// Weight distribution follows geometric progression with multiplier ~1.3:
/// - S1-S6 (Early Stage): Lower weights (1.3 to 4.6)
/// - S7-S12 (Full Analysis): Higher weights (6.0 to 22.0)
pub const CYCLE_WEIGHTS: [f32; 12] = [
    1.3,  // S1
    1.7,  // S2
    2.2,  // S3
    2.8,  // S4
    3.6,  // S5
    4.6,  // S6 (end of Early Stage)
    6.0,  // S7 (start of Full Analysis)
    7.8,  // S8
    10.0, // S9
    13.0, // S10
    17.0, // S11
    22.0, // S12 (Dominant weight)
];

/// Sum of all cycle weights for normalization
/// Calculated: 1.3 + 1.7 + 2.2 + 2.8 + 3.6 + 4.6 + 6.0 + 7.8 + 10.0 + 13.0 + 17.0 + 22.0 = 92.0
pub const CYCLE_WEIGHTS_SUM: f32 = 92.0;

/// Gunshot Thresholds for immediate entry (Early Exit).
/// If the raw score in a cycle meets or exceeds this threshold, buy immediately.
///
/// Early Stage (S1-S6): Higher thresholds due to limited data confidence
/// Full Analysis (S7-S12): Lower thresholds as confidence increases
pub const GUNSHOT_THRESHOLDS: [f32; 12] = [
    100.0, // S1: Perfection required
    99.0,  // S2
    98.0,  // S3
    97.0,  // S4
    96.0,  // S5
    95.0,  // S6 (end of Early Stage)
    88.0,  // S7: Transition to Full Analysis
    87.0,  // S8
    86.0,  // S9
    85.0,  // S10: Strong trend confirmation
    83.5,  // S11
    82.0,  // S12: Final Stand
];

/// Quality formula weights for Early Stage Mode (S1-S6)
/// SCR is excluded because FFT requires ~20+ samples for statistical significance
pub mod early_stage_weights {
    /// MPCF weight in Early Stage Quality formula
    pub const MPCF: f32 = 0.44;
    /// MESA weight in Early Stage Quality formula  
    pub const MESA: f32 = 0.31;
    /// Wallet ratio weight in Early Stage Quality formula
    pub const WALLET: f32 = 0.25;
}

/// Quality formula weights for Full Analysis Mode (S7-S12)
/// All metrics active including SCR for bot detection
pub mod full_analysis_weights {
    /// MPCF weight in Full Analysis Quality formula
    pub const MPCF: f32 = 0.35;
    /// MESA weight in Full Analysis Quality formula
    pub const MESA: f32 = 0.25;
    /// SCR (inverted) weight in Full Analysis Quality formula
    pub const SCR: f32 = 0.20;
    /// Wallet ratio weight in Full Analysis Quality formula
    pub const WALLET: f32 = 0.20;
}

/// Get the weight for a specific cycle (0-indexed 0..11).
/// Returns 1.0 if out of bounds (fallback).
pub fn get_cycle_weight(cycle_idx: usize) -> f32 {
    if cycle_idx < CYCLE_WEIGHTS.len() {
        CYCLE_WEIGHTS[cycle_idx]
    } else {
        1.0
    }
}

/// Get the gunshot threshold for a specific cycle (0-indexed 0..11).
/// Returns 100.0 (max difficulty) if out of bounds (safety).
pub fn get_gunshot_threshold(cycle_idx: usize) -> f32 {
    if cycle_idx < GUNSHOT_THRESHOLDS.len() {
        GUNSHOT_THRESHOLDS[cycle_idx]
    } else {
        100.0
    }
}

/// Calculate the weighted geometric mean of scores collected so far.
///
/// Geometric mean is more sensitive to outliers (low scores punish more heavily).
/// Formula: exp( sum(w_i * ln(x_i)) / sum(w_i) )
pub fn calculate_weighted_geometric_mean(scores: &[f32]) -> f32 {
    if scores.is_empty() {
        return 0.0;
    }

    let mut weighted_log_sum = 0.0;
    let mut weight_sum = 0.0;

    for (i, &score) in scores.iter().enumerate() {
        let weight = get_cycle_weight(i);
        // Avoid ln(0) or negative numbers. Min score floor 0.1.
        let safe_score = score.max(0.1);

        weighted_log_sum += weight * safe_score.ln();
        weight_sum += weight;
    }

    if weight_sum == 0.0 {
        return 0.0;
    }

    (weighted_log_sum / weight_sum).exp()
}

/// Calculate the weighted average of scores collected so far.
/// Kept for backward compatibility or alternative strategies.
pub fn calculate_weighted_average(scores: &[f32]) -> f32 {
    if scores.is_empty() {
        return 0.0;
    }

    let mut weighted_sum = 0.0;
    let mut weight_sum = 0.0;

    for (i, &score) in scores.iter().enumerate() {
        let weight = get_cycle_weight(i);
        weighted_sum += score * weight;
        weight_sum += weight;
    }

    if weight_sum == 0.0 {
        return 0.0;
    }

    weighted_sum / weight_sum
}

/// Determine if a cycle is in Early Stage mode
///
/// # Arguments
/// * `cycle_idx` - Zero-indexed cycle number (0-11)
///
/// # Returns
/// `true` if cycle is in Early Stage (S1-S6), `false` if Full Analysis (S7-S12)
#[inline]
pub fn is_early_stage_cycle(cycle_idx: usize) -> bool {
    cycle_idx < EARLY_STAGE_CYCLE_THRESHOLD
}

/// Calculate Quality score using Early Stage formula (no SCR)
///
/// Formula: `0.44 * mpcf + 0.31 * mesa + 0.25 * wallet_ratio`
///
/// Used in cycles S1-S6 where SCR FFT doesn't have enough samples.
///
/// # Arguments
/// * `mpcf_organic` - MPCF organic ratio (0.0-1.0)
/// * `mesa_organic` - MESA organic likeness (0.0-1.0)
/// * `wallet_ratio` - Unique wallet ratio (0.0-1.0)
///
/// # Returns
/// Quality score in range [0.0, 1.0]
pub fn calculate_quality_early_stage(
    mpcf_organic: f32,
    mesa_organic: f32,
    wallet_ratio: f32,
) -> f32 {
    let quality = early_stage_weights::MPCF * mpcf_organic
        + early_stage_weights::MESA * mesa_organic
        + early_stage_weights::WALLET * wallet_ratio;
    quality.clamp(0.0, 1.0)
}

/// Calculate Quality score using Full Analysis formula (with SCR)
///
/// Formula: `0.35 * mpcf + 0.25 * mesa + 0.20 * (1-scr_bot) + 0.20 * wallet_ratio`
///
/// Used in cycles S7-S12 when SCR FFT has sufficient samples.
///
/// # Arguments
/// * `mpcf_organic` - MPCF organic ratio (0.0-1.0)
/// * `mesa_organic` - MESA organic likeness (0.0-1.0)
/// * `scr_bot_score` - SCR bot detection score (0.0-1.0, will be inverted)
/// * `wallet_ratio` - Unique wallet ratio (0.0-1.0)
///
/// # Returns
/// Quality score in range [0.0, 1.0]
pub fn calculate_quality_full_analysis(
    mpcf_organic: f32,
    mesa_organic: f32,
    scr_bot_score: f32,
    wallet_ratio: f32,
) -> f32 {
    let scr_organic = 1.0 - scr_bot_score; // Invert: low bot score = high organic
    let quality = full_analysis_weights::MPCF * mpcf_organic
        + full_analysis_weights::MESA * mesa_organic
        + full_analysis_weights::SCR * scr_organic
        + full_analysis_weights::WALLET * wallet_ratio;
    quality.clamp(0.0, 1.0)
}

/// Calculate Quality score based on current cycle phase
///
/// Automatically selects the appropriate formula based on cycle index:
/// - S1-S6: Early Stage (no SCR)
/// - S7-S12: Full Analysis (with SCR)
///
/// # Arguments
/// * `cycle_idx` - Zero-indexed cycle number (0-11)
/// * `mpcf_organic` - MPCF organic ratio (0.0-1.0)
/// * `mesa_organic` - MESA organic likeness (0.0-1.0)
/// * `scr_bot_score` - SCR bot detection score (0.0-1.0), ignored in Early Stage
/// * `wallet_ratio` - Unique wallet ratio (0.0-1.0)
///
/// # Returns
/// Quality score in range [0.0, 1.0]
pub fn calculate_quality_for_cycle(
    cycle_idx: usize,
    mpcf_organic: f32,
    mesa_organic: f32,
    scr_bot_score: Option<f32>,
    wallet_ratio: f32,
) -> f32 {
    if is_early_stage_cycle(cycle_idx) {
        calculate_quality_early_stage(mpcf_organic, mesa_organic, wallet_ratio)
    } else {
        // In Full Analysis, use SCR if available, otherwise fall back to Early Stage formula
        match scr_bot_score {
            Some(scr) => {
                calculate_quality_full_analysis(mpcf_organic, mesa_organic, scr, wallet_ratio)
            }
            None => calculate_quality_early_stage(mpcf_organic, mesa_organic, wallet_ratio),
        }
    }
}

/// Scoring phase for a candidate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoringPhase {
    /// Early Stage Mode (S1-S6): Static analysis only
    EarlyStage,
    /// Full Analysis Mode (S7-S12): All metrics active
    FullAnalysis,
}

impl ScoringPhase {
    /// Get the phase for a given cycle index (0-indexed)
    pub fn from_cycle_idx(cycle_idx: usize) -> Self {
        if is_early_stage_cycle(cycle_idx) {
            ScoringPhase::EarlyStage
        } else {
            ScoringPhase::FullAnalysis
        }
    }

    /// Get display name for the phase
    pub fn display_name(&self) -> &'static str {
        match self {
            ScoringPhase::EarlyStage => "Early Stage",
            ScoringPhase::FullAnalysis => "Full Analysis",
        }
    }

    /// Check if this phase is Early Stage
    pub fn is_early_stage(&self) -> bool {
        matches!(self, ScoringPhase::EarlyStage)
    }

    /// Check if SCR should be used in this phase
    pub fn should_use_scr(&self) -> bool {
        matches!(self, ScoringPhase::FullAnalysis)
    }

    /// Check if ULVF should be used in this phase
    pub fn should_use_ulvf(&self) -> bool {
        matches!(self, ScoringPhase::FullAnalysis)
    }

    /// Check if POVC should be used in this phase
    pub fn should_use_povc(&self) -> bool {
        matches!(self, ScoringPhase::FullAnalysis)
    }
}

// ==============================================================================
// Config-Aware Functions
// ==============================================================================
// These functions accept GhostBrainConfig and use configured values instead of
// hardcoded constants. This enables hot-reload - changes to config take effect
// without recompilation.

use crate::config::GhostBrainConfig;

/// Get the weight for a specific cycle using config (0-indexed 0..11).
/// Uses configured weights if available, falls back to hardcoded defaults.
pub fn get_cycle_weight_from_config(cycle_idx: usize, config: &GhostBrainConfig) -> f32 {
    config.get_cycle_weight(cycle_idx)
}

/// Get the gunshot threshold for a specific cycle using config (0-indexed 0..11).
/// Uses configured thresholds if available, falls back to hardcoded defaults.
pub fn get_gunshot_threshold_from_config(cycle_idx: usize, config: &GhostBrainConfig) -> f32 {
    config.get_gunshot_threshold(cycle_idx)
}

/// Calculate the weighted geometric mean using config-based weights.
///
/// Geometric mean is more sensitive to outliers (low scores punish more heavily).
/// Formula: exp( sum(w_i * ln(x_i)) / sum(w_i) )
pub fn calculate_weighted_geometric_mean_with_config(
    scores: &[f32],
    config: &GhostBrainConfig,
) -> f32 {
    if scores.is_empty() {
        return 0.0;
    }

    let mut weighted_log_sum = 0.0;
    let mut weight_sum = 0.0;

    for (i, &score) in scores.iter().enumerate() {
        let weight = config.get_cycle_weight(i);
        // Avoid ln(0) or negative numbers. Min score floor 0.1.
        let safe_score = score.max(0.1);

        weighted_log_sum += weight * safe_score.ln();
        weight_sum += weight;
    }

    if weight_sum == 0.0 {
        return 0.0;
    }

    (weighted_log_sum / weight_sum).exp()
}

/// Calculate the weighted average using config-based weights.
pub fn calculate_weighted_average_with_config(scores: &[f32], config: &GhostBrainConfig) -> f32 {
    if scores.is_empty() {
        return 0.0;
    }

    let mut weighted_sum = 0.0;
    let mut weight_sum = 0.0;

    for (i, &score) in scores.iter().enumerate() {
        let weight = config.get_cycle_weight(i);
        weighted_sum += score * weight;
        weight_sum += weight;
    }

    if weight_sum == 0.0 {
        return 0.0;
    }

    weighted_sum / weight_sum
}

/// Calculate Quality score for Early Stage using config-based weights.
///
/// Uses weights from survivor_score.quality_early_stage if configured,
/// otherwise falls back to hardcoded defaults.
pub fn calculate_quality_early_stage_with_config(
    mpcf_organic: f32,
    mesa_organic: f32,
    wallet_ratio: f32,
    config: &GhostBrainConfig,
) -> f32 {
    let ss_config = config.get_survivor_score_config();
    let weights = &ss_config.quality_early_stage;

    let quality =
        weights.mpcf * mpcf_organic + weights.mesa * mesa_organic + weights.wallet * wallet_ratio;
    quality.clamp(0.0, 1.0)
}

/// Calculate Quality score for Full Analysis using config-based weights.
///
/// Uses weights from survivor_score.quality_full_analysis if configured,
/// otherwise falls back to hardcoded defaults.
pub fn calculate_quality_full_analysis_with_config(
    mpcf_organic: f32,
    mesa_organic: f32,
    scr_bot_score: f32,
    wallet_ratio: f32,
    config: &GhostBrainConfig,
) -> f32 {
    let ss_config = config.get_survivor_score_config();
    let weights = &ss_config.quality_full_analysis;

    let scr_organic = 1.0 - scr_bot_score; // Invert: low bot score = high organic
    let quality = weights.mpcf * mpcf_organic
        + weights.mesa * mesa_organic
        + weights.scr * scr_organic
        + weights.wallet * wallet_ratio;
    quality.clamp(0.0, 1.0)
}

/// Calculate Quality score based on current cycle phase using config-based weights.
///
/// Uses survivor_score config if available, otherwise falls back to defaults.
pub fn calculate_quality_for_cycle_with_config(
    cycle_idx: usize,
    mpcf_organic: f32,
    mesa_organic: f32,
    scr_bot_score: Option<f32>,
    wallet_ratio: f32,
    config: &GhostBrainConfig,
) -> f32 {
    if is_early_stage_cycle(cycle_idx) {
        calculate_quality_early_stage_with_config(mpcf_organic, mesa_organic, wallet_ratio, config)
    } else {
        // In Full Analysis, use SCR if available, otherwise fall back to Early Stage formula
        match scr_bot_score {
            Some(scr) => calculate_quality_full_analysis_with_config(
                mpcf_organic,
                mesa_organic,
                scr,
                wallet_ratio,
                config,
            ),
            None => calculate_quality_early_stage_with_config(
                mpcf_organic,
                mesa_organic,
                wallet_ratio,
                config,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weights_length() {
        assert_eq!(CYCLE_WEIGHTS.len(), 12);
    }

    #[test]
    fn test_thresholds_length() {
        assert_eq!(GUNSHOT_THRESHOLDS.len(), 12);
    }

    #[test]
    fn test_weighted_geometric_mean_perfect() {
        let scores = vec![100.0; 12];
        let mean = calculate_weighted_geometric_mean(&scores);
        assert!((mean - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_weighted_geometric_mean_punishment() {
        // One low score should drag down geometric mean more than arithmetic
        let mut scores = vec![100.0; 12];
        scores[11] = 50.0; // Last cycle (weight 22.0) is 50.0

        // Arithmetic mean would be:
        // (100*sum(w0..w10) + 50*w11) / sum(w)
        // Total weight approx 92. w11=22.
        // (100*70 + 50*22) / 92 = (7000 + 1100) / 92 = 88

        // Geometric mean:
        // exp( (ln(100)*70 + ln(50)*22) / 92 )
        // ln(100)=4.605, ln(50)=3.912
        // (322.35 + 86.06) / 92 = 408.41 / 92 = 4.439
        // exp(4.439) = 84.6

        let geo_mean = calculate_weighted_geometric_mean(&scores);
        let arith_mean = calculate_weighted_average(&scores);

        assert!(
            geo_mean < arith_mean,
            "Geometric mean should be lower than arithmetic mean"
        );
    }

    #[test]
    fn test_weighted_average_perfect() {
        let scores = vec![100.0; 12];
        let avg = calculate_weighted_average(&scores);
        assert!((avg - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_weighted_average_rising() {
        // Low start, high finish. Should be closer to high due to exponential weights.
        let mut scores = vec![50.0; 12];
        scores[9] = 90.0;
        scores[10] = 95.0;
        scores[11] = 100.0; // S12 has huge weight (22.0)

        let avg = calculate_weighted_average(&scores);
        // Total weight: 92.0
        // S12 alone is ~23.9% of weight.
        // It should be significantly higher than 50.
        assert!(
            avg > 60.0,
            "Weighted average should reflect late rising trend"
        );
    }

    #[test]
    fn test_cycle_weights_sum() {
        let sum: f32 = CYCLE_WEIGHTS.iter().sum();
        assert!(
            (sum - CYCLE_WEIGHTS_SUM).abs() < 0.1,
            "CYCLE_WEIGHTS_SUM constant should match actual sum: {} vs {}",
            CYCLE_WEIGHTS_SUM,
            sum
        );
    }

    #[test]
    fn test_early_stage_cycle_detection() {
        // S1-S6 (indices 0-5) should be Early Stage
        for i in 0..6 {
            assert!(
                is_early_stage_cycle(i),
                "Cycle S{} should be Early Stage",
                i + 1
            );
        }
        // S7-S12 (indices 6-11) should be Full Analysis
        for i in 6..12 {
            assert!(
                !is_early_stage_cycle(i),
                "Cycle S{} should be Full Analysis",
                i + 1
            );
        }
    }

    #[test]
    fn test_scoring_phase_from_cycle() {
        // S1-S6 should be Early Stage
        for i in 0..6 {
            assert_eq!(ScoringPhase::from_cycle_idx(i), ScoringPhase::EarlyStage);
        }
        // S7-S12 should be Full Analysis
        for i in 6..12 {
            assert_eq!(ScoringPhase::from_cycle_idx(i), ScoringPhase::FullAnalysis);
        }
    }

    #[test]
    fn test_scoring_phase_scr_usage() {
        assert!(
            !ScoringPhase::EarlyStage.should_use_scr(),
            "Early Stage should not use SCR"
        );
        assert!(
            ScoringPhase::FullAnalysis.should_use_scr(),
            "Full Analysis should use SCR"
        );
    }

    #[test]
    fn test_quality_early_stage_formula() {
        // Test Early Stage quality formula: 0.44*mpcf + 0.31*mesa + 0.25*wallet
        let quality = calculate_quality_early_stage(1.0, 1.0, 1.0);
        // Perfect score should be close to 1.0
        assert!(
            (quality - 1.0).abs() < 0.01,
            "Perfect inputs should give quality ~1.0, got {}",
            quality
        );

        // Test with mixed inputs
        let quality_mixed = calculate_quality_early_stage(0.8, 0.6, 0.5);
        let expected = 0.44 * 0.8 + 0.31 * 0.6 + 0.25 * 0.5;
        assert!(
            (quality_mixed - expected).abs() < 0.01,
            "Mixed inputs should give quality ~{}, got {}",
            expected,
            quality_mixed
        );
    }

    #[test]
    fn test_quality_full_analysis_formula() {
        // Test Full Analysis quality formula: 0.35*mpcf + 0.25*mesa + 0.20*(1-scr) + 0.20*wallet
        let quality = calculate_quality_full_analysis(1.0, 1.0, 0.0, 1.0); // scr=0 means organic
                                                                           // Perfect score should be close to 1.0
        assert!(
            (quality - 1.0).abs() < 0.01,
            "Perfect inputs should give quality ~1.0, got {}",
            quality
        );

        // Test with bot-like SCR (high bot score reduces quality)
        let quality_bot = calculate_quality_full_analysis(0.8, 0.6, 0.8, 0.5); // scr=0.8 = bot
        let expected = 0.35 * 0.8 + 0.25 * 0.6 + 0.20 * (1.0 - 0.8) + 0.20 * 0.5;
        assert!(
            (quality_bot - expected).abs() < 0.01,
            "Bot-like inputs should give quality ~{}, got {}",
            expected,
            quality_bot
        );
    }

    #[test]
    fn test_quality_for_cycle_early_stage() {
        // Cycles 0-5 (S1-S6) should use Early Stage formula (ignore SCR)
        for cycle_idx in 0..6 {
            let quality_with_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, Some(0.9), 0.5);
            let quality_without_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, None, 0.5);
            let expected = calculate_quality_early_stage(0.8, 0.6, 0.5);

            // Both should equal Early Stage result (SCR should be ignored)
            assert!(
                (quality_with_scr - expected).abs() < 0.01,
                "Cycle S{} with SCR should use Early Stage formula",
                cycle_idx + 1
            );
            assert!(
                (quality_without_scr - expected).abs() < 0.01,
                "Cycle S{} without SCR should use Early Stage formula",
                cycle_idx + 1
            );
        }
    }

    #[test]
    fn test_quality_for_cycle_full_analysis() {
        // Cycles 6-11 (S7-S12) should use Full Analysis formula (use SCR)
        for cycle_idx in 6..12 {
            let quality_with_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, Some(0.3), 0.5);
            let expected_full = calculate_quality_full_analysis(0.8, 0.6, 0.3, 0.5);

            assert!(
                (quality_with_scr - expected_full).abs() < 0.01,
                "Cycle S{} with SCR should use Full Analysis formula",
                cycle_idx + 1
            );
        }
    }

    #[test]
    fn test_quality_for_cycle_full_analysis_fallback() {
        // Full Analysis without SCR should fall back to Early Stage formula
        for cycle_idx in 6..12 {
            let quality_no_scr = calculate_quality_for_cycle(cycle_idx, 0.8, 0.6, None, 0.5);
            let expected_early = calculate_quality_early_stage(0.8, 0.6, 0.5);

            assert!(
                (quality_no_scr - expected_early).abs() < 0.01,
                "Cycle S{} without SCR should fall back to Early Stage formula",
                cycle_idx + 1
            );
        }
    }

    #[test]
    fn test_quality_weights_sum_to_one() {
        // Early Stage weights should sum to ~1.0
        let early_sum =
            early_stage_weights::MPCF + early_stage_weights::MESA + early_stage_weights::WALLET;
        assert!(
            (early_sum - 1.0).abs() < 0.01,
            "Early Stage weights should sum to 1.0, got {}",
            early_sum
        );

        // Full Analysis weights should sum to ~1.0
        let full_sum = full_analysis_weights::MPCF
            + full_analysis_weights::MESA
            + full_analysis_weights::SCR
            + full_analysis_weights::WALLET;
        assert!(
            (full_sum - 1.0).abs() < 0.01,
            "Full Analysis weights should sum to 1.0, got {}",
            full_sum
        );
    }
}
