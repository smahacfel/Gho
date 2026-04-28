//! Paradox Signal Integration Module
//!
//! This module provides ParadoxSensor signal collection for the HyperPrediction Oracle.
//!
//! ## Key Features
//!
//! - **Packet Manipulation Detection**: Detects HFT countermeasures and
//!   time-based manipulation attacks.
//!
//! - **No Veto Logic**: Paradox is informational only and does not trigger vetoes.
//!
//! - **Explicit Tracking**: Returns SignalResult with source tracking.

use crate::oracle::hyper_prediction::signals::{SignalResult, SignalSource};
use seer::paradox_sensor::ParadoxState;
use tracing::debug;

/// Check Paradox sensor state for packet manipulation
///
/// This function collects ParadoxSensor state which detects HFT bot activity,
/// packet manipulation, and time-based attacks.
///
/// ## Veto Conditions
///
/// Paradox does NOT trigger vetoes. It provides informational signals that
/// can be used for analysis and wave injection in QASS.
///
/// ## Arguments
///
/// * `paradox_state` - Optional Paradox sensor state
///
/// ## Returns
///
/// - `SignalResult::Explicit` - Paradox state provided
/// - `SignalResult::Unavailable` - No Paradox state available
pub fn check_paradox_sensor(paradox_state: Option<&ParadoxState>) -> SignalResult<ParadoxState> {
    let Some(state) = paradox_state else {
        debug!("PARADOX: No sensor state available");
        return SignalResult::unavailable();
    };

    // Log Paradox metrics using correct field names from ParadoxState
    debug!(
        "PARADOX: tension={:.2}, pds_score={:.2}, \
        anomaly_detected={}, phase_sync={:.2}",
        state.tension, state.pds_score, state.anomaly_detected, state.phase_sync
    );

    // Paradox is informational - no veto logic
    SignalResult::explicit(state.clone(), 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use seer::paradox_sensor::ParadoxState;

    fn create_test_paradox_state() -> ParadoxState {
        ParadoxState {
            tension: 30.0,
            jitter_ms: 5.0,
            density_bps: 100.0,
            anomaly_detected: false,
            derivative: 0.2,
            phase_sync: 0.4,
            pds_score: 50.0,
            is_echo_spike: false,
        }
    }

    #[test]
    fn test_paradox_unavailable() {
        let result = check_paradox_sensor(None);
        assert_eq!(result.source, SignalSource::Unavailable);
        assert_eq!(result.value, None);
    }

    #[test]
    fn test_paradox_available() {
        let state = create_test_paradox_state();
        let result = check_paradox_sensor(Some(&state));

        assert_eq!(result.source, SignalSource::Explicit);
        assert!(result.value.is_some());
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn test_paradox_no_veto() {
        // Even with high anomaly scores, Paradox should not veto
        let state = ParadoxState {
            tension: 95.0,
            jitter_ms: 1.0,
            density_bps: 500.0,
            anomaly_detected: true,
            derivative: 0.9,
            phase_sync: 0.99,
            pds_score: 95.0,
            is_echo_spike: true,
        };

        let result = check_paradox_sensor(Some(&state));

        // Should still return explicit result (no veto)
        assert_eq!(result.source, SignalSource::Explicit);
        assert!(result.value.is_some());
    }
}
