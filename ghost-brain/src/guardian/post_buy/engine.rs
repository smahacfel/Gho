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

use parking_lot::RwLock;
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::CanonicalPoolState;
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

use super::config::PostBuyGuardianConfig;
use super::integration::{
    PositionRuntimeRouter, ShadowPositionBookAemAdapter, SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS,
};

const SHADOW_POSITION_TIME_STOP_MS: u64 = SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS * 1000;
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
            take_profit_pct: sanitize_shadow_threshold_pct(take_profit_pct),
            stop_loss_pct: sanitize_shadow_threshold_pct(stop_loss_pct),
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

fn sanitize_shadow_threshold_pct(value: f64) -> f64 {
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
    pub candidate_id: String,
    pub entry_order_id: String,
    pub quote_id: String,
    pub slot: Option<u64>,
    pub lane: Lane,
    pub position_id: Option<String>,
    pub position_epoch: Option<u64>,
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
}

#[derive(Debug, Serialize)]
struct ShadowLifecycleRecord {
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
    sample_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_timestamp_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_price_state: Option<ghost_core::shadow_ledger::types::PriceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_price_reason: Option<ghost_core::shadow_ledger::types::PriceReason>,
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
            .max(SHADOW_POSITION_TIME_STOP_MS)
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

        self.legacy_shadow_curve_snapshot(base_mint)
    }

    fn current_runtime_shadow_snapshot(
        &self,
        base_mint: &Pubkey,
        observed_at_ms: u64,
    ) -> Option<MarketSnapshot> {
        let mut snapshot = self.current_shadow_curve_snapshot(base_mint)?;
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
        let position_bonding_curve = {
            let positions = self.positions.read();
            positions.get(base_mint).map(|pos| pos.bonding_curve)
        };
        let curve_key = position_bonding_curve
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
    }

    fn shadow_lifecycle_record_base(
        &self,
        pos: &MonitoredPosition,
        record_type: ShadowLifecycleRecordType,
        now_ms: u64,
        evidence: &PriceTruthEvidence,
    ) -> ShadowLifecycleRecord {
        ShadowLifecycleRecord {
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
            fraction_bps: None,
            remaining_fraction_bps: pos.remaining_fraction_bps,
            entry_price: pos.entry_price_sol,
            exit_price: None,
            entry_value_sol: None,
            exit_value_sol: None,
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
            sample_slot: evidence.slot,
            sample_timestamp_ms: evidence.timestamp_ms,
            sample_age_ms: evidence.age_ms,
            sample_price_state: evidence.price_state,
            sample_price_reason: evidence.price_reason,
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
        });
        let position_id = event_context
            .position_id
            .clone()
            .unwrap_or_else(|| format!("{}:{}:{}", pool_amm_id, base_mint, now_ms));
        let position_epoch = event_context.position_epoch.unwrap_or(1_u64);
        let shadow_market_activity =
            ShadowMarketActivityAnchor::from_registration(now_ms, initial_shadow_snapshot.as_ref());
        let position = MonitoredPosition {
            candidate_id: event_context.candidate_id.clone(),
            lane: event_context.lane,
            pool_amm_id,
            base_mint,
            bonding_curve,
            entry_time: Instant::now(),
            entry_unix_ms: now_ms,
            entry_price_sol,
            entry_size_lamports: entry_amount_lamports.unwrap_or(0),
            entry_token_amount_raw: entry_token_amount_raw.unwrap_or(0),
            remaining_token_amount_raw: entry_token_amount_raw.unwrap_or(0),
            position_id: position_id.clone(),
            position_epoch,
            entry_order_id: event_context.entry_order_id.clone(),
            quote_id: event_context.quote_id.clone(),
            slot: event_context.slot,
            peak_since_entry: entry_price_sol.unwrap_or(0.0),
            last_peak_unix_ms: now_ms,
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
            snapshot_timeline,
        };

        positions.insert(base_mint, position);
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
            now_ms,
            entry_token_amount_raw.unwrap_or(0),
            entry_amount_lamports.unwrap_or(0),
        );

        Some(RegisteredPosition {
            position_id,
            position_epoch,
            opened_at_ms: now_ms,
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
            let duration = pos.entry_time.elapsed();
            self.emit_position_closed(&pos, duration.as_millis().min(u128::from(u64::MAX)) as u64);
            info!(
                "🛡️ PostBuyGuardian: Stopped monitoring mint={} (held {:.1}s, signals={})",
                base_mint,
                duration.as_secs_f64(),
                pos.recent_signals.len()
            );
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
        let mint_keys: Vec<Pubkey> = {
            let positions = self.positions.read();
            if positions.is_empty() {
                return;
            }
            positions.keys().cloned().collect()
        };

        let tick_start = Instant::now();
        let now_ms = current_time_ms();

        for base_mint in &mint_keys {
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
                        self.remember_shadow_snapshot(base_mint, snapshot);
                        self.run_shadow_runtime_tick(base_mint, Some(snapshot), now_ms)
                            .await;
                    } else {
                        self.run_shadow_runtime_tick(base_mint, None, now_ms).await;
                    }
                    continue;
                }
            };

            let latest = &snapshots[snapshots.len() - 1];
            if self.note_shadow_market_activity(base_mint, latest, now_ms) {
                self.refresh_shadow_time_stop_anchor(base_mint).await;
            }
            self.remember_shadow_snapshot(base_mint, latest);

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
            self.run_shadow_runtime_tick(base_mint, Some(runtime_snapshot), now_ms)
                .await;
        }

        self.flush_aem_outcomes(now_ms);

        // ── Auto-unregister: sync with managed position runtime ──
        self.sync_with_position_runtime(&mint_keys).await;

        let tick_elapsed = tick_start.elapsed();
        if tick_elapsed.as_millis() > self.config.tick_interval_ms as u128 {
            warn!(
                "🛡️ PostBuyGuardian: Tick overrun! Took {}ms (budget={}ms, positions={})",
                tick_elapsed.as_millis(),
                self.config.tick_interval_ms,
                mint_keys.len()
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
        let time_stop_due = inactivity_elapsed_ms >= SHADOW_POSITION_TIME_STOP_MS;
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
        let time_stop_due = inactivity_elapsed_ms >= SHADOW_POSITION_TIME_STOP_MS;
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
                candidate_id: "cand-shadow-lazy".to_string(),
                entry_order_id: "shadow-entry-1".to_string(),
                quote_id: "shadow-quote-1".to_string(),
                slot: Some(42),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:1".to_string()),
                position_epoch: Some(1),
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
                candidate_id: "cand-shadow-gap".to_string(),
                entry_order_id: "shadow-entry-gap".to_string(),
                quote_id: "shadow-quote-gap".to_string(),
                slot: Some(21),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:gap".to_string()),
                position_epoch: Some(3),
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
                candidate_id: "cand-shadow-close".to_string(),
                entry_order_id: "shadow-entry-close".to_string(),
                quote_id: "shadow-quote-close".to_string(),
                slot: Some(77),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:close".to_string()),
                position_epoch: Some(4),
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
                candidate_id: "cand-shadow-simple-tp".to_string(),
                entry_order_id: "shadow-entry-simple-tp".to_string(),
                quote_id: "shadow-quote-simple-tp".to_string(),
                slot: Some(101),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:simple-tp".to_string()),
                position_epoch: Some(5),
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
                candidate_id: "cand-shadow-simple-sl".to_string(),
                entry_order_id: "shadow-entry-simple-sl".to_string(),
                quote_id: "shadow-quote-simple-sl".to_string(),
                slot: Some(121),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:simple-sl".to_string()),
                position_epoch: Some(6),
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
                candidate_id: "cand-shadow-time-stop".to_string(),
                entry_order_id: "shadow-entry-time-stop".to_string(),
                quote_id: "shadow-quote-time-stop".to_string(),
                slot: Some(55),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:time-stop".to_string()),
                position_epoch: Some(5),
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
                candidate_id: candidate_id.clone(),
                entry_order_id: "shadow-entry-expired-time-stop".to_string(),
                quote_id: "shadow-quote-expired-time-stop".to_string(),
                slot: Some(57),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:expired-time-stop".to_string()),
                position_epoch: Some(7),
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
                    candidate_id: "cand-shadow-inactivity-guard".to_string(),
                    entry_order_id: "shadow-entry-inactivity-guard".to_string(),
                    quote_id: "shadow-quote-inactivity-guard".to_string(),
                    slot: Some(58),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:inactivity-guard".to_string()),
                    position_epoch: Some(12),
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
                candidate_id: "cand-shadow-time-stop-cached".to_string(),
                entry_order_id: "shadow-entry-time-stop-cached".to_string(),
                quote_id: "shadow-quote-time-stop-cached".to_string(),
                slot: Some(56),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:time-stop-cached".to_string()),
                position_epoch: Some(6),
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
                    candidate_id: "cand-shadow-current-curve".to_string(),
                    entry_order_id: "shadow-entry-current-curve".to_string(),
                    quote_id: "shadow-quote-current-curve".to_string(),
                    slot: Some(71),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:current-curve".to_string()),
                    position_epoch: Some(8),
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
                    candidate_id: "cand-shadow-account-state-core".to_string(),
                    entry_order_id: "shadow-entry-account-state-core".to_string(),
                    quote_id: "shadow-quote-account-state-core".to_string(),
                    slot: Some(72),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:account-state-core".to_string()),
                    position_epoch: Some(9),
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
                    candidate_id: "cand-shadow-current-canonical-runtime".to_string(),
                    entry_order_id: "shadow-entry-current-canonical-runtime".to_string(),
                    quote_id: "shadow-quote-current-canonical-runtime".to_string(),
                    slot: Some(72),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:current-canonical-runtime".to_string()),
                    position_epoch: Some(10),
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
                    candidate_id: "cand-shadow-current-canonical-guard".to_string(),
                    entry_order_id: "shadow-entry-current-canonical-guard".to_string(),
                    quote_id: "shadow-quote-current-canonical-guard".to_string(),
                    slot: Some(73),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:current-canonical-guard".to_string()),
                    position_epoch: Some(11),
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
                    candidate_id: "cand-outcome-timeline".to_string(),
                    entry_order_id: "shadow-entry-outcome-timeline".to_string(),
                    quote_id: "shadow-quote-outcome-timeline".to_string(),
                    slot: Some(99),
                    lane: Lane::Shadow,
                    position_id: Some("position-outcome-timeline".to_string()),
                    position_epoch: Some(1),
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
                candidate_id: "cand-shadow-stale-time-stop".to_string(),
                entry_order_id: "shadow-entry-stale-time-stop".to_string(),
                quote_id: "shadow-quote-stale-time-stop".to_string(),
                slot: Some(88),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:stale-time-stop".to_string()),
                position_epoch: Some(7),
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
                candidate_id: "cand-shadow-stale".to_string(),
                entry_order_id: "shadow-entry-stale".to_string(),
                quote_id: "shadow-quote-stale".to_string(),
                slot: Some(11),
                lane: Lane::Shadow,
                position_id: Some("shadow:test:stale".to_string()),
                position_epoch: Some(2),
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
