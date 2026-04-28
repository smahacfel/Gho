//! QEDD Signal Integration Module
//!
//! This module provides QEDD (Quantum Exponential Decay Detector) signal
//! collection for the HyperPrediction Oracle.
//!
//! ## Key Features
//!
//! - **Phase-Aware**: QEDD runs in both early stage and full analysis, but
//!   veto logic is only active in full analysis mode to avoid false rejections
//!   on sparse data.
//!
//! - **Cold-Start Support**: Uses MarketSignals extrapolation when tx_count < 10
//!   to provide meaningful predictions even with limited history.
//!
//! - **Veto Logic**: QEDD can veto if lambda_now exceeds abort threshold
//!   (only in full analysis mode).

use crate::models::qedd_result::QeddResult;
use crate::oracle::hyper_prediction::signals::{SignalResult, SignalSource};
use crate::oracle::hyper_prediction::state::AnalysisPhase;
use crate::qedd::QeddEngine;
use crate::signals::MarketSignals;
use tracing::{debug, warn};

/// Check QEDD survival probability for a candidate
///
/// This function computes QEDD decay rate (lambda_now) and survival probabilities
/// at various time horizons (1s, 5s, 30s, 60s).
///
/// ## Veto Conditions
///
/// QEDD can trigger a VETO if:
/// - `lambda_now > veto_threshold` (only in FullAnalysis mode)
///
/// In EarlyStage mode, QEDD still computes but does not veto to avoid
/// false rejections on sparse data.
///
/// ## Arguments
///
/// * `market_signals` - Aggregated market signals for QEDD computation
/// * `qedd_engine` - QEDD engine instance
/// * `phase` - Current analysis phase (early stage or full analysis)
///
/// ## Returns
///
/// - `Ok(SignalResult::Explicit)` - QEDD computed successfully
/// - `Err(QeddVeto)` - QEDD triggered veto (only in full analysis mode)
pub fn check_qedd_survival(
    market_signals: &MarketSignals,
    qedd_engine: &QeddEngine,
    phase: AnalysisPhase,
) -> Result<SignalResult<QeddResult>, QeddVeto> {
    let result = qedd_engine.compute_qedd_sync(market_signals);

    // Log QEDD metrics
    debug!(
        "QEDD: lambda_now={:.3}, survival_1s={:.2}%, survival_5s={:.2}%, \
        survival_30s={:.2}%, survival_60s={:.2}%, veto_threshold={:.3}",
        result.lambda_now,
        result.survival_1s * 100.0,
        result.survival_5s * 100.0,
        result.survival_30s * 100.0,
        result.survival_60s * 100.0,
        qedd_engine.config.lambda_abort_threshold
    );

    // Veto check (only in full analysis mode)
    if phase == AnalysisPhase::FullAnalysis {
        let lambda_abort = qedd_engine.config.lambda_abort_threshold;

        if result.lambda_now > lambda_abort {
            warn!(
                "QEDD VETO: lambda_now={:.3} > threshold={:.3}",
                result.lambda_now, lambda_abort
            );
            return Err(QeddVeto {
                lambda_now: result.lambda_now,
                threshold: lambda_abort,
                survival_1s: result.survival_1s,
                survival_5s: result.survival_5s,
            });
        }
    } else {
        debug!(
            "QEDD: Early stage mode - veto checks disabled (lambda={:.3})",
            result.lambda_now
        );
    }

    Ok(SignalResult::explicit(result, 1.0))
}

/// QEDD veto reason
///
/// When QEDD detects high decay rate (lambda_now > threshold), it triggers
/// a veto indicating the token is likely to crash imminently.
#[derive(Debug, Clone)]
pub struct QeddVeto {
    pub lambda_now: f32,
    pub threshold: f32,
    pub survival_1s: f32,
    pub survival_5s: f32,
}

impl QeddVeto {
    /// Generate a human-readable interpretation of the veto
    pub fn interpretation(&self) -> String {
        format!(
            "📊 PATIENT | 🔴 VETO: QEDD lambda={:.3} exceeds threshold={:.3} (survival_1s={:.1}%, survival_5s={:.1}%)",
            self.lambda_now,
            self.threshold,
            self.survival_1s * 100.0,
            self.survival_5s * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::qedd_config::QeddConfig;
    use crate::qedd::QeddEngine;
    use crate::signals::MarketSignals;

    fn create_test_market_signals() -> MarketSignals {
        MarketSignals::mock()
    }

    #[test]
    fn test_qedd_early_stage_no_veto() {
        let signals = create_test_market_signals();
        let config = QeddConfig {
            lambda_abort_threshold: 0.1, // Low threshold
            ..Default::default()
        };
        let engine = QeddEngine::new(config);

        // Even if lambda is high, early stage should not veto
        let result = check_qedd_survival(&signals, &engine, AnalysisPhase::EarlyStage);

        // Should succeed (no veto in early stage)
        assert!(result.is_ok());
        let signal = result.unwrap();
        assert_eq!(signal.source, SignalSource::Explicit);
    }

    #[test]
    fn test_qedd_veto_interpretation() {
        let veto = QeddVeto {
            lambda_now: 0.85,
            threshold: 0.5,
            survival_1s: 0.4,
            survival_5s: 0.1,
        };

        let interpretation = veto.interpretation();
        assert!(interpretation.contains("QEDD"));
        assert!(interpretation.contains("VETO"));
        assert!(interpretation.contains("0.85"));
        assert!(interpretation.contains("0.5"));
    }
}
