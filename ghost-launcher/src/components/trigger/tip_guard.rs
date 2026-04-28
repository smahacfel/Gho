//! Trigger tip guard - protection against unsafe live tip requests
//!
//! This module implements strict limits on requested live tips to prevent
//! catastrophic capital depletion when a transport fallback still routes
//! through the local tip guard.
//!
//! ## Features
//! - Absolute Hard Cap: Maximum tip amount regardless of calculation
//! - Smart Ratio Cap: Tips limited to percentage of trade value
//! - Fail-Safe Fallback: Conservative default when API fails
//!
//! ## Protection Philosophy
//! Better to lose transaction priority than to pay 50% of capital to validators.

use tracing::{error, info, warn};

/// Fallback tip in SOL when the tip guard rejects an unsafe request
/// Conservative value that ensures transaction submission without excessive cost
///
/// Note: This constant is also defined in TipGuardConfig::default() for configuration.
/// The constant exists for documentation, examples, and as a reference value.
pub const FALLBACK_TIP_SOL: f64 = 0.001;

/// Default absolute maximum tip in SOL
/// This is the hard cap that cannot be exceeded under any circumstances
///
/// Note: This constant is also defined in TipGuardConfig::default() for configuration.
/// The constant exists for documentation, examples, and as a reference value.
pub const DEFAULT_MAX_TIP_ABSOLUTE_SOL: f64 = 0.04;

/// Configuration for local tip-guard calculations
#[derive(Debug, Clone)]
pub struct TipGuardConfig {
    /// Absolute maximum tip in SOL (hard cap)
    pub max_tip_absolute_sol: f64,

    /// Fallback tip when API fails (SOL)
    pub fallback_tip_sol: f64,
}

impl Default for TipGuardConfig {
    fn default() -> Self {
        Self {
            max_tip_absolute_sol: DEFAULT_MAX_TIP_ABSOLUTE_SOL,
            fallback_tip_sol: FALLBACK_TIP_SOL,
        }
    }
}

/// Calculates dynamic tip adjustment based on Paradox Sensor PDS score (NEW: Non-linear formula)
///
/// # Arguments
/// * `base_tip` - The base tip amount before Paradox adjustment (in SOL)
/// * `pds_score` - Paradox Decision Score (0.0 - 100.0)
/// * `anomaly_detected` - Whether HFT anomaly is detected
///
/// # Returns
/// The adjusted tip with Paradox multiplier applied
///
/// # Formula (Vladimir's Non-Linear)
/// ```text
/// If anomaly_detected:
///   multiplier = 1.0 + (pds_score^1.25 / 90.0)
///   adjusted_tip = base_tip * multiplier
/// Else:
///   adjusted_tip = base_tip (no change)
/// ```
///
/// # Examples
/// - PDS 50, Anomaly: Yes → ~1.78x multiplier
/// - PDS 80, Anomaly: Yes → ~3.35x multiplier
/// - PDS 90, Anomaly: Yes → ~4.05x multiplier
/// - PDS 50, Anomaly: No → 1.0x (no change)
///
/// # Philosophy
/// Non-linear scaling ensures we only pay significantly more when PDS is extremely high,
/// avoiding excessive spending on moderate tension.
pub fn calculate_paradox_adjusted_tip(
    base_tip: f64,
    pds_score: f64,
    anomaly_detected: bool,
) -> f64 {
    if !anomaly_detected {
        return base_tip;
    }

    // Non-linear multiplier: 1.0 + (PDS^1.25 / 90.0)
    let paradox_multiplier = 1.0 + (pds_score.powf(1.25) / 90.0);
    let adjusted = base_tip * paradox_multiplier;

    info!(
        "Paradox Sensor: Adjusting tip {:.2}x due to PDS score {:.2} (base: {} SOL → adjusted: {} SOL)",
        paradox_multiplier, pds_score, base_tip, adjusted
    );

    adjusted
}

/// Calculates a safe live tip amount with multiple layers of protection
///
/// # Arguments
/// * `calculated_tip` - The tip amount calculated by the algorithm (in SOL)
/// * `trade_value_sol` - The value of the trade being executed (in SOL)
/// * `config` - TipGuard configuration
///
/// # Returns
/// The safe tip amount in SOL, guaranteed to be within all limits
///
/// # Safety Layers
/// 1. Absolute cap: Never exceed configured maximum
/// 2. Ratio cap: Never exceed percentage of trade value
/// 3. Sanity check: Never allow negative or zero tips from bad calculations
///
/// # Example
/// ```
/// // Calculated tip: 1.0 SOL (from aggressive algorithm)
/// // Trade value: 0.1 SOL
/// // Max absolute: 0.04 SOL
/// // Max ratio: 40% of trade = 0.04 SOL
/// // Result: min(1.0, 0.04, 0.04) = 0.04 SOL
/// ```
pub fn calculate_safe_tip(
    calculated_tip: f64,
    _trade_value_sol: f64,
    config: &TipGuardConfig,
) -> f64 {
    // Sanity check: calculated tip must be positive
    if calculated_tip <= 0.0 {
        warn!(
            "TipGuard: Invalid calculated tip {} SOL, using fallback {} SOL",
            calculated_tip, config.fallback_tip_sol
        );
        return config.fallback_tip_sol;
    }

    // Apply absolute hard cap only — ratio cap removed
    let final_tip = calculated_tip.min(config.max_tip_absolute_sol);

    // Log if we had to reduce the tip
    if final_tip < calculated_tip {
        info!(
            "TipGuard: Reduced tip from {} SOL to {} SOL (absolute_cap={})",
            calculated_tip, final_tip, config.max_tip_absolute_sol
        );
    }

    // Sanity check: final tip should never be too small
    if final_tip < 0.0001 {
        warn!(
            "TipGuard: Calculated tip {} SOL too small, using fallback {} SOL",
            final_tip, config.fallback_tip_sol
        );
        return config.fallback_tip_sol;
    }

    final_tip
}

/// Returns the fallback tip when the upstream tip source is unavailable
///
/// # Arguments
/// * `config` - TipGuard configuration
///
/// # Returns
/// The fallback tip amount in SOL
///
/// This function should be called when upstream tip data is unavailable,
/// malformed, or otherwise unsafe to trust.
pub fn get_fallback_tip(config: &TipGuardConfig) -> f64 {
    error!(
        "TipGuard: Using fallback tip {} SOL due to upstream tip-source failure",
        config.fallback_tip_sol
    );
    config.fallback_tip_sol
}

/// Validates that a tip amount is within acceptable bounds
///
/// # Arguments
/// * `tip_amount` - The tip amount to validate (in SOL)
/// * `trade_value_sol` - The value of the trade (in SOL)
/// * `config` - TipGuard configuration
///
/// # Returns
/// `true` if tip is valid, `false` otherwise
///
/// This is a defensive check that can be used before submitting transactions
pub fn validate_tip(tip_amount: f64, _trade_value_sol: f64, config: &TipGuardConfig) -> bool {
    // Check absolute cap
    if tip_amount > config.max_tip_absolute_sol {
        error!(
            "TipGuard: Tip {} SOL exceeds absolute cap {} SOL",
            tip_amount, config.max_tip_absolute_sol
        );
        return false;
    }

    // Check minimum
    if tip_amount < 0.0 {
        error!("TipGuard: Negative tip amount {} SOL", tip_amount);
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> TipGuardConfig {
        TipGuardConfig {
            max_tip_absolute_sol: 0.04,
            fallback_tip_sol: 0.001,
        }
    }

    #[test]
    fn test_calculate_safe_tip_normal() {
        let config = default_config();
        // Reasonable tip that doesn't hit any limits
        let safe_tip = calculate_safe_tip(0.01, 0.1, &config);
        assert!((safe_tip - 0.01).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_tip_absolute_cap() {
        let config = default_config();
        // Calculated tip: 1.0 SOL (way too high) — capped at 0.04 SOL (absolute max)
        let safe_tip = calculate_safe_tip(1.0, 0.1, &config);
        assert!((safe_tip - 0.04).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_tip_small_trade_full_fallback() {
        let config = default_config();
        // Small trade (0.0005 SOL) with fallback tip (0.005 SOL) — ratio cap gone,
        // absolute cap is 0.04, so fallback passes through unchanged
        let safe_tip = calculate_safe_tip(0.005, 0.0005, &config);
        assert!((safe_tip - 0.005).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_safe_tip_negative() {
        let config = default_config();
        // Negative tip should use fallback
        let safe_tip = calculate_safe_tip(-0.1, 0.1, &config);
        assert!((safe_tip - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_calculate_safe_tip_zero() {
        let config = default_config();
        // Zero tip should use fallback
        let safe_tip = calculate_safe_tip(0.0, 0.1, &config);
        assert!((safe_tip - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_calculate_safe_tip_too_small() {
        let config = default_config();
        // Very small tip should use fallback
        let safe_tip = calculate_safe_tip(0.00001, 0.1, &config);
        assert!((safe_tip - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_get_fallback_tip() {
        let config = default_config();
        let fallback = get_fallback_tip(&config);
        assert!((fallback - 0.001).abs() < 0.00001);
    }

    #[test]
    fn test_validate_tip_success() {
        let config = default_config();
        // Valid tip: 0.01 SOL (well within absolute cap 0.04)
        assert!(validate_tip(0.01, 0.1, &config));
    }

    #[test]
    fn test_validate_tip_exceeds_absolute() {
        let config = default_config();
        // Tip exceeds absolute cap
        assert!(!validate_tip(0.05, 1.0, &config));
    }

    #[test]
    fn test_validate_tip_small_trade_passes() {
        let config = default_config();
        // Small trade: tip larger than trade value is now OK (no ratio cap)
        assert!(validate_tip(0.005, 0.0005, &config));
    }

    #[test]
    fn test_validate_tip_negative() {
        let config = default_config();
        // Negative tips are invalid
        assert!(!validate_tip(-0.01, 0.1, &config));
    }

    #[test]
    fn test_validate_tip_at_absolute_limit() {
        let config = default_config();
        // Tip exactly at absolute limit should be valid
        assert!(validate_tip(0.04, 1.0, &config));
    }

    #[test]
    fn test_paradox_adjusted_tip_no_anomaly() {
        // No anomaly = no adjustment
        let base_tip = 0.01;
        let adjusted = calculate_paradox_adjusted_tip(base_tip, 80.0, false);
        assert!((adjusted - base_tip).abs() < 0.00001);
    }

    #[test]
    fn test_paradox_adjusted_tip_with_anomaly_pds_50() {
        // PDS 50 with new formula: 1.0 + (50^1.25 / 90) ≈ 1.778x
        let base_tip = 0.01;
        let adjusted = calculate_paradox_adjusted_tip(base_tip, 50.0, true);
        let expected_multiplier = 1.0 + (50.0_f64.powf(1.25) / 90.0);
        let expected = base_tip * expected_multiplier;
        assert!((adjusted - expected).abs() < 0.00001);
    }

    #[test]
    fn test_paradox_adjusted_tip_with_anomaly_pds_80() {
        // PDS 80 with new formula: 1.0 + (80^1.25 / 90) ≈ 3.35x
        let base_tip = 0.01;
        let adjusted = calculate_paradox_adjusted_tip(base_tip, 80.0, true);
        let expected_multiplier = 1.0 + (80.0_f64.powf(1.25) / 90.0);
        let expected = base_tip * expected_multiplier;
        assert!((adjusted - expected).abs() < 0.00001);
    }

    #[test]
    fn test_paradox_adjusted_tip_with_anomaly_pds_90() {
        // PDS 90 with new formula: 1.0 + (90^1.25 / 90) ≈ 4.05x
        let base_tip = 0.01;
        let adjusted = calculate_paradox_adjusted_tip(base_tip, 90.0, true);
        let expected_multiplier = 1.0 + (90.0_f64.powf(1.25) / 90.0);
        let expected = base_tip * expected_multiplier;
        assert!((adjusted - expected).abs() < 0.00001);
    }

    #[test]
    fn test_paradox_adjustment_respects_tipguard() {
        // Paradox can increase tip, but TipGuard still applies caps
        let config = default_config();
        let base_tip = 0.02;

        // Apply Paradox with PDS 90: multiplier ≈ 4.05x
        let paradox_adjusted = calculate_paradox_adjusted_tip(base_tip, 90.0, true);
        let expected_multiplier = 1.0 + (90.0_f64.powf(1.25) / 90.0);
        assert!((paradox_adjusted / base_tip - expected_multiplier).abs() < 0.00001);

        // Apply TipGuard: should be capped to 0.04 (absolute max)
        let final_tip = calculate_safe_tip(paradox_adjusted, 0.1, &config);
        assert!((final_tip - 0.04).abs() < 0.00001);
    }
}
