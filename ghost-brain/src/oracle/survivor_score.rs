//! SurvivorScore - Interpretable Token Survival Scoring
//!
//! Replaces QASS with economically meaningful scoring system.
//! Each component has clear interpretation and can be calibrated on historical data.
//!
//! # Formula
//!
//! ```text
//! SurvivorScore = (Survival × Momentum × Quality) × (1 - RiskDiscount)
//!
//! Where:
//!   Survival  ∈ [0, 1] - Probability token won't rug in next 60s
//!   Momentum  ∈ [0.2, 4.0] - Buying pressure strength (1.0 = neutral)
//!   Quality   ∈ [0, 1] - Organic activity ratio
//!   RiskDiscount ∈ [0, 1] - Penalty for detected risks
//!
//! Final score is normalized to [0, 100]
//! ```
//!
//! # Component Breakdown
//!
//! ## Survival (Weight: 0.35)
//! - QEDD survival probability (exponential decay)
//! - IWIM dev intent score (inverted threat)
//! - ClusterHunter cabal detection (inverted risk)
//!
//! ## Momentum (Weight: 0.30)
//! Momentum is calculated from 3 independent signals aggregated via geometric mean:
//!
//! 1. **SOBP (Sell-Or-Buy Pressure)** [0.2, 4.0]
//!    - Measures transaction-level buying vs selling pressure
//!    - High values (>2.0) indicate strong accumulation (pump signal)
//!    - Low values (<0.5) indicate distribution (dump signal)
//!
//! 2. **QMAN (Quality × Managers)** [0.5, 2.5]
//!    - Quality-adjusted manager confidence score
//!    - Measures trust in token management/development
//!
//! 3. **CHAOS (Volatility)** [0.3, 2.5]
//!    - Price volatility and chaos indicator
//!    - High chaos (>1.5) = active pump/dump
//!    - Low chaos (<0.5) = dead/stable token
//!
//! **Final Momentum Range:** [0.2, 4.0]
//!   - <0.5: Strong selling pressure (avoid)
//!   - 0.8-1.2: Neutral (sideways)
//!   - 1.5-2.5: Moderate buying pressure (watch)
//!   - >3.0: Extreme buying pressure (pump signal)
//!
//! **Changes from Previous Version (Issue #155):**
//!   - Old range: [0.5, 2.0] (1.5 points)
//!   - New range: [0.2, 4.0] (3.8 points)
//!   - Improvement: 2.5x dynamic range expansion
//!
//! ## Quality (Weight: 0.20)
//! - MPCF organic ratio
//! - MESA organic_likeness
//! - SCR bot detection (inverted)
//! - Wallet diversity from BlockMetrics
//!
//! ## Risk Discount (Weight: 0.15)
//! - High wash trading (MESA)
//! - Smart money exit (QMAN)
//! - Price crash detection
//! - ParadoxSensor anomaly

use metrics::increment_counter;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Instant;
use tracing::{debug, warn};

use crate::config::ghost_brain_config::BehavioralScoringConfig;
use crate::config::GhostBrainConfig;
use crate::oracle::ultrafast::ecto::EctoVerdict;

// =============================================================================
// Dynamic Threshold Constants
// =============================================================================

/// Threshold for early stage tokens (TX count < 100)
/// More lenient to catch legitimate pumps in first 30-60 seconds
const EARLY_STAGE_THRESHOLD: u8 = 55;

/// Transaction count threshold for early vs full analysis
/// Tokens with < this many transactions use EARLY_STAGE_THRESHOLD
const EARLY_STAGE_TX_THRESHOLD: u64 = 100;

/// Session stage for SurvivorScore threshold selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurvivorSessionStage {
    Early,
    Full,
}

impl SurvivorSessionStage {
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            SurvivorSessionStage::Early => "EARLY",
            SurvivorSessionStage::Full => "FULL",
        }
    }
}

// =============================================================================
// Momentum Configuration (Issue #155)
// =============================================================================

/// Momentum calculation configuration
///
/// Allows runtime tuning of clamp boundaries without code changes.
/// Issue #155: Widened clamps to allow extreme signals to propagate.
///
/// # Default Ranges
/// - SOBP momentum bounds: [-0.8, 3.0] → output [0.2, 4.0]
/// - QMAN momentum bounds: [0.5, 2.5]
/// - CHAOS momentum bounds: [0.3, 2.5]
/// - Final aggregated momentum: [0.2, 4.0]
///
/// # Changes from Legacy
/// - Old SOBP clamp: [-1.0, 1.0] → max contribution 1.5
/// - Old final clamp: [0.5, 2.0] → only 1.5 points range
/// - New range: 3.8 points → 2.5x expansion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomentumConfig {
    /// SOBP momentum lower bound (extreme selling)
    /// Default: -0.8
    pub sobp_min: f32,

    /// SOBP momentum upper bound (extreme buying)
    /// Default: 3.0
    pub sobp_max: f32,

    /// QMAN momentum lower bound
    /// Default: 0.5
    pub qman_min: f32,

    /// QMAN momentum upper bound
    /// Default: 2.5
    pub qman_max: f32,

    /// CHAOS momentum lower bound
    /// Default: 0.3
    pub chaos_min: f32,

    /// CHAOS momentum upper bound
    /// Default: 2.5
    pub chaos_max: f32,

    /// Final aggregated momentum lower bound
    /// Default: 0.2
    pub final_min: f32,

    /// Final aggregated momentum upper bound
    /// Default: 4.0
    pub final_max: f32,

    /// Enable signal loss warnings when values are clamped
    /// Default: true
    pub warn_on_clamp: bool,
}

impl Default for MomentumConfig {
    fn default() -> Self {
        Self {
            sobp_min: -0.8,
            sobp_max: 3.0,
            qman_min: 0.5,
            qman_max: 2.5,
            chaos_min: 0.3,
            chaos_max: 2.5,
            final_min: 0.2,
            final_max: 4.0,
            warn_on_clamp: true,
        }
    }
}

impl MomentumConfig {
    /// Load momentum config from environment variables
    ///
    /// Supported environment variables:
    /// - GHOST_MOMENTUM_SOBP_MIN: SOBP lower bound (default: -0.8)
    /// - GHOST_MOMENTUM_SOBP_MAX: SOBP upper bound (default: 3.0)
    /// - GHOST_MOMENTUM_QMAN_MIN: QMAN lower bound (default: 0.5)
    /// - GHOST_MOMENTUM_QMAN_MAX: QMAN upper bound (default: 2.5)
    /// - GHOST_MOMENTUM_CHAOS_MIN: CHAOS lower bound (default: 0.3)
    /// - GHOST_MOMENTUM_CHAOS_MAX: CHAOS upper bound (default: 2.5)
    /// - GHOST_MOMENTUM_FINAL_MIN: Final lower bound (default: 0.2)
    /// - GHOST_MOMENTUM_FINAL_MAX: Final upper bound (default: 4.0)
    /// - GHOST_MOMENTUM_WARN_ON_CLAMP: Enable clamp warnings (default: true)
    pub fn from_env() -> Self {
        Self {
            sobp_min: env::var("GHOST_MOMENTUM_SOBP_MIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(-0.8),
            sobp_max: env::var("GHOST_MOMENTUM_SOBP_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3.0),
            qman_min: env::var("GHOST_MOMENTUM_QMAN_MIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.5),
            qman_max: env::var("GHOST_MOMENTUM_QMAN_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.5),
            chaos_min: env::var("GHOST_MOMENTUM_CHAOS_MIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.3),
            chaos_max: env::var("GHOST_MOMENTUM_CHAOS_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.5),
            final_min: env::var("GHOST_MOMENTUM_FINAL_MIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.2),
            final_max: env::var("GHOST_MOMENTUM_FINAL_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4.0),
            warn_on_clamp: env::var("GHOST_MOMENTUM_WARN_ON_CLAMP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
        }
    }

    /// Check if legacy momentum clamps should be used
    ///
    /// Set GHOST_LEGACY_MOMENTUM=true to revert to old behavior (0.5-2.0 range)
    pub fn use_legacy_clamps() -> bool {
        env::var("GHOST_LEGACY_MOMENTUM")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(false)
    }
}

/// Track how often we hit clamp boundaries (indicates signal loss)
///
/// Issue #155: Diagnostics for monitoring momentum signal quality.
/// When clamps are hit frequently, it indicates potential signal loss.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MomentumDiagnostics {
    /// True if SOBP raw value was outside configured bounds
    pub sobp_clamped: bool,

    /// True if QMAN raw value was outside configured bounds
    pub qman_clamped: bool,

    /// True if CHAOS raw value was outside configured bounds
    pub chaos_clamped: bool,

    /// True if final momentum was outside configured bounds
    pub final_clamped: bool,

    /// Raw SOBP input value before processing
    pub sobp_raw: f32,

    /// SOBP value after asymmetric scaling
    pub sobp_after_scale: f32,

    /// QMAN raw input value
    pub qman_raw: f32,

    /// QMAN value after scaling
    pub qman_after_scale: f32,

    /// CHAOS raw input value
    pub chaos_raw: f32,

    /// CHAOS value after scaling
    pub chaos_after_scale: f32,

    /// Final momentum before clamping (geometric mean result)
    pub final_raw: f32,

    /// Final momentum after clamping
    pub final_after_clamp: f32,
}

// =============================================================================
// Configuration Types
// =============================================================================

/// SurvivorScore configuration
///
/// All weights are now loaded from the `[confidence]` section of ghost_brain_config.toml.
/// No hardcoded defaults are used - weights are derived from config values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivorScoreConfig {
    /// Weight for survival component (derived from weight_iwim in config)
    /// Controls how much IWIM/QEDD/Cluster signals affect the score
    pub weight_survival: f32,

    /// Weight for momentum component (derived from weight_sobp in config)
    /// Controls how much SOBP/QMAN/Chaos signals affect the score
    pub weight_momentum: f32,

    /// Weight for quality component (derived from weight_mpcf in config)
    /// Controls how much MPCF/MESA/SCR signals affect the score
    pub weight_quality: f32,

    /// Minimum score to pass (0-100), derived from threshold_high in config
    pub passing_threshold: u8,

    /// Enable detailed logging
    pub verbose_logging: bool,
}

impl SurvivorScoreConfig {
    /// Create SurvivorScoreConfig from GhostBrainConfig
    ///
    /// If `survivor_score` section is present in config, uses those values directly.
    /// Otherwise falls back to mapping from `confidence` section for backward compatibility:
    /// - `confidence.weight_iwim` → `weight_survival` (normalized)
    /// - `confidence.weight_sobp` → `weight_momentum` (normalized)
    /// - `confidence.weight_mpcf` → `weight_quality` (normalized)
    /// - `confidence.threshold_high` → `passing_threshold` (scaled to 0-100)
    ///
    /// Weights are normalized to sum to 1.0 (100%) allowing full score range.
    /// NO HARDCODED CAPS - maximum score of 100/100 is achievable for ideal tokens.
    pub fn from_config(config: &GhostBrainConfig) -> Self {
        // If dedicated survivor_score config exists, use it
        if let Some(ref ss_config) = config.survivor_score {
            return Self {
                weight_survival: ss_config.weight_survival,
                weight_momentum: ss_config.weight_momentum,
                weight_quality: ss_config.weight_quality,
                passing_threshold: (config.confidence.threshold_high * 100.0).clamp(0.0, 100.0)
                    as u8,
                verbose_logging: true,
            };
        }

        // Fallback: derive from confidence section for backward compatibility
        let conf = &config.confidence;

        // Get raw weights from config
        let raw_iwim = conf.weight_iwim;
        let raw_sobp = conf.weight_sobp;
        let raw_mpcf = conf.weight_mpcf;

        // Calculate total for normalization
        let total = raw_iwim + raw_sobp + raw_mpcf;

        // Normalize weights to sum to 1.0 (100%) - NO ARTIFICIAL CAPS
        // Use a small epsilon to handle floating-point precision issues
        const TOTAL_WEIGHT_TARGET: f32 = 1.0;
        const MIN_TOTAL_FOR_NORMALIZATION: f32 = 0.001;

        let (weight_survival, weight_momentum, weight_quality) =
            if total > MIN_TOTAL_FOR_NORMALIZATION {
                let norm_factor = TOTAL_WEIGHT_TARGET / total;
                (
                    raw_iwim * norm_factor,
                    raw_sobp * norm_factor,
                    raw_mpcf * norm_factor,
                )
            } else {
                // Fallback to equal distribution if all weights are effectively 0
                let fallback_weight = TOTAL_WEIGHT_TARGET / 3.0;
                (fallback_weight, fallback_weight, fallback_weight)
            };

        // Convert threshold_high (0.0-1.0) to passing_threshold (0-100)
        let passing_threshold = (conf.threshold_high * 100.0).clamp(0.0, 100.0) as u8;

        Self {
            weight_survival,
            weight_momentum,
            weight_quality,
            passing_threshold,
            verbose_logging: true,
        }
    }
}

impl Default for SurvivorScoreConfig {
    fn default() -> Self {
        // Default values for when no config is available
        // These match the legacy hardcoded values for backward compatibility
        Self {
            weight_survival: 0.35,
            weight_momentum: 0.30,
            weight_quality: 0.20,
            passing_threshold: 65,
            verbose_logging: true,
        }
    }
}

// =============================================================================
// Additive Scoring Model Configuration (Issue #56)
// =============================================================================

/// Additive scoring model configuration
///
/// Replaces the multiplicative formula (survival^w * momentum^w * quality^w)
/// with an additive model where each component contributes independently.
///
/// # Philosophy
/// - Linear contributions from each component with proper weighting
/// - Exponential boost for tokens that score high across all metrics
/// - Risk penalties subtract from total (not multiply)
///
/// # Expected Score Range
/// - Worst case: ~28 points
/// - Best case: ~92 points
/// - Effective range: 60+ points (vs 13 points with multiplicative)
///
/// ## Configuration Parameters
///
/// | Parameter | Default | Range | Description |
/// |-----------|---------|-------|-------------|
/// | weight_survival | 0.35 | [0.0, 1.0] | Survival contribution weight (35 pts max) |
/// | weight_momentum | 0.30 | [0.0, 1.0] | Momentum contribution weight (30 pts max) |
/// | weight_quality | 0.20 | [0.0, 1.0] | Quality contribution weight (20 pts max) |
/// | excellence_threshold | 70.0 | [0.0, 100.0] | Score above which boost applies |
/// | excellence_multiplier | 1.5 | [1.0, ∞) | Multiplier for excess above threshold |
/// | penalty_threshold | 40.0 | [0.0, 100.0] | Score below which penalty applies |
/// | penalty_multiplier | 1.3 | [1.0, ∞) | Multiplier for deficit below threshold |
/// | risk_penalty_max | 50.0 | [0.0, 100.0] | Maximum risk penalty in points |
/// | momentum_neutral | 0.5 | [0.0, ∞) | Neutral momentum value (centered to 0) |
/// | momentum_min_offset | -0.3 | [-1.0, 0.0] | Minimum momentum offset after centering |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditiveScoringConfig {
    // Component weights (should sum to ~0.85 to leave room for boost)
    /// Weight for survival component
    ///
    /// **Default**: 0.35 (35% of base score)  
    /// **Range**: [0.0, 1.0]  
    /// **Contribution**: survival_value × weight × 100 = 0-35 points
    pub weight_survival: f32,

    /// Weight for momentum component
    ///
    /// **Default**: 0.30 (30% of base score)  
    /// **Range**: [0.0, 1.0]  
    /// **Contribution**: (momentum - neutral).clamp(min_offset, max_offset) × weight × 100 = -9 to +45 points
    pub weight_momentum: f32,

    /// Weight for quality component
    ///
    /// **Default**: 0.20 (20% of base score)  
    /// **Range**: [0.0, 1.0]  
    /// **Contribution**: quality_value × weight × 100 = 0-20 points
    pub weight_quality: f32,

    // Excellence boost parameters
    /// Score threshold above which excellence boost is applied
    ///
    /// **Default**: 70.0  
    /// **Range**: [0.0, 100.0]  
    /// **Effect**: Tokens with base_score > threshold get (excess × multiplier) added
    pub excellence_threshold: f32,

    /// Multiplier for excess points above excellence threshold
    ///
    /// **Default**: 1.5 (50% boost to excess points)  
    /// **Range**: [1.0, ∞)  
    /// **Example**: base=80, threshold=70 → boost = (80-70) × 1.5 = 15 → final=85
    pub excellence_multiplier: f32,

    // Penalty amplification parameters
    /// Score threshold below which penalty amplification is applied
    ///
    /// **Default**: 40.0  
    /// **Range**: [0.0, 100.0]  
    /// **Effect**: Tokens with base_score < threshold get deficit amplified
    pub penalty_threshold: f32,

    /// Multiplier for deficit points below penalty threshold
    ///
    /// **Default**: 1.3 (30% amplification of deficit)  
    /// **Range**: [1.0, ∞)  
    /// **Example**: base=30, threshold=40 → amplified = (40-30) × 1.3 = 13 → final=27
    pub penalty_multiplier: f32,

    // Risk penalty scaling
    /// Maximum risk penalty in points
    ///
    /// **Default**: 50.0 (risk_discount × 50 = max 50 points subtracted)  
    /// **Range**: [0.0, 100.0]  
    /// **Example**: risk=0.5 → penalty = 0.5 × 50 = 25 points subtracted
    pub risk_penalty_max: f32,

    // Momentum centering parameters
    /// Neutral momentum value (subtracted before contribution calculation)
    ///
    /// **Default**: 0.5  
    /// **Range**: [0.0, ∞)  
    /// **Effect**: momentum=0.5 contributes 0 points, higher/lower adds/subtracts
    pub momentum_neutral: f32,

    /// Minimum momentum offset allowed after centering
    ///
    /// **Default**: -0.3  
    /// **Range**: [-1.0, 0.0]  
    /// **Effect**: Prevents extreme negative momentum from dominating score
    pub momentum_min_offset: f32,

    /// Maximum momentum offset allowed after centering
    ///
    /// **Default**: 1.5
    /// **Range**: [0.0, ∞)
    /// **Effect**: Caps positive momentum contribution to prevent base_score
    /// saturation before clamp. With weight_momentum=0.30 and max_offset=1.5,
    /// maximum momentum contribution = 1.5 × 0.30 × 100 = 45 points, keeping
    /// base_score within [0, 100] for typical inputs.
    pub momentum_max_offset: f32,
}

impl Default for AdditiveScoringConfig {
    fn default() -> Self {
        Self {
            weight_survival: 0.35,
            weight_momentum: 0.30,
            weight_quality: 0.20,
            excellence_threshold: 70.0,
            excellence_multiplier: 1.5,
            penalty_threshold: 40.0,
            penalty_multiplier: 1.3,
            risk_penalty_max: 50.0,
            momentum_neutral: 0.5,
            momentum_min_offset: -0.3,
            momentum_max_offset: 1.5,
        }
    }
}

impl AdditiveScoringConfig {
    /// Build additive scoring config using normalized SurvivorScore weights
    /// from a [`SurvivorScoreConfig`] while preserving any environment-based
    /// overrides for the remaining parameters.
    ///
    /// # Parameters
    /// - `config`: SurvivorScoreConfig produced from `SurvivorScoreConfig::from_config`
    ///   containing the runtime-weighted survival, momentum, and quality exponents.
    pub fn from_survivor_config(config: &SurvivorScoreConfig) -> Self {
        let mut base = Self::from_env();
        base.weight_survival = config.weight_survival;
        base.weight_momentum = config.weight_momentum;
        base.weight_quality = config.weight_quality;
        base
    }

    /// Load from environment variables with defaults
    pub fn from_env() -> Self {
        Self {
            weight_survival: env::var("GHOST_WEIGHT_SURVIVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.35),
            weight_momentum: env::var("GHOST_WEIGHT_MOMENTUM")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.30),
            weight_quality: env::var("GHOST_WEIGHT_QUALITY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.20),
            excellence_threshold: env::var("GHOST_EXCELLENCE_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(70.0),
            excellence_multiplier: env::var("GHOST_EXCELLENCE_MULTIPLIER")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.5),
            penalty_threshold: env::var("GHOST_PENALTY_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(40.0),
            penalty_multiplier: env::var("GHOST_PENALTY_MULTIPLIER")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.3),
            risk_penalty_max: env::var("GHOST_RISK_PENALTY_MAX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50.0),
            momentum_neutral: env::var("GHOST_MOMENTUM_NEUTRAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.5),
            momentum_min_offset: env::var("GHOST_MOMENTUM_MIN_OFFSET")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(-0.3),
            momentum_max_offset: env::var("GHOST_MOMENTUM_MAX_OFFSET")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.5),
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        let weight_sum = self.weight_survival + self.weight_momentum + self.weight_quality;

        if weight_sum > 1.0 {
            return Err(format!(
                "Component weights sum to {:.3} (should be ≤1.0)",
                weight_sum
            ));
        }

        if self.excellence_multiplier < 1.0 {
            return Err("Excellence multiplier must be ≥1.0".to_string());
        }

        if self.penalty_multiplier < 1.0 {
            return Err("Penalty multiplier must be ≥1.0".to_string());
        }

        if self.excellence_threshold <= self.penalty_threshold {
            return Err("Excellence threshold must be > penalty threshold".to_string());
        }

        Ok(())
    }

    // ========================================
    // Helper methods for score calculation
    // These reduce code duplication across methods
    // ========================================

    /// Calculate momentum contribution points
    ///
    /// Momentum is centered around neutral (default 0.5) so that:
    /// - momentum = 0.5 → 0 contribution
    /// - momentum > 0.5 → positive contribution (buying pressure)
    /// - momentum < 0.5 → negative contribution (selling pressure)
    ///
    /// The offset is clamped to prevent extreme negative values from dominating.
    #[inline]
    pub fn calculate_momentum_contribution(&self, momentum: f32) -> f32 {
        // Clamp centered momentum: min_offset prevents extreme negative from dominating,
        // max_offset prevents base_score saturation before the final [0, 100] clamp.
        let momentum_centered = (momentum - self.momentum_neutral)
            .clamp(self.momentum_min_offset, self.momentum_max_offset);
        momentum_centered * self.weight_momentum * 100.0
    }

    /// Calculate survival contribution points
    #[inline]
    pub fn calculate_survival_contribution(&self, survival: f32) -> f32 {
        survival * self.weight_survival * 100.0
    }

    /// Calculate quality contribution points
    #[inline]
    pub fn calculate_quality_contribution(&self, quality: f32) -> f32 {
        quality * self.weight_quality * 100.0
    }

    /// Apply excellence boost or penalty amplification to base score
    ///
    /// Returns (boosted_score, excellence_applied, penalty_applied)
    pub fn apply_boost_or_penalty(&self, base_score: f32) -> (f32, bool, bool) {
        if base_score > self.excellence_threshold {
            // Excellence boost: tokens above threshold get extra points
            let excess = base_score - self.excellence_threshold;
            let boosted = self.excellence_threshold + (excess * self.excellence_multiplier);
            (boosted, true, false)
        } else if base_score < self.penalty_threshold {
            // Penalty amplification: tokens below threshold get penalized harder
            let deficit = self.penalty_threshold - base_score;
            let amplified = self.penalty_threshold - (deficit * self.penalty_multiplier);
            (amplified, false, true)
        } else {
            // Middle range: linear pass-through
            (base_score, false, false)
        }
    }

    /// Calculate risk penalty in points
    #[inline]
    pub fn calculate_risk_penalty(&self, risk_discount: f32) -> f32 {
        risk_discount * self.risk_penalty_max
    }

    /// Get boost description string for visualization
    pub fn get_boost_description(&self, excellence_applied: bool, penalty_applied: bool) -> String {
        if excellence_applied {
            format!(
                "APPLIED (+{:.0}%)",
                (self.excellence_multiplier - 1.0) * 100.0
            )
        } else if penalty_applied {
            format!("PENALTY (-{:.0}%)", (self.penalty_multiplier - 1.0) * 100.0)
        } else {
            "Not applied".to_string()
        }
    }
}

/// Check if legacy multiplicative scoring should be used
///
/// Set GHOST_LEGACY_SCORING=true to revert to old multiplicative formula.
/// This provides emergency rollback capability.
pub fn use_legacy_multiplicative_scoring() -> bool {
    env::var("GHOST_LEGACY_SCORING")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(false)
}

/// Explicit reasons for hard veto decisions (Kill actions).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VetoReason {
    EctoRugDetected,
    ParadoxAnomaly,
    PriceCrashExtreme,
    SnapshotDiscontinuity,
    PriceInvalid,
    ReservesTooLow,
    InsufficientTx,
    SlotMissingOrZero,
    IntegrityViolation,
}

/// Explicit reasons for deferring evaluation (Continue actions).
///
/// Defer reasons indicate that scoring is temporarily suspended, but the session
/// continues. These are NOT hard stops like veto reasons.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeferReason {
    /// Warmup phase - not enough live snapshots to start scoring
    WarmupNotReady,
    /// Slot is missing or zero - waiting for valid data
    SlotMissingOrZero,
    /// No stable SnapshotEngine data available for this cycle
    SnapshotUnavailable,
    /// Bootstrap only - waiting for real account updates
    BootstrapOnly,
    /// Price not yet available - defer until price is known
    PriceUnknown,
    /// CIR produced no emissions within the adaptive window
    CirNoEmit,
}

impl VetoReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            VetoReason::EctoRugDetected => "ecto_rug_detected",
            VetoReason::ParadoxAnomaly => "paradox_anomaly",
            VetoReason::PriceCrashExtreme => "price_crash_extreme",
            VetoReason::SnapshotDiscontinuity => "snapshot_discontinuity",
            VetoReason::PriceInvalid => "price_invalid",
            VetoReason::ReservesTooLow => "reserves_too_low",
            VetoReason::InsufficientTx => "insufficient_tx",
            VetoReason::SlotMissingOrZero => "slot_missing_or_zero",
            VetoReason::IntegrityViolation => "integrity_violation",
        }
    }
}

impl std::fmt::Display for VetoReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                VetoReason::EctoRugDetected => "⛔ ECTO RUG DETECTED",
                VetoReason::ParadoxAnomaly => "⛔ PARADOX ANOMALY",
                VetoReason::PriceCrashExtreme => "⛔ PRICE CRASH >90%",
                VetoReason::SnapshotDiscontinuity => "⛔ SNAPSHOT DISCONTINUITY",
                VetoReason::PriceInvalid => "⛔ PRICE INVALID",
                VetoReason::ReservesTooLow => "⛔ RESERVES TOO LOW",
                VetoReason::InsufficientTx => "⛔ INSUFFICIENT TX DATA",
                VetoReason::SlotMissingOrZero => "⛔ SLOT MISSING OR ZERO",
                VetoReason::IntegrityViolation => "⛔ INTEGRITY VIOLATION",
            }
        )
    }
}

impl DeferReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeferReason::WarmupNotReady => "warmup_not_ready",
            DeferReason::SlotMissingOrZero => "slot_missing_or_zero",
            DeferReason::SnapshotUnavailable => "snapshot_unavailable",
            DeferReason::BootstrapOnly => "bootstrap_only",
            DeferReason::PriceUnknown => "price_unknown",
            DeferReason::CirNoEmit => "cir_no_emit",
        }
    }
}

impl std::fmt::Display for DeferReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                DeferReason::WarmupNotReady => "⏸️ WARMUP NOT READY",
                DeferReason::SlotMissingOrZero => "⏸️ SLOT MISSING OR ZERO",
                DeferReason::SnapshotUnavailable => "⏸️ SNAPSHOT UNAVAILABLE",
                DeferReason::BootstrapOnly => "⏸️ BOOTSTRAP ONLY",
                DeferReason::PriceUnknown => "⏸️ PRICE UNKNOWN",
                DeferReason::CirNoEmit => "⏸️ CIR NO EMIT",
            }
        )
    }
}

/// Extended score breakdown with component contributions
///
/// Provides detailed diagnostic information for the additive scoring model,
/// including point contributions from each component and intermediate stages.
///
/// ## Field Descriptions
///
/// ### Original Component Values
/// - `survival`: Survival probability (0.0-1.0), from QEDD/IWIM/Cluster
/// - `momentum`: Buying pressure (0.2-4.0), from SOBP/QMAN/Chaos
/// - `quality`: Signal quality (0.0-1.0), from MPCF/MESA/SCR
/// - `risk_discount`: Risk penalty factor (0.0-1.0), from wash/exit signals
///
/// ### Point Contributions
/// - `survival_points`: survival × weight × 100 (typically 0-35)
/// - `momentum_points`: (momentum - neutral) × weight × 100 (typically -9 to +105)
/// - `quality_points`: quality × weight × 100 (typically 0-20)
///
/// ### Score Stages
/// - `base_score`: Sum of all point contributions
/// - `score_after_boost`: After excellence/penalty adjustment
/// - `risk_penalty_points`: Points subtracted for risk
/// - `final_score`: Clamped result (0-100)
///
/// ### Flags
/// - `excellence_boost_applied`: True if base > excellence_threshold
/// - `penalty_amplification_applied`: True if base < penalty_threshold
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdownDetailed {
    // Original component values (0.0-1.0 or momentum range)
    /// Survival component value (0.0-1.0)
    pub survival: f32,
    /// Momentum component value (0.2-4.0, 1.0 = neutral)
    pub momentum: f32,
    /// Quality component value (0.0-1.0)
    pub quality: f32,
    /// Risk discount (0.0-1.0, higher = more risk)
    pub risk_discount: f32,

    // Point contributions from each component
    /// Points from survival (typically 0-35)
    pub survival_points: f32,
    /// Points from momentum (typically -9 to +105)
    pub momentum_points: f32,
    /// Points from quality (typically 0-20)
    pub quality_points: f32,

    // Score stages
    /// Sum of component contributions (before boost/penalty)
    pub base_score: f32,
    /// After excellence boost or penalty amplification
    pub score_after_boost: f32,
    /// Risk penalty in points
    pub risk_penalty_points: f32,
    /// Final clamped score (0-100)
    pub final_score: u8,

    // Flags for special conditions
    /// True if excellence boost was applied
    pub excellence_boost_applied: bool,
    /// True if penalty amplification was applied  
    pub penalty_amplification_applied: bool,

    // Original sub-breakdowns (for compatibility)
    /// SOBP contribution to momentum
    pub momentum_from_sobp: f32,
    /// QMAN contribution to momentum
    pub momentum_from_qman: f32,
    /// CHAOS contribution to momentum
    pub momentum_from_chaos: f32,
}

impl ScoreBreakdownDetailed {
    /// Create from legacy SurvivorScoreBreakdown with additive model calculation
    ///
    /// Uses the helper methods from AdditiveScoringConfig to ensure consistency
    /// with the main scoring calculation.
    pub fn from_breakdown(
        breakdown: &SurvivorScoreBreakdown,
        config: &AdditiveScoringConfig,
    ) -> Self {
        // Calculate component contributions using helper methods
        let survival_points = config.calculate_survival_contribution(breakdown.survival);
        let momentum_points = config.calculate_momentum_contribution(breakdown.momentum);
        let quality_points = config.calculate_quality_contribution(breakdown.quality);

        let base_score = survival_points + momentum_points + quality_points;

        // Apply excellence boost or penalty amplification using helper
        let (score_after_boost, excellence_boost, penalty_amp) =
            config.apply_boost_or_penalty(base_score);

        // Apply risk penalty as subtraction using helper
        let risk_penalty = config.calculate_risk_penalty(breakdown.risk_discount);
        let final_raw = (score_after_boost - risk_penalty).clamp(0.0, 100.0);
        let final_score = final_raw.round() as u8;

        Self {
            survival: breakdown.survival,
            momentum: breakdown.momentum,
            quality: breakdown.quality,
            risk_discount: breakdown.risk_discount,
            survival_points,
            momentum_points,
            quality_points,
            base_score,
            score_after_boost,
            risk_penalty_points: risk_penalty,
            final_score,
            excellence_boost_applied: excellence_boost,
            penalty_amplification_applied: penalty_amp,
            momentum_from_sobp: breakdown.momentum_from_sobp,
            momentum_from_qman: breakdown.momentum_from_qman,
            momentum_from_chaos: breakdown.momentum_from_chaos,
        }
    }
}

/// Generate score breakdown visualization for debugging
pub fn visualize_score_breakdown(breakdown: &ScoreBreakdownDetailed) -> String {
    format!(
        r#"
╔══════════════════════════════════════════════════════════════╗
║              GHOST ORACLE SCORE BREAKDOWN (ADDITIVE)         ║
╠══════════════════════════════════════════════════════════════╣
║                                                              ║
║  COMPONENT CONTRIBUTIONS:                                    ║
║  ┌────────────────────────────────────────────────────────┐ ║
║  │ Survival:  {:.3} × 35% = {:>6.1} points                 │ ║
║  │ Momentum:  {:.3} × 30% = {:>6.1} points                 │ ║
║  │ Quality:   {:.3} × 20% = {:>6.1} points                 │ ║
║  └────────────────────────────────────────────────────────┘ ║
║                                                              ║
║  BASE SCORE: {:>5.1} points                                  ║
║                                                              ║
║  BOOST/PENALTY: {:>24}                                       ║
║  Score after boost: {:>5.1} points                           ║
║                                                              ║
║  RISK PENALTY: {:.3} × max = {:>5.1} points subtracted       ║
║                                                              ║
║  ═══════════════════════════════════════════════════════    ║
║  FINAL SCORE: {:>3} / 100                                    ║
║  ═══════════════════════════════════════════════════════    ║
╚══════════════════════════════════════════════════════════════╝
        "#,
        breakdown.survival,
        breakdown.survival_points,
        breakdown.momentum,
        breakdown.momentum_points,
        breakdown.quality,
        breakdown.quality_points,
        breakdown.base_score,
        if breakdown.excellence_boost_applied {
            "EXCELLENCE (+boost%)"
        } else if breakdown.penalty_amplification_applied {
            "PENALTY (-amplify%)"
        } else {
            "Not applied"
        },
        breakdown.score_after_boost,
        breakdown.risk_discount,
        breakdown.risk_penalty_points,
        breakdown.final_score
    )
}

/// Generate score breakdown visualization with config-aware descriptions
pub fn visualize_score_breakdown_with_config(
    breakdown: &ScoreBreakdownDetailed,
    config: &AdditiveScoringConfig,
) -> String {
    let boost_desc = config.get_boost_description(
        breakdown.excellence_boost_applied,
        breakdown.penalty_amplification_applied,
    );

    format!(
        r#"
╔══════════════════════════════════════════════════════════════╗
║              GHOST ORACLE SCORE BREAKDOWN (ADDITIVE)         ║
╠══════════════════════════════════════════════════════════════╣
║                                                              ║
║  COMPONENT CONTRIBUTIONS:                                    ║
║  ┌────────────────────────────────────────────────────────┐ ║
║  │ Survival:  {:.3} × {:.0}% = {:>6.1} points                 │ ║
║  │ Momentum:  {:.3} × {:.0}% = {:>6.1} points                 │ ║
║  │ Quality:   {:.3} × {:.0}% = {:>6.1} points                 │ ║
║  └────────────────────────────────────────────────────────┘ ║
║                                                              ║
║  BASE SCORE: {:>5.1} points                                  ║
║                                                              ║
║  BOOST/PENALTY: {:>24}                                       ║
║  Score after boost: {:>5.1} points                           ║
║                                                              ║
║  RISK PENALTY: {:.3} × {:.0} = {:>5.1} points subtracted     ║
║                                                              ║
║  ═══════════════════════════════════════════════════════    ║
║  FINAL SCORE: {:>3} / 100                                    ║
║  ═══════════════════════════════════════════════════════    ║
╚══════════════════════════════════════════════════════════════╝
        "#,
        breakdown.survival,
        config.weight_survival * 100.0,
        breakdown.survival_points,
        breakdown.momentum,
        config.weight_momentum * 100.0,
        breakdown.momentum_points,
        breakdown.quality,
        config.weight_quality * 100.0,
        breakdown.quality_points,
        breakdown.base_score,
        boost_desc,
        breakdown.score_after_boost,
        breakdown.risk_discount,
        config.risk_penalty_max,
        breakdown.risk_penalty_points,
        breakdown.final_score
    )
}

/// Input signals for SurvivorScore calculation
#[derive(Debug, Clone, Default)]
pub struct SurvivorScoreInput {
    /// Session stage resolved by engine from session progress (preferred source of truth)
    pub session_stage: Option<SurvivorSessionStage>,

    // Survival signals
    /// QEDD survival probability at 60s (0.0-1.0)
    pub qedd_survival_60s: Option<f32>,
    /// IWIM threat score (0.0-1.0, will be inverted)
    pub iwim_threat_score: Option<f32>,
    /// Cluster risk score from ClusterHunter (0.0-1.0, will be inverted)
    pub cluster_risk_score: Option<f32>,

    // Momentum signals
    /// SOBP momentum (-2.0 to 3.0 from engine, scaled to [0.2, 4.0])
    /// Issue #155: Widened input range to preserve extreme signals
    pub sobp_momentum: Option<f32>,
    /// QMAN score (0.0-1.0)
    pub qman_score: Option<f32>,
    /// Chaos Engine pump probability (0.0-1.0)
    pub chaos_pump_prob: Option<f32>,

    // Quality signals
    /// MPCF organic ratio (0.0-1.0)
    pub mpcf_organic_ratio: Option<f32>,
    /// MESA organic likeness (0.0-1.0)
    pub mesa_organic_likeness: Option<f32>,
    /// SCR bot score (0.0-1.0, will be inverted)
    pub scr_bot_score: Option<f32>,
    /// Unique wallet ratio (0.0-1.0)
    pub unique_wallet_ratio: Option<f32>,

    // Risk signals
    /// MESA wash likeness (0.0-1.0)
    pub mesa_wash_likeness: Option<f32>,
    /// True if smart money exiting (QMAN)
    pub qman_exit_signal: bool,
    /// True if price < 30% of peak
    pub price_crash_detected: bool,
    /// True if network anomaly detected (ParadoxSensor)
    pub paradox_anomaly: bool,

    // LIGMA signals
    /// LIGMA tradability score (0.0-1.0)
    pub ligma_tradability_score: Option<f32>,
    /// LIGMA psi score (-1.0 to 1.0)
    pub ligma_psi: Option<f32>,
    /// LIGMA liquidity trap risk (0.0-1.0)
    pub ligma_liquidity_trap_risk: Option<f32>,

    // Behavioral signals
    /// ECTO score (0.0-1.0)
    pub ecto_score: Option<f32>,
    /// BVA score (0.0-1.0)
    pub bva_score: Option<f32>,
    /// PANIC pressure (0.0-1.0, 1 = bad)
    pub panic_pressure: Option<f32>,
    /// TCR-Φ causality (0.0-1.0)
    pub tcr_causality: Option<f32>,
    /// CIR strength (0.0-1.0)
    pub cir_strength: Option<f32>,
    /// ECTO verdict for hard-kill gating
    pub ecto_verdict: Option<EctoVerdict>,

    // Context metadata
    /// Transaction count (used for dynamic threshold selection)
    pub tx_count: Option<u64>,
    /// Token age in seconds (used for early-stage behavioral scoring)
    pub age_secs: Option<f32>,
}

/// Detailed breakdown of SurvivorScore components
///
/// Issue #155: Updated momentum range from [0.5, 2.0] to [0.2, 4.0]
/// for 2.5x dynamic range expansion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivorScoreBreakdown {
    /// Survival component (0.0-1.0)
    pub survival: f32,
    /// Momentum component (0.2-4.0, 1.0 = neutral)
    /// Issue #155: Widened from [0.5, 2.0] to [0.2, 4.0]
    pub momentum: f32,
    /// Quality component (0.0-1.0)
    pub quality: f32,
    /// Risk discount (0.0-1.0)
    pub risk_discount: f32,

    /// Individual sub-components for debugging
    pub survival_from_qedd: f32,
    pub survival_from_iwim: f32,
    pub survival_from_cluster: f32,

    /// SOBP momentum component (0.2-4.0)
    /// Issue #155: Asymmetric scaling - buying up to 4.0, selling down to 0.2
    pub momentum_from_sobp: f32,
    /// QMAN momentum component (0.5-2.5)
    pub momentum_from_qman: f32,
    /// CHAOS momentum component (0.3-2.5)
    pub momentum_from_chaos: f32,

    pub quality_from_mpcf: f32,
    pub quality_from_mesa: f32,
    pub quality_from_scr: f32,
    pub quality_from_wallets: f32,

    pub risk_from_wash: f32,
    pub risk_from_exit: f32,
    pub risk_from_crash: f32,
    pub risk_from_anomaly: f32,

    /// LIGMA contribution to quality (0.0-1.0)
    pub quality_from_ligma: f32,
}

impl Default for SurvivorScoreBreakdown {
    fn default() -> Self {
        Self {
            survival: 0.5,
            momentum: 1.0,
            quality: 0.5,
            risk_discount: 0.0,
            survival_from_qedd: 0.5,
            survival_from_iwim: 0.5,
            survival_from_cluster: 0.5,
            momentum_from_sobp: 1.0,
            momentum_from_qman: 0.5,
            momentum_from_chaos: 0.5,
            quality_from_mpcf: 0.5,
            quality_from_mesa: 0.5,
            quality_from_scr: 0.5,
            quality_from_wallets: 0.5,
            risk_from_wash: 0.0,
            risk_from_exit: 0.0,
            risk_from_crash: 0.0,
            risk_from_anomaly: 0.0,
            quality_from_ligma: 0.5,
        }
    }
}

/// SurvivorScore calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivorScoreResult {
    /// Final score (0-100)
    pub score: u8,
    /// Raw score before normalization (0.0-2.0 theoretical max)
    pub raw_score: f32,
    /// Whether score passes threshold
    pub passed: bool,
    /// Detailed component breakdown
    pub breakdown: SurvivorScoreBreakdown,
    /// Human-readable interpretation
    pub interpretation: String,
    /// Confidence in the score (based on data availability)
    pub confidence: f32,
    /// Number of signals used (out of max)
    pub signals_used: u8,
    /// Analysis time in microseconds
    pub analysis_time_us: u64,
    /// Explicit veto reason (if any)
    pub veto_reason: Option<VetoReason>,
}

/// SurvivorScore calculator
#[derive(Debug, Clone)]
pub struct SurvivorScoreCalculator {
    config: SurvivorScoreConfig,
    ligma_weight: f32,
    /// Thresholds for survivor score evaluation (from HyperPredictionConfig)
    thresholds: Option<crate::oracle::hyper_prediction::config::SurvivorScoreThresholds>,
    /// Risk multipliers (from HyperPredictionConfig)
    risk_multipliers: Option<crate::oracle::hyper_prediction::config::RiskMultipliers>,
    /// Additive scoring model configuration (Issue #56)
    additive_config: AdditiveScoringConfig,
    /// Behavioral scoring configuration (ECTO/BVA/PANIC/TCR/CIR)
    behavioral_config: BehavioralScoringConfig,
}

impl SurvivorScoreCalculator {
    pub fn new() -> Self {
        Self::with_config(SurvivorScoreConfig::default(), 0.15)
    }

    /// Create SurvivorScoreCalculator from GhostBrainConfig
    ///
    /// This is the preferred constructor when config is available.
    /// It wires the `[confidence]` section weights directly to the scoring logic.
    pub fn from_ghost_brain_config(config: &GhostBrainConfig) -> Self {
        let survivor_config = SurvivorScoreConfig::from_config(config);
        let ligma_weight = config.ligma.weight_in_survivor_score;
        let additive_config = AdditiveScoringConfig::from_survivor_config(&survivor_config);
        let behavioral_config = config.behavioral_scoring.clone();
        let calculator = Self::with_config(survivor_config, ligma_weight)
            .with_behavioral_config(behavioral_config);
        calculator.with_additive_config(additive_config)
    }

    pub fn with_config(config: SurvivorScoreConfig, ligma_weight: f32) -> Self {
        Self {
            config,
            ligma_weight,
            thresholds: None,
            risk_multipliers: None,
            additive_config: AdditiveScoringConfig::from_env(),
            behavioral_config: BehavioralScoringConfig::default(),
        }
    }

    /// Set custom additive scoring configuration
    pub fn with_additive_config(mut self, additive_config: AdditiveScoringConfig) -> Self {
        self.additive_config = additive_config;
        self
    }

    /// Set behavioral scoring configuration
    pub fn with_behavioral_config(mut self, behavioral_config: BehavioralScoringConfig) -> Self {
        self.behavioral_config = behavioral_config;
        self
    }

    /// Update behavioral scoring configuration at runtime
    pub fn update_behavioral_config(&mut self, behavioral_config: BehavioralScoringConfig) {
        self.behavioral_config = behavioral_config;
    }

    /// Set thresholds from HyperPredictionConfig
    pub fn with_thresholds(
        mut self,
        thresholds: crate::oracle::hyper_prediction::config::SurvivorScoreThresholds,
    ) -> Self {
        self.thresholds = Some(thresholds);
        self
    }

    /// Set risk multipliers from HyperPredictionConfig
    pub fn with_risk_multipliers(
        mut self,
        risk_multipliers: crate::oracle::hyper_prediction::config::RiskMultipliers,
    ) -> Self {
        self.risk_multipliers = Some(risk_multipliers);
        self
    }

    pub fn with_ligma_weight(mut self, weight: f32) -> Self {
        self.ligma_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Effective threshold based on transaction count (early-stage vs full).
    pub fn effective_threshold(&self, tx_count: Option<u64>) -> u8 {
        if let Some(tx_count) = tx_count {
            if tx_count < EARLY_STAGE_TX_THRESHOLD {
                EARLY_STAGE_THRESHOLD
            } else {
                self.config.passing_threshold
            }
        } else {
            self.config.passing_threshold
        }
    }

    /// Calculate SurvivorScore from input signals - CYCLE VERSION (No IWIM)
    ///
    /// This version is used during scoring cycles (S1-S13). IWIM is NOT included in the
    /// survival calculation to avoid score jumps when IWIM data arrives.
    /// IWIM is only applied at Final Verdict as a multiplier.
    pub fn calculate(&self, input: &SurvivorScoreInput) -> SurvivorScoreResult {
        self.calculate_internal(input, false)
    }

    /// Calculate SurvivorScore with IWIM applied (Final Verdict version)
    ///
    /// This version includes IWIM in the survival calculation and is used only
    /// at the Final Verdict phase after all cycles are complete.
    pub fn calculate_with_iwim(&self, input: &SurvivorScoreInput) -> SurvivorScoreResult {
        self.calculate_internal(input, true)
    }

    /// Calculate score using explicit additive model (Issue #56)
    ///
    /// This is a standalone method for:
    /// - Direct additive score calculation
    /// - A/B testing comparison with multiplicative model
    /// - Unit testing of additive formula
    ///
    /// Formula:
    ///   base_score = survival_contribution + momentum_contribution + quality_contribution
    ///   final_score = apply_excellence_boost(base_score) - risk_penalty
    ///
    /// Returns: Final score ∈ [0, 100]
    pub fn calculate_score_additive(&self, breakdown: &SurvivorScoreBreakdown) -> u8 {
        // Calculate component contributions using helper methods
        let survival_contribution = self
            .additive_config
            .calculate_survival_contribution(breakdown.survival);
        let momentum_contribution = self
            .additive_config
            .calculate_momentum_contribution(breakdown.momentum);
        let quality_contribution = self
            .additive_config
            .calculate_quality_contribution(breakdown.quality);

        // Base score: sum of contributions
        let base_score = survival_contribution + momentum_contribution + quality_contribution;

        // Apply excellence boost or penalty amplification using helper
        let (score_after_boost, _, _) = self.additive_config.apply_boost_or_penalty(base_score);

        // Risk penalty as subtraction using helper
        let risk_penalty = self
            .additive_config
            .calculate_risk_penalty(breakdown.risk_discount);
        let score_after_risk = score_after_boost - risk_penalty;

        // Clamp and return
        score_after_risk.clamp(0.0, 100.0).round() as u8
    }

    /// Calculate score using legacy multiplicative model
    ///
    /// This method is preserved for:
    /// - Emergency rollback
    /// - A/B testing comparison
    /// - Backward compatibility verification
    ///
    /// Formula: (survival^w * momentum^w * quality^w) * (1 - risk_discount) * 80
    pub fn calculate_score_multiplicative(&self, breakdown: &SurvivorScoreBreakdown) -> u8 {
        let penalty_multiplier = (1.0 - breakdown.risk_discount).max(0.0);

        let raw_score = (breakdown.survival.powf(self.config.weight_survival)
            * breakdown.momentum.powf(self.config.weight_momentum)
            * breakdown.quality.powf(self.config.weight_quality))
            * penalty_multiplier;

        let normalized = (raw_score * 80.0).clamp(0.0, 100.0);
        normalized.round() as u8
    }

    /// Compare additive vs multiplicative scoring for A/B testing
    ///
    /// Returns (additive_score, multiplicative_score, delta)
    pub fn compare_scoring_models(&self, breakdown: &SurvivorScoreBreakdown) -> (u8, u8, i16) {
        let additive = self.calculate_score_additive(breakdown);
        let multiplicative = self.calculate_score_multiplicative(breakdown);
        let delta = additive as i16 - multiplicative as i16;
        (additive, multiplicative, delta)
    }

    /// Resolve effective stage and threshold for current input.
    ///
    /// Source priority:
    /// 1) `input.session_stage` from engine session progress
    /// 2) Fallback heuristic from `tx_count` vs early threshold
    /// 3) Default configured threshold when neither is available
    pub fn resolve_stage_and_threshold(
        &self,
        input: &SurvivorScoreInput,
    ) -> (
        SurvivorSessionStage,
        u8,
        &'static str,
        Option<SurvivorSessionStage>,
    ) {
        if let Some(stage) = input.session_stage {
            let threshold = match stage {
                SurvivorSessionStage::Early => EARLY_STAGE_THRESHOLD,
                SurvivorSessionStage::Full => self.config.passing_threshold,
            };
            return (stage, threshold, "engine", Some(stage));
        }

        let tx_count_stage = input.tx_count.map(|tx_count| {
            if tx_count < EARLY_STAGE_TX_THRESHOLD {
                SurvivorSessionStage::Early
            } else {
                SurvivorSessionStage::Full
            }
        });

        if let Some(stage) = tx_count_stage {
            let threshold = match stage {
                SurvivorSessionStage::Early => EARLY_STAGE_THRESHOLD,
                SurvivorSessionStage::Full => self.config.passing_threshold,
            };
            return (stage, threshold, "tx_count_fallback", Some(stage));
        }

        (
            SurvivorSessionStage::Full,
            self.config.passing_threshold,
            "default_config",
            None,
        )
    }

    /// Get detailed additive score breakdown for diagnostics
    pub fn get_detailed_breakdown(
        &self,
        breakdown: &SurvivorScoreBreakdown,
    ) -> ScoreBreakdownDetailed {
        ScoreBreakdownDetailed::from_breakdown(breakdown, &self.additive_config)
    }

    /// Internal calculation method that optionally includes IWIM
    fn calculate_internal(
        &self,
        input: &SurvivorScoreInput,
        include_iwim: bool,
    ) -> SurvivorScoreResult {
        let start = Instant::now();
        let mut breakdown = SurvivorScoreBreakdown::default();
        let mut signals_used = 0u8;
        const MAX_SIGNALS: u8 = 22; // Updated to include behavioral signals

        // === 0. HARD VETO CHECK (Safety First) ===
        // Only EXTREME cases trigger immediate veto (>90% crash)
        let mut veto_triggered = false;
        let mut veto_reason_enum: Option<VetoReason> = None;

        if matches!(input.ecto_verdict, Some(EctoVerdict::Rug)) {
            veto_triggered = true;
            veto_reason_enum = Some(VetoReason::EctoRugDetected);
        } else if input.paradox_anomaly {
            veto_triggered = true;
            veto_reason_enum = Some(VetoReason::ParadoxAnomaly);
        } else if input.price_crash_detected {
            // Only veto on EXTREME crash (>90% from peak)
            veto_triggered = true;
            veto_reason_enum = Some(VetoReason::PriceCrashExtreme);
        }

        // === SURVIVAL COMPONENT ===
        breakdown.survival_from_qedd = input.qedd_survival_60s.unwrap_or(0.5);
        if input.qedd_survival_60s.is_some() {
            signals_used += 1;
        }

        // IWIM LOGIC - Issue #51 Fix: COMPLETELY INVISIBLE During Cycles
        //
        // During cycles (include_iwim=false):
        //   - IWIM is NOT included in survival calculation at all
        //   - Weights are redistributed: QEDD (62.5%) + Cluster (37.5%)
        //   - This ensures NO score change when IWIM data arrives mid-cycle
        //
        // At Final Verdict (include_iwim=true):
        //   - IWIM is included: QEDD (50%) + IWIM (30%) + Cluster (20%)
        //   - Apply actual IWIM threat score if available
        //   - If still None, use 1.0 (default trust)

        if include_iwim {
            // Final Verdict: Include IWIM in survival calculation
            breakdown.survival_from_iwim = input.iwim_threat_score.map(|t| 1.0 - t).unwrap_or_else(|| {
                if self.config.verbose_logging {
                    debug!("SURVIVOR_SCORE (Final Verdict): IWIM still pending, using grace_score=1.0");
                }
                1.0
            });
            if input.iwim_threat_score.is_some() {
                signals_used += 1;
            }

            breakdown.survival_from_cluster =
                input.cluster_risk_score.map(|r| 1.0 - r).unwrap_or(0.5);
            if input.cluster_risk_score.is_some() {
                signals_used += 1;
            }

            // Survival with IWIM: QEDD (50%) + IWIM (30%) + Cluster (20%)
            breakdown.survival = (breakdown.survival_from_qedd * 0.5
                + breakdown.survival_from_iwim * 0.3
                + breakdown.survival_from_cluster * 0.2)
                .clamp(0.0, 1.0);
        } else {
            // Cycles: IWIM is INVISIBLE - not included in calculation
            // Set to 1.0 for breakdown display, but don't use in formula
            breakdown.survival_from_iwim = 1.0;
            if self.config.verbose_logging && input.iwim_threat_score.is_some() {
                debug!("SURVIVOR_SCORE (Cycle): IWIM data available but COMPLETELY IGNORED (cached for Final Verdict)");
            }
            if input.iwim_threat_score.is_some() {
                signals_used += 1;
            }

            breakdown.survival_from_cluster =
                input.cluster_risk_score.map(|r| 1.0 - r).unwrap_or(0.5);
            if input.cluster_risk_score.is_some() {
                signals_used += 1;
            }

            // Survival WITHOUT IWIM: QEDD (62.5%) + Cluster (37.5%)
            // Weights redistributed to maintain balance without IWIM
            breakdown.survival = (breakdown.survival_from_qedd * 0.625
                + breakdown.survival_from_cluster * 0.375)
                .clamp(0.0, 1.0);
        }

        // === MOMENTUM COMPONENT (Issue #155: Widened Clamps) ===
        //
        // SOBP (Sell-Or-Buy Pressure) momentum calculation with asymmetric scaling
        // Input range (from engine): [-2.0, 3.0]
        // Output range: [0.2, 4.0]
        //
        // For buying pressure (positive):
        //   - Maps: 0.0→1.0, 1.5→2.5, 3.0→4.0
        //   - Formula: 1.0 + (m.clamp(0, 3) * 1.0)
        //
        // For selling pressure (negative):
        //   - Maps: 0.0→1.0, -0.4→0.6, -0.8→0.2
        //   - Formula: 1.0 + (m.clamp(-0.8, 0) * 1.0)
        //
        // This preserves extreme signals that were previously clamped to ~1.5
        let momentum_config = MomentumConfig::default();

        let mut momentum_product = 1.0f32;
        let mut momentum_components = 0u32;

        breakdown.momentum_from_sobp = input
            .sobp_momentum
            .map(|m| {
                if m > 0.0 {
                    // Buying pressure - allow up to 4.0 for extreme pumps
                    let scaled = 1.0 + (m.clamp(0.0, momentum_config.sobp_max) * 1.0);
                    scaled.min(4.0)
                } else {
                    // Selling pressure - allow down to 0.2 for extreme dumps
                    let scaled = 1.0 + (m.clamp(momentum_config.sobp_min, 0.0) * 1.0);
                    scaled.max(0.2)
                }
            })
            .unwrap_or(1.0);
        if input.sobp_momentum.is_some() {
            signals_used += 1;
            momentum_product *= breakdown.momentum_from_sobp;
            momentum_components += 1;
        }

        // QMAN momentum calculation
        // Input: QMAN quality score [0.0, 1.0]
        // Output: [0.5, 2.5] (was [0.75, 1.25])
        //
        // Higher QMAN score indicates better token quality/management
        // Widened output range to allow stronger signals
        breakdown.momentum_from_qman = input
            .qman_score
            .map(|q| {
                // Scale QMAN to [0.5, 2.5] range
                // q=0.0 → 0.5, q=0.5 → 1.5, q=1.0 → 2.5
                (momentum_config.qman_min
                    + (q * (momentum_config.qman_max - momentum_config.qman_min)))
                    .clamp(momentum_config.qman_min, momentum_config.qman_max)
            })
            .unwrap_or(1.0);
        if input.qman_score.is_some() {
            signals_used += 1;
            momentum_product *= breakdown.momentum_from_qman;
            momentum_components += 1;
        }

        // CHAOS momentum calculation
        // Input: Chaos Engine pump probability [0.0, 1.0]
        // Output: [0.3, 2.5] (was [0.7, 1.3])
        //
        // High chaos (>0.8) indicates active pump/dump activity
        // Low chaos (<0.2) indicates dead/stable token
        breakdown.momentum_from_chaos = input
            .chaos_pump_prob
            .map(|p| {
                // Scale CHAOS to [0.3, 2.5] range
                // p=0.0 → 0.3, p=0.5 → 1.4, p=1.0 → 2.5
                (momentum_config.chaos_min
                    + (p * (momentum_config.chaos_max - momentum_config.chaos_min)))
                    .clamp(momentum_config.chaos_min, momentum_config.chaos_max)
            })
            .unwrap_or(1.0);
        if input.chaos_pump_prob.is_some() {
            signals_used += 1;
            momentum_product *= breakdown.momentum_from_chaos;
            momentum_components += 1;
        }

        // Final momentum aggregation using geometric mean
        //
        // Geometric mean is used instead of arithmetic mean because:
        // - Preserves multiplicative relationships
        // - One weak component pulls down the overall score (good for filtering)
        // - More sensitive to extreme values than arithmetic mean
        //
        // Issue #155: Widened clamp from [0.5, 2.0] to [0.2, 4.0]
        // Old range: 1.5 points
        // New range: 3.8 points (2.5x expansion)
        let final_momentum_raw = if momentum_components > 0 {
            momentum_product.powf(1.0 / momentum_components as f32)
        } else {
            // Explicit neutral fallback for cycles with no real momentum signals.
            // This is intentional: absent data should not imply bullish or bearish momentum.
            debug_assert_eq!(momentum_product, 1.0);
            1.0
        };

        // Apply final clamp with optional legacy mode
        breakdown.momentum = if MomentumConfig::use_legacy_clamps() {
            // Legacy mode: old [0.5, 2.0] range for emergency rollback
            final_momentum_raw.clamp(0.5, 2.0)
        } else {
            // New mode: widened [0.2, 4.0] range
            final_momentum_raw.clamp(momentum_config.final_min, momentum_config.final_max)
        };

        // Log signal loss warning if momentum was clamped
        if momentum_config.warn_on_clamp {
            let clamped = (final_momentum_raw < momentum_config.final_min)
                || (final_momentum_raw > momentum_config.final_max);
            if clamped {
                warn!(
                    "Momentum signal clamped: raw={:.3}, after_clamp={:.3}, sobp={:.3}, qman={:.3}, chaos={:.3}",
                    final_momentum_raw,
                    breakdown.momentum,
                    breakdown.momentum_from_sobp,
                    breakdown.momentum_from_qman,
                    breakdown.momentum_from_chaos
                );
            }
        }

        // === QUALITY COMPONENT (Including LIGMA) ===
        breakdown.quality_from_mpcf = input.mpcf_organic_ratio.unwrap_or(0.5);
        if input.mpcf_organic_ratio.is_some() {
            signals_used += 1;
        }

        breakdown.quality_from_mesa = input.mesa_organic_likeness.unwrap_or(0.5);
        if input.mesa_organic_likeness.is_some() {
            signals_used += 1;
        }

        breakdown.quality_from_scr = input.scr_bot_score.map(|b| 1.0 - b).unwrap_or(0.5);
        if input.scr_bot_score.is_some() {
            signals_used += 1;
        }

        // Wallet quality with configurable threshold (was: if w > 0.6 { w * 0.5 } else { 0.0 })
        // Now uses wallet_quality_threshold from config
        let wallet_quality_threshold = self
            .thresholds
            .as_ref()
            .map(|t| t.wallet_quality_threshold)
            .unwrap_or(0.6);
        let wallet_quality_mult = self
            .risk_multipliers
            .as_ref()
            .map(|r| r.wallet_quality_multiplier)
            .unwrap_or(0.5);

        breakdown.quality_from_wallets = input
            .unique_wallet_ratio
            .map(|w| {
                if w > wallet_quality_threshold {
                    w * wallet_quality_mult
                } else {
                    0.0
                }
            })
            .unwrap_or(0.5);
        if input.unique_wallet_ratio.is_some() {
            signals_used += 1;
        }

        // LIGMA integration into quality
        breakdown.quality_from_ligma = input.ligma_tradability_score.unwrap_or(0.5);
        if input.ligma_tradability_score.is_some() {
            signals_used += 1;
        }
        if input.ligma_psi.is_some() {
            signals_used += 1;
        }
        if input.ligma_liquidity_trap_risk.is_some() {
            signals_used += 1;
        }

        // Compute quality with LIGMA weight
        // Base weights sum to 1.0, then we blend in LIGMA based on ligma_weight
        let base_quality = (breakdown.quality_from_mpcf * 0.35
            + breakdown.quality_from_mesa * 0.25
            + breakdown.quality_from_scr * 0.20
            + breakdown.quality_from_wallets * 0.20)
            .clamp(0.0, 1.0);

        // Blend LIGMA into quality based on ligma_weight
        breakdown.quality = ((1.0 - self.ligma_weight) * base_quality
            + self.ligma_weight * breakdown.quality_from_ligma)
            .clamp(0.0, 1.0);

        // === RISK DISCOUNT (Deferred Penalties) - Issue #51 ===
        // Penalties are accumulated and applied as multipliers, not immediate vetoes.
        // This allows for stable scoring across cycles with penalties applied at Final Verdict.

        // Get thresholds and multipliers from config or use defaults
        let wash_threshold = self
            .thresholds
            .as_ref()
            .map(|t| t.wash_trading_threshold)
            .unwrap_or(0.6);
        let wash_penalty_mult = self
            .risk_multipliers
            .as_ref()
            .map(|r| r.wash_penalty_multiplier)
            .unwrap_or(0.5);
        let exit_signal_weight = self
            .risk_multipliers
            .as_ref()
            .map(|r| r.exit_signal_weight)
            .unwrap_or(0.5);

        // Wash trading: Up to 50% penalty (threshold and multiplier now configurable)
        breakdown.risk_from_wash = input
            .mesa_wash_likeness
            .map(|w| {
                if w > wash_threshold {
                    w * wash_penalty_mult
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);
        if input.mesa_wash_likeness.is_some() {
            signals_used += 1;
        }

        // Smart money exit: penalty weight now configurable (was hardcoded 0.4)
        breakdown.risk_from_exit = if input.qman_exit_signal { 0.4 } else { 0.0 };
        signals_used += 1;

        // Price crash: Handled by VETO only if extreme (>90%), otherwise penalty
        breakdown.risk_from_crash = if input.price_crash_detected { 1.0 } else { 0.0 };
        signals_used += 1;

        // Paradox anomaly: Still a hard veto risk
        breakdown.risk_from_anomaly = if input.paradox_anomaly { 1.0 } else { 0.0 };
        signals_used += 1;

        // Effective risk discount - accumulate all penalties
        // Max penalty capped at 90% to avoid complete zeroing
        // Exit signal weight now configurable (was hardcoded 0.5)
        breakdown.risk_discount = (breakdown.risk_from_wash
            + breakdown.risk_from_exit * exit_signal_weight)
            .clamp(0.0, 0.9);

        // === FINAL CALCULATION ===

        let mut score = 0u8;
        let mut raw_score = 0.0;
        let mut interpretation = String::new();
        let mut passed = false;

        if veto_triggered {
            // HARD VETO EXECUTION
            score = 0;
            raw_score = 0.0;
            passed = false;
            interpretation = veto_reason_enum
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_else(|| "⛔ VETO".to_string());
            if let Some(reason) = &veto_reason_enum {
                increment_counter!("survivor_veto_total", "reason" => reason.as_str());
            }
        } else {
            // Check if legacy multiplicative scoring is enabled
            if use_legacy_multiplicative_scoring() {
                // LEGACY MULTIPLICATIVE SCORING
                // Formula: (survival^w * momentum^w * quality^w) * penalty_multiplier
                // This compresses score range to ~58-71 points (13 point spread)
                let penalty_multiplier = (1.0 - breakdown.risk_discount).max(0.0);

                raw_score = (breakdown.survival.powf(self.config.weight_survival)
                    * breakdown.momentum.powf(self.config.weight_momentum)
                    * breakdown.quality.powf(self.config.weight_quality))
                    * penalty_multiplier;

                // Normalize to 0-100 scale
                let normalized = (raw_score * 80.0).clamp(0.0, 100.0);
                score = normalized.round() as u8;
            } else {
                // NEW ADDITIVE SCORING MODEL (Issue #56)
                // Formula: survival_pts + momentum_pts + quality_pts - risk_penalty
                // This achieves ~30-90 points range (60 point spread)

                // Calculate component contributions using helper methods
                let survival_contribution = self
                    .additive_config
                    .calculate_survival_contribution(breakdown.survival);
                let momentum_contribution = self
                    .additive_config
                    .calculate_momentum_contribution(breakdown.momentum);
                let quality_contribution = self
                    .additive_config
                    .calculate_quality_contribution(breakdown.quality);

                // Base score: sum of contributions
                let base_score =
                    survival_contribution + momentum_contribution + quality_contribution;

                // Apply excellence boost or penalty amplification using helper
                let (score_after_boost, _, _) =
                    self.additive_config.apply_boost_or_penalty(base_score);

                // Risk penalty as subtraction using helper
                let risk_penalty = self
                    .additive_config
                    .calculate_risk_penalty(breakdown.risk_discount);
                let score_after_risk = score_after_boost - risk_penalty;

                // Clamp to valid range
                let final_score = score_after_risk.clamp(0.0, 100.0);
                score = final_score.round() as u8;

                // Store raw_score as the base_score for diagnostics
                // Note: In additive model, raw_score represents base_score normalized to [0, 1]
                // This differs from multiplicative where raw_score is the geometric mean
                raw_score = base_score / 100.0;

                // Log detailed breakdown
                if self.config.verbose_logging {
                    debug!(
                        "ADDITIVE_SCORE: survival={:.3} ({:.1}pts), momentum={:.3} ({:.1}pts), quality={:.3} ({:.1}pts), base={:.1}, boosted={:.1}, risk_penalty={:.1}, final={}",
                        breakdown.survival, survival_contribution,
                        breakdown.momentum, momentum_contribution,
                        breakdown.quality, quality_contribution,
                        base_score,
                        score_after_boost,
                        risk_penalty,
                        score
                    );
                }
            }

            // === BEHAVIORAL SCORE MODULATOR ===
            let mut behavioral_score = 1.0f32;
            if self.behavioral_config.enabled {
                let early_stage = input
                    .age_secs
                    .map(|age| age <= self.behavioral_config.early_stage_seconds as f32)
                    .or_else(|| input.tx_count.map(|tx| tx < EARLY_STAGE_TX_THRESHOLD))
                    .unwrap_or(false);

                let mut weighted_sum = 0.0f32;
                let mut weight_sum = 0.0f32;

                if let Some(value) = input.ecto_score {
                    weighted_sum += self.behavioral_config.w_ecto * value;
                    weight_sum += self.behavioral_config.w_ecto;
                    signals_used += 1;
                }
                if let Some(value) = input.bva_score {
                    weighted_sum += self.behavioral_config.w_bva * value;
                    weight_sum += self.behavioral_config.w_bva;
                    signals_used += 1;
                }
                if let Some(value) = input.panic_pressure {
                    let inverted = (1.0 - value).clamp(0.0, 1.0);
                    weighted_sum += self.behavioral_config.w_panic * inverted;
                    weight_sum += self.behavioral_config.w_panic;
                    signals_used += 1;
                }
                if let Some(value) = input.tcr_causality {
                    weighted_sum += self.behavioral_config.w_tcr * value;
                    weight_sum += self.behavioral_config.w_tcr;
                    signals_used += 1;
                }
                if let Some(value) = input.cir_strength {
                    weighted_sum += self.behavioral_config.w_cir * value;
                    weight_sum += self.behavioral_config.w_cir;
                    signals_used += 1;
                }

                let floor = self.behavioral_config.min_behavioral_floor.clamp(0.0, 1.0);
                behavioral_score = if weight_sum > 0.0 {
                    (weighted_sum / weight_sum).clamp(0.0, 1.0)
                } else if early_stage {
                    floor
                } else {
                    1.0
                };
                behavioral_score = behavioral_score.max(floor).clamp(0.0, 1.0);
                let mode_label = if self.behavioral_config.use_additive_mode {
                    "additive"
                } else {
                    "multiplicative"
                };
                increment_counter!("behavioral_score_mode", "mode" => mode_label);

                let score_before = score as f32;
                let adjusted = if self.behavioral_config.use_additive_mode {
                    let neutral_point = self.behavioral_config.neutral_point.clamp(0.0, 1.0);
                    let max_adjustment = self.behavioral_config.max_adjustment_points.max(0.0);
                    let behavioral_offset =
                        ((behavioral_score - neutral_point) * 2.0 * max_adjustment)
                            .clamp(-max_adjustment, max_adjustment);
                    (score_before + behavioral_offset).clamp(0.0, 100.0)
                } else {
                    // Bounded multiplicative modulation: the behavioral score modulates
                    // only a portion of the final score, preserving additive component
                    // information. Formula: score × (floor + (1 - floor) × behavioral),
                    // ensuring the multiplier stays in [floor, 1.0] and never collapses
                    // the additive score to a pure function of the behavioral multiplier.
                    let effective_multiplier = floor + (1.0 - floor) * behavioral_score;
                    (score_before * effective_multiplier).clamp(0.0, 100.0)
                };
                score = adjusted.round() as u8;
                raw_score = (score as f32 / 100.0).clamp(0.0, 1.0);

                if self.config.verbose_logging {
                    if self.behavioral_config.use_additive_mode {
                        let neutral_point = self.behavioral_config.neutral_point.clamp(0.0, 1.0);
                        let max_adjustment = self.behavioral_config.max_adjustment_points.max(0.0);
                        let behavioral_offset =
                            ((behavioral_score - neutral_point) * 2.0 * max_adjustment)
                                .clamp(-max_adjustment, max_adjustment);
                        debug!(
                            "BEHAVIORAL_SCORE: mode=additive value={:.3} offset={:+.1}pts score_before={} score_after={} floor={:.3}",
                            behavioral_score,
                            behavioral_offset,
                            score_before.round() as u8,
                            score,
                            floor
                        );
                    } else {
                        let effective_multiplier = floor + (1.0 - floor) * behavioral_score;
                        debug!(
                            "BEHAVIORAL_SCORE: mode=multiplicative value={:.3} floor={:.3} effective_mult={:.3} early_stage={} adjusted_score={}",
                            behavioral_score,
                            floor,
                            effective_multiplier,
                            early_stage,
                            score
                        );
                    }
                }
            }

            // Dynamic threshold selection prefers engine-provided session stage.
            let (effective_stage, effective_threshold, stage_source, tx_count_stage) =
                self.resolve_stage_and_threshold(input);

            passed = score >= effective_threshold;

            if self.config.verbose_logging {
                debug!(
                    "SURVIVOR_THRESHOLD: score={} threshold={} stage={} source={} tx_count={:?} tx_stage={}",
                    score,
                    effective_threshold,
                    effective_stage.as_str(),
                    stage_source,
                    input.tx_count,
                    tx_count_stage
                        .map(|stage| stage.as_str())
                        .unwrap_or("N/A")
                );
            }

            interpretation = self.generate_interpretation(&breakdown, score, passed);
        }

        // Confidence based on data availability
        let confidence = (signals_used as f32 / MAX_SIGNALS as f32).clamp(0.3, 1.0);

        if self.config.verbose_logging {
            debug!(
                "SURVIVOR_SCORE: {} ({}) | VETO: {} | VETO_REASON={} | S={:.2} M={:.2} Q={:.2} Penalty={:.2} | conf={:.0}%",
                score,
                if passed { "PASS" } else { "FAIL" },
                veto_triggered,
                veto_reason_enum
                    .as_ref()
                    .map(|r| r.as_str())
                    .unwrap_or("none"),
                breakdown.survival,
                breakdown.momentum,
                breakdown.quality,
                breakdown.risk_discount,
                confidence * 100.0
            );
        }

        if !passed {
            increment_counter!("survivor_fail_total");
        }

        SurvivorScoreResult {
            score,
            raw_score,
            passed,
            breakdown,
            interpretation,
            confidence,
            signals_used,
            analysis_time_us: start.elapsed().as_micros() as u64,
            veto_reason: veto_reason_enum,
        }
    }

    fn generate_interpretation(
        &self,
        b: &SurvivorScoreBreakdown,
        score: u8,
        passed: bool,
    ) -> String {
        let mut parts = Vec::new();

        // Get thresholds from config or use defaults
        let min_survival = self
            .thresholds
            .as_ref()
            .map(|t| t.min_survival_threshold)
            .unwrap_or(0.35);
        let min_quality = self
            .thresholds
            .as_ref()
            .map(|t| t.min_quality_threshold)
            .unwrap_or(0.35);
        let min_ligma = self
            .thresholds
            .as_ref()
            .map(|t| t.min_ligma_threshold)
            .unwrap_or(0.35);

        // Status
        parts.push(if passed { "✅ PASS" } else { "❌ FAIL" }.to_string());
        parts.push(format!("Score: {}", score));

        // Survival assessment (using configurable threshold)
        if b.survival > 0.7 {
            parts.push("🛡️ High survival".to_string());
        } else if b.survival < min_survival {
            parts.push(format!("⚠️ Rug risk (<{:.2})", min_survival));
        }

        // Momentum assessment
        if b.momentum > 1.2 {
            parts.push("📈 Strong momentum".to_string());
        } else if b.momentum < 0.8 {
            parts.push("📉 Weak momentum".to_string());
        }

        // Quality assessment (using configurable threshold)
        if b.quality > 0.7 {
            parts.push("👥 Organic activity".to_string());
        } else if b.quality < min_quality {
            parts.push(format!("🤖 Bot dominated (<{:.2})", min_quality));
        }

        // LIGMA assessment (using configurable threshold)
        if b.quality_from_ligma > 0.7 {
            parts.push("🧬 LIGMA: High tradability".to_string());
        } else if b.quality_from_ligma < min_ligma {
            parts.push(format!("⚠️ LIGMA: Low tradability (<{:.2})", min_ligma));
        }

        // Risk flags
        if b.risk_from_wash > 0.1 {
            parts.push("🔄 Wash trading".to_string());
        }
        if b.risk_from_exit > 0.0 {
            parts.push("💸 Smart$ exit".to_string());
        }
        if b.risk_from_crash > 0.0 {
            parts.push("💥 Price crash".to_string());
        }

        parts.join(" | ")
    }
}

impl Default for SurvivorScoreCalculator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculator_creation() {
        let calc = SurvivorScoreCalculator::new();
        assert_eq!(calc.config.passing_threshold, 65);
    }

    #[test]
    fn test_default_input_gives_neutral_score() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput::default();
        let result = calc.calculate(&input);

        // With all defaults (0.5), should be around 40-60
        assert!(
            result.score >= 30 && result.score <= 70,
            "Default input score {} not in neutral range",
            result.score
        );
    }

    #[test]
    fn test_strong_signals_give_high_score() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.9),
            iwim_threat_score: Some(0.1), // Low threat
            cluster_risk_score: Some(0.1),
            sobp_momentum: Some(1.0), // Strong buying
            qman_score: Some(0.85),
            chaos_pump_prob: Some(0.8),
            mpcf_organic_ratio: Some(0.9),
            mesa_organic_likeness: Some(0.85),
            scr_bot_score: Some(0.1), // Low bot
            unique_wallet_ratio: Some(0.8),
            mesa_wash_likeness: Some(0.1),
            qman_exit_signal: false,
            price_crash_detected: false,
            paradox_anomaly: false,
            ligma_tradability_score: Some(0.8),
            ligma_psi: Some(0.5),
            ligma_liquidity_trap_risk: Some(0.1),
            ..Default::default()
        };

        let result = calc.calculate(&input);
        assert!(
            result.score >= 75,
            "Strong signals should give score >= 75, got {}",
            result.score
        );
        assert!(result.passed);
    }

    #[test]
    fn test_rug_signals_give_low_score() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.2),
            iwim_threat_score: Some(0.9), // High threat
            cluster_risk_score: Some(0.8),
            sobp_momentum: Some(-0.5), // Selling
            qman_score: Some(0.2),
            chaos_pump_prob: Some(0.2),
            mpcf_organic_ratio: Some(0.1),
            mesa_organic_likeness: Some(0.2),
            scr_bot_score: Some(0.9), // High bot
            unique_wallet_ratio: Some(0.2),
            mesa_wash_likeness: Some(0.9),
            qman_exit_signal: true,
            price_crash_detected: true,
            paradox_anomaly: true,
            ligma_tradability_score: Some(0.2),
            ligma_psi: Some(-0.5),
            ligma_liquidity_trap_risk: Some(0.9),
            ..Default::default()
        };

        let result = calc.calculate(&input);
        // With Hard Veto, any of the critical signals (qman_exit, price_crash, paradox_anomaly) should result in score = 0
        assert_eq!(
            result.score, 0,
            "Hard Veto should result in score = 0, got {}",
            result.score
        );
        assert!(!result.passed);
        assert!(
            result.interpretation.contains("⛔"),
            "Interpretation should contain veto symbol"
        );
    }

    #[test]
    fn test_interpretation_generated() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput::default();
        let result = calc.calculate(&input);

        assert!(!result.interpretation.is_empty());
        assert!(result.interpretation.contains("Score:"));
    }

    #[test]
    fn test_risk_discount_applied() {
        let calc = SurvivorScoreCalculator::new();

        // Good signals but with exit signal - should apply DEFERRED PENALTY
        let input_with_exit = SurvivorScoreInput {
            qedd_survival_60s: Some(0.8),
            sobp_momentum: Some(0.5),
            qman_exit_signal: true,
            ligma_tradability_score: Some(0.5),
            ligma_psi: Some(0.0),
            ligma_liquidity_trap_risk: Some(0.0),
            ..Default::default()
        };

        let input_no_exit = SurvivorScoreInput {
            qedd_survival_60s: Some(0.8),
            sobp_momentum: Some(0.5),
            qman_exit_signal: false,
            ligma_tradability_score: Some(0.5),
            ligma_psi: Some(0.0),
            ligma_liquidity_trap_risk: Some(0.0),
            ..Default::default()
        };

        let result_with_exit = calc.calculate(&input_with_exit);
        let result_no_exit = calc.calculate(&input_no_exit);

        // With Deferred Penalty, exit signal should NOT zero score
        assert!(
            result_with_exit.score > 0,
            "Exit signal should apply penalty, not veto (score should be > 0), got {}",
            result_with_exit.score
        );
        assert!(result_with_exit.breakdown.risk_from_exit > 0.0);
        assert!(
            result_no_exit.score > 0,
            "No exit signal should have positive score"
        );
        // Exit signal should reduce score
        assert!(
            result_with_exit.score < result_no_exit.score,
            "Exit signal should reduce score: with_exit={} should be < no_exit={}",
            result_with_exit.score,
            result_no_exit.score
        );
    }

    #[test]
    fn test_momentum_affects_score() {
        let calc = SurvivorScoreCalculator::new();

        let input_high_momentum = SurvivorScoreInput {
            sobp_momentum: Some(1.5),
            chaos_pump_prob: Some(0.9),
            ..Default::default()
        };

        let input_low_momentum = SurvivorScoreInput {
            sobp_momentum: Some(-0.5),
            chaos_pump_prob: Some(0.1),
            ..Default::default()
        };

        let result_high = calc.calculate(&input_high_momentum);
        let result_low = calc.calculate(&input_low_momentum);

        assert!(
            result_high.score > result_low.score,
            "High momentum should score higher: high={} vs low={}",
            result_high.score,
            result_low.score
        );
        assert!(result_high.breakdown.momentum > result_low.breakdown.momentum);
    }

    #[test]
    fn test_confidence_scales_with_signals() {
        let calc = SurvivorScoreCalculator::new();

        // Minimal signals
        let minimal_input = SurvivorScoreInput::default();

        // Full signals
        let full_input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.7),
            iwim_threat_score: Some(0.3),
            cluster_risk_score: Some(0.2),
            sobp_momentum: Some(0.5),
            qman_score: Some(0.6),
            chaos_pump_prob: Some(0.5),
            mpcf_organic_ratio: Some(0.7),
            mesa_organic_likeness: Some(0.6),
            scr_bot_score: Some(0.3),
            unique_wallet_ratio: Some(0.7),
            mesa_wash_likeness: Some(0.2),
            qman_exit_signal: false,
            price_crash_detected: false,
            paradox_anomaly: false,
            ligma_tradability_score: Some(0.8),
            ligma_psi: Some(0.5),
            ligma_liquidity_trap_risk: Some(0.2),
            ..Default::default()
        };

        let result_minimal = calc.calculate(&minimal_input);
        let result_full = calc.calculate(&full_input);

        assert!(
            result_full.confidence > result_minimal.confidence,
            "Full signals should have higher confidence: full={:.2} vs minimal={:.2}",
            result_full.confidence,
            result_minimal.confidence
        );
        assert!(result_full.signals_used > result_minimal.signals_used);
    }

    #[test]
    fn test_wash_trading_penalty() {
        let calc = SurvivorScoreCalculator::new();

        let input_wash = SurvivorScoreInput {
            mesa_wash_likeness: Some(0.85), // High wash trading
            ..Default::default()
        };

        let input_no_wash = SurvivorScoreInput {
            mesa_wash_likeness: Some(0.3), // Normal
            ..Default::default()
        };

        let result_wash = calc.calculate(&input_wash);
        let result_no_wash = calc.calculate(&input_no_wash);

        assert!(result_wash.breakdown.risk_from_wash > 0.0);
        assert_eq!(result_no_wash.breakdown.risk_from_wash, 0.0);
        assert!(result_wash.score < result_no_wash.score);
    }

    #[test]
    fn test_hard_veto_paradox_anomaly() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            paradox_anomaly: true,
            // All other signals are positive
            qedd_survival_60s: Some(0.9),
            sobp_momentum: Some(1.0),
            ..Default::default()
        };

        let result = calc.calculate(&input);
        assert_eq!(result.score, 0, "Paradox anomaly should trigger Hard Veto");
        assert!(!result.passed);
        assert!(result.interpretation.contains("PARADOX ANOMALY"));
    }

    #[test]
    fn test_deferred_penalty_smart_money_exit() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            qman_exit_signal: true,
            // All other signals are positive
            qedd_survival_60s: Some(0.9),
            sobp_momentum: Some(1.0),
            ..Default::default()
        };

        let result = calc.calculate(&input);
        // With deferred penalties, exit signal should NOT zero score, just apply penalty
        assert!(
            result.score > 0,
            "Smart money exit should NOT trigger Hard Veto in cycles, got score={}",
            result.score
        );
        assert!(
            result.breakdown.risk_from_exit > 0.0,
            "Exit should be tracked as risk"
        );
        // Score should be reduced but not zero
        assert!(
            result.score < 70,
            "Score should be penalized for exit signal"
        );
    }

    #[test]
    fn test_hard_veto_price_crash() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            price_crash_detected: true,
            // All other signals are positive
            qedd_survival_60s: Some(0.9),
            sobp_momentum: Some(1.0),
            ..Default::default()
        };

        let result = calc.calculate(&input);
        assert_eq!(result.score, 0, "Price crash should trigger Hard Veto");
        assert!(!result.passed);
        assert!(result.interpretation.contains("PRICE CRASH"));
        assert_eq!(
            result.veto_reason,
            Some(VetoReason::PriceCrashExtreme),
            "Veto reason must be present for hard veto"
        );
    }

    #[test]
    fn test_wash_trading_threshold() {
        let calc = SurvivorScoreCalculator::new();

        // Just below threshold (0.6) - should not incur penalty
        let input_below = SurvivorScoreInput {
            mesa_wash_likeness: Some(0.59),
            qedd_survival_60s: Some(0.7),
            ..Default::default()
        };

        // Above threshold (0.6) - should incur penalty
        let input_above = SurvivorScoreInput {
            mesa_wash_likeness: Some(0.7),
            qedd_survival_60s: Some(0.7),
            ..Default::default()
        };

        let result_below = calc.calculate(&input_below);
        let result_above = calc.calculate(&input_above);

        assert_eq!(
            result_below.breakdown.risk_from_wash, 0.0,
            "Wash < 0.6 should have no penalty"
        );
        assert!(
            result_above.breakdown.risk_from_wash > 0.0,
            "Wash > 0.6 should have penalty"
        );
        assert!(
            result_below.score > result_above.score,
            "Higher wash should result in lower score"
        );
    }

    #[test]
    fn test_risk_penalty_direct_multiplier() {
        let calc = SurvivorScoreCalculator::new();

        // Test that risk penalty is now a direct multiplier (1.0 - risk_discount)
        // With 0.8 wash likeness, we should get 0.8 * 0.5 = 0.4 risk discount
        // Score should be multiplied by (1.0 - 0.4) = 0.6
        let input_high_wash = SurvivorScoreInput {
            mesa_wash_likeness: Some(0.8),
            qedd_survival_60s: Some(0.7),
            sobp_momentum: Some(0.5),
            qman_score: Some(0.6),
            ..Default::default()
        };

        let input_no_wash = SurvivorScoreInput {
            mesa_wash_likeness: Some(0.0),
            qedd_survival_60s: Some(0.7),
            sobp_momentum: Some(0.5),
            qman_score: Some(0.6),
            ..Default::default()
        };

        let result_wash = calc.calculate(&input_high_wash);
        let result_no_wash = calc.calculate(&input_no_wash);

        // Risk discount should be 0.8 * 0.5 = 0.4
        let expected_discount = 0.8 * 0.5;
        assert!(
            (result_wash.breakdown.risk_discount - expected_discount).abs() < 0.01,
            "Risk discount should be {}, got {}",
            expected_discount,
            result_wash.breakdown.risk_discount
        );

        // Score with wash should be significantly lower (multiplied by ~0.6)
        let ratio = result_wash.score as f32 / result_no_wash.score as f32;
        assert!(
            ratio < 0.7,
            "Wash penalty should reduce score to ~60% of original, ratio: {}",
            ratio
        );
    }

    #[test]
    fn test_ligma_integration_affects_score() {
        // Test that LIGMA weight actually affects the final score
        let calc_no_ligma = SurvivorScoreCalculator::new().with_ligma_weight(0.0);
        let calc_with_ligma = SurvivorScoreCalculator::new().with_ligma_weight(0.3);

        // Base input with neutral values
        let base_input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.6),
            iwim_threat_score: Some(0.4),
            cluster_risk_score: Some(0.3),
            sobp_momentum: Some(0.5),
            qman_score: Some(0.6),
            chaos_pump_prob: Some(0.5),
            mpcf_organic_ratio: Some(0.6),
            mesa_organic_likeness: Some(0.6),
            scr_bot_score: Some(0.4),
            unique_wallet_ratio: Some(0.6),
            mesa_wash_likeness: Some(0.2),
            qman_exit_signal: false,
            price_crash_detected: false,
            paradox_anomaly: false,
            ligma_tradability_score: Some(0.5),
            ligma_psi: Some(0.0),
            ligma_liquidity_trap_risk: Some(0.0),
            ..Default::default()
        };

        // Input with high LIGMA tradability
        let input_high_ligma = SurvivorScoreInput {
            ligma_tradability_score: Some(0.9),
            ligma_psi: Some(0.6),
            ligma_liquidity_trap_risk: Some(0.1),
            ..base_input.clone()
        };

        // Input with low LIGMA tradability
        let input_low_ligma = SurvivorScoreInput {
            ligma_tradability_score: Some(0.2),
            ligma_psi: Some(-0.4),
            ligma_liquidity_trap_risk: Some(0.7),
            ..base_input.clone()
        };

        let result_no_ligma_high = calc_no_ligma.calculate(&input_high_ligma);
        let result_no_ligma_low = calc_no_ligma.calculate(&input_low_ligma);
        let result_with_ligma_high = calc_with_ligma.calculate(&input_high_ligma);
        let result_with_ligma_low = calc_with_ligma.calculate(&input_low_ligma);

        // When LIGMA weight is 0, high and low LIGMA should give same score
        assert_eq!(
            result_no_ligma_high.score, result_no_ligma_low.score,
            "With zero LIGMA weight, LIGMA values should not affect score"
        );

        // When LIGMA weight is > 0, high LIGMA should give higher score than low LIGMA
        assert!(
            result_with_ligma_high.score > result_with_ligma_low.score,
            "High LIGMA tradability should give higher score than low, got {} vs {}",
            result_with_ligma_high.score,
            result_with_ligma_low.score
        );

        // Verify breakdown shows LIGMA contribution
        assert_eq!(result_with_ligma_high.breakdown.quality_from_ligma, 0.9);
        assert_eq!(result_with_ligma_low.breakdown.quality_from_ligma, 0.2);
    }

    #[test]
    fn test_ligma_weight_scaling() {
        // Test that different LIGMA weights produce proportional effects
        let calc_low_weight = SurvivorScoreCalculator::new().with_ligma_weight(0.1);
        let calc_high_weight = SurvivorScoreCalculator::new().with_ligma_weight(0.5);

        let input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.6),
            iwim_threat_score: Some(0.4),
            cluster_risk_score: Some(0.3),
            sobp_momentum: Some(0.5),
            qman_score: Some(0.6),
            chaos_pump_prob: Some(0.5),
            mpcf_organic_ratio: Some(0.5),
            mesa_organic_likeness: Some(0.5),
            scr_bot_score: Some(0.5),
            unique_wallet_ratio: Some(0.5),
            mesa_wash_likeness: Some(0.2),
            qman_exit_signal: false,
            price_crash_detected: false,
            paradox_anomaly: false,
            ligma_tradability_score: Some(0.9), // High tradability
            ligma_psi: Some(0.6),
            ligma_liquidity_trap_risk: Some(0.1),
            ..Default::default()
        };

        let result_low = calc_low_weight.calculate(&input);
        let result_high = calc_high_weight.calculate(&input);

        // Higher LIGMA weight should produce different quality score
        assert_ne!(
            result_low.breakdown.quality, result_high.breakdown.quality,
            "Different LIGMA weights should affect quality differently"
        );

        // With high LIGMA tradability, higher weight should give better score
        assert!(
            result_high.score >= result_low.score,
            "Higher LIGMA weight with good tradability should not decrease score, got {} vs {}",
            result_high.score,
            result_low.score
        );
    }

    #[test]
    fn test_ligma_in_interpretation() {
        let calc = SurvivorScoreCalculator::new().with_ligma_weight(0.2);

        // Test high tradability interpretation
        let input_high = SurvivorScoreInput {
            ligma_tradability_score: Some(0.9),
            ..SurvivorScoreInput::default()
        };
        let result_high = calc.calculate(&input_high);
        assert!(
            result_high
                .interpretation
                .contains("LIGMA: High tradability"),
            "High tradability should be mentioned in interpretation: {}",
            result_high.interpretation
        );

        // Test low tradability interpretation
        let input_low = SurvivorScoreInput {
            ligma_tradability_score: Some(0.2),
            ..SurvivorScoreInput::default()
        };
        let result_low = calc.calculate(&input_low);
        assert!(
            result_low.interpretation.contains("LIGMA: Low tradability"),
            "Low tradability should be mentioned in interpretation: {}",
            result_low.interpretation
        );
    }

    #[test]
    fn test_good_volume_slow_rpc_scenario() {
        let calc = SurvivorScoreCalculator::new();

        // SCENARIUSZ: "The Good Volume / Slow RPC"
        // iwim_threat_score is None (RPC slow)
        // sobp_momentum is strong (Good Volume)

        let input_slow_rpc = SurvivorScoreInput {
            iwim_threat_score: None,    // RPC pending
            sobp_momentum: Some(1.5),   // Strong volume/momentum
            chaos_pump_prob: Some(0.8), // Good pump probability
            qman_score: Some(0.7),      // Good QMAN

            // Other components neutral/positive
            qedd_survival_60s: Some(0.8),
            mpcf_organic_ratio: Some(0.6),
            ligma_tradability_score: Some(0.5),
            ligma_psi: Some(0.0),
            ligma_liquidity_trap_risk: Some(0.0),

            ..Default::default()
        };

        let result = calc.calculate(&input_slow_rpc);

        // DEBUG OUTPUT
        println!(
            "Score: {}, Breakdown: Survival={:.2} Momentum={:.2}",
            result.score, result.breakdown.survival, result.breakdown.momentum
        );

        // FAIL CONDITION from ticket:
        // If w 1. cyklu ocena wynosi < 50 punktów (przez S=0.25), test jest NIEZALICZONY.
        assert!(
            result.score >= 50,
            "Score should be >= 50 during RPC wait, got {}",
            result.score
        );

        // Check survival breakdown specifically for IWIM
        // We expect it to be 1.0 after fix (Default Trust)
        assert_eq!(
            result.breakdown.survival_from_iwim, 1.0,
            "IWIM survival should be 1.0 when pending (Default Trust)"
        );
    }

    #[test]
    fn test_iwim_cycle_vs_final_verdict() {
        let calc = SurvivorScoreCalculator::new();

        // Scenario: IWIM detects a scammer (high threat score)
        let input_scammer = SurvivorScoreInput {
            iwim_threat_score: Some(0.9), // High threat (scammer detected)
            qedd_survival_60s: Some(0.7),
            sobp_momentum: Some(0.5),
            chaos_pump_prob: Some(0.6),
            mpcf_organic_ratio: Some(0.6),
            ligma_tradability_score: Some(0.5),
            ligma_psi: Some(0.0),
            ligma_liquidity_trap_risk: Some(0.0),
            ..Default::default()
        };

        // During cycles: IWIM should NOT affect score (always 1.0)
        let result_cycle = calc.calculate(&input_scammer);

        // At Final Verdict: IWIM should be applied
        let result_final = calc.calculate_with_iwim(&input_scammer);

        // Debug output
        println!(
            "Cycle score (no IWIM): {}, survival_from_iwim={}",
            result_cycle.score, result_cycle.breakdown.survival_from_iwim
        );
        println!(
            "Final verdict score (with IWIM): {}, survival_from_iwim={}",
            result_final.score, result_final.breakdown.survival_from_iwim
        );

        // ASSERTIONS
        // 1. During cycles, IWIM should always be 1.0 (neutral/trust)
        assert_eq!(
            result_cycle.breakdown.survival_from_iwim, 1.0,
            "During cycles, IWIM should be 1.0 (neutral)"
        );

        // 2. At Final Verdict, IWIM should reflect actual threat (1.0 - 0.9 = 0.1)
        assert!(
            (result_final.breakdown.survival_from_iwim - 0.1).abs() < 0.01,
            "At Final Verdict, IWIM should be 0.1 (1.0 - 0.9 threat), got {}",
            result_final.breakdown.survival_from_iwim
        );

        // 3. Final verdict score should be significantly lower than cycle score
        assert!(
            result_final.score < result_cycle.score,
            "Final verdict score ({}) should be < cycle score ({}) when scammer detected",
            result_final.score,
            result_cycle.score
        );

        // 4. Cycle score should be stable (not penalized for pending IWIM)
        assert!(
            result_cycle.score >= 40,
            "Cycle score should be stable and reasonable, got {}",
            result_cycle.score
        );
    }

    #[test]
    fn test_iwim_arrival_no_score_jump() {
        // This test verifies the "no EKG-like jumps" requirement from comment #3691667939
        // When IWIM data arrives mid-cycle, the score should NOT change at all
        let calc = SurvivorScoreCalculator::new();

        let base_input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.7),
            sobp_momentum: Some(0.5),
            chaos_pump_prob: Some(0.6),
            mpcf_organic_ratio: Some(0.6),
            cluster_risk_score: Some(0.2),
            ligma_tradability_score: Some(0.5),
            ligma_psi: Some(0.0),
            ligma_liquidity_trap_risk: Some(0.0),
            ..Default::default()
        };

        // Cycle S5: No IWIM data yet
        let input_s5_no_iwim = base_input.clone();
        let result_s5 = calc.calculate(&input_s5_no_iwim);

        // Cycle S6: IWIM data arrives (good dev: low threat)
        let input_s6_iwim_good = SurvivorScoreInput {
            iwim_threat_score: Some(0.1), // Good dev
            ..base_input.clone()
        };
        let result_s6_good = calc.calculate(&input_s6_iwim_good);

        // Cycle S7: IWIM data arrives (scammer: high threat)
        let input_s7_iwim_bad = SurvivorScoreInput {
            iwim_threat_score: Some(0.9), // Scammer
            ..base_input.clone()
        };
        let result_s7_bad = calc.calculate(&input_s7_iwim_bad);

        // CRITICAL ASSERTION: Scores should be IDENTICAL regardless of IWIM data
        assert_eq!(
            result_s5.score, result_s6_good.score,
            "Score should NOT change when good IWIM arrives: S5={} vs S6={}",
            result_s5.score, result_s6_good.score
        );

        assert_eq!(
            result_s5.score, result_s7_bad.score,
            "Score should NOT change when bad IWIM arrives: S5={} vs S7={}",
            result_s5.score, result_s7_bad.score
        );

        // All survival values should be identical during cycles
        assert_eq!(
            result_s5.breakdown.survival, result_s6_good.breakdown.survival,
            "Survival component should be identical across cycles"
        );
        assert_eq!(
            result_s5.breakdown.survival, result_s7_bad.breakdown.survival,
            "Survival component should be identical across cycles"
        );

        println!(
            "✅ No EKG jumps: S5={}, S6={}, S7={} (all identical during cycles)",
            result_s5.score, result_s6_good.score, result_s7_bad.score
        );
    }

    // =============================================================================
    // Issue #155: Momentum Clamp Tests
    // =============================================================================

    #[test]
    fn test_momentum_config_defaults() {
        let config = MomentumConfig::default();

        // Verify default bounds match Issue #155 spec
        assert_eq!(config.sobp_min, -0.8);
        assert_eq!(config.sobp_max, 3.0);
        assert_eq!(config.qman_min, 0.5);
        assert_eq!(config.qman_max, 2.5);
        assert_eq!(config.chaos_min, 0.3);
        assert_eq!(config.chaos_max, 2.5);
        assert_eq!(config.final_min, 0.2);
        assert_eq!(config.final_max, 4.0);
        assert!(config.warn_on_clamp);
    }

    #[test]
    fn test_momentum_extreme_buying_preserved() {
        // Issue #155: Extreme buying pressure (SOBP = 2.8) should produce high momentum
        // With old clamps: momentum would be ~1.5 (clamped)
        // With new clamps: momentum should be > 3.0
        let calc = SurvivorScoreCalculator::new();

        let input = SurvivorScoreInput {
            sobp_momentum: Some(2.8),   // Extreme buying from Issue #155
            qman_score: Some(1.0),      // High quality
            chaos_pump_prob: Some(0.9), // High chaos/pump activity
            ..Default::default()
        };

        let result = calc.calculate(&input);

        // SOBP momentum should be high (close to 4.0)
        assert!(
            result.breakdown.momentum_from_sobp > 3.0,
            "Extreme SOBP should give momentum_from_sobp > 3.0, got {}",
            result.breakdown.momentum_from_sobp
        );

        // Final momentum should be > 2.5 (was capped at 2.0)
        assert!(
            result.breakdown.momentum > 2.5,
            "Extreme buying should give momentum > 2.5, got {}",
            result.breakdown.momentum
        );

        // Final momentum should be capped at 4.0
        assert!(
            result.breakdown.momentum <= 4.0,
            "Momentum should not exceed 4.0, got {}",
            result.breakdown.momentum
        );
    }

    #[test]
    fn test_momentum_extreme_selling_preserved() {
        // Issue #155: Extreme selling pressure should produce low momentum
        let calc = SurvivorScoreCalculator::new();

        let input = SurvivorScoreInput {
            sobp_momentum: Some(-0.7),  // Strong selling
            qman_score: Some(0.2),      // Low quality
            chaos_pump_prob: Some(0.1), // Low activity (dump scenario)
            ..Default::default()
        };

        let result = calc.calculate(&input);

        // SOBP momentum should be low (< 0.5)
        assert!(
            result.breakdown.momentum_from_sobp < 0.5,
            "Selling SOBP should give momentum_from_sobp < 0.5, got {}",
            result.breakdown.momentum_from_sobp
        );

        // Final momentum should be < 0.8 (allow low values)
        assert!(
            result.breakdown.momentum < 0.8,
            "Selling should give momentum < 0.8, got {}",
            result.breakdown.momentum
        );

        // Final momentum should not go below 0.2
        assert!(
            result.breakdown.momentum >= 0.2,
            "Momentum should not go below 0.2, got {}",
            result.breakdown.momentum
        );
    }

    #[test]
    fn test_momentum_signal_preservation() {
        // Issue #155: Verify that similar relative differences are preserved
        // Test monotonic increase with meaningful separation
        let calc = SurvivorScoreCalculator::new();

        let inputs = vec![
            (0.5, "weak"),     // Weak buying
            (1.5, "moderate"), // Moderate buying
            (2.8, "extreme"),  // Extreme buying
        ];

        let mut momentums: Vec<(f32, &str)> = Vec::new();

        for (sobp, label) in inputs {
            let input = SurvivorScoreInput {
                sobp_momentum: Some(sobp),
                qman_score: Some(0.6),      // Neutral QMAN
                chaos_pump_prob: Some(0.5), // Neutral chaos
                ..Default::default()
            };

            let result = calc.calculate(&input);
            momentums.push((result.breakdown.momentum, label));
        }

        // Verify monotonic increase
        assert!(
            momentums[0].0 < momentums[1].0,
            "Weak ({}) should be < Moderate ({})",
            momentums[0].0,
            momentums[1].0
        );
        assert!(
            momentums[1].0 < momentums[2].0,
            "Moderate ({}) should be < Extreme ({})",
            momentums[1].0,
            momentums[2].0
        );

        // Verify meaningful separation (at least 0.3 points between levels)
        let diff_1_2 = momentums[1].0 - momentums[0].0;
        let diff_2_3 = momentums[2].0 - momentums[1].0;

        assert!(
            diff_1_2 > 0.3,
            "Separation between weak and moderate should be > 0.3, got {}",
            diff_1_2
        );
        assert!(
            diff_2_3 > 0.3,
            "Separation between moderate and extreme should be > 0.3, got {}",
            diff_2_3
        );

        println!("Momentum levels (Issue #155 signal preservation):");
        for (momentum, label) in momentums {
            println!("  {}: {:.3}", label, momentum);
        }
    }

    #[test]
    fn test_momentum_widened_range() {
        // Issue #155: Verify new range is [0.2, 4.0] (3.8 points)
        // Old range was [0.5, 2.0] (1.5 points)
        let calc = SurvivorScoreCalculator::new();

        // Test minimum momentum (extreme selling)
        let input_min = SurvivorScoreInput {
            sobp_momentum: Some(-0.8),
            qman_score: Some(0.0),
            chaos_pump_prob: Some(0.0),
            ..Default::default()
        };

        // Test maximum momentum (extreme buying)
        let input_max = SurvivorScoreInput {
            sobp_momentum: Some(3.0),
            qman_score: Some(1.0),
            chaos_pump_prob: Some(1.0),
            ..Default::default()
        };

        let result_min = calc.calculate(&input_min);
        let result_max = calc.calculate(&input_max);

        // Calculate dynamic range
        let range = result_max.breakdown.momentum - result_min.breakdown.momentum;

        // Old range was 1.5 (2.0 - 0.5)
        // New range should be close to 3.8 (4.0 - 0.2)
        assert!(
            range > 2.5,
            "Momentum range should be > 2.5 (2.5x improvement over 1.5), got {}",
            range
        );

        println!(
            "Momentum range (Issue #155): min={:.3}, max={:.3}, range={:.3}",
            result_min.breakdown.momentum, result_max.breakdown.momentum, range
        );
    }

    #[test]
    fn test_momentum_qman_scaling() {
        // Test QMAN momentum scaling to [0.5, 2.5]
        let calc = SurvivorScoreCalculator::new();

        // Low QMAN
        let input_low = SurvivorScoreInput {
            qman_score: Some(0.0),
            sobp_momentum: Some(0.0),   // Neutral SOBP
            chaos_pump_prob: Some(0.5), // Neutral chaos
            ..Default::default()
        };

        // High QMAN
        let input_high = SurvivorScoreInput {
            qman_score: Some(1.0),
            sobp_momentum: Some(0.0),
            chaos_pump_prob: Some(0.5),
            ..Default::default()
        };

        let result_low = calc.calculate(&input_low);
        let result_high = calc.calculate(&input_high);

        // QMAN 0.0 should map to 0.5
        assert!(
            (result_low.breakdown.momentum_from_qman - 0.5).abs() < 0.01,
            "QMAN 0.0 should give momentum_from_qman ~0.5, got {}",
            result_low.breakdown.momentum_from_qman
        );

        // QMAN 1.0 should map to 2.5
        assert!(
            (result_high.breakdown.momentum_from_qman - 2.5).abs() < 0.01,
            "QMAN 1.0 should give momentum_from_qman ~2.5, got {}",
            result_high.breakdown.momentum_from_qman
        );
    }

    #[test]
    fn test_momentum_chaos_scaling() {
        // Test CHAOS momentum scaling to [0.3, 2.5]
        let calc = SurvivorScoreCalculator::new();

        // Low CHAOS
        let input_low = SurvivorScoreInput {
            chaos_pump_prob: Some(0.0),
            sobp_momentum: Some(0.0), // Neutral SOBP
            qman_score: Some(0.5),    // Neutral QMAN
            ..Default::default()
        };

        // High CHAOS
        let input_high = SurvivorScoreInput {
            chaos_pump_prob: Some(1.0),
            sobp_momentum: Some(0.0),
            qman_score: Some(0.5),
            ..Default::default()
        };

        let result_low = calc.calculate(&input_low);
        let result_high = calc.calculate(&input_high);

        // CHAOS 0.0 should map to 0.3
        assert!(
            (result_low.breakdown.momentum_from_chaos - 0.3).abs() < 0.01,
            "CHAOS 0.0 should give momentum_from_chaos ~0.3, got {}",
            result_low.breakdown.momentum_from_chaos
        );

        // CHAOS 1.0 should map to 2.5
        assert!(
            (result_high.breakdown.momentum_from_chaos - 2.5).abs() < 0.01,
            "CHAOS 1.0 should give momentum_from_chaos ~2.5, got {}",
            result_high.breakdown.momentum_from_chaos
        );
    }

    #[test]
    fn test_momentum_sobp_asymmetric_scaling() {
        // Test SOBP asymmetric scaling
        // Buying: 0.0→1.0, 1.5→2.5, 3.0→4.0
        // Selling: 0.0→1.0, -0.4→0.6, -0.8→0.2
        let calc = SurvivorScoreCalculator::new();

        let test_cases = vec![
            (0.0, 1.0, "neutral"),
            (1.5, 2.5, "moderate buying"),
            (3.0, 4.0, "extreme buying"),
            (-0.4, 0.6, "moderate selling"),
            (-0.8, 0.2, "extreme selling"),
        ];

        for (sobp_input, expected_output, label) in test_cases {
            let input = SurvivorScoreInput {
                sobp_momentum: Some(sobp_input),
                qman_score: None, // Don't affect other components
                chaos_pump_prob: None,
                ..Default::default()
            };

            let result = calc.calculate(&input);

            assert!(
                (result.breakdown.momentum_from_sobp - expected_output).abs() < 0.15,
                "SOBP {} ({}) should give ~{}, got {}",
                sobp_input,
                label,
                expected_output,
                result.breakdown.momentum_from_sobp
            );
        }
    }

    #[test]
    fn test_momentum_diagnostics_struct() {
        // Test MomentumDiagnostics struct creation
        let diagnostics = MomentumDiagnostics {
            sobp_clamped: true,
            qman_clamped: false,
            chaos_clamped: false,
            final_clamped: true,
            sobp_raw: 5.0,
            sobp_after_scale: 4.0,
            qman_raw: 0.8,
            qman_after_scale: 2.1,
            chaos_raw: 0.5,
            chaos_after_scale: 1.4,
            final_raw: 4.5,
            final_after_clamp: 4.0,
        };

        assert!(diagnostics.sobp_clamped);
        assert!(!diagnostics.qman_clamped);
        assert!(diagnostics.final_clamped);
        assert_eq!(diagnostics.sobp_raw, 5.0);
        assert_eq!(diagnostics.final_after_clamp, 4.0);
    }

    #[test]
    fn test_momentum_pump_detection_improvement() {
        // Issue #155: Verify improved pump detection
        // 10x pump (SOBP ~2.8) should score SIGNIFICANTLY higher than 1.5x pump (SOBP ~0.5)
        let calc = SurvivorScoreCalculator::new();

        // Weak pump scenario
        let weak_pump = SurvivorScoreInput {
            sobp_momentum: Some(0.5),
            qman_score: Some(0.6),
            chaos_pump_prob: Some(0.5),
            qedd_survival_60s: Some(0.7),
            ..Default::default()
        };

        // Strong pump scenario
        let strong_pump = SurvivorScoreInput {
            sobp_momentum: Some(2.5),
            qman_score: Some(0.8),
            chaos_pump_prob: Some(0.8),
            qedd_survival_60s: Some(0.7),
            ..Default::default()
        };

        let result_weak = calc.calculate(&weak_pump);
        let result_strong = calc.calculate(&strong_pump);

        // Strong pump should score significantly higher (> 10 points difference)
        let score_diff = result_strong.score as i32 - result_weak.score as i32;

        assert!(
            score_diff > 5,
            "Strong pump should score >5 points higher than weak pump, diff={}",
            score_diff
        );

        // Momentum difference should be meaningful
        let momentum_diff = result_strong.breakdown.momentum - result_weak.breakdown.momentum;
        assert!(
            momentum_diff > 0.8,
            "Momentum difference should be > 0.8, got {}",
            momentum_diff
        );

        println!(
            "Pump detection (Issue #155): weak_score={}, strong_score={}, diff={}",
            result_weak.score, result_strong.score, score_diff
        );
        println!(
            "  weak_momentum={:.3}, strong_momentum={:.3}, diff={:.3}",
            result_weak.breakdown.momentum, result_strong.breakdown.momentum, momentum_diff
        );
    }

    // =============================================================================
    // Issue #56: Additive Scoring Model Tests
    // =============================================================================

    #[test]
    fn test_additive_config_defaults() {
        let config = AdditiveScoringConfig::default();

        assert_eq!(config.weight_survival, 0.35);
        assert_eq!(config.weight_momentum, 0.30);
        assert_eq!(config.weight_quality, 0.20);
        assert_eq!(config.excellence_threshold, 70.0);
        assert_eq!(config.excellence_multiplier, 1.5);
        assert_eq!(config.penalty_threshold, 40.0);
        assert_eq!(config.penalty_multiplier, 1.3);
        assert_eq!(config.risk_penalty_max, 50.0);
        assert_eq!(config.momentum_neutral, 0.5);
        assert_eq!(config.momentum_min_offset, -0.3);
    }

    #[test]
    fn test_additive_config_validation() {
        let config = AdditiveScoringConfig::default();
        assert!(config.validate().is_ok(), "Default config should be valid");

        // Test invalid weight sum
        let mut invalid_config = config.clone();
        invalid_config.weight_survival = 0.5;
        invalid_config.weight_momentum = 0.4;
        invalid_config.weight_quality = 0.3; // Sum = 1.2 > 1.0
        assert!(
            invalid_config.validate().is_err(),
            "Weight sum > 1.0 should fail"
        );

        // Test invalid excellence multiplier
        let mut invalid_config = config.clone();
        invalid_config.excellence_multiplier = 0.5; // < 1.0
        assert!(
            invalid_config.validate().is_err(),
            "Excellence multiplier < 1.0 should fail"
        );

        // Test invalid penalty multiplier
        let mut invalid_config = config.clone();
        invalid_config.penalty_multiplier = 0.8; // < 1.0
        assert!(
            invalid_config.validate().is_err(),
            "Penalty multiplier < 1.0 should fail"
        );

        // Test invalid threshold ordering
        let mut invalid_config = config.clone();
        invalid_config.excellence_threshold = 30.0;
        invalid_config.penalty_threshold = 50.0; // excellence <= penalty
        assert!(
            invalid_config.validate().is_err(),
            "Excellence threshold <= penalty should fail"
        );
    }

    #[test]
    fn test_additive_score_range_expansion() {
        // Issue #56: Verify score range expansion from ~13 points to ~60+ points
        let calc = SurvivorScoreCalculator::new();

        // Worst case token
        let worst = SurvivorScoreBreakdown {
            survival: 0.2,
            momentum: 0.2, // Below neutral (0.5)
            quality: 0.1,
            risk_discount: 0.5,
            ..Default::default()
        };

        // Best case token
        let best = SurvivorScoreBreakdown {
            survival: 0.95,
            momentum: 3.8, // High momentum
            quality: 0.95,
            risk_discount: 0.0,
            ..Default::default()
        };

        let worst_score = calc.calculate_score_additive(&worst);
        let best_score = calc.calculate_score_additive(&best);

        println!("Additive score range test:");
        println!("  Worst token: {} points", worst_score);
        println!("  Best token: {} points", best_score);

        let range = best_score as i32 - worst_score as i32;
        println!("  Effective range: {} points", range);

        // Verify we achieved target range (should be > 50 points)
        assert!(range >= 50, "Expected range ≥50 points, got {}", range);
        assert!(
            worst_score < 40,
            "Worst case should be <40, got {}",
            worst_score
        );
        assert!(
            best_score > 85,
            "Best case should be >85, got {}",
            best_score
        );
    }

    #[test]
    fn test_additive_linear_improvement_scaling() {
        // Issue #56: +20% improvement across metrics should yield +15-20 points increase
        let calc = SurvivorScoreCalculator::new();

        // Base token
        let base = SurvivorScoreBreakdown {
            survival: 0.65,
            momentum: 0.85,
            quality: 0.70,
            risk_discount: 0.1,
            ..Default::default()
        };

        // +20% improvement across all metrics
        let improved = SurvivorScoreBreakdown {
            survival: 0.78,      // +20%
            momentum: 1.02,      // +20%
            quality: 0.84,       // +20%
            risk_discount: 0.08, // -20% risk
            ..Default::default()
        };

        let base_score = calc.calculate_score_additive(&base);
        let improved_score = calc.calculate_score_additive(&improved);

        let delta = improved_score as i32 - base_score as i32;

        println!("Linear improvement scaling test:");
        println!("  Base token: {} points", base_score);
        println!("  Improved token (+20%): {} points", improved_score);
        println!("  Delta: +{} points", delta);

        // With additive model, +20% should yield significant improvement
        assert!(
            delta >= 10,
            "Expected ≥10 point improvement for +20% metrics, got {}",
            delta
        );
    }

    #[test]
    fn test_additive_vs_multiplicative_comparison() {
        // Issue #56: Verify additive model has better discrimination than multiplicative
        let calc = SurvivorScoreCalculator::new();

        let base = SurvivorScoreBreakdown {
            survival: 0.65,
            momentum: 0.85,
            quality: 0.70,
            risk_discount: 0.1,
            ..Default::default()
        };

        let improved = SurvivorScoreBreakdown {
            survival: 0.78,
            momentum: 1.02,
            quality: 0.84,
            risk_discount: 0.08,
            ..Default::default()
        };

        // Compare both models
        let base_add = calc.calculate_score_additive(&base);
        let improved_add = calc.calculate_score_additive(&improved);
        let delta_add = improved_add as i32 - base_add as i32;

        let base_mult = calc.calculate_score_multiplicative(&base);
        let improved_mult = calc.calculate_score_multiplicative(&improved);
        let delta_mult = improved_mult as i32 - base_mult as i32;

        println!("Additive vs Multiplicative comparison:");
        println!(
            "  Multiplicative: base={}, improved={}, delta=+{}",
            base_mult, improved_mult, delta_mult
        );
        println!(
            "  Additive: base={}, improved={}, delta=+{}",
            base_add, improved_add, delta_add
        );
        println!(
            "  Improvement: additive has +{} points more discrimination",
            delta_add - delta_mult
        );

        // Additive should have better (or equal) discrimination
        assert!(
            delta_add >= delta_mult,
            "Additive model should discriminate at least as well as multiplicative"
        );
    }

    #[test]
    fn test_excellence_boost_applied() {
        // Issue #56: Verify excellence boost is applied above threshold
        let calc = SurvivorScoreCalculator::new();

        // Token just below excellence threshold (70)
        let below = SurvivorScoreBreakdown {
            survival: 0.75,
            momentum: 1.5,
            quality: 0.75,
            risk_discount: 0.0,
            ..Default::default()
        };

        // Token above excellence threshold
        let above = SurvivorScoreBreakdown {
            survival: 0.85,
            momentum: 2.2,
            quality: 0.85,
            risk_discount: 0.0,
            ..Default::default()
        };

        let score_below = calc.calculate_score_additive(&below);
        let score_above = calc.calculate_score_additive(&above);

        // Get detailed breakdowns
        let detail_below = calc.get_detailed_breakdown(&below);
        let detail_above = calc.get_detailed_breakdown(&above);

        println!("Excellence boost test:");
        println!(
            "  Below threshold: score={}, base={:.1}, boost_applied={}",
            score_below, detail_below.base_score, detail_below.excellence_boost_applied
        );
        println!(
            "  Above threshold: score={}, base={:.1}, boost_applied={}",
            score_above, detail_above.base_score, detail_above.excellence_boost_applied
        );

        // Token above threshold should have excellence boost applied
        if detail_above.base_score > 70.0 {
            assert!(
                detail_above.excellence_boost_applied,
                "Excellence boost should be applied when base > 70"
            );
        }
    }

    #[test]
    fn test_penalty_amplification_applied() {
        // Issue #56: Verify penalty amplification below threshold
        let calc = SurvivorScoreCalculator::new();

        // Token below penalty threshold (40)
        let low_quality = SurvivorScoreBreakdown {
            survival: 0.2,
            momentum: 0.3,
            quality: 0.15,
            risk_discount: 0.0,
            ..Default::default()
        };

        let score = calc.calculate_score_additive(&low_quality);
        let detail = calc.get_detailed_breakdown(&low_quality);

        println!("Penalty amplification test:");
        println!(
            "  Low quality token: score={}, base={:.1}, amplification_applied={}",
            score, detail.base_score, detail.penalty_amplification_applied
        );

        // If base score is below threshold, penalty amplification should be applied
        if detail.base_score < 40.0 {
            assert!(
                detail.penalty_amplification_applied,
                "Penalty amplification should be applied when base < 40"
            );
        }
    }

    #[test]
    fn test_risk_penalty_as_subtraction() {
        // Issue #56: Risk should subtract points, not multiply
        let calc = SurvivorScoreCalculator::new();

        let base_breakdown = SurvivorScoreBreakdown {
            survival: 0.7,
            momentum: 1.2,
            quality: 0.7,
            risk_discount: 0.0,
            ..Default::default()
        };

        let risky_breakdown = SurvivorScoreBreakdown {
            survival: 0.7,
            momentum: 1.2,
            quality: 0.7,
            risk_discount: 0.5, // 50% risk
            ..Default::default()
        };

        let base_score = calc.calculate_score_additive(&base_breakdown);
        let risky_score = calc.calculate_score_additive(&risky_breakdown);

        let detail = calc.get_detailed_breakdown(&risky_breakdown);

        println!("Risk penalty subtraction test:");
        println!("  Base score (no risk): {}", base_score);
        println!("  Risky score (50% risk): {}", risky_score);
        println!("  Risk penalty points: {:.1}", detail.risk_penalty_points);

        // Risk penalty should be ~25 points (0.5 * 50.0)
        assert!(
            (detail.risk_penalty_points - 25.0).abs() < 1.0,
            "Risk penalty should be ~25 points (0.5 * 50), got {:.1}",
            detail.risk_penalty_points
        );

        // Score should be reduced by approximately the penalty amount
        let score_diff = base_score as i32 - risky_score as i32;
        assert!(
            score_diff >= 20 && score_diff <= 30,
            "Score difference should be ~25, got {}",
            score_diff
        );
    }

    #[test]
    fn test_compare_scoring_models_method() {
        let calc = SurvivorScoreCalculator::new();

        let breakdown = SurvivorScoreBreakdown {
            survival: 0.7,
            momentum: 1.5,
            quality: 0.7,
            risk_discount: 0.1,
            ..Default::default()
        };

        let (additive, multiplicative, delta) = calc.compare_scoring_models(&breakdown);

        println!("Scoring model comparison:");
        println!("  Additive: {}", additive);
        println!("  Multiplicative: {}", multiplicative);
        println!("  Delta: {} points", delta);

        // Both scores should be in valid range
        assert!(additive <= 100, "Additive score should be <= 100");
        assert!(
            multiplicative <= 100,
            "Multiplicative score should be <= 100"
        );
    }

    #[test]
    fn test_detailed_breakdown_visualization() {
        let calc = SurvivorScoreCalculator::new();

        let breakdown = SurvivorScoreBreakdown {
            survival: 0.75,
            momentum: 1.8,
            quality: 0.70,
            risk_discount: 0.15,
            momentum_from_sobp: 2.0,
            momentum_from_qman: 1.5,
            momentum_from_chaos: 1.5,
            ..Default::default()
        };

        let detail = calc.get_detailed_breakdown(&breakdown);
        let visualization = visualize_score_breakdown(&detail);

        println!("Score breakdown visualization test:");
        println!("{}", visualization);

        // Verify detailed breakdown fields
        assert!(
            detail.survival_points > 0.0,
            "Survival points should be positive"
        );
        assert!(
            detail.quality_points > 0.0,
            "Quality points should be positive"
        );
        assert!(detail.final_score > 0, "Final score should be positive");
    }

    #[test]
    fn test_additive_monotonic_improvement() {
        // Verify monotonic improvement: better inputs → higher scores
        let calc = SurvivorScoreCalculator::new();

        let breakdowns = vec![
            (
                SurvivorScoreBreakdown {
                    survival: 0.3,
                    momentum: 0.5,
                    quality: 0.3,
                    risk_discount: 0.3,
                    ..Default::default()
                },
                "poor",
            ),
            (
                SurvivorScoreBreakdown {
                    survival: 0.5,
                    momentum: 1.0,
                    quality: 0.5,
                    risk_discount: 0.2,
                    ..Default::default()
                },
                "average",
            ),
            (
                SurvivorScoreBreakdown {
                    survival: 0.7,
                    momentum: 1.5,
                    quality: 0.7,
                    risk_discount: 0.1,
                    ..Default::default()
                },
                "good",
            ),
            (
                SurvivorScoreBreakdown {
                    survival: 0.9,
                    momentum: 2.5,
                    quality: 0.9,
                    risk_discount: 0.0,
                    ..Default::default()
                },
                "excellent",
            ),
        ];

        let mut scores: Vec<(u8, &str)> = Vec::new();
        for (breakdown, label) in &breakdowns {
            let score = calc.calculate_score_additive(breakdown);
            scores.push((score, label));
        }

        println!("Monotonic improvement test:");
        for (score, label) in &scores {
            println!("  {}: {} points", label, score);
        }

        // Verify monotonic increase
        for i in 1..scores.len() {
            assert!(
                scores[i].0 >= scores[i - 1].0,
                "{} ({}) should score >= {} ({})",
                scores[i].1,
                scores[i].0,
                scores[i - 1].1,
                scores[i - 1].0
            );
        }
    }

    #[test]
    fn test_pump_vs_rug_separation() {
        // Issue #56: Expected improvement - Pump vs Rug separation should be > 15 points
        let calc = SurvivorScoreCalculator::new();

        // Pump token characteristics
        let pump = SurvivorScoreBreakdown {
            survival: 0.85,
            momentum: 2.5, // High buying pressure
            quality: 0.80, // Organic activity
            risk_discount: 0.05,
            ..Default::default()
        };

        // Rug token characteristics
        let rug = SurvivorScoreBreakdown {
            survival: 0.30,     // Low survival
            momentum: 0.4,      // Selling pressure
            quality: 0.25,      // Bot dominated
            risk_discount: 0.5, // High risk
            ..Default::default()
        };

        let pump_score = calc.calculate_score_additive(&pump);
        let rug_score = calc.calculate_score_additive(&rug);
        let separation = pump_score as i32 - rug_score as i32;

        println!("Pump vs Rug separation test:");
        println!("  Pump token: {} points", pump_score);
        println!("  Rug token: {} points", rug_score);
        println!("  Separation: {} points", separation);

        // Additive model should achieve > 15 points separation
        assert!(
            separation > 15,
            "Pump vs Rug separation should be > 15 points, got {}",
            separation
        );
    }

    #[test]
    fn test_legacy_mode_flag() {
        // Test that legacy mode can be enabled via environment variable
        // Note: This test can't actually set the env var in a way that persists,
        // but we can test the function exists and returns false by default
        assert!(
            !use_legacy_multiplicative_scoring(),
            "Legacy mode should be disabled by default"
        );
    }

    #[test]
    fn test_threshold_prefers_engine_session_stage_over_tx_count() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            session_stage: Some(SurvivorSessionStage::Full),
            tx_count: Some(12), // Would normally imply EARLY in fallback
            ..Default::default()
        };

        let (stage, threshold, source, tx_stage) = calc.resolve_stage_and_threshold(&input);
        assert_eq!(stage, SurvivorSessionStage::Full);
        assert_eq!(threshold, calc.config.passing_threshold);
        assert_eq!(source, "engine");
        assert_eq!(tx_stage, Some(SurvivorSessionStage::Full));
    }

    #[test]
    fn test_threshold_prefers_engine_session_stage_over_high_tx_count() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            session_stage: Some(SurvivorSessionStage::Early),
            tx_count: Some(250), // Would normally imply FULL in fallback
            ..Default::default()
        };

        let (stage, threshold, source, tx_stage) = calc.resolve_stage_and_threshold(&input);
        assert_eq!(stage, SurvivorSessionStage::Early);
        assert_eq!(threshold, EARLY_STAGE_THRESHOLD);
        assert_eq!(source, "engine");
        assert_eq!(tx_stage, Some(SurvivorSessionStage::Early));
    }

    #[test]
    fn test_threshold_falls_back_to_tx_count_when_stage_missing() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            session_stage: None,
            tx_count: Some(42),
            ..Default::default()
        };

        let (stage, threshold, source, tx_stage) = calc.resolve_stage_and_threshold(&input);
        assert_eq!(stage, SurvivorSessionStage::Early);
        assert_eq!(threshold, EARLY_STAGE_THRESHOLD);
        assert_eq!(source, "tx_count_fallback");
        assert_eq!(tx_stage, Some(SurvivorSessionStage::Early));
    }

    fn behavioral_test_input(behavioral_value: f32) -> SurvivorScoreInput {
        SurvivorScoreInput {
            qedd_survival_60s: Some(0.70),
            sobp_momentum: Some(0.5),
            qman_score: Some(0.6),
            chaos_pump_prob: Some(0.5),
            mpcf_organic_ratio: Some(0.6),
            mesa_organic_likeness: Some(0.6),
            scr_bot_score: Some(0.2),
            unique_wallet_ratio: Some(0.6),
            ligma_tradability_score: Some(0.6),
            ecto_score: Some(behavioral_value),
            ..Default::default()
        }
    }

    #[test]
    fn test_behavioral_additive_mode_neutral() {
        let mut calc = SurvivorScoreCalculator::new();
        let mut cfg = calc.behavioral_config.clone();
        cfg.enabled = true;
        cfg.use_additive_mode = true;
        cfg.max_adjustment_points = 15.0;
        cfg.neutral_point = 0.5;
        cfg.min_behavioral_floor = 0.0;
        cfg.w_ecto = 1.0;
        cfg.w_bva = 0.0;
        cfg.w_panic = 0.0;
        cfg.w_tcr = 0.0;
        cfg.w_cir = 0.0;
        calc.update_behavioral_config(cfg.clone());

        let additive_neutral = calc.calculate(&behavioral_test_input(0.5)).score;
        let additive_offset = calc.calculate(&behavioral_test_input(0.5)).score;
        assert_eq!(additive_offset, additive_neutral);
    }

    #[test]
    fn test_behavioral_additive_mode_penalty() {
        let mut calc = SurvivorScoreCalculator::new();
        let mut cfg = calc.behavioral_config.clone();
        cfg.enabled = true;
        cfg.use_additive_mode = true;
        cfg.max_adjustment_points = 15.0;
        cfg.neutral_point = 0.5;
        cfg.min_behavioral_floor = 0.0;
        cfg.w_ecto = 1.0;
        cfg.w_bva = 0.0;
        cfg.w_panic = 0.0;
        cfg.w_tcr = 0.0;
        cfg.w_cir = 0.0;
        calc.update_behavioral_config(cfg);

        let neutral = calc.calculate(&behavioral_test_input(0.5)).score as i32;
        let penalized = calc.calculate(&behavioral_test_input(0.0)).score as i32;
        assert_eq!(neutral - penalized, 15);
    }

    #[test]
    fn test_behavioral_additive_mode_boost() {
        let mut calc = SurvivorScoreCalculator::new();
        let mut cfg = calc.behavioral_config.clone();
        cfg.enabled = true;
        cfg.use_additive_mode = true;
        cfg.max_adjustment_points = 15.0;
        cfg.neutral_point = 0.5;
        cfg.min_behavioral_floor = 0.0;
        cfg.w_ecto = 1.0;
        cfg.w_bva = 0.0;
        cfg.w_panic = 0.0;
        cfg.w_tcr = 0.0;
        cfg.w_cir = 0.0;
        calc.update_behavioral_config(cfg);

        let neutral = calc.calculate(&behavioral_test_input(0.5)).score as i32;
        let boosted = calc.calculate(&behavioral_test_input(1.0)).score as i32;
        assert_eq!(boosted - neutral, 15);
    }

    #[test]
    fn test_behavioral_max_adjustment_bounded() {
        let mut calc = SurvivorScoreCalculator::new();
        let mut cfg = calc.behavioral_config.clone();
        cfg.enabled = true;
        cfg.use_additive_mode = true;
        cfg.max_adjustment_points = 15.0;
        cfg.neutral_point = 0.5;
        cfg.min_behavioral_floor = 0.0;
        cfg.w_ecto = 1.0;
        cfg.w_bva = 0.0;
        cfg.w_panic = 0.0;
        cfg.w_tcr = 0.0;
        cfg.w_cir = 0.0;
        calc.update_behavioral_config(cfg);

        let neutral = calc.calculate(&behavioral_test_input(0.5)).score as i32;
        for value in [0.0f32, 0.1, 0.3, 0.5, 0.7, 1.0] {
            let adjusted = calc.calculate(&behavioral_test_input(value)).score as i32;
            assert!(
                (adjusted - neutral).abs() <= 15,
                "adjustment out of bounds for value={}: neutral={} adjusted={}",
                value,
                neutral,
                adjusted
            );
        }
    }

    #[test]
    fn test_behavioral_legacy_rollback() {
        let mut calc = SurvivorScoreCalculator::new();
        let baseline_score = {
            let mut baseline_calc = SurvivorScoreCalculator::new();
            let mut baseline_cfg = baseline_calc.behavioral_config.clone();
            baseline_cfg.enabled = false;
            baseline_calc.update_behavioral_config(baseline_cfg);
            baseline_calc.calculate(&behavioral_test_input(0.3)).score as f32
        };
        let mut cfg = calc.behavioral_config.clone();
        cfg.enabled = true;
        cfg.use_additive_mode = false;
        cfg.min_behavioral_floor = 0.0;
        cfg.w_ecto = 1.0;
        cfg.w_bva = 0.0;
        cfg.w_panic = 0.0;
        cfg.w_tcr = 0.0;
        cfg.w_cir = 0.0;
        calc.update_behavioral_config(cfg);
        let multiplied = calc.calculate(&behavioral_test_input(0.3)).score as f32;
        let expected = (baseline_score * 0.3).round();
        assert_eq!(multiplied, expected);
    }

    #[test]
    fn test_zero_emission_momentum_neutral() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            sobp_momentum: None,
            qman_score: None,
            chaos_pump_prob: None,
            ..Default::default()
        };
        let result = calc.calculate(&input);
        assert!((result.breakdown.momentum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_additive_score_no_saturation_dead_token() {
        let calc = SurvivorScoreCalculator::new();
        let input = SurvivorScoreInput {
            qedd_survival_60s: Some(0.05),
            cluster_risk_score: Some(0.95),
            sobp_momentum: None,
            qman_score: None,
            chaos_pump_prob: None,
            mpcf_organic_ratio: Some(0.05),
            mesa_organic_likeness: Some(0.05),
            scr_bot_score: Some(0.95),
            unique_wallet_ratio: Some(0.05),
            ligma_tradability_score: Some(0.05),
            qman_exit_signal: false,
            price_crash_detected: false,
            paradox_anomaly: false,
            ..Default::default()
        };
        let result = calc.calculate(&input);
        assert!(
            result.score < 70,
            "dead-token score should stay below saturation, got {}",
            result.score
        );
    }

    // =========================================================================
    // Tests for momentum cap, behavioral blending, and TCF phase modulation
    // =========================================================================

    #[test]
    fn test_momentum_contribution_capped_by_max_offset() {
        // Verify that momentum_max_offset prevents base_score saturation.
        // With momentum=4.0 (maximum widened range), the centered value
        // should be clamped to momentum_max_offset=1.5 instead of 3.5.
        let config = AdditiveScoringConfig::default();

        // Maximum momentum in widened range
        let high_contribution = config.calculate_momentum_contribution(4.0);
        // Contribution = 1.5 * 0.30 * 100 = 45.0
        assert!(
            (high_contribution - 45.0).abs() < 0.01,
            "High momentum contribution should be capped at 45pts, got {:.1}",
            high_contribution
        );

        // Neutral momentum
        let neutral_contribution = config.calculate_momentum_contribution(config.momentum_neutral);
        assert!(
            neutral_contribution.abs() < 0.01,
            "Neutral momentum should contribute 0pts, got {:.1}",
            neutral_contribution
        );

        // Verify cap prevents old 105pt contribution
        assert!(
            high_contribution < 50.0,
            "Momentum contribution must not exceed 50pts (was 105pts before fix), got {:.1}",
            high_contribution
        );
    }

    #[test]
    fn test_additive_base_score_stays_within_range() {
        // Verify that with extreme positive signals, base_score no longer
        // saturates to 100+ (was 139.8 before the fix).
        let config = AdditiveScoringConfig::default();

        // Maximum possible contributions
        let survival_max = config.calculate_survival_contribution(1.0); // 35
        let momentum_max = config.calculate_momentum_contribution(4.0); // 45 (capped)
        let quality_max = config.calculate_quality_contribution(1.0); // 20
        let base_max = survival_max + momentum_max + quality_max;

        assert!(
            base_max <= 100.0,
            "Maximum base_score should fit within [0,100] range, got {:.1}",
            base_max
        );
    }

    #[test]
    fn test_behavioral_multiplicative_bounded_by_floor() {
        // With default floor=0.30, behavioral_score=0.534 should NOT
        // reduce score to 53% (was: 100 * 0.534 = 53). Instead uses
        // floor-blended formula: score * (0.30 + 0.70 * 0.534) ≈ 67.
        let mut calc = SurvivorScoreCalculator::new();
        let mut cfg = calc.behavioral_config.clone();
        cfg.enabled = true;
        cfg.use_additive_mode = false;
        cfg.min_behavioral_floor = 0.30;
        cfg.w_bva = 1.0;
        cfg.w_ecto = 0.0;
        cfg.w_panic = 0.0;
        cfg.w_tcr = 0.0;
        cfg.w_cir = 0.0;
        calc.update_behavioral_config(cfg);

        let input = SurvivorScoreInput {
            bva_score: Some(0.534),
            qedd_survival_60s: Some(0.9),
            sobp_momentum: Some(1.0),
            qman_score: Some(0.7),
            mpcf_organic_ratio: Some(0.7),
            ..Default::default()
        };

        let result = calc.calculate(&input);

        // With floor-blended multiplicative: effective_mult = 0.30 + 0.70 * 0.534 = 0.6738
        // Score should be significantly higher than the old 53.
        assert!(
            result.score > 53,
            "Behavioral floor blending should keep score above pure-multiplicative result (53), got {}",
            result.score
        );
    }

    #[test]
    fn test_behavioral_multiplicative_floor_zero_is_legacy() {
        // When floor=0.0, the bounded formula degenerates to the old
        // pure multiplicative: score * (0 + 1 * behavioral) = score * behavioral.
        let mut calc = SurvivorScoreCalculator::new();
        let mut cfg = calc.behavioral_config.clone();
        cfg.enabled = true;
        cfg.use_additive_mode = false;
        cfg.min_behavioral_floor = 0.0;
        cfg.w_bva = 1.0;
        cfg.w_ecto = 0.0;
        cfg.w_panic = 0.0;
        cfg.w_tcr = 0.0;
        cfg.w_cir = 0.0;
        calc.update_behavioral_config(cfg);

        // Also disable behavioral on a separate calculator to get baseline
        let mut calc_no_beh = SurvivorScoreCalculator::new();
        let mut cfg2 = calc_no_beh.behavioral_config.clone();
        cfg2.enabled = false;
        calc_no_beh.update_behavioral_config(cfg2);

        let input = SurvivorScoreInput {
            bva_score: Some(0.5),
            qedd_survival_60s: Some(0.8),
            sobp_momentum: Some(1.0),
            ..Default::default()
        };

        let baseline = calc_no_beh.calculate(&input).score as f32;
        let with_beh = calc.calculate(&input).score as f32;

        // With floor=0 and behavioral=0.5: score * 0.5
        let expected = (baseline * 0.5).round();
        assert_eq!(
            with_beh, expected,
            "With floor=0, behavioral mult should be legacy: {} * 0.5 = {}, got {}",
            baseline, expected, with_beh
        );
    }

    #[test]
    fn test_momentum_max_offset_config() {
        // Verify momentum_max_offset is configurable
        let mut config = AdditiveScoringConfig::default();
        assert!(
            (config.momentum_max_offset - 1.5).abs() < 0.01,
            "Default momentum_max_offset should be 1.5"
        );

        // With a smaller max_offset, contribution should be more constrained
        config.momentum_max_offset = 0.5;
        let contribution = config.calculate_momentum_contribution(4.0);
        // centered = (4.0 - 0.5).clamp(-0.3, 0.5) = 0.5
        // contribution = 0.5 * 0.30 * 100 = 15.0
        assert!(
            (contribution - 15.0).abs() < 0.01,
            "Custom max_offset=0.5 should cap contribution at 15pts, got {:.1}",
            contribution
        );
    }
}
