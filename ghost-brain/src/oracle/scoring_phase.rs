//! ScoringPhase enum for the new 12-cycle scoring engine
//!
//! This module defines the two phases of the scoring loop:
//! - **EarlyStage** (S1-S6): Limited modules, 16-22 TX, static analysis only
//! - **FullAnalysis** (S7-S12): All modules active, 23+ TX
//!
//! The phase determines which analysis modules are activated during each cycle.
//!
//! # Active Modules by Phase
//!
//! ## EarlyStage (S1-S6)
//! - ✅ SOBP (Buying Pressure)
//! - ✅ MPCF (Fingerprinting)
//! - ✅ LIGMA (with VETO rights)
//! - ✅ ClusterHunter (with VETO rights)
//! - ✅ MESA (Microstructure)
//! - ✅ QEDD (Survival)
//! - ✅ Chaos Engine (Monte Carlo)
//! - ✅ PRAECOG (Adversarial)
//! - ✅ ParadoxSensor (informational only)
//! - ✅ TCF (accumulation)
//!
//! ## FullAnalysis (S7-S12) - All EarlyStage modules PLUS:
//! - ✅ SCR (Bot detection via FFT - requires ~20+ samples)
//! - ✅ ULVF (Divergence/Curl analysis - requires flow history)
//! - ✅ POVC (Cluster prediction - requires wallet patterns)
//! - ✅ IWIM (if RPC response available)

use serde::{Deserialize, Serialize};

/// Scoring phase within the 12-cycle scoring loop
///
/// The scoring engine operates in two distinct phases based on the cycle number
/// and available data. Early cycles use a subset of modules due to insufficient
/// data for some analysis types (e.g., FFT requires ~20+ samples).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScoringPhase {
    /// Early stage phase (S1-S6): 16-22 TX, limited modules
    ///
    /// Active modules: SOBP, MPCF, LIGMA, ClusterHunter, MESA, QEDD,
    /// Chaos Engine, PRAECOG, ParadoxSensor, TCF
    ///
    /// Inactive: SCR, ULVF, POVC (insufficient data)
    EarlyStage,

    /// Full analysis phase (S7-S12): 23+ TX, all modules active
    ///
    /// All EarlyStage modules plus: SCR, ULVF, POVC, IWIM (if available)
    FullAnalysis,
}

impl ScoringPhase {
    /// Determine the scoring phase from cycle number (1-indexed)
    ///
    /// # Arguments
    /// * `cycle` - Cycle number (1-12)
    ///
    /// # Returns
    /// * `EarlyStage` for cycles 1-6
    /// * `FullAnalysis` for cycles 7-12
    ///
    /// # Example
    /// ```
    /// use ghost_brain::oracle::scoring_phase::ScoringPhase;
    ///
    /// assert_eq!(ScoringPhase::from_cycle(1), ScoringPhase::EarlyStage);
    /// assert_eq!(ScoringPhase::from_cycle(6), ScoringPhase::EarlyStage);
    /// assert_eq!(ScoringPhase::from_cycle(7), ScoringPhase::FullAnalysis);
    /// assert_eq!(ScoringPhase::from_cycle(12), ScoringPhase::FullAnalysis);
    /// ```
    #[inline]
    pub fn from_cycle(cycle: u8) -> Self {
        if cycle <= 6 {
            ScoringPhase::EarlyStage
        } else {
            ScoringPhase::FullAnalysis
        }
    }

    /// Check if SCR (Spectral Coherence Resonance) module is enabled
    ///
    /// SCR requires ~20+ samples for FFT analysis, which is typically
    /// only available in the FullAnalysis phase.
    #[inline]
    pub fn is_scr_enabled(&self) -> bool {
        matches!(self, ScoringPhase::FullAnalysis)
    }

    /// Check if ULVF (Ultra-Low Volume Field) module is enabled
    ///
    /// ULVF requires flow history for divergence/curl analysis,
    /// which is only available in the FullAnalysis phase.
    #[inline]
    pub fn is_ulvf_enabled(&self) -> bool {
        matches!(self, ScoringPhase::FullAnalysis)
    }

    /// Check if POVC (Predictive Order-flow Volume Clusters) module is enabled
    ///
    /// POVC requires wallet pattern history for cluster prediction,
    /// which is only available in the FullAnalysis phase.
    #[inline]
    pub fn is_povc_enabled(&self) -> bool {
        matches!(self, ScoringPhase::FullAnalysis)
    }

    /// Check if this is the early stage phase
    #[inline]
    pub fn is_early_stage(&self) -> bool {
        matches!(self, ScoringPhase::EarlyStage)
    }

    /// Check if this is the full analysis phase
    #[inline]
    pub fn is_full_analysis(&self) -> bool {
        matches!(self, ScoringPhase::FullAnalysis)
    }

    /// Get the minimum transaction count required for this phase
    #[inline]
    pub fn min_tx_count(&self) -> u32 {
        match self {
            ScoringPhase::EarlyStage => 16,
            ScoringPhase::FullAnalysis => 23,
        }
    }

    /// Get a human-readable name for this phase
    #[inline]
    pub fn name(&self) -> &'static str {
        match self {
            ScoringPhase::EarlyStage => "Early Stage (S1-S6)",
            ScoringPhase::FullAnalysis => "Full Analysis (S7-S12)",
        }
    }
}

impl Default for ScoringPhase {
    fn default() -> Self {
        ScoringPhase::EarlyStage
    }
}

impl std::fmt::Display for ScoringPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_cycle_early_stage() {
        for cycle in 1..=6 {
            assert_eq!(
                ScoringPhase::from_cycle(cycle),
                ScoringPhase::EarlyStage,
                "Cycle {} should be EarlyStage",
                cycle
            );
        }
    }

    #[test]
    fn test_from_cycle_full_analysis() {
        for cycle in 7..=12 {
            assert_eq!(
                ScoringPhase::from_cycle(cycle),
                ScoringPhase::FullAnalysis,
                "Cycle {} should be FullAnalysis",
                cycle
            );
        }
    }

    #[test]
    fn test_module_activation_early_stage() {
        let phase = ScoringPhase::EarlyStage;
        assert!(
            !phase.is_scr_enabled(),
            "SCR should be disabled in EarlyStage"
        );
        assert!(
            !phase.is_ulvf_enabled(),
            "ULVF should be disabled in EarlyStage"
        );
        assert!(
            !phase.is_povc_enabled(),
            "POVC should be disabled in EarlyStage"
        );
    }

    #[test]
    fn test_module_activation_full_analysis() {
        let phase = ScoringPhase::FullAnalysis;
        assert!(
            phase.is_scr_enabled(),
            "SCR should be enabled in FullAnalysis"
        );
        assert!(
            phase.is_ulvf_enabled(),
            "ULVF should be enabled in FullAnalysis"
        );
        assert!(
            phase.is_povc_enabled(),
            "POVC should be enabled in FullAnalysis"
        );
    }

    #[test]
    fn test_min_tx_count() {
        assert_eq!(ScoringPhase::EarlyStage.min_tx_count(), 16);
        assert_eq!(ScoringPhase::FullAnalysis.min_tx_count(), 23);
    }

    #[test]
    fn test_phase_predicates() {
        let early = ScoringPhase::EarlyStage;
        let full = ScoringPhase::FullAnalysis;

        assert!(early.is_early_stage());
        assert!(!early.is_full_analysis());

        assert!(!full.is_early_stage());
        assert!(full.is_full_analysis());
    }

    #[test]
    fn test_default() {
        assert_eq!(ScoringPhase::default(), ScoringPhase::EarlyStage);
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", ScoringPhase::EarlyStage),
            "Early Stage (S1-S6)"
        );
        assert_eq!(
            format!("{}", ScoringPhase::FullAnalysis),
            "Full Analysis (S7-S12)"
        );
    }

    #[test]
    fn test_serialization() {
        let early = ScoringPhase::EarlyStage;
        let full = ScoringPhase::FullAnalysis;

        let early_json = serde_json::to_string(&early).unwrap();
        let full_json = serde_json::to_string(&full).unwrap();

        assert_eq!(early_json, "\"EarlyStage\"");
        assert_eq!(full_json, "\"FullAnalysis\"");

        let early_parsed: ScoringPhase = serde_json::from_str(&early_json).unwrap();
        let full_parsed: ScoringPhase = serde_json::from_str(&full_json).unwrap();

        assert_eq!(early_parsed, early);
        assert_eq!(full_parsed, full);
    }
}
