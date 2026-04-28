//! Penalty Calculations
//!
//! This module handles all penalty calculations for the scoring system.
//! Penalties can now exceed score bounds - the score can go negative.
//! Final clamping for display happens in the main scoring orchestration.

use super::weights::ScoringWeights;
use crate::analyzers::mesa::MesaResult;
use crate::oracle::{
    cluster_hunter::ClusterAnalysis,
    ultrafast::{ActorInference, ActorType, IwimResult, SourceType, SsmiResult},
};
use tracing::debug;

/// Apply all penalties to the base score WITHOUT capping
///
/// CRITICAL: This function allows scores to go negative.
/// This is intentional - it preserves signal information that was
/// previously lost due to premature clamping.
///
/// The final score will be clamped to [0, 100] only for UI display.
pub fn apply_penalties(
    base_score: f32,
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
    is_early_stage: bool,
) -> f32 {
    let mut score = base_score;

    // ========== MESA MICROSTRUCTURE PENALTIES ==========
    // Only in full analysis mode (requires transaction history)
    if !is_early_stage {
        if let Some(ref mesa) = mesa_result {
            // Wash Trading Detection
            // High wash_likeness = alternating buy/sell with balanced volumes
            // This is artificial volume generation, major red flag
            if mesa.wash_likeness > weights.mesa_wash_severe_threshold {
                let penalty = 25.0 * weights.wash_penalty_mult;
                score -= penalty;
                debug!(
                    "MESA PENALTY: Severe wash trading detected! wash={:.0}% → -{:.1} pts",
                    mesa.wash_likeness * 100.0,
                    penalty
                );
            } else if mesa.wash_likeness > weights.mesa_wash_elevated_threshold {
                let penalty = 12.0 * weights.wash_penalty_mult;
                score -= penalty;
                debug!(
                    "MESA PENALTY: Elevated wash trading. wash={:.0}% → -{:.1} pts",
                    mesa.wash_likeness * 100.0,
                    penalty
                );
            }

            // Bot Pattern Detection
            // High bot_likeness = identical transaction sizes = coordinated bots
            if mesa.bot_likeness > weights.mesa_bot_high_threshold {
                let penalty = 15.0 * weights.bot_penalty_mult;
                score -= penalty;
                debug!(
                    "MESA PENALTY: Bot pattern detected! bot={:.0}% → -{:.1} pts",
                    mesa.bot_likeness * 100.0,
                    penalty
                );
            } else if mesa.bot_likeness > weights.mesa_bot_moderate_threshold {
                let penalty = 8.0 * weights.bot_penalty_mult;
                score -= penalty;
                debug!(
                    "MESA PENALTY: Suspicious bot activity. bot={:.0}% → -{:.1} pts",
                    mesa.bot_likeness * 100.0,
                    penalty
                );
            }
        }
    }

    // ========== SSMI PENALTIES ==========
    // Only in full analysis mode
    if !is_early_stage {
        if let Some(ssmi_res) = ssmi_result {
            if let SourceType::Bot = ssmi_res.source_type {
                let penalty = 15.0 * weights.ssmi_bot_penalty_mult;
                score -= penalty;
                debug!("SSMI PENALTY: Bot source detected → -{:.1} pts", penalty);
            }
        }
    }

    // ========== MPCF PENALTIES ==========
    // Runs in both modes (early warning system)
    if let Some(mpcf_res) = mpcf_result {
        match mpcf_res.actor {
            ActorType::SniperScript | ActorType::MEVArb => {
                let penalty = 10.0 * weights.mpcf_sniper_penalty_mult;
                score -= penalty;
                debug!("MPCF PENALTY: Sniper/MEV detected → -{:.1} pts", penalty);
            }
            ActorType::SybilBot => {
                let penalty = 20.0 * weights.mpcf_sybil_penalty_mult;
                score -= penalty;
                debug!("MPCF PENALTY: Sybil bot detected → -{:.1} pts", penalty);
            }
            _ => {}
        }
    }

    // ========== SCR PENALTY ==========
    // Only in full analysis mode
    if !is_early_stage {
        if let Some(scr_val) = scr_score {
            if scr_val > 0.7 {
                let penalty = 10.0 * weights.scr_penalty_mult;
                score -= penalty;
                debug!("SCR PENALTY: High bot probability → -{:.1} pts", penalty);
            }
        }
    }

    // ========== ULVF PENALTIES ==========
    // Only in full analysis mode
    if !is_early_stage {
        if let Some(div) = ulvf_divergence {
            if div < 0.3 {
                let penalty = 5.0 * weights.ulvf_div_penalty_mult;
                score -= penalty;
                debug!("ULVF PENALTY: Low divergence → -{:.1} pts", penalty);
            }
        }
        if let Some(curl) = ulvf_curl {
            if curl > 15.0 {
                let penalty = 10.0 * weights.ulvf_curl_penalty_mult;
                score -= penalty;
                debug!("ULVF PENALTY: High curl → -{:.1} pts", penalty);
            }
        }
    }

    // ========== POVC CLUSTER PENALTIES ==========
    // Only in full analysis mode
    // BUGFIX: Corrected cluster mapping per AGENTS.md spec
    // - Cluster 0 = ULTRA_ORGANIC (whales/real traders) → BOOST
    // - Cluster 1 = ORGANIC (small genuine traders) → BOOST
    // - Cluster 2 = BOT_NOISE (sniper bots) → PENALTY
    // - Cluster 3 = SYBIL_ATTACK (coordinated wallets) → PENALTY
    if !is_early_stage {
        if let Some(cluster) = povc_cluster {
            // Note: This uses penalties.rs which applies penalties
            // For boosts, we'll need to handle them in boosters.rs
            // For now, only apply penalties for clusters 2 and 3
            if cluster == 2 {
                // Bot noise
                let penalty = 10.0 * weights.povc_penalty_mult;
                score -= penalty;
                debug!("POVC PENALTY: Bot noise (cluster 2) → -{:.1} pts", penalty);
            } else if cluster == 3 {
                // Sybil attack / coordinated dump
                let penalty = 20.0 * weights.povc_penalty_mult;
                score -= penalty;
                debug!(
                    "POVC PENALTY: Sybil/Dump trajectory (cluster 3) → -{:.1} pts",
                    penalty
                );
            }
            // Clusters 0 and 1 are positive (ULTRA_ORGANIC, ORGANIC)
            // These should be handled by boosters, not penalties
        }
    }

    // ========== IWIM PENALTIES ==========
    // Runs in both modes
    if let Some(iwim_res) = iwim_result {
        if iwim_res.rug_threat_score > 0.8 {
            let penalty = 30.0 * weights.rug_penalty_mult;
            score -= penalty;
            debug!("IWIM PENALTY: High rug threat → -{:.1} pts", penalty);
        } else if iwim_res.rug_threat_score > 0.6 {
            let penalty = 15.0 * weights.rug_penalty_mult;
            score -= penalty;
            debug!("IWIM PENALTY: Moderate rug threat → -{:.1} pts", penalty);
        }

        if iwim_res.sybil_score > 0.6 {
            let penalty = 15.0 * weights.rug_penalty_mult;
            score -= penalty;
            debug!("IWIM PENALTY: Sybil behavior → -{:.1} pts", penalty);
        }
    }

    // ========== CLUSTER HUNTER PENALTIES ==========
    // Runs in both modes (essential early warning)
    if let Some(ref cluster) = cluster_result {
        if cluster.risk_score > 0.5 {
            // Moderate cabal risk - apply penalty proportional to risk
            // (risk_score - 0.5) * 30 gives 0-15 points penalty for scores 0.5-1.0
            let risk_clamped = cluster.risk_score.clamp(0.5, 1.0);
            let penalty = (risk_clamped - 0.5) * 30.0 * weights.cluster_penalty_mult;
            score -= penalty;
            debug!(
                "CLUSTER PENALTY: Cabal risk={:.2} → -{:.1} pts",
                cluster.risk_score, penalty
            );
        }
    }

    // ========== CHAOS ENGINE PENALTIES ==========
    // Runs in both modes (essential for Sniper)
    if let Some(chaos_res) = chaos_result {
        if chaos_res.crash_probability > 50.0 {
            let penalty = 20.0 * weights.chaos;
            score -= penalty;
            debug!(
                "CHAOS PENALTY: High crash probability → -{:.1} pts",
                penalty
            );
        } else if chaos_res.crash_probability > 30.0 {
            let penalty = 10.0 * weights.chaos;
            score -= penalty;
            debug!(
                "CHAOS PENALTY: Moderate crash probability → -{:.1} pts",
                penalty
            );
        }

        if chaos_res.median_roi < -10.0 {
            let penalty = 15.0 * weights.chaos;
            score -= penalty;
            debug!("CHAOS PENALTY: Negative median ROI → -{:.1} pts", penalty);
        } else if chaos_res.median_roi < 0.0 {
            let penalty = 5.0 * weights.chaos;
            score -= penalty;
            debug!("CHAOS PENALTY: Slightly negative ROI → -{:.1} pts", penalty);
        }
    }

    // ========== RESONANCE DETECTOR PENALTIES ==========
    // Runs in both modes
    if let Some(res_res) = resonance_result {
        if res_res.is_bot_likely() {
            let penalty = 15.0;
            score -= penalty;
            debug!("RESONANCE PENALTY: Bot-like pattern → -{:.1} pts", penalty);
        } else if res_res.is_suspicious() {
            let penalty = 8.0;
            score -= penalty;
            debug!(
                "RESONANCE PENALTY: Suspicious pattern → -{:.1} pts",
                penalty
            );
        }
    }

    // ========== GENE MAPPER SECURITY PENALTIES ==========
    // Runs in both modes (essential for Sniper)
    if let Some(gene_res) = gene_safety_result {
        use crate::security::gene_mapper::RiskLevel as GeneRiskLevel;

        match gene_res.risk_level {
            GeneRiskLevel::Critical => {
                let penalty = 50.0;
                score -= penalty;
                debug!("GENE PENALTY: Critical security risk → -{:.1} pts", penalty);
            }
            GeneRiskLevel::High => {
                let penalty = 30.0;
                score -= penalty;
                debug!("GENE PENALTY: High security risk → -{:.1} pts", penalty);
            }
            GeneRiskLevel::Medium => {
                let penalty = 15.0;
                score -= penalty;
                debug!("GENE PENALTY: Medium security risk → -{:.1} pts", penalty);
            }
            GeneRiskLevel::Low => {
                let penalty = 5.0;
                score -= penalty;
                debug!("GENE PENALTY: Low security risk → -{:.1} pts", penalty);
            }
            GeneRiskLevel::Safe => {
                // No penalty for safe contracts
            }
        }
    }

    score // Can be negative!
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_penalties_allow_negative_scores() {
        let weights = ScoringWeights::default();

        // Create terrible signals that should drive score negative
        let mesa = MesaResult {
            execution_fingerprint: 0,
            wash_likeness: 0.95, // Severe wash trading
            bot_likeness: 0.95,  // High bot activity
            organic_likeness: 0.0,
            entropy_score: 0.1,
            impact_efficiency: 0.0,
            tx_count: 0,
            analysis_time_us: 0,
        };

        let iwim = IwimResult {
            rug_threat_score: 0.9, // High rug threat
            sybil_score: 0.8,
            organic_score: 0.0,
            confidence: 0.9,
            execution_time_us: 1000,
        };

        // Start with a low base score
        let base_score = 20.0;

        let final_score = apply_penalties(
            base_score,
            &None, // ssmi
            &None, // mpcf
            &Some(iwim),
            None, // scr
            None, // ulvf_div
            None, // ulvf_curl
            None, // povc
            &Some(mesa),
            &None, // cluster
            &None, // chaos
            &None, // resonance
            &None, // gene
            &weights,
            false, // not early stage
        );

        // With severe wash (25) + high bot (15) + high rug threat (30) = 70 points penalty
        // Starting from 20, should go to -50
        assert!(
            final_score < 0.0,
            "Severe penalties should allow negative scores, got {}",
            final_score
        );
    }

    #[test]
    fn test_early_stage_skips_trend_penalties() {
        let weights = ScoringWeights::default();

        let mesa = MesaResult {
            execution_fingerprint: 0,
            wash_likeness: 0.95,
            bot_likeness: 0.0,
            organic_likeness: 0.0,
            entropy_score: 0.0,
            impact_efficiency: 0.0,
            tx_count: 0,
            analysis_time_us: 0,
        };

        let base_score = 50.0;

        // Early stage should skip MESA penalties
        let score_early = apply_penalties(
            base_score,
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &Some(mesa.clone()),
            &None,
            &None,
            &None,
            &None,
            &weights,
            true, // early stage
        );

        // Full analysis should apply MESA penalties
        let score_full = apply_penalties(
            base_score,
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &Some(mesa),
            &None,
            &None,
            &None,
            &None,
            &weights,
            false, // not early stage
        );

        assert_eq!(
            score_early, base_score,
            "Early stage should not apply MESA penalties"
        );
        assert!(
            score_full < base_score,
            "Full analysis should apply MESA penalties"
        );
    }

    #[test]
    fn test_penalty_multipliers_work() {
        let mut weights = ScoringWeights::default();
        weights.wash_penalty_mult = 2.0; // Double the wash penalty

        let mesa = MesaResult {
            execution_fingerprint: 0,
            wash_likeness: 0.90, // Severe wash
            bot_likeness: 0.0,
            organic_likeness: 0.0,
            entropy_score: 0.0,
            impact_efficiency: 0.0,
            tx_count: 0,
            analysis_time_us: 0,
        };

        let base_score = 100.0;

        let final_score = apply_penalties(
            base_score,
            &None,
            &None,
            &None,
            None,
            None,
            None,
            None,
            &Some(mesa),
            &None,
            &None,
            &None,
            &None,
            &weights,
            false,
        );

        // Severe wash penalty = 25 * 2.0 = 50
        // Score should be 100 - 50 = 50
        assert_eq!(
            final_score, 50.0,
            "Penalty multiplier should double the penalty"
        );
    }

    // =================================================================
    // REGRESSION TESTS FOR BUG #2: POVC PENALTY MAPPING
    // =================================================================

    #[test]
    fn test_povc_cluster_0_no_penalty() {
        // BUG #2 Regression Test: Cluster 0 (ULTRA_ORGANIC) should NOT be penalized
        // Production log showed: cluster 0 got -20 penalty (inverted logic)
        // Expected: cluster 0 should have NO penalty (boost is in boosters.rs)
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        // Cluster 0 = ULTRA_ORGANIC (whales/real traders)
        let final_score = apply_penalties(
            base_score,
            &None,
            &None,
            &None,
            None,
            None,
            None,
            Some(0), // cluster 0
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            false, // full analysis mode
        );

        assert_eq!(
            final_score, base_score,
            "BUGFIX: Cluster 0 (ULTRA_ORGANIC) should have NO penalty, got score {}",
            final_score
        );
    }

    #[test]
    fn test_povc_cluster_1_no_penalty() {
        // BUG #2 Regression Test: Cluster 1 (ORGANIC) should NOT be penalized
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        // Cluster 1 = ORGANIC (small genuine traders)
        let final_score = apply_penalties(
            base_score,
            &None,
            &None,
            &None,
            None,
            None,
            None,
            Some(1), // cluster 1
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            false,
        );

        assert_eq!(
            final_score, base_score,
            "BUGFIX: Cluster 1 (ORGANIC) should have NO penalty, got score {}",
            final_score
        );
    }

    #[test]
    fn test_povc_cluster_2_gets_penalty() {
        // BUG #2 Regression Test: Cluster 2 (BOT_NOISE) should be penalized
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        // Cluster 2 = BOT_NOISE (sniper bots)
        let final_score = apply_penalties(
            base_score,
            &None,
            &None,
            &None,
            None,
            None,
            None,
            Some(2), // cluster 2
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            false,
        );

        assert!(
            final_score < base_score,
            "BUGFIX: Cluster 2 (BOT_NOISE) should be penalized, got score {}",
            final_score
        );

        // Should be -10 pts
        assert_eq!(final_score, 40.0, "Cluster 2 penalty should be -10 pts");
    }

    #[test]
    fn test_povc_cluster_3_gets_heavy_penalty() {
        // BUG #2 Regression Test: Cluster 3 (SYBIL_ATTACK) should be heavily penalized
        let weights = ScoringWeights::default();
        let base_score = 50.0;

        // Cluster 3 = SYBIL_ATTACK (coordinated dump)
        let final_score = apply_penalties(
            base_score,
            &None,
            &None,
            &None,
            None,
            None,
            None,
            Some(3), // cluster 3
            &None,
            &None,
            &None,
            &None,
            &None,
            &weights,
            false,
        );

        assert!(
            final_score < base_score,
            "BUGFIX: Cluster 3 (SYBIL_ATTACK) should be heavily penalized, got score {}",
            final_score
        );

        // Should be -20 pts
        assert_eq!(final_score, 30.0, "Cluster 3 penalty should be -20 pts");
    }
}
