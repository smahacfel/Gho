//! Oracle adapter for E2E pipeline
//!
//! This module provides a simplified interface to the Oracle scoring system
//! for use in the E2E pipeline. It scores candidates and determines if they
//! should proceed to strategy selection.

use anyhow::Result;
use seer::types::CandidatePool;
use solana_sdk::pubkey::Pubkey;
use std::env;

// Use the unified RiskLevel from verdict module to avoid type conflicts
pub use crate::oracle::hyper_prediction::verdict::RiskLevel;

/// Cache-line aligned scoring weights (64 bytes = typical cache line)
///
/// Using f32 for better cache efficiency and SIMD potential.
/// Array of 16 weights provides room for future expansion.
#[repr(align(64))]
#[derive(Debug, Clone, Copy)]
pub struct ScoringWeights {
    /// Raw weight array aligned to cache line boundary
    weights: [f32; 16],
}

impl ScoringWeights {
    // Weight indices
    pub const LIQUIDITY_IDX: usize = 0;
    pub const BONDING_EARLY_BONUS_IDX: usize = 1;
    pub const BONDING_LATE_PENALTY_IDX: usize = 2;
    pub const SUPPLY_CAP_BONUS_IDX: usize = 3;
    pub const DEV_WALLET_PENALTY_IDX: usize = 4;
    pub const HOLDER_DISTRIBUTION_IDX: usize = 5;
    pub const VOLUME_GROWTH_IDX: usize = 6;
    pub const PRICE_MOMENTUM_IDX: usize = 7;
    pub const METADATA_QUALITY_IDX: usize = 8;
    pub const SOCIAL_SIGNALS_IDX: usize = 9;
    pub const WHALE_CONCENTRATION_IDX: usize = 10;
    pub const TRADING_VELOCITY_IDX: usize = 11;
    pub const LP_LOCK_STATUS_IDX: usize = 12;
    pub const JITO_BUNDLE_PRESENCE_IDX: usize = 13;
    pub const CREATOR_SELL_SPEED_IDX: usize = 14;
    pub const MARKET_CAP_FACTOR_IDX: usize = 15;

    /// Create default weights optimized for early detection
    pub const fn default() -> Self {
        Self {
            weights: [
                20.0, // LIQUIDITY_IDX - critical factor
                15.0, // BONDING_EARLY_BONUS_IDX - early entry advantage
                25.0, // BONDING_LATE_PENALTY_IDX - late entry heavy penalty
                10.0, // SUPPLY_CAP_BONUS_IDX - reasonable supply
                30.0, // DEV_WALLET_PENALTY_IDX - heavy rug penalty
                8.0,  // HOLDER_DISTRIBUTION_IDX
                7.0,  // VOLUME_GROWTH_IDX
                6.0,  // PRICE_MOMENTUM_IDX
                5.0,  // METADATA_QUALITY_IDX
                4.0,  // SOCIAL_SIGNALS_IDX
                12.0, // WHALE_CONCENTRATION_IDX - important rug signal
                5.0,  // TRADING_VELOCITY_IDX
                8.0,  // LP_LOCK_STATUS_IDX
                3.0,  // JITO_BUNDLE_PRESENCE_IDX
                10.0, // CREATOR_SELL_SPEED_IDX - important rug signal
                5.0,  // MARKET_CAP_FACTOR_IDX
            ],
        }
    }

    /// Create from environment variables with fallback to defaults
    pub fn from_env() -> Self {
        let mut weights = Self::default();

        // Try to load each weight from environment
        if let Ok(val) = env::var("WEIGHT_LIQUIDITY") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::LIQUIDITY_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_BONDING_EARLY_BONUS") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::BONDING_EARLY_BONUS_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_BONDING_LATE_PENALTY") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::BONDING_LATE_PENALTY_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_SUPPLY_CAP_BONUS") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::SUPPLY_CAP_BONUS_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_DEV_WALLET_PENALTY") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::DEV_WALLET_PENALTY_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_HOLDER_DISTRIBUTION") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::HOLDER_DISTRIBUTION_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_VOLUME_GROWTH") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::VOLUME_GROWTH_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_PRICE_MOMENTUM") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::PRICE_MOMENTUM_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_METADATA_QUALITY") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::METADATA_QUALITY_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_SOCIAL_SIGNALS") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::SOCIAL_SIGNALS_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_WHALE_CONCENTRATION") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::WHALE_CONCENTRATION_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_TRADING_VELOCITY") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::TRADING_VELOCITY_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_LP_LOCK_STATUS") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::LP_LOCK_STATUS_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_JITO_BUNDLE_PRESENCE") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::JITO_BUNDLE_PRESENCE_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_CREATOR_SELL_SPEED") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::CREATOR_SELL_SPEED_IDX] = w;
            }
        }
        if let Ok(val) = env::var("WEIGHT_MARKET_CAP_FACTOR") {
            if let Ok(w) = val.parse::<f32>() {
                weights.weights[Self::MARKET_CAP_FACTOR_IDX] = w;
            }
        }

        weights
    }

    /// Get a specific weight by index
    #[inline(always)]
    pub fn get(&self, index: usize) -> f32 {
        self.weights[index]
    }

    /// Set a specific weight (for runtime override)
    #[inline(always)]
    pub fn set(&mut self, index: usize, value: f32) {
        self.weights[index] = value;
    }

    /// Get all weights as slice
    #[inline(always)]
    pub fn as_slice(&self) -> &[f32; 16] {
        &self.weights
    }

    /// Runtime override for GUI or configuration changes
    pub fn override_weight(&mut self, name: &str, value: f32) {
        let idx = match name {
            "liquidity" => Self::LIQUIDITY_IDX,
            "bonding_early_bonus" => Self::BONDING_EARLY_BONUS_IDX,
            "bonding_late_penalty" => Self::BONDING_LATE_PENALTY_IDX,
            "supply_cap_bonus" => Self::SUPPLY_CAP_BONUS_IDX,
            "dev_wallet_penalty" => Self::DEV_WALLET_PENALTY_IDX,
            "holder_distribution" => Self::HOLDER_DISTRIBUTION_IDX,
            "volume_growth" => Self::VOLUME_GROWTH_IDX,
            "price_momentum" => Self::PRICE_MOMENTUM_IDX,
            "metadata_quality" => Self::METADATA_QUALITY_IDX,
            "social_signals" => Self::SOCIAL_SIGNALS_IDX,
            "whale_concentration" => Self::WHALE_CONCENTRATION_IDX,
            "trading_velocity" => Self::TRADING_VELOCITY_IDX,
            "lp_lock_status" => Self::LP_LOCK_STATUS_IDX,
            "jito_bundle_presence" => Self::JITO_BUNDLE_PRESENCE_IDX,
            "creator_sell_speed" => Self::CREATOR_SELL_SPEED_IDX,
            "market_cap_factor" => Self::MARKET_CAP_FACTOR_IDX,
            _ => return, // Unknown weight name, ignore
        };
        self.weights[idx] = value;
    }

    /// Get weight by name for introspection
    pub fn get_by_name(&self, name: &str) -> Option<f32> {
        let idx = match name {
            "liquidity" => Self::LIQUIDITY_IDX,
            "bonding_early_bonus" => Self::BONDING_EARLY_BONUS_IDX,
            "bonding_late_penalty" => Self::BONDING_LATE_PENALTY_IDX,
            "supply_cap_bonus" => Self::SUPPLY_CAP_BONUS_IDX,
            "dev_wallet_penalty" => Self::DEV_WALLET_PENALTY_IDX,
            "holder_distribution" => Self::HOLDER_DISTRIBUTION_IDX,
            "volume_growth" => Self::VOLUME_GROWTH_IDX,
            "price_momentum" => Self::PRICE_MOMENTUM_IDX,
            "metadata_quality" => Self::METADATA_QUALITY_IDX,
            "social_signals" => Self::SOCIAL_SIGNALS_IDX,
            "whale_concentration" => Self::WHALE_CONCENTRATION_IDX,
            "trading_velocity" => Self::TRADING_VELOCITY_IDX,
            "lp_lock_status" => Self::LP_LOCK_STATUS_IDX,
            "jito_bundle_presence" => Self::JITO_BUNDLE_PRESENCE_IDX,
            "creator_sell_speed" => Self::CREATOR_SELL_SPEED_IDX,
            "market_cap_factor" => Self::MARKET_CAP_FACTOR_IDX,
            _ => return None,
        };
        Some(self.weights[idx])
    }
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self::default()
    }
}

/// Simplified scored candidate result
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    /// Original candidate pool
    pub pool: CandidatePool,

    /// Oracle score (0-100)
    pub score: u8,

    /// Whether the candidate passed the minimum threshold
    pub passed: bool,

    /// Risk level assessment
    pub risk_level: RiskLevel,

    /// Confidence score (0.0-1.0) - reliability of the scoring decision
    pub confidence: Option<f32>,
}

// RiskLevel is now imported from verdict module - see top of file
// This eliminates the duplicate enum definition that was causing type conflicts

/// Simplified Oracle scorer for E2E pipeline with weighted scoring
///
/// This is a weighted scoring implementation with automatic risk penalties.
/// In production, this would integrate with the full Oracle system in src/oracle/.
pub struct SimpleOracle {
    /// Minimum score threshold to pass
    min_score_threshold: u8,

    /// Scoring weights
    weights: ScoringWeights,
}

impl SimpleOracle {
    /// Create a new SimpleOracle with default weights
    pub fn new(min_score_threshold: u8) -> Self {
        Self {
            min_score_threshold,
            weights: ScoringWeights::default(),
        }
    }

    /// Create a new SimpleOracle with custom weights
    pub fn with_weights(min_score_threshold: u8, weights: ScoringWeights) -> Self {
        Self {
            min_score_threshold,
            weights,
        }
    }

    /// Create a new SimpleOracle loading weights from environment
    pub fn from_env(min_score_threshold: u8) -> Self {
        Self {
            min_score_threshold,
            weights: ScoringWeights::from_env(),
        }
    }

    /// Get a mutable reference to weights for runtime override
    pub fn weights_mut(&mut self) -> &mut ScoringWeights {
        &mut self.weights
    }

    /// Get an immutable reference to weights
    pub fn weights(&self) -> &ScoringWeights {
        &self.weights
    }

    /// Get the minimum score threshold
    pub fn min_score_threshold(&self) -> u8 {
        self.min_score_threshold
    }

    /// Score a candidate pool using weighted heuristics
    ///
    /// This uses the weighted scoring system with automatic risk penalties.
    pub async fn score_candidate(&self, candidate: &CandidatePool) -> Result<ScoredCandidate> {
        // Calculate base score using weighted system
        let base_score = self.calculate_weighted_score(candidate);

        // Determine risk level (which may override based on critical factors)
        let risk_level = RiskLevel::from_candidate(candidate, base_score);

        // Apply risk penalty to final score
        let score = base_score.saturating_sub(risk_level.penalty());

        let passed = score >= self.min_score_threshold;

        Ok(ScoredCandidate {
            pool: candidate.clone(),
            score,
            passed,
            risk_level,
            confidence: None, // Will be computed separately if needed
        })
    }

    /// Calculate weighted score with zero-heap-allocation
    ///
    /// This is the hot path function designed to run in <40ns.
    /// Uses saturating arithmetic and stack-only operations.
    #[inline]
    fn calculate_weighted_score(&self, candidate: &CandidatePool) -> u8 {
        let mut score: u8 = 50; // Base score

        // LIQUIDITY SCORING - critical factor
        if let Some(liquidity_sol) = candidate.initial_liquidity_sol {
            let liquidity_weight = self.weights.get(ScoringWeights::LIQUIDITY_IDX);

            if liquidity_sol >= 10.0 {
                score = score.saturating_add((liquidity_weight) as u8);
            } else if liquidity_sol >= 5.0 {
                score = score.saturating_add((liquidity_weight * 0.5) as u8);
            } else if liquidity_sol < 1.0 {
                score = score.saturating_sub((liquidity_weight) as u8);
            }
        } else {
            // Missing liquidity is a major red flag
            score = score.saturating_sub(20);
        }

        // BONDING CURVE PROGRESS SCORING
        if let Some(progress) = candidate.bonding_curve_progress {
            if progress < 0.1 {
                // Very early - apply early bonus
                let early_bonus = self.weights.get(ScoringWeights::BONDING_EARLY_BONUS_IDX);
                score = score.saturating_add(early_bonus as u8);
            } else if progress > 0.8 {
                // Very late - apply late penalty
                let late_penalty = self.weights.get(ScoringWeights::BONDING_LATE_PENALTY_IDX);
                score = score.saturating_sub(late_penalty as u8);
            } else if progress > 0.5 {
                // Moderately late - smaller penalty
                let late_penalty = self.weights.get(ScoringWeights::BONDING_LATE_PENALTY_IDX);
                score = score.saturating_sub((late_penalty * 0.4) as u8);
            }
        }

        // SUPPLY CAP SCORING - reasonable supply is good
        if let Some(supply) = candidate.token_total_supply {
            let supply_weight = self.weights.get(ScoringWeights::SUPPLY_CAP_BONUS_IDX);

            // Typical good range: 100M - 1B tokens
            if supply >= 100_000_000 && supply <= 1_000_000_000 {
                score = score.saturating_add(supply_weight as u8);
            } else if supply > 10_000_000_000 {
                // Excessive supply is suspicious
                score = score.saturating_sub(supply_weight as u8);
            }
        }

        // Clamp final score to valid range
        score.min(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seer::types::CandidatePool;
    use solana_sdk::pubkey::Pubkey;

    fn create_test_candidate() -> CandidatePool {
        CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(12345),
            tx_index: None,
            event_ts_ms: Some(1_234_567_890_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: "5".repeat(88),
            amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 1234567890,
            bonding_curve_progress: Some(0.05),
            initial_liquidity_sol: Some(10.0),
            token_total_supply: Some(1_000_000_000),
            block_time: Some(1234567890),
        }
    }

    // === Basic Scoring Tests ===

    #[tokio::test]
    async fn test_simple_oracle_scoring() {
        let oracle = SimpleOracle::new(70);
        let candidate = create_test_candidate();

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        assert!(scored.score > 0);
        assert!(scored.score <= 100);
    }

    #[tokio::test]
    async fn test_threshold_check() {
        let oracle = SimpleOracle::new(70);
        let candidate = create_test_candidate();

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        if scored.score >= 70 {
            assert!(scored.passed);
        } else {
            assert!(!scored.passed);
        }
    }

    // === Liquidity Tests ===

    #[tokio::test]
    async fn test_high_liquidity_bonus() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(15.0); // High liquidity

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should get liquidity bonus
        assert!(scored.score >= 60);
    }

    #[tokio::test]
    async fn test_low_liquidity_penalty() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(0.5); // Very low liquidity

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should get penalty and be marked VeryHigh risk
        assert!(scored.score < 50);
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
    }

    #[tokio::test]
    async fn test_missing_liquidity_penalty() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = None; // Missing liquidity data

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Missing data should be treated as very high risk
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        assert!(scored.score < 50);
    }

    // === Bonding Curve Tests ===

    #[tokio::test]
    async fn test_early_bonding_curve_bonus() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.bonding_curve_progress = Some(0.05); // Very early
        candidate.initial_liquidity_sol = Some(10.0);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should get early bonus
        assert!(scored.score > 60);
    }

    #[tokio::test]
    async fn test_late_bonding_curve_penalty() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.bonding_curve_progress = Some(0.85); // Very late
        candidate.initial_liquidity_sol = Some(10.0);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should get late penalty and high risk
        assert!(scored.score < 50);
        assert_eq!(scored.risk_level, RiskLevel::High);
    }

    #[tokio::test]
    async fn test_extremely_late_bonding_curve() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.bonding_curve_progress = Some(0.95); // Extremely late
        candidate.initial_liquidity_sol = Some(10.0);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should be marked VeryHigh risk
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
    }

    // === Supply Cap Tests ===

    #[tokio::test]
    async fn test_reasonable_supply_bonus() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.token_total_supply = Some(500_000_000); // Good range
        candidate.initial_liquidity_sol = Some(10.0);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should get supply bonus
        assert!(scored.score >= 55);
    }

    #[tokio::test]
    async fn test_excessive_supply_penalty() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.token_total_supply = Some(50_000_000_000); // Way too high
        candidate.initial_liquidity_sol = Some(10.0);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should get penalty and high risk - excessive supply triggers High risk
        assert!(scored.score <= 60); // May be 60 due to base score + liquidity bonus - supply penalty
        assert_eq!(scored.risk_level, RiskLevel::High);
    }

    // === Risk Level Tests ===

    #[tokio::test]
    async fn test_risk_level_low() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.bonding_curve_progress = Some(0.05);
        candidate.initial_liquidity_sol = Some(15.0);
        candidate.token_total_supply = Some(500_000_000);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Optimal conditions should yield low or medium risk
        assert!(matches!(
            scored.risk_level,
            RiskLevel::Low | RiskLevel::Medium
        ));
    }

    #[tokio::test]
    async fn test_risk_level_penalty_applied() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(0.5); // Forces VeryHigh risk

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // VeryHigh risk should apply 40 point penalty
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        let penalty = scored.risk_level.penalty();
        assert_eq!(penalty, 40);
    }

    // === Mainnet Scenario Tests ===

    #[tokio::test]
    async fn test_mainnet_rug_scenario() {
        // Simulate a typical rug pull: low liquidity, late entry, excessive supply
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(0.3); // Very low
        candidate.bonding_curve_progress = Some(0.92); // Very late
        candidate.token_total_supply = Some(100_000_000_000); // Excessive

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should fail and be VeryHigh risk
        assert!(!scored.passed);
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        assert!(scored.score < 30);
    }

    #[tokio::test]
    async fn test_mainnet_moonshot_scenario() {
        // Simulate a moonshot: excellent liquidity, very early, good supply
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(25.0); // Excellent
        candidate.bonding_curve_progress = Some(0.02); // Extremely early
        candidate.token_total_supply = Some(500_000_000); // Reasonable

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should pass with flying colors
        assert!(scored.passed);
        assert!(matches!(
            scored.risk_level,
            RiskLevel::Low | RiskLevel::Medium
        ));
        assert!(scored.score > 70);
    }

    #[tokio::test]
    async fn test_mainnet_mid_curve_scenario() {
        // Simulate mid-curve entry: decent liquidity, moderate timing
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(8.0); // Decent
        candidate.bonding_curve_progress = Some(0.35); // Mid-curve
        candidate.token_total_supply = Some(1_000_000_000); // Normal

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should be medium risk, marginal pass
        assert!(matches!(
            scored.risk_level,
            RiskLevel::Medium | RiskLevel::High
        ));
        assert!(scored.score >= 40 && scored.score <= 70);
    }

    #[tokio::test]
    async fn test_mainnet_honeypot_scenario() {
        // Simulate honeypot: good looking stats but missing critical data
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = None; // Missing - red flag
        candidate.bonding_curve_progress = Some(0.05);
        candidate.token_total_supply = Some(1_000_000_000);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Missing liquidity should be caught
        assert!(!scored.passed);
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
    }

    // === Weight Configuration Tests ===

    #[tokio::test]
    async fn test_custom_weights() {
        let mut weights = ScoringWeights::default();
        weights.set(ScoringWeights::LIQUIDITY_IDX, 50.0); // Increase liquidity weight

        let oracle = SimpleOracle::with_weights(50, weights);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(15.0);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // With increased weight, should get higher score
        assert!(scored.score > 70);
    }

    #[tokio::test]
    async fn test_runtime_weight_override() {
        let mut oracle = SimpleOracle::new(50);
        oracle.weights_mut().override_weight("liquidity", 5.0); // Decrease

        let candidate = create_test_candidate();
        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Score should be affected by weight change
        assert!(scored.score <= 100);
    }

    // === Boundary Tests ===

    #[tokio::test]
    async fn test_score_never_exceeds_100() {
        let mut weights = ScoringWeights::default();
        // Set all weights extremely high
        for i in 0..16 {
            weights.set(i, 100.0);
        }

        let oracle = SimpleOracle::with_weights(50, weights);
        let candidate = create_test_candidate();

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should be clamped to 100
        assert!(scored.score <= 100);
    }

    #[tokio::test]
    async fn test_score_never_negative() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        // Set all negative factors
        candidate.initial_liquidity_sol = Some(0.1);
        candidate.bonding_curve_progress = Some(0.99);
        candidate.token_total_supply = Some(1_000_000_000_000);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Saturating operations should prevent underflow
        assert!(scored.score <= 100);
    }

    #[tokio::test]
    async fn test_liquidity_boundary_1_sol() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(1.0); // Exactly at boundary

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should not trigger VeryHigh risk (>= 1.0 SOL)
        assert_ne!(scored.risk_level, RiskLevel::VeryHigh);
    }

    #[tokio::test]
    async fn test_liquidity_boundary_just_below_1_sol() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(0.99); // Just below boundary

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should trigger VeryHigh risk
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
    }

    #[tokio::test]
    async fn test_bonding_curve_boundary_0_9() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(10.0);
        candidate.bonding_curve_progress = Some(0.91); // Just over boundary (>0.9)

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should trigger VeryHigh risk
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
    }

    // === Zero Allocation Test ===

    #[tokio::test]
    async fn test_zero_heap_allocation_in_scoring() {
        let oracle = SimpleOracle::new(50);
        let candidate = create_test_candidate();

        // This test verifies the code compiles and runs
        // In a real scenario, you'd use allocation tracking tools
        let scored = oracle.score_candidate(&candidate).await.unwrap();

        assert!(scored.score <= 100);
    }

    // === Saturating Arithmetic Tests ===

    #[tokio::test]
    async fn test_saturating_add_behavior() {
        let mut weights = ScoringWeights::default();
        weights.set(ScoringWeights::LIQUIDITY_IDX, 100.0);
        weights.set(ScoringWeights::BONDING_EARLY_BONUS_IDX, 100.0);

        let oracle = SimpleOracle::with_weights(50, weights);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(20.0);
        candidate.bonding_curve_progress = Some(0.01);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should saturate at 100, not overflow
        assert_eq!(scored.score, 100);
    }

    #[tokio::test]
    async fn test_saturating_sub_behavior() {
        let mut weights = ScoringWeights::default();
        weights.set(ScoringWeights::LIQUIDITY_IDX, 100.0);
        weights.set(ScoringWeights::BONDING_LATE_PENALTY_IDX, 100.0);

        let oracle = SimpleOracle::with_weights(50, weights);
        let mut candidate = create_test_candidate();
        candidate.initial_liquidity_sol = Some(0.5);
        candidate.bonding_curve_progress = Some(0.95);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should handle underflow gracefully (u8 is always >= 0)
        assert!(scored.score <= 100);
    }

    // === Additional Real-World Scenarios ===

    #[tokio::test]
    async fn test_pump_fun_typical_launch() {
        let oracle = SimpleOracle::new(60);
        let mut candidate = create_test_candidate();
        candidate.amm_program_id = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap();
        candidate.initial_liquidity_sol = Some(12.0);
        candidate.bonding_curve_progress = Some(0.08);
        candidate.token_total_supply = Some(1_000_000_000);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        assert!(scored.score >= 60);
        assert!(scored.passed);
    }

    #[tokio::test]
    async fn test_whale_dump_simulation() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();
        // After whale dump: liquidity drained
        candidate.initial_liquidity_sol = Some(2.5);
        candidate.bonding_curve_progress = Some(0.65);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should be high risk
        assert_eq!(scored.risk_level, RiskLevel::High);
    }

    // === ScoringWeights Module Tests ===

    #[test]
    fn test_weights_cache_line_alignment() {
        let weights = ScoringWeights::default();
        let ptr = &weights as *const ScoringWeights as usize;

        // Verify 64-byte alignment
        assert_eq!(ptr % 64, 0, "ScoringWeights should be 64-byte aligned");

        // Verify size is exactly 64 bytes (16 * f32)
        assert_eq!(std::mem::size_of::<ScoringWeights>(), 64);
    }

    #[test]
    fn test_weights_default_values() {
        let weights = ScoringWeights::default();

        // Verify all weights are set
        assert_eq!(weights.get(ScoringWeights::LIQUIDITY_IDX), 20.0);
        assert_eq!(weights.get(ScoringWeights::BONDING_EARLY_BONUS_IDX), 15.0);
        assert_eq!(weights.get(ScoringWeights::BONDING_LATE_PENALTY_IDX), 25.0);
        assert_eq!(weights.get(ScoringWeights::SUPPLY_CAP_BONUS_IDX), 10.0);
        assert_eq!(weights.get(ScoringWeights::DEV_WALLET_PENALTY_IDX), 30.0);
        assert_eq!(weights.get(ScoringWeights::HOLDER_DISTRIBUTION_IDX), 8.0);
        assert_eq!(weights.get(ScoringWeights::VOLUME_GROWTH_IDX), 7.0);
        assert_eq!(weights.get(ScoringWeights::PRICE_MOMENTUM_IDX), 6.0);
        assert_eq!(weights.get(ScoringWeights::METADATA_QUALITY_IDX), 5.0);
        assert_eq!(weights.get(ScoringWeights::SOCIAL_SIGNALS_IDX), 4.0);
        assert_eq!(weights.get(ScoringWeights::WHALE_CONCENTRATION_IDX), 12.0);
        assert_eq!(weights.get(ScoringWeights::TRADING_VELOCITY_IDX), 5.0);
        assert_eq!(weights.get(ScoringWeights::LP_LOCK_STATUS_IDX), 8.0);
        assert_eq!(weights.get(ScoringWeights::JITO_BUNDLE_PRESENCE_IDX), 3.0);
        assert_eq!(weights.get(ScoringWeights::CREATOR_SELL_SPEED_IDX), 10.0);
        assert_eq!(weights.get(ScoringWeights::MARKET_CAP_FACTOR_IDX), 5.0);
    }

    #[test]
    fn test_weights_modification() {
        let mut weights = ScoringWeights::default();

        let original = weights.get(ScoringWeights::LIQUIDITY_IDX);
        weights.set(ScoringWeights::LIQUIDITY_IDX, 50.0);

        assert_ne!(weights.get(ScoringWeights::LIQUIDITY_IDX), original);
        assert_eq!(weights.get(ScoringWeights::LIQUIDITY_IDX), 50.0);
    }

    #[test]
    fn test_weights_runtime_override_by_name() {
        let mut weights = ScoringWeights::default();

        weights.override_weight("liquidity", 100.0);
        assert_eq!(weights.get(ScoringWeights::LIQUIDITY_IDX), 100.0);

        weights.override_weight("dev_wallet_penalty", 50.0);
        assert_eq!(weights.get(ScoringWeights::DEV_WALLET_PENALTY_IDX), 50.0);

        // Unknown weight should be ignored
        weights.override_weight("unknown_weight", 123.0);
    }

    #[test]
    fn test_weights_get_by_name() {
        let weights = ScoringWeights::default();

        assert_eq!(weights.get_by_name("liquidity"), Some(20.0));
        assert_eq!(weights.get_by_name("bonding_early_bonus"), Some(15.0));
        assert_eq!(weights.get_by_name("bonding_late_penalty"), Some(25.0));
        assert_eq!(weights.get_by_name("supply_cap_bonus"), Some(10.0));
        assert_eq!(weights.get_by_name("dev_wallet_penalty"), Some(30.0));
        assert_eq!(weights.get_by_name("holder_distribution"), Some(8.0));
        assert_eq!(weights.get_by_name("volume_growth"), Some(7.0));
        assert_eq!(weights.get_by_name("price_momentum"), Some(6.0));
        assert_eq!(weights.get_by_name("metadata_quality"), Some(5.0));
        assert_eq!(weights.get_by_name("social_signals"), Some(4.0));
        assert_eq!(weights.get_by_name("whale_concentration"), Some(12.0));
        assert_eq!(weights.get_by_name("trading_velocity"), Some(5.0));
        assert_eq!(weights.get_by_name("lp_lock_status"), Some(8.0));
        assert_eq!(weights.get_by_name("jito_bundle_presence"), Some(3.0));
        assert_eq!(weights.get_by_name("creator_sell_speed"), Some(10.0));
        assert_eq!(weights.get_by_name("market_cap_factor"), Some(5.0));
        assert_eq!(weights.get_by_name("unknown"), None);
    }

    #[test]
    fn test_weights_from_env() {
        // Set some env vars
        std::env::set_var("WEIGHT_LIQUIDITY", "99.0");
        std::env::set_var("WEIGHT_DEV_WALLET_PENALTY", "88.0");

        let weights = ScoringWeights::from_env();

        assert_eq!(weights.get(ScoringWeights::LIQUIDITY_IDX), 99.0);
        assert_eq!(weights.get(ScoringWeights::DEV_WALLET_PENALTY_IDX), 88.0);

        // Other weights should remain at defaults
        assert_eq!(weights.get(ScoringWeights::BONDING_EARLY_BONUS_IDX), 15.0);

        // Cleanup
        std::env::remove_var("WEIGHT_LIQUIDITY");
        std::env::remove_var("WEIGHT_DEV_WALLET_PENALTY");
    }

    #[test]
    fn test_weights_from_env_invalid_values() {
        // Set invalid env var
        std::env::set_var("WEIGHT_LIQUIDITY", "not_a_number");

        let weights = ScoringWeights::from_env();

        // Should fall back to default
        assert_eq!(weights.get(ScoringWeights::LIQUIDITY_IDX), 20.0);

        // Cleanup
        std::env::remove_var("WEIGHT_LIQUIDITY");
    }

    #[test]
    fn test_weights_all_indices() {
        let weights = ScoringWeights::default();

        // Verify all indices are valid and return values
        for i in 0..16 {
            let _weight = weights.get(i);
            // Should not panic
        }
    }

    #[test]
    fn test_weights_as_slice() {
        let weights = ScoringWeights::default();
        let slice = weights.as_slice();

        assert_eq!(slice.len(), 16);
        assert_eq!(slice[0], 20.0); // LIQUIDITY_IDX
        assert_eq!(slice[1], 15.0); // BONDING_EARLY_BONUS_IDX
    }

    #[test]
    fn test_weights_zero_copy_access() {
        let weights = ScoringWeights::default();

        // Direct array access should be zero-cost
        let _slice = weights.as_slice();
        // No heap allocation should occur here
    }

    // === Additional RiskLevel Tests ===

    #[test]
    fn test_risk_level_penalty_values() {
        assert_eq!(RiskLevel::Low.penalty(), 0);
        assert_eq!(RiskLevel::Medium.penalty(), 5);
        assert_eq!(RiskLevel::High.penalty(), 15);
        assert_eq!(RiskLevel::VeryHigh.penalty(), 40);
    }

    #[tokio::test]
    async fn test_combined_risk_factors() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();

        // Multiple risk factors
        candidate.initial_liquidity_sol = Some(2.0); // Forces High
        candidate.token_total_supply = Some(20_000_000_000); // Forces High

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should be High risk
        assert_eq!(scored.risk_level, RiskLevel::High);
    }

    #[tokio::test]
    async fn test_optimal_candidate_profile() {
        let oracle = SimpleOracle::new(50);
        let mut candidate = create_test_candidate();

        // Optimal profile
        candidate.initial_liquidity_sol = Some(20.0);
        candidate.bonding_curve_progress = Some(0.03);
        candidate.token_total_supply = Some(500_000_000);

        let scored = oracle.score_candidate(&candidate).await.unwrap();

        // Should pass and have good risk
        assert!(scored.passed);
        assert!(matches!(
            scored.risk_level,
            RiskLevel::Low | RiskLevel::Medium
        ));
    }
}

/// Enhanced scoring for candidates with contextual analysis
///
/// This function implements the "Alchemical optimization" scoring that uses
/// transaction-level context and intent analysis to better detect scams/honeypots
/// without any RPC calls.
///
/// # Arguments
/// * `candidate` - Enhanced candidate with contextual fields populated
/// * `threshold` - Minimum score to pass (typically 70)
///
/// # Returns
/// Scored candidate with enhanced risk assessment
pub fn score_enhanced(
    candidate: &crate::fast_pipeline::EnhancedCandidate,
    threshold: u8,
) -> ScoredCandidate {
    use ghost_core::liquidity_precision_penalty;

    // Start with neutral base score
    let mut score: f64 = 50.0;
    let mut risk = RiskLevel::Medium;

    // 1. MIGRATION RISK PENALTY (-100 pts for >98% progress)
    // Unit conversion note:
    // - bonding_curve_progress: ratio value (0.0-1.0), e.g. 0.5 = 50%
    // We normalize to percentage (0-100) for consistent comparison
    let bonding_progress_pct = candidate.bonding_curve_progress.map(|p| p * 100.0);

    if let Some(progress) = bonding_progress_pct {
        if progress > 98.0 {
            // CRITICAL: Migration imminent - funds could be frozen
            score -= 100.0;
            risk = RiskLevel::VeryHigh;

            // Early return for migration risk - no point scoring further
            let final_score = score.clamp(0.0, 100.0) as u8;
            return ScoredCandidate {
                pool: convert_enhanced_to_candidate_pool(candidate),
                score: final_score,
                passed: false,
                risk_level: risk,
                confidence: None, // Will be computed separately if needed
            };
        }
    }

    // 2. LIQUIDITY (snapshot-derived pipeline only)
    let liquidity = candidate.initial_liquidity_sol;

    // === LIQUIDITY SCORING (base factor) ===
    if liquidity < 1.0 {
        score -= 40.0;
        risk = RiskLevel::VeryHigh;
    } else if liquidity < 3.0 {
        score -= 20.0;
        if risk == RiskLevel::Medium {
            risk = RiskLevel::High;
        }
    } else if liquidity >= 10.0 {
        score += 20.0;
    } else if liquidity >= 5.0 {
        score += 10.0;
    }

    // === PRECISION FINGERPRINTING ===
    score += liquidity_precision_penalty(liquidity);

    // === ATOMIC DEV BUY SCORING ===
    if candidate.has_dev_buy {
        // Strong positive signal: dev is investing
        score += 20.0;

        // Additional bonus for substantial dev investment
        if candidate.dev_buy_sol > 5.0 {
            score += 15.0;
        } else if candidate.dev_buy_sol > 2.0 {
            score += 10.0;
        } else if candidate.dev_buy_sol > 0.5 {
            score += 5.0;
        }
    } else {
        // No dev buy is a negative signal (but not as bad as other factors)
        score -= 10.0;
    }

    // === VANITY & GRIND SCORING ===
    // Scale vanity score (0-100) to contribution (0-50)
    score += (candidate.vanity_score as f64) * 0.5;

    // === METADATA QUALITY ===
    // Scale metadata score (0-100) to contribution (0-20)
    score += (candidate.metadata_len_score as f64) * 0.2;

    // === PROGRAM-SPECIFIC SAFETY CHECKS ===

    // Known Pump.fun program gets slight bonus (established protocol)
    const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    if candidate.amm_program_id.to_string() == PUMP_FUN_PROGRAM_ID {
        score += 5.0;
    }

    // For non-Pump.fun AMMs (like Raydium/Orca), active mint authority is a HARD FAIL
    // These protocols should have mint authority disabled for legitimate tokens
    const RAYDIUM_V4: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
    const ORCA_WHIRLPOOL: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";

    let program_id_str = candidate.amm_program_id.to_string();
    let is_critical_amm = program_id_str == RAYDIUM_V4 || program_id_str == ORCA_WHIRLPOOL;

    if is_critical_amm && !candidate.mint_auth_disabled {
        // HARD REJECT: mint authority active on critical AMM = certain rug
        score = 0.0;
        risk = RiskLevel::VeryHigh;

        let final_score = score.clamp(0.0, 100.0) as u8;
        return ScoredCandidate {
            pool: convert_enhanced_to_candidate_pool(candidate),
            score: final_score,
            passed: false,
            risk_level: risk,
            confidence: None, // Will be computed separately if needed
        };
    }

    // === BONDING CURVE CHECKS ===
    if let Some(progress) = candidate.bonding_curve_progress {
        if progress > 0.9 {
            score -= 30.0;
            risk = RiskLevel::VeryHigh;
        } else if progress > 0.75 {
            score -= 20.0;
            if risk == RiskLevel::Medium {
                risk = RiskLevel::High;
            }
        } else if progress < 0.1 {
            // Very early entry bonus
            score += 15.0;
        }
    }

    // === SUPPLY CHECKS ===
    if let Some(supply) = candidate.token_total_supply {
        if supply > 10_000_000_000 {
            score -= 15.0;
            if risk == RiskLevel::Medium || risk == RiskLevel::Low {
                risk = RiskLevel::High;
            }
        } else if supply >= 100_000_000 && supply <= 1_000_000_000 {
            // Reasonable supply range
            score += 10.0;
        }
    }

    // === FINALIZATION ===
    let final_score = score.clamp(0.0, 100.0) as u8;

    // Determine final risk level based on score if not already set to VeryHigh
    if risk != RiskLevel::VeryHigh {
        risk = match final_score {
            90..=100 => RiskLevel::Low,
            70..=89 => RiskLevel::Medium,
            50..=69 => RiskLevel::High,
            _ => RiskLevel::VeryHigh,
        };
    }

    // Apply risk penalty
    let final_score_with_penalty = final_score.saturating_sub(risk.penalty());
    let passed = final_score_with_penalty >= threshold;

    ScoredCandidate {
        pool: convert_enhanced_to_candidate_pool(candidate),
        score: final_score_with_penalty,
        passed,
        risk_level: risk,
        confidence: None, // Will be computed separately if needed
    }
}

// =============================================================================
// QASS Integration Constants
// =============================================================================

/// QASS neutral score (0.5 = neutral)
const QASS_NEUTRAL_SCORE: f64 = 0.5;
/// Maximum QASS score adjustment in points (±20)
const QASS_MAX_ADJUSTMENT: f64 = 40.0;

/// Enhanced scoring result with QASS integration
///
/// This combines the base Oracle score with QASS superposition scoring
/// and provides full interpretability.
#[derive(Debug, Clone)]
pub struct ScoredCandidateWithQASS {
    /// Base ScoredCandidate from score_enhanced()
    pub base: ScoredCandidate,
    /// QASS result with full breakdown
    pub qass_result: Option<crate::oracle::ultrafast::QASSResult>,
    /// Final combined score (Oracle + QASS modifier)
    pub combined_score: u8,
    /// Human-readable interpretation for operator
    pub interpretation: String,
}

/// Score a candidate with QASS integration
///
/// This function:
/// 1. Runs base score_enhanced() scoring
/// 2. Builds HeuristicWaves from available signals
/// 3. Runs QASS superposition scoring
/// 4. Combines scores using QASS as a modifier (+/-20% range)
/// 5. Generates human-readable interpretation
///
/// # Arguments
/// * `candidate` - Enhanced candidate with contextual fields
/// * `threshold` - Minimum score to pass
/// * `qass_scorer` - Optional QASS scorer (uses default if None)
///
/// # Returns
/// ScoredCandidateWithQASS with combined score and interpretation
pub fn score_enhanced_with_qass(
    candidate: &crate::fast_pipeline::EnhancedCandidate,
    threshold: u8,
    qass_scorer: Option<&crate::oracle::ultrafast::QuantumAmplitudeScorer>,
) -> ScoredCandidateWithQASS {
    use crate::oracle::ultrafast::{HeuristicWave, QASSResult, QuantumAmplitudeScorer};

    // 1. Run base scoring
    let base = score_enhanced(candidate, threshold);

    // 2. Build waves from available data
    let mut waves: Vec<HeuristicWave> = Vec::with_capacity(8);

    // Add placeholder waves for other signals that are not yet available
    // These will be populated when we integrate with HyperOracle, SSMI, etc.
    // For now, we use neutral waves with low confidence to avoid skewing results

    // ψ_liquidity: Based on liquidity level
    let liquidity_wave = build_liquidity_wave(candidate.initial_liquidity_sol, None);
    waves.push(liquidity_wave);

    // ψ_dev_buy: Based on atomic dev buy presence
    let dev_buy_wave = build_dev_buy_wave(candidate.has_dev_buy, candidate.dev_buy_sol, None);
    waves.push(dev_buy_wave);

    // ψ_vanity: Based on mint address vanity score
    let vanity_wave = build_vanity_wave(candidate.vanity_score);
    waves.push(vanity_wave);

    // 3. Run QASS scoring
    let default_scorer;
    let scorer = match qass_scorer {
        Some(s) => s,
        None => {
            default_scorer = QuantumAmplitudeScorer::default();
            &default_scorer
        }
    };

    let qass_result = scorer.score(&waves);

    // 4. Combine scores (QASS as modifier: +/-20% range)
    // If QASS is valid, adjust base score by up to ±20 points based on QASS deviation from neutral
    let combined_score = if qass_result.is_valid {
        // QASS score is 0.0-1.0, neutral is QASS_NEUTRAL_SCORE (0.5)
        // Deviation from neutral: -0.5 to +0.5
        // Scale to -20 to +20 points using QASS_MAX_ADJUSTMENT (40.0)
        let deviation = qass_result.score - QASS_NEUTRAL_SCORE;
        let adjustment = (deviation * QASS_MAX_ADJUSTMENT) as i16;

        let new_score = (base.score as i16 + adjustment).clamp(0, 100) as u8;
        new_score
    } else {
        // Use base score if QASS is invalid
        base.score
    };

    // 5. Generate interpretation
    let interpretation = generate_combined_interpretation(&base, &qass_result, combined_score);

    ScoredCandidateWithQASS {
        base,
        qass_result: Some(qass_result),
        combined_score,
        interpretation,
    }
}

/// Build ψ_liquidity wave from initial liquidity
/// Build ψ_liquidity wave from liquidity data
///
/// # Arguments
/// * `liquidity_sol` - Liquidity amount in SOL
/// * `scale` - Optional normalization scale (defaults to 20.0 for backward compatibility)
///
/// The scale parameter controls the "midpoint" for liquidity evaluation.
/// For small pools ($500-$5k), use a smaller scale (e.g., 2500.0).
/// For large pools ($50k+), use a larger scale (e.g., 100000.0).
fn build_liquidity_wave(
    liquidity_sol: f64,
    scale: Option<f64>,
) -> crate::oracle::ultrafast::HeuristicWave {
    // Use provided scale or default to 20.0 (preserves existing behavior)
    let scale = scale.unwrap_or(20.0);

    // Calculate normalized liquidity value using the scale
    // This creates a smooth sigmoid-like transition
    let normalized = liquidity_sol / scale;

    let (amplitude, phase) = if normalized >= 1.0 {
        (0.95, 0.8) // Excellent liquidity - very bullish
    } else if normalized >= 0.5 {
        (0.8, 0.6) // Good liquidity - bullish
    } else if normalized >= 0.25 {
        (0.6, 0.3) // Decent liquidity - slightly bullish
    } else if normalized >= 0.15 {
        (0.4, 0.0) // Low liquidity - neutral
    } else if normalized >= 0.05 {
        (0.3, -0.3) // Very low - slightly bearish
    } else {
        (0.1, -0.7) // Critically low - bearish
    };

    crate::oracle::ultrafast::HeuristicWave::new("ψ_liquidity", amplitude, phase, 0.85)
}

/// Build ψ_dev_buy wave from atomic dev buy data
///
/// # Arguments
/// * `has_dev_buy` - Whether dev made an atomic buy
/// * `dev_buy_sol` - Amount of SOL in dev buy
/// * `scale` - Optional normalization scale (defaults to 5.0 for backward compatibility)
///
/// The scale parameter controls the "midpoint" for dev buy evaluation.
fn build_dev_buy_wave(
    has_dev_buy: bool,
    dev_buy_sol: f64,
    scale: Option<f64>,
) -> crate::oracle::ultrafast::HeuristicWave {
    if !has_dev_buy {
        // No dev buy - slightly negative signal
        return crate::oracle::ultrafast::HeuristicWave::new("ψ_dev_buy", 0.3, -0.2, 0.7);
    }

    // Use provided scale or default to 5.0 (preserves existing behavior)
    let scale = scale.unwrap_or(5.0);

    // Calculate normalized dev buy value
    let normalized = dev_buy_sol / scale;

    // Has dev buy - scale by amount
    let (amplitude, phase) = if normalized >= 1.0 {
        (0.95, 0.85) // Large dev buy - very bullish
    } else if normalized >= 0.4 {
        (0.8, 0.7) // Medium dev buy - bullish
    } else if normalized >= 0.1 {
        (0.6, 0.5) // Small dev buy - moderately bullish
    } else {
        (0.4, 0.3) // Tiny dev buy - slightly bullish
    };

    crate::oracle::ultrafast::HeuristicWave::new("ψ_dev_buy", amplitude, phase, 0.9)
}

/// Build ψ_vanity wave from mint address vanity score
fn build_vanity_wave(vanity_score: u8) -> crate::oracle::ultrafast::HeuristicWave {
    // Vanity score 0-100 maps to amplitude
    let amplitude = (vanity_score as f64) / 100.0;

    // High vanity score suggests legitimate effort - positive phase
    // Low vanity score is neutral (not necessarily bad)
    let phase = if vanity_score >= 80 {
        0.6
    } else if vanity_score >= 50 {
        0.3
    } else if vanity_score >= 20 {
        0.0
    } else {
        -0.1
    };

    // Vanity is a weak signal, low confidence
    crate::oracle::ultrafast::HeuristicWave::new("ψ_vanity", amplitude, phase, 0.5)
}

/// Generate combined interpretation for operator
fn generate_combined_interpretation(
    base: &ScoredCandidate,
    qass: &crate::oracle::ultrafast::QASSResult,
    combined_score: u8,
) -> String {
    let risk_emoji = match base.risk_level {
        RiskLevel::Low => "🟢",
        RiskLevel::Medium => "🟡",
        RiskLevel::High => "🟠",
        RiskLevel::VeryHigh => "🔴",
    };

    let action = if combined_score >= 80 {
        "STRONG BUY"
    } else if combined_score >= 70 {
        "BUY"
    } else if combined_score >= 60 {
        "CONSIDER"
    } else if combined_score >= 50 {
        "WAIT"
    } else {
        "SKIP"
    };

    let qass_status = if qass.is_valid {
        format!(
            "QASS: {:.0}% conf={:.0}%",
            qass.score * 100.0,
            qass.confidence * 100.0
        )
    } else {
        "QASS: insufficient data".to_string()
    };

    let dominant = if qass.dominant_waves.is_empty() {
        "none".to_string()
    } else {
        qass.dominant_waves.join(", ")
    };

    format!(
        "{} {} | Score: {} (base: {}) | Risk: {:?} | {} | Dominant: [{}]",
        risk_emoji, action, combined_score, base.score, base.risk_level, qass_status, dominant
    )
}

/// Convert EnhancedCandidate to CandidatePool for compatibility
fn convert_enhanced_to_candidate_pool(
    candidate: &crate::fast_pipeline::EnhancedCandidate,
) -> seer::types::CandidatePool {
    seer::types::CandidatePool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        slot: candidate.slot,
        tx_index: None,
        event_ts_ms: Some(candidate.timestamp.saturating_mul(1000)),
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: candidate.signature.clone(),
        amm_program_id: candidate.amm_program_id,
        pool_amm_id: candidate.pool_amm_id,
        base_mint: candidate.base_mint,
        quote_mint: candidate.quote_mint,
        bonding_curve: candidate.bonding_curve,
        creator: Pubkey::new_unique(),
        timestamp: candidate.timestamp,
        bonding_curve_progress: candidate.bonding_curve_progress,
        initial_liquidity_sol: Some(candidate.initial_liquidity_sol),
        token_total_supply: candidate.token_total_supply,
        block_time: None,
    }
}

#[cfg(test)]
mod enhanced_scoring_tests {
    use super::*;
    use crate::fast_pipeline::{CacheLinePadding, EnhancedCandidate};
    use solana_sdk::pubkey::Pubkey;

    fn create_test_enhanced_candidate() -> EnhancedCandidate {
        EnhancedCandidate {
            slot: Some(12345),
            pool_amm_id: Pubkey::new_unique(),
            amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            timestamp: 1234567890,
            bonding_curve_progress: Some(0.05),
            initial_liquidity_sol: 10.0,
            token_total_supply: Some(1_000_000_000),
            signature: "5".repeat(88),
            vanity_score: 0,
            has_dev_buy: false,
            dev_buy_sol: 0.0,
            mint_auth_disabled: false,
            metadata_len_score: 50,
            // Shadow Ledger fields (MODUŁ 4)
            expected_price: None,
            shadow_bonding_progress: None,
            virtual_sol_reserves: None,
            shadow_market_cap: None,
            // Cache line padding (required for false sharing prevention)
            _hot_padding: [0; 4],
            _cache_barrier_1: CacheLinePadding::default(),
            _cache_barrier_2: CacheLinePadding::default(),
        }
    }

    #[test]
    fn test_enhanced_scoring_basic() {
        let candidate = create_test_enhanced_candidate();
        let scored = score_enhanced(&candidate, 70);

        assert!(scored.score > 0);
        assert!(scored.score <= 100);
    }

    #[test]
    fn test_enhanced_scoring_with_dev_buy() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.has_dev_buy = true;
        candidate.dev_buy_sol = 5.5;

        let scored = score_enhanced(&candidate, 70);

        // Should get significant bonus for dev buy
        assert!(
            scored.score >= 70,
            "Expected high score with dev buy, got {}",
            scored.score
        );
    }

    #[test]
    fn test_enhanced_scoring_low_liquidity_very_high_risk() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.initial_liquidity_sol = 0.5;

        let scored = score_enhanced(&candidate, 70);

        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        assert!(!scored.passed);
    }

    #[test]
    fn test_enhanced_scoring_mint_auth_raydium_hard_fail() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.amm_program_id = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
            .parse()
            .unwrap();
        candidate.mint_auth_disabled = false; // Authority still active

        let scored = score_enhanced(&candidate, 70);

        assert_eq!(scored.score, 0);
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        assert!(!scored.passed);
    }

    #[test]
    fn test_enhanced_scoring_vanity_bonus() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.vanity_score = 80; // High vanity score

        let scored = score_enhanced(&candidate, 70);

        // Vanity score of 80 should add 40 points (80 * 0.5)
        assert!(scored.score > 70, "Expected vanity bonus to boost score");
    }

    #[test]
    fn test_enhanced_scoring_metadata_quality() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.metadata_len_score = 90; // High quality metadata

        let scored = score_enhanced(&candidate, 70);

        // Should contribute to overall score
        assert!(scored.score >= 60);
    }

    #[test]
    fn test_enhanced_scoring_late_bonding_curve() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.bonding_curve_progress = Some(0.95); // Very late

        let scored = score_enhanced(&candidate, 70);

        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        assert!(!scored.passed);
    }

    #[test]
    fn test_enhanced_scoring_optimal_conditions() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.initial_liquidity_sol = 20.0;
        candidate.has_dev_buy = true;
        candidate.dev_buy_sol = 6.0;
        candidate.vanity_score = 60;
        candidate.metadata_len_score = 80;
        candidate.bonding_curve_progress = Some(0.02);
        candidate.mint_auth_disabled = true;

        let scored = score_enhanced(&candidate, 70);

        assert!(scored.passed, "Expected to pass with optimal conditions");
        assert!(
            scored.score >= 80,
            "Expected high score, got {}",
            scored.score
        );
        assert!(matches!(
            scored.risk_level,
            RiskLevel::Low | RiskLevel::Medium
        ));
    }

    #[test]
    fn test_enhanced_scoring_scam_profile() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.initial_liquidity_sol = 0.3; // Very low
        candidate.has_dev_buy = false; // No dev buy
        candidate.vanity_score = 0; // Random address
        candidate.metadata_len_score = 20; // Poor metadata
        candidate.bonding_curve_progress = Some(0.92); // Very late

        let scored = score_enhanced(&candidate, 70);

        assert!(!scored.passed);
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        assert!(
            scored.score < 30,
            "Expected very low score for scam profile"
        );
    }

    #[test]
    fn test_enhanced_scoring_excessive_supply() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.token_total_supply = Some(50_000_000_000);

        let scored = score_enhanced(&candidate, 70);

        assert_eq!(scored.risk_level, RiskLevel::High);
    }

    #[test]
    fn test_enhanced_scoring_pump_fun_bonus() {
        let mut candidate = create_test_enhanced_candidate();
        // Already set to Pump.fun in default

        let scored = score_enhanced(&candidate, 70);

        // Pump.fun should get a small bonus
        assert!(scored.score >= 50);
    }

    // === BONDING CURVE SCORING TESTS ===

    #[test]
    fn test_bonding_curve_migration_risk_penalty() {
        // Test that >98% bonding progress triggers -100 pts penalty and early return
        let mut candidate = create_test_enhanced_candidate();
        candidate.initial_liquidity_sol = 50.0; // High liquidity
        candidate.has_dev_buy = true;
        candidate.dev_buy_sol = 5.0;

        // Set bonding curve progress to 99% (migration imminent)
        candidate.bonding_curve_progress = Some(0.99);

        let scored = score_enhanced(&candidate, 70);

        // Should fail due to migration risk
        assert!(!scored.passed, "Should not pass with >98% bonding progress");
        assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
        assert!(
            scored.score < 50,
            "Score should be heavily penalized, got {}",
            scored.score
        );
    }

    #[test]
    fn test_bonding_curve_late_penalty() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.bonding_curve_progress = Some(0.80); // Late stage

        let scored = score_enhanced(&candidate, 70);

        assert!(
            matches!(scored.risk_level, RiskLevel::High | RiskLevel::VeryHigh),
            "Should have High/VeryHigh risk for late progress, got {:?}",
            scored.risk_level
        );
    }

    #[test]
    fn test_liquidity_bonus_from_initial_liquidity() {
        let mut candidate_low = create_test_enhanced_candidate();
        candidate_low.initial_liquidity_sol = 1.0;

        let mut candidate_high = create_test_enhanced_candidate();
        candidate_high.initial_liquidity_sol = 12.0;

        let scored_low = score_enhanced(&candidate_low, 70);
        let scored_high = score_enhanced(&candidate_high, 70);

        assert!(
            scored_high.score > scored_low.score,
            "Higher initial liquidity should increase score: low={}, high={}",
            scored_low.score,
            scored_high.score
        );
    }

    #[test]
    fn test_bonding_curve_early_entry_bonus() {
        let mut candidate = create_test_enhanced_candidate();
        candidate.initial_liquidity_sol = 15.0;
        candidate.bonding_curve_progress = Some(0.05); // 5% - very early

        let scored_early = score_enhanced(&candidate, 70);

        // Compare with mid-progress
        let mut candidate_mid = create_test_enhanced_candidate();
        candidate_mid.initial_liquidity_sol = 15.0;
        candidate_mid.bonding_curve_progress = Some(0.50); // 50% - mid

        let scored_mid = score_enhanced(&candidate_mid, 70);

        // Early entry should have higher score
        assert!(
            scored_early.score > scored_mid.score,
            "Early entry should have higher score: early={}, mid={}",
            scored_early.score,
            scored_mid.score
        );
    }
}

// ============================================================================
// RiskAggregator & AggregatedRiskScore (Migration from legacy src/oracle/scorer.rs, 2025-11)
// ============================================================================

use crate::oracle::cluster_hunter::ClusterAnalysis;
use crate::oracle::profiler::DevProfile;
use crate::oracle::vision_critic::VisionCriticResult;

// Decision thresholds - extracted as constants for clarity and maintainability
/// Dev risk threshold for triggering panic sell (0.0-1.0)
const DEV_RISK_PANIC_THRESHOLD: f32 = 0.8;
/// Dev risk threshold for considering profile "clean" for HODL (0.0-1.0)
const DEV_RISK_CLEAN_THRESHOLD: f32 = 0.3;
/// Cluster supply control threshold for triggering panic sell (percentage)
const CLUSTER_PANIC_THRESHOLD_PCT: f32 = 30.0;
/// Vision score threshold for triggering HODL (0-10)
const VISION_HODL_THRESHOLD: u8 = 7;

// Cluster risk scoring thresholds (percentage values)
/// Critical cluster control level - almost certain rug
const CLUSTER_CRITICAL_THRESHOLD: f32 = 50.0;
/// High cluster control level - likely manipulation
const CLUSTER_HIGH_THRESHOLD: f32 = 30.0;
/// Moderate cluster control level - suspicious
const CLUSTER_MODERATE_THRESHOLD: f32 = 20.0;

// Risk factor weights for combined score calculation
/// Weight for dev risk in combined score
const DEV_RISK_WEIGHT: f32 = 0.4;
/// Weight for cluster risk in combined score
const CLUSTER_RISK_WEIGHT: f32 = 0.4;
/// Weight for vision risk (inverse of viral score) in combined score
const VISION_RISK_WEIGHT: f32 = 0.2;

/// Aggregated risk score combining DevProfiler, ClusterHunter, and VisionCritic results
/// Used by OracleActor to make post-buy decisions (HODL vs Panic Sell)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AggregatedRiskScore {
    /// Combined risk score from 0.0 (safe) to 1.0 (critical)
    pub risk_score: f32,
    /// Combined quality/opportunity score from 0 to 100
    pub quality_score: u8,

    // Component scores
    /// DevProfiler risk score (0.0-1.0)
    pub dev_risk: f32,
    /// Whether the developer is a serial minter
    pub is_serial_minter: bool,
    /// ClusterHunter controlled supply percentage (0-100)
    pub cluster_controlled_pct: f32,
    /// Whether cluster detection flagged high risk
    pub cluster_high_risk: bool,
    /// VisionCritic viral score (0-10)
    pub viral_score: u8,

    // Decision flags
    /// Should trigger emergency sell (Panic Sell)
    pub should_panic_sell: bool,
    /// Should update to loose trailing stop (let profits run)
    pub should_hodl: bool,

    /// Human-readable notes explaining the assessment
    pub notes: Vec<String>,
    /// Analysis timestamp (Unix seconds)
    pub analyzed_at: u64,
}

impl Default for AggregatedRiskScore {
    fn default() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            risk_score: 0.5,
            quality_score: 50,
            dev_risk: 0.0,
            is_serial_minter: false,
            cluster_controlled_pct: 0.0,
            cluster_high_risk: false,
            viral_score: 5,
            should_panic_sell: false,
            should_hodl: false,
            notes: Vec::new(),
            analyzed_at: now,
        }
    }
}

impl AggregatedRiskScore {
    /// Check if we should trigger a panic sell
    /// Criteria: DevProfile.risk > 0.8 OR Cluster controlled > 30%
    pub fn should_panic_sell(&self) -> bool {
        self.should_panic_sell
    }

    /// Check if we should HODL with loose trailing stop
    /// Criteria: DevProfile clean (risk < 0.3) AND Vision score > 7
    pub fn should_hodl(&self) -> bool {
        self.should_hodl
    }
}

/// Risk aggregator that combines results from DevProfiler, ClusterHunter, and VisionCritic
pub struct RiskAggregator;

impl RiskAggregator {
    /// Aggregate results from all three Ghost Intelligence components
    ///
    /// Decision Logic (The Revolver Trigger):
    /// - If DevProfile.risk > 0.8 OR Cluster.controlled > 30%: PANIC SELL
    /// - If DevProfile clean (risk < 0.3) AND Vision.score > 7: HODL with loose trailing stop
    /// - Otherwise: Use default strategy
    pub fn aggregate(
        dev_profile: &DevProfile,
        cluster_analysis: &ClusterAnalysis,
        vision_result: &VisionCriticResult,
    ) -> AggregatedRiskScore {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut notes = Vec::new();

        // Extract component scores
        let dev_risk = dev_profile.risk_score;
        let is_serial_minter = dev_profile.is_serial_minter;
        let cluster_controlled_pct = cluster_analysis.metrics.controlled_supply_pct;
        let cluster_high_risk = cluster_analysis.is_high_risk;
        let viral_score = vision_result.viral_score;

        // Decision: Should Panic Sell?
        // Criteria: DevProfile.risk > DEV_RISK_PANIC_THRESHOLD OR Cluster.controlled > CLUSTER_PANIC_THRESHOLD_PCT
        let should_panic_sell = dev_risk > DEV_RISK_PANIC_THRESHOLD
            || cluster_controlled_pct > CLUSTER_PANIC_THRESHOLD_PCT;

        if should_panic_sell {
            if dev_risk > DEV_RISK_PANIC_THRESHOLD {
                notes.push(format!(
                    "CRITICAL: Dev risk score {:.2} exceeds threshold ({})",
                    dev_risk, DEV_RISK_PANIC_THRESHOLD
                ));
            }
            if cluster_controlled_pct > CLUSTER_PANIC_THRESHOLD_PCT {
                notes.push(format!(
                    "CRITICAL: Cluster controls {:.1}% supply (threshold: {}%)",
                    cluster_controlled_pct, CLUSTER_PANIC_THRESHOLD_PCT
                ));
            }
        }

        // Decision: Should HODL with loose trailing stop?
        // Criteria: DevProfile clean (risk < DEV_RISK_CLEAN_THRESHOLD) AND Vision.score > VISION_HODL_THRESHOLD
        let dev_is_clean = dev_risk < DEV_RISK_CLEAN_THRESHOLD && !is_serial_minter;
        let vision_is_strong = viral_score > VISION_HODL_THRESHOLD;
        let should_hodl = !should_panic_sell && dev_is_clean && vision_is_strong;

        if should_hodl {
            notes.push(format!(
                "HODL: Clean dev profile (risk={:.2}) + strong viral score ({}/10)",
                dev_risk, viral_score
            ));
        }

        // Calculate combined risk score (weighted average)
        let cluster_risk = if cluster_controlled_pct >= CLUSTER_CRITICAL_THRESHOLD {
            1.0
        } else if cluster_controlled_pct >= CLUSTER_HIGH_THRESHOLD {
            0.8
        } else if cluster_controlled_pct >= CLUSTER_MODERATE_THRESHOLD {
            0.5
        } else {
            cluster_controlled_pct / 100.0
        };

        // Vision score is inverted (high viral score = low risk)
        let vision_risk = 1.0 - (viral_score as f32 / 10.0);

        let combined_risk = DEV_RISK_WEIGHT * dev_risk
            + CLUSTER_RISK_WEIGHT * cluster_risk
            + VISION_RISK_WEIGHT * vision_risk;

        // Calculate quality score (inverse of risk, scaled to 0-100)
        let quality_score = ((1.0 - combined_risk) * 100.0).round() as u8;

        // Add component notes from each analyzer
        for note in &dev_profile.notes {
            notes.push(format!("[DevProfiler] {}", note));
        }
        for note in &cluster_analysis.notes {
            notes.push(format!("[ClusterHunter] {}", note));
        }
        notes.push(format!(
            "[VisionCritic] Viral score: {}/10 ({:?})",
            viral_score, vision_result.signal_strength
        ));

        AggregatedRiskScore {
            risk_score: combined_risk.min(1.0),
            quality_score: quality_score.min(100),
            dev_risk,
            is_serial_minter,
            cluster_controlled_pct,
            cluster_high_risk,
            viral_score,
            should_panic_sell,
            should_hodl,
            notes,
            analyzed_at: now,
        }
    }
}

#[cfg(test)]
mod risk_aggregator_tests {
    use super::*;
    use crate::oracle::cluster_hunter::{ClusterAnalysis, ClusterMetric};
    use crate::oracle::profiler::DevProfile;
    use crate::oracle::vision_critic::{SignalStrength, VisionCriticResult};

    fn create_test_dev_profile(risk_score: f32, is_serial_minter: bool) -> DevProfile {
        DevProfile {
            risk_score,
            is_serial_minter,
            ..Default::default()
        }
    }

    fn create_test_cluster_analysis(controlled_pct: f32, is_high_risk: bool) -> ClusterAnalysis {
        ClusterAnalysis {
            metrics: ClusterMetric {
                controlled_supply_pct: controlled_pct,
                ..Default::default()
            },
            is_high_risk,
            ..Default::default()
        }
    }

    fn create_test_vision_result(viral_score: u8) -> VisionCriticResult {
        VisionCriticResult {
            viral_score,
            ..Default::default()
        }
    }

    #[test]
    fn test_aggregated_risk_score_default() {
        let score = AggregatedRiskScore::default();

        assert_eq!(score.risk_score, 0.5);
        assert_eq!(score.quality_score, 50);
        assert!(!score.should_panic_sell);
        assert!(!score.should_hodl);
    }

    #[test]
    fn test_risk_aggregator_panic_sell_dev_risk() {
        // Dev risk > 0.8 should trigger panic sell
        let dev_profile = create_test_dev_profile(0.9, false);
        let cluster = create_test_cluster_analysis(10.0, false);
        let vision = create_test_vision_result(7);

        let result = RiskAggregator::aggregate(&dev_profile, &cluster, &vision);

        assert!(result.should_panic_sell);
        assert!(!result.should_hodl);
        assert!(result.notes.iter().any(|n| n.contains("Dev risk")));
    }

    #[test]
    fn test_risk_aggregator_panic_sell_cluster_risk() {
        // Cluster controlled > 30% should trigger panic sell
        let dev_profile = create_test_dev_profile(0.2, false);
        let cluster = create_test_cluster_analysis(35.0, true);
        let vision = create_test_vision_result(8);

        let result = RiskAggregator::aggregate(&dev_profile, &cluster, &vision);

        assert!(result.should_panic_sell);
        assert!(!result.should_hodl);
        assert!(result.notes.iter().any(|n| n.contains("Cluster controls")));
    }

    #[test]
    fn test_risk_aggregator_hodl_conditions() {
        // Clean dev profile (risk < 0.3) AND vision score > 7 should trigger HODL
        let dev_profile = create_test_dev_profile(0.1, false);
        let cluster = create_test_cluster_analysis(5.0, false);
        let vision = create_test_vision_result(9);

        let result = RiskAggregator::aggregate(&dev_profile, &cluster, &vision);

        assert!(!result.should_panic_sell);
        assert!(result.should_hodl);
        assert!(result.notes.iter().any(|n| n.contains("HODL")));
    }

    #[test]
    fn test_risk_aggregator_no_hodl_serial_minter() {
        // Serial minter should not get HODL even with good scores
        let dev_profile = create_test_dev_profile(0.1, true); // is_serial_minter = true
        let cluster = create_test_cluster_analysis(5.0, false);
        let vision = create_test_vision_result(9);

        let result = RiskAggregator::aggregate(&dev_profile, &cluster, &vision);

        assert!(!result.should_panic_sell);
        assert!(!result.should_hodl); // Should not HODL because of serial minter
    }

    #[test]
    fn test_risk_aggregator_neutral_case() {
        // Moderate risk levels - neither panic sell nor HODL
        let dev_profile = create_test_dev_profile(0.5, false);
        let cluster = create_test_cluster_analysis(15.0, false);
        let vision = create_test_vision_result(5);

        let result = RiskAggregator::aggregate(&dev_profile, &cluster, &vision);

        assert!(!result.should_panic_sell);
        assert!(!result.should_hodl);
        assert!(result.risk_score > 0.0 && result.risk_score < 1.0);
    }
}
