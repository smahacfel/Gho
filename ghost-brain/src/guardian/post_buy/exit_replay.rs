//! Compact shadow exit path replay evidence.
//!
//! This module is deliberately decision-neutral. It records enough post-entry
//! price-path evidence to evaluate many target/stop pairs offline, without
//! changing the active shadow lifecycle or emitting raw tick-by-tick streams.

use std::collections::BTreeMap;

use serde::Serialize;
use solana_sdk::pubkey::Pubkey;

use super::config::ShadowExitReplayConfig;

pub const SHADOW_EXIT_REPLAY_SCHEMA: &str = "shadow_exit_replay_v1";
const QUALITY_CLEAN: &str = "clean";
const QUALITY_DEGRADED: &str = "degraded";
const QUALITY_UNAVAILABLE: &str = "unavailable";
const REASON_INVALID_ENTRY_PRICE: &str = "invalid_entry_price";
const REASON_NO_PRICE_PATH: &str = "no_price_path";
const REASON_PATH_TRUNCATED: &str = "path_truncated";
pub const REASON_SHUTDOWN_BEFORE_HORIZON: &str = "shutdown_before_horizon";

#[derive(Debug, Clone)]
pub struct ShadowExitReplayIdentity {
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    pub candidate_id: String,
    pub position_id: String,
    pub pool_id: String,
    pub base_mint: String,
    pub bonding_curve: Pubkey,
    pub entry_ts_ms: u64,
    pub entry_price: f64,
    pub entry_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowExitReplayRecord {
    pub schema: &'static str,
    pub run_id: String,
    pub session_id: String,
    pub candidate_id: String,
    pub position_id: String,
    pub pool_id: String,
    pub base_mint: String,
    pub entry_ts_ms: u64,
    pub entry_price: f64,
    pub entry_source: String,
    pub horizon_ms: u64,
    pub close_age_ms: u64,
    pub levels_bps: Vec<i32>,
    pub first_hit_ms: BTreeMap<String, u64>,
    pub mfe_bps: Option<i32>,
    pub mae_bps: Option<i32>,
    pub time_to_mfe_ms: Option<u64>,
    pub time_to_mae_ms: Option<u64>,
    pub last_pnl_bps: Option<i32>,
    pub pnl_step_bps: i32,
    pub heartbeat_ms: u64,
    pub max_path_points: usize,
    pub path_bps: Vec<(u64, i32)>,
    pub sample_count_seen: u64,
    pub path_points_written: usize,
    pub truncated: bool,
    pub quality: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShadowExitReplayTracker {
    identity: ShadowExitReplayIdentity,
    horizon_ms: u64,
    levels_bps: Vec<i32>,
    first_hit_ms: BTreeMap<String, u64>,
    mfe_bps: Option<i32>,
    mae_bps: Option<i32>,
    time_to_mfe_ms: Option<u64>,
    time_to_mae_ms: Option<u64>,
    last_pnl_bps: Option<i32>,
    last_sample_age_ms: Option<u64>,
    last_observed_sample: Option<(u64, i32)>,
    pnl_step_bps: i32,
    heartbeat_ms: u64,
    max_path_points: usize,
    path_bps: Vec<(u64, i32)>,
    sample_count_seen: u64,
    truncated: bool,
}

impl ShadowExitReplayTracker {
    pub fn new(identity: ShadowExitReplayIdentity, config: &ShadowExitReplayConfig) -> Self {
        let mut tracker = Self {
            identity,
            horizon_ms: config.horizon_ms(),
            levels_bps: config.sanitized_levels_bps(),
            first_hit_ms: BTreeMap::new(),
            mfe_bps: None,
            mae_bps: None,
            time_to_mfe_ms: None,
            time_to_mae_ms: None,
            last_pnl_bps: None,
            last_sample_age_ms: None,
            last_observed_sample: None,
            pnl_step_bps: config.pnl_step_bps(),
            heartbeat_ms: config.heartbeat_ms(),
            max_path_points: config.max_path_points(),
            path_bps: Vec::with_capacity(config.max_path_points().min(64)),
            sample_count_seen: 0,
            truncated: false,
        };

        if tracker.has_valid_entry_price() {
            tracker.mfe_bps = Some(0);
            tracker.mae_bps = Some(0);
            tracker.time_to_mfe_ms = Some(0);
            tracker.time_to_mae_ms = Some(0);
            tracker.last_pnl_bps = Some(0);
            tracker.last_sample_age_ms = Some(0);
            tracker.path_bps.push((0, 0));
        }

        tracker
    }

    pub fn has_valid_entry_price(&self) -> bool {
        self.identity.entry_price.is_finite() && self.identity.entry_price > 0.0
    }

    pub fn bonding_curve(&self) -> Pubkey {
        self.identity.bonding_curve
    }

    pub fn is_horizon_reached(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.identity.entry_ts_ms) >= self.horizon_ms
    }

    pub fn observe_price_sample(&mut self, sample_ts_ms: u64, price: f64) {
        if !self.has_valid_entry_price() || !price.is_finite() || price <= 0.0 {
            return;
        }
        if sample_ts_ms < self.identity.entry_ts_ms {
            return;
        }

        let age_ms = sample_ts_ms.saturating_sub(self.identity.entry_ts_ms);
        if age_ms > self.horizon_ms {
            return;
        }

        let pnl_bps = pnl_bps(self.identity.entry_price, price);
        if let Some((last_age_ms, last_pnl_bps)) = self.last_observed_sample {
            if age_ms < last_age_ms || (age_ms == last_age_ms && pnl_bps == last_pnl_bps) {
                return;
            }
        }
        self.last_observed_sample = Some((age_ms, pnl_bps));
        self.sample_count_seen = self.sample_count_seen.saturating_add(1);
        self.last_pnl_bps = Some(pnl_bps);
        self.last_sample_age_ms = Some(age_ms);

        if self.mfe_bps.map_or(true, |mfe| pnl_bps > mfe) {
            self.mfe_bps = Some(pnl_bps);
            self.time_to_mfe_ms = Some(age_ms);
        }
        if self.mae_bps.map_or(true, |mae| pnl_bps < mae) {
            self.mae_bps = Some(pnl_bps);
            self.time_to_mae_ms = Some(age_ms);
        }

        let hit_new_level = self.observe_first_hits(age_ms, pnl_bps);
        if self.should_write_path_point(age_ms, pnl_bps, hit_new_level) {
            self.write_path_point(age_ms, pnl_bps);
        }
    }

    pub fn finalize(mut self, now_ms: u64, forced_reason: Option<&str>) -> ShadowExitReplayRecord {
        let close_age_ms = now_ms
            .saturating_sub(self.identity.entry_ts_ms)
            .min(self.horizon_ms);

        let mut reason = forced_reason.map(str::to_string);
        let mut quality = QUALITY_CLEAN;

        if !self.has_valid_entry_price() {
            quality = QUALITY_UNAVAILABLE;
            reason = Some(REASON_INVALID_ENTRY_PRICE.to_string());
        } else if self.sample_count_seen == 0 {
            quality = QUALITY_UNAVAILABLE;
            reason = Some(REASON_NO_PRICE_PATH.to_string());
        } else {
            if let (Some(age_ms), Some(pnl_bps)) = (self.last_sample_age_ms, self.last_pnl_bps) {
                self.write_path_point(age_ms, pnl_bps);
            }

            if reason.is_some() || close_age_ms < self.horizon_ms || self.truncated {
                quality = QUALITY_DEGRADED;
                if reason.is_none() && self.truncated {
                    reason = Some(REASON_PATH_TRUNCATED.to_string());
                }
            }
        }

        ShadowExitReplayRecord {
            schema: SHADOW_EXIT_REPLAY_SCHEMA,
            run_id: self.identity.run_id.unwrap_or_default(),
            session_id: self.identity.session_id.unwrap_or_default(),
            candidate_id: self.identity.candidate_id,
            position_id: self.identity.position_id,
            pool_id: self.identity.pool_id,
            base_mint: self.identity.base_mint,
            entry_ts_ms: self.identity.entry_ts_ms,
            entry_price: self.identity.entry_price,
            entry_source: self.identity.entry_source,
            horizon_ms: self.horizon_ms,
            close_age_ms,
            levels_bps: self.levels_bps,
            first_hit_ms: self.first_hit_ms,
            mfe_bps: self.mfe_bps,
            mae_bps: self.mae_bps,
            time_to_mfe_ms: self.time_to_mfe_ms,
            time_to_mae_ms: self.time_to_mae_ms,
            last_pnl_bps: self.last_pnl_bps,
            pnl_step_bps: self.pnl_step_bps,
            heartbeat_ms: self.heartbeat_ms,
            max_path_points: self.max_path_points,
            path_points_written: self.path_bps.len(),
            path_bps: self.path_bps,
            sample_count_seen: self.sample_count_seen,
            truncated: self.truncated,
            quality,
            reason,
        }
    }

    fn observe_first_hits(&mut self, age_ms: u64, pnl_bps: i32) -> bool {
        let mut hit_new_level = false;
        for level_bps in &self.levels_bps {
            let hit = if *level_bps > 0 {
                pnl_bps >= *level_bps
            } else {
                pnl_bps <= *level_bps
            };
            if hit {
                let key = level_bps.to_string();
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    self.first_hit_ms.entry(key)
                {
                    entry.insert(age_ms);
                    hit_new_level = true;
                }
            }
        }
        hit_new_level
    }

    fn should_write_path_point(&self, age_ms: u64, pnl_bps: i32, hit_new_level: bool) -> bool {
        if hit_new_level {
            return true;
        }
        let Some((last_age_ms, last_pnl_bps)) = self.path_bps.last().copied() else {
            return true;
        };
        pnl_bps.abs_diff(last_pnl_bps) >= self.pnl_step_bps as u32
            || age_ms.saturating_sub(last_age_ms) >= self.heartbeat_ms
    }

    fn write_path_point(&mut self, age_ms: u64, pnl_bps: i32) {
        if self.path_bps.is_empty() {
            self.path_bps.push((age_ms, pnl_bps));
            return;
        }

        if let Some(last) = self.path_bps.last_mut() {
            if last.0 == age_ms {
                *last = (age_ms, pnl_bps);
                return;
            }
        }

        if self.path_bps.len() < self.max_path_points {
            self.path_bps.push((age_ms, pnl_bps));
            return;
        }

        self.truncated = true;
        if self.max_path_points >= 2 && self.path_bps.len() >= 2 {
            let final_index = self.path_bps.len() - 1;
            self.path_bps[final_index] = (age_ms, pnl_bps);
        }
    }
}

fn pnl_bps(entry_price: f64, price: f64) -> i32 {
    (((price / entry_price) - 1.0) * 10_000.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardian::post_buy::config::{
        DEFAULT_EXIT_REPLAY_LEVELS_BPS, DEFAULT_EXIT_REPLAY_MAX_PATH_POINTS,
    };

    fn identity(entry_price: f64) -> ShadowExitReplayIdentity {
        ShadowExitReplayIdentity {
            run_id: Some("run-test".to_string()),
            session_id: Some("session-test".to_string()),
            candidate_id: "candidate-test".to_string(),
            position_id: "position-test".to_string(),
            pool_id: Pubkey::new_unique().to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            bonding_curve: Pubkey::new_unique(),
            entry_ts_ms: 1_000,
            entry_price,
            entry_source: "shadow_simulated".to_string(),
        }
    }

    fn config() -> ShadowExitReplayConfig {
        ShadowExitReplayConfig {
            enabled: true,
            horizon_ms: 120_000,
            pnl_step_bps: 25,
            heartbeat_ms: 1_000,
            max_path_points: DEFAULT_EXIT_REPLAY_MAX_PATH_POINTS,
            levels_bps: DEFAULT_EXIT_REPLAY_LEVELS_BPS.to_vec(),
            flush_on_shutdown: false,
            shutdown_flush_budget_ms: 3_000,
        }
    }

    #[test]
    fn first_hits_preserve_positive_then_negative_order() {
        let cfg = config();
        let mut tracker = ShadowExitReplayTracker::new(identity(1.0), &cfg);
        tracker.observe_price_sample(1_500, 1.05);
        tracker.observe_price_sample(2_000, 0.97);

        let record = tracker.finalize(121_000, None);
        for level in [100, 200, 300, 400, 500] {
            assert_eq!(record.first_hit_ms.get(&level.to_string()), Some(&500));
        }
        for level in [-100, -200, -300] {
            assert_eq!(record.first_hit_ms.get(&level.to_string()), Some(&1_000));
        }
        assert!(
            record.first_hit_ms["500"] < record.first_hit_ms["-300"],
            "positive levels must be hit before negative levels"
        );
        assert_eq!(record.quality, QUALITY_CLEAN);
    }

    #[test]
    fn path_is_capped_and_final_sample_is_preserved() {
        let mut cfg = config();
        cfg.max_path_points = 4;
        cfg.pnl_step_bps = 1;
        let mut tracker = ShadowExitReplayTracker::new(identity(1.0), &cfg);

        for idx in 1..20 {
            tracker.observe_price_sample(1_000 + idx * 100, 1.0 + (idx as f64 * 0.001));
        }

        let record = tracker.finalize(121_000, None);
        assert!(record.truncated);
        assert_eq!(record.path_points_written, 4);
        assert!(record.path_bps.len() <= 4);
        assert_eq!(
            record.path_bps.last(),
            Some(&(1_000 + 19 * 100 - 1_000, 190))
        );
        assert_eq!(record.reason.as_deref(), Some(REASON_PATH_TRUNCATED));
    }

    #[test]
    fn invalid_entry_price_is_unavailable() {
        let cfg = config();
        let tracker = ShadowExitReplayTracker::new(identity(0.0), &cfg);
        let record = tracker.finalize(1_000, None);
        assert_eq!(record.quality, QUALITY_UNAVAILABLE);
        assert_eq!(record.reason.as_deref(), Some(REASON_INVALID_ENTRY_PRICE));
        assert_eq!(record.sample_count_seen, 0);
    }

    #[test]
    fn no_price_path_is_unavailable_without_guessing_hits() {
        let cfg = config();
        let tracker = ShadowExitReplayTracker::new(identity(1.0), &cfg);
        let record = tracker.finalize(121_000, None);
        assert_eq!(record.quality, QUALITY_UNAVAILABLE);
        assert_eq!(record.reason.as_deref(), Some(REASON_NO_PRICE_PATH));
        assert!(record.first_hit_ms.is_empty());
        assert_eq!(record.path_bps, vec![(0, 0)]);
    }
}
