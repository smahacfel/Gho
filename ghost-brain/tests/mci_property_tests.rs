//! Property-based Tests for MCI (Market Coherence Index)
//!
//! This module contains property-based tests using `proptest` to verify
//! critical invariants and mathematical properties of the MCI engine.
//!
//! ## Properties Tested
//!
//! 1. **Bounds Invariance**: MCI, DC, and SC must always be in [0.0, 1.0]
//! 2. **Monotonicity of Stability**: Higher SOBP stability → Higher SC
//! 3. **Stability Under Noise**: Small input perturbations → Small output changes
//! 4. **Weight Consistency**: Changing weights maintains valid bounds
//! 5. **Component Consistency**: DC and SC contribute correctly to MCI

use ghost_brain::{MarketSignals, MciConfig, MciEngine};
use proptest::prelude::*;

// =============================================================================
// Test Strategies (Input Generators)
// =============================================================================

/// Generate valid MciConfig with random weights
fn valid_mci_config() -> impl Strategy<Value = MciConfig> {
    (0.0f32..=1.0, 0.0f32..=1.0).prop_map(|(w_dc, w_sc)| {
        let mut config = MciConfig::default();
        config.weight_dc = w_dc;
        config.weight_sc = w_sc;
        config
    })
}

/// Generate valid MarketSignals with all fields in valid ranges
fn valid_market_signals() -> impl Strategy<Value = MarketSignals> {
    (
        -1.0f64..=1.0,   // qass_alignment
        0.0f64..=1.0,    // magnitude
        0.0f64..=1.0,    // mpcf
        0.0f64..=1.0,    // combined entropy
        0.0f64..=1.0,    // sobp_drop
        0.0f64..=1.0,    // deviation_risk
        1.0f64..=1000.0, // sobp_current
        1.0f64..=1000.0, // sobp_ma
    )
        .prop_map(
            |(qass_align, mag, mpcf, entropy, drop, dev_risk, sobp_curr, sobp_ma)| {
                let mut signals = MarketSignals::mock();
                signals.flow.qass_alignment = qass_align;
                signals.flow.magnitude = mag;
                signals.entropy.mpcf = mpcf;
                signals.entropy.combined = entropy;
                signals.sobp.drop = drop;
                signals.sobp.current = sobp_curr;
                signals.sobp.ma = sobp_ma;
                signals.deviation.risk = dev_risk;
                signals
            },
        )
}

// =============================================================================
// Property 1: Bounds Invariance - MCI ∈ [0, 1]
// =============================================================================

proptest! {
    /// Property: MCI must always be in [0.0, 1.0] regardless of input
    ///
    /// This is a critical safety property ensuring that the MCI score
    /// never produces invalid values that could cause downstream issues.
    #[test]
    fn prop_mci_bounds(signals in valid_market_signals(), config in valid_mci_config()) {
        let engine = MciEngine::new(config);
        let result = engine.compute_mci(&signals);

        // Core invariant: MCI must be in valid range
        prop_assert!(
            result.mci >= 0.0 && result.mci <= 1.0,
            "MCI out of bounds: {} (expected [0.0, 1.0])",
            result.mci
        );

        // DC must be in valid range
        prop_assert!(
            result.dc >= 0.0 && result.dc <= 1.0,
            "DC out of bounds: {} (expected [0.0, 1.0])",
            result.dc
        );

        // SC must be in valid range
        prop_assert!(
            result.sc >= 0.0 && result.sc <= 1.0,
            "SC out of bounds: {} (expected [0.0, 1.0])",
            result.sc
        );
    }

    /// Property: MCI bounds hold even with extreme signal values
    ///
    /// Test with pathological inputs to ensure clamping works correctly.
    #[test]
    fn prop_mci_bounds_extreme_inputs(
        qass_align in -10.0f64..=10.0,
        magnitude in 0.0f64..=10.0,
        entropy in -1.0f64..=2.0,
        drop in -1.0f64..=2.0,
    ) {
        let mut signals = MarketSignals::mock();
        signals.flow.qass_alignment = qass_align;
        signals.flow.magnitude = magnitude.clamp(0.0, 1.0);
        signals.entropy.combined = entropy;
        signals.sobp.drop = drop;

        let config = MciConfig::default();
        let engine = MciEngine::new(config);
        let result = engine.compute_mci(&signals);

        // Even with extreme inputs, outputs must be bounded
        prop_assert!(result.mci >= 0.0 && result.mci <= 1.0);
        prop_assert!(result.dc >= 0.0 && result.dc <= 1.0);
        prop_assert!(result.sc >= 0.0 && result.sc <= 1.0);
    }
}

// =============================================================================
// Property 2: Monotonicity of Stability (SOBP)
// =============================================================================

proptest! {
    /// Property: Higher SOBP stability → Higher Structural Coherence
    ///
    /// SOBP stability is a key component of SC. When SOBP improves
    /// (lower drop, closer to MA), SC should increase, all else equal.
    #[test]
    fn prop_sobp_monotonicity(
        base_signals in valid_market_signals(),
        drop_increase in 0.0f64..=0.5,
    ) {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Baseline calculation
        let result_baseline = engine.compute_mci(&base_signals);

        // Create degraded version with higher SOBP drop (worse stability)
        let mut degraded = base_signals.clone();
        degraded.sobp.drop = (degraded.sobp.drop + drop_increase).min(1.0);

        let result_degraded = engine.compute_mci(&degraded);

        // Property: Increasing drop (worse stability) should decrease SC
        // Allow small floating point tolerance
        let tolerance = 0.001;
        if drop_increase > 0.01 {  // Only test meaningful changes
            prop_assert!(
                result_degraded.sc <= result_baseline.sc + tolerance,
                "SC should decrease with higher SOBP drop. Baseline SC: {}, Degraded SC: {} (drop increased by {})",
                result_baseline.sc,
                result_degraded.sc,
                drop_increase
            );
        }
    }

    /// Property: Closer SOBP.current to SOBP.ma → Higher SC
    ///
    /// When current buying pressure matches historical average,
    /// market is more stable and coherent.
    #[test]
    fn prop_sobp_ma_proximity_improves_sc(
        base_entropy in 0.5f64..=1.0,
        base_drop in 0.0f64..=0.3,
        ma_deviation_pct in 0.0f64..=0.5,
    ) {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Create signals with SOBP close to MA
        let mut signals_close = MarketSignals::mock();
        signals_close.sobp.ma = 100.0;
        signals_close.sobp.current = 100.0;  // Exactly at MA
        signals_close.sobp.drop = base_drop;
        signals_close.entropy.mpcf = base_entropy;
        signals_close.entropy.combined = base_entropy;
        signals_close.deviation.risk = 0.3;

        // Create signals with SOBP far from MA
        let mut signals_far = signals_close.clone();
        signals_far.sobp.current = 100.0 * (1.0 + ma_deviation_pct);

        let result_close = engine.compute_mci(&signals_close);
        let result_far = engine.compute_mci(&signals_far);

        // Property: Closer to MA should have higher or equal SC
        if ma_deviation_pct > 0.05 {  // Only test meaningful deviations
            prop_assert!(
                result_far.sc <= result_close.sc + 0.001,
                "SC should be higher when SOBP.current is closer to MA. Close: {}, Far: {} (deviation: {}%)",
                result_close.sc,
                result_far.sc,
                ma_deviation_pct * 100.0
            );
        }
    }
}

// =============================================================================
// Property 3: Stability Under Noise
// =============================================================================

proptest! {
    /// Property: Small input perturbations → Small output changes (Lipschitz continuity)
    ///
    /// The MCI function should be reasonably continuous. Small changes
    /// in input signals should not cause large jumps in output.
    #[test]
    fn prop_stability_under_noise(
        base_signals in valid_market_signals(),
        noise_scale in 0.0f64..=0.1,  // Max 10% perturbation
    ) {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        let result_base = engine.compute_mci(&base_signals);

        // Apply small perturbation to each signal component
        let mut perturbed = base_signals.clone();
        perturbed.flow.qass_alignment = (perturbed.flow.qass_alignment + noise_scale * 0.1).clamp(-1.0, 1.0);
        perturbed.flow.magnitude = (perturbed.flow.magnitude + noise_scale * 0.1).clamp(0.0, 1.0);
        perturbed.entropy.mpcf = (perturbed.entropy.mpcf + noise_scale * 0.1).clamp(0.0, 1.0);
        perturbed.entropy.combined = (perturbed.entropy.combined + noise_scale * 0.1).clamp(0.0, 1.0);
        perturbed.sobp.drop = (perturbed.sobp.drop + noise_scale * 0.1).clamp(0.0, 1.0);
        perturbed.deviation.risk = (perturbed.deviation.risk + noise_scale * 0.1).clamp(0.0, 1.0);

        let result_perturbed = engine.compute_mci(&perturbed);

        // Property: Output change should be proportional to input change
        // With 10% input noise, expect at most ~20% output change (Lipschitz constant ~2)
        let mci_change = (result_perturbed.mci - result_base.mci).abs();
        let max_expected_change = noise_scale * 2.0; // Lipschitz constant estimate

        prop_assert!(
            mci_change <= max_expected_change as f32,
            "MCI changed too much for small input perturbation. Change: {}, Noise: {}, Expected max: {}",
            mci_change,
            noise_scale,
            max_expected_change
        );
    }

    /// Property: Adding noise should not violate bounds
    ///
    /// Even with noisy inputs, clamping should ensure outputs stay valid.
    #[test]
    fn prop_noise_preserves_bounds(
        base_signals in valid_market_signals(),
        noise in -0.2f64..=0.2,
    ) {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        let mut noisy = base_signals;
        noisy.flow.qass_alignment = (noisy.flow.qass_alignment + noise).clamp(-1.0, 1.0);
        noisy.entropy.mpcf = (noisy.entropy.mpcf + noise).clamp(0.0, 1.0);

        let result = engine.compute_mci(&noisy);

        prop_assert!(result.mci >= 0.0 && result.mci <= 1.0);
        prop_assert!(result.dc >= 0.0 && result.dc <= 1.0);
        prop_assert!(result.sc >= 0.0 && result.sc <= 1.0);
    }
}

// =============================================================================
// Property 4: Weight Consistency
// =============================================================================

proptest! {
    /// Property: Changing weights preserves bounds
    ///
    /// Regardless of weight values, MCI must stay in [0, 1].
    #[test]
    fn prop_weights_preserve_bounds(
        signals in valid_market_signals(),
        weight_dc in 0.0f32..=1.0,
        weight_sc in 0.0f32..=1.0,
    ) {
        let mut config = MciConfig::default();
        config.weight_dc = weight_dc;
        config.weight_sc = weight_sc;

        let engine = MciEngine::new(config);
        let result = engine.compute_mci(&signals);

        prop_assert!(result.mci >= 0.0 && result.mci <= 1.0);
        prop_assert!(result.dc >= 0.0 && result.dc <= 1.0);
        prop_assert!(result.sc >= 0.0 && result.sc <= 1.0);
    }

    /// Property: MCI increases with DC weight when DC > SC
    ///
    /// If DC is higher than SC, increasing DC weight should increase MCI.
    #[test]
    fn prop_dc_weight_effect(
        signals in valid_market_signals(),
    ) {
        // Create signals where DC is high, SC is low
        let mut signals_dc_dominant = signals;
        signals_dc_dominant.flow.qass_alignment = 0.9;  // High alignment
        signals_dc_dominant.flow.magnitude = 0.9;
        signals_dc_dominant.entropy.mpcf = 0.2;  // Low entropy
        signals_dc_dominant.entropy.combined = 0.2;
        signals_dc_dominant.deviation.risk = 0.8;  // High deviation

        // Low DC weight
        let mut config_low = MciConfig::default();
        config_low.weight_dc = 0.3;
        config_low.weight_sc = 0.7;

        // High DC weight
        let mut config_high = MciConfig::default();
        config_high.weight_dc = 0.7;
        config_high.weight_sc = 0.3;

        let engine_low = MciEngine::new(config_low);
        let engine_high = MciEngine::new(config_high);

        let result_low = engine_low.compute_mci(&signals_dc_dominant);
        let result_high = engine_high.compute_mci(&signals_dc_dominant);

        // When DC is strong, higher DC weight should increase MCI
        // (assuming DC > SC in this scenario)
        if result_low.dc > result_low.sc {
            prop_assert!(
                result_high.mci >= result_low.mci - 0.001,
                "Higher DC weight should increase MCI when DC > SC. Low: {}, High: {}",
                result_low.mci,
                result_high.mci
            );
        }
    }
}

// =============================================================================
// Property 5: Component Consistency
// =============================================================================

proptest! {
    /// Property: MCI is a weighted combination of DC and SC
    ///
    /// Verify that MCI = weight_dc * DC + weight_sc * SC (after clamping).
    #[test]
    fn prop_mci_weighted_combination(
        signals in valid_market_signals(),
        config in valid_mci_config(),
    ) {
        let engine = MciEngine::new(config.clone());
        let result = engine.compute_mci(&signals);

        // Calculate expected MCI from weighted formula
        let expected_mci = (config.weight_dc * result.dc + config.weight_sc * result.sc)
            .max(0.0)
            .min(1.0);

        // Allow small floating point tolerance
        let diff = (result.mci - expected_mci).abs();
        prop_assert!(
            diff < 0.001,
            "MCI should match weighted formula. Actual: {}, Expected: {}, DC: {}, SC: {}, w_dc: {}, w_sc: {}",
            result.mci,
            expected_mci,
            result.dc,
            result.sc,
            config.weight_dc,
            config.weight_sc
        );
    }

    /// Property: DC correlates with QASS alignment
    ///
    /// Higher QASS alignment (closer to 1.0) should increase DC.
    #[test]
    fn prop_dc_correlates_with_alignment(
        base_magnitude in 0.5f64..=1.0,
        alignment_low in -0.5f64..=0.0,
        alignment_high in 0.5f64..=1.0,
    ) {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Low alignment scenario
        let mut signals_low = MarketSignals::mock();
        signals_low.flow.qass_alignment = alignment_low;
        signals_low.flow.magnitude = base_magnitude;

        // High alignment scenario
        let mut signals_high = MarketSignals::mock();
        signals_high.flow.qass_alignment = alignment_high;
        signals_high.flow.magnitude = base_magnitude;

        let result_low = engine.compute_mci(&signals_low);
        let result_high = engine.compute_mci(&signals_high);

        // Property: Higher alignment → Higher DC
        prop_assert!(
            result_high.dc >= result_low.dc,
            "Higher QASS alignment should give higher DC. Low: {} (align={}), High: {} (align={})",
            result_low.dc,
            alignment_low,
            result_high.dc,
            alignment_high
        );
    }

    /// Property: SC increases with higher entropy
    ///
    /// Higher combined entropy should contribute to higher SC.
    #[test]
    fn prop_sc_increases_with_entropy(
        entropy_low in 0.1f64..=0.3,
        entropy_high in 0.7f64..=1.0,
    ) {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);

        // Low entropy scenario
        let mut signals_low = MarketSignals::mock();
        signals_low.entropy.combined = entropy_low;
        signals_low.entropy.mpcf = entropy_low;
        signals_low.sobp.drop = 0.3;
        signals_low.deviation.risk = 0.5;

        // High entropy scenario
        let mut signals_high = signals_low.clone();
        signals_high.entropy.combined = entropy_high;
        signals_high.entropy.mpcf = entropy_high;

        let result_low = engine.compute_mci(&signals_low);
        let result_high = engine.compute_mci(&signals_high);

        // Property: Higher entropy → Higher SC (all else equal)
        prop_assert!(
            result_high.sc >= result_low.sc - 0.001,
            "Higher entropy should give higher SC. Low: {} (entropy={}), High: {} (entropy={})",
            result_low.sc,
            entropy_low,
            result_high.sc,
            entropy_high
        );
    }
}

// =============================================================================
// Property 6: Abort Threshold Consistency
// =============================================================================

proptest! {
    /// Property: should_abort is consistent with MCI value
    ///
    /// Verify that abort logic correctly compares MCI to threshold.
    #[test]
    fn prop_abort_threshold_consistency(
        signals in valid_market_signals(),
        threshold in 0.0f32..=1.0,
    ) {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);
        let result = engine.compute_mci(&signals);

        let should_abort = result.should_abort(threshold);

        // Property: should_abort iff MCI < threshold
        if result.mci < threshold {
            prop_assert!(should_abort, "Should abort when MCI ({}) < threshold ({})", result.mci, threshold);
        } else {
            prop_assert!(!should_abort, "Should not abort when MCI ({}) >= threshold ({})", result.mci, threshold);
        }
    }
}
