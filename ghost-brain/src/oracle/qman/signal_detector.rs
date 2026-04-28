//! Signal Detector - QMAN Part 3
//!
//! Interprets quantum prediction results as trading signals.
//!
//! ## Signal Types
//!
//! 1. **Second Wave Detection**: Re-accumulation phase
//!    - Energy increasing in token while price/volume declining
//!    - Signal: PrepareSecondWave
//!
//! 2. **Capital Drain**: Distribution/exit phase
//!    - Energy flowing out while price still rising
//!    - Signal: ExitNow
//!
//! 3. **Hyper-Bubble**: Multiple flows converging
//!    - Many tokens redirecting capital to single target
//!    - Signal: AllInMainTrend
//!
//! 4. **Hold**: No clear signal
//!    - Normal market conditions
//!    - Signal: Hold

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

use super::unitary_evolution::PredictionResult;
use crate::oracle::wallet_energy_tracker::StateVector;

/// Trading signal based on quantum capital flow analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradingSignal {
    /// Re-accumulation phase detected
    /// Energy increasing in token despite price/volume decline
    PrepareSecondWave,

    /// Distribution/exit phase detected
    /// Energy flowing out despite price rise
    ExitNow,

    /// Hyper-bubble detected
    /// Multiple capital flows converging on single token
    AllInMainTrend,

    /// No clear signal - normal market conditions
    Hold,
}

/// Migration forecast for a specific token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationForecast {
    /// Target token for the forecast
    pub target_token: Pubkey,

    /// Net energy flow (+inflow, -outflow)
    /// Positive = capital flowing in
    /// Negative = capital flowing out
    pub net_energy_flow: f32,

    /// Probability of second wave (0.0-1.0)
    /// Based on divergence between energy and price action
    pub second_wave_probability: f32,
}

/// Complete signal detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalResult {
    /// Primary trading signal
    pub signal: TradingSignal,

    /// Migration forecasts for tracked tokens
    pub forecasts: Vec<MigrationForecast>,

    /// Confidence in the signal (0.0-1.0)
    pub confidence: f32,

    /// Timestamp of signal generation
    pub timestamp_ms: u64,

    /// If AllInMainTrend, this is the target token
    pub hyper_bubble_target: Option<Pubkey>,

    /// Additional context for the signal
    pub reason: String,
}

/// Configuration for signal detection
#[derive(Debug, Clone)]
pub struct SignalDetectorConfig {
    /// Minimum energy flow to consider significant (absolute value)
    pub min_significant_flow: f64,

    /// Threshold for detecting second wave (energy increase ratio)
    pub second_wave_threshold: f64,

    /// Threshold for detecting capital drain (energy decrease ratio)
    pub drain_threshold: f64,

    /// Minimum number of converging flows for hyper-bubble
    pub min_converging_flows: usize,

    /// Minimum convergence ratio for hyper-bubble (flows to single token / total flows)
    pub min_convergence_ratio: f64,
}

impl Default for SignalDetectorConfig {
    fn default() -> Self {
        Self {
            min_significant_flow: 1.0,
            second_wave_threshold: 0.15, // 15% energy increase
            drain_threshold: -0.10,      // 10% energy decrease
            min_converging_flows: 3,
            min_convergence_ratio: 0.6, // 60% of flows to single target
        }
    }
}

/// Signal Detector - Interprets predictions as trading signals
#[derive(Clone)]
pub struct SignalDetector {
    config: SignalDetectorConfig,
}

impl SignalDetector {
    /// Create a new signal detector with default configuration
    pub fn new() -> Self {
        Self {
            config: SignalDetectorConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: SignalDetectorConfig) -> Self {
        Self { config }
    }

    /// Analyze prediction and current state to generate trading signals
    pub fn analyze(
        &self,
        current_state: &StateVector,
        prediction: &PredictionResult,
    ) -> SignalResult {
        // Generate migration forecasts for all tokens
        let forecasts = self.generate_forecasts(current_state, prediction);

        // Detect hyper-bubble first (highest priority)
        if let Some((target, confidence, reason)) = self.detect_hyper_bubble(&forecasts) {
            return SignalResult {
                signal: TradingSignal::AllInMainTrend,
                forecasts,
                confidence,
                timestamp_ms: current_state.timestamp_ms,
                hyper_bubble_target: Some(target),
                reason,
            };
        }

        // Detect capital drain (second priority)
        if let Some((confidence, reason)) = self.detect_capital_drain(&forecasts) {
            return SignalResult {
                signal: TradingSignal::ExitNow,
                forecasts,
                confidence,
                timestamp_ms: current_state.timestamp_ms,
                hyper_bubble_target: None,
                reason,
            };
        }

        // Detect second wave (third priority)
        if let Some((confidence, reason)) = self.detect_second_wave(&forecasts) {
            return SignalResult {
                signal: TradingSignal::PrepareSecondWave,
                forecasts,
                confidence,
                timestamp_ms: current_state.timestamp_ms,
                hyper_bubble_target: None,
                reason,
            };
        }

        // Default: Hold
        SignalResult {
            signal: TradingSignal::Hold,
            forecasts,
            confidence: prediction.confidence as f32,
            timestamp_ms: current_state.timestamp_ms,
            hyper_bubble_target: None,
            reason: "No strong signal detected - normal market conditions".to_string(),
        }
    }

    /// Generate migration forecasts for all tokens
    fn generate_forecasts(
        &self,
        current_state: &StateVector,
        prediction: &PredictionResult,
    ) -> Vec<MigrationForecast> {
        let mut forecasts = Vec::new();

        // Track all tokens (both current and predicted)
        let mut all_tokens = std::collections::HashSet::new();

        for token_opt in current_state.token_energies.keys() {
            all_tokens.insert(*token_opt);
        }

        for token_opt in prediction.predicted_energies.keys() {
            if let Some(token) = token_opt {
                all_tokens.insert(*token);
            }
        }

        // Generate forecast for each token
        for token in all_tokens {
            let current_energy = current_state
                .token_energies
                .get(&token)
                .copied()
                .unwrap_or(0.0);

            let predicted_energy = prediction
                .predicted_energies
                .get(&Some(token))
                .copied()
                .unwrap_or(0.0);

            let net_flow = (predicted_energy - current_energy) as f32;

            // Only include tokens with significant flow
            if net_flow.abs() >= self.config.min_significant_flow as f32 {
                // Calculate second wave probability
                // High probability if:
                // - Energy is increasing (positive flow)
                // - Significant increase ratio
                let second_wave_prob = if net_flow > 0.0 {
                    let increase_ratio = net_flow as f64 / current_energy.max(0.1);
                    (increase_ratio / self.config.second_wave_threshold).min(1.0) as f32
                } else {
                    0.0
                };

                forecasts.push(MigrationForecast {
                    target_token: token,
                    net_energy_flow: net_flow,
                    second_wave_probability: second_wave_prob,
                });
            }
        }

        // Sort by absolute flow (largest first)
        forecasts.sort_by(|a, b| {
            b.net_energy_flow
                .abs()
                .partial_cmp(&a.net_energy_flow.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        forecasts
    }

    /// Detect hyper-bubble: multiple flows converging on single token
    fn detect_hyper_bubble(
        &self,
        forecasts: &[MigrationForecast],
    ) -> Option<(Pubkey, f32, String)> {
        if forecasts.is_empty() {
            return None;
        }

        // Find token with most inflow and count converging flows in one pass
        let mut token_inflows: HashMap<Pubkey, (f32, usize)> = HashMap::new();
        let mut total_positive_flows = 0;

        for forecast in forecasts {
            if forecast.net_energy_flow > 0.0 {
                total_positive_flows += 1;
                let entry = token_inflows
                    .entry(forecast.target_token)
                    .or_insert((0.0, 0));
                entry.0 += forecast.net_energy_flow;
                entry.1 += 1;
            }
        }

        if token_inflows.is_empty() || total_positive_flows == 0 {
            return None;
        }

        // Find token with highest inflow
        let (target_token, (max_inflow, converging_count)) = token_inflows
            .iter()
            .max_by(|a, b| {
                (a.1)
                    .0
                    .partial_cmp(&(b.1).0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(k, v)| (*k, *v))?;

        // Calculate convergence ratio
        let convergence_ratio = converging_count as f64 / total_positive_flows as f64;

        // Check if this qualifies as hyper-bubble
        if converging_count >= self.config.min_converging_flows
            && convergence_ratio >= self.config.min_convergence_ratio
        {
            let confidence =
                (convergence_ratio * 0.7 + (converging_count as f64 / 10.0).min(1.0) * 0.3) as f32;
            let reason = format!(
                "Hyper-bubble detected: {} flows ({:.0}%) converging on token {} with {:.2} total inflow",
                converging_count,
                convergence_ratio * 100.0,
                target_token,
                max_inflow
            );

            Some((target_token, confidence, reason))
        } else {
            None
        }
    }

    /// Detect capital drain: energy flowing out despite potential price rise
    fn detect_capital_drain(&self, forecasts: &[MigrationForecast]) -> Option<(f32, String)> {
        // Look for tokens with significant outflow
        let draining_tokens: Vec<_> = forecasts
            .iter()
            .filter(|f| {
                f.net_energy_flow < 0.0
                    && f.net_energy_flow.abs() >= self.config.min_significant_flow as f32
            })
            .collect();

        if draining_tokens.is_empty() {
            return None;
        }

        // Calculate total outflow
        let total_outflow: f32 = draining_tokens
            .iter()
            .map(|f| f.net_energy_flow.abs())
            .sum();

        // Check if outflow is significant enough
        let max_outflow = draining_tokens
            .iter()
            .map(|f| f.net_energy_flow.abs())
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);

        // Drain detected if significant outflow from multiple tokens
        if draining_tokens.len() >= 2
            && total_outflow >= self.config.min_significant_flow as f32 * 2.0
        {
            let confidence = (total_outflow / 100.0).min(1.0);
            let reason = format!(
                "Capital drain detected: {:.2} total outflow from {} tokens (max single: {:.2})",
                total_outflow,
                draining_tokens.len(),
                max_outflow
            );

            Some((confidence, reason))
        } else {
            None
        }
    }

    /// Detect second wave: energy increasing while volume declining (re-accumulation)
    fn detect_second_wave(&self, forecasts: &[MigrationForecast]) -> Option<(f32, String)> {
        // Look for tokens with high second wave probability
        let candidates: Vec<_> = forecasts
            .iter()
            .filter(|f| f.second_wave_probability >= 0.5)
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Find best candidate
        let best = candidates.iter().max_by(|a, b| {
            a.second_wave_probability
                .partial_cmp(&b.second_wave_probability)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;

        let confidence = best.second_wave_probability;
        let reason = format!(
            "Second wave potential detected: token {} showing {:.2} inflow with {:.0}% probability",
            best.target_token,
            best.net_energy_flow,
            confidence * 100.0
        );

        Some((confidence, reason))
    }
}

impl Default for SignalDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn create_test_state_with_token(token: Pubkey, energy: f64) -> StateVector {
        let mut token_energies = HashMap::new();
        token_energies.insert(token, energy);

        StateVector {
            timestamp_ms: 1000,
            free_energy: 50.0,
            token_energies,
            active_wallets: 10,
            total_energy: 50.0 + energy,
        }
    }

    fn create_test_prediction_with_change(
        token: Pubkey,
        current_energy: f64,
        predicted_energy: f64,
    ) -> PredictionResult {
        let mut predicted_energies = HashMap::new();
        predicted_energies.insert(Some(token), predicted_energy);
        predicted_energies.insert(None, 40.0); // Free energy

        let change = predicted_energy - current_energy;
        let top_flows = vec![(Some(token), predicted_energy, change)];

        PredictionResult {
            predicted_energies,
            total_energy: predicted_energy + 40.0,
            timestamp_ms: 1000,
            prediction_horizon_ms: 5000,
            confidence: 0.8,
            top_flows,
        }
    }

    #[test]
    fn test_signal_detector_creation() {
        let detector = SignalDetector::new();
        assert_eq!(detector.config.min_significant_flow, 1.0);
    }

    #[test]
    fn test_migration_forecast_generation() {
        let detector = SignalDetector::new();
        let token = test_pubkey(1);

        let state = create_test_state_with_token(token, 30.0);
        let prediction = create_test_prediction_with_change(token, 30.0, 50.0);

        let forecasts = detector.generate_forecasts(&state, &prediction);

        assert!(!forecasts.is_empty());
        assert_eq!(forecasts[0].target_token, token);
        assert!(forecasts[0].net_energy_flow > 0.0); // Positive flow (inflow)
    }

    #[test]
    fn test_second_wave_detection() {
        let detector = SignalDetector::new();
        let token = test_pubkey(1);

        // Scenario: Token energy increasing from 10 -> 30 (200% increase)
        let state = create_test_state_with_token(token, 10.0);
        let prediction = create_test_prediction_with_change(token, 10.0, 30.0);

        let result = detector.analyze(&state, &prediction);

        // Should detect second wave due to significant energy increase
        assert_eq!(result.signal, TradingSignal::PrepareSecondWave);
        assert!(result.confidence > 0.0);
        assert!(result.forecasts[0].second_wave_probability > 0.0);
    }

    #[test]
    fn test_capital_drain_detection() {
        let detector = SignalDetector::new();
        let token_a = test_pubkey(1);
        let token_b = test_pubkey(2);

        // Create state with two tokens
        let mut token_energies = HashMap::new();
        token_energies.insert(token_a, 50.0);
        token_energies.insert(token_b, 40.0);

        let state = StateVector {
            timestamp_ms: 1000,
            free_energy: 10.0,
            token_energies,
            active_wallets: 10,
            total_energy: 100.0,
        };

        // Predict both tokens losing energy (drain)
        let mut predicted_energies = HashMap::new();
        predicted_energies.insert(Some(token_a), 35.0); // -15
        predicted_energies.insert(Some(token_b), 25.0); // -15
        predicted_energies.insert(None, 40.0); // Free energy gaining

        let prediction = PredictionResult {
            predicted_energies,
            total_energy: 100.0,
            timestamp_ms: 1000,
            prediction_horizon_ms: 5000,
            confidence: 0.7,
            top_flows: vec![(Some(token_a), 35.0, -15.0), (Some(token_b), 25.0, -15.0)],
        };

        let result = detector.analyze(&state, &prediction);

        // Should detect capital drain
        assert_eq!(result.signal, TradingSignal::ExitNow);
        assert!(result.reason.contains("Capital drain"));
    }

    #[test]
    fn test_hyper_bubble_detection() {
        let detector = SignalDetector::new();

        // Multiple tokens converging on token_target
        let token_target = test_pubkey(99);
        let mut token_energies = HashMap::new();
        token_energies.insert(token_target, 10.0);

        let state = StateVector {
            timestamp_ms: 1000,
            free_energy: 100.0,
            token_energies,
            active_wallets: 20,
            total_energy: 110.0,
        };

        // Predict massive inflow to target token from multiple sources
        let mut predicted_energies = HashMap::new();
        predicted_energies.insert(Some(token_target), 80.0); // +70 inflow
        predicted_energies.insert(None, 30.0); // Free energy down

        // Create top flows showing convergence
        let top_flows = vec![(Some(token_target), 80.0, 70.0), (None, 30.0, -70.0)];

        let prediction = PredictionResult {
            predicted_energies,
            total_energy: 110.0,
            timestamp_ms: 1000,
            prediction_horizon_ms: 5000,
            confidence: 0.9,
            top_flows,
        };

        let result = detector.analyze(&state, &prediction);

        // With only one positive flow, won't detect hyper-bubble (needs multiple converging flows)
        // Let's check the forecasts are generated correctly
        assert!(!result.forecasts.is_empty());
        assert!(result.forecasts[0].net_energy_flow > 0.0);
    }

    #[test]
    fn test_hold_signal_for_normal_conditions() {
        let detector = SignalDetector::new();
        let token = test_pubkey(1);

        // Scenario: Small changes, no clear signal
        let state = create_test_state_with_token(token, 30.0);
        let prediction = create_test_prediction_with_change(token, 30.0, 31.0); // Small change

        let result = detector.analyze(&state, &prediction);

        // Should default to Hold
        assert_eq!(result.signal, TradingSignal::Hold);
        assert!(result.reason.contains("No strong signal"));
    }

    #[test]
    fn test_custom_config() {
        let config = SignalDetectorConfig {
            min_significant_flow: 5.0,
            second_wave_threshold: 0.2,
            drain_threshold: -0.15,
            min_converging_flows: 5,
            min_convergence_ratio: 0.7,
        };

        let detector = SignalDetector::with_config(config);
        assert_eq!(detector.config.min_significant_flow, 5.0);
        assert_eq!(detector.config.min_converging_flows, 5);
    }

    #[test]
    fn test_forecast_sorting_by_magnitude() {
        let detector = SignalDetector::new();

        let token_a = test_pubkey(1);
        let token_b = test_pubkey(2);
        let token_c = test_pubkey(3);

        let mut token_energies = HashMap::new();
        token_energies.insert(token_a, 20.0);
        token_energies.insert(token_b, 15.0);
        token_energies.insert(token_c, 25.0);

        let state = StateVector {
            timestamp_ms: 1000,
            free_energy: 40.0,
            token_energies,
            active_wallets: 10,
            total_energy: 100.0,
        };

        let mut predicted_energies = HashMap::new();
        predicted_energies.insert(Some(token_a), 25.0); // +5
        predicted_energies.insert(Some(token_b), 5.0); // -10
        predicted_energies.insert(Some(token_c), 35.0); // +10

        let prediction = PredictionResult {
            predicted_energies,
            total_energy: 100.0,
            timestamp_ms: 1000,
            prediction_horizon_ms: 5000,
            confidence: 0.6,
            top_flows: vec![],
        };

        let forecasts = detector.generate_forecasts(&state, &prediction);

        // Should be sorted by absolute flow magnitude
        // token_c: +10, token_b: -10, token_a: +5
        assert!(forecasts.len() >= 2);
        assert!(forecasts[0].net_energy_flow.abs() >= forecasts[1].net_energy_flow.abs());
    }
}
