//! Signal Wave Builders for HyperPrediction Oracle
//!
//! This module contains functions that build `HeuristicWave` structures from
//! various input data. These waves are used in QASS (Quantum Amplitude Scoring
//! System) superposition analysis.
//!
//! ## Wave Types
//!
//! - `ψ_liquidity`: Wave from initial liquidity amount
//! - `ψ_dev_buy`: Wave from developer buy-in data
//!
//! ## Design Philosophy
//!
//! Wave builders are pure functions that transform input data into standardized
//! `HeuristicWave` structures. Each wave has:
//! - **Amplitude**: Signal strength (0.0 to 1.0)
//! - **Phase**: Signal direction (-1.0 to 1.0, positive = bullish)
//! - **Confidence**: Reliability of the signal (0.0 to 1.0)
//!
//! ## Normalization
//!
//! All builder functions support dynamic normalization via a `scale` parameter.
//! This allows calibration of thresholds based on market conditions or config.
//! Backward-compatible wrapper functions use default scales.

use crate::oracle::ultrafast::HeuristicWave;

// =============================================================================
// Liquidity Wave Builders
// =============================================================================

/// Build ψ_liquidity wave from initial liquidity with configurable scale
///
/// The liquidity wave represents the strength of the token's initial liquidity.
/// Higher liquidity generally indicates lower slippage risk and better tradability.
///
/// # Arguments
/// * `liquidity_sol` - Liquidity amount in SOL
/// * `scale` - Normalization scale for dynamic calibration (e.g., 20.0 = 20 SOL is "excellent")
///
/// # Returns
/// A `HeuristicWave` with amplitude and phase based on normalized liquidity
///
/// # Wave Characteristics
/// | Normalized | Amplitude | Phase | Interpretation |
/// |------------|-----------|-------|----------------|
/// | >= 1.0     | 0.95      | 0.8   | Excellent - very bullish |
/// | >= 0.5     | 0.8       | 0.6   | Good - bullish |
/// | >= 0.25    | 0.6       | 0.3   | Decent - slightly bullish |
/// | >= 0.15    | 0.4       | 0.0   | Low - neutral |
/// | >= 0.05    | 0.3       | -0.3  | Very low - slightly bearish |
/// | < 0.05     | 0.1       | -0.7  | Critical - bearish |
///
/// # Example
///
/// ```ignore
/// // 25 SOL liquidity with 20 SOL scale = normalized 1.25 (excellent)
/// let wave = build_liquidity_wave_scaled(25.0, 20.0);
/// assert_eq!(wave.amplitude, 0.95);
/// assert_eq!(wave.phase, 0.8);
/// ```
pub fn build_liquidity_wave_scaled(liquidity_sol: f64, scale: f64) -> HeuristicWave {
    // Calculate normalized liquidity value using the scale
    let normalized = liquidity_sol / scale;

    let (amplitude, phase) = if normalized >= 1.0 {
        (0.95, 0.8) // Excellent liquidity - very bullish
    } else if normalized >= 0.5 {
        (0.8, 0.6) // Good liquidity - bullish
    } else if normalized >= 0.25 {
        (0.6, 0.3) // Decent liquidity - slightly bullish
    } else if normalized >= 0.15 {
        (0.4, 0.0) // Low liquidity - neutral
    } else if normalized >= 0.05 {
        (0.3, -0.3) // Very low - slightly bearish
    } else {
        (0.1, -0.7) // Critically low - bearish
    };

    HeuristicWave::new("ψ_liquidity", amplitude, phase, 0.85)
}

/// Build ψ_liquidity wave from initial liquidity (backward compatible)
///
/// Uses default scale of 20.0 SOL for "excellent" liquidity threshold.
/// This function maintains API compatibility for callers that don't need
/// custom scaling. Most internal usage now goes through the `_scaled` variant.
///
/// # Arguments
/// * `liquidity_sol` - Liquidity amount in SOL
///
/// # Returns
/// A `HeuristicWave` representing liquidity strength
#[allow(dead_code)] // Maintained for API compatibility
pub fn build_liquidity_wave(liquidity_sol: f64) -> HeuristicWave {
    build_liquidity_wave_scaled(liquidity_sol, 20.0) // Default scale
}

// =============================================================================
// Developer Buy Wave Builders
// =============================================================================

/// Build ψ_dev_buy wave from atomic dev buy data with configurable scale
///
/// The dev buy wave represents the developer's financial commitment to the token.
/// A developer who buys into their own token demonstrates confidence and alignment
/// with other holders.
///
/// # Arguments
/// * `has_dev_buy` - Whether developer made an atomic buy
/// * `dev_buy_sol` - Amount of SOL in developer buy
/// * `scale` - Normalization scale for dynamic calibration (e.g., 5.0 = 5 SOL is "large")
///
/// # Returns
/// A `HeuristicWave` with amplitude and phase based on dev buy presence and size
///
/// # Wave Characteristics
///
/// ## No Dev Buy
/// - Amplitude: 0.3 (weak signal)
/// - Phase: -0.2 (slightly bearish)
/// - Confidence: 0.7
///
/// ## With Dev Buy (normalized by scale)
/// | Normalized | Amplitude | Phase | Interpretation |
/// |------------|-----------|-------|----------------|
/// | >= 1.0     | 0.95      | 0.85  | Large buy - very bullish |
/// | >= 0.4     | 0.8       | 0.7   | Medium buy - bullish |
/// | >= 0.1     | 0.6       | 0.5   | Small buy - moderately bullish |
/// | < 0.1      | 0.4       | 0.3   | Tiny buy - slightly bullish |
///
/// # Example
///
/// ```ignore
/// // 6 SOL dev buy with 5 SOL scale = normalized 1.2 (large)
/// let wave = build_dev_buy_wave_scaled(true, 6.0, 5.0);
/// assert_eq!(wave.amplitude, 0.95);
/// assert_eq!(wave.phase, 0.85);
///
/// // No dev buy
/// let wave = build_dev_buy_wave_scaled(false, 0.0, 5.0);
/// assert_eq!(wave.amplitude, 0.3);
/// assert_eq!(wave.phase, -0.2);
/// ```
pub fn build_dev_buy_wave_scaled(has_dev_buy: bool, dev_buy_sol: f64, scale: f64) -> HeuristicWave {
    if !has_dev_buy {
        return HeuristicWave::new("ψ_dev_buy", 0.3, -0.2, 0.7);
    }

    // Calculate normalized dev buy value
    let normalized = dev_buy_sol / scale;

    let (amplitude, phase) = if normalized >= 1.0 {
        (0.95, 0.85) // Large dev buy - very bullish
    } else if normalized >= 0.4 {
        (0.8, 0.7) // Medium dev buy - bullish
    } else if normalized >= 0.1 {
        (0.6, 0.5) // Small dev buy - moderately bullish
    } else {
        (0.4, 0.3) // Tiny dev buy - slightly bullish
    };

    HeuristicWave::new("ψ_dev_buy", amplitude, phase, 0.9)
}

/// Build ψ_dev_buy wave from atomic dev buy data (backward compatible)
///
/// Uses default scale of 5.0 SOL for "large" dev buy threshold.
/// This function maintains API compatibility for callers that don't need
/// custom scaling. Most internal usage now goes through the `_scaled` variant.
///
/// # Arguments
/// * `has_dev_buy` - Whether developer made an atomic buy
/// * `dev_buy_sol` - Amount of SOL in developer buy
///
/// # Returns
/// A `HeuristicWave` representing dev commitment strength
#[allow(dead_code)] // Maintained for API compatibility
pub fn build_dev_buy_wave(has_dev_buy: bool, dev_buy_sol: f64) -> HeuristicWave {
    build_dev_buy_wave_scaled(has_dev_buy, dev_buy_sol, 5.0) // Default scale
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Liquidity Wave Tests
    // =========================================================================

    #[test]
    fn test_liquidity_wave_excellent() {
        let wave = build_liquidity_wave_scaled(25.0, 20.0);
        assert_eq!(wave.amplitude, 0.95);
        assert_eq!(wave.phase, 0.8);
        assert_eq!(wave.confidence, 0.85);
        assert_eq!(wave.name, "ψ_liquidity");
    }

    #[test]
    fn test_liquidity_wave_good() {
        let wave = build_liquidity_wave_scaled(12.0, 20.0); // normalized 0.6
        assert_eq!(wave.amplitude, 0.8);
        assert_eq!(wave.phase, 0.6);
    }

    #[test]
    fn test_liquidity_wave_decent() {
        let wave = build_liquidity_wave_scaled(6.0, 20.0); // normalized 0.3
        assert_eq!(wave.amplitude, 0.6);
        assert_eq!(wave.phase, 0.3);
    }

    #[test]
    fn test_liquidity_wave_low() {
        let wave = build_liquidity_wave_scaled(3.5, 20.0); // normalized 0.175
        assert_eq!(wave.amplitude, 0.4);
        assert_eq!(wave.phase, 0.0);
    }

    #[test]
    fn test_liquidity_wave_very_low() {
        let wave = build_liquidity_wave_scaled(1.5, 20.0); // normalized 0.075
        assert_eq!(wave.amplitude, 0.3);
        assert_eq!(wave.phase, -0.3);
    }

    #[test]
    fn test_liquidity_wave_critical() {
        let wave = build_liquidity_wave_scaled(0.5, 20.0); // normalized 0.025
        assert_eq!(wave.amplitude, 0.1);
        assert_eq!(wave.phase, -0.7);
    }

    #[test]
    fn test_liquidity_wave_default_scale() {
        let wave = build_liquidity_wave(20.0); // normalized 1.0 with default scale
        assert_eq!(wave.amplitude, 0.95);
        assert_eq!(wave.phase, 0.8);
    }

    // =========================================================================
    // Dev Buy Wave Tests
    // =========================================================================

    #[test]
    fn test_dev_buy_wave_no_buy() {
        let wave = build_dev_buy_wave_scaled(false, 0.0, 5.0);
        assert_eq!(wave.amplitude, 0.3);
        assert_eq!(wave.phase, -0.2);
        assert_eq!(wave.confidence, 0.7);
        assert_eq!(wave.name, "ψ_dev_buy");
    }

    #[test]
    fn test_dev_buy_wave_large() {
        let wave = build_dev_buy_wave_scaled(true, 6.0, 5.0); // normalized 1.2
        assert_eq!(wave.amplitude, 0.95);
        assert_eq!(wave.phase, 0.85);
        assert_eq!(wave.confidence, 0.9);
    }

    #[test]
    fn test_dev_buy_wave_medium() {
        let wave = build_dev_buy_wave_scaled(true, 2.5, 5.0); // normalized 0.5
        assert_eq!(wave.amplitude, 0.8);
        assert_eq!(wave.phase, 0.7);
    }

    #[test]
    fn test_dev_buy_wave_small() {
        let wave = build_dev_buy_wave_scaled(true, 0.8, 5.0); // normalized 0.16
        assert_eq!(wave.amplitude, 0.6);
        assert_eq!(wave.phase, 0.5);
    }

    #[test]
    fn test_dev_buy_wave_tiny() {
        let wave = build_dev_buy_wave_scaled(true, 0.3, 5.0); // normalized 0.06
        assert_eq!(wave.amplitude, 0.4);
        assert_eq!(wave.phase, 0.3);
    }

    #[test]
    fn test_dev_buy_wave_default_scale() {
        let wave = build_dev_buy_wave(true, 5.0); // normalized 1.0 with default scale
        assert_eq!(wave.amplitude, 0.95);
        assert_eq!(wave.phase, 0.85);
    }

    #[test]
    fn test_dev_buy_wave_ignores_amount_when_no_buy() {
        // Even with large amount, should return no-buy wave if has_dev_buy is false
        let wave = build_dev_buy_wave_scaled(false, 100.0, 5.0);
        assert_eq!(wave.amplitude, 0.3);
        assert_eq!(wave.phase, -0.2);
    }
}
