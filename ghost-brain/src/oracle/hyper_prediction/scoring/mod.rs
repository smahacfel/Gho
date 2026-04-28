//! Scoring Module - Unified scoring orchestration
//!
//! This module provides the main scoring function that combines:
//! - SurvivorScore as the base
//! - Penalties (uncapped - can go negative)
//! - Boosters (uncapped - can exceed 100)
//! - Risk determination from RAW score
//! - Display score clamping (0-100 for UI)

pub mod boosters;
pub mod penalties;
pub mod weights;

pub use weights::ScoringWeights;

use crate::analyzers::mesa::MesaResult;
use crate::config::FallbackTracker;
use crate::oracle::hyper_prediction::verdict::RiskThresholds;
use crate::oracle::{
    cluster_hunter::ClusterAnalysis,
    hyper_prediction::verdict::RiskLevel,
    survivor_score::SurvivorScoreResult,
    ultrafast::{ActorInference, IwimResult, QASSResult, SsmiResult},
};
use tracing::debug;

/// Main scoring function - replaces combine_scores
///
/// This function orchestrates the complete scoring pipeline:
/// 1. Base score from SurvivorScore (or fallback)
/// 2. Apply QASS secondary modifier (±10 points max)
/// 3. Apply fallback confidence multiplier
/// 4. Apply penalties (UNCAPPED - can go negative)
/// 5. Apply boosters (UNCAPPED - can exceed 100)
/// 6. Determine risk level from RAW score
/// 7. Clamp ONLY for UI display (0-100)
///
/// Returns: (display_score, risk_level, passed)
pub fn calculate_final_score(
    survivor_result: &Option<SurvivorScoreResult>,
    qass_result: &QASSResult,
    ssmi_result: &Option<SsmiResult>,
    mpcf_result: &Option<ActorInference>,
    iwim_result: &Option<IwimResult>,
    scr_score: Option<f32>,
    ulvf_divergence: Option<f32>,
    ulvf_curl: Option<f32>,
    povc_cluster: Option<usize>,
    mesa_result: &Option<MesaResult>,
    cluster_result: &Option<ClusterAnalysis>,
    chaos_result: &Option<crate::chaos::engine::ChaosResult>,
    resonance_result: &Option<crate::signals::resonance::ResonanceResult>,
    gene_safety_result: &Option<crate::security::gene_mapper::GeneAnalysisResult>,
    weights: &ScoringWeights,
    risk_thresholds: &RiskThresholds,
    fallback_tracker: &FallbackTracker,
    threshold: u8,
    base_score: u8,
    is_early_stage: bool,
) -> (u8, RiskLevel, bool) {
    // =================================================================
    // STEP 1: Get base score from SurvivorScore (or fallback)
    // =================================================================
    let (survivor_score, survivor_passed, survivor_confidence) =
        if let Some(ref survivor) = survivor_result {
            (survivor.score, survivor.passed, survivor.confidence)
        } else {
            // Fallback if SurvivorScore unavailable
            const SURVIVOR_FALLBACK_CONFIDENCE: f32 = 0.5;
            (
                base_score,
                base_score >= threshold,
                SURVIVOR_FALLBACK_CONFIDENCE,
            )
        };

    // =================================================================
    // STEP 2: Apply QASS secondary modifier (from weights config)
    // =================================================================
    let qass_modifier: i8 = if qass_result.is_valid
        && qass_result.confidence > (weights.qass_min_confidence_for_modifier as f64)
    {
        // QASS can add/subtract max adjustment points (configured in weights)
        // Formula: (score_100 - 50) / 50 * max_adj → maps [0, 100] to [-max_adj, +max_adj]
        let score_100 = (qass_result.score * 100.0) as f32;
        let raw_mod =
            ((score_100 - 50.0) / 50.0 * weights.qass_secondary_max_adjustment as f32) as i8;
        raw_mod.clamp(
            -weights.qass_secondary_max_adjustment,
            weights.qass_secondary_max_adjustment,
        )
    } else {
        0
    };

    // =================================================================
    // STEP 3: Apply fallback confidence multiplier
    // =================================================================
    let confidence_multiplier = fallback_tracker.confidence_multiplier();

    // =================================================================
    // STEP 4: Calculate adjusted base (with QASS + fallback penalty)
    // =================================================================
    let adjusted_score = (survivor_score as i16 + qass_modifier as i16).clamp(0, 100) as u8;
    let base_with_fallback = (adjusted_score as f32 * confidence_multiplier) as f32;

    debug!(
        "SCORING: survivor={} qass_mod={:+} fallback_mult={:.2} → base={:.1}",
        survivor_score, qass_modifier, confidence_multiplier, base_with_fallback
    );

    // =================================================================
    // STEP 5: Apply penalties (UNCAPPED - can go negative)
    // =================================================================
    let penalized_score = penalties::apply_penalties(
        base_with_fallback,
        ssmi_result,
        mpcf_result,
        iwim_result,
        scr_score,
        ulvf_divergence,
        ulvf_curl,
        povc_cluster,
        mesa_result,
        cluster_result,
        chaos_result,
        resonance_result,
        gene_safety_result,
        weights,
        is_early_stage,
    );

    // =================================================================
    // STEP 6: Apply boosters (UNCAPPED - can exceed 100)
    // =================================================================
    let boosted_score = boosters::apply_boosters(
        penalized_score,
        ssmi_result,
        iwim_result,
        povc_cluster, // Added povc_cluster parameter for BUGFIX
        mesa_result,
        cluster_result,
        chaos_result,
        resonance_result,
        weights,
        is_early_stage,
    );

    debug!(
        "SCORING: base={:.1} → penalized={:.1} → boosted={:.1} (RAW, uncapped)",
        base_with_fallback, penalized_score, boosted_score
    );

    // =================================================================
    // STEP 7: Determine risk level from RAW score (not clamped)
    // =================================================================
    // Risk determination uses the uncapped score to better represent
    // extreme cases (very negative = VeryHigh, very high = Low)
    let risk = if boosted_score < 20.0 {
        RiskLevel::VeryHigh
    } else if boosted_score < 40.0 {
        RiskLevel::High
    } else if boosted_score < 60.0 {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };

    // =================================================================
    // STEP 8: Clamp ONLY for display (0-100)
    // =================================================================
    let display_score = boosted_score.clamp(0.0, 100.0) as u8;

    // =================================================================
    // STEP 9: Determine passed status
    // =================================================================
    // Passed if:
    // - SurvivorScore passed (or fallback passed)
    // - Display score >= threshold
    let passed = survivor_passed && display_score >= threshold;

    debug!(
        "SCORING FINAL: display={} risk={:?} passed={}",
        display_score, risk, passed
    );

    (display_score, risk, passed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::survivor_score::{SurvivorScoreBreakdown, SurvivorScoreResult};

    /// Creates a mock QASSResult for backward compatibility testing.
    ///
    /// NOTE: QASSResult is deprecated (replaced by SurvivorScore) but is still
    /// used in `calculate_final_score` for QASS secondary modifier calculation.
    /// These tests verify the scoring pipeline works correctly during the
    /// transition period while some components still reference QASS.
    fn create_mock_qass() -> QASSResult {
        QASSResult {
            score: 0.7,
            score_100: 70,
            confidence: 0.8,
            is_valid: true,
            dominant_waves: vec![],
            wave_breakdown: vec![],
            data_source: crate::oracle::ultrafast::qass_stub::DataSource::Synthetic,
            analysis_time_ns: 0,
        }
    }

    /// Creates a mock SurvivorScoreResult for testing the scoring pipeline.
    fn create_mock_survivor(score: u8, confidence: f32, passed: bool) -> SurvivorScoreResult {
        SurvivorScoreResult {
            score,
            raw_score: score as f32 / 100.0,
            passed,
            breakdown: SurvivorScoreBreakdown::default(),
            interpretation: String::new(),
            confidence,
            signals_used: 10,      // Reasonable mock value for test coverage
            analysis_time_us: 100, // Reasonable mock value for test timing
            veto_reason: None,
        }
    }

    #[test]
    fn test_scoring_allows_negative_internal() {
        let weights = ScoringWeights::default();
        let risk_thresholds = RiskThresholds::default();
        let fallback_tracker = FallbackTracker::new();

        // Create a low survivor score
        let survivor = create_mock_survivor(30, 0.5, false);

        // Add severe penalties through IWIM
        let iwim = IwimResult {
            rug_threat_score: 0.9,
            sybil_score: 0.8,
            organic_score: 0.0,
            confidence: 0.9,
            execution_time_us: 1000,
        };

        let (display, risk, passed) = calculate_final_score(
            &Some(survivor),
            &create_mock_qass(),
            &None,
            &None,
            &Some(iwim),
            None,
            None,
            None,
            None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            &risk_thresholds,
            &fallback_tracker,
            70,
            30,
            false,
        );

        // Display should be clamped to 0
        assert_eq!(display, 0, "Display score should be clamped to 0");

        // Risk should be VeryHigh (because internal score is very negative)
        assert_eq!(
            risk,
            RiskLevel::VeryHigh,
            "Risk should be VeryHigh for very negative internal scores"
        );

        // Should not pass
        assert!(!passed, "Should not pass with negative internal score");
    }

    #[test]
    fn test_scoring_allows_above_100_internal() {
        let weights = ScoringWeights::default();
        let risk_thresholds = RiskThresholds::default();
        let fallback_tracker = FallbackTracker::new();

        // Create a high survivor score
        let survivor = create_mock_survivor(95, 0.9, true);

        // Add strong boosters
        let chaos = crate::chaos::engine::ChaosResult {
            pump_probability: 75.0,
            crash_probability: 5.0,
            median_roi: 30.0,
            p5_roi: 10.0,
            p95_roi: 50.0,
            mean_price_change: 25.0,
            price_volatility: 20.0,
            num_simulations: 10000,
            execution_time_ms: 500,
            avg_time_per_sim_us: 50.0,
        };

        let (display, risk, passed) = calculate_final_score(
            &Some(survivor),
            &create_mock_qass(),
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &None,
            &None,
            &Some(chaos),
            &None,
            &None,
            &weights,
            &risk_thresholds,
            &fallback_tracker,
            70,
            95,
            false,
        );

        // Display should be clamped to 100
        assert_eq!(display, 100, "Display score should be clamped to 100");

        // Risk should be Low (because internal score is very high)
        assert_eq!(
            risk,
            RiskLevel::Low,
            "Risk should be Low for very high internal scores"
        );

        // Should pass
        assert!(passed, "Should pass with high internal score");
    }

    #[test]
    fn test_risk_determined_from_raw_score() {
        let weights = ScoringWeights::default();
        let risk_thresholds = RiskThresholds::default();
        let fallback_tracker = FallbackTracker::new();

        // Test VeryHigh risk (< 20)
        let survivor_very_low = create_mock_survivor(15, 0.5, false);

        let (_, risk, _) = calculate_final_score(
            &Some(survivor_very_low),
            &create_mock_qass(),
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            &risk_thresholds,
            &fallback_tracker,
            70,
            15,
            false,
        );
        assert_eq!(risk, RiskLevel::VeryHigh);

        // Test High risk (20-40)
        let survivor_low = create_mock_survivor(35, 0.5, false);

        let (_, risk, _) = calculate_final_score(
            &Some(survivor_low),
            &create_mock_qass(),
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            &risk_thresholds,
            &fallback_tracker,
            70,
            35,
            false,
        );
        assert_eq!(risk, RiskLevel::High);

        // Test Medium risk (40-60)
        let survivor_medium = create_mock_survivor(55, 0.7, false);

        let (_, risk, _) = calculate_final_score(
            &Some(survivor_medium),
            &create_mock_qass(),
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            &risk_thresholds,
            &fallback_tracker,
            70,
            55,
            false,
        );
        assert_eq!(risk, RiskLevel::Medium);

        // Test Low risk (>= 60)
        let survivor_high = create_mock_survivor(85, 0.9, true);

        let (_, risk, _) = calculate_final_score(
            &Some(survivor_high),
            &create_mock_qass(),
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            &risk_thresholds,
            &fallback_tracker,
            70,
            85,
            false,
        );
        assert_eq!(risk, RiskLevel::Low);
    }

    #[test]
    fn test_weights_validation() {
        let mut weights = ScoringWeights::default();
        assert!(weights.validate().is_ok());

        // Invalid negative penalty
        weights.wash_penalty_mult = -0.5;
        assert!(weights.validate().is_err());
    }
}
