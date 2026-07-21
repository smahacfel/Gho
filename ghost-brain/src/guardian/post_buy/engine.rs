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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver as StdReceiver, SyncSender, TrySendError as StdTrySendError,
};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::{CanonicalPoolState, StatePhase};
#[cfg(test)]
use ghost_core::shadow_ledger::types::PriceReason;
use ghost_core::shadow_ledger::types::PriceState;
use ghost_core::shadow_ledger::{MarketSnapshot, ShadowLedger};
use ghost_core::ShadowV2PoolPhase;

use crate::aem::{
    AemLedgerWriter, AemRuntime, JsonlAemLedger, ManagementDecisionEvent, ManagementOutcomeEvent,
    OutcomeFeatureSource, OutcomeSample, ReclaimFlag, RevolverAemAdapter, StateFeatures,
    StressBucket, TriggerControlAdapter,
};
use crate::events::{
    CloseReason, ControlCommandAppliedPayload, ControlCommandIssuedPayload, EventEmitter,
    EventKind, ExecutionEvent, ExecutionStressChangedPayload, ExitFilledPayload,
    ExitSubmittedPayload, OracleStalePayload, PositionClosedPayload, PositionOpenedPayload,
    ShadowPositionUnresolvedPayload, ShadowUnresolvedReason,
};
use crate::execution::backend::{
    CommandId as ExecCommandId, ExecutionStressSnapshot as ExecStressSnapshot,
    FillStatus as ExecFillStatus, Lane, StressBucket as ExecStressBucket,
};
use crate::execution::shadow::ShadowBackend;
use crate::oracle::tcf::field::TrendCohesionField;
use crate::oracle::tcf::observation::MarketObservation;
use trigger::{
    PriceTruthError, PriceTruthEvidence, PriceTruthFailureKind, PriceTruthResolver,
    PriceTruthSource, PriceTruthStatus, ShadowExitPriceSample, ShadowExitTruth,
};

#[cfg(test)]
use super::config::DEFAULT_WAIT_FOR_TIMESTOP_MS;
use super::config::{CrashGuardMode, PostBuyGuardianConfig, TimeStopV2Config, TimeStopV2Mode};
use super::exit_policy_v1::{
    CrashGuardNotTriggeredReason, CrashGuardObservationState, CrashGuardPreQuoteDecision,
    CrashGuardQuoteDecision, CrashGuardQuoteRejectionReason, CrashGuardQuoteRequirementV1,
    CrashSampleV1, CrashVectorV1, EffectiveExitPolicyV1Config, ExecutableExitQuote, ExitCandidate,
    ExitCandidateReason, ExitPolicyConfigError, ExitPolicyV1, FinalPolicyDecision,
    MarkEvidenceStatus, PositionSnapshotGuard, PostBuyDecisionSnapshot, PreQuoteDecision,
    QuoteEvidenceRevisionV1, EXECUTABLE_QUOTE_GRADE, EXECUTION_COST_COVERAGE_UNMODELED,
};
#[cfg(test)]
use super::exit_policy_v1::{EXIT_POLICY_V1_ID, EXIT_POLICY_V1_VERSION};
#[cfg(test)]
use super::exit_policy_v2::HetPmGateQuoteStatusV2;
use super::exit_policy_v2::{
    build_entry_value_contract, materialize_anchor, prequote_label, EffectiveHetPmV2Config,
    ExecutablePeakAnchorV1, ExecutableQuoteKeyV2, ExitPolicyV2, HetComparisonCorrelationV1,
    HetPmCandidateV2, HetPmExitReasonV2, HetPmFinalDecisionV2, HetPmGateEvaluationV2, HetPmGateV2,
    HetPmPreQuoteEvaluationV2, HetPmQuoteFinalizationInputV2, HetPmUnknownReasonV2,
    HetPmV2ConfigError, HetPmV2Status, PeakAnchorPreQuoteDecisionV1, PostBuyDecisionExtrasV2,
    PostBuyDecisionViewV2, PostBuySnapshotBundle, PreparedHetComparisonV1,
    PreparedV1V2ComparisonCoreV1, RouteStatusV1, TerminalV2ComparisonOutcomeUnknownReasonV1,
    TerminalV2ComparisonSkipReasonV1, TimeStopV2ProjectionV1, TimeStopV2Subreason,
    TimeStopV2WindowStatus, V1AuthorityTickOutcomeV1, V1AuthorityTickReceiptV1,
    V1ExitApplyStatusV1, V1TerminalCommitStatusV1, V1V2ComparisonRecord, VitalityFeaturesV1,
    HET_PM_V2_MAX_QUOTE_CELLS, HET_PM_V2_POLICY_ID, HET_PM_V2_POLICY_VERSION,
    HET_PM_V2_SAMPLING_MODE, HET_PM_V2_SCHEMA_VERSION, HET_PM_V2_TRAJECTORY_GRADE,
};
use super::exit_replay::{
    ShadowExitReplayIdentity, ShadowExitReplayRecord, ShadowExitReplayTracker,
    REASON_SHUTDOWN_BEFORE_HORIZON,
};
#[cfg(test)]
use super::integration::SHADOW_VIRTUAL_MAGAZINE_TIME_STOP_SECS;
use super::integration::{PositionRuntimeRouter, ShadowPositionBookAemAdapter};
use super::rug_scalp::{
    evaluate_rug_scalp_exit_v1, RugScalpDataCompletenessV1, RugScalpEntryWatermarkV1,
    RugScalpExitProfileConfigErrorV1, RugScalpExitReasonV1, RugScalpFactIngressResultV1,
    RugScalpMarketFactKindV1, RugScalpMarketFactStateV1, RugScalpMarketFactV1,
    RUG_SCALP_EXIT_PROFILE_ID, RUG_SCALP_V2_STRATEGY_ID,
};
#[cfg(test)]
use super::shadow_v2::ShadowV2ValidationHarnessConfig;
use super::shadow_v2::{
    executable_pnl_link_from_canonical_position_fills, ClockDomain, ClockedTimestamp,
    EventOrderComponent, EventOrderKey, ExecutableDynamicExitCandidatePolicyV1,
    ExecutableDynamicExitEvidenceV1, ExecutableDynamicExitObservationV1,
    ExecutableDynamicExitPolicyEvaluatorV1, MeasurementGrade, PoolStateSampleV2,
    ShadowExitAttemptV2, ShadowExitFillModelConfig, ShadowExitFillV2, ShadowPathSampleV2,
    ShadowPathSamplerConfigV2, ShadowPathSamplingModeV2, ShadowPathSamplingReasonV2,
    ShadowTerminalTruthV2, ShadowV2Envelope, ShadowV2ExecutablePnlLink, ShadowV2Record,
    ShadowV2ValidationEvidenceStatus, ShadowV2ValidationHarness, ShadowV2WriteStatus,
    SimulationLevel, TemporalClass, TerminalReasonV2, SHADOW_V2_EXIT_FILL_MODEL_VERSION,
};

#[cfg(test)]
const SHADOW_POSITION_TIME_STOP_MS: u64 = DEFAULT_WAIT_FOR_TIMESTOP_MS;
const SHADOW_LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;
const SHADOW_TOKEN_DECIMAL_FACTOR_F64: f64 = 1_000_000.0;
const SHADOW_V2_EXIT_FEE_BPS_DIAGNOSTIC_MODEL: u16 = 100;
const SHADOW_V2_EXIT_SLIPPAGE_BPS_DIAGNOSTIC_MODEL: u16 = 150;
use super::signals::*;
use super::trajectory_v1::{materialize_trajectory_v1, TrajectoryFeaturesV1};

type PostBuySnapshotBundleMaterialization = (
    PostBuySnapshotBundle,
    Option<MarketSnapshot>,
    Option<MarketSnapshot>,
    Option<MarketSnapshot>,
);

const SHADOW_QUOTE_RETRY_INTERVAL_MS: u64 = 500;

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
        // `slot` and `timestamp_ms` may advance after a read-only RPC
        // observation of identical bytes.  They prove quote freshness, not a
        // market write.  AccountStateCore materializes `tx_count` from its
        // data-change counter, so it is the only activity heartbeat here.
        if snapshot.tx_count <= self.tx_count {
            return false;
        }
        self.last_seen_ms = now_ms;
        self.snapshot_ts_ms = snapshot.timestamp_ms;
        self.slot = snapshot.slot;
        self.tx_count = snapshot.tx_count;
        true
    }
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
        // A later observation timestamp is not a later market update.  The
        // checkpoint is for vitality/activity, therefore only the canonical
        // AccountStateCore data-change counter can advance it.
        self.tx_count > previous.tx_count
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
    last_window_at_ms: Option<u64>,
    last_alive_at_ms: Option<u64>,
    latest_window_price_delta_bps: Option<i32>,
    latest_window_state_update_delta: Option<u64>,
    source_window_index: Option<u32>,
    source_checkpoint_slot: Option<u64>,
    source_latest_slot: Option<u64>,
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
        self.last_window_at_ms = Some(now_ms);
        if matches!(status, TimeStopV2WindowStatus::Alive) {
            self.last_alive_at_ms = Some(now_ms);
        }
        self.latest_window_price_delta_bps = price_delta_pct_window.map(|value| {
            (value * 100.0)
                .round()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32
        });
        self.latest_window_state_update_delta = tx_delta_window;
        self.source_window_index = Some(window_index);
        self.source_checkpoint_slot = previous_checkpoint.and_then(|checkpoint| checkpoint.slot);
        self.source_latest_slot = latest_checkpoint.and_then(|checkpoint| checkpoint.slot);
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

    fn project(&self, now_ms: u64, window_ms: u64) -> TimeStopV2ProjectionV1 {
        let quality_fresh = self
            .last_window_at_ms
            .is_some_and(|timestamp_ms| now_ms.saturating_sub(timestamp_ms) <= window_ms.max(1));
        TimeStopV2ProjectionV1 {
            current_status: self
                .last_status
                .unwrap_or(TimeStopV2WindowStatus::StaleOrInsufficient),
            current_subreason: self
                .last_subreason
                .unwrap_or(TimeStopV2Subreason::MissingMarketSample),
            consecutive_non_alive_windows: self.failed_windows,
            last_window_at_ms: self.last_window_at_ms,
            last_alive_at_ms: self.last_alive_at_ms,
            latest_window_price_delta_bps: self.latest_window_price_delta_bps,
            latest_window_state_update_delta: self.latest_window_state_update_delta,
            source_window_index: self.source_window_index,
            source_checkpoint_slot: self.source_checkpoint_slot,
            source_latest_slot: self.source_latest_slot,
            quality_fresh,
        }
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

    /// Signed mark-return range over the bounded canonical timeline. MFE is
    /// the greatest return and MAE the lowest return (normally negative).
    fn mark_excursions_pct(&self, entry_price_sol: Option<f64>) -> (Option<f64>, Option<f64>) {
        let Some(entry_price_sol) =
            entry_price_sol.filter(|price| price.is_finite() && *price > 0.0)
        else {
            return (None, None);
        };
        let mut mfe: Option<f64> = None;
        let mut mae: Option<f64> = None;
        for mark_price_sol in self
            .snapshots
            .iter()
            .filter_map(PriceTruthResolver::normalize_shadow_snapshot_price_sol)
        {
            let return_pct = ((mark_price_sol - entry_price_sol) / entry_price_sol) * 100.0;
            if !return_pct.is_finite() {
                continue;
            }
            mfe = Some(mfe.map_or(return_pct, |current| current.max(return_pct)));
            mae = Some(mae.map_or(return_pct, |current| current.min(return_pct)));
        }
        (mfe, mae)
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
            .is_none_or(|last| !Self::equivalent(last, &snapshot));
        if should_append {
            self.cumulative_volume_sol = snapshot.cum_volume_sol;
            self.snapshots.push(snapshot);
            self.trim(max_snapshots, retention_ms);
        } else if let Some(latest) = self.snapshots.last_mut() {
            // Preserve the newest observation boundary for quote freshness
            // without turning an identical read-only refresh into another
            // trajectory/activity sample.
            latest.slot = snapshot.slot;
            latest.timestamp_ms = snapshot.timestamp_ms;
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
            timestamp_ms: state.last_observed_ts_ms.max(state.last_update_ts_ms),
            cum_volume_sol,
            tx_count: state.data_change_count.max(state.update_count),
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
        lhs.tx_count == rhs.tx_count
            && (lhs.price_sol_per_token - rhs.price_sol_per_token).abs() <= 1e-12
            && (lhs.market_cap_sol - rhs.market_cap_sol).abs() <= 1e-12
            && (lhs.reserve_base - rhs.reserve_base).abs() <= 1e-6
            && (lhs.reserve_quote - rhs.reserve_quote).abs() <= 1e-12
    }

    fn trim_snapshots(
        snapshots: &mut Vec<MarketSnapshot>,
        max_snapshots: usize,
        retention_ms: u64,
    ) {
        if max_snapshots > 0 && snapshots.len() > max_snapshots {
            let excess = snapshots.len() - max_snapshots;
            snapshots.drain(..excess);
        }

        if retention_ms > 0 && snapshots.len() > 1 {
            if let Some(latest_ts) = snapshots.last().map(|snapshot| snapshot.timestamp_ms) {
                let cutoff_ts = latest_ts.saturating_sub(retention_ms);
                let first_retained = snapshots
                    .iter()
                    .position(|snapshot| snapshot.timestamp_ms >= cutoff_ts)
                    .unwrap_or_else(|| snapshots.len().saturating_sub(1));
                if first_retained > 0 {
                    snapshots.drain(..first_retained);
                }
            }
        }
    }

    fn trim(&mut self, max_snapshots: usize, retention_ms: u64) {
        Self::trim_snapshots(&mut self.snapshots, max_snapshots, retention_ms);

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
    state_revision: u64,
    next_exit_action_seq: u64,
    pending_exit_proposal: Option<PendingExitProposal>,
    pending_terminal_commit: Option<PendingTerminalCommit>,
    terminal_tx: Option<oneshot::Sender<ShadowTerminalDisposition>>,
    last_applied_action_id: Option<String>,
    last_source_snapshot_id: Option<String>,
    last_resolved_exit_metrics: Option<ResolvedShadowExitMetrics>,
    last_shadow_outcome: Option<ShadowOutcomeKind>,
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
    last_would_hold_under_legacy_inactivity_policy: Option<bool>,
    last_price_truth: Option<PriceTruthEvidence>,
    last_blocked_truth_status: Option<PriceTruthStatus>,
    last_blocked_truth_timestamp_ms: Option<u64>,
    last_snapshot_source: PriceTruthSource,
    last_shadow_snapshot: Option<MarketSnapshot>,
    last_shadow_v2_path_sample_age_ms: Option<u64>,
    /// Bounded deduplication state for counterfactual CrashGuard lifecycle
    /// records. It is diagnostics-only and never contributes to policy state.
    last_crash_guard_observation: Option<CrashGuardObservationKey>,
    /// Candidate evidence is logged once per canonical sample revision even
    /// when the subsequent quote reaches a different terminal observation.
    last_crash_guard_candidate_revision: Option<(u64, u64)>,
    /// A short reservation prevents two concurrent ticks from appending the
    /// same observation, but is never promoted to durable dedupe state until
    /// the lifecycle JSONL append succeeds.
    pending_crash_guard_observation: Option<PendingCrashGuardObservation>,
    executable_dynamic_exit_evaluator: Option<ExecutableDynamicExitPolicyEvaluatorV1>,
    shadow_market_activity: ShadowMarketActivityAnchor,
    time_stop_v2: TimeStopV2State,
    snapshot_timeline: SnapshotTimeline,
    het_route_status: RouteStatusV1,
    het_executable_peak_anchor: Option<ExecutablePeakAnchorV1>,
    het_next_anchor_seq: u64,
    last_het_pm_v2_comparison_id: Option<String>,
    last_het_pm_v2_candidate_gate: Option<String>,
    last_het_pm_v2_candidate_at_ms: Option<u64>,
    /// Position-owned typed RUG market evidence.  `None` means this is not a
    /// RUG profile position; generic PM positions never consume RUG facts.
    rug_scalp_facts: Option<RugScalpMarketFactStateV1>,
    /// First slot in which a RUG exit condition was canonically observed.
    /// The profile keeps this pending until the frozen primary exit-latency
    /// boundary is reached, then submits one normal PM intent.
    rug_scalp_pending_exit: Option<RugScalpPendingExitV1>,
}

#[derive(Debug, Clone, Copy)]
struct RugScalpPendingExitV1 {
    reason: RugScalpExitReasonV1,
    observed_slot: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CrashGuardObservationKey {
    state: CrashGuardObservationState,
    not_triggered_reason: Option<CrashGuardNotTriggeredReason>,
    quote_rejection_reason: Option<CrashGuardQuoteRejectionReason>,
    sample_slot: Option<u64>,
    sample_timestamp_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingCrashGuardObservation {
    position_id: String,
    position_epoch: u64,
    key: CrashGuardObservationKey,
    candidate_revision: Option<(u64, u64)>,
}

#[derive(Debug, Clone)]
struct PendingExitProposal {
    action_id: String,
    position_id: String,
    position_epoch: u64,
    reason: ExitCandidateReason,
    triggered_at_ms: u64,
    recovery_deadline_ms: u64,
    expected_remaining_quantity: u64,
    source_snapshot_id: String,
    would_hold_under_legacy_inactivity_policy: Option<bool>,
    crash_guard_quote_requirement: Option<CrashGuardQuoteRequirementV1>,
    /// Route bound only for a V2-authoritative action.  A later route change
    /// must prevent the stale Pump-curve proposal from becoming a fill.
    execution_route_id: Option<String>,
    last_quote_attempt_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct PendingTerminalCommit {
    action_id: String,
    record: ShadowLifecycleRecord,
    disposition: ShadowTerminalDisposition,
    last_attempt_ms: Option<u64>,
    lifecycle_jsonl_committed: bool,
    prepared_het_comparison: Option<PreparedHetComparisonV1>,
    het_comparison_write_status: HetComparisonWriteStatusV1,
}

const HET_PM_V2_WRITER_HEALTH_SCHEMA_VERSION: u16 = 2;
const HET_PM_V2_WRITER_HEALTH_ARTIFACT_TYPE: &str = "het_pm_v2_writer_health";
const HET_PM_V2_WRITER_HEALTH_COALESCE_MS: u64 = 100;
const HET_PM_V2_WRITER_HEALTH_SHUTDOWN_BUDGET_MS: u64 = 250;
static HET_PM_V2_WRITER_INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const POSITION_CENSORED_SCHEMA_VERSION: u16 = 1;
const POSITION_CENSORED_ARTIFACT_TYPE: &str = "position_censored_v1";

#[derive(Debug, Serialize)]
struct ShadowPositionCensoredRecord {
    schema_version: u16,
    artifact_type: &'static str,
    run_id: Option<String>,
    position_id: String,
    position_epoch: u64,
    candidate_id: String,
    pool_id: String,
    base_mint: String,
    lane: Lane,
    age_ms: u64,
    reason: &'static str,
    had_v2_candidate: bool,
    candidate_gate: Option<String>,
    comparison_id: Option<String>,
    replay_status: &'static str,
    timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum HetPmV2ObservationJobStateV1 {
    Queued = 0,
    Writing = 1,
    Written = 2,
    Failed = 3,
    CancelledBeforeWrite = 4,
}

impl HetPmV2ObservationJobStateV1 {
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Queued,
            1 => Self::Writing,
            2 => Self::Written,
            3 => Self::Failed,
            4 => Self::CancelledBeforeWrite,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug)]
struct HetPmV2ObservationJobControlV1 {
    state: AtomicU8,
}

impl HetPmV2ObservationJobControlV1 {
    fn queued() -> Self {
        Self {
            state: AtomicU8::new(HetPmV2ObservationJobStateV1::Queued as u8),
        }
    }

    fn start_writing(&self) -> Result<(), HetPmV2ObservationJobStateV1> {
        self.state
            .compare_exchange(
                HetPmV2ObservationJobStateV1::Queued as u8,
                HetPmV2ObservationJobStateV1::Writing as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(HetPmV2ObservationJobStateV1::from_raw)
    }

    fn cancel_before_write(&self) -> Result<(), HetPmV2ObservationJobStateV1> {
        self.state
            .compare_exchange(
                HetPmV2ObservationJobStateV1::Queued as u8,
                HetPmV2ObservationJobStateV1::CancelledBeforeWrite as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(HetPmV2ObservationJobStateV1::from_raw)
    }

    fn finish(&self, success: bool) {
        let final_state = if success {
            HetPmV2ObservationJobStateV1::Written
        } else {
            HetPmV2ObservationJobStateV1::Failed
        };
        self.state.store(final_state as u8, Ordering::Release);
    }
}

struct HetPmV2ObservationWriteJobV1 {
    correlation: HetComparisonCorrelationV1,
    encoded: Vec<u8>,
    acknowledgement: Option<oneshot::Sender<Result<(), String>>>,
    control: Arc<HetPmV2ObservationJobControlV1>,
}

#[derive(Debug, Default)]
struct HetPmV2ObservationWriterStatsV1 {
    comparison_attempts: AtomicU64,
    comparison_ready_for_enqueue: AtomicU64,
    core_validation_skips: AtomicU64,
    final_validation_skips: AtomicU64,
    serialization_skips: AtomicU64,
    payload_oversized_skips: AtomicU64,
    enqueue_attempts: AtomicU64,
    enqueued: AtomicU64,
    queue_full_drops: AtomicU64,
    queue_closed_drops: AtomicU64,
    writes_succeeded: AtomicU64,
    writes_failed: AtomicU64,
    cancelled_before_write: AtomicU64,
    terminal_timeouts: AtomicU64,
    terminal_outcome_unknown: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct HetPmV2ObservationWriterStatsSnapshotV1 {
    comparison_attempts: u64,
    comparison_ready_for_enqueue: u64,
    core_validation_skips: u64,
    final_validation_skips: u64,
    serialization_skips: u64,
    payload_oversized_skips: u64,
    enqueue_attempts: u64,
    enqueued: u64,
    queue_full_drops: u64,
    queue_closed_drops: u64,
    writes_succeeded: u64,
    writes_failed: u64,
    cancelled_before_write: u64,
    terminal_timeouts: u64,
    terminal_outcome_unknown: u64,
}

impl HetPmV2ObservationWriterStatsV1 {
    fn snapshot(&self) -> HetPmV2ObservationWriterStatsSnapshotV1 {
        HetPmV2ObservationWriterStatsSnapshotV1 {
            comparison_attempts: self.comparison_attempts.load(Ordering::Acquire),
            comparison_ready_for_enqueue: self.comparison_ready_for_enqueue.load(Ordering::Acquire),
            core_validation_skips: self.core_validation_skips.load(Ordering::Acquire),
            final_validation_skips: self.final_validation_skips.load(Ordering::Acquire),
            serialization_skips: self.serialization_skips.load(Ordering::Acquire),
            payload_oversized_skips: self.payload_oversized_skips.load(Ordering::Acquire),
            enqueue_attempts: self.enqueue_attempts.load(Ordering::Acquire),
            enqueued: self.enqueued.load(Ordering::Acquire),
            queue_full_drops: self.queue_full_drops.load(Ordering::Acquire),
            queue_closed_drops: self.queue_closed_drops.load(Ordering::Acquire),
            writes_succeeded: self.writes_succeeded.load(Ordering::Acquire),
            writes_failed: self.writes_failed.load(Ordering::Acquire),
            cancelled_before_write: self.cancelled_before_write.load(Ordering::Acquire),
            terminal_timeouts: self.terminal_timeouts.load(Ordering::Acquire),
            terminal_outcome_unknown: self.terminal_outcome_unknown.load(Ordering::Acquire),
        }
    }
}

struct HetPmV2WriterHealthContextV1 {
    writer_instance_id: String,
    sidecar_path: String,
    policy_config_hash: String,
    started_at_ms: u64,
    revision: AtomicU64,
    run_id: OnceLock<String>,
    mixed_run_ids: AtomicBool,
    shutdown_complete: AtomicBool,
    stats: Arc<HetPmV2ObservationWriterStatsV1>,
}

impl HetPmV2WriterHealthContextV1 {
    fn new(
        sidecar_path: &Path,
        policy_config_hash: String,
        stats: Arc<HetPmV2ObservationWriterStatsV1>,
    ) -> Self {
        let started_at_ms = current_time_ms();
        let sequence = HET_PM_V2_WRITER_INSTANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let identity = format!(
            "{}:{policy_config_hash}:{started_at_ms}:{sequence}",
            sidecar_path.display()
        );
        Self {
            writer_instance_id: blake3::hash(identity.as_bytes()).to_hex().to_string(),
            sidecar_path: sidecar_path.display().to_string(),
            policy_config_hash,
            started_at_ms,
            revision: AtomicU64::new(0),
            run_id: OnceLock::new(),
            mixed_run_ids: AtomicBool::new(false),
            shutdown_complete: AtomicBool::new(false),
            stats,
        }
    }

    fn observe_run_id(&self, run_id: &str) {
        if let Some(existing) = self.run_id.get() {
            if existing != run_id {
                self.mixed_run_ids.store(true, Ordering::Release);
            }
            return;
        }
        if self.run_id.set(run_id.to_string()).is_err()
            && self.run_id.get().is_some_and(|existing| existing != run_id)
        {
            self.mixed_run_ids.store(true, Ordering::Release);
        }
    }

    fn mark_changed(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }

    fn snapshot(&self) -> HetPmV2WriterHealthRecordV1 {
        HetPmV2WriterHealthRecordV1 {
            schema_version: HET_PM_V2_WRITER_HEALTH_SCHEMA_VERSION,
            artifact_type: HET_PM_V2_WRITER_HEALTH_ARTIFACT_TYPE,
            writer_instance_id: self.writer_instance_id.clone(),
            run_id: self.run_id.get().cloned(),
            mixed_run_ids: self.mixed_run_ids.load(Ordering::Acquire),
            shutdown_complete: self.shutdown_complete.load(Ordering::Acquire),
            policy_id: HET_PM_V2_POLICY_ID,
            policy_version: HET_PM_V2_POLICY_VERSION,
            policy_config_hash: self.policy_config_hash.clone(),
            sidecar_path: self.sidecar_path.clone(),
            started_at_ms: self.started_at_ms,
            snapshot_generated_at_ms: current_time_ms(),
            revision: self.revision.load(Ordering::Acquire),
            stats: self.stats.snapshot(),
        }
    }
}

#[derive(Debug, Serialize)]
struct HetPmV2WriterHealthRecordV1 {
    schema_version: u16,
    artifact_type: &'static str,
    writer_instance_id: String,
    run_id: Option<String>,
    mixed_run_ids: bool,
    shutdown_complete: bool,
    policy_id: &'static str,
    policy_version: u16,
    policy_config_hash: String,
    sidecar_path: String,
    started_at_ms: u64,
    snapshot_generated_at_ms: u64,
    revision: u64,
    #[serde(flatten)]
    stats: HetPmV2ObservationWriterStatsSnapshotV1,
}

enum HetPmV2ObservationEnqueueErrorV1 {
    Full,
    Closed,
}

enum HetPmV2ObservationIoV1 {
    File(PathBuf),
    #[cfg(test)]
    Controlled {
        path: PathBuf,
        started: SyncSender<()>,
        release: StdReceiver<()>,
    },
}

impl HetPmV2ObservationIoV1 {
    fn path(&self) -> &Path {
        match self {
            Self::File(path) => path,
            #[cfg(test)]
            Self::Controlled { path, .. } => path,
        }
    }

    fn append(&mut self, encoded: &[u8]) -> std::io::Result<()> {
        match self {
            Self::File(path) => MonitoringEngine::append_prepared_jsonl_bytes(path, encoded),
            #[cfg(test)]
            Self::Controlled {
                path,
                started,
                release,
            } => {
                let _ = started.try_send(());
                release.recv().map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("controlled HET writer release channel closed: {error}"),
                    )
                })?;
                MonitoringEngine::append_prepared_jsonl_bytes(path, encoded)
            }
        }
    }
}

struct HetPmV2ObservationWriterV1 {
    sender: Option<SyncSender<HetPmV2ObservationWriteJobV1>>,
    worker: Option<JoinHandle<()>>,
    health_sender: Option<SyncSender<()>>,
    health_worker: Option<JoinHandle<()>>,
    health_path: Option<PathBuf>,
    health_write_lock: Arc<Mutex<()>>,
    stats: Arc<HetPmV2ObservationWriterStatsV1>,
    health_context: Arc<HetPmV2WriterHealthContextV1>,
    #[cfg(test)]
    stalled_receiver: Option<parking_lot::Mutex<StdReceiver<HetPmV2ObservationWriteJobV1>>>,
}

impl HetPmV2ObservationWriterV1 {
    fn health_path_for_sidecar(path: &Path, writer_instance_id: &str) -> PathBuf {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!(
                "het_pm_v2_writer_health_v1.{writer_instance_id}.json"
            ))
    }

    fn spawn(
        path: PathBuf,
        policy_config_hash: String,
        queue_capacity: usize,
    ) -> std::io::Result<Self> {
        Self::spawn_with_io(
            HetPmV2ObservationIoV1::File(path),
            policy_config_hash,
            queue_capacity,
        )
    }

    fn spawn_with_io(
        io: HetPmV2ObservationIoV1,
        policy_config_hash: String,
        queue_capacity: usize,
    ) -> std::io::Result<Self> {
        let sidecar_path = io.path().to_path_buf();
        let (sender, receiver) = sync_channel(queue_capacity);
        let (health_sender, health_receiver) = sync_channel(1);
        let health_write_lock = Arc::new(Mutex::new(()));
        let stats = Arc::new(HetPmV2ObservationWriterStatsV1::default());
        let health_context = Arc::new(HetPmV2WriterHealthContextV1::new(
            &sidecar_path,
            policy_config_hash,
            Arc::clone(&stats),
        ));
        let health_path =
            Self::health_path_for_sidecar(&sidecar_path, &health_context.writer_instance_id);
        let health_worker_context = Arc::clone(&health_context);
        let health_worker_write_lock = Arc::clone(&health_write_lock);
        let health_worker = std::thread::Builder::new()
            .name("het-pm-v2-writer-health".to_string())
            .spawn(move || {
                Self::run_health_reporter(
                    health_path,
                    health_receiver,
                    health_worker_context,
                    health_worker_write_lock,
                )
            })?;
        let worker_stats = Arc::clone(&stats);
        let worker_health = Arc::clone(&health_context);
        let worker_health_sender = health_sender.clone();
        let worker = match std::thread::Builder::new()
            .name("het-pm-v2-sidecar".to_string())
            .spawn(move || {
                Self::run(
                    io,
                    receiver,
                    worker_stats,
                    worker_health,
                    worker_health_sender,
                )
            }) {
            Ok(worker) => worker,
            Err(error) => {
                drop(health_sender);
                let _ = health_worker.join();
                return Err(error);
            }
        };
        let writer = Self {
            sender: Some(sender),
            worker: Some(worker),
            health_sender: Some(health_sender),
            health_worker: Some(health_worker),
            health_path: Some(Self::health_path_for_sidecar(
                &sidecar_path,
                &health_context.writer_instance_id,
            )),
            health_write_lock,
            stats,
            health_context,
            #[cfg(test)]
            stalled_receiver: None,
        };
        writer.notify_health();
        Ok(writer)
    }

    fn run(
        mut io: HetPmV2ObservationIoV1,
        receiver: StdReceiver<HetPmV2ObservationWriteJobV1>,
        stats: Arc<HetPmV2ObservationWriterStatsV1>,
        health_context: Arc<HetPmV2WriterHealthContextV1>,
        health_sender: SyncSender<()>,
    ) {
        while let Ok(job) = receiver.recv() {
            if let Err(state) = job.control.start_writing() {
                debug_assert_eq!(state, HetPmV2ObservationJobStateV1::CancelledBeforeWrite);
                continue;
            }
            let result = io.append(&job.encoded).map_err(|error| error.to_string());
            job.control.finish(result.is_ok());
            match &result {
                Ok(()) => {
                    stats.writes_succeeded.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    stats.writes_failed.fetch_add(1, Ordering::Relaxed);
                    warn!(
                        path = %io.path().display(),
                        comparison_id = %job.correlation.comparison_id,
                        source_snapshot_id = %job.correlation.source_snapshot_id,
                        error,
                        reason = TerminalV2ComparisonSkipReasonV1::WriterIoFailed.as_label(),
                        "PostBuyGuardian: asynchronous HET-PM V2 sidecar write failed; shadow lifecycle executor remains unaffected"
                    );
                }
            }
            health_context.mark_changed();
            let _ = health_sender.try_send(());
            if let Some(acknowledgement) = job.acknowledgement {
                let _ = acknowledgement.send(result);
            }
        }
        health_context
            .shutdown_complete
            .store(true, Ordering::Release);
        health_context.mark_changed();
        let _ = health_sender.try_send(());
    }

    fn run_health_reporter(
        path: PathBuf,
        receiver: StdReceiver<()>,
        context: Arc<HetPmV2WriterHealthContextV1>,
        write_lock: Arc<Mutex<()>>,
    ) {
        while receiver.recv().is_ok() {
            std::thread::sleep(Duration::from_millis(HET_PM_V2_WRITER_HEALTH_COALESCE_MS));
            while receiver.try_recv().is_ok() {}
            if let Err(error) = Self::persist_health_snapshot(&path, &context, &write_lock) {
                warn!(
                    path = %path.display(),
                    error = %error,
                    writer_instance_id = %context.writer_instance_id,
                    "PostBuyGuardian: HET-PM V2 writer-health snapshot failed; coverage must degrade to unknown"
                );
            }
        }
        let _ = Self::persist_health_snapshot(&path, &context, &write_lock);
    }

    fn persist_health_snapshot(
        path: &Path,
        context: &HetPmV2WriterHealthContextV1,
        write_lock: &Mutex<()>,
    ) -> std::io::Result<()> {
        let _guard = write_lock.lock();
        MonitoringEngine::write_het_pm_v2_writer_health(path, &context.snapshot())
    }

    fn notify_health(&self) {
        self.health_context.mark_changed();
        if let Some(sender) = self.health_sender.as_ref() {
            let _ = sender.try_send(());
        }
    }

    fn writer_instance_id(&self) -> &str {
        &self.health_context.writer_instance_id
    }

    fn record_comparison_core_outcome(&self, core: &PreparedV1V2ComparisonCoreV1) {
        let correlation = match core {
            PreparedV1V2ComparisonCoreV1::Ready(record) => HetComparisonCorrelationV1 {
                comparison_id: record.comparison_id.clone(),
                source_snapshot_id: record.snapshot_id.clone(),
                run_id: record.run_id.clone(),
                writer_instance_id: record.writer_instance_id.clone(),
            },
            PreparedV1V2ComparisonCoreV1::Skipped { correlation, .. } => correlation.clone(),
        };
        self.health_context
            .shutdown_complete
            .store(false, Ordering::Release);
        self.health_context.observe_run_id(&correlation.run_id);
        self.stats
            .comparison_attempts
            .fetch_add(1, Ordering::Relaxed);
        if matches!(
            core,
            PreparedV1V2ComparisonCoreV1::Skipped {
                reason: TerminalV2ComparisonSkipReasonV1::CoreSemanticValidationFailed,
                ..
            }
        ) {
            self.stats
                .core_validation_skips
                .fetch_add(1, Ordering::Relaxed);
        }
        self.notify_health();
    }

    fn record_comparison_final_outcome(&self, prepared: &PreparedHetComparisonV1) {
        self.health_context
            .shutdown_complete
            .store(false, Ordering::Release);
        self.health_context
            .observe_run_id(&prepared.correlation().run_id);
        match prepared {
            PreparedHetComparisonV1::Ready { .. } => {
                self.stats
                    .comparison_ready_for_enqueue
                    .fetch_add(1, Ordering::Relaxed);
            }
            PreparedHetComparisonV1::Skipped {
                reason: TerminalV2ComparisonSkipReasonV1::FinalSemanticValidationFailed,
                ..
            } => {
                self.stats
                    .final_validation_skips
                    .fetch_add(1, Ordering::Relaxed);
            }
            PreparedHetComparisonV1::Skipped {
                reason: TerminalV2ComparisonSkipReasonV1::SerializationFailed,
                ..
            } => {
                self.stats
                    .serialization_skips
                    .fetch_add(1, Ordering::Relaxed);
            }
            PreparedHetComparisonV1::Skipped {
                reason: TerminalV2ComparisonSkipReasonV1::PayloadOversized,
                ..
            } => {
                self.stats
                    .payload_oversized_skips
                    .fetch_add(1, Ordering::Relaxed);
            }
            PreparedHetComparisonV1::Skipped { .. } => {}
        }
        self.notify_health();
    }

    fn try_enqueue(
        &self,
        correlation: HetComparisonCorrelationV1,
        encoded: Vec<u8>,
        acknowledgement: Option<oneshot::Sender<Result<(), String>>>,
    ) -> Result<Arc<HetPmV2ObservationJobControlV1>, HetPmV2ObservationEnqueueErrorV1> {
        self.health_context
            .shutdown_complete
            .store(false, Ordering::Release);
        self.health_context.observe_run_id(&correlation.run_id);
        self.stats.enqueue_attempts.fetch_add(1, Ordering::Relaxed);
        let control = Arc::new(HetPmV2ObservationJobControlV1::queued());
        let Some(sender) = self.sender.as_ref() else {
            self.stats
                .queue_closed_drops
                .fetch_add(1, Ordering::Relaxed);
            self.notify_health();
            return Err(HetPmV2ObservationEnqueueErrorV1::Closed);
        };
        let job = HetPmV2ObservationWriteJobV1 {
            correlation,
            encoded,
            acknowledgement,
            control: Arc::clone(&control),
        };
        match sender.try_send(job) {
            Ok(()) => {
                self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
                self.notify_health();
                Ok(control)
            }
            Err(StdTrySendError::Full(_)) => {
                self.stats.queue_full_drops.fetch_add(1, Ordering::Relaxed);
                self.notify_health();
                Err(HetPmV2ObservationEnqueueErrorV1::Full)
            }
            Err(StdTrySendError::Disconnected(_)) => {
                self.stats
                    .queue_closed_drops
                    .fetch_add(1, Ordering::Relaxed);
                self.notify_health();
                Err(HetPmV2ObservationEnqueueErrorV1::Closed)
            }
        }
    }

    #[cfg(test)]
    fn stalled(queue_capacity: usize, policy_config_hash: String) -> Self {
        let (sender, receiver) = sync_channel(queue_capacity);
        let stats = Arc::new(HetPmV2ObservationWriterStatsV1::default());
        let health_context = Arc::new(HetPmV2WriterHealthContextV1::new(
            Path::new("stalled-het-pm-v2-sidecar.jsonl"),
            policy_config_hash,
            Arc::clone(&stats),
        ));
        Self {
            sender: Some(sender),
            worker: None,
            health_sender: None,
            health_worker: None,
            health_path: None,
            health_write_lock: Arc::new(Mutex::new(())),
            stats,
            health_context,
            stalled_receiver: Some(parking_lot::Mutex::new(receiver)),
        }
    }

    #[cfg(test)]
    fn controlled(
        path: PathBuf,
        policy_config_hash: String,
        queue_capacity: usize,
    ) -> std::io::Result<(Self, StdReceiver<()>, SyncSender<()>)> {
        let (started_sender, started_receiver) = sync_channel(1);
        let (release_sender, release_receiver) = sync_channel(1);
        let writer = Self::spawn_with_io(
            HetPmV2ObservationIoV1::Controlled {
                path,
                started: started_sender,
                release: release_receiver,
            },
            policy_config_hash,
            queue_capacity,
        )?;
        Ok((writer, started_receiver, release_sender))
    }

    #[cfg(test)]
    fn stats_snapshot(&self) -> HetPmV2ObservationWriterStatsSnapshotV1 {
        self.stats.snapshot()
    }

    fn counters_are_quiescent(&self) -> bool {
        let stats = self.stats.snapshot();
        let local_outcomes = stats.comparison_ready_for_enqueue
            + stats.core_validation_skips
            + stats.final_validation_skips
            + stats.serialization_skips
            + stats.payload_oversized_skips;
        stats.comparison_attempts == local_outcomes
            && stats.comparison_ready_for_enqueue == stats.enqueue_attempts
            && stats.enqueue_attempts
                == stats.enqueued + stats.queue_full_drops + stats.queue_closed_drops
            && stats.enqueued
                == stats.writes_succeeded + stats.writes_failed + stats.cancelled_before_write
    }
}

impl Drop for HetPmV2ObservationWriterV1 {
    fn drop(&mut self) {
        self.sender.take();
        #[cfg(test)]
        self.stalled_receiver.take();
        if let Some(worker) = self.worker.take() {
            if worker.is_finished() {
                if worker.join().is_err() {
                    warn!("PostBuyGuardian: HET-PM V2 sidecar worker panicked during shutdown");
                }
            } else {
                debug!(
                    "PostBuyGuardian: HET-PM V2 sidecar worker detached on bounded shutdown; lifecycle remains fail-open"
                );
            }
        }
        self.health_sender.take();
        if let Some(worker) = self.health_worker.take() {
            if worker.is_finished() {
                if worker.join().is_err() {
                    warn!(
                        "PostBuyGuardian: HET-PM V2 writer-health worker panicked during shutdown"
                    );
                }
            } else {
                debug!(
                    "PostBuyGuardian: HET-PM V2 writer-health worker detached on bounded shutdown"
                );
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HetComparisonWriteStatusV1 {
    NotApplicable,
    NotAttempted,
    Written,
    Skipped {
        reason: TerminalV2ComparisonSkipReasonV1,
        detail: String,
    },
    OutcomeUnknown {
        reason: TerminalV2ComparisonOutcomeUnknownReasonV1,
        detail: String,
    },
}

impl HetComparisonWriteStatusV1 {
    const fn as_label(&self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::NotAttempted => "not_attempted",
            Self::Written => "written",
            Self::Skipped { .. } => "skipped",
            Self::OutcomeUnknown { .. } => "outcome_unknown",
        }
    }

    const fn skip_reason(&self) -> Option<TerminalV2ComparisonSkipReasonV1> {
        match self {
            Self::Skipped { reason, .. } => Some(*reason),
            Self::NotApplicable
            | Self::NotAttempted
            | Self::Written
            | Self::OutcomeUnknown { .. } => None,
        }
    }

    const fn outcome_unknown_reason(&self) -> Option<TerminalV2ComparisonOutcomeUnknownReasonV1> {
        match self {
            Self::OutcomeUnknown { reason, .. } => Some(*reason),
            Self::NotApplicable | Self::NotAttempted | Self::Written | Self::Skipped { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalWriteStatus {
    Ok,
    NotConfigured,
    NotRequired,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalCommitReceipt {
    lifecycle_jsonl: TerminalWriteStatus,
    canonical_shadow_v2: TerminalWriteStatus,
    replay_projection: TerminalWriteStatus,
}

impl TerminalCommitReceipt {
    fn canonical_committed(&self) -> bool {
        matches!(self.canonical_shadow_v2, TerminalWriteStatus::Ok)
    }
}

#[derive(Debug, Clone)]
struct ExecutableQuoteFailure {
    kind: ExecutableQuoteFailureKind,
    evidence: PriceTruthEvidence,
}

#[derive(Debug, Clone)]
struct HetPmV2QuoteCell {
    key: ExecutableQuoteKeyV2,
    outcome: Result<ShadowExitTruth, ExecutableQuoteFailure>,
}

#[derive(Debug, Default)]
struct HetPmV2QuotePlan {
    cells: Vec<(ExecutableQuoteKeyV2, MarketSnapshot)>,
}

impl HetPmV2QuotePlan {
    fn add(
        &mut self,
        mut key: ExecutableQuoteKeyV2,
        sample: &MarketSnapshot,
    ) -> ExecutableQuoteKeyV2 {
        key.sample_slot = sample.slot;
        key.sample_timestamp_ms = Some(sample.timestamp_ms);
        if !self.cells.iter().any(|(existing, _)| existing == &key) {
            if self.cells.len() >= HET_PM_V2_MAX_QUOTE_CELLS {
                error!(
                    quote_cell_count = self.cells.len(),
                    "HET-PM V2 quote plan rejected a cell beyond its static bound"
                );
                return key;
            }
            self.cells.push((key.clone(), sample.clone()));
        }
        key
    }

    fn into_cells(self) -> Vec<(ExecutableQuoteKeyV2, MarketSnapshot)> {
        self.cells
    }
}

struct HetPmV2ResolvedQuoteForFinalization<'a> {
    cell: Option<&'a HetPmV2QuoteCell>,
    quote: Option<ExecutableExitQuote>,
    evidence: Option<QuoteEvidenceRevisionV1>,
}

impl<'a> HetPmV2ResolvedQuoteForFinalization<'a> {
    fn input<'b>(
        &'b self,
        crash_requirement: Option<&'b CrashGuardQuoteRequirementV1>,
    ) -> HetPmQuoteFinalizationInputV2<'b> {
        HetPmQuoteFinalizationInputV2 {
            quote: self.quote.as_ref(),
            quote_key: self.cell.map(|cell| &cell.key),
            quote_evidence: self.evidence,
            crash_requirement,
        }
    }
}

fn het_pm_v2_quote_source_for_reason<'a>(
    reason: HetPmExitReasonV2,
    latest_snapshot: Option<&'a MarketSnapshot>,
    raw_canonical_snapshot: Option<&'a MarketSnapshot>,
) -> Option<&'a MarketSnapshot> {
    match reason {
        // CrashGuard must retain raw canonical provenance. Runtime-observed
        // freshness is intentionally not allowed to make a stale crash path
        // look executable.
        HetPmExitReasonV2::Crash => raw_canonical_snapshot.or(latest_snapshot),
        // The remaining gates decide against the currently executable path.
        // Using the raw CrashGuard snapshot here would make Trailing/Vitality
        // inherit stale Crash evidence and suppress valid lower-gate exits.
        HetPmExitReasonV2::HardLoss
        | HetPmExitReasonV2::ExecutableTrailing
        | HetPmExitReasonV2::VitalityDecay
        | HetPmExitReasonV2::AbsoluteMaxHold => latest_snapshot.or(raw_canonical_snapshot),
    }
}

fn het_pm_v2_quote_key_for_source(
    view: PostBuyDecisionViewV2<'_>,
    source: &MarketSnapshot,
) -> ExecutableQuoteKeyV2 {
    let mut key = ExecutableQuoteKeyV2::from_view(view);
    key.sample_slot = source.slot;
    key.sample_timestamp_ms = Some(source.timestamp_ms);
    key
}

fn het_pm_v2_snapshot_id_for_quote_key(key: &ExecutableQuoteKeyV2) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        key.position_id,
        key.position_epoch,
        key.state_revision,
        key.remaining_quantity_raw,
        key.sample_slot
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        key.sample_timestamp_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string())
    )
}

fn het_pm_v2_quote_key_matches_source(key: &ExecutableQuoteKeyV2, source: &MarketSnapshot) -> bool {
    key.sample_slot == source.slot && key.sample_timestamp_ms == Some(source.timestamp_ms)
}

fn het_pm_v2_peak_snapshot_matches_trajectory(
    source: &MarketSnapshot,
    trajectory: &TrajectoryFeaturesV1,
) -> bool {
    if trajectory.peak_sample_slot != source.slot
        || trajectory.peak_sample_timestamp_ms != Some(source.timestamp_ms)
    {
        return false;
    }
    let Some(peak_mark_price_sol) = trajectory.peak_mark_price_sol else {
        return false;
    };
    let Some(source_price_sol) = PriceTruthResolver::normalize_shadow_snapshot_price_sol(source)
    else {
        return false;
    };
    (source_price_sol - peak_mark_price_sol).abs() <= peak_mark_price_sol.abs().max(1.0) * 1e-9
}

fn het_pm_v2_historical_peak_anchor_request(
    view: PostBuyDecisionViewV2<'_>,
    peak_source: &MarketSnapshot,
) -> Option<PeakAnchorPreQuoteDecisionV1> {
    if view.extras.executable_peak_anchor.is_some()
        || !het_pm_v2_peak_snapshot_matches_trajectory(peak_source, &view.extras.trajectory)
    {
        return None;
    }
    let peak_mark_price_sol = view.extras.trajectory.peak_mark_price_sol?;
    let mut key = ExecutableQuoteKeyV2::from_view(view);
    key.sample_slot = peak_source.slot;
    key.sample_timestamp_ms = Some(peak_source.timestamp_ms);
    Some(PeakAnchorPreQuoteDecisionV1::QuoteRequired {
        source_snapshot_id: het_pm_v2_snapshot_id_for_quote_key(&key),
        key,
        peak_mark_price_sol,
    })
}

fn het_pm_v2_anchor_source_for_key<'a>(
    key: &ExecutableQuoteKeyV2,
    latest_snapshot: Option<&'a MarketSnapshot>,
    raw_canonical_snapshot: Option<&'a MarketSnapshot>,
    trajectory_peak_snapshot: Option<&'a MarketSnapshot>,
) -> Option<&'a MarketSnapshot> {
    latest_snapshot
        .filter(|source| het_pm_v2_quote_key_matches_source(key, source))
        .or_else(|| {
            raw_canonical_snapshot.filter(|source| het_pm_v2_quote_key_matches_source(key, source))
        })
        .or_else(|| {
            trajectory_peak_snapshot
                .filter(|source| het_pm_v2_quote_key_matches_source(key, source))
        })
}

fn het_pm_v2_resolved_quote_for_candidate<'a>(
    view: PostBuyDecisionViewV2<'_>,
    candidate: &HetPmCandidateV2,
    latest_snapshot: Option<&'a MarketSnapshot>,
    raw_canonical_snapshot: Option<&'a MarketSnapshot>,
    quote_cells: &'a [HetPmV2QuoteCell],
) -> HetPmV2ResolvedQuoteForFinalization<'a> {
    let Some(source) = (match candidate {
        HetPmCandidateV2::QuoteRequired(reason) => {
            het_pm_v2_quote_source_for_reason(*reason, latest_snapshot, raw_canonical_snapshot)
        }
        HetPmCandidateV2::Hold | HetPmCandidateV2::Pending | HetPmCandidateV2::Blocked(_) => None,
    }) else {
        return HetPmV2ResolvedQuoteForFinalization {
            cell: None,
            quote: None,
            evidence: None,
        };
    };
    let key = het_pm_v2_quote_key_for_source(view, source);
    let cell = quote_cells.iter().find(|cell| cell.key == key);
    let truth = cell.and_then(|cell| cell.outcome.as_ref().ok());
    HetPmV2ResolvedQuoteForFinalization {
        cell,
        quote: truth.map(|truth| {
            ExecutableExitQuote::new(
                truth.exit_token_amount_raw,
                truth.exit_price_sol,
                truth.exit_value_sol,
                truth.gross_pnl_sol,
                truth.pnl_pct,
            )
        }),
        evidence: truth.map(|truth| {
            QuoteEvidenceRevisionV1::new(
                truth.evidence.slot,
                truth.evidence.timestamp_ms,
                truth.evidence.age_ms,
            )
        }),
    }
}

#[derive(Debug)]
struct PreparedHetPmV2Tick {
    comparison_core: PreparedV1V2ComparisonCoreV1,
    quote_cells: Vec<HetPmV2QuoteCell>,
    anchor_quote_cell: Option<HetPmV2QuoteCell>,
    anchor_request: PeakAnchorPreQuoteDecisionV1,
    /// The only candidate allowed to enter the guarded shadow execution path
    /// when this profile explicitly enables V2 authority.
    authority_candidate: Option<ExitCandidate>,
}

struct HetPmV2TickInput<'a> {
    bundle: &'a PostBuySnapshotBundle,
    latest_snapshot: Option<&'a MarketSnapshot>,
    raw_canonical_snapshot: Option<&'a MarketSnapshot>,
    trajectory_peak_snapshot: Option<&'a MarketSnapshot>,
    v1_prequote: &'a PreQuoteDecision,
    crash_prequote: &'a CrashGuardPreQuoteDecision,
    v1_policy: &'a EffectiveExitPolicyV1Config,
    het_policy: &'a EffectiveHetPmV2Config,
    now_ms: u64,
}

struct V1AuthorityTickInput<'a> {
    snapshot: &'a PostBuyDecisionSnapshot,
    latest_snapshot: Option<&'a MarketSnapshot>,
    crash_evidence_snapshot: Option<&'a MarketSnapshot>,
    authoritative_prequote: &'a PreQuoteDecision,
    crash_prequote: &'a CrashGuardPreQuoteDecision,
    pre_resolved_quotes: &'a [HetPmV2QuoteCell],
    /// `Some` only when this tick deliberately hands a V2-selected candidate
    /// to the shared guarded shadow executor.
    v2_execution_route_id: Option<&'a str>,
    policy: &'a EffectiveExitPolicyV1Config,
    now_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutableQuoteFailureKind {
    MissingSnapshot,
    StaleSnapshot,
    InvalidReserves,
    InvalidNormalization,
    QuantityMismatch,
    ZeroOutput,
    SemanticViolation,
    InternalFailure,
}

impl ExecutableQuoteFailureKind {
    fn unresolved_reason(self) -> ShadowUnresolvedReason {
        match self {
            Self::MissingSnapshot
            | Self::StaleSnapshot
            | Self::InvalidReserves
            | Self::InvalidNormalization
            | Self::QuantityMismatch
            | Self::SemanticViolation => ShadowUnresolvedReason::BlockedByData,
            Self::ZeroOutput => ShadowUnresolvedReason::NoFill,
            Self::InternalFailure => ShadowUnresolvedReason::Failed,
        }
    }
}

impl ExecutableQuoteFailure {
    fn from_price_truth_error(error: PriceTruthError) -> Self {
        let kind = match &error {
            PriceTruthError::Stale { .. } => ExecutableQuoteFailureKind::StaleSnapshot,
            PriceTruthError::BackfillRequired { .. } => ExecutableQuoteFailureKind::MissingSnapshot,
            PriceTruthError::Failure { kind, .. }
            | PriceTruthError::SemanticViolation { kind, .. } => match kind {
                PriceTruthFailureKind::InvalidReserves => {
                    ExecutableQuoteFailureKind::InvalidReserves
                }
                PriceTruthFailureKind::InvalidNormalization => {
                    ExecutableQuoteFailureKind::InvalidNormalization
                }
                PriceTruthFailureKind::QuantityMismatch => {
                    ExecutableQuoteFailureKind::QuantityMismatch
                }
                PriceTruthFailureKind::ZeroOutput => ExecutableQuoteFailureKind::ZeroOutput,
                PriceTruthFailureKind::SemanticViolation => {
                    ExecutableQuoteFailureKind::SemanticViolation
                }
                PriceTruthFailureKind::InternalFailure => {
                    ExecutableQuoteFailureKind::InternalFailure
                }
            },
        };
        Self {
            kind,
            evidence: error.evidence().clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedShadowExitMetrics {
    entry_token_amount_raw: u64,
    exit_token_amount_raw: u64,
    mark_return_pct: Option<f64>,
    executable_gross_return_pct: f64,
    mfe_mark_pct: Option<f64>,
    mae_mark_pct: Option<f64>,
    quote_reserve_base_raw: Option<f64>,
    quote_reserve_quote_sol: Option<f64>,
    quote_own_impact_bps: Option<f64>,
    decision_mark_source: PriceTruthSource,
    decision_mark_slot: Option<u64>,
    decision_mark_timestamp_ms: Option<u64>,
    decision_mark_age_ms: Option<u64>,
}

impl ResolvedShadowExitMetrics {
    fn from_snapshot_and_truth(
        snapshot: &PostBuyDecisionSnapshot,
        truth: &ShadowExitTruth,
    ) -> Self {
        let mark_return_pct = snapshot
            .entry_price_sol()
            .zip(snapshot.mark_price_sol())
            .and_then(|(entry, mark)| {
                (entry.is_finite() && entry > 0.0 && mark.is_finite() && mark > 0.0)
                    .then_some(((mark - entry) / entry) * 100.0)
            });
        let quote_own_impact_bps = snapshot.mark_price_sol().and_then(|mark| {
            (mark.is_finite()
                && mark > 0.0
                && truth.exit_price_sol.is_finite()
                && truth.exit_price_sol > 0.0)
                .then_some(((mark - truth.exit_price_sol) / mark).max(0.0) * 10_000.0)
        });
        Self {
            entry_token_amount_raw: snapshot.entry_token_amount_raw(),
            exit_token_amount_raw: truth.exit_token_amount_raw,
            mark_return_pct,
            executable_gross_return_pct: truth.pnl_pct,
            mfe_mark_pct: snapshot.mfe_mark_pct(),
            mae_mark_pct: snapshot.mae_mark_pct(),
            quote_reserve_base_raw: snapshot.quote_reserve_base_raw(),
            quote_reserve_quote_sol: snapshot.quote_reserve_quote_sol(),
            quote_own_impact_bps,
            decision_mark_source: snapshot.mark_source(),
            decision_mark_slot: snapshot.latest_sample_slot(),
            decision_mark_timestamp_ms: snapshot.latest_sample_timestamp_ms(),
            decision_mark_age_ms: snapshot.latest_sample_age_ms(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShadowOutcomeKind {
    SimulatedFilled,
    BlockedByData,
    NoFill,
    Failed,
}

#[derive(Debug, Clone)]
struct ShadowExitActionHandle {
    base_mint: Pubkey,
    action_id: String,
    position_id: String,
    position_epoch: u64,
    state_revision: u64,
    expected_remaining_quantity: u64,
    reason: ExitCandidateReason,
    triggered_at_ms: u64,
    recovery_deadline_ms: u64,
    source_snapshot_id: String,
    would_hold_under_legacy_inactivity_policy: Option<bool>,
    crash_guard_quote_requirement: Option<CrashGuardQuoteRequirementV1>,
    /// Present only for a V2-authoritative shadow action.  It binds the
    /// guarded proposal to the route that made its executable decision valid.
    execution_route_id: Option<String>,
}

impl ShadowUnresolvedReason {
    fn terminal_reason_v2(self) -> TerminalReasonV2 {
        match self {
            Self::BlockedByData => TerminalReasonV2::BlockedByData,
            Self::NoFill => TerminalReasonV2::NoFill,
            Self::Failed => TerminalReasonV2::Failed,
        }
    }

    fn outcome_kind(self) -> ShadowOutcomeKind {
        match self {
            Self::BlockedByData => ShadowOutcomeKind::BlockedByData,
            Self::NoFill => ShadowOutcomeKind::NoFill,
            Self::Failed => ShadowOutcomeKind::Failed,
        }
    }

    pub const fn as_label(self) -> &'static str {
        match self {
            Self::BlockedByData => "blocked_by_data",
            Self::NoFill => "no_fill",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowTerminalDisposition {
    SimulatedClosed {
        action_id: String,
        reason: String,
        /// Exact net result materialized by Position Manager at its terminal
        /// lifecycle boundary.  `None` remains distinct from a zero PnL.
        net_pnl_lamports: Option<i64>,
        /// Modelled landing slot for the PM-owned exit, if the canonical
        /// executable evidence supplied one.
        exit_landed_slot: Option<u64>,
    },
    SimulationBlocked {
        action_id: String,
        reason: ShadowUnresolvedReason,
    },
}

pub struct RegisteredShadowPosition {
    registration: RegisteredPosition,
    terminal_rx: oneshot::Receiver<ShadowTerminalDisposition>,
}

impl RegisteredShadowPosition {
    pub fn into_parts(
        self,
    ) -> (
        RegisteredPosition,
        oneshot::Receiver<ShadowTerminalDisposition>,
    ) {
        (self.registration, self.terminal_rx)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum PositionApplyError {
    #[error("position not found")]
    PositionNotFound,
    #[error("position epoch mismatch")]
    EpochMismatch,
    #[error("stale position revision")]
    StaleRevision,
    #[error("remaining quantity mismatch")]
    QuantityMismatch,
    #[error("pending action mismatch")]
    ActionMismatch,
    #[error("another action is already pending")]
    ConcurrentActionPending,
    #[error("position is already terminal")]
    AlreadyTerminal,
    #[error("V2 action route is no longer executable")]
    RouteNotExecutable,
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
    pub strategy_id: Option<String>,
    pub exit_profile_id: Option<String>,
    /// Canonical entry ordering evidence for RUG market-fact ingress.  A
    /// modelled fill intentionally carries only the slot, which makes any
    /// same-slot trade a typed ambiguity instead of a guessed post-entry fact.
    pub rug_scalp_entry_watermark_slot: Option<u64>,
    pub rug_scalp_entry_watermark_tx_index: Option<u32>,
    pub rug_scalp_entry_watermark_event_ordinal: Option<u32>,
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
    PositionUnresolved,
    CrashGuardObservation,
    TimeStopV2Window,
}

#[derive(Debug, Clone, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_version: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_config_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    het_pm_v2_comparison_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    het_pm_v2_writer_instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    het_pm_v2_source_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    het_pm_v2_comparison_write_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    het_pm_v2_comparison_skip_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    het_pm_v2_comparison_outcome_unknown_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_reason_v2: Option<TerminalReasonV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_policy_reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_disposition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_token_amount_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_token_amount_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executable_quote_grade: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_cost_coverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    net_pnl_authoritative: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mark_return_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executable_gross_return_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mfe_mark_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mae_mark_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote_reserve_base_raw: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote_reserve_quote_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote_own_impact_bps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_mark_source: Option<PriceTruthSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_mark_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_mark_timestamp_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision_mark_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peak_drawdown_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    absolute_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inactivity_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capacity_occupancy_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    would_hold_under_legacy_inactivity_policy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_guard_mode: Option<CrashGuardMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_guard_state: Option<CrashGuardObservationState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_guard_not_triggered_reason: Option<CrashGuardNotTriggeredReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_guard_quote_rejection_reason: Option<CrashGuardQuoteRejectionReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_guard_consumed_by_policy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authoritative_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_guard_candidate_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_short_window_drop_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_peak_drawdown_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_distinct_slots: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_oldest_sample_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_previous_distinct_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_latest_sample_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_latest_sample_timestamp_ms: Option<u64>,
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
    source_block_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_tx_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_transaction_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_instruction_index: Option<u32>,
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
        ShadowLifecycleRecordType::PositionUnresolved => "position_unresolved",
        ShadowLifecycleRecordType::CrashGuardObservation => "crash_guard_observation",
        ShadowLifecycleRecordType::TimeStopV2Window => "time_stop_v2_window",
    }
}

fn shadow_v2_event_order_key(
    slot: Option<u64>,
    signature: Option<&str>,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    shadow_v2_event_order_key_with_components(
        slot,
        None,
        signature,
        None,
        None,
        None,
        None,
        event_seq_in_process,
        observed_at_wall_ms,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the event payload keeps every identity and economic fact explicit"
)]
fn shadow_v2_event_order_key_with_components(
    slot: Option<u64>,
    block_time: Option<i64>,
    signature: Option<&str>,
    transaction_index: Option<u32>,
    instruction_index: Option<u32>,
    inner_instruction_index: Option<u32>,
    log_message_index_internal: Option<u32>,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    EventOrderKey {
        slot: slot
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        block_time: block_time
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        signature: signature
            .filter(|signature| !signature.trim().is_empty())
            .map(|signature| EventOrderComponent::known(signature.to_string()))
            .unwrap_or_else(EventOrderComponent::unknown),
        transaction_index_or_unknown: transaction_index
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        instruction_index_or_unknown: instruction_index
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        inner_instruction_index_or_unknown: inner_instruction_index
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        // Solana has no native EVM-style logIndex. A known value here is
        // reserved for an internal ordinal produced by enumerating
        // meta.logMessages, not for provider-native chain order.
        log_index_or_unknown: log_message_index_internal
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::not_applicable),
        event_seq_in_process,
        observed_at_wall_ms,
    }
}

fn shadow_v2_lifecycle_has_exact_source_join(record: &ShadowLifecycleRecord) -> bool {
    record
        .source_tx_signature
        .as_deref()
        .map(str::trim)
        .is_some_and(|signature| !signature.is_empty())
}

fn shadow_v2_lifecycle_source_order_key(
    record: &ShadowLifecycleRecord,
    slot: Option<u64>,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    let has_exact_source_join = shadow_v2_lifecycle_has_exact_source_join(record);
    shadow_v2_event_order_key_with_components(
        slot,
        has_exact_source_join
            .then_some(record.source_block_time)
            .flatten(),
        has_exact_source_join
            .then_some(record.source_tx_signature.as_deref())
            .flatten(),
        has_exact_source_join
            .then_some(record.source_transaction_index)
            .flatten(),
        has_exact_source_join
            .then_some(record.source_instruction_index)
            .flatten(),
        None,
        None,
        event_seq_in_process,
        observed_at_wall_ms,
    )
}

fn shadow_v2_lifecycle_source_order_limitations(record: &ShadowLifecycleRecord) -> Vec<String> {
    let mut limitations = Vec::new();
    if record
        .source_tx_signature
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        limitations.push("EXIT_PATH_SOURCE_JOIN_NOT_PROVEN".to_string());
    }
    if record.source_block_time.is_none() {
        limitations.push("EXIT_PATH_SOURCE_BLOCK_TIME_UNAVAILABLE".to_string());
    }
    if record.source_transaction_index.is_none() {
        limitations.push("EXIT_PATH_SOURCE_TRANSACTION_INDEX_UNAVAILABLE".to_string());
    }
    if record.source_instruction_index.is_none() {
        limitations.push("EXIT_PATH_SOURCE_INSTRUCTION_INDEX_UNAVAILABLE".to_string());
    }
    limitations.push("INNER_GROUP_INDEX_NOT_EXACT_INNER_INSTRUCTION_INDEX".to_string());
    limitations.push("SOLANA_NATIVE_LOG_INDEX_NOT_APPLICABLE".to_string());
    limitations.push("LOG_MESSAGE_INDEX_INTERNAL_UNAVAILABLE".to_string());
    limitations
}

fn shadow_v2_derived_event_order_key(
    _slot: Option<u64>,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    let mut order_key =
        shadow_v2_event_order_key(None, None, event_seq_in_process, observed_at_wall_ms);
    order_key.slot = EventOrderComponent::derived();
    order_key.block_time = EventOrderComponent::derived();
    order_key.signature = EventOrderComponent::derived();
    order_key.transaction_index_or_unknown = EventOrderComponent::derived();
    order_key.instruction_index_or_unknown = EventOrderComponent::derived();
    order_key.inner_instruction_index_or_unknown = EventOrderComponent::derived();
    order_key.log_index_or_unknown = EventOrderComponent::derived();
    order_key
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
#[derive(Debug, thiserror::Error)]
pub enum MonitoringEngineConfigError {
    #[error(transparent)]
    ExitPolicyV1(#[from] ExitPolicyConfigError),
    #[error(transparent)]
    HetPmV2(#[from] HetPmV2ConfigError),
    #[error(transparent)]
    RugScalpExitProfile(#[from] RugScalpExitProfileConfigErrorV1),
    #[error("TimeStop V2 projection config could not be serialized for hashing")]
    TimeStopV2ConfigHashSerialization,
}

/// Immutable address tuple used by the launcher-owned, read-only stale-market
/// refresh task.  The monitoring engine only exposes existing shadow position
/// identity here; it neither performs network I/O nor mutates policy state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowMarketRefreshTarget {
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,
}

pub struct MonitoringEngine {
    config: PostBuyGuardianConfig,
    shadow_ledger: Arc<ShadowLedger>,
    account_state_core: Option<Arc<AccountStateReducer>>,
    exit_policy_v1: Option<EffectiveExitPolicyV1Config>,
    het_pm_v2: Option<EffectiveHetPmV2Config>,
    time_stop_v2_config_hash: String,
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
    /// Identity-level shutdown censoring evidence for promotion denominators.
    position_censored_log_path: Option<PathBuf>,
    /// Single bounded observe-only HET V2 comparison writer. Filesystem I/O
    /// runs outside the Tokio authority task and never owns terminal truth.
    het_pm_v2_observation_writer: Option<HetPmV2ObservationWriterV1>,
    /// Preserves a typed startup failure when the optional writer thread could
    /// not be created. The shadow lifecycle executor remains active and HET
    /// rows degrade to `Skipped`.
    het_pm_v2_observation_writer_start_error: Option<String>,
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
    #[cfg(test)]
    pub fn new(
        config: PostBuyGuardianConfig,
        shadow_ledger: Arc<ShadowLedger>,
        signal_tx: mpsc::Sender<GuardianSignal>,
    ) -> Self {
        let exit_policy_v1 = match (config.target_threshold, config.stoploss_threshold) {
            (Some(_), Some(_)) => EffectiveExitPolicyV1Config::from_guardian(&config).ok(),
            _ => None,
        };
        let het_pm_v2 = EffectiveHetPmV2Config::from_guardian(&config).ok();
        let time_stop_v2_config_hash = config
            .time_stop_v2
            .projection_config_hash()
            .expect("TimeStop V2 test config must be serializable");
        Self::from_effective_configs(
            config,
            shadow_ledger,
            signal_tx,
            exit_policy_v1,
            het_pm_v2,
            time_stop_v2_config_hash,
        )
    }

    fn from_effective_configs(
        config: PostBuyGuardianConfig,
        shadow_ledger: Arc<ShadowLedger>,
        signal_tx: mpsc::Sender<GuardianSignal>,
        exit_policy_v1: Option<EffectiveExitPolicyV1Config>,
        het_pm_v2: Option<EffectiveHetPmV2Config>,
        time_stop_v2_config_hash: String,
    ) -> Self {
        Self {
            config,
            shadow_ledger,
            account_state_core: None,
            exit_policy_v1,
            het_pm_v2,
            time_stop_v2_config_hash,
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
            position_censored_log_path: None,
            het_pm_v2_observation_writer: None,
            het_pm_v2_observation_writer_start_error: None,
            shadow_v2_validation_harness: None,
            exit_replay_trackers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Construct the active Position Manager Lite runtime with fail-closed
    /// validation of its effective policy config.
    pub fn try_new(
        config: PostBuyGuardianConfig,
        shadow_ledger: Arc<ShadowLedger>,
        signal_tx: mpsc::Sender<GuardianSignal>,
    ) -> Result<Self, MonitoringEngineConfigError> {
        let exit_policy_v1 = EffectiveExitPolicyV1Config::from_guardian(&config)?;
        let het_pm_v2 = EffectiveHetPmV2Config::from_guardian(&config)?;
        if config.rug_scalp_exit_v1.enabled {
            config.rug_scalp_exit_v1.validate()?;
        }
        let time_stop_v2_config_hash = config
            .time_stop_v2
            .projection_config_hash()
            .map_err(|_| MonitoringEngineConfigError::TimeStopV2ConfigHashSerialization)?;
        Ok(Self::from_effective_configs(
            config,
            shadow_ledger,
            signal_tx,
            Some(exit_policy_v1),
            Some(het_pm_v2),
            time_stop_v2_config_hash,
        ))
    }

    pub fn exit_policy_v1_status(&self) -> Option<super::ExitPolicyV1Status> {
        self.exit_policy_v1
            .as_ref()
            .map(EffectiveExitPolicyV1Config::status)
    }

    pub fn het_pm_v2_status(&self) -> Option<HetPmV2Status> {
        self.het_pm_v2
            .as_ref()
            .map(|policy| policy.status(self.config.exit_policy_v1.crash_guard_mode))
    }

    /// Attach the lane-aware position-management router shared with SignalRouter/AEM.
    pub fn set_position_router(&mut self, position_router: Arc<PositionRuntimeRouter>) {
        self.position_router = Some(position_router);
    }

    pub fn set_account_state_core(&mut self, account_state_core: Arc<AccountStateReducer>) {
        self.account_state_core = Some(account_state_core);
    }

    /// Return active shadow positions whose canonical curve state is absent or
    /// older than `stale_after_ms`.  A caller may use these identities for a
    /// bounded, read-only point query; this method deliberately has no I/O and
    /// never makes a stale sample look current.
    pub fn stale_shadow_market_refresh_targets(
        &self,
        now_ms: u64,
        stale_after_ms: u64,
    ) -> Vec<ShadowMarketRefreshTarget> {
        let Some(account_state_core) = self.account_state_core.as_ref() else {
            return Vec::new();
        };
        let stale_after_ms = stale_after_ms.max(1);
        let positions = self.positions.read();
        let mut targets = positions
            .values()
            .filter(|position| matches!(position.lane, Lane::Shadow))
            .filter_map(|position| {
                let is_stale_or_absent = account_state_core
                    .get_canonical_state(&position.base_mint)
                    .map(|state| now_ms.saturating_sub(state.last_update_ts_ms) >= stale_after_ms)
                    .unwrap_or(true);
                is_stale_or_absent.then_some(ShadowMarketRefreshTarget {
                    pool_amm_id: position.pool_amm_id,
                    base_mint: position.base_mint,
                    bonding_curve: position.bonding_curve,
                })
            })
            .collect::<Vec<_>>();
        targets.sort_by_key(|target| target.base_mint);
        targets
    }

    #[cfg(test)]
    fn set_exit_policy_v1_thresholds_for_tests(
        &mut self,
        take_profit_pct: f64,
        stop_loss_pct: f64,
    ) {
        self.exit_policy_v1 = EffectiveExitPolicyV1Config::new(
            take_profit_pct,
            stop_loss_pct,
            self.config.wait_for_timestop_ms(),
            self.config.exit_policy_v1.quote_recovery_ms,
        )
        .ok();
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
        let het_pm_v2_observation_log_path = shadow_lifecycle_log_path.as_ref().map(|path| {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join("het_pm_v2_observations_v1.jsonl")
        });
        self.position_censored_log_path = shadow_lifecycle_log_path.as_ref().map(|path| {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join("position_censored_v1.jsonl")
        });
        self.set_het_pm_v2_observation_log_path(het_pm_v2_observation_log_path);
        self.shadow_lifecycle_log_path = shadow_lifecycle_log_path;
    }

    /// Configure the probe lifecycle sink without constructing a second HET
    /// sidecar worker. The primary shadow monitor remains the sole producer.
    pub fn set_shadow_lifecycle_log_path_without_het_pm_v2_sidecar(
        &mut self,
        shadow_lifecycle_log_path: Option<PathBuf>,
    ) {
        self.set_het_pm_v2_observation_log_path(None);
        self.position_censored_log_path = shadow_lifecycle_log_path.as_ref().map(|path| {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join("position_censored_v1.jsonl")
        });
        self.shadow_lifecycle_log_path = shadow_lifecycle_log_path;
    }

    pub fn set_het_pm_v2_observation_log_path(&mut self, path: Option<PathBuf>) {
        self.het_pm_v2_observation_writer = None;
        self.het_pm_v2_observation_writer_start_error = None;
        let Some(path) = path else {
            return;
        };
        let Some(policy) = self.het_pm_v2.as_ref().filter(|policy| policy.enabled()) else {
            return;
        };
        match HetPmV2ObservationWriterV1::spawn(
            path.clone(),
            policy.config_hash().to_string(),
            policy.writer_queue_capacity(),
        ) {
            Ok(writer) => {
                self.het_pm_v2_observation_writer = Some(writer);
            }
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "PostBuyGuardian: HET-PM V2 sidecar worker could not start; shadow lifecycle executor remains active"
                );
                self.het_pm_v2_observation_writer_start_error = Some(error.to_string());
            }
        }
    }

    #[cfg(test)]
    fn set_stalled_het_pm_v2_observation_writer(&mut self, queue_capacity: usize) {
        let policy_config_hash = self
            .het_pm_v2
            .as_ref()
            .map(|policy| policy.config_hash().to_string())
            .unwrap_or_else(|| "het-pm-v2-test-config-unavailable".to_string());
        self.het_pm_v2_observation_writer = Some(HetPmV2ObservationWriterV1::stalled(
            queue_capacity,
            policy_config_hash,
        ));
        self.het_pm_v2_observation_writer_start_error = None;
    }

    #[cfg(test)]
    fn set_controlled_het_pm_v2_observation_writer(
        &mut self,
        path: PathBuf,
        queue_capacity: usize,
    ) -> (StdReceiver<()>, SyncSender<()>) {
        let policy_config_hash = self
            .het_pm_v2
            .as_ref()
            .map(|policy| policy.config_hash().to_string())
            .unwrap_or_else(|| "het-pm-v2-test-config-unavailable".to_string());
        let (writer, started, release) =
            HetPmV2ObservationWriterV1::controlled(path, policy_config_hash, queue_capacity)
                .expect("controlled HET-PM V2 writer");
        self.het_pm_v2_observation_writer = Some(writer);
        self.het_pm_v2_observation_writer_start_error = None;
        (started, release)
    }

    #[cfg(test)]
    fn het_pm_v2_observation_writer_stats(
        &self,
    ) -> Option<HetPmV2ObservationWriterStatsSnapshotV1> {
        self.het_pm_v2_observation_writer
            .as_ref()
            .map(HetPmV2ObservationWriterV1::stats_snapshot)
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

    // This is a compatibility boundary for the typed execution event: each
    // field is deliberately explicit so callers cannot construct a partially
    // identified PositionOpened payload.
    #[expect(
        clippy::too_many_arguments,
        reason = "the guarded proposal boundary keeps each immutable snapshot component explicit"
    )]
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
            .max(
                self.het_pm_v2
                    .as_ref()
                    .filter(|policy| policy.enabled())
                    .map(EffectiveHetPmV2Config::trajectory_long_ms)
                    .unwrap_or(0),
            )
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
                let previous = pos.last_shadow_snapshot.as_ref();
                let changed = previous
                    .is_none_or(|previous| !SnapshotTimeline::equivalent(previous, snapshot));
                if !changed {
                    // A later observation of identical account data is fresh
                    // quote evidence, but not a new market write. Keep it at
                    // the boundary used by guarded quote resolution without
                    // advancing trajectory/activity, peak, or revision.
                    if previous
                        .is_some_and(|previous| snapshot.timestamp_ms > previous.timestamp_ms)
                    {
                        pos.last_shadow_snapshot = Some(snapshot.clone());
                        pos.last_snapshot_source = snapshot_source;
                    }
                    return;
                }
                pos.last_shadow_snapshot = Some(snapshot.clone());
                pos.last_snapshot_source = snapshot_source;
                Self::advance_canonical_peak(pos, std::iter::once(snapshot));
                pos.state_revision = pos.state_revision.saturating_add(1);
            }
        }
    }

    /// Peak is an evidence aggregate, not an AEM/Guardian decision. Every
    /// valid canonical sample is allowed to advance it, independently of
    /// whether any observer recommends an action.
    fn advance_canonical_peak<'a>(
        pos: &mut MonitoredPosition,
        snapshots: impl IntoIterator<Item = &'a MarketSnapshot>,
    ) {
        for snapshot in snapshots {
            let Some(mark_price) =
                PriceTruthResolver::normalize_shadow_snapshot_price_sol(snapshot)
            else {
                continue;
            };
            if mark_price.is_finite() && mark_price > pos.peak_since_entry {
                pos.peak_since_entry = mark_price;
                pos.last_peak_unix_ms = snapshot.timestamp_ms;
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
        let changed = pos
            .shadow_market_activity
            .observe_snapshot(snapshot, now_ms);
        if changed {
            pos.state_revision = pos.state_revision.saturating_add(1);
        }
        changed
    }

    /// Materialize the bounded, immutable CrashGuard evidence projection.
    ///
    /// This intentionally reads the raw canonical timeline, rather than the
    /// runtime mark projection used by the legacy inactivity compatibility
    /// path. That projection can carry an observed-at timestamp for a quiet
    /// pool; reusing it here would incorrectly make old crash evidence look
    /// fresh. The materialized value keeps only three samples and scalar
    /// aggregates, and does not allocate or sort the bounded source history.
    fn materialize_crash_vector(
        pos: &MonitoredPosition,
        now_ms: u64,
        policy: &EffectiveExitPolicyV1Config,
    ) -> CrashVectorV1 {
        if matches!(policy.crash_guard_mode(), CrashGuardMode::Disabled) {
            return CrashVectorV1::default();
        }

        let valid_sample = |snapshot: &MarketSnapshot| {
            let slot = snapshot.slot?;
            let price_sol = PriceTruthResolver::normalize_shadow_snapshot_price_sol(snapshot)?;
            (price_sol.is_finite() && price_sol > 0.0 && snapshot.timestamp_ms > 0)
                .then_some(CrashSampleV1::new(price_sol, slot, snapshot.timestamp_ms))
        };

        let Some((latest_index, latest)) = pos
            .snapshot_timeline
            .snapshots
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, snapshot)| valid_sample(snapshot).map(|sample| (index, sample)))
        else {
            return CrashVectorV1::new(
                pos.peak_since_entry,
                None,
                None,
                None,
                None,
                0,
                None,
                None,
                false,
                false,
            );
        };

        let cutoff_ms = latest
            .timestamp_ms()
            .saturating_sub(policy.crash_window_ms());
        let mut oldest: Option<CrashSampleV1> = None;
        let mut previous_distinct_slot: Option<CrashSampleV1> = None;
        let mut latest_in_window: Option<CrashSampleV1> = None;
        let mut previous_in_window: Option<CrashSampleV1> = None;
        let mut current_distinct_slot: Option<u64> = None;
        let mut distinct_slots = 0_u8;
        let mut ordering_valid = latest.timestamp_ms() <= now_ms;
        let mut monotonic_decrease = true;

        for (index, snapshot) in pos.snapshot_timeline.snapshots.iter().enumerate() {
            let sample = valid_sample(snapshot);
            let timestamp_ms = snapshot.timestamp_ms;
            let potentially_relevant = timestamp_ms == 0 || timestamp_ms >= cutoff_ms;
            let Some(sample) = sample else {
                // An old malformed point does not contaminate an otherwise
                // self-contained 1.5s crash window. An unplaceable or recent
                // malformed point does: its chronology cannot be proven.
                if potentially_relevant || index >= latest_index {
                    ordering_valid = false;
                }
                continue;
            };
            if sample.timestamp_ms() > latest.timestamp_ms() {
                ordering_valid = false;
                continue;
            }
            if index > latest_index || sample.timestamp_ms() < cutoff_ms {
                if index > latest_index {
                    ordering_valid = false;
                }
                continue;
            }
            if let Some(previous) = previous_in_window.as_ref() {
                if sample.slot() < previous.slot()
                    || sample.timestamp_ms() <= previous.timestamp_ms()
                {
                    ordering_valid = false;
                }
                if sample.price_sol() >= previous.price_sol() {
                    monotonic_decrease = false;
                }
            }
            if oldest.is_none() {
                oldest = Some(sample.clone());
            }
            if current_distinct_slot != Some(sample.slot()) {
                if let Some(previous) = previous_in_window.as_ref() {
                    previous_distinct_slot = Some(previous.clone());
                }
                current_distinct_slot = Some(sample.slot());
                distinct_slots = distinct_slots.saturating_add(1);
            }
            previous_in_window = Some(sample.clone());
            latest_in_window = Some(sample);
        }

        if latest_in_window.as_ref() != Some(&latest) {
            ordering_valid = false;
        }
        let short_window_drop_fraction = oldest.as_ref().and_then(|oldest| {
            latest_in_window.as_ref().and_then(|latest| {
                (oldest.price_sol().is_finite()
                    && oldest.price_sol() > 0.0
                    && latest.price_sol().is_finite()
                    && latest.price_sol() > 0.0)
                    .then_some(
                        ((oldest.price_sol() - latest.price_sol()) / oldest.price_sol()).max(0.0),
                    )
            })
        });
        let peak_drawdown_fraction = latest_in_window.as_ref().and_then(|latest| {
            (pos.peak_since_entry.is_finite()
                && pos.peak_since_entry > 0.0
                && latest.price_sol().is_finite()
                && latest.price_sol() > 0.0)
                .then_some(
                    ((pos.peak_since_entry - latest.price_sol()) / pos.peak_since_entry).max(0.0),
                )
        });

        CrashVectorV1::new(
            pos.peak_since_entry,
            oldest,
            previous_distinct_slot,
            latest_in_window,
            // `bool::then_some` evaluates its argument eagerly. A canonical
            // sample can legitimately be a few milliseconds ahead of the
            // local wall clock, so subtracting before checking the predicate
            // used to panic the entire monitoring task. Preserve that state as
            // typed unavailable chronology evidence instead.
            (latest.timestamp_ms() <= now_ms).then(|| now_ms.saturating_sub(latest.timestamp_ms())),
            distinct_slots,
            short_window_drop_fraction,
            peak_drawdown_fraction,
            monotonic_decrease && distinct_slots >= 2,
            ordering_valid,
        )
    }

    fn crash_evidence_snapshot(
        pos: &MonitoredPosition,
        vector: &CrashVectorV1,
    ) -> Option<MarketSnapshot> {
        let latest = vector.latest()?;
        pos.snapshot_timeline
            .snapshots
            .iter()
            .rev()
            .find(|snapshot| {
                snapshot.slot == Some(latest.slot())
                    && snapshot.timestamp_ms == latest.timestamp_ms()
                    && PriceTruthResolver::normalize_shadow_snapshot_price_sol(snapshot)
                        .is_some_and(|price| (price - latest.price_sol()).abs() <= f64::EPSILON)
            })
            .cloned()
    }

    fn materialize_post_buy_snapshot_bundle(
        &self,
        base_mint: &Pubkey,
        now_ms: u64,
    ) -> Option<PostBuySnapshotBundleMaterialization> {
        let policy = self.exit_policy_v1.as_ref()?;
        let het_policy = self.het_pm_v2.as_ref()?;
        let positions = self.positions.read();
        let pos = positions.get(base_mint)?;
        if !matches!(pos.lane, Lane::Shadow) {
            return None;
        }
        let (snapshot, latest_snapshot, crash_evidence_snapshot) =
            self.materialize_post_buy_base_from_position(pos, now_ms, policy);
        let trajectory_snapshots = self.trajectory_snapshots_for_materialization(pos);
        let trajectory = materialize_trajectory_v1(
            &trajectory_snapshots,
            now_ms,
            self.config.tick_interval_ms,
            het_policy.trajectory_short_ms(),
            het_policy.trajectory_medium_ms(),
            het_policy.trajectory_long_ms(),
            het_policy.max_newest_sample_age_ms(),
        );
        let trajectory_peak_snapshot =
            Self::trajectory_peak_snapshot(&trajectory_snapshots, &trajectory);
        let time_stop_projection = pos
            .time_stop_v2
            .project(now_ms, self.config.time_stop_v2.window_ms());
        let vitality = VitalityFeaturesV1::from(&time_stop_projection);
        let (entry_value_quote_raw, entry_value_source, entry_value_authoritative_for_shadow) =
            build_entry_value_contract(
                pos.entry_size_lamports,
                pos.entry_price_sol,
                pos.entry_token_amount_raw,
            );
        let extras = PostBuyDecisionExtrasV2 {
            run_id: pos
                .join_metadata
                .run_id
                .clone()
                .unwrap_or_else(|| "unknown_run".to_string()),
            trajectory,
            vitality,
            route_status: pos.het_route_status,
            executable_peak_anchor: pos.het_executable_peak_anchor.clone(),
            entry_value_quote_raw,
            entry_value_source,
            entry_value_authoritative_for_shadow,
        };
        Some((
            PostBuySnapshotBundle {
                base: snapshot,
                v2: extras,
            },
            latest_snapshot,
            crash_evidence_snapshot,
            trajectory_peak_snapshot,
        ))
    }

    fn trajectory_peak_snapshot(
        snapshots: &[MarketSnapshot],
        trajectory: &TrajectoryFeaturesV1,
    ) -> Option<MarketSnapshot> {
        snapshots
            .iter()
            .find(|snapshot| het_pm_v2_peak_snapshot_matches_trajectory(snapshot, trajectory))
            .cloned()
    }

    fn materialize_post_buy_base_from_position(
        &self,
        pos: &MonitoredPosition,
        now_ms: u64,
        policy: &EffectiveExitPolicyV1Config,
    ) -> (
        PostBuyDecisionSnapshot,
        Option<MarketSnapshot>,
        Option<MarketSnapshot>,
    ) {
        let latest_snapshot = pos.last_shadow_snapshot.clone();
        let (mark_price_sol, mut mark_evidence_status, sample_slot, sample_timestamp_ms) =
            match latest_snapshot.as_ref() {
                Some(snapshot) => {
                    match PriceTruthResolver::normalize_shadow_snapshot_price_sol(snapshot) {
                        Some(price) => (
                            Some(price),
                            MarkEvidenceStatus::Available,
                            snapshot.slot,
                            Some(snapshot.timestamp_ms),
                        ),
                        None => (
                            None,
                            MarkEvidenceStatus::Invalid,
                            snapshot.slot,
                            Some(snapshot.timestamp_ms),
                        ),
                    }
                }
                None => (None, MarkEvidenceStatus::Unavailable, None, None),
            };
        let drawdown_pct = mark_price_sol.and_then(|price| {
            (pos.peak_since_entry.is_finite() && pos.peak_since_entry > 0.0)
                .then_some(((pos.peak_since_entry - price) / pos.peak_since_entry).max(0.0) * 100.0)
        });
        let sample_age_ms =
            sample_timestamp_ms.map(|timestamp_ms| now_ms.saturating_sub(timestamp_ms));
        if matches!(mark_evidence_status, MarkEvidenceStatus::Available)
            && sample_age_ms.is_some_and(|age_ms| {
                let stale_after_ms = self.shadow_exit_stale_after_ms();
                stale_after_ms > 0 && age_ms > stale_after_ms
            })
        {
            mark_evidence_status = MarkEvidenceStatus::Stale;
        }
        let quote_reserve_base_raw = latest_snapshot.as_ref().and_then(|snapshot| {
            (snapshot.reserve_base.is_finite() && snapshot.reserve_base > 0.0)
                .then_some(snapshot.reserve_base)
        });
        let quote_reserve_quote_sol = latest_snapshot.as_ref().and_then(|snapshot| {
            (snapshot.reserve_quote.is_finite() && snapshot.reserve_quote > 0.0)
                .then_some(snapshot.reserve_quote)
        });
        let (mut mfe_mark_pct, mut mae_mark_pct) = pos
            .snapshot_timeline
            .mark_excursions_pct(pos.entry_price_sol);
        if let Some(current_mark_return_pct) =
            pos.entry_price_sol
                .zip(mark_price_sol)
                .and_then(|(entry, mark)| {
                    (entry.is_finite() && entry > 0.0 && mark.is_finite() && mark > 0.0)
                        .then_some(((mark - entry) / entry) * 100.0)
                })
        {
            mfe_mark_pct = Some(mfe_mark_pct.map_or(current_mark_return_pct, |current| {
                current.max(current_mark_return_pct)
            }));
            mae_mark_pct = Some(mae_mark_pct.map_or(current_mark_return_pct, |current| {
                current.min(current_mark_return_pct)
            }));
        }
        let guard = PositionSnapshotGuard::new(
            pos.position_id.clone(),
            pos.position_epoch,
            pos.state_revision,
            pos.remaining_token_amount_raw,
            sample_slot,
            sample_timestamp_ms,
        );
        let crash_vector = Self::materialize_crash_vector(pos, now_ms, policy);
        let crash_evidence_snapshot = Self::crash_evidence_snapshot(pos, &crash_vector)
            .or_else(|| pos.snapshot_timeline.latest().cloned());
        let snapshot = PostBuyDecisionSnapshot::new(
            guard,
            pos.lane,
            pos.entry_price_sol,
            pos.entry_token_amount_raw,
            pos.remaining_token_amount_raw,
            pos.entry_unix_ms,
            now_ms.saturating_sub(pos.entry_unix_ms),
            now_ms.saturating_sub(pos.shadow_market_activity.last_seen_ms),
            mark_price_sol,
            mark_evidence_status,
            pos.last_snapshot_source,
            sample_slot,
            sample_timestamp_ms,
            sample_age_ms,
            quote_reserve_base_raw,
            quote_reserve_quote_sol,
            mfe_mark_pct,
            mae_mark_pct,
            pos.peak_since_entry,
            drawdown_pct,
            crash_vector,
            pos.pending_exit_proposal.is_some(),
            policy.config_hash().to_string(),
        );
        (snapshot, latest_snapshot, crash_evidence_snapshot)
    }

    fn materialize_post_buy_decision_snapshot(
        &self,
        base_mint: &Pubkey,
        now_ms: u64,
    ) -> Option<(
        PostBuyDecisionSnapshot,
        Option<MarketSnapshot>,
        Option<MarketSnapshot>,
    )> {
        let policy = self.exit_policy_v1.as_ref()?;
        let positions = self.positions.read();
        let pos = positions.get(base_mint)?;
        if !matches!(pos.lane, Lane::Shadow) {
            return None;
        }
        Some(self.materialize_post_buy_base_from_position(pos, now_ms, policy))
    }

    fn validate_snapshot_guard(
        pos: &MonitoredPosition,
        guard: &PositionSnapshotGuard,
    ) -> Result<(), PositionApplyError> {
        if pos.position_id != guard.position_id() {
            return Err(PositionApplyError::PositionNotFound);
        }
        if pos.position_epoch != guard.position_epoch() {
            return Err(PositionApplyError::EpochMismatch);
        }
        if pos.state_revision != guard.state_revision() {
            return Err(PositionApplyError::StaleRevision);
        }
        if pos.remaining_token_amount_raw != guard.remaining_token_amount_raw() {
            return Err(PositionApplyError::QuantityMismatch);
        }
        if pos.last_shadow_outcome.is_some() {
            return Err(PositionApplyError::AlreadyTerminal);
        }
        Ok(())
    }

    // The guarded proposal boundary keeps each immutable snapshot component
    // explicit; do not collapse it into mutable position state.
    #[expect(
        clippy::too_many_arguments,
        reason = "compatibility callers pass independently derived entry facts explicitly"
    )]
    fn begin_exit_proposal(
        &self,
        base_mint: &Pubkey,
        snapshot_guard: &PositionSnapshotGuard,
        candidate: &ExitCandidate,
        source_snapshot_id: &str,
        inactivity_age_ms: u64,
        crash_guard_quote_requirement: Option<CrashGuardQuoteRequirementV1>,
        execution_route_id: Option<&str>,
        now_ms: u64,
    ) -> Result<ShadowExitActionHandle, PositionApplyError> {
        let policy = self
            .exit_policy_v1
            .as_ref()
            .ok_or(PositionApplyError::PositionNotFound)?;
        let quote_recovery_ms = policy.quote_recovery_ms();
        let would_hold_under_legacy_inactivity_policy =
            matches!(candidate.reason(), ExitCandidateReason::AbsoluteMaxHold)
                .then_some(inactivity_age_ms < policy.inactivity_timeout_ms());
        if matches!(candidate.reason(), ExitCandidateReason::CrashGuard)
            != crash_guard_quote_requirement.is_some()
        {
            return Err(PositionApplyError::ActionMismatch);
        }
        if crash_guard_quote_requirement
            .as_ref()
            .is_some_and(|requirement| requirement.candidate_snapshot_id() != source_snapshot_id)
        {
            return Err(PositionApplyError::ActionMismatch);
        }
        let mut positions = self.positions.write();
        let pos = positions
            .get_mut(base_mint)
            .ok_or(PositionApplyError::PositionNotFound)?;
        Self::validate_snapshot_guard(pos, snapshot_guard)?;
        if pos.pending_exit_proposal.is_some() {
            return Err(PositionApplyError::ConcurrentActionPending);
        }

        let action_seq = pos.next_exit_action_seq;
        pos.next_exit_action_seq = pos.next_exit_action_seq.saturating_add(1);
        let action_id = format!("{}:{}:{}", pos.position_id, pos.position_epoch, action_seq);
        let proposal = PendingExitProposal {
            action_id: action_id.clone(),
            position_id: pos.position_id.clone(),
            position_epoch: pos.position_epoch,
            reason: candidate.reason(),
            triggered_at_ms: now_ms,
            recovery_deadline_ms: now_ms.saturating_add(quote_recovery_ms),
            expected_remaining_quantity: pos.remaining_token_amount_raw,
            source_snapshot_id: source_snapshot_id.to_string(),
            would_hold_under_legacy_inactivity_policy,
            crash_guard_quote_requirement: crash_guard_quote_requirement.clone(),
            execution_route_id: execution_route_id.map(ToOwned::to_owned),
            last_quote_attempt_ms: Some(now_ms),
        };
        pos.pending_exit_proposal = Some(proposal.clone());
        pos.state_revision = pos.state_revision.saturating_add(1);

        Ok(ShadowExitActionHandle {
            base_mint: *base_mint,
            action_id,
            position_id: pos.position_id.clone(),
            position_epoch: pos.position_epoch,
            state_revision: pos.state_revision,
            expected_remaining_quantity: pos.remaining_token_amount_raw,
            reason: candidate.reason(),
            triggered_at_ms: now_ms,
            recovery_deadline_ms: proposal.recovery_deadline_ms,
            source_snapshot_id: source_snapshot_id.to_string(),
            would_hold_under_legacy_inactivity_policy,
            crash_guard_quote_requirement,
            execution_route_id: proposal.execution_route_id,
        })
    }

    fn prepare_pending_quote_retry(
        &self,
        base_mint: &Pubkey,
        snapshot_guard: &PositionSnapshotGuard,
        now_ms: u64,
    ) -> Result<Option<ShadowExitActionHandle>, PositionApplyError> {
        let mut positions = self.positions.write();
        let pos = positions
            .get_mut(base_mint)
            .ok_or(PositionApplyError::PositionNotFound)?;
        Self::validate_snapshot_guard(pos, snapshot_guard)?;
        let proposal = pos
            .pending_exit_proposal
            .as_mut()
            .ok_or(PositionApplyError::ActionMismatch)?;
        if proposal
            .last_quote_attempt_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < SHADOW_QUOTE_RETRY_INTERVAL_MS)
        {
            return Ok(None);
        }
        proposal.last_quote_attempt_ms = Some(now_ms);
        let proposal = proposal.clone();
        pos.state_revision = pos.state_revision.saturating_add(1);
        Ok(Some(ShadowExitActionHandle {
            base_mint: *base_mint,
            action_id: proposal.action_id,
            position_id: proposal.position_id,
            position_epoch: proposal.position_epoch,
            state_revision: pos.state_revision,
            expected_remaining_quantity: proposal.expected_remaining_quantity,
            reason: proposal.reason,
            triggered_at_ms: proposal.triggered_at_ms,
            recovery_deadline_ms: proposal.recovery_deadline_ms,
            source_snapshot_id: proposal.source_snapshot_id,
            would_hold_under_legacy_inactivity_policy: proposal
                .would_hold_under_legacy_inactivity_policy,
            crash_guard_quote_requirement: proposal.crash_guard_quote_requirement,
            execution_route_id: proposal.execution_route_id,
        }))
    }

    fn validate_action_handle(
        pos: &MonitoredPosition,
        handle: &ShadowExitActionHandle,
    ) -> Result<(), PositionApplyError> {
        if pos.position_id != handle.position_id {
            return Err(PositionApplyError::PositionNotFound);
        }
        if pos.position_epoch != handle.position_epoch {
            return Err(PositionApplyError::EpochMismatch);
        }
        if pos.state_revision != handle.state_revision {
            return Err(PositionApplyError::StaleRevision);
        }
        if pos.remaining_token_amount_raw != handle.expected_remaining_quantity {
            return Err(PositionApplyError::QuantityMismatch);
        }
        if pos.last_shadow_outcome.is_some() {
            return Err(PositionApplyError::AlreadyTerminal);
        }
        match pos.pending_exit_proposal.as_ref() {
            Some(proposal) if proposal.action_id == handle.action_id => Ok(()),
            _ => Err(PositionApplyError::ActionMismatch),
        }
    }

    fn apply_shadow_quote_outcome(
        &self,
        handle: &ShadowExitActionHandle,
        snapshot: &PostBuyDecisionSnapshot,
        truth: &ShadowExitTruth,
    ) -> Result<(), PositionApplyError> {
        let mut positions = self.positions.write();
        let pos = positions
            .get_mut(&handle.base_mint)
            .ok_or(PositionApplyError::PositionNotFound)?;
        Self::validate_action_handle(pos, handle)?;
        if let Some(route_id) = handle.execution_route_id.as_deref() {
            if route_id != RouteStatusV1::PumpCurveSupported.route_id()
                || !matches!(pos.het_route_status, RouteStatusV1::PumpCurveSupported)
            {
                return Err(PositionApplyError::RouteNotExecutable);
            }
        }
        if truth.exit_token_amount_raw != handle.expected_remaining_quantity {
            return Err(PositionApplyError::QuantityMismatch);
        }

        pos.realized_exit_value_sol += truth.exit_value_sol;
        // `rug_scalp_exit_v1` freezes the complete entry+exit fixed-cost
        // model at registration, and its quote path is deliberately invoked
        // with zero generic costs.  Do not add another generic estimate here:
        // it would make the emitted net PnL disagree with the +10%/-5%
        // profile lattice.  Other PM profiles preserve their existing path.
        if pos.rug_scalp_facts.is_none() {
            pos.estimated_costs_sol += truth.estimated_costs_sol;
        }
        pos.realized_pnl_sol += truth.gross_pnl_sol;
        if pos.entry_value_sol > 0.0 {
            pos.realized_pnl_pct = (pos.realized_pnl_sol / pos.entry_value_sol) * 100.0;
        }
        pos.total_exits = pos.total_exits.saturating_add(1);
        pos.remaining_fraction_bps = 0;
        pos.remaining_token_amount_raw = 0;
        pos.last_price_truth = Some(truth.evidence.clone());
        pos.last_blocked_truth_status = None;
        pos.last_blocked_truth_timestamp_ms = None;
        pos.last_force_exit_reason_code = Some(handle.reason.reason_code().to_string());
        pos.last_would_hold_under_legacy_inactivity_policy =
            handle.would_hold_under_legacy_inactivity_policy;
        pos.last_close_reason = Some(Self::shadow_close_reason_from_reason_code(Some(
            handle.reason.reason_code(),
        )));
        pos.last_applied_action_id = Some(handle.action_id.clone());
        pos.last_source_snapshot_id = Some(handle.source_snapshot_id.clone());
        pos.last_resolved_exit_metrics = Some(ResolvedShadowExitMetrics::from_snapshot_and_truth(
            snapshot, truth,
        ));
        pos.last_shadow_outcome = Some(ShadowOutcomeKind::SimulatedFilled);
        pos.pending_exit_proposal = None;
        pos.state_revision = pos.state_revision.saturating_add(1);
        Ok(())
    }

    fn terminate_shadow_proposal(
        &self,
        handle: &ShadowExitActionHandle,
        reason: ShadowUnresolvedReason,
        evidence: PriceTruthEvidence,
    ) -> Result<(), PositionApplyError> {
        let mut positions = self.positions.write();
        let pos = positions
            .get_mut(&handle.base_mint)
            .ok_or(PositionApplyError::PositionNotFound)?;
        Self::validate_action_handle(pos, handle)?;
        pos.last_price_truth = Some(evidence);
        pos.last_applied_action_id = Some(handle.action_id.clone());
        pos.last_source_snapshot_id = Some(handle.source_snapshot_id.clone());
        pos.last_would_hold_under_legacy_inactivity_policy =
            handle.would_hold_under_legacy_inactivity_policy;
        pos.last_shadow_outcome = Some(reason.outcome_kind());
        pos.pending_exit_proposal = None;
        pos.state_revision = pos.state_revision.saturating_add(1);
        Ok(())
    }

    /// Drop a sticky proposal only when a fully resolved CrashGuard quote
    /// disproves the crash-specific executable threshold. This is deliberately
    /// narrower than a generic retry cancellation: data failures continue to
    /// follow the existing bounded recovery/unresolved contract.
    fn cancel_shadow_proposal_after_crash_rejection(
        &self,
        handle: &ShadowExitActionHandle,
    ) -> Result<(), PositionApplyError> {
        let mut positions = self.positions.write();
        let pos = positions
            .get_mut(&handle.base_mint)
            .ok_or(PositionApplyError::PositionNotFound)?;
        Self::validate_action_handle(pos, handle)?;
        if !matches!(handle.reason, ExitCandidateReason::CrashGuard) {
            return Err(PositionApplyError::ActionMismatch);
        }
        pos.pending_exit_proposal = None;
        pos.state_revision = pos.state_revision.saturating_add(1);
        Ok(())
    }

    /// A confirmed executable quote can disprove the extra CrashGuard
    /// threshold while the baseline V1 policy still requires a full exit
    /// (for example, the ordinary stop-loss is already hit). Reuse the exact
    /// guarded action and the quote from this tick instead of dropping a
    /// valid baseline exit or opening a second action/quote path.
    fn retarget_shadow_proposal_after_crash_rejection(
        &self,
        handle: &ShadowExitActionHandle,
        fallback_candidate: &ExitCandidate,
        inactivity_age_ms: u64,
    ) -> Result<ShadowExitActionHandle, PositionApplyError> {
        let policy = self
            .exit_policy_v1
            .as_ref()
            .ok_or(PositionApplyError::PositionNotFound)?;
        let fallback_reason = fallback_candidate.reason();
        let would_hold_under_legacy_inactivity_policy =
            matches!(fallback_reason, ExitCandidateReason::AbsoluteMaxHold)
                .then_some(inactivity_age_ms < policy.inactivity_timeout_ms());
        let mut positions = self.positions.write();
        let pos = positions
            .get_mut(&handle.base_mint)
            .ok_or(PositionApplyError::PositionNotFound)?;
        Self::validate_action_handle(pos, handle)?;
        if !matches!(handle.reason, ExitCandidateReason::CrashGuard) {
            return Err(PositionApplyError::ActionMismatch);
        }
        let proposal = pos
            .pending_exit_proposal
            .as_mut()
            .ok_or(PositionApplyError::ActionMismatch)?;
        proposal.reason = fallback_reason;
        proposal.would_hold_under_legacy_inactivity_policy =
            would_hold_under_legacy_inactivity_policy;
        proposal.crash_guard_quote_requirement = None;
        let proposal = proposal.clone();
        pos.state_revision = pos.state_revision.saturating_add(1);

        Ok(ShadowExitActionHandle {
            base_mint: handle.base_mint,
            action_id: proposal.action_id,
            position_id: proposal.position_id,
            position_epoch: proposal.position_epoch,
            state_revision: pos.state_revision,
            expected_remaining_quantity: proposal.expected_remaining_quantity,
            reason: proposal.reason,
            triggered_at_ms: proposal.triggered_at_ms,
            recovery_deadline_ms: proposal.recovery_deadline_ms,
            source_snapshot_id: proposal.source_snapshot_id,
            would_hold_under_legacy_inactivity_policy: proposal
                .would_hold_under_legacy_inactivity_policy,
            crash_guard_quote_requirement: proposal.crash_guard_quote_requirement,
            execution_route_id: proposal.execution_route_id,
        })
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
        _observed_at_ms: u64,
        bonding_curve_override: Option<Pubkey>,
    ) -> Option<MarketSnapshot> {
        // Runtime freshness belongs to this pool's canonical observation
        // boundary (`last_observed_*`), which is materialized by
        // `SnapshotTimeline`.  Global Geyser progress can come from an
        // unrelated pool and therefore must never make this pool's price look
        // fresh or look like market activity.
        self.current_shadow_curve_snapshot_with_curve(base_mint, bonding_curve_override)
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
        pos.het_route_status = match canonical_state.state_phase {
            StatePhase::Canonical if !canonical_state.is_complete => {
                RouteStatusV1::PumpCurveSupported
            }
            StatePhase::Canonical | StatePhase::Migrated => {
                RouteStatusV1::CurveCompletePumpSwapUnsupported
            }
            StatePhase::Bootstrap | StatePhase::PendingConfirmation => RouteStatusV1::Unknown,
        };
        pos.snapshot_timeline
            .ingest_canonical_state(&canonical_state, max_snapshots, retention_ms);
        let timeline = pos.snapshot_timeline.clone_snapshots();
        Self::advance_canonical_peak(pos, timeline.iter());
        Some(timeline)
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
        let timeline = pos.snapshot_timeline.clone_snapshots();
        Self::advance_canonical_peak(pos, timeline.iter());
        Some(timeline)
    }

    fn snapshots_for_tick(&self, base_mint: &Pubkey) -> Option<Vec<MarketSnapshot>> {
        if self.account_state_core.is_some() {
            self.refresh_snapshot_timeline_from_canonical(base_mint)
        } else {
            self.refresh_snapshot_timeline_from_legacy(base_mint)
        }
    }

    fn trajectory_snapshots_for_materialization(
        &self,
        pos: &MonitoredPosition,
    ) -> Vec<MarketSnapshot> {
        let mut snapshots = pos.snapshot_timeline.clone_snapshots();
        if let Some(runtime_snapshot) = pos.last_shadow_snapshot.as_ref() {
            let should_append = snapshots.last().is_none_or(|latest| {
                runtime_snapshot.timestamp_ms > latest.timestamp_ms
                    && !SnapshotTimeline::equivalent(latest, runtime_snapshot)
            });
            if should_append {
                snapshots.push(runtime_snapshot.clone());
                SnapshotTimeline::trim_snapshots(
                    &mut snapshots,
                    self.snapshot_history_max_snapshots(),
                    self.snapshot_history_retention_ms(),
                );
            }
        }
        snapshots
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

    fn append_jsonl_record(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.flush()
    }

    fn append_prepared_jsonl_bytes(path: &Path, encoded: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(encoded)?;
        file.write_all(b"\n")?;
        file.flush()
    }

    fn write_het_pm_v2_writer_health(
        path: &Path,
        record: &HetPmV2WriterHealthRecordV1,
    ) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let encoded = serde_json::to_vec(record).map_err(std::io::Error::other)?;
        let temporary_path = path.with_extension("json.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.flush()?;
        drop(file);
        std::fs::rename(temporary_path, path)
    }

    fn het_pm_v2_writer_instance_id_for_record(&self) -> String {
        self.het_pm_v2_observation_writer
            .as_ref()
            .map(HetPmV2ObservationWriterV1::writer_instance_id)
            .unwrap_or("writer_unavailable")
            .to_string()
    }

    fn record_het_pm_v2_censoring_evidence(
        &self,
        base_mint: &Pubkey,
        core: &PreparedV1V2ComparisonCoreV1,
        now_ms: u64,
    ) {
        let (correlation, had_candidate, candidate_gate) = core.censoring_evidence();
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(base_mint) else {
            return;
        };
        pos.last_het_pm_v2_comparison_id = Some(correlation.comparison_id);
        if had_candidate {
            pos.last_het_pm_v2_candidate_gate = candidate_gate.map(str::to_string);
            pos.last_het_pm_v2_candidate_at_ms = Some(now_ms);
        }
    }

    fn record_het_pm_v2_comparison_core_outcome(&self, core: &PreparedV1V2ComparisonCoreV1) {
        if let Some(writer) = self.het_pm_v2_observation_writer.as_ref() {
            writer.record_comparison_core_outcome(core);
        }
    }

    fn record_het_pm_v2_comparison_final_outcome(&self, prepared: &PreparedHetComparisonV1) {
        if let Some(writer) = self.het_pm_v2_observation_writer.as_ref() {
            writer.record_comparison_final_outcome(prepared);
        }
    }

    fn append_position_censored_record(&self, record: &ShadowPositionCensoredRecord) {
        let Some(path) = self.position_censored_log_path.as_deref() else {
            return;
        };
        if let Err(error) = Self::append_jsonl_record(path, record) {
            warn!(
                position_id = %record.position_id,
                position_epoch = record.position_epoch,
                error = %error,
                "PostBuyGuardian: failed to append shutdown censoring evidence"
            );
        }
    }

    fn enqueue_prepared_het_pm_v2_comparison(
        &self,
        prepared: &PreparedHetComparisonV1,
        acknowledgement: Option<oneshot::Sender<Result<(), String>>>,
    ) -> Result<Arc<HetPmV2ObservationJobControlV1>, HetComparisonWriteStatusV1> {
        let correlation = prepared.correlation();
        let encoded = match prepared {
            PreparedHetComparisonV1::Ready { encoded, .. } => encoded,
            PreparedHetComparisonV1::Skipped { reason, detail, .. } => {
                warn!(
                    comparison_id = %correlation.comparison_id,
                    source_snapshot_id = %correlation.source_snapshot_id,
                    reason = reason.as_label(),
                    detail,
                    "PostBuyGuardian: HET-PM V2 comparison locally degraded to typed Skipped"
                );
                return Err(HetComparisonWriteStatusV1::Skipped {
                    reason: *reason,
                    detail: detail.clone(),
                });
            }
        };
        let Some(writer) = self.het_pm_v2_observation_writer.as_ref() else {
            let (reason, detail) = self
                .het_pm_v2_observation_writer_start_error
                .as_ref()
                .map(|detail| {
                    (
                        TerminalV2ComparisonSkipReasonV1::WriterUnavailable,
                        detail.clone(),
                    )
                })
                .unwrap_or_else(|| {
                    (
                        TerminalV2ComparisonSkipReasonV1::WriterNotConfigured,
                        "het_pm_v2_observation_writer_missing".to_string(),
                    )
                });
            debug!(
                comparison_id = %correlation.comparison_id,
                source_snapshot_id = %correlation.source_snapshot_id,
                reason = reason.as_label(),
                "PostBuyGuardian: HET-PM V2 sidecar unavailable; shadow lifecycle executor remains unaffected"
            );
            return Err(HetComparisonWriteStatusV1::Skipped { reason, detail });
        };
        match writer.try_enqueue(correlation.clone(), encoded.clone(), acknowledgement) {
            Ok(control) => Ok(control),
            Err(HetPmV2ObservationEnqueueErrorV1::Full) => {
                warn!(
                    comparison_id = %correlation.comparison_id,
                    source_snapshot_id = %correlation.source_snapshot_id,
                    reason = TerminalV2ComparisonSkipReasonV1::WriterQueueFull.as_label(),
                    "PostBuyGuardian: bounded HET-PM V2 sidecar queue is full; observer row dropped"
                );
                Err(HetComparisonWriteStatusV1::Skipped {
                    reason: TerminalV2ComparisonSkipReasonV1::WriterQueueFull,
                    detail: "het_pm_v2_observation_writer_queue_full".to_string(),
                })
            }
            Err(HetPmV2ObservationEnqueueErrorV1::Closed) => {
                warn!(
                    comparison_id = %correlation.comparison_id,
                    source_snapshot_id = %correlation.source_snapshot_id,
                    reason = TerminalV2ComparisonSkipReasonV1::WriterQueueClosed.as_label(),
                    "PostBuyGuardian: HET-PM V2 sidecar queue is closed; observer row dropped"
                );
                Err(HetComparisonWriteStatusV1::Skipped {
                    reason: TerminalV2ComparisonSkipReasonV1::WriterQueueClosed,
                    detail: "het_pm_v2_observation_writer_queue_closed".to_string(),
                })
            }
        }
    }

    fn enqueue_nonterminal_het_pm_v2_comparison(&self, prepared: &PreparedHetComparisonV1) {
        if let Err(status) = self.enqueue_prepared_het_pm_v2_comparison(prepared, None) {
            debug!(
                comparison_id = %prepared.correlation().comparison_id,
                source_snapshot_id = %prepared.correlation().source_snapshot_id,
                write_status = status.as_label(),
                skip_reason = status.skip_reason().map(TerminalV2ComparisonSkipReasonV1::as_label),
                "PostBuyGuardian: nonterminal HET-PM V2 observation dropped without delaying V1"
            );
        }
    }

    async fn persist_terminal_het_pm_v2_comparison(
        &self,
        prepared: &PreparedHetComparisonV1,
    ) -> HetComparisonWriteStatusV1 {
        let (acknowledgement, receiver) = oneshot::channel();
        let control =
            match self.enqueue_prepared_het_pm_v2_comparison(prepared, Some(acknowledgement)) {
                Ok(control) => control,
                Err(status) => return status,
            };
        let budget_ms = self
            .het_pm_v2
            .as_ref()
            .map(EffectiveHetPmV2Config::terminal_write_budget_ms)
            .unwrap_or(1);
        match tokio::time::timeout(Duration::from_millis(budget_ms), receiver).await {
            Ok(Ok(Ok(()))) => HetComparisonWriteStatusV1::Written,
            Ok(Ok(Err(detail))) => HetComparisonWriteStatusV1::Skipped {
                reason: TerminalV2ComparisonSkipReasonV1::WriterIoFailed,
                detail,
            },
            Ok(Err(error)) => self.terminal_status_after_writer_ack_loss(
                &control,
                false,
                format!("writer_ack_channel_closed:{error}"),
            ),
            Err(_) => self.terminal_status_after_writer_ack_loss(
                &control,
                true,
                format!("terminal_het_write_budget_exhausted:{budget_ms}ms"),
            ),
        }
    }

    fn terminal_status_after_writer_ack_loss(
        &self,
        control: &HetPmV2ObservationJobControlV1,
        timed_out: bool,
        detail: String,
    ) -> HetComparisonWriteStatusV1 {
        let writer = self.het_pm_v2_observation_writer.as_ref();
        if timed_out {
            if let Some(writer) = writer {
                writer
                    .stats
                    .terminal_timeouts
                    .fetch_add(1, Ordering::Relaxed);
                writer.notify_health();
            }
        }
        match control.cancel_before_write() {
            Ok(()) => {
                if let Some(writer) = writer {
                    writer
                        .stats
                        .cancelled_before_write
                        .fetch_add(1, Ordering::Relaxed);
                    writer.notify_health();
                }
                HetComparisonWriteStatusV1::Skipped {
                    reason: if timed_out {
                        TerminalV2ComparisonSkipReasonV1::WriterTimedOutBeforeWrite
                    } else {
                        TerminalV2ComparisonSkipReasonV1::WriterQueueClosed
                    },
                    detail,
                }
            }
            Err(HetPmV2ObservationJobStateV1::Written) => HetComparisonWriteStatusV1::Written,
            Err(HetPmV2ObservationJobStateV1::Failed) => HetComparisonWriteStatusV1::Skipped {
                reason: TerminalV2ComparisonSkipReasonV1::WriterIoFailed,
                detail: format!("writer_failed_before_ack_observed:{detail}"),
            },
            Err(HetPmV2ObservationJobStateV1::CancelledBeforeWrite) => {
                HetComparisonWriteStatusV1::Skipped {
                    reason: if timed_out {
                        TerminalV2ComparisonSkipReasonV1::WriterTimedOutBeforeWrite
                    } else {
                        TerminalV2ComparisonSkipReasonV1::WriterQueueClosed
                    },
                    detail,
                }
            }
            Err(HetPmV2ObservationJobStateV1::Writing) => {
                if let Some(writer) = writer {
                    writer
                        .stats
                        .terminal_outcome_unknown
                        .fetch_add(1, Ordering::Relaxed);
                    writer.notify_health();
                }
                warn!(
                    timed_out,
                    detail,
                    "PostBuyGuardian: terminal HET-PM V2 writer outcome is unknown; canonical terminal flow continues"
                );
                HetComparisonWriteStatusV1::OutcomeUnknown {
                    reason: if timed_out {
                        TerminalV2ComparisonOutcomeUnknownReasonV1::WriterAckTimedOut
                    } else {
                        TerminalV2ComparisonOutcomeUnknownReasonV1::WriterAckChannelClosed
                    },
                    detail,
                }
            }
            Err(HetPmV2ObservationJobStateV1::Queued) => {
                if let Some(writer) = writer {
                    writer
                        .stats
                        .terminal_outcome_unknown
                        .fetch_add(1, Ordering::Relaxed);
                    writer.notify_health();
                }
                HetComparisonWriteStatusV1::OutcomeUnknown {
                    reason: TerminalV2ComparisonOutcomeUnknownReasonV1::WriterAckChannelClosed,
                    detail: format!("queued_state_cancellation_race:{detail}"),
                }
            }
        }
    }

    fn executable_dynamic_exit_sidecar_settings(
        &self,
    ) -> Option<(PathBuf, Vec<ExecutableDynamicExitCandidatePolicyV1>)> {
        self.shadow_v2_validation_harness
            .as_ref()
            .and_then(|harness| harness.lock().executable_dynamic_exit_sidecar_settings())
    }

    fn executable_dynamic_exit_evaluator_for_position(
        position_id: &str,
        entry_fill_event_id: &str,
        entry_token_amount_raw: u64,
        entry_amount_lamports: u64,
        entry_price_sol: Option<f64>,
        policies: Vec<ExecutableDynamicExitCandidatePolicyV1>,
    ) -> ExecutableDynamicExitPolicyEvaluatorV1 {
        let entry_fill_amount_tokens_raw =
            (entry_token_amount_raw > 0).then_some(entry_token_amount_raw);
        let entry_fill_amount_tokens =
            entry_fill_amount_tokens_raw.map(|raw| raw as f64 / SHADOW_TOKEN_DECIMAL_FACTOR_F64);
        let entry_fill_amount_sol =
            (entry_amount_lamports > 0).then_some(entry_amount_lamports as f64 / 1_000_000_000.0);
        ExecutableDynamicExitPolicyEvaluatorV1::new(
            position_id,
            entry_fill_event_id,
            entry_fill_amount_tokens_raw,
            entry_fill_amount_tokens,
            entry_fill_amount_sol,
            entry_price_sol,
            "post_buy_guardian.entry_fill_context",
            policies,
        )
    }

    fn append_executable_dynamic_exit_sidecar_rows(
        &self,
        path: &Path,
        position_id: &str,
        rows: Vec<ExecutableDynamicExitEvidenceV1>,
    ) {
        for row in rows {
            if let Err(error) = Self::append_jsonl_record(path, &row) {
                warn!(
                    path = %path.display(),
                    %position_id,
                    error = %error,
                    "PostBuyGuardian: executable dynamic exit sidecar write failed; runtime remains fail-open"
                );
                return;
            }
        }
    }

    fn maybe_emit_executable_dynamic_exit_sidecar_from_runtime_path(
        &self,
        base_mint: &Pubkey,
        path_sample: &ShadowPathSampleV2,
        pool_state: &PoolStateSampleV2,
    ) {
        let Some((path, policies)) = self.executable_dynamic_exit_sidecar_settings() else {
            return;
        };
        let (position_id, rows) = {
            let mut positions = self.positions.write();
            let Some(pos) = positions.get_mut(base_mint) else {
                return;
            };
            if pos.executable_dynamic_exit_evaluator.is_none() {
                let entry_fill_amount_tokens_raw =
                    (pos.entry_token_amount_raw > 0).then_some(pos.entry_token_amount_raw);
                let entry_fill_amount_tokens = entry_fill_amount_tokens_raw
                    .map(|raw| raw as f64 / SHADOW_TOKEN_DECIMAL_FACTOR_F64);
                let entry_fill_amount_sol = (pos.entry_size_lamports > 0)
                    .then_some(pos.entry_size_lamports as f64 / 1_000_000_000.0);
                pos.executable_dynamic_exit_evaluator =
                    Some(ExecutableDynamicExitPolicyEvaluatorV1::new(
                        pos.position_id.clone(),
                        pos.entry_order_id.clone(),
                        entry_fill_amount_tokens_raw,
                        entry_fill_amount_tokens,
                        entry_fill_amount_sol,
                        pos.entry_price_sol,
                        "post_buy_guardian.entry_fill_context",
                        policies,
                    ));
            }
            let Some(evaluator) = pos.executable_dynamic_exit_evaluator.as_mut() else {
                return;
            };
            let rows = evaluator.observe_path_sample(ExecutableDynamicExitObservationV1 {
                run_id: &path_sample.envelope.run_id,
                candidate_id: path_sample.envelope.candidate_id.as_deref(),
                pool_id: &path_sample.envelope.pool_id,
                base_mint: &path_sample.envelope.base_mint,
                path_sample,
                pool_state,
                trigger_observed_at_ms: path_sample.event_order_key.observed_at_wall_ms,
                slippage_bps: SHADOW_V2_EXIT_SLIPPAGE_BPS_DIAGNOSTIC_MODEL,
                fee_bps: SHADOW_V2_EXIT_FEE_BPS_DIAGNOSTIC_MODEL,
            });
            (pos.position_id.clone(), rows)
        };
        if !rows.is_empty() {
            self.append_executable_dynamic_exit_sidecar_rows(&path, &position_id, rows);
        }
    }

    fn append_shadow_lifecycle_record(
        &self,
        record: &ShadowLifecycleRecord,
    ) -> TerminalCommitReceipt {
        self.append_shadow_record(record, true)
    }

    fn append_shadow_record(
        &self,
        record: &ShadowLifecycleRecord,
        write_lifecycle_jsonl: bool,
    ) -> TerminalCommitReceipt {
        let lifecycle_jsonl = match (
            write_lifecycle_jsonl,
            self.shadow_lifecycle_log_path.as_deref(),
        ) {
            (false, _) => TerminalWriteStatus::NotRequired,
            (true, Some(path)) => match super::lifecycle_jsonl::append_jsonl_record(path, record) {
                Ok(()) => TerminalWriteStatus::Ok,
                Err(error) => {
                    error!(
                        path = %path.display(),
                        position_id = %record.position_id,
                        error = %error,
                        "PostBuyGuardian: failed to append shadow lifecycle proof"
                    );
                    TerminalWriteStatus::Failed(error.to_string())
                }
            },
            (true, None) => TerminalWriteStatus::NotConfigured,
        };
        let (canonical_shadow_v2, replay_projection) =
            self.append_shadow_v2_lifecycle_record(record);
        TerminalCommitReceipt {
            lifecycle_jsonl,
            canonical_shadow_v2,
            replay_projection,
        }
    }

    fn append_shadow_v2_lifecycle_record(
        &self,
        record: &ShadowLifecycleRecord,
    ) -> (TerminalWriteStatus, TerminalWriteStatus) {
        let Some(harness) = self.shadow_v2_validation_harness.as_ref() else {
            return (
                TerminalWriteStatus::NotConfigured,
                TerminalWriteStatus::NotConfigured,
            );
        };
        if !matches!(record.lane, Lane::Shadow) {
            return (
                TerminalWriteStatus::NotRequired,
                TerminalWriteStatus::NotRequired,
            );
        }

        let mut records = Vec::new();
        let mut terminal_truth_requested = false;
        let mut pending_exit_fill_for_terminal: Option<ShadowExitFillV2> = None;
        if matches!(
            record.record_type,
            ShadowLifecycleRecordType::ExitFilled
                | ShadowLifecycleRecordType::ExitBlocked
                | ShadowLifecycleRecordType::PositionClosed
                | ShadowLifecycleRecordType::PositionUnresolved
        ) {
            records.push(ShadowV2Record::ShadowPathSampleV2(
                self.shadow_v2_path_sample_from_lifecycle(record),
            ));
        }

        if matches!(
            record.record_type,
            ShadowLifecycleRecordType::ExitFilled
                | ShadowLifecycleRecordType::ExitBlocked
                | ShadowLifecycleRecordType::PositionUnresolved
        ) {
            records.push(ShadowV2Record::ShadowExitAttemptV2(
                self.shadow_v2_exit_attempt_from_lifecycle(record),
            ));
            if matches!(record.record_type, ShadowLifecycleRecordType::ExitFilled) {
                let exit_pool_state = self.shadow_v2_exit_pool_state_sample_from_lifecycle(record);
                if let Some(pool_state) = exit_pool_state.as_ref() {
                    records.push(ShadowV2Record::PoolStateSampleV2(pool_state.clone()));
                }
                let exit_fill =
                    self.shadow_v2_exit_fill_from_lifecycle(record, exit_pool_state.as_ref());
                pending_exit_fill_for_terminal = Some(exit_fill.clone());
                records.push(ShadowV2Record::ShadowExitFillV2(exit_fill));
            }
        }

        if matches!(
            record.record_type,
            ShadowLifecycleRecordType::PositionClosed
                | ShadowLifecycleRecordType::PositionUnresolved
        ) {
            if matches!(
                record.record_type,
                ShadowLifecycleRecordType::PositionClosed
            ) && record.total_exits == 0
            {
                records.push(ShadowV2Record::ShadowExitAttemptV2(
                    self.shadow_v2_exit_attempt_from_lifecycle(record),
                ));
                let exit_pool_state = self.shadow_v2_exit_pool_state_sample_from_lifecycle(record);
                if let Some(pool_state) = exit_pool_state.as_ref() {
                    records.push(ShadowV2Record::PoolStateSampleV2(pool_state.clone()));
                }
                let exit_fill =
                    self.shadow_v2_exit_fill_from_lifecycle(record, exit_pool_state.as_ref());
                pending_exit_fill_for_terminal = Some(exit_fill.clone());
                records.push(ShadowV2Record::ShadowExitFillV2(exit_fill));
            }
            terminal_truth_requested = true;
        }

        let mut harness = harness.lock();
        if terminal_truth_requested {
            let executable_pnl_link = if matches!(
                record.record_type,
                ShadowLifecycleRecordType::PositionUnresolved
            ) {
                None
            } else {
                executable_pnl_link_from_canonical_position_fills(
                    harness.canonical_stream(),
                    &record.position_id,
                    pending_exit_fill_for_terminal.as_ref(),
                )
            };
            records.push(ShadowV2Record::ShadowTerminalTruthV2(
                self.shadow_v2_terminal_truth_from_lifecycle(record, executable_pnl_link),
            ));
        }
        let mut terminal_canonical = TerminalWriteStatus::NotRequired;
        let mut terminal_replay = TerminalWriteStatus::NotRequired;
        for record in records {
            let is_terminal = matches!(record, ShadowV2Record::ShadowTerminalTruthV2(_));
            let event_id = record.envelope().event_id.clone();
            let outcome = harness.append_record(record);
            if is_terminal {
                terminal_canonical = Self::terminal_write_status(outcome.canonical_write.clone());
                terminal_replay = Self::terminal_write_status(outcome.replay_write.clone());
            }
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
        (terminal_canonical, terminal_replay)
    }

    fn terminal_write_status(status: ShadowV2WriteStatus) -> TerminalWriteStatus {
        match status {
            ShadowV2WriteStatus::Ok => TerminalWriteStatus::Ok,
            ShadowV2WriteStatus::Err(error) | ShadowV2WriteStatus::Skipped(error) => {
                TerminalWriteStatus::Failed(error)
            }
        }
    }

    fn maybe_emit_shadow_v2_runtime_path_sample(&self, base_mint: &Pubkey, sample_ts_ms: u64) {
        let Some(harness) = self.shadow_v2_validation_harness.as_ref() else {
            return;
        };
        let Some(state) = self.current_canonical_state(base_mint) else {
            return;
        };
        let sampler_config = ShadowPathSamplerConfigV2::standard_120s();
        let (
            candidate_id,
            position_id,
            pool_id,
            bonding_curve,
            entry_price_sol,
            run_id,
            session_id,
            position_epoch,
            age_ms,
        ) = {
            let positions = self.positions.read();
            let Some(pos) = positions.get(base_mint) else {
                return;
            };
            if !matches!(pos.lane, Lane::Shadow) {
                return;
            }
            let age_ms = sample_ts_ms.saturating_sub(pos.entry_unix_ms);
            if age_ms > sampler_config.max_horizon_ms {
                return;
            }
            if pos.last_shadow_v2_path_sample_age_ms.is_none()
                && age_ms < sampler_config.heartbeat_ms
            {
                return;
            }
            if let Some(previous_age_ms) = pos.last_shadow_v2_path_sample_age_ms {
                if age_ms <= previous_age_ms {
                    return;
                }
                if age_ms.saturating_sub(previous_age_ms) < sampler_config.heartbeat_ms {
                    return;
                }
            }
            (
                pos.candidate_id.clone(),
                pos.position_id.clone(),
                pos.pool_amm_id,
                pos.bonding_curve,
                pos.entry_price_sol,
                pos.join_metadata
                    .run_id
                    .clone()
                    .or_else(|| pos.join_metadata.rollout_namespace.clone())
                    .unwrap_or_else(|| "UNKNOWN_SHADOW_V2_RUN".to_string()),
                pos.join_metadata.session_id.clone(),
                pos.position_epoch,
                age_ms,
            )
        };

        let source_write_version = state
            .source_write_version
            .map(|write_version| write_version.to_string())
            .unwrap_or_else(|| "none".to_string());
        let pool_state_event_id = format!(
            "shadow_v2_pool_state_runtime_path:{position_id}:{age_ms}:{}:{source_write_version}",
            state.last_update_slot
        );
        let path_event_id = format!(
            "shadow_v2_runtime_path_sample:{position_id}:{age_ms}:{}:{source_write_version}",
            state.last_update_slot
        );

        let mut pool_envelope = ShadowV2Envelope::contract_header(
            "pool_state_sample_v2",
            run_id.clone(),
            position_id.clone(),
            pool_state_event_id,
            pool_id.to_string(),
            base_mint.to_string(),
        );
        pool_envelope.session_id = session_id.clone();
        pool_envelope.candidate_id = Some(candidate_id.clone());
        pool_envelope.bonding_curve = Some(bonding_curve.to_string());
        pool_envelope.parent_event_id = Some(format!("position_epoch:{position_epoch}"));
        pool_envelope
            .source_refs
            .push("post_buy_guardian:runtime_path_sample_tick".to_string());
        pool_envelope
            .source_refs
            .push("account_state_core:get_canonical_state".to_string());
        pool_envelope
            .limitations
            .push("RUNTIME_PATH_SAMPLE_FROM_ACCOUNT_STATE_CORE".to_string());
        pool_envelope
            .limitations
            .push("TOKEN_DECIMALS_ASSUMED_PUMPFUN_6".to_string());
        pool_envelope
            .limitations
            .push("TRANSACTION_SOURCE_PROOF_NOT_REQUIRED_FOR_ACCOUNT_STATE_BOUNDARY".to_string());

        let pool_event_order_key = shadow_v2_event_order_key(
            Some(state.last_update_slot),
            None,
            shadow_v2_event_seq(sample_ts_ms, 4),
            sample_ts_ms,
        );
        let pool_state = PoolStateSampleV2::from_account_state_core(
            pool_envelope,
            pool_event_order_key.clone(),
            &state,
            sample_ts_ms,
            state.account_data_hash.clone(),
            TemporalClass::PostEntry,
            ClockDomain::StreamObservedMs,
            6,
        );

        let mut path_envelope = ShadowV2Envelope::contract_header(
            "shadow_path_sample_v2",
            run_id,
            position_id.clone(),
            path_event_id,
            pool_id.to_string(),
            base_mint.to_string(),
        );
        path_envelope.session_id = session_id;
        path_envelope.candidate_id = Some(candidate_id);
        path_envelope.bonding_curve = Some(bonding_curve.to_string());
        path_envelope.parent_event_id = Some(pool_state.envelope.event_id.clone());
        path_envelope.produced_at_ms = sample_ts_ms;
        path_envelope.produced_at_slot = Some(state.last_update_slot);
        path_envelope
            .source_refs
            .push("post_buy_guardian:runtime_path_sample_tick".to_string());
        path_envelope.source_refs.push(format!(
            "pool_state_sample_v2:{}",
            pool_state.envelope.event_id
        ));
        path_envelope
            .limitations
            .push("RUNTIME_PATH_SAMPLE_FROM_ACCOUNT_STATE_CORE".to_string());

        let path_sample = ShadowPathSampleV2::from_pool_state_mark(
            path_envelope,
            pool_event_order_key,
            ClockedTimestamp {
                field_name: "sample_ts_ms".to_string(),
                value: Some(sample_ts_ms as i64),
                clock_domain: ClockDomain::StreamObservedMs,
                clock_source: "post_buy_guardian.runtime_path_sample_tick".to_string(),
                causal_boundary: "POST_ENTRY_RUNTIME_PATH_SAMPLE".to_string(),
            },
            age_ms,
            &pool_state,
            ShadowV2PoolPhase::BondingCurve,
            entry_price_sol,
            ShadowPathSamplingModeV2::Standard120s,
            ShadowPathSamplingReasonV2::Heartbeat,
        );

        let sidecar_pool_state = pool_state.clone();
        let sidecar_path_sample = path_sample.clone();
        let mut harness = harness.lock();
        let pool_outcome = harness.append_record(ShadowV2Record::PoolStateSampleV2(pool_state));
        let path_outcome = harness.append_record(ShadowV2Record::ShadowPathSampleV2(path_sample));
        let path_sample_complete =
            path_outcome.validation_evidence_status == ShadowV2ValidationEvidenceStatus::Complete;
        if pool_outcome.validation_evidence_status == ShadowV2ValidationEvidenceStatus::Complete
            && path_sample_complete
        {
            debug!(
                %position_id,
                age_ms,
                slot = state.last_update_slot,
                "PostBuyGuardian: Shadow V2 runtime path sample emitted"
            );
        } else {
            warn!(
                %position_id,
                age_ms,
                pool_status = ?pool_outcome.validation_evidence_status,
                path_status = ?path_outcome.validation_evidence_status,
                "PostBuyGuardian: Shadow V2 runtime path sample append incomplete"
            );
        }
        drop(harness);
        if path_sample_complete {
            self.maybe_emit_executable_dynamic_exit_sidecar_from_runtime_path(
                base_mint,
                &sidecar_path_sample,
                &sidecar_pool_state,
            );
        }

        let mut positions = self.positions.write();
        if let Some(pos) = positions.get_mut(base_mint) {
            pos.last_shadow_v2_path_sample_age_ms = Some(age_ms);
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
        limitations.extend(shadow_v2_lifecycle_source_order_limitations(record));

        ShadowPathSampleV2::from_legacy_lifecycle_mark(
            envelope,
            shadow_v2_lifecycle_source_order_key(
                record,
                record.sample_slot.or(record.exit_sample_slot),
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
                    | ShadowLifecycleRecordType::PositionUnresolved
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
        envelope
            .limitations
            .extend(shadow_v2_lifecycle_source_order_limitations(record));

        let mut attempt = ShadowExitAttemptV2::from_mark_path_trigger(
            envelope,
            shadow_v2_lifecycle_source_order_key(
                record,
                record.exit_sample_slot.or(record.sample_slot),
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
            self.exit_policy_v1
                .as_ref()
                .map(|policy| (policy.take_profit_fraction() * 100.0).round() as i32),
            self.exit_policy_v1
                .as_ref()
                .map(|policy| -((policy.stop_loss_fraction() * 100.0).round() as i32)),
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
        envelope
            .limitations
            .extend(shadow_v2_lifecycle_source_order_limitations(record));

        let mut sample = PoolStateSampleV2::from_account_state_core(
            envelope,
            shadow_v2_lifecycle_source_order_key(
                record,
                Some(state.last_update_slot),
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
        envelope
            .limitations
            .extend(shadow_v2_lifecycle_source_order_limitations(record));
        let fill_order_key = shadow_v2_lifecycle_source_order_key(
            record,
            record.exit_landed_slot.or(record.exit_sample_slot),
            shadow_v2_event_seq(fill_ts, 4),
            fill_ts,
        );
        if matches!(record.record_type, ShadowLifecycleRecordType::ExitFilled) {
            if let (Some(pool_state), Some(exit_token_amount_raw)) =
                (pool_state_before, record.exit_token_amount_raw)
            {
                envelope
                    .limitations
                    .push("EXIT_FILL_L1_SELL_MODEL_FROM_LIFECYCLE_EXIT_BOUNDARY".to_string());
                envelope.limitations.push(format!(
                    "EXIT_FILL_MODEL_FEE_BPS_ASSUMPTION={SHADOW_V2_EXIT_FEE_BPS_DIAGNOSTIC_MODEL}"
                ));
                envelope.limitations.push(format!(
                    "EXIT_FILL_MODEL_SLIPPAGE_BPS_ASSUMPTION={SHADOW_V2_EXIT_SLIPPAGE_BPS_DIAGNOSTIC_MODEL}"
                ));
                let config = ShadowExitFillModelConfig::bonding_curve(
                    exit_token_amount_raw,
                    SHADOW_V2_EXIT_SLIPPAGE_BPS_DIAGNOSTIC_MODEL,
                    SHADOW_V2_EXIT_FEE_BPS_DIAGNOSTIC_MODEL,
                    SHADOW_V2_EXIT_FILL_MODEL_VERSION,
                );
                return ShadowExitFillV2::from_static_sell_model(
                    envelope,
                    fill_order_key,
                    pool_state,
                    &config,
                );
            }
        }
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
                (Some(pool_slot), Some(fill_slot))
                    if pool_slot == fill_slot
                        && pool_state
                            .event_order_key
                            .same_slot_ambiguous_with(&fill_order_key) =>
                {
                    blockers.push("EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string());
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
        executable_pnl_link: Option<ShadowV2ExecutablePnlLink>,
    ) -> ShadowTerminalTruthV2 {
        let terminal_source = if matches!(
            record.record_type,
            ShadowLifecycleRecordType::PositionUnresolved
        ) {
            "shadow_lifecycle.position_unresolved"
        } else {
            "shadow_lifecycle.position_closed"
        };
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
        envelope.source_refs.push(terminal_source.replace('.', ":"));
        if let Some(comparison_id) = record.het_pm_v2_comparison_id.as_deref() {
            envelope
                .source_refs
                .push(format!("het_pm_v2:comparison_id:{comparison_id}"));
            if let Some(writer_instance_id) = record.het_pm_v2_writer_instance_id.as_deref() {
                envelope
                    .source_refs
                    .push(format!("het_pm_v2:writer_instance_id:{writer_instance_id}"));
            }
            if let Some(snapshot_id) = record.het_pm_v2_source_snapshot_id.as_deref() {
                envelope
                    .source_refs
                    .push(format!("het_pm_v2:source_snapshot_id:{snapshot_id}"));
            }
            if let Some(action_id) = record.action_id.as_deref() {
                envelope
                    .source_refs
                    .push(format!("het_pm_v2:v1_action_id:{action_id}"));
            }
            if let Some(status) = record.het_pm_v2_comparison_write_status.as_deref() {
                envelope
                    .source_refs
                    .push(format!("het_pm_v2:comparison_write_status:{status}"));
            }
            if let Some(reason) = record.het_pm_v2_comparison_skip_reason.as_deref() {
                envelope
                    .source_refs
                    .push(format!("het_pm_v2:comparison_skip_reason:{reason}"));
            }
            if let Some(reason) = record
                .het_pm_v2_comparison_outcome_unknown_reason
                .as_deref()
            {
                envelope.source_refs.push(format!(
                    "het_pm_v2:comparison_outcome_unknown_reason:{reason}"
                ));
            }
        }
        envelope
            .limitations
            .push("TERMINAL_TRUTH_MARK_PATH_ONLY_NOT_EXECUTABLE_FILL".to_string());
        envelope
            .limitations
            .push("TERMINAL_TRUTH_DERIVED_FROM_LEGACY_LIFECYCLE_RECORD".to_string());
        if matches!(
            record.record_type,
            ShadowLifecycleRecordType::PositionUnresolved
        ) {
            envelope
                .limitations
                .push("TERMINAL_OBSERVED_CHAIN_SLOT_UNAVAILABLE_RUNTIME_ONLY_TERMINAL".to_string());
        }

        let (final_pnl_executable_bps, linked_entry_fill, linked_exit_fill, reconciliation_status) =
            if let Some(link) = executable_pnl_link {
                envelope.simulation_level = SimulationLevel::FillModelStatic;
                envelope.measurement_grade = MeasurementGrade::DiagnosticOnly;
                envelope.quality =
                    "TERMINAL_TRUTH_WITH_DIAGNOSTIC_EXECUTABLE_PNL_FROM_CANONICAL_FILLS"
                        .to_string();
                envelope.limitations.push(
                    "TERMINAL_EXECUTABLE_PNL_FROM_CANONICAL_ENTRY_EXIT_FILLED_EVENTS".to_string(),
                );
                envelope
                    .limitations
                    .push("TERMINAL_EXECUTABLE_PNL_DIAGNOSTIC_ONLY_NOT_LIVE_CONFIRMED".to_string());
                (
                    Some(link.final_pnl_executable_bps),
                    Some(link.linked_entry_fill),
                    Some(link.linked_exit_fill),
                    "TERMINAL_TRUTH_WITH_DIAGNOSTIC_EXECUTABLE_PNL".to_string(),
                )
            } else {
                envelope
                    .limitations
                    .push("TERMINAL_EXECUTABLE_PNL_BLOCKED_BY_ENTRY_EXIT_FILL_LINK".to_string());
                envelope
                    .limitations
                    .push("TERMINAL_ENTRY_FILL_LINK_BLOCKED_BY_CANONICAL_FILL_JOIN".to_string());
                envelope
                    .limitations
                    .push("TERMINAL_EXIT_FILL_LINK_BLOCKED_BY_CANONICAL_FILL_JOIN".to_string());
                (
                    None,
                    None,
                    None,
                    "TERMINAL_TRUTH_FROM_LEGACY_LIFECYCLE_MARK_ONLY".to_string(),
                )
            };

        ShadowTerminalTruthV2 {
            envelope,
            event_order_key: shadow_v2_derived_event_order_key(
                record.exit_landed_slot.or(record.exit_sample_slot),
                shadow_v2_event_seq(terminal_ts, 5),
                terminal_ts,
            ),
            terminal_reason: record
                .terminal_reason_v2
                .unwrap_or_else(|| shadow_v2_terminal_reason(record.close_reason)),
            terminal_ts_ms: ClockedTimestamp {
                field_name: "terminal_ts_ms".to_string(),
                value: Some(terminal_ts as i64),
                clock_domain: ClockDomain::StreamObservedMs,
                clock_source: terminal_source.to_string(),
                causal_boundary: "POST_EXIT_TERMINAL_TRUTH".to_string(),
            },
            truth_slot: record.exit_sample_slot.or(record.sample_slot),
            terminal_observed_slot: if matches!(
                record.record_type,
                ShadowLifecycleRecordType::PositionUnresolved
            ) {
                None
            } else {
                record.exit_landed_slot
            },
            terminal_slot: if matches!(
                record.record_type,
                ShadowLifecycleRecordType::PositionUnresolved
            ) {
                None
            } else {
                record.exit_landed_slot.or(record.exit_sample_slot)
            },
            terminal_source: terminal_source.to_string(),
            final_pnl_mark_bps: if matches!(
                record.record_type,
                ShadowLifecycleRecordType::PositionUnresolved
            ) {
                None
            } else {
                shadow_v2_pnl_bps_from_lifecycle(record)
            },
            final_pnl_executable_bps,
            close_age_ms: record.duration_ms,
            linked_entry_fill,
            linked_exit_fill,
            reconciliation_status,
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

    /// Finalizes the independent HET-PM V2 writer-health denominator without
    /// putting filesystem I/O back on an authority tick or terminal path.
    /// A timeout leaves `shutdown_complete=false`, forcing offline coverage to
    /// degrade to unknown instead of claiming complete evidence.
    pub async fn flush_het_pm_v2_writer_health_for_shutdown(&self) {
        let Some(writer) = self.het_pm_v2_observation_writer.as_ref() else {
            return;
        };
        let Some(path) = writer.health_path.clone() else {
            return;
        };
        let budget = Duration::from_millis(HET_PM_V2_WRITER_HEALTH_SHUTDOWN_BUDGET_MS);
        let deadline = tokio::time::Instant::now() + budget;
        while !writer.counters_are_quiescent() {
            if tokio::time::Instant::now() >= deadline {
                warn!(
                    path = %path.display(),
                    budget_ms = HET_PM_V2_WRITER_HEALTH_SHUTDOWN_BUDGET_MS,
                    "PostBuyGuardian: HET-PM V2 writer-health shutdown drain timed out; coverage remains unknown"
                );
                writer.notify_health();
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        writer
            .health_context
            .shutdown_complete
            .store(true, Ordering::Release);
        writer.health_context.mark_changed();
        let context = Arc::clone(&writer.health_context);
        let write_lock = Arc::clone(&writer.health_write_lock);
        let write = tokio::task::spawn_blocking(move || {
            HetPmV2ObservationWriterV1::persist_health_snapshot(&path, &context, &write_lock)
        });
        match tokio::time::timeout(budget, write).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => warn!(
                error = %error,
                "PostBuyGuardian: final HET-PM V2 writer-health snapshot failed; coverage remains unknown"
            ),
            Ok(Err(error)) => warn!(
                error = %error,
                "PostBuyGuardian: final HET-PM V2 writer-health task failed; coverage remains unknown"
            ),
            Err(_) => warn!(
                budget_ms = HET_PM_V2_WRITER_HEALTH_SHUTDOWN_BUDGET_MS,
                "PostBuyGuardian: final HET-PM V2 writer-health snapshot timed out; coverage remains unknown"
            ),
        }
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
        let has_executable_quote_contract = matches!(
            record_type,
            ShadowLifecycleRecordType::ExitFilled
                | ShadowLifecycleRecordType::ExitBlocked
                | ShadowLifecycleRecordType::PositionClosed
                | ShadowLifecycleRecordType::PositionUnresolved
        );
        // A synthetic landing slot models only the simulated execution
        // boundary. Diagnostic, blocked, unresolved and observation records
        // have evidence/sample provenance but never represent a landed exit.
        let exit_landed_slot = matches!(
            record_type,
            ShadowLifecycleRecordType::ExitFilled | ShadowLifecycleRecordType::PositionClosed
        )
        .then(|| synthetic_next_slot(evidence.slot))
        .flatten();
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
            policy_id: self
                .exit_policy_v1
                .as_ref()
                .map(|policy| policy.policy_id().to_string()),
            policy_version: self
                .exit_policy_v1
                .as_ref()
                .map(EffectiveExitPolicyV1Config::policy_version),
            policy_config_hash: self
                .exit_policy_v1
                .as_ref()
                .map(|policy| policy.config_hash().to_string()),
            source_snapshot_id: pos
                .pending_exit_proposal
                .as_ref()
                .map(|proposal| proposal.source_snapshot_id.clone())
                .or_else(|| pos.last_source_snapshot_id.clone()),
            action_id: pos
                .pending_exit_proposal
                .as_ref()
                .map(|proposal| proposal.action_id.clone())
                .or_else(|| pos.last_applied_action_id.clone()),
            het_pm_v2_comparison_id: None,
            het_pm_v2_writer_instance_id: None,
            het_pm_v2_source_snapshot_id: None,
            het_pm_v2_comparison_write_status: None,
            het_pm_v2_comparison_skip_reason: None,
            het_pm_v2_comparison_outcome_unknown_reason: None,
            terminal_reason_v2: None,
            exit_policy_reason_code: pos.last_force_exit_reason_code.clone(),
            terminal_disposition: None,
            remaining_token_amount_raw: Some(pos.remaining_token_amount_raw),
            entry_token_amount_raw: Some(
                pos.last_resolved_exit_metrics
                    .as_ref()
                    .map_or(pos.entry_token_amount_raw, |metrics| {
                        metrics.entry_token_amount_raw
                    }),
            ),
            recovery_elapsed_ms: None,
            executable_quote_grade: has_executable_quote_contract
                .then(|| EXECUTABLE_QUOTE_GRADE.to_string()),
            execution_cost_coverage: has_executable_quote_contract
                .then(|| EXECUTION_COST_COVERAGE_UNMODELED.to_string()),
            net_pnl_authoritative: has_executable_quote_contract.then_some(false),
            mark_return_pct: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.mark_return_pct),
            executable_gross_return_pct: pos
                .last_resolved_exit_metrics
                .as_ref()
                .map(|metrics| metrics.executable_gross_return_pct),
            mfe_mark_pct: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.mfe_mark_pct),
            mae_mark_pct: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.mae_mark_pct),
            quote_reserve_base_raw: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.quote_reserve_base_raw),
            quote_reserve_quote_sol: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.quote_reserve_quote_sol),
            quote_own_impact_bps: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.quote_own_impact_bps),
            decision_mark_source: Some(
                pos.last_resolved_exit_metrics
                    .as_ref()
                    .map_or(pos.last_snapshot_source, |metrics| {
                        metrics.decision_mark_source
                    }),
            ),
            decision_mark_slot: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.decision_mark_slot)
                .or_else(|| {
                    pos.last_shadow_snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.slot)
                }),
            decision_mark_timestamp_ms: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.decision_mark_timestamp_ms)
                .or_else(|| {
                    pos.last_shadow_snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.timestamp_ms)
                }),
            decision_mark_age_ms: pos
                .last_resolved_exit_metrics
                .as_ref()
                .and_then(|metrics| metrics.decision_mark_age_ms)
                .or_else(|| {
                    pos.last_shadow_snapshot
                        .as_ref()
                        .map(|snapshot| now_ms.saturating_sub(snapshot.timestamp_ms))
                }),
            peak_drawdown_pct: pos.last_shadow_snapshot.as_ref().and_then(|snapshot| {
                let mark = PriceTruthResolver::normalize_shadow_snapshot_price_sol(snapshot)?;
                (pos.peak_since_entry.is_finite() && pos.peak_since_entry > 0.0).then_some(
                    ((pos.peak_since_entry - mark) / pos.peak_since_entry).max(0.0) * 100.0,
                )
            }),
            absolute_age_ms: Some(now_ms.saturating_sub(pos.entry_unix_ms)),
            inactivity_age_ms: Some(now_ms.saturating_sub(pos.shadow_market_activity.last_seen_ms)),
            capacity_occupancy_age_ms: Some(now_ms.saturating_sub(pos.entry_unix_ms)),
            would_hold_under_legacy_inactivity_policy: pos
                .pending_exit_proposal
                .as_ref()
                .and_then(|proposal| proposal.would_hold_under_legacy_inactivity_policy)
                .or(pos.last_would_hold_under_legacy_inactivity_policy),
            crash_guard_mode: None,
            crash_guard_state: None,
            crash_guard_not_triggered_reason: None,
            crash_guard_quote_rejection_reason: None,
            crash_guard_consumed_by_policy: None,
            authoritative_decision: None,
            crash_guard_candidate_decision: None,
            crash_short_window_drop_pct: None,
            crash_peak_drawdown_pct: None,
            crash_distinct_slots: None,
            crash_oldest_sample_slot: None,
            crash_previous_distinct_slot: None,
            crash_latest_sample_slot: None,
            crash_latest_sample_timestamp_ms: None,
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
            exit_token_amount_raw: pos
                .last_resolved_exit_metrics
                .as_ref()
                .map(|metrics| metrics.exit_token_amount_raw),
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
            source_block_time: None,
            source_tx_signature: None,
            source_transaction_index: None,
            source_instruction_index: None,
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

    fn emit_position_closed(
        &self,
        pos: &MonitoredPosition,
        duration_ms: u64,
        now_ms: u64,
    ) -> Option<ShadowLifecycleRecord> {
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
            let mut env = emitter.make_envelope_at(&pos.candidate_id, now_ms);
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
                    timestamp_ms: Some(now_ms),
                    age_ms: None,
                    price_state: None,
                    price_reason: None,
                });
            let mut record = self.shadow_lifecycle_record_base(
                pos,
                ShadowLifecycleRecordType::PositionClosed,
                now_ms,
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
            return Some(record);
        }
        None
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
    // Existing active and compatibility callers pass separately derived entry
    // facts. Keep this stable public boundary rather than silently rebuilding
    // those facts from a competing mutable source.
    #[expect(
        clippy::too_many_arguments,
        reason = "the shadow handoff carries immutable entry facts and its exact terminal receiver"
    )]
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
        self.register_position_with_context_internal(
            pool_amm_id,
            base_mint,
            bonding_curve,
            entry_price_sol,
            entry_amount_lamports,
            entry_token_amount_raw,
            context,
            None,
        )
    }

    /// Register an active shadow position together with the exact terminal
    /// notification channel consumed by `PostBuyRuntime`.
    // The handoff carries explicit, immutable entry facts plus the exact
    // terminal receiver; grouping them would blur the lifecycle contract.
    #[expect(
        clippy::too_many_arguments,
        reason = "runtime and lifecycle emitters require the full explicit identity and truth contract"
    )]
    pub fn register_shadow_position_with_terminal(
        &self,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        entry_price_sol: Option<f64>,
        entry_amount_lamports: Option<u64>,
        entry_token_amount_raw: Option<u64>,
        context: PositionEventContext,
    ) -> Option<RegisteredShadowPosition> {
        if !matches!(context.lane, Lane::Shadow) {
            return None;
        }
        let (terminal_tx, terminal_rx) = oneshot::channel();
        let registration = self.register_position_with_context_internal(
            pool_amm_id,
            base_mint,
            bonding_curve,
            entry_price_sol,
            entry_amount_lamports,
            entry_token_amount_raw,
            Some(context),
            Some(terminal_tx),
        )?;
        Some(RegisteredShadowPosition {
            registration,
            terminal_rx,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn register_position_with_context_internal(
        &self,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        entry_price_sol: Option<f64>,
        entry_amount_lamports: Option<u64>,
        entry_token_amount_raw: Option<u64>,
        context: Option<PositionEventContext>,
        terminal_tx: Option<oneshot::Sender<ShadowTerminalDisposition>>,
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
        let executable_dynamic_exit_policies = self
            .executable_dynamic_exit_sidecar_settings()
            .map(|(_, policies)| policies);
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
        let requested_rug_profile = event_context.join_metadata.exit_profile_id.as_deref()
            == Some(RUG_SCALP_EXIT_PROFILE_ID);
        let requested_rug_strategy =
            event_context.join_metadata.strategy_id.as_deref() == Some(RUG_SCALP_V2_STRATEGY_ID);
        // `rug_scalp_exit_v1` belongs exclusively to the isolated V2 adapter.
        // Reject either half of the pair rather than accepting a profile that
        // a different strategy could use to acquire PM lifecycle authority.
        if requested_rug_profile != requested_rug_strategy {
            warn!(
                position_id = %position_id,
                base_mint = %base_mint,
                strategy_id = ?event_context.join_metadata.strategy_id,
                exit_profile_id = ?event_context.join_metadata.exit_profile_id,
                "PostBuyGuardian: refused incomplete RUG SCALP strategy/profile join"
            );
            return None;
        }
        let is_rug_scalp_position = requested_rug_profile;
        if is_rug_scalp_position && !self.config.rug_scalp_exit_v1.enabled {
            warn!(
                position_id = %position_id,
                base_mint = %base_mint,
                "PostBuyGuardian: refused RUG SCALP position because rug_scalp_exit_v1 is disabled"
            );
            return None;
        }
        if matches!(event_context.lane, Lane::Shadow) {
            let valid_entry_price =
                entry_price_sol.is_some_and(|price| price.is_finite() && price > 0.0);
            let valid_quantity = entry_token_amount_raw.is_some_and(|quantity| quantity > 0);
            let valid_identity = !event_context.candidate_id.trim().is_empty()
                && !position_id.trim().is_empty()
                && position_epoch > 0;
            if !valid_identity || !valid_entry_price || !valid_quantity {
                warn!(
                    candidate_id = %event_context.candidate_id,
                    %position_id,
                    position_epoch,
                    entry_price_sol = ?entry_price_sol,
                    entry_token_amount_raw = ?entry_token_amount_raw,
                    "PostBuyGuardian: rejected invalid immutable shadow position contract"
                );
                return None;
            }
        }
        let shadow_market_activity = ShadowMarketActivityAnchor::from_registration(
            opened_at_ms,
            initial_shadow_snapshot.as_ref(),
        );
        let time_stop_v2 = TimeStopV2State::from_registration(initial_shadow_snapshot.as_ref());
        let initial_peak_since_entry = initial_shadow_snapshot
            .as_ref()
            .and_then(PriceTruthResolver::normalize_shadow_snapshot_price_sol)
            .filter(|price| price.is_finite() && *price > 0.0)
            .map(|price| price.max(entry_price_sol.unwrap_or(0.0)))
            .unwrap_or_else(|| entry_price_sol.unwrap_or(0.0));
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
            state_revision: 1,
            next_exit_action_seq: 1,
            pending_exit_proposal: None,
            pending_terminal_commit: None,
            terminal_tx,
            last_applied_action_id: None,
            last_source_snapshot_id: None,
            last_resolved_exit_metrics: None,
            last_shadow_outcome: None,
            join_metadata: event_context.join_metadata.clone(),
            entry_order_id: event_context.entry_order_id.clone(),
            quote_id: event_context.quote_id.clone(),
            slot: event_context.slot,
            peak_since_entry: initial_peak_since_entry,
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
            estimated_costs_sol: if is_rug_scalp_position {
                (self.config.rug_scalp_exit_v1.entry_fixed_cost_lamports
                    + self.config.rug_scalp_exit_v1.exit_fixed_cost_lamports) as f64
                    / SHADOW_LAMPORTS_PER_SOL_F64
            } else {
                0.0
            },
            realized_pnl_sol: 0.0,
            realized_pnl_pct: 0.0,
            total_exits: 0,
            remaining_fraction_bps: 10_000,
            last_close_reason: None,
            last_force_exit_reason_code: None,
            last_would_hold_under_legacy_inactivity_policy: None,
            last_price_truth: None,
            last_blocked_truth_status: None,
            last_blocked_truth_timestamp_ms: None,
            last_snapshot_source: self.default_snapshot_source(),
            last_shadow_snapshot: initial_shadow_snapshot,
            last_shadow_v2_path_sample_age_ms: None,
            last_crash_guard_observation: None,
            last_crash_guard_candidate_revision: None,
            pending_crash_guard_observation: None,
            executable_dynamic_exit_evaluator: executable_dynamic_exit_policies.map(|policies| {
                Self::executable_dynamic_exit_evaluator_for_position(
                    &position_id,
                    &event_context.entry_order_id,
                    entry_token_amount_raw.unwrap_or(0),
                    entry_amount_lamports.unwrap_or(0),
                    entry_price_sol,
                    policies,
                )
            }),
            shadow_market_activity,
            time_stop_v2,
            snapshot_timeline,
            // Absence of route truth is not evidence of PumpCurve support.
            // Canonical AccountStateCore refresh is the only producer allowed
            // to promote this value out of Unknown.
            het_route_status: RouteStatusV1::Unknown,
            het_executable_peak_anchor: None,
            het_next_anchor_seq: 1,
            last_het_pm_v2_comparison_id: None,
            last_het_pm_v2_candidate_gate: None,
            last_het_pm_v2_candidate_at_ms: None,
            rug_scalp_facts: is_rug_scalp_position.then(|| {
                let watermark = event_context
                    .join_metadata
                    .rug_scalp_entry_watermark_slot
                    .map(|slot| RugScalpEntryWatermarkV1 {
                        slot,
                        tx_index: event_context
                            .join_metadata
                            .rug_scalp_entry_watermark_tx_index,
                        event_ordinal: event_context
                            .join_metadata
                            .rug_scalp_entry_watermark_event_ordinal,
                    });
                watermark.map_or_else(
                    || RugScalpMarketFactStateV1::new(position_id.clone(), base_mint),
                    |watermark| {
                        RugScalpMarketFactStateV1::with_entry_watermark(
                            position_id.clone(),
                            base_mint,
                            watermark,
                        )
                    },
                )
            }),
            rug_scalp_pending_exit: None,
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

    /// Administrative removal for shutdown/tests. It deliberately emits no
    /// economic terminal event and therefore cannot masquerade as a fill.
    pub fn remove_position_administratively(&self, base_mint: &Pubkey) {
        let pos = {
            let mut positions = self.positions.write();
            positions.remove(base_mint)
        };
        if let Some(pos) = pos {
            let now_ms = current_time_ms();
            let replay_status = if !self.config.exit_replay_v1.enabled
                || self.shadow_exit_replay_log_path.is_none()
            {
                "disabled"
            } else if self.exit_replay_trackers.read().contains_key(base_mint) {
                "active_at_censor"
            } else {
                "flushed_or_not_active"
            };
            self.append_position_censored_record(&ShadowPositionCensoredRecord {
                schema_version: POSITION_CENSORED_SCHEMA_VERSION,
                artifact_type: POSITION_CENSORED_ARTIFACT_TYPE,
                run_id: pos.join_metadata.run_id.clone(),
                position_id: pos.position_id.clone(),
                position_epoch: pos.position_epoch,
                candidate_id: pos.candidate_id.clone(),
                pool_id: pos.pool_amm_id.to_string(),
                base_mint: pos.base_mint.to_string(),
                lane: pos.lane,
                age_ms: now_ms.saturating_sub(pos.entry_unix_ms),
                reason: "controlled_runtime_horizon",
                had_v2_candidate: pos.last_het_pm_v2_candidate_gate.is_some(),
                candidate_gate: pos.last_het_pm_v2_candidate_gate.clone(),
                comparison_id: pos.last_het_pm_v2_comparison_id.clone(),
                replay_status,
                timestamp_ms: now_ms,
            });
            if let Some(ref runtime) = self.aem_runtime {
                let mut rt = runtime.lock();
                let _ = rt.unregister_position(&pos.position_id);
            }
            let duration_ms = now_ms.saturating_sub(pos.entry_unix_ms);
            info!(
                "🛡️ PostBuyGuardian: Stopped monitoring mint={} (held {:.1}s, signals={})",
                base_mint,
                duration_ms as f64 / 1000.0,
                pos.recent_signals.len()
            );
        }
    }

    /// Drain every observer-owned position during controlled process shutdown.
    ///
    /// This is deliberately not an economic terminal path: no proposal, fill,
    /// PnL, lifecycle terminal, or capacity decision is fabricated. Dropping
    /// the per-position terminal sender unblocks the launcher's shutdown-only
    /// watcher after the authority loop has already been stopped.
    pub fn remove_all_positions_administratively(&self) -> usize {
        let active_mints = self.active_mints();
        let removed = active_mints.len();
        for base_mint in active_mints {
            self.remove_position_administratively(&base_mint);
        }
        removed
    }

    /// Returns the number of currently monitored positions.
    pub fn active_position_count(&self) -> usize {
        self.positions.read().len()
    }

    /// Applies one canonical, typed RUG market fact to the matching active
    /// RUG position.  The PM receives no raw trade object and therefore
    /// cannot become a second ingest or signal authority.
    pub fn observe_rug_scalp_market_fact(
        &self,
        fact: RugScalpMarketFactV1,
    ) -> RugScalpFactIngressResultV1 {
        let mut positions = self.positions.write();
        let Some(position) = positions.get_mut(&fact.mint) else {
            return RugScalpFactIngressResultV1::RejectedUnknownPosition;
        };
        let Some(facts) = position.rug_scalp_facts.as_mut() else {
            return RugScalpFactIngressResultV1::RejectedProfileMismatch;
        };
        facts.apply_fact(fact, &self.config.rug_scalp_exit_v1)
    }

    /// Lightweight lifecycle query for launcher-owned background helpers.
    /// It performs no I/O and does not expose mutable position state.
    pub fn active_position_contains(&self, base_mint: &Pubkey) -> bool {
        self.positions.read().contains_key(base_mint)
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
                        self.maybe_emit_shadow_v2_runtime_path_sample(base_mint, now_ms);
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

            // ── Position Manager Lite V1 policy ────────────────────
            let runtime_snapshot = self.current_runtime_shadow_snapshot(base_mint, now_ms);
            let runtime_snapshot = runtime_snapshot.as_ref().unwrap_or(latest);
            self.observe_exit_replay_snapshot(base_mint, runtime_snapshot);
            self.maybe_emit_shadow_v2_runtime_path_sample(base_mint, now_ms);
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
                    {
                        let mut positions = self.positions.write();
                        if let Some(pos) = positions.get_mut(&mint) {
                            pos.runtime_registered = false;
                        }
                    }
                    let repaired = self.ensure_shadow_runtime_registered(&mint).await;
                    warn!(
                        position_id = %position_id,
                        mint = %mint,
                        consumed_by_policy = false,
                        mirror_repaired = repaired,
                        "PostBuyGuardian: non-authoritative shadow mirror was missing; canonical position retained"
                    );
                    continue;
                }
                self.remove_position_administratively(&mint);
                info!(
                    "🛡️ PostBuyGuardian: Administratively removed lane={} mint={} (no longer in managed runtime)",
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

    fn resolve_shadow_exit_truth_for_policy(
        &self,
        snapshot: &PostBuyDecisionSnapshot,
        latest_snapshot: Option<&MarketSnapshot>,
        expected_quantity: u64,
        now_ms: u64,
        evidence_source: PriceTruthSource,
    ) -> Result<ShadowExitTruth, ExecutableQuoteFailure> {
        latest_snapshot
            .ok_or_else(|| ExecutableQuoteFailure {
                kind: ExecutableQuoteFailureKind::MissingSnapshot,
                evidence: PriceTruthEvidence {
                    source: evidence_source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "no canonical snapshot available for shadow executable quote".to_string(),
                    ),
                    slot: snapshot.guard().latest_sample_slot(),
                    timestamp_ms: snapshot.guard().latest_sample_timestamp_ms(),
                    age_ms: None,
                    price_state: None,
                    price_reason: None,
                },
            })
            .and_then(|latest_snapshot| {
                Self::resolve_shadow_exit_sample_for_runtime(
                    latest_snapshot,
                    now_ms,
                    self.shadow_exit_stale_after_ms(),
                    evidence_source,
                )
                .map_err(ExecutableQuoteFailure::from_price_truth_error)
            })
            .and_then(|sample| {
                let entry_price =
                    snapshot
                        .entry_price_sol()
                        .ok_or_else(|| ExecutableQuoteFailure {
                            kind: ExecutableQuoteFailureKind::InternalFailure,
                            evidence: PriceTruthEvidence {
                                source: sample.evidence.source,
                                status: PriceTruthStatus::SemanticViolation,
                                detail: Some(
                                    "shadow entry price missing at quote boundary".to_string(),
                                ),
                                slot: sample.evidence.slot,
                                timestamp_ms: sample.evidence.timestamp_ms,
                                age_ms: sample.evidence.age_ms,
                                price_state: sample.evidence.price_state,
                                price_reason: sample.evidence.price_reason,
                            },
                        })?;
                PriceTruthResolver::resolve_shadow_exit(
                    entry_price,
                    expected_quantity,
                    &sample,
                    0.0,
                )
                .map_err(ExecutableQuoteFailure::from_price_truth_error)
            })
    }

    fn resolve_shadow_exit_truth_for_anchor(
        &self,
        snapshot: &PostBuyDecisionSnapshot,
        anchor_snapshot: &MarketSnapshot,
        expected_quantity: u64,
        now_ms: u64,
        evidence_source: PriceTruthSource,
    ) -> Result<ShadowExitTruth, ExecutableQuoteFailure> {
        Self::resolve_shadow_exit_sample_for_runtime(anchor_snapshot, now_ms, 0, evidence_source)
            .map_err(ExecutableQuoteFailure::from_price_truth_error)
            .and_then(|sample| {
                let entry_price =
                    snapshot
                        .entry_price_sol()
                        .ok_or_else(|| ExecutableQuoteFailure {
                            kind: ExecutableQuoteFailureKind::InternalFailure,
                            evidence: PriceTruthEvidence {
                                source: sample.evidence.source,
                                status: PriceTruthStatus::SemanticViolation,
                                detail: Some(
                                    "shadow entry price missing at anchor quote boundary"
                                        .to_string(),
                                ),
                                slot: sample.evidence.slot,
                                timestamp_ms: sample.evidence.timestamp_ms,
                                age_ms: sample.evidence.age_ms,
                                price_state: sample.evidence.price_state,
                                price_reason: sample.evidence.price_reason,
                            },
                        })?;
                PriceTruthResolver::resolve_shadow_exit(
                    entry_price,
                    expected_quantity,
                    &sample,
                    0.0,
                )
                .map_err(ExecutableQuoteFailure::from_price_truth_error)
            })
    }

    fn prepare_het_pm_v2_tick(&self, input: HetPmV2TickInput<'_>) -> PreparedHetPmV2Tick {
        let HetPmV2TickInput {
            bundle,
            latest_snapshot,
            raw_canonical_snapshot,
            trajectory_peak_snapshot,
            v1_prequote,
            crash_prequote,
            v1_policy,
            het_policy,
            now_ms,
        } = input;
        let view = bundle.view();
        let absolute_max_hold_due = v1_policy.absolute_max_hold_enabled()
            && bundle.base.absolute_age_ms() >= v1_policy.absolute_max_hold_ms();
        let v2_prequote = ExitPolicyV2::evaluate_prequote_with_absolute_max_hold(
            view,
            v1_prequote,
            crash_prequote,
            het_policy,
            absolute_max_hold_due,
        );
        let mut anchor_request = ExitPolicyV2::evaluate_anchor_request(view, now_ms, het_policy);
        let v2_gate_prequotes = ExitPolicyV2::evaluate_gate_lattice_with_absolute_max_hold(
            view,
            v1_prequote,
            crash_prequote,
            het_policy,
            absolute_max_hold_due,
        );
        let trailing_blocked_for_missing_anchor = v2_gate_prequotes.iter().any(|gate| {
            gate.reason == HetPmExitReasonV2::ExecutableTrailing
                && matches!(
                    gate.candidate,
                    HetPmCandidateV2::Blocked(HetPmUnknownReasonV2::AnchorUnavailable)
                )
        });
        if matches!(anchor_request, PeakAnchorPreQuoteDecisionV1::NoChange)
            && trailing_blocked_for_missing_anchor
        {
            if let Some(backfill_request) = trajectory_peak_snapshot
                .and_then(|source| het_pm_v2_historical_peak_anchor_request(view, source))
            {
                anchor_request = backfill_request;
            }
        }

        let mut quote_plan = HetPmV2QuotePlan::default();

        let baseline_quote_required = matches!(v1_prequote, PreQuoteDecision::QuoteRequired { .. });
        let crash_quote_required = matches!(
            crash_prequote,
            CrashGuardPreQuoteDecision::QuoteRequired { .. }
        );
        if baseline_quote_required || crash_quote_required {
            let crash_is_authoritative = crash_quote_required
                && matches!(
                    v1_policy.crash_guard_mode(),
                    CrashGuardMode::AuthoritativeShadow
                );
            let source = if crash_is_authoritative || !baseline_quote_required {
                raw_canonical_snapshot.or(latest_snapshot)
            } else {
                latest_snapshot.or(raw_canonical_snapshot)
            };
            if let Some(source) = source {
                let _ = quote_plan.add(ExecutableQuoteKeyV2::from_view(view), source);
            }
        }

        for gate_prequote in &v2_gate_prequotes {
            if let HetPmCandidateV2::QuoteRequired(reason) = &gate_prequote.candidate {
                if let Some(source) = het_pm_v2_quote_source_for_reason(
                    *reason,
                    latest_snapshot,
                    raw_canonical_snapshot,
                ) {
                    let _ = quote_plan.add(ExecutableQuoteKeyV2::from_view(view), source);
                }
            }
        }
        let planned_cells = quote_plan.into_cells();
        let mut quote_cells = Vec::with_capacity(planned_cells.len());
        let mut quote_keys = Vec::with_capacity(planned_cells.len());
        let mut quote_statuses = Vec::with_capacity(planned_cells.len());
        for (key, source) in planned_cells {
            quote_keys.push(key.stable_label());
            let evidence_source = bundle.base.mark_source();
            let outcome = self.resolve_shadow_exit_truth_for_policy(
                &bundle.base,
                Some(&source),
                key.remaining_quantity_raw,
                now_ms,
                evidence_source,
            );
            match &outcome {
                Ok(_) => quote_statuses.push("resolved".to_string()),
                Err(failure) => quote_statuses.push(format!("blocked:{:?}", failure.kind)),
            }
            quote_cells.push(HetPmV2QuoteCell { key, outcome });
        }
        let anchor_quote_cell = if let PeakAnchorPreQuoteDecisionV1::QuoteRequired { key, .. } =
            &anchor_request
        {
            if let Some(existing_cell) = quote_cells.iter().find(|cell| &cell.key == key) {
                Some(existing_cell.clone())
            } else if quote_cells.len() < HET_PM_V2_MAX_QUOTE_CELLS {
                het_pm_v2_anchor_source_for_key(
                    key,
                    latest_snapshot,
                    raw_canonical_snapshot,
                    trajectory_peak_snapshot,
                )
                .map(|source| {
                    let outcome = self.resolve_shadow_exit_truth_for_anchor(
                        &bundle.base,
                        source,
                        key.remaining_quantity_raw,
                        now_ms,
                        bundle.base.mark_source(),
                    );
                    quote_keys.push(key.stable_label());
                    match &outcome {
                        Ok(_) => quote_statuses.push("resolved".to_string()),
                        Err(failure) => quote_statuses.push(format!("blocked:{:?}", failure.kind)),
                    }
                    HetPmV2QuoteCell {
                        key: key.clone(),
                        outcome,
                    }
                })
            } else {
                None
            }
        } else {
            None
        };

        let v2_crash_requirement = matches!(
            &v2_prequote.candidate,
            HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::Crash)
        )
        .then(|| ExitPolicyV1::crash_guard_quote_requirement(&bundle.base))
        .flatten();
        let v2_resolved_quote = het_pm_v2_resolved_quote_for_candidate(
            view,
            &v2_prequote.candidate,
            latest_snapshot,
            raw_canonical_snapshot,
            &quote_cells,
        );
        let v2_final = ExitPolicyV2::finalize_with_quote(
            view,
            &v2_prequote,
            v2_resolved_quote.input(v2_crash_requirement.as_ref()),
            v1_policy,
            het_policy,
        );
        let v2_crash_quote_decision = match &v2_final {
            HetPmFinalDecisionV2::ExitAll {
                reason: HetPmExitReasonV2::Crash,
                ..
            } => Some(CrashGuardQuoteDecision::Confirmed),
            HetPmFinalDecisionV2::CrashRejectedByQuote { reason } => {
                Some(CrashGuardQuoteDecision::RejectedByQuote { reason: *reason })
            }
            HetPmFinalDecisionV2::CrashBlockedByData => {
                Some(CrashGuardQuoteDecision::BlockedByData)
            }
            _ => None,
        };
        let v2_gate_evaluations: Vec<HetPmGateEvaluationV2> = v2_gate_prequotes
            .into_iter()
            .map(|gate_prequote| {
                let prequote = HetPmPreQuoteEvaluationV2 {
                    candidate: gate_prequote.candidate.clone(),
                    winning_gate: match gate_prequote.reason {
                        HetPmExitReasonV2::Crash => HetPmGateV2::Crash,
                        HetPmExitReasonV2::HardLoss => HetPmGateV2::HardLoss,
                        HetPmExitReasonV2::ExecutableTrailing => HetPmGateV2::ExecutableTrailing,
                        HetPmExitReasonV2::VitalityDecay => HetPmGateV2::VitalityDecay,
                        HetPmExitReasonV2::AbsoluteMaxHold => HetPmGateV2::AbsoluteMaxHold,
                    },
                    suppressed_gates_mask: 0,
                };
                let crash_requirement = matches!(
                    &gate_prequote.candidate,
                    HetPmCandidateV2::QuoteRequired(HetPmExitReasonV2::Crash)
                )
                .then(|| ExitPolicyV1::crash_guard_quote_requirement(&bundle.base))
                .flatten();
                let resolved_quote = het_pm_v2_resolved_quote_for_candidate(
                    view,
                    &gate_prequote.candidate,
                    latest_snapshot,
                    raw_canonical_snapshot,
                    &quote_cells,
                );
                let final_decision = ExitPolicyV2::finalize_with_quote(
                    view,
                    &prequote,
                    resolved_quote.input(crash_requirement.as_ref()),
                    v1_policy,
                    het_policy,
                );
                let executable_gross_return_bps = match final_decision {
                    HetPmFinalDecisionV2::ExitAll {
                        executable_gross_return_bps,
                        ..
                    } => Some(executable_gross_return_bps),
                    _ => None,
                };
                HetPmGateEvaluationV2 {
                    gate: gate_prequote.reason,
                    prequote: gate_prequote.candidate.clone(),
                    quote_status: ExitPolicyV2::gate_quote_status(
                        &gate_prequote.candidate,
                        &final_decision,
                    ),
                    final_decision,
                    executable_gross_return_bps,
                }
            })
            .collect();
        let authority_candidate = het_policy
            .authoritative_shadow()
            .then(|| {
                ExitPolicyV2::select_authoritative_exit(
                    &v2_gate_evaluations,
                    v1_policy.crash_guard_mode(),
                )
            })
            .flatten()
            .map(|reason| ExitCandidate::from_reason(reason.exit_candidate_reason()));
        // Preserve the precise V2 candidate passed into the shared guarded
        // executor. The top-level V2 diagnostic winner is intentionally pure
        // hierarchy and may be a higher typed blocker; it is not sufficient to
        // attribute a lower hard ceiling such as AbsoluteMaxHold after apply.
        // A pre-existing V1 proposal remains V1-owned through completion.
        let v2_selected_execution_reason = (het_policy.authoritative_shadow()
            && !bundle.base.has_pending_proposal())
        .then(|| {
            authority_candidate
                .as_ref()
                .map(|candidate| candidate.reason().as_label().to_string())
        })
        .flatten();
        let current_executable_truth = v2_resolved_quote
            .cell
            .and_then(|cell| cell.outcome.as_ref().ok());
        let current_executable_value_sol =
            current_executable_truth.map(|truth| truth.exit_value_sol);
        let current_executable_gross_return_bps = current_executable_truth.map(|truth| {
            (truth.pnl_pct * 100.0)
                .round()
                .clamp(i32::MIN as f64, i32::MAX as f64) as i32
        });
        let known_estimated_costs_sol =
            current_executable_truth.map(|truth| truth.estimated_costs_sol);

        let anchor_request_label = match &anchor_request {
            PeakAnchorPreQuoteDecisionV1::NoChange => None,
            PeakAnchorPreQuoteDecisionV1::QuoteRequired { key, .. } => Some(
                if key.sample_slot == bundle.v2.trajectory.newest_sample_slot
                    && key.sample_timestamp_ms == bundle.v2.trajectory.newest_sample_timestamp_ms
                {
                    "quote_required_on_new_canonical_peak"
                } else {
                    "quote_required_on_historical_peak_backfill"
                }
                .to_string(),
            ),
            PeakAnchorPreQuoteDecisionV1::Blocked { reason } => Some(format!("blocked:{reason:?}")),
        };

        let comparison_identity_material = format!(
            "{}:{}:{}:{}:{}",
            bundle.base.guard().position_id(),
            bundle.base.guard().position_epoch(),
            bundle.base.guard().state_revision(),
            bundle.base.snapshot_id(),
            now_ms
        );
        let comparison_id = format!(
            "het_pm_v2:{}",
            blake3::hash(comparison_identity_material.as_bytes()).to_hex()
        );
        let writer_instance_id = self.het_pm_v2_writer_instance_id_for_record();
        let record = V1V2ComparisonRecord {
            schema_version: HET_PM_V2_SCHEMA_VERSION,
            comparison_id,
            policy_id: HET_PM_V2_POLICY_ID.to_string(),
            policy_version: HET_PM_V2_POLICY_VERSION,
            policy_config_hash: het_policy.config_hash().to_string(),
            v1_policy_id: v1_policy.policy_id().to_string(),
            v1_policy_version: v1_policy.policy_version(),
            v1_policy_config_hash: v1_policy.config_hash().to_string(),
            time_stop_v2_config_hash: self.time_stop_v2_config_hash.clone(),
            run_id: bundle.v2.run_id.clone(),
            writer_instance_id,
            lane: bundle.base.lane(),
            position_id: bundle.base.guard().position_id().to_string(),
            position_epoch: bundle.base.guard().position_epoch(),
            state_revision: bundle.base.guard().state_revision(),
            remaining_quantity_raw: bundle.base.remaining_token_amount_raw(),
            snapshot_id: bundle.base.snapshot_id().to_string(),
            observation_timestamp_ms: now_ms,
            terminal_tick: false,
            trajectory_sampling_mode: HET_PM_V2_SAMPLING_MODE.to_string(),
            trajectory_measurement_grade: HET_PM_V2_TRAJECTORY_GRADE.to_string(),
            monitor_tick_ms: self.config.tick_interval_ms,
            v1_prequote: prequote_label(v1_prequote),
            v1_crash_prequote: format!("{crash_prequote:?}"),
            v1_final: None,
            v1_authority_receipt: None,
            v2_prequote: format!("{:?}", v2_prequote.candidate),
            v2_final: Some(format!("{v2_final:?}")),
            v2_selected_execution_reason,
            v2_crash_quote_decision,
            v2_winning_gate: v2_prequote.winning_gate,
            v2_suppressed_gates_mask: v2_prequote.suppressed_gates_mask,
            v2_gate_evaluations,
            consumed_by_policy: het_policy.authoritative_shadow(),
            v1_shadow_authority: !het_policy.authoritative_shadow(),
            v2_shadow_authority: het_policy.authoritative_shadow(),
            live_authority: false,
            v2_economic_mutation: false,
            v2_proposal_created: false,
            v2_time_stop_mutation: false,
            duplicate_action_observed: false,
            route_build_authority_changed: false,
            terminal_isolation_violation: false,
            trajectory: bundle.v2.trajectory.clone(),
            vitality: bundle.v2.vitality.clone(),
            route_status: bundle.v2.route_status,
            entry_value_quote_raw: bundle.v2.entry_value_quote_raw,
            entry_value_source: bundle.v2.entry_value_source,
            entry_value_authoritative_for_shadow: bundle.v2.entry_value_authoritative_for_shadow,
            anchor_before: bundle.v2.executable_peak_anchor.clone(),
            anchor_request: anchor_request_label,
            anchor_applied: false,
            quote_keys,
            quote_resolution_count: quote_statuses.len() as u8,
            quote_statuses,
            current_executable_value_sol,
            current_executable_gross_return_bps,
            known_estimated_costs_sol,
        };

        let comparison_core = PreparedV1V2ComparisonCoreV1::prepare(record);
        self.record_het_pm_v2_comparison_core_outcome(&comparison_core);

        PreparedHetPmV2Tick {
            comparison_core,
            quote_cells,
            anchor_quote_cell,
            anchor_request,
            authority_candidate,
        }
    }

    fn apply_het_pm_v2_anchor_after_v1(
        &self,
        base_mint: &Pubkey,
        request: &PeakAnchorPreQuoteDecisionV1,
        anchor_quote_cell: Option<&HetPmV2QuoteCell>,
        entry_value_quote_raw: Option<u64>,
        policy_config_hash: &str,
        now_ms: u64,
    ) -> bool {
        let PeakAnchorPreQuoteDecisionV1::QuoteRequired { key, .. } = request else {
            return false;
        };
        let Some(cell) = anchor_quote_cell.filter(|cell| &cell.key == key) else {
            return false;
        };
        let Ok(truth) = &cell.outcome else {
            return false;
        };
        let quote = ExecutableExitQuote::new(
            truth.exit_token_amount_raw,
            truth.exit_price_sol,
            truth.exit_value_sol,
            truth.gross_pnl_sol,
            truth.pnl_pct,
        );
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(base_mint) else {
            return false;
        };
        if pos.position_id != key.position_id
            || pos.position_epoch != key.position_epoch
            || pos.state_revision < key.state_revision
            || pos.remaining_token_amount_raw != key.remaining_quantity_raw
            || pos.last_shadow_outcome.is_some()
            || pos.het_route_status.route_id() != key.route_id
        {
            return false;
        }
        let Ok(anchor) = materialize_anchor(
            request,
            &quote,
            entry_value_quote_raw,
            pos.het_next_anchor_seq,
            now_ms,
            policy_config_hash,
        ) else {
            return false;
        };
        if pos
            .het_executable_peak_anchor
            .as_ref()
            .is_some_and(|existing| existing.peak_mark_price_sol >= anchor.peak_mark_price_sol)
        {
            return false;
        }
        pos.het_next_anchor_seq = pos.het_next_anchor_seq.saturating_add(1);
        pos.het_executable_peak_anchor = Some(anchor);
        true
    }

    async fn run_shadow_runtime_tick(
        &self,
        base_mint: &Pubkey,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) {
        // RUG positions select an immutable per-position exit profile.  They
        // never enter the generic HET/V1 policy lattice, so neither
        // CrashGuard nor TimeStop can be mistaken for their typed facts.
        if self
            .positions
            .read()
            .get(base_mint)
            .is_some_and(|position| position.rug_scalp_facts.is_some())
        {
            self.prepare_and_run_rug_scalp_runtime_tick(base_mint, latest, now_ms)
                .await;
            return;
        }
        let Some(het_policy) = self.het_pm_v2.as_ref().filter(|policy| policy.enabled()) else {
            self.prepare_and_run_shadow_runtime_tick_v1(base_mint, latest, now_ms)
                .await;
            return;
        };
        if self.has_pending_terminal_commit(base_mint) {
            self.retry_pending_terminal_commit(base_mint, now_ms).await;
            return;
        }
        let Some(v1_policy) = self.exit_policy_v1.as_ref() else {
            return;
        };
        if let Some(latest) = latest {
            self.remember_shadow_snapshot(base_mint, latest);
        }
        let Some((bundle, latest_snapshot, raw_canonical_snapshot, trajectory_peak_snapshot)) =
            self.materialize_post_buy_snapshot_bundle(base_mint, now_ms)
        else {
            return;
        };
        let v1_prequote = ExitPolicyV1::evaluate_prequote(&bundle.base, v1_policy);
        let crash_prequote = ExitPolicyV1::evaluate_crash_guard_prequote(&bundle.base, v1_policy);
        let prepared = self.prepare_het_pm_v2_tick(HetPmV2TickInput {
            bundle: &bundle,
            latest_snapshot: latest_snapshot.as_ref(),
            raw_canonical_snapshot: raw_canonical_snapshot.as_ref(),
            trajectory_peak_snapshot: trajectory_peak_snapshot.as_ref(),
            v1_prequote: &v1_prequote,
            crash_prequote: &crash_prequote,
            v1_policy,
            het_policy,
            now_ms,
        });
        self.record_het_pm_v2_censoring_evidence(base_mint, &prepared.comparison_core, now_ms);

        // An already-created V1 proposal belongs to V1 until it has either
        // filled or reached its existing typed recovery terminal.  We never
        // convert it in place and never run two sell owners for one position.
        let (authoritative_prequote, authority_crash_prequote) =
            if het_policy.authoritative_shadow() && !bundle.base.has_pending_proposal() {
                match prepared.authority_candidate.as_ref() {
                    Some(candidate) => (
                        PreQuoteDecision::QuoteRequired {
                            candidate: candidate.clone(),
                        },
                        if matches!(candidate.reason(), ExitCandidateReason::CrashGuard) {
                            crash_prequote.clone()
                        } else {
                            CrashGuardPreQuoteDecision::Disabled
                        },
                    ),
                    None => (PreQuoteDecision::Hold, CrashGuardPreQuoteDecision::Disabled),
                }
            } else {
                (v1_prequote.clone(), crash_prequote.clone())
            };
        let v2_execution_route_id = (het_policy.authoritative_shadow()
            && !bundle.base.has_pending_proposal()
            && prepared.authority_candidate.is_some())
        .then_some(bundle.v2.route_status.route_id());

        let receipt = self
            .run_shadow_runtime_tick_v1(
                base_mint,
                V1AuthorityTickInput {
                    snapshot: &bundle.base,
                    latest_snapshot: latest_snapshot.as_ref(),
                    crash_evidence_snapshot: raw_canonical_snapshot.as_ref(),
                    authoritative_prequote: &authoritative_prequote,
                    crash_prequote: &authority_crash_prequote,
                    pre_resolved_quotes: &prepared.quote_cells,
                    v2_execution_route_id,
                    policy: v1_policy,
                    now_ms,
                },
            )
            .await;
        let anchor_applied = self.apply_het_pm_v2_anchor_after_v1(
            base_mint,
            &prepared.anchor_request,
            prepared.anchor_quote_cell.as_ref(),
            bundle.v2.entry_value_quote_raw,
            het_policy.config_hash(),
            now_ms,
        );
        let prepared_comparison = prepared.comparison_core.finalize(receipt, anchor_applied);
        self.record_het_pm_v2_comparison_final_outcome(&prepared_comparison);
        if self.has_pending_terminal_commit(base_mint) {
            match self
                .attach_prepared_het_comparison_to_pending_terminal(base_mint, prepared_comparison)
            {
                Ok(()) => {}
                Err(unattached) => {
                    let status = self
                        .persist_terminal_het_pm_v2_comparison(&unattached)
                        .await;
                    error!(
                        base_mint = %base_mint,
                        comparison_id = %unattached.correlation().comparison_id,
                        write_status = status.as_label(),
                        "PostBuyGuardian: terminal comparison could not attach to pending commit; persisted before fail-open canonical retry"
                    );
                }
            }
            self.retry_pending_terminal_commit(base_mint, now_ms).await;
        } else {
            self.enqueue_nonterminal_het_pm_v2_comparison(&prepared_comparison);
        }
    }

    /// Executes only the `rug_scalp_exit_v1` precedence lattice, then hands a
    /// single typed candidate to the existing guarded PM exit executor.  The
    /// profile computes target/stop from a full executable quote; it never
    /// treats the mark price as a fill or maps its facts to legacy policies.
    async fn prepare_and_run_rug_scalp_runtime_tick(
        &self,
        base_mint: &Pubkey,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) {
        if self.has_pending_terminal_commit(base_mint) {
            self.retry_pending_terminal_commit(base_mint, now_ms).await;
            return;
        }
        let Some(policy) = self.exit_policy_v1.as_ref() else {
            return;
        };
        let Some((facts, entry_slot, entry_size_lamports)) =
            self.positions.read().get(base_mint).and_then(|position| {
                position
                    .rug_scalp_facts
                    .clone()
                    .map(|facts| (facts, position.slot, position.entry_size_lamports))
            })
        else {
            return;
        };
        // A gap or incomplete route transition is not a trade signal and
        // must not be reinterpreted as a later TimeStop.  Position Manager
        // owns the terminal unresolved lifecycle: there is no sell intent,
        // no synthetic PnL and no second launcher-owned close path.
        if facts.blocker_active() {
            self.stage_rug_scalp_data_invalidated_terminal(base_mint, now_ms);
            self.retry_pending_terminal_commit(base_mint, now_ms).await;
            return;
        }
        if let Some(latest) = latest {
            self.remember_shadow_snapshot(base_mint, latest);
        }
        let Some((snapshot, latest_snapshot, _crash_evidence_snapshot)) =
            self.materialize_post_buy_decision_snapshot(base_mint, now_ms)
        else {
            // Missing canonical state is a typed data blocker.  We do not
            // create a synthetic time-stop or mark-price exit here.
            return;
        };
        let profile = &self.config.rug_scalp_exit_v1;
        let net_return_bps = if facts.blocker_active() {
            None
        } else {
            self.resolve_shadow_exit_truth_for_policy(
                &snapshot,
                latest_snapshot.as_ref(),
                snapshot.remaining_token_amount_raw(),
                now_ms,
                self.snapshot_source_for_position(base_mint),
            )
            .ok()
            .and_then(|truth| {
                let exit_value_lamports =
                    (truth.exit_value_sol * SHADOW_LAMPORTS_PER_SOL_F64).round() as i128;
                let intended_lamports = entry_size_lamports as i128;
                (intended_lamports > 0).then(|| {
                    let numerator = exit_value_lamports
                        - intended_lamports
                        - profile.entry_fixed_cost_lamports as i128
                        - profile.exit_fixed_cost_lamports as i128;
                    numerator
                        .saturating_mul(10_000)
                        .saturating_div(intended_lamports)
                        .clamp(i32::MIN as i128, i32::MAX as i128) as i32
                })
            })
        };
        let observed_slot = latest_snapshot.as_ref().and_then(|sample| sample.slot);
        let reason = evaluate_rug_scalp_exit_v1(
            &facts,
            profile,
            snapshot.has_pending_proposal(),
            false,
            net_return_bps,
            entry_slot,
            observed_slot,
            snapshot.absolute_age_ms(),
        );
        // A condition is observed at one canonical slot, but the PRIMARY
        // model must submit from the first later state that covers the frozen
        // exit-latency boundary.  This is position-owned PM state, not a
        // second reducer/lifecycle in the launcher.
        let candidate_reason = observed_slot.and_then(|current_slot| {
            let requires_exit = matches!(
                reason,
                RugScalpExitReasonV1::MaterialSellEmergency
                    | RugScalpExitReasonV1::TargetReached10PctNet
                    | RugScalpExitReasonV1::BaselineHardLoss5PctNet
                    | RugScalpExitReasonV1::FlowExhausted
                    | RugScalpExitReasonV1::MaxHold
            );
            if !requires_exit {
                return None;
            }
            let mut positions = self.positions.write();
            let position = positions.get_mut(base_mint)?;
            match position.rug_scalp_pending_exit {
                None => {
                    position.rug_scalp_pending_exit = Some(RugScalpPendingExitV1 {
                        reason,
                        observed_slot: current_slot,
                    });
                }
                Some(pending)
                    if matches!(reason, RugScalpExitReasonV1::MaterialSellEmergency)
                        && !matches!(
                            pending.reason,
                            RugScalpExitReasonV1::MaterialSellEmergency
                        ) =>
                {
                    // A completed same-slot dump is the sole override of an
                    // earlier lower-priority pending condition (DUMP_WINS).
                    position.rug_scalp_pending_exit = Some(RugScalpPendingExitV1 {
                        reason,
                        observed_slot: current_slot,
                    });
                }
                Some(_) => {}
            }
            position.rug_scalp_pending_exit.and_then(|pending| {
                (current_slot
                    >= pending
                        .observed_slot
                        .saturating_add(profile.primary_exit_latency_slots))
                .then_some(pending.reason)
            })
        });
        let candidate = match candidate_reason {
            Some(RugScalpExitReasonV1::MaterialSellEmergency) => Some(ExitCandidate::from_reason(
                ExitCandidateReason::RugScalpMaterialSellEmergency,
            )),
            Some(RugScalpExitReasonV1::TargetReached10PctNet) => Some(ExitCandidate::from_reason(
                ExitCandidateReason::RugScalpTargetReached10PctNet,
            )),
            Some(RugScalpExitReasonV1::BaselineHardLoss5PctNet) => Some(
                ExitCandidate::from_reason(ExitCandidateReason::RugScalpBaselineHardLoss5PctNet),
            ),
            Some(RugScalpExitReasonV1::FlowExhausted) => Some(ExitCandidate::from_reason(
                ExitCandidateReason::RugScalpFlowExhausted,
            )),
            Some(RugScalpExitReasonV1::MaxHold) => Some(ExitCandidate::from_reason(
                ExitCandidateReason::RugScalpMaxHold,
            )),
            Some(RugScalpExitReasonV1::PendingReconciliation)
            | Some(RugScalpExitReasonV1::DataIdentityRouteBlocked)
            | Some(RugScalpExitReasonV1::Hold)
            | None => None,
        };
        let authoritative_prequote = candidate.map_or(PreQuoteDecision::Hold, |candidate| {
            PreQuoteDecision::QuoteRequired { candidate }
        });
        let crash_prequote = CrashGuardPreQuoteDecision::Disabled;
        let _ = self
            .run_shadow_runtime_tick_v1(
                base_mint,
                V1AuthorityTickInput {
                    snapshot: &snapshot,
                    latest_snapshot: latest_snapshot.as_ref(),
                    crash_evidence_snapshot: None,
                    authoritative_prequote: &authoritative_prequote,
                    crash_prequote: &crash_prequote,
                    pre_resolved_quotes: &[],
                    v2_execution_route_id: None,
                    policy,
                    now_ms,
                },
            )
            .await;
        if self.has_pending_terminal_commit(base_mint) {
            self.retry_pending_terminal_commit(base_mint, now_ms).await;
        }
    }

    /// A typed RUG data/identity/route blocker has no executable sell fact.
    /// It therefore terminates as a PM-owned unresolved lifecycle record
    /// rather than fabricating a quote, an exit attempt, or generic legacy
    /// stop semantics.  The normal pending-terminal persistence path still
    /// makes the outcome one-shot and durable before the launcher is told.
    fn stage_rug_scalp_data_invalidated_terminal(&self, base_mint: &Pubkey, now_ms: u64) {
        let mut positions = self.positions.write();
        let Some(position) = positions.get_mut(base_mint) else {
            return;
        };
        if position.rug_scalp_facts.is_none()
            || position.pending_terminal_commit.is_some()
            || position.last_shadow_outcome.is_some()
        {
            return;
        }
        let action_id = format!(
            "rug-scalp-data-invalidated:{}:{}",
            position.position_id, position.state_revision
        );
        let evidence = PriceTruthEvidence {
            source: position.last_snapshot_source,
            status: PriceTruthStatus::SemanticViolation,
            detail: Some("rug_scalp_typed_data_or_route_blocker".to_string()),
            slot: position
                .last_shadow_snapshot
                .as_ref()
                .and_then(|sample| sample.slot)
                .or(position.slot),
            timestamp_ms: Some(now_ms),
            age_ms: None,
            price_state: None,
            price_reason: None,
        };
        position.last_price_truth = Some(evidence.clone());
        position.last_applied_action_id = Some(action_id.clone());
        position.last_source_snapshot_id = Some("rug_scalp_data_invalidated".to_string());
        position.last_shadow_outcome = Some(ShadowOutcomeKind::BlockedByData);
        position.pending_exit_proposal = None;
        position.state_revision = position.state_revision.saturating_add(1);

        let mut record = self.shadow_lifecycle_record_base(
            position,
            ShadowLifecycleRecordType::PositionUnresolved,
            now_ms,
            &evidence,
        );
        record.action_id = Some(action_id.clone());
        record.source_snapshot_id = Some("rug_scalp_data_invalidated".to_string());
        record.terminal_reason_v2 =
            Some(ShadowUnresolvedReason::BlockedByData.terminal_reason_v2());
        record.exit_policy_reason_code = Some("rug_scalp_data_invalidated".to_string());
        record.terminal_disposition = Some("simulation_blocked".to_string());
        record.recovery_elapsed_ms = Some(0);
        record.remaining_token_amount_raw = Some(position.remaining_token_amount_raw);
        record.final_pnl = None;
        record.final_pnl_pct = None;
        record.gross_pnl_sol = None;
        record.net_pnl_sol = None;
        record.exit_price = None;
        record.exit_value_sol = None;
        record.exit_token_amount_raw = None;
        record.exit_landed_slot = None;
        record.exit_landed_slot_source = None;
        position.pending_terminal_commit = Some(PendingTerminalCommit {
            action_id: action_id.clone(),
            record,
            disposition: ShadowTerminalDisposition::SimulationBlocked {
                action_id,
                reason: ShadowUnresolvedReason::BlockedByData,
            },
            last_attempt_ms: None,
            lifecycle_jsonl_committed: false,
            prepared_het_comparison: None,
            het_comparison_write_status: HetComparisonWriteStatusV1::NotApplicable,
        });
    }

    async fn prepare_and_run_shadow_runtime_tick_v1(
        &self,
        base_mint: &Pubkey,
        latest: Option<&MarketSnapshot>,
        now_ms: u64,
    ) {
        if self.has_pending_terminal_commit(base_mint) {
            self.retry_pending_terminal_commit(base_mint, now_ms).await;
            return;
        }
        let Some(policy) = self.exit_policy_v1.as_ref() else {
            return;
        };
        if let Some(latest) = latest {
            self.remember_shadow_snapshot(base_mint, latest);
        }
        let Some((snapshot, latest_snapshot, crash_evidence_snapshot)) =
            self.materialize_post_buy_decision_snapshot(base_mint, now_ms)
        else {
            return;
        };
        let authoritative_prequote = ExitPolicyV1::evaluate_prequote(&snapshot, policy);
        let crash_prequote = ExitPolicyV1::evaluate_crash_guard_prequote(&snapshot, policy);
        let _ = self
            .run_shadow_runtime_tick_v1(
                base_mint,
                V1AuthorityTickInput {
                    snapshot: &snapshot,
                    latest_snapshot: latest_snapshot.as_ref(),
                    crash_evidence_snapshot: crash_evidence_snapshot.as_ref(),
                    authoritative_prequote: &authoritative_prequote,
                    crash_prequote: &crash_prequote,
                    pre_resolved_quotes: &[],
                    v2_execution_route_id: None,
                    policy,
                    now_ms,
                },
            )
            .await;
        if self.has_pending_terminal_commit(base_mint) {
            self.retry_pending_terminal_commit(base_mint, now_ms).await;
        }
    }

    async fn run_shadow_runtime_tick_v1(
        &self,
        base_mint: &Pubkey,
        input: V1AuthorityTickInput<'_>,
    ) -> V1AuthorityTickReceiptV1 {
        let V1AuthorityTickInput {
            snapshot,
            latest_snapshot,
            crash_evidence_snapshot,
            authoritative_prequote,
            crash_prequote,
            pre_resolved_quotes,
            v2_execution_route_id,
            policy,
            now_ms,
        } = input;
        let mut receipt_outcome = V1AuthorityTickOutcomeV1::Hold;
        let mut receipt_action_id = None;
        let mut receipt_reason = None;
        let mut receipt_crash_quote_decision = None;
        let mut exit_applied = false;

        if let PreQuoteDecision::UnknownEvidence { reason } = authoritative_prequote {
            receipt_outcome = V1AuthorityTickOutcomeV1::Blocked;
            receipt_reason = Some(format!("prequote_unknown:{reason:?}"));
        }

        'authority_tick: {
            let baseline_candidate = match authoritative_prequote {
                PreQuoteDecision::QuoteRequired { candidate } => Some(candidate.clone()),
                PreQuoteDecision::Hold | PreQuoteDecision::UnknownEvidence { .. } => None,
            };
            let crash_prequote_evidence = Self::crash_guard_prequote_evidence(snapshot, policy);

            if let CrashGuardPreQuoteDecision::NotTriggered { reason } = crash_prequote {
                self.maybe_record_crash_guard_observation(
                    base_mint,
                    snapshot,
                    CrashGuardObservationState::NotTriggered,
                    Some(*reason),
                    None,
                    authoritative_prequote,
                    crash_prequote,
                    &crash_prequote_evidence,
                    policy,
                    now_ms,
                );
            }

            let crash_candidate = matches!(
                crash_prequote,
                CrashGuardPreQuoteDecision::QuoteRequired { .. }
            );
            let prequote_crash_requirement = crash_candidate
                .then(|| ExitPolicyV1::crash_guard_quote_requirement(snapshot))
                .flatten();
            if crash_candidate {
                self.maybe_record_crash_guard_observation(
                    base_mint,
                    snapshot,
                    CrashGuardObservationState::Candidate,
                    None,
                    None,
                    authoritative_prequote,
                    crash_prequote,
                    &crash_prequote_evidence,
                    policy,
                    now_ms,
                );
            }
            let observe_crash_candidate =
                matches!(policy.crash_guard_mode(), CrashGuardMode::ObserveOnly) && crash_candidate;
            let mut action = if snapshot.has_pending_proposal() {
                match self.prepare_pending_quote_retry(base_mint, snapshot.guard(), now_ms) {
                    Ok(Some(action)) => Some(action),
                    Ok(None) => break 'authority_tick,
                    Err(PositionApplyError::StaleRevision) => {
                        receipt_outcome = V1AuthorityTickOutcomeV1::ApplyRejected;
                        receipt_reason = Some("pending_retry_stale_revision".to_string());
                        break 'authority_tick;
                    }
                    Err(error) => {
                        receipt_outcome = V1AuthorityTickOutcomeV1::ApplyRejected;
                        receipt_reason = Some(format!("pending_retry_rejected:{error}"));
                        debug!(
                            base_mint = %base_mint,
                            error = %error,
                            "PostBuyGuardian: pending exit proposal could not be retried"
                        );
                        break 'authority_tick;
                    }
                }
            } else {
                let selected = match (crash_prequote, authoritative_prequote) {
                    (CrashGuardPreQuoteDecision::QuoteRequired { candidate }, _)
                        if matches!(
                            policy.crash_guard_mode(),
                            CrashGuardMode::AuthoritativeShadow
                        ) =>
                    {
                        let Some(requirement) = prequote_crash_requirement.clone() else {
                            error!(
                                base_mint = %base_mint,
                                "PostBuyGuardian: CrashGuard candidate lacked immutable quote provenance"
                            );
                            break 'authority_tick;
                        };
                        Some((candidate, Some(requirement)))
                    }
                    (_, _) => baseline_candidate
                        .as_ref()
                        .map(|candidate| (candidate, None)),
                };
                match selected {
                    Some((candidate, crash_guard_quote_requirement)) => match self
                        .begin_exit_proposal(
                            base_mint,
                            snapshot.guard(),
                            candidate,
                            snapshot.snapshot_id(),
                            snapshot.inactivity_age_ms(),
                            crash_guard_quote_requirement,
                            v2_execution_route_id,
                            now_ms,
                        ) {
                        Ok(action) => Some(action),
                        Err(PositionApplyError::StaleRevision) => {
                            receipt_outcome = V1AuthorityTickOutcomeV1::ApplyRejected;
                            receipt_reason = Some("proposal_stale_revision".to_string());
                            break 'authority_tick;
                        }
                        Err(error) => {
                            receipt_outcome = V1AuthorityTickOutcomeV1::ApplyRejected;
                            receipt_reason = Some(format!("proposal_rejected:{error}"));
                            debug!(
                                base_mint = %base_mint,
                                error = %error,
                                "PostBuyGuardian: exit proposal rejected by guarded apply"
                            );
                            break 'authority_tick;
                        }
                    },
                    None => None,
                }
            };
            if let Some(action) = action.as_ref() {
                receipt_outcome = V1AuthorityTickOutcomeV1::ProposalStarted;
                receipt_action_id = Some(action.action_id.clone());
                receipt_reason = Some(action.reason.as_label().to_string());
            }

            let crash_action_owned = action
                .as_ref()
                .is_some_and(|action| matches!(action.reason, ExitCandidateReason::CrashGuard));
            let crash_quote_requirement = if crash_action_owned {
                action
                    .as_ref()
                    .and_then(|action| action.crash_guard_quote_requirement.clone())
            } else if observe_crash_candidate {
                prequote_crash_requirement
            } else {
                None
            };
            if crash_action_owned && crash_quote_requirement.is_none() {
                receipt_outcome = V1AuthorityTickOutcomeV1::Blocked;
                receipt_reason = Some("crash_quote_requirement_missing".to_string());
                let failure = ExecutableQuoteFailure {
                kind: ExecutableQuoteFailureKind::SemanticViolation,
                evidence: PriceTruthEvidence {
                    source: crash_prequote_evidence.source,
                    status: PriceTruthStatus::SemanticViolation,
                    detail: Some(
                        "CrashGuard pending proposal is missing its immutable candidate provenance"
                            .to_string(),
                    ),
                    slot: crash_prequote_evidence.slot,
                    timestamp_ms: crash_prequote_evidence.timestamp_ms,
                    age_ms: crash_prequote_evidence.age_ms,
                    price_state: crash_prequote_evidence.price_state,
                    price_reason: crash_prequote_evidence.price_reason,
                },
            };
                if let Some(action) = action {
                    self.handle_shadow_quote_failure(action, failure, now_ms)
                        .await;
                }
                break 'authority_tick;
            }

            if action.is_none() && crash_quote_requirement.is_none() {
                break 'authority_tick;
            }

            let expected_quantity = action
                .as_ref()
                .map(|action| action.expected_remaining_quantity)
                .unwrap_or_else(|| snapshot.remaining_token_amount_raw());
            let evidence_source = self.snapshot_source_for_position(base_mint);
            // A CrashGuard-owned action must use the raw canonical sample that
            // proved the path. A baseline-owned action deliberately keeps the
            // PR1 runtime projection even when CrashGuard is observing the same
            // tick: observation must not alter a TP/SL/inactivity/max-hold fill.
            // The one local resolution is still shared; the CrashGuard result is
            // then either confirmed/rejected or blocked by its provenance check.
            let quote_snapshot = if crash_action_owned || action.is_none() {
                crash_evidence_snapshot.or(latest_snapshot)
            } else {
                latest_snapshot
            };
            let pre_resolved_outcome = quote_snapshot.and_then(|quote_snapshot| {
                pre_resolved_quotes
                    .iter()
                    .find(|cell| {
                        cell.key.position_id == snapshot.guard().position_id()
                            && cell.key.position_epoch == snapshot.guard().position_epoch()
                            && cell.key.state_revision == snapshot.guard().state_revision()
                            && cell.key.remaining_quantity_raw == expected_quantity
                            && cell.key.sample_slot == quote_snapshot.slot
                            && cell.key.sample_timestamp_ms == Some(quote_snapshot.timestamp_ms)
                    })
                    .map(|cell| cell.outcome.clone())
            });
            let truth_result = pre_resolved_outcome.unwrap_or_else(|| {
                self.resolve_shadow_exit_truth_for_policy(
                    snapshot,
                    quote_snapshot,
                    expected_quantity,
                    now_ms,
                    evidence_source,
                )
            });
            let truth = match truth_result {
                Ok(truth) => truth,
                Err(failure) => {
                    receipt_outcome = if action.is_some() {
                        V1AuthorityTickOutcomeV1::PendingRecovery
                    } else {
                        V1AuthorityTickOutcomeV1::Blocked
                    };
                    receipt_reason = Some(format!("quote_blocked:{:?}", failure.kind));
                    if crash_quote_requirement.is_some() {
                        self.maybe_record_crash_guard_observation(
                            base_mint,
                            snapshot,
                            CrashGuardObservationState::BlockedByData,
                            None,
                            None,
                            authoritative_prequote,
                            crash_prequote,
                            &failure.evidence,
                            policy,
                            now_ms,
                        );
                    }
                    if let Some(action) = action {
                        self.handle_shadow_quote_failure(action, failure, now_ms)
                            .await;
                    }
                    break 'authority_tick;
                }
            };
            let quote = ExecutableExitQuote::new(
                truth.exit_token_amount_raw,
                truth.exit_price_sol,
                truth.exit_value_sol,
                truth.gross_pnl_sol,
                truth.pnl_pct,
            );
            let quote_evidence = QuoteEvidenceRevisionV1::new(
                truth.evidence.slot,
                truth.evidence.timestamp_ms,
                truth.evidence.age_ms,
            );
            let crash_quote_decision = crash_quote_requirement.as_ref().map(|requirement| {
                ExitPolicyV1::evaluate_crash_guard_quote(
                    snapshot,
                    &quote,
                    quote_evidence,
                    requirement,
                    policy,
                )
            });
            if let Some(crash_quote_decision) = crash_quote_decision {
                receipt_crash_quote_decision = Some(crash_quote_decision);
                if matches!(crash_quote_decision, CrashGuardQuoteDecision::BlockedByData)
                    && action.is_none()
                {
                    receipt_outcome = V1AuthorityTickOutcomeV1::Blocked;
                    receipt_reason = Some("crash_quote_blocked_by_data".to_string());
                }
                match crash_quote_decision {
                    CrashGuardQuoteDecision::Confirmed => self
                        .maybe_record_crash_guard_observation(
                            base_mint,
                            snapshot,
                            CrashGuardObservationState::Confirmed,
                            None,
                            None,
                            authoritative_prequote,
                            crash_prequote,
                            &truth.evidence,
                            policy,
                            now_ms,
                        ),
                    CrashGuardQuoteDecision::RejectedByQuote { reason } => {
                        self.maybe_record_crash_guard_observation(
                            base_mint,
                            snapshot,
                            CrashGuardObservationState::RejectedByQuote,
                            None,
                            Some(reason),
                            authoritative_prequote,
                            crash_prequote,
                            &truth.evidence,
                            policy,
                            now_ms,
                        );
                    }
                    CrashGuardQuoteDecision::BlockedByData => self
                        .maybe_record_crash_guard_observation(
                            base_mint,
                            snapshot,
                            CrashGuardObservationState::BlockedByData,
                            None,
                            None,
                            authoritative_prequote,
                            crash_prequote,
                            &truth.evidence,
                            policy,
                            now_ms,
                        ),
                }

                if crash_action_owned {
                    match crash_quote_decision {
                        CrashGuardQuoteDecision::Confirmed => {}
                        CrashGuardQuoteDecision::RejectedByQuote { .. } => {
                            let Some(action_handle) = action.as_ref() else {
                                break 'authority_tick;
                            };
                            if let Some(fallback_candidate) = baseline_candidate.as_ref() {
                                match self.retarget_shadow_proposal_after_crash_rejection(
                                    action_handle,
                                    fallback_candidate,
                                    snapshot.inactivity_age_ms(),
                                ) {
                                    Ok(retargeted) => action = Some(retargeted),
                                    Err(error) => {
                                        receipt_outcome = V1AuthorityTickOutcomeV1::ApplyRejected;
                                        receipt_reason = Some(format!(
                                            "crash_fallback_retarget_rejected:{error}"
                                        ));
                                        debug!(
                                            action_id = %action_handle.action_id,
                                            error = %error,
                                            "PostBuyGuardian: CrashGuard rejection could not preserve baseline proposal"
                                        );
                                        break 'authority_tick;
                                    }
                                }
                            } else if let Err(error) =
                                self.cancel_shadow_proposal_after_crash_rejection(action_handle)
                            {
                                receipt_outcome = V1AuthorityTickOutcomeV1::ApplyRejected;
                                receipt_reason =
                                    Some(format!("crash_proposal_cancel_rejected:{error}"));
                                debug!(
                                    action_id = %action_handle.action_id,
                                    error = %error,
                                    "PostBuyGuardian: CrashGuard quote rejection could not clear proposal"
                                );
                                break 'authority_tick;
                            }
                            if baseline_candidate.is_none() {
                                break 'authority_tick;
                            }
                        }
                        CrashGuardQuoteDecision::BlockedByData => {
                            receipt_outcome = V1AuthorityTickOutcomeV1::PendingRecovery;
                            receipt_reason = Some("crash_quote_blocked_by_data".to_string());
                            if let Some(action) = action {
                                let failure = ExecutableQuoteFailure {
                                kind: ExecutableQuoteFailureKind::SemanticViolation,
                                evidence: PriceTruthEvidence {
                                    source: truth.evidence.source,
                                    status: PriceTruthStatus::SemanticViolation,
                                    detail: Some(
                                        "CrashGuard quote provenance is stale or older than its candidate evidence"
                                            .to_string(),
                                    ),
                                    slot: truth.evidence.slot,
                                    timestamp_ms: truth.evidence.timestamp_ms,
                                    age_ms: truth.evidence.age_ms,
                                    price_state: truth.evidence.price_state,
                                    price_reason: truth.evidence.price_reason,
                                },
                            };
                                self.handle_shadow_quote_failure(action, failure, now_ms)
                                    .await;
                            }
                            break 'authority_tick;
                        }
                    }
                }
            }

            let Some(action) = action else {
                // Observation-only CrashGuard has completed its one lazy quote.
                break 'authority_tick;
            };
            let candidate = ExitCandidate::from_reason(action.reason);
            match ExitPolicyV1::finalize_with_quote(snapshot, &candidate, &quote, policy) {
                FinalPolicyDecision::Exit { intent } => {
                    if intent.quantity_raw() != action.expected_remaining_quantity
                        || intent.reason() != action.reason
                    {
                        let failure = ExecutableQuoteFailure {
                            kind: ExecutableQuoteFailureKind::QuantityMismatch,
                            evidence: PriceTruthEvidence {
                                source: truth.evidence.source,
                                status: PriceTruthStatus::SemanticViolation,
                                detail: Some(
                                    "pure exit intent disagreed with guarded pending proposal"
                                        .to_string(),
                                ),
                                slot: truth.evidence.slot,
                                timestamp_ms: truth.evidence.timestamp_ms,
                                age_ms: truth.evidence.age_ms,
                                price_state: truth.evidence.price_state,
                                price_reason: truth.evidence.price_reason,
                            },
                        };
                        self.handle_shadow_quote_failure(action, failure, now_ms)
                            .await;
                        receipt_outcome = V1AuthorityTickOutcomeV1::PendingRecovery;
                        receipt_reason = Some("final_intent_mismatch".to_string());
                        break 'authority_tick;
                    }
                    if let Err(error) = self.apply_shadow_quote_outcome(&action, snapshot, &truth) {
                        receipt_outcome = V1AuthorityTickOutcomeV1::ApplyRejected;
                        receipt_reason = Some(format!("resolved_quote_apply_rejected:{error}"));
                        debug!(
                            action_id = %action.action_id,
                            error = %error,
                            "PostBuyGuardian: resolved quote rejected by guarded apply"
                        );
                        break 'authority_tick;
                    }

                    let exit = super::integration::ShadowExitExecution {
                        position_id: action.position_id.clone(),
                        position_epoch: action.position_epoch,
                        fraction_bps: 10_000,
                        remaining_fraction_bps: 0,
                        fill_price: truth.exit_price_sol,
                    };
                    self.emit_shadow_exit_for_action(&action, &exit, &truth, now_ms);
                    exit_applied = true;
                    receipt_action_id = Some(action.action_id.clone());
                    receipt_reason = Some(action.reason.as_label().to_string());
                    self.finish_resolved_shadow_position(action, now_ms);
                }
                FinalPolicyDecision::UnknownEvidence { reason } => {
                    receipt_outcome = V1AuthorityTickOutcomeV1::PendingRecovery;
                    receipt_reason = Some(format!("final_policy_unknown:{reason:?}"));
                    let failure = ExecutableQuoteFailure {
                        kind: ExecutableQuoteFailureKind::SemanticViolation,
                        evidence: PriceTruthEvidence {
                            source: truth.evidence.source,
                            status: PriceTruthStatus::SemanticViolation,
                            detail: Some(format!("exit quote rejected by policy: {reason:?}")),
                            slot: truth.evidence.slot,
                            timestamp_ms: truth.evidence.timestamp_ms,
                            age_ms: truth.evidence.age_ms,
                            price_state: truth.evidence.price_state,
                            price_reason: truth.evidence.price_reason,
                        },
                    };
                    self.handle_shadow_quote_failure(action, failure, now_ms)
                        .await;
                }
                FinalPolicyDecision::Hold => {}
            }
        }

        let position_state = self.positions.read().get(base_mint).map(|pos| {
            (
                pos.pending_exit_proposal.is_some(),
                pos.pending_terminal_commit.is_some(),
            )
        });
        let terminal_commit_status = match position_state {
            None => V1TerminalCommitStatusV1::Committed,
            Some((_, true)) => V1TerminalCommitStatusV1::Pending,
            Some(_) => V1TerminalCommitStatusV1::NotRequired,
        };
        match position_state {
            None if exit_applied => receipt_outcome = V1AuthorityTickOutcomeV1::ExitApplied,
            None => receipt_outcome = V1AuthorityTickOutcomeV1::PendingRecovery,
            Some((_, true)) if exit_applied => {
                receipt_outcome = V1AuthorityTickOutcomeV1::ExitApplied;
            }
            Some((_, true)) => receipt_outcome = V1AuthorityTickOutcomeV1::PendingRecovery,
            Some((true, false))
                if !matches!(receipt_outcome, V1AuthorityTickOutcomeV1::ApplyRejected) =>
            {
                receipt_outcome = V1AuthorityTickOutcomeV1::PendingRecovery;
            }
            Some(_) => {}
        }

        V1AuthorityTickReceiptV1 {
            snapshot_id: snapshot.snapshot_id().to_string(),
            state_revision: snapshot.guard().state_revision(),
            remaining_quantity_raw: snapshot.remaining_token_amount_raw(),
            outcome: receipt_outcome,
            exit_apply_status: if exit_applied {
                V1ExitApplyStatusV1::Applied
            } else if matches!(receipt_outcome, V1AuthorityTickOutcomeV1::ApplyRejected) {
                V1ExitApplyStatusV1::Rejected
            } else {
                V1ExitApplyStatusV1::NotApplied
            },
            terminal_commit_status,
            action_id: receipt_action_id,
            reason: receipt_reason,
            crash_quote_decision: receipt_crash_quote_decision,
        }
    }

    async fn handle_shadow_quote_failure(
        &self,
        action: ShadowExitActionHandle,
        failure: ExecutableQuoteFailure,
        now_ms: u64,
    ) {
        let evidence = failure.evidence.clone();
        self.maybe_record_shadow_exit_blocked(&action.base_mint, now_ms, 10_000, &evidence);
        if now_ms < action.recovery_deadline_ms {
            return;
        }

        let unresolved_reason = failure.kind.unresolved_reason();
        match self.terminate_shadow_proposal(&action, unresolved_reason, evidence.clone()) {
            Ok(()) => {
                let recovery_elapsed_ms = now_ms.saturating_sub(action.triggered_at_ms);
                let terminal = {
                    let positions = self.positions.read();
                    let Some(pos) = positions.get(&action.base_mint) else {
                        return;
                    };
                    self.emit_shadow_unresolved(
                        pos,
                        &action,
                        unresolved_reason,
                        recovery_elapsed_ms,
                        now_ms,
                        &evidence,
                    )
                };
                let disposition = ShadowTerminalDisposition::SimulationBlocked {
                    action_id: action.action_id.clone(),
                    reason: unresolved_reason,
                };
                let _ = self.stage_terminal_commit(&action, terminal, disposition);
            }
            Err(PositionApplyError::StaleRevision) => {}
            Err(error) => debug!(
                action_id = %action.action_id,
                error = %error,
                "PostBuyGuardian: unresolved shadow terminal rejected by guarded apply"
            ),
        }
    }

    fn emit_shadow_exit_for_action(
        &self,
        action: &ShadowExitActionHandle,
        exit: &super::integration::ShadowExitExecution,
        truth: &ShadowExitTruth,
        now_ms: u64,
    ) {
        let identity = self
            .positions
            .read()
            .get(&action.base_mint)
            .map(|pos| (pos.candidate_id.clone(), pos.quote_id.clone(), pos.slot));
        let Some((candidate_id, quote_id, slot)) = identity else {
            return;
        };
        self.emit_shadow_exit(
            &action.base_mint,
            &action.action_id,
            &candidate_id,
            &action.position_id,
            action.position_epoch,
            &quote_id,
            slot,
            exit,
            truth,
            now_ms,
        );
    }

    fn finish_resolved_shadow_position(&self, action: ShadowExitActionHandle, now_ms: u64) {
        let terminal = {
            let positions = self.positions.read();
            let Some(pos) = positions.get(&action.base_mint) else {
                return;
            };
            if pos.position_id != action.position_id
                || pos.position_epoch != action.position_epoch
                || pos.last_applied_action_id.as_deref() != Some(action.action_id.as_str())
                || !matches!(
                    pos.last_shadow_outcome,
                    Some(ShadowOutcomeKind::SimulatedFilled)
                )
            {
                return;
            }
            let duration_ms = now_ms.saturating_sub(pos.entry_unix_ms);
            self.emit_position_closed(pos, duration_ms, now_ms)
        };
        let Some(terminal) = terminal else {
            return;
        };
        let net_pnl_lamports = terminal.net_pnl_sol.and_then(|net_pnl_sol| {
            let lamports = net_pnl_sol * SHADOW_LAMPORTS_PER_SOL_F64;
            (lamports.is_finite() && lamports >= i64::MIN as f64 && lamports <= i64::MAX as f64)
                .then(|| lamports.round() as i64)
        });
        let disposition = ShadowTerminalDisposition::SimulatedClosed {
            action_id: action.action_id.clone(),
            reason: action.reason.reason_code().to_string(),
            net_pnl_lamports,
            exit_landed_slot: terminal.exit_landed_slot,
        };
        let _ = self.stage_terminal_commit(&action, terminal, disposition);
    }

    fn has_pending_terminal_commit(&self, base_mint: &Pubkey) -> bool {
        self.positions
            .read()
            .get(base_mint)
            .is_some_and(|pos| pos.pending_terminal_commit.is_some())
    }

    fn stage_terminal_commit(
        &self,
        action: &ShadowExitActionHandle,
        record: ShadowLifecycleRecord,
        disposition: ShadowTerminalDisposition,
    ) -> Result<(), PositionApplyError> {
        let mut positions = self.positions.write();
        let pos = positions
            .get_mut(&action.base_mint)
            .ok_or(PositionApplyError::PositionNotFound)?;
        if pos.position_id != action.position_id {
            return Err(PositionApplyError::PositionNotFound);
        }
        if pos.position_epoch != action.position_epoch {
            return Err(PositionApplyError::EpochMismatch);
        }
        if pos.last_applied_action_id.as_deref() != Some(action.action_id.as_str()) {
            return Err(PositionApplyError::ActionMismatch);
        }
        if pos.pending_terminal_commit.is_some() {
            return Err(PositionApplyError::ConcurrentActionPending);
        }
        pos.pending_terminal_commit = Some(PendingTerminalCommit {
            action_id: action.action_id.clone(),
            record,
            disposition,
            last_attempt_ms: None,
            lifecycle_jsonl_committed: false,
            prepared_het_comparison: None,
            het_comparison_write_status: HetComparisonWriteStatusV1::NotApplicable,
        });
        Ok(())
    }

    fn apply_het_comparison_status_to_terminal_record(
        record: &mut ShadowLifecycleRecord,
        prepared: &PreparedHetComparisonV1,
        status: &HetComparisonWriteStatusV1,
    ) {
        let correlation = prepared.correlation();
        record.het_pm_v2_comparison_id = Some(correlation.comparison_id.clone());
        record.het_pm_v2_writer_instance_id = Some(correlation.writer_instance_id.clone());
        record.het_pm_v2_source_snapshot_id = Some(correlation.source_snapshot_id.clone());
        record.het_pm_v2_comparison_write_status = Some(status.as_label().to_string());
        record.het_pm_v2_comparison_skip_reason = status
            .skip_reason()
            .map(|reason| reason.as_label().to_string());
        record.het_pm_v2_comparison_outcome_unknown_reason = status
            .outcome_unknown_reason()
            .map(|reason| reason.as_label().to_string());
    }

    fn attach_prepared_het_comparison_to_pending_terminal(
        &self,
        base_mint: &Pubkey,
        prepared: PreparedHetComparisonV1,
    ) -> Result<(), Box<PreparedHetComparisonV1>> {
        let Some(action_id) = prepared.action_id().map(str::to_string) else {
            return Err(Box::new(prepared));
        };
        let initial_status = match &prepared {
            PreparedHetComparisonV1::Ready { .. } => HetComparisonWriteStatusV1::NotAttempted,
            PreparedHetComparisonV1::Skipped { reason, detail, .. } => {
                HetComparisonWriteStatusV1::Skipped {
                    reason: *reason,
                    detail: detail.clone(),
                }
            }
        };
        let mut positions = self.positions.write();
        let Some(pos) = positions.get_mut(base_mint) else {
            return Err(Box::new(prepared));
        };
        let Some(pending) = pos.pending_terminal_commit.as_mut() else {
            return Err(Box::new(prepared));
        };
        if pending.action_id != action_id || pending.prepared_het_comparison.is_some() {
            return Err(Box::new(prepared));
        }
        Self::apply_het_comparison_status_to_terminal_record(
            &mut pending.record,
            &prepared,
            &initial_status,
        );
        pending.prepared_het_comparison = Some(prepared);
        pending.het_comparison_write_status = initial_status;
        Ok(())
    }

    async fn retry_pending_terminal_commit(&self, base_mint: &Pubkey, now_ms: u64) {
        let mut pending = {
            let mut positions = self.positions.write();
            let Some(pos) = positions.get_mut(base_mint) else {
                return;
            };
            let Some(pending) = pos.pending_terminal_commit.as_mut() else {
                return;
            };
            if pending
                .last_attempt_ms
                .is_some_and(|last| now_ms.saturating_sub(last) < SHADOW_QUOTE_RETRY_INTERVAL_MS)
            {
                return;
            }
            pending.last_attempt_ms = Some(now_ms);
            pending.clone()
        };

        if matches!(
            pending.het_comparison_write_status,
            HetComparisonWriteStatusV1::NotAttempted
        ) {
            if let Some(prepared) = pending.prepared_het_comparison.as_ref() {
                let write_status = self.persist_terminal_het_pm_v2_comparison(prepared).await;
                let mut positions = self.positions.write();
                let Some(pos) = positions.get_mut(base_mint) else {
                    return;
                };
                let Some(current) = pos.pending_terminal_commit.as_mut() else {
                    return;
                };
                if current.action_id != pending.action_id
                    || !matches!(
                        current.het_comparison_write_status,
                        HetComparisonWriteStatusV1::NotAttempted
                    )
                {
                    return;
                }
                let Some(current_prepared) = current.prepared_het_comparison.clone() else {
                    return;
                };
                Self::apply_het_comparison_status_to_terminal_record(
                    &mut current.record,
                    &current_prepared,
                    &write_status,
                );
                current.het_comparison_write_status = write_status;
                pending = current.clone();
            }
        }

        let receipt =
            self.append_shadow_record(&pending.record, !pending.lifecycle_jsonl_committed);
        if !pending.lifecycle_jsonl_committed
            && matches!(receipt.lifecycle_jsonl, TerminalWriteStatus::Ok)
        {
            if let Some(pos) = self.positions.write().get_mut(base_mint) {
                if let Some(current) = pos.pending_terminal_commit.as_mut() {
                    if current.action_id == pending.action_id {
                        current.lifecycle_jsonl_committed = true;
                    }
                }
            }
        }

        if !receipt.canonical_committed() {
            error!(
                base_mint = %base_mint,
                action_id = %pending.action_id,
                lifecycle_jsonl = ?receipt.lifecycle_jsonl,
                canonical_shadow_v2 = ?receipt.canonical_shadow_v2,
                replay_projection = ?receipt.replay_projection,
                "PostBuyGuardian: terminal persistence pending; capacity remains reserved"
            );
            return;
        }

        if !matches!(receipt.replay_projection, TerminalWriteStatus::Ok) {
            warn!(
                base_mint = %base_mint,
                action_id = %pending.action_id,
                replay_projection = ?receipt.replay_projection,
                "PostBuyGuardian: canonical terminal committed with degraded replay projection"
            );
        }

        let removed = {
            let mut positions = self.positions.write();
            let matches_pending = positions.get(base_mint).is_some_and(|pos| {
                pos.pending_terminal_commit
                    .as_ref()
                    .is_some_and(|current| current.action_id == pending.action_id)
            });
            matches_pending
                .then(|| positions.remove(base_mint))
                .flatten()
        };
        let Some(mut pos) = removed else {
            return;
        };
        if let Some(terminal_tx) = pos.terminal_tx.take() {
            let _ = terminal_tx.send(pending.disposition);
        }
        self.cleanup_shadow_runtime_artifacts(base_mint, &pos.position_id)
            .await;
    }

    async fn cleanup_shadow_runtime_artifacts(&self, base_mint: &Pubkey, position_id: &str) {
        if let Some(router) = self.position_router.as_ref() {
            if let Some(shadow_book) = router.shadow_book() {
                let _ = shadow_book.write().await.remove_position(position_id);
            }
        }
        let shadow_backend = { self.shadow_backend.read().clone() };
        if let Some(shadow_backend) = shadow_backend {
            let _ = shadow_backend.unregister_position(position_id).await;
        }
        debug!(
            base_mint = %base_mint,
            position_id,
            "PostBuyGuardian: cleaned non-authoritative shadow runtime mirrors"
        );
    }

    fn emit_shadow_unresolved(
        &self,
        pos: &MonitoredPosition,
        action: &ShadowExitActionHandle,
        reason: ShadowUnresolvedReason,
        recovery_elapsed_ms: u64,
        now_ms: u64,
        evidence: &PriceTruthEvidence,
    ) -> ShadowLifecycleRecord {
        let policy = self.exit_policy_v1.as_ref();
        if let Some(emitter) = self.event_emitter.as_ref() {
            let mut env = emitter.make_envelope_at(&pos.candidate_id, now_ms);
            env.position_id = Some(pos.position_id.clone());
            env.position_epoch = Some(pos.position_epoch);
            env.order_id = Some(format!("shadow-unresolved:{}", action.action_id));
            env.quote_id = Some(pos.quote_id.clone());
            env.slot = evidence.slot.or(pos.slot);
            emitter.emit_raw(ExecutionEvent::new(
                env,
                EventKind::ShadowPositionUnresolved(ShadowPositionUnresolvedPayload {
                    reason,
                    action_id: action.action_id.clone(),
                    policy_id: policy
                        .map(|policy| policy.policy_id().to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    policy_version: policy.map_or(0, EffectiveExitPolicyV1Config::policy_version),
                    policy_config_hash: policy
                        .map(|policy| policy.config_hash().to_string())
                        .unwrap_or_default(),
                    remaining_qty: pos.remaining_token_amount_raw,
                    recovery_elapsed_ms,
                    truth_status: evidence.status,
                    truth_source: evidence.source,
                    truth_slot: evidence.slot,
                    truth_timestamp_ms: evidence.timestamp_ms,
                    truth_age_ms: evidence.age_ms,
                    truth_detail: evidence.detail.clone(),
                    source_snapshot_id: action.source_snapshot_id.clone(),
                    execution_cost_coverage: EXECUTION_COST_COVERAGE_UNMODELED.to_string(),
                    net_pnl_authoritative: false,
                }),
            ));
        }

        let mut record = self.shadow_lifecycle_record_base(
            pos,
            ShadowLifecycleRecordType::PositionUnresolved,
            now_ms,
            evidence,
        );
        record.action_id = Some(action.action_id.clone());
        record.source_snapshot_id = Some(action.source_snapshot_id.clone());
        record.terminal_reason_v2 = Some(reason.terminal_reason_v2());
        record.terminal_disposition = Some("simulation_blocked".to_string());
        record.recovery_elapsed_ms = Some(recovery_elapsed_ms);
        record.remaining_token_amount_raw = Some(pos.remaining_token_amount_raw);
        record.final_pnl = None;
        record.final_pnl_pct = None;
        record.gross_pnl_sol = None;
        record.net_pnl_sol = None;
        record.exit_price = None;
        record.exit_value_sol = None;
        record.exit_token_amount_raw = None;
        record.exit_landed_slot = None;
        record.exit_landed_slot_source = None;
        record
    }

    fn maybe_record_shadow_exit_blocked(
        &self,
        base_mint: &Pubkey,
        now_ms: u64,
        fraction_bps: u16,
        evidence: &PriceTruthEvidence,
    ) {
        let record = {
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
            record
        };
        self.append_shadow_lifecycle_record(&record);
    }

    fn crash_guard_prequote_evidence(
        snapshot: &PostBuyDecisionSnapshot,
        policy: &EffectiveExitPolicyV1Config,
    ) -> PriceTruthEvidence {
        if let Some(sample) = snapshot.crash_vector().latest() {
            let age_ms = snapshot.crash_vector().latest_sample_age_ms();
            let is_stale = age_ms.is_some_and(|age| age > policy.crash_max_sample_age_ms());
            return PriceTruthEvidence {
                source: snapshot.mark_source(),
                status: match age_ms {
                    None => PriceTruthStatus::Failure,
                    Some(_) if is_stale => PriceTruthStatus::Stale,
                    Some(_) => PriceTruthStatus::Resolved,
                },
                detail: match age_ms {
                    None => Some("CrashGuard canonical sample age is unavailable".to_string()),
                    Some(age_ms) if is_stale => Some(format!(
                        "CrashGuard canonical sample is stale: sample_age_ms={age_ms}, max_sample_age_ms={}",
                        policy.crash_max_sample_age_ms()
                    )),
                    Some(_) => None,
                },
                slot: Some(sample.slot()),
                timestamp_ms: Some(sample.timestamp_ms()),
                age_ms,
                price_state: None,
                price_reason: None,
            };
        }
        let (status, detail) = match snapshot.mark_evidence_status() {
            MarkEvidenceStatus::Available => (PriceTruthStatus::Resolved, None),
            MarkEvidenceStatus::Stale => (
                PriceTruthStatus::Stale,
                Some("CrashGuard prequote mark evidence is stale".to_string()),
            ),
            MarkEvidenceStatus::Unavailable => (
                PriceTruthStatus::Failure,
                Some("CrashGuard prequote mark evidence is unavailable".to_string()),
            ),
            MarkEvidenceStatus::Invalid => (
                PriceTruthStatus::SemanticViolation,
                Some("CrashGuard prequote mark evidence is invalid".to_string()),
            ),
        };
        PriceTruthEvidence {
            source: snapshot.mark_source(),
            status,
            detail,
            slot: snapshot.latest_sample_slot(),
            timestamp_ms: snapshot.latest_sample_timestamp_ms(),
            age_ms: snapshot.latest_sample_age_ms(),
            price_state: None,
            price_reason: None,
        }
    }

    fn prequote_decision_label(decision: &PreQuoteDecision) -> String {
        match decision {
            PreQuoteDecision::Hold => "hold".to_string(),
            PreQuoteDecision::UnknownEvidence { reason } => {
                format!("unknown_evidence:{reason:?}").to_lowercase()
            }
            PreQuoteDecision::QuoteRequired { candidate } => {
                format!("full_exit:{}", candidate.reason().as_label())
            }
        }
    }

    fn crash_guard_candidate_label(decision: &CrashGuardPreQuoteDecision) -> String {
        match decision {
            CrashGuardPreQuoteDecision::Disabled => "disabled".to_string(),
            CrashGuardPreQuoteDecision::NotTriggered { reason } => {
                format!("not_triggered:{reason:?}").to_lowercase()
            }
            CrashGuardPreQuoteDecision::QuoteRequired { candidate } => {
                format!("full_exit:{}", candidate.reason().as_label())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn maybe_record_crash_guard_observation(
        &self,
        base_mint: &Pubkey,
        snapshot: &PostBuyDecisionSnapshot,
        state: CrashGuardObservationState,
        not_triggered_reason: Option<CrashGuardNotTriggeredReason>,
        quote_rejection_reason: Option<CrashGuardQuoteRejectionReason>,
        authoritative_decision: &PreQuoteDecision,
        crash_decision: &CrashGuardPreQuoteDecision,
        evidence: &PriceTruthEvidence,
        policy: &EffectiveExitPolicyV1Config,
        now_ms: u64,
    ) {
        if matches!(policy.crash_guard_mode(), CrashGuardMode::Disabled) {
            return;
        }
        let (record, reservation) = {
            let mut positions = self.positions.write();
            let Some(pos) = positions.get_mut(base_mint) else {
                return;
            };
            let evidence_revision = snapshot
                .crash_vector()
                .latest()
                .map(|sample| (sample.slot(), sample.timestamp_ms()));
            let include_evidence_revision =
                !matches!(state, CrashGuardObservationState::NotTriggered);
            if matches!(state, CrashGuardObservationState::Candidate)
                && evidence_revision.is_some()
                && (pos.last_crash_guard_candidate_revision == evidence_revision
                    || pos
                        .pending_crash_guard_observation
                        .as_ref()
                        .and_then(|pending| pending.candidate_revision)
                        == evidence_revision)
            {
                return;
            }
            let observation_key = CrashGuardObservationKey {
                state,
                not_triggered_reason,
                quote_rejection_reason,
                sample_slot: include_evidence_revision
                    .then_some(evidence_revision.map(|(slot, _)| slot))
                    .flatten(),
                sample_timestamp_ms: include_evidence_revision
                    .then_some(evidence_revision.map(|(_, timestamp_ms)| timestamp_ms))
                    .flatten(),
            };
            if pos.last_crash_guard_observation == Some(observation_key) {
                return;
            }
            if pos.pending_crash_guard_observation.is_some() {
                // The append is synchronous but happens outside the position
                // lock. A reservation keeps concurrent ticks from producing
                // two records for the same evidence transition.
                return;
            }
            let reservation = PendingCrashGuardObservation {
                position_id: pos.position_id.clone(),
                position_epoch: pos.position_epoch,
                key: observation_key,
                candidate_revision: matches!(state, CrashGuardObservationState::Candidate)
                    .then_some(evidence_revision)
                    .flatten(),
            };
            pos.pending_crash_guard_observation = Some(reservation.clone());
            let mut record = self.shadow_lifecycle_record_base(
                pos,
                ShadowLifecycleRecordType::CrashGuardObservation,
                now_ms,
                evidence,
            );
            record.crash_guard_mode = Some(policy.crash_guard_mode());
            record.crash_guard_state = Some(state);
            record.crash_guard_not_triggered_reason = not_triggered_reason;
            record.crash_guard_quote_rejection_reason = quote_rejection_reason;
            record.crash_guard_consumed_by_policy = Some(matches!(
                policy.crash_guard_mode(),
                CrashGuardMode::AuthoritativeShadow
            ));
            record.authoritative_decision =
                Some(Self::prequote_decision_label(authoritative_decision));
            record.crash_guard_candidate_decision =
                Some(Self::crash_guard_candidate_label(crash_decision));
            let vector = snapshot.crash_vector();
            record.crash_short_window_drop_pct = vector
                .short_window_drop_fraction()
                .map(|value| value * 100.0);
            record.crash_peak_drawdown_pct =
                vector.peak_drawdown_fraction().map(|value| value * 100.0);
            record.crash_distinct_slots = Some(vector.distinct_slots());
            record.crash_oldest_sample_slot = vector.oldest_in_window().map(CrashSampleV1::slot);
            record.crash_previous_distinct_slot =
                vector.previous_distinct_slot().map(CrashSampleV1::slot);
            record.crash_latest_sample_slot = vector.latest().map(CrashSampleV1::slot);
            record.crash_latest_sample_timestamp_ms =
                vector.latest().map(CrashSampleV1::timestamp_ms);
            (record, reservation)
        };
        let receipt = self.append_shadow_lifecycle_record(&record);
        let lifecycle_jsonl_committed = matches!(receipt.lifecycle_jsonl, TerminalWriteStatus::Ok);
        {
            let mut positions = self.positions.write();
            if let Some(pos) = positions.get_mut(base_mint) {
                let reservation_matches = pos.position_id == reservation.position_id
                    && pos.position_epoch == reservation.position_epoch
                    && pos.pending_crash_guard_observation.as_ref() == Some(&reservation);
                if reservation_matches {
                    pos.pending_crash_guard_observation = None;
                    if lifecycle_jsonl_committed {
                        pos.last_crash_guard_observation = Some(reservation.key);
                        if let Some(candidate_revision) = reservation.candidate_revision {
                            pos.last_crash_guard_candidate_revision = Some(candidate_revision);
                        }
                    }
                }
            }
        }
        if !lifecycle_jsonl_committed {
            warn!(
                base_mint = %base_mint,
                state = ?state,
                lifecycle_jsonl = ?receipt.lifecycle_jsonl,
                "PostBuyGuardian: CrashGuard observation lifecycle evidence append failed"
            );
        }
    }

    // This emitter writes both the runtime execution event and the lifecycle
    // projection, so all identity and truth fields remain explicit.
    #[allow(clippy::too_many_arguments)]
    fn emit_shadow_exit(
        &self,
        base_mint: &Pubkey,
        action_id: &str,
        candidate_id: &str,
        position_id: &str,
        position_epoch: u64,
        quote_id: &str,
        slot: Option<u64>,
        exit: &super::integration::ShadowExitExecution,
        truth: &ShadowExitTruth,
        now_ms: u64,
    ) {
        let exit_order_id = format!("shadow-exit:{action_id}");
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
            exit_sub_env.command_id = Some(action_id.to_string());
            emitter.emit_raw(ExecutionEvent::new(
                exit_sub_env,
                EventKind::ExitSubmitted(ExitSubmittedPayload {
                    fraction_bps: exit.fraction_bps,
                    command_ref: Some(action_id.to_string()),
                }),
            ));

            let mut exit_fill_env = emitter.make_envelope_at(&candidate_id.to_string(), now_ms);
            exit_fill_env.position_id = Some(position_id.to_string());
            exit_fill_env.position_epoch = Some(position_epoch);
            exit_fill_env.order_id = Some(exit_order_id);
            exit_fill_env.quote_id = Some(quote_id.to_string());
            exit_fill_env.slot = slot;
            exit_fill_env.command_id = Some(action_id.to_string());
            emitter.emit_raw(ExecutionEvent::new(
                exit_fill_env,
                EventKind::ExitFilled(ExitFilledPayload {
                    fill_price: truth.exit_price_sol,
                    fill_qty: truth.exit_token_amount_raw,
                    realized_pnl_delta: truth.gross_pnl_sol,
                    status: ExecFillStatus::Filled,
                    is_partial: exit.remaining_fraction_bps > 0,
                    remaining_qty,
                }),
            ));
        }

        if let Some(pos) = self.positions.read().get(base_mint) {
            let mut record = self.shadow_lifecycle_record_base(
                pos,
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

    fn close_reason_from_reason_code_with_default(
        reason_code: Option<&str>,
        default_reason: CloseReason,
    ) -> CloseReason {
        let reason_code = reason_code.unwrap_or_default().to_ascii_lowercase();
        if reason_code.contains("hard_safety") {
            CloseReason::HardSafety
        } else if reason_code.contains("panic") || reason_code.contains("crash_guard") {
            CloseReason::Panic
        } else if reason_code.contains("stop_loss") || reason_code.contains("stop-loss") {
            CloseReason::StopLoss
        } else if reason_code.contains("time_stop")
            || reason_code.contains("time-stop")
            || reason_code.contains("absolute_max_hold")
        {
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
    use crate::guardian::post_buy::shadow_v2::{
        FillStatus, ShadowEntryFillModelConfig, ShadowEntryFillV2, ShadowV2WriteStatus,
        SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
    };
    use crate::guardian::post_buy::shadow_v2_execution::ShadowV2ExecutionLabelGrade;
    use ghost_core::account_state_core::reducer::AccountStateReducer;
    use ghost_core::account_state_core::types::{
        AccountStateUpdate, RpcRefreshResult, UpdateSource,
    };
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

    #[test]
    fn concurrent_lifecycle_jsonl_appends_preserve_record_boundaries() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("shadow_lifecycle.jsonl");
        const WRITER_COUNT: usize = 16;
        const ROWS_PER_WRITER: usize = 32;

        std::thread::scope(|scope| {
            for writer_id in 0..WRITER_COUNT {
                let path = path.clone();
                scope.spawn(move || {
                    for row_id in 0..ROWS_PER_WRITER {
                        let record = serde_json::json!({
                            "writer_id": writer_id,
                            "row_id": row_id,
                            "payload": "x".repeat(8 * 1024),
                        });
                        crate::guardian::post_buy::lifecycle_jsonl::append_jsonl_record(
                            &path, &record,
                        )
                        .expect("serialized lifecycle append");
                    }
                });
            }
        });

        let rows = read_jsonl_rows(&path);
        assert_eq!(rows.len(), WRITER_COUNT * ROWS_PER_WRITER);
        let identities = rows
            .iter()
            .map(|row| {
                (
                    row["writer_id"].as_u64().expect("writer id"),
                    row["row_id"].as_u64().expect("row id"),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(identities.len(), WRITER_COUNT * ROWS_PER_WRITER);
    }

    async fn wait_for_jsonl_rows(path: &Path, minimum_rows: usize) -> Vec<Value> {
        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let rows = read_jsonl_rows(path);
                if rows.len() >= minimum_rows {
                    return rows;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("bounded wait for asynchronous HET sidecar writer")
    }

    async fn wait_for_writer_health<F>(directory: &Path, predicate: F) -> Value
    where
        F: Fn(&Value) -> bool,
    {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(entries) = std::fs::read_dir(directory) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                            continue;
                        };
                        if !name.starts_with("het_pm_v2_writer_health_v1.")
                            || path.extension().and_then(|extension| extension.to_str())
                                != Some("json")
                        {
                            continue;
                        }
                        let Ok(encoded) = std::fs::read_to_string(&path) else {
                            continue;
                        };
                        let Ok(record) = serde_json::from_str::<Value>(&encoded) else {
                            continue;
                        };
                        if predicate(&record) {
                            return record;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("bounded wait for HET-PM V2 writer-health artifact")
    }

    fn terminal_het_source_ref<'a>(terminal: &'a Value, field: &str) -> Option<&'a str> {
        let prefix = format!("het_pm_v2:{field}:");
        terminal["payload"]["record"]["envelope"]["source_refs"]
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .find_map(|value| value.strip_prefix(&prefix))
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

    fn enable_baseline_exit_policy(engine: &mut MonitoringEngine) {
        engine.set_exit_policy_v1_thresholds_for_tests(0.50, 0.50);
    }

    fn pr2_guardian_config(
        absolute_max_hold_enabled: bool,
        crash_guard_mode: CrashGuardMode,
    ) -> PostBuyGuardianConfig {
        PostBuyGuardianConfig {
            enabled: true,
            target_threshold: Some(50.0),
            stoploss_threshold: Some(50.0),
            wait_for_timestop: Some(30_000),
            exit_policy_v1: crate::guardian::post_buy::ExitPolicyV1Config {
                quote_recovery_ms: 5_000,
                absolute_max_hold_enabled,
                absolute_max_hold_ms: 120_000,
                crash_guard_mode,
                crash_window_ms: 1_500,
                crash_min_short_window_drop_pct: 25.0,
                crash_min_peak_drawdown_pct: 30.0,
                crash_min_distinct_slots: 2,
                crash_max_sample_age_ms: 1_500,
                crash_max_executable_return_pct: -20.0,
            },
            aem: crate::aem::config::AemConfig {
                enabled: false,
                ..Default::default()
            },
            ..PostBuyGuardianConfig::default()
        }
    }

    fn enable_terminal_truth_harness(engine: &mut MonitoringEngine, path: &Path) {
        let harness = ShadowV2ValidationHarness::new(shadow_v2_harness_config_for_dir(path))
            .expect("terminal truth harness");
        engine.set_shadow_v2_validation_harness(Arc::new(Mutex::new(harness)));
    }

    struct HetTerminalTestContext {
        engine: Arc<MonitoringEngine>,
        mint: Pubkey,
        terminal_rx: oneshot::Receiver<ShadowTerminalDisposition>,
        snapshot: MarketSnapshot,
        tick_ms: u64,
        sidecar_path: PathBuf,
        lifecycle_path: PathBuf,
        canonical_path: PathBuf,
    }

    fn setup_het_terminal_exit(
        tmp: &TempDir,
        fail_canonical_terminal: bool,
        fail_sidecar: bool,
    ) -> HetTerminalTestContext {
        setup_het_terminal_exit_with_writer(tmp, fail_canonical_terminal, fail_sidecar, None)
    }

    fn setup_het_terminal_exit_with_writer(
        tmp: &TempDir,
        fail_canonical_terminal: bool,
        fail_sidecar: bool,
        stalled_writer_capacity: Option<usize>,
    ) -> HetTerminalTestContext {
        setup_het_terminal_exit_with_writer_and_authority(
            tmp,
            fail_canonical_terminal,
            fail_sidecar,
            stalled_writer_capacity,
            false,
        )
    }

    fn setup_authoritative_het_terminal_exit(tmp: &TempDir) -> HetTerminalTestContext {
        setup_het_terminal_exit_with_writer_and_authority(tmp, false, false, None, true)
    }

    fn setup_het_terminal_exit_with_writer_and_authority(
        tmp: &TempDir,
        fail_canonical_terminal: bool,
        fail_sidecar: bool,
        stalled_writer_capacity: Option<usize>,
        authoritative_v2: bool,
    ) -> HetTerminalTestContext {
        let mut config = pr2_guardian_config(authoritative_v2, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        if authoritative_v2 {
            config.het_pm_v2.mode = super::super::config::HetPmV2Mode::AuthoritativeShadow;
        }
        config.het_pm_v2.terminal_write_budget_ms = if stalled_writer_capacity.is_some() {
            10
        } else {
            100
        };
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid HET terminal test config");
        let lifecycle_path = tmp.path().join("shadow_lifecycle.jsonl");
        let sidecar_path = tmp.path().join("het_pm_v2_observations_v1.jsonl");
        let canonical_path = tmp.path().join("shadow_position_event_v2.jsonl");
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_path.clone()));
        if let Some(capacity) = stalled_writer_capacity {
            engine.set_stalled_het_pm_v2_observation_writer(capacity);
        }
        if fail_canonical_terminal {
            std::fs::create_dir(&canonical_path).expect("canonical writer fault directory");
        }
        if fail_sidecar {
            std::fs::create_dir(&sidecar_path).expect("sidecar writer fault directory");
        }

        let mint = Pubkey::new_unique();
        let registered = engine
            .register_shadow_position_with_terminal(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("het-terminal-boundary".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "het-terminal-boundary".to_string(),
                    entry_order_id: "het-terminal-boundary-entry".to_string(),
                    quote_id: "het-terminal-boundary-quote".to_string(),
                    slot: Some(10),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:het-terminal-boundary".to_string()),
                    position_epoch: Some(11),
                    opened_at_ms: Some(1_000),
                },
            )
            .expect("valid HET terminal test registration");
        if authoritative_v2 {
            // Production promotes this only from AccountStateCore.  The unit
            // harness has no reducer feed, so provide explicit supported-route
            // evidence for tests that exercise the V2 execution path.
            engine
                .positions
                .write()
                .get_mut(&mint)
                .expect("registered authoritative test position")
                .het_route_status = RouteStatusV1::PumpCurveSupported;
        }
        let snapshot = MarketSnapshot {
            slot: Some(11),
            timestamp_ms: 2_000,
            price_sol_per_token: 10.0,
            price_state: PriceState::Valid,
            market_cap_sol: 10.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 10.0,
            ..MarketSnapshot::default()
        };

        HetTerminalTestContext {
            engine: Arc::new(engine),
            mint,
            terminal_rx: registered.terminal_rx,
            snapshot,
            tick_ms: 2_000,
            sidecar_path,
            lifecycle_path,
            canonical_path,
        }
    }

    async fn complete_het_terminal_retry(tmp: &TempDir) -> (HetTerminalTestContext, Value, Value) {
        let context = setup_het_terminal_exit(tmp, true, false);
        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;
        let comparison = read_jsonl_rows(&context.sidecar_path)
            .into_iter()
            .next()
            .expect("pre-canonical HET comparison");
        std::fs::remove_dir(&context.canonical_path).expect("repair canonical writer");
        context
            .engine
            .run_shadow_runtime_tick(
                &context.mint,
                None,
                context
                    .tick_ms
                    .saturating_add(SHADOW_QUOTE_RETRY_INTERVAL_MS),
            )
            .await;
        let terminal = read_jsonl_rows(&context.canonical_path)
            .into_iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("canonical terminal outcome");
        (context, comparison, terminal)
    }

    #[test]
    fn crash_vector_future_sample_is_invalid_evidence_not_monitor_panic() {
        let config = pr2_guardian_config(false, CrashGuardMode::ObserveOnly);
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid crash-guard config");
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "future-crash-sample".to_string(),
                    entry_order_id: "future-crash-entry".to_string(),
                    quote_id: "future-crash-quote".to_string(),
                    slot: Some(10),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:future-crash-sample".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("valid registration");

        let future_sample = MarketSnapshot {
            slot: Some(11),
            timestamp_ms: 2_001,
            price_sol_per_token: 0.5,
            price_state: PriceState::Valid,
            market_cap_sol: 0.5,
            reserve_base: 1_000_000.0,
            reserve_quote: 10.0,
            ..MarketSnapshot::default()
        };
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            pos.snapshot_timeline
                .replace_with(vec![future_sample], 8, 10_000);
        }

        let positions = engine.positions.read();
        let pos = positions.get(&mint).expect("registered position");
        let policy = engine.exit_policy_v1.as_ref().expect("V1 policy");
        let vector = MonitoringEngine::materialize_crash_vector(pos, 2_000, policy);

        assert_eq!(vector.latest_sample_age_ms(), None);
        assert!(!vector.ordering_valid());
    }

    #[test]
    fn shadow_registration_rejects_invalid_immutable_contract_before_open_event() {
        let tmp = TempDir::new().expect("tempdir");
        let events_dir = tmp.path().join("events");
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::new(
            PostBuyGuardianConfig::default(),
            Arc::new(ShadowLedger::new()),
            tx,
        );
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));

        let invalid_price = engine.register_position_with_context(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            None,
            Some(1_000_000),
            Some(1_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "invalid-price".to_string(),
                entry_order_id: "invalid-price-entry".to_string(),
                quote_id: "invalid-price-quote".to_string(),
                slot: Some(1),
                lane: Lane::Shadow,
                position_id: Some("shadow:invalid-price".to_string()),
                position_epoch: Some(1),
                opened_at_ms: Some(1),
            }),
        );
        let invalid_quantity = engine.register_position_with_context(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000),
            Some(0),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: "invalid-quantity".to_string(),
                entry_order_id: "invalid-quantity-entry".to_string(),
                quote_id: "invalid-quantity-quote".to_string(),
                slot: Some(1),
                lane: Lane::Shadow,
                position_id: Some("shadow:invalid-quantity".to_string()),
                position_epoch: Some(1),
                opened_at_ms: Some(1),
            }),
        );
        let invalid_identity = engine.register_position_with_context(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Some(1.0),
            Some(1_000_000),
            Some(1_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata::default(),
                candidate_id: String::new(),
                entry_order_id: "invalid-identity-entry".to_string(),
                quote_id: "invalid-identity-quote".to_string(),
                slot: Some(1),
                lane: Lane::Shadow,
                position_id: Some(String::new()),
                position_epoch: Some(0),
                opened_at_ms: Some(1),
            }),
        );

        assert!(invalid_price.is_none());
        assert!(invalid_quantity.is_none());
        assert!(invalid_identity.is_none());
        assert_eq!(engine.active_position_count(), 0);
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush events");
        assert!(read_event_rows(&events_dir).iter().all(|row| {
            row.pointer("/kind/type") != Some(&Value::String("PositionOpened".to_string()))
        }));
    }

    #[test]
    fn quote_failure_terminal_reason_depends_on_type_not_detail_text() {
        let failure = ExecutableQuoteFailure {
            kind: ExecutableQuoteFailureKind::InternalFailure,
            evidence: PriceTruthEvidence {
                source: PriceTruthSource::ShadowLedgerSnapshot,
                status: PriceTruthStatus::Failure,
                detail: Some("zero no fill no executable output".to_string()),
                slot: None,
                timestamp_ms: None,
                age_ms: None,
                price_state: None,
                price_reason: None,
            },
        };

        assert_eq!(
            failure.kind.unresolved_reason(),
            ShadowUnresolvedReason::Failed
        );
    }

    #[tokio::test]
    async fn administrative_shutdown_removes_all_positions_without_terminal_disposition() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let censor_log = tmp.path().join("position_censored_v1.jsonl");
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, shadow_ledger, tx);
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log));
        enable_baseline_exit_policy(&mut engine);

        let mint = Pubkey::new_unique();
        let registered = engine
            .register_shadow_position_with_terminal(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(0.0000001),
                Some(7_000_000),
                Some(7_000_000_000),
                PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("administrative-shutdown-test".to_string()),
                        ..Default::default()
                    },
                    candidate_id: "administrative-shutdown-candidate".to_string(),
                    entry_order_id: "shadow-entry-administrative-shutdown".to_string(),
                    quote_id: "shadow-quote-administrative-shutdown".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("pool:mint:administrative-shutdown".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1),
                },
            )
            .expect("register shutdown-only position");
        let (_, terminal_rx) = registered.into_parts();

        assert_eq!(engine.active_position_count(), 1);
        assert_eq!(engine.remove_all_positions_administratively(), 1);
        assert_eq!(engine.active_position_count(), 0);
        assert_eq!(engine.remove_all_positions_administratively(), 0);
        assert!(
            terminal_rx.await.is_err(),
            "administrative shutdown must drop the sender without fabricating an economic terminal"
        );
        let censor_rows = read_jsonl_rows(&censor_log);
        assert_eq!(censor_rows.len(), 1);
        assert_eq!(
            censor_rows[0]["artifact_type"],
            POSITION_CENSORED_ARTIFACT_TYPE
        );
        assert_eq!(censor_rows[0]["run_id"], "administrative-shutdown-test");
        assert_eq!(
            censor_rows[0]["position_id"],
            "pool:mint:administrative-shutdown"
        );
        assert_eq!(censor_rows[0]["position_epoch"], 1);
        assert_eq!(censor_rows[0]["reason"], "controlled_runtime_horizon");
        assert_eq!(censor_rows[0]["had_v2_candidate"], false);
        assert_eq!(censor_rows[0]["candidate_gate"], Value::Null);
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
            source_account_pubkey: None,
            source_account_owner_or_program: None,
            account_data_len: None,
            account_data_hash: None,
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

    fn apply_test_canonical_update_with_account_proof(
        account_state_core: &AccountStateReducer,
        mint: Pubkey,
        bonding_curve: Pubkey,
        slot: u64,
        receive_ts_ms: u64,
    ) {
        let source_account_pubkey = Pubkey::new_unique();
        let source_owner = Pubkey::new_unique();
        let apply_result = account_state_core.apply_account_update(AccountStateUpdate {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: mint,
            bonding_curve,
            sol_reserves: 210_000_000_000,
            token_reserves: 760_000_000_000_000,
            is_complete: 0,
            slot,
            write_version: Some(17),
            source_account_pubkey: Some(source_account_pubkey),
            source_account_owner_or_program: Some(source_owner),
            account_data_len: Some(512),
            account_data_hash: Some("test-blake3-account-data-hash".to_string()),
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
    fn unchanged_rpc_refresh_updates_quote_observation_without_vitality_activity() {
        let account_state_core = AccountStateReducer::new();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        apply_test_canonical_update_with_account_proof(
            &account_state_core,
            mint,
            bonding_curve,
            100,
            1_000,
        );

        let canonical = account_state_core
            .get_canonical_state(&mint)
            .expect("canonical state from Geyser update");
        let mut timeline = SnapshotTimeline::default();
        let initial = timeline
            .ingest_canonical_state(&canonical, 8, 60_000)
            .clone();
        let mut activity = ShadowMarketActivityAnchor::from_registration(1_000, Some(&initial));
        let mut vitality = TimeStopV2State::from_registration(Some(&initial));

        let refresh = AccountStateUpdate {
            pool_amm_id: canonical.pool_amm_id,
            base_mint: mint,
            bonding_curve,
            sol_reserves: 210_000_000_000,
            token_reserves: 760_000_000_000_000,
            is_complete: 0,
            // A node may be far ahead of the Geyser event that supplied the
            // canonical state.  This must refresh only the quote boundary.
            slot: 500,
            write_version: Some(0),
            source_account_pubkey: canonical.source_account_pubkey,
            source_account_owner_or_program: canonical.source_account_owner_or_program,
            account_data_len: canonical.account_data_len,
            account_data_hash: canonical.account_data_hash.clone(),
            receive_ts_ms: 4_000,
            receive_seq: 2,
            curve_finality: canonical.curve_finality,
            source: UpdateSource::RpcRefresh,
        };
        assert_eq!(
            account_state_core.apply_rpc_refresh(refresh),
            RpcRefreshResult::ObservationRefreshed
        );

        let refreshed_state = account_state_core
            .get_canonical_state(&mint)
            .expect("refreshed canonical state");
        let refreshed = timeline
            .ingest_canonical_state(&refreshed_state, 8, 60_000)
            .clone();

        assert_eq!(refreshed.timestamp_ms, 4_000);
        assert_eq!(refreshed.tx_count, initial.tx_count);
        assert_eq!(timeline.clone_snapshots().len(), 1);
        assert!(
            !activity.observe_snapshot(&refreshed, 4_000),
            "identical RPC bytes must not become a market-activity heartbeat"
        );
        assert_eq!(activity.last_seen_ms, 1_000);

        let cfg = TimeStopV2Config {
            enabled: true,
            first_check_ms: 3_000,
            window_ms: 3_000,
            ..TimeStopV2Config::default()
        };
        let evaluation = vitality
            .evaluate(
                &cfg,
                1_000,
                Some(initial.price_sol_per_token),
                Some(&refreshed),
                4_000,
            )
            .expect("first scheduled vitality window");
        assert_eq!(evaluation.status, TimeStopV2WindowStatus::Weak);
        assert_eq!(evaluation.subreason, TimeStopV2Subreason::NoNewMarketSample);
        assert_eq!(evaluation.tx_delta_window, Some(0));
        assert_eq!(
            vitality
                .last_checkpoint
                .map(|checkpoint| checkpoint.tx_count),
            Some(initial.tx_count),
            "a quote-only refresh must not advance the vitality checkpoint"
        );
    }

    fn make_shadow_v2_exit_test_engine(
        state_slot: u64,
        state_ts_ms: u64,
    ) -> (MonitoringEngine, Arc<AccountStateReducer>, Pubkey, Pubkey) {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, shadow_ledger, tx);
        engine.set_account_state_core(Arc::clone(&account_state_core));

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        apply_test_canonical_update_with_receive_ts(
            &account_state_core,
            mint,
            bonding_curve,
            state_slot,
            state_ts_ms,
        );
        let registered = engine.register_position_with_context(
            pool,
            mint,
            bonding_curve,
            Some(0.0000001),
            Some(2_000_000_000),
            Some(10_000_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata {
                    run_id: Some("shadow-v2-pr38b-test".to_string()),
                    session_id: Some("session-pr38b-test".to_string()),
                    decision_plane: Some("pr38b-test".to_string()),
                    ..Default::default()
                },
                candidate_id: "candidate-pr38b-test".to_string(),
                entry_order_id: "entry-order-pr38b-test".to_string(),
                quote_id: "quote-pr38b-test".to_string(),
                slot: Some(state_slot.saturating_sub(1)),
                lane: Lane::Shadow,
                position_id: Some("shadow-v2-pr38b-position".to_string()),
                position_epoch: Some(9),
                opened_at_ms: Some(state_ts_ms.saturating_sub(1_000)),
            }),
        );
        assert!(registered.is_some());

        (engine, account_state_core, mint, bonding_curve)
    }

    #[test]
    fn stale_shadow_market_refresh_targets_use_raw_canonical_state_age() {
        let now_ms = current_time_ms();
        let (engine, account_state_core, mint, bonding_curve) =
            make_shadow_v2_exit_test_engine(42, now_ms.saturating_sub(10_000));

        let stale = engine.stale_shadow_market_refresh_targets(now_ms, 1_500);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].base_mint, mint);
        assert_eq!(stale[0].bonding_curve, bonding_curve);

        apply_test_canonical_update_with_receive_ts(
            &account_state_core,
            mint,
            bonding_curve,
            43,
            now_ms,
        );
        assert!(engine
            .stale_shadow_market_refresh_targets(now_ms.saturating_add(1), 1_500)
            .is_empty());
    }

    fn shadow_v2_exit_test_record(
        engine: &MonitoringEngine,
        mint: &Pubkey,
        record_type: ShadowLifecycleRecordType,
        fill_ts_ms: u64,
        fill_slot: Option<u64>,
        exit_token_amount_raw: Option<u64>,
    ) -> ShadowLifecycleRecord {
        let evidence = PriceTruthEvidence {
            source: PriceTruthSource::CanonicalAccountStateSnapshot,
            status: PriceTruthStatus::Resolved,
            detail: None,
            slot: fill_slot,
            timestamp_ms: Some(fill_ts_ms),
            age_ms: Some(0),
            price_state: Some(PriceState::Valid),
            price_reason: None,
        };
        let positions = engine.positions.read();
        let pos = positions.get(mint).expect("registered position");
        let mut record =
            engine.shadow_lifecycle_record_base(pos, record_type, fill_ts_ms, &evidence);
        record.fraction_bps = Some(10_000);
        record.remaining_fraction_bps = 0;
        record.exit_price = Some(0.00000012);
        record.exit_token_amount_raw = exit_token_amount_raw;
        record.exit_sample_slot = fill_slot;
        record.exit_market_anchor_slot = fill_slot;
        record.exit_reason_evaluation_ts_ms = Some(fill_ts_ms);
        record.exit_landed_slot = fill_slot;
        record.exit_landed_slot_source = fill_slot.map(|_| "test_exit_boundary_slot".to_string());
        record.sample_slot = fill_slot;
        record.sample_timestamp_ms = Some(fill_ts_ms);
        record.close_reason = Some(CloseReason::Target);
        record
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

    #[test]
    fn canonical_timeline_refresh_does_not_bypass_peak_and_revision_apply() {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, shadow_ledger, tx);
        engine.set_account_state_core(Arc::clone(&account_state_core));

        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                bonding_curve,
                Some(0.000_000_001),
                Some(1_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "candidate-peak-apply".to_string(),
                    entry_order_id: "entry-peak-apply".to_string(),
                    quote_id: "quote-peak-apply".to_string(),
                    slot: Some(10),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:peak-apply".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("position registration");

        let revision_before = engine
            .positions
            .read()
            .get(&mint)
            .expect("position")
            .state_revision;
        apply_test_canonical_update_with_receive_ts(
            &account_state_core,
            mint,
            bonding_curve,
            11,
            1_100,
        );
        let snapshots = engine
            .snapshots_for_tick(&mint)
            .expect("canonical timeline");
        let latest = snapshots.last().expect("latest canonical snapshot");
        let expected_peak = PriceTruthResolver::normalize_shadow_snapshot_price_sol(latest)
            .expect("normalized canonical mark");
        engine.remember_shadow_snapshot(&mint, latest);

        let positions = engine.positions.read();
        let pos = positions.get(&mint).expect("position retained");
        assert_eq!(pos.peak_since_entry, expected_peak);
        assert!(pos
            .last_shadow_snapshot
            .as_ref()
            .is_some_and(|snapshot| SnapshotTimeline::equivalent(snapshot, latest)));
        assert!(pos.state_revision > revision_before);
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

        shadow_ledger.set_snapshots(mint, vec![snapshot]);
        engine.tick().await;

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

    #[tokio::test]
    async fn shadow_tick_emits_runtime_account_state_path_sample() {
        let tmp = TempDir::new().expect("tempdir");
        let harness = Arc::new(Mutex::new(
            ShadowV2ValidationHarness::new(shadow_v2_harness_config_for_dir(tmp.path()))
                .expect("shadow v2 harness"),
        ));
        let account_state_core = Arc::new(AccountStateReducer::new());
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_shadow_v2_validation_harness(Arc::clone(&harness));
        engine.set_account_state_core(Arc::clone(&account_state_core));
        engine.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            AsyncRwLock::new(ShadowPositionBook::new()),
        ))));
        let engine = Arc::new(engine);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let now_ms = current_time_ms();
        apply_test_canonical_update_with_account_proof(
            &account_state_core,
            mint,
            bonding_curve,
            430_000_120,
            now_ms.saturating_sub(1_000),
        );
        let registered = engine.register_position_with_context(
            pool,
            mint,
            bonding_curve,
            Some(0.0000001),
            Some(7_000_000),
            Some(7_000_000_000),
            Some(PositionEventContext {
                join_metadata: PositionJoinMetadata {
                    run_id: Some("shadow-v2-runtime-path-test".to_string()),
                    session_id: Some("session-runtime-path-test".to_string()),
                    decision_plane: Some("l2-f-runtime-path-test".to_string()),
                    ..Default::default()
                },
                candidate_id: "candidate-runtime-path-test".to_string(),
                entry_order_id: "entry-order-runtime-path-test".to_string(),
                quote_id: "quote-runtime-path-test".to_string(),
                slot: Some(430_000_119),
                lane: Lane::Shadow,
                position_id: Some("shadow-v2-runtime-path-position".to_string()),
                position_epoch: Some(11),
                opened_at_ms: Some(now_ms.saturating_sub(2_000)),
            }),
        );
        assert!(registered.is_some());

        engine.tick().await;

        let canonical_rows = read_jsonl_rows(&tmp.path().join("shadow_position_event_v2.jsonl"));
        let pool_state_row = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "POOL_STATE_SAMPLE")
            .expect("runtime pool state sample row");
        assert_eq!(
            pool_state_row["payload"]["record"]["account_data_hash"],
            "test-blake3-account-data-hash"
        );
        assert_eq!(
            pool_state_row["payload"]["record"]["source_account_slot"],
            430_000_120
        );
        assert_eq!(
            pool_state_row["payload"]["record"]["source_write_version"],
            17
        );

        let path_row = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "PATH_SAMPLE")
            .expect("runtime path sample row");
        let pool_state_ref = path_row["payload"]["record"]["pool_state_ref"]
            .as_str()
            .expect("path pool_state_ref");
        assert!(
            !pool_state_ref.starts_with("MISSING_POOL_STATE_SAMPLE"),
            "runtime path sample must point at a real pool-state sample: {path_row:?}"
        );
        assert_eq!(
            path_row["payload"]["record"]["sampling_reason"],
            "HEARTBEAT"
        );
        assert_eq!(
            path_row["payload"]["record"]["source_quality"],
            "ACCOUNT_STATE_CORE_CANONICAL_STALENESS_MARKED"
        );
    }

    #[tokio::test]
    async fn rug_scalp_data_gap_ends_as_one_pm_owned_data_invalidated_terminal() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_path = tmp.path().join("shadow_lifecycle.jsonl");
        let mut config = PostBuyGuardianConfig::default();
        config.target_threshold = Some(50.0);
        config.stoploss_threshold = Some(50.0);
        config.rug_scalp_exit_v1.enabled = true;
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid RUG PM profile");
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_path.clone()));
        let mint = Pubkey::new_unique();
        let position_id = "rug-scalp-position:data-gap".to_string();
        let registered = engine
            .register_shadow_position_with_terminal(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(0.0001),
                Some(100_000_000),
                Some(1_000_000),
                PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        strategy_id: Some(RUG_SCALP_V2_STRATEGY_ID.to_string()),
                        exit_profile_id: Some(RUG_SCALP_EXIT_PROFILE_ID.to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "rug-scalp-data-gap".to_string(),
                    entry_order_id: "rug-scalp-entry-data-gap".to_string(),
                    quote_id: "rug-scalp-quote-data-gap".to_string(),
                    slot: Some(70),
                    lane: Lane::Shadow,
                    position_id: Some(position_id.clone()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1_000),
                },
            )
            .expect("RUG position registration");
        assert_eq!(
            engine.observe_rug_scalp_market_fact(RugScalpMarketFactV1 {
                position_id: position_id.clone(),
                mint,
                slot: 71,
                tx_index: None,
                event_ordinal: None,
                fact_kind: RugScalpMarketFactKindV1::DataGap,
                successful_buy_count_in_slot: 0,
                sell_quote_lamports: None,
                reserve_before: None,
                reserve_after: None,
                executable_position_value_before: None,
                executable_position_value_after: None,
                data_completeness: RugScalpDataCompletenessV1::Gap,
            }),
            RugScalpFactIngressResultV1::Applied
        );

        let engine = Arc::new(engine);
        engine.run_shadow_runtime_tick(&mint, None, 1_100).await;
        assert_eq!(engine.active_position_count(), 0);
        let (_, terminal_rx) = registered.into_parts();
        assert!(matches!(
            terminal_rx.await,
            Ok(ShadowTerminalDisposition::SimulationBlocked {
                reason: ShadowUnresolvedReason::BlockedByData,
                ..
            })
        ));
        let lifecycle_rows = read_jsonl_rows(&lifecycle_path);
        assert_eq!(
            lifecycle_rows
                .iter()
                .filter(|row| row["record_type"] == "position_unresolved")
                .count(),
            1
        );
        let terminal = lifecycle_rows
            .iter()
            .find(|row| row["record_type"] == "position_unresolved")
            .expect("data-invalidated terminal record");
        assert_eq!(
            terminal["exit_policy_reason_code"],
            "rug_scalp_data_invalidated"
        );
        assert_eq!(terminal["terminal_reason_v2"], "BLOCKED_BY_DATA");
    }

    #[tokio::test]
    async fn rug_scalp_same_slot_replayed_material_sell_closes_once_after_entry_watermark() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_path = tmp.path().join("shadow_lifecycle.jsonl");
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let mut config = PostBuyGuardianConfig::default();
        config.target_threshold = Some(50.0);
        config.stoploss_threshold = Some(50.0);
        config.rug_scalp_exit_v1.enabled = true;
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::try_new(config, Arc::clone(&shadow_ledger), tx)
            .expect("valid RUG PM profile");
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_path.clone()));
        let mint = Pubkey::new_unique();
        let position_id = "rug-scalp-position:replayed-material-sell".to_string();
        let now_ms = current_time_ms();
        let registered = engine
            .register_shadow_position_with_terminal(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(0.000_000_1),
                Some(100_000_000),
                Some(1_000_000),
                PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        strategy_id: Some(RUG_SCALP_V2_STRATEGY_ID.to_string()),
                        exit_profile_id: Some(RUG_SCALP_EXIT_PROFILE_ID.to_string()),
                        rug_scalp_entry_watermark_slot: Some(71),
                        rug_scalp_entry_watermark_tx_index: Some(1),
                        rug_scalp_entry_watermark_event_ordinal: Some(0),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "rug-scalp-replayed-material-sell".to_string(),
                    entry_order_id: "rug-scalp-entry-replayed-material-sell".to_string(),
                    quote_id: "rug-scalp-quote-replayed-material-sell".to_string(),
                    slot: Some(70),
                    lane: Lane::Shadow,
                    position_id: Some(position_id.clone()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(now_ms.saturating_sub(1)),
                },
            )
            .expect("RUG position registration");
        let sell_fact = |tx_index, event_ordinal, reserve_after| RugScalpMarketFactV1 {
            position_id: position_id.clone(),
            mint,
            slot: 71,
            tx_index: Some(tx_index),
            event_ordinal: Some(event_ordinal),
            fact_kind: RugScalpMarketFactKindV1::SuccessfulSell,
            successful_buy_count_in_slot: 0,
            sell_quote_lamports: Some(reserve_after),
            reserve_before: Some(1_000),
            reserve_after: Some(reserve_after),
            executable_position_value_before: Some(1_000),
            executable_position_value_after: Some(reserve_after),
            data_completeness: RugScalpDataCompletenessV1::Complete,
        };
        assert_eq!(
            engine.observe_rug_scalp_market_fact(sell_fact(0, 0, 800)),
            RugScalpFactIngressResultV1::IgnoredPreEntry,
            "same-slot fact before fill watermark cannot become dump evidence"
        );
        assert_eq!(
            engine.observe_rug_scalp_market_fact(sell_fact(2, 0, 800)),
            RugScalpFactIngressResultV1::Applied
        );
        assert_eq!(
            engine.observe_rug_scalp_market_fact(RugScalpMarketFactV1 {
                position_id: position_id.clone(),
                mint,
                slot: 71,
                tx_index: None,
                event_ordinal: None,
                fact_kind: RugScalpMarketFactKindV1::SlotComplete,
                successful_buy_count_in_slot: 0,
                sell_quote_lamports: None,
                reserve_before: None,
                reserve_after: None,
                executable_position_value_before: None,
                executable_position_value_after: None,
                data_completeness: RugScalpDataCompletenessV1::Complete,
            }),
            RugScalpFactIngressResultV1::Applied
        );
        shadow_ledger.set_snapshots(
            mint,
            vec![MarketSnapshot {
                slot: Some(71),
                timestamp_ms: now_ms,
                price_sol_per_token: 0.000_000_1,
                price_state: PriceState::Valid,
                market_cap_sol: 1.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.1,
                ..MarketSnapshot::default()
            }],
        );
        let engine = Arc::new(engine);
        engine.run_shadow_runtime_tick(&mint, None, now_ms).await;
        assert_eq!(engine.active_position_count(), 0);
        let (_, terminal_rx) = registered.into_parts();
        match terminal_rx.await.expect("one PM terminal disposition") {
            ShadowTerminalDisposition::SimulatedClosed { reason, .. } => {
                assert_eq!(reason, "material_sell_emergency");
            }
            other => panic!("expected PM material-sell close, got {other:?}"),
        }
        let lifecycle_rows = read_jsonl_rows(&lifecycle_path);
        assert_eq!(
            lifecycle_rows
                .iter()
                .filter(|row| row["record_type"] == "position_closed")
                .count(),
            1,
            "replay and duplicate direct ingress cannot create a second terminal close"
        );
        let closed = lifecycle_rows
            .iter()
            .find(|row| row["record_type"] == "position_closed")
            .expect("material sell lifecycle");
        assert_eq!(closed["exit_policy_reason_code"], "material_sell_emergency");
    }

    #[tokio::test]
    async fn rug_scalp_two_complete_empty_slots_produce_one_pm_flow_exit_and_close() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_path = tmp.path().join("shadow_lifecycle.jsonl");
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let mut config = PostBuyGuardianConfig::default();
        config.target_threshold = Some(50.0);
        config.stoploss_threshold = Some(50.0);
        config.rug_scalp_exit_v1.enabled = true;
        config.rug_scalp_exit_v1.entry_fixed_cost_lamports = 100;
        config.rug_scalp_exit_v1.exit_fixed_cost_lamports = 200;
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::try_new(config, Arc::clone(&shadow_ledger), tx)
            .expect("valid RUG PM profile");
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_path.clone()));
        let mint = Pubkey::new_unique();
        let position_id = "rug-scalp-position:flow-exit".to_string();
        let now_ms = current_time_ms();
        let registered = engine
            .register_shadow_position_with_terminal(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(0.000_000_1),
                Some(100_000_000),
                Some(1_000_000),
                PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        strategy_id: Some(RUG_SCALP_V2_STRATEGY_ID.to_string()),
                        exit_profile_id: Some(RUG_SCALP_EXIT_PROFILE_ID.to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "rug-scalp-flow-exit".to_string(),
                    entry_order_id: "rug-scalp-entry-flow-exit".to_string(),
                    quote_id: "rug-scalp-quote-flow-exit".to_string(),
                    slot: Some(70),
                    lane: Lane::Shadow,
                    position_id: Some(position_id.clone()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(now_ms.saturating_sub(1)),
                },
            )
            .expect("RUG position registration");
        let complete = |slot| RugScalpMarketFactV1 {
            position_id: position_id.clone(),
            mint,
            slot,
            tx_index: None,
            event_ordinal: None,
            fact_kind: RugScalpMarketFactKindV1::SlotComplete,
            successful_buy_count_in_slot: 0,
            sell_quote_lamports: None,
            reserve_before: None,
            reserve_after: None,
            executable_position_value_before: None,
            executable_position_value_after: None,
            data_completeness: RugScalpDataCompletenessV1::Complete,
        };
        assert_eq!(
            engine.observe_rug_scalp_market_fact(complete(71)),
            RugScalpFactIngressResultV1::Applied
        );
        let second_empty = complete(72);
        assert_eq!(
            engine.observe_rug_scalp_market_fact(second_empty.clone()),
            RugScalpFactIngressResultV1::Applied
        );
        shadow_ledger.set_snapshots(
            mint,
            vec![MarketSnapshot {
                slot: Some(72),
                timestamp_ms: now_ms,
                price_sol_per_token: 0.000_000_1,
                price_state: PriceState::Valid,
                market_cap_sol: 1.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.1,
                ..MarketSnapshot::default()
            }],
        );

        let engine = Arc::new(engine);
        engine.run_shadow_runtime_tick(&mint, None, now_ms).await;
        assert_eq!(engine.active_position_count(), 0);
        let (_, terminal_rx) = registered.into_parts();
        match terminal_rx.await.expect("one PM terminal disposition") {
            ShadowTerminalDisposition::SimulatedClosed {
                reason,
                net_pnl_lamports,
                exit_landed_slot,
                ..
            } => {
                assert_eq!(reason, "flow_exhausted");
                assert_eq!(net_pnl_lamports, Some(-300));
                assert_eq!(exit_landed_slot, Some(73));
            }
            other => panic!("expected PM flow close, got {other:?}"),
        }
        assert_eq!(
            engine.observe_rug_scalp_market_fact(second_empty),
            RugScalpFactIngressResultV1::RejectedUnknownPosition,
            "a duplicate after terminal close cannot start another exit"
        );
        let lifecycle_rows = read_jsonl_rows(&lifecycle_path);
        assert_eq!(
            lifecycle_rows
                .iter()
                .filter(|row| row["record_type"] == "position_closed")
                .count(),
            1
        );
        let closed = lifecycle_rows
            .iter()
            .find(|row| row["record_type"] == "position_closed")
            .expect("flow close lifecycle");
        assert_eq!(closed["exit_policy_reason_code"], "flow_exhausted");
        assert_eq!(closed["net_pnl_sol"], -0.000_000_3);
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

        let evidence = PriceTruthEvidence {
            source: PriceTruthSource::ShadowLedgerSnapshot,
            status: PriceTruthStatus::Stale,
            detail: Some("join metadata projection test".to_string()),
            slot: Some(88),
            timestamp_ms: Some(1_000),
            age_ms: Some(1),
            price_state: Some(PriceState::Valid),
            price_reason: None,
        };
        let record = {
            let positions = engine.positions.read();
            let pos = positions.get(&mint).expect("registered position");
            engine.shadow_lifecycle_record_base(
                pos,
                ShadowLifecycleRecordType::ExitBlocked,
                1_001,
                &evidence,
            )
        };
        engine.append_shadow_lifecycle_record(&record);

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
        assert!(
            row.get("exit_landed_slot").is_none() && row.get("exit_landed_slot_source").is_none(),
            "an ExitBlocked record has evidence provenance but does not represent a simulated fill"
        );
    }

    #[tokio::test]
    async fn shadow_v2_unresolved_emits_terminal_blocked_without_fill_or_close() {
        let tmp = TempDir::new().expect("tempdir");
        let harness = Arc::new(Mutex::new(
            ShadowV2ValidationHarness::new(shadow_v2_harness_config_for_dir(tmp.path()))
                .expect("shadow v2 harness"),
        ));

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        enable_baseline_exit_policy(&mut engine);
        engine.set_shadow_v2_validation_harness(Arc::clone(&harness));
        let operational_lifecycle_path = tmp.path().join("shadow_operational_lifecycle.jsonl");
        engine.set_shadow_lifecycle_log_path(Some(operational_lifecycle_path.clone()));
        let events_dir = tmp.path().join("events");
        let emitter = make_shadow_emitter(&events_dir);
        engine.set_event_emitter(Arc::clone(&emitter));
        let engine = Arc::new(engine);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let opened_at_ms = 1_785_000_200_000;
        let position_id = "shadow-v2-terminal-test-position".to_string();
        let registered = engine
            .register_shadow_position_with_terminal(
                pool,
                mint,
                bonding_curve,
                Some(0.0000001),
                Some(7_000_000),
                Some(7_000_000_000),
                PositionEventContext {
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
                },
            )
            .expect("shadow registration with terminal receiver");

        engine
            .run_shadow_runtime_tick(&mint, None, opened_at_ms + 30_000)
            .await;
        assert_eq!(engine.active_position_count(), 1);
        engine
            .run_shadow_runtime_tick(&mint, None, opened_at_ms + 35_000)
            .await;
        assert_eq!(engine.active_position_count(), 0);
        let (_, terminal_rx) = registered.into_parts();
        let terminal_disposition = terminal_rx
            .await
            .expect("typed shadow terminal disposition");
        let terminal_action_id = match terminal_disposition {
            ShadowTerminalDisposition::SimulationBlocked { action_id, reason } => {
                assert_eq!(reason, ShadowUnresolvedReason::BlockedByData);
                action_id
            }
            other => panic!("expected simulation-blocked disposition, got {other:?}"),
        };
        emitter
            .shared_writer()
            .lock()
            .expect("event writer")
            .flush()
            .expect("flush unresolved event");

        let event_rows = read_event_rows(&events_dir);
        let unresolved_payload = event_rows
            .iter()
            .find_map(|row| {
                let kind = row.get("kind")?.as_object()?;
                (kind.get("type")? == "ShadowPositionUnresolved").then(|| kind.get("payload"))?
            })
            .and_then(Value::as_object)
            .expect("operational unresolved payload");
        assert_eq!(
            unresolved_payload.get("reason"),
            Some(&Value::String("blocked_by_data".to_string()))
        );
        assert_eq!(
            unresolved_payload.get("action_id"),
            Some(&Value::String(terminal_action_id))
        );
        assert_eq!(
            unresolved_payload.get("net_pnl_authoritative"),
            Some(&Value::Bool(false))
        );
        assert!(event_rows.iter().all(|row| {
            row.get("kind")
                .and_then(Value::as_object)
                .and_then(|kind| kind.get("type"))
                .is_none_or(|kind| kind != "ExitFilled" && kind != "PositionClosed")
        }));

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
            !event_kinds.contains(&"EXIT_FILL"),
            "unresolved shadow terminal must not contain EXIT_FILL: {canonical_rows:?}"
        );
        assert!(
            event_kinds.contains(&"TERMINAL_TRUTH"),
            "missing TERMINAL_TRUTH in canonical rows: {canonical_rows:?}"
        );

        let terminal = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("terminal truth row");
        assert_eq!(
            terminal["payload"]["record"]["terminal_reason"],
            "BLOCKED_BY_DATA"
        );
        assert_eq!(
            terminal["payload"]["record"]["final_pnl_executable_bps"],
            serde_json::Value::Null
        );
        assert_eq!(
            terminal["payload"]["record"]["linked_exit_fill"],
            serde_json::Value::Null
        );
        assert_eq!(
            terminal["payload"]["record"]["final_pnl_mark_bps"],
            serde_json::Value::Null
        );
        assert_eq!(terminal["payload"]["record"]["terminal_slot"], Value::Null);
        assert_eq!(
            terminal["payload"]["record"]["terminal_observed_slot"],
            Value::Null
        );
        assert_eq!(terminal["payload"]["record"]["truth_slot"], Value::Null);

        let unresolved_record = read_jsonl_rows(&operational_lifecycle_path)
            .into_iter()
            .find(|row| row["record_type"] == "position_unresolved")
            .expect("operational unresolved record");
        assert_eq!(
            unresolved_record["entry_slot"],
            serde_json::json!(430_000_020_u64)
        );
        assert_eq!(unresolved_record["sample_slot"], Value::Null);
        assert_eq!(unresolved_record["exit_sample_slot"], Value::Null);

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

    #[test]
    fn shadow_v2_terminal_truth_sets_executable_pnl_from_canonical_filled_entry_and_exit() {
        let tmp = TempDir::new().expect("tempdir");
        let harness = Arc::new(Mutex::new(
            ShadowV2ValidationHarness::new(shadow_v2_harness_config_for_dir(tmp.path()))
                .expect("shadow v2 harness"),
        ));
        let state_ts_ms = 1_785_000_300_000;
        let (mut engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_044, state_ts_ms);
        engine.set_shadow_v2_validation_harness(Arc::clone(&harness));

        let exit_record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_045),
            Some(1_000_000_000),
        );
        let mut pool_state = engine
            .shadow_v2_exit_pool_state_sample_from_lifecycle(&exit_record)
            .expect("lifecycle exit pool state");
        pool_state.event_order_key.event_seq_in_process = 1;
        let mut entry_order_key = shadow_v2_event_order_key(
            Some(430_000_045),
            None,
            shadow_v2_event_seq(state_ts_ms + 500, 2),
            state_ts_ms + 500,
        );
        entry_order_key.event_seq_in_process = 2;
        let entry_fill = ShadowEntryFillV2::from_static_buy_model(
            engine.shadow_v2_lifecycle_envelope(
                &exit_record,
                "shadow_entry_fill_v2",
                "shadow_v2_entry_fill:terminal-pnl-test".to_string(),
                TemporalClass::PostEntry,
                ClockDomain::LandingTsMs,
            ),
            entry_order_key,
            &pool_state,
            &ShadowEntryFillModelConfig::bonding_curve(
                7_000_000,
                2_000,
                100,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            ),
        );
        assert_eq!(entry_fill.fill_status, FillStatus::Filled);
        let append_outcome = harness
            .lock()
            .append_record(ShadowV2Record::ShadowEntryFillV2(entry_fill));
        assert_eq!(append_outcome.canonical_write, ShadowV2WriteStatus::Ok);

        engine.append_shadow_v2_lifecycle_record(&exit_record);

        let mut close_record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::PositionClosed,
            state_ts_ms + 1_100,
            Some(430_000_046),
            Some(1_000_000_000),
        );
        close_record.total_exits = 1;
        engine.append_shadow_v2_lifecycle_record(&close_record);

        let canonical_rows = read_jsonl_rows(&tmp.path().join("shadow_position_event_v2.jsonl"));
        let entry_fill_row = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "ENTRY_FILL")
            .expect("entry fill row");
        let exit_fill_row = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "EXIT_FILL")
            .expect("exit fill row");
        assert_eq!(exit_fill_row["payload"]["record"]["fill_status"], "FILLED");

        let terminal = canonical_rows
            .iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("terminal truth row");
        assert!(
            terminal["payload"]["record"]["final_pnl_executable_bps"].is_i64(),
            "terminal executable pnl missing: {terminal:?}"
        );
        assert_eq!(
            terminal["payload"]["record"]["linked_entry_fill"],
            entry_fill_row["envelope"]["event_id"]
        );
        assert_eq!(
            terminal["payload"]["record"]["linked_exit_fill"],
            exit_fill_row["envelope"]["event_id"]
        );
        assert_eq!(
            terminal["payload"]["record"]["reconciliation_status"],
            "TERMINAL_TRUTH_WITH_DIAGNOSTIC_EXECUTABLE_PNL"
        );
        assert!(terminal["envelope"]["limitations"]
            .as_array()
            .expect("terminal limitations")
            .iter()
            .any(
                |value| value == "TERMINAL_EXECUTABLE_PNL_FROM_CANONICAL_ENTRY_EXIT_FILLED_EVENTS"
            ));
    }

    #[test]
    fn shadow_v2_event_order_terminal_truth_marks_chain_components_as_derived() {
        let state_ts_ms = 1_785_000_320_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_044, state_ts_ms);
        let record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::PositionClosed,
            state_ts_ms + 1_100,
            Some(430_000_046),
            Some(1_000_000_000),
        );

        let terminal = engine.shadow_v2_terminal_truth_from_lifecycle(&record, None);

        assert_eq!(
            terminal
                .event_order_key
                .not_applicable_or_derived_chain_order_components(),
            vec![
                "slot:DERIVED".to_string(),
                "block_time:DERIVED".to_string(),
                "signature:DERIVED".to_string(),
                "transaction_index_or_unknown:DERIVED".to_string(),
                "instruction_index_or_unknown:DERIVED".to_string(),
                "inner_instruction_index_or_unknown:DERIVED".to_string(),
                "log_index_or_unknown:DERIVED".to_string(),
            ]
        );
        assert_eq!(terminal.terminal_slot, Some(430_000_046));
        assert!(!terminal.event_order_key.has_complete_chain_order());
        assert!(terminal
            .event_order_key
            .explicit_unknown_chain_order_components()
            .is_empty());
    }

    #[test]
    fn shadow_v2_event_order_lifecycle_source_components_propagate_without_inner_or_log() {
        let state_ts_ms = 1_785_000_321_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_050, state_ts_ms);
        let mut record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_051),
            Some(1_000_000_000),
        );
        record.source_block_time = Some(1_785_000_300);
        record.source_tx_signature = Some("exit-source-signature".to_string());
        record.source_transaction_index = Some(12);
        record.source_instruction_index = Some(5);
        record.exit_market_anchor_tx_signature = Some("legacy-anchor-not-source".to_string());

        let path = engine.shadow_v2_path_sample_from_lifecycle(&record);
        let attempt = engine.shadow_v2_exit_attempt_from_lifecycle(&record);
        let pool_state = engine
            .shadow_v2_exit_pool_state_sample_from_lifecycle(&record)
            .expect("exit pool state");
        let fill = engine.shadow_v2_exit_fill_from_lifecycle(&record, Some(&pool_state));

        for order in [
            &path.event_order_key,
            &attempt.event_order_key,
            &pool_state.event_order_key,
            &fill.event_order_key,
        ] {
            assert_eq!(order.block_time.as_known(), Some(&1_785_000_300));
            assert_eq!(
                order.signature.as_known().map(String::as_str),
                Some("exit-source-signature")
            );
            assert_eq!(order.transaction_index_or_unknown.as_known(), Some(&12));
            assert_eq!(order.instruction_index_or_unknown.as_known(), Some(&5));
            assert!(order.inner_instruction_index_or_unknown.is_unknown());
            assert_eq!(
                order.log_index_or_unknown.non_known_classification(),
                Some("NOT_APPLICABLE")
            );
            assert!(!order.has_complete_chain_order());
        }
    }

    #[test]
    fn shadow_v2_event_order_lifecycle_partial_source_without_signature_stays_unknown() {
        let state_ts_ms = 1_785_000_322_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_060, state_ts_ms);
        let mut record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_061),
            Some(1_000_000_000),
        );
        record.source_block_time = Some(1_785_000_301);
        record.source_transaction_index = Some(13);
        record.source_instruction_index = Some(6);

        let path = engine.shadow_v2_path_sample_from_lifecycle(&record);
        let attempt = engine.shadow_v2_exit_attempt_from_lifecycle(&record);
        let pool_state = engine
            .shadow_v2_exit_pool_state_sample_from_lifecycle(&record)
            .expect("exit pool state");
        let fill = engine.shadow_v2_exit_fill_from_lifecycle(&record, Some(&pool_state));

        for order in [
            &path.event_order_key,
            &attempt.event_order_key,
            &pool_state.event_order_key,
            &fill.event_order_key,
        ] {
            assert!(order.block_time.is_unknown());
            assert!(order.signature.is_unknown());
            assert!(order.transaction_index_or_unknown.is_unknown());
            assert!(order.instruction_index_or_unknown.is_unknown());
            assert!(order.inner_instruction_index_or_unknown.is_unknown());
            assert_eq!(
                order.log_index_or_unknown.non_known_classification(),
                Some("NOT_APPLICABLE")
            );
            assert!(!order.has_complete_chain_order());
        }
    }

    #[test]
    fn shadow_v2_exit_fill_uses_lifecycle_pool_state_sell_engine_when_available() {
        let state_ts_ms = 1_785_000_300_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_044, state_ts_ms);
        let record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_045),
            Some(1_000_000_000),
        );
        let pool_state = engine
            .shadow_v2_exit_pool_state_sample_from_lifecycle(&record)
            .expect("lifecycle exit pool state");

        let fill = engine.shadow_v2_exit_fill_from_lifecycle(&record, Some(&pool_state));

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::DiagnosticSim)
        );
        assert_eq!(fill.research_provenance_ready, Some(false));
        assert_eq!(
            fill.envelope.measurement_grade,
            MeasurementGrade::DiagnosticOnly
        );
        assert_eq!(
            fill.execution_model_version.as_deref(),
            Some(SHADOW_V2_EXIT_FILL_MODEL_VERSION)
        );
        assert!(fill.fill_price.is_some());
        assert!(fill.fill_amount_sol.is_some());
        assert!(fill.fill_amount_tokens.is_some());
        assert_eq!(fill.slippage_tolerance_bps, Some(150));
        assert_eq!(fill.fee_bps, Some(100));
        assert!(fill.own_impact_bps.is_some());
        assert!(fill.pool_state_before.is_some());
        assert!(fill.pool_state_after.is_some());
        assert!(fill.realized_slippage_bps.is_none());
        assert!(fill.quote_fill_divergence_bps.is_none());
        assert!(fill
            .provenance_blockers
            .contains(&"POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME".to_string()));
        assert!(fill
            .envelope
            .limitations
            .contains(&"EXIT_FILL_L1_SELL_MODEL_FROM_LIFECYCLE_EXIT_BOUNDARY".to_string()));
    }

    #[test]
    fn shadow_v2_exit_fill_blocks_without_lifecycle_pool_state() {
        let state_ts_ms = 1_785_000_300_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_044, state_ts_ms);
        let record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_045),
            Some(1_000_000_000),
        );

        let fill = engine.shadow_v2_exit_fill_from_lifecycle(&record, None);

        assert_eq!(fill.fill_status, FillStatus::BlockedByData);
        assert_eq!(fill.execution_simulation_ready, Some(false));
        assert!(fill.pool_state_before.is_none());
        assert!(fill.fill_price.is_none());
        assert!(fill
            .limitations
            .contains(&"EXIT_FILL_POOL_STATE_SAMPLE_MISSING".to_string()));
        assert!(fill
            .limitations
            .contains(&"EXIT_POOL_STATE_BEFORE_UNAVAILABLE".to_string()));
        assert!(fill
            .limitations
            .contains(&"EXIT_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME".to_string()));
    }

    #[test]
    fn shadow_v2_exit_fill_blocks_without_exit_token_amount_raw() {
        let state_ts_ms = 1_785_000_300_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_044, state_ts_ms);
        let record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_045),
            None,
        );
        let pool_state = engine
            .shadow_v2_exit_pool_state_sample_from_lifecycle(&record)
            .expect("lifecycle exit pool state");

        let fill = engine.shadow_v2_exit_fill_from_lifecycle(&record, Some(&pool_state));

        assert_eq!(fill.fill_status, FillStatus::BlockedByData);
        assert_eq!(fill.execution_simulation_ready, Some(false));
        assert!(fill.pool_state_before.is_some());
        assert!(fill.pool_state_after.is_none());
        assert!(fill.fill_price.is_none());
        assert!(fill
            .limitations
            .contains(&"EXIT_FILL_TOKEN_AMOUNT_RAW_UNAVAILABLE".to_string()));
        assert!(!fill
            .envelope
            .limitations
            .contains(&"EXIT_FILL_L1_SELL_MODEL_FROM_LIFECYCLE_EXIT_BOUNDARY".to_string()));
    }

    #[test]
    fn shadow_v2_exit_fill_preserves_same_slot_ordering_provenance_blocker() {
        let state_ts_ms = 1_785_000_300_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_045, state_ts_ms);
        let record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_045),
            Some(1_000_000_000),
        );
        let pool_state = engine
            .shadow_v2_exit_pool_state_sample_from_lifecycle(&record)
            .expect("lifecycle exit pool state");

        let fill = engine.shadow_v2_exit_fill_from_lifecycle(&record, Some(&pool_state));

        assert_eq!(fill.fill_status, FillStatus::Filled);
        assert_eq!(fill.execution_simulation_ready, Some(true));
        assert_eq!(fill.research_provenance_ready, Some(false));
        assert_eq!(
            fill.execution_label_grade,
            Some(ShadowV2ExecutionLabelGrade::DiagnosticSim)
        );
        assert!(fill
            .limitations
            .contains(&"EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string()));
        assert!(fill
            .provenance_blockers
            .contains(&"EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string()));
        assert!(!fill
            .blocked_reasons
            .contains(&"EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS".to_string()));
        assert!(fill.pool_state_after.is_some());
    }

    #[test]
    fn shadow_v2_exit_fill_preserves_future_pool_state_blocker() {
        let state_ts_ms = 1_785_000_300_000;
        let (engine, _account_state_core, mint, _bonding_curve) =
            make_shadow_v2_exit_test_engine(430_000_046, state_ts_ms + 2_000);
        let record = shadow_v2_exit_test_record(
            &engine,
            &mint,
            ShadowLifecycleRecordType::ExitFilled,
            state_ts_ms + 1_000,
            Some(430_000_045),
            Some(1_000_000_000),
        );
        let pool_state = engine
            .shadow_v2_exit_pool_state_sample_from_lifecycle(&record)
            .expect("lifecycle exit pool state");

        let fill = engine.shadow_v2_exit_fill_from_lifecycle(&record, Some(&pool_state));

        assert_eq!(fill.fill_status, FillStatus::BlockedByData);
        assert_eq!(fill.execution_simulation_ready, Some(false));
        assert!(fill
            .limitations
            .contains(&"EXIT_FILL_POOL_STATE_AFTER_FILL_BOUNDARY".to_string()));
        assert!(fill.pool_state_after.is_none());
    }

    #[test]
    fn guarded_apply_rejects_stale_revision_without_mutating_position() {
        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, shadow_ledger, tx);
        enable_baseline_exit_policy(&mut engine);

        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-guarded-stale-apply".to_string(),
                    entry_order_id: "entry-guarded-stale-apply".to_string(),
                    quote_id: "quote-guarded-stale-apply".to_string(),
                    slot: Some(10),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:guarded-stale-apply".to_string()),
                    position_epoch: Some(3),
                    opened_at_ms: Some(100),
                }),
            )
            .expect("shadow registration");

        let market_snapshot = MarketSnapshot {
            slot: Some(11),
            timestamp_ms: 200,
            price_sol_per_token: 2.0,
            price_state: PriceState::Valid,
            reserve_base: 1_000_000.0,
            reserve_quote: 2.0,
            ..MarketSnapshot::default()
        };
        engine.remember_shadow_snapshot(&mint, &market_snapshot);
        let (decision_snapshot, _, _) = engine
            .materialize_post_buy_decision_snapshot(&mint, 200)
            .expect("decision snapshot");
        let candidate = match ExitPolicyV1::evaluate_prequote(
            &decision_snapshot,
            engine.exit_policy_v1.as_ref().expect("exit policy"),
        ) {
            PreQuoteDecision::QuoteRequired { candidate } => candidate,
            other => panic!("expected take-profit candidate, got {other:?}"),
        };
        let action = engine
            .begin_exit_proposal(
                &mint,
                decision_snapshot.guard(),
                &candidate,
                decision_snapshot.snapshot_id(),
                decision_snapshot.inactivity_age_ms(),
                None,
                None,
                200,
            )
            .expect("pending proposal");
        assert!(
            matches!(
                engine.prepare_pending_quote_retry(&mint, decision_snapshot.guard(), 700),
                Err(PositionApplyError::StaleRevision)
            ),
            "a pending retry must not reuse the pre-proposal snapshot guard"
        );

        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("monitored position");
            pos.state_revision = pos.state_revision.saturating_add(1);
        }

        let truth = ShadowExitTruth {
            exit_price_sol: 2.0,
            exit_token_amount_raw: 1_000,
            entry_value_sol: 1.0,
            exit_value_sol: 2.0,
            gross_pnl_sol: 1.0,
            net_pnl_sol: 1.0,
            estimated_costs_sol: 0.0,
            pnl_pct: 100.0,
            evidence: PriceTruthEvidence {
                source: PriceTruthSource::ShadowLedgerSnapshot,
                status: PriceTruthStatus::Resolved,
                detail: None,
                slot: Some(11),
                timestamp_ms: Some(200),
                age_ms: Some(0),
                price_state: Some(PriceState::Valid),
                price_reason: None,
            },
        };
        assert_eq!(
            engine.apply_shadow_quote_outcome(&action, &decision_snapshot, &truth),
            Err(PositionApplyError::StaleRevision)
        );

        let positions = engine.positions.read();
        let pos = positions.get(&mint).expect("position retained");
        assert_eq!(pos.remaining_token_amount_raw, 1_000);
        assert_eq!(pos.total_exits, 0);
        assert!(pos.last_shadow_outcome.is_none());
        assert_eq!(
            pos.pending_exit_proposal
                .as_ref()
                .map(|proposal| proposal.action_id.as_str()),
            Some(action.action_id.as_str())
        );
    }

    #[test]
    fn rejected_authoritative_crash_retargets_the_existing_action_to_baseline_exit() {
        let (tx, _rx) = mpsc::channel(16);
        let engine = MonitoringEngine::new(
            pr2_guardian_config(false, CrashGuardMode::AuthoritativeShadow),
            Arc::new(ShadowLedger::new()),
            tx,
        );
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "crash-fallback".to_string(),
                    entry_order_id: "crash-fallback-entry".to_string(),
                    quote_id: "crash-fallback-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:crash-fallback".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(100),
                }),
            )
            .expect("shadow registration");

        let market_snapshot = MarketSnapshot {
            slot: Some(2),
            timestamp_ms: 200,
            // `price_sol_per_token` is a raw Pump ratio; the resolver
            // normalizes it by the token/lamport decimal ratio.
            price_sol_per_token: 490.0,
            price_state: PriceState::Valid,
            ..MarketSnapshot::default()
        };
        engine.remember_shadow_snapshot(&mint, &market_snapshot);
        let crash_path = [
            MarketSnapshot {
                slot: Some(3),
                timestamp_ms: 100,
                price_sol_per_token: 1.0,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 1.0,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(4),
                timestamp_ms: 150,
                price_sol_per_token: 0.80,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.80,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(5),
                timestamp_ms: 200,
                price_sol_per_token: 0.65,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.65,
                ..MarketSnapshot::default()
            },
        ];
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            pos.snapshot_timeline
                .replace_with(crash_path.to_vec(), 16, 60_000);
            MonitoringEngine::advance_canonical_peak(pos, crash_path.iter());
        }
        let (snapshot, _, _) = engine
            .materialize_post_buy_decision_snapshot(&mint, 200)
            .expect("decision snapshot");
        let crash_requirement = ExitPolicyV1::crash_guard_quote_requirement(&snapshot)
            .expect("CrashGuard candidate provenance");
        let baseline_candidate = match ExitPolicyV1::evaluate_prequote(
            &snapshot,
            engine.exit_policy_v1.as_ref().expect("exit policy"),
        ) {
            PreQuoteDecision::QuoteRequired { candidate } => candidate,
            other => panic!("expected stop-loss baseline candidate, got {other:?}"),
        };
        assert_eq!(baseline_candidate.reason(), ExitCandidateReason::StopLoss);

        let crash_action = engine
            .begin_exit_proposal(
                &mint,
                snapshot.guard(),
                &ExitCandidate::from_reason(ExitCandidateReason::CrashGuard),
                snapshot.snapshot_id(),
                snapshot.inactivity_age_ms(),
                Some(crash_requirement),
                None,
                200,
            )
            .expect("CrashGuard proposal");
        let retargeted = engine
            .retarget_shadow_proposal_after_crash_rejection(
                &crash_action,
                &baseline_candidate,
                snapshot.inactivity_age_ms(),
            )
            .expect("retarget baseline proposal");

        assert_eq!(retargeted.action_id, crash_action.action_id);
        assert_eq!(retargeted.reason, ExitCandidateReason::StopLoss);
        assert_eq!(retargeted.triggered_at_ms, crash_action.triggered_at_ms);
        assert_eq!(
            retargeted.recovery_deadline_ms,
            crash_action.recovery_deadline_ms
        );
        let positions = engine.positions.read();
        let position = positions.get(&mint).expect("retained position");
        let proposal = position
            .pending_exit_proposal
            .as_ref()
            .expect("retargeted pending proposal");
        assert_eq!(proposal.action_id, crash_action.action_id);
        assert_eq!(proposal.reason, ExitCandidateReason::StopLoss);
        assert_eq!(position.next_exit_action_seq, 2);
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
        enable_baseline_exit_policy(&mut engine);
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
        assert_eq!(exit_filled_row["entry_token_amount_raw"], 1_000_000);
        assert_eq!(exit_filled_row["exit_token_amount_raw"], 1_000_000);
        assert_eq!(exit_filled_row["mark_return_pct"], 900.0);
        assert!(exit_filled_row["executable_gross_return_pct"]
            .as_f64()
            .is_some());
        assert_eq!(exit_filled_row["mfe_mark_pct"], 900.0);
        assert_eq!(exit_filled_row["mae_mark_pct"], 900.0);
        assert_eq!(exit_filled_row["quote_reserve_base_raw"], 1_000_000.0);
        assert_eq!(exit_filled_row["quote_reserve_quote_sol"], 10.0);
        assert!(exit_filled_row["quote_own_impact_bps"]
            .as_f64()
            .is_some_and(|impact| impact >= 0.0));
        assert_eq!(
            exit_filled_row["decision_mark_source"],
            "shadow_ledger_snapshot"
        );
        assert_eq!(exit_filled_row["decision_mark_slot"], 99);
        assert_eq!(exit_filled_row["decision_mark_timestamp_ms"], 1_000);
        assert_eq!(exit_filled_row["decision_mark_age_ms"], 0);
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
    async fn exit_policy_v1_take_profit_closes_without_virtual_magazine() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_exit_policy_v1_thresholds_for_tests(0.02, 0.02);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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
    async fn exit_policy_v1_stop_loss_closes_with_stop_loss_reason() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        engine.set_exit_policy_v1_thresholds_for_tests(0.02, 0.02);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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

    #[tokio::test]
    async fn absolute_max_hold_is_independent_of_activity_and_records_legacy_comparison() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::try_new(
            pr2_guardian_config(true, CrashGuardMode::Disabled),
            Arc::clone(&shadow_ledger),
            tx,
        )
        .expect("valid PR2 policy");
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let opened_at_ms = 1_u64;
        assert!(engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-max-hold".to_string(),
                    entry_order_id: "shadow-entry-max-hold".to_string(),
                    quote_id: "shadow-quote-max-hold".to_string(),
                    slot: Some(200),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:max-hold".to_string()),
                    position_epoch: Some(8),
                    opened_at_ms: Some(opened_at_ms),
                }),
            )
            .is_some());

        let trigger_ms = opened_at_ms + 120_000;
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            // A heartbeat just before the decision must not reset absolute
            // age. It does prove this is max-hold rather than inactivity.
            pos.shadow_market_activity.last_seen_ms = trigger_ms;
        }
        let snapshot = MarketSnapshot {
            slot: Some(201),
            timestamp_ms: trigger_ms,
            price_sol_per_token: 1.0,
            price_state: PriceState::Valid,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.0,
            ..MarketSnapshot::default()
        };
        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshot), trigger_ms)
            .await;

        assert_eq!(engine.active_position_count(), 0);
        let rows = read_jsonl_rows(&lifecycle_log);
        let closed = rows
            .iter()
            .find(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
            })
            .expect("max-hold close lifecycle row");
        assert_eq!(closed["exit_policy_reason_code"], "absolute_max_hold");
        assert_eq!(closed["close_reason"], "TimeStop");
        assert_eq!(closed["absolute_age_ms"], 120_000);
        assert_eq!(closed["inactivity_age_ms"], 0);
        assert_eq!(closed["capacity_occupancy_age_ms"], 120_000);
        assert_eq!(
            closed["would_hold_under_legacy_inactivity_policy"],
            Value::Bool(true)
        );
    }

    #[tokio::test]
    async fn observe_only_crash_guard_is_logged_once_per_evidence_revision_without_lifecycle_mutation(
    ) {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::try_new(
            pr2_guardian_config(false, CrashGuardMode::ObserveOnly),
            Arc::clone(&shadow_ledger),
            tx,
        )
        .expect("valid observe-only CrashGuard policy");
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        assert!(engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-crash-observe".to_string(),
                    entry_order_id: "shadow-entry-crash-observe".to_string(),
                    quote_id: "shadow-quote-crash-observe".to_string(),
                    slot: Some(300),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:crash-observe".to_string()),
                    position_epoch: Some(9),
                    opened_at_ms: Some(9_000),
                }),
            )
            .is_some());

        let snapshots = [
            MarketSnapshot {
                slot: Some(301),
                timestamp_ms: 10_000,
                price_sol_per_token: 1.0,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 1.0,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(302),
                timestamp_ms: 10_500,
                price_sol_per_token: 0.80,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.80,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(303),
                timestamp_ms: 11_000,
                price_sol_per_token: 0.70,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.70,
                ..MarketSnapshot::default()
            },
        ];
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            pos.snapshot_timeline
                .replace_with(snapshots.to_vec(), 16, 60_000);
            MonitoringEngine::advance_canonical_peak(pos, snapshots.iter());
        }

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshots[2]), 11_000)
            .await;
        // Identical evidence on a second tick must not produce a lifecycle
        // action or duplicate diagnostic transitions.
        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshots[2]), 11_000)
            .await;

        assert_eq!(engine.active_position_count(), 1);
        let rows = read_jsonl_rows(&lifecycle_log);
        let crash_rows: Vec<_> = rows
            .iter()
            .filter(|row| {
                row.get("record_type")
                    == Some(&Value::String("crash_guard_observation".to_string()))
            })
            .collect();
        assert_eq!(crash_rows.len(), 2, "candidate then confirmed once");
        assert!(crash_rows
            .iter()
            .all(|row| { row.get("crash_guard_consumed_by_policy") == Some(&Value::Bool(false)) }));
        assert!(crash_rows.iter().any(|row| {
            row.get("crash_guard_state") == Some(&Value::String("candidate".to_string()))
        }));
        assert!(crash_rows.iter().any(|row| {
            row.get("crash_guard_state") == Some(&Value::String("confirmed".to_string()))
        }));
        assert!(rows.iter().all(|row| {
            row.get("record_type") != Some(&Value::String("exit_filled".to_string()))
                && row.get("record_type") != Some(&Value::String("position_closed".to_string()))
        }));
        assert!(crash_rows.iter().all(|row| {
            row.get("exit_landed_slot").is_none() && row.get("exit_landed_slot_source").is_none()
        }));
    }

    #[tokio::test]
    async fn crash_guard_observation_retries_after_lifecycle_append_failure() {
        let tmp = TempDir::new().expect("tempdir");
        let failed_lifecycle_path = tmp.path().join("lifecycle-directory");
        std::fs::create_dir(&failed_lifecycle_path).expect("fault-injection directory");
        let recovered_lifecycle_path = tmp.path().join("shadow_lifecycle.jsonl");
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::try_new(
            pr2_guardian_config(false, CrashGuardMode::ObserveOnly),
            Arc::clone(&shadow_ledger),
            tx,
        )
        .expect("valid observe-only CrashGuard policy");
        engine.set_shadow_lifecycle_log_path(Some(failed_lifecycle_path));

        let mint = Pubkey::new_unique();
        assert!(engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-crash-append-retry".to_string(),
                    entry_order_id: "shadow-entry-crash-append-retry".to_string(),
                    quote_id: "shadow-quote-crash-append-retry".to_string(),
                    slot: Some(300),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:crash-append-retry".to_string()),
                    position_epoch: Some(13),
                    opened_at_ms: Some(9_000),
                }),
            )
            .is_some());

        let snapshots = [
            MarketSnapshot {
                slot: Some(301),
                timestamp_ms: 10_000,
                price_sol_per_token: 1.0,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 1.0,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(302),
                timestamp_ms: 10_500,
                price_sol_per_token: 0.80,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.80,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(303),
                timestamp_ms: 11_000,
                price_sol_per_token: 0.70,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.70,
                ..MarketSnapshot::default()
            },
        ];
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            pos.snapshot_timeline
                .replace_with(snapshots.to_vec(), 16, 60_000);
            MonitoringEngine::advance_canonical_peak(pos, snapshots.iter());
        }

        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshots[2]), 11_000)
            .await;
        {
            let positions = engine.positions.read();
            let pos = positions.get(&mint).expect("position remains monitored");
            assert!(
                pos.last_crash_guard_observation.is_none(),
                "failed append must not commit observation dedupe state"
            );
            assert!(
                pos.last_crash_guard_candidate_revision.is_none(),
                "failed append must not commit candidate dedupe state"
            );
            assert!(pos.pending_crash_guard_observation.is_none());
        }

        engine.set_shadow_lifecycle_log_path(Some(recovered_lifecycle_path.clone()));
        engine
            .run_shadow_runtime_tick(&mint, Some(&snapshots[2]), 11_000)
            .await;

        let rows = read_jsonl_rows(&recovered_lifecycle_path);
        let crash_rows: Vec<_> = rows
            .iter()
            .filter(|row| {
                row.get("record_type")
                    == Some(&Value::String("crash_guard_observation".to_string()))
            })
            .collect();
        assert_eq!(
            crash_rows.len(),
            2,
            "candidate and confirmed observation must retry after a transient lifecycle write failure"
        );
        assert!(crash_rows.iter().any(|row| {
            row.get("crash_guard_state") == Some(&Value::String("candidate".to_string()))
        }));
        assert!(crash_rows.iter().any(|row| {
            row.get("crash_guard_state") == Some(&Value::String("confirmed".to_string()))
        }));
    }

    #[tokio::test]
    async fn authoritative_crash_guard_retry_revalidates_original_candidate_quote() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::try_new(
            pr2_guardian_config(false, CrashGuardMode::AuthoritativeShadow),
            Arc::clone(&shadow_ledger),
            tx,
        )
        .expect("valid authoritative CrashGuard policy");
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));

        let mint = Pubkey::new_unique();
        assert!(engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                // Keep the full position tiny relative to the recovered
                // curve so its executable return is genuinely about -10%,
                // rather than a fixture artifact dominated by own impact.
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-crash-retry".to_string(),
                    entry_order_id: "shadow-entry-crash-retry".to_string(),
                    quote_id: "shadow-quote-crash-retry".to_string(),
                    slot: Some(300),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:crash-retry".to_string()),
                    position_epoch: Some(12),
                    opened_at_ms: Some(9_000),
                }),
            )
            .is_some());

        let candidate_path = [
            MarketSnapshot {
                slot: Some(301),
                timestamp_ms: 10_000,
                price_sol_per_token: 1.0,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 1.0,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(302),
                timestamp_ms: 10_500,
                price_sol_per_token: 0.80,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.80,
                ..MarketSnapshot::default()
            },
            // Mark evidence remains valid through the raw price fallback, but
            // the curve cannot materialize for a position-sized quote.
            MarketSnapshot {
                slot: Some(303),
                timestamp_ms: 11_000,
                price_sol_per_token: 650.0,
                price_state: PriceState::Valid,
                reserve_base: 0.0,
                reserve_quote: 0.65,
                ..MarketSnapshot::default()
            },
        ];
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            pos.snapshot_timeline
                .replace_with(candidate_path.to_vec(), 16, 60_000);
            MonitoringEngine::advance_canonical_peak(pos, candidate_path.iter());
        }

        engine
            .run_shadow_runtime_tick(&mint, Some(&candidate_path[2]), 11_000)
            .await;
        {
            let positions = engine.positions.read();
            let pos = positions.get(&mint).expect("pending position");
            let pending = pos.pending_exit_proposal.as_ref().unwrap_or_else(|| {
                panic!(
                    "CrashGuard proposal was not retained after quote failure: outcome={:?} terminal_commit={:?} revision={}",
                    pos.last_shadow_outcome,
                    pos.pending_terminal_commit,
                    pos.state_revision,
                )
            });
            assert_eq!(pending.reason, ExitCandidateReason::CrashGuard);
            assert!(pending.crash_guard_quote_requirement.is_some());
            assert!(pos.last_shadow_outcome.is_none());
        }

        let recovered = MarketSnapshot {
            slot: Some(304),
            timestamp_ms: 11_500,
            price_sol_per_token: 0.90,
            price_state: PriceState::Valid,
            reserve_base: 1_000_000_000.0,
            // 1_000_000_000 raw units are 1_000 tokens. Keep the reserve
            // ratio at 0.90 SOL/token while making the tiny position's own
            // impact negligible.
            reserve_quote: 900.0,
            ..MarketSnapshot::default()
        };
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("pending position");
            let mut recovered_timeline = candidate_path[..2].to_vec();
            recovered_timeline.push(recovered.clone());
            pos.snapshot_timeline
                .replace_with(recovered_timeline, 16, 60_000);
        }

        engine
            .run_shadow_runtime_tick(&mint, Some(&recovered), 11_500)
            .await;

        let positions = engine.positions.read();
        let pos = positions.get(&mint).expect("position stays open");
        assert!(pos.pending_exit_proposal.is_none());
        assert!(
            pos.last_shadow_outcome.is_none(),
            "recovered non-severe quote must cancel CrashGuard instead of terminalizing: {:?}",
            pos.last_shadow_outcome
        );
        assert_eq!(pos.remaining_token_amount_raw, 1_000);
        drop(positions);

        let rows = read_jsonl_rows(&lifecycle_log);
        assert!(rows.iter().any(|row| {
            row.get("record_type") == Some(&Value::String("crash_guard_observation".to_string()))
                && row.get("crash_guard_state")
                    == Some(&Value::String("rejected_by_quote".to_string()))
                && row.get("crash_guard_quote_rejection_reason")
                    == Some(&Value::String(
                        "executable_return_not_severe_enough".to_string(),
                    ))
        }));
        assert!(rows.iter().all(|row| {
            row.get("record_type") != Some(&Value::String("exit_filled".to_string()))
                && row.get("record_type") != Some(&Value::String("position_closed".to_string()))
        }));
    }

    #[tokio::test]
    async fn observe_only_crash_guard_does_not_change_baseline_exit_quote_source() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::try_new(
            pr2_guardian_config(false, CrashGuardMode::ObserveOnly),
            Arc::clone(&shadow_ledger),
            tx,
        )
        .expect("valid observe-only CrashGuard policy");
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        assert!(engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "cand-crash-baseline".to_string(),
                    entry_order_id: "shadow-entry-crash-baseline".to_string(),
                    quote_id: "shadow-quote-crash-baseline".to_string(),
                    slot: Some(300),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:crash-baseline".to_string()),
                    position_epoch: Some(10),
                    opened_at_ms: Some(9_000),
                }),
            )
            .is_some());

        // The bounded canonical timeline proves the CrashGuard candidate at
        // 0.40. The PR1 baseline is independently triggered by the current
        // runtime projection at 0.45. Observe-only CrashGuard must reuse the
        // baseline quote rather than silently changing its fill economics.
        let crash_path = [
            MarketSnapshot {
                slot: Some(301),
                timestamp_ms: 10_000,
                price_sol_per_token: 1.0,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 1.0,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(302),
                timestamp_ms: 10_500,
                price_sol_per_token: 0.80,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.80,
                ..MarketSnapshot::default()
            },
            MarketSnapshot {
                slot: Some(303),
                timestamp_ms: 11_000,
                price_sol_per_token: 0.40,
                price_state: PriceState::Valid,
                reserve_base: 1_000_000.0,
                reserve_quote: 0.40,
                ..MarketSnapshot::default()
            },
        ];
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            pos.snapshot_timeline
                .replace_with(crash_path.to_vec(), 16, 60_000);
            MonitoringEngine::advance_canonical_peak(pos, crash_path.iter());
        }
        let baseline_runtime_snapshot = MarketSnapshot {
            slot: Some(304),
            timestamp_ms: 11_100,
            price_sol_per_token: 0.45,
            price_state: PriceState::Valid,
            reserve_base: 1_000_000.0,
            reserve_quote: 0.45,
            ..MarketSnapshot::default()
        };

        engine
            .run_shadow_runtime_tick(&mint, Some(&baseline_runtime_snapshot), 11_100)
            .await;

        assert_eq!(engine.active_position_count(), 0);
        let rows = read_jsonl_rows(&lifecycle_log);
        let filled = rows
            .iter()
            .find(|row| row.get("record_type") == Some(&Value::String("exit_filled".to_string())))
            .expect("baseline exit fill");
        assert_eq!(filled["exit_policy_reason_code"], "stop_loss");
        // `exit_price` is the position-sized executable price, not the mark.
        // The evidence slot proves that the baseline action kept the current
        // runtime projection (304) instead of borrowing CrashGuard's raw
        // candidate sample (303).
        assert_eq!(filled["exit_sample_slot"], 304);
        assert_ne!(filled["exit_sample_slot"], 303);
        assert!(rows.iter().any(|row| {
            row.get("record_type") == Some(&Value::String("crash_guard_observation".to_string()))
                && row.get("crash_guard_state") == Some(&Value::String("confirmed".to_string()))
                && row.get("crash_guard_consumed_by_policy") == Some(&Value::Bool(false))
        }));
    }

    #[test]
    fn effective_exit_policy_allows_target_above_100_percent() {
        let policy = EffectiveExitPolicyV1Config::new(1.5, 0.5, 30_000, 5_000)
            .expect("valid effective policy");

        assert_eq!(policy.take_profit_fraction(), 1.5);
        assert_eq!(policy.stop_loss_fraction(), 0.5);
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

    #[test]
    fn invalid_het_config_fails_every_non_test_constructor() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = false;
        let (tx, _rx) = mpsc::channel(4);

        assert!(matches!(
            MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx),
            Err(MonitoringEngineConfigError::HetPmV2(
                HetPmV2ConfigError::VitalitySourceDisabled
            ))
        ));
    }

    #[test]
    fn authoritative_shadow_is_an_explicit_valid_runtime_mode() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        config.het_pm_v2.mode = super::super::config::HetPmV2Mode::AuthoritativeShadow;
        let (tx, _rx) = mpsc::channel(4);

        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("authoritative shadow configuration is valid");
        let status = engine
            .het_pm_v2
            .as_ref()
            .expect("authoritative V2 policy is installed")
            .status(CrashGuardMode::Disabled);
        assert!(status.v2_shadow_authority);
        assert!(!status.v1_shadow_authority);
        assert!(!status.live_authority);
    }

    #[test]
    fn invalid_het_config_cannot_disable_v1_snapshot_or_authority() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = false;
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::new(config, Arc::new(ShadowLedger::new()), tx);
        assert!(engine.exit_policy_v1_status().is_some());
        assert!(engine.het_pm_v2_status().is_none());

        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(100),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "invalid-het-v1-intact".to_string(),
                    entry_order_id: "invalid-het-v1-intact-entry".to_string(),
                    quote_id: "invalid-het-v1-intact-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:invalid-het-v1-intact".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("V1 position registration remains available");

        assert!(engine
            .materialize_post_buy_decision_snapshot(&mint, 1_500)
            .is_some());
        assert!(engine
            .materialize_post_buy_snapshot_bundle(&mint, 1_500)
            .is_none());
    }

    #[test]
    fn route_defaults_to_unknown_without_canonical_account_state_evidence() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid HET config");
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(100),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "route-unknown".to_string(),
                    entry_order_id: "route-unknown-entry".to_string(),
                    quote_id: "route-unknown-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:route-unknown".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("shadow registration");

        assert_eq!(
            engine
                .positions
                .read()
                .get(&mint)
                .expect("registered position")
                .het_route_status,
            RouteStatusV1::Unknown
        );
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
    fn active_profile_produces_vitality_windows_and_max_hold_remains_reachable() {
        let mut config = pr2_guardian_config(true, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        // The TimeStop fixture carries synthetic reserve magnitudes used by
        // its vitality windows. Keep TP out of this gate-reachability test.
        config.target_threshold = Some(1_000_000.0);
        // Keep the legacy inactivity gate later than AbsoluteMaxHold so this
        // test isolates reachability of the V2 hierarchy's max-hold gate.
        config.wait_for_timestop = Some(180_000);
        config.het_pm_v2.vitality_min_age_ms = 11_000;
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("active HET profile requires a valid vitality source");

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

        let samples = [
            time_stop_v2_test_snapshot(2, 106_000, 1.04, 104.0, 11.0, 2, 0.2),
            time_stop_v2_test_snapshot(3, 116_000, 1.08, 108.0, 12.0, 4, 0.4),
            time_stop_v2_test_snapshot(4, 119_500, 1.12, 112.0, 13.0, 6, 0.6),
            time_stop_v2_test_snapshot(5, 120_500, 1.16, 116.0, 14.0, 8, 0.8),
            time_stop_v2_test_snapshot(6, 121_001, 1.20, 120.0, 15.0, 10, 1.0),
        ];
        for sample in &samples {
            engine.remember_shadow_snapshot(&mint, sample);
            engine.evaluate_time_stop_v2_observe_only(&mint, Some(sample), sample.timestamp_ms);
        }
        let mut trajectory_samples = vec![initial.clone()];
        trajectory_samples.extend(samples.iter().cloned());
        let history_max = engine.snapshot_history_max_snapshots();
        let history_retention_ms = engine.snapshot_history_retention_ms();
        {
            let mut positions = engine.positions.write();
            let pos = positions.get_mut(&mint).expect("registered position");
            pos.snapshot_timeline.replace_with(
                trajectory_samples,
                history_max,
                history_retention_ms,
            );
            // Test-injected explicit route evidence; production promotion is
            // performed only by AccountStateCore refresh.
            pos.het_route_status = RouteStatusV1::PumpCurveSupported;
        }

        let (bundle, _, _, _) = engine
            .materialize_post_buy_snapshot_bundle(&mint, 121_001)
            .expect("active HET bundle");
        assert!(bundle.v2.vitality.quality_fresh);
        assert_eq!(
            bundle.v2.vitality.current_state,
            super::super::exit_policy_v2::VitalityStateV1::Alive
        );
        assert!(bundle.v2.vitality.last_window_at_ms.is_some());
        assert!(
            matches!(
                bundle.v2.trajectory.quality,
                super::super::trajectory_v1::TrajectoryQualityV1::PartialHistory
                    | super::super::trajectory_v1::TrajectoryQualityV1::Usable
            ),
            "max-hold fixture requires admissible trajectory evidence, got {:?}",
            bundle.v2.trajectory.quality
        );

        let v1_policy = engine.exit_policy_v1.as_ref().expect("V1 policy");
        let het_policy = engine.het_pm_v2.as_ref().expect("HET policy");
        let v1_prequote = ExitPolicyV1::evaluate_prequote(&bundle.base, v1_policy);
        assert!(
            matches!(
                &v1_prequote,
                PreQuoteDecision::QuoteRequired { candidate }
                    if candidate.reason() == ExitCandidateReason::AbsoluteMaxHold
            ),
            "expected AbsoluteMaxHold after fresh vitality windows, got {v1_prequote:?}"
        );
        let v2_prequote = ExitPolicyV2::evaluate_prequote(
            bundle.view(),
            &v1_prequote,
            &CrashGuardPreQuoteDecision::Disabled,
            het_policy,
        );
        assert_eq!(
            v2_prequote.winning_gate,
            super::super::exit_policy_v2::HetPmGateV2::AbsoluteMaxHold,
            "expected max-hold after fresh vitality/trajectory evidence, got {:?}",
            v2_prequote.candidate
        );
    }

    #[test]
    fn het_enabled_and_disabled_preserve_time_stop_state_and_rows() {
        let tmp = TempDir::new().expect("tempdir");
        let off_log = tmp.path().join("off.jsonl");
        let on_log = tmp.path().join("on.jsonl");
        let mut base_config = PostBuyGuardianConfig::default();
        base_config.time_stop_v2.enabled = true;
        base_config.time_stop_v2.first_check_ms = 3_000;
        base_config.time_stop_v2.window_ms = 4_000;
        base_config.time_stop_v2.failed_windows_to_signal = 3;
        base_config.time_stop_v2.min_age_before_signal_ms = 11_000;

        let mut off_config = base_config.clone();
        off_config.het_pm_v2.enabled = false;
        let mut on_config = base_config;
        on_config.het_pm_v2.enabled = true;
        let (off_tx, _off_rx) = mpsc::channel(4);
        let (on_tx, _on_rx) = mpsc::channel(4);
        let mut off = MonitoringEngine::new(off_config, Arc::new(ShadowLedger::new()), off_tx);
        let mut on = MonitoringEngine::new(on_config, Arc::new(ShadowLedger::new()), on_tx);
        off.set_shadow_lifecycle_log_path(Some(off_log.clone()));
        on.set_shadow_lifecycle_log_path(Some(on_log.clone()));

        let mint = Pubkey::new_unique();
        let opened_at_ms = 1_000;
        let initial = time_stop_v2_test_snapshot(1, opened_at_ms, 1.0, 100.0, 10.0, 0, 0.0);
        register_time_stop_v2_shadow_position(
            &off,
            mint,
            opened_at_ms,
            &initial,
            PositionJoinMetadata::default(),
        );
        register_time_stop_v2_shadow_position(
            &on,
            mint,
            opened_at_ms,
            &initial,
            PositionJoinMetadata::default(),
        );

        for snapshot in [
            time_stop_v2_test_snapshot(2, 4_000, 1.001, 100.05, 10.01, 1, 0.01),
            time_stop_v2_test_snapshot(3, 8_000, 1.002, 100.10, 10.02, 2, 0.02),
            time_stop_v2_test_snapshot(4, 12_000, 1.003, 100.15, 10.03, 3, 0.03),
        ] {
            off.evaluate_time_stop_v2_observe_only(&mint, Some(&snapshot), snapshot.timestamp_ms);
            on.evaluate_time_stop_v2_observe_only(&mint, Some(&snapshot), snapshot.timestamp_ms);
        }

        let state_contract = |engine: &MonitoringEngine| {
            let positions = engine.positions.read();
            let state = &positions.get(&mint).expect("position").time_stop_v2;
            (
                state.next_window_index,
                state.failed_windows,
                state.last_status,
                state.last_subreason,
                state.candidate_emitted,
                state.candidate_ts_ms,
                state.candidate_subreason,
                state.source_window_index,
                state.source_checkpoint_slot,
                state.source_latest_slot,
            )
        };
        assert_eq!(state_contract(&off), state_contract(&on));

        let row_contract = |path: &Path| {
            read_jsonl_rows(path)
                .into_iter()
                .map(|row| {
                    (
                        row["time_stop_v2_window_index"].clone(),
                        row["time_stop_v2_status"].clone(),
                        row["time_stop_v2_subreason"].clone(),
                        row["time_stop_v2_failed_windows"].clone(),
                        row["time_stop_v2_candidate"].clone(),
                        row["time_stop_v2_tx_delta_window"].clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(row_contract(&off_log), row_contract(&on_log));
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
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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
    async fn expired_virtual_bullets_do_not_override_exit_policy_v1() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        enable_baseline_exit_policy(&mut engine);
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

        let now_ms = registered.opened_at_ms + 1_000;
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

        assert_eq!(engine.active_position_count(), 1);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        let candidate_rows: Vec<_> = lifecycle_rows
            .iter()
            .filter(|row| row.get("candidate_id") == Some(&Value::String(candidate_id.clone())))
            .collect();
        assert!(
            !candidate_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_closed".to_string()))
            }),
            "non-authoritative virtual bullets must not close the canonical position: {candidate_rows:?}"
        );

        let event_rows = read_event_rows(&events_dir);
        assert!(event_rows.iter().all(|row| {
            row.get("kind")
                .and_then(Value::as_object)
                .and_then(|kind| kind.get("type"))
                != Some(&Value::String("PositionClosed".to_string()))
        }));
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
            tx_count: 1,
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
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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
        assert_eq!(engine.active_position_count(), 1);
        engine
            .run_shadow_runtime_tick(&mint, None, now_ms + 5_000)
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
            lifecycle_rows.iter().all(
                |row| row.get("record_type") != Some(&Value::String("exit_filled".to_string()))
            ),
            "cached snapshot fallback must not emit exit_filled rows: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("exit_blocked".to_string()))
                    && row.get("truth_status") == Some(&Value::String("stale".to_string()))
                    && row
                        .get("truth_detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains("exceeded stale_after_ms=1500"))
            }),
            "missing cache-reject exit_blocked proof: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("record_type") == Some(&Value::String("position_unresolved".to_string()))
                    && row.get("terminal_reason_v2")
                        == Some(&Value::String("BLOCKED_BY_DATA".to_string()))
                    && row.get("truth_status") == Some(&Value::String("stale".to_string()))
            }),
            "missing cache-reject unresolved proof: {lifecycle_rows:?}"
        );

        let event_rows = read_event_rows(&events_dir);
        assert!(event_rows.iter().any(|row| {
            row.get("kind")
                .and_then(Value::as_object)
                .and_then(|kind| kind.get("type"))
                == Some(&Value::String("ShadowPositionUnresolved".to_string()))
        }));
        assert!(event_rows.iter().all(|row| {
            row.get("kind")
                .and_then(Value::as_object)
                .and_then(|kind| kind.get("type"))
                != Some(&Value::String("PositionClosed".to_string()))
        }));
    }

    #[tokio::test]
    async fn shadow_runtime_time_stop_uses_current_curve_state_when_snapshot_buffer_missing() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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
                Some(0.0001),
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
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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
    async fn shadow_runtime_time_stop_does_not_treat_other_pool_progress_as_freshness() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let account_state_core = Arc::new(AccountStateReducer::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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
        assert_eq!(
            runtime_snapshot.timestamp_ms,
            historical_latest.timestamp_ms
        );

        engine.tick().await;

        assert_eq!(engine.active_position_count(), 1);

        let lifecycle_rows = read_jsonl_rows(&lifecycle_log);
        assert!(
            lifecycle_rows.iter().all(|row| {
                row.get("record_type") != Some(&Value::String("exit_filled".to_string()))
            }),
            "an unrelated pool update must not manufacture a fresh executable exit: {lifecycle_rows:?}"
        );
        assert!(
            lifecycle_rows.iter().any(|row| {
                row.get("truth_status") == Some(&Value::String("stale".to_string()))
            }),
            "the stale pool snapshot must remain stale until this pool is observed again: {lifecycle_rows:?}"
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
    fn crash_guard_uses_raw_timeline_time_not_runtime_observed_at_time() {
        let (tx, _rx) = mpsc::channel(16);
        let engine = MonitoringEngine::new(
            pr2_guardian_config(false, CrashGuardMode::ObserveOnly),
            Arc::new(ShadowLedger::new()),
            tx,
        );
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "crash-raw-timeline-time".to_string(),
                    entry_order_id: "crash-raw-timeline-entry".to_string(),
                    quote_id: "crash-raw-timeline-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:crash-raw-timeline-time".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1),
                }),
            )
            .expect("shadow registration");

        {
            let mut positions = engine.positions.write();
            let position = positions.get_mut(&mint).expect("monitored position");
            position.peak_since_entry = 1.10;
            position.snapshot_timeline = SnapshotTimeline {
                cumulative_volume_sol: 0.0,
                snapshots: vec![
                    MarketSnapshot {
                        slot: Some(100),
                        timestamp_ms: 1_000,
                        price_sol_per_token: 1.10,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                    MarketSnapshot {
                        slot: Some(101),
                        timestamp_ms: 1_500,
                        price_sol_per_token: 0.80,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                    MarketSnapshot {
                        slot: Some(102),
                        timestamp_ms: 2_000,
                        price_sol_per_token: 0.70,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                ],
            };
            // This is the runtime compatibility projection. Its observed-at
            // timestamp is deliberately current, but it must never refresh
            // the raw canonical CrashGuard path above.
            position.last_shadow_snapshot = Some(MarketSnapshot {
                slot: Some(102),
                timestamp_ms: 10_000,
                price_sol_per_token: 0.70,
                price_state: PriceState::Valid,
                ..MarketSnapshot::default()
            });
        }

        let (snapshot, runtime_snapshot, crash_snapshot) = engine
            .materialize_post_buy_decision_snapshot(&mint, 10_000)
            .expect("decision snapshot");
        assert_eq!(
            runtime_snapshot.map(|sample| sample.timestamp_ms),
            Some(10_000)
        );
        assert_eq!(
            crash_snapshot.map(|sample| sample.timestamp_ms),
            Some(2_000),
            "CrashGuard must retain raw canonical provenance rather than the runtime projection"
        );
        assert_eq!(
            snapshot.crash_vector().latest_sample_age_ms(),
            Some(8_000),
            "runtime observation time must not make a stale raw path fresh"
        );
        let crash_evidence = MonitoringEngine::crash_guard_prequote_evidence(
            &snapshot,
            engine.exit_policy_v1.as_ref().expect("exit policy"),
        );
        assert_eq!(crash_evidence.status, PriceTruthStatus::Stale);
        assert_eq!(crash_evidence.age_ms, Some(8_000));
        assert!(matches!(
            ExitPolicyV1::evaluate_crash_guard_prequote(
                &snapshot,
                engine.exit_policy_v1.as_ref().expect("exit policy"),
            ),
            CrashGuardPreQuoteDecision::NotTriggered {
                reason: CrashGuardNotTriggeredReason::StaleSample
            }
        ));
    }

    #[test]
    fn het_trajectory_does_not_treat_timestamp_only_observation_as_market_activity() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::ObserveOnly);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        let (tx, _rx) = mpsc::channel(16);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid HET config");
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "trajectory-runtime-observation".to_string(),
                    entry_order_id: "trajectory-runtime-observation-entry".to_string(),
                    quote_id: "trajectory-runtime-observation-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:test:trajectory-runtime-observation".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1),
                }),
            )
            .expect("shadow registration");

        {
            let mut positions = engine.positions.write();
            let position = positions.get_mut(&mint).expect("monitored position");
            position.peak_since_entry = 1.10;
            position.snapshot_timeline = SnapshotTimeline {
                cumulative_volume_sol: 0.0,
                snapshots: vec![
                    MarketSnapshot {
                        slot: Some(100),
                        timestamp_ms: 1_000,
                        price_sol_per_token: 1.10,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                    MarketSnapshot {
                        slot: Some(101),
                        timestamp_ms: 1_500,
                        price_sol_per_token: 0.80,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                    MarketSnapshot {
                        slot: Some(102),
                        timestamp_ms: 2_000,
                        price_sol_per_token: 0.70,
                        price_state: PriceState::Valid,
                        ..MarketSnapshot::default()
                    },
                ],
            };
            position.last_shadow_snapshot = Some(MarketSnapshot {
                slot: Some(102),
                timestamp_ms: 10_000,
                price_sol_per_token: 0.70,
                price_state: PriceState::Valid,
                ..MarketSnapshot::default()
            });
        }

        let (bundle, runtime_snapshot, crash_snapshot, _) = engine
            .materialize_post_buy_snapshot_bundle(&mint, 10_000)
            .expect("HET snapshot bundle");

        assert_eq!(
            runtime_snapshot.map(|sample| sample.timestamp_ms),
            Some(10_000)
        );
        assert_eq!(
            crash_snapshot.map(|sample| sample.timestamp_ms),
            Some(2_000),
            "CrashGuard must stay on raw canonical samples"
        );
        assert_eq!(bundle.v2.trajectory.newest_sample_timestamp_ms, Some(2_000));
        assert_eq!(bundle.v2.trajectory.newest_sample_age_ms, Some(8_000));
        assert_eq!(
            bundle.v2.trajectory.quality,
            super::super::trajectory_v1::TrajectoryQualityV1::Stale,
            "timestamp-only observation must not create a trajectory sample"
        );
        assert_eq!(
            bundle.base.crash_vector().latest_sample_age_ms(),
            Some(8_000),
            "trajectory freshness must not refresh CrashGuard evidence"
        );
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
        let events_dir = tmp.path().join("events");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
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
        assert_eq!(engine.active_position_count(), 1);
        engine
            .run_shadow_runtime_tick(&mint, None, now_ms + 5_000)
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
                            detail.contains("sample_age_ms=10000")
                                && detail.contains("stale_after_ms=1500")
                        })
            }),
            "missing stale time-stop rejection proof: {lifecycle_rows:?}"
        );
        let unresolved = lifecycle_rows
            .iter()
            .find(|row| {
                row.get("record_type") == Some(&Value::String("position_unresolved".to_string()))
            })
            .expect("stale quote must end as unresolved shadow terminal");
        assert_eq!(unresolved["terminal_disposition"], "simulation_blocked");
        assert_eq!(unresolved["terminal_reason_v2"], "BLOCKED_BY_DATA");
        assert_eq!(
            unresolved["remaining_token_amount_raw"],
            120_080_136_032_u64
        );
        assert_eq!(unresolved["recovery_elapsed_ms"], 5_000);
        for forbidden_pnl_field in [
            "exit_price",
            "exit_value_sol",
            "exit_token_amount_raw",
            "gross_pnl_sol",
            "net_pnl_sol",
            "final_pnl",
            "final_pnl_pct",
            "mark_return_pct",
            "executable_gross_return_pct",
            "mfe_mark_pct",
            "mae_mark_pct",
            "exit_landed_slot",
            "exit_landed_slot_source",
        ] {
            assert!(
                unresolved.get(forbidden_pnl_field).is_none(),
                "unresolved outcome leaked {forbidden_pnl_field}: {unresolved:?}"
            );
        }
        assert!(lifecycle_rows.iter().all(|row| {
            row.get("record_type") != Some(&Value::String("position_closed".to_string()))
        }));

        let event_rows = read_event_rows(&events_dir);
        assert!(event_rows.iter().any(|row| {
            row.get("kind")
                .and_then(Value::as_object)
                .and_then(|kind| kind.get("type"))
                == Some(&Value::String("ShadowPositionUnresolved".to_string()))
        }));
        assert!(event_rows.iter().all(|row| {
            row.get("kind")
                .and_then(Value::as_object)
                .and_then(|kind| kind.get("type"))
                != Some(&Value::String("PositionClosed".to_string()))
        }));
    }

    #[tokio::test]
    async fn terminal_persistence_failure_withholds_notification_until_canonical_retry() {
        let tmp = TempDir::new().expect("tempdir");
        let canonical_path = tmp.path().join("shadow_position_event_v2.jsonl");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::new(
            PostBuyGuardianConfig::default(),
            Arc::new(ShadowLedger::new()),
            tx,
        );
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log));
        let engine = Arc::new(engine);

        std::fs::create_dir(&canonical_path).expect("fault injection canonical directory");
        let mint = Pubkey::new_unique();
        let registered = engine
            .register_shadow_position_with_terminal(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("terminal-persistence-fault".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "terminal-persistence-fault".to_string(),
                    entry_order_id: "terminal-persistence-entry".to_string(),
                    quote_id: "terminal-persistence-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:terminal-persistence-fault".to_string()),
                    position_epoch: Some(7),
                    opened_at_ms: Some(1_000),
                },
            )
            .expect("valid shadow registration");
        let mut terminal_rx = registered.terminal_rx;

        let trigger_ms = 1_000 + SHADOW_POSITION_TIME_STOP_MS + 1;
        engine
            .run_shadow_runtime_tick(&mint, None, trigger_ms)
            .await;
        engine
            .run_shadow_runtime_tick(&mint, None, trigger_ms + 5_000)
            .await;

        assert_eq!(engine.active_position_count(), 1);
        assert!(engine.has_pending_terminal_commit(&mint));
        assert!(matches!(
            terminal_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        std::fs::remove_dir(&canonical_path).expect("remove injected canonical directory");
        engine
            .run_shadow_runtime_tick(
                &mint,
                None,
                trigger_ms + 5_000 + SHADOW_QUOTE_RETRY_INTERVAL_MS,
            )
            .await;

        assert_eq!(engine.active_position_count(), 0);
        assert!(matches!(
            terminal_rx.try_recv(),
            Ok(ShadowTerminalDisposition::SimulationBlocked {
                reason: ShadowUnresolvedReason::BlockedByData,
                ..
            })
        ));
        let canonical_rows = read_jsonl_rows(&canonical_path);
        assert_eq!(
            canonical_rows
                .iter()
                .filter(|row| row["event_kind"] == "TERMINAL_TRUTH")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn authoritative_het_v2_ignores_v1_take_profit_and_executes_v2_max_hold() {
        let tmp = TempDir::new().expect("tempdir");
        let mut context = setup_authoritative_het_terminal_exit(&tmp);

        // At 10x the entry price V1's old take-profit would sell immediately.
        // V2 has no usable trajectory yet, so the authoritative V2 decision is
        // Hold and the position must remain open.
        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;
        assert_eq!(context.engine.active_position_count(), 1);

        // The same route/trajectory blocker must not defeat the hard maximum
        // hold.  V2 chooses AbsoluteMaxHold, then the existing guarded shadow
        // execution envelope applies the full simulated sell.
        context.tick_ms = 121_001;
        context.snapshot.timestamp_ms = context.tick_ms;
        context.snapshot.slot = Some(12);
        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        let comparisons = wait_for_jsonl_rows(&context.sidecar_path, 2).await;
        let position_still_open = context.engine.positions.read().contains_key(&context.mint);
        assert_eq!(
            context.engine.active_position_count(),
            0,
            "V2 MaxHold must pass the guarded executor: position_still_open={position_still_open}, lifecycle={:?}, comparisons={comparisons:?}",
            read_jsonl_rows(&context.lifecycle_path),
        );
        let lifecycle = read_jsonl_rows(&context.lifecycle_path);
        let fill = lifecycle
            .iter()
            .find(|row| row["record_type"] == "exit_filled")
            .expect("V2 max-hold shadow fill");
        assert_eq!(fill["exit_policy_reason_code"], "absolute_max_hold");

        assert!(comparisons.iter().any(|row| {
            row["v2_shadow_authority"] == Value::Bool(true)
                && row["v1_shadow_authority"] == Value::Bool(false)
                && row["v2_proposal_created"] == Value::Bool(true)
                && row["v2_economic_mutation"] == Value::Bool(true)
        }));
    }

    #[tokio::test]
    async fn authoritative_shadow_max_hold_on_unsupported_route_never_simulates_fill() {
        let tmp = TempDir::new().expect("tempdir");
        let mut context = setup_authoritative_het_terminal_exit(&tmp);
        {
            let mut positions = context.engine.positions.write();
            let position = positions
                .get_mut(&context.mint)
                .expect("registered position");
            position.het_route_status = RouteStatusV1::CurveCompletePumpSwapUnsupported;
        }
        context.tick_ms = 121_001;
        context.snapshot.timestamp_ms = context.tick_ms;
        context.snapshot.slot = Some(12);

        // Exercise the real active-shadow chain: one bundle, HET lattice,
        // quote planning, authoritative selection and the shared executor.
        // A fresh synthetic Pump quote is deliberately available; route truth
        // must still make a fill impossible.
        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        assert_eq!(context.engine.active_position_count(), 1);
        assert!(!context.engine.has_pending_terminal_commit(&context.mint));
        let positions = context.engine.positions.read();
        let position = positions.get(&context.mint).expect("position remains open");
        assert!(position.pending_exit_proposal.is_none());
        assert!(!matches!(
            position.last_shadow_outcome,
            Some(ShadowOutcomeKind::SimulatedFilled)
        ));
        drop(positions);
        assert!(read_jsonl_rows(&context.lifecycle_path)
            .iter()
            .all(|row| row["record_type"] != "exit_filled"));

        let comparisons = wait_for_jsonl_rows(&context.sidecar_path, 1).await;
        assert!(comparisons.iter().any(|row| {
            row["v2_final"] == "Blocked(RouteUnsupported)"
                && row["v2_selected_execution_reason"].is_null()
        }));
    }

    #[tokio::test]
    async fn authoritative_het_v2_finishes_a_preexisting_v1_proposal_without_replacing_it() {
        let tmp = TempDir::new().expect("tempdir");
        let mut context = setup_authoritative_het_terminal_exit(&tmp);

        // Model the exact deploy boundary: V1 started a sticky take-profit
        // proposal before V2 authority became active.  V2 must not replace it
        // with its own later max-hold decision for the same position.
        context
            .engine
            .remember_shadow_snapshot(&context.mint, &context.snapshot);
        let (decision_snapshot, _, _) = context
            .engine
            .materialize_post_buy_decision_snapshot(&context.mint, context.tick_ms)
            .expect("decision snapshot for existing V1 proposal");
        let v1_action = context
            .engine
            .begin_exit_proposal(
                &context.mint,
                decision_snapshot.guard(),
                &ExitCandidate::from_reason(ExitCandidateReason::TakeProfit),
                decision_snapshot.snapshot_id(),
                decision_snapshot.inactivity_age_ms(),
                None,
                None,
                context.tick_ms,
            )
            .expect("preexisting V1 proposal");

        context.tick_ms = 121_001;
        context.snapshot.timestamp_ms = context.tick_ms;
        context.snapshot.slot = Some(12);
        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        let comparisons = wait_for_jsonl_rows(&context.sidecar_path, 1).await;
        let position_still_open = context.engine.positions.read().contains_key(&context.mint);
        assert_eq!(
            context.engine.active_position_count(),
            0,
            "the sticky V1 proposal must complete unchanged: position_still_open={position_still_open}, lifecycle={:?}, comparisons={comparisons:?}",
            read_jsonl_rows(&context.lifecycle_path),
        );
        let lifecycle = read_jsonl_rows(&context.lifecycle_path);
        let fill = lifecycle
            .iter()
            .find(|row| row["record_type"] == "exit_filled")
            .expect("the existing V1 proposal fills");
        assert_eq!(fill["action_id"], v1_action.action_id);
        assert_eq!(fill["exit_policy_reason_code"], "target");

        assert!(comparisons.iter().any(|row| {
            row["v2_shadow_authority"] == Value::Bool(true)
                && row["v1_shadow_authority"] == Value::Bool(false)
                && row["v2_proposal_created"] == Value::Bool(false)
                && row["v2_economic_mutation"] == Value::Bool(false)
        }));
    }

    #[tokio::test]
    async fn het_exit_tick_persists_original_pre_mutation_comparison() {
        let tmp = TempDir::new().expect("tempdir");
        let mut context = setup_het_terminal_exit(&tmp, false, false);

        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        let comparison_rows = read_jsonl_rows(&context.sidecar_path);
        assert_eq!(comparison_rows.len(), 1);
        let comparison = &comparison_rows[0];
        assert_eq!(comparison["terminal_tick"], true);
        assert_eq!(comparison["v1_final"], "ExitApplied");
        assert_eq!(
            comparison["v1_authority_receipt"]["exit_apply_status"],
            "applied"
        );
        assert_eq!(
            comparison["v1_authority_receipt"]["terminal_commit_status"],
            "pending"
        );
        assert_eq!(
            comparison["snapshot_id"],
            comparison["v1_authority_receipt"]["snapshot_id"]
        );
        assert_eq!(
            comparison["state_revision"],
            comparison["v1_authority_receipt"]["state_revision"]
        );
        assert_eq!(
            comparison["remaining_quantity_raw"],
            comparison["v1_authority_receipt"]["remaining_quantity_raw"]
        );

        let terminal_rows = read_jsonl_rows(&context.canonical_path);
        let terminal = terminal_rows
            .iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("canonical terminal truth");
        assert_eq!(
            terminal_het_source_ref(terminal, "comparison_id"),
            comparison["comparison_id"].as_str()
        );
        assert_eq!(
            terminal_het_source_ref(terminal, "source_snapshot_id"),
            comparison["snapshot_id"].as_str()
        );
        assert_eq!(
            terminal_het_source_ref(terminal, "comparison_write_status"),
            Some("written")
        );
        assert_eq!(
            terminal_het_source_ref(terminal, "v1_action_id"),
            comparison["v1_authority_receipt"]["action_id"].as_str()
        );
        assert_eq!(context.engine.active_position_count(), 0);
        assert!(matches!(
            context.terminal_rx.try_recv(),
            Ok(ShadowTerminalDisposition::SimulatedClosed { .. })
        ));
    }

    #[tokio::test]
    async fn het_terminal_retry_uses_original_comparison_without_v2_reevaluation() {
        let tmp = TempDir::new().expect("tempdir");
        let context = setup_het_terminal_exit(&tmp, true, false);

        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;
        let before_retry = read_jsonl_rows(&context.sidecar_path);
        assert_eq!(before_retry.len(), 1);
        let comparison_id = before_retry[0]["comparison_id"].clone();
        let snapshot_id = before_retry[0]["snapshot_id"].clone();
        {
            let positions = context.engine.positions.read();
            let pending = positions[&context.mint]
                .pending_terminal_commit
                .as_ref()
                .expect("retained terminal commit");
            assert!(matches!(
                pending.het_comparison_write_status,
                HetComparisonWriteStatusV1::Written
            ));
            let prepared = pending
                .prepared_het_comparison
                .as_ref()
                .expect("retained original comparison");
            assert_eq!(prepared.correlation().comparison_id, comparison_id);
            assert_eq!(prepared.correlation().source_snapshot_id, snapshot_id);
        }

        std::fs::remove_dir(&context.canonical_path).expect("repair canonical writer");
        context
            .engine
            .run_shadow_runtime_tick(
                &context.mint,
                None,
                context
                    .tick_ms
                    .saturating_add(SHADOW_QUOTE_RETRY_INTERVAL_MS),
            )
            .await;

        let after_retry = read_jsonl_rows(&context.sidecar_path);
        assert_eq!(
            after_retry.len(),
            1,
            "retry must not reevaluate or rewrite V2"
        );
        assert_eq!(after_retry[0]["comparison_id"], comparison_id);
        assert_eq!(after_retry[0]["snapshot_id"], snapshot_id);
        let terminal = read_jsonl_rows(&context.canonical_path)
            .into_iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("terminal truth after retry");
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_id"),
            comparison_id.as_str()
        );
    }

    #[tokio::test]
    async fn het_terminal_retry_success_emits_exactly_one_terminal_outcome_for_original_action() {
        let tmp = TempDir::new().expect("tempdir");
        let (mut context, comparison, terminal) = complete_het_terminal_retry(&tmp).await;
        let canonical_rows = read_jsonl_rows(&context.canonical_path);
        assert_eq!(
            canonical_rows
                .iter()
                .filter(|row| row["event_kind"] == "TERMINAL_TRUTH")
                .count(),
            1
        );
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_id"),
            comparison["comparison_id"].as_str()
        );
        assert_eq!(
            terminal_het_source_ref(&terminal, "v1_action_id"),
            comparison["v1_authority_receipt"]["action_id"].as_str()
        );
        assert_eq!(context.engine.active_position_count(), 0);
        assert!(matches!(
            context.terminal_rx.try_recv(),
            Ok(ShadowTerminalDisposition::SimulatedClosed { .. })
        ));
    }

    #[tokio::test]
    async fn exit_applied_with_terminal_commit_pending_is_not_labeled_generic_pending_recovery() {
        let tmp = TempDir::new().expect("tempdir");
        let context = setup_het_terminal_exit(&tmp, true, false);

        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        let comparison = read_jsonl_rows(&context.sidecar_path)
            .into_iter()
            .next()
            .expect("exit comparison");
        assert_eq!(comparison["v1_final"], "ExitApplied");
        assert_eq!(
            comparison["v1_authority_receipt"]["outcome"],
            "exit_applied"
        );
        assert_eq!(
            comparison["v1_authority_receipt"]["exit_apply_status"],
            "applied"
        );
        assert_eq!(
            comparison["v1_authority_receipt"]["terminal_commit_status"],
            "pending"
        );
        assert!(context.engine.has_pending_terminal_commit(&context.mint));
    }

    #[tokio::test]
    async fn process_boundary_after_canonical_commit_cannot_silently_erase_exit_comparison() {
        let tmp = TempDir::new().expect("tempdir");
        let context = setup_het_terminal_exit(&tmp, true, false);

        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        let comparison_rows = read_jsonl_rows(&context.sidecar_path);
        assert_eq!(comparison_rows.len(), 1);
        assert!(context.canonical_path.is_dir());
        assert!(context.engine.has_pending_terminal_commit(&context.mint));
        let lifecycle_rows = read_jsonl_rows(&context.lifecycle_path);
        let closed = lifecycle_rows
            .iter()
            .find(|row| row["record_type"] == "position_closed")
            .expect("operational terminal record");
        assert_eq!(closed["het_pm_v2_comparison_write_status"], "written");
        assert_eq!(
            closed["het_pm_v2_comparison_id"],
            comparison_rows[0]["comparison_id"]
        );
    }

    #[tokio::test]
    async fn terminal_sidecar_failure_records_typed_skipped_without_blocking_capacity_release() {
        let tmp = TempDir::new().expect("tempdir");
        let mut context = setup_het_terminal_exit(&tmp, false, true);

        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        assert_eq!(context.engine.active_position_count(), 0);
        assert!(!context.engine.has_pending_terminal_commit(&context.mint));
        assert!(matches!(
            context.terminal_rx.try_recv(),
            Ok(ShadowTerminalDisposition::SimulatedClosed { .. })
        ));
        let lifecycle_rows = read_jsonl_rows(&context.lifecycle_path);
        let closed = lifecycle_rows
            .iter()
            .find(|row| row["record_type"] == "position_closed")
            .expect("operational terminal record");
        assert_eq!(closed["het_pm_v2_comparison_write_status"], "skipped");
        assert_eq!(
            closed["het_pm_v2_comparison_skip_reason"],
            "writer_io_failed"
        );
        let terminal = read_jsonl_rows(&context.canonical_path)
            .into_iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("canonical terminal truth");
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_write_status"),
            Some("skipped")
        );
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_skip_reason"),
            Some("writer_io_failed")
        );
    }

    fn setup_stalled_nonterminal_het_engine(
        queue_capacity: usize,
    ) -> (Arc<MonitoringEngine>, Pubkey) {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        config.het_pm_v2.writer_queue_capacity = queue_capacity;
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid HET-PM stalled-writer config");
        engine.set_stalled_het_pm_v2_observation_writer(queue_capacity);
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("het-stalled-nonterminal-writer".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "het-stalled-nonterminal-writer".to_string(),
                    entry_order_id: "het-stalled-nonterminal-writer-entry".to_string(),
                    quote_id: "het-stalled-nonterminal-writer-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:het-stalled-nonterminal-writer".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("valid shadow registration");
        (Arc::new(engine), mint)
    }

    #[tokio::test]
    async fn slow_nonterminal_het_writer_does_not_delay_next_v1_tick() {
        let (engine, mint) = setup_stalled_nonterminal_het_engine(1);
        engine.run_shadow_runtime_tick(&mint, None, 1_500).await;
        tokio::time::timeout(
            Duration::from_millis(100),
            engine.run_shadow_runtime_tick(&mint, None, 1_600),
        )
        .await
        .expect("full observer queue must not delay the next V1 tick");

        let stats = engine
            .het_pm_v2_observation_writer_stats()
            .expect("stalled writer stats");
        assert_eq!(stats.enqueue_attempts, 2);
        assert_eq!(stats.enqueued, 1);
        assert_eq!(stats.queue_full_drops, 1);
    }

    #[tokio::test]
    async fn full_het_writer_queue_does_not_block_v1_evaluation() {
        let (engine, mint) = setup_stalled_nonterminal_het_engine(1);
        engine.run_shadow_runtime_tick(&mint, None, 1_500).await;
        engine.run_shadow_runtime_tick(&mint, None, 1_600).await;

        let stats = engine
            .het_pm_v2_observation_writer_stats()
            .expect("stalled writer stats");
        assert_eq!(stats.enqueue_attempts, 2);
        assert_eq!(stats.queue_full_drops, 1);
        assert!(
            engine.positions.read().contains_key(&mint),
            "observer backpressure must not mutate or remove the V1 position"
        );
    }

    #[tokio::test]
    async fn slow_writer_cannot_trigger_missed_authority_tick() {
        let (engine, mint) = setup_stalled_nonterminal_het_engine(1);
        tokio::time::timeout(Duration::from_millis(150), async {
            let mut interval = tokio::time::interval(Duration::from_millis(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            for offset in 0..5 {
                interval.tick().await;
                engine
                    .run_shadow_runtime_tick(&mint, None, 1_500 + offset)
                    .await;
            }
        })
        .await
        .expect("stalled sidecar must not overrun the authority cadence");

        let stats = engine
            .het_pm_v2_observation_writer_stats()
            .expect("stalled writer stats");
        assert_eq!(stats.enqueue_attempts, 5);
        assert_eq!(stats.enqueued, 1);
        assert_eq!(stats.queue_full_drops, 4);
    }

    #[tokio::test]
    async fn terminal_het_writer_timeout_marks_skipped_and_continues_canonical_commit() {
        let tmp = TempDir::new().expect("tempdir");
        let mut context = setup_het_terminal_exit_with_writer(&tmp, false, false, Some(1));

        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        assert_eq!(context.engine.active_position_count(), 0);
        assert!(!context.engine.has_pending_terminal_commit(&context.mint));
        assert!(matches!(
            context.terminal_rx.try_recv(),
            Ok(ShadowTerminalDisposition::SimulatedClosed { .. })
        ));
        let closed = read_jsonl_rows(&context.lifecycle_path)
            .into_iter()
            .find(|row| row["record_type"] == "position_closed")
            .expect("operational terminal record");
        assert_eq!(closed["het_pm_v2_comparison_write_status"], "skipped");
        assert_eq!(
            closed["het_pm_v2_comparison_skip_reason"],
            "writer_timed_out_before_write"
        );
        assert!(read_jsonl_rows(&context.canonical_path)
            .iter()
            .any(|row| row["event_kind"] == "TERMINAL_TRUTH"));
    }

    #[tokio::test]
    async fn terminal_het_writer_timeout_does_not_delay_capacity_beyond_configured_budget() {
        let tmp = TempDir::new().expect("tempdir");
        let context = setup_het_terminal_exit_with_writer(&tmp, false, false, Some(1));
        let started = Instant::now();

        tokio::time::timeout(
            Duration::from_millis(250),
            context.engine.run_shadow_runtime_tick(
                &context.mint,
                Some(&context.snapshot),
                context.tick_ms,
            ),
        )
        .await
        .expect("terminal must finish within the 10ms HET budget plus test tolerance");

        assert!(
            started.elapsed() <= Duration::from_millis(250),
            "capacity release exceeded the configured writer budget plus tolerance"
        );
        assert_eq!(context.engine.active_position_count(), 0);
        let stats = context
            .engine
            .het_pm_v2_observation_writer_stats()
            .expect("stalled writer stats");
        assert_eq!(stats.terminal_timeouts, 1);
    }

    #[tokio::test]
    async fn terminal_writer_timeout_preserves_comparison_id_and_typed_skip_reason() {
        let tmp = TempDir::new().expect("tempdir");
        let context = setup_het_terminal_exit_with_writer(&tmp, false, false, Some(1));

        context
            .engine
            .run_shadow_runtime_tick(&context.mint, Some(&context.snapshot), context.tick_ms)
            .await;

        let closed = read_jsonl_rows(&context.lifecycle_path)
            .into_iter()
            .find(|row| row["record_type"] == "position_closed")
            .expect("operational terminal record");
        let comparison_id = closed["het_pm_v2_comparison_id"]
            .as_str()
            .expect("comparison correlation ID");
        assert!(!comparison_id.is_empty());
        assert_eq!(
            closed["het_pm_v2_comparison_skip_reason"],
            "writer_timed_out_before_write"
        );
        let terminal = read_jsonl_rows(&context.canonical_path)
            .into_iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("canonical terminal truth");
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_id"),
            Some(comparison_id)
        );
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_write_status"),
            Some("skipped")
        );
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_skip_reason"),
            Some("writer_timed_out_before_write")
        );
    }

    #[tokio::test]
    async fn terminal_timeout_after_writer_started_is_not_reported_as_skipped() {
        let tmp = TempDir::new().expect("tempdir");
        let mut context = setup_het_terminal_exit_with_writer(&tmp, false, false, Some(1));
        let (writer_started, release_writer) = Arc::get_mut(&mut context.engine)
            .expect("test owns the only engine reference")
            .set_controlled_het_pm_v2_observation_writer(context.sidecar_path.clone(), 1);
        let engine = Arc::clone(&context.engine);
        let mint = context.mint;
        let snapshot = context.snapshot.clone();
        let tick_ms = context.tick_ms;
        let tick = tokio::spawn(async move {
            engine
                .run_shadow_runtime_tick(&mint, Some(&snapshot), tick_ms)
                .await;
        });

        tokio::time::timeout(Duration::from_millis(100), async {
            loop {
                match writer_started.try_recv() {
                    Ok(()) => break,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    Err(error) => panic!("controlled writer start channel failed: {error}"),
                }
            }
        })
        .await
        .expect("worker must start I/O before the terminal budget expires");
        tick.await.expect("terminal tick task");

        assert_eq!(context.engine.active_position_count(), 0);
        assert!(!context.engine.has_pending_terminal_commit(&context.mint));
        assert!(matches!(
            context.terminal_rx.try_recv(),
            Ok(ShadowTerminalDisposition::SimulatedClosed { .. })
        ));
        let closed = read_jsonl_rows(&context.lifecycle_path)
            .into_iter()
            .find(|row| row["record_type"] == "position_closed")
            .expect("operational terminal record");
        assert_eq!(
            closed["het_pm_v2_comparison_write_status"],
            "outcome_unknown"
        );
        assert_eq!(
            closed["het_pm_v2_comparison_outcome_unknown_reason"],
            "writer_ack_timed_out"
        );
        assert!(closed["het_pm_v2_comparison_skip_reason"].is_null());
        let terminal = read_jsonl_rows(&context.canonical_path)
            .into_iter()
            .find(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .expect("canonical terminal truth");
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_write_status"),
            Some("outcome_unknown")
        );
        assert_eq!(
            terminal_het_source_ref(&terminal, "comparison_outcome_unknown_reason"),
            Some("writer_ack_timed_out")
        );

        release_writer.send(()).expect("release controlled writer");
        let comparisons = wait_for_jsonl_rows(&context.sidecar_path, 1).await;
        assert_eq!(
            comparisons[0]["comparison_id"], closed["het_pm_v2_comparison_id"],
            "late successful write remains correlated without contradicting OutcomeUnknown"
        );
    }

    #[tokio::test]
    async fn writer_health_artifact_durably_exposes_nonterminal_queue_drops() {
        let tmp = TempDir::new().expect("tempdir");
        let sidecar_path = tmp.path().join("het_pm_v2_observations_v1.jsonl");
        let (mut engine, mint) = setup_stalled_nonterminal_het_engine(1);
        let (writer_started, release_writer) = Arc::get_mut(&mut engine)
            .expect("test owns the only engine reference")
            .set_controlled_het_pm_v2_observation_writer(sidecar_path.clone(), 1);

        engine.run_shadow_runtime_tick(&mint, None, 1_500).await;
        tokio::time::timeout(Duration::from_millis(100), async {
            loop {
                match writer_started.try_recv() {
                    Ok(()) => break,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    Err(error) => panic!("controlled writer start channel failed: {error}"),
                }
            }
        })
        .await
        .expect("first comparison must enter writer I/O");
        engine.run_shadow_runtime_tick(&mint, None, 1_600).await;
        engine.run_shadow_runtime_tick(&mint, None, 1_700).await;

        let health = wait_for_writer_health(tmp.path(), |record| {
            record["enqueue_attempts"] == 3 && record["queue_full_drops"] == 1
        })
        .await;
        assert_eq!(health["run_id"], "het-stalled-nonterminal-writer");
        assert_eq!(health["comparison_attempts"], 3);
        assert_eq!(health["comparison_ready_for_enqueue"], 3);
        assert_eq!(health["core_validation_skips"], 0);
        assert_eq!(health["final_validation_skips"], 0);
        assert_eq!(health["serialization_skips"], 0);
        assert_eq!(health["payload_oversized_skips"], 0);
        assert_eq!(health["enqueued"], 2);
        assert_eq!(health["writes_succeeded"], 0);
        assert_eq!(health["shutdown_complete"], false);

        release_writer.send(()).expect("release first write");
        wait_for_jsonl_rows(&sidecar_path, 1).await;
        release_writer.send(()).expect("release second write");
        wait_for_jsonl_rows(&sidecar_path, 2).await;
        engine.flush_het_pm_v2_writer_health_for_shutdown().await;

        let final_health = wait_for_writer_health(tmp.path(), |record| {
            record["shutdown_complete"] == true && record["writes_succeeded"] == 2
        })
        .await;
        assert_eq!(final_health["comparison_attempts"], 3);
        assert_eq!(final_health["comparison_ready_for_enqueue"], 3);
        assert_eq!(final_health["enqueue_attempts"], 3);
        assert_eq!(final_health["queue_full_drops"], 1);
        assert_eq!(final_health["writes_failed"], 0);
        let rows = read_jsonl_rows(&sidecar_path);
        let writer_instance_id = final_health["writer_instance_id"]
            .as_str()
            .expect("writer instance id");
        assert!(
            rows.iter()
                .all(|row| row["writer_instance_id"].as_str() == Some(writer_instance_id)),
            "every comparison row must bind to the durable writer-health instance"
        );
        drop(engine);
    }

    #[tokio::test]
    async fn actual_v1_receipt_and_v2_record_share_exact_snapshot_id_revision_quantity() {
        let tmp = TempDir::new().expect("tempdir");
        let sidecar_path = tmp.path().join("het_pm_v2_observations_v1.jsonl");
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        let expected_time_stop_v2_hash = config
            .time_stop_v2
            .projection_config_hash()
            .expect("serializable TimeStop V2 config");
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid HET-PM runtime config");
        let expected_v1_hash = engine
            .exit_policy_v1
            .as_ref()
            .expect("V1 policy")
            .config_hash()
            .to_string();
        engine.set_het_pm_v2_observation_log_path(Some(sidecar_path.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("exact-shared-bundle".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "exact-shared-bundle".to_string(),
                    entry_order_id: "exact-shared-bundle-entry".to_string(),
                    quote_id: "exact-shared-bundle-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:exact-shared-bundle".to_string()),
                    position_epoch: Some(3),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("valid shadow registration");

        engine.run_shadow_runtime_tick(&mint, None, 1_500).await;

        let rows = wait_for_jsonl_rows(&sidecar_path, 1).await;
        assert_eq!(rows.len(), 1, "one comparison row per HET tick");
        let row = &rows[0];
        let receipt = row["v1_authority_receipt"]
            .as_object()
            .expect("typed V1 authority receipt");
        assert_eq!(receipt.get("snapshot_id"), row.get("snapshot_id"));
        assert_eq!(receipt.get("state_revision"), row.get("state_revision"));
        assert_eq!(
            receipt.get("remaining_quantity_raw"),
            row.get("remaining_quantity_raw")
        );
        assert_eq!(
            receipt.get("outcome"),
            Some(&Value::String("blocked".into()))
        );
        assert_eq!(row["v1_policy_id"], EXIT_POLICY_V1_ID);
        assert_eq!(row["v1_policy_version"], EXIT_POLICY_V1_VERSION);
        assert_eq!(row["v1_policy_config_hash"], expected_v1_hash);
        assert_eq!(row["time_stop_v2_config_hash"], expected_time_stop_v2_hash);
    }

    #[tokio::test]
    async fn v1_unknown_prequote_is_receipted_as_blocked_not_hold() {
        let tmp = TempDir::new().expect("tempdir");
        let sidecar_path = tmp.path().join("het_pm_v2_observations_v1.jsonl");
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("valid HET-PM runtime config");
        engine.set_het_pm_v2_observation_log_path(Some(sidecar_path.clone()));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("v1-unknown-receipt".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "v1-unknown-receipt".to_string(),
                    entry_order_id: "v1-unknown-receipt-entry".to_string(),
                    quote_id: "v1-unknown-receipt-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:v1-unknown-receipt".to_string()),
                    position_epoch: Some(4),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("valid shadow registration");

        let stale = MarketSnapshot {
            slot: Some(2),
            timestamp_ms: 1_000,
            price_sol_per_token: 1.0,
            price_state: PriceState::Valid,
            market_cap_sol: 1.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.0,
            ..MarketSnapshot::default()
        };
        engine
            .run_shadow_runtime_tick(&mint, Some(&stale), 10_000)
            .await;

        let rows = wait_for_jsonl_rows(&sidecar_path, 1).await;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["v1_prequote"], "unknown:MarkStale");
        assert_eq!(row["v1_final"], "Blocked");
        assert_eq!(row["v1_authority_receipt"]["outcome"], "blocked");
        assert_eq!(
            row["v1_authority_receipt"]["reason"],
            "prequote_unknown:MarkStale"
        );
        assert!(engine.positions.read().contains_key(&mint));
    }

    #[tokio::test]
    async fn het_sidecar_writer_failure_cannot_retain_v1_terminal_or_capacity() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");
        let sidecar_path = tmp.path().join("het_pm_v2_observations_v1.jsonl");
        std::fs::create_dir(&sidecar_path).expect("fault injection sidecar directory");

        let mut config = PostBuyGuardianConfig::default();
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        let (tx, _rx) = mpsc::channel(4);
        let mut engine = MonitoringEngine::new(config, Arc::new(ShadowLedger::new()), tx);
        enable_baseline_exit_policy(&mut engine);
        enable_terminal_truth_harness(&mut engine, tmp.path());
        engine.set_shadow_lifecycle_log_path(Some(lifecycle_log));
        engine.set_het_pm_v2_observation_log_path(Some(sidecar_path));
        let engine = Arc::new(engine);

        let mint = Pubkey::new_unique();
        let registered = engine
            .register_shadow_position_with_terminal(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000),
                Some(1_000),
                PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("het-sidecar-writer-fault".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "het-sidecar-writer-fault".to_string(),
                    entry_order_id: "het-sidecar-writer-fault-entry".to_string(),
                    quote_id: "het-sidecar-writer-fault-quote".to_string(),
                    slot: Some(1),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:het-sidecar-writer-fault".to_string()),
                    position_epoch: Some(1),
                    opened_at_ms: Some(1_000),
                },
            )
            .expect("valid shadow registration");
        let mut terminal_rx = registered.terminal_rx;

        let trigger_ms = 1_000 + SHADOW_POSITION_TIME_STOP_MS + 1;
        engine
            .run_shadow_runtime_tick(&mint, None, trigger_ms)
            .await;
        engine
            .run_shadow_runtime_tick(&mint, None, trigger_ms + 5_000)
            .await;

        assert_eq!(engine.active_position_count(), 0);
        assert!(!engine.has_pending_terminal_commit(&mint));
        assert!(matches!(
            terminal_rx.try_recv(),
            Ok(ShadowTerminalDisposition::SimulationBlocked {
                reason: ShadowUnresolvedReason::BlockedByData,
                ..
            })
        ));
        let canonical_rows = read_jsonl_rows(&tmp.path().join("shadow_position_event_v2.jsonl"));
        assert_eq!(
            canonical_rows
                .iter()
                .filter(|row| row["event_kind"] == "TERMINAL_TRUTH")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn shadow_runtime_stale_price_does_not_create_price_triggered_proposal() {
        let tmp = TempDir::new().expect("tempdir");
        let lifecycle_log = tmp.path().join("shadow_lifecycle.jsonl");

        let config = PostBuyGuardianConfig::default();
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let (tx, _rx) = mpsc::channel(16);
        let mut engine = MonitoringEngine::new(config, Arc::clone(&shadow_ledger), tx);
        enable_baseline_exit_policy(&mut engine);
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
            lifecycle_rows.is_empty(),
            "stale mark must not create a sticky TP/SL proposal: {lifecycle_rows:?}"
        );
        assert_eq!(engine.active_position_count(), 1);
        let (decision_snapshot, _, _) = engine
            .materialize_post_buy_decision_snapshot(&mint, 10_000)
            .expect("decision snapshot");
        assert_eq!(
            decision_snapshot.mark_evidence_status(),
            MarkEvidenceStatus::Stale
        );
        assert!(!decision_snapshot.has_pending_proposal());
    }

    #[test]
    fn het_anchor_apply_allows_forward_revisions_and_never_moves_down() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        let policy = EffectiveHetPmV2Config::from_guardian(&config).expect("HET config");
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("monitoring engine");
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata::default(),
                    candidate_id: "het-anchor".to_string(),
                    entry_order_id: "het-anchor-entry".to_string(),
                    quote_id: "het-anchor-quote".to_string(),
                    slot: Some(10),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:het-anchor".to_string()),
                    position_epoch: Some(3),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("position");
        let (state_revision, quantity) = {
            let mut positions = engine.positions.write();
            let position = positions.get_mut(&mint).expect("position state");
            // Anchor materialization requires explicit route evidence. In
            // production only AccountStateCore promotes this status.
            position.het_route_status = RouteStatusV1::PumpCurveSupported;
            (position.state_revision, position.remaining_token_amount_raw)
        };
        let key = ExecutableQuoteKeyV2 {
            position_id: "shadow:het-anchor".to_string(),
            position_epoch: 3,
            state_revision,
            remaining_quantity_raw: quantity,
            route_id: "pump_curve".to_string(),
            quote_model_id: "price_truth_resolver_shadow_curve_v1".to_string(),
            sample_slot: Some(11),
            sample_timestamp_ms: Some(2_000),
        };
        let request = PeakAnchorPreQuoteDecisionV1::QuoteRequired {
            key: key.clone(),
            peak_mark_price_sol: 1.2,
            source_snapshot_id: "snapshot:peak".to_string(),
        };
        let truth = ShadowExitTruth {
            exit_price_sol: 1.1,
            exit_token_amount_raw: quantity,
            entry_value_sol: 1.0,
            exit_value_sol: 1.1,
            gross_pnl_sol: 0.1,
            net_pnl_sol: 0.1,
            estimated_costs_sol: 0.0,
            pnl_pct: 10.0,
            evidence: PriceTruthEvidence {
                source: PriceTruthSource::CanonicalAccountStateSnapshot,
                status: PriceTruthStatus::Resolved,
                detail: None,
                slot: Some(11),
                timestamp_ms: Some(2_000),
                age_ms: Some(0),
                price_state: Some(PriceState::Valid),
                price_reason: None,
            },
        };
        let cells = vec![HetPmV2QuoteCell {
            key: key.clone(),
            outcome: Ok(truth),
        }];

        assert!(engine.apply_het_pm_v2_anchor_after_v1(
            &mint,
            &request,
            cells.first(),
            Some(1_000_000_000),
            policy.config_hash(),
            2_000,
        ));
        {
            let positions = engine.positions.read();
            let position = positions.get(&mint).expect("position state");
            assert_eq!(position.state_revision, state_revision);
            assert_eq!(
                position
                    .het_executable_peak_anchor
                    .as_ref()
                    .map(|anchor| anchor.peak_mark_price_sol),
                Some(1.2)
            );
        }
        assert!(
            !engine.apply_het_pm_v2_anchor_after_v1(
                &mint,
                &request,
                cells.first(),
                Some(1_000_000_000),
                policy.config_hash(),
                2_500,
            ),
            "the same or lower peak cannot replace the historical anchor"
        );

        let advanced_revision = {
            let mut positions = engine.positions.write();
            let position = positions.get_mut(&mint).expect("position");
            position.state_revision += 1;
            position.state_revision
        };
        let advanced_position_request = PeakAnchorPreQuoteDecisionV1::QuoteRequired {
            key: key.clone(),
            peak_mark_price_sol: 1.3,
            source_snapshot_id: "snapshot:advanced-position".to_string(),
        };
        assert!(
            engine.apply_het_pm_v2_anchor_after_v1(
                &mint,
                &advanced_position_request,
                cells.first(),
                Some(1_000_000_000),
                policy.config_hash(),
                3_000,
            ),
            "a newer position revision with the same identity, quantity and route must still accept a historical peak anchor"
        );
        {
            let positions = engine.positions.read();
            let position = positions.get(&mint).expect("position state");
            let anchor = position
                .het_executable_peak_anchor
                .as_ref()
                .expect("updated anchor");
            assert_eq!(position.state_revision, advanced_revision);
            assert_eq!(anchor.peak_mark_price_sol, 1.3);
            assert_eq!(anchor.quote_state_revision, state_revision);
        }

        let mut future_key = key;
        future_key.state_revision = advanced_revision.saturating_add(10);
        let future_request = PeakAnchorPreQuoteDecisionV1::QuoteRequired {
            key: future_key.clone(),
            peak_mark_price_sol: 1.4,
            source_snapshot_id: "snapshot:future".to_string(),
        };
        let future_cells = vec![HetPmV2QuoteCell {
            key: future_key,
            outcome: cells[0].outcome.clone(),
        }];
        assert!(
            !engine.apply_het_pm_v2_anchor_after_v1(
                &mint,
                &future_request,
                future_cells.first(),
                Some(1_000_000_000),
                policy.config_hash(),
                3_500,
            ),
            "an anchor quote from a future position revision cannot mutate the observer state"
        );
    }

    #[test]
    fn het_anchor_quote_plan_uses_exact_runtime_peak_key_not_stale_crash_source() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("monitoring engine");
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("het-anchor-exact-key".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "het-anchor-exact-key".to_string(),
                    entry_order_id: "het-anchor-exact-key-entry".to_string(),
                    quote_id: "het-anchor-exact-key-quote".to_string(),
                    slot: Some(10),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:het-anchor-exact-key".to_string()),
                    position_epoch: Some(5),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("position");

        {
            let mut positions = engine.positions.write();
            let position = positions.get_mut(&mint).expect("position state");
            position.het_route_status = RouteStatusV1::PumpCurveSupported;
            position.snapshot_timeline = SnapshotTimeline {
                cumulative_volume_sol: 100.0,
                snapshots: vec![MarketSnapshot {
                    slot: Some(100),
                    timestamp_ms: 2_000,
                    price_sol_per_token: 1.1,
                    price_state: PriceState::Valid,
                    market_cap_sol: 1.1,
                    reserve_base: 10_000_000.0,
                    reserve_quote: 11.0,
                    ..MarketSnapshot::default()
                }],
            };
            position.last_shadow_snapshot = Some(MarketSnapshot {
                slot: Some(101),
                timestamp_ms: 10_000,
                price_sol_per_token: 1.3,
                price_state: PriceState::Valid,
                market_cap_sol: 1.3,
                reserve_base: 10_000_000.0,
                reserve_quote: 13.0,
                ..MarketSnapshot::default()
            });
        }

        let (bundle, latest_snapshot, raw_canonical_snapshot, trajectory_peak_snapshot) = engine
            .materialize_post_buy_snapshot_bundle(&mint, 10_000)
            .expect("HET snapshot bundle");
        assert_eq!(
            latest_snapshot.as_ref().map(|sample| sample.timestamp_ms),
            Some(10_000)
        );
        assert_eq!(
            raw_canonical_snapshot
                .as_ref()
                .map(|sample| sample.timestamp_ms),
            Some(2_000),
            "the raw CrashGuard source is intentionally stale in this regression"
        );
        assert_eq!(
            bundle.v2.trajectory.newest_sample_timestamp_ms,
            Some(10_000)
        );
        assert_eq!(bundle.v2.trajectory.peak_sample_timestamp_ms, Some(10_000));

        let v1_policy = engine.exit_policy_v1.as_ref().expect("V1 policy");
        let het_policy = engine.het_pm_v2.as_ref().expect("HET policy");
        let v1_prequote = ExitPolicyV1::evaluate_prequote(&bundle.base, v1_policy);
        let crash_prequote = ExitPolicyV1::evaluate_crash_guard_prequote(&bundle.base, v1_policy);
        let prepared = engine.prepare_het_pm_v2_tick(HetPmV2TickInput {
            bundle: &bundle,
            latest_snapshot: latest_snapshot.as_ref(),
            raw_canonical_snapshot: raw_canonical_snapshot.as_ref(),
            trajectory_peak_snapshot: trajectory_peak_snapshot.as_ref(),
            v1_prequote: &v1_prequote,
            crash_prequote: &crash_prequote,
            v1_policy,
            het_policy,
            now_ms: 10_000,
        });

        let PeakAnchorPreQuoteDecisionV1::QuoteRequired { key, .. } = &prepared.anchor_request
        else {
            panic!("fresh newest peak must request an executable anchor quote");
        };
        assert_eq!(key.sample_timestamp_ms, Some(10_000));
        assert!(
            prepared
                .anchor_quote_cell
                .as_ref()
                .is_some_and(|cell| &cell.key == key && cell.outcome.is_ok()),
            "quote plan must resolve the exact anchor key instead of the stale raw CrashGuard key"
        );
        assert!(engine.apply_het_pm_v2_anchor_after_v1(
            &mint,
            &prepared.anchor_request,
            prepared.anchor_quote_cell.as_ref(),
            bundle.v2.entry_value_quote_raw,
            het_policy.config_hash(),
            10_000,
        ));
    }

    #[test]
    fn het_trailing_quote_uses_latest_runtime_source_not_stale_crash_source() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        config.het_pm_v2.trailing_arm_mark_return_bps = 500;
        config.het_pm_v2.trailing_mark_candidate_drawdown_bps = 500;
        config.het_pm_v2.trailing_executable_breach_bps = 500;
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("monitoring engine");
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("het-trailing-latest-quote".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "het-trailing-latest-quote".to_string(),
                    entry_order_id: "het-trailing-latest-quote-entry".to_string(),
                    quote_id: "het-trailing-latest-quote-quote".to_string(),
                    slot: Some(20),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:het-trailing-latest-quote".to_string()),
                    position_epoch: Some(6),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("position");

        let policy_hash = engine
            .het_pm_v2
            .as_ref()
            .expect("HET policy")
            .config_hash()
            .to_string();
        {
            let mut positions = engine.positions.write();
            let position = positions.get_mut(&mint).expect("position state");
            position.het_route_status = RouteStatusV1::PumpCurveSupported;
            position.snapshot_timeline = SnapshotTimeline {
                cumulative_volume_sol: 100.0,
                snapshots: vec![MarketSnapshot {
                    slot: Some(200),
                    timestamp_ms: 2_000,
                    price_sol_per_token: 1.8,
                    price_state: PriceState::Valid,
                    market_cap_sol: 1.8,
                    reserve_base: 10_000_000.0,
                    reserve_quote: 18.0,
                    ..MarketSnapshot::default()
                }],
            };
            position.last_shadow_snapshot = Some(MarketSnapshot {
                slot: Some(201),
                timestamp_ms: 10_000,
                price_sol_per_token: 1.3,
                price_state: PriceState::Valid,
                market_cap_sol: 1.3,
                reserve_base: 10_000_000.0,
                reserve_quote: 13.0,
                ..MarketSnapshot::default()
            });
            position.het_executable_peak_anchor = Some(ExecutablePeakAnchorV1 {
                position_id: "shadow:het-trailing-latest-quote".to_string(),
                position_epoch: 6,
                remaining_quantity_raw: position.remaining_token_amount_raw,
                route_id: "pump_curve".to_string(),
                quote_model_id: "price_truth_resolver_shadow_curve_v1".to_string(),
                policy_config_hash: policy_hash,
                quote_state_revision: position.state_revision,
                source_snapshot_id: "snapshot:peak".to_string(),
                source_sample_slot: Some(200),
                source_sample_timestamp_ms: Some(2_000),
                peak_mark_price_sol: 1.8,
                executable_value_quote_raw: None,
                executable_value_sol: 2.0,
                executable_gross_return_bps: Some(10_000),
                anchor_seq: 1,
                created_at_ms: 2_000,
            });
        }

        let (bundle, latest_snapshot, raw_canonical_snapshot, trajectory_peak_snapshot) = engine
            .materialize_post_buy_snapshot_bundle(&mint, 10_000)
            .expect("HET snapshot bundle");
        assert_eq!(
            latest_snapshot.as_ref().map(|sample| sample.timestamp_ms),
            Some(10_000)
        );
        assert_eq!(
            raw_canonical_snapshot
                .as_ref()
                .map(|sample| sample.timestamp_ms),
            Some(2_000)
        );
        assert_eq!(
            bundle.v2.trajectory.newest_sample_timestamp_ms,
            Some(10_000)
        );
        assert_eq!(bundle.v2.trajectory.peak_sample_timestamp_ms, Some(2_000));

        let v1_policy = engine.exit_policy_v1.as_ref().expect("V1 policy");
        let het_policy = engine.het_pm_v2.as_ref().expect("HET policy");
        let v1_prequote = ExitPolicyV1::evaluate_prequote(&bundle.base, v1_policy);
        let crash_prequote = ExitPolicyV1::evaluate_crash_guard_prequote(&bundle.base, v1_policy);
        let prepared = engine.prepare_het_pm_v2_tick(HetPmV2TickInput {
            bundle: &bundle,
            latest_snapshot: latest_snapshot.as_ref(),
            raw_canonical_snapshot: raw_canonical_snapshot.as_ref(),
            trajectory_peak_snapshot: trajectory_peak_snapshot.as_ref(),
            v1_prequote: &v1_prequote,
            crash_prequote: &crash_prequote,
            v1_policy,
            het_policy,
            now_ms: 10_000,
        });

        assert!(
            prepared.quote_cells.iter().any(|cell| {
                cell.key.sample_timestamp_ms == Some(10_000) && cell.outcome.is_ok()
            }),
            "Trailing must resolve against the fresh runtime quote"
        );
        assert!(
            prepared.quote_cells.iter().all(|cell| {
                cell.key.sample_timestamp_ms != Some(2_000) || cell.outcome.is_err()
            }),
            "the stale raw Crash source must not be the resolved Trailing quote"
        );
        let trailing = prepared.comparison_core;
        let PreparedV1V2ComparisonCoreV1::Ready(record) = trailing else {
            panic!("comparison core must be ready");
        };
        let trailing = record
            .v2_gate_evaluations
            .iter()
            .find(|evaluation| evaluation.gate == HetPmExitReasonV2::ExecutableTrailing)
            .expect("trailing evaluation");
        assert_eq!(trailing.quote_status, HetPmGateQuoteStatusV2::Resolved);
        assert!(matches!(
            trailing.final_decision,
            HetPmFinalDecisionV2::ExitAll {
                reason: HetPmExitReasonV2::ExecutableTrailing,
                ..
            }
        ));
    }

    #[test]
    fn het_missing_anchor_is_backfilled_from_historical_peak_snapshot_before_next_tick() {
        let mut config = pr2_guardian_config(false, CrashGuardMode::Disabled);
        config.het_pm_v2.enabled = true;
        config.time_stop_v2.enabled = true;
        config.het_pm_v2.trailing_arm_mark_return_bps = 500;
        config.het_pm_v2.trailing_mark_candidate_drawdown_bps = 500;
        config.het_pm_v2.trailing_executable_breach_bps = 500;
        let (tx, _rx) = mpsc::channel(4);
        let engine = MonitoringEngine::try_new(config, Arc::new(ShadowLedger::new()), tx)
            .expect("monitoring engine");
        let mint = Pubkey::new_unique();
        engine
            .register_position_with_context(
                Pubkey::new_unique(),
                mint,
                Pubkey::new_unique(),
                Some(1.0),
                Some(1_000_000_000),
                Some(1_000_000),
                Some(PositionEventContext {
                    join_metadata: PositionJoinMetadata {
                        run_id: Some("het-trailing-anchor-backfill".to_string()),
                        ..PositionJoinMetadata::default()
                    },
                    candidate_id: "het-trailing-anchor-backfill".to_string(),
                    entry_order_id: "het-trailing-anchor-backfill-entry".to_string(),
                    quote_id: "het-trailing-anchor-backfill-quote".to_string(),
                    slot: Some(30),
                    lane: Lane::Shadow,
                    position_id: Some("shadow:het-trailing-anchor-backfill".to_string()),
                    position_epoch: Some(7),
                    opened_at_ms: Some(1_000),
                }),
            )
            .expect("position");

        {
            let mut positions = engine.positions.write();
            let position = positions.get_mut(&mint).expect("position state");
            position.het_route_status = RouteStatusV1::PumpCurveSupported;
            position.snapshot_timeline = SnapshotTimeline {
                cumulative_volume_sol: 100.0,
                snapshots: vec![MarketSnapshot {
                    slot: Some(300),
                    timestamp_ms: 2_000,
                    price_sol_per_token: 1.8,
                    price_state: PriceState::Valid,
                    market_cap_sol: 1.8,
                    reserve_base: 10_000_000.0,
                    reserve_quote: 18.0,
                    ..MarketSnapshot::default()
                }],
            };
            position.last_shadow_snapshot = Some(MarketSnapshot {
                slot: Some(301),
                timestamp_ms: 10_000,
                price_sol_per_token: 1.3,
                price_state: PriceState::Valid,
                market_cap_sol: 1.3,
                reserve_base: 10_000_000.0,
                reserve_quote: 13.0,
                ..MarketSnapshot::default()
            });
        }

        let (bundle, latest_snapshot, raw_canonical_snapshot, trajectory_peak_snapshot) = engine
            .materialize_post_buy_snapshot_bundle(&mint, 10_000)
            .expect("HET snapshot bundle");
        assert_eq!(
            bundle.v2.trajectory.newest_sample_timestamp_ms,
            Some(10_000)
        );
        assert_eq!(bundle.v2.trajectory.peak_sample_timestamp_ms, Some(2_000));
        assert_eq!(
            trajectory_peak_snapshot
                .as_ref()
                .map(|sample| sample.timestamp_ms),
            Some(2_000)
        );
        assert!(
            bundle.v2.executable_peak_anchor.is_none(),
            "fixture starts with the exact runtime residual: peak exists, executable anchor does not"
        );

        let v1_policy = engine.exit_policy_v1.as_ref().expect("V1 policy");
        let het_policy = engine.het_pm_v2.as_ref().expect("HET policy");
        let v1_prequote = ExitPolicyV1::evaluate_prequote(&bundle.base, v1_policy);
        let crash_prequote = ExitPolicyV1::evaluate_crash_guard_prequote(&bundle.base, v1_policy);
        let prepared = engine.prepare_het_pm_v2_tick(HetPmV2TickInput {
            bundle: &bundle,
            latest_snapshot: latest_snapshot.as_ref(),
            raw_canonical_snapshot: raw_canonical_snapshot.as_ref(),
            trajectory_peak_snapshot: trajectory_peak_snapshot.as_ref(),
            v1_prequote: &v1_prequote,
            crash_prequote: &crash_prequote,
            v1_policy,
            het_policy,
            now_ms: 10_000,
        });
        let PeakAnchorPreQuoteDecisionV1::QuoteRequired { key, .. } = &prepared.anchor_request
        else {
            panic!("historical peak without anchor must request an anchor backfill quote");
        };
        assert_eq!(key.sample_timestamp_ms, Some(2_000));
        assert!(
            prepared
                .anchor_quote_cell
                .as_ref()
                .is_some_and(|cell| &cell.key == key && cell.outcome.is_ok()),
            "anchor backfill must resolve from the historical peak sample, independently of current exit quote freshness"
        );
        let PreparedV1V2ComparisonCoreV1::Ready(record) = &prepared.comparison_core else {
            panic!("comparison core must be ready");
        };
        let first_trailing = record
            .v2_gate_evaluations
            .iter()
            .find(|evaluation| evaluation.gate == HetPmExitReasonV2::ExecutableTrailing)
            .expect("trailing evaluation");
        assert_eq!(
            first_trailing.quote_status,
            HetPmGateQuoteStatusV2::BlockedPreQuote,
            "first tick records the original missing-anchor blocker while preparing the backfill"
        );
        assert!(engine.apply_het_pm_v2_anchor_after_v1(
            &mint,
            &prepared.anchor_request,
            prepared.anchor_quote_cell.as_ref(),
            bundle.v2.entry_value_quote_raw,
            het_policy.config_hash(),
            10_000,
        ));

        let (next_bundle, next_latest, next_raw, next_peak) = engine
            .materialize_post_buy_snapshot_bundle(&mint, 10_500)
            .expect("next HET snapshot bundle");
        assert!(
            next_bundle.v2.executable_peak_anchor.is_some(),
            "historical peak anchor must be available to the next policy tick"
        );
        let next_v1_prequote = ExitPolicyV1::evaluate_prequote(&next_bundle.base, v1_policy);
        let next_crash_prequote =
            ExitPolicyV1::evaluate_crash_guard_prequote(&next_bundle.base, v1_policy);
        let next_prepared = engine.prepare_het_pm_v2_tick(HetPmV2TickInput {
            bundle: &next_bundle,
            latest_snapshot: next_latest.as_ref(),
            raw_canonical_snapshot: next_raw.as_ref(),
            trajectory_peak_snapshot: next_peak.as_ref(),
            v1_prequote: &next_v1_prequote,
            crash_prequote: &next_crash_prequote,
            v1_policy,
            het_policy,
            now_ms: 10_500,
        });
        let PreparedV1V2ComparisonCoreV1::Ready(next_record) = next_prepared.comparison_core else {
            panic!("next comparison core must be ready");
        };
        let next_trailing = next_record
            .v2_gate_evaluations
            .iter()
            .find(|evaluation| evaluation.gate == HetPmExitReasonV2::ExecutableTrailing)
            .expect("next trailing evaluation");
        assert_eq!(next_trailing.quote_status, HetPmGateQuoteStatusV2::Resolved);
        assert!(matches!(
            next_trailing.final_decision,
            HetPmFinalDecisionV2::ExitAll {
                reason: HetPmExitReasonV2::ExecutableTrailing,
                ..
            }
        ));
    }

    #[test]
    fn het_quote_plan_deduplicates_exact_keys_and_preserves_full_quantity_and_provenance() {
        let base_key = ExecutableQuoteKeyV2 {
            position_id: "shadow:quote-plan".to_string(),
            position_epoch: 4,
            state_revision: 9,
            remaining_quantity_raw: 777,
            route_id: "pump_curve".to_string(),
            quote_model_id: "price_truth_resolver_shadow_curve_v1".to_string(),
            sample_slot: None,
            sample_timestamp_ms: None,
        };
        let projected = MarketSnapshot {
            timestamp_ms: 10_000,
            slot: Some(100),
            ..MarketSnapshot::default()
        };
        let raw = MarketSnapshot {
            timestamp_ms: 10_500,
            slot: Some(101),
            ..MarketSnapshot::default()
        };

        let hold_plan = HetPmV2QuotePlan::default();
        assert!(
            hold_plan.cells.is_empty(),
            "Hold must not allocate a quote cell"
        );

        let mut plan = HetPmV2QuotePlan::default();
        let projected_key = plan.add(base_key.clone(), &projected);
        let duplicate_key = plan.add(base_key.clone(), &projected);
        assert_eq!(projected_key, duplicate_key);
        assert_eq!(plan.cells.len(), 1, "the same key resolves at most once");
        assert_eq!(projected_key.remaining_quantity_raw, 777);

        let raw_key = plan.add(base_key, &raw);
        assert_ne!(projected_key, raw_key);
        assert_eq!(plan.cells.len(), HET_PM_V2_MAX_QUOTE_CELLS);
        assert_eq!(raw_key.sample_slot, Some(101));
        assert_eq!(raw_key.sample_timestamp_ms, Some(10_500));

        let next_tick = HetPmV2QuotePlan::default();
        assert!(
            next_tick.cells.is_empty(),
            "the quote plan must not cache cells between monitor ticks"
        );
    }

    #[test]
    fn timestop_projection_is_immutable_and_does_not_advance_window_state() {
        let initial = time_stop_v2_test_snapshot(1, 1_000, 1.0, 100.0, 10.0, 1, 1.0);
        let latest = time_stop_v2_test_snapshot(2, 4_000, 1.1, 110.0, 11.0, 2, 2.0);
        let cfg = TimeStopV2Config {
            enabled: true,
            ..TimeStopV2Config::default()
        };
        let mut state = TimeStopV2State::from_registration(Some(&initial));
        state
            .evaluate(&cfg, 1_000, Some(1.0), Some(&latest), 4_000)
            .expect("due window");
        let index_before = state.next_window_index;
        let failed_before = state.failed_windows;
        let first = state.project(4_000, cfg.window_ms());
        let second = state.project(4_000, cfg.window_ms());

        assert_eq!(state.next_window_index, index_before);
        assert_eq!(state.failed_windows, failed_before);
        assert_eq!(first.current_status, second.current_status);
        assert_eq!(first.current_subreason, second.current_subreason);
        assert_eq!(
            first.consecutive_non_alive_windows,
            second.consecutive_non_alive_windows
        );
        assert_eq!(first.source_window_index, second.source_window_index);
    }
}
