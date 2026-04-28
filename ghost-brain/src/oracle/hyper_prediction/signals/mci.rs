//! MCI Signal Integration Module
//!
//! This module provides MCI (Market Coherence Index) signal collection
//! for the HyperPrediction Oracle.
//!
//! ## Key Features
//!
//! - **Phase-Aware**: MCI runs in both early stage and full analysis, but
//!   veto logic is only active in full analysis mode.
//!
//! - **Coherence Detection**: Measures directional coherence (DC) and
//!   structural coherence (SC) to detect market manipulation.
//!
//! - **Veto Logic**: Can veto if MCI falls below coherence_abort_threshold
//!   (only in full analysis mode).

use crate::mci::MciEngine;
use crate::models::mci_result::MciResult;
use crate::oracle::hyper_prediction::signals::{SignalResult, SignalSource};
use crate::oracle::hyper_prediction::state::AnalysisPhase;
use crate::signals::MarketSignals;
use tracing::{debug, warn};

/// Check MCI market coherence for a candidate
///
/// This function computes MCI (overall coherence), DC (directional coherence),
/// and SC (structural coherence) to detect market manipulation or incoherence.
///
/// ## Veto Conditions
///
/// MCI can trigger a VETO if:
/// - `mci < coherence_abort_threshold` (only in FullAnalysis mode)
///
/// In EarlyStage mode, MCI still computes but does not veto to avoid
/// false rejections on sparse data.
///
/// ## Arguments
///
/// * `market_signals` - Aggregated market signals for MCI computation
/// * `mci_engine` - MCI engine instance
/// * `phase` - Current analysis phase (early stage or full analysis)
///
/// ## Returns
///
/// - `Ok(SignalResult::Explicit)` - MCI computed successfully
/// - `Err(MciVeto)` - MCI triggered veto (only in full analysis mode)
pub fn check_mci_coherence(
    market_signals: &MarketSignals,
    mci_engine: &MciEngine,
    phase: AnalysisPhase,
) -> Result<SignalResult<MciResult>, MciVeto> {
    let result = mci_engine.compute_mci(market_signals);

    // Log MCI metrics
    debug!(
        "MCI: mci={:.3}, dc={:.3}, sc={:.3}, veto_threshold={:.3}",
        result.mci, result.dc, result.sc, mci_engine.config.coherence_abort_threshold
    );

    // Veto check (only in full analysis mode)
    if phase == AnalysisPhase::FullAnalysis {
        let coherence_abort = mci_engine.config.coherence_abort_threshold;

        if result.mci < coherence_abort {
            warn!(
                "MCI VETO: mci={:.3} < threshold={:.3}",
                result.mci, coherence_abort
            );
            return Err(MciVeto {
                mci: result.mci,
                threshold: coherence_abort,
                dc: result.dc,
                sc: result.sc,
            });
        }
    } else {
        debug!(
            "MCI: Early stage mode - veto checks disabled (mci={:.3})",
            result.mci
        );
    }

    Ok(SignalResult::explicit(result, 1.0))
}

/// MCI veto reason
///
/// When MCI detects low market coherence (mci < threshold), it triggers
/// a veto indicating the market activity is incoherent or manipulated.
#[derive(Debug, Clone)]
pub struct MciVeto {
    pub mci: f32,
    pub threshold: f32,
    pub dc: f32,
    pub sc: f32,
}

impl MciVeto {
    /// Generate a human-readable interpretation of the veto
    pub fn interpretation(&self) -> String {
        format!(
            "📊 PATIENT | 🔴 VETO: MCI coherence={:.3} below threshold={:.3} (DC={:.3}, SC={:.3})",
            self.mci, self.threshold, self.dc, self.sc
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::mci_config::MciConfig;
    use crate::mci::MciEngine;
    use crate::signals::MarketSignals;

    fn create_test_market_signals() -> MarketSignals {
        MarketSignals::mock()
    }

    #[test]
    fn test_mci_early_stage_no_veto() {
        let signals = create_test_market_signals();
        let config = MciConfig {
            coherence_abort_threshold: 0.9, // High threshold
            ..Default::default()
        };
        let engine = MciEngine::new(config);

        // Even if MCI is low, early stage should not veto
        let result = check_mci_coherence(&signals, &engine, AnalysisPhase::EarlyStage);

        // Should succeed (no veto in early stage)
        assert!(result.is_ok());
        let signal = result.unwrap();
        assert_eq!(signal.source, SignalSource::Explicit);
    }

    #[test]
    fn test_mci_veto_interpretation() {
        let veto = MciVeto {
            mci: 0.3,
            threshold: 0.6,
            dc: 0.25,
            sc: 0.35,
        };

        let interpretation = veto.interpretation();
        assert!(interpretation.contains("MCI"));
        assert!(interpretation.contains("VETO"));
        assert!(interpretation.contains("0.3"));
        assert!(interpretation.contains("0.6"));
    }
}
