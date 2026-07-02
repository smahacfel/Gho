//! MonitoringEngine — Real-time tick-based position monitor.
//!
//! Runs as a `tokio::spawn` task, ticking at a configurable interval.
//! Each tick evaluates all 4 lightweight modules (LIGMA, WHF, TCF, PANIC)
//! against each tracked position, using data from ShadowLedger.
//!
//! Design invariants:
//! - Zero RPC calls on the hot path (all data comes from ShadowLedger).
//! - No allocations in the steady-state hot loop (pre-allocated buffers).
//! - Total tick time for 10 positions < 5ms.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::CanonicalPoolState;
#[cfg(test)]
use ghost_core::shadow_ledger::types::PriceReason;
use ghost_core::shadow_ledger::types::PriceState;
use ghost_core::shadow_ledger::{MarketSnapshot, ShadowLedger};

use crate::aem::{
    AemLedgerWriter, AemRuntime, JsonlAemLedger, ManagementDecisionEvent, ManagementOutcomeEvent,
    OutcomeFeatureSource, OutcomeSample, ReclaimFlag, RevolverAemAdapter, StateFeatures,
    StressBucket, TriggerControlAdapter,
};
use crate::events::{
    CloseReason, ControlCommandAppliedPayload, ControlCommandIssuedPayload, EventEmitter,
    EventKind, ExecutionEvent, ExecutionStressChangedPayload, ExitFilledPayload,
    ExitSubmittedPayload, OracleStalePayload, PositionClosedPayload, PositionOpenedPayload,
};
use crate::execution::backend::{
    CommandId as ExecCommandId, ExecutionStressSnapshot as ExecStressSnapshot,
    FillStatus as ExecFillStatus, Lane, StressBucket as ExecStressBucket,
};
use crate::execution::shadow::ShadowBackend;
use crate::oracle::tcf::field::TrendCohesionField;
use crate::oracle::tcf::observation::MarketObservation;
use trigger::{
    PriceTruthEvidence, PriceTruthResolver, PriceTruthSource, PriceTruthStatus,
    ShadowExitPriceSample, ShadowExitTruth,
};

#[cfg(test)]
use super::config::DEFAULT_WAIT_FOR_TIMESTOP_MS;
use super::config::{PostBuyGuardianConfig, TimeStopV2Config, TimeStopV2Mode};
use super::exit_replay::{
    ShadowExitReplayIdentity, ShadowExitReplayRecord, ShadowExitReplayTracker,
    REASON_SHUTDOWN_BEFORE_HORIZON,
};
#[cfg(test)]
use super::integration::SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS;
use super::integration::{PositionRuntimeRouter, ShadowPositionBookAemAdapter};
#[cfg(test)]
use super::shadow_v2::ShadowV2ValidationHarnessConfig;
use super::shadow_v2::{
    ClockDomain, ClockedTimestamp, EventOrderComponent, EventOrderKey, MeasurementGrade,
    PoolStateSampleV2, ShadowExitAttemptV2, ShadowExitFillV2, ShadowPathSampleV2,
    ShadowPathSamplingModeV2, ShadowPathSamplingReasonV2, ShadowTerminalTruthV2, ShadowV2Envelope,
    ShadowV2Record, ShadowV2ValidationEvidenceStatus, ShadowV2ValidationHarness, SimulationLevel,
    TemporalClass, TerminalReasonV2, SHADOW_V2_EXIT_FILL_MODEL_VERSION,
};

#[cfg(test)]
const SHADOW_POSITION_TIME_STOP_MS: u64 = DEFAULT_WAIT_FOR_TIMESTOP_MS;
const SHADOW_EXIT_TRACE_FORMULA_ID: &str = "bonding_curve.calculate_sell_price.v1";
const SHADOW_TIME_STOP_STALE_SOURCE_PATH: &str = "guardian.post_buy.shadow_time_stop_stale";
const SHADOW_LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;
const SHADOW_TOKEN_DECIMAL_FACTOR_F64: f64 = 1_000_000.0;
use super::signals::*;

#[derive(Debug, Clone, Copy)]
struct ShadowSimpleExitThresholds {
    take_profit_pct: f64,
    stop_loss_pct: f64,
}

impl ShadowSimpleExitThresholds {
    fn new(take_profit_pct: f64, stop_loss_pct: f64) -> Self {
        Self {
            take_profit_pct: sanitize_shadow_target_threshold_pct(take_profit_pct),
            stop_loss_pct: sanitize_shadow_stoploss_threshold_pct(stop_loss_pct),
        }
    }

    fn prices_for_entry(self, entry_price_sol: f64) -> Option<(f64, f64)> {
        if !entry_price_sol.is_finite() || entry_price_sol <= 0.0 {
            return None;
        }

        let upper = entry_price_sol * (1.0 + self.take_profit_pct);
        let lower = entry_price_sol * (1.0 - self.stop_loss_pct);
        (upper.is_finite() && lower.is_finite() && upper > 0.0 && lower >= 0.0)
            .then_some((upper, lower))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShadowSimpleExitTrigger {
    TakeProfit,
    StopLoss,
    TimeStop,
}

impl ShadowSimpleExitTrigger {
    const fn as_label(self) -> &'static str {
        match self {
            Self::TakeProfit => "take_profit",
            Self::StopLoss => "stop_loss",
            Self::TimeStop => "time_stop",
        }
    }

    const fn reason_code(self) -> &'static str {
        match self {
            Self::TakeProfit => "target",
            Self::StopLoss => "stop_loss",
            Self::TimeStop => "time_stop",
        }
    }
}

fn sanitize_shadow_target_threshold_pct(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn sanitize_shadow_stoploss_threshold_pct(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[derive(Debug, Default)]
struct NoopAemLedgerWriter;

impl AemLedgerWriter for NoopAemLedgerWriter {
    fn append_decision(
        &self,
        _event: &ManagementDecisionEvent,
    ) -> Result<(), crate::aem::AemError> {
        Ok(())
    }

    fn append_outcome(&self, _event: &ManagementOutcomeEvent) -> Result<(), crate::aem::AemError> {
        Ok(())
    }

    fn append_time_index(
        &self,
        _idx: &crate::aem::TimeIndexRecord,
    ) -> Result<(), crate::aem::AemError> {
        Ok(())
    }

    fn append_regime_index(
        &self,
        _idx: &crate::aem::RegimeIndexRecord,
    ) -> Result<(), crate::aem::AemError> {
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Per-position tracking state
// ═══════════════════════════════════════════════════════════════════════

#[derive(Debug, Default, Clone)]
struct SnapshotTimeline {
    snapshots: Vec<MarketSnapshot>,
    cumulative_volume_sol: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ShadowMarketActivityAnchor {
    last_seen_ms: u64,
    snapshot_ts_ms: u64,
    slot: Option<u64>,
    tx_count: u64,
}

impl ShadowMarketActivityAnchor {
    fn from_registration(now_ms: u64, snapshot: Option<&MarketSnapshot>) -> Self {
        Self {
            last_seen_ms: now_ms,
            snapshot_ts_ms: snapshot.map(|snapshot| snapshot.timestamp_ms).unwrap_or(0),
            slot: snapshot.and_then(|snapshot| snapshot.slot),
            tx_count: snapshot.map(|snapshot| snapshot.tx_count).unwrap_or(0),
        }
    }

    fn observe_snapshot(&mut self, snapshot: &MarketSnapshot, now_ms: u64) -> bool {
        let is_newer_slot = match (self.slot, snapshot.slot) {
            (Some(previous), Some(current)) => current > previous,
            (None, Some(_)) => true,
            _ => false,
        };
        let is_newer_snapshot =
            snapshot.timestamp_ms > self.snapshot_ts_ms || snapshot.tx_count > self.tx_count;
        if !is_newer_slot && !is_newer_snapshot {
            return false;
        }
        self.last_seen_ms = now_ms;
        self.snapshot_ts_ms = snapshot.timestamp_ms;
        self.slot = snapshot.slot;
        self.tx_count = snapshot.tx_count;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TimeStopV2WindowStatus {
    Alive,
    Weak,
    Heartbeat,
    StaleOrInsufficient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TimeStopV2Subreason {
    AliveMeaningfulProgress,
    LowVitalityNoMeaningfulProgress,
    MicroTxHeartbeatNoPriceProgress,
    StaleOrMissingMarketSample,
    MissingMarketSample,
    InvalidMarketSample,
    NoNewMarketSample,
    MixedFailedVitalityWindows,
}

#[derive(Debug, Clone, Copy)]
struct TimeStopV2Checkpoint {
    slot: Option<u64>,
    timestamp_ms: u64,
    price_sol_per_token: f64,
    price_state: PriceState,
    market_cap_sol: f64,
    bonding_progress_pct: f64,
    tx_count: u64,
    cum_volume_sol: f64,
}

impl TimeStopV2Checkpoint {
    fn from_snapshot(snapshot: &MarketSnapshot) -> Self {
        Self {
            slot: snapshot.slot,
            timestamp_ms: snapshot.timestamp_ms,
            price_sol_per_token: snapshot.price_sol_per_token,
            price_state: snapshot.price_state,
            market_cap_sol: snapshot.market_cap_sol,
            bonding_progress_pct: snapshot.bonding_progress_pct,
            tx_count: snapshot.tx_count,
            cum_volume_sol: snapshot.cum_volume_sol,
        }
    }

    fn is_newer_than(self, previous: Self) -> bool {
        match (previous.slot, self.slot) {
            (Some(previous_slot), Some(current_slot)) if current_slot > previous_slot => true,
            (None, Some(_)) => true,
            _ => self.timestamp_ms > previous.timestamp_ms || self.tx_count > previous.tx_count,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct TimeStopV2State {
    next_window_index: u32,
    last_checkpoint: Option<TimeStopV2Checkpoint>,
    failed_windows: u32,
    failed_subreason: Option<TimeStopV2Subreason>,
    last_status: Option<TimeStopV2WindowStatus>,
    last_subreason: Option<TimeStopV2Subreason>,
    candidate_emitted: bool,
    candidate_ts_ms: Option<u64>,
    candidate_subreason: Option<TimeStopV2Subreason>,
}

#[derive(Debug, Clone)]
struct TimeStopV2Evaluation {
    mode: TimeStopV2Mode,
    window_index: u32,
    scheduled_check_ms: u64,
    position_age_ms: u64,
    status: TimeStopV2WindowStatus,
    subreason: TimeStopV2Subreason,
    failed_windows: u32,
    candidate: bool,
    candidate_ts_ms: Option<u64>,
    candidate_subreason: Option<TimeStopV2Subreason>,
    price_delta_pct_window: Option<f64>,
    price_delta_pct_from_entry: Option<f64>,
    mcap_delta_pct_window: Option<f64>,
    bonding_delta_pct_window: Option<f64>,
    tx_delta_window: Option<u64>,
    volume_delta_sol_window: Option<f64>,
    avg_volume_per_tx_sol_window: Option<f64>,
    checkpoint_slot: Option<u64>,
    latest_slot: Option<u64>,
    checkpoint_timestamp_ms: Option<u64>,
    latest_timestamp_ms: Option<u64>,
}

impl TimeStopV2State {
    fn from_registration(snapshot: Option<&MarketSnapshot>) -> Self {
        Self {
            last_checkpoint: snapshot.map(TimeStopV2Checkpoint::from_snapshot),
            ..Self::default()
        }
    }

    fn has_observed(&self) -> bool {
        self.next_window_index > 0 || self.last_status.is_some() || self.candidate_emitted
    }

    fn next_scheduled_check_ms(&self, entry_unix_ms: u64, cfg: &TimeStopV2Config) -> u64 {
        entry_unix_ms
            .saturating_add(cfg.first_check_ms())
            .saturating_add(self.next_window_index as u64 * cfg.window_ms())
    }

    fn evaluate(
        &mut self,
        cfg: &TimeStopV2Config,
        entry_unix_ms: u64,
        entry_price_sol: Option<f64>,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) -> Option<TimeStopV2Evaluation> {
        let scheduled_check_ms = self.next_scheduled_check_ms(entry_unix_ms, cfg);
        if now_ms < scheduled_check_ms {
            return None;
        }

        let window_index = self.next_window_index;
        let position_age_ms = now_ms.saturating_sub(entry_unix_ms);
        let previous_checkpoint = self.last_checkpoint;
        let latest_checkpoint = latest.map(TimeStopV2Checkpoint::from_snapshot);
        let fresh_latest = match (previous_checkpoint, latest_checkpoint) {
            (Some(previous), Some(current)) => current.is_newer_than(previous),
            (None, Some(_)) => false,
            _ => false,
        };

        let mut price_delta_pct_window = None;
        let mut price_delta_pct_from_entry = None;
        let mut mcap_delta_pct_window = None;
        let mut bonding_delta_pct_window = None;
        let mut tx_delta_window = None;
        let mut volume_delta_sol_window = None;
        let mut avg_volume_per_tx_sol_window = None;

        let (status, subreason) = if let (Some(previous), Some(current)) =
            (previous_checkpoint, latest_checkpoint)
        {
            let price_delta_pct =
                pct_delta(current.price_sol_per_token, previous.price_sol_per_token);
            let mcap_delta_pct = pct_delta(current.market_cap_sol, previous.market_cap_sol);
            let bonding_delta_pct = current.bonding_progress_pct - previous.bonding_progress_pct;
            let tx_delta = current.tx_count.saturating_sub(previous.tx_count);
            let volume_delta_sol = (current.cum_volume_sol - previous.cum_volume_sol).max(0.0);
            let avg_volume_per_tx_sol =
                (tx_delta > 0).then_some(volume_delta_sol / tx_delta as f64);
            let price_from_entry_pct = entry_price_sol
                .filter(|price| price.is_finite() && *price > 0.0)
                .map(|entry_price| pct_delta(current.price_sol_per_token, entry_price));

            price_delta_pct_window = Some(price_delta_pct);
            price_delta_pct_from_entry = price_from_entry_pct;
            mcap_delta_pct_window = Some(mcap_delta_pct);
            bonding_delta_pct_window = Some(bonding_delta_pct);
            tx_delta_window = Some(tx_delta);
            volume_delta_sol_window = Some(volume_delta_sol);
            avg_volume_per_tx_sol_window = avg_volume_per_tx_sol;

            if !current.price_sol_per_token.is_finite()
                || current.price_sol_per_token <= 0.0
                || !current.market_cap_sol.is_finite()
                || current.market_cap_sol <= 0.0
                || !current.price_state.is_valid()
            {
                (
                    TimeStopV2WindowStatus::StaleOrInsufficient,
                    TimeStopV2Subreason::InvalidMarketSample,
                )
            } else if !fresh_latest {
                (
                    TimeStopV2WindowStatus::Weak,
                    TimeStopV2Subreason::NoNewMarketSample,
                )
            } else {
                let meaningful_progress = price_delta_pct >= cfg.min_price_delta_pct_alive
                    || mcap_delta_pct >= cfg.min_mcap_delta_pct_alive
                    || bonding_delta_pct >= cfg.min_bonding_delta_pct_alive
                    || (volume_delta_sol >= cfg.min_volume_delta_sol_alive
                        && price_delta_pct >= cfg.min_price_delta_pct_for_volume_alive);

                if meaningful_progress {
                    (
                        TimeStopV2WindowStatus::Alive,
                        TimeStopV2Subreason::AliveMeaningfulProgress,
                    )
                } else {
                    let heartbeat_like = tx_delta >= cfg.min_tx_delta_for_heartbeat
                        && avg_volume_per_tx_sol
                            .map(|avg| avg <= cfg.max_avg_volume_per_tx_sol_heartbeat)
                            .unwrap_or(false)
                        && price_delta_pct.abs() <= cfg.max_abs_price_delta_pct_heartbeat
                        && mcap_delta_pct.abs() <= cfg.max_abs_mcap_delta_pct_heartbeat
                        && bonding_delta_pct.abs() <= cfg.max_bonding_delta_pct_heartbeat;
                    if heartbeat_like {
                        (
                            TimeStopV2WindowStatus::Heartbeat,
                            TimeStopV2Subreason::MicroTxHeartbeatNoPriceProgress,
                        )
                    } else {
                        (
                            TimeStopV2WindowStatus::Weak,
                            TimeStopV2Subreason::LowVitalityNoMeaningfulProgress,
                        )
                    }
                }
            }
        } else {
            (
                TimeStopV2WindowStatus::StaleOrInsufficient,
                if latest_checkpoint.is_some() {
                    TimeStopV2Subreason::StaleOrMissingMarketSample
                } else {
                    TimeStopV2Subreason::MissingMarketSample
                },
            )
        };

        if matches!(status, TimeStopV2WindowStatus::Alive) {
            self.failed_windows = 0;
            self.failed_subreason = None;
        } else {
            self.failed_windows = self.failed_windows.saturating_add(1);
            self.failed_subreason = match self.failed_subreason {
                None => Some(subreason),
                Some(previous) if previous == subreason => Some(previous),
                Some(_) => Some(TimeStopV2Subreason::MixedFailedVitalityWindows),
            };
        }

        if !self.candidate_emitted
            && self.failed_windows >= cfg.failed_windows_to_signal()
            && position_age_ms >= cfg.min_age_before_signal_ms()
        {
            self.candidate_emitted = true;
            self.candidate_ts_ms = Some(now_ms);
            self.candidate_subreason = self.failed_subreason;
        }

        self.last_status = Some(status);
        self.last_subreason = Some(subreason);
        if latest_checkpoint.is_some() && (fresh_latest || previous_checkpoint.is_none()) {
            self.last_checkpoint = latest_checkpoint;
        }
        self.next_window_index = self.next_window_index.saturating_add(1);

        Some(TimeStopV2Evaluation {
            mode: cfg.mode,
            window_index,
            scheduled_check_ms,
            position_age_ms,
            status,
            subreason,
            failed_windows: self.failed_windows,
            candidate: self.candidate_emitted,
            candidate_ts_ms: self.candidate_ts_ms,
            candidate_subreason: self.candidate_subreason,
            price_delta_pct_window,
            price_delta_pct_from_entry,
            mcap_delta_pct_window,
            bonding_delta_pct_window,
            tx_delta_window,
            volume_delta_sol_window,
            avg_volume_per_tx_sol_window,
            checkpoint_slot: previous_checkpoint.and_then(|checkpoint| checkpoint.slot),
            latest_slot: latest_checkpoint.and_then(|checkpoint| checkpoint.slot),
            checkpoint_timestamp_ms: previous_checkpoint.map(|checkpoint| checkpoint.timestamp_ms),
            latest_timestamp_ms: latest_checkpoint.map(|checkpoint| checkpoint.timestamp_ms),
        })
    }
}

fn pct_delta(current: f64, previous: f64) -> f64 {
    if current.is_finite() && previous.is_finite() && previous.abs() > f64::EPSILON {
        ((current - previous) / previous) * 100.0
    } else {
        0.0
    }
}

impl SnapshotTimeline {
    fn latest(&self) -> Option<&MarketSnapshot> {
        self.snapshots.last()
    }

    fn clone_snapshots(&self) -> Vec<MarketSnapshot> {
        self.snapshots.clone()
    }

    fn replace_with(
        &mut self,
        snapshots: Vec<MarketSnapshot>,
        max_snapshots: usize,
        retention_ms: u64,
    ) {
        self.snapshots = snapshots;
        self.cumulative_volume_sol = self
            .snapshots
            .last()
            .map(|snapshot| snapshot.cum_volume_sol)
            .unwrap_or(0.0);
        self.trim(max_snapshots, retention_ms);
    }

    fn ingest_canonical_state(
        &mut self,
        state: &CanonicalPoolState,
        max_snapshots: usize,
        retention_ms: u64,
    ) -> &MarketSnapshot {
        let previous = self.latest().cloned();
        let snapshot = Self::materialize_canonical_snapshot(
            state,
            previous.as_ref(),
            self.cumulative_volume_sol,
        );
        let should_append = previous
            .as_ref()
            .map_or(true, |last| !Self::equivalent(last, &snapshot));
        if should_append {
            self.cumulative_volume_sol = snapshot.cum_volume_sol;
            self.snapshots.push(snapshot);
            self.trim(max_snapshots, retention_ms);
        }
        self.latest()
            .expect("snapshot timeline must contain latest after canonical ingest")
    }

    fn materialize_canonical_snapshot(
        state: &CanonicalPoolState,
        previous: Option<&MarketSnapshot>,
        previous_cumulative_volume_sol: f64,
    ) -> MarketSnapshot {
        let reserve_quote_sol = state.virtual_sol_reserves as f64 / SHADOW_LAMPORTS_PER_SOL_F64;
        let reserve_base_raw = state.virtual_token_reserves as f64;
        let reserve_base_tokens = reserve_base_raw / SHADOW_TOKEN_DECIMAL_FACTOR_F64;
        let price_sol_per_token = if state.price_sol.is_finite() && state.price_sol > 0.0 {
            state.price_sol
        } else if reserve_quote_sol.is_finite()
            && reserve_quote_sol > 0.0
            && reserve_base_tokens.is_finite()
            && reserve_base_tokens > 0.0
        {
            reserve_quote_sol / reserve_base_tokens
        } else {
            0.0
        };
        let (price_state, price_reason) = PriceState::from_price(price_sol_per_token);

        let mut cum_volume_sol = previous_cumulative_volume_sol.max(0.0);
        let mut d_price_d_volume = 0.0;
        let mut d_price_d_liquidity = 0.0;
        if let Some(prev) = previous {
            let delta_quote_sol = (reserve_quote_sol - prev.reserve_quote).abs();
            if delta_quote_sol.is_finite() {
                cum_volume_sol = prev.cum_volume_sol + delta_quote_sol;
            } else {
                cum_volume_sol = prev.cum_volume_sol;
            }

            let delta_price = price_sol_per_token - prev.price_sol_per_token;
            let delta_volume_sol = (cum_volume_sol - prev.cum_volume_sol).abs();
            if delta_volume_sol > 1e-12 {
                d_price_d_volume = delta_price / delta_volume_sol;
            }

            let delta_liquidity = reserve_quote_sol - prev.reserve_quote;
            if delta_liquidity.abs() > 1e-12 {
                d_price_d_liquidity = delta_price / delta_liquidity;
            }
        }

        MarketSnapshot {
            slot: (state.last_update_slot > 0).then_some(state.last_update_slot),
            tx_key: None,
            timestamp_ms: state.last_update_ts_ms,
            cum_volume_sol,
            tx_count: state.update_count,
            unique_addrs: previous.map(|snap| snap.unique_addrs).unwrap_or(1),
            price_sol_per_token,
            price_state,
            price_reason,
            market_cap_sol: state.market_cap_sol,
            reserve_base: reserve_base_raw,
            reserve_quote: reserve_quote_sol,
            bonding_progress_pct: state.bonding_curve_progress * 100.0,
            d_price_d_volume,
            d_price_d_liquidity,
            d_price_d_slippage: 0.0,
        }
    }

    fn equivalent(lhs: &MarketSnapshot, rhs: &MarketSnapshot) -> bool {
        lhs.slot == rhs.slot
            && lhs.timestamp_ms == rhs.timestamp_ms
            && lhs.tx_count == rhs.tx_count
            && (lhs.price_sol_per_token - rhs.price_sol_per_token).abs() <= 1e-12
            && (lhs.market_cap_sol - rhs.market_cap_sol).abs() <= 1e-12
            && (lhs.reserve_base - rhs.reserve_base).abs() <= 1e-6
            && (lhs.reserve_quote - rhs.reserve_quote).abs() <= 1e-12
    }

    fn trim(&mut self, max_snapshots: usize, retention_ms: u64) {
        if max_snapshots > 0 && self.snapshots.len() > max_snapshots {
            let excess = self.snapshots.len() - max_snapshots;
            self.snapshots.drain(..excess);
        }

        if retention_ms > 0 && self.snapshots.len() > 1 {
            if let Some(latest_ts) = self.snapshots.last().map(|snapshot| snapshot.timestamp_ms) {
                let cutoff_ts = latest_ts.saturating_sub(retention_ms);
                let first_retained = self
                    .snapshots
                    .iter()
                    .position(|snapshot| snapshot.timestamp_ms >= cutoff_ts)
                    .unwrap_or_else(|| self.snapshots.len().saturating_sub(1));
                if first_retained > 0 {
                    self.snapshots.drain(..first_retained);
                }
            }
        }

        self.cumulative_volume_sol = self
            .snapshots
            .last()
            .map(|snapshot| snapshot.cum_volume_sol)
            .unwrap_or(0.0);
    }
}

/// Internal state tracked per monitored position.
#[allow(dead_code)] // Fields stored for telemetry/diagnostics, not all read on hot path
struct MonitoredPosition {
    candidate_id: String,
    lane: Lane,
    pool_amm_id: Pubkey,
    base_mint: Pubkey,
    #[allow(dead_code)]
    bonding_curve: Pubkey,
    entry_time: Instant,
    entry_unix_ms: u64,
    entry_price_sol: Option<f64>,
    entry_size_lamports: u64,
    entry_token_amount_raw: u64,
    remaining_token_amount_raw: u64,
    position_id: String,
    position_epoch: u64,
    join_metadata: PositionJoinMetadata,
    entry_order_id: String,
    quote_id: String,
    slot: Option<u64>,
    peak_since_entry: f64,
    last_peak_unix_ms: u64,
    aem_registered: bool,
    runtime_registered: bool,
    last_stress_bucket: Option<StressBucket>,

    // ── TCF state (per-position instance) ───────────────────────────
    tcf: TrendCohesionField,
    consecutive_low_cohesion: u32,
    last_tcf_score: f64,

    // ── LIGMA state ─────────────────────────────────────────────────
    last_tradability: f32,

    // ── Signal history (ring buffer for aggregation window) ─────────
    recent_signals: Vec<TimestampedSignal>,
    entry_value_sol: f64,
    realized_exit_value_sol: f64,
    estimated_costs_sol: f64,
    realized_pnl_sol: f64,
    realized_pnl_pct: f64,
    total_exits: u32,
    remaining_fraction_bps: u16,
    last_close_reason: Option<CloseReason>,
    last_force_exit_reason_code: Option<String>,
    last_price_truth: Option<PriceTruthEvidence>,
    last_blocked_truth_status: Option<PriceTruthStatus>,
    last_blocked_truth_timestamp_ms: Option<u64>,
    last_snapshot_source: PriceTruthSource,
    last_shadow_snapshot: Option<MarketSnapshot>,
    shadow_market_activity: ShadowMarketActivityAnchor,
    time_stop_v2: TimeStopV2State,
    snapshot_timeline: SnapshotTimeline,
}

/// Signal with its emission timestamp, for aggregation window management.
struct TimestampedSignal {
    timestamp_ms: u64,
    signal: GuardianSignal,
}

/// Registration context passed from the execution lane to keep IDs consistent.
#[derive(Debug, Clone)]
pub struct PositionEventContext {
    pub join_metadata: PositionJoinMetadata,
    pub candidate_id: String,
    pub entry_order_id: String,
    pub quote_id: String,
    pub slot: Option<u64>,
    pub lane: Lane,
    pub position_id: Option<String>,
    pub position_epoch: Option<u64>,
    pub opened_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PositionJoinMetadata {
    pub ab_record_id: Option<String>,
    pub source_ab_record_id: Option<String>,
    pub probe_id: Option<String>,
    pub dispatch_source: Option<String>,
    pub collection_plane: Option<String>,
    pub probe_plane: Option<String>,
    pub v3_feature_snapshot_hash: Option<String>,
    pub v3_policy_config_hash: Option<String>,
    pub decision_plane: Option<String>,
    pub rollout_namespace: Option<String>,
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    pub brain_config_path: Option<String>,
    pub brain_config_hash: Option<String>,
    pub entry_simulation_rpc_slot: Option<u64>,
    pub entry_market_anchor_slot: Option<u64>,
    pub entry_market_anchor_tx_signature: Option<String>,
    pub entry_market_anchor_source: Option<String>,
    pub entry_landed_slot: Option<u64>,
    pub entry_landed_slot_source: Option<String>,
}

/// Minimal position identity returned after successful registration.
#[derive(Debug, Clone)]
pub struct RegisteredPosition {
    pub position_id: String,
    pub position_epoch: u64,
    pub opened_at_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ShadowLifecycleRecordType {
    ExitFilled,
    ExitBlocked,
    PositionClosed,
    TimeStopV2Window,
}

#[derive(Debug, Serialize)]
struct ShadowLifecycleRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    ab_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_ab_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    probe_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dispatch_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    collection_plane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    probe_plane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    v3_feature_snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    v3_policy_config_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_plane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rollout_namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    brain_config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    brain_config_hash: Option<String>,
    record_type: ShadowLifecycleRecordType,
    timestamp: String,
    timestamp_ms: u64,
    candidate_id: String,
    pool_id: String,
    mint_id: String,
    position_id: String,
    position_epoch: u64,
    lane: Lane,
    entry_order_id: String,
    quote_id: String,
    entry_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_simulation_rpc_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_market_anchor_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_market_anchor_tx_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_market_anchor_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_landed_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_landed_slot_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fraction_bps: Option<u16>,
    remaining_fraction_bps: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_value_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_value_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_token_amount_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gross_pnl_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    net_pnl_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    estimated_costs_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_pnl: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_pnl_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    close_reason: Option<CloseReason>,
    total_exits: u32,
    truth_source: trigger::PriceTruthSource,
    truth_status: PriceTruthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    truth_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_sample_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_market_anchor_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_market_anchor_tx_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_market_anchor_source: Option<trigger::PriceTruthSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_reason_evaluation_ts_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_landed_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_landed_slot_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_timestamp_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_price_state: Option<ghost_core::shadow_ledger::types::PriceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_price_reason: Option<ghost_core::shadow_ledger::types::PriceReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_mode: Option<TimeStopV2Mode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_window_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_scheduled_check_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_position_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_status: Option<TimeStopV2WindowStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_subreason: Option<TimeStopV2Subreason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_failed_windows: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_candidate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_candidate_ts_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_candidate_subreason: Option<TimeStopV2Subreason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_price_delta_pct_window: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_price_delta_pct_from_entry: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_mcap_delta_pct_window: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_bonding_delta_pct_window: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_tx_delta_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_volume_delta_sol_window: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_avg_volume_per_tx_sol_window: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_checkpoint_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_latest_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_checkpoint_timestamp_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_stop_v2_latest_timestamp_ms: Option<u64>,
}

impl ShadowLifecycleRecord {
    fn entry_timestamp_ms(&self) -> u64 {
        self.sample_timestamp_ms
            .and_then(|sample_ts_ms| {
                self.sample_age_ms
                    .map(|age_ms| sample_ts_ms.saturating_sub(age_ms))
            })
            .or_else(|| {
                self.duration_ms
                    .map(|duration_ms| self.timestamp_ms.saturating_sub(duration_ms))
            })
            .unwrap_or(self.timestamp_ms)
    }
}

fn shadow_lifecycle_record_type_label(record_type: ShadowLifecycleRecordType) -> &'static str {
    match record_type {
        ShadowLifecycleRecordType::ExitFilled => "exit_filled",
        ShadowLifecycleRecordType::ExitBlocked => "exit_blocked",
        ShadowLifecycleRecordType::PositionClosed => "position_closed",
        ShadowLifecycleRecordType::TimeStopV2Window => "time_stop_v2_window",
    }
}

fn shadow_v2_event_order_key(
    slot: Option<u64>,
    signature: Option<&str>,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    EventOrderKey {
        slot: slot
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        block_time: EventOrderComponent::unknown(),
        signature: signature
            .filter(|signature| !signature.trim().is_empty())
            .map(|signature| EventOrderComponent::known(signature.to_string()))
            .unwrap_or_else(EventOrderComponent::unknown),
        transaction_index_or_unknown: EventOrderComponent::unknown(),
        instruction_index_or_unknown: EventOrderComponent::unknown(),
        inner_instruction_index_or_unknown: EventOrderComponent::unknown(),
        log_index_or_unknown: EventOrderComponent::unknown(),
        event_seq_in_process,
        observed_at_wall_ms,
    }
}

fn shadow_v2_event_seq(timestamp_ms: u64, offset: u64) -> u64 {
    timestamp_ms.saturating_mul(10).saturating_add(offset)
}

fn shadow_v2_exit_trigger_label(record: &ShadowLifecycleRecord) -> String {
    match record.close_reason {
        Some(CloseReason::Target) => "TARGET".to_string(),
        Some(CloseReason::StopLoss) => "STOP".to_string(),
        Some(CloseReason::TimeStop) => "TIMEOUT".to_string(),
        Some(CloseReason::Panic) => "PANIC".to_string(),
        Some(CloseReason::Manual) => "MANUAL".to_string(),
        Some(CloseReason::HardSafety) => "HARD_SAFETY".to_string(),
        Some(CloseReason::Default) => "DEFAULT_CLOSE".to_string(),
        None => shadow_lifecycle_record_type_label(record.record_type)
            .to_ascii_uppercase()
            .replace('-', "_"),
    }
}

fn shadow_v2_terminal_reason(close_reason: Option<CloseReason>) -> TerminalReasonV2 {
    match close_reason {
        Some(CloseReason::Target) => TerminalReasonV2::Target,
        Some(CloseReason::StopLoss | CloseReason::Panic | CloseReason::HardSafety) => {
            TerminalReasonV2::Stop
        }
        Some(CloseReason::TimeStop) => TerminalReasonV2::Timeout,
        Some(CloseReason::Manual) => TerminalReasonV2::ManualDiagnosticClose,
        Some(CloseReason::Default) | None => TerminalReasonV2::Unknown,
    }
}

fn shadow_v2_pnl_bps_from_lifecycle(record: &ShadowLifecycleRecord) -> Option<i32> {
    record
        .final_pnl_pct
        .filter(|value| value.is_finite())
        .map(|pct| (pct * 100.0).round() as i32)
        .or_else(|| {
            let entry_price = record.entry_price?;
            let exit_price = record.exit_price?;
            if !entry_price.is_finite()
                || !exit_price.is_finite()
                || entry_price <= 0.0
                || exit_price <= 0.0
            {
                return None;
            }
            Some((((exit_price - entry_price) / entry_price) * 10_000.0).round() as i32)
        })
}

// ═══════════════════════════════════════════════════════════════════════
// MonitoringEngine
// ═══════════════════════════════════════════════════════════════════════

/// The monitoring engine runs as a tokio task, ticking at configured interval.
///
/// # Thread safety
/// - `positions` behind `RwLock` — read-heavy, write-rare.
/// - `signal_tx` is `mpsc::Sender` (clone-safe).
/// - `shadow_ledger` is `Arc<ShadowLedger>` (shared across system).
pub struct MonitoringEngine {
    config: PostBuyGuardianConfig,
    shadow_ledger: Arc<ShadowLedger>,
    account_state_core: Option<Arc<AccountStateReducer>>,
    shadow_simple_exit_thresholds: Option<ShadowSimpleExitThresholds>,
    positions: Arc<RwLock<HashMap<Pubkey, MonitoredPosition>>>,
    signal_tx: mpsc::Sender<GuardianSignal>,
    /// Optional lane-aware position-management router.
    position_router: Option<Arc<PositionRuntimeRouter>>,
    /// Optional ShadowBackend handle so closed shadow positions stop counting
    /// toward the synthetic concurrency budget.
    shadow_backend: Arc<RwLock<Option<Arc<ShadowBackend>>>>,
    /// Optional AEM runtime.
    aem_runtime: Option<Arc<parking_lot::Mutex<AemRuntime>>>,
    /// Optional AEM ledger.
    aem_ledger: Option<Arc<JsonlAemLedger>>,
    /// Optional execution event emitter used by Etap 7 instrumentation hooks.
    event_emitter: Option<Arc<EventEmitter>>,
    /// Optional secondary emitter (dual mode mirror lane).
    event_emitter_secondary: Option<Arc<EventEmitter>>,
    /// Canonical shadow lifecycle/PnL proof log.
    shadow_lifecycle_log_path: Option<PathBuf>,
    /// Compact research-only exit replay sidecar log.
    shadow_exit_replay_log_path: Option<PathBuf>,
    /// Optional Shadow V2 validation harness. Logging-only evidence sink; never consumed by policy.
    shadow_v2_validation_harness: Option<Arc<Mutex<ShadowV2ValidationHarness>>>,
    /// Passive replay trackers keyed by mint; independent from active position lifecycle.
    exit_replay_trackers: Arc<RwLock<HashMap<Pubkey, Vec<ShadowExitReplayTracker>>>>,
}

impl MonitoringEngine {
    /// Create a new MonitoringEngine.
    ///
    /// # Arguments
    /// - `config` — Guardian-specific thresholds and intervals.
    /// - `shadow_ledger` — Shared ShadowLedger for market data.
    /// - `signal_tx` — Channel sender for emitting GuardianSignals.
    pub fn new(
        config: PostBuyGuardianConfig,
        shadow_ledger: Arc<ShadowLedger>,
        signal_tx: mpsc::Sender<GuardianSignal>,
    ) -> Self {
        Self {
            config,
            shadow_ledger,
            account_state_core: None,
            shadow_simple_exit_thresholds: None,
            positions: Arc::new(RwLock::new(HashMap::new())),
            signal_tx,
            position_router: None,
            shadow_backend: Arc::new(RwLock::new(None)),
            aem_runtime: None,
            aem_ledger: None,
            event_emitter: None,
            event_emitter_secondary: None,
            shadow_lifecycle_log_path: None,
            shadow_exit_replay_log_path: None,
            shadow_v2_validation_harness: None,
            exit_replay_trackers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Attach the lane-aware position-management router shared with SignalRouter/AEM.
    pub fn set_position_router(&mut self, position_router: Arc<PositionRuntimeRouter>) {
        self.position_router = Some(position_router);
    }

    pub fn set_account_state_core(&mut self, account_state_core: Arc<AccountStateReducer>) {
        self.account_state_core = Some(account_state_core);
    }

    pub fn set_shadow_simple_exit_thresholds(&mut self, take_profit_pct: f64, stop_loss_pct: f64) {
        self.shadow_simple_exit_thresholds = Some(ShadowSimpleExitThresholds::new(
            take_profit_pct,
            stop_loss_pct,
        ));
    }

    pub async fn wait_for_canonical_snapshot(
        &self,
        base_mint: &Pubkey,
        min_slot: Option<u64>,
        max_wait: Duration,
        poll_interval: Duration,
    ) -> bool {
        if self.account_state_core.is_none() {
            return true;
        }

        let deadline = Instant::now() + max_wait;
        loop {
            if let Some(canonical_state) = self.current_canonical_state(base_mint) {
                let slot_ready = min_slot
                    .map(|slot| canonical_state.last_update_slot >= slot)
                    .unwrap_or(true);
                if slot_ready {
                    return true;
                }
            }

            if Instant::now() >= deadline {
                return false;
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Attach the ShadowBackend used by canonical synthetic settlement.
    pub fn attach_shadow_backend(&self, shadow_backend: Arc<ShadowBackend>) {
        *self.shadow_backend.write() = Some(shadow_backend);
    }

    /// Attach execution event emitter for Etap 7 lifecycle instrumentation.
    pub fn set_event_emitter(&mut self, event_emitter: Arc<EventEmitter>) {
        self.event_emitter = Some(event_emitter);
    }

    /// Attach secondary emitter for dual-mode mirrored lane events.
    pub fn set_secondary_event_emitter(&mut self, event_emitter: Arc<EventEmitter>) {
        self.event_emitter_secondary = Some(event_emitter);
    }

    pub fn set_shadow_lifecycle_log_path(&mut self, shadow_lifecycle_log_path: Option<PathBuf>) {
        self.shadow_lifecycle_log_path = shadow_lifecycle_log_path;
    }

    pub fn set_shadow_exit_replay_log_path(
        &mut self,
        shadow_exit_replay_log_path: Option<PathBuf>,
    ) {
        self.shadow_exit_replay_log_path = shadow_exit_replay_log_path;
    }

    pub fn set_shadow_v2_validation_harness(
        &mut self,
        shadow_v2_validation_harness: Arc<Mutex<ShadowV2ValidationHarness>>,
    ) {
        self.shadow_v2_validation_harness = Some(shadow_v2_validation_harness);
    }

    pub fn set_aem(
        &mut self,
        runtime: Arc<parking_lot::Mutex<AemRuntime>>,
        ledger: Arc<JsonlAemLedger>,
    ) {
        self.aem_runtime = Some(runtime);
        self.aem_ledger = Some(ledger);
    }

    fn default_candidate_id(pool_amm_id: Pubkey, base_mint: Pubkey, now_ms: u64) -> String {
        format!("{}_{}_{}", base_mint, pool_amm_id, now_ms)
    }

    fn emit_position_opened(
        &self,
        candidate_id: &str,
        position_id: &str,
        position_epoch: u64,
        entry_order_id: &str,
        quote_id: &str,
        slot: Option<u64>,
        entry_price_sol: Option<f64>,
        opened_at_ms: u64,
        size_tokens: u64,
        size_sol: u64,
    ) {
        let Some(emitter) = self.event_emitter.as_ref() else {
            return;
        };
        let mut env = emitter.make_envelope_at(&candidate_id.to_string(), opened_at_ms);
        env.position_id = Some(position_id.to_string());
        env.position_epoch = Some(position_epoch);
        env.order_id = Some(entry_order_id.to_string());
        env.quote_id = Some(quote_id.to_string());
        env.slot = slot;
        emitter.emit_raw(ExecutionEvent::new(
            env,
            EventKind::PositionOpened(PositionOpenedPayload {
                entry_price: entry_price_sol.unwrap_or(0.0),
                entry_time_ms: opened_at_ms,
                epoch_id: position_epoch,
                size_tokens,
                size_sol,
            }),
        ));
    }

    fn shadow_exit_stale_after_ms(&self) -> u64 {
        self.config
            .aem
            .oracle_stale_hard_ms
            .max(self.config.tick_interval_ms)
            .max(1)
    }

    fn snapshot_history_retention_ms(&self) -> u64 {
        self.config
            .panic_rate_window_ms
            .max(self.config.signal_aggregation_window_ms.saturating_mul(2))
            .max(self.config.aem.derived_time_windows().outcome_horizon_ms)
            .max(self.shadow_position_time_stop_ms())
            .saturating_add(self.config.tick_interval_ms.saturating_mul(2))
    }

    fn snapshot_history_max_snapshots(&self) -> usize {
        let tick_ms = self.config.tick_interval_ms.max(1);
        let retention = self.snapshot_history_retention_ms().max(tick_ms);
        retention
            .saturating_div(tick_ms)
            .saturating_add(8)
            .min(2_048) as usize
    }

    fn shadow_position_time_stop_ms(&self) -> u64 {
        self.config.wait_for_timestop_ms()
    }

    fn default_snapshot_source(&self) -> PriceTruthSource {
        if self.account_state_core.is_some() {
            PriceTruthSource::CanonicalAccountStateSnapshot
        } else {
            PriceTruthSource::ShadowLedgerSnapshot
        }
    }

    fn snapshot_source_for_position(&self, base_mint: &Pubkey) -> PriceTruthSource {
        self.positions
            .read()
            .get(base_mint)
            .map(|pos| pos.last_snapshot_source)
            .unwrap_or_else(|| self.default_snapshot_source())
    }

    fn remember_shadow_snapshot(&self, base_mint: &Pubkey, snapshot: &MarketSnapshot) {
        let snapshot_source = self.snapshot_source_for_position(base_mint);
        let mut positions = self.positions.write();
        if let Some(pos) = positions.get_mut(base_mint) {
            if matches!(pos.lane, Lane::Shadow) {
                pos.last_shadow_snapshot = Some(snapshot.clone());
                pos.last_snapshot_source = snapshot_source;
            }
        }
    }

    fn note_shadow_market_activity(
        &self,
        base_mint: &Pubkey,
        snapshot: &MarketSnapshot,
        now_ms: u64,
    ) -> bool {
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(base_mint) else {
            return false;
        };
        if !matches!(pos.lane, Lane::Shadow) {
            return false;
        }
        pos.shadow_market_activity
            .observe_snapshot(snapshot, now_ms)
    }

    async fn refresh_shadow_time_stop_anchor(&self, base_mint: &Pubkey) {
        let Some(router) = self.position_router.as_ref() else {
            return;
        };
        let Some(shadow_book) = router.shadow_book() else {
            return;
        };
        let _ = shadow_book
            .write()
            .await
            .refresh_time_stop_anchor(base_mint);
    }

    fn current_canonical_state(&self, base_mint: &Pubkey) -> Option<CanonicalPoolState> {
        self.account_state_core
            .as_ref()
            .and_then(|account_state_core| account_state_core.get_canonical_state(base_mint))
    }

    fn current_shadow_curve_snapshot(&self, base_mint: &Pubkey) -> Option<MarketSnapshot> {
        self.current_shadow_curve_snapshot_with_curve(base_mint, None)
    }

    fn current_shadow_curve_snapshot_with_curve(
        &self,
        base_mint: &Pubkey,
        bonding_curve_override: Option<Pubkey>,
    ) -> Option<MarketSnapshot> {
        if let Some(canonical_state) = self.current_canonical_state(base_mint) {
            return Some(SnapshotTimeline::materialize_canonical_snapshot(
                &canonical_state,
                None,
                0.0,
            ));
        }

        if self.account_state_core.is_some() {
            return None;
        }

        self.legacy_shadow_curve_snapshot_with_curve(base_mint, bonding_curve_override)
    }

    fn current_runtime_shadow_snapshot(
        &self,
        base_mint: &Pubkey,
        observed_at_ms: u64,
    ) -> Option<MarketSnapshot> {
        self.current_runtime_shadow_snapshot_with_curve(base_mint, observed_at_ms, None)
    }

    fn current_runtime_shadow_snapshot_with_curve(
        &self,
        base_mint: &Pubkey,
        observed_at_ms: u64,
        bonding_curve_override: Option<Pubkey>,
    ) -> Option<MarketSnapshot> {
        let mut snapshot =
            self.current_shadow_curve_snapshot_with_curve(base_mint, bonding_curve_override)?;
        let Some(account_state_core) = self.account_state_core.as_ref() else {
            return Some(snapshot);
        };
        let Some(snapshot_slot) = snapshot.slot else {
            return Some(snapshot);
        };
        let Some(latest_observed_slot) = account_state_core.latest_observed_slot() else {
            return Some(snapshot);
        };

        // History modules keep the original write timestamp, but runtime exit truth may use the
        // same canonical state as "current" once AccountStateCore has already advanced beyond the
        // pool's last write. That proves the stream is still progressing after this state and lets
        // TimeStop close quiet pools without reviving any cached/avg fallback.
        if latest_observed_slot > snapshot_slot {
            debug!(
                %base_mint,
                snapshot_slot,
                latest_observed_slot,
                state_age_ms = observed_at_ms.saturating_sub(snapshot.timestamp_ms),
                "PostBuyGuardian: using currently observed canonical state for shadow runtime"
            );
            snapshot.timestamp_ms = observed_at_ms;
        }

        Some(snapshot)
    }

    fn legacy_shadow_curve_snapshot(&self, base_mint: &Pubkey) -> Option<MarketSnapshot> {
        self.legacy_shadow_curve_snapshot_with_curve(base_mint, None)
    }

    fn legacy_shadow_curve_snapshot_with_curve(
        &self,
        base_mint: &Pubkey,
        bonding_curve_override: Option<Pubkey>,
    ) -> Option<MarketSnapshot> {
        let position_bonding_curve = {
            let positions = self.positions.read();
            positions.get(base_mint).map(|pos| pos.bonding_curve)
        };
        let curve_key = bonding_curve_override
            .or(position_bonding_curve)
            .or_else(|| self.shadow_ledger.resolve_curve_key(base_mint))
            .unwrap_or(*base_mint);
        let curve_state = self.shadow_ledger.get_old(&curve_key).or_else(|| {
            if curve_key != *base_mint {
                self.shadow_ledger.get_old(base_mint)
            } else {
                None
            }
        })?;
        let mut snapshot =
            MarketSnapshot::from_curve_genesis(&curve_state.curve, curve_state.last_update_ts_ms);
        snapshot.slot =
            (curve_state.last_updated_slot > 0).then_some(curve_state.last_updated_slot);
        Some(snapshot)
    }

    fn refresh_snapshot_timeline_from_canonical(
        &self,
        base_mint: &Pubkey,
    ) -> Option<Vec<MarketSnapshot>> {
        let canonical_state = self.current_canonical_state(base_mint)?;
        let retention_ms = self.snapshot_history_retention_ms();
        let max_snapshots = self.snapshot_history_max_snapshots();
        let mut positions = self.positions.write();
        let pos = positions.get_mut(base_mint)?;
        let latest = pos
            .snapshot_timeline
            .ingest_canonical_state(&canonical_state, max_snapshots, retention_ms)
            .clone();
        if matches!(pos.lane, Lane::Shadow) {
            pos.last_shadow_snapshot = Some(latest);
            pos.last_snapshot_source = PriceTruthSource::CanonicalAccountStateSnapshot;
        }
        Some(pos.snapshot_timeline.clone_snapshots())
    }

    fn refresh_snapshot_timeline_from_legacy(
        &self,
        base_mint: &Pubkey,
    ) -> Option<Vec<MarketSnapshot>> {
        let snapshots = match self.shadow_ledger.get_snapshots(base_mint) {
            Some(snapshots) if !snapshots.is_empty() => snapshots,
            _ => self
                .legacy_shadow_curve_snapshot(base_mint)
                .map(|snapshot| vec![snapshot])?,
        };
        let retention_ms = self.snapshot_history_retention_ms();
        let max_snapshots = self.snapshot_history_max_snapshots();
        let mut positions = self.positions.write();
        let pos = positions.get_mut(base_mint)?;
        pos.snapshot_timeline
            .replace_with(snapshots, max_snapshots, retention_ms);
        if let Some(latest) = pos.snapshot_timeline.latest().cloned() {
            if matches!(pos.lane, Lane::Shadow) {
                pos.last_shadow_snapshot = Some(latest);
                pos.last_snapshot_source = PriceTruthSource::ShadowLedgerSnapshot;
            }
        }
        Some(pos.snapshot_timeline.clone_snapshots())
    }

    fn snapshots_for_tick(&self, base_mint: &Pubkey) -> Option<Vec<MarketSnapshot>> {
        if self.account_state_core.is_some() {
            self.refresh_snapshot_timeline_from_canonical(base_mint)
        } else {
            self.refresh_snapshot_timeline_from_legacy(base_mint)
        }
    }

    fn remember_shadow_time_stop_reason(&self, base_mint: &Pubkey) {
        let mut positions = self.positions.write();
        if let Some(pos) = positions.get_mut(base_mint) {
            if pos.last_force_exit_reason_code.is_none() {
                pos.last_force_exit_reason_code = Some("time_stop".to_string());
            }
        }
    }

    fn resolve_shadow_exit_sample_for_runtime(
        snapshot: &MarketSnapshot,
        now_ms: u64,
        stale_after_ms: u64,
        source: PriceTruthSource,
    ) -> Result<ShadowExitPriceSample, trigger::PriceTruthError> {
        PriceTruthResolver::resolve_shadow_exit_sample_with_source(
            snapshot,
            now_ms,
            stale_after_ms,
            source,
        )
    }

    fn shadow_snapshot_trace_id(snapshot: &MarketSnapshot) -> String {
        let slot = snapshot
            .slot
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string());
        format!("slot={slot}:timestamp_ms={}", snapshot.timestamp_ms)
    }

    fn stale_time_stop_rejection_evidence(
        position_id: &str,
        snapshot: &MarketSnapshot,
        now_ms: u64,
        exit_token_amount_raw: u64,
        evidence: &PriceTruthEvidence,
    ) -> PriceTruthEvidence {
        let oracle_spot_price = PriceTruthResolver::normalize_shadow_snapshot_price_sol(snapshot);
        let computed_exit_price = PriceTruthResolver::resolve_shadow_exit_sample_with_source(
            snapshot,
            now_ms,
            0,
            evidence.source,
        )
        .ok()
        .and_then(|sample| {
            let exit_qty_tokens = exit_token_amount_raw as f64 / SHADOW_TOKEN_DECIMAL_FACTOR_F64;
            if exit_qty_tokens <= 0.0 {
                return None;
            }
            let exit_value_sol = sample.curve.calculate_sell_price(exit_token_amount_raw) as f64
                / SHADOW_LAMPORTS_PER_SOL_F64;
            let exit_price_sol = exit_value_sol / exit_qty_tokens;
            (exit_price_sol.is_finite() && exit_price_sol > 0.0).then_some(exit_price_sol)
        });
        let snapshot_id = Self::shadow_snapshot_trace_id(snapshot);
        warn!(
            position_id = %position_id,
            truth_status = "stale",
            sample_slot = ?snapshot.slot,
            oracle_spot_price = oracle_spot_price.unwrap_or(0.0),
            reserve_in = snapshot.reserve_base,
            reserve_out = snapshot.reserve_quote,
            exit_qty = exit_token_amount_raw,
            computed_exit_price = computed_exit_price.unwrap_or(0.0),
            formula_id = SHADOW_EXIT_TRACE_FORMULA_ID,
            snapshot_id = %snapshot_id,
            source_path = SHADOW_TIME_STOP_STALE_SOURCE_PATH,
            "PostBuyGuardian: stale time-stop trace"
        );

        let mut blocked_evidence = evidence.clone();
        let mut detail = blocked_evidence
            .detail
            .clone()
            .unwrap_or_else(|| "stale shadow exit sample".to_string());
        detail.push_str("; stale time-stop rejected without emitting fill");
        detail.push_str(&format!("; formula_id={SHADOW_EXIT_TRACE_FORMULA_ID}"));
        detail.push_str(&format!("; snapshot_id={snapshot_id}"));
        detail.push_str(&format!(
            "; source_path={SHADOW_TIME_STOP_STALE_SOURCE_PATH}"
        ));
        detail.push_str(&format!("; reserve_in={}", snapshot.reserve_base));
        detail.push_str(&format!("; reserve_out={}", snapshot.reserve_quote));
        detail.push_str(&format!("; exit_qty={exit_token_amount_raw}"));
        if let Some(oracle_spot_price) = oracle_spot_price {
            detail.push_str(&format!("; oracle_spot_price={oracle_spot_price}"));
        }
        if let Some(computed_exit_price) = computed_exit_price {
            detail.push_str(&format!("; computed_exit_price={computed_exit_price}"));
        }

        let semantic_violation = match (oracle_spot_price, computed_exit_price) {
            (Some(oracle_spot_price), Some(computed_exit_price))
                if oracle_spot_price.is_finite()
                    && oracle_spot_price > 0.0
                    && computed_exit_price
                        > oracle_spot_price + (oracle_spot_price.abs() * 1e-9 + 1e-15) =>
            {
                true
            }
            _ => false,
        };
        if semantic_violation {
            detail.push_str("; semantic_violation=exit_fill_above_oracle_spot");
            blocked_evidence.status = PriceTruthStatus::SemanticViolation;
        }
        blocked_evidence.detail = Some(detail);
        blocked_evidence
    }

    async fn force_close_shadow_without_exit_truth(
        &self,
        base_mint: &Pubkey,
        position_id: &str,
        now_ms: u64,
        evidence: PriceTruthEvidence,
    ) {
        self.maybe_record_shadow_exit_blocked(base_mint, now_ms, 10_000, &evidence);

        {
            let mut positions = self.positions.write();
            let Some(pos) = positions.get_mut(base_mint) else {
                return;
            };
            pos.last_force_exit_reason_code = Some("time_stop".to_string());
            pos.last_close_reason = Some(CloseReason::TimeStop);
            pos.last_price_truth = Some(evidence);
        }

        if let Some(router) = self.position_router.as_ref() {
            if let Some(shadow_book) = router.shadow_book() {
                let _ = shadow_book.write().await.remove_position(position_id);
            }
        }
        let shadow_backend = { self.shadow_backend.read().clone() };
        if let Some(shadow_backend) = shadow_backend {
            let _ = shadow_backend.unregister_position(position_id).await;
        }

        warn!(
            position_id = %position_id,
            "PostBuyGuardian: forcing shadow time-stop close without resolved exit truth"
        );
        self.unregister_position(base_mint);
    }

    fn append_jsonl_record(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.flush()
    }

    fn append_shadow_lifecycle_record(&self, record: &ShadowLifecycleRecord) {
        let Some(path) = self.shadow_lifecycle_log_path.as_deref() else {
            self.append_shadow_v2_lifecycle_record(record);
            return;
        };
        if let Err(error) = Self::append_jsonl_record(path, record) {
            error!(
                path = %path.display(),
                position_id = %record.position_id,
                error = %error,
                "PostBuyGuardian: failed to append shadow lifecycle proof"
            );
        }
        self.append_shadow_v2_lifecycle_record(record);
    }

    fn append_shadow_v2_lifecycle_record(&self, record: &ShadowLifecycleRecord) {
        let Some(harness) = self.shadow_v2_validation_harness.as_ref() else {
            return;
        };
        if !matches!(record.lane, Lane::Shadow) {
            return;
        }

        let mut records = Vec::new();
        if matches!(
            record.record_type,
            ShadowLifecycleRecordType::ExitFilled
                | ShadowLifecycleRecordType::ExitBlocked
                | ShadowLifecycleRecordType::PositionClosed
        ) {
            records.push(ShadowV2Record::ShadowPathSampleV2(
                self.shadow_v2_path_sample_from_lifecycle(record),
            ));
        }

        if matches!(
            record.record_type,
            ShadowLifecycleRecordType::ExitFilled | ShadowLifecycleRecordType::ExitBlocked
        ) {
            records.push(ShadowV2Record::ShadowExitAttemptV2(
                self.shadow_v2_exit_attempt_from_lifecycle(record),
            ));
            let exit_pool_state = self.shadow_v2_exit_pool_state_sample_from_lifecycle(record);
            if let Some(pool_state) = exit_pool_state.as_ref() {
                records.push(ShadowV2Record::PoolStateSampleV2(pool_state.clone()));
            }
            records.push(ShadowV2Record::ShadowExitFillV2(
                self.shadow_v2_exit_fill_from_lifecycle(record, exit_pool_state.as_ref()),
            ));
        }

        if matches!(
            record.record_type,
            ShadowLifecycleRecordType::PositionClosed
        ) {
            if record.total_exits == 0 {
                records.push(ShadowV2Record::ShadowExitAttemptV2(
                    self.shadow_v2_exit_attempt_from_lifecycle(record),
                ));
                let exit_pool_state = self.shadow_v2_exit_pool_state_sample_from_lifecycle(record);
                if let Some(pool_state) = exit_pool_state.as_ref() {
                    records.push(ShadowV2Record::PoolStateSampleV2(pool_state.clone()));
                }
                records.push(ShadowV2Record::ShadowExitFillV2(
                    self.shadow_v2_exit_fill_from_lifecycle(record, exit_pool_state.as_ref()),
                ));
            }
            records.push(ShadowV2Record::ShadowTerminalTruthV2(
                self.shadow_v2_terminal_truth_from_lifecycle(record),
            ));
        }

        let mut harness = harness.lock();
        for record in records {
            let event_id = record.envelope().event_id.clone();
            let outcome = harness.append_record(record);
            if outcome.validation_evidence_status == ShadowV2ValidationEvidenceStatus::Complete {
                debug!(
                    position_id = %event_id,
                    "PostBuyGuardian: Shadow V2 lifecycle evidence emitted"
                );
            } else {
                warn!(
                    event_id = %event_id,
                    status = ?outcome.validation_evidence_status,
                    canonical_write = ?outcome.canonical_write,
                    replay_write = ?outcome.replay_write,
                    lifecycle_write = ?outcome.lifecycle_write,
                    density_write = ?outcome.density_write,
                    "PostBuyGuardian: Shadow V2 lifecycle evidence append incomplete"
                );
            }
        }
    }

    fn shadow_v2_path_sample_from_lifecycle(
        &self,
        record: &ShadowLifecycleRecord,
    ) -> ShadowPathSampleV2 {
        let event_id = format!(
            "shadow_v2_path_sample:{}:{}:{}",
            record.position_id,
            record.timestamp_ms,
            shadow_lifecycle_record_type_label(record.record_type)
        );
        let mut envelope = self.shadow_v2_lifecycle_envelope(
            record,
            "shadow_path_sample_v2",
            event_id,
            TemporalClass::PostEntry,
            ClockDomain::StreamObservedMs,
        );
        envelope.source_refs.push(format!(
            "shadow_lifecycle:{}",
            shadow_lifecycle_record_type_label(record.record_type)
        ));
        let sample_ts = record.sample_timestamp_ms.unwrap_or(record.timestamp_ms);
        let age_ms = record
            .sample_timestamp_ms
            .map(|sample_ts_ms| sample_ts_ms.saturating_sub(record.entry_timestamp_ms()))
            .or(record.sample_age_ms)
            .or(record.duration_ms)
            .unwrap_or_default();
        let pnl_mark_bps = shadow_v2_pnl_bps_from_lifecycle(record);
        let mut limitations = Vec::new();
        if record.sample_timestamp_ms.is_none() {
            limitations.push("PATH_SAMPLE_TIMESTAMP_MISSING_USED_RECORD_TIMESTAMP".to_string());
        }
        if record.exit_price.is_none() {
            limitations.push("PATH_SAMPLE_EXIT_PRICE_MISSING".to_string());
        }
        if record.sample_price_state.is_none() {
            limitations.push("PATH_SAMPLE_PRICE_STATE_MISSING".to_string());
        }

        ShadowPathSampleV2::from_legacy_lifecycle_mark(
            envelope,
            shadow_v2_event_order_key(
                record.sample_slot.or(record.exit_sample_slot),
                record.exit_market_anchor_tx_signature.as_deref(),
                shadow_v2_event_seq(record.timestamp_ms, 1),
                sample_ts,
            ),
            ClockedTimestamp {
                field_name: "sample_ts_ms".to_string(),
                value: Some(sample_ts as i64),
                clock_domain: ClockDomain::StreamObservedMs,
                clock_source: "shadow_lifecycle.price_truth_evidence".to_string(),
                causal_boundary: "POST_ENTRY_MONITORING_SAMPLE".to_string(),
            },
            record.sample_slot.or(record.exit_sample_slot),
            age_ms,
            record.exit_price,
            pnl_mark_bps,
            ShadowPathSamplingModeV2::Standard120s,
            if matches!(
                record.record_type,
                ShadowLifecycleRecordType::PositionClosed
            ) {
                ShadowPathSamplingReasonV2::Terminal
            } else {
                ShadowPathSamplingReasonV2::EventSample
            },
            format!("{:?}", record.truth_status),
            limitations,
        )
    }

    fn shadow_v2_exit_attempt_from_lifecycle(
        &self,
        record: &ShadowLifecycleRecord,
    ) -> ShadowExitAttemptV2 {
        let trigger_ts = record
            .exit_reason_evaluation_ts_ms
            .or(record.sample_timestamp_ms)
            .unwrap_or(record.timestamp_ms);
        let exit_trigger = shadow_v2_exit_trigger_label(record);
        let mut envelope = self.shadow_v2_lifecycle_envelope(
            record,
            "shadow_exit_attempt_v2",
            format!(
                "shadow_v2_exit_attempt:{}:{}:{}",
                record.position_id, trigger_ts, exit_trigger
            ),
            TemporalClass::PostEntry,
            ClockDomain::StreamObservedMs,
        );
        envelope.source_refs.push(format!(
            "shadow_lifecycle:{}",
            shadow_lifecycle_record_type_label(record.record_type)
        ));
        if matches!(record.record_type, ShadowLifecycleRecordType::ExitBlocked) {
            envelope
                .limitations
                .push("EXIT_ATTEMPT_LEGACY_LIFECYCLE_BLOCKED".to_string());
        }

        let mut attempt = ShadowExitAttemptV2::from_mark_path_trigger(
            envelope,
            shadow_v2_event_order_key(
                record.exit_sample_slot.or(record.sample_slot),
                record.exit_market_anchor_tx_signature.as_deref(),
                shadow_v2_event_seq(trigger_ts, 2),
                trigger_ts,
            ),
            exit_trigger,
            ClockedTimestamp {
                field_name: "trigger_ts_ms".to_string(),
                value: Some(trigger_ts as i64),
                clock_domain: ClockDomain::StreamObservedMs,
                clock_source: "shadow_lifecycle.exit_reason_evaluation_ts_ms".to_string(),
                causal_boundary: "POST_ENTRY_EXIT_TRIGGER".to_string(),
            },
            record.exit_sample_slot.or(record.sample_slot),
            format!("{:?}", record.truth_source),
            self.shadow_simple_exit_thresholds
                .map(|thresholds| (thresholds.take_profit_pct * 100.0).round() as i32),
            self.shadow_simple_exit_thresholds
                .map(|thresholds| -((thresholds.stop_loss_pct * 100.0).round() as i32)),
            Some(self.config.wait_for_timestop_ms()),
            false,
            Some("BLOCK_AMBIGUOUS".to_string()),
        );
        attempt.attach_static_exit_model(SHADOW_V2_EXIT_FILL_MODEL_VERSION);
        attempt
    }

    fn shadow_v2_exit_pool_state_sample_from_lifecycle(
        &self,
        record: &ShadowLifecycleRecord,
    ) -> Option<PoolStateSampleV2> {
        let base_mint = record.mint_id.parse::<Pubkey>().ok()?;
        let state = self.current_canonical_state(&base_mint)?;
        let sample_ts = record
            .exit_reason_evaluation_ts_ms
            .or(record.sample_timestamp_ms)
            .unwrap_or(record.timestamp_ms);
        let mut envelope = self.shadow_v2_lifecycle_envelope(
            record,
            "pool_state_sample_v2",
            format!(
                "shadow_v2_pool_state_exit_before:{}:{}:{}",
                record.position_id,
                sample_ts,
                shadow_lifecycle_record_type_label(record.record_type)
            ),
            TemporalClass::PostEntry,
            ClockDomain::StreamObservedMs,
        );
        envelope.source_refs.push(format!(
            "shadow_lifecycle:{}",
            shadow_lifecycle_record_type_label(record.record_type)
        ));
        envelope
            .source_refs
            .push("account_state_core:get_canonical_state".to_string());
        envelope
            .limitations
            .push("POOL_STATE_SAMPLE_FROM_ACCOUNT_STATE_CORE_WITHOUT_RAW_ACCOUNT_HASH".to_string());
        envelope
            .limitations
            .push("POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME".to_string());
        envelope
            .limitations
            .push("TOKEN_DECIMALS_ASSUMED_PUMPFUN_6".to_string());

        let mut sample = PoolStateSampleV2::from_account_state_core(
            envelope,
            shadow_v2_event_order_key(
                Some(state.last_update_slot),
                record.exit_market_anchor_tx_signature.as_deref(),
                shadow_v2_event_seq(sample_ts, 3),
                sample_ts,
            ),
            &state,
            sample_ts,
            None,
            TemporalClass::PostEntry,
            ClockDomain::StreamObservedMs,
            6,
        );
        sample.event_order_key.slot = EventOrderComponent::known(state.last_update_slot);
        Some(sample)
    }

    fn shadow_v2_exit_fill_from_lifecycle(
        &self,
        record: &ShadowLifecycleRecord,
        pool_state_before: Option<&PoolStateSampleV2>,
    ) -> ShadowExitFillV2 {
        let fill_ts = record
            .exit_reason_evaluation_ts_ms
            .or(record.sample_timestamp_ms)
            .unwrap_or(record.timestamp_ms);
        let mut envelope = self.shadow_v2_lifecycle_envelope(
            record,
            "shadow_exit_fill_v2",
            format!(
                "shadow_v2_exit_fill:{}:{}:{}",
                record.position_id,
                fill_ts,
                shadow_lifecycle_record_type_label(record.record_type)
            ),
            TemporalClass::PostExit,
            ClockDomain::LandingTsMs,
        );
        envelope.source_refs.push(format!(
            "shadow_lifecycle:{}",
            shadow_lifecycle_record_type_label(record.record_type)
        ));
        let mut blockers = vec![
            "EXIT_FILL_DERIVED_FROM_LEGACY_LIFECYCLE_EVIDENCE".to_string(),
            "EXIT_POOL_STATE_AFTER_UNAVAILABLE".to_string(),
            "FILL_PRICE_UNAVAILABLE".to_string(),
            "SLIPPAGE_BPS_UNAVAILABLE".to_string(),
            "OWN_IMPACT_BPS_UNAVAILABLE".to_string(),
            "FEE_BPS_UNAVAILABLE".to_string(),
            "LANDING_TELEMETRY_UNAVAILABLE".to_string(),
            "QUOTE_FILL_DIVERGENCE_UNAVAILABLE".to_string(),
        ];
        if pool_state_before.is_none() {
            blockers.push("EXIT_POOL_STATE_BEFORE_UNAVAILABLE".to_string());
            blockers.push("EXIT_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME".to_string());
        }
        if matches!(record.record_type, ShadowLifecycleRecordType::ExitBlocked) {
            blockers.push("EXIT_FILL_LEGACY_LIFECYCLE_EXIT_BLOCKED".to_string());
        }
        if record.exit_price.is_none() {
            blockers.push("EXIT_FILL_LEGACY_EXIT_PRICE_MISSING".to_string());
        }
        if record.exit_token_amount_raw.is_none() {
            blockers.push("EXIT_FILL_TOKEN_AMOUNT_RAW_UNAVAILABLE".to_string());
        }
        let fill_order_key = shadow_v2_event_order_key(
            record.exit_landed_slot.or(record.exit_sample_slot),
            record.exit_market_anchor_tx_signature.as_deref(),
            shadow_v2_event_seq(fill_ts, 4),
            fill_ts,
        );
        if let Some(pool_state) = pool_state_before {
            if pool_state.observed_at_wall_ms > fill_ts {
                blockers.push("EXIT_FILL_POOL_STATE_AFTER_EXIT_FILL_BOUNDARY".to_string());
            }
            match (
                pool_state.event_order_key.slot.as_known(),
                fill_order_key.slot.as_known(),
            ) {
                (Some(pool_slot), Some(fill_slot)) if pool_slot > fill_slot => {
                    blockers.push("EXIT_FILL_POOL_STATE_AFTER_EXIT_FILL_BOUNDARY".to_string());
                }
                (Some(pool_slot), Some(fill_slot)) if pool_slot == fill_slot => {
                    if pool_state
                        .event_order_key
                        .same_slot_ambiguous_with(&fill_order_key)
                    {
                        blockers.push("EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string());
                    }
                }
                _ => {}
            }
            ShadowExitFillV2::blocked_with_pool_state(
                envelope,
                fill_order_key,
                pool_state,
                blockers,
            )
        } else {
            ShadowExitFillV2::blocked_without_pool_state(envelope, fill_order_key, blockers)
        }
    }

    fn shadow_v2_terminal_truth_from_lifecycle(
        &self,
        record: &ShadowLifecycleRecord,
    ) -> ShadowTerminalTruthV2 {
        let terminal_ts = record
            .exit_reason_evaluation_ts_ms
            .or(record.sample_timestamp_ms)
            .unwrap_or(record.timestamp_ms);
        let mut envelope = self.shadow_v2_lifecycle_envelope(
            record,
            "shadow_terminal_truth_v2",
            format!(
                "shadow_v2_terminal_truth:{}:{}:{}",
                record.position_id,
                terminal_ts,
                shadow_v2_exit_trigger_label(record)
            ),
            TemporalClass::PostExit,
            ClockDomain::StreamObservedMs,
        );
        envelope.simulation_level = SimulationLevel::MarkOnly;
        envelope.measurement_grade = MeasurementGrade::MarkPriceReplay;
        envelope.quality = "TERMINAL_TRUTH_DERIVED_FROM_LEGACY_LIFECYCLE".to_string();
        envelope
            .source_refs
            .push("shadow_lifecycle:position_closed".to_string());
        envelope
            .limitations
            .push("TERMINAL_TRUTH_MARK_PATH_ONLY_NOT_EXECUTABLE_FILL".to_string());
        envelope
            .limitations
            .push("TERMINAL_EXECUTABLE_PNL_BLOCKED_BY_EXIT_FILL_PROVENANCE".to_string());
        envelope
            .limitations
            .push("TERMINAL_TRUTH_DERIVED_FROM_LEGACY_LIFECYCLE_RECORD".to_string());
        envelope
            .limitations
            .push("TERMINAL_ENTRY_FILL_LINK_BEST_EFFORT_FROM_LEGACY_TIMELINE".to_string());

        let linked_exit_fill = if record.total_exits == 0 {
            Some(format!(
                "shadow_v2_exit_fill:{}:{}:{}",
                record.position_id,
                terminal_ts,
                shadow_lifecycle_record_type_label(ShadowLifecycleRecordType::PositionClosed)
            ))
        } else {
            envelope.limitations.push(
                "TERMINAL_EXIT_FILL_LINK_BLOCKED_BY_LEGACY_EXIT_TIMESTAMP_MISMATCH_RISK"
                    .to_string(),
            );
            None
        };

        ShadowTerminalTruthV2 {
            envelope,
            event_order_key: shadow_v2_event_order_key(
                record.exit_landed_slot.or(record.exit_sample_slot),
                record.exit_market_anchor_tx_signature.as_deref(),
                shadow_v2_event_seq(terminal_ts, 5),
                terminal_ts,
            ),
            terminal_reason: shadow_v2_terminal_reason(record.close_reason),
            terminal_ts_ms: ClockedTimestamp {
                field_name: "terminal_ts_ms".to_string(),
                value: Some(terminal_ts as i64),
                clock_domain: ClockDomain::StreamObservedMs,
                clock_source: "shadow_lifecycle.position_closed".to_string(),
                causal_boundary: "POST_EXIT_TERMINAL_TRUTH".to_string(),
            },
            terminal_slot: record.exit_landed_slot.or(record.exit_sample_slot),
            terminal_source: "shadow_lifecycle.position_closed".to_string(),
            final_pnl_mark_bps: shadow_v2_pnl_bps_from_lifecycle(record),
            final_pnl_executable_bps: None,
            close_age_ms: record.duration_ms,
            linked_entry_fill: Some(format!(
                "shadow_v2_entry_fill:{}:{}",
                record.position_id,
                record.entry_timestamp_ms()
            )),
            linked_exit_fill,
            reconciliation_status: "TERMINAL_TRUTH_FROM_LEGACY_LIFECYCLE_MARK_ONLY".to_string(),
            duplicate_terminal_handling: "CANONICAL_STREAM_REJECTS_DUPLICATE_TERMINAL_TRUTH"
                .to_string(),
        }
    }

    fn shadow_v2_lifecycle_envelope(
        &self,
        record: &ShadowLifecycleRecord,
        schema: &str,
        event_id: String,
        temporal_class: TemporalClass,
        clock_domain: ClockDomain,
    ) -> ShadowV2Envelope {
        let run_id = record
            .run_id
            .clone()
            .or_else(|| record.rollout_namespace.clone())
            .unwrap_or_else(|| "UNKNOWN_RUN".to_string());
        let mut envelope = ShadowV2Envelope::contract_header(
            schema,
            run_id,
            record.position_id.clone(),
            event_id,
            record.pool_id.clone(),
            record.mint_id.clone(),
        );
        envelope.session_id = record
            .session_id
            .clone()
            .or_else(|| Some("UNKNOWN_SESSION".to_string()));
        envelope.candidate_id = Some(record.candidate_id.clone());
        envelope.produced_at_ms = record.timestamp_ms;
        envelope.produced_at_slot = record
            .exit_landed_slot
            .or(record.exit_sample_slot)
            .or(record.sample_slot)
            .or(record.entry_landed_slot)
            .or(record.entry_slot);
        envelope.temporal_class = temporal_class;
        envelope.clock_domain = clock_domain;
        envelope
            .source_refs
            .push("post_buy_guardian:shadow_lifecycle_record".to_string());
        envelope
            .source_refs
            .push(format!("position_epoch:{}", record.position_epoch));
        envelope
            .source_refs
            .push(format!("entry_order_id:{}", record.entry_order_id));
        envelope
            .source_refs
            .push(format!("quote_id:{}", record.quote_id));
        envelope
            .limitations
            .push("SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS".to_string());
        envelope.limitations.push("NOT_LIVE_EQUIVALENT".to_string());
        if record.session_id.is_none() {
            envelope
                .limitations
                .push("SESSION_ID_MISSING_FROM_LIFECYCLE_EXPLICIT_UNKNOWN".to_string());
        }
        envelope
    }

    fn append_shadow_exit_replay_record(&self, record: &ShadowExitReplayRecord) {
        let Some(path) = self.shadow_exit_replay_log_path.as_deref() else {
            return;
        };
        if let Err(error) = Self::append_jsonl_record(path, record) {
            error!(
                path = %path.display(),
                position_id = %record.position_id,
                error = %error,
                "PostBuyGuardian: failed to append shadow exit replay proof"
            );
        }
    }

    fn build_exit_replay_tracker(
        &self,
        pos: &MonitoredPosition,
    ) -> Option<ShadowExitReplayTracker> {
        if !self.config.exit_replay_v1.enabled
            || self.shadow_exit_replay_log_path.is_none()
            || !matches!(pos.lane, Lane::Shadow)
        {
            return None;
        }

        let identity = ShadowExitReplayIdentity {
            run_id: pos.join_metadata.run_id.clone(),
            session_id: pos.join_metadata.session_id.clone(),
            candidate_id: pos.candidate_id.clone(),
            position_id: pos.position_id.clone(),
            pool_id: pos.pool_amm_id.to_string(),
            base_mint: pos.base_mint.to_string(),
            bonding_curve: pos.bonding_curve,
            entry_ts_ms: pos.entry_unix_ms,
            entry_price: pos.entry_price_sol.unwrap_or(0.0),
            entry_source: "shadow_simulated".to_string(),
        };
        Some(ShadowExitReplayTracker::new(
            identity,
            &self.config.exit_replay_v1,
        ))
    }

    fn register_exit_replay_tracker(&self, base_mint: Pubkey, tracker: ShadowExitReplayTracker) {
        if !tracker.has_valid_entry_price() {
            let record = tracker.finalize(current_time_ms(), None);
            self.append_shadow_exit_replay_record(&record);
            return;
        }

        let mut trackers = self.exit_replay_trackers.write();
        trackers.entry(base_mint).or_default().push(tracker);
    }

    fn active_exit_replay_mints(&self) -> Vec<Pubkey> {
        self.exit_replay_trackers
            .read()
            .iter()
            .filter_map(|(mint, trackers)| (!trackers.is_empty()).then_some(*mint))
            .collect()
    }

    pub fn active_exit_replay_tracker_count(&self) -> usize {
        self.exit_replay_trackers
            .read()
            .values()
            .map(Vec::len)
            .sum()
    }

    fn exit_replay_bonding_curve(&self, base_mint: &Pubkey) -> Option<Pubkey> {
        self.exit_replay_trackers
            .read()
            .get(base_mint)
            .and_then(|trackers| trackers.first().map(ShadowExitReplayTracker::bonding_curve))
    }

    fn observe_exit_replay_snapshot(&self, base_mint: &Pubkey, snapshot: &MarketSnapshot) {
        if !self.config.exit_replay_v1.enabled {
            return;
        }
        let Some(current_price_sol) =
            PriceTruthResolver::normalize_shadow_snapshot_price_sol(snapshot)
        else {
            return;
        };

        let mut trackers = self.exit_replay_trackers.write();
        let Some(trackers_for_mint) = trackers.get_mut(base_mint) else {
            return;
        };
        for tracker in trackers_for_mint {
            tracker.observe_price_sample(snapshot.timestamp_ms, current_price_sol);
        }
    }

    fn observe_exit_replay_current_snapshot(&self, base_mint: &Pubkey, now_ms: u64) {
        let bonding_curve = self.exit_replay_bonding_curve(base_mint);
        if let Some(snapshot) =
            self.current_runtime_shadow_snapshot_with_curve(base_mint, now_ms, bonding_curve)
        {
            self.observe_exit_replay_snapshot(base_mint, &snapshot);
        }
    }

    fn flush_due_exit_replay_trackers(&self, now_ms: u64, forced_reason: Option<&str>) {
        if !self.config.exit_replay_v1.enabled {
            return;
        }

        let mut records = Vec::new();
        let mut empty_mints = Vec::new();
        {
            let mut trackers = self.exit_replay_trackers.write();
            for (mint, trackers_for_mint) in trackers.iter_mut() {
                let mut idx = 0;
                while idx < trackers_for_mint.len() {
                    let should_finalize = forced_reason.is_some()
                        || trackers_for_mint[idx].is_horizon_reached(now_ms);
                    if should_finalize {
                        let tracker = trackers_for_mint.remove(idx);
                        records.push(tracker.finalize(now_ms, forced_reason));
                    } else {
                        idx += 1;
                    }
                }
                if trackers_for_mint.is_empty() {
                    empty_mints.push(*mint);
                }
            }
            for mint in empty_mints {
                trackers.remove(&mint);
            }
        }

        for record in records {
            self.append_shadow_exit_replay_record(&record);
        }
    }

    fn observe_all_exit_replay_current_snapshots(&self, now_ms: u64) {
        for mint in self.active_exit_replay_mints() {
            self.observe_exit_replay_current_snapshot(&mint, now_ms);
        }
    }

    pub async fn flush_exit_replay_for_shutdown(&self) {
        if !self.config.exit_replay_v1.enabled {
            return;
        }

        if self.config.exit_replay_v1.flush_on_shutdown {
            let deadline = Instant::now()
                + Duration::from_millis(self.config.exit_replay_v1.shutdown_flush_budget_ms());
            while self.active_exit_replay_tracker_count() > 0 && Instant::now() < deadline {
                let now_ms = current_time_ms();
                self.observe_all_exit_replay_current_snapshots(now_ms);
                self.flush_due_exit_replay_trackers(now_ms, None);
                if self.active_exit_replay_tracker_count() == 0 {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(self.config.tick_interval_ms.max(50)))
                    .await;
            }
        }

        let now_ms = current_time_ms();
        self.observe_all_exit_replay_current_snapshots(now_ms);
        self.flush_due_exit_replay_trackers(now_ms, None);
        if self.active_exit_replay_tracker_count() == 0 {
            return;
        }
        self.flush_due_exit_replay_trackers(now_ms, Some(REASON_SHUTDOWN_BEFORE_HORIZON));
    }

    fn time_stop_v2_evidence(
        source: PriceTruthSource,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) -> PriceTruthEvidence {
        match latest {
            Some(snapshot) => PriceTruthEvidence {
                source,
                status: PriceTruthStatus::Resolved,
                detail: Some("time_stop_v2_observe_only_window".to_string()),
                slot: snapshot.slot,
                timestamp_ms: Some(snapshot.timestamp_ms),
                age_ms: Some(now_ms.saturating_sub(snapshot.timestamp_ms)),
                price_state: Some(snapshot.price_state),
                price_reason: snapshot.price_reason,
            },
            None => PriceTruthEvidence {
                source,
                status: PriceTruthStatus::Failure,
                detail: Some("time_stop_v2_observe_only_window_without_market_sample".to_string()),
                slot: None,
                timestamp_ms: Some(now_ms),
                age_ms: None,
                price_state: None,
                price_reason: None,
            },
        }
    }

    fn apply_time_stop_v2_evaluation_to_record(
        record: &mut ShadowLifecycleRecord,
        evaluation: &TimeStopV2Evaluation,
    ) {
        record.time_stop_v2_mode = Some(evaluation.mode);
        record.time_stop_v2_window_index = Some(evaluation.window_index);
        record.time_stop_v2_scheduled_check_ms = Some(evaluation.scheduled_check_ms);
        record.time_stop_v2_position_age_ms = Some(evaluation.position_age_ms);
        record.time_stop_v2_status = Some(evaluation.status);
        record.time_stop_v2_subreason = Some(evaluation.subreason);
        record.time_stop_v2_failed_windows = Some(evaluation.failed_windows);
        record.time_stop_v2_candidate = Some(evaluation.candidate);
        record.time_stop_v2_candidate_ts_ms = evaluation.candidate_ts_ms;
        record.time_stop_v2_candidate_subreason = evaluation.candidate_subreason;
        record.time_stop_v2_price_delta_pct_window = evaluation.price_delta_pct_window;
        record.time_stop_v2_price_delta_pct_from_entry = evaluation.price_delta_pct_from_entry;
        record.time_stop_v2_mcap_delta_pct_window = evaluation.mcap_delta_pct_window;
        record.time_stop_v2_bonding_delta_pct_window = evaluation.bonding_delta_pct_window;
        record.time_stop_v2_tx_delta_window = evaluation.tx_delta_window;
        record.time_stop_v2_volume_delta_sol_window = evaluation.volume_delta_sol_window;
        record.time_stop_v2_avg_volume_per_tx_sol_window = evaluation.avg_volume_per_tx_sol_window;
        record.time_stop_v2_checkpoint_slot = evaluation.checkpoint_slot;
        record.time_stop_v2_latest_slot = evaluation.latest_slot;
        record.time_stop_v2_checkpoint_timestamp_ms = evaluation.checkpoint_timestamp_ms;
        record.time_stop_v2_latest_timestamp_ms = evaluation.latest_timestamp_ms;
    }

    fn evaluate_time_stop_v2_observe_only(
        &self,
        base_mint: &Pubkey,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) {
        let cfg = self.config.time_stop_v2.clone();
        if !cfg.enabled {
            return;
        }

        let mut record_to_emit = None;
        {
            let mut positions = self.positions.write();
            let Some(pos) = positions.get_mut(base_mint) else {
                return;
            };
            if !matches!(pos.lane, Lane::Shadow) {
                return;
            }

            let entry_unix_ms = pos.entry_unix_ms;
            let entry_price_sol = pos.entry_price_sol;
            let snapshot_source = pos.last_snapshot_source;
            let Some(evaluation) =
                pos.time_stop_v2
                    .evaluate(&cfg, entry_unix_ms, entry_price_sol, latest, now_ms)
            else {
                return;
            };

            if cfg.emit_window_records {
                let evidence = Self::time_stop_v2_evidence(snapshot_source, latest, now_ms);
                let mut record = self.shadow_lifecycle_record_base(
                    pos,
                    ShadowLifecycleRecordType::TimeStopV2Window,
                    now_ms,
                    &evidence,
                );
                Self::apply_time_stop_v2_evaluation_to_record(&mut record, &evaluation);
                record_to_emit = Some(record);
            }
        }

        if let Some(record) = record_to_emit {
            self.append_shadow_lifecycle_record(&record);
        }
    }

    fn shadow_lifecycle_record_base(
        &self,
        pos: &MonitoredPosition,
        record_type: ShadowLifecycleRecordType,
        now_ms: u64,
        evidence: &PriceTruthEvidence,
    ) -> ShadowLifecycleRecord {
        let exit_landed_slot = synthetic_next_slot(evidence.slot);
        let time_stop_v2_observed = pos.time_stop_v2.has_observed();
        ShadowLifecycleRecord {
            ab_record_id: pos.join_metadata.ab_record_id.clone(),
            source_ab_record_id: pos.join_metadata.source_ab_record_id.clone(),
            probe_id: pos.join_metadata.probe_id.clone(),
            dispatch_source: pos.join_metadata.dispatch_source.clone(),
            collection_plane: pos.join_metadata.collection_plane.clone(),
            probe_plane: pos.join_metadata.probe_plane.clone(),
            v3_feature_snapshot_hash: pos.join_metadata.v3_feature_snapshot_hash.clone(),
            v3_policy_config_hash: pos.join_metadata.v3_policy_config_hash.clone(),
            decision_plane: pos.join_metadata.decision_plane.clone(),
            rollout_namespace: pos.join_metadata.rollout_namespace.clone(),
            run_id: pos.join_metadata.run_id.clone(),
            session_id: pos.join_metadata.session_id.clone(),
            brain_config_path: pos.join_metadata.brain_config_path.clone(),
            brain_config_hash: pos.join_metadata.brain_config_hash.clone(),
            record_type,
            timestamp: chrono::Utc::now().to_rfc3339(),
            timestamp_ms: now_ms,
            candidate_id: pos.candidate_id.clone(),
            pool_id: pos.pool_amm_id.to_string(),
            mint_id: pos.base_mint.to_string(),
            position_id: pos.position_id.clone(),
            position_epoch: pos.position_epoch,
            lane: pos.lane,
            entry_order_id: pos.entry_order_id.clone(),
            quote_id: pos.quote_id.clone(),
            entry_slot: pos.slot,
            entry_simulation_rpc_slot: pos.join_metadata.entry_simulation_rpc_slot,
            entry_market_anchor_slot: pos.join_metadata.entry_market_anchor_slot,
            entry_market_anchor_tx_signature: pos
                .join_metadata
                .entry_market_anchor_tx_signature
                .clone(),
            entry_market_anchor_source: pos.join_metadata.entry_market_anchor_source.clone(),
            entry_landed_slot: pos.join_metadata.entry_landed_slot,
            entry_landed_slot_source: pos.join_metadata.entry_landed_slot_source.clone(),
            fraction_bps: None,
            remaining_fraction_bps: pos.remaining_fraction_bps,
            entry_price: pos.entry_price_sol,
            exit_price: None,
            entry_value_sol: None,
            exit_value_sol: None,
            exit_token_amount_raw: None,
            gross_pnl_sol: None,
            net_pnl_sol: None,
            estimated_costs_sol: None,
            final_pnl: None,
            final_pnl_pct: None,
            duration_ms: None,
            close_reason: None,
            total_exits: pos.total_exits,
            truth_source: evidence.source,
            truth_status: evidence.status,
            truth_detail: evidence.detail.clone(),
            exit_sample_slot: evidence.slot,
            exit_market_anchor_slot: evidence.slot,
            exit_market_anchor_tx_signature: None,
            exit_market_anchor_source: Some(evidence.source),
            exit_reason_evaluation_ts_ms: Some(now_ms),
            exit_landed_slot,
            exit_landed_slot_source: exit_landed_slot
                .map(|_| "synthetic_next_slot_after_exit_sample".to_string()),
            sample_slot: evidence.slot,
            sample_timestamp_ms: evidence.timestamp_ms,
            sample_age_ms: evidence.age_ms,
            sample_price_state: evidence.price_state,
            sample_price_reason: evidence.price_reason,
            time_stop_v2_mode: None,
            time_stop_v2_window_index: None,
            time_stop_v2_scheduled_check_ms: None,
            time_stop_v2_position_age_ms: None,
            time_stop_v2_status: if time_stop_v2_observed {
                pos.time_stop_v2.last_status
            } else {
                None
            },
            time_stop_v2_subreason: if time_stop_v2_observed {
                pos.time_stop_v2.last_subreason
            } else {
                None
            },
            time_stop_v2_failed_windows: time_stop_v2_observed
                .then_some(pos.time_stop_v2.failed_windows),
            time_stop_v2_candidate: time_stop_v2_observed
                .then_some(pos.time_stop_v2.candidate_emitted),
            time_stop_v2_candidate_ts_ms: pos.time_stop_v2.candidate_ts_ms,
            time_stop_v2_candidate_subreason: pos.time_stop_v2.candidate_subreason,
            time_stop_v2_price_delta_pct_window: None,
            time_stop_v2_price_delta_pct_from_entry: None,
            time_stop_v2_mcap_delta_pct_window: None,
            time_stop_v2_bonding_delta_pct_window: None,
            time_stop_v2_tx_delta_window: None,
            time_stop_v2_volume_delta_sol_window: None,
            time_stop_v2_avg_volume_per_tx_sol_window: None,
            time_stop_v2_checkpoint_slot: None,
            time_stop_v2_latest_slot: None,
            time_stop_v2_checkpoint_timestamp_ms: None,
            time_stop_v2_latest_timestamp_ms: None,
        }
    }

    fn emit_position_closed(&self, pos: &MonitoredPosition, duration_ms: u64) {
        let gross_pnl_sol = if pos.total_exits > 0 {
            Some(pos.realized_exit_value_sol - pos.entry_value_sol)
        } else {
            None
        };
        let net_pnl_sol = gross_pnl_sol.map(|gross| gross - pos.estimated_costs_sol);
        let final_pnl = gross_pnl_sol.unwrap_or(pos.realized_pnl_sol);
        let final_pnl_pct = if pos.entry_value_sol > 0.0 {
            (final_pnl / pos.entry_value_sol) * 100.0
        } else {
            pos.realized_pnl_pct
        };
        let close_reason = pos.last_close_reason.unwrap_or(CloseReason::Default);

        if let Some(emitter) = self.event_emitter.as_ref() {
            let mut env = emitter.make_envelope_at(&pos.candidate_id, current_time_ms());
            env.position_id = Some(pos.position_id.clone());
            env.position_epoch = Some(pos.position_epoch);
            env.order_id = Some(pos.entry_order_id.clone());
            env.quote_id = Some(pos.quote_id.clone());
            env.slot = pos.slot;
            emitter.emit_raw(ExecutionEvent::new(
                env,
                EventKind::PositionClosed(PositionClosedPayload {
                    final_pnl,
                    final_pnl_pct,
                    entry_value_sol: (pos.total_exits > 0).then_some(pos.entry_value_sol),
                    exit_value_sol: (pos.total_exits > 0).then_some(pos.realized_exit_value_sol),
                    gross_pnl_sol,
                    net_pnl_sol,
                    estimated_costs_sol: (pos.total_exits > 0).then_some(pos.estimated_costs_sol),
                    duration_ms,
                    reason: close_reason,
                    total_exits: pos.total_exits,
                }),
            ));
        }

        if matches!(pos.lane, Lane::Shadow) {
            let evidence = pos
                .last_price_truth
                .clone()
                .unwrap_or(PriceTruthEvidence {
                    source: pos.last_snapshot_source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow position closed without resolved exit truth; no synthetic fallback applied"
                            .to_string(),
                    ),
                    slot: pos.slot,
                    timestamp_ms: Some(current_time_ms()),
                    age_ms: None,
                    price_state: None,
                    price_reason: None,
                });
            let mut record = self.shadow_lifecycle_record_base(
                pos,
                ShadowLifecycleRecordType::PositionClosed,
                current_time_ms(),
                &evidence,
            );
            record.entry_value_sol = (pos.total_exits > 0).then_some(pos.entry_value_sol);
            record.exit_value_sol = (pos.total_exits > 0).then_some(pos.realized_exit_value_sol);
            record.gross_pnl_sol = gross_pnl_sol;
            record.net_pnl_sol = net_pnl_sol;
            record.estimated_costs_sol = (pos.total_exits > 0).then_some(pos.estimated_costs_sol);
            record.final_pnl = (pos.total_exits > 0).then_some(final_pnl);
            record.final_pnl_pct = (pos.total_exits > 0).then_some(final_pnl_pct);
            record.duration_ms = Some(duration_ms);
            record.close_reason = Some(close_reason);
            self.append_shadow_lifecycle_record(&record);
        }
    }

    // ═════════════════════════════════════════════════════════════════
    // Position lifecycle
    // ═════════════════════════════════════════════════════════════════

    /// Register a new position for monitoring after successful buy.
    ///
    /// Returns `true` if the position was registered, `false` if rejected
    /// (limit reached or already monitored).
    pub fn register_position(
        &self,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        entry_price_sol: Option<f64>,
    ) -> bool {
        self.register_position_with_context(
            pool_amm_id,
            base_mint,
            bonding_curve,
            entry_price_sol,
            None,
            None,
            None,
        )
        .is_some()
    }

    /// Register a new position with explicit event identifiers from the entry lane.
    pub fn register_position_with_context(
        &self,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        entry_price_sol: Option<f64>,
        entry_amount_lamports: Option<u64>,
        entry_token_amount_raw: Option<u64>,
        context: Option<PositionEventContext>,
    ) -> Option<RegisteredPosition> {
        let initial_shadow_snapshot = self.current_shadow_curve_snapshot(&base_mint);
        let mut snapshot_timeline = SnapshotTimeline::default();
        if let Some(snapshot) = initial_shadow_snapshot.clone() {
            snapshot_timeline.replace_with(
                vec![snapshot],
                self.snapshot_history_max_snapshots(),
                self.snapshot_history_retention_ms(),
            );
        }
        let mut positions = self.positions.write();

        if positions.len() >= self.config.max_monitored_positions {
            warn!(
                "🛡️ PostBuyGuardian: Position limit reached ({}/{}), cannot monitor mint={}",
                positions.len(),
                self.config.max_monitored_positions,
                base_mint
            );
            return None;
        }

        if positions.contains_key(&base_mint) {
            debug!("🛡️ PostBuyGuardian: Already monitoring mint={}", base_mint);
            return None;
        }

        let now_ms = current_time_ms();
        let fallback_candidate_id = Self::default_candidate_id(pool_amm_id, base_mint, now_ms);
        let event_context = context.unwrap_or(PositionEventContext {
            join_metadata: PositionJoinMetadata::default(),
            candidate_id: fallback_candidate_id,
            entry_order_id: format!("entry-open-{}", now_ms),
            quote_id: format!("quote-open-{}", now_ms),
            slot: None,
            lane: self
                .event_emitter
                .as_ref()
                .map(|emitter| emitter.lane())
                .unwrap_or(Lane::Single),
            position_id: None,
            position_epoch: None,
            opened_at_ms: None,
        });
        let opened_at_ms = event_context
            .opened_at_ms
            .filter(|timestamp_ms| *timestamp_ms > 0)
            .unwrap_or(now_ms);
        let position_id = event_context
            .position_id
            .clone()
            .unwrap_or_else(|| format!("{}:{}:{}", pool_amm_id, base_mint, now_ms));
        let position_epoch = event_context.position_epoch.unwrap_or(1_u64);
        let shadow_market_activity = ShadowMarketActivityAnchor::from_registration(
            opened_at_ms,
            initial_shadow_snapshot.as_ref(),
        );
        let time_stop_v2 = TimeStopV2State::from_registration(initial_shadow_snapshot.as_ref());
        let position = MonitoredPosition {
            candidate_id: event_context.candidate_id.clone(),
            lane: event_context.lane,
            pool_amm_id,
            base_mint,
            bonding_curve,
            entry_time: Instant::now(),
            entry_unix_ms: opened_at_ms,
            entry_price_sol,
            entry_size_lamports: entry_amount_lamports.unwrap_or(0),
            entry_token_amount_raw: entry_token_amount_raw.unwrap_or(0),
            remaining_token_amount_raw: entry_token_amount_raw.unwrap_or(0),
            position_id: position_id.clone(),
            position_epoch,
            join_metadata: event_context.join_metadata.clone(),
            entry_order_id: event_context.entry_order_id.clone(),
            quote_id: event_context.quote_id.clone(),
            slot: event_context.slot,
            peak_since_entry: entry_price_sol.unwrap_or(0.0),
            last_peak_unix_ms: opened_at_ms,
            aem_registered: false,
            runtime_registered: false,
            last_stress_bucket: None,
            tcf: TrendCohesionField::new(),
            consecutive_low_cohesion: 0,
            last_tcf_score: 1.0,
            last_tradability: 1.0,
            recent_signals: Vec::with_capacity(64),
            entry_value_sol: entry_amount_lamports.unwrap_or(0) as f64 / 1_000_000_000.0,
            realized_exit_value_sol: 0.0,
            estimated_costs_sol: 0.0,
            realized_pnl_sol: 0.0,
            realized_pnl_pct: 0.0,
            total_exits: 0,
            remaining_fraction_bps: 10_000,
            last_close_reason: None,
            last_force_exit_reason_code: None,
            last_price_truth: None,
            last_blocked_truth_status: None,
            last_blocked_truth_timestamp_ms: None,
            last_snapshot_source: self.default_snapshot_source(),
            last_shadow_snapshot: initial_shadow_snapshot,
            shadow_market_activity,
            time_stop_v2,
            snapshot_timeline,
        };

        let exit_replay_tracker = self.build_exit_replay_tracker(&position);
        positions.insert(base_mint, position);
        drop(positions);
        if let Some(tracker) = exit_replay_tracker {
            self.register_exit_replay_tracker(base_mint, tracker);
        }
        info!(
            "🛡️ PostBuyGuardian: Monitoring started — mint={} pool={} entry_price={:?} SOL",
            base_mint, pool_amm_id, entry_price_sol
        );

        self.emit_position_opened(
            &event_context.candidate_id,
            &position_id,
            position_epoch,
            &event_context.entry_order_id,
            &event_context.quote_id,
            event_context.slot,
            entry_price_sol,
            opened_at_ms,
            entry_token_amount_raw.unwrap_or(0),
            entry_amount_lamports.unwrap_or(0),
        );

        Some(RegisteredPosition {
            position_id,
            position_epoch,
            opened_at_ms,
        })
    }

    /// Remove position from monitoring (after sell, expiry, or panic kill).
    pub fn unregister_position(&self, base_mint: &Pubkey) {
        let mut positions = self.positions.write();
        if let Some(pos) = positions.remove(base_mint) {
            if let Some(ref runtime) = self.aem_runtime {
                let mut rt = runtime.lock();
                let _ = rt.unregister_position(&pos.position_id);
            }
            let duration_ms = current_time_ms().saturating_sub(pos.entry_unix_ms);
            info!(
                "🛡️ PostBuyGuardian: Stopped monitoring mint={} (held {:.1}s, signals={})",
                base_mint,
                duration_ms as f64 / 1000.0,
                pos.recent_signals.len()
            );
            self.emit_position_closed(&pos, duration_ms);
        }
    }

    /// Returns the number of currently monitored positions.
    pub fn active_position_count(&self) -> usize {
        self.positions.read().len()
    }

    /// Returns the list of currently monitored base mints.
    pub fn active_mints(&self) -> Vec<Pubkey> {
        self.positions.read().keys().cloned().collect()
    }

    async fn ensure_shadow_runtime_registered(&self, base_mint: &Pubkey) -> bool {
        let Some(ref router) = self.position_router else {
            return false;
        };
        let Some(shadow_book) = router.shadow_book() else {
            return false;
        };

        let registration = {
            let positions = self.positions.read();
            let Some(pos) = positions.get(base_mint) else {
                return false;
            };
            if !matches!(pos.lane, Lane::Shadow) {
                return true;
            }
            if pos.runtime_registered {
                return true;
            }
            let Some(entry_price_sol) = pos
                .entry_price_sol
                .filter(|price| price.is_finite() && *price > 0.0)
            else {
                return false;
            };
            (pos.position_id.clone(), pos.position_epoch, entry_price_sol)
        };

        let (position_id, position_epoch, entry_price_sol) = registration;
        let register_result = {
            let mut shadow_book = shadow_book.write().await;
            shadow_book.register_position(*base_mint, &position_id, position_epoch, entry_price_sol)
        };
        match register_result {
            Ok(()) => {
                let mut positions = self.positions.write();
                if let Some(pos) = positions.get_mut(base_mint) {
                    pos.runtime_registered = true;
                }
                true
            }
            Err(error) => {
                warn!(
                    position_id = %position_id,
                    error = %error,
                    "PostBuyGuardian: failed to register shadow virtual magazine"
                );
                false
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════
    // Health query
    // ═════════════════════════════════════════════════════════════════

    /// Get current health assessment for a position.
    ///
    /// Returns `None` if the mint is not being monitored.
    pub fn get_position_health(&self, base_mint: &Pubkey) -> Option<PositionHealth> {
        let positions = self.positions.read();
        let pos = positions.get(base_mint)?;

        let now_ms = current_time_ms();
        let window_start = now_ms.saturating_sub(self.config.signal_aggregation_window_ms);

        let mut warning_count = 0u32;
        let mut critical_count = 0u32;
        let mut manipulation_detected = false;
        let mut panic_impulse_active = false;

        for ts_sig in &pos.recent_signals {
            if ts_sig.timestamp_ms < window_start {
                continue;
            }
            match ts_sig.signal.severity {
                SignalSeverity::Warning => warning_count += 1,
                SignalSeverity::Critical => critical_count += 1,
                SignalSeverity::Info => {}
            }
            if ts_sig.signal.source == SignalSource::Whf
                && ts_sig.signal.severity >= SignalSeverity::Warning
            {
                manipulation_detected = true;
            }
            if ts_sig.signal.source == SignalSource::Panic
                && ts_sig.signal.severity >= SignalSeverity::Warning
            {
                panic_impulse_active = true;
            }
        }

        let recommended_action = self.compute_recommendation(
            warning_count,
            critical_count,
            manipulation_detected,
            panic_impulse_active,
        );

        // Health score: starts at 1.0, decremented by signals
        let health_score =
            (1.0 - (warning_count as f32 * 0.1) - (critical_count as f32 * 0.3)).clamp(0.0, 1.0);

        Some(PositionHealth {
            health_score,
            liquidity_tradability: pos.last_tradability,
            trend_cohesion: pos.last_tcf_score as f32,
            manipulation_detected,
            panic_impulse_active,
            warning_count,
            critical_count,
            recommended_action,
        })
    }

    fn position_signal_context(&self, base_mint: &Pubkey) -> Option<(Pubkey, Lane, String)> {
        let positions = self.positions.read();
        let pos = positions.get(base_mint)?;
        Some((pos.pool_amm_id, pos.lane, pos.position_id.clone()))
    }

    fn compute_recommendation(
        &self,
        warning_count: u32,
        critical_count: u32,
        manipulation_detected: bool,
        panic_impulse: bool,
    ) -> RecommendedAction {
        // Critical signals or panic impulse → immediate exit
        if critical_count >= self.config.escalation_critical_count || panic_impulse {
            return RecommendedAction::PanicSell;
        }
        // Manipulation detected → defensive mode
        if manipulation_detected {
            return RecommendedAction::DefensiveMode;
        }
        // Too many warnings → tighten stop
        if warning_count >= self.config.escalation_warning_count {
            return RecommendedAction::TightenStop;
        }
        RecommendedAction::Hold
    }

    // ═════════════════════════════════════════════════════════════════
    // Main loop
    // ═════════════════════════════════════════════════════════════════

    /// Start the monitoring loop as a tokio task.
    ///
    /// The task runs indefinitely until dropped/cancelled.
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let tick_interval = std::time::Duration::from_millis(self.config.tick_interval_ms);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            info!(
                "🛡️ PostBuyGuardian: Monitoring loop started (tick={}ms, max_positions={})",
                self.config.tick_interval_ms, self.config.max_monitored_positions
            );

            loop {
                interval.tick().await;
                self.tick().await;
            }
        })
    }

    /// Single monitoring tick — runs all modules against all positions.
    async fn tick(&self) {
        let active_mint_keys: Vec<Pubkey> = {
            let positions = self.positions.read();
            positions.keys().cloned().collect()
        };
        let replay_mint_keys = self.active_exit_replay_mints();
        if active_mint_keys.is_empty() && replay_mint_keys.is_empty() {
            return;
        }

        let tick_start = Instant::now();
        let now_ms = current_time_ms();

        for base_mint in &active_mint_keys {
            // Shadow positions must join the managed runtime before the first market
            // snapshot arrives; otherwise the sync step can misclassify "not yet seeded"
            // as "already closed" and emit a bogus PositionClosed without economics.
            let _ = self.ensure_shadow_runtime_registered(base_mint).await;

            // Refresh the per-position market timeline from the active truth source.
            let snapshots = match self.snapshots_for_tick(base_mint) {
                Some(s) if !s.is_empty() => s,
                _ => {
                    self.cleanup_old_signals(base_mint, now_ms);
                    let runtime_snapshot = self.current_runtime_shadow_snapshot(base_mint, now_ms);
                    if let Some(snapshot) = runtime_snapshot.as_ref() {
                        self.observe_exit_replay_snapshot(base_mint, snapshot);
                        self.remember_shadow_snapshot(base_mint, snapshot);
                        self.evaluate_time_stop_v2_observe_only(base_mint, Some(snapshot), now_ms);
                        self.run_shadow_runtime_tick(base_mint, Some(snapshot), now_ms)
                            .await;
                    } else {
                        self.evaluate_time_stop_v2_observe_only(base_mint, None, now_ms);
                        self.run_shadow_runtime_tick(base_mint, None, now_ms).await;
                    }
                    continue;
                }
            };

            for snapshot in &snapshots {
                self.observe_exit_replay_snapshot(base_mint, snapshot);
            }
            let latest = &snapshots[snapshots.len() - 1];
            if self.note_shadow_market_activity(base_mint, latest, now_ms) {
                self.refresh_shadow_time_stop_anchor(base_mint).await;
            }
            self.remember_shadow_snapshot(base_mint, latest);
            self.evaluate_time_stop_v2_observe_only(base_mint, Some(latest), now_ms);

            // ── MODULE 1: LIGMA (liquidity check) ────────────────────
            self.run_ligma_check(base_mint, latest, now_ms).await;

            // ── MODULE 2: WHF (wash trading / manipulation) ──────────
            self.run_whf_check(base_mint, &snapshots, now_ms).await;

            // ── MODULE 3: TCF (trend cohesion) ───────────────────────
            self.run_tcf_check(base_mint, &snapshots, now_ms).await;

            // ── MODULE 4: PANIC (impulse detection) ──────────────────
            self.run_panic_check(base_mint, &snapshots, now_ms).await;

            // ── Cleanup old signals ──────────────────────────────────
            self.cleanup_old_signals(base_mint, now_ms);

            // ── AEM v1 decision loop ────────────────────────────────
            self.run_aem_tick(base_mint, &snapshots, now_ms).await;

            // ── Shadow virtual magazine / exit runtime ─────────────
            let runtime_snapshot = self.current_runtime_shadow_snapshot(base_mint, now_ms);
            let runtime_snapshot = runtime_snapshot.as_ref().unwrap_or(latest);
            self.observe_exit_replay_snapshot(base_mint, runtime_snapshot);
            self.run_shadow_runtime_tick(base_mint, Some(runtime_snapshot), now_ms)
                .await;
        }

        for base_mint in replay_mint_keys {
            if !active_mint_keys.contains(&base_mint) {
                self.observe_exit_replay_current_snapshot(&base_mint, now_ms);
            }
        }
        self.flush_due_exit_replay_trackers(now_ms, None);

        self.flush_aem_outcomes(now_ms);

        // ── Auto-unregister: sync with managed position runtime ──
        self.sync_with_position_runtime(&active_mint_keys).await;

        let tick_elapsed = tick_start.elapsed();
        if tick_elapsed.as_millis() > self.config.tick_interval_ms as u128 {
            warn!(
                "🛡️ PostBuyGuardian: Tick overrun! Took {}ms (budget={}ms, positions={})",
                tick_elapsed.as_millis(),
                self.config.tick_interval_ms,
                active_mint_keys.len()
            );
        }
    }

    /// Sync monitored positions with their managed runtime sink.
    async fn sync_with_position_runtime(&self, monitored_mints: &[Pubkey]) {
        let Some(ref router) = self.position_router else {
            return;
        };

        let monitored_positions: Vec<(Pubkey, String, Lane, bool)> = {
            let positions = self.positions.read();
            monitored_mints
                .iter()
                .filter_map(|mint| {
                    positions.get(mint).map(|pos| {
                        (
                            *mint,
                            pos.position_id.clone(),
                            pos.lane,
                            pos.runtime_registered,
                        )
                    })
                })
                .collect()
        };

        for (mint, position_id, lane, runtime_registered) in monitored_positions {
            if matches!(lane, Lane::Shadow) && !runtime_registered {
                continue;
            }
            if !router.is_position_active(lane, &mint, &position_id).await {
                if matches!(lane, Lane::Shadow) {
                    let shadow_backend = { self.shadow_backend.read().clone() };
                    if let Some(shadow_backend) = shadow_backend {
                        let _ = shadow_backend.unregister_position(&position_id).await;
                    }
                }
                self.unregister_position(&mint);
                info!(
                    "🛡️ PostBuyGuardian: Auto-unregistered lane={} mint={} (no longer in managed runtime)",
                    lane, mint
                );
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════
    // Module 1: LIGMA — Liquidity Impact Guard
    // ═════════════════════════════════════════════════════════════════

    async fn run_ligma_check(&self, base_mint: &Pubkey, latest: &MarketSnapshot, now_ms: u64) {
        // Use reserve data directly from snapshot (no RPC call)
        let reserve_sol = latest.reserve_quote;
        let reserve_token = latest.reserve_base;

        if reserve_sol <= 0.0 || reserve_token <= 0.0 {
            return;
        }

        // Compute retail impact for probe sell size using constant-product formula:
        //   impact_bps = (sell_sol / reserve_sol) * 10_000
        // This is the price impact for selling `ligma_probe_sol` SOL worth of tokens
        let probe_sol = self.config.ligma_probe_sol;
        let impact_bps = (probe_sol / reserve_sol) * 10_000.0;

        // Tradability: inverse of impact, clamped to [0, 1]
        let tradability = (1.0 - (impact_bps / 10_000.0)).clamp(0.0, 1.0) as f32;

        // Update position state
        {
            let mut positions = self.positions.write();
            if let Some(pos) = positions.get_mut(base_mint) {
                pos.last_tradability = tradability;
            }
        }

        let Some((pool_amm_id, lane, position_id)) = self.position_signal_context(base_mint) else {
            return;
        };

        // Evaluate thresholds
        if impact_bps >= self.config.ligma_critical_impact_bps
            || (tradability as f64) < self.config.ligma_critical_tradability
        {
            self.emit_signal(GuardianSignal {
                lane,
                position_id: Some(position_id.clone()),
                base_mint: *base_mint,
                pool_amm_id,
                source: SignalSource::Ligma,
                severity: SignalSeverity::Critical,
                reason: format!(
                    "Liquidity trap: impact={:.0}bps tradability={:.3} reserve={:.2}SOL",
                    impact_bps, tradability, reserve_sol
                ),
                confidence: 0.95,
                timestamp_ms: now_ms,
                raw_score: Some(impact_bps),
            })
            .await;
        } else if impact_bps >= self.config.ligma_warning_impact_bps
            || (tradability as f64) < self.config.ligma_warning_tradability
        {
            self.emit_signal(GuardianSignal {
                lane,
                position_id: Some(position_id),
                base_mint: *base_mint,
                pool_amm_id,
                source: SignalSource::Ligma,
                severity: SignalSeverity::Warning,
                reason: format!(
                    "Liquidity thinning: impact={:.0}bps tradability={:.3} reserve={:.2}SOL",
                    impact_bps, tradability, reserve_sol
                ),
                confidence: 0.80,
                timestamp_ms: now_ms,
                raw_score: Some(impact_bps),
            })
            .await;
        }
    }

    // ═════════════════════════════════════════════════════════════════
    // Module 2: WHF — Wash Trading & Harmonic Field
    // ═════════════════════════════════════════════════════════════════

    async fn run_whf_check(&self, base_mint: &Pubkey, snapshots: &[MarketSnapshot], now_ms: u64) {
        if snapshots.len() < 3 {
            return;
        }

        // Compute volume deltas between consecutive snapshots
        let volumes: Vec<f64> = snapshots
            .windows(2)
            .map(|w| (w[1].cum_volume_sol - w[0].cum_volume_sol).abs())
            .collect();

        let prices: Vec<f64> = snapshots
            .iter()
            .filter(|s| s.price_sol_per_token > 0.0)
            .map(|s| s.price_sol_per_token)
            .collect();

        if volumes.len() < 2 || prices.len() < 2 {
            return;
        }

        // Total flow volume in window
        let net_flow: f64 = volumes.iter().sum();

        // Price change over the window
        let first_price = *prices.first().unwrap();
        let last_price = *prices.last().unwrap();
        let price_change = if first_price > 1e-12 {
            (last_price - first_price) / first_price
        } else {
            0.0
        };

        // Volume coefficient of variation
        let volume_cv = {
            let n = volumes.len() as f64;
            let mean = net_flow / n;
            if mean > 0.0 {
                let variance = volumes.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
                variance.sqrt() / mean
            } else {
                0.0
            }
        };

        let Some((pool_amm_id, lane, position_id)) = self.position_signal_context(base_mint) else {
            return;
        };

        // ── Check 1: Wash Trading (high volume, no price movement) ──
        // High volume but price barely moves → circular wash trading
        if net_flow > self.config.whf_min_net_flow_sol
            && price_change.abs() < self.config.whf_wash_max_price_change
        {
            let wash_score = (1.0 - price_change.abs() / self.config.whf_wash_max_price_change)
                .clamp(0.0, 1.0) as f32;

            if wash_score > self.config.whf_min_confidence {
                let severity = if self.config.whf_wash_trading_is_critical {
                    SignalSeverity::Critical
                } else {
                    SignalSeverity::Warning
                };

                self.emit_signal(GuardianSignal {
                    lane,
                    position_id: Some(position_id.clone()),
                    base_mint: *base_mint,
                    pool_amm_id,
                    source: SignalSource::Whf,
                    severity,
                    reason: format!(
                        "Wash trading: vol={:.2}SOL price_Δ={:.4}% wash_score={:.2}",
                        net_flow,
                        price_change * 100.0,
                        wash_score
                    ),
                    confidence: wash_score,
                    timestamp_ms: now_ms,
                    raw_score: Some(wash_score as f64),
                })
                .await;
            }
        }

        // ── Check 2: Trend Decay (distribution phase) ───────────────
        // Price dropping with uniform (low CV) selling → controlled dump
        if price_change < -self.config.whf_decay_min_price_drop
            && volume_cv < self.config.whf_decay_max_volume_cv
        {
            self.emit_signal(GuardianSignal {
                lane,
                position_id: Some(position_id),
                base_mint: *base_mint,
                pool_amm_id,
                source: SignalSource::Whf,
                severity: SignalSeverity::Warning,
                reason: format!(
                    "Trend decay: price_Δ={:.2}% uniform_selling(cv={:.2})",
                    price_change * 100.0,
                    volume_cv
                ),
                confidence: 0.70,
                timestamp_ms: now_ms,
                raw_score: Some(price_change),
            })
            .await;
        }
    }

    // ═════════════════════════════════════════════════════════════════
    // Module 3: TCF — Trend Cohesion Field
    // ═════════════════════════════════════════════════════════════════

    async fn run_tcf_check(&self, base_mint: &Pubkey, snapshots: &[MarketSnapshot], now_ms: u64) {
        if snapshots.len() < 2 {
            return;
        }

        let latest = &snapshots[snapshots.len() - 1];
        let prev_idx = snapshots.len().saturating_sub(2);
        let prev = &snapshots[prev_idx];

        // Build MarketObservation from snapshot deltas
        let price_delta = if prev.price_sol_per_token > 1e-12 {
            ((latest.price_sol_per_token - prev.price_sol_per_token) / prev.price_sol_per_token)
                .clamp(-1.0, 1.0)
        } else {
            0.0
        };

        let volume_delta = {
            let vol_diff = latest.cum_volume_sol - prev.cum_volume_sol;
            // Normalize to [-1, 1] using sigmoid-like scaling
            let scale = 1.0; // 1 SOL as reference
            (vol_diff / scale).clamp(-1.0, 1.0)
        };

        // Liquidity entropy from reserve ratio movement
        let liquidity_entropy = if latest.reserve_quote > 1e-12 && prev.reserve_quote > 1e-12 {
            let ratio_change = (latest.reserve_quote / prev.reserve_quote - 1.0).abs();
            (1.0 - ratio_change).clamp(0.0, 1.0)
        } else {
            0.5
        };

        // Order flow imbalance from d_price_d_volume gradient
        let order_flow_imbalance = latest.d_price_d_volume.clamp(-1.0, 1.0);

        let observation = MarketObservation::new(
            price_delta,
            volume_delta,
            liquidity_entropy,
            order_flow_imbalance,
            0.0, // mpcf — not available post-buy
            0.0, // jitter — not tracked here
            0.0, // phase_sync — not tracked here
        );

        // Update TCF and read results
        let (tcf_score, cliff_detected, consecutive, pool_amm_id, lane, position_id) = {
            let mut positions = self.positions.write();
            let Some(pos) = positions.get_mut(base_mint) else {
                return;
            };

            let result = pos.tcf.update(&observation);
            let tcf_score = result.tcf_score;
            let cliff_detected = result.cliff_detected;

            pos.last_tcf_score = tcf_score;

            // Track consecutive low cohesion
            if tcf_score < self.config.tcf_critical_cohesion {
                pos.consecutive_low_cohesion += 1;
            } else if tcf_score > self.config.tcf_warning_cohesion {
                pos.consecutive_low_cohesion = 0;
            }

            let consecutive = pos.consecutive_low_cohesion;
            let pool_amm_id = pos.pool_amm_id;
            let lane = pos.lane;
            let position_id = pos.position_id.clone();

            (
                tcf_score,
                cliff_detected,
                consecutive,
                pool_amm_id,
                lane,
                position_id,
            )
        };

        // Evaluate thresholds
        if tcf_score < self.config.tcf_critical_cohesion
            || consecutive >= self.config.tcf_consecutive_low_max
        {
            self.emit_signal(GuardianSignal {
                lane,
                position_id: Some(position_id.clone()),
                base_mint: *base_mint,
                pool_amm_id,
                source: SignalSource::Tcf,
                severity: SignalSeverity::Critical,
                reason: format!(
                    "Trend regime collapse: tcf={:.3} consecutive_low={}/{}",
                    tcf_score, consecutive, self.config.tcf_consecutive_low_max
                ),
                confidence: 0.85,
                timestamp_ms: now_ms,
                raw_score: Some(tcf_score),
            })
            .await;
        } else if tcf_score < self.config.tcf_warning_cohesion
            || (cliff_detected && self.config.tcf_cliff_is_warning)
        {
            self.emit_signal(GuardianSignal {
                lane,
                position_id: Some(position_id),
                base_mint: *base_mint,
                pool_amm_id,
                source: SignalSource::Tcf,
                severity: SignalSeverity::Warning,
                reason: format!(
                    "Trend weakening: tcf={:.3} cliff={}",
                    tcf_score, cliff_detected
                ),
                confidence: 0.70,
                timestamp_ms: now_ms,
                raw_score: Some(tcf_score),
            })
            .await;
        }
    }

    // ═════════════════════════════════════════════════════════════════
    // Module 4: PANIC — Congestion & Impulse Detection
    // ═════════════════════════════════════════════════════════════════

    async fn run_panic_check(&self, base_mint: &Pubkey, snapshots: &[MarketSnapshot], now_ms: u64) {
        if snapshots.len() < 3 {
            return;
        }

        let window_start = now_ms.saturating_sub(self.config.panic_rate_window_ms);

        // Filter snapshots within the rate window
        let recent: Vec<&MarketSnapshot> = snapshots
            .iter()
            .filter(|s| s.timestamp_ms >= window_start)
            .collect();

        if recent.len() < 2 {
            return;
        }

        let first = recent[0];
        let last = recent[recent.len() - 1];

        let time_span_s = (last.timestamp_ms.saturating_sub(first.timestamp_ms)) as f64 / 1000.0;
        if time_span_s < 0.1 {
            return;
        }

        // TX rate in the window
        let tx_delta = last.tx_count.saturating_sub(first.tx_count);
        let tx_rate = tx_delta as f64 / time_span_s;

        // Entropy of inter-snapshot intervals (low entropy = coordinated/regular timing)
        let intervals: Vec<f64> = recent
            .windows(2)
            .map(|w| w[1].timestamp_ms.saturating_sub(w[0].timestamp_ms) as f64)
            .filter(|&i| i > 0.0)
            .collect();

        let interval_entropy = compute_shannon_entropy(&intervals);

        let Some((pool_amm_id, lane, position_id)) = self.position_signal_context(base_mint) else {
            return;
        };

        // Evaluate: high TX rate + low entropy = coordinated activity
        if tx_rate >= self.config.panic_critical_txps {
            self.emit_signal(GuardianSignal {
                lane,
                position_id: Some(position_id.clone()),
                base_mint: *base_mint,
                pool_amm_id,
                source: SignalSource::Panic,
                severity: SignalSeverity::Critical,
                reason: format!(
                    "Panic impulse: {:.1} TX/s (entropy={:.2}) — coordinated sell-off",
                    tx_rate, interval_entropy
                ),
                confidence: 0.90,
                timestamp_ms: now_ms,
                raw_score: Some(tx_rate),
            })
            .await;
        } else if tx_rate >= self.config.panic_warning_txps
            && interval_entropy < self.config.panic_low_entropy_threshold
        {
            self.emit_signal(GuardianSignal {
                lane,
                position_id: Some(position_id),
                base_mint: *base_mint,
                pool_amm_id,
                source: SignalSource::Panic,
                severity: SignalSeverity::Warning,
                reason: format!(
                    "Elevated sell pressure: {:.1} TX/s (entropy={:.2})",
                    tx_rate, interval_entropy
                ),
                confidence: 0.75,
                timestamp_ms: now_ms,
                raw_score: Some(tx_rate),
            })
            .await;
        }
    }

    // ═════════════════════════════════════════════════════════════════
    // Signal emission & cleanup
    // ═════════════════════════════════════════════════════════════════

    async fn emit_signal(&self, signal: GuardianSignal) {
        // Store in position's signal history
        {
            let mut positions = self.positions.write();
            if let Some(pos) = positions.get_mut(&signal.base_mint) {
                pos.recent_signals.push(TimestampedSignal {
                    timestamp_ms: signal.timestamp_ms,
                    signal: signal.clone(),
                });
            }
        }

        // Log with appropriate level
        match signal.severity {
            SignalSeverity::Info => debug!("{}", signal),
            SignalSeverity::Warning => warn!("{}", signal),
            SignalSeverity::Critical => error!("{}", signal),
        }

        // Send to signal router (non-blocking)
        if let Err(e) = self.signal_tx.try_send(signal) {
            warn!("🛡️ PostBuyGuardian: Signal channel full or closed: {}", e);
        }
    }

    fn cleanup_old_signals(&self, base_mint: &Pubkey, now_ms: u64) {
        let window_start = now_ms.saturating_sub(self.config.signal_aggregation_window_ms * 2);

        let mut positions = self.positions.write();
        if let Some(pos) = positions.get_mut(base_mint) {
            // Remove signals older than 2× aggregation window
            pos.recent_signals
                .retain(|ts| ts.timestamp_ms >= window_start);

            // Cap total signals per position
            if pos.recent_signals.len() > self.config.max_signals_per_position {
                let excess = pos.recent_signals.len() - self.config.max_signals_per_position;
                pos.recent_signals.drain(..excess);
            }
        }
    }

    async fn run_aem_tick(&self, base_mint: &Pubkey, snapshots: &[MarketSnapshot], now_ms: u64) {
        let Some(ref aem_runtime) = self.aem_runtime else {
            return;
        };
        let Some(ref router) = self.position_router else {
            return;
        };
        let noop_ledger = NoopAemLedgerWriter;
        let ledger_writer: &dyn AemLedgerWriter = match self.aem_ledger.as_ref() {
            Some(ledger) => ledger.as_ref(),
            None => &noop_ledger,
        };

        if snapshots.is_empty() {
            return;
        }

        let latest = snapshots[snapshots.len() - 1].clone();
        let prev = if snapshots.len() >= 2 {
            Some(snapshots[snapshots.len() - 2].clone())
        } else {
            None
        };

        let (
            lane,
            candidate_id,
            position_id,
            position_epoch,
            entry_order_id,
            quote_id,
            slot,
            pool_amm_id,
            base_mint_copy,
            entry_unix_ms,
            entry_metric,
            current_metric,
            peak,
            drawdown_pct,
            unrealized_pnl_pct,
            slope_pct_per_s,
            volatility_proxy,
            reclaim_flag,
            time_since_entry_s,
            time_since_last_peak_s,
            previous_bucket,
            should_register_aem,
            should_register_runtime,
        ) = {
            let mut positions = self.positions.write();
            let Some(pos) = positions.get_mut(base_mint) else {
                return;
            };

            let current_metric = if pos.entry_price_sol.unwrap_or(0.0) > 0.0 {
                latest.price_sol_per_token
            } else {
                latest.market_cap_sol
            };
            let entry_metric = pos.entry_price_sol.unwrap_or_else(|| {
                if pos.peak_since_entry > 0.0 {
                    pos.peak_since_entry
                } else {
                    current_metric.max(1e-9)
                }
            });

            if current_metric > pos.peak_since_entry {
                pos.peak_since_entry = current_metric;
                pos.last_peak_unix_ms = now_ms;
            }

            let peak = pos
                .peak_since_entry
                .max(current_metric)
                .max(entry_metric)
                .max(1e-9);
            let drawdown_pct = ((peak - current_metric) / peak * 100.0).max(0.0);
            let unrealized_pnl_pct =
                ((current_metric - entry_metric) / entry_metric.max(1e-9)) * 100.0;

            let slope_pct_per_s = prev
                .as_ref()
                .and_then(|p| {
                    let prev_metric = if pos.entry_price_sol.unwrap_or(0.0) > 0.0 {
                        p.price_sol_per_token
                    } else {
                        p.market_cap_sol
                    };
                    let dt_ms = latest.timestamp_ms.saturating_sub(p.timestamp_ms);
                    if dt_ms == 0 || prev_metric <= 0.0 {
                        None
                    } else {
                        let dt_s = dt_ms as f64 / 1000.0;
                        Some(
                            ((current_metric - prev_metric) / prev_metric) * 100.0 / dt_s.max(1e-6),
                        )
                    }
                })
                .unwrap_or(0.0);

            let volatility_proxy = if snapshots.len() >= 3 {
                let mut returns = Vec::new();
                for w in snapshots[snapshots.len().saturating_sub(5)..].windows(2) {
                    let v0 = if pos.entry_price_sol.unwrap_or(0.0) > 0.0 {
                        w[0].price_sol_per_token
                    } else {
                        w[0].market_cap_sol
                    };
                    let v1 = if pos.entry_price_sol.unwrap_or(0.0) > 0.0 {
                        w[1].price_sol_per_token
                    } else {
                        w[1].market_cap_sol
                    };
                    if v0 > 0.0 {
                        returns.push((v1 - v0) / v0);
                    }
                }
                if returns.len() >= 2 {
                    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
                    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                        / returns.len() as f64;
                    Some(var.sqrt())
                } else {
                    None
                }
            } else {
                None
            };

            let reclaim_flag = if current_metric >= entry_metric {
                ReclaimFlag::Full
            } else if current_metric >= entry_metric * 0.9 {
                ReclaimFlag::Partial
            } else {
                ReclaimFlag::None
            };

            (
                pos.lane,
                pos.candidate_id.clone(),
                pos.position_id.clone(),
                pos.position_epoch,
                pos.entry_order_id.clone(),
                pos.quote_id.clone(),
                pos.slot,
                pos.pool_amm_id,
                pos.base_mint,
                pos.entry_unix_ms,
                entry_metric,
                current_metric,
                peak,
                drawdown_pct,
                unrealized_pnl_pct,
                slope_pct_per_s,
                volatility_proxy,
                reclaim_flag,
                now_ms
                    .saturating_sub(pos.entry_unix_ms)
                    .saturating_div(1000) as u32,
                now_ms
                    .saturating_sub(pos.last_peak_unix_ms)
                    .saturating_div(1000) as u32,
                pos.last_stress_bucket,
                !pos.aem_registered,
                matches!(pos.lane, Lane::Shadow) && !pos.runtime_registered,
            )
        };

        if should_register_runtime
            && matches!(lane, Lane::Shadow)
            && !self.ensure_shadow_runtime_registered(base_mint).await
        {
            return;
        }

        let default_stress = crate::aem::ExecutionStressSnapshot {
            requeue_count: 0,
            send_fail_count: 0,
            relax_count: 0,
            oracle_stale_age_ms: 0,
            last_sell_attempt_age_ms: None,
        };

        let mut registered_aem = false;
        let emitter_for_runtime = self.event_emitter.as_deref();

        let (stress, stress_bucket, report) = if matches!(lane, Lane::Shadow) {
            let Some(shadow_book) = router.shadow_book() else {
                return;
            };
            let mut shadow_book = shadow_book.write().await;
            let mut adapter = ShadowPositionBookAemAdapter::new(&mut shadow_book);

            if should_register_aem {
                let mut runtime = aem_runtime.lock();
                let _ = runtime.register_position(
                    position_id.clone(),
                    pool_amm_id,
                    base_mint_copy,
                    entry_unix_ms,
                    entry_metric,
                    position_epoch,
                );
                drop(runtime);
                adapter.register_position_epoch(&position_id, position_epoch);
                registered_aem = true;
            }

            let stress = adapter
                .get_execution_stress(&position_id)
                .unwrap_or_else(|| default_stress.clone());
            let stress_bucket = self.classify_stress(
                stress.requeue_count,
                stress.send_fail_count,
                stress.relax_count,
            );
            let features = StateFeatures {
                position_id: position_id.clone(),
                pool_amm_id,
                base_mint: base_mint_copy,
                entry_price_or_mcap: entry_metric,
                current_price_or_mcap: current_metric,
                peak_since_entry: peak,
                drawdown_pct,
                unrealized_pnl_pct,
                slope_pct_per_s,
                volatility_proxy,
                reclaim_flag,
                time_since_entry_s,
                time_since_last_peak_s,
                requeue_count: stress.requeue_count,
                send_fail_count: stress.send_fail_count,
                relax_count: stress.relax_count,
                oracle_stale_age_ms: stress.oracle_stale_age_ms,
                last_sell_attempt_age_ms: stress.last_sell_attempt_age_ms,
                stress_bucket,
            };
            let mut runtime = aem_runtime.lock();
            let report = match runtime.on_tick_with_report(
                features,
                now_ms,
                ledger_writer,
                &mut adapter,
                emitter_for_runtime,
                Some(candidate_id.as_str()),
            ) {
                Ok(report) => report,
                Err(error) => {
                    warn!("AEM on_tick_with_report failed: {}", error);
                    None
                }
            };
            (stress, stress_bucket, report)
        } else {
            let Some(revolver) = router.live_revolver() else {
                return;
            };
            let mut revolver = revolver.write().await;
            let mut adapter = RevolverAemAdapter::new(&mut revolver);

            if should_register_aem {
                let mut runtime = aem_runtime.lock();
                let _ = runtime.register_position(
                    position_id.clone(),
                    pool_amm_id,
                    base_mint_copy,
                    entry_unix_ms,
                    entry_metric,
                    position_epoch,
                );
                drop(runtime);
                adapter.register_position_epoch(&position_id, position_epoch);
                registered_aem = true;
            }

            let stress = adapter
                .get_execution_stress(&position_id)
                .unwrap_or_else(|| default_stress.clone());
            let stress_bucket = self.classify_stress(
                stress.requeue_count,
                stress.send_fail_count,
                stress.relax_count,
            );
            let features = StateFeatures {
                position_id: position_id.clone(),
                pool_amm_id,
                base_mint: base_mint_copy,
                entry_price_or_mcap: entry_metric,
                current_price_or_mcap: current_metric,
                peak_since_entry: peak,
                drawdown_pct,
                unrealized_pnl_pct,
                slope_pct_per_s,
                volatility_proxy,
                reclaim_flag,
                time_since_entry_s,
                time_since_last_peak_s,
                requeue_count: stress.requeue_count,
                send_fail_count: stress.send_fail_count,
                relax_count: stress.relax_count,
                oracle_stale_age_ms: stress.oracle_stale_age_ms,
                last_sell_attempt_age_ms: stress.last_sell_attempt_age_ms,
                stress_bucket,
            };
            let mut runtime = aem_runtime.lock();
            let report = match runtime.on_tick_with_report(
                features,
                now_ms,
                ledger_writer,
                &mut adapter,
                emitter_for_runtime,
                Some(candidate_id.as_str()),
            ) {
                Ok(report) => report,
                Err(error) => {
                    warn!("AEM on_tick_with_report failed: {}", error);
                    None
                }
            };
            (stress, stress_bucket, report)
        };

        {
            let mut positions = self.positions.write();
            if let Some(pos) = positions.get_mut(base_mint) {
                if registered_aem {
                    pos.aem_registered = true;
                }
                pos.last_stress_bucket = Some(stress_bucket);
            }
        }

        if let Some(emitter) = self.event_emitter.as_ref() {
            if let Some(prev) = previous_bucket {
                if prev != stress_bucket {
                    let mut env = emitter.make_envelope_at(&candidate_id, now_ms);
                    env.position_id = Some(position_id.clone());
                    env.position_epoch = Some(position_epoch);
                    env.order_id = Some(entry_order_id.clone());
                    env.quote_id = Some(quote_id.clone());
                    env.slot = slot;
                    emitter.emit_raw(ExecutionEvent::new(
                        env,
                        EventKind::ExecutionStressChanged(ExecutionStressChangedPayload {
                            previous_bucket: Self::to_exec_stress_bucket(prev),
                            new_bucket: Self::to_exec_stress_bucket(stress_bucket),
                            snapshot: Self::to_exec_stress_snapshot(&stress, stress_bucket),
                        }),
                    ));
                }
            }

            if stress.oracle_stale_age_ms > self.config.aem.oracle_stale_hard_ms {
                let mut env = emitter.make_envelope_at(&candidate_id, now_ms);
                env.position_id = Some(position_id.clone());
                env.position_epoch = Some(position_epoch);
                env.order_id = Some(entry_order_id.clone());
                env.quote_id = Some(quote_id.clone());
                env.slot = slot;
                emitter.emit_raw(ExecutionEvent::new(
                    env,
                    EventKind::OracleStale(OracleStalePayload {
                        stale_age_ms: stress.oracle_stale_age_ms,
                        threshold_ms: self.config.aem.oracle_stale_hard_ms,
                    }),
                ));
            }
        }

        if let Some(report) = report {
            let command = report.decision.control_command.clone();
            let command_id: ExecCommandId = report.decision.decision_event_id.clone();
            let directive = format!("{:?}", command.directive);
            let fraction_bps = match command.directive {
                crate::aem::CommandDirective::ForceExitFractionBps { fraction_bps } => {
                    Some(fraction_bps)
                }
                _ => None,
            };
            let freeze_until = match command.directive {
                crate::aem::CommandDirective::FreezePanic => Some(command.expires_at_unix_ms),
                _ => None,
            };
            let priority = format!("{:?}", command.priority);
            let is_force_exit = matches!(
                command.directive,
                crate::aem::CommandDirective::ForceExitAll
                    | crate::aem::CommandDirective::ForceExitFractionBps { .. }
            );
            let accepted_primary = report
                .apply_result
                .as_ref()
                .map(|r| r.accepted)
                .unwrap_or(false);
            let exit_fraction_bps = fraction_bps.unwrap_or(10_000);
            let emit_immediate_exit_events = is_force_exit && !matches!(lane, Lane::Shadow);

            if is_force_exit && accepted_primary {
                let mut positions = self.positions.write();
                if let Some(pos) = positions.get_mut(base_mint) {
                    pos.last_force_exit_reason_code = Some(command.reason_code.clone());
                    if emit_immediate_exit_events {
                        let applied_fraction_bps =
                            exit_fraction_bps.min(pos.remaining_fraction_bps);
                        let applied_fraction = applied_fraction_bps as f64 / 10_000.0;
                        let entry_price = pos
                            .entry_price_sol
                            .unwrap_or(report.decision.features_snapshot.entry_price_or_mcap);
                        let current_price = report.decision.features_snapshot.current_price_or_mcap;
                        if entry_price.is_finite()
                            && current_price.is_finite()
                            && entry_price > 0.0
                            && applied_fraction > 0.0
                        {
                            let pnl_pct_delta = ((current_price - entry_price) / entry_price)
                                * 100.0
                                * applied_fraction;
                            pos.realized_pnl_pct += pnl_pct_delta;
                            if pos.entry_size_lamports > 0 {
                                let entry_sol = pos.entry_size_lamports as f64 / 1_000_000_000.0;
                                pos.realized_pnl_sol += entry_sol * (pnl_pct_delta / 100.0);
                            }
                        }
                        pos.total_exits = pos.total_exits.saturating_add(1);
                        pos.remaining_fraction_bps = pos
                            .remaining_fraction_bps
                            .saturating_sub(applied_fraction_bps);
                        if pos.remaining_fraction_bps == 0 {
                            pos.last_close_reason = Some(Self::close_reason_from_reason_code(
                                Some(command.reason_code.as_str()),
                            ));
                        }
                    }
                }
            }

            let mut emitters: Vec<(&Arc<EventEmitter>, bool)> = Vec::new();
            if let Some(emitter) = self.event_emitter.as_ref() {
                emitters.push((emitter, false));
            }
            if let Some(emitter) = self.event_emitter_secondary.as_ref() {
                emitters.push((emitter, true));
            }

            for (emitter, mirrored_lane) in emitters {
                let mut issued_env = emitter.make_envelope_at(&candidate_id, now_ms);
                issued_env.position_id = Some(position_id.clone());
                issued_env.position_epoch = Some(position_epoch);
                issued_env.order_id = Some(entry_order_id.clone());
                issued_env.command_id = Some(command_id.clone());
                issued_env.quote_id = Some(quote_id.clone());
                issued_env.slot = slot;
                emitter.emit_raw(ExecutionEvent::new(
                    issued_env,
                    EventKind::ControlCommandIssued(ControlCommandIssuedPayload {
                        directive: directive.clone(),
                        fraction_bps,
                        freeze_until_ms: freeze_until,
                        issued_at_ms: command.issued_at_unix_ms,
                        valid_from_ms: command.valid_from_unix_ms,
                        expires_at_ms: command.expires_at_unix_ms,
                        epoch: command.position_epoch,
                        priority: priority.clone(),
                        reason_code: command.reason_code.clone(),
                    }),
                ));

                let mut applied_env = emitter.make_envelope_at(&candidate_id, now_ms);
                applied_env.position_id = Some(position_id.clone());
                applied_env.position_epoch = Some(position_epoch);
                applied_env.order_id = Some(entry_order_id.clone());
                applied_env.command_id = Some(command_id.clone());
                applied_env.quote_id = Some(quote_id.clone());
                applied_env.slot = slot;
                let accepted = if mirrored_lane {
                    false
                } else {
                    report
                        .apply_result
                        .as_ref()
                        .map(|r| r.accepted)
                        .unwrap_or(false)
                };
                let reject_reason = if mirrored_lane {
                    Some("priority_lock".to_string())
                } else {
                    report
                        .apply_result
                        .as_ref()
                        .and_then(|r| r.reject_reason.clone())
                };
                emitter.emit_raw(ExecutionEvent::new(
                    applied_env,
                    EventKind::ControlCommandApplied(ControlCommandAppliedPayload {
                        accepted,
                        reject_reason,
                        applied_at_ms: now_ms,
                    }),
                ));

                if emit_immediate_exit_events {
                    let exit_order_id = if mirrored_lane {
                        format!("exit-live-{}", command_id)
                    } else {
                        format!("exit-{}", command_id)
                    };

                    let mut exit_sub_env = emitter.make_envelope_at(&candidate_id, now_ms);
                    exit_sub_env.position_id = Some(position_id.clone());
                    exit_sub_env.position_epoch = Some(position_epoch);
                    exit_sub_env.order_id = Some(exit_order_id.clone());
                    exit_sub_env.command_id = Some(command_id.clone());
                    exit_sub_env.quote_id = Some(quote_id.clone());
                    exit_sub_env.slot = slot;
                    emitter.emit_raw(ExecutionEvent::new(
                        exit_sub_env,
                        EventKind::ExitSubmitted(ExitSubmittedPayload {
                            fraction_bps: exit_fraction_bps,
                            command_ref: Some(command_id.clone()),
                        }),
                    ));

                    let status = if accepted {
                        ExecFillStatus::Confirmed
                    } else {
                        ExecFillStatus::Failed
                    };
                    let mut exit_fill_env = emitter.make_envelope_at(&candidate_id, now_ms);
                    exit_fill_env.position_id = Some(position_id.clone());
                    exit_fill_env.position_epoch = Some(position_epoch);
                    exit_fill_env.order_id = Some(exit_order_id);
                    exit_fill_env.command_id = Some(command_id.clone());
                    exit_fill_env.quote_id = Some(quote_id.clone());
                    exit_fill_env.slot = slot;
                    emitter.emit_raw(ExecutionEvent::new(
                        exit_fill_env,
                        EventKind::ExitFilled(ExitFilledPayload {
                            fill_price: report.decision.features_snapshot.current_price_or_mcap,
                            fill_qty: 0,
                            realized_pnl_delta: 0.0,
                            status,
                            is_partial: exit_fraction_bps < 10_000,
                            remaining_qty: if accepted {
                                self.positions
                                    .read()
                                    .get(base_mint)
                                    .map(|pos| u64::from(pos.remaining_fraction_bps))
                                    .unwrap_or(0)
                            } else {
                                0
                            },
                        }),
                    ));
                }
            }
        }
    }

    async fn run_shadow_runtime_tick(
        &self,
        base_mint: &Pubkey,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) {
        if self.shadow_simple_exit_thresholds.is_some() {
            self.run_shadow_simple_threshold_tick(base_mint, latest, now_ms)
                .await;
            return;
        }

        let Some(ref router) = self.position_router else {
            return;
        };
        let Some(shadow_book) = router.shadow_book() else {
            return;
        };
        if !self.ensure_shadow_runtime_registered(base_mint).await {
            return;
        }

        let (
            candidate_id,
            position_id,
            position_epoch,
            entry_order_id,
            quote_id,
            slot,
            entry_price_opt,
            entry_unix_ms,
            last_market_activity_seen_ms,
            snapshot_source,
        ) = {
            let positions = self.positions.read();
            let Some(pos) = positions.get(base_mint) else {
                return;
            };
            if !matches!(pos.lane, Lane::Shadow) {
                return;
            }
            (
                pos.candidate_id.clone(),
                pos.position_id.clone(),
                pos.position_epoch,
                pos.entry_order_id.clone(),
                pos.quote_id.clone(),
                pos.slot,
                pos.entry_price_sol,
                pos.entry_unix_ms,
                pos.shadow_market_activity.last_seen_ms,
                pos.last_snapshot_source,
            )
        };

        let Some(entry_price_sol) =
            entry_price_opt.and_then(|price| (price.is_finite() && price > 0.0).then_some(price))
        else {
            warn!(
                position_id = %position_id,
                "PostBuyGuardian: shadow runtime missing authoritative entry price; refusing synthetic fallback"
            );
            return;
        };

        let inactivity_elapsed_ms = now_ms.saturating_sub(last_market_activity_seen_ms);
        let time_stop_due = inactivity_elapsed_ms >= self.shadow_position_time_stop_ms();
        let latest_snapshot = latest.cloned();
        let Some(latest_snapshot) = latest_snapshot else {
            if time_stop_due {
                let evidence = PriceTruthEvidence {
                    source: snapshot_source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow time-stop expired before any canonical snapshot reached guardian"
                            .to_string(),
                    ),
                    slot,
                    timestamp_ms: Some(now_ms),
                    age_ms: None,
                    price_state: None,
                    price_reason: None,
                };
                self.force_close_shadow_without_exit_truth(
                    base_mint,
                    &position_id,
                    now_ms,
                    evidence,
                )
                .await;
            }
            return;
        };
        self.remember_shadow_snapshot(base_mint, &latest_snapshot);
        let Some(current_price_sol) =
            PriceTruthResolver::normalize_shadow_snapshot_price_sol(&latest_snapshot)
        else {
            if time_stop_due {
                let evidence = PriceTruthEvidence {
                    source: snapshot_source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow snapshot price could not be normalized into canonical SOL/token"
                            .to_string(),
                    ),
                    slot,
                    timestamp_ms: Some(latest_snapshot.timestamp_ms),
                    age_ms: Some(now_ms.saturating_sub(latest_snapshot.timestamp_ms)),
                    price_state: Some(latest_snapshot.price_state),
                    price_reason: latest_snapshot.price_reason,
                };
                self.force_close_shadow_without_exit_truth(
                    base_mint,
                    &position_id,
                    now_ms,
                    evidence,
                )
                .await;
            }
            return;
        };

        let mut exit_preview = shadow_book
            .read()
            .await
            .preview_exit(base_mint, current_price_sol);
        let mut triggered_fraction_bps = exit_preview.fraction_bps;
        if time_stop_due && exit_preview.has_time_stop_trigger {
            self.remember_shadow_time_stop_reason(base_mint);
        }
        if triggered_fraction_bps == 0 && time_stop_due {
            {
                let mut positions = self.positions.write();
                if let Some(pos) = positions.get_mut(base_mint) {
                    if pos.last_force_exit_reason_code.is_none() {
                        pos.last_force_exit_reason_code = Some("time_stop".to_string());
                    }
                }
            }
            if shadow_book.write().await.force_exit_all(base_mint) {
                info!(
                    position_id = %position_id,
                    inactivity_elapsed_ms,
                    position_age_ms = now_ms.saturating_sub(entry_unix_ms),
                    sample_age_ms = now_ms.saturating_sub(latest_snapshot.timestamp_ms),
                    "PostBuyGuardian: shadow inactivity time-stop forcing full exit"
                );
            }
            exit_preview = shadow_book
                .read()
                .await
                .preview_exit(base_mint, current_price_sol);
            triggered_fraction_bps = exit_preview.fraction_bps;
            if time_stop_due && exit_preview.has_time_stop_trigger {
                self.remember_shadow_time_stop_reason(base_mint);
            }
        }
        if triggered_fraction_bps == 0 {
            return;
        }

        let sample = match Self::resolve_shadow_exit_sample_for_runtime(
            &latest_snapshot,
            now_ms,
            self.shadow_exit_stale_after_ms(),
            snapshot_source,
        ) {
            Ok(sample) => {
                let mut positions = self.positions.write();
                if let Some(pos) = positions.get_mut(base_mint) {
                    pos.last_blocked_truth_status = None;
                    pos.last_blocked_truth_timestamp_ms = None;
                }
                sample
            }
            Err(error) => {
                if time_stop_due {
                    let evidence = match &error {
                        trigger::PriceTruthError::Stale { evidence, .. } => {
                            let exit_token_amount_raw = self
                                .positions
                                .read()
                                .get(base_mint)
                                .map(|pos| pos.remaining_token_amount_raw)
                                .unwrap_or(0);
                            Self::stale_time_stop_rejection_evidence(
                                &position_id,
                                &latest_snapshot,
                                now_ms,
                                exit_token_amount_raw,
                                evidence,
                            )
                        }
                        _ => error.evidence().clone(),
                    };
                    self.force_close_shadow_without_exit_truth(
                        base_mint,
                        &position_id,
                        now_ms,
                        evidence,
                    )
                    .await;
                    return;
                }
                self.maybe_record_shadow_exit_blocked(
                    base_mint,
                    now_ms,
                    triggered_fraction_bps,
                    error.evidence(),
                );
                warn!(
                    position_id = %position_id,
                    truth_status = ?error.status(),
                    error = %error,
                    "PostBuyGuardian: shadow exit blocked because price truth is unavailable"
                );
                return;
            }
        };

        let exits = shadow_book.write().await.process_market_snapshot(
            base_mint,
            sample.exit_price_sol,
            now_ms,
        );
        if exits.is_empty() {
            return;
        }

        for exit in exits {
            let exit_token_amount_result = {
                let positions = self.positions.read();
                let Some(pos) = positions.get(base_mint) else {
                    return;
                };
                Self::shadow_exit_token_amount_raw(pos, &exit)
            };
            let exit_token_amount_raw = match exit_token_amount_result {
                Ok(amount) => amount,
                Err(detail) => {
                    let evidence = PriceTruthEvidence {
                        source: sample.evidence.source,
                        status: PriceTruthStatus::Failure,
                        detail: Some(detail),
                        slot: sample.evidence.slot,
                        timestamp_ms: sample.evidence.timestamp_ms,
                        age_ms: sample.evidence.age_ms,
                        price_state: sample.evidence.price_state,
                        price_reason: sample.evidence.price_reason,
                    };
                    if time_stop_due {
                        self.force_close_shadow_without_exit_truth(
                            base_mint,
                            &position_id,
                            now_ms,
                            evidence,
                        )
                        .await;
                    } else {
                        self.maybe_record_shadow_exit_blocked(
                            base_mint,
                            now_ms,
                            exit.fraction_bps,
                            &evidence,
                        );
                        warn!(
                            position_id = %position_id,
                            detail = %evidence.detail.as_deref().unwrap_or("shadow_exit_qty_missing"),
                            "PostBuyGuardian: shadow exit blocked because authoritative token quantity is unavailable"
                        );
                    }
                    return;
                }
            };
            let truth = match PriceTruthResolver::resolve_shadow_exit(
                entry_price_sol,
                exit_token_amount_raw,
                &sample,
                0.0,
            ) {
                Ok(truth) => truth,
                Err(error) => {
                    if time_stop_due {
                        self.force_close_shadow_without_exit_truth(
                            base_mint,
                            &position_id,
                            now_ms,
                            error.evidence().clone(),
                        )
                        .await;
                    } else {
                        self.maybe_record_shadow_exit_blocked(
                            base_mint,
                            now_ms,
                            exit.fraction_bps,
                            error.evidence(),
                        );
                        warn!(
                            position_id = %position_id,
                            truth_status = ?error.status(),
                            error = %error,
                            "PostBuyGuardian: shadow exit truth failed after trigger"
                        );
                    }
                    return;
                }
            };
            self.apply_shadow_exit_execution(base_mint, &exit, &truth);
            self.emit_shadow_exit(
                base_mint,
                &candidate_id,
                &position_id,
                position_epoch,
                &entry_order_id,
                &quote_id,
                slot,
                &exit,
                &truth,
                now_ms,
            );
        }
    }

    async fn run_shadow_simple_threshold_tick(
        &self,
        base_mint: &Pubkey,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) {
        let Some(thresholds) = self.shadow_simple_exit_thresholds else {
            return;
        };

        let (
            candidate_id,
            position_id,
            position_epoch,
            entry_order_id,
            quote_id,
            slot,
            entry_price_opt,
            entry_unix_ms,
            last_market_activity_seen_ms,
            snapshot_source,
            remaining_fraction_bps,
        ) = {
            let positions = self.positions.read();
            let Some(pos) = positions.get(base_mint) else {
                return;
            };
            if !matches!(pos.lane, Lane::Shadow) {
                return;
            }
            (
                pos.candidate_id.clone(),
                pos.position_id.clone(),
                pos.position_epoch,
                pos.entry_order_id.clone(),
                pos.quote_id.clone(),
                pos.slot,
                pos.entry_price_sol,
                pos.entry_unix_ms,
                pos.shadow_market_activity.last_seen_ms,
                pos.last_snapshot_source,
                pos.remaining_fraction_bps,
            )
        };

        let Some(entry_price_sol) =
            entry_price_opt.and_then(|price| (price.is_finite() && price > 0.0).then_some(price))
        else {
            warn!(
                position_id = %position_id,
                "PostBuyGuardian: shadow simple exit missing authoritative entry price; refusing synthetic fallback"
            );
            return;
        };
        let Some((upper_exit_price_sol, lower_exit_price_sol)) =
            thresholds.prices_for_entry(entry_price_sol)
        else {
            warn!(
                position_id = %position_id,
                entry_price_sol,
                "PostBuyGuardian: shadow simple exit thresholds are invalid for the current entry price"
            );
            return;
        };

        let inactivity_elapsed_ms = now_ms.saturating_sub(last_market_activity_seen_ms);
        let time_stop_due = inactivity_elapsed_ms >= self.shadow_position_time_stop_ms();
        let latest_snapshot = latest.cloned();
        let Some(latest_snapshot) = latest_snapshot else {
            if time_stop_due {
                let evidence = PriceTruthEvidence {
                    source: snapshot_source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow time-stop expired before any canonical snapshot reached guardian"
                            .to_string(),
                    ),
                    slot,
                    timestamp_ms: Some(now_ms),
                    age_ms: None,
                    price_state: None,
                    price_reason: None,
                };
                self.force_close_shadow_without_exit_truth(
                    base_mint,
                    &position_id,
                    now_ms,
                    evidence,
                )
                .await;
            }
            return;
        };
        self.remember_shadow_snapshot(base_mint, &latest_snapshot);
        let Some(current_price_sol) =
            PriceTruthResolver::normalize_shadow_snapshot_price_sol(&latest_snapshot)
        else {
            if time_stop_due {
                let evidence = PriceTruthEvidence {
                    source: snapshot_source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow snapshot price could not be normalized into canonical SOL/token"
                            .to_string(),
                    ),
                    slot,
                    timestamp_ms: Some(latest_snapshot.timestamp_ms),
                    age_ms: Some(now_ms.saturating_sub(latest_snapshot.timestamp_ms)),
                    price_state: Some(latest_snapshot.price_state),
                    price_reason: latest_snapshot.price_reason,
                };
                self.force_close_shadow_without_exit_truth(
                    base_mint,
                    &position_id,
                    now_ms,
                    evidence,
                )
                .await;
            }
            return;
        };

        let Some(trigger) = Self::determine_shadow_simple_exit_trigger(
            current_price_sol,
            upper_exit_price_sol,
            lower_exit_price_sol,
            time_stop_due,
        ) else {
            return;
        };
        let triggered_fraction_bps = remaining_fraction_bps.max(1);

        let sample = match Self::resolve_shadow_exit_sample_for_runtime(
            &latest_snapshot,
            now_ms,
            self.shadow_exit_stale_after_ms(),
            snapshot_source,
        ) {
            Ok(sample) => {
                let mut positions = self.positions.write();
                if let Some(pos) = positions.get_mut(base_mint) {
                    pos.last_blocked_truth_status = None;
                    pos.last_blocked_truth_timestamp_ms = None;
                }
                sample
            }
            Err(error) => {
                if matches!(trigger, ShadowSimpleExitTrigger::TimeStop) {
                    let evidence = match &error {
                        trigger::PriceTruthError::Stale { evidence, .. } => {
                            let exit_token_amount_raw = self
                                .positions
                                .read()
                                .get(base_mint)
                                .map(|pos| pos.remaining_token_amount_raw)
                                .unwrap_or(0);
                            Self::stale_time_stop_rejection_evidence(
                                &position_id,
                                &latest_snapshot,
                                now_ms,
                                exit_token_amount_raw,
                                evidence,
                            )
                        }
                        _ => error.evidence().clone(),
                    };
                    self.force_close_shadow_without_exit_truth(
                        base_mint,
                        &position_id,
                        now_ms,
                        evidence,
                    )
                    .await;
                    return;
                }
                self.maybe_record_shadow_exit_blocked(
                    base_mint,
                    now_ms,
                    triggered_fraction_bps,
                    error.evidence(),
                );
                warn!(
                    position_id = %position_id,
                    trigger = trigger.as_label(),
                    truth_status = ?error.status(),
                    error = %error,
                    "PostBuyGuardian: shadow simple threshold exit blocked because price truth is unavailable"
                );
                return;
            }
        };

        let exit_token_amount_result = {
            let positions = self.positions.read();
            let Some(pos) = positions.get(base_mint) else {
                return;
            };
            if pos.remaining_token_amount_raw == 0 {
                Err(PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::Failure,
                    detail: Some("shadow remaining token amount is exhausted".to_string()),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                })
            } else {
                Ok(pos.remaining_token_amount_raw)
            }
        };
        let exit_token_amount_raw = match exit_token_amount_result {
            Ok(amount) => amount,
            Err(evidence) => {
                if matches!(trigger, ShadowSimpleExitTrigger::TimeStop) {
                    self.force_close_shadow_without_exit_truth(
                        base_mint,
                        &position_id,
                        now_ms,
                        evidence,
                    )
                    .await;
                } else {
                    self.maybe_record_shadow_exit_blocked(
                        base_mint,
                        now_ms,
                        triggered_fraction_bps,
                        &evidence,
                    );
                    warn!(
                        position_id = %position_id,
                        trigger = trigger.as_label(),
                        detail = %evidence.detail.as_deref().unwrap_or("shadow_exit_qty_missing"),
                        "PostBuyGuardian: shadow simple threshold exit blocked because authoritative token quantity is unavailable"
                    );
                }
                return;
            }
        };
        let truth = match PriceTruthResolver::resolve_shadow_exit(
            entry_price_sol,
            exit_token_amount_raw,
            &sample,
            0.0,
        ) {
            Ok(truth) => truth,
            Err(error) => {
                if matches!(trigger, ShadowSimpleExitTrigger::TimeStop) {
                    self.force_close_shadow_without_exit_truth(
                        base_mint,
                        &position_id,
                        now_ms,
                        error.evidence().clone(),
                    )
                    .await;
                } else {
                    self.maybe_record_shadow_exit_blocked(
                        base_mint,
                        now_ms,
                        triggered_fraction_bps,
                        error.evidence(),
                    );
                    warn!(
                        position_id = %position_id,
                        trigger = trigger.as_label(),
                        truth_status = ?error.status(),
                        error = %error,
                        "PostBuyGuardian: shadow simple threshold exit truth failed after trigger"
                    );
                }
                return;
            }
        };

        self.set_shadow_exit_reason_code(base_mint, trigger.reason_code());
        let exit = super::integration::ShadowExitExecution {
            position_id: position_id.clone(),
            position_epoch,
            fraction_bps: triggered_fraction_bps,
            remaining_fraction_bps: 0,
            fill_price: sample.exit_price_sol,
        };
        self.apply_shadow_exit_execution(base_mint, &exit, &truth);
        self.emit_shadow_exit(
            base_mint,
            &candidate_id,
            &position_id,
            position_epoch,
            &entry_order_id,
            &quote_id,
            slot,
            &exit,
            &truth,
            now_ms,
        );
        self.cleanup_closed_shadow_position(base_mint, &position_id)
            .await;
        info!(
            position_id = %position_id,
            trigger = trigger.as_label(),
            current_price_sol,
            entry_price_sol,
            upper_exit_price_sol,
            lower_exit_price_sol,
            inactivity_elapsed_ms,
            position_age_ms = now_ms.saturating_sub(entry_unix_ms),
            "PostBuyGuardian: shadow simple threshold exit executed"
        );
    }

    fn determine_shadow_simple_exit_trigger(
        current_price_sol: f64,
        upper_exit_price_sol: f64,
        lower_exit_price_sol: f64,
        time_stop_due: bool,
    ) -> Option<ShadowSimpleExitTrigger> {
        if current_price_sol <= lower_exit_price_sol {
            Some(ShadowSimpleExitTrigger::StopLoss)
        } else if current_price_sol >= upper_exit_price_sol {
            Some(ShadowSimpleExitTrigger::TakeProfit)
        } else if time_stop_due {
            Some(ShadowSimpleExitTrigger::TimeStop)
        } else {
            None
        }
    }

    fn set_shadow_exit_reason_code(&self, base_mint: &Pubkey, reason_code: &str) {
        let mut positions = self.positions.write();
        if let Some(pos) = positions.get_mut(base_mint) {
            pos.last_force_exit_reason_code = Some(reason_code.to_string());
        }
    }

    async fn cleanup_closed_shadow_position(&self, base_mint: &Pubkey, position_id: &str) {
        if let Some(router) = self.position_router.as_ref() {
            if let Some(shadow_book) = router.shadow_book() {
                let _ = shadow_book.write().await.remove_position(position_id);
            }
        }
        let shadow_backend = { self.shadow_backend.read().clone() };
        if let Some(shadow_backend) = shadow_backend {
            let _ = shadow_backend.unregister_position(position_id).await;
        }
        self.unregister_position(base_mint);
    }

    fn maybe_record_shadow_exit_blocked(
        &self,
        base_mint: &Pubkey,
        now_ms: u64,
        fraction_bps: u16,
        evidence: &PriceTruthEvidence,
    ) {
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(base_mint) else {
            return;
        };
        if pos.last_blocked_truth_status == Some(evidence.status)
            && pos.last_blocked_truth_timestamp_ms == evidence.timestamp_ms
        {
            return;
        }
        pos.last_blocked_truth_status = Some(evidence.status);
        pos.last_blocked_truth_timestamp_ms = evidence.timestamp_ms;

        let mut record = self.shadow_lifecycle_record_base(
            pos,
            ShadowLifecycleRecordType::ExitBlocked,
            now_ms,
            evidence,
        );
        record.fraction_bps = Some(fraction_bps);
        self.append_shadow_lifecycle_record(&record);
    }

    fn apply_shadow_exit_execution(
        &self,
        base_mint: &Pubkey,
        exit: &super::integration::ShadowExitExecution,
        truth: &ShadowExitTruth,
    ) {
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(base_mint) else {
            return;
        };

        pos.realized_exit_value_sol += truth.exit_value_sol;
        pos.estimated_costs_sol += truth.estimated_costs_sol;
        pos.realized_pnl_sol += truth.gross_pnl_sol;
        if pos.entry_value_sol > 0.0 {
            pos.realized_pnl_pct = (pos.realized_pnl_sol / pos.entry_value_sol) * 100.0;
        }
        pos.total_exits = pos.total_exits.saturating_add(1);
        pos.remaining_fraction_bps = exit.remaining_fraction_bps;
        pos.remaining_token_amount_raw = pos
            .remaining_token_amount_raw
            .saturating_sub(truth.exit_token_amount_raw);
        pos.last_price_truth = Some(truth.evidence.clone());
        pos.last_blocked_truth_status = None;
        pos.last_blocked_truth_timestamp_ms = None;
        if pos.remaining_fraction_bps == 0 {
            pos.remaining_token_amount_raw = 0;
            pos.last_close_reason = Some(Self::shadow_close_reason_from_reason_code(
                pos.last_force_exit_reason_code.as_deref(),
            ));
        }
    }

    fn emit_shadow_exit(
        &self,
        base_mint: &Pubkey,
        candidate_id: &str,
        position_id: &str,
        position_epoch: u64,
        entry_order_id: &str,
        quote_id: &str,
        slot: Option<u64>,
        exit: &super::integration::ShadowExitExecution,
        truth: &ShadowExitTruth,
        now_ms: u64,
    ) {
        let exit_order_id = format!(
            "shadow-exit:{}:{}:{}",
            position_id, now_ms, exit.remaining_fraction_bps
        );
        let remaining_qty = self
            .positions
            .read()
            .get(base_mint)
            .map(|pos| pos.remaining_token_amount_raw)
            .unwrap_or(0);

        if let Some(emitter) = self.event_emitter.as_ref() {
            let mut exit_sub_env = emitter.make_envelope_at(&candidate_id.to_string(), now_ms);
            exit_sub_env.position_id = Some(position_id.to_string());
            exit_sub_env.position_epoch = Some(position_epoch);
            exit_sub_env.order_id = Some(exit_order_id.clone());
            exit_sub_env.quote_id = Some(quote_id.to_string());
            exit_sub_env.slot = slot;
            exit_sub_env.command_id = Some(format!("shadow-runtime-{}", entry_order_id));
            emitter.emit_raw(ExecutionEvent::new(
                exit_sub_env,
                EventKind::ExitSubmitted(ExitSubmittedPayload {
                    fraction_bps: exit.fraction_bps,
                    command_ref: None,
                }),
            ));

            let mut exit_fill_env = emitter.make_envelope_at(&candidate_id.to_string(), now_ms);
            exit_fill_env.position_id = Some(position_id.to_string());
            exit_fill_env.position_epoch = Some(position_epoch);
            exit_fill_env.order_id = Some(exit_order_id);
            exit_fill_env.quote_id = Some(quote_id.to_string());
            exit_fill_env.slot = slot;
            emitter.emit_raw(ExecutionEvent::new(
                exit_fill_env,
                EventKind::ExitFilled(ExitFilledPayload {
                    fill_price: truth.exit_price_sol,
                    fill_qty: truth.exit_token_amount_raw,
                    realized_pnl_delta: truth.gross_pnl_sol,
                    status: ExecFillStatus::Confirmed,
                    is_partial: exit.remaining_fraction_bps > 0,
                    remaining_qty,
                }),
            ));
        }

        if let Some(pos) = self.positions.read().get(base_mint) {
            let mut record = self.shadow_lifecycle_record_base(
                &pos,
                ShadowLifecycleRecordType::ExitFilled,
                now_ms,
                &truth.evidence,
            );
            record.fraction_bps = Some(exit.fraction_bps);
            record.remaining_fraction_bps = exit.remaining_fraction_bps;
            record.exit_price = Some(truth.exit_price_sol);
            record.entry_value_sol = Some(truth.entry_value_sol);
            record.exit_value_sol = Some(truth.exit_value_sol);
            record.exit_token_amount_raw = Some(truth.exit_token_amount_raw);
            record.gross_pnl_sol = Some(truth.gross_pnl_sol);
            record.net_pnl_sol = Some(truth.net_pnl_sol);
            record.estimated_costs_sol = Some(truth.estimated_costs_sol);
            record.final_pnl = Some(truth.gross_pnl_sol);
            record.final_pnl_pct = Some(truth.pnl_pct);
            self.append_shadow_lifecycle_record(&record);
        }
    }

    fn shadow_exit_token_amount_raw(
        pos: &MonitoredPosition,
        exit: &super::integration::ShadowExitExecution,
    ) -> Result<u64, String> {
        if pos.entry_token_amount_raw == 0 {
            return Err("shadow entry token amount is missing".to_string());
        }
        if pos.remaining_token_amount_raw == 0 {
            return Err("shadow remaining token amount is exhausted".to_string());
        }
        if exit.fraction_bps == 0 || exit.fraction_bps > 10_000 {
            return Err(format!(
                "shadow exit fraction is outside the valid 1..=10000 range: {}",
                exit.fraction_bps
            ));
        }
        if exit.remaining_fraction_bps == 0 {
            return Ok(pos.remaining_token_amount_raw);
        }

        let proportional = (u128::from(pos.entry_token_amount_raw) * u128::from(exit.fraction_bps)
            / 10_000) as u64;
        Ok(proportional.max(1).min(pos.remaining_token_amount_raw))
    }

    fn close_reason_from_reason_code_with_default(
        reason_code: Option<&str>,
        default_reason: CloseReason,
    ) -> CloseReason {
        let reason_code = reason_code.unwrap_or_default().to_ascii_lowercase();
        if reason_code.contains("hard_safety") {
            CloseReason::HardSafety
        } else if reason_code.contains("panic") {
            CloseReason::Panic
        } else if reason_code.contains("stop_loss") || reason_code.contains("stop-loss") {
            CloseReason::StopLoss
        } else if reason_code.contains("time_stop") || reason_code.contains("time-stop") {
            CloseReason::TimeStop
        } else if reason_code.contains("manual") {
            CloseReason::Manual
        } else if reason_code.contains("target") {
            CloseReason::Target
        } else {
            default_reason
        }
    }

    fn close_reason_from_reason_code(reason_code: Option<&str>) -> CloseReason {
        Self::close_reason_from_reason_code_with_default(reason_code, CloseReason::Default)
    }

    fn shadow_close_reason_from_reason_code(reason_code: Option<&str>) -> CloseReason {
        Self::close_reason_from_reason_code_with_default(reason_code, CloseReason::Target)
    }

    fn flush_aem_outcomes(&self, now_ms: u64) {
        let Some(ref aem_runtime) = self.aem_runtime else {
            return;
        };
        let source = GuardianOutcomeSource {
            positions: Arc::clone(&self.positions),
        };
        let noop_ledger = NoopAemLedgerWriter;
        let ledger_writer: &dyn AemLedgerWriter = match self.aem_ledger.as_ref() {
            Some(ledger) => ledger.as_ref(),
            None => &noop_ledger,
        };
        let mut runtime = aem_runtime.lock();
        let outcomes =
            match runtime.flush_due_outcomes(now_ms, &source, ledger_writer, None, |_| None) {
                Ok(outcomes) => outcomes,
                Err(e) => {
                    warn!("AEM flush_due_outcomes failed: {}", e);
                    return;
                }
            };
        drop(runtime);

        if outcomes.is_empty() {
            return;
        }
        let positions = self.positions.read();
        for outcome in outcomes {
            let ctx = positions
                .values()
                .find(|p| p.position_id == outcome.position_id)
                .map(|p| (p.candidate_id.clone(), p.position_id.clone()));
            let (candidate_id, position_id) = ctx.unwrap_or_else(|| {
                (
                    format!("unknown_{}", outcome.position_id),
                    outcome.position_id.clone(),
                )
            });
            let payload = serde_json::to_value(&outcome).unwrap_or_else(|_| serde_json::json!({}));

            if let Some(emitter) = self.event_emitter.as_ref() {
                emitter.emit_management_outcome(
                    &candidate_id,
                    &position_id,
                    payload.clone(),
                    Some(outcome.decision_event_id.clone()),
                );
            }
            if let Some(emitter) = self.event_emitter_secondary.as_ref() {
                emitter.emit_management_outcome(
                    &candidate_id,
                    &position_id,
                    payload.clone(),
                    Some(outcome.decision_event_id.clone()),
                );
            }
        }
    }

    fn classify_stress(
        &self,
        requeue_count: u32,
        send_fail_count: u32,
        relax_count: u32,
    ) -> StressBucket {
        let cfg = &self.config.aem;
        if requeue_count >= cfg.stress_high_requeue_min
            || send_fail_count >= cfg.stress_high_send_fail_min
            || relax_count >= cfg.stress_high_relax_min
        {
            StressBucket::High
        } else if (requeue_count >= cfg.stress_med_requeue_min
            && requeue_count <= cfg.stress_med_requeue_max)
            || send_fail_count == cfg.stress_med_send_fail_eq
            || relax_count == cfg.stress_med_relax_eq
        {
            StressBucket::Med
        } else {
            StressBucket::Low
        }
    }

    fn to_exec_stress_bucket(bucket: StressBucket) -> ExecStressBucket {
        match bucket {
            StressBucket::Low => ExecStressBucket::Low,
            StressBucket::Med => ExecStressBucket::Med,
            StressBucket::High => ExecStressBucket::High,
        }
    }

    fn to_exec_stress_snapshot(
        snapshot: &crate::aem::ExecutionStressSnapshot,
        bucket: StressBucket,
    ) -> ExecStressSnapshot {
        ExecStressSnapshot {
            requeue_count: snapshot.requeue_count,
            send_fail_count: snapshot.send_fail_count,
            relax_count: snapshot.relax_count,
            oracle_stale_age_ms: snapshot.oracle_stale_age_ms,
            last_sell_attempt_age_ms: snapshot.last_sell_attempt_age_ms,
            stress_bucket: Self::to_exec_stress_bucket(bucket),
            concurrent_exits_count: 0,
            injected: false,
        }
    }
}

struct GuardianOutcomeSource {
    positions: Arc<RwLock<HashMap<Pubkey, MonitoredPosition>>>,
}

impl OutcomeFeatureSource for GuardianOutcomeSource {
    fn sample_outcome(
        &self,
        position_id: &str,
        decision_ts_unix_ms: u64,
        horizon_ms: u64,
    ) -> Option<OutcomeSample> {
        let positions = self.positions.read();
        let pos = positions.values().find(|p| p.position_id == position_id)?;
        let use_price = pos.entry_price_sol.unwrap_or(0.0) > 0.0;
        let snapshots = pos.snapshot_timeline.clone_snapshots();
        drop(positions);

        if snapshots.is_empty() {
            return Some(OutcomeSample {
                price_at_t: None,
                peak_in_t: None,
                reclaim_happened: false,
                time_to_reclaim_ms: None,
                outcome_data_gap: true,
            });
        }

        let window_end = decision_ts_unix_ms.saturating_add(horizon_ms);
        let in_window: Vec<&MarketSnapshot> = snapshots
            .iter()
            .filter(|s| s.timestamp_ms >= decision_ts_unix_ms && s.timestamp_ms <= window_end)
            .collect();

        if in_window.is_empty() {
            return Some(OutcomeSample {
                price_at_t: None,
                peak_in_t: None,
                reclaim_happened: false,
                time_to_reclaim_ms: None,
                outcome_data_gap: true,
            });
        }

        let decision_price = in_window
            .first()
            .map(|s| {
                if use_price {
                    s.price_sol_per_token
                } else {
                    s.market_cap_sol
                }
            })
            .unwrap_or(0.0);
        let mut peak = decision_price;
        let mut reclaim = false;
        let mut reclaim_time = None;

        for snap in &in_window {
            let value = if use_price {
                snap.price_sol_per_token
            } else {
                snap.market_cap_sol
            };
            if value > peak {
                peak = value;
            }
            if !reclaim && value >= decision_price {
                reclaim = true;
                reclaim_time = Some(snap.timestamp_ms.saturating_sub(decision_ts_unix_ms));
            }
        }
        let last = in_window.last().copied();

        Some(OutcomeSample {
            price_at_t: last.map(|s| {
                if use_price {
                    s.price_sol_per_token
                } else {
                    s.market_cap_sol
                }
            }),
            peak_in_t: Some(peak),
            reclaim_happened: reclaim,
            time_to_reclaim_ms: reclaim_time,
            outcome_data_gap: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Utility functions
// ═══════════════════════════════════════════════════════════════════════

/// Compute Shannon entropy of a sequence of positive values.
///
/// Returns 0.0 for empty or single-element sequences.
fn compute_shannon_entropy(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }

    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }

    let mut entropy = 0.0;
    for &v in values {
        if v > 0.0 {
            let p = v / total;
            entropy -= p * p.ln();
        }
    }
    entropy
}

/// Returns the current Unix timestamp in milliseconds.
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn synthetic_next_slot(slot: Option<u64>) -> Option<u64> {
    slot.and_then(|slot| slot.checked_add(1))
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventEmitter, EventWriterConfig};
    use crate::guardian::post_buy::integration::{PositionRuntimeRouter, ShadowPositionBook};
    use ghost_core::account_state_core::reducer::AccountStateReducer;
    use ghost_core::account_state_core::types::{AccountStateUpdate, UpdateSource};
    use ghost_core::market_state::BondingCurve;
    use ghost_core::shadow_ledger::types::PriceState;
    use ghost_core::shadow_ledger::ShadowLedger;
    use ghost_core::CurveFinality;
    use serde_json::Value;
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::sync::RwLock as AsyncRwLock;

    fn read_jsonl_rows(path: &Path) -> Vec<Value> {
        if !path.exists() {
            return Vec::new();
        }
        std::fs::read_to_string(path)
            .expect("read jsonl")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<Value>(line).expect("valid json row"))
            .collect()
    }

    fn shadow_v2_harness_config_for_dir(path: &Path) -> ShadowV2ValidationHarnessConfig {
        ShadowV2ValidationHarnessConfig::new(
            "shadow-v2-pr18-test",
            path.join("shadow_position_event_v2.jsonl"),
            path.join("shadow_replay_v2.jsonl"),
            path.join("shadow_lifecycle_v2.jsonl"),
            path.join("shadow_path_density_v2.jsonl"),
        )
    }

    fn read_event_rows(dir: &Path) -> Vec<Value> {
        let mut rows = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    stack.push(entry_path);
                } else if entry_path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                    rows.extend(read_jsonl_rows(&entry_path));
                }
            }
        }
        rows
    }

    fn make_shadow_emitter(dir: &Path) -> Arc<EventEmitter> {
        let config = EventWriterConfig {
            output_dir: dir.to_string_lossy().to_string(),
            flush_interval_ms: 0,
            enable_optional_events: true,
            ..EventWriterConfig::default()
        };
        Arc::new(
            EventEmitter::new(config, "run-shadow-test".to_string(), Lane::Shadow)
                .expect("shadow emitter"),
        )
    }

    fn apply_test_canonical_update(
        account_state_core: &AccountStateReducer,
        mint: Pubkey,
        bonding_curve: Pubkey,
        slot: u64,
    ) {
        apply_test_canonical_update_with_receive_ts(
            account_state_core,
            mint,
            bonding_curve,
            slot,
            current_time_ms(),
        );
    }

    fn apply_test_canonical_update_with_receive_ts(
        account_state_core: &AccountStateReducer,
        mint: Pubkey,
        bonding_curve: Pubkey,
        slot: u64,
        receive_ts_ms: u64,
    ) {
        let apply_result = account_state_core.apply_account_update(AccountStateUpdate {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: mint,
            bonding_curve,
            sol_reserves: 210_000_000_000,
            token_reserves: 760_000_000_000_000,
            is_complete: 0,
            slot,
            write_version: Some(1),
            receive_ts_ms,
            receive_seq: 1,
            curve_finality: CurveFinality::Provisional,
            source: UpdateSource::GeyserAccountUpdate,
        });
        assert!(matches!(
            apply_result,
            ghost_core::account_state_core::types::AccountUpdateResult::Applied
                | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
        ));
    }

    #[test]
    fn shannon_entropy_uniform() {
        // Uniform distribution should have high entropy
        let values = vec![1.0, 1.0, 1.0, 1.0, 1.0];
        let e = compute_shannon_entropy(&values);
        // ln(5) ≈ 1.609
        assert!((e - 5.0_f64.ln()).abs() < 0.01);
    }

    #[test]
    fn shannon_entropy_concentrated() {
        // One dominant value → low entropy
        let values = vec![100.0, 1.0, 1.0, 1.0];
        let e = compute_shannon_entropy(&values);
        assert!(e < 0.5);
    }

    #[test]
    fn shannon_entropy_empty() {
        assert_eq!(compute_shannon_entropy(&[]), 0.0);
        assert_eq!(compute_shannon_entropy(&[1.0]), 0.0);
    }

    #[test]
    fn compute_recommendation_logic() {
        let config = PostBuyGuardianConfig::default();
        let shadow = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let engine = MonitoringEngine::new(config, shadow, tx);

        // No signals → Hold
        assert_eq!(
            engine.compute_recommendation(0, 0, false, false),
            RecommendedAction::Hold
        );

        // Panic impulse → PanicSell
        assert_eq!(
            engine.compute_recommendation(0, 0, false, true),
            RecommendedAction::PanicSell
        );

        // Critical signal → PanicSell
        assert_eq!(
            engine.compute_recommendation(0, 1, false, false),
            RecommendedAction::PanicSell
        );

        // Manipulation → DefensiveMode
        assert_eq!(
            engine.compute_recommendation(0, 0, true, false),
            RecommendedAction::DefensiveMode
        );

        // Many warnings → TightenStop
        assert_eq!(
            engine.compute_recommendation(3, 0, false, false),
            RecommendedAction::TightenStop
        );
    }

    #[tokio::test]
    async fn wait_for_canonical_snapshot_times_out_when_only_older_slot_is_available() {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, shadow_ledger, tx);
        engine.set_account_state_core(Arc::clone(&account_state_core));

        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        apply_test_canonical_update(&account_state_core, mint, bonding_curve, 9);

        let ready = engine
            .wait_for_canonical_snapshot(
                &mint,
                Some(10),
                Duration::from_millis(35),
                Duration::from_millis(5),
            )
            .await;

        assert!(
            !ready,
            "wait helper must reject canonical state older than the post-buy landed slot"
        );
    }

    #[tokio::test]
    async fn wait_for_canonical_snapshot_accepts_delayed_matching_update() {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, shadow_ledger, tx);
        engine.set_account_state_core(Arc::clone(&account_state_core));

        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let delayed_account_state_core = Arc::clone(&account_state_core);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            apply_test_canonical_update(&delayed_account_state_core, mint, bonding_curve, 10);
        });

        let ready = engine
            .wait_for_canonical_snapshot(
                &mint,
                Some(10),
                Duration::from_millis(100),
                Duration::from_millis(5),
            )
            .await;

        assert!(
            ready,
            "wait helper must accept the delayed canonical update once a matching post-buy slot arrives"
        );
    }

    #[tokio::test]
    async fn shadow_runtime_lazily_registers_virtual_magazine_without_aem() {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        )));
        engine.set_position_router(Arc::clone(&runtime_router));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000),
            Some(1_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-lazy".to_string(),
                entry_order_id: "shadow-entry-1".to_string(),
                quote_id: "shadow-quote-1".to_string(),
                slot: Some(42),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:1".to_string()),
                position_epoch: Some(1),
                opened_at_ms: None,
            }),
        );
        assert!(registered.is_some());

        let snapshot = MarketSnapshot {
            timestamp_ms: 1_000,
            price_sol_per_token: 1.0,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.0,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), 1_000)
            .await;

        let shadow_book = runtime_router.shadow_book().expect("shadow book");
        assert!(shadow_book.read().await.has_position("shadow:test:1"));
    }

    #[tokio::test]
    async fn shadow_tick_without_snapshots_does_not_auto_close_position() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        )));
        engine.set_position_router(Arc::clone(&runtime_router));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-gap".to_string(),
                entry_order_id: "shadow-entry-gap".to_string(),
                quote_id: "shadow-quote-gap".to_string(),
                slot: Some(21),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:gap".to_string()),
                position_epoch: Some(3),
                opened_at_ms: None,
            }),
        );
        assert!(registered.is_some());

        engine.tick().await;

        let shadow_book = runtime_router.shadow_book().expect("shadow book");
        assert!(shadow_book.read().await.has_position("shadow:test:gap"));
        assert_eq!(engine.active_position_count(), 1);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.is_empty(),
            "unexpected lifecycle rows before first shadow snapshot: {lifecycle_rows:?}"
        );
    }

    #[test]
    fn shadow_lifecycle_join_metadata_is_inherited_from_position_context() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let join_metadata = PositionJoinMetadata {
            ab_record_id: Some("pool:1000:11000:BUY".to_string()),
            source_ab_record_id: Some("pool:1000:11000:REJECT".to_string()),
            probe_id: Some("probe-id".to_string()),
            dispatch_source: Some("counterfactual_shadow_probe".to_string()),
            collection_plane: Some("counterfactual_shadow_probe".to_string()),
            probe_plane: Some("p37_shadow_probe".to_string()),
            v3_feature_snapshot_hash: Some("feature-hash".to_string()),
            v3_policy_config_hash: Some("policy-hash".to_string()),
            decision_plane: Some("legacy_live".to_string()),
            rollout_namespace: Some("r14-smoke".to_string()),
            run_id: Some("r16-run".to_string()),
            session_id: Some("r16-session".to_string()),
            brain_config_path: Some("configs/rollout/ghost_brain_r16.toml".to_string()),
            brain_config_hash: Some("brain-hash".to_string()),
            ..Default::default()
        };
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata,
                candidate_id: "cand-shadow-join".to_string(),
                entry_order_id: "shadow-entry-join".to_string(),
                quote_id: "shadow-quote-join".to_string(),
                slot: Some(88),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:join".to_string()),
                position_epoch: Some(5),
                opened_at_ms: None,
            }),
        );
        assert!(registered.is_some());

        engine.unregister_position(&mint);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert_eq!(lifecycle_rows.len(), 1);
        let row = &lifecycle_rows[0];
        assert_eq!(row["ab_record_id"], "pool:1000:11000:BUY");
        assert_eq!(row["source_ab_record_id"], "pool:1000:11000:REJECT");
        assert_eq!(row["probe_id"], "probe-id");
        assert_eq!(row["dispatch_source"], "counterfactual_shadow_probe");
        assert_eq!(row["collection_plane"], "counterfactual_shadow_probe");
        assert_eq!(row["probe_plane"], "p37_shadow_probe");
        assert_eq!(row["v3_feature_snapshot_hash"], "feature-hash");
        assert_eq!(row["v3_policy_config_hash"], "policy-hash");
        assert_eq!(row["decision_plane"], "legacy_live");
        assert_eq!(row["rollout_namespace"], "r14-smoke");
        assert_eq!(row["run_id"], "r16-run");
        assert_eq!(row["session_id"], "r16-session");
        assert_eq!(
            row["brain_config_path"],
            "configs/rollout/ghost_brain_r16.toml"
        );
        assert_eq!(row["brain_config_hash"], "brain-hash");
    }

    #[test]
    fn shadow_v2_lifecycle_close_emits_path_exit_terminal_records() {
        let tmp = TempDir::new().expect("tempdir");
        let harness = Arc::new(Mutex::new(
            ShadowV2ValidationHarness::new(shadow_v2_harness_config_for_dir(tmp.path()))
                .expect("shadow v2 harness"),
        ));

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_v2_validation_harness(Arc::clone(&harness));
        let engine = Arc::new(engine);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let opened_at_ms = 1_785_000_200_000;
        let position_id = "shadow-v2-terminal-test-position".to_string();
        let registered = engine.register_position_with_context(
            pool,
            mint,
            bonding_curve,
            Some(0.0000001),
            Some(7_000_000),
            Some(7_000_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata {
                    run_id: Some("shadow-v2-pr18-test".to_string()),
                    session_id: Some("session-terminal-test".to_string()),
                    decision_plane: Some("pr18-test".to_string()),
                    ..Default::default()
                },
                candidate_id: "candidate-terminal-test".to_string(),
                entry_order_id: "entry-order-terminal-test".to_string(),
                quote_id: "quote-terminal-test".to_string(),
                slot: Some(430_000_020),
                lane: Lane::Shadow,
                position_id: Some(position_id.clone()),
                position_epoch: Some(7),
                opened_at_ms: Some(opened_at_ms),
            }),
        );
        assert!(registered.is_some());

        engine.unregister_position(&mint);

        let canonical_rows = read_jsonl_rows(&tmp.path().join("shadow_position_event_v2.jsonl"));
        let event_kinds: Vec<_> = canonical_rows
            .iter()
            .filter_map(|row| row["event_kind"].as_str())
            .collect();
        assert!(
            event_kinds.contains(&"PATH_SAMPLE"),
            "missing PATH_SAMPLE in canonical rows: {canonical_rows:?}"
        );
        assert!(
            event_kinds.contains(&"EXIT_ATTEMPT"),
            "missing EXIT_ATTEMPT in canonical rows: {canonical_rows:?}"
        );
        assert!(
            event_kinds.contains(&"EXIT_FILL"),
            "missing EXIT_FILL in canonical rows: {canonical_rows:?}"
        );
        assert!(
            event_kinds.contains(&"TERMINAL_TRUTH"),
            "missing TERMINAL_TRUTH in canonical rows: {canonical_rows:?}"
        );

        let exit_fill = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "EXIT_FILL")
            .expect("exit fill row");
        assert_eq!(
            exit_fill["payload"]["record"]["fill_status"],
            "BLOCKED_BY_DATA"
        );
        let exit_fill_limitations = exit_fill["payload"]["record"]["limitations"]
            .as_array()
            .expect("exit fill limitations");
        assert!(exit_fill_limitations
            .iter()
            .any(|value| value == "EXIT_FILL_POOL_STATE_SAMPLE_MISSING"));

        let terminal = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("terminal truth row");
        assert_eq!(
            terminal["payload"]["record"]["terminal_source"],
            "shadow_lifecycle.position_closed"
        );
        assert_eq!(
            terminal["payload"]["record"]["final_pnl_executable_bps"],
            serde_json::Value::Null
        );
        assert_eq!(
            terminal["payload"]["record"]["linked_exit_fill"],
            exit_fill["envelope"]["event_id"]
        );

        assert_eq!(
            std::fs::read_to_string(tmp.path().join("shadow_replay_v2.jsonl"))
                .expect("replay jsonl")
                .lines()
                .count(),
            canonical_rows.len()
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("shadow_lifecycle_v2.jsonl"))
                .expect("lifecycle jsonl")
                .lines()
                .count(),
            canonical_rows.len()
        );
        assert!(
            std::fs::read_to_string(tmp.path().join("shadow_path_density_v2.jsonl"))
                .expect("density jsonl")
                .lines()
                .count()
                > 0
        );
    }

    #[tokio::test]
    async fn shadow_runtime_close_writes_economics_and_lifecycle_proof() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        ))));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            pool,
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata {
                    entry_simulation_rpc_slot: Some(77),
                    entry_market_anchor_slot: Some(77),
                    entry_market_anchor_source: Some("shadow_simulation_rpc_context".to_string()),
                    entry_landed_slot: Some(78),
                    entry_landed_slot_source: Some(
                        "synthetic_next_slot_after_entry_simulation_rpc_slot".to_string(),
                    ),
                    ..Default::default()
                },
                candidate_id: "cand-shadow-close".to_string(),
                entry_order_id: "shadow-entry-close".to_string(),
                quote_id: "shadow-quote-close".to_string(),
                slot: Some(77),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:close".to_string()),
                position_epoch: Some(4),
                opened_at_ms: None,
            }),
        );
        assert!(registered.is_some());

        let snapshot = MarketSnapshot {
            slot: Some(99),
            timestamp_ms: 1_000,
            price_sol_per_token: 10.0,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 10.0,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), 1_000)
            .await;
        engine.sync_with_position_runtime(&[mint]).await;
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush events");

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        let exit_filled_row = lifecycle_rows
            .iter()
            .find(|row| row.get("record_type") == Some(&Value::String("exit_filled".to_string())))
            .expect("exit_filled lifecycle proof");
        assert_eq!(exit_filled_row["entry_simulation_rpc_slot"], 77);
        assert_eq!(exit_filled_row["entry_market_anchor_slot"], 77);
        assert_eq!(
            exit_filled_row["entry_market_anchor_source"],
            "shadow_simulation_rpc_context"
        );
        assert!(exit_filled_row
            .get("entry_market_anchor_tx_signature")
            .is_none());
        assert_eq!(exit_filled_row["entry_landed_slot"], 78);
        assert_eq!(
            exit_filled_row["entry_landed_slot_source"],
            "synthetic_next_slot_after_entry_simulation_rpc_slot"
        );
        assert_eq!(exit_filled_row["exit_sample_slot"], 99);
        assert_eq!(exit_filled_row["exit_market_anchor_slot"], 99);
        assert!(exit_filled_row
            .get("exit_market_anchor_tx_signature")
            .is_none());
        assert!(exit_filled_row["exit_market_anchor_source"]
            .as_str()
            .is_some());
        assert!(exit_filled_row["exit_reason_evaluation_ts_ms"]
            .as_u64()
            .is_some());
        assert_eq!(exit_filled_row["exit_landed_slot"], 100);
        assert_eq!(
            exit_filled_row["exit_landed_slot_source"],
            "synthetic_next_slot_after_exit_sample"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_filled".to_string()))
                    && row.get("truth_status") == Some(&Value::String("resolved".to_string()))
                    && row.get("gross_pnl_sol").and_then(Value::as_f64).is_some()
            }),
            "missing resolved exit_filled proof: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                    && row.get("close_reason") == Some(&Value::String("Target".to_string()))
                    && row.get("net_pnl_sol").and_then(Value::as_f64).is_some()
                    && row.get("entry_value_sol").and_then(Value::as_f64).is_some()
                    && row.get("exit_value_sol").and_then(Value::as_f64).is_some()
            }),
            "missing position_closed proof with economics: {lifecycle_rows:?}"
        );

        let event_rows = read_event_rows(&events_dir);
        let closed_payload = event_rows
            .iter()
            .find_map(|row| {
                let kind = row.get("kind")?.as_object()?;
                if kind.get("type")? != "PositionClosed" {
                    return None;
                }
                kind.get("payload")
            })
            .and_then(Value::as_object)
            .cloned()
            .expect("position closed payload");
        let exit_value_sol_from_fills: f64 = lifecycle_rows
            .iter()
            .filter(|row| row.get("record_type") == Some(&Value::String("exit_filled".to_string())))
            .filter_map(|row| row.get("exit_value_sol").and_then(Value::as_f64))
            .sum();
        assert!(closed_payload
            .get("entry_value_sol")
            .and_then(Value::as_f64)
            .is_some());
        assert!(closed_payload
            .get("exit_value_sol")
            .and_then(Value::as_f64)
            .is_some());
        assert!(closed_payload
            .get("gross_pnl_sol")
            .and_then(Value::as_f64)
            .is_some());
        assert!(closed_payload
            .get("net_pnl_sol")
            .and_then(Value::as_f64)
            .is_some());
        let entry_value_sol = closed_payload
            .get("entry_value_sol")
            .and_then(Value::as_f64)
            .expect("entry value");
        let exit_value_sol = closed_payload
            .get("exit_value_sol")
            .and_then(Value::as_f64)
            .expect("exit value");
        let net_pnl_sol = closed_payload
            .get("net_pnl_sol")
            .and_then(Value::as_f64)
            .expect("net pnl");
        let final_pnl_pct = closed_payload
            .get("final_pnl_pct")
            .and_then(Value::as_f64)
            .expect("final pnl pct");
        assert_eq!(entry_value_sol, 1.0);
        assert!((exit_value_sol - exit_value_sol_from_fills).abs() < 1e-9);
        assert!((net_pnl_sol - (exit_value_sol - entry_value_sol)).abs() < 1e-9);
        assert!((final_pnl_pct - ((net_pnl_sol / entry_value_sol) * 100.0)).abs() < 1e-9);
        assert_eq!(
            closed_payload.get("reason"),
            Some(&Value::String("Target".to_string()))
        );
    }

    #[tokio::test]
    async fn shadow_runtime_simple_threshold_take_profit_closes_without_virtual_magazine() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_simple_exit_thresholds(0.02, 0.02);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-simple-tp".to_string(),
                entry_order_id: "shadow-entry-simple-tp".to_string(),
                quote_id: "shadow-quote-simple-tp".to_string(),
                slot: Some(101),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:simple-tp".to_string()),
                position_epoch: Some(5),
                opened_at_ms: None,
            }),
        );
        assert!(registered.is_some());

        let snapshot = MarketSnapshot {
            slot: Some(111),
            timestamp_ms: 1_000,
            price_sol_per_token: 1.03,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.03,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), 1_000)
            .await;
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush events");

        assert_eq!(engine.active_position_count(), 0);
        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(lifecycle_rows.iter().any(|row| {
            row.get("record_type") == Some(&Value::String("exit_filled".to_string()))
                && row.get("truth_status") == Some(&Value::String("resolved".to_string()))
        }));
        assert!(lifecycle_rows.iter().any(|row| {
            row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                && row.get("close_reason") == Some(&Value::String("Target".to_string()))
        }));

        let event_rows = read_event_rows(&events_dir);
        assert!(event_rows.iter().any(|row| {
            row.pointer("/kind/type") == Some(&Value::String("PositionClosed".to_string()))
                && row.pointer("/kind/payload/reason") == Some(&Value::String("Target".to_string()))
        }));
    }

    #[tokio::test]
    async fn shadow_runtime_simple_threshold_stop_loss_closes_with_stop_loss_reason() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_simple_exit_thresholds(0.02, 0.02);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-simple-sl".to_string(),
                entry_order_id: "shadow-entry-simple-sl".to_string(),
                quote_id: "shadow-quote-simple-sl".to_string(),
                slot: Some(121),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:simple-sl".to_string()),
                position_epoch: Some(6),
                opened_at_ms: None,
            }),
        );
        assert!(registered.is_some());

        let snapshot = MarketSnapshot {
            slot: Some(131),
            timestamp_ms: 1_000,
            price_sol_per_token: 0.97,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 0.97,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), 1_000)
            .await;
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush events");

        assert_eq!(engine.active_position_count(), 0);
        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(lifecycle_rows.iter().any(|row| {
            row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                && row.get("close_reason") == Some(&Value::String("StopLoss".to_string()))
        }));

        let event_rows = read_event_rows(&events_dir);
        assert!(event_rows.iter().any(|row| {
            row.pointer("/kind/type") == Some(&Value::String("PositionClosed".to_string()))
                && row.pointer("/kind/payload/reason")
                    == Some(&Value::String("StopLoss".to_string()))
        }));
    }

    #[test]
    fn shadow_simple_exit_thresholds_allow_target_above_100_percent() {
        let thresholds = ShadowSimpleExitThresholds::new(1.5, 0.5);
        let (upper, lower) = thresholds
            .prices_for_entry(1.0)
            .expect("thresholds should produce prices");

        assert_eq!(upper, 2.5);
        assert_eq!(lower, 0.5);
        assert_eq!(
            MonitoringEngine::determine_shadow_simple_exit_trigger(2.4, upper, lower, true),
            Some(ShadowSimpleExitTrigger::TimeStop)
        );
        assert_eq!(
            MonitoringEngine::determine_shadow_simple_exit_trigger(2.5, upper, lower, false),
            Some(ShadowSimpleExitTrigger::TakeProfit)
        );
    }

    #[test]
    fn monitoring_engine_uses_configured_timestop_ms() {
        let mut config = PostBuyGuardianConfig::default();
        config.wait_for_timestop = Some(12_345);
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let engine = MonitoringEngine::new(config, shadow_ledger, tx);

        assert_eq!(engine.shadow_position_time_stop_ms(), 12_345);
    }

    fn time_stop_v2_test_snapshot(
        slot: u64,
        timestamp_ms: u64,
        price_sol_per_token: f64,
        market_cap_sol: f64,
        bonding_progress_pct: f64,
        tx_count: u64,
        cum_volume_sol: f64,
    ) -> MarketSnapshot {
        MarketSnapshot {
            slot: Some(slot),
            timestamp_ms,
            price_sol_per_token,
            price_state: PriceState::Valid,
            market_cap_sol,
            reserve_base: 1_000_000.0,
            reserve_quote: market_cap_sol.max(0.0),
            bonding_progress_pct,
            tx_count,
            cum_volume_sol,
            ..MarketSnapshot::default()
        }
    }

    fn register_time_stop_v2_shadow_position(
        engine: &MonitoringEngine,
        mint: Pubkey,
        opened_at_ms: u64,
        initial_snapshot: &MarketSnapshot,
        join_metadata: PositionJoinMetadata,
    ) {
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(initial_snapshot.price_sol_per_token),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata,
                candidate_id: "cand-time-stop-v2".to_string(),
                entry_order_id: "shadow-entry-time-stop-v2".to_string(),
                quote_id: "shadow-quote-time-stop-v2".to_string(),
                slot: initial_snapshot.slot,
                lane: Lane::Shadow,
                position_id: Some("shadow:test:time-stop-v2".to_string()),
                position_epoch: Some(1),
                opened_at_ms: Some(opened_at_ms),
            }),
        );
        assert!(registered.is_some());

        let mut positions = engine.positions.write();
        let pos = positions.get_mut(&mint).expect("monitored position");
        pos.time_stop_v2 = TimeStopV2State::from_registration(Some(initial_snapshot));
        pos.last_snapshot_source = PriceTruthSource::ShadowLedgerSnapshot;
    }

    #[test]
    fn time_stop_v2_default_disabled_emits_no_window_rows() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let opened_at_ms = 1_000;
        let initial = time_stop_v2_test_snapshot(1, opened_at_ms, 1.0, 100.0, 10.0, 0, 0.0);
        register_time_stop_v2_shadow_position(
            &engine,
            mint,
            opened_at_ms,
            &initial,
            PositionJoinMetadata::default(),
        );

        let latest = time_stop_v2_test_snapshot(2, 4_000, 1.001, 100.05, 10.01, 1, 0.01);
        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&latest), 4_000);

        assert_eq!(engine.active_position_count(), 1);
        assert!(
            read_jsonl_rows(&lifecycle_log).is_empty(),
            "disabled TimeStop V2 must not emit lifecycle rows"
        );
    }

    #[test]
    fn time_stop_v2_observe_only_emits_candidate_without_closing_position() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let mut config = PostBuyGuardianConfig::default();
        config.time_stop_v2.enabled = true;
        config.time_stop_v2.first_check_ms = 3_000;
        config.time_stop_v2.window_ms = 4_000;
        config.time_stop_v2.failed_windows_to_signal = 3;
        config.time_stop_v2.min_age_before_signal_ms = 11_000;
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let opened_at_ms = 1_000;
        let initial = time_stop_v2_test_snapshot(1, opened_at_ms, 1.0, 100.0, 10.0, 0, 0.0);
        register_time_stop_v2_shadow_position(
            &engine,
            mint,
            opened_at_ms,
            &initial,
            PositionJoinMetadata {
                probe_id: Some("probe-time-stop-v2".to_string()),
                dispatch_source: Some("counterfactual_shadow_probe".to_string()),
                ..Default::default()
            },
        );

        let heartbeat_1 = time_stop_v2_test_snapshot(2, 4_000, 1.001, 100.05, 10.01, 1, 0.01);
        let heartbeat_2 = time_stop_v2_test_snapshot(3, 8_000, 1.002, 100.10, 10.02, 2, 0.02);
        let heartbeat_3 = time_stop_v2_test_snapshot(4, 12_000, 1.003, 100.15, 10.03, 3, 0.03);

        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&heartbeat_1), 4_000);
        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&heartbeat_2), 8_000);
        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&heartbeat_3), 12_000);

        assert_eq!(
            engine.active_position_count(),
            1,
            "observe-only candidate must not close the monitored position"
        );

        let rows = read_jsonl_rows(&lifecycle_log);
        let window_rows: Vec<_> = rows
            .iter()
            .filter(|row| {
                row.get("record_type") == Some(&Value::String("time_stop_v2_window".to_string()))
            })
            .collect();
        assert_eq!(
            window_rows.len(),
            3,
            "expected three V2 window rows: {rows:?}"
        );
        let last = window_rows.last().expect("last V2 row");
        assert_eq!(last["probe_id"], "probe-time-stop-v2");
        assert_eq!(last["dispatch_source"], "counterfactual_shadow_probe");
        assert_eq!(last["time_stop_v2_mode"], "observe_only");
        assert_eq!(last["time_stop_v2_status"], "heartbeat");
        assert_eq!(
            last["time_stop_v2_subreason"],
            "micro_tx_heartbeat_no_price_progress"
        );
        assert_eq!(last["time_stop_v2_failed_windows"], 3);
        assert_eq!(last["time_stop_v2_candidate"], true);
        assert_eq!(last["time_stop_v2_window_index"], 2);
        assert_eq!(last["time_stop_v2_candidate_ts_ms"], 12_000);
        assert_eq!(
            last["time_stop_v2_candidate_subreason"],
            "micro_tx_heartbeat_no_price_progress"
        );
        assert!(
            rows.iter()
                .all(|row| row.get("record_type")
                    != Some(&Value::String("position_closed".to_string()))),
            "observe-only V2 must not emit position_closed: {rows:?}"
        );
    }

    #[test]
    fn time_stop_v2_alive_window_resets_failed_windows() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let mut config = PostBuyGuardianConfig::default();
        config.time_stop_v2.enabled = true;
        config.time_stop_v2.first_check_ms = 3_000;
        config.time_stop_v2.window_ms = 4_000;
        config.time_stop_v2.failed_windows_to_signal = 2;
        config.time_stop_v2.min_age_before_signal_ms = 7_000;
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let opened_at_ms = 1_000;
        let initial = time_stop_v2_test_snapshot(1, opened_at_ms, 1.0, 100.0, 10.0, 0, 0.0);
        register_time_stop_v2_shadow_position(
            &engine,
            mint,
            opened_at_ms,
            &initial,
            PositionJoinMetadata::default(),
        );

        let weak = time_stop_v2_test_snapshot(2, 4_000, 1.001, 100.05, 10.01, 0, 0.0);
        let alive = time_stop_v2_test_snapshot(3, 8_000, 1.060, 106.0, 10.90, 3, 1.2);

        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&weak), 4_000);
        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&alive), 8_000);

        let rows = read_jsonl_rows(&lifecycle_log);
        let window_rows: Vec<_> = rows
            .iter()
            .filter(|row| {
                row.get("record_type") == Some(&Value::String("time_stop_v2_window".to_string()))
            })
            .collect();
        assert_eq!(
            window_rows.len(),
            2,
            "expected two V2 window rows: {rows:?}"
        );
        assert_eq!(window_rows[0]["time_stop_v2_status"], "weak");
        assert_eq!(window_rows[0]["time_stop_v2_failed_windows"], 1);
        assert_eq!(window_rows[1]["time_stop_v2_status"], "alive");
        assert_eq!(
            window_rows[1]["time_stop_v2_subreason"],
            "alive_meaningful_progress"
        );
        assert_eq!(window_rows[1]["time_stop_v2_failed_windows"], 0);
        assert_eq!(window_rows[1]["time_stop_v2_candidate"], false);
    }

    #[test]
    fn time_stop_v2_valid_unchanged_snapshot_emits_zero_delta_weak_window() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let mut config = PostBuyGuardianConfig::default();
        config.time_stop_v2.enabled = true;
        config.time_stop_v2.first_check_ms = 3_000;
        config.time_stop_v2.window_ms = 4_000;
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let opened_at_ms = 1_000;
        let initial = time_stop_v2_test_snapshot(1, opened_at_ms, 1.0, 100.0, 10.0, 4, 1.5);
        register_time_stop_v2_shadow_position(
            &engine,
            mint,
            opened_at_ms,
            &initial,
            PositionJoinMetadata::default(),
        );

        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&initial), 4_000);

        let rows = read_jsonl_rows(&lifecycle_log);
        let row = rows
            .iter()
            .find(|row| {
                row.get("record_type") == Some(&Value::String("time_stop_v2_window".to_string()))
            })
            .expect("time_stop_v2 window row");
        assert_eq!(row["time_stop_v2_status"], "weak");
        assert_eq!(row["time_stop_v2_subreason"], "no_new_market_sample");
        assert_eq!(row["time_stop_v2_tx_delta_window"], 0);
        assert_eq!(row["time_stop_v2_volume_delta_sol_window"], 0.0);
        assert_eq!(row["time_stop_v2_price_delta_pct_window"], 0.0);
        assert_eq!(row["time_stop_v2_mcap_delta_pct_window"], 0.0);
        assert_eq!(row["time_stop_v2_bonding_delta_pct_window"], 0.0);
    }

    #[test]
    fn time_stop_v2_invalid_snapshot_remains_stale_or_insufficient() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let mut config = PostBuyGuardianConfig::default();
        config.time_stop_v2.enabled = true;
        config.time_stop_v2.first_check_ms = 3_000;
        config.time_stop_v2.window_ms = 4_000;
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let opened_at_ms = 1_000;
        let initial = time_stop_v2_test_snapshot(1, opened_at_ms, 1.0, 100.0, 10.0, 4, 1.5);
        register_time_stop_v2_shadow_position(
            &engine,
            mint,
            opened_at_ms,
            &initial,
            PositionJoinMetadata::default(),
        );

        let mut invalid = time_stop_v2_test_snapshot(2, 4_000, 0.0, 0.0, 10.0, 5, 2.0);
        invalid.price_state = PriceState::Unknown;
        invalid.price_reason = Some(PriceReason::MissingPriceData);

        engine.evaluate_time_stop_v2_observe_only(&mint, Some(&invalid), 4_000);

        let rows = read_jsonl_rows(&lifecycle_log);
        let row = rows
            .iter()
            .find(|row| {
                row.get("record_type") == Some(&Value::String("time_stop_v2_window".to_string()))
            })
            .expect("time_stop_v2 window row");
        assert_eq!(row["time_stop_v2_status"], "stale_or_insufficient");
        assert_eq!(row["time_stop_v2_subreason"], "invalid_market_sample");
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_closes_dead_zone_position() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        ))));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-time-stop".to_string(),
                entry_order_id: "shadow-entry-time-stop".to_string(),
                quote_id: "shadow-quote-time-stop".to_string(),
                slot: Some(55),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:time-stop".to_string()),
                position_epoch: Some(5),
                opened_at_ms: None,
            }),
        );
        let registered = registered.expect("shadow registration");

        let now_ms = registered.opened_at_ms + SHADOW_POSITION_TIME_STOP_MS + 1;
        let snapshot = MarketSnapshot {
            slot: Some(66),
            timestamp_ms: now_ms,
            price_sol_per_token: 1.0,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.0,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), now_ms)
            .await;
        engine.sync_with_position_runtime(&[mint]).await;
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush events");

        assert_eq!(engine.active_position_count(), 0);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                    && row.get("close_reason") == Some(&Value::String("TimeStop".to_string()))
            }),
            "missing time-stop close proof: {lifecycle_rows:?}"
        );

        let event_rows = read_event_rows(&events_dir);
        let closed_payload = event_rows
            .iter()
            .find_map(|row| {
                let kind = row.get("kind")?.as_object()?;
                if kind.get("type")? != "PositionClosed" {
                    return None;
                }
                kind.get("payload")
            })
            .and_then(Value::as_object)
            .cloned()
            .expect("position closed payload");
        assert_eq!(
            closed_payload.get("reason"),
            Some(&Value::String("TimeStop".to_string()))
        );
    }

    #[tokio::test]
    async fn shadow_runtime_expired_bullets_close_as_time_stop_below_target() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        let shadow_book = Arc::new(AsyncRwLock::new(ShadowPositionBook::new()));
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::clone(&shadow_book),
        )));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let candidate_id = "cand-shadow-expired-time-stop".to_string();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: candidate_id.clone(),
                entry_order_id: "shadow-entry-expired-time-stop".to_string(),
                quote_id: "shadow-quote-expired-time-stop".to_string(),
                slot: Some(57),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:expired-time-stop".to_string()),
                position_epoch: Some(7),
                opened_at_ms: None,
            }),
        );
        let registered = registered.expect("shadow registration");
        assert!(engine.ensure_shadow_runtime_registered(&mint).await);
        assert!(shadow_book
            .write()
            .await
            .age_position_for_time_stop_for_tests(
                &mint,
                SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS + 1
            ));

        let now_ms = registered.opened_at_ms + SHADOW_POSITION_TIME_STOP_MS + 1;
        let snapshot = MarketSnapshot {
            slot: Some(68),
            timestamp_ms: now_ms,
            price_sol_per_token: 0.9,
            price_state: PriceState::Valid,
            market_cap_sol: 0.9,
            reserve_base: 1_000_000.0,
            reserve_quote: 0.9,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), now_ms)
            .await;
        engine.sync_with_position_runtime(&[mint]).await;
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush events");

        assert_eq!(engine.active_position_count(), 0);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        let candidate_rows: Vec<_> = lifecycle_rows
            .iter()
            .filter(|row| row.get("candidate_id") == Some(&Value::String(candidate_id.clone())))
            .collect();
        assert!(
            candidate_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                    && row.get("close_reason") == Some(&Value::String("TimeStop".to_string()))
                    && row
                        .get("final_pnl_pct")
                        .and_then(Value::as_f64)
                        .is_some_and(|pct| pct < 0.0)
            }),
            "missing time-stop close proof for expired bullets: {candidate_rows:?}"
        );
        assert!(
            !candidate_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                    && row.get("close_reason") == Some(&Value::String("Target".to_string()))
            }),
            "expired below-target bullets must not close as Target: {candidate_rows:?}"
        );

        let event_rows = read_event_rows(&events_dir);
        let closed_payload = event_rows
            .iter()
            .find_map(|row| {
                let kind = row.get("kind")?.as_object()?;
                if kind.get("type")? != "PositionClosed" {
                    return None;
                }
                kind.get("payload")
            })
            .and_then(Value::as_object)
            .cloned()
            .expect("position closed payload");
        assert_eq!(
            closed_payload.get("reason"),
            Some(&Value::String("TimeStop".to_string()))
        );
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_waits_for_inactivity_not_position_age() {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        let shadow_book = Arc::new(AsyncRwLock::new(ShadowPositionBook::new()));
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::clone(&shadow_book),
        )));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-shadow-inactivity-guard".to_string(),
                    entry_order_id: "shadow-entry-inactivity-guard".to_string(),
                    quote_id: "shadow-quote-inactivity-guard".to_string(),
                    slot: Some(58),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:inactivity-guard".to_string()),
                    position_epoch: Some(12),
                    opened_at_ms: None,
                }),
            )
            .expect("shadow registration");
        assert!(engine.ensure_shadow_runtime_registered(&mint).await);
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.entry_unix_ms = registered
                .opened_at_ms
                .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1);
            pos.shadow_market_activity.last_seen_ms = registered.opened_at_ms;
        }

        let now_ms = registered.opened_at_ms + SHADOW_POSITION_TIME_STOP_MS + 1;
        let snapshot = MarketSnapshot {
            slot: Some(69),
            timestamp_ms: now_ms,
            price_sol_per_token: 1.0,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.0,
            ..MarketSnapshot::default()
        };
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            assert!(pos
                .shadow_market_activity
                .observe_snapshot(&snapshot, now_ms));
        }

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), now_ms)
            .await;
        engine.sync_with_position_runtime(&[mint]).await;

        assert_eq!(engine.active_position_count(), 1);
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_does_not_use_cached_snapshot_fallback() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        ))));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-time-stop-cached".to_string(),
                entry_order_id: "shadow-entry-time-stop-cached".to_string(),
                quote_id: "shadow-quote-time-stop-cached".to_string(),
                slot: Some(56),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:time-stop-cached".to_string()),
                position_epoch: Some(6),
                opened_at_ms: None,
            }),
        );
        let registered = registered.expect("shadow registration");

        let cached_snapshot = MarketSnapshot {
            slot: Some(67),
            timestamp_ms: registered.opened_at_ms.saturating_add(1_000),
            price_sol_per_token: 1.0,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.0,
            ..MarketSnapshot::default()
        };
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.last_shadow_snapshot = Some(cached_snapshot);
        }

        let now_ms = registered.opened_at_ms + SHADOW_POSITION_TIME_STOP_MS + 1;
        engine.run_shadow_runtime_tick(&mint, None, now_ms).await;
        engine.sync_with_position_runtime(&[mint]).await;
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush events");

        assert_eq!(engine.active_position_count(), 0);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().all(
                |row| row.get("record_type") != Some(&Value::String("exit_filled".to_string()))
            ),
            "cached snapshot fallback must not emit exit_filled rows: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_blocked".to_string()))
                    && row.get("truth_status") == Some(&Value::String("failure".to_string()))
                    && row
                        .get("truth_detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains(
                                "shadow time-stop expired before any canonical snapshot reached guardian",
                            )
                        })
            }),
            "missing cache-reject exit_blocked proof: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                    && row.get("close_reason") == Some(&Value::String("TimeStop".to_string()))
                    && row.get("truth_status") == Some(&Value::String("failure".to_string()))
            }),
            "missing cache-reject time-stop close proof: {lifecycle_rows:?}"
        );

        let event_rows = read_event_rows(&events_dir);
        let closed_payload = event_rows
            .iter()
            .find_map(|row| {
                let kind = row.get("kind")?.as_object()?;
                if kind.get("type")? != "PositionClosed" {
                    return None;
                }
                kind.get("payload")
            })
            .and_then(Value::as_object)
            .cloned()
            .expect("position closed payload");
        assert_eq!(
            closed_payload.get("reason"),
            Some(&Value::String("TimeStop".to_string()))
        );
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_uses_current_curve_state_when_snapshot_buffer_missing() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        let shadow_book = Arc::new(AsyncRwLock::new(ShadowPositionBook::new()));
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::clone(&shadow_book),
        )));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let registered = engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                bonding_curve,
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-shadow-current-curve".to_string(),
                    entry_order_id: "shadow-entry-current-curve".to_string(),
                    quote_id: "shadow-quote-current-curve".to_string(),
                    slot: Some(71),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:current-curve".to_string()),
                    position_epoch: Some(8),
                    opened_at_ms: None,
                }),
            )
            .expect("shadow registration");
        assert!(engine.ensure_shadow_runtime_registered(&mint).await);
        assert!(shadow_book
            .write()
            .await
            .age_position_for_time_stop_for_tests(
                &mint,
                SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS + 1
            ));
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.entry_unix_ms = registered
                .opened_at_ms
                .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1);
            pos.shadow_market_activity.last_seen_ms = registered
                .opened_at_ms
                .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1);
        }

        shadow_ledger.register_curve_alias(mint, bonding_curve);
        shadow_ledger.insert_with_slot_at(
            bonding_curve,
            BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 1_000_000_000_000,
                virtual_sol_reserves: 100_000_000_000,
                real_token_reserves: 1_000_000_000_000,
                real_sol_reserves: 100_000_000_000,
                token_total_supply: 1_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            },
            414_525_981,
            current_time_ms(),
        );
        let latest_snapshot = engine
            .current_shadow_curve_snapshot(&mint)
            .expect("current curve snapshot");
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.shadow_market_activity = ShadowMarketActivityAnchor {
                last_seen_ms: registered
                    .opened_at_ms
                    .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1),
                snapshot_ts_ms: latest_snapshot.timestamp_ms,
                slot: latest_snapshot.slot,
                tx_count: latest_snapshot.tx_count,
            };
        }

        engine.tick().await;

        assert_eq!(engine.active_position_count(), 0);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_filled".to_string()))
                    && row.get("truth_status") == Some(&Value::String("resolved".to_string()))
            }),
            "missing exit_filled proof from current curve fallback: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                    && row.get("close_reason") == Some(&Value::String("TimeStop".to_string()))
                    && row.get("truth_status") == Some(&Value::String("resolved".to_string()))
            }),
            "missing time-stop close proof from current curve fallback: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().all(|row| {
                row.get("truth_detail")
                    .and_then(Value::as_str)
                    .map_or(true, |detail| {
                        !detail.contains(
                            "shadow time-stop expired before any canonical snapshot reached guardian",
                        )
                    })
            }),
            "current curve fallback must prevent no-snapshot failure proof: {lifecycle_rows:?}"
        );
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_prefers_fresh_account_state_core_over_stale_shadow_curve() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_account_state_core(Arc::clone(&account_state_core));
        let shadow_book = Arc::new(AsyncRwLock::new(ShadowPositionBook::new()));
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::clone(&shadow_book),
        )));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let registered = engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                bonding_curve,
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-shadow-account-state-core".to_string(),
                    entry_order_id: "shadow-entry-account-state-core".to_string(),
                    quote_id: "shadow-quote-account-state-core".to_string(),
                    slot: Some(72),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:account-state-core".to_string()),
                    position_epoch: Some(9),
                    opened_at_ms: None,
                }),
            )
            .expect("shadow registration");
        assert!(engine.ensure_shadow_runtime_registered(&mint).await);
        assert!(shadow_book
            .write()
            .await
            .age_position_for_time_stop_for_tests(
                &mint,
                SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS + 1
            ));
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.entry_unix_ms = registered
                .opened_at_ms
                .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1);
            pos.shadow_market_activity.last_seen_ms = registered
                .opened_at_ms
                .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1);
        }

        shadow_ledger.register_curve_alias(mint, bonding_curve);
        shadow_ledger.insert_with_slot_at(
            bonding_curve,
            BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 1_000_000_000_000,
                virtual_sol_reserves: 100_000_000_000,
                real_token_reserves: 1_000_000_000_000,
                real_sol_reserves: 100_000_000_000,
                token_total_supply: 1_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            },
            414_525_981,
            registered.opened_at_ms,
        );
        shadow_ledger.set_snapshots(
            mint,
            vec![MarketSnapshot {
                slot: Some(414_525_981),
                timestamp_ms: registered.opened_at_ms,
                price_sol_per_token: 0.1,
                price_state: PriceState::Valid,
                market_cap_sol: 0.1,
                reserve_base: 1_000_000_000_000.0,
                reserve_quote: 100.0,
                ..MarketSnapshot::default()
            }],
        );
        apply_test_canonical_update(&account_state_core, mint, bonding_curve, 414_526_333);
        let latest_snapshot = engine
            .current_shadow_curve_snapshot(&mint)
            .expect("initial canonical snapshot");
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.shadow_market_activity = ShadowMarketActivityAnchor {
                last_seen_ms: registered
                    .opened_at_ms
                    .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1),
                snapshot_ts_ms: latest_snapshot.timestamp_ms,
                slot: latest_snapshot.slot,
                tx_count: latest_snapshot.tx_count,
            };
        }

        engine.tick().await;

        assert_eq!(engine.active_position_count(), 0);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_filled".to_string()))
                    && row.get("truth_status") == Some(&Value::String("resolved".to_string()))
                    && row.get("truth_source")
                        == Some(&Value::String(
                            "canonical_account_state_snapshot".to_string(),
                        ))
            }),
            "fresh account-state-core snapshot must emit exit_filled proof: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().all(|row| {
                row.get("truth_status") != Some(&Value::String("stale".to_string()))
            }),
            "fresh account-state-core snapshot must avoid stale close proof: {lifecycle_rows:?}"
        );
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_uses_currently_observed_canonical_state_for_quiet_pool() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_account_state_core(Arc::clone(&account_state_core));
        let shadow_book = Arc::new(AsyncRwLock::new(ShadowPositionBook::new()));
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::clone(&shadow_book),
        )));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let registered = engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                bonding_curve,
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-shadow-current-canonical-runtime".to_string(),
                    entry_order_id: "shadow-entry-current-canonical-runtime".to_string(),
                    quote_id: "shadow-quote-current-canonical-runtime".to_string(),
                    slot: Some(72),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:current-canonical-runtime".to_string()),
                    position_epoch: Some(10),
                    opened_at_ms: None,
                }),
            )
            .expect("shadow registration");
        assert!(engine.ensure_shadow_runtime_registered(&mint).await);
        assert!(shadow_book
            .write()
            .await
            .age_position_for_time_stop_for_tests(
                &mint,
                SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS + 1
            ));
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.entry_unix_ms = registered
                .opened_at_ms
                .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1);
            pos.shadow_market_activity.last_seen_ms = registered
                .opened_at_ms
                .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1);
        }

        let stale_update_ts_ms = current_time_ms().saturating_sub(10_000);
        apply_test_canonical_update_with_receive_ts(
            &account_state_core,
            mint,
            bonding_curve,
            414_526_777,
            stale_update_ts_ms,
        );
        let latest_snapshot = engine
            .current_shadow_curve_snapshot(&mint)
            .expect("initial canonical snapshot");
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.shadow_market_activity = ShadowMarketActivityAnchor {
                last_seen_ms: registered
                    .opened_at_ms
                    .saturating_sub(SHADOW_POSITION_TIME_STOP_MS + 1),
                snapshot_ts_ms: latest_snapshot.timestamp_ms,
                slot: latest_snapshot.slot,
                tx_count: latest_snapshot.tx_count,
            };
        }
        apply_test_canonical_update_with_receive_ts(
            &account_state_core,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            414_526_888,
            stale_update_ts_ms.saturating_add(1),
        );

        let historical_snapshots = engine
            .snapshots_for_tick(&mint)
            .expect("historical canonical snapshots");
        let historical_latest = historical_snapshots
            .last()
            .expect("historical latest snapshot");
        assert_eq!(historical_latest.timestamp_ms, stale_update_ts_ms);

        let observed_at_ms = current_time_ms();
        let runtime_snapshot = engine
            .current_runtime_shadow_snapshot(&mint, observed_at_ms)
            .expect("runtime canonical snapshot");
        assert_eq!(runtime_snapshot.slot, historical_latest.slot);
        assert_eq!(runtime_snapshot.timestamp_ms, observed_at_ms);

        engine.tick().await;

        assert_eq!(engine.active_position_count(), 0);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_filled".to_string()))
                    && row.get("truth_status") == Some(&Value::String("resolved".to_string()))
                    && row.get("truth_source")
                        == Some(&Value::String(
                            "canonical_account_state_snapshot".to_string(),
                        ))
            }),
            "currently observed canonical state must emit exit_filled proof: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().all(|row| {
                row.get("truth_status") != Some(&Value::String("stale".to_string()))
            }),
            "currently observed canonical state must avoid stale close proof: {lifecycle_rows:?}"
        );
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_does_not_refresh_stale_canonical_state_without_newer_global_slot(
    ) {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_account_state_core(Arc::clone(&account_state_core));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                bonding_curve,
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-shadow-current-canonical-guard".to_string(),
                    entry_order_id: "shadow-entry-current-canonical-guard".to_string(),
                    quote_id: "shadow-quote-current-canonical-guard".to_string(),
                    slot: Some(73),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:current-canonical-guard".to_string()),
                    position_epoch: Some(11),
                    opened_at_ms: None,
                }),
            )
            .expect("shadow registration");

        let stale_update_ts_ms = current_time_ms().saturating_sub(10_000);
        apply_test_canonical_update_with_receive_ts(
            &account_state_core,
            mint,
            bonding_curve,
            414_526_999,
            stale_update_ts_ms,
        );

        let runtime_snapshot = engine
            .current_runtime_shadow_snapshot(&mint, current_time_ms())
            .expect("runtime canonical snapshot");
        assert_eq!(runtime_snapshot.timestamp_ms, stale_update_ts_ms);
    }

    #[test]
    fn guardian_outcome_source_reads_position_timeline_without_shadow_ledger_history() {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let engine = MonitoringEngine::new(config, shadow_ledger, tx);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let registered = engine
            .register_position_with_context(
                pool,
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-outcome-timeline".to_string(),
                    entry_order_id: "shadow-entry-outcome-timeline".to_string(),
                    quote_id: "shadow-quote-outcome-timeline".to_string(),
                    slot: Some(99),
                    lane: Lane::Shadow,
                    position_id: Some("position-outcome-timeline".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: None,
                }),
            )
            .expect("position registered");

        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.snapshot_timeline = SnapshotTimeline {
                cumulative_volume_sol: 0.0,
                snapshots: vec![
                    MarketSnapshot {
                        slot: Some(1),
                        timestamp_ms: 1_000,
                        price_sol_per_token: 1.0,
                        market_cap_sol: 10.0,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                    MarketSnapshot {
                        slot: Some(2),
                        timestamp_ms: 1_100,
                        price_sol_per_token: 1.2,
                        market_cap_sol: 12.0,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                    MarketSnapshot {
                        slot: Some(3),
                        timestamp_ms: 1_250,
                        price_sol_per_token: 0.9,
                        market_cap_sol: 9.0,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                ],
            };
        }

        let source = GuardianOutcomeSource {
            positions: Arc::clone(&engine.positions),
        };
        let sample = source
            .sample_outcome(&registered.position_id, 1_000, 500)
            .expect("outcome sample");

        assert_eq!(sample.price_at_t, Some(0.9));
        assert_eq!(sample.peak_in_t, Some(1.2));
        assert!(sample.reclaim_happened);
        assert_eq!(sample.time_to_reclaim_ms, Some(0));
        assert!(!sample.outcome_data_gap);
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_rejects_stale_snapshot_without_emitting_fill() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        ))));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(7_000_000),
            Some(120_080_136_032),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-stale-time-stop".to_string(),
                entry_order_id: "shadow-entry-stale-time-stop".to_string(),
                quote_id: "shadow-quote-stale-time-stop".to_string(),
                slot: Some(88),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:stale-time-stop".to_string()),
                position_epoch: Some(7),
                opened_at_ms: None,
            }),
        );
        let registered = registered.expect("shadow registration");

        let now_ms = registered.opened_at_ms + SHADOW_POSITION_TIME_STOP_MS + 1;
        let snapshot = MarketSnapshot {
            slot: Some(414_525_981),
            timestamp_ms: now_ms.saturating_sub(10_000),
            price_sol_per_token: 54.928389038,
            price_state: PriceState::Valid,
            market_cap_sol: 54.928389038,
            reserve_base: 765_529_722_604_345.0,
            reserve_quote: 42.049_314_424,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), now_ms)
            .await;
        engine.sync_with_position_runtime(&[mint]).await;

        assert_eq!(engine.active_position_count(), 0);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().all(
                |row| row.get("record_type") != Some(&Value::String("exit_filled".to_string()))
            ),
            "stale time-stop must not emit exit_filled rows: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_blocked".to_string()))
                    && row.get("truth_status") == Some(&Value::String("stale".to_string()))
                    && row
                        .get("truth_detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("stale time-stop rejected without emitting fill")
                                && detail.contains(
                                    "source_path=guardian.post_buy.shadow_time_stop_stale",
                                )
                        })
            }),
            "missing stale time-stop rejection proof: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
                    && row.get("close_reason") == Some(&Value::String("TimeStop".to_string()))
                    && row.get("truth_status") == Some(&Value::String("stale".to_string()))
            }),
            "missing stale time-stop close proof: {lifecycle_rows:?}"
        );
    }

    #[tokio::test]
    async fn shadow_runtime_records_blocked_exit_when_price_truth_is_stale() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        )));
        engine.set_position_router(Arc::clone(&runtime_router));
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine.register_position_with_context(
            Pubkey::new_unique(),
            mint,
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000_000),
            Some(1_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "cand-shadow-stale".to_string(),
                entry_order_id: "shadow-entry-stale".to_string(),
                quote_id: "shadow-quote-stale".to_string(),
                slot: Some(11),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:stale".to_string()),
                position_epoch: Some(2),
                opened_at_ms: None,
            }),
        );
        assert!(registered.is_some());

        let snapshot = MarketSnapshot {
            slot: Some(22),
            timestamp_ms: 1_000,
            price_sol_per_token: 10.0,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 10.0,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), 10_000)
            .await;

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_blocked".to_string()))
                    && row.get("truth_status") == Some(&Value::String("stale".to_string()))
            }),
            "missing exit_blocked stale proof: {lifecycle_rows:?}"
        );

        let shadow_book = runtime_router.shadow_book().expect("shadow book");
        assert!(shadow_book.read().await.has_position("shadow:test:stale"));
        assert_eq!(engine.active_position_count(), 1);
    }
}
