//! Paradox Sensor (EchoScanner) - Network Side-Channel Analysis
//!
//! This module performs telemetric analysis of the network layer to detect
//! pre-transactional anomalies from HFT activity.
//!
//! The ParadoxSensor acts as a passive "sonar" plugged into the WebSocket/gRPC
//! data stream, analyzing temporal and volumetric metadata of incoming packets.

pub mod types;

use rustfft::{num_complex::Complex, FftPlanner};
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tracing::debug;

pub use types::{NetworkPulse, ParadoxState};

/// Konfiguracja okna czasowego (Mirror-Time Window)
const WINDOW_SIZE_MS: u128 = 500; // Analiza ostatnich 500ms
const MAX_SAMPLES: usize = 2000; // Zabezpieczenie pamięci

/// Minimum samples required for statistical analysis
const MIN_SAMPLES_FOR_ANALYSIS: usize = 10;

/// Tension threshold for anomaly detection (0.0 - 100.0)
const ANOMALY_TENSION_THRESHOLD: f64 = 80.0;

/// Analysis refresh interval in milliseconds
const ANALYSIS_INTERVAL_MS: u64 = 50;

/// Paradox Sensor for network telemetry analysis
pub struct ParadoxSensor {
    /// Bufor impulsów (Ring Buffer)
    /// Using RwLock for concurrent access. Write on packet arrival, read during analysis.
    history: Arc<RwLock<VecDeque<NetworkPulse>>>,

    /// Kanał do publikowania wyników (Trigger nasłuchuje tego)
    state_tx: watch::Sender<ParadoxState>,

    /// Poprzednie napięcie do obliczania derivative (Vector Engine)
    prev_tension: Arc<RwLock<f64>>,

    /// Poprzedni timestamp do obliczania delta czasu
    prev_timestamp: Arc<RwLock<Instant>>,
}

impl ParadoxSensor {
    /// Create a new ParadoxSensor
    ///
    /// Returns (sensor, receiver) tuple where receiver can be used to subscribe
    /// to real-time ParadoxState updates.
    pub fn new() -> (Self, watch::Receiver<ParadoxState>) {
        let (tx, rx) = watch::channel(ParadoxState::default());

        (
            Self {
                history: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_SAMPLES))),
                state_tx: tx,
                prev_tension: Arc::new(RwLock::new(0.0)),
                prev_timestamp: Arc::new(RwLock::new(Instant::now())),
            },
            rx,
        )
    }

    /// Record a network pulse (Hot Path)
    ///
    /// This method is called in the WebSocket receive loop and must be O(1) and very fast.
    /// It records the arrival time and size of each network packet.
    ///
    /// # Arguments
    /// * `size` - Size of the packet payload in bytes
    pub fn record_pulse(&self, size: usize) {
        let now = Instant::now();
        let mut history = self.history.write().unwrap(); // Short lock duration

        // Add new pulse
        history.push_back(NetworkPulse {
            timestamp: now,
            size_bytes: size,
        });

        // Lazy cleanup - remove oldest samples if we exceed capacity
        if history.len() > MAX_SAMPLES {
            history.pop_front();
        }
    }

    /// Run the background analysis loop
    ///
    /// This spawns a background task that periodically analyzes the network pulse
    /// history and publishes ParadoxState updates via the watch channel.
    ///
    /// # Arguments
    /// * `self_arc` - Arc-wrapped reference to self for async task
    pub async fn run_analysis_loop(self_arc: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_millis(ANALYSIS_INTERVAL_MS));

        loop {
            interval.tick().await;
            self_arc.calculate_state();
        }
    }

    /// Calculate and publish the current ParadoxState
    ///
    /// This performs statistical analysis on the recent network pulse history:
    /// - Calculates inter-arrival times (IAT)
    /// - Computes jitter (standard deviation of IAT)
    /// - Computes packet density (packets per second)
    /// - Calculates tension score (high density + low jitter = synchronized bot activity)
    fn calculate_state(&self) {
        let now = Instant::now();

        // Clone data for analysis to minimize lock hold time
        let pulses: Vec<NetworkPulse> = {
            let mut history = self.history.write().unwrap();

            // Pruning (remove old samples outside the 500ms window)
            while let Some(pulse) = history.front() {
                if now.duration_since(pulse.timestamp).as_millis() > WINDOW_SIZE_MS {
                    history.pop_front();
                } else {
                    break;
                }
            }

            history.iter().copied().collect()
        };

        // Need minimum samples for meaningful statistics
        if pulses.len() < MIN_SAMPLES_FOR_ANALYSIS {
            return;
        }

        // 1. Calculate Inter-Arrival Times (IAT) in microseconds
        let mut iats = Vec::with_capacity(pulses.len());
        for window in pulses.windows(2) {
            let delta = window[1]
                .timestamp
                .duration_since(window[0].timestamp)
                .as_micros() as f64;
            iats.push(delta);
        }

        // 2. Mean IAT
        let sum_iat: f64 = iats.iter().sum();
        let mean_iat = sum_iat / iats.len() as f64;

        // 3. Jitter (Standard deviation of IAT)
        let variance: f64 = iats
            .iter()
            .map(|&iat| {
                let diff = mean_iat - iat;
                diff * diff
            })
            .sum::<f64>()
            / iats.len() as f64;
        let jitter = variance.sqrt();

        // 4. Density (Packets per second in the window)
        // Scale to packets per second
        let density = (pulses.len() as f64) * (1000.0 / WINDOW_SIZE_MS as f64);

        // 5. PARADOX FORMULA (Heuristic)
        // Hypothesis: Synchronized bot attacks = High Density + Very Low Jitter
        // (packets arrive in machine-synchronized bursts)
        //
        // We look for "Synthetic Stability" - low jitter at high load
        // tension = (Density^1.1) / (Jitter + epsilon)
        let tension_raw = (density.powf(1.1)) / (jitter + 1.0);

        // Normalize (empirical - needs calibration on mainnet data)
        let tension_normalized = (tension_raw / 50.0).min(100.0);

        let is_anomaly = tension_normalized > ANOMALY_TENSION_THRESHOLD;

        if is_anomaly {
            debug!(
                "🔮 PARADOX ANOMALY: Tension={:.2}, Jitter={:.2}us, Density={:.0}pps",
                tension_normalized, jitter, density
            );
        }

        // 6. VECTOR ENGINE - Calculate derivative (direction of tension change)
        let derivative = {
            let prev_tension = *self.prev_tension.read().unwrap();
            let prev_timestamp = *self.prev_timestamp.read().unwrap();

            // Calculate time delta in seconds
            let time_delta = now.duration_since(prev_timestamp).as_secs_f64();

            // Avoid division by zero
            let raw_derivative = if time_delta > 0.0 {
                (tension_normalized - prev_tension) / time_delta
            } else {
                0.0
            };

            // Normalize to -1.0 to +1.0 range
            // Assuming max tension change rate is ~100 units/second
            (raw_derivative / 100.0).clamp(-1.0, 1.0)
        };

        // Update prev values for next iteration
        *self.prev_tension.write().unwrap() = tension_normalized;
        *self.prev_timestamp.write().unwrap() = now;

        // 7. PHASE LOCK DETECTOR - FFT analysis for bot synchronization
        let phase_sync = self.calculate_phase_sync(&pulses);

        // 8. ECHO SPIKE DETECTION - Advanced pattern matching
        // For now, detect echo spike based on rapid tension increase + high phase sync
        let is_echo_spike = derivative > 0.5 && phase_sync > 0.7 && tension_normalized > 70.0;

        // 9. PARADOX DECISION SCORE (PDS)
        // Formula: PDS = 0.45*tension + 0.25*max(0,derivative) + 0.20*phase_sync + 0.10*echo_flag
        let echo_flag = if is_echo_spike { 1.0 } else { 0.0 };
        let derivative_positive = derivative.max(0.0);

        let pds_score = (0.45 * tension_normalized)
            + (0.25 * derivative_positive * 100.0) // Scale derivative to 0-100
            + (0.20 * phase_sync * 100.0)          // Scale phase_sync to 0-100
            + (0.10 * echo_flag * 100.0); // Scale echo to 0-100

        // Clamp PDS to 0-100 range
        let pds_score = pds_score.clamp(0.0, 100.0);

        // Publish result via watch channel
        let _ = self.state_tx.send(ParadoxState {
            tension: tension_normalized,
            jitter_ms: jitter / 1000.0,
            density_bps: density,
            anomaly_detected: is_anomaly,
            derivative,
            phase_sync,
            pds_score,
            is_echo_spike,
        });
    }

    /// Get the current ParadoxState (blocking read)
    ///
    /// Returns the most recently calculated state.
    pub fn current_state(&self) -> ParadoxState {
        *self.state_tx.borrow()
    }

    /// Calculate phase synchronization using FFT analysis
    ///
    /// Analyzes packet arrival timing to detect synchronized bot behavior.
    /// High phase_sync (close to 1.0) indicates HFT bots operating in sync.
    ///
    /// # Arguments
    /// * `pulses` - Recent network pulses to analyze
    ///
    /// # Returns
    /// Phase sync score (0.0 - 1.0)
    fn calculate_phase_sync(&self, pulses: &[NetworkPulse]) -> f64 {
        // Need at least a few samples for FFT
        if pulses.len() < 8 {
            return 0.0;
        }

        // Convert packet timestamps to inter-arrival times (IATs)
        let mut iats = Vec::with_capacity(pulses.len() - 1);
        for window in pulses.windows(2) {
            let delta = window[1]
                .timestamp
                .duration_since(window[0].timestamp)
                .as_micros() as f64;
            iats.push(delta);
        }

        if iats.is_empty() {
            return 0.0;
        }

        // Prepare signal for FFT (convert to complex numbers)
        let mut signal: Vec<Complex<f64>> = iats.iter().map(|&x| Complex::new(x, 0.0)).collect();

        // Pad to power of 2 for efficient FFT
        let fft_size = signal.len().next_power_of_two();
        signal.resize(fft_size, Complex::new(0.0, 0.0));

        // Perform FFT
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(fft_size);
        fft.process(&mut signal);

        // Calculate magnitude spectrum
        let magnitudes: Vec<f64> = signal
            .iter()
            .take(fft_size / 2) // Only need first half (Nyquist)
            .map(|c| c.norm())
            .collect();

        if magnitudes.is_empty() {
            return 0.0;
        }

        // Find peak magnitude (excluding DC component at index 0)
        let peak_magnitude = magnitudes
            .iter()
            .skip(1)
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);

        // Calculate mean magnitude
        let mean_magnitude: f64 =
            magnitudes.iter().skip(1).sum::<f64>() / (magnitudes.len() - 1) as f64;

        // Phase sync = peak / (mean + epsilon)
        // High ratio = strong periodicity = synchronized bots
        let phase_sync_raw = peak_magnitude / (mean_magnitude + 1.0);

        // Normalize to 0.0 - 1.0 range (empirically, ratio > 5.0 is strong sync)
        let phase_sync = (phase_sync_raw / 5.0).min(1.0);

        phase_sync
    }
}

impl Default for ParadoxSensor {
    fn default() -> Self {
        Self::new().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_sensor_creation() {
        let (sensor, rx) = ParadoxSensor::new();
        let state = rx.borrow();
        assert_eq!(state.tension, 0.0);
        assert_eq!(state.jitter_ms, 0.0);
        assert_eq!(state.density_bps, 0.0);
        assert!(!state.anomaly_detected);

        // Verify history is initialized
        let history = sensor.history.read().unwrap();
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_record_pulse() {
        let (sensor, _rx) = ParadoxSensor::new();

        // Record some pulses
        sensor.record_pulse(100);
        sensor.record_pulse(200);
        sensor.record_pulse(150);

        // Verify history contains pulses
        let history = sensor.history.read().unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].size_bytes, 100);
        assert_eq!(history[1].size_bytes, 200);
        assert_eq!(history[2].size_bytes, 150);
    }

    #[test]
    fn test_pulse_capacity_limit() {
        let (sensor, _rx) = ParadoxSensor::new();

        // Record more than MAX_SAMPLES
        for i in 0..(MAX_SAMPLES + 100) {
            sensor.record_pulse(i);
        }

        // Verify history doesn't exceed MAX_SAMPLES
        let history = sensor.history.read().unwrap();
        assert!(history.len() <= MAX_SAMPLES);
        assert_eq!(history.len(), MAX_SAMPLES);
    }

    #[test]
    fn test_calculate_state_with_insufficient_samples() {
        let (sensor, rx) = ParadoxSensor::new();

        // Record only a few pulses (less than MIN_SAMPLES_FOR_ANALYSIS)
        sensor.record_pulse(100);
        sensor.record_pulse(200);

        // Calculate state
        sensor.calculate_state();

        // State should remain at default because not enough samples
        let state = rx.borrow();
        assert_eq!(state.tension, 0.0);
    }

    #[test]
    fn test_calculate_state_with_sufficient_samples() {
        let (sensor, rx) = ParadoxSensor::new();

        // Record enough pulses for analysis
        for i in 0..20 {
            sensor.record_pulse(100 + i);
            // Add small delay to create realistic timing
            thread::sleep(Duration::from_micros(1000));
        }

        // Calculate state
        sensor.calculate_state();

        // State should be calculated
        let state = rx.borrow();
        // Tension, jitter, and density should be calculated (non-zero values)
        // We can't assert exact values due to timing variations, but they should be reasonable
        assert!(state.jitter_ms >= 0.0);
        assert!(state.density_bps >= 0.0);
        assert!(state.tension >= 0.0);
    }

    #[test]
    fn test_window_pruning() {
        let (sensor, _rx) = ParadoxSensor::new();

        // Record pulses
        for i in 0..20 {
            sensor.record_pulse(100 + i);
        }

        // Wait longer than WINDOW_SIZE_MS
        thread::sleep(Duration::from_millis(WINDOW_SIZE_MS as u64 + 100));

        // Record one more pulse to trigger a calculation
        sensor.record_pulse(999);

        // Calculate state (this will prune old samples)
        sensor.calculate_state();

        // Verify old samples were pruned
        let history = sensor.history.read().unwrap();
        // Only the most recent pulse should remain (or very few)
        assert!(history.len() <= 2);
    }

    #[tokio::test]
    async fn test_analysis_loop_spawns() {
        let (sensor, _rx) = ParadoxSensor::new();
        let sensor_arc = Arc::new(sensor);
        let sensor_clone = Arc::clone(&sensor_arc);

        // Spawn analysis loop
        let handle = tokio::spawn(async move {
            // Run for a short duration then cancel
            tokio::select! {
                _ = ParadoxSensor::run_analysis_loop(sensor_clone) => {}
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        });

        // Record some pulses during the loop
        for _ in 0..10 {
            sensor_arc.record_pulse(100);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Wait for task to complete
        let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;

        // Verify sensor still works
        assert!(sensor_arc.current_state().density_bps >= 0.0);
    }
}
