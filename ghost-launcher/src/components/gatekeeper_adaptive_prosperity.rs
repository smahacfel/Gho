//! Gatekeeper V2.5 Adaptive Prosperity (APS) — shadow/offline
//!
//! Market regime detection and shadow threshold suggestions for the
//! prosperity filter. APS does NOT mutate live thresholds — it only
//! produces diagnostic suggestions for offline calibration and ablation.
//!
//! Shadow-first: `adaptive_enabled = false` by default.

use ghost_brain::config::gatekeeper_v25_config::AdaptiveProsperityConfig;

use crate::components::gatekeeper::GatekeeperAssessment;

/// Market regime classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketRegime {
    Normal,
    HighVolatility,
    LowVolatility,
}

impl MarketRegime {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::HighVolatility => "HighVolatility",
            Self::LowVolatility => "LowVolatility",
        }
    }
}

impl Default for MarketRegime {
    fn default() -> Self {
        Self::Normal
    }
}

/// Full APS diagnostics produced by `evaluate_aps()`.
#[derive(Debug, Clone)]
pub struct ApsDiagnostics {
    pub enabled: bool,
    pub adaptive_enabled: bool,
    /// Whether adaptive thresholds were applied to the live prosperity evaluation
    pub adaptive_thresholds_applied: bool,
    /// Detected market regime (Normal if insufficient calibration samples)
    pub regime: MarketRegime,
    // Shadow-suggested thresholds per regime
    pub shadow_entry_drift_max_pct: f64,
    pub shadow_confidence_min: f64,
    pub shadow_prosperity_mcap_sol: f64,
    pub shadow_branch1_sniped_pct: f64,
    pub shadow_branch3_hhi_max: f64,
    /// Contrafactual: would the prosperity filter have passed with APS thresholds?
    pub shadow_prosperity_would_pass: Option<bool>,
}

impl ApsDiagnostics {
    pub fn not_run() -> Self {
        Self {
            enabled: false,
            adaptive_enabled: false,
            adaptive_thresholds_applied: false,
            regime: MarketRegime::Normal,
            shadow_entry_drift_max_pct: 5.0,
            shadow_confidence_min: 0.60,
            shadow_prosperity_mcap_sol: 35.0,
            shadow_branch1_sniped_pct: 0.28,
            shadow_branch3_hhi_max: 0.0416,
            shadow_prosperity_would_pass: None,
        }
    }
}

/// Evaluate APS: detect market regime and produce shadow threshold suggestions
/// plus a contrafactual prosperity verdict ("would the filter have passed with APS thresholds?").
///
/// Does NOT mutate live prosperity thresholds — `adaptive_thresholds_applied` is
/// always false unless `config.adaptive_enabled` (requires ADR promotion).
///
/// **Calibration guard:** Falls back to `Normal` regime when the system has
/// insufficient historical samples to support regime detection (tracked by
/// `min_calibration_samples` and `regime_lookback_hours`).
pub fn evaluate_aps(
    assessment: &GatekeeperAssessment,
    config: &AdaptiveProsperityConfig,
    pdd_spike_detected: bool,
) -> ApsDiagnostics {
    if !config.enabled || !config.shadow_suggestions_enabled {
        return ApsDiagnostics::not_run();
    }

    // ── Calibration guard ──
    // Pool-local heuristic requires >= min_calibration_samples TX to produce
    // a meaningful regime signal. Below that, default to Normal.
    let has_sufficient_history =
        config.regime_local_heuristic_enabled || config.cross_pool_outcome_tracker_available;
    let sample_adequate = assessment.total_tx_evaluated as usize >= config.min_calibration_samples;
    let regime = if has_sufficient_history && sample_adequate {
        detect_regime(assessment, config, pdd_spike_detected)
    } else {
        MarketRegime::Normal
    };

    let (entry_drift, conf_min, mcap, branch1, branch3_hhi) = match regime {
        MarketRegime::Normal => (
            config.regime_normal_entry_drift_max_pct,
            config.regime_normal_confidence_min,
            35.0,   // baseline prosperity_mcap
            0.28,   // baseline branch1 sniped
            0.0416, // baseline branch3 HHI max
        ),
        MarketRegime::HighVolatility => (
            config.regime_high_vol_entry_drift_max_pct,
            config.regime_high_vol_confidence_min,
            45.0,  // +30% vs baseline
            0.34,  // +20% vs baseline
            0.033, // -20% vs baseline
        ),
        MarketRegime::LowVolatility => (
            config.regime_normal_entry_drift_max_pct,
            config.regime_normal_confidence_min,
            30.0,  // -15% vs baseline
            0.24,  // -15% vs baseline
            0.054, // +30% vs baseline
        ),
    };

    // Contrafactual: would prosperity filter have passed with APS shadow thresholds?
    let shadow_would_pass = compute_shadow_prosperity_pass(assessment, mcap, branch1, branch3_hhi);

    // adaptive_thresholds_applied: only true when adaptive_enabled is on,
    // regime is not Normal, and live_execution is still false (shadow-first).
    // This allows V2.5 shadow plane to react to regime without touching legacy.
    let thresholds_applied = config.adaptive_enabled && regime != MarketRegime::Normal;

    // Telemetry: record regime distribution (provisional).
    crate::oracle_metrics::record_aps_regime(regime.as_str());

    ApsDiagnostics {
        enabled: true,
        adaptive_enabled: config.adaptive_enabled,
        adaptive_thresholds_applied: thresholds_applied,
        regime,
        shadow_entry_drift_max_pct: entry_drift,
        shadow_confidence_min: conf_min,
        shadow_prosperity_mcap_sol: mcap,
        shadow_branch1_sniped_pct: branch1,
        shadow_branch3_hhi_max: branch3_hhi,
        shadow_prosperity_would_pass: Some(shadow_would_pass),
    }
}

/// Detect market regime from current pool assessment.
///
/// Heurystyka (v1, jednopoolowa):
/// - HHI spike: phase3 HHI > regime_high_volatility_threshold (0.6)
///   → HighVolatility signal (extreme cabal concentration)
/// - Price spike: phase6 price_change_ratio > 3.0
///   → HighVolatility signal (pump in progress)
/// - Volume spike: PDD spike_detected == true
///   → HighVolatility signal (anomalous volume pattern)
/// - Low activity: very few TX, low HHI, stable price
///   → LowVolatility signal (quiet market)
///
/// Returns `HighVolatility` if any spike detected,
/// `LowVolatility` if low activity indicators present,
/// `Normal` otherwise.
fn detect_regime(
    assessment: &GatekeeperAssessment,
    config: &AdaptiveProsperityConfig,
    pdd_spike_detected: bool,
) -> MarketRegime {
    if !config.regime_detection_enabled {
        return MarketRegime::Normal;
    }

    let mut high_vol_signals = 0u8;
    let mut low_vol_signals = 0u8;

    // HHI spike — extreme signer concentration → cabal/pump indicator
    if let Some(ref div) = assessment.phase3_diversity {
        if div.hhi > config.regime_high_volatility_threshold {
            high_vol_signals += 1;
        }
        // Low HHI + few TX → quiet, low-activity pool
        if div.hhi < 0.05 && assessment.total_tx_evaluated < 20 {
            low_vol_signals += 1;
        }
    }

    // Price spike — extreme price movement → volatile
    if let Some(ref curve) = assessment.phase6_curve {
        if curve.price_change_ratio > 3.0 {
            high_vol_signals += 1;
        }
        // Stable price near initial → low volatility
        if curve.price_change_ratio > 0.9
            && curve.price_change_ratio < 1.1
            && curve.curve_data_known
        {
            low_vol_signals += 1;
        }
    }

    // Volume spike — PDD already detected anomalous volume pattern
    if pdd_spike_detected {
        high_vol_signals += 1;
    }

    if high_vol_signals >= 1 {
        MarketRegime::HighVolatility
    } else if low_vol_signals >= 2 {
        MarketRegime::LowVolatility
    } else {
        MarketRegime::Normal
    }
}

/// Contrafactual prosperity pass check using APS shadow thresholds.
///
/// Mirrors `evaluate_prosperity_filter()` logic but with shadow (regime-adapted)
/// thresholds instead of static config values. Checks:
/// - Market cap floor (shadow mcap)
/// - CPV ceiling (static — regime doesn't change this significantly)
/// - Three balanced branches (B1/B2/B3) with shadow thresholds
/// - Simple overlay: price_change_ratio within bounds
fn compute_shadow_prosperity_pass(
    assessment: &GatekeeperAssessment,
    mcap_floor: f64,
    branch1_sniped: f64,
    branch3_hhi: f64,
) -> bool {
    let curve = match assessment.phase6_curve.as_ref() {
        Some(c) if c.curve_data_known => c,
        _ => return false,
    };

    // Market cap floor (light veto)
    if curve.current_market_cap_sol < mcap_floor {
        return false;
    }

    // CPV light veto (static — cross-pool velocity not regime-sensitive in v1)
    let cpv_ok = assessment
        .feature_snapshot
        .sybil_resistance
        .signer_cross_pool_velocity
        .map_or(true, |cpv| cpv <= 0.50);

    if !cpv_ok {
        return false;
    }

    // Simple overlay: price must not have moved too far
    let price_ok = curve.price_change_ratio <= 2.2;
    if !price_ok {
        return false;
    }

    // B1: conviction_clean_sells — shadow branch1 sniped threshold
    let b1_pass = assessment.early_fingerprint.as_ref().map_or(false, |fp| {
        fp.block0_sniped_supply_pct.unwrap_or(0.0) >= branch1_sniped
            && assessment
                .feature_snapshot
                .alpha_fingerprint
                .sell_buy_ratio
                .unwrap_or(999.0)
                <= 0.16
    });

    // B2: large_cap_buy_dominance — shadow mcap floor
    let b2_pass = assessment
        .feature_snapshot
        .alpha_fingerprint
        .early_slot_volume_dominance_buy
        .map_or(false, |dom| dom >= 0.90)
        && curve.current_market_cap_sol >= 50.0;

    // B3: organic_structure — shadow branch3 HHI max
    let b3_pass = assessment.phase3_diversity.as_ref().map_or(false, |div| {
        div.hhi <= branch3_hhi
            && assessment
                .feature_snapshot
                .sybil_resistance
                .fee_topology_diversity_index
                .unwrap_or(0.0)
                >= 0.0909
    });

    b1_pass || b2_pass || b3_pass
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::gatekeeper::{
        BondingCurveDynamics, GatekeeperAssessment, SignerDiversityProfile,
    };
    use ghost_brain::config::GatekeeperV2Config;
    use ghost_core::checkpoint::MaterializedFeatureSet;

    fn aps_test_config() -> AdaptiveProsperityConfig {
        let cfg = GatekeeperV2Config::default();
        let mut aps = cfg.aps;
        aps.enabled = true;
        aps.shadow_suggestions_enabled = true;
        aps.regime_detection_enabled = true;
        aps
    }

    fn empty_assessment() -> GatekeeperAssessment {
        GatekeeperAssessment {
            phase1_passed: false,
            phase2_velocity: None,
            phase2_passed: false,
            phase3_diversity: None,
            phase3_passed: false,
            phase4_volume: None,
            phase4_passed: false,
            phase5_dev: None,
            phase5_passed: false,
            phase6_curve: None,
            phase6_passed: false,
            phases_passed: 0,
            hard_reject_reason: None,
            total_tx_evaluated: 0,
            unique_tx_evaluated: 0,
            unique_signers_evaluated: 0,
            observation_duration_ms: 0,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 0,
            buy_count: 0,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            feature_snapshot: MaterializedFeatureSet::default(),
            checkpoint_count: 0,
            trajectory_available: false,
            v25_shadow_decisions: Vec::new(),
            trajectory: None,
            pdd_assessment: None,
            aps_diagnostics: None,
            observation_stage: None,
            entry_drift_pct: None,
            entry_drift_anchor_quality: None,
            adaptive_thresholds_applied: false,
            v25_confidence: None,
        }
    }

    #[test]
    fn test_aps_disabled_returns_not_run() {
        let mut config = aps_test_config();
        config.enabled = false;
        let assessment = empty_assessment();
        let result = evaluate_aps(&assessment, &config, false);
        assert!(!result.enabled);
        assert!(!result.adaptive_thresholds_applied);
    }

    #[test]
    fn test_regime_normal_by_default() {
        let config = aps_test_config();
        let assessment = empty_assessment();
        let result = evaluate_aps(&assessment, &config, false);
        assert_eq!(result.regime, MarketRegime::Normal);
        assert!((result.shadow_entry_drift_max_pct - 5.0).abs() < f64::EPSILON);
        assert!((result.shadow_confidence_min - 0.60).abs() < f64::EPSILON);
        assert!((result.shadow_prosperity_mcap_sol - 35.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_regime_high_vol_from_pdd_spike() {
        let config = aps_test_config();
        let assessment = empty_assessment();
        // evaluate_aps always returns Normal until calibration guard passes
        let result = evaluate_aps(&assessment, &config, true);
        // With insufficient history, returns Normal (calibration guard)
        assert_eq!(result.regime, MarketRegime::Normal);
        assert!((result.shadow_entry_drift_max_pct - 5.0).abs() < f64::EPSILON);
        assert!((result.shadow_confidence_min - 0.60).abs() < f64::EPSILON);
    }

    #[test]
    fn test_detect_regime_hhi_spike_heuristic() {
        // Test the pool-local heuristic directly (not gated by calibration guard)
        let config = aps_test_config();
        let mut assessment = empty_assessment();
        assessment.phase3_diversity = Some(SignerDiversityProfile {
            unique_ratio: 0.3,
            hhi: 0.75,
            max_tx_per_signer: 10,
            volume_gini: 0.4,
            top3_volume_pct: 0.5,
            same_ms_tx_ratio: 0.1,
        });
        assessment.total_tx_evaluated = 30;
        let regime = detect_regime(&assessment, &config, false);
        assert_eq!(regime, MarketRegime::HighVolatility);
    }

    #[test]
    fn test_detect_regime_price_spike_heuristic() {
        // Test the pool-local heuristic directly (not gated by calibration guard)
        let config = aps_test_config();
        let mut assessment = empty_assessment();
        assessment.phase6_curve = Some(BondingCurveDynamics {
            initial_price: 0.001,
            current_price: 0.005,
            max_price: 0.006,
            price_change_ratio: 5.0,
            max_single_tx_price_impact_pct: 10.0,
            max_single_sell_impact_pct: 5.0,
            current_market_cap_sol: 80.0,
            market_cap_change_ratio: 2.0,
            bonding_progress_pct: 15.0,
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            price_data_points: 5,
        });
        let regime = detect_regime(&assessment, &config, false);
        assert_eq!(regime, MarketRegime::HighVolatility);
    }

    #[test]
    fn test_market_regime_as_str() {
        assert_eq!(MarketRegime::Normal.as_str(), "Normal");
        assert_eq!(MarketRegime::HighVolatility.as_str(), "HighVolatility");
        assert_eq!(MarketRegime::LowVolatility.as_str(), "LowVolatility");
    }

    #[test]
    fn test_aps_five_thresholds_all_populated() {
        let config = aps_test_config();
        let assessment = empty_assessment();
        let result = evaluate_aps(&assessment, &config, false);
        assert!(result.shadow_entry_drift_max_pct > 0.0);
        assert!(result.shadow_confidence_min > 0.0);
        assert!(result.shadow_prosperity_mcap_sol > 0.0);
        assert!(result.shadow_branch1_sniped_pct > 0.0);
        assert!(result.shadow_branch3_hhi_max > 0.0);
    }
}
