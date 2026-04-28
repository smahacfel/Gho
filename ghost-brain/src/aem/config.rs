use serde::{Deserialize, Serialize};

use crate::aem::error::AemError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AemConfig {
    pub enabled: bool,

    pub t_s: u64,
    pub min_stabilization_ticks: u32,

    pub n_min_per_key: u32,
    pub min_edge: f64,
    pub k_ci: f64,
    pub tail_risk_limit_wait: f64,
    pub oracle_stale_hard_ms: u64,
    pub partial_fraction_bps: u16,
    pub ci_compare_against_sell_now: bool,
    pub ci_compare_against_partial: bool,

    pub decay_half_life_days: f64,
    pub replay_window_days: u32,
    pub replay_max_events: usize,

    pub stress_low_requeue_max: u32,
    pub stress_med_requeue_min: u32,
    pub stress_med_requeue_max: u32,
    pub stress_high_requeue_min: u32,
    pub stress_low_send_fail_max: u32,
    pub stress_med_send_fail_eq: u32,
    pub stress_high_send_fail_min: u32,
    pub stress_low_relax_max: u32,
    pub stress_med_relax_eq: u32,
    pub stress_high_relax_min: u32,

    pub drawdown_bucket_edges_pct: [f64; 2],
    pub time_bucket_edges_s: [u32; 2],
    pub slope_fast_down_pct_per_s: f64,
    pub slope_slow_down_pct_per_s: f64,
    pub slope_up_pct_per_s: f64,

    pub shadow_positions: u32,
    pub pilot_drawdown_min_pct: f64,
    pub pilot_requires_stress_low: bool,
    pub full_live_requires_positive_mean_delta: bool,
    pub full_live_tail_risk_max: f64,

    pub ledger_dir: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DerivedTimeWindows {
    pub outcome_horizon_ms: u64,
    pub reclaim_timeout_ms: u64,
    pub panic_freeze_start_ms: u64,
    pub panic_freeze_end_ms: u64,
}

impl AemConfig {
    pub fn validate(&self) -> Result<(), AemError> {
        if self.t_s == 0 {
            return Err(AemError::InvalidConfig("t_s must be > 0".to_string()));
        }
        if !self.k_ci.is_finite() || self.k_ci <= 0.0 {
            return Err(AemError::InvalidConfig(
                "k_ci must be finite and > 0".to_string(),
            ));
        }
        if self.n_min_per_key < 2 {
            return Err(AemError::InvalidConfig(
                "n_min_per_key must be >= 2".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.tail_risk_limit_wait) {
            return Err(AemError::InvalidConfig(
                "tail_risk_limit_wait must be in [0,1]".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.full_live_tail_risk_max) {
            return Err(AemError::InvalidConfig(
                "full_live_tail_risk_max must be in [0,1]".to_string(),
            ));
        }
        let dw = self.derived_time_windows();
        if dw.panic_freeze_end_ms > dw.reclaim_timeout_ms {
            return Err(AemError::InvalidConfig(
                "panic_freeze_end_ms must be <= reclaim_timeout_ms".to_string(),
            ));
        }
        Ok(())
    }

    pub fn derived_time_windows(&self) -> DerivedTimeWindows {
        let outcome_horizon_ms = self.t_s.saturating_mul(1_000);
        let reclaim_timeout_ms = outcome_horizon_ms.saturating_mul(3) / 4;
        let panic_freeze_start_ms = ((outcome_horizon_ms as f64) * 0.17).round() as u64;
        let panic_freeze_end_ms = outcome_horizon_ms / 2;
        DerivedTimeWindows {
            outcome_horizon_ms,
            reclaim_timeout_ms,
            panic_freeze_start_ms,
            panic_freeze_end_ms,
        }
    }

    pub fn decay_lambda(&self) -> f64 {
        let half_life = self.decay_half_life_days.max(0.001);
        std::f64::consts::LN_2 / half_life
    }
}

impl Default for AemConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            t_s: 120,
            min_stabilization_ticks: 2,
            n_min_per_key: 20,
            min_edge: 0.02,
            k_ci: 1.28,
            tail_risk_limit_wait: 0.20,
            oracle_stale_hard_ms: 1500,
            partial_fraction_bps: 5000,
            ci_compare_against_sell_now: true,
            ci_compare_against_partial: true,
            decay_half_life_days: 7.0,
            replay_window_days: 14,
            replay_max_events: 200_000,
            stress_low_requeue_max: 1,
            stress_med_requeue_min: 2,
            stress_med_requeue_max: 4,
            stress_high_requeue_min: 5,
            stress_low_send_fail_max: 0,
            stress_med_send_fail_eq: 1,
            stress_high_send_fail_min: 2,
            stress_low_relax_max: 0,
            stress_med_relax_eq: 1,
            stress_high_relax_min: 2,
            drawdown_bucket_edges_pct: [20.0, 40.0],
            time_bucket_edges_s: [30, 120],
            slope_fast_down_pct_per_s: -0.8,
            slope_slow_down_pct_per_s: -0.15,
            slope_up_pct_per_s: 0.15,
            shadow_positions: 200,
            pilot_drawdown_min_pct: 40.0,
            pilot_requires_stress_low: true,
            full_live_requires_positive_mean_delta: true,
            full_live_tail_risk_max: 0.20,
            ledger_dir: "datasets/decisions/aem".to_string(),
        }
    }
}
