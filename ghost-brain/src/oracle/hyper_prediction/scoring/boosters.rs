//! Boost Calculations
//!
//! This module handles all boost calculations for the scoring system.
//! Boosts can now exceed score bounds - the score can go above 100.
//! Final clamping for display happens in the main scoring orchestration.

use super::weights::ScoringWeights;
use crate::analyzers::mesa::MesaResult;
use crate::oracle::{
    cluster_hunter::ClusterAnalysis,
    ultrafast::{IwimResult, SourceType, SsmiResult},
};
use tracing::debug;

/// Apply all boosts to the score WITHOUT capping
///
/// CRITICAL: This function allows scores to exceed 100.
/// This is intentional - it preserves signal information that was
/// previously lost due to premature clamping.
///
/// The final score will be clamped to [0, 100] only for UI display.
pub fn apply_boosters(
    base_score: f32,
    ssmi_result: &Option<SsmiResult>,
    iwim_result: &Option<IwimResult>,
    povc_cluster: Option<usize>,
    mesa_result: &Option<MesaResult>,
    cluster_result: &Option<ClusterAnalysis>,
    chaos_result: &Option<crate::chaos::engine::ChaosResult>,
    resonance_result: &Option<crate::signals::resonance::ResonanceResult>,
    weights: &ScoringWeights,
    is_early_stage: bool,
) -> f32 {
    let mut score = base_score;

    // ========== SSMI BOOSTS ==========
    // Only in full analysis mode
    if !is_early_stage {
        if let Some(ssmi_res) = ssmi_result {
            match ssmi_res.source_type {
                SourceType::ViralLaunch => {
                    let boost = 10.0 * weights.ssmi_viral_boost_mult;
                    score += boost;
                    debug!("SSMI BOOST: Viral launch detected → +{:.1} pts", boost);
                }
                SourceType::Human => {
                    let boost = 5.0 * weights.ssmi_human_boost_mult;
                    score += boost;
                    debug!("SSMI BOOST: Human activity detected → +{:.1} pts", boost);
                }
                _ => {}
            }
        }
    }

    // ========== POVC CLUSTER BOOSTS ==========
    // Only in full analysis mode
    // BUGFIX: Added POVC boosts for organic clusters per AGENTS.md
    // - Cluster 0 = ULTRA_ORGANIC (whales/real traders) → +15 boost
    // - Cluster 1 = ORGANIC (small genuine traders) → +5 boost
    if !is_early_stage {
        if let Some(cluster) = povc_cluster {
            if cluster == 0 {
                // ULTRA_ORGANIC: whales/real traders
                let boost = 15.0 * weights.povc_organic_boost_mult;
                score += boost;
                debug!("POVC BOOST: ULTRA_ORGANIC (cluster 0) → +{:.1} pts", boost);
            } else if cluster == 1 {
                // ORGANIC: small genuine traders
                let boost = 5.0 * weights.povc_organic_boost_mult;
                score += boost;
                debug!("POVC BOOST: ORGANIC (cluster 1) → +{:.1} pts", boost);
            }
        }
    }

    // ========== MESA BOOSTS ==========
    // Only in full analysis mode
    if !is_early_stage {
        if let Some(ref mesa) = mesa_result {
            // Organic Activity Bonus
            // High organic + low wash = healthy market
            if mesa.organic_likeness > weights.mesa_organic_bonus_threshold
                && mesa.wash_likeness < weights.mesa_organic_max_wash
            {
                let boost = 8.0 * weights.mesa_organic_boost_mult;
                score += boost;
                debug!(
                    "MESA BOOST: Organic activity detected. organic={:.0}% → +{:.1} pts",
                    mesa.organic_likeness * 100.0,
                    boost
                );
            }

            // High Entropy Bonus (balanced buy/sell with no wash)
            if mesa.entropy_score > weights.mesa_entropy_bonus_threshold
                && mesa.wash_likeness < weights.mesa_entropy_max_wash
            {
                let boost = 5.0 * weights.mesa_entropy_boost_mult;
                score += boost;
                debug!(
                    "MESA BOOST: High entropy trading. entropy={:.0}% → +{:.1} pts",
                    mesa.entropy_score * 100.0,
                    boost
                );
            }
        }
    }

    // ========== IWIM BOOSTS ==========
    // Runs in both modes
    if let Some(iwim_res) = iwim_result {
        if iwim_res.organic_score > 0.7 {
            let boost = 10.0 * weights.organic_boost_mult;
            score += boost;
            debug!("IWIM BOOST: High organic score → +{:.1} pts", boost);
        }
    }

    // ========== CLUSTER HUNTER BOOSTS ==========
    // Runs in both modes
    if let Some(ref cluster) = cluster_result {
        // Bonus for very clean wallet distribution (no clusters detected)
        if cluster.metrics.cluster_count == 0 {
            let boost = 5.0 * weights.cluster_clean_boost_mult;
            score += boost;
            debug!(
                "CLUSTER BOOST: Clean wallet distribution → +{:.1} pts",
                boost
            );
        }
    }

    // ========== CHAOS ENGINE BOOSTS ==========
    // Runs in both modes (essential for Sniper)
    if let Some(chaos_res) = chaos_result {
        if chaos_res.pump_probability > 60.0 {
            let boost = 15.0 * weights.chaos_pump_boost_mult;
            score += boost;
            debug!("CHAOS BOOST: High pump probability → +{:.1} pts", boost);
        } else if chaos_res.pump_probability > 40.0 {
            let boost = 8.0 * weights.chaos_pump_boost_mult;
            score += boost;
            debug!("CHAOS BOOST: Moderate pump probability → +{:.1} pts", boost);
        }
    }

    // ========== RESONANCE DETECTOR BOOSTS ==========
    // Runs in both modes
    if let Some(res_res) = resonance_result {
        if res_res.is_human_likely() {
            let boost = 5.0 * weights.resonance_human_boost_mult;
            score += boost;
            debug!("RESONANCE BOOST: Human-like pattern → +{:.1} pts", boost);
        }
    }

    score // Can exceed 100!
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boosters_allow_scores_above_100() {
        let weights = ScoringWeights::default();

        // Create perfect signals that should drive score above 100
        let mesa = MesaResult {
            execution_fingerprint: 0,
            wash_likeness: 0.0,
            bot_likeness: 0.0,
            organic_likeness: 0.85, // High organic
            entropy_score: 0.90,    // High entropy
            impact_efficiency: 1.0,
            tx_count: 0,
            analysis_time_us: 0,
        };

        let iwim = IwimResult {
            rug_threat_score: 0.0,
            sybil_score: 0.0,
            organic_score: 0.9, // High organic
            confidence: 0.9,
            execution_time_us: 1000,
        };

        let chaos = crate::chaos::engine::ChaosResult {
            pump_probability: 75.0, // High pump probability
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

        // Start with a high base score
        let base_score = 95.0;

        let final_score = apply_boosters(
            base_score,
            &None, // ssmi
            &Some(iwim),
            None, // povc_cluster
            &Some(mesa),
            &None, // cluster
            &Some(chaos),
            &None, // resonance
            &weights,
            false, // not early stage
        );

        // With organic MESA (8) + entropy MESA (5) + IWIM organic (10) + Chaos pump (15) = 38 points boost
        // Starting from 95, should go to 133
        assert!(
            final_score > 100.0,
            "Strong boosts should allow scores above 100, got {}",
            final_score
        );
    }

    #[test]
    fn test_early_stage_skips_trend_boosts() {
        let weights = ScoringWeights::default();

        let mesa = MesaResult {
            execution_fingerprint: 0,
            wash_likeness: 0.0,
            bot_likeness: 0.0,
            organic_likeness: 0.85, // Would trigger boost in full analysis
            entropy_score: 0.0,
            impact_efficiency: 1.0,
            tx_count: 0,
            analysis_time_us: 0,
        };

        let base_score = 50.0;

        // Early stage should skip MESA boosts
        let score_early = apply_boosters(
            base_score,
            &None,
            &None,
            None,
            &Some(mesa.clone()),
            &None,
            &None,
            &None,
            &weights,
            true, // early stage
        );

        // Full analysis should apply MESA boosts
        let score_full = apply_boosters(
            base_score,
            &None,
            &None,
            None,
            &Some(mesa),
            &None,
            &None,
            &None,
            &weights,
            false, // not early stage
        );

        assert_eq!(
            score_early, base_score,
            "Early stage should not apply MESA boosts"
        );
        assert!(
            score_full > base_score,
            "Full analysis should apply MESA boosts"
        );
    }

    #[test]
    fn test_boost_multipliers_work() {
        let mut weights = ScoringWeights::default();
        weights.chaos_pump_boost_mult = 2.0; // Double the chaos pump boost

        let chaos = crate::chaos::engine::ChaosResult {
            pump_probability: 70.0, // High pump
            crash_probability: 10.0,
            median_roi: 20.0,
            p5_roi: 10.0,
            p95_roi: 30.0,
            mean_price_change: 15.0,
            price_volatility: 25.0,
            num_simulations: 10000,
            execution_time_ms: 500,
            avg_time_per_sim_us: 50.0,
        };

        let base_score = 50.0;

        let final_score = apply_boosters(
            base_score,
            &None,
            &None,
            None,
            &None,
            &None,
            &Some(chaos),
            &None,
            &weights,
            false,
        );

        // Chaos pump boost = 15 * 2.0 = 30
        // Score should be 50 + 30 = 80
        assert_eq!(
            final_score, 80.0,
            "Boost multiplier should double the boost"
        );
    }

    // =================================================================
    // REGRESSION TESTS FOR BUG #2: POVC BOOSTS
    // =================================================================

    #[test]
    fn test_povc_cluster_0_gets_boost() {
        // BUG #2 Regression Test: Cluster 0 (ULTRA_ORGANIC) should get +15 boost
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        // Cluster 0 = ULTRA_ORGANIC (whales/real traders)
        let final_score = apply_boosters(
            base_score,
            &None,
            &None,
            Some(0), // cluster 0
            &None,
            &None,
            &None,
            &None,
            &weights,
            false, // full analysis mode
        );

        assert!(
            final_score > base_score,
            "BUGFIX: Cluster 0 (ULTRA_ORGANIC) should get boost, got score {}",
            final_score
        );

        // Should be +15 pts
        assert_eq!(final_score, 65.0, "Cluster 0 boost should be +15 pts");
    }

    #[test]
    fn test_povc_cluster_1_gets_small_boost() {
        // BUG #2 Regression Test: Cluster 1 (ORGANIC) should get +5 boost
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        // Cluster 1 = ORGANIC (small genuine traders)
        let final_score = apply_boosters(
            base_score,
            &None,
            &None,
            Some(1), // cluster 1
            &None,
            &None,
            &None,
            &None,
            &weights,
            false,
        );

        assert!(
            final_score > base_score,
            "BUGFIX: Cluster 1 (ORGANIC) should get boost, got score {}",
            final_score
        );

        // Should be +5 pts
        assert_eq!(final_score, 55.0, "Cluster 1 boost should be +5 pts");
    }

    #[test]
    fn test_povc_cluster_2_no_boost() {
        // Cluster 2 (BOT_NOISE) should get NO boost
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        let final_score = apply_boosters(
            base_score,
            &None,
            &None,
            Some(2), // cluster 2
            &None,
            &None,
            &None,
            &None,
            &weights,
            false,
        );

        assert_eq!(
            final_score, base_score,
            "Cluster 2 (BOT_NOISE) should have NO boost"
        );
    }

    #[test]
    fn test_povc_cluster_3_no_boost() {
        // Cluster 3 (SYBIL_ATTACK) should get NO boost
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        let final_score = apply_boosters(
            base_score,
            &None,
            &None,
            Some(3), // cluster 3
            &None,
            &None,
            &None,
            &None,
            &weights,
            false,
        );

        assert_eq!(
            final_score, base_score,
            "Cluster 3 (SYBIL_ATTACK) should have NO boost"
        );
    }

    #[test]
    fn test_povc_early_stage_skips_boost() {
        // Early stage should skip POVC boosts (requires transaction history)
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        let score_early = apply_boosters(
            base_score,
            &None,
            &None,
            Some(0), // cluster 0 would normally give boost
            &None,
            &None,
            &None,
            &None,
            &weights,
            true, // early stage
        );

        assert_eq!(
            score_early, base_score,
            "Early stage should NOT apply POVC boosts"
        );
    }
}
