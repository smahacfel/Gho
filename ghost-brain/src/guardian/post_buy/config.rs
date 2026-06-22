//! Configuration for the PostBuy Guardian monitoring layer.
//!
//! Loaded from `[post_buy_guardian]` section in `ghost_brain_config.toml`.
//! All fields have `#[serde(default)]` via the `Default` impl, so the section
//! is entirely optional — missing fields get sensible production defaults.

use crate::aem::config::AemConfig;
use serde::{Deserialize, Serialize};

pub const DEFAULT_WAIT_FOR_TIMESTOP_MS: u64 = 30_000;
pub const DEFAULT_TIME_STOP_V2_FIRST_CHECK_MS: u64 = 3_000;
pub const DEFAULT_TIME_STOP_V2_WINDOW_MS: u64 = 4_000;
pub const DEFAULT_TIME_STOP_V2_FAILED_WINDOWS_TO_SIGNAL: u32 = 3;
pub const DEFAULT_TIME_STOP_V2_MIN_AGE_BEFORE_SIGNAL_MS: u64 = 11_000;
pub const DEFAULT_TIME_STOP_V2_MIN_PRICE_DELTA_PCT_ALIVE: f64 = 3.0;
pub const DEFAULT_TIME_STOP_V2_MIN_MCAP_DELTA_PCT_ALIVE: f64 = 3.0;
pub const DEFAULT_TIME_STOP_V2_MIN_BONDING_DELTA_PCT_ALIVE: f64 = 0.75;
pub const DEFAULT_TIME_STOP_V2_MIN_VOLUME_DELTA_SOL_ALIVE: f64 = 1.0;
pub const DEFAULT_TIME_STOP_V2_MIN_PRICE_DELTA_PCT_FOR_VOLUME_ALIVE: f64 = 1.0;
pub const DEFAULT_TIME_STOP_V2_MIN_TX_DELTA_FOR_HEARTBEAT: u64 = 1;
pub const DEFAULT_TIME_STOP_V2_MAX_AVG_VOLUME_PER_TX_SOL_HEARTBEAT: f64 = 0.05;
pub const DEFAULT_TIME_STOP_V2_MAX_ABS_PRICE_DELTA_PCT_HEARTBEAT: f64 = 1.0;
pub const DEFAULT_TIME_STOP_V2_MAX_ABS_MCAP_DELTA_PCT_HEARTBEAT: f64 = 1.0;
pub const DEFAULT_TIME_STOP_V2_MAX_BONDING_DELTA_PCT_HEARTBEAT: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeStopV2Mode {
    ObserveOnly,
}

impl Default for TimeStopV2Mode {
    fn default() -> Self {
        Self::ObserveOnly
    }
}

/// Observe-only vitality checks for the post-buy TimeStop V2 experiment.
///
/// The config is intentionally nested under `[post_buy_guardian.time_stop_v2]`
/// and defaults to disabled. When enabled in this phase, it only emits
/// counterfactual lifecycle evidence and does not close positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeStopV2Config {
    pub enabled: bool,
    pub mode: TimeStopV2Mode,
    pub first_check_ms: u64,
    pub window_ms: u64,
    pub failed_windows_to_signal: u32,
    pub min_age_before_signal_ms: u64,
    pub min_price_delta_pct_alive: f64,
    pub min_mcap_delta_pct_alive: f64,
    pub min_bonding_delta_pct_alive: f64,
    pub min_volume_delta_sol_alive: f64,
    pub min_price_delta_pct_for_volume_alive: f64,
    pub min_tx_delta_for_heartbeat: u64,
    pub max_avg_volume_per_tx_sol_heartbeat: f64,
    pub max_abs_price_delta_pct_heartbeat: f64,
    pub max_abs_mcap_delta_pct_heartbeat: f64,
    pub max_bonding_delta_pct_heartbeat: f64,
    pub emit_window_records: bool,
}

impl Default for TimeStopV2Config {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: TimeStopV2Mode::ObserveOnly,
            first_check_ms: DEFAULT_TIME_STOP_V2_FIRST_CHECK_MS,
            window_ms: DEFAULT_TIME_STOP_V2_WINDOW_MS,
            failed_windows_to_signal: DEFAULT_TIME_STOP_V2_FAILED_WINDOWS_TO_SIGNAL,
            min_age_before_signal_ms: DEFAULT_TIME_STOP_V2_MIN_AGE_BEFORE_SIGNAL_MS,
            min_price_delta_pct_alive: DEFAULT_TIME_STOP_V2_MIN_PRICE_DELTA_PCT_ALIVE,
            min_mcap_delta_pct_alive: DEFAULT_TIME_STOP_V2_MIN_MCAP_DELTA_PCT_ALIVE,
            min_bonding_delta_pct_alive: DEFAULT_TIME_STOP_V2_MIN_BONDING_DELTA_PCT_ALIVE,
            min_volume_delta_sol_alive: DEFAULT_TIME_STOP_V2_MIN_VOLUME_DELTA_SOL_ALIVE,
            min_price_delta_pct_for_volume_alive:
                DEFAULT_TIME_STOP_V2_MIN_PRICE_DELTA_PCT_FOR_VOLUME_ALIVE,
            min_tx_delta_for_heartbeat: DEFAULT_TIME_STOP_V2_MIN_TX_DELTA_FOR_HEARTBEAT,
            max_avg_volume_per_tx_sol_heartbeat:
                DEFAULT_TIME_STOP_V2_MAX_AVG_VOLUME_PER_TX_SOL_HEARTBEAT,
            max_abs_price_delta_pct_heartbeat:
                DEFAULT_TIME_STOP_V2_MAX_ABS_PRICE_DELTA_PCT_HEARTBEAT,
            max_abs_mcap_delta_pct_heartbeat: DEFAULT_TIME_STOP_V2_MAX_ABS_MCAP_DELTA_PCT_HEARTBEAT,
            max_bonding_delta_pct_heartbeat: DEFAULT_TIME_STOP_V2_MAX_BONDING_DELTA_PCT_HEARTBEAT,
            emit_window_records: true,
        }
    }
}

impl TimeStopV2Config {
    pub fn first_check_ms(&self) -> u64 {
        self.first_check_ms.max(1)
    }

    pub fn window_ms(&self) -> u64 {
        self.window_ms.max(1)
    }

    pub fn failed_windows_to_signal(&self) -> u32 {
        self.failed_windows_to_signal.max(1)
    }

    pub fn min_age_before_signal_ms(&self) -> u64 {
        self.min_age_before_signal_ms.max(self.first_check_ms())
    }
}

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

    // -- Shadow lifecycle exit thresholds -----------------------------------
    /// Shadow/probe target threshold in percentage points (50.0 = +50%).
    ///
    /// This is optional for backward compatibility. When absent, launchers may
    /// fall back to their legacy `live_exit_take_profit_pct` fraction.
    #[serde(default)]
    pub target_threshold: Option<f64>,

    /// Shadow/probe stop-loss threshold in percentage points (50.0 = -50%).
    ///
    /// Values above 100 are clamped by the consumer because stop-loss cannot
    /// move below zero price.
    #[serde(default)]
    pub stoploss_threshold: Option<f64>,

    /// Shadow/probe inactivity timeout in milliseconds before TimeStop close.
    #[serde(default)]
    pub wait_for_timestop: Option<u64>,

    /// Observe-only TimeStop V2 vitality telemetry.
    #[serde(default)]
    pub time_stop_v2: TimeStopV2Config,

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
            target_threshold: None,
            stoploss_threshold: None,
            wait_for_timestop: None,
            time_stop_v2: TimeStopV2Config::default(),

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

impl PostBuyGuardianConfig {
    pub fn wait_for_timestop_ms(&self) -> u64 {
        self.wait_for_timestop
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_WAIT_FOR_TIMESTOP_MS)
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

    #[test]
    fn deserialize_shadow_lifecycle_thresholds_from_percent_and_ms() {
        let toml_str = r#"
            target_threshold = 150.0
            stoploss_threshold = 50.0
            wait_for_timestop = 45000
        "#;
        let cfg: PostBuyGuardianConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.target_threshold, Some(150.0));
        assert_eq!(cfg.stoploss_threshold, Some(50.0));
        assert_eq!(cfg.wait_for_timestop, Some(45_000));
        assert_eq!(cfg.wait_for_timestop_ms(), 45_000);
    }

    #[test]
    fn default_timestop_preserves_legacy_thirty_seconds() {
        let cfg = PostBuyGuardianConfig::default();
        assert_eq!(cfg.target_threshold, None);
        assert_eq!(cfg.stoploss_threshold, None);
        assert_eq!(cfg.wait_for_timestop, None);
        assert_eq!(cfg.wait_for_timestop_ms(), DEFAULT_WAIT_FOR_TIMESTOP_MS);
    }

    #[test]
    fn default_time_stop_v2_is_disabled_observe_only() {
        let cfg = PostBuyGuardianConfig::default();

        assert!(!cfg.time_stop_v2.enabled);
        assert_eq!(cfg.time_stop_v2.mode, TimeStopV2Mode::ObserveOnly);
        assert_eq!(
            cfg.time_stop_v2.first_check_ms(),
            DEFAULT_TIME_STOP_V2_FIRST_CHECK_MS
        );
        assert_eq!(cfg.time_stop_v2.window_ms(), DEFAULT_TIME_STOP_V2_WINDOW_MS);
        assert_eq!(
            cfg.time_stop_v2.failed_windows_to_signal(),
            DEFAULT_TIME_STOP_V2_FAILED_WINDOWS_TO_SIGNAL
        );
    }

    #[test]
    fn deserialize_time_stop_v2_nested_config() {
        let toml_str = r#"
            [time_stop_v2]
            enabled = true
            mode = "observe_only"
            first_check_ms = 3000
            window_ms = 4000
            failed_windows_to_signal = 2
            min_age_before_signal_ms = 7000
            min_price_delta_pct_alive = 2.5
            min_mcap_delta_pct_alive = 2.0
            min_bonding_delta_pct_alive = 0.5
            min_volume_delta_sol_alive = 0.75
            min_price_delta_pct_for_volume_alive = 0.8
            min_tx_delta_for_heartbeat = 2
            max_avg_volume_per_tx_sol_heartbeat = 0.03
            max_abs_price_delta_pct_heartbeat = 0.7
            max_abs_mcap_delta_pct_heartbeat = 0.9
            max_bonding_delta_pct_heartbeat = 0.2
            emit_window_records = false
        "#;

        let cfg: PostBuyGuardianConfig = toml::from_str(toml_str).unwrap();

        assert!(cfg.time_stop_v2.enabled);
        assert_eq!(cfg.time_stop_v2.mode, TimeStopV2Mode::ObserveOnly);
        assert_eq!(cfg.time_stop_v2.first_check_ms(), 3_000);
        assert_eq!(cfg.time_stop_v2.window_ms(), 4_000);
        assert_eq!(cfg.time_stop_v2.failed_windows_to_signal(), 2);
        assert_eq!(cfg.time_stop_v2.min_age_before_signal_ms(), 7_000);
        assert_eq!(cfg.time_stop_v2.min_price_delta_pct_alive, 2.5);
        assert_eq!(cfg.time_stop_v2.min_mcap_delta_pct_alive, 2.0);
        assert_eq!(cfg.time_stop_v2.min_bonding_delta_pct_alive, 0.5);
        assert_eq!(cfg.time_stop_v2.min_volume_delta_sol_alive, 0.75);
        assert_eq!(cfg.time_stop_v2.min_price_delta_pct_for_volume_alive, 0.8);
        assert_eq!(cfg.time_stop_v2.min_tx_delta_for_heartbeat, 2);
        assert_eq!(cfg.time_stop_v2.max_avg_volume_per_tx_sol_heartbeat, 0.03);
        assert_eq!(cfg.time_stop_v2.max_abs_price_delta_pct_heartbeat, 0.7);
        assert_eq!(cfg.time_stop_v2.max_abs_mcap_delta_pct_heartbeat, 0.9);
        assert_eq!(cfg.time_stop_v2.max_bonding_delta_pct_heartbeat, 0.2);
        assert!(!cfg.time_stop_v2.emit_window_records);
    }
}
