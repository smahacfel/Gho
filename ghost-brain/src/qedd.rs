//! QEDD Engine
//!
//! Quantum Entropy-Driven Decay (QEDD) engine for computing survival probabilities
//! across multiple time horizons based on market signals.

use crate::config::qedd_config::QeddConfig;
use crate::models::qedd_result::{
    QeddResult, FEATURE_DEVIATION_RISK, FEATURE_LAMBDA_BASE, FEATURE_OUTFLOW,
    FEATURE_RESONANCE_RISK, FEATURE_SOBP_DROP,
};
use crate::signals::MarketSignals;

/// QEDD computation engine
#[derive(Clone)]
pub struct QeddEngine {
    /// Engine configuration
    pub config: QeddConfig,
}

impl QeddEngine {
    /// Create a new QEDD engine with the given configuration
    pub fn new(config: QeddConfig) -> Self {
        // Ensure decay multipliers are always consistent with horizons, even when
        // deserialized (skip fields default to 0.0).
        let mut config = config;
        config.decay_mult_1s = -(config.horizon_1s as f32 / 1000.0);
        config.decay_mult_5s = -(config.horizon_5s as f32 / 1000.0);
        config.decay_mult_30s = -(config.horizon_30s as f32 / 1000.0);
        config.decay_mult_60s = -(config.horizon_60s as f32 / 1000.0);

        Self { config }
    }

    /// Compute QEDD result from market signals
    ///
    /// Calculates the hazard rate λ and survival probabilities S(t) across multiple time horizons.
    ///
    /// Hazard rate formula:
    /// λ(t) = λ_base + α * SOBP_drop + β * outflow + γ * resonance_risk + δ * dev_risk
    ///
    /// Survival probability formula:
    /// S(t) = exp(-λ * t)
    ///
    /// # Arguments
    /// * `signals` - Aggregated market signals
    /// * `cancel_token` - Optional cancellation token for early abort (e.g., from Watchdog)
    ///
    /// # Returns
    /// QeddResult containing lambda and survival probabilities, or None if cancelled
    pub fn compute_qedd(
        &self,
        signals: &MarketSignals,
        cancel_token: Option<&tokio_util::sync::CancellationToken>,
    ) -> Option<QeddResult> {
        use std::time::Instant;
        let start = Instant::now();

        // Early exit if already cancelled
        if let Some(token) = cancel_token {
            if token.is_cancelled() {
                return None;
            }
        }

        // Calculate hazard rate λ using the enhanced formula
        let lambda = self.config.lambda_base
            + self.config.alpha_sobp_drop * (signals.sobp.drop as f32)
            + self.config.beta_outflow * (signals.flow.outflow as f32)
            + self.config.gamma_resonance * (signals.resonance.risk as f32)
            + self.config.delta_deviation * (signals.deviation.risk as f32);

        // Clamp lambda to reasonable bounds [0.0, infinity)
        let lambda = lambda.max(0.0);

        // Check cancellation before expensive exponential calculations
        if let Some(token) = cancel_token {
            if token.is_cancelled() {
                return None;
            }
        }

        // Calculate survival probabilities for each horizon: S(t) = exp(-λ * t)
        // Using pre-computed decay multipliers for zero-division performance
        // Note: decay_mult values are already negative (e.g., -1.0 for 1s)
        // So lambda * decay_mult_1s = lambda * (-1.0) = -lambda, giving us exp(-lambda)
        let survival_1s = (lambda * self.config.decay_mult_1s).exp();
        let survival_5s = (lambda * self.config.decay_mult_5s).exp();
        let survival_30s = (lambda * self.config.decay_mult_30s).exp();
        let survival_60s = (lambda * self.config.decay_mult_60s).exp();

        // Track which features were used in the computation using bitflags (zero allocation)
        let features_flags = FEATURE_LAMBDA_BASE
            | FEATURE_SOBP_DROP
            | FEATURE_OUTFLOW
            | FEATURE_RESONANCE_RISK
            | FEATURE_DEVIATION_RISK;

        let computation_ms = start.elapsed().as_micros() as u64 / 1000;

        let result = QeddResult {
            lambda_now: lambda,
            survival_1s,
            survival_5s,
            survival_30s,
            survival_60s,
            #[allow(deprecated)]
            features_used: vec![], // Empty for performance; use features_flags or call features_to_vec()
            features_flags,
            computation_ms,
        };

        Some(result)
    }

    /// Compute QEDD result from market signals (non-cancellable version for backward compatibility)
    ///
    /// This is a convenience wrapper around `compute_qedd` that doesn't support cancellation.
    /// For new code, prefer using `compute_qedd` with explicit cancellation token handling.
    pub fn compute_qedd_sync(&self, signals: &MarketSignals) -> QeddResult {
        self.compute_qedd(signals, None)
            .expect("compute_qedd without cancellation token should never return None")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let config = QeddConfig::default();
        let engine = QeddEngine::new(config);
        assert_eq!(engine.config.version, 1);
    }

    #[test]
    fn test_compute_qedd_basic() {
        let config = QeddConfig::default();
        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock();
        let result = engine.compute_qedd_sync(&signals);

        // Lambda should be computed, not zero
        assert!(result.lambda_now > 0.0);

        // Survival probabilities should be in [0, 1] and decreasing over time
        assert!(result.survival_1s >= 0.0 && result.survival_1s <= 1.0);
        assert!(result.survival_5s >= 0.0 && result.survival_5s <= 1.0);
        assert!(result.survival_30s >= 0.0 && result.survival_30s <= 1.0);
        assert!(result.survival_60s >= 0.0 && result.survival_60s <= 1.0);

        // Longer horizons should have lower survival probability
        assert!(result.survival_1s >= result.survival_5s);
        assert!(result.survival_5s >= result.survival_30s);
        assert!(result.survival_30s >= result.survival_60s);

        // Features should be tracked using bitflags
        assert!(result.features_flags != 0);
        // All 5 features should be used
        let expected_flags = FEATURE_LAMBDA_BASE
            | FEATURE_SOBP_DROP
            | FEATURE_OUTFLOW
            | FEATURE_RESONANCE_RISK
            | FEATURE_DEVIATION_RISK;
        assert_eq!(result.features_flags, expected_flags);
    }

    #[test]
    fn test_compute_qedd_hype_scenario() {
        let config = QeddConfig::default();
        let threshold = config.lambda_abort_threshold;
        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock_hype();
        let result = engine.compute_qedd_sync(&signals);

        // In hype scenario: low SOBP drop, low outflow, low resonance/deviation risk
        // Lambda should be relatively low (close to base around 0.54)
        assert!(
            result.lambda_now < 0.7,
            "Hype lambda should be low: {}",
            result.lambda_now
        );

        // Moderate-high survival probabilities expected
        // S(1s) ≈ 0.58, S(5s) ≈ 0.07
        assert!(
            result.survival_1s > 0.55,
            "Hype 1s survival should be above 0.55: {}",
            result.survival_1s
        );
        assert!(
            result.survival_5s > 0.05,
            "Hype 5s survival should be above 0.05: {}",
            result.survival_5s
        );

        // Survival probabilities should decrease with time
        assert!(result.survival_1s > result.survival_5s);
        assert!(result.survival_5s > result.survival_60s);

        // Should not trigger abort
        assert!(!result.should_abort(threshold));
    }

    #[test]
    fn test_compute_qedd_rug_scenario() {
        let config = QeddConfig::default();
        let threshold = config.lambda_abort_threshold;
        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock_rug();
        let result = engine.compute_qedd_sync(&signals);

        // In rug scenario: high SOBP drop, high outflow, high resonance/deviation risk
        // Lambda should be very high
        assert!(
            result.lambda_now > 1.0,
            "Rug lambda should be high: {}",
            result.lambda_now
        );

        // Low survival probabilities expected
        assert!(
            result.survival_1s < 0.5,
            "Rug 1s survival should be low: {}",
            result.survival_1s
        );
        assert!(
            result.survival_5s < 0.2,
            "Rug 5s survival should be low: {}",
            result.survival_5s
        );

        // Should trigger abort
        assert!(result.should_abort(threshold));
    }

    #[test]
    fn test_compute_qedd_stable_scenario() {
        let config = QeddConfig::default();
        let threshold = config.lambda_abort_threshold;
        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock_stable();
        let result = engine.compute_qedd_sync(&signals);

        // In stable scenario: moderate values across the board
        // Lambda should be moderate (around 0.78)
        assert!(
            result.lambda_now > 0.7 && result.lambda_now < 0.9,
            "Stable lambda should be moderate: {}",
            result.lambda_now
        );

        // Moderate survival probabilities (around 0.46 for 1s)
        assert!(result.survival_1s > 0.40 && result.survival_1s < 0.55);

        // Should not trigger abort
        assert!(!result.should_abort(threshold));
    }

    #[test]
    fn test_qedd_lambda_formula() {
        let mut config = QeddConfig::default();
        config.lambda_base = 0.5;
        config.alpha_sobp_drop = 0.3;
        config.beta_outflow = 0.25;
        config.gamma_resonance = 0.15;
        config.delta_deviation = 0.20;

        let engine = QeddEngine::new(config);

        // Create signals with known values
        let mut signals = MarketSignals::mock();
        signals.sobp.drop = 0.5;
        signals.flow.outflow = 0.4;
        signals.resonance.risk = 0.2;
        signals.deviation.risk = 0.3;

        let result = engine.compute_qedd_sync(&signals);

        // Expected: 0.5 + 0.3*0.5 + 0.25*0.4 + 0.15*0.2 + 0.20*0.3
        //         = 0.5 + 0.15 + 0.10 + 0.03 + 0.06 = 0.84
        let expected = 0.84;
        assert!(
            (result.lambda_now - expected).abs() < 0.01,
            "Lambda calculation incorrect: expected {}, got {}",
            expected,
            result.lambda_now
        );
    }

    #[test]
    fn test_qedd_survival_exponential_decay() {
        let config = QeddConfig::default();
        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock();
        let result = engine.compute_qedd_sync(&signals);

        // Verify exponential decay formula: S(t) = exp(-λ * t)
        let lambda = result.lambda_now;
        let expected_1s = (-lambda * 1.0).exp();
        let expected_5s = (-lambda * 5.0).exp();

        assert!((result.survival_1s - expected_1s).abs() < 0.001);
        assert!((result.survival_5s - expected_5s).abs() < 0.001);
    }

    #[test]
    fn test_qedd_boundary_conditions() {
        let config = QeddConfig::default();
        let engine = QeddEngine::new(config);

        // Test with all risk factors at maximum
        let mut signals = MarketSignals::mock();
        signals.sobp.drop = 1.0;
        signals.flow.outflow = 1.0;
        signals.resonance.risk = 1.0;
        signals.deviation.risk = 1.0;

        let result = engine.compute_qedd_sync(&signals);

        // Lambda should be very high but finite
        assert!(result.lambda_now > 0.0);
        assert!(result.lambda_now.is_finite());

        // Survival probabilities should still be in valid range
        assert!(result.survival_1s >= 0.0 && result.survival_1s <= 1.0);
        assert!(result.survival_60s >= 0.0 && result.survival_60s <= 1.0);
    }

    #[test]
    fn test_qedd_decay_recomputed_after_deserialize() {
        // Simulate a deserialized config where decay multipliers were skipped
        let mut config = QeddConfig::default();
        config.horizon_1s = 2000; // 2 seconds
        config.decay_mult_1s = 0.0;
        config.decay_mult_5s = 0.0;

        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock();
        let result = engine.compute_qedd_sync(&signals);

        let lambda = result.lambda_now;
        let expected_2s = (-lambda * 2.0).exp();
        let expected_5s = (-lambda * 5.0).exp();

        assert!(
            (result.survival_1s - expected_2s).abs() < 0.001,
            "1s horizon should reflect 2s custom horizon decay"
        );
        assert!(
            (result.survival_5s - expected_5s).abs() < 0.001,
            "5s survival should recompute decay multiplier"
        );
    }

    #[test]
    fn test_qedd_with_cancellation_token() {
        let config = QeddConfig::default();
        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock();

        // Test without cancellation
        let result = engine.compute_qedd(&signals, None);
        assert!(result.is_some());

        // Test with cancelled token
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let result = engine.compute_qedd(&signals, Some(&token));
        assert!(result.is_none(), "Should return None when cancelled");

        // Test with active token
        let token = tokio_util::sync::CancellationToken::new();
        let result = engine.compute_qedd(&signals, Some(&token));
        assert!(result.is_some());
    }
}
