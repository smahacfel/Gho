//! Type definitions for Paradox Sensor (EchoScanner)
//!
//! This module defines data structures for network telemetry analysis.

use std::time::Instant;

/// Pojedynczy "puls" sieciowy.
/// Rejestrujemy to przy każdym odebraniu ramki z gniazda.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetworkPulse {
    /// Monotoniczny czas nadejścia (z dokładnością do nanosekund)
    pub timestamp: Instant,
    /// Wielkość payloadu (może sugerować typ instrukcji)
    pub size_bytes: usize,
}

/// Wynik analizy (Sygnał wyjściowy)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParadoxState {
    /// Tension Score (0.0 - 100.0)
    /// Wyliczana metryka "napięcia" rynku.
    /// Wysokie napięcie = niska wariancja czasowa (synchronizacja) + wysoka gęstość.
    pub tension: f64,

    /// Jitter (ms)
    /// Średnie odchylenie standardowe odstępów między pakietami.
    pub jitter_ms: f64,

    /// Packet Density (pakiety/s)
    pub density_bps: f64,

    /// Flaga alarmowa. True, jeśli tension przekracza próg krytyczny.
    pub anomaly_detected: bool,

    /// Derivative (Vector Engine) - kierunek zmian napięcia (-1.0 do +1.0)
    /// Dodatni = napięcie rośnie (boty atakują)
    /// Ujemny = napięcie spada (boty uciekają)
    pub derivative: f64,

    /// Phase Sync (FFT) - siła synchronizacji botów (0.0 do 1.0)
    /// Wysokie wartości wskazują na zsynchronizowane działanie HFT
    pub phase_sync: f64,

    /// Paradox Decision Score (0.0 - 100.0)
    /// Złożona metryka decyzyjna łącząca tension, derivative, phase_sync
    pub pds_score: f64,

    /// Echo Spike Detection - czy wykryto "odbicie" przyszłej pompy
    /// True = możliwe pre-pump activity
    pub is_echo_spike: bool,
}

impl Default for ParadoxState {
    fn default() -> Self {
        Self {
            tension: 0.0,
            jitter_ms: 0.0,
            density_bps: 0.0,
            anomaly_detected: false,
            derivative: 0.0,
            phase_sync: 0.0,
            pds_score: 0.0,
            is_echo_spike: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paradox_state_default() {
        let state = ParadoxState::default();
        assert_eq!(state.tension, 0.0);
        assert_eq!(state.jitter_ms, 0.0);
        assert_eq!(state.density_bps, 0.0);
        assert!(!state.anomaly_detected);
        assert_eq!(state.derivative, 0.0);
        assert_eq!(state.phase_sync, 0.0);
        assert_eq!(state.pds_score, 0.0);
        assert!(!state.is_echo_spike);
    }

    #[test]
    fn test_network_pulse_creation() {
        let pulse = NetworkPulse {
            timestamp: Instant::now(),
            size_bytes: 1024,
        };
        assert_eq!(pulse.size_bytes, 1024);
    }
}
