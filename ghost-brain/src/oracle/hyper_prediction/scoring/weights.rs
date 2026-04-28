//! Scoring Weights Configuration
//!
//! This module contains all configurable weights and thresholds for the scoring system.
//! Previously these were hardcoded constants scattered throughout the codebase.

use crate::config::{ghost_brain_config::ScoringWeightsConfig, GhostBrainConfig};
use serde::{Deserialize, Serialize};

/// Centralized scoring weights and thresholds
/// Replaces all hardcoded constants from hyper_prediction/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeights {
    // ============= Signal Weights =============
    /// Weight for LIGMA signal in scoring
    pub ligma: f32,

    /// Weight for QEDD signal in scoring
    pub qedd: f32,

    /// Weight for SurvivorScore in final calculation
    pub survivor: f32,

    /// Weight for QASS as secondary modifier
    pub qass_secondary: f32,

    /// Weight for MCI signal in scoring
    pub mci: f32,

    /// Weight for Cluster analysis in scoring
    pub cluster: f32,

    /// Weight for Chaos Engine results
    pub chaos: f32,

    // ============= Penalty Multipliers =============
    /// Multiplier for wash trading penalty
    pub wash_penalty_mult: f32,

    /// Multiplier for bot pattern penalty
    pub bot_penalty_mult: f32,

    /// Multiplier for rug threat penalty
    pub rug_penalty_mult: f32,

    /// Multiplier for cluster cabal penalty
    pub cluster_penalty_mult: f32,

    /// Multiplier for SSMI bot detection penalty
    pub ssmi_bot_penalty_mult: f32,

    /// Multiplier for SCR bot detection penalty
    pub scr_penalty_mult: f32,

    /// Multiplier for ULVF divergence penalty
    pub ulvf_div_penalty_mult: f32,

    /// Multiplier for ULVF curl penalty
    pub ulvf_curl_penalty_mult: f32,

    /// Multiplier for POVC cluster penalty
    pub povc_penalty_mult: f32,

    /// Multiplier for POVC organic cluster boost
    pub povc_organic_boost_mult: f32,

    /// Multiplier for MPCF sniper/MEV penalty
    pub mpcf_sniper_penalty_mult: f32,

    /// Multiplier for MPCF sybil penalty
    pub mpcf_sybil_penalty_mult: f32,

    // ============= Boost Multipliers =============
    /// Multiplier for organic activity boost
    pub organic_boost_mult: f32,

    /// Multiplier for smart money boost
    pub smart_money_boost_mult: f32,

    /// Multiplier for SSMI viral launch boost
    pub ssmi_viral_boost_mult: f32,

    /// Multiplier for SSMI human boost
    pub ssmi_human_boost_mult: f32,

    /// Multiplier for MESA organic bonus
    pub mesa_organic_boost_mult: f32,

    /// Multiplier for MESA entropy bonus
    pub mesa_entropy_boost_mult: f32,

    /// Multiplier for Chaos pump probability boost
    pub chaos_pump_boost_mult: f32,

    /// Multiplier for Resonance human detection boost
    pub resonance_human_boost_mult: f32,

    /// Multiplier for clean cluster bonus
    pub cluster_clean_boost_mult: f32,

    // ============= Normalization Scales =============
    /// Minimum volume scale for normalization
    pub volume_scale: f64,

    /// Liquidity scale for normalization
    pub liquidity_scale: f64,

    /// Cap for relative factor calculations
    pub relative_factor_cap: f64,

    /// Normalization factor for burst detection
    pub burst_normalization: f64,

    // ============= Thresholds =============
    /// MESA wash trading severe threshold (triggers major penalty)
    pub mesa_wash_severe_threshold: f32,

    /// MESA wash trading elevated threshold
    pub mesa_wash_elevated_threshold: f32,

    /// MESA bot pattern high threshold
    pub mesa_bot_high_threshold: f32,

    /// MESA bot pattern moderate threshold
    pub mesa_bot_moderate_threshold: f32,

    /// MESA organic activity bonus threshold
    pub mesa_organic_bonus_threshold: f32,

    /// Maximum wash likeness for organic bonus
    pub mesa_organic_max_wash: f32,

    /// MESA entropy bonus threshold
    pub mesa_entropy_bonus_threshold: f32,

    /// Maximum wash likeness for entropy bonus
    pub mesa_entropy_max_wash: f32,

    /// SurvivorScore critical threshold for early exit
    pub survivor_critical_threshold: u8,

    /// Maximum QASS adjustment as secondary modifier
    pub qass_secondary_max_adjustment: i8,

    /// Minimum QASS confidence for modifier to apply
    pub qass_min_confidence_for_modifier: f32,

    /// Maximum adjustment factor for Cold Start multiplier
    pub cold_start_max_adjustment: f32,

    /// QEDD/MCI weight in cold start mode
    pub cold_start_qedd_mci_weight: f32,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            // Signal weights (neutral baseline)
            ligma: 1.0,
            qedd: 1.0,
            survivor: 1.0,
            qass_secondary: 1.0,
            mci: 1.0,
            cluster: 1.0,
            chaos: 1.0,

            // Penalty multipliers (baseline = 1.0, adjust to increase/decrease severity)
            wash_penalty_mult: 1.0,
            bot_penalty_mult: 1.0,
            rug_penalty_mult: 1.0,
            cluster_penalty_mult: 1.0,
            ssmi_bot_penalty_mult: 1.0,
            scr_penalty_mult: 1.0,
            ulvf_div_penalty_mult: 1.0,
            ulvf_curl_penalty_mult: 1.0,
            povc_penalty_mult: 1.0,
            povc_organic_boost_mult: 1.0,
            mpcf_sniper_penalty_mult: 1.0,
            mpcf_sybil_penalty_mult: 1.0,

            // Boost multipliers (baseline = 1.0)
            organic_boost_mult: 1.0,
            smart_money_boost_mult: 1.0,
            ssmi_viral_boost_mult: 1.0,
            ssmi_human_boost_mult: 1.0,
            mesa_organic_boost_mult: 1.0,
            mesa_entropy_boost_mult: 1.0,
            chaos_pump_boost_mult: 1.0,
            resonance_human_boost_mult: 1.0,
            cluster_clean_boost_mult: 1.0,

            // Normalization scales (from original constants)
            volume_scale: 1e-4,
            liquidity_scale: 1.0,
            relative_factor_cap: 2.0,
            burst_normalization: 2.0,

            // Thresholds (from original MESA constants)
            mesa_wash_severe_threshold: 0.85,
            mesa_wash_elevated_threshold: 0.70,
            mesa_bot_high_threshold: 0.90,
            mesa_bot_moderate_threshold: 0.75,
            mesa_organic_bonus_threshold: 0.75,
            mesa_organic_max_wash: 0.40,
            mesa_entropy_bonus_threshold: 0.80,
            mesa_entropy_max_wash: 0.50,

            // SurvivorScore integration (from original constants)
            survivor_critical_threshold: 35,
            qass_secondary_max_adjustment: 10,
            qass_min_confidence_for_modifier: 0.6,
            cold_start_max_adjustment: 0.3,
            cold_start_qedd_mci_weight: 10.0,
        }
    }
}

impl ScoringWeights {
    /// Load scoring weights from GhostBrainConfig
    ///
    /// This allows runtime configuration of all scoring parameters
    /// without recompilation.
    ///
    /// If the [scoring] section is missing from the config, uses defaults.
    /// This ensures backward compatibility with existing configs.
    pub fn from_config(config: &GhostBrainConfig) -> Self {
        if let Some(ref scoring_config) = config.scoring {
            // Load from config
            let mut weights = Self::default();

            // Signal weights
            weights.ligma = scoring_config.ligma;
            weights.qedd = scoring_config.qedd;
            weights.survivor = scoring_config.survivor;
            weights.qass_secondary = scoring_config.qass_secondary;
            weights.mci = scoring_config.mci;
            weights.cluster = scoring_config.cluster;
            weights.chaos = scoring_config.chaos;

            // Penalty multipliers
            weights.wash_penalty_mult = scoring_config.wash_penalty_mult;
            weights.bot_penalty_mult = scoring_config.bot_penalty_mult;
            weights.rug_penalty_mult = scoring_config.rug_penalty_mult;
            weights.cluster_penalty_mult = scoring_config.cluster_penalty_mult;
            weights.ssmi_bot_penalty_mult = scoring_config.ssmi_bot_penalty_mult;
            weights.scr_penalty_mult = scoring_config.scr_penalty_mult;
            weights.ulvf_div_penalty_mult = scoring_config.ulvf_div_penalty_mult;
            weights.ulvf_curl_penalty_mult = scoring_config.ulvf_curl_penalty_mult;
            weights.povc_penalty_mult = scoring_config.povc_penalty_mult;
            weights.povc_organic_boost_mult = scoring_config.povc_organic_boost_mult;
            weights.mpcf_sniper_penalty_mult = scoring_config.mpcf_sniper_penalty_mult;
            weights.mpcf_sybil_penalty_mult = scoring_config.mpcf_sybil_penalty_mult;

            // Boost multipliers
            weights.organic_boost_mult = scoring_config.organic_boost_mult;
            weights.smart_money_boost_mult = scoring_config.smart_money_boost_mult;
            weights.ssmi_viral_boost_mult = scoring_config.ssmi_viral_boost_mult;
            weights.ssmi_human_boost_mult = scoring_config.ssmi_human_boost_mult;
            weights.mesa_organic_boost_mult = scoring_config.mesa_organic_boost_mult;
            weights.mesa_entropy_boost_mult = scoring_config.mesa_entropy_boost_mult;
            weights.chaos_pump_boost_mult = scoring_config.chaos_pump_boost_mult;
            weights.resonance_human_boost_mult = scoring_config.resonance_human_boost_mult;
            weights.cluster_clean_boost_mult = scoring_config.cluster_clean_boost_mult;

            // Thresholds and scales remain from HyperPredictionConfig
            // (they are already loaded there, no duplication needed)

            weights
        } else {
            // No [scoring] section in config - use defaults for backward compatibility
            Self::default()
        }
    }

    /// Validate weight ranges
    ///
    /// Ensures all weights are within reasonable bounds to prevent
    /// configuration errors from causing extreme scoring behavior.
    pub fn validate(&self) -> Result<(), String> {
        // Check penalty multipliers are non-negative
        if self.wash_penalty_mult < 0.0 {
            return Err("wash_penalty_mult must be non-negative".to_string());
        }
        if self.bot_penalty_mult < 0.0 {
            return Err("bot_penalty_mult must be non-negative".to_string());
        }
        if self.rug_penalty_mult < 0.0 {
            return Err("rug_penalty_mult must be non-negative".to_string());
        }
        if self.cluster_penalty_mult < 0.0 {
            return Err("cluster_penalty_mult must be non-negative".to_string());
        }

        // Check boost multipliers are non-negative
        if self.organic_boost_mult < 0.0 {
            return Err("organic_boost_mult must be non-negative".to_string());
        }
        if self.smart_money_boost_mult < 0.0 {
            return Err("smart_money_boost_mult must be non-negative".to_string());
        }

        // Check normalization scales are positive
        if self.volume_scale <= 0.0 {
            return Err("volume_scale must be positive".to_string());
        }
        if self.liquidity_scale <= 0.0 {
            return Err("liquidity_scale must be positive".to_string());
        }
        if self.relative_factor_cap <= 0.0 {
            return Err("relative_factor_cap must be positive".to_string());
        }
        if self.burst_normalization <= 0.0 {
            return Err("burst_normalization must be positive".to_string());
        }

        // Check thresholds are in valid ranges [0, 1] for probabilities
        if self.mesa_wash_severe_threshold < 0.0 || self.mesa_wash_severe_threshold > 1.0 {
            return Err("mesa_wash_severe_threshold must be between 0 and 1".to_string());
        }
        if self.mesa_wash_elevated_threshold < 0.0 || self.mesa_wash_elevated_threshold > 1.0 {
            return Err("mesa_wash_elevated_threshold must be between 0 and 1".to_string());
        }

        // Check QASS adjustment is reasonable
        if self.qass_secondary_max_adjustment < 0 || self.qass_secondary_max_adjustment > 50 {
            return Err("qass_secondary_max_adjustment must be between 0 and 50".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_weights_are_valid() {
        let weights = ScoringWeights::default();
        assert!(weights.validate().is_ok());
    }

    #[test]
    fn test_negative_penalty_mult_fails_validation() {
        let mut weights = ScoringWeights::default();
        weights.wash_penalty_mult = -0.5;
        assert!(weights.validate().is_err());
    }

    #[test]
    fn test_zero_volume_scale_fails_validation() {
        let mut weights = ScoringWeights::default();
        weights.volume_scale = 0.0;
        assert!(weights.validate().is_err());
    }

    #[test]
    fn test_invalid_threshold_fails_validation() {
        let mut weights = ScoringWeights::default();
        weights.mesa_wash_severe_threshold = 1.5; // > 1.0
        assert!(weights.validate().is_err());
    }

    #[test]
    fn test_extreme_qass_adjustment_fails_validation() {
        let mut weights = ScoringWeights::default();
        weights.qass_secondary_max_adjustment = 100;
        assert!(weights.validate().is_err());
    }
}
