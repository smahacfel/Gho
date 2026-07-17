//! Configuration for the PostBuy Guardian monitoring layer.
//!
//! Loaded from `[post_buy_guardian]` section in `ghost_brain_config.toml`.
//! All fields have `#[serde(default)]` via the `Default` impl, so the section
//! is entirely optional — missing fields get sensible production defaults.

use crate::aem::config::AemConfig;
use serde::{Deserialize, Serialize};

pub const DEFAULT_WAIT_FOR_TIMESTOP_MS: u64 = 30_000;
pub const DEFAULT_EXIT_POLICY_V1_QUOTE_RECOVERY_MS: u64 = 5_000;
pub const DEFAULT_EXIT_POLICY_V1_ABSOLUTE_MAX_HOLD_MS: u64 = 120_000;
pub const DEFAULT_EXIT_POLICY_V1_CRASH_WINDOW_MS: u64 = 1_500;
pub const DEFAULT_EXIT_POLICY_V1_CRASH_MIN_SHORT_WINDOW_DROP_PCT: f64 = 25.0;
pub const DEFAULT_EXIT_POLICY_V1_CRASH_MIN_PEAK_DRAWDOWN_PCT: f64 = 30.0;
pub const DEFAULT_EXIT_POLICY_V1_CRASH_MIN_DISTINCT_SLOTS: u8 = 2;
pub const DEFAULT_EXIT_POLICY_V1_CRASH_MAX_SAMPLE_AGE_MS: u64 = 1_500;
pub const DEFAULT_EXIT_POLICY_V1_CRASH_MAX_EXECUTABLE_RETURN_PCT: f64 = -20.0;
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
pub const DEFAULT_EXIT_REPLAY_HORIZON_MS: u64 = 120_000;
pub const DEFAULT_EXIT_REPLAY_PNL_STEP_BPS: i32 = 25;
pub const DEFAULT_EXIT_REPLAY_HEARTBEAT_MS: u64 = 1_000;
pub const DEFAULT_EXIT_REPLAY_MAX_PATH_POINTS: usize = 512;
pub const DEFAULT_EXIT_REPLAY_SHUTDOWN_FLUSH_BUDGET_MS: u64 = 3_000;
pub const DEFAULT_HET_PM_V2_TRAJECTORY_SHORT_MS: u64 = 1_500;
pub const DEFAULT_HET_PM_V2_TRAJECTORY_MEDIUM_MS: u64 = 5_000;
pub const DEFAULT_HET_PM_V2_TRAJECTORY_LONG_MS: u64 = 15_000;
pub const DEFAULT_HET_PM_V2_MAX_NEWEST_SAMPLE_AGE_MS: u64 = 1_500;
pub const DEFAULT_HET_PM_V2_TRAILING_ARM_MARK_RETURN_BPS: i32 = 2_500;
pub const DEFAULT_HET_PM_V2_TRAILING_MARK_CANDIDATE_DRAWDOWN_BPS: u32 = 1_500;
pub const DEFAULT_HET_PM_V2_TRAILING_EXECUTABLE_BREACH_BPS: u32 = 1_800;
pub const DEFAULT_HET_PM_V2_PEAK_ANCHOR_MIN_STEP_BPS: u32 = 500;
pub const DEFAULT_HET_PM_V2_PEAK_ANCHOR_FORCE_REFRESH_MS: u64 = 5_000;
pub const DEFAULT_HET_PM_V2_VITALITY_MIN_AGE_MS: u64 = 11_000;
pub const DEFAULT_HET_PM_V2_VITALITY_REQUIRED_NON_ALIVE_WINDOWS: u32 = 3;
pub const DEFAULT_HET_PM_V2_VITALITY_MIN_TIME_SINCE_PEAK_MS: u64 = 5_000;
pub const DEFAULT_HET_PM_V2_VITALITY_RECOVERY_RETURN_BPS: i32 = 300;
pub const DEFAULT_HET_PM_V2_WRITER_QUEUE_CAPACITY: usize = 256;
pub const DEFAULT_HET_PM_V2_TERMINAL_WRITE_BUDGET_MS: u64 = 25;
pub const DEFAULT_EXIT_REPLAY_LEVELS_BPS: [i32; 23] = [
    -5000, -3000, -2000, -1500, -1000, -700, -500, -300, -200, -100, 100, 200, 300, 400, 500, 700,
    1000, 1500, 2000, 3000, 5000, 7500, 10000,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeStopV2Mode {
    #[default]
    ObserveOnly,
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
    /// Deterministic identity of the complete TimeStop V2 evidence producer.
    ///
    /// HET-PM V2 consumes the projection emitted by this source, so burn-in
    /// records must remain distinguishable when any source knob changes. The
    /// HET policy hash intentionally stays separate from this identity.
    pub fn projection_config_hash(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct HashInput<'a> {
            projection_id: &'static str,
            projection_version: u16,
            config: &'a TimeStopV2Config,
        }

        let encoded = serde_json::to_vec(&HashInput {
            projection_id: "post_buy_time_stop_v2_projection_v1",
            projection_version: 1,
            config: self,
        })?;
        Ok(blake3::hash(&encoded).to_hex().to_string())
    }

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

/// CrashGuard authority contract.
///
/// `ObserveOnly` materializes and logs a counterfactual candidate but cannot
/// mutate a position. `AuthoritativeShadow` is intentionally accepted only by
/// the pure policy; launcher startup must separately prove the complete shadow
/// profile before allowing it to become active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrashGuardMode {
    #[default]
    Disabled,
    ObserveOnly,
    AuthoritativeShadow,
}

/// Backward-compatible runtime knobs owned by Position Manager Lite V1.
///
/// Missing fields deliberately preserve the PR1 behavior: max-hold and
/// CrashGuard are disabled. The numerical defaults are retained solely so an
/// operator can enable either feature with an explicit, complete setting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExitPolicyV1Config {
    /// Maximum time spent retrying a shadow executable quote after an exit
    /// proposal has become sticky.
    pub quote_recovery_ms: u64,
    /// Enables the shadow-only absolute hold-time exit.
    pub absolute_max_hold_enabled: bool,
    /// Absolute position age at which a full exit is proposed when enabled.
    pub absolute_max_hold_ms: u64,
    /// CrashGuard authority mode. Defaults to disabled for old configs.
    pub crash_guard_mode: CrashGuardMode,
    /// Maximum chronological window considered by CrashGuard.
    pub crash_window_ms: u64,
    /// Required oldest-to-newest mark-price fall within the crash window.
    pub crash_min_short_window_drop_pct: f64,
    /// Required drawdown from the canonical peak since entry.
    pub crash_min_peak_drawdown_pct: f64,
    /// Minimum number of valid, strictly ordered distinct-slot samples.
    pub crash_min_distinct_slots: u8,
    /// Maximum age of the newest CrashGuard sample.
    pub crash_max_sample_age_ms: u64,
    /// Maximum executable gross return needed to confirm a crash candidate.
    pub crash_max_executable_return_pct: f64,
}

impl Default for ExitPolicyV1Config {
    fn default() -> Self {
        Self {
            quote_recovery_ms: DEFAULT_EXIT_POLICY_V1_QUOTE_RECOVERY_MS,
            absolute_max_hold_enabled: false,
            absolute_max_hold_ms: DEFAULT_EXIT_POLICY_V1_ABSOLUTE_MAX_HOLD_MS,
            crash_guard_mode: CrashGuardMode::Disabled,
            crash_window_ms: DEFAULT_EXIT_POLICY_V1_CRASH_WINDOW_MS,
            crash_min_short_window_drop_pct: DEFAULT_EXIT_POLICY_V1_CRASH_MIN_SHORT_WINDOW_DROP_PCT,
            crash_min_peak_drawdown_pct: DEFAULT_EXIT_POLICY_V1_CRASH_MIN_PEAK_DRAWDOWN_PCT,
            crash_min_distinct_slots: DEFAULT_EXIT_POLICY_V1_CRASH_MIN_DISTINCT_SLOTS,
            crash_max_sample_age_ms: DEFAULT_EXIT_POLICY_V1_CRASH_MAX_SAMPLE_AGE_MS,
            crash_max_executable_return_pct: DEFAULT_EXIT_POLICY_V1_CRASH_MAX_EXECUTABLE_RETURN_PCT,
        }
    }
}

/// Compact, shadow-only post-buy price path evidence for offline exit replay.
///
/// This is a research sidecar. It must never influence BUY/REJECT, live exits,
/// selector scoring, alpha scoring, or canonical V2.5 confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowExitReplayConfig {
    pub enabled: bool,
    pub horizon_ms: u64,
    pub pnl_step_bps: i32,
    pub heartbeat_ms: u64,
    pub max_path_points: usize,
    pub levels_bps: Vec<i32>,
    pub flush_on_shutdown: bool,
    pub shutdown_flush_budget_ms: u64,
}

impl Default for ShadowExitReplayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            horizon_ms: DEFAULT_EXIT_REPLAY_HORIZON_MS,
            pnl_step_bps: DEFAULT_EXIT_REPLAY_PNL_STEP_BPS,
            heartbeat_ms: DEFAULT_EXIT_REPLAY_HEARTBEAT_MS,
            max_path_points: DEFAULT_EXIT_REPLAY_MAX_PATH_POINTS,
            levels_bps: DEFAULT_EXIT_REPLAY_LEVELS_BPS.to_vec(),
            flush_on_shutdown: false,
            shutdown_flush_budget_ms: DEFAULT_EXIT_REPLAY_SHUTDOWN_FLUSH_BUDGET_MS,
        }
    }
}

impl ShadowExitReplayConfig {
    pub fn horizon_ms(&self) -> u64 {
        self.horizon_ms.max(1)
    }

    pub fn pnl_step_bps(&self) -> i32 {
        if self.pnl_step_bps > 0 {
            self.pnl_step_bps
        } else {
            DEFAULT_EXIT_REPLAY_PNL_STEP_BPS
        }
    }

    pub fn heartbeat_ms(&self) -> u64 {
        self.heartbeat_ms.max(1)
    }

    pub fn max_path_points(&self) -> usize {
        self.max_path_points.max(2)
    }

    pub fn shutdown_flush_budget_ms(&self) -> u64 {
        self.shutdown_flush_budget_ms
    }

    pub fn sanitized_levels_bps(&self) -> Vec<i32> {
        let mut levels = if self.levels_bps.is_empty() {
            DEFAULT_EXIT_REPLAY_LEVELS_BPS.to_vec()
        } else {
            self.levels_bps.clone()
        };
        levels.retain(|level| *level != 0);
        levels.sort_unstable();
        levels.dedup();
        levels
    }
}

/// Authority mode for the Hierarchical Executable Trajectory observer.
///
/// PR A accepts only `ObserveOnly`. The second variant is deserializable so a
/// premature rollout request fails with a typed startup error rather than an
/// opaque TOML enum error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HetPmV2Mode {
    #[default]
    ObserveOnly,
    AuthoritativeShadow,
}

/// Observe-only hypotheses for HET Position Manager V2 PR A.
///
/// An absent section remains disabled. These defaults are research starting
/// points, not production exit authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HetPmV2Config {
    pub enabled: bool,
    pub mode: HetPmV2Mode,
    pub trajectory_short_ms: u64,
    pub trajectory_medium_ms: u64,
    pub trajectory_long_ms: u64,
    pub max_newest_sample_age_ms: u64,
    pub trailing_arm_mark_return_bps: i32,
    pub trailing_mark_candidate_drawdown_bps: u32,
    pub trailing_executable_breach_bps: u32,
    pub peak_anchor_min_step_bps: u32,
    pub peak_anchor_force_refresh_on_new_peak_after_ms: u64,
    pub vitality_min_age_ms: u64,
    pub vitality_required_non_alive_windows: u32,
    pub vitality_min_time_since_peak_ms: u64,
    pub vitality_recovery_return_bps: i32,
    /// Maximum number of pre-serialized observation rows waiting for the
    /// single HET sidecar writer. A full queue drops observer-only rows.
    pub writer_queue_capacity: usize,
    /// Hard upper bound for awaiting a terminal sidecar acknowledgement.
    /// Expiry degrades the observer payload to typed `Skipped` and canonical
    /// V1 terminal persistence continues.
    pub terminal_write_budget_ms: u64,
}

impl Default for HetPmV2Config {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: HetPmV2Mode::ObserveOnly,
            trajectory_short_ms: DEFAULT_HET_PM_V2_TRAJECTORY_SHORT_MS,
            trajectory_medium_ms: DEFAULT_HET_PM_V2_TRAJECTORY_MEDIUM_MS,
            trajectory_long_ms: DEFAULT_HET_PM_V2_TRAJECTORY_LONG_MS,
            max_newest_sample_age_ms: DEFAULT_HET_PM_V2_MAX_NEWEST_SAMPLE_AGE_MS,
            trailing_arm_mark_return_bps: DEFAULT_HET_PM_V2_TRAILING_ARM_MARK_RETURN_BPS,
            trailing_mark_candidate_drawdown_bps:
                DEFAULT_HET_PM_V2_TRAILING_MARK_CANDIDATE_DRAWDOWN_BPS,
            trailing_executable_breach_bps: DEFAULT_HET_PM_V2_TRAILING_EXECUTABLE_BREACH_BPS,
            peak_anchor_min_step_bps: DEFAULT_HET_PM_V2_PEAK_ANCHOR_MIN_STEP_BPS,
            peak_anchor_force_refresh_on_new_peak_after_ms:
                DEFAULT_HET_PM_V2_PEAK_ANCHOR_FORCE_REFRESH_MS,
            vitality_min_age_ms: DEFAULT_HET_PM_V2_VITALITY_MIN_AGE_MS,
            vitality_required_non_alive_windows:
                DEFAULT_HET_PM_V2_VITALITY_REQUIRED_NON_ALIVE_WINDOWS,
            vitality_min_time_since_peak_ms: DEFAULT_HET_PM_V2_VITALITY_MIN_TIME_SINCE_PEAK_MS,
            vitality_recovery_return_bps: DEFAULT_HET_PM_V2_VITALITY_RECOVERY_RETURN_BPS,
            writer_queue_capacity: DEFAULT_HET_PM_V2_WRITER_QUEUE_CAPACITY,
            terminal_write_budget_ms: DEFAULT_HET_PM_V2_TERMINAL_WRITE_BUDGET_MS,
        }
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
    /// Values outside 0..=100 are rejected at Position Manager startup because
    /// a stop-loss cannot move below zero price.
    #[serde(default)]
    pub stoploss_threshold: Option<f64>,

    /// Shadow/probe inactivity timeout in milliseconds before TimeStop close.
    #[serde(default)]
    pub wait_for_timestop: Option<u64>,

    /// Observe-only TimeStop V2 vitality telemetry.
    #[serde(default)]
    pub time_stop_v2: TimeStopV2Config,

    /// Shadow-only compact exit path replay evidence.
    #[serde(default)]
    pub exit_replay_v1: ShadowExitReplayConfig,

    /// Position Manager Lite V1 policy runtime settings.
    #[serde(default)]
    pub exit_policy_v1: ExitPolicyV1Config,

    /// Hierarchical Executable Trajectory Position Manager V2 observer.
    #[serde(default)]
    pub het_pm_v2: HetPmV2Config,

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
            exit_replay_v1: ShadowExitReplayConfig::default(),
            exit_policy_v1: ExitPolicyV1Config::default(),
            het_pm_v2: HetPmV2Config::default(),

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
    fn deserialize_exit_replay_v1_defaults_and_overrides() {
        let cfg: PostBuyGuardianConfig = toml::from_str(
            r#"
            [exit_replay_v1]
            enabled = true
            horizon_ms = 90000
            pnl_step_bps = 10
            heartbeat_ms = 500
            max_path_points = 128
            flush_on_shutdown = true
            shutdown_flush_budget_ms = 2500
            levels_bps = [-300, 100, 300]
            "#,
        )
        .unwrap();

        assert!(cfg.exit_replay_v1.enabled);
        assert_eq!(cfg.exit_replay_v1.horizon_ms(), 90_000);
        assert_eq!(cfg.exit_replay_v1.pnl_step_bps(), 10);
        assert_eq!(cfg.exit_replay_v1.heartbeat_ms(), 500);
        assert_eq!(cfg.exit_replay_v1.max_path_points(), 128);
        assert!(cfg.exit_replay_v1.flush_on_shutdown);
        assert_eq!(cfg.exit_replay_v1.shutdown_flush_budget_ms(), 2_500);
        assert_eq!(
            cfg.exit_replay_v1.sanitized_levels_bps(),
            vec![-300, 100, 300]
        );
    }

    #[test]
    fn default_timestop_preserves_legacy_thirty_seconds() {
        let cfg = PostBuyGuardianConfig::default();
        assert_eq!(cfg.target_threshold, None);
        assert_eq!(cfg.stoploss_threshold, None);
        assert_eq!(cfg.wait_for_timestop, None);
        assert_eq!(cfg.wait_for_timestop_ms(), DEFAULT_WAIT_FOR_TIMESTOP_MS);
        assert_eq!(
            cfg.exit_policy_v1.quote_recovery_ms,
            DEFAULT_EXIT_POLICY_V1_QUOTE_RECOVERY_MS
        );
        assert!(!cfg.exit_policy_v1.absolute_max_hold_enabled);
        assert_eq!(
            cfg.exit_policy_v1.crash_guard_mode,
            CrashGuardMode::Disabled
        );
    }

    #[test]
    fn deserialize_exit_policy_v1_defaults_and_override() {
        let default_cfg: PostBuyGuardianConfig = toml::from_str("").unwrap();
        assert_eq!(
            default_cfg.exit_policy_v1.quote_recovery_ms,
            DEFAULT_EXIT_POLICY_V1_QUOTE_RECOVERY_MS
        );

        let cfg: PostBuyGuardianConfig = toml::from_str(
            r#"
            [exit_policy_v1]
            quote_recovery_ms = 7500
            "#,
        )
        .unwrap();
        assert_eq!(cfg.exit_policy_v1.quote_recovery_ms, 7_500);
    }

    #[test]
    fn deserialize_exit_policy_v1_pr2_fields_is_backward_compatible_and_exact() {
        let defaults: PostBuyGuardianConfig = toml::from_str("").unwrap();
        assert!(!defaults.exit_policy_v1.absolute_max_hold_enabled);
        assert_eq!(defaults.exit_policy_v1.absolute_max_hold_ms, 120_000);
        assert_eq!(
            defaults.exit_policy_v1.crash_guard_mode,
            CrashGuardMode::Disabled
        );

        let cfg: PostBuyGuardianConfig = toml::from_str(
            r#"
            [exit_policy_v1]
            quote_recovery_ms = 5000
            absolute_max_hold_enabled = true
            absolute_max_hold_ms = 120000
            crash_guard_mode = "observe_only"
            crash_window_ms = 1500
            crash_min_short_window_drop_pct = 25.0
            crash_min_peak_drawdown_pct = 30.0
            crash_min_distinct_slots = 2
            crash_max_sample_age_ms = 1500
            crash_max_executable_return_pct = -20.0
            "#,
        )
        .unwrap();
        let policy = &cfg.exit_policy_v1;
        assert!(policy.absolute_max_hold_enabled);
        assert_eq!(policy.absolute_max_hold_ms, 120_000);
        assert_eq!(policy.crash_guard_mode, CrashGuardMode::ObserveOnly);
        assert_eq!(policy.crash_window_ms, 1_500);
        assert_eq!(policy.crash_min_short_window_drop_pct, 25.0);
        assert_eq!(policy.crash_min_peak_drawdown_pct, 30.0);
        assert_eq!(policy.crash_min_distinct_slots, 2);
        assert_eq!(policy.crash_max_sample_age_ms, 1_500);
        assert_eq!(policy.crash_max_executable_return_pct, -20.0);
    }

    #[test]
    fn missing_het_pm_v2_section_is_disabled_and_does_not_change_crash_guard() {
        let cfg: PostBuyGuardianConfig = toml::from_str("").unwrap();

        assert!(!cfg.het_pm_v2.enabled);
        assert_eq!(cfg.het_pm_v2.mode, HetPmV2Mode::ObserveOnly);
        assert_eq!(
            cfg.exit_policy_v1.crash_guard_mode,
            CrashGuardMode::Disabled
        );
    }

    #[test]
    fn deserialize_het_pm_v2_nested_config_exactly() {
        let cfg: PostBuyGuardianConfig = toml::from_str(
            r#"
            [het_pm_v2]
            enabled = true
            mode = "observe_only"
            trajectory_short_ms = 1500
            trajectory_medium_ms = 5000
            trajectory_long_ms = 15000
            max_newest_sample_age_ms = 1500
            trailing_arm_mark_return_bps = 2500
            trailing_mark_candidate_drawdown_bps = 1500
            trailing_executable_breach_bps = 1800
            peak_anchor_min_step_bps = 500
            peak_anchor_force_refresh_on_new_peak_after_ms = 5000
            vitality_min_age_ms = 11000
            vitality_required_non_alive_windows = 3
            vitality_min_time_since_peak_ms = 5000
            vitality_recovery_return_bps = 300
            "#,
        )
        .unwrap();

        let het = &cfg.het_pm_v2;
        assert!(het.enabled);
        assert_eq!(het.mode, HetPmV2Mode::ObserveOnly);
        assert_eq!(het.trajectory_short_ms, 1_500);
        assert_eq!(het.trajectory_medium_ms, 5_000);
        assert_eq!(het.trajectory_long_ms, 15_000);
        assert_eq!(het.max_newest_sample_age_ms, 1_500);
        assert_eq!(het.trailing_arm_mark_return_bps, 2_500);
        assert_eq!(het.trailing_mark_candidate_drawdown_bps, 1_500);
        assert_eq!(het.trailing_executable_breach_bps, 1_800);
        assert_eq!(het.peak_anchor_min_step_bps, 500);
        assert_eq!(het.peak_anchor_force_refresh_on_new_peak_after_ms, 5_000);
        assert_eq!(het.vitality_min_age_ms, 11_000);
        assert_eq!(het.vitality_required_non_alive_windows, 3);
        assert_eq!(het.vitality_min_time_since_peak_ms, 5_000);
        assert_eq!(het.vitality_recovery_return_bps, 300);
        assert_eq!(
            cfg.exit_policy_v1.crash_guard_mode,
            CrashGuardMode::Disabled
        );
    }

    #[test]
    fn unknown_het_pm_v2_mode_is_rejected_by_deserialization() {
        let result = toml::from_str::<PostBuyGuardianConfig>(
            r#"
            [het_pm_v2]
            enabled = true
            mode = "unknown_future_mode"
            "#,
        );

        assert!(result.is_err(), "unknown HET-PM V2 mode must fail closed");
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

    #[test]
    fn time_stop_v2_projection_hash_covers_source_semantics() {
        let base = TimeStopV2Config::default();
        let same = TimeStopV2Config::default();
        assert_eq!(
            base.projection_config_hash().unwrap(),
            same.projection_config_hash().unwrap()
        );

        let mut changed_window = base.clone();
        changed_window.window_ms += 1;
        assert_ne!(
            base.projection_config_hash().unwrap(),
            changed_window.projection_config_hash().unwrap()
        );

        let mut changed_vitality_threshold = base.clone();
        changed_vitality_threshold.min_price_delta_pct_alive += 0.5;
        assert_ne!(
            base.projection_config_hash().unwrap(),
            changed_vitality_threshold.projection_config_hash().unwrap()
        );
    }
}
