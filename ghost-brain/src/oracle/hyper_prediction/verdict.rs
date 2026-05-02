//! **DEPRECATED – Legacy HyperPrediction Oracle verdict types**
//!
//! This module used to provide the decision, risk and final‑verdict types for
//! the old cycle‑based (S1‑S13) HyperPrediction engine.  The new pipeline
//! replaces that engine with a simpler post‑Gatekeeper flow:
//!
//! ```text
//! Hard Fails → Core Checks → Soft Signals → Sybil Interference →
//! Alpha Gate → Prosperity Filter → IWIM Veto
//! ```
//!
//! The types defined below (`OracleDecision`, `RiskLevel`, `FinalVerdict`) are
//! therefore **not used** in the current pipeline.  They are retained only for
//! backward compatibility with older test code that may still import them.
//! All new or refactored logic should avoid these types entirely.
//!
//! # Removal timeline
//!
//! Once all import sites have been cleaned up this file can be deleted.
//! If you are reading this notice, please do not add new dependencies on
//! the public items in this module.

#![allow(deprecated)] // the file itself still needs to compile

use seer::types::CandidatePool;
use serde::{Deserialize, Serialize};

// =============================================================================
// Oracle Decision
// =============================================================================

/// **Deprecated** – Legacy Oracle decision outcome for a token evaluation.
///
/// This enum was part of the old HyperPrediction engine.  In the current
/// pipeline the decision is determined by the Gatekeeper verdict chain;
/// there is no separate "OracleDecision" type.
#[deprecated(
    since = "6.0.0",
    note = "Replaced by GatekeeperVerdictType and the post-Gatekeeper policy matrix"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OracleDecision {
    /// Buy signal - token passed all checks
    Buy,

    /// Skip signal - token failed checks or did not meet threshold
    Skip,

    /// Watch signal - token is interesting but needs more observation
    /// (currently unused, reserved for future Patient Observer enhancements)
    Watch,
}

impl Default for OracleDecision {
    fn default() -> Self {
        Self::Skip
    }
}

// =============================================================================
// Risk Level
// =============================================================================

/// **Deprecated** – Legacy risk level classification for token evaluation.
///
/// Risk assessment in the new pipeline is captured by
/// [`GatekeeperStrength`](super::super::components::gatekeeper::GatekeeperStrength)
/// and the IWIM veto quality classification; this enum is no longer needed.
#[deprecated(
    since = "6.0.0",
    note = "Replaced by GatekeeperStrength / IWIM quality; no longer used in the scoring pipeline"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Low risk - high confidence in token quality
    Low,

    /// Medium risk - moderate confidence, some concerns
    Medium,

    /// High risk - low confidence, multiple red flags
    High,

    /// Very high risk - critical issues detected
    VeryHigh,
}

impl Default for RiskLevel {
    fn default() -> Self {
        Self::VeryHigh
    }
}

impl RiskLevel {
    /// Get the score penalty associated with this risk level
    ///
    /// These penalties are applied to reduce the final score based on risk.
    /// Higher risk = larger penalty.
    pub fn penalty(&self) -> u8 {
        match self {
            RiskLevel::Low => 0,
            RiskLevel::Medium => 5,
            RiskLevel::High => 15,
            RiskLevel::VeryHigh => 40,
        }
    }

    /// Determine risk level from candidate data with automatic penalties
    ///
    /// This implements automatic risk detection rules:
    /// - liquidity < 1 SOL → VeryHigh
    /// - liquidity < 3 SOL → High
    /// - bonding curve > 0.9 → VeryHigh
    /// - bonding curve > 0.75 → High
    /// - token supply > 10B → High
    /// - missing critical data → VeryHigh
    pub fn from_candidate(candidate: &CandidatePool, base_score: u8) -> Self {
        // Critical risk factors that force VeryHigh
        if let Some(liquidity_sol) = candidate.initial_liquidity_sol {
            if liquidity_sol < 1.0 {
                return RiskLevel::VeryHigh;
            }
        } else {
            // Missing liquidity data is extremely risky
            return RiskLevel::VeryHigh;
        }

        if let Some(progress) = candidate.bonding_curve_progress {
            if progress > 0.9 {
                return RiskLevel::VeryHigh;
            }
        }

        // High risk factors
        if let Some(liquidity_sol) = candidate.initial_liquidity_sol {
            if liquidity_sol < 3.0 {
                return RiskLevel::High;
            }
        }

        if let Some(progress) = candidate.bonding_curve_progress {
            if progress > 0.75 {
                return RiskLevel::High;
            }
        }

        if let Some(supply) = candidate.token_total_supply {
            if supply > 10_000_000_000 {
                return RiskLevel::High;
            }
        }

        // Fall back to score-based assessment
        match base_score {
            90..=100 => RiskLevel::Low,
            70..=89 => RiskLevel::Medium,
            50..=69 => RiskLevel::High,
            _ => RiskLevel::VeryHigh,
        }
    }
}

// =============================================================================
// Risk Thresholds
// =============================================================================

/// **Deprecated** – Legacy configurable risk thresholds.
///
/// The new pipeline does not use these thresholds; soft‑point limits and the
/// prosperity filter serve similar purposes.
#[deprecated(
    since = "6.0.0",
    note = "Superseded by the Gatekeeper soft‑point limits and ProsperityFilter thresholds"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskThresholds {
    /// SurvivorScore confidence below this = VeryHigh risk
    ///
    /// Default: 0.5
    pub very_high_confidence: f32,

    /// SurvivorScore confidence below this = High risk
    ///
    /// Default: 0.7
    pub high_confidence: f32,

    /// Final score below this = Medium risk (if confidence is OK)
    ///
    /// Default: 60
    pub medium_score: u8,
}

impl Default for RiskThresholds {
    fn default() -> Self {
        Self {
            very_high_confidence: 0.5,
            high_confidence: 0.7,
            medium_score: 60,
        }
    }
}

impl RiskThresholds {
    /// Create new risk thresholds with custom values
    pub fn new(very_high_confidence: f32, high_confidence: f32, medium_score: u8) -> Self {
        Self {
            very_high_confidence,
            high_confidence,
            medium_score,
        }
    }

    /// Determine risk level from SurvivorScore confidence and final score
    pub fn determine_risk(&self, survivor_confidence: f32, final_score: u8) -> RiskLevel {
        if survivor_confidence < self.very_high_confidence {
            RiskLevel::VeryHigh
        } else if survivor_confidence < self.high_confidence {
            RiskLevel::High
        } else if final_score < self.medium_score {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        }
    }
}

// =============================================================================
// Final Verdict
// =============================================================================

/// **Deprecated** – Legacy final verdict from the HyperPrediction Oracle.
///
/// The new pipeline produces a `GatekeeperDecision` which already contains the
/// final verdict, risk/strength classification, and reason chain.
#[deprecated(
    since = "6.0.0",
    note = "Replaced by GatekeeperDecision; avoid this struct in new code"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalVerdict {
    /// Final score (0-100)
    pub score: u8,

    /// Oracle decision
    pub decision: OracleDecision,

    /// Risk level assessment
    pub risk_level: RiskLevel,

    /// Whether the token passed the threshold
    pub passed: bool,

    /// Human-readable interpretation
    pub interpretation: String,
}

impl FinalVerdict {
    /// Create a new final verdict
    pub fn new(
        score: u8,
        decision: OracleDecision,
        risk_level: RiskLevel,
        passed: bool,
        interpretation: String,
    ) -> Self {
        Self {
            score,
            decision,
            risk_level,
            passed,
            interpretation,
        }
    }

    /// Create a Skip verdict with VeryHigh risk
    pub fn skip(score: u8, interpretation: String) -> Self {
        Self {
            score,
            decision: OracleDecision::Skip,
            risk_level: RiskLevel::VeryHigh,
            passed: false,
            interpretation,
        }
    }

    /// Create a Buy verdict with Low risk
    pub fn buy(score: u8, interpretation: String) -> Self {
        Self {
            score,
            decision: OracleDecision::Buy,
            risk_level: RiskLevel::Low,
            passed: true,
            interpretation,
        }
    }
}

impl Default for FinalVerdict {
    fn default() -> Self {
        Self {
            score: 0,
            decision: OracleDecision::Skip,
            risk_level: RiskLevel::VeryHigh,
            passed: false,
            interpretation: "No evaluation performed".to_string(),
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_thresholds_defaults() {
        let thresholds = RiskThresholds::default();
        assert_eq!(thresholds.very_high_confidence, 0.5);
        assert_eq!(thresholds.high_confidence, 0.7);
        assert_eq!(thresholds.medium_score, 60);
    }

    #[test]
    fn test_risk_thresholds_custom() {
        let thresholds = RiskThresholds::new(0.4, 0.8, 70);
        assert_eq!(thresholds.very_high_confidence, 0.4);
        assert_eq!(thresholds.high_confidence, 0.8);
        assert_eq!(thresholds.medium_score, 70);
    }

    #[test]
    fn test_determine_risk_very_high() {
        let thresholds = RiskThresholds::default();

        // Very low confidence → VeryHigh risk
        assert_eq!(thresholds.determine_risk(0.3, 80), RiskLevel::VeryHigh);
        assert_eq!(thresholds.determine_risk(0.49, 90), RiskLevel::VeryHigh);
    }

    #[test]
    fn test_determine_risk_high() {
        let thresholds = RiskThresholds::default();

        // Medium confidence → High risk
        assert_eq!(thresholds.determine_risk(0.6, 80), RiskLevel::High);
        assert_eq!(thresholds.determine_risk(0.69, 90), RiskLevel::High);
    }

    #[test]
    fn test_determine_risk_medium() {
        let thresholds = RiskThresholds::default();

        // Good confidence but low score → Medium risk
        assert_eq!(thresholds.determine_risk(0.8, 50), RiskLevel::Medium);
        assert_eq!(thresholds.determine_risk(0.75, 59), RiskLevel::Medium);
    }

    #[test]
    fn test_determine_risk_low() {
        let thresholds = RiskThresholds::default();

        // Good confidence and good score → Low risk
        assert_eq!(thresholds.determine_risk(0.8, 70), RiskLevel::Low);
        assert_eq!(thresholds.determine_risk(0.9, 85), RiskLevel::Low);
        assert_eq!(thresholds.determine_risk(0.7, 60), RiskLevel::Low);
    }

    #[test]
    fn test_verdict_serialization() {
        let verdict = FinalVerdict {
            score: 75,
            decision: OracleDecision::Buy,
            risk_level: RiskLevel::Low,
            passed: true,
            interpretation: "Test verdict".to_string(),
        };

        let json = serde_json::to_string(&verdict).unwrap();
        assert!(json.contains("risk_level"));
        assert!(json.contains("decision"));
        assert!(json.contains("Buy"));
        assert!(json.contains("Low"));

        // Verify deserialization
        let deserialized: FinalVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.score, 75);
        assert_eq!(deserialized.decision, OracleDecision::Buy);
        assert_eq!(deserialized.risk_level, RiskLevel::Low);
    }

    #[test]
    fn test_risk_level_penalty() {
        assert_eq!(RiskLevel::Low.penalty(), 0);
        assert_eq!(RiskLevel::Medium.penalty(), 5);
        assert_eq!(RiskLevel::High.penalty(), 15);
        assert_eq!(RiskLevel::VeryHigh.penalty(), 40);
    }

    #[test]
    fn test_oracle_decision_default() {
        assert_eq!(OracleDecision::default(), OracleDecision::Skip);
    }

    #[test]
    fn test_risk_level_default() {
        assert_eq!(RiskLevel::default(), RiskLevel::VeryHigh);
    }

    #[test]
    fn test_final_verdict_skip_helper() {
        let verdict = FinalVerdict::skip(30, "Low score".to_string());
        assert_eq!(verdict.decision, OracleDecision::Skip);
        assert_eq!(verdict.risk_level, RiskLevel::VeryHigh);
        assert!(!verdict.passed);
    }

    #[test]
    fn test_final_verdict_buy_helper() {
        let verdict = FinalVerdict::buy(80, "Good token".to_string());
        assert_eq!(verdict.decision, OracleDecision::Buy);
        assert_eq!(verdict.risk_level, RiskLevel::Low);
        assert!(verdict.passed);
    }
}
