//! Configuration for the PostBuy Guardian monitoring layer.
//!
//! Loaded from `[post_buy_guardian]` section in `ghost_brain_config.toml`.
//! All fields have `#[serde(default)]` via the `Default` impl, so the section
//! is entirely optional — missing fields get sensible production defaults.

use crate::aem::config::AemConfig;
use serde::{Deserialize, Serialize};

/// Configuration for PostBuy Guardian real-time position monitoring.
///
/// Controls tick frequency, per-module thresholds, and signal aggregation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PostBuyGuardianConfig {
    // ── Global ──────────────────────────────────────────────────────────
    /// Enable/disable the entire PostBuy Guardian layer.
    pub enabled: bool,

    /// Monitoring tick interval in milliseconds.
    /// Each tick runs all 4 modules against all active positions.
    pub tick_interval_ms: u64,

    /// Maximum number of concurrent monitored positions.
    /// Beyond this limit, new positions are NOT monitored (warning logged).
    pub max_monitored_positions: usize,

    /// Channel buffer size for GuardianSignal sender.
    pub signal_channel_buffer: usize,

    // ── LIGMA thresholds ────────────────────────────────────────────────
    /// Retail impact (bps) above which we emit Warning.
    pub ligma_warning_impact_bps: f64,

    /// Retail impact (bps) above which we emit Critical.
    pub ligma_critical_impact_bps: f64,

    /// Tradability (ψ_LIGMA) below which we emit Warning.
    pub ligma_warning_tradability: f64,

    /// Tradability (ψ_LIGMA) below which we emit Critical (liquidity trap).
    pub ligma_critical_tradability: f64,

    /// SOL amount used to probe liquidity impact (simulated sell size).
    pub ligma_probe_sol: f64,

    // ── WHF thresholds ──────────────────────────────────────────────────
    /// Minimum confidence for WHF signal to be actionable.
    pub whf_min_confidence: f32,

    /// Wash trading detection → automatic Critical?
    pub whf_wash_trading_is_critical: bool,

    /// Minimum net flow (SOL) to trigger wash-trading check.
    pub whf_min_net_flow_sol: f64,

    /// Maximum price change (absolute ratio) to still consider wash trading.
    /// e.g. 0.02 = if price moved less than 2% with high volume → suspicious.
    pub whf_wash_max_price_change: f64,

    /// Trend decay: minimum price drop (ratio) to consider distribution.
    pub whf_decay_min_price_drop: f64,

    /// Trend decay: maximum volume CV to consider uniform selling.
    pub whf_decay_max_volume_cv: f64,

    // ── TCF thresholds ──────────────────────────────────────────────────
    /// Cohesion below this → Warning.
    pub tcf_warning_cohesion: f64,

    /// Cohesion below this → Critical.
    pub tcf_critical_cohesion: f64,

    /// Cliff detection (sudden cohesion drop) → auto Warning.
    pub tcf_cliff_is_warning: bool,

    /// Number of consecutive low-cohesion ticks before escalation to Critical.
    pub tcf_consecutive_low_max: u32,

    // ── PANIC thresholds ────────────────────────────────────────────────
    /// TX/s above which we emit Warning.
    pub panic_warning_txps: f64,

    /// TX/s above which we emit Critical.
    pub panic_critical_txps: f64,

    /// Entropy below this threshold combined with high TX rate → coordinated sell.
    pub panic_low_entropy_threshold: f64,

    /// Time window (ms) for TX rate computation.
    pub panic_rate_window_ms: u64,

    // ── Signal aggregation ──────────────────────────────────────────────
    /// Number of Warning signals in window before auto-escalation to TightenStop.
    pub escalation_warning_count: u32,

    /// Number of Critical signals in window before PanicSell.
    pub escalation_critical_count: u32,

    /// Time window (ms) for signal aggregation.
    pub signal_aggregation_window_ms: u64,

    /// Maximum old signals retained per position (memory cap).
    pub max_signals_per_position: usize,

    /// Adaptive Exit Manager (AEM) v1 configuration.
    pub aem: AemConfig,
}

impl Default for PostBuyGuardianConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tick_interval_ms: 500,
            max_monitored_positions: 10,
            signal_channel_buffer: 256,

            // LIGMA
            ligma_warning_impact_bps: 3500.0,
            ligma_critical_impact_bps: 8000.0,
            ligma_warning_tradability: 0.4,
            ligma_critical_tradability: 0.15,
            ligma_probe_sol: 0.1,

            // WHF
            whf_min_confidence: 0.6,
            whf_wash_trading_is_critical: true,
            whf_min_net_flow_sol: 0.5,
            whf_wash_max_price_change: 0.02,
            whf_decay_min_price_drop: 0.05,
            whf_decay_max_volume_cv: 0.3,

            // TCF
            tcf_warning_cohesion: 0.4,
            tcf_critical_cohesion: 0.2,
            tcf_cliff_is_warning: true,
            tcf_consecutive_low_max: 5,

            // PANIC
            panic_warning_txps: 15.0,
            panic_critical_txps: 30.0,
            panic_low_entropy_threshold: 1.0,
            panic_rate_window_ms: 2000,

            // Aggregation
            escalation_warning_count: 3,
            escalation_critical_count: 1,
            signal_aggregation_window_ms: 5000,
            max_signals_per_position: 200,
            aem: AemConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = PostBuyGuardianConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.tick_interval_ms > 0);
        assert!(cfg.max_monitored_positions > 0);
        assert!(cfg.ligma_warning_impact_bps < cfg.ligma_critical_impact_bps);
        assert!(cfg.ligma_critical_tradability < cfg.ligma_warning_tradability);
        assert!(cfg.tcf_critical_cohesion < cfg.tcf_warning_cohesion);
        assert!(cfg.panic_warning_txps < cfg.panic_critical_txps);
        assert!(cfg.escalation_critical_count <= cfg.escalation_warning_count);
    }

    #[test]
    fn deserialize_empty_toml_gives_default() {
        let cfg: PostBuyGuardianConfig = toml::from_str("").unwrap();
        let default = PostBuyGuardianConfig::default();
        assert_eq!(cfg.tick_interval_ms, default.tick_interval_ms);
        assert_eq!(cfg.max_monitored_positions, default.max_monitored_positions);
    }

    #[test]
    fn deserialize_partial_toml() {
        let toml_str = r#"
            enabled = false
            tick_interval_ms = 250
        "#;
        let cfg: PostBuyGuardianConfig = toml::from_str(toml_str).unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.tick_interval_ms, 250);
        // Other fields should be default
        assert_eq!(cfg.max_monitored_positions, 10);
    }
}
