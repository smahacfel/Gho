//! Gatekeeper V2.5 "PRECISION STRIKE" — konfiguracja czterech modułów
//!
//! Każdy struct ma `#[serde(default)]` dla wstecznej kompatybilności.
//! Rust `Default` impl ustawia `enabled = false` — TOML jawnie aktywuje moduły.

use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════════════════════════════
// Rollout guardrails
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatekeeperV25RolloutConfig {
    /// Master enable for V2.5 shadow pipeline. When false, no V2.5 code paths activate.
    pub shadow_enabled: bool,
    /// Allow V2.5 verdicts to be promoted to live execution. Requires ADR sign-off.
    pub live_execution_enabled: bool,
    /// Require a formal ADR before any V2.5 verdict can be promoted from shadow to live.
    pub require_promotion_adr: bool,
    /// Emit shadow decision JSONL entries (v16 schema).
    pub emit_shadow_decisions: bool,
    /// Emit ablation metadata fields in decision JSONL for offline analysis.
    pub emit_ablation_fields: bool,
}

impl Default for GatekeeperV25RolloutConfig {
    fn default() -> Self {
        Self {
            shadow_enabled: false,
            live_execution_enabled: false,
            require_promotion_adr: true,
            emit_shadow_decisions: false,
            emit_ablation_fields: false,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Dynamic Observation Window (dow)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DynamicObservationWindowConfig {
    pub enabled: bool,

    // Early entry (shadow-only, 2-5s)
    pub early_entry_enabled: bool,
    pub early_entry_min_ms: u64,
    pub early_entry_max_ms: u64,
    pub early_entry_min_confidence: f64,
    pub early_entry_min_tx_count: usize,
    pub early_entry_min_phases_passed: u8,
    pub early_entry_min_momentum: f64,
    pub early_entry_max_sybil_points: u8,
    pub early_entry_max_entry_drift_pct: f64,

    // Normal window (5-7s)
    pub normal_window_ms: u64,
    pub normal_window_min_confidence: f64,

    // Extended window (7-10s)
    pub extended_window_ms: u64,
    pub extended_window_min_confidence: f64,
    pub extended_require_pdd_clean: bool,

    // DOW timer
    /// Per-pool DOW timer tick interval in milliseconds.
    pub tick_interval_ms: u64,
}

impl Default for DynamicObservationWindowConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            early_entry_enabled: false,
            early_entry_min_ms: 2000,
            early_entry_max_ms: 5000,
            early_entry_min_confidence: 0.85,
            early_entry_min_tx_count: 15,
            early_entry_min_phases_passed: 6,
            early_entry_min_momentum: 0.40,
            early_entry_max_sybil_points: 1,
            early_entry_max_entry_drift_pct: 3.0,
            normal_window_ms: 7000,
            normal_window_min_confidence: 0.65,
            extended_window_ms: 10000,
            extended_window_min_confidence: 0.55,
            extended_require_pdd_clean: true,
            tick_interval_ms: 250,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Trajectory Aware Scoring (tas)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrajectoryAwareScoringConfig {
    pub enabled: bool,

    /// Shadow hard reject only at extremely negative trajectory.
    pub tas_hard_reject_threshold: f64,
    /// Confidence modulation range: [min, max]
    pub tas_confidence_modulator_min: f64,
    pub tas_confidence_modulator_max: f64,
    /// Minimum TX per segment for TAS to be computed.
    pub tas_min_tx_per_segment: usize,
    /// Minimum total observation duration for trajectory to be valid.
    pub tas_min_total_duration_ms: u64,

    // Dimension weights
    pub momentum_trajectory_weight: f64,
    pub momentum_accel_min_ratio: f64,
    pub momentum_decel_max_ratio: f64,
    pub hhi_trajectory_weight: f64,
    pub hhi_decline_min_ratio: f64,
    pub volume_trajectory_weight: f64,
    pub volume_cv_max: f64,
    pub interval_trajectory_weight: f64,
    pub interval_shortening_min_ratio: f64,
    pub buy_ratio_trajectory_weight: f64,
    pub buy_ratio_stability_min: f64,
}

impl Default for TrajectoryAwareScoringConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tas_hard_reject_threshold: 0.30,
            tas_confidence_modulator_min: 0.75,
            tas_confidence_modulator_max: 1.25,
            tas_min_tx_per_segment: 3,
            tas_min_total_duration_ms: 3000,
            momentum_trajectory_weight: 0.25,
            momentum_accel_min_ratio: 1.15,
            momentum_decel_max_ratio: 0.85,
            hhi_trajectory_weight: 0.20,
            hhi_decline_min_ratio: 0.85,
            volume_trajectory_weight: 0.20,
            volume_cv_max: 0.60,
            interval_trajectory_weight: 0.15,
            interval_shortening_min_ratio: 0.80,
            buy_ratio_trajectory_weight: 0.20,
            buy_ratio_stability_min: 0.55,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Pump & Dump Detector (pdd)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PumpAndDumpDetectorConfig {
    pub enabled: bool,

    // ── Per-threshold live promotion gates (shadow-first → live after ADR) ──
    /// Promote entry drift hard veto from shadow to live execution
    pub entry_drift_promoted_to_live: bool,
    /// Promote spike hard veto from shadow to live execution
    pub spike_promoted_to_live: bool,
    /// Promote ramping hard veto from shadow to live execution
    pub ramping_promoted_to_live: bool,
    /// Promote whale concentration hard veto from shadow to live execution
    pub whale_promoted_to_live: bool,
    /// Promote reserve health hard veto from shadow to live execution
    pub reserve_promoted_to_live: bool,
    /// Promote flash crash hard veto from shadow to live execution
    pub flash_crash_promoted_to_live: bool,

    // Entry drift — CORE PROTECTION
    pub entry_drift_max_pct: f64,
    pub entry_drift_soft_max_pct: f64,
    pub entry_drift_soft_weight: u8,

    // Spike pattern detection
    pub spike_detection_enabled: bool,
    pub spike_observation_window_ms: u64,
    pub spike_ratio_threshold: f64,
    pub spike_hard_veto: bool,
    pub spike_soft_penalty: u8,

    // Reserve health
    pub reserve_min_sol: f64,
    pub reserve_min_ratio: f64,

    // Whale concentration
    pub whale_top3_max_pct: f64,
    pub whale_top3_size_max_sol: f64,
    pub whale_single_max_pct: f64,

    // Ramping detection
    pub ramping_detection_enabled: bool,
    pub ramping_min_consecutive_buys: usize,
    pub ramping_size_tolerance_pct: f64,
    pub ramping_hard_veto: bool,

    // Flash crash protection
    pub flash_crash_protection_enabled: bool,
    pub flash_crash_max_price_impact_pct: f64,
    pub flash_crash_sell_cluster_max_ms: u64,
}

impl Default for PumpAndDumpDetectorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // Per-threshold promotion — all false by default (shadow-only)
            entry_drift_promoted_to_live: false,
            spike_promoted_to_live: false,
            ramping_promoted_to_live: false,
            whale_promoted_to_live: false,
            reserve_promoted_to_live: false,
            flash_crash_promoted_to_live: false,
            entry_drift_max_pct: 5.0,
            entry_drift_soft_max_pct: 3.0,
            entry_drift_soft_weight: 2,
            spike_detection_enabled: true,
            spike_observation_window_ms: 3000,
            spike_ratio_threshold: 2.0,
            spike_hard_veto: true,
            spike_soft_penalty: 5, // -0.25 score penalty (plan: 5 * 0.05 = 0.25)
            reserve_min_sol: 30.0,
            reserve_min_ratio: 0.15,
            whale_top3_max_pct: 60.0,
            whale_top3_size_max_sol: 15.0,
            whale_single_max_pct: 35.0,
            ramping_detection_enabled: true,
            ramping_min_consecutive_buys: 4,
            ramping_size_tolerance_pct: 15.0,
            ramping_hard_veto: true,
            flash_crash_protection_enabled: true,
            flash_crash_max_price_impact_pct: 15.0,
            flash_crash_sell_cluster_max_ms: 500,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Adaptive Prosperity (aps)
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdaptiveProsperityConfig {
    pub enabled: bool,

    /// Whether adaptive thresholds are applied live (requires promotion).
    pub adaptive_enabled: bool,
    /// Emit shadow threshold suggestions in telemetry.
    pub shadow_suggestions_enabled: bool,

    pub adaptation_interval_buys: usize,
    pub calibration_lookback_buys: usize,
    pub min_calibration_samples: usize,

    pub branch_weight_adaptation: bool,
    pub branch_weight_learning_rate: f64,
    pub branch_min_weight: f64,

    pub regime_detection_enabled: bool,
    pub regime_lookback_hours: u64,
    pub regime_high_volatility_threshold: f64,

    pub regime_high_vol_entry_drift_max_pct: f64,
    pub regime_high_vol_confidence_min: f64,
    pub regime_normal_entry_drift_max_pct: f64,
    pub regime_normal_confidence_min: f64,

    /// Enable pool-local regime heuristic (no cross-pool outcome tracker required).
    /// When false, regime always defaults to Normal. Default: false.
    /// Turn on in shadow-burnin rollout to get provisional regime labels.
    #[serde(default)]
    pub regime_local_heuristic_enabled: bool,
    /// Future flag: set to true once cross-pool outcome tracker is operational.
    #[serde(default)]
    pub cross_pool_outcome_tracker_available: bool,
}

impl Default for AdaptiveProsperityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            adaptive_enabled: false,
            shadow_suggestions_enabled: true,
            adaptation_interval_buys: 50,
            calibration_lookback_buys: 100,
            min_calibration_samples: 30,
            branch_weight_adaptation: true,
            branch_weight_learning_rate: 0.05,
            branch_min_weight: 0.15,
            regime_detection_enabled: true,
            regime_lookback_hours: 4,
            regime_high_volatility_threshold: 0.6,
            regime_high_vol_entry_drift_max_pct: 3.0,
            regime_high_vol_confidence_min: 0.70,
            regime_normal_entry_drift_max_pct: 5.0,
            regime_normal_confidence_min: 0.60,
            regime_local_heuristic_enabled: false,
            cross_pool_outcome_tracker_available: false,
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Shared enums
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryDriftAnchorQuality {
    Strong,
    Weak,
}

impl Default for EntryDriftAnchorQuality {
    fn default() -> Self {
        Self::Strong
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_all_v25_structs() {
        let v25 = GatekeeperV25RolloutConfig::default();
        assert!(!v25.shadow_enabled);
        assert!(!v25.live_execution_enabled);
        assert!(v25.require_promotion_adr);

        let dow = DynamicObservationWindowConfig::default();
        assert!(!dow.enabled);
        assert_eq!(dow.early_entry_min_ms, 2000);
        assert_eq!(dow.normal_window_ms, 7000);
        assert_eq!(dow.extended_window_ms, 10000);

        let tas = TrajectoryAwareScoringConfig::default();
        assert!(!tas.enabled);
        assert!((tas.tas_hard_reject_threshold - 0.30).abs() < f64::EPSILON);
        assert!((tas.tas_confidence_modulator_min - 0.75).abs() < f64::EPSILON);
        assert!((tas.tas_confidence_modulator_max - 1.25).abs() < f64::EPSILON);

        let pdd = PumpAndDumpDetectorConfig::default();
        assert!(!pdd.enabled);
        assert!((pdd.entry_drift_max_pct - 5.0).abs() < f64::EPSILON);
        assert!(pdd.spike_detection_enabled);
        assert!(pdd.ramping_detection_enabled);

        let aps = AdaptiveProsperityConfig::default();
        assert!(!aps.enabled);
        assert!(!aps.adaptive_enabled);
        assert_eq!(aps.adaptation_interval_buys, 50);
    }

    #[test]
    fn test_empty_toml_section_uses_defaults() {
        let v25: GatekeeperV25RolloutConfig = toml::from_str("").unwrap();
        assert!(!v25.shadow_enabled);
        assert!(!v25.live_execution_enabled);

        let dow: DynamicObservationWindowConfig = toml::from_str("").unwrap();
        assert!(!dow.enabled);
        assert_eq!(dow.early_entry_min_ms, 2000);

        let pdd: PumpAndDumpDetectorConfig = toml::from_str("").unwrap();
        assert_eq!(pdd.entry_drift_max_pct, 5.0);
        assert!(!pdd.enabled);

        let aps: AdaptiveProsperityConfig = toml::from_str("").unwrap();
        assert!(!aps.adaptive_enabled);
        assert_eq!(aps.calibration_lookback_buys, 100);
    }

    #[test]
    fn test_partial_override() {
        let toml_str = r#"
enabled = true
entry_drift_max_pct = 7.5
"#;
        let pdd: PumpAndDumpDetectorConfig = toml::from_str(toml_str).unwrap();
        assert!(pdd.enabled);
        assert!((pdd.entry_drift_max_pct - 7.5).abs() < f64::EPSILON);
        // Unchanged fields keep their defaults
        assert!(pdd.spike_detection_enabled);
        assert_eq!(pdd.ramping_min_consecutive_buys, 4);
        assert!((pdd.spike_ratio_threshold - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_tas_partial_override() {
        let toml_str = r#"
enabled = true
tas_hard_reject_threshold = 0.25
momentum_trajectory_weight = 0.30
"#;
        let tas: TrajectoryAwareScoringConfig = toml::from_str(toml_str).unwrap();
        assert!(tas.enabled);
        assert!((tas.tas_hard_reject_threshold - 0.25).abs() < f64::EPSILON);
        assert!((tas.momentum_trajectory_weight - 0.30).abs() < f64::EPSILON);
        // Other weights keep defaults
        assert!((tas.hhi_trajectory_weight - 0.20).abs() < f64::EPSILON);
    }

    #[test]
    fn test_dow_partial_override() {
        let toml_str = r#"
enabled = true
normal_window_ms = 6000
early_entry_min_confidence = 0.90
"#;
        let dow: DynamicObservationWindowConfig = toml::from_str(toml_str).unwrap();
        assert!(dow.enabled);
        assert_eq!(dow.normal_window_ms, 6000);
        assert!((dow.early_entry_min_confidence - 0.90).abs() < f64::EPSILON);
        // Unchanged
        assert_eq!(dow.extended_window_ms, 10000);
    }

    #[test]
    fn test_entry_drift_anchor_quality_default() {
        let q: EntryDriftAnchorQuality = Default::default();
        assert_eq!(q, EntryDriftAnchorQuality::Strong);
        // Verify serde round-trip through a struct
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            quality: EntryDriftAnchorQuality,
        }
        let from_toml: Wrapper = toml::from_str("quality = \"weak\"").unwrap();
        assert_eq!(from_toml.quality, EntryDriftAnchorQuality::Weak);
    }
}
