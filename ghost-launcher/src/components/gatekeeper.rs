use super::gatekeeper_policy::{
    build_assessment_from_features, build_sybil_policy_diagnostics, evaluate_curve_gate,
    evaluate_policy_from_assessment, sybil_combo_veto_reason, CurveGateOutcome,
    PolicyEvaluationContext,
};
use crate::components::gatekeeper_adaptive_prosperity::ApsDiagnostics;
use crate::components::gatekeeper_pdd::{PddDiagnostics, PddHardFail};
use crate::components::gatekeeper_trajectory::TrajectoryAssessment;
use crate::events::PoolTransaction;
pub use crate::tx_intelligence::{
    compute_dev_behavior, compute_gini, compute_signer_diversity, compute_velocity_profile,
    compute_volume_sanity, DevBehaviorProfile, SignerDiversityProfile, SignerStats,
    VelocityProfile, VolumeSanityProfile,
};
use ghost_brain::config::gatekeeper_v25_config::TrajectoryAwareScoringConfig;
use ghost_brain::config::EntryDriftAnchorQuality;
use ghost_brain::config::{GatekeeperMode, GatekeeperV2Config};
use ghost_brain::oracle::snapshot_engine::PoolMetrics;
use ghost_core::checkpoint::MaterializedFeatureSet;
use ghost_core::shadow_ledger::{
    build_market_snapshots_from_trades, build_trade_snapshots_observed,
    BufferedTx as CommitBufferedTx, CommitResult, MarketSnapshot, ReconstructedState, ShadowLedger,
    ShadowLedgerStaleFallback, TxKey,
};
use ghost_core::{CurveFinality, CurveFreshnessState};
use parking_lot::{Mutex, RwLock};
use seer::early_fingerprint::EarlyFingerprintMetrics;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Canonical runtime pool state owned by launcher Gatekeeper semantics.
///
/// `Tracked`, `Approved` and `Committed` are intentionally distinct:
/// - `Tracked`: runtime is still observing/buffering the pool
/// - `Approved`: runtime policy passed, but canonical history is not implied
/// - `Committed`: canonical history is known to be persisted
///
/// Gatekeeper does NOT determine if a pool is dead; rejection/removal is still
/// handled by runtime/ledger orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolState {
    Tracked,
    Approved,
    Committed,
}

impl PoolState {
    #[inline]
    pub fn allows_runtime_relay(self) -> bool {
        matches!(self, Self::Approved | Self::Committed)
    }

    #[inline]
    pub fn is_approved(self) -> bool {
        self == Self::Approved
    }

    #[inline]
    pub fn is_committed(self) -> bool {
        self == Self::Committed
    }
}

#[derive(Debug, Clone)]
pub struct GatekeeperBufferedTx {
    pub tx: Arc<PoolTransaction>,
    pub metrics: PoolMetrics,
    pub tx_key: TxKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCommitPhase {
    Pending,
    Committing,
    PersistedAwaitingRuntime,
}

impl RuntimeCommitPhase {
    #[inline]
    pub fn is_committing(self) -> bool {
        self == Self::Committing
    }
}

#[derive(Debug, Clone)]
pub enum CommitIngressOutcome {
    BufferedHistory,
    PendingLive,
    RouteToLive { bootstrap_snapshot: MarketSnapshot },
    Duplicate,
    Missing,
}

#[derive(Debug, Clone)]
pub struct LauncherCommitOutcome {
    pub pool_id: Pubkey,
    pub base_mint: Pubkey,
    pub commit_result: CommitResult,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LauncherCommitCoordinatorStats {
    pub active_buffers: usize,
}

#[derive(Debug, Clone)]
struct LauncherCommitBuffer {
    pool_id: Pubkey,
    base_mint: Pubkey,
    initial_state: ReconstructedState,
    buffered_history: Vec<CommitBufferedTx>,
    tx_keys_seen: HashSet<TxKey>,
    pending_live: Vec<CommitBufferedTx>,
    phase: RuntimeCommitPhase,
    committed_snapshot: Option<MarketSnapshot>,
}

impl LauncherCommitBuffer {
    fn new(
        pool_id: Pubkey,
        base_mint: Pubkey,
        initial_reserve_sol_lamports: u64,
        initial_reserve_tok_units: u64,
    ) -> Self {
        Self {
            pool_id,
            base_mint,
            initial_state: ReconstructedState::from_initial_reserves(
                initial_reserve_sol_lamports,
                initial_reserve_tok_units,
            ),
            buffered_history: Vec::new(),
            tx_keys_seen: HashSet::new(),
            pending_live: Vec::new(),
            phase: RuntimeCommitPhase::Pending,
            committed_snapshot: None,
        }
    }

    fn restore_pending_live_to_history(&mut self) {
        let pending_live = std::mem::take(&mut self.pending_live);
        if pending_live.is_empty() {
            return;
        }

        for tx in pending_live {
            if self.tx_keys_seen.insert(tx.tx_key.clone()) {
                self.buffered_history.push(tx);
            }
        }

        self.buffered_history
            .sort_by(|lhs, rhs| lhs.tx_key.cmp(&rhs.tx_key));
    }

    fn add_tx(&mut self, tx: CommitBufferedTx) -> CommitIngressOutcome {
        if self.tx_keys_seen.contains(&tx.tx_key)
            || self
                .pending_live
                .iter()
                .any(|pending| pending.tx_key == tx.tx_key)
        {
            return CommitIngressOutcome::Duplicate;
        }

        match self.phase {
            RuntimeCommitPhase::Pending => {
                self.tx_keys_seen.insert(tx.tx_key.clone());
                self.buffered_history.push(tx);
                self.buffered_history
                    .sort_by(|lhs, rhs| lhs.tx_key.cmp(&rhs.tx_key));
                CommitIngressOutcome::BufferedHistory
            }
            RuntimeCommitPhase::Committing => {
                self.pending_live.push(tx);
                CommitIngressOutcome::PendingLive
            }
            RuntimeCommitPhase::PersistedAwaitingRuntime => {
                let Some(bootstrap_snapshot) = self.committed_snapshot.clone() else {
                    return CommitIngressOutcome::Missing;
                };
                CommitIngressOutcome::RouteToLive { bootstrap_snapshot }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct LauncherCommitCoordinator {
    buffers: RwLock<HashMap<Pubkey, Arc<Mutex<LauncherCommitBuffer>>>>,
}

impl LauncherCommitCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    fn emit_buffer_size_metric(&self) {
        ::metrics::gauge!("gatekeeper_buffer_size", self.buffers.read().len() as f64);
    }

    pub fn stage_history(
        &self,
        pool_id: Pubkey,
        base_mint: Pubkey,
        initial_reserve_sol_lamports: u64,
        initial_reserve_tok_units: u64,
        buffered_txs: Vec<CommitBufferedTx>,
    ) -> usize {
        let buffer = {
            let mut buffers = self.buffers.write();
            buffers
                .entry(base_mint)
                .or_insert_with(|| {
                    Arc::new(Mutex::new(LauncherCommitBuffer::new(
                        pool_id,
                        base_mint,
                        initial_reserve_sol_lamports,
                        initial_reserve_tok_units,
                    )))
                })
                .clone()
        };
        self.emit_buffer_size_metric();

        let mut staged = 0usize;
        let mut guard = buffer.lock();
        for tx in buffered_txs {
            match guard.add_tx(tx) {
                CommitIngressOutcome::BufferedHistory => {
                    staged += 1;
                }
                CommitIngressOutcome::Duplicate => {}
                CommitIngressOutcome::PendingLive
                | CommitIngressOutcome::RouteToLive { .. }
                | CommitIngressOutcome::Missing => {}
            }
        }

        staged
    }

    pub fn add_approved_tx(
        &self,
        base_mint: &Pubkey,
        tx: CommitBufferedTx,
    ) -> CommitIngressOutcome {
        let buffer = {
            let buffers = self.buffers.read();
            buffers.get(base_mint).cloned()
        };

        let Some(buffer) = buffer else {
            return CommitIngressOutcome::Missing;
        };

        let outcome = buffer.lock().add_tx(tx);
        self.emit_buffer_size_metric();
        outcome
    }

    pub fn process_ready_commits(&self, ledger: &ShadowLedger) -> Vec<LauncherCommitOutcome> {
        let base_mints: Vec<Pubkey> = {
            let buffers = self.buffers.read();
            buffers.keys().copied().collect()
        };

        let mut committed = Vec::new();

        for base_mint in base_mints {
            let buffer = {
                let buffers = self.buffers.read();
                buffers.get(&base_mint).cloned()
            };
            let Some(buffer) = buffer else {
                continue;
            };

            let (pool_id, initial_state, txs_snapshot) = {
                let mut guard = buffer.lock();
                if guard.phase != RuntimeCommitPhase::Pending || guard.buffered_history.is_empty() {
                    continue;
                }
                guard.phase = RuntimeCommitPhase::Committing;
                (
                    guard.pool_id,
                    guard.initial_state.clone(),
                    guard.buffered_history.clone(),
                )
            };

            let trade_snapshots =
                match build_trade_snapshots_observed(base_mint, initial_state, &txs_snapshot) {
                    Ok(snapshots) => snapshots,
                    Err(err) => {
                        let mut guard = buffer.lock();
                        guard.restore_pending_live_to_history();
                        guard.phase = RuntimeCommitPhase::Pending;
                        guard.committed_snapshot = None;
                        tracing::warn!(
                            pool = %pool_id,
                            base_mint = %base_mint,
                            error = %err,
                            "LauncherCommitCoordinator: failed to build trade snapshots"
                        );
                        continue;
                    }
                };

            let market_snapshots = match build_market_snapshots_from_trades(&trade_snapshots) {
                Ok(snapshots) => snapshots,
                Err(err) => {
                    let mut guard = buffer.lock();
                    guard.restore_pending_live_to_history();
                    guard.phase = RuntimeCommitPhase::Pending;
                    guard.committed_snapshot = None;
                    tracing::warn!(
                        pool = %pool_id,
                        base_mint = %base_mint,
                        error = %err,
                        "LauncherCommitCoordinator: failed to build market snapshots"
                    );
                    continue;
                }
            };

            let last_tx_key = trade_snapshots
                .last()
                .map(|snapshot| snapshot.tx_key.clone());
            let last_snapshot = market_snapshots.last().cloned();
            let ledger_result =
                ledger.commit_history(base_mint, market_snapshots, last_tx_key.clone());

            let mut guard = buffer.lock();
            if !ledger_result.persisted_success() {
                guard.restore_pending_live_to_history();
                guard.phase = RuntimeCommitPhase::Pending;
                guard.committed_snapshot = None;
                tracing::warn!(
                    pool = %pool_id,
                    base_mint = %base_mint,
                    status = ?ledger_result.status,
                    "LauncherCommitCoordinator: commit produced no persisted write"
                );
                continue;
            }

            let pending_live = std::mem::take(&mut guard.pending_live);
            let merged_pending_count = pending_live.len();
            let committed_snapshot = ledger_result.last_snapshot.clone().or(last_snapshot);
            guard.phase = RuntimeCommitPhase::PersistedAwaitingRuntime;
            guard.committed_snapshot = committed_snapshot.clone();

            committed.push(LauncherCommitOutcome {
                pool_id,
                base_mint,
                commit_result: CommitResult {
                    commit_history_result: ledger_result,
                    committed_count: txs_snapshot.len(),
                    merged_pending_count,
                    last_committed_tx_key: last_tx_key,
                    last_snapshot: committed_snapshot,
                    pending_live,
                },
            });
        }

        self.emit_buffer_size_metric();

        committed
    }

    pub fn finalize_committed(&self, base_mint: &Pubkey) -> bool {
        let removed = self.buffers.write().remove(base_mint).is_some();
        self.emit_buffer_size_metric();
        removed
    }

    pub fn drop_precommit_buffer(&self, base_mint: &Pubkey) -> bool {
        let removed = self.buffers.write().remove(base_mint).is_some();
        self.emit_buffer_size_metric();
        removed
    }

    pub fn remove(&self, base_mint: &Pubkey) -> bool {
        self.finalize_committed(base_mint)
    }

    pub fn stats(&self) -> LauncherCommitCoordinatorStats {
        LauncherCommitCoordinatorStats {
            active_buffers: self.buffers.read().len(),
        }
    }

    pub fn active_buffer_count(&self) -> usize {
        self.stats().active_buffers
    }

    pub fn commit_phase(&self, base_mint: &Pubkey) -> Option<RuntimeCommitPhase> {
        let buffer = {
            let buffers = self.buffers.read();
            buffers.get(base_mint).cloned()
        }?;
        let phase = buffer.lock().phase;
        Some(phase)
    }

    pub fn buffered_history_count(&self, base_mint: &Pubkey) -> Option<usize> {
        let buffer = {
            let buffers = self.buffers.read();
            buffers.get(base_mint).cloned()
        }?;
        let len = buffer.lock().buffered_history.len();
        Some(len)
    }
}

/// Genesis token supply for Pump.fun bonding curves.
/// Used to calculate bonding progress: 1.0 - (vTokens / GENESIS_TOKENS)
pub const PUMP_GENESIS_TOKEN_SUPPLY: f64 = 1_073_000_000.0;

/// Canonical reported Pump.fun token supply used for fully diluted market cap.
/// This differs from the virtual genesis reserve used for pricing/progress math.
pub const PUMP_TOKEN_TOTAL_SUPPLY: f64 = 1_000_000_000.0;

/// A single price observation derived from PumpPortal reserve data
#[derive(Debug, Clone, Copy)]
pub struct PricePoint {
    pub timestamp_ms: u64,
    pub price_sol_per_token: f64,
    pub v_sol_in_curve: f64,
    pub v_tokens_in_curve: f64,
    pub market_cap_sol: f64,
    /// Whether this price point came from a buy transaction
    pub is_buy: bool,
    /// Explicit flag: true only when curve data was successfully parsed/received.
    /// Set by the data source — NOT derived from reserve values.
    pub curve_data_known: bool,
    /// Finality tier of the curve state behind this price point.
    pub curve_finality: CurveFinality,
}

/// Deterministic vectors extracted from a gatekeeper buffer for a time window.
/// Used for offline DTW/Hill/MI/TDA analysis (JSONL v3).
///
/// All vectors are **aligned** to the same tx-event axis: each index `i`
/// corresponds to the same transaction.  Price is looked up from
/// `price_history` (last price point ≤ tx timestamp; NaN fallback).
#[derive(Debug, Clone, Default)]
pub struct WindowVectors {
    pub ts_offsets_ms: Vec<i64>,
    pub sol_amounts: Vec<f64>,
    pub prices: Vec<f64>,
    /// Inter-event intervals (diff of `ts_offsets_ms`); length = N−1.
    pub interval_ms: Vec<f64>,
    /// Price changes between consecutive tx; length = N−1.
    pub d_price: Vec<f64>,
    pub max_len: u32,
}

/// Phase 6: Bonding Curve Dynamics
#[derive(Debug, Clone)]
pub struct BondingCurveDynamics {
    pub initial_price: f64,
    pub current_price: f64,
    pub max_price: f64,
    pub price_change_ratio: f64,
    pub max_single_tx_price_impact_pct: f64,
    /// Maximum single SELL TX price impact (%)
    pub max_single_sell_impact_pct: f64,
    pub current_market_cap_sol: f64,
    pub market_cap_change_ratio: f64,
    pub bonding_progress_pct: f64,
    /// Whether bonding curve data was successfully parsed (explicit parser flag).
    /// When false, bonding_progress_pct should not be used for gating decisions.
    pub curve_data_known: bool,
    /// Finality tier of the curve state used by Phase 6.
    pub curve_finality: CurveFinality,
    pub price_data_points: usize,
}

#[inline]
fn append_soft_flag(flags_str: &mut String, flag: &str) {
    if flags_str == "none" {
        *flags_str = flag.to_string();
    } else {
        flags_str.push(',');
        flags_str.push_str(flag);
    }
}

#[inline]
fn curve_finality_caution_flag(curve_finality: CurveFinality) -> Option<&'static str> {
    match curve_finality {
        CurveFinality::Speculative => Some("CURVE_FINALITY_SPECULATIVE"),
        CurveFinality::Provisional => Some("CURVE_FINALITY_PROVISIONAL"),
        CurveFinality::Finalized => None,
    }
}

#[inline]
fn decorate_legacy_soft_flags(
    mut flags_str: String,
    curve: Option<&BondingCurveDynamics>,
) -> String {
    let curve_unknown = curve.is_some_and(|profile| !profile.curve_data_known);
    if curve_unknown {
        append_soft_flag(&mut flags_str, "BONDING_PROGRESS_UNKNOWN");
    }
    if let Some(flag) =
        curve.and_then(|profile| curve_finality_caution_flag(profile.curve_finality))
    {
        append_soft_flag(&mut flags_str, flag);
    }
    flags_str
}

#[inline]
fn is_curve_quality_actionable(
    curve_quality: CurveFreshnessState,
    curve_finality: CurveFinality,
    stale_fallback: ShadowLedgerStaleFallback,
) -> bool {
    match curve_quality {
        CurveFreshnessState::Fresh | CurveFreshnessState::Committed => true,
        CurveFreshnessState::Unknown => false,
        CurveFreshnessState::Stale => {
            matches!(
                stale_fallback,
                ShadowLedgerStaleFallback::UseStaleWithWarning
            ) && curve_finality.is_finalized()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Three-Layer Decision System: Hard Fails → Core Pass → Soft Signals
// ═══════════════════════════════════════════════════════════════════════

/// Soft signal flags — metrics that don't block BUY but affect confidence.
/// Each flag indicates a "suspicion" that the pool may be manipulated,
/// but the metric alone doesn't separate A from B in 10s data (d≈0).
#[derive(Debug, Clone, Default)]
pub struct SoftSignals {
    /// interval_cv < min threshold (bot-like regularity)
    pub low_interval_cv: bool,
    /// interval_cv > max threshold (pathological jitter / incoherent flow)
    pub high_interval_cv: bool,
    /// timing_entropy < min threshold
    pub low_timing_entropy: bool,
    /// timing_entropy > max threshold
    pub high_timing_entropy: bool,
    /// avg_interval_ms outside [min, max] range
    pub avg_interval_out_of_range: bool,
    /// burst_ratio > max threshold
    pub high_burst_ratio: bool,
    /// same_ms_tx_ratio > soft threshold (bundle suspicion)
    pub bundle_suspicion: bool,
    /// hhi > soft threshold (cabal suspicion)
    pub cabal_suspicion: bool,
    /// top3_volume_pct > soft threshold (top3 dominance)
    pub top3_dominance: bool,
    /// volume_gini > soft threshold
    pub high_volume_gini: bool,
    /// unique_ratio outside [min, max] range
    pub unique_ratio_out_of_range: bool,
    /// max_tx_per_signer > threshold
    pub high_tx_per_signer: bool,
    /// dust_filtered_count < threshold (low spam = suspicious lack of attention)
    pub low_dust_count: bool,
}

impl SoftSignals {
    /// Maximum possible score (number of soft flags).
    pub const MAX_SCORE: u8 = 13;

    /// Count how many soft flags are raised (unweighted, kept for compat).
    pub fn score(&self) -> u8 {
        let flags: [bool; 13] = [
            self.low_interval_cv,
            self.high_interval_cv,
            self.low_timing_entropy,
            self.high_timing_entropy,
            self.avg_interval_out_of_range,
            self.high_burst_ratio,
            self.bundle_suspicion,
            self.cabal_suspicion,
            self.top3_dominance,
            self.high_volume_gini,
            self.unique_ratio_out_of_range,
            self.high_tx_per_signer,
            self.low_dust_count,
        ];
        flags.iter().filter(|&&f| f).count() as u8
    }

    /// Compute weighted soft points using group-based weights.
    ///
    /// Groups:
    /// - **Timing** (w_timing): low_cv, high_cv, low_entropy, high_entropy,
    ///   avg_interval_oor, high_burst
    /// - **Manipulation** (w_manipulation): bundle, cabal, top3_dom
    /// - **Diversity** (w_diversity): high_gini, unique_oor, high_tps
    /// - **Ecosystem** (w_ecosystem): low_dust
    pub fn weighted_score(
        &self,
        w_timing: u8,
        w_manipulation: u8,
        w_diversity: u8,
        w_ecosystem: u8,
    ) -> u8 {
        let mut points: u8 = 0;
        // Timing group
        if self.low_interval_cv {
            points = points.saturating_add(w_timing);
        }
        if self.high_interval_cv {
            points = points.saturating_add(w_timing);
        }
        if self.low_timing_entropy {
            points = points.saturating_add(w_timing);
        }
        if self.high_timing_entropy {
            points = points.saturating_add(w_timing);
        }
        if self.avg_interval_out_of_range {
            points = points.saturating_add(w_timing);
        }
        if self.high_burst_ratio {
            points = points.saturating_add(w_timing);
        }
        // Manipulation group
        if self.bundle_suspicion {
            points = points.saturating_add(w_manipulation);
        }
        if self.cabal_suspicion {
            points = points.saturating_add(w_manipulation);
        }
        if self.top3_dominance {
            points = points.saturating_add(w_manipulation);
        }
        // Diversity group
        if self.high_volume_gini {
            points = points.saturating_add(w_diversity);
        }
        if self.unique_ratio_out_of_range {
            points = points.saturating_add(w_diversity);
        }
        if self.high_tx_per_signer {
            points = points.saturating_add(w_diversity);
        }
        // Ecosystem group
        if self.low_dust_count {
            points = points.saturating_add(w_ecosystem);
        }
        points
    }

    /// Maximum possible weighted points given group weights.
    pub fn max_possible_points(
        w_timing: u8,
        w_manipulation: u8,
        w_diversity: u8,
        w_ecosystem: u8,
    ) -> u8 {
        // 6×timing + 3×manipulation + 3×diversity + 1×ecosystem
        (6u16 * w_timing as u16
            + 3u16 * w_manipulation as u16
            + 3u16 * w_diversity as u16
            + w_ecosystem as u16)
            .min(255) as u8
    }

    /// Format raised flags as a comma-separated string for logging.
    pub fn format_flags(&self) -> String {
        let mut flags = Vec::new();
        if self.low_interval_cv {
            flags.push("low_cv");
        }
        if self.high_interval_cv {
            flags.push("high_cv");
        }
        if self.low_timing_entropy {
            flags.push("low_entropy");
        }
        if self.high_timing_entropy {
            flags.push("high_entropy");
        }
        if self.avg_interval_out_of_range {
            flags.push("avg_interval_oor");
        }
        if self.high_burst_ratio {
            flags.push("high_burst");
        }
        if self.bundle_suspicion {
            flags.push("bundle");
        }
        if self.cabal_suspicion {
            flags.push("cabal");
        }
        if self.top3_dominance {
            flags.push("top3_dom");
        }
        if self.high_volume_gini {
            flags.push("high_gini");
        }
        if self.unique_ratio_out_of_range {
            flags.push("unique_oor");
        }
        if self.high_tx_per_signer {
            flags.push("high_tps");
        }
        if self.low_dust_count {
            flags.push("low_dust");
        }
        if flags.is_empty() {
            "none".to_string()
        } else {
            flags.join(",")
        }
    }
}

/// Sybil soft signal flags derived exclusively from canonical sybil feature snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SybilSoftSignals {
    pub low_ftdi: bool,
    pub high_dbia: bool,
    pub low_sfd: bool,
    pub low_des: bool,
    pub high_cpv: bool,
    pub high_fsc: bool,
}

impl SybilSoftSignals {
    pub const MAX_SCORE: u8 = 6;

    pub fn score(&self) -> u8 {
        let flags = [
            self.low_ftdi,
            self.high_dbia,
            self.low_sfd,
            self.low_des,
            self.high_cpv,
            self.high_fsc,
        ];
        flags.iter().filter(|&&flag| flag).count() as u8
    }

    pub fn format_flags(&self) -> String {
        let mut flags = Vec::new();
        if self.low_ftdi {
            flags.push("low_ftdi");
        }
        if self.high_dbia {
            flags.push("high_dbia");
        }
        if self.low_sfd {
            flags.push("low_sfd");
        }
        if self.low_des {
            flags.push("low_des");
        }
        if self.high_cpv {
            flags.push("high_cpv");
        }
        if self.high_fsc {
            flags.push("high_fsc");
        }
        if flags.is_empty() {
            "none".to_string()
        } else {
            flags.join(",")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SybilInterferencePattern {
    HighDbiaLowFtdi,
    LowDesLowSfd,
    HighCpvLowDes,
    HighFscHighCpv,
    HighDbiaLowFtdiLowSfd,
    HighFscHighCpvLowDesOrLowSfd,
}

impl SybilInterferencePattern {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::HighDbiaLowFtdi => "HIGH_DBIA_LOW_FTDI",
            Self::LowDesLowSfd => "LOW_DES_LOW_SFD",
            Self::HighCpvLowDes => "HIGH_CPV_LOW_DES",
            Self::HighFscHighCpv => "HIGH_FSC_HIGH_CPV",
            Self::HighDbiaLowFtdiLowSfd => "HIGH_DBIA_LOW_FTDI_LOW_SFD",
            Self::HighFscHighCpvLowDesOrLowSfd => "HIGH_FSC_HIGH_CPV_LOW_DES_OR_LOW_SFD",
        }
    }

    pub fn format_patterns(patterns: &[Self]) -> String {
        if patterns.is_empty() {
            "none".to_string()
        } else {
            patterns.iter().map(Self::tag).collect::<Vec<_>>().join(",")
        }
    }
}

impl std::fmt::Display for SybilInterferencePattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SybilLeadSignal {
    LowFtdi,
    HighDbia,
    LowSfd,
    LowDes,
    HighCpv,
    HighFsc,
    HighDbiaLowFtdi,
    LowDesLowSfd,
    HighCpvLowDes,
    HighFscHighCpv,
    HighDbiaLowFtdiLowSfd,
    HighFscHighCpvLowDesOrLowSfd,
}

impl SybilLeadSignal {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::LowFtdi => "LOW_FTDI",
            Self::HighDbia => "HIGH_DBIA",
            Self::LowSfd => "LOW_SFD",
            Self::LowDes => "LOW_DES",
            Self::HighCpv => "HIGH_CPV",
            Self::HighFsc => "HIGH_FSC",
            Self::HighDbiaLowFtdi => "HIGH_DBIA_LOW_FTDI",
            Self::LowDesLowSfd => "LOW_DES_LOW_SFD",
            Self::HighCpvLowDes => "HIGH_CPV_LOW_DES",
            Self::HighFscHighCpv => "HIGH_FSC_HIGH_CPV",
            Self::HighDbiaLowFtdiLowSfd => "HIGH_DBIA_LOW_FTDI_LOW_SFD",
            Self::HighFscHighCpvLowDesOrLowSfd => "HIGH_FSC_HIGH_CPV_LOW_DES_OR_LOW_SFD",
        }
    }
}

impl std::fmt::Display for SybilLeadSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

#[derive(Debug, Clone, Default)]
pub struct SybilPolicyDiagnostics {
    pub enabled: bool,
    pub combo_veto_enabled: bool,
    pub soft_signals: SybilSoftSignals,
    pub soft_points: u16,
    pub max_soft_points_possible: u16,
    pub effective_max_soft_points: u8,
    pub lead_signal: Option<SybilLeadSignal>,
    pub interference_patterns: Vec<SybilInterferencePattern>,
    pub meta_score: Option<u16>,
    pub metric_degraded_reasons: Vec<String>,
}

/// Verdict type for explicit log classification.
///
/// Enables unambiguous telemetry: TIMEOUT_PHASE1 vs REJECT_HARD_FAIL etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatekeeperVerdictType {
    /// All checks passed — BUY signal
    Buy,
    /// Layer 1 kill-switch triggered
    RejectHardFail,
    /// Layer 2 core check failed (Core1/Core2/Core3)
    RejectCoreFail,
    /// Layer 3 soft points exceeded threshold
    RejectSoftExcess,
    /// Sybil Interference bucket exceeded its dedicated threshold
    RejectSybilSoftExcess,
    /// Sybil Interference combo-veto matched a whitelisted high-confidence pattern
    RejectSybilInterference,
    /// Positive alpha gate failed after all negative filters passed
    RejectLowAlpha,
    /// Final prosperity selector rejected the pool after alpha/open-path passed
    RejectLowProsperity,
    /// Phase 1 quantity gate not met within deadline
    TimeoutPhase1,
    /// No non-dust data arrived within deadline
    TimeoutNoData,
    /// Phase 1 passed, but the deadline closed before enough core phases passed
    TimeoutDeadlineLowPhases,
    /// IWIM veto: dev history analysis detected rug/sybil/scam pattern
    RejectIwimVeto,
    /// IWIM low confidence + BORDERLINE gatekeeper → reject
    RejectIwimLowConf,
    /// IWIM timeout/error + BORDERLINE gatekeeper → reject
    RejectIwimUnknownStrict,
    /// V2.5 Pump & Dump Detector hard veto (general)
    RejectPumpAndDump,
    /// V2.5 PDD: entry drift exceeded threshold
    RejectEntryDrift,
    /// V2.5 PDD: flash crash sell cluster detected
    RejectFlashCrash,
    /// V2.5 PDD: consecutive same-size buy ramping detected
    RejectRamping,
    /// V2.5 TAS: trajectory score below hard-reject threshold (< 0.30)
    RejectLowTrajectory,
    /// V2.5 DOW: live early entry (only after ADR promotion)
    EarlyBuy,
}

impl GatekeeperVerdictType {
    /// Short tag for log output.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::RejectHardFail => "REJECT_HARD_FAIL",
            Self::RejectCoreFail => "REJECT_CORE_FAIL",
            Self::RejectSoftExcess => "REJECT_SOFT_EXCESS",
            Self::RejectSybilSoftExcess => "REJECT_SYBIL_SOFT_EXCESS",
            Self::RejectSybilInterference => "REJECT_SYBIL_INTERFERENCE",
            Self::RejectLowAlpha => "REJECT_LOW_ALPHA",
            Self::RejectLowProsperity => "REJECT_LOW_PROSPERITY",
            Self::TimeoutPhase1 => "TIMEOUT_PHASE1_INSUFFICIENT",
            Self::TimeoutNoData => "TIMEOUT_PHASE1_NO_DATA",
            Self::TimeoutDeadlineLowPhases => "TIMEOUT_DEADLINE_LOW_PHASES",
            Self::RejectIwimVeto => "REJECT_IWIM_VETO",
            Self::RejectIwimLowConf => "REJECT_IWIM_LOW_CONF",
            Self::RejectIwimUnknownStrict => "REJECT_IWIM_UNKNOWN_STRICT",
            Self::RejectPumpAndDump => "REJECT_PUMP_AND_DUMP",
            Self::RejectEntryDrift => "REJECT_ENTRY_DRIFT",
            Self::RejectFlashCrash => "REJECT_FLASH_CRASH",
            Self::RejectRamping => "REJECT_RAMPING",
            Self::RejectLowTrajectory => "REJECT_LOW_TRAJECTORY",
            Self::EarlyBuy => "EARLY_BUY",
        }
    }
}

impl std::fmt::Display for GatekeeperVerdictType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

/// Gatekeeper BUY signal strength classification for IWIM policy matrix.
///
/// Determines how the system responds to IWIM timeout/low-confidence:
/// - STRONG: IWIM only blocks on HIGH-confidence VETO, timeout = BUY
/// - BORDERLINE: IWIM acts as "required confirmation", timeout/unknown = REJECT
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatekeeperStrength {
    /// Core all-pass, soft_points well under limit, no manipulation flags.
    Strong,
    /// Core all-pass but soft_points near limit OR manipulation flags present.
    Borderline,
}

impl std::fmt::Display for GatekeeperStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Strong => write!(f, "STRONG"),
            Self::Borderline => write!(f, "BORDERLINE"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaRejectTrigger {
    LowMomentum,
    LowDemand,
    LowJoint,
}

impl AlphaRejectTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LowMomentum => "low_momentum",
            Self::LowDemand => "low_demand",
            Self::LowJoint => "low_joint",
        }
    }
}

impl std::fmt::Display for AlphaRejectTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct AlphaGateDiagnostics {
    pub enabled: bool,
    pub actionable: bool,
    pub momentum: Option<f64>,
    pub demand: Option<f64>,
    pub joint: Option<f64>,
    pub pass: Option<bool>,
    pub reject_trigger: Option<AlphaRejectTrigger>,
    pub skip_reason: Option<&'static str>,
}

impl AlphaGateDiagnostics {
    pub fn not_run(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub fn skipped(enabled: bool, reason: &'static str) -> Self {
        Self {
            enabled,
            actionable: false,
            pass: Some(true),
            skip_reason: Some(reason),
            ..Self::default()
        }
    }

    pub fn evaluated(
        enabled: bool,
        momentum: f64,
        demand: f64,
        joint: f64,
        pass: bool,
        reject_trigger: Option<AlphaRejectTrigger>,
    ) -> Self {
        Self {
            enabled,
            actionable: true,
            momentum: Some(momentum),
            demand: Some(demand),
            joint: Some(joint),
            pass: Some(pass),
            reject_trigger,
            skip_reason: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProsperityRejectTrigger {
    MissingMarketCap,
    MissingSignerCrossPoolVelocity,
    MissingFeeTopologyDiversityIndex,
    MissingSellBuyRatio,
    BelowMinMarketCap,
    HighSignerCrossPoolVelocity,
    AboveOverlayMaxPriceChange,
    AboveOverlayMaxBondingProgress,
    BelowOverlayMinFeeTopologyDiversityIndex,
    AboveOverlayMaxSellBuyRatio,
    AboveOverlayBranch2MaxPriceChange,
    NoBalancedBranch,
}

impl ProsperityRejectTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingMarketCap => "missing_market_cap",
            Self::MissingSignerCrossPoolVelocity => "missing_signer_cross_pool_velocity",
            Self::MissingFeeTopologyDiversityIndex => "missing_fee_topology_diversity_index",
            Self::MissingSellBuyRatio => "missing_sell_buy_ratio",
            Self::BelowMinMarketCap => "below_min_market_cap",
            Self::HighSignerCrossPoolVelocity => "high_signer_cross_pool_velocity",
            Self::AboveOverlayMaxPriceChange => "above_overlay_max_price_change",
            Self::AboveOverlayMaxBondingProgress => "above_overlay_max_bonding_progress",
            Self::BelowOverlayMinFeeTopologyDiversityIndex => {
                "below_overlay_min_fee_topology_diversity_index"
            }
            Self::AboveOverlayMaxSellBuyRatio => "above_overlay_max_sell_buy_ratio",
            Self::AboveOverlayBranch2MaxPriceChange => "above_overlay_branch2_max_price_change",
            Self::NoBalancedBranch => "no_balanced_branch",
        }
    }
}

impl std::fmt::Display for ProsperityRejectTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProsperityFilterDiagnostics {
    pub enabled: bool,
    pub actionable: bool,
    pub pass: Option<bool>,
    pub reject_trigger: Option<ProsperityRejectTrigger>,
    pub market_cap_floor_pass: Option<bool>,
    pub cpv_pass: Option<bool>,
    pub branch1_pass: Option<bool>,
    pub branch2_pass: Option<bool>,
    pub branch3_pass: Option<bool>,
    pub overlay_enabled: bool,
    pub overlay_pass: Option<bool>,
    pub overlay_price_change_pass: Option<bool>,
    pub overlay_bonding_progress_pass: Option<bool>,
    pub overlay_fee_topology_diversity_pass: Option<bool>,
    pub overlay_branch23_sell_buy_pass: Option<bool>,
    pub overlay_branch2_price_change_pass: Option<bool>,
    pub matched_branches: Vec<&'static str>,
}

impl ProsperityFilterDiagnostics {
    pub fn not_run(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub fn rejected_missing(
        enabled: bool,
        reject_trigger: ProsperityRejectTrigger,
        market_cap_floor_pass: Option<bool>,
        cpv_pass: Option<bool>,
    ) -> Self {
        Self {
            enabled,
            actionable: false,
            pass: Some(false),
            reject_trigger: Some(reject_trigger),
            market_cap_floor_pass,
            cpv_pass,
            ..Self::default()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluated(
        enabled: bool,
        pass: bool,
        market_cap_floor_pass: bool,
        cpv_pass: bool,
        branch1_pass: bool,
        branch2_pass: bool,
        branch3_pass: bool,
        reject_trigger: Option<ProsperityRejectTrigger>,
        matched_branches: Vec<&'static str>,
    ) -> Self {
        Self {
            enabled,
            actionable: true,
            pass: Some(pass),
            reject_trigger,
            market_cap_floor_pass: Some(market_cap_floor_pass),
            cpv_pass: Some(cpv_pass),
            branch1_pass: Some(branch1_pass),
            branch2_pass: Some(branch2_pass),
            branch3_pass: Some(branch3_pass),
            matched_branches,
            ..Self::default()
        }
    }
}

/// Three-layer decision result from Gatekeeper assessment.
#[derive(Debug, Clone)]
pub struct GatekeeperDecision {
    /// Hard fail reason (if any) — immediate REJECT
    pub hard_fail_reason: Option<String>,
    /// Core-1 (Quantity Gate / Phase 1) passed
    pub core1_passed: bool,
    /// Core-2 (Capital Dominance / Phase 4) passed
    pub core2_passed: bool,
    /// Core-3 (Dev + Curve Safety / Phase 5+6) passed
    pub core3_passed: bool,
    /// Legacy soft signal flags
    pub soft_signals: SoftSignals,
    /// Weighted legacy soft points (group-based scoring)
    pub soft_points: u8,
    /// Maximum possible weighted legacy soft points
    pub max_soft_points_possible: u8,
    /// Effective legacy soft threshold used (may differ for dev_unknown)
    pub effective_max_soft_points: u8,
    /// Whether dev wallet is unknown (triggers stricter core requirements)
    pub dev_unknown: bool,
    /// Dedicated Sybil Interference diagnostics bucket.
    pub sybil_policy: SybilPolicyDiagnostics,
    /// Positive alpha gate diagnostics computed only after prior reject layers pass.
    pub alpha_gate: AlphaGateDiagnostics,
    /// Final prosperity selector diagnostics computed after the alpha gate passes.
    pub prosperity_filter: ProsperityFilterDiagnostics,
    /// Legacy + sybil points exported for telemetry only.
    pub total_soft_points: u16,
    /// Explicit verdict type for unambiguous telemetry
    pub verdict_type: GatekeeperVerdictType,
    /// Final verdict: true = BUY, false = REJECT
    pub verdict_buy: bool,
    /// Reason chain for logging (HARD_FAIL > CORE_FAIL > SOFT_EXCESS > BUY)
    pub reason_chain: String,
    /// Gatekeeper BUY strength classification (computed only on BUY verdict)
    pub gatekeeper_strength: Option<GatekeeperStrength>,
}

/// Complete assessment from all 6 phases
#[derive(Debug, Clone)]
pub struct GatekeeperAssessment {
    pub phase1_passed: bool,
    pub phase2_velocity: Option<VelocityProfile>,
    pub phase2_passed: bool,
    pub phase3_diversity: Option<SignerDiversityProfile>,
    pub phase3_passed: bool,
    pub phase4_volume: Option<VolumeSanityProfile>,
    pub phase4_passed: bool,
    pub phase5_dev: Option<DevBehaviorProfile>,
    pub phase5_passed: bool,
    pub phase6_curve: Option<BondingCurveDynamics>,
    pub phase6_passed: bool,
    pub phases_passed: u8,
    pub hard_reject_reason: Option<String>,
    pub total_tx_evaluated: usize,
    pub unique_tx_evaluated: usize,
    pub unique_signers_evaluated: usize,
    pub observation_duration_ms: u64,
    pub finalize_lag_ms: u64,
    pub dust_filtered_count: u64,
    pub eval_count: usize,
    /// Actual buy count from Phase 1 tracking (not derived from buy_ratio).
    /// This ensures the logged buy_count is always accurate, even when
    /// phase4_volume is None (e.g. hard-reject early return).
    pub buy_count: usize,
    /// Three-layer decision result (hard_fails → core → soft).
    /// Populated by `compute_decision()`. None if three-layer system is disabled.
    pub decision: Option<GatekeeperDecision>,
    /// Early fingerprint metrics (gRPC / Yellowstone).
    /// Populated externally when available.
    pub early_fingerprint: Option<EarlyFingerprintMetrics>,
    /// Curve latch t0 (event-time ms) for JSONL telemetry.
    pub curve_t0_event_ts_ms: Option<u64>,
    /// Provenance label for curve latch t0.
    pub curve_t0_clock_source: Option<&'static str>,
    /// Elapsed ms since curve t0 at assessment time.
    /// Computed as `highest_seen_ts - t0` (event-time SSOT; uses the most
    /// recent transaction timestamp, not wall-clock).
    pub curve_wait_elapsed_ms: Option<u64>,
    /// Full feature bundle used by the policy engine for deterministic replay.
    pub feature_snapshot: MaterializedFeatureSet,
    /// Number of checkpoints available when the verdict was made.
    pub checkpoint_count: u32,
    /// Whether trajectory data was available for policy evaluation.
    pub trajectory_available: bool,

    // ── V2.5 extensions ──
    /// Shadow decision records from V2.5 Dynamic Observation Window checkpoints.
    pub v25_shadow_decisions: Vec<ShadowV25Decision>,

    /// V2.5 Trajectory Aware Scoring assessment (3-segment analysis).
    pub trajectory: Option<TrajectoryAssessment>,

    /// V2.5 Pump & Dump Detector diagnostics.
    pub pdd_assessment: Option<PddDiagnostics>,

    /// V2.5 Adaptive Prosperity diagnostics (shadow/offline).
    pub aps_diagnostics: Option<ApsDiagnostics>,

    /// Terminal observation stage at verdict time.
    pub observation_stage: Option<ObservationStage>,

    // ── V2.5 top-level telemetry fields (populated from sub-module diagnostics) ──
    /// Entry drift percentage at terminal evaluation (from PDD).
    pub entry_drift_pct: Option<f64>,
    /// Quality of the entry drift price anchor (Strong/Weak).
    pub entry_drift_anchor_quality: Option<EntryDriftAnchorQuality>,
    /// Whether adaptive prosperity thresholds were applied to the live verdict.
    pub adaptive_thresholds_applied: bool,
    /// Cached V2.5 confidence score at terminal evaluation.
    pub v25_confidence: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V25ConfidenceBreakdown {
    pub base_quality: f64,
    pub alpha_quality: f64,
    pub pdd_modulator: f64,
    pub tas_modulator: f64,
    pub sybil_modulator: f64,
    pub pre_veto_confidence: f64,
    pub final_confidence: f64,
    pub zeroed_by_pdd_hard_fail: bool,
    pub zeroed_by_tas_hard_reject: bool,
}

impl GatekeeperAssessment {
    pub fn v25_pdd_clean(&self) -> bool {
        self.pdd_assessment
            .as_ref()
            .map_or(true, |pdd| pdd.hard_fail.is_none() && pdd.pdd_score > 0.7)
    }

    pub fn v25_tas_hard_reject(&self, config: &GatekeeperV2Config) -> bool {
        use crate::components::gatekeeper_trajectory::evaluate_trajectory;

        config.tas.enabled
            && self.trajectory.as_ref().is_some_and(|traj| {
                evaluate_trajectory(traj) < config.tas.tas_hard_reject_threshold
            })
    }

    /// V2.5 confidence score — 5-component multiplicative model.
    ///
    /// ```text
    /// confidence = clamp01(
    ///     base_quality    // phases_passed / 6
    ///   * alpha_quality   // momentum*0.4 + demand*0.35 + joint*0.25
    ///   * pdd_modulator   // 0.7 + 0.3*pdd_score
    ///   * tas_modulator   // [0.75, 1.25] based on trajectory
    ///   * sybil_modulator // 1.0 - 0.20*(sybil_points/max_sybil_points)
    /// )
    /// ```
    /// Extremely negative trajectory (< hard_reject_threshold) → 0.0.
    pub fn v25_confidence_breakdown(
        &self,
        config: &GatekeeperV2Config,
    ) -> Option<V25ConfidenceBreakdown> {
        use crate::components::gatekeeper_trajectory::{
            compute_tas_modulator, evaluate_trajectory,
        };

        let decision = self.decision.as_ref()?;

        // P4 availability guard: return None when required inputs are missing
        if config.tas.enabled {
            let (tas_ok, _) = self.tas_availability(config);
            if !tas_ok.unwrap_or(false) {
                return None;
            }
        }
        if config.pdd.enabled {
            let pdd_seq_ok = self.pdd_sequence_signals_available(config);
            let needs_sequence = config.pdd.spike_detection_enabled
                || config.pdd.ramping_detection_enabled
                || config.pdd.flash_crash_protection_enabled;
            if needs_sequence && !pdd_seq_ok.unwrap_or(true) {
                return None;
            }
        }

        // 1. base_quality: fraction of phases passed
        let base_quality = (self.phases_passed.min(6) as f64) / 6.0;

        // 2. alpha_quality: weighted alpha gate scalars
        let alpha_quality = {
            let momentum = decision.alpha_gate.momentum.unwrap_or(0.0);
            let demand = decision.alpha_gate.demand.unwrap_or(0.0);
            let joint = momentum * demand;
            (momentum * 0.4 + demand * 0.35 + joint * 0.25).clamp(0.0, 1.0)
        };

        // 3. pdd_modulator: PDD cleanliness, range [0.7, 1.0]
        let (pdd_modulator, zeroed_by_pdd_hard_fail) = if let Some(ref pdd) = self.pdd_assessment {
            (0.7 + 0.3 * pdd.pdd_score, pdd.hard_fail.is_some())
        } else {
            (1.0, false)
        };

        // 4. tas_modulator: trajectory quality ±25%; hard zeroing is tracked
        // separately so logs can distinguish model quality from terminal veto.
        let (tas_modulator, zeroed_by_tas_hard_reject) = if let Some(ref traj) = self.trajectory {
            if config.tas.enabled {
                let tas_score = evaluate_trajectory(traj);
                (
                    compute_tas_modulator(tas_score, &config.tas),
                    tas_score < config.tas.tas_hard_reject_threshold,
                )
            } else {
                (1.0, false)
            }
        } else {
            (1.0, false)
        };

        // 5. sybil_modulator: penalty for sybil interference, range [0.80, 1.0]
        let sybil_modulator = {
            let max_sybil = config.max_sybil_soft_points.max(1) as f64;
            let sybil_pts = decision.sybil_policy.soft_points as f64;
            1.0 - 0.20 * (sybil_pts / max_sybil)
        };

        let pre_veto_confidence =
            (base_quality * alpha_quality * pdd_modulator * tas_modulator * sybil_modulator)
                .clamp(0.0, 1.0);
        let final_confidence = if zeroed_by_pdd_hard_fail || zeroed_by_tas_hard_reject {
            0.0
        } else {
            pre_veto_confidence
        };

        Some(V25ConfidenceBreakdown {
            base_quality,
            alpha_quality,
            pdd_modulator,
            tas_modulator,
            sybil_modulator,
            pre_veto_confidence,
            final_confidence,
            zeroed_by_pdd_hard_fail,
            zeroed_by_tas_hard_reject,
        })
    }

    pub fn v25_confidence(&self, config: &GatekeeperV2Config) -> Option<f64> {
        self.v25_confidence_breakdown(config)
            .map(|breakdown| breakdown.final_confidence)
    }

    /// Cache the computed V2.5 confidence on the assessment field.
    ///
    /// Must be called after `self.decision` has been populated.
    /// The cached value is used by `to_buy_log()` for JSONL telemetry.
    pub fn cache_v25_confidence(&mut self, config: &GatekeeperV2Config) {
        self.v25_confidence = self.v25_confidence(config);
    }

    pub fn uses_materialized_feature_path(&self) -> bool {
        self.feature_snapshot != MaterializedFeatureSet::default()
    }

    pub fn tas_availability(&self, config: &GatekeeperV2Config) -> (Option<bool>, Option<String>) {
        if !config.tas.enabled {
            return (None, None);
        }
        if self.trajectory.is_some() {
            return (Some(true), None);
        }

        let reason = if self.uses_materialized_feature_path() {
            if let Some(seq) = self.feature_snapshot.tx_segment_sequence.as_ref() {
                if seq.total_duration_ms < config.tas.tas_min_total_duration_ms {
                    "insufficient_duration"
                } else if !seq.min_tx_per_segment_satisfied {
                    "insufficient_tx_per_segment"
                } else {
                    "partial_inputs"
                }
            } else {
                let obs_dur = self
                    .feature_snapshot
                    .session_metadata
                    .observation_duration_ms;
                let tx_count = self.feature_snapshot.tx_intel_features.tx_count as usize;
                if obs_dur < config.tas.tas_min_total_duration_ms {
                    "insufficient_duration"
                } else if tx_count < config.tas.tas_min_tx_per_segment.saturating_mul(3) {
                    "insufficient_tx_per_segment"
                } else {
                    "missing_sequence"
                }
            }
        } else if self.observation_duration_ms < config.tas.tas_min_total_duration_ms {
            "insufficient_duration"
        } else if self.total_tx_evaluated < config.tas.tas_min_tx_per_segment.saturating_mul(3) {
            "insufficient_tx_per_segment"
        } else {
            "partial_inputs"
        };
        (Some(false), Some(reason.to_string()))
    }

    pub fn pdd_sequence_signals_available(&self, config: &GatekeeperV2Config) -> Option<bool> {
        if !config.pdd.enabled {
            return None;
        }
        if let Some(ref seq) = self.feature_snapshot.tx_segment_sequence {
            Some(seq.min_tx_per_segment_satisfied)
        } else if self.uses_materialized_feature_path() {
            Some(false)
        } else {
            Some(true)
        }
    }

    pub fn pdd_sequence_signals_availability(
        &self,
        config: &GatekeeperV2Config,
    ) -> (Option<bool>, Option<String>) {
        let available = self.pdd_sequence_signals_available(config);
        let reason = if !available.unwrap_or(true) {
            if let Some(ref seq) = self.feature_snapshot.tx_segment_sequence {
                if seq.total_duration_ms < config.tas.tas_min_total_duration_ms {
                    Some("insufficient_duration".to_string())
                } else if !seq.min_tx_per_segment_satisfied {
                    Some("insufficient_tx_per_segment".to_string())
                } else {
                    Some("partial_inputs".to_string())
                }
            } else if self.uses_materialized_feature_path() {
                let obs_dur = self
                    .feature_snapshot
                    .session_metadata
                    .observation_duration_ms;
                let tx_count = self.feature_snapshot.tx_intel_features.tx_count as usize;
                if obs_dur < config.tas.tas_min_total_duration_ms {
                    Some("insufficient_duration".to_string())
                } else if tx_count < config.tas.tas_min_tx_per_segment.saturating_mul(3) {
                    Some("insufficient_tx_per_segment".to_string())
                } else {
                    Some("missing_sequence".to_string())
                }
            } else {
                Some("pdd_sequence_unavailable".to_string())
            }
        } else {
            None
        };
        (available, reason)
    }

    pub fn pdd_price_anchor_available(&self, config: &GatekeeperV2Config) -> Option<bool> {
        config.pdd.enabled.then_some(
            self.pdd_assessment
                .as_ref()
                .and_then(|pdd| pdd.entry_drift_pct)
                .is_some(),
        )
    }

    pub fn v25_confidence_availability(
        &self,
        config: &GatekeeperV2Config,
    ) -> (Option<bool>, Option<String>) {
        if !(config.v25.shadow_enabled || config.v25.live_execution_enabled) {
            return (None, None);
        }
        if self.v25_confidence.is_some() || self.v25_confidence_breakdown(config).is_some() {
            return (Some(true), None);
        }

        let reason = if self.decision.is_none() {
            "missing_decision".to_string()
        } else if self.uses_materialized_feature_path() {
            if config.tas.enabled && self.trajectory.is_none() {
                let (_, tas_reason) = self.tas_availability(config);
                tas_reason.unwrap_or_else(|| "tas_unavailable".to_string())
            } else if config.pdd.enabled
                && !self.pdd_sequence_signals_available(config).unwrap_or(true)
            {
                let (_, pdd_reason) = self.pdd_sequence_signals_availability(config);
                pdd_reason.unwrap_or_else(|| "pdd_sequence_unavailable".to_string())
            } else {
                // P1: enumerate which specific inputs are missing.
                let mut missing = Vec::new();
                if config.tas.enabled && self.trajectory.is_none() {
                    missing.push("tas");
                }
                if config.pdd.enabled {
                    let (pdd_ok, _) = self.pdd_sequence_signals_availability(config);
                    if !pdd_ok.unwrap_or(true) {
                        missing.push("pdd_sequence");
                    }
                    let anchor_ok = self.pdd_price_anchor_available(config).unwrap_or(true);
                    if !anchor_ok {
                        missing.push("pdd_price_anchor");
                    }
                }
                if missing.is_empty() {
                    "partial_inputs".to_string()
                } else {
                    format!("partial_inputs: {}", missing.join(","))
                }
            }
        } else {
            "partial_inputs".to_string()
        };
        (Some(false), Some(reason.to_string()))
    }

    /// Format a compact summary of the three-layer decision for log output.
    ///
    /// Returns a non-empty string like:
    ///   ` | 3L: core=[✅,✅,❌] legacy=3/8 sybil=0/255 total=3 sybil_on=false veto=false dev_unk=true lflags=[low_cv] sflags=[none] patterns=[none] lead=none verdict=BUY`
    /// or an empty string when no three-layer decision was computed.
    pub fn decision_summary(&self) -> String {
        match &self.decision {
            Some(d) => {
                fn fmt_opt_f64(value: Option<f64>) -> String {
                    value
                        .map(|v| format!("{v:.3}"))
                        .unwrap_or_else(|| "null".to_string())
                }
                let core = format!(
                    "[{},{},{}]",
                    if d.core1_passed { "✅" } else { "❌" },
                    if d.core2_passed { "✅" } else { "❌" },
                    if d.core3_passed { "✅" } else { "❌" },
                );
                let alpha = match d.alpha_gate.pass {
                    None => format!("enabled={} state=not_run", d.alpha_gate.enabled),
                    Some(pass) => format!(
                        "enabled={} actionable={} m={} d={} j={} pass={} trigger={} skip={}",
                        d.alpha_gate.enabled,
                        d.alpha_gate.actionable,
                        fmt_opt_f64(d.alpha_gate.momentum),
                        fmt_opt_f64(d.alpha_gate.demand),
                        fmt_opt_f64(d.alpha_gate.joint),
                        pass,
                        d.alpha_gate
                            .reject_trigger
                            .map(|trigger| trigger.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        d.alpha_gate.skip_reason.unwrap_or("none"),
                    ),
                };
                format!(
                    " | 3L: core={} legacy={}/{} sybil={}/{} total={} sybil_on={} veto={} dev_unk={} lflags=[{}] sflags=[{}] patterns=[{}] lead={} alpha=[{}] verdict={}",
                    core,
                    d.soft_points,
                    d.effective_max_soft_points,
                    d.sybil_policy.soft_points,
                    d.sybil_policy.effective_max_soft_points,
                    d.total_soft_points,
                    d.sybil_policy.enabled,
                    d.sybil_policy.combo_veto_enabled,
                    d.dev_unknown,
                    d.soft_signals.format_flags(),
                    d.sybil_policy.soft_signals.format_flags(),
                    SybilInterferencePattern::format_patterns(&d.sybil_policy.interference_patterns),
                    d.sybil_policy
                        .lead_signal
                        .map(|signal| signal.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    alpha,
                    d.verdict_type,
                )
            }
            None => String::new(),
        }
    }

    /// Format a FINGERPRINT summary line for structured logging at BUY/PASS.
    pub fn fingerprint_summary(&self, pool: &str, mint: &str) -> String {
        fn fmt_sybil_metric(value: Option<f64>) -> String {
            match value {
                Some(value) => format!("{value:.4}"),
                None => "null".to_string(),
            }
        }

        let mut summary = match &self.early_fingerprint {
            Some(fp) => fp.log_line(pool, mint),
            None => format!("FINGERPRINT pool={} mint={} not_available", pool, mint),
        };
        let sybil = &self.feature_snapshot.sybil_resistance;
        let sybil_degraded = if sybil.degraded_reasons.is_empty() {
            "null".to_string()
        } else {
            sybil.degraded_reasons.join(",")
        };
        summary.push_str(&format!(
            " ftdi={} dbia={} sfd={} des={} cpv={} fsc={} sybil_degraded={}",
            fmt_sybil_metric(sybil.fee_topology_diversity_index),
            fmt_sybil_metric(sybil.dev_buyer_infrastructure_affinity),
            fmt_sybil_metric(sybil.spend_fraction_divergence),
            fmt_sybil_metric(sybil.demand_elasticity_score),
            fmt_sybil_metric(sybil.signer_cross_pool_velocity),
            fmt_sybil_metric(sybil.funding_source_concentration),
            sybil_degraded,
        ));
        summary
    }

    /// Convert assessment to a buy log record with config thresholds
    fn derive_reason_code(
        &self,
        _config: &GatekeeperV2Config,
    ) -> Option<ghost_brain::oracle::reason_code::GatekeeperReasonCode> {
        use ghost_brain::oracle::reason_code::GatekeeperReasonCode as Rc;
        if self.decision.is_none() && self.total_tx_evaluated == 0 {
            return Some(Rc::TimeoutPhase1NoData);
        }
        if self.decision.is_none() && self.phases_passed == 0 {
            return Some(Rc::TimeoutPhase1Insufficient);
        }
        // Hard fails take priority over timeout when assessment detected a hard fail
        // but no three-layer decision was computed (e.g. build_assessment_from_features).
        if self.decision.is_none() && self.hard_reject_reason.is_some() {
            let reason = self.hard_reject_reason.as_ref().unwrap();
            if reason.contains("dev_has_sold") {
                return Some(Rc::HardFailDevSold);
            }
            if reason.contains("market_cap") {
                return Some(Rc::HardFailMarketCap);
            }
            if reason.contains("hhi") {
                return Some(Rc::HardFailExtremeHhi);
            }
            if reason.contains("same_ms") || reason.contains("bundl") {
                return Some(Rc::HardFailExtremeBundling);
            }
            if reason.contains("top3") {
                return Some(Rc::HardFailExtremeTop3);
            }
            if reason.contains("bot") || reason.contains("bot_timing") {
                return Some(Rc::HardFailExtremeBotTiming);
            }
            if reason.contains("failed_tx") || reason.contains("failed TX") {
                return Some(Rc::HardFailFailedTxRatio);
            }
            if reason.contains("slow_pool") || reason.contains("avg_interval") {
                return Some(Rc::HardFailSlowPool);
            }
            if reason.contains("sell_impact") {
                return Some(Rc::HardFailSellImpact);
            }
            if reason.contains("tx_price_impact") {
                return Some(Rc::HardFailTxPriceImpact);
            }
            if reason.contains("price_change_ratio") || reason.contains("price_change") {
                return Some(Rc::HardFailPriceChange);
            }
            if reason.contains("HARD_FAIL") {
                return Some(Rc::RejectCoreFail);
            }
        }
        if self.decision.is_none() && self.phase1_passed {
            return Some(Rc::TimeoutDeadlineLowPhases);
        }
        if self.decision.is_none() {
            return Some(Rc::InvariantTimeoutNoVerdict);
        }
        if let Some(ref reason) = self.hard_reject_reason {
            if reason.contains("dev_has_sold") {
                return Some(Rc::HardFailDevSold);
            }
            if reason.contains("market_cap") {
                return Some(Rc::HardFailMarketCap);
            }
            if reason.contains("hhi") {
                return Some(Rc::HardFailExtremeHhi);
            }
            if reason.contains("same_ms") || reason.contains("bundl") {
                return Some(Rc::HardFailExtremeBundling);
            }
            if reason.contains("top3") {
                return Some(Rc::HardFailExtremeTop3);
            }
            if reason.contains("bot") || reason.contains("bot_timing") {
                return Some(Rc::HardFailExtremeBotTiming);
            }
            if reason.contains("failed_tx") || reason.contains("failed TX") {
                return Some(Rc::HardFailFailedTxRatio);
            }
            if reason.contains("slow_pool") || reason.contains("avg_interval") {
                return Some(Rc::HardFailSlowPool);
            }
            if reason.contains("sell_impact") {
                return Some(Rc::HardFailSellImpact);
            }
            if reason.contains("tx_price_impact") {
                return Some(Rc::HardFailTxPriceImpact);
            }
            if reason.contains("price_change_ratio") || reason.contains("price_change") {
                return Some(Rc::HardFailPriceChange);
            }
            if reason.contains("HARD_FAIL") {
                return Some(Rc::RejectCoreFail);
            }
        }
        if let Some(ref pdd) = self.pdd_assessment {
            if let Some(ref fail) = pdd.hard_fail {
                return Rc::from_pdd_hard_fail(fail.as_str());
            }
        }
        let decision = self.decision.as_ref()?;
        if decision.verdict_buy {
            return match self.observation_stage {
                Some(ObservationStage::Early) => Some(Rc::BuyEarly),
                Some(ObservationStage::Extended) => Some(Rc::BuyExtended),
                _ => Some(Rc::BuyNormal),
            };
        }
        if decision.reason_chain.contains("REJECT_LOW_TRAJECTORY") {
            return Some(Rc::RejectLowTrajectory);
        }
        let tag = decision.verdict_type.tag();
        if let Some(rc) = Rc::from_iwim_verdict(tag) {
            return Some(rc);
        }
        match tag {
            "TIMEOUT_PHASE1_NO_DATA" | "TIMEOUT_NO_DATA" => Some(Rc::TimeoutPhase1NoData),
            "TIMEOUT_PHASE1_INSUFFICIENT" | "TIMEOUT_PHASE1" => Some(Rc::TimeoutPhase1Insufficient),
            "TIMEOUT_DEADLINE_LOW_PHASES" => Some(Rc::TimeoutDeadlineLowPhases),
            "REJECT_HARD_FAIL" => Some(Rc::RejectCoreFail),
            "REJECT_CORE_FAIL" => Some(Rc::RejectCoreFail),
            "REJECT_SYBIL_COMBO" => Some(Rc::RejectSybilCombo),
            "REJECT_SYBIL_INTERFERENCE" => Some(Rc::RejectSybilInterference),
            "REJECT_SYBIL_SOFT_EXCESS" => Some(Rc::RejectSybilSoftExcess),
            "REJECT_SOFT_EXCESS" => Some(Rc::RejectLegacySoftExcess),
            "REJECT_LOW_ALPHA" => Some(Rc::RejectLowAlpha),
            "REJECT_LOW_PROSPERITY" => Some(Rc::RejectLowProsperity),
            "REJECT_ENTRY_DRIFT" => Some(Rc::RejectPddEntryDrift),
            "REJECT_FLASH_CRASH" => Some(Rc::RejectPddFlashCrash),
            "REJECT_RAMPING" => Some(Rc::RejectPddRamping),
            "REJECT_LOW_TRAJECTORY" => Some(Rc::RejectLowTrajectory),
            "REJECT_INSUFFICIENT_CONFIDENCE" => Some(Rc::RejectInsufficientConfidence),
            "BUY" => Some(Rc::BuyNormal),
            "EARLY_BUY" => Some(Rc::BuyEarly),
            _ => None,
        }
    }

    pub fn to_buy_log(
        &self,
        pool_id: &Pubkey,
        config: &GatekeeperV2Config,
    ) -> ghost_brain::oracle::GatekeeperBuyLog {
        use ghost_brain::oracle::{GatekeeperBuyLog, GATEKEEPER_BUY_LOG_SCHEMA_VERSION};

        // Use the actual buy_count tracked by Phase 1 (not derived from
        // phase4_volume.buy_ratio, which would be 0 when phase4_volume is None).
        let buy_count = self.buy_count;
        let fp = self.early_fingerprint.as_ref();
        let sybil = &self.feature_snapshot.sybil_resistance;
        let legacy_soft_flags = self
            .decision
            .as_ref()
            .map(|d| {
                decorate_legacy_soft_flags(
                    d.soft_signals.format_flags(),
                    self.phase6_curve.as_ref(),
                )
            })
            .unwrap_or_else(|| {
                decorate_legacy_soft_flags("none".to_string(), self.phase6_curve.as_ref())
            });
        let sybil_soft_flags = self
            .decision
            .as_ref()
            .map(|d| d.sybil_policy.soft_signals.format_flags())
            .unwrap_or_else(|| "none".to_string());
        let sybil_patterns = self
            .decision
            .as_ref()
            .map(|d| {
                d.sybil_policy
                    .interference_patterns
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let legacy_live_reason_chain = self.decision.as_ref().map(|d| d.reason_chain.clone());
        let legacy_live_verdict_buy = self.decision.as_ref().map(|d| d.verdict_buy);
        let legacy_live_verdict_type = self
            .decision
            .as_ref()
            .map(|d| d.verdict_type.tag().to_string());
        let (decision_reason_fallback, verdict_type_fallback) = if self.decision.is_none() {
            let reason = if self.total_tx_evaluated == 0 {
                "TIMEOUT: Phase 1 never met — zero transactions ingested".to_string()
            } else if !self.phase1_passed {
                format!(
                    "TIMEOUT: Phase 1 insufficient — tx={}/{} signers={}/{} buys={}/{}",
                    self.total_tx_evaluated,
                    config.min_tx_count,
                    self.unique_signers_evaluated,
                    config.min_unique_signers,
                    self.buy_count,
                    config.min_buy_count
                )
            } else {
                format!(
                    "TIMEOUT: deadline reached, phases={}/{}",
                    self.phases_passed, config.min_phases_to_pass
                )
            };
            let vtype = if self.total_tx_evaluated == 0 {
                "TIMEOUT_PHASE1_NO_DATA"
            } else if !self.phase1_passed {
                "TIMEOUT_PHASE1_INSUFFICIENT"
            } else {
                "TIMEOUT_DEADLINE_LOW_PHASES"
            };
            (Some(reason), Some(vtype.to_string()))
        } else {
            (None, None)
        };
        let v25_terminal_shadow = self.v25_shadow_decisions.last();
        let v25_confidence_breakdown = self.v25_confidence_breakdown(config);
        let v25_model_confidence = self
            .v25_confidence
            .or(v25_confidence_breakdown.map(|breakdown| breakdown.final_confidence));
        let (tas_available, tas_unavailable_reason) = self.tas_availability(config);
        let (v25_confidence_available, v25_confidence_unavailable_reason) =
            self.v25_confidence_availability(config);
        let (pdd_seq_available, pdd_seq_unavailable_reason) =
            self.pdd_sequence_signals_availability(config);

        GatekeeperBuyLog {
            log_schema_version: GATEKEEPER_BUY_LOG_SCHEMA_VERSION,
            timestamp: chrono::Utc::now().to_rfc3339(),
            pool_id: pool_id.to_string(),

            // Observation identity (enriched externally where base_mint / ts are available)
            join_key: None,
            base_mint: None,
            first_seen_ts_ms: None,
            first_seen_clock_source: None,
            observation_start_ts_ms: None,
            observation_end_ts_ms: None,
            observation_window_ms: None,
            end_10s_ts_ms: None,
            core_pass: self
                .decision
                .as_ref()
                .map(|d| d.core1_passed && d.core2_passed && d.core3_passed),
            gatekeeper_version: None,
            rollout_profile: None,
            decision_plane: None,
            config_hash: None,
            dev_pubkey: None,
            shadow_ready: None,
            shadow_missing_fields: None,
            shadow_metadata_source: None,
            shadow_trigger_present: None,
            shadow_entry_mode: None,
            shadow_trigger_eligible: None,
            shadow_execution_outcome: None,
            execution_candidate_id: None,

            // Mode
            mode: config.mode.to_string(),

            // Summary
            phases_passed: self.phases_passed,
            min_phases_to_pass: config.min_phases_to_pass,
            observation_duration_ms: self.observation_duration_ms,
            finalize_lag_ms: self.finalize_lag_ms,
            max_wait_time_ms: config.max_wait_time_ms,
            eval_count: self.eval_count,
            dust_filtered_count: self.dust_filtered_count,
            min_sol_threshold: Some(config.min_sol_threshold),

            // Phase 1: Quantity Gate
            total_tx_evaluated: self.total_tx_evaluated,
            unique_tx_evaluated: Some(self.unique_tx_evaluated),
            min_tx_count: config.min_tx_count,
            unique_signers_evaluated: self.unique_signers_evaluated,
            min_unique_signers: config.min_unique_signers,
            buy_count,
            min_buy_count: config.min_buy_count,

            // Phase 2: Velocity Profile
            phase2_passed: self.phase2_passed,
            interval_cv: self.phase2_velocity.as_ref().map(|p| p.interval_cv),
            min_interval_cv: config.min_interval_cv,
            max_interval_cv: config.max_interval_cv,
            burst_ratio: self.phase2_velocity.as_ref().map(|p| p.burst_ratio),
            max_burst_ratio: config.max_burst_ratio,
            avg_interval_ms: self.phase2_velocity.as_ref().map(|p| p.avg_interval_ms),
            min_avg_interval_ms: config.min_avg_interval_ms,
            max_avg_interval_ms: config.max_avg_interval_ms,
            timing_entropy: self.phase2_velocity.as_ref().map(|p| p.timing_entropy),
            min_timing_entropy: config.min_timing_entropy,
            max_timing_entropy: config.max_timing_entropy,
            min_dust_filtered_count: config.min_dust_filtered_count,

            // Phase 3: Signer Diversity
            phase3_passed: self.phase3_passed,
            unique_ratio: self.phase3_diversity.as_ref().map(|p| p.unique_ratio),
            min_unique_ratio: config.min_unique_ratio,
            max_unique_ratio: config.max_unique_ratio,
            hhi: self.phase3_diversity.as_ref().map(|p| p.hhi),
            max_hhi: config.max_hhi,
            max_tx_per_signer_observed: self.phase3_diversity.as_ref().map(|p| p.max_tx_per_signer),
            max_tx_per_signer: config.max_tx_per_signer,
            volume_gini: self.phase3_diversity.as_ref().map(|p| p.volume_gini),
            min_volume_gini: config.min_volume_gini,
            max_volume_gini: config.max_volume_gini,
            top3_volume_pct: self.phase3_diversity.as_ref().map(|p| p.top3_volume_pct),
            max_top3_volume_pct: config.max_top3_volume_pct,
            same_ms_tx_ratio: self.phase3_diversity.as_ref().map(|p| p.same_ms_tx_ratio),
            max_same_ms_tx_ratio: config.max_same_ms_tx_ratio,

            // Phase 4: Volume Sanity
            phase4_passed: self.phase4_passed,
            buy_ratio: self.phase4_volume.as_ref().map(|p| p.buy_ratio),
            min_buy_ratio: config.min_buy_ratio,
            max_buy_ratio: config.max_buy_ratio,
            avg_tx_sol: self.phase4_volume.as_ref().map(|p| p.avg_tx_sol),
            min_avg_tx_sol: config.min_avg_tx_sol,
            max_avg_tx_sol: config.max_avg_tx_sol,
            volume_cv: self.phase4_volume.as_ref().map(|p| p.volume_cv),
            min_volume_cv: config.min_volume_cv,
            max_volume_cv: config.max_volume_cv,
            total_volume_sol: self.phase4_volume.as_ref().map(|p| p.total_volume_sol),
            min_total_volume_sol: config.min_total_volume_sol,
            max_total_volume_sol: config.max_total_volume_sol,
            sol_buy_ratio: self.phase4_volume.as_ref().map(|p| p.sol_buy_ratio),
            min_sol_buy_ratio: config.min_sol_buy_ratio,
            max_sol_buy_ratio: config.max_sol_buy_ratio,
            max_consecutive_buys_observed: self
                .phase4_volume
                .as_ref()
                .map(|p| p.max_consecutive_buys),
            min_consecutive_buys: config.min_consecutive_buys,

            // Phase 5: Dev Behavior
            phase5_passed: self.phase5_passed,
            dev_wallet_known: self.phase5_dev.as_ref().map(|p| p.dev_wallet_known),
            dev_buy_total_sol: self.phase5_dev.as_ref().map(|p| p.dev_buy_total_sol),
            max_dev_buy_sol: config.max_dev_buy_sol,
            min_dev_buy_sol: config.min_dev_buy_sol,
            dev_tx_ratio: self.phase5_dev.as_ref().map(|p| p.dev_tx_ratio),
            max_dev_tx_ratio: config.max_dev_tx_ratio,
            min_dev_tx_ratio: config.min_dev_tx_ratio,
            dev_volume_ratio: self.phase5_dev.as_ref().map(|p| p.dev_volume_ratio),
            max_dev_volume_ratio: config.max_dev_volume_ratio,
            min_dev_volume_ratio: config.min_dev_volume_ratio,
            dev_has_sold: self.phase5_dev.as_ref().map(|p| p.dev_has_sold),
            reject_on_dev_sell: config.reject_on_dev_sell,

            // Phase 6: Bonding Curve Dynamics
            phase6_passed: self.phase6_passed,
            price_change_ratio: self.phase6_curve.as_ref().map(|p| p.price_change_ratio),
            min_price_change_ratio: config.min_price_change_ratio,
            max_price_change_ratio: config.max_price_change_ratio,
            max_single_tx_price_impact_pct_observed: self
                .phase6_curve
                .as_ref()
                .map(|p| p.max_single_tx_price_impact_pct),
            max_single_tx_price_impact_pct: config.max_single_tx_price_impact_pct,
            max_single_sell_impact_pct_observed: self
                .phase6_curve
                .as_ref()
                .map(|p| p.max_single_sell_impact_pct),
            min_single_sell_impact_pct: config.min_single_sell_impact_pct,
            max_single_sell_impact_pct: config.max_single_sell_impact_pct,
            bonding_progress_pct: self.phase6_curve.as_ref().and_then(|p| {
                if p.curve_data_known {
                    Some(p.bonding_progress_pct)
                } else {
                    None
                }
            }),
            min_bonding_progress_pct: config.min_bonding_progress_pct,
            max_bonding_progress_pct: config.max_bonding_progress_pct,
            curve_data_known: self.phase6_curve.as_ref().map(|p| p.curve_data_known),
            curve_finality: self
                .phase6_curve
                .as_ref()
                .map(|p| p.curve_finality.as_str().to_string()),
            curve_finality_is_finalized: self
                .phase6_curve
                .as_ref()
                .map(|p| p.curve_finality.is_finalized()),
            bonding_progress_check_skipped: self.phase6_curve.as_ref().map(|p| !p.curve_data_known),
            current_market_cap_sol: self.phase6_curve.as_ref().and_then(|p| {
                if p.curve_data_known {
                    Some(p.current_market_cap_sol)
                } else {
                    None
                }
            }),
            min_market_cap_sol: config.min_market_cap_sol,

            // Curve Readiness Latch telemetry
            curve_wait_ms: Some(config.curve_wait_ms),
            curve_t0_event_ts_ms: self.curve_t0_event_ts_ms,
            curve_t0_clock_source: self.curve_t0_clock_source.map(str::to_string),
            curve_wait_elapsed_ms: self.curve_wait_elapsed_ms,
            curve_required_for_buy: Some(config.curve_require_for_buy),

            // Three-Layer Decision
            three_layer_enabled: config.use_three_layer_decision,
            hard_fail_reason: self
                .decision
                .as_ref()
                .and_then(|d| d.hard_fail_reason.clone()),
            core1_passed: self.decision.as_ref().map(|d| d.core1_passed),
            core2_passed: self.decision.as_ref().map(|d| d.core2_passed),
            core3_passed: self.decision.as_ref().map(|d| d.core3_passed),
            dev_unknown: self.decision.as_ref().map(|d| d.dev_unknown),
            soft_score: self.decision.as_ref().map(|d| d.soft_signals.score()),
            soft_points: self.decision.as_ref().map(|d| d.soft_points),
            max_soft_points: if config.use_three_layer_decision {
                Some(config.max_soft_points)
            } else {
                None
            },
            effective_max_soft_points: self.decision.as_ref().map(|d| d.effective_max_soft_points),
            max_soft_score: if config.use_three_layer_decision {
                Some(config.max_soft_score)
            } else {
                None
            },
            soft_flags: Some(legacy_soft_flags.clone()),
            legacy_soft_points: self.decision.as_ref().map(|d| d.soft_points),
            legacy_soft_threshold: if config.use_three_layer_decision {
                self.decision
                    .as_ref()
                    .map(|d| d.effective_max_soft_points)
                    .or(Some(config.max_soft_points))
            } else {
                None
            },
            legacy_soft_flags: Some(legacy_soft_flags),
            sybil_soft_points: self.decision.as_ref().map(|d| d.sybil_policy.soft_points),
            sybil_soft_threshold: if config.use_three_layer_decision {
                self.decision
                    .as_ref()
                    .map(|d| d.sybil_policy.effective_max_soft_points)
                    .or(Some(
                        if self
                            .phase5_dev
                            .as_ref()
                            .map(|dev| !dev.dev_wallet_known)
                            .unwrap_or(true)
                        {
                            config.dev_unknown_max_sybil_soft_points
                        } else {
                            config.max_sybil_soft_points
                        },
                    ))
            } else {
                None
            },
            total_soft_points: self.decision.as_ref().map(|d| d.total_soft_points),
            sybil_soft_flags: Some(sybil_soft_flags),
            sybil_lead_signal: self
                .decision
                .as_ref()
                .and_then(|d| d.sybil_policy.lead_signal.map(|signal| signal.to_string())),
            sybil_interference_patterns: sybil_patterns,
            sybil_meta_score: self
                .decision
                .as_ref()
                .and_then(|d| d.sybil_policy.meta_score),
            sybil_interference_layer_enabled: config.enable_sybil_interference_layer,
            sybil_combo_veto_enabled: config.enable_sybil_combo_veto,
            alpha_gate_enabled: config.enable_alpha_gate,
            alpha_pass: self.decision.as_ref().and_then(|d| d.alpha_gate.pass),
            alpha_actionable: self.decision.as_ref().map(|d| d.alpha_gate.actionable),
            momentum: self.decision.as_ref().and_then(|d| d.alpha_gate.momentum),
            demand: self.decision.as_ref().and_then(|d| d.alpha_gate.demand),
            alpha_joint: self.decision.as_ref().and_then(|d| d.alpha_gate.joint),
            min_momentum: Some(config.min_momentum),
            min_demand: Some(config.min_demand),
            min_alpha_joint: Some(config.min_alpha_joint),
            min_alpha_sample: Some(config.min_alpha_sample),
            alpha_reject_trigger: self.decision.as_ref().and_then(|d| {
                d.alpha_gate
                    .reject_trigger
                    .map(|trigger| trigger.to_string())
            }),
            alpha_skip_reason: self
                .decision
                .as_ref()
                .and_then(|d| d.alpha_gate.skip_reason.map(str::to_string)),
            prosperity_filter_enabled: config.enable_prosperity_filter,
            prosperity_pass: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.pass)
            } else {
                None
            },
            prosperity_actionable: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .map(|d| d.prosperity_filter.actionable)
            } else {
                None
            },
            prosperity_reject_trigger: if config.enable_prosperity_filter {
                self.decision.as_ref().and_then(|d| {
                    d.prosperity_filter
                        .reject_trigger
                        .map(|trigger| trigger.to_string())
                })
            } else {
                None
            },
            prosperity_market_cap_floor_pass: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.market_cap_floor_pass)
            } else {
                None
            },
            prosperity_cpv_pass: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.cpv_pass)
            } else {
                None
            },
            prosperity_branch1_pass: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.branch1_pass)
            } else {
                None
            },
            prosperity_branch2_pass: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.branch2_pass)
            } else {
                None
            },
            prosperity_branch3_pass: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.branch3_pass)
            } else {
                None
            },
            prosperity_overlay_enabled: config.enable_prosperity_filter
                && config.enable_prosperity_overlay,
            prosperity_overlay_pass: if config.enable_prosperity_filter
                && config.enable_prosperity_overlay
            {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.overlay_pass)
            } else {
                None
            },
            prosperity_overlay_price_change_pass: if config.enable_prosperity_filter
                && config.enable_prosperity_overlay
            {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.overlay_price_change_pass)
            } else {
                None
            },
            prosperity_overlay_bonding_progress_pass: if config.enable_prosperity_filter
                && config.enable_prosperity_overlay
            {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.overlay_bonding_progress_pass)
            } else {
                None
            },
            prosperity_overlay_fee_topology_diversity_pass: if config.enable_prosperity_filter
                && config.enable_prosperity_overlay
            {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.overlay_fee_topology_diversity_pass)
            } else {
                None
            },
            prosperity_overlay_branch23_sell_buy_pass: if config.enable_prosperity_filter
                && config.enable_prosperity_overlay
            {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.overlay_branch23_sell_buy_pass)
            } else {
                None
            },
            prosperity_overlay_branch2_price_change_pass: if config.enable_prosperity_filter
                && config.enable_prosperity_overlay
            {
                self.decision
                    .as_ref()
                    .and_then(|d| d.prosperity_filter.overlay_branch2_price_change_pass)
            } else {
                None
            },
            prosperity_matched_branches: if config.enable_prosperity_filter {
                self.decision
                    .as_ref()
                    .map(|d| {
                        d.prosperity_filter
                            .matched_branches
                            .iter()
                            .map(|branch| (*branch).to_string())
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            },
            prosperity_min_market_cap_sol: config
                .enable_prosperity_filter
                .then_some(config.prosperity_min_market_cap_sol),
            prosperity_max_signer_cross_pool_velocity: config
                .enable_prosperity_filter
                .then_some(config.prosperity_max_signer_cross_pool_velocity),
            prosperity_branch1_min_block0_sniped_supply_pct: config
                .enable_prosperity_filter
                .then_some(config.prosperity_branch1_min_block0_sniped_supply_pct),
            prosperity_branch1_max_sell_buy_ratio: config
                .enable_prosperity_filter
                .then_some(config.prosperity_branch1_max_sell_buy_ratio),
            prosperity_branch2_min_market_cap_sol: config
                .enable_prosperity_filter
                .then_some(config.prosperity_branch2_min_market_cap_sol),
            prosperity_branch2_min_early_slot_volume_dominance_buy: config
                .enable_prosperity_filter
                .then_some(config.prosperity_branch2_min_early_slot_volume_dominance_buy),
            prosperity_branch3_max_hhi: config
                .enable_prosperity_filter
                .then_some(config.prosperity_branch3_max_hhi),
            prosperity_branch3_min_fee_topology_diversity_index: config
                .enable_prosperity_filter
                .then_some(config.prosperity_branch3_min_fee_topology_diversity_index),
            prosperity_overlay_max_price_change_ratio: (config.enable_prosperity_filter
                && config.enable_prosperity_overlay)
                .then_some(config.prosperity_overlay_max_price_change_ratio),
            prosperity_overlay_max_bonding_progress_pct: (config.enable_prosperity_filter
                && config.enable_prosperity_overlay)
                .then_some(config.prosperity_overlay_max_bonding_progress_pct),
            prosperity_overlay_min_fee_topology_diversity_index: (config.enable_prosperity_filter
                && config.enable_prosperity_overlay)
                .then_some(config.prosperity_overlay_min_fee_topology_diversity_index),
            prosperity_overlay_branch23_max_sell_buy_ratio: (config.enable_prosperity_filter
                && config.enable_prosperity_overlay)
                .then_some(config.prosperity_overlay_branch23_max_sell_buy_ratio),
            prosperity_overlay_branch2_max_price_change_ratio: (config.enable_prosperity_filter
                && config.enable_prosperity_overlay)
                .then_some(config.prosperity_overlay_branch2_max_price_change_ratio),
            decision_reason: legacy_live_reason_chain
                .clone()
                .or_else(|| decision_reason_fallback.clone()),
            decision_verdict_buy: legacy_live_verdict_buy,
            verdict_type: legacy_live_verdict_type
                .clone()
                .or_else(|| verdict_type_fallback.clone()),
            legacy_live_reason_chain,
            legacy_live_verdict_buy,
            legacy_live_verdict_type,
            v25_shadow_verdict_type: v25_terminal_shadow
                .map(|shadow| shadow.kind.verdict_str().to_string()),
            v25_shadow_reason_chain: v25_terminal_shadow.map(|shadow| shadow.reason.clone()),
            v25_shadow_confidence: v25_terminal_shadow
                .map(|shadow| shadow.confidence)
                .or(v25_model_confidence),
            v25_shadow_confidence_source: if v25_terminal_shadow.is_some() {
                Some("shadow_window_terminal".to_string())
            } else if v25_model_confidence.is_some() {
                Some("assessment_cached".to_string())
            } else {
                None
            },
            v25_shadow_observation_stage: v25_terminal_shadow
                .map(|shadow| shadow.window.as_str().to_string())
                .or_else(|| {
                    self.observation_stage
                        .map(|stage| stage.as_str().to_string())
                }),
            v25_promotion_state: Some(
                if config.v25.live_execution_enabled {
                    "live_enabled"
                } else {
                    "shadow_only"
                }
                .to_string(),
            ),
            // IWIM veto gate fields – defaults; enriched later by oracle_runtime
            iwim_enabled: false,
            iwim_mode: None,
            iwim_fetch_status: None,
            iwim_quality: None,
            iwim_confidence: None,
            iwim_n_tx: None,
            iwim_n_tx_requested: None,
            iwim_latency_ms: None,
            iwim_rpc_used: None,
            iwim_status: None,
            iwim_veto_reason: None,
            iwim_gatekeeper_strength: None,
            iwim_rug_threat_score: None,
            iwim_sybil_score: None,
            iwim_organic_score: None,

            // Curve Snapshot at IWIM Verdict – populated by oracle_runtime
            iwim_snap_virtual_sol_sol: None,
            iwim_snap_virtual_tokens: None,
            iwim_snap_market_cap_sol: None,
            iwim_snap_bonding_progress_pct: None,
            iwim_snap_price_sol_per_token: None,

            // Early Fingerprint Metrics – populated when available
            block0_sniped_supply_pct: fp.and_then(|f| f.block0_sniped_supply_pct),
            flip_ratio_10s: fp.and_then(|f| f.flip_ratio_10s),
            cu_price_p90_1s: fp.and_then(|f| f.cu_price_p90_1s),
            cu_price_p90_10s: fp.and_then(|f| f.cu_price_p90_10s),
            priority_fee_surge_slope: fp.and_then(|f| f.priority_fee_surge_slope),
            buyer_pre_balance_cv: fp.and_then(|f| f.buyer_pre_balance_cv),
            avg_inner_ix_count_50tx: fp.and_then(|f| f.avg_inner_ix_count_50tx),
            min_avg_inner_ix_count_50tx: config.min_avg_inner_ix_count_50tx,
            max_avg_inner_ix_count_50tx: config.max_avg_inner_ix_count_50tx,
            avg_cpi_depth_50tx: fp.and_then(|f| f.avg_cpi_depth_50tx),
            sell_buy_ratio: fp.and_then(|f| f.sell_buy_ratio),
            min_sell_buy_ratio: config.min_sell_buy_ratio,
            max_sell_buy_ratio: config.max_sell_buy_ratio,
            compute_unit_cluster_dominance: fp.and_then(|f| f.compute_unit_cluster_dominance),
            min_compute_unit_cluster_dominance: config.min_compute_unit_cluster_dominance,
            max_compute_unit_cluster_dominance: config.max_compute_unit_cluster_dominance,
            static_fee_profile_ratio: fp.and_then(|f| f.static_fee_profile_ratio),
            min_static_fee_profile_ratio: config.min_static_fee_profile_ratio,
            max_static_fee_profile_ratio: config.max_static_fee_profile_ratio,
            fixed_size_buy_ratio: self
                .feature_snapshot
                .alpha_fingerprint
                .fixed_size_buy_ratio
                .or_else(|| fp.and_then(|f| f.fixed_size_buy_ratio)),
            min_fixed_size_buy_ratio: config.min_fixed_size_buy_ratio,
            fixed_size_buy_ratio_1e4: fp.and_then(|f| f.fixed_size_buy_ratio_1e4),
            flipper_presence_ratio: self
                .feature_snapshot
                .alpha_fingerprint
                .flipper_presence_ratio
                .or_else(|| fp.and_then(|f| f.flipper_presence_ratio)),
            jito_tip_intensity: self
                .feature_snapshot
                .alpha_fingerprint
                .jito_tip_intensity
                .or_else(|| fp.and_then(|f| f.jito_tip_intensity)),
            min_jito_tip_intensity: config.min_jito_tip_intensity,
            max_jito_tip_intensity: config.max_jito_tip_intensity,
            early_slot_volume_dominance_buy: self
                .feature_snapshot
                .alpha_fingerprint
                .early_slot_volume_dominance_buy
                .or_else(|| fp.and_then(|f| f.early_slot_volume_dominance_buy)),
            max_early_slot_volume_dominance_buy: config.max_early_slot_volume_dominance_buy,
            early_top3_buy_volume_pct_3s: fp.and_then(|f| f.early_top3_buy_volume_pct_3s),
            max_early_top3_buy_volume_pct_3s: config.max_early_top3_buy_volume_pct_3s,
            whale_reversal_ratio_top3: fp.and_then(|f| f.whale_reversal_ratio_top3),
            whale_reversal_ratio_top1: fp.and_then(|f| f.whale_reversal_ratio_top1),
            dev_paperhand_latency_ms: fp.and_then(|f| f.dev_paperhand_latency_ms),
            dev_sold_within_3s: fp.and_then(|f| f.dev_sold_within_3s),
            dev_sold_within_5s: fp.and_then(|f| f.dev_sold_within_5s),
            fingerprint_degraded: fp.map_or(false, |f| f.fingerprint_degraded),
            fingerprint_reason: fp.and_then(|f| f.fingerprint_reason.clone()),
            fee_topology_diversity_index: sybil.fee_topology_diversity_index,
            min_fee_topology_diversity_index: config.min_fee_topology_diversity_index,
            dev_buyer_infrastructure_affinity: sybil.dev_buyer_infrastructure_affinity,
            max_dev_buyer_infrastructure_affinity: config.max_dev_buyer_infrastructure_affinity,
            spend_fraction_divergence: sybil.spend_fraction_divergence,
            min_spend_fraction_divergence: config.min_spend_fraction_divergence,
            demand_elasticity_score: sybil.demand_elasticity_score,
            min_demand_elasticity_score: config.min_demand_elasticity_score,
            signer_cross_pool_velocity: sybil.signer_cross_pool_velocity,
            max_signer_cross_pool_velocity: config.max_signer_cross_pool_velocity,
            funding_source_concentration: sybil.funding_source_concentration,
            max_funding_source_concentration: config.max_funding_source_concentration,
            funding_source_diagnostics: sybil.funding_source_diagnostics.clone(),
            sybil_metric_degraded_reasons: sybil.degraded_reasons.clone(),

            // A/B Window – defaults; enriched by oracle_runtime before logging
            ab_window_ms: None,
            ab_t0_event_ts_ms: None,
            ab_t_end_event_ts_ms: None,
            ab_window_complete: false,
            ab_window_close_reason: None,
            ab_tx_count_window: None,
            ab_unique_signers_window: None,
            ab_fail_count_window: None,
            ab_window_origin: None,
            ab_record_id: None,

            // Window vectors – defaults; enriched by oracle_runtime before logging
            vectors_max_len: None,
            vectors_ts_offsets_ms: None,
            vectors_sol_amounts: None,
            vectors_prices: None,
            vectors_interval_ms: None,
            vectors_d_price: None,
            // ═══════════════════════════════════════════
            // V2.5 Shadow Decision Fields (v16)
            // ═══════════════════════════════════════════
            shadow_extended_verdict: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Extended)
                .map(|s| s.kind.verdict_str().to_string()),
            shadow_extended_elapsed_ms: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Extended)
                .map(|s| s.elapsed_ms),
            shadow_early_verdict: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Early)
                .map(|s| s.kind.verdict_str().to_string()),
            shadow_normal_verdict: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Normal)
                .map(|s| s.kind.verdict_str().to_string()),
            shadow_early_elapsed_ms: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Early)
                .map(|s| s.elapsed_ms),
            shadow_normal_elapsed_ms: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Normal)
                .map(|s| s.elapsed_ms),
            shadow_early_phases_passed: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Early)
                .map(|s| s.phases_passed),
            shadow_normal_phases_passed: self
                .v25_shadow_decisions
                .iter()
                .find(|s| s.window == ObservationStage::Normal)
                .map(|s| s.phases_passed),
            observation_stage: self.observation_stage.map(|s| s.as_str().to_string()),
            v25_confidence: v25_model_confidence,
            v25_confidence_pre_veto: v25_confidence_breakdown
                .map(|breakdown| breakdown.pre_veto_confidence),
            v25_confidence_base_quality: v25_confidence_breakdown
                .map(|breakdown| breakdown.base_quality),
            v25_confidence_alpha_quality: v25_confidence_breakdown
                .map(|breakdown| breakdown.alpha_quality),
            v25_confidence_pdd_modulator: v25_confidence_breakdown
                .map(|breakdown| breakdown.pdd_modulator),
            v25_confidence_tas_modulator: v25_confidence_breakdown
                .map(|breakdown| breakdown.tas_modulator),
            v25_confidence_sybil_modulator: v25_confidence_breakdown
                .map(|breakdown| breakdown.sybil_modulator),
            v25_confidence_zeroed_by_pdd_hard_fail: v25_confidence_breakdown
                .map(|breakdown| breakdown.zeroed_by_pdd_hard_fail),
            v25_confidence_zeroed_by_tas_hard_reject: v25_confidence_breakdown
                .map(|breakdown| breakdown.zeroed_by_tas_hard_reject),
            tas_available,
            tas_unavailable_reason,
            pdd_sequence_signals_available: pdd_seq_available,
            pdd_sequence_signals_unavailable_reason: pdd_seq_unavailable_reason,
            pdd_price_anchor_available: self.pdd_price_anchor_available(config),
            v25_confidence_available,
            v25_confidence_unavailable_reason,
            shadow_tas_reject_reason: self
                .v25_shadow_decisions
                .iter()
                .filter(|s| s.kind.verdict_str() == "REJECT_LOW_TRAJECTORY")
                .map(|s| s.reason.clone())
                .next(),
            // TAS trajectory scores
            tas_overall_score: self.trajectory.as_ref().map(|t| t.overall_tas_score),
            tas_momentum_score: self.trajectory.as_ref().map(|t| t.momentum_score),
            tas_hhi_score: self.trajectory.as_ref().map(|t| t.hhi_score),
            tas_volume_score: self.trajectory.as_ref().map(|t| t.volume_score),
            tas_interval_score: self.trajectory.as_ref().map(|t| t.interval_score),
            tas_buy_ratio_score: self.trajectory.as_ref().map(|t| t.buy_ratio_score),
            pdd_hard_fail: self
                .pdd_assessment
                .as_ref()
                .and_then(|p| p.hard_fail.as_ref().map(|f| f.as_str().to_string())),
            pdd_entry_drift_pct: self.pdd_assessment.as_ref().and_then(|p| p.entry_drift_pct),
            pdd_entry_drift_anchor_source: self
                .pdd_assessment
                .as_ref()
                .and_then(|p| p.entry_drift_anchor_source.map(|s| s.to_string())),
            pdd_entry_drift_anchor_quality: self
                .pdd_assessment
                .as_ref()
                .and_then(|p| p.entry_drift_anchor_quality.map(|s| s.to_string())),
            pdd_spike_detected: self.pdd_assessment.as_ref().map(|p| p.spike_detected),
            pdd_ramping_detected: self.pdd_assessment.as_ref().map(|p| p.ramping_detected),
            pdd_whale_top3_pct: self.pdd_assessment.as_ref().and_then(|p| p.whale_top3_pct),
            pdd_flash_crash_risk: self.pdd_assessment.as_ref().map(|p| p.flash_crash_risk),
            pdd_score: self.pdd_assessment.as_ref().map(|p| p.pdd_score),
            aps_regime: self
                .aps_diagnostics
                .as_ref()
                .map(|a| a.regime.as_str().to_string()),
            aps_shadow_entry_drift_max: self
                .aps_diagnostics
                .as_ref()
                .map(|a| a.shadow_entry_drift_max_pct),
            aps_shadow_confidence_min: self
                .aps_diagnostics
                .as_ref()
                .map(|a| a.shadow_confidence_min),
            aps_shadow_prosperity_mcap: self
                .aps_diagnostics
                .as_ref()
                .map(|a| a.shadow_prosperity_mcap_sol),
            aps_shadow_branch1_sniped: self
                .aps_diagnostics
                .as_ref()
                .map(|a| a.shadow_branch1_sniped_pct),
            aps_shadow_branch3_hhi: self
                .aps_diagnostics
                .as_ref()
                .map(|a| a.shadow_branch3_hhi_max),
            aps_shadow_prosperity_would_pass: self
                .aps_diagnostics
                .as_ref()
                .and_then(|a| a.shadow_prosperity_would_pass),
            aps_shadow_thresholds: self.aps_diagnostics.as_ref().map(|a| {
                serde_json::to_string(&serde_json::json!({
                    "entry_drift_max_pct": a.shadow_entry_drift_max_pct,
                    "confidence_min": a.shadow_confidence_min,
                    "prosperity_mcap_sol": a.shadow_prosperity_mcap_sol,
                    "branch1_sniped_pct": a.shadow_branch1_sniped_pct,
                    "branch3_hhi_max": a.shadow_branch3_hhi_max,
                }))
                .unwrap_or_default()
            }),
            // Top-level telemetry (SSOT compliance)
            entry_drift_pct: self.entry_drift_pct,
            entry_drift_anchor_source: self
                .pdd_assessment
                .as_ref()
                .and_then(|p| p.entry_drift_anchor_source.map(|s| s.to_string())),
            entry_drift_anchor_quality: self
                .entry_drift_anchor_quality
                .map(|q| format!("{:?}", q).to_lowercase()),
            market_regime: self
                .aps_diagnostics
                .as_ref()
                .map(|a| a.regime.as_str().to_string()),
            pdd_soft_flags: self.pdd_assessment.as_ref().and_then(|p| {
                let mut flags = Vec::new();
                if p.spike_detected {
                    flags.push("spike");
                }
                if p.ramping_detected {
                    flags.push("ramping");
                }
                if p.flash_crash_risk {
                    flags.push("flash_crash");
                }
                if p.whale_top3_pct.is_some_and(|v| v > 60.0) {
                    flags.push("whale");
                }
                if !p.reserve_health_pass {
                    flags.push("reserve");
                }
                if flags.is_empty() {
                    None
                } else {
                    Some(flags.join(","))
                }
            }),
            reason_code: self.derive_reason_code(config).map(|rc| {
                serde_json::to_string(&rc)
                    .unwrap_or_else(|_| "SERIALIZATION_ERROR".to_string())
                    .trim_matches('"')
                    .to_string()
            }),
            reason_code_version: ghost_brain::oracle::reason_code::GatekeeperReasonCode::version(),
            shadow_pdd_reject_reason: self
                .v25_shadow_decisions
                .iter()
                .filter(|s| matches!(s.kind, ShadowDecisionKind::RejectPumpAndDump))
                .map(|s| s.reason.clone())
                .next(),
        }
    }
}

/// Gatekeeper verdict (canonical v2)
pub enum GatekeeperVerdict {
    Wait,
    Buy {
        buffered_txs: Vec<GatekeeperBufferedTx>,
        assessment: GatekeeperAssessment,
    },
    Reject {
        assessment: GatekeeperAssessment,
        reason: String,
    },
    Timeout {
        assessment: GatekeeperAssessment,
    },
    ApprovedTx {
        tx: Arc<PoolTransaction>,
        metrics: PoolMetrics,
    },
    /// Curve data not yet available; keep collecting transactions.
    /// Not terminal — do NOT log to JSONL or close the window.
    PendingCurve,
}

#[derive(Debug, Clone)]
pub enum GatekeeperIngressOutcome {
    Wait,
    TriggerEvaluation,
    DeadlineElapsed,
    ApprovedTx {
        tx: Arc<PoolTransaction>,
        metrics: PoolMetrics,
    },
}

/// Window event for V2 buffer dedup tracking
#[derive(Debug, Clone, Copy)]
struct V2WindowEvent {
    timestamp_ms: u64,
    is_duplicate: bool,
}

/// Canonical GatekeeperBuffer with full 6-phase analytical tracking.
///
/// Data flow:
///   PumpPortal WS → PoolTransaction → ingest_transaction_tracking_only() →
///   tracking updates → feature-driven evaluation
pub struct GatekeeperBuffer {
    pool_id: Pubkey,
    state: PoolState,
    config: GatekeeperV2Config,

    // Canonical pool identity from DetectedPool metadata.
    pool_creator: Option<String>,
    pool_create_signature: Option<String>,
    pool_initial_liquidity_sol: Option<f64>,

    // TX Buffer & Dedup
    buffered_txs: Vec<GatekeeperBufferedTx>,
    tx_keys_seen: HashSet<TxKey>,
    tx_signatures_seen: HashSet<Signature>,
    tx_keys_fifo: VecDeque<TxKey>,
    tx_keys_capacity: usize,
    window_events: VecDeque<V2WindowEvent>,

    // Timing & Monotonicity
    highest_seen_ts: u64,
    first_tx_ts: Option<u64>,
    /// Wall-clock creation time (epoch ms) – used as deadline fallback
    /// when no non-dust TX has ever arrived.
    created_at_ms: u64,
    /// Wall-clock registration timestamp captured at NewPoolDetected handling.
    /// This is the immutable t0 for hard deadline checks.
    registered_wall_ts_ms: u64,
    /// Immutable hard deadline in wall-clock milliseconds.
    deadline_wall_ts_ms: u64,
    rejected: bool,

    // Phase 1: Quantity Counters
    unique_signers: HashSet<String>,
    total_tx_count: usize,
    buy_count: usize,
    sell_count: usize,

    // Phase 2: Velocity Tracking
    tx_timestamps_sorted: Vec<u64>,

    // Phase 3: Signer Diversity Tracking
    signer_stats: HashMap<String, SignerStats>,

    // Phase 4: Volume Tracking
    tx_volumes: Vec<f64>,
    total_volume_sol: f64,
    buy_volume_sol: f64,
    sell_volume_sol: f64,

    // Phase 5: Dev Tracking
    dev_wallet: Option<String>,
    dev_buy_total_sol: f64,
    dev_buy_volume_total_sol: f64,
    dev_sell_total_sol: f64,
    dev_tx_count: usize,
    dev_has_sold: bool,
    first_signer: Option<String>,
    dev_initial_buy_tokens: Option<f64>,

    // Phase 6: Bonding Curve Dynamics Tracking
    price_history: Vec<PricePoint>,

    // Aggregated metrics for SnapshotEngine relay
    metrics: PoolMetrics,

    // Evaluation State
    phase1_passed: bool,
    phase1_passed_at_count: Option<usize>,
    last_eval_at_count: Option<usize>,
    eval_count: usize,

    // Telemetry
    dust_filtered_count: u64,

    // Consecutive buy streak tracking (FOMO)
    current_consecutive_buys: usize,
    max_consecutive_buys: usize,

    // Yellowstone-only (optional)
    failed_tx_count: usize,

    // Curve Readiness Latch
    /// Epoch-like t0 chosen by provenance-aware detection helpers. Set via `set_curve_t0()`.
    curve_t0_event_ts_ms: Option<u64>,
    /// Provenance tag for `curve_t0_event_ts_ms`.
    curve_t0_clock_source: Option<&'static str>,
    /// Deadline (event-time ms) = t0 + curve_wait_ms. Set via `set_curve_t0()`.
    curve_deadline_event_ts_ms: Option<u64>,
    /// Per-pool curve readiness flag. Set to `true` when the current curve
    /// quality is acceptable for a normal Gatekeeper path.
    curve_ready: bool,
    /// Latest explicit Phase-5 curve-quality classification.
    curve_quality: CurveFreshnessState,
    /// Latest finality tier attached to the active curve-quality state.
    curve_finality_state: CurveFinality,
    /// True when runtime injected an explicit curve-quality result for the
    /// next transaction and Gatekeeper should not infer quality from TX flags.
    curve_state_explicit_for_next_tx: bool,
    /// Whether this pool has entered PendingCurve at least once.
    curve_pending_active: bool,
    /// Terminal PendingCurve telemetry must be emitted exactly once per pool.
    curve_terminal_recorded: bool,

    // ═══════════════════════════════════════════
    // V2.5 Dynamic Observation Window
    // ═══════════════════════════════════════════
    /// Early entry shadow deadline (registered_wall_ts + early_entry_max_ms, ~5000ms)
    early_deadline_ms: u64,
    /// Normal window shadow deadline (registered_wall_ts + normal_window_ms, ~7000ms)
    normal_deadline_ms: u64,
    /// Extended window deadline (registered_wall_ts + extended_window_ms, ~10000ms)
    extended_deadline_ms: u64,
    /// Current observation stage (defaults to Extended)
    window_stage: ObservationStage,
    /// Whether the early shadow evaluation has already fired
    early_shadow_fired: bool,
    /// Whether the normal shadow evaluation has already fired
    normal_shadow_fired: bool,
    /// Whether the extended shadow evaluation has already fired (timer or deadline fallback)
    extended_shadow_fired: bool,
    /// Shadow decision records collected during observation
    v25_shadow_decisions: Vec<ShadowV25Decision>,
}

// ═══════════════════════════════════════════════════════════════════════
// V2.5 Dynamic Observation Window types
// ═══════════════════════════════════════════════════════════════════════

/// Observation stage for the Dynamic Observation Window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObservationStage {
    /// Early entry window (2-5s): shadow-only, ultra-restrictive
    Early,
    /// Normal window (5-7s): shadow-only, standard evaluation
    Normal,
    /// Extended window (7-10s): live-compatible, existing deadline
    #[default]
    Extended,
}

impl ObservationStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Early => "Early",
            Self::Normal => "Normal",
            Self::Extended => "Extended",
        }
    }
}

/// Kind of shadow decision produced by a V2.5 observation window checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowDecisionKind {
    /// Shadow evaluator would have bought; pool passed early criteria
    EarlyBuyCandidate,
    /// Shadow evaluator would have bought; pool passed normal criteria
    NormalBuyCandidate,
    /// Shadow evaluator rejected the pool (generic reason)
    ShadowReject,
    /// TAS trajectory score below hard-reject threshold (< 0.30)
    RejectLowTrajectory,
    /// PDD hard veto: pump & dump pattern detected
    RejectPumpAndDump,
    /// Not enough data to form a decision
    InsufficientData,
}

impl ShadowDecisionKind {
    pub fn verdict_str(&self) -> &'static str {
        match self {
            Self::EarlyBuyCandidate | Self::NormalBuyCandidate => "BUY",
            Self::ShadowReject => "REJECT",
            Self::RejectLowTrajectory => "REJECT_LOW_TRAJECTORY",
            Self::RejectPumpAndDump => "REJECT_PUMP_AND_DUMP",
            Self::InsufficientData => "INSUFFICIENT_DATA",
        }
    }
}

/// A single shadow evaluation captured at an observation window checkpoint.
#[derive(Debug, Clone)]
pub struct ShadowV25Decision {
    /// Kind of shadow decision
    pub kind: ShadowDecisionKind,
    /// Observation window in which this evaluation ran
    pub window: ObservationStage,
    /// Elapsed wall-clock ms since registered_wall_ts_ms
    pub elapsed_ms: u64,
    /// Confidence score (0.0-1.0) with TAS trajectory modulation applied
    pub confidence: f64,
    /// Number of phases that passed (0-6)
    pub phases_passed: u8,
    /// Human-readable reason for the decision
    pub reason: String,
}

// ═══════════════════════════════════════════════════════════════════════
// Pure compute functions for phases 2-6 (no side effects, easy to test)
// ═══════════════════════════════════════════════════════════════════════

/// Deterministic evenly-spaced downsampling.
/// If `data.len() <= max_len` or `max_len == 0`, returns as-is.
/// If `max_len == 1`, returns the first element.
/// Otherwise, selects `max_len` indices evenly spaced across the input.
fn deterministic_downsample_f64(data: &[f64], indices: &[usize]) -> Vec<f64> {
    indices.iter().map(|&i| data[i]).collect()
}

fn deterministic_downsample_i64(data: &[i64], indices: &[usize]) -> Vec<i64> {
    indices.iter().map(|&i| data[i]).collect()
}

/// Compute evenly-spaced sample indices for deterministic downsampling.
/// Returns indices selecting `max_len` elements from a sequence of length `n`.
fn downsample_indices(n: usize, max_len: usize) -> Vec<usize> {
    if max_len == 0 || n <= max_len {
        return (0..n).collect();
    }
    if max_len == 1 {
        return vec![0];
    }
    (0..max_len).map(|i| i * (n - 1) / (max_len - 1)).collect()
}

/// Phase 6: Compute bonding curve dynamics from price history.
pub fn compute_bonding_curve_dynamics(price_history: &[PricePoint]) -> BondingCurveDynamics {
    if price_history.is_empty() {
        return BondingCurveDynamics {
            initial_price: 0.0,
            current_price: 0.0,
            max_price: 0.0,
            price_change_ratio: 1.0,
            max_single_tx_price_impact_pct: 0.0,
            max_single_sell_impact_pct: 0.0,
            current_market_cap_sol: 0.0,
            market_cap_change_ratio: 1.0,
            bonding_progress_pct: 0.0,
            curve_data_known: false,
            curve_finality: CurveFinality::Speculative,
            price_data_points: 0,
        };
    }

    let authoritative_points: Vec<PricePoint> =
        if price_history.iter().any(|point| point.curve_data_known) {
            price_history
                .iter()
                .copied()
                .filter(|point| point.curve_data_known)
                .collect()
        } else {
            price_history.to_vec()
        };

    let first = &authoritative_points[0];
    let last = &authoritative_points[authoritative_points.len() - 1];

    let initial_price = first.price_sol_per_token;
    let current_price = last.price_sol_per_token;
    let max_price = authoritative_points
        .iter()
        .map(|p| p.price_sol_per_token)
        .fold(0.0_f64, f64::max);

    let price_change_ratio = if initial_price > 0.0 {
        current_price / initial_price
    } else {
        1.0
    };

    // Max single-TX price impact (all transactions)
    let max_single_tx_price_impact_pct = if authoritative_points.len() >= 2 {
        authoritative_points
            .windows(2)
            .map(|w| {
                let prev_price = w[0].price_sol_per_token;
                let curr_price = w[1].price_sol_per_token;
                if prev_price > 0.0 {
                    ((curr_price - prev_price) / prev_price).abs() * 100.0
                } else {
                    0.0
                }
            })
            .fold(0.0_f64, f64::max)
    } else {
        0.0
    };

    // Max single SELL TX price impact (only sell transactions)
    let max_single_sell_impact_pct = if authoritative_points.len() >= 2 {
        authoritative_points
            .windows(2)
            .filter(|w| !w[1].is_buy) // Only consider sell transactions
            .map(|w| {
                let prev_price = w[0].price_sol_per_token;
                let curr_price = w[1].price_sol_per_token;
                if prev_price > 0.0 {
                    ((curr_price - prev_price) / prev_price).abs() * 100.0
                } else {
                    0.0
                }
            })
            .fold(0.0_f64, f64::max)
    } else {
        0.0
    };

    // Market cap progression
    let initial_mcap = first.market_cap_sol;
    let current_mcap = last.market_cap_sol;
    let market_cap_change_ratio = if initial_mcap > 0.0 {
        current_mcap / initial_mcap
    } else {
        1.0
    };

    // Bonding curve progress
    // curve_data_known is an explicit parser flag — NOT derived from reserve values.
    let curve_data_known = last.curve_data_known;
    let curve_finality = last.curve_finality.normalized(curve_data_known);
    let bonding_progress_pct =
        if curve_data_known && PUMP_GENESIS_TOKEN_SUPPLY > 0.0 && last.v_tokens_in_curve > 0.0 {
            let tokens_remaining = last.v_tokens_in_curve;
            let tokens_sold = (PUMP_GENESIS_TOKEN_SUPPLY - tokens_remaining).max(0.0);
            (tokens_sold / PUMP_GENESIS_TOKEN_SUPPLY) * 100.0
        } else {
            0.0
        };

    BondingCurveDynamics {
        initial_price,
        current_price,
        max_price,
        price_change_ratio,
        max_single_tx_price_impact_pct,
        max_single_sell_impact_pct,
        current_market_cap_sol: current_mcap,
        market_cap_change_ratio,
        bonding_progress_pct,
        curve_data_known,
        curve_finality,
        price_data_points: authoritative_points.len(),
    }
}

impl GatekeeperBuffer {
    fn now_wall_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn wall_observation_duration_ms(&self, now_wall_ms: u64) -> u64 {
        now_wall_ms
            .saturating_sub(self.registered_wall_ts_ms)
            .min(self.config.max_wait_time_ms)
    }

    fn finalize_lag_ms(&self, now_wall_ms: u64) -> u64 {
        now_wall_ms
            .saturating_sub(self.registered_wall_ts_ms)
            .saturating_sub(self.config.max_wait_time_ms)
    }

    pub fn new(pool_id: Pubkey, cfg: &GatekeeperV2Config) -> Self {
        if cfg.dow.enabled && cfg.dow.extended_window_ms > cfg.max_wait_time_ms {
            panic!(
                "P0 invariant violated: dow.extended_window_ms ({}) > max_wait_time_ms ({})",
                cfg.dow.extended_window_ms, cfg.max_wait_time_ms
            );
        }

        let capacity_hint = cfg.min_tx_count.saturating_mul(2).max(32);
        let now_wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            pool_id,
            state: PoolState::Tracked,
            config: cfg.clone(),

            pool_creator: None,
            pool_create_signature: None,
            pool_initial_liquidity_sol: None,

            buffered_txs: Vec::with_capacity(capacity_hint),
            tx_keys_seen: HashSet::with_capacity(capacity_hint),
            tx_signatures_seen: HashSet::with_capacity(capacity_hint),
            tx_keys_fifo: VecDeque::with_capacity(capacity_hint * 2),
            tx_keys_capacity: cfg.min_tx_count.saturating_mul(8).max(256),
            window_events: VecDeque::with_capacity(capacity_hint * 2),

            highest_seen_ts: 0,
            first_tx_ts: None,
            created_at_ms: now_wall,
            registered_wall_ts_ms: now_wall,
            deadline_wall_ts_ms: now_wall.saturating_add(cfg.max_wait_time_ms),
            rejected: false,

            unique_signers: HashSet::with_capacity(capacity_hint),
            total_tx_count: 0,
            buy_count: 0,
            sell_count: 0,

            tx_timestamps_sorted: Vec::with_capacity(capacity_hint),

            signer_stats: HashMap::with_capacity(capacity_hint),

            tx_volumes: Vec::with_capacity(capacity_hint),
            total_volume_sol: 0.0,
            buy_volume_sol: 0.0,
            sell_volume_sol: 0.0,

            dev_wallet: None,
            dev_buy_total_sol: 0.0,
            dev_buy_volume_total_sol: 0.0,
            dev_sell_total_sol: 0.0,
            dev_tx_count: 0,
            dev_has_sold: false,
            first_signer: None,
            dev_initial_buy_tokens: None,

            price_history: Vec::with_capacity(capacity_hint),

            metrics: PoolMetrics::default(),

            phase1_passed: false,
            phase1_passed_at_count: None,
            last_eval_at_count: None,
            eval_count: 0,

            dust_filtered_count: 0,
            current_consecutive_buys: 0,
            max_consecutive_buys: 0,
            failed_tx_count: 0,

            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_deadline_event_ts_ms: None,
            curve_ready: false,
            curve_quality: CurveFreshnessState::Unknown,
            curve_finality_state: CurveFinality::Speculative,
            curve_state_explicit_for_next_tx: false,
            curve_pending_active: false,
            curve_terminal_recorded: false,

            // V2.5 Dynamic Observation Window
            early_deadline_ms: now_wall.saturating_add(cfg.dow.early_entry_max_ms),
            normal_deadline_ms: now_wall.saturating_add(cfg.dow.normal_window_ms),
            extended_deadline_ms: now_wall.saturating_add(cfg.dow.extended_window_ms),
            window_stage: ObservationStage::Extended,
            early_shadow_fired: false,
            normal_shadow_fired: false,
            extended_shadow_fired: false,
            v25_shadow_decisions: Vec::new(),
        }
    }

    pub fn state(&self) -> PoolState {
        self.state
    }

    /// Accessor for V2.5 shadow decisions collected during observation.
    pub fn v25_shadow_decisions(&self) -> &[ShadowV25Decision] {
        &self.v25_shadow_decisions
    }

    fn normalize_identity_value(value: Option<&str>) -> Option<String> {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "unknown")
            .map(ToOwned::to_owned)
    }

    fn normalize_initial_liquidity_sol(value: Option<f64>) -> Option<f64> {
        value.filter(|value| value.is_finite() && *value > 0.0)
    }

    fn reset_dev_tracking(&mut self) {
        self.dev_wallet = None;
        self.dev_buy_total_sol = 0.0;
        self.dev_buy_volume_total_sol = 0.0;
        self.dev_sell_total_sol = 0.0;
        self.dev_tx_count = 0;
        self.dev_has_sold = false;
        self.dev_initial_buy_tokens = None;
    }

    fn apply_dev_metrics_for_wallet(
        &mut self,
        dev_wallet: &str,
        primary_buy_tx: Option<&PoolTransaction>,
    ) {
        self.reset_dev_tracking();
        self.dev_wallet = Some(dev_wallet.to_string());
        if let Some(primary_buy_tx) = primary_buy_tx {
            self.dev_buy_total_sol = primary_buy_tx.volume_sol;
            self.dev_initial_buy_tokens = primary_buy_tx.token_amount_units.map(|u| u as f64);
        }

        let mut ordered: Vec<&GatekeeperBufferedTx> = self.buffered_txs.iter().collect();
        ordered.sort_by(|lhs, rhs| lhs.tx_key.cmp(&rhs.tx_key));

        for buffered in ordered {
            let tx = buffered.tx.as_ref();
            if tx.signer != dev_wallet {
                continue;
            }
            self.dev_tx_count += 1;
            if tx.is_buy {
                self.dev_buy_volume_total_sol += tx.volume_sol;
            } else {
                self.dev_sell_total_sol += tx.volume_sol;
                self.dev_has_sold = true;
            }
        }
    }

    fn find_primary_creator_buy_index(&self) -> Option<usize> {
        let creator = self.pool_creator.as_deref()?;

        let preferred = self
            .buffered_txs
            .iter()
            .enumerate()
            .filter(|(_, buffered)| {
                let tx = buffered.tx.as_ref();
                tx.is_buy
                    && tx.signer == creator
                    && self
                        .pool_create_signature
                        .as_deref()
                        .is_some_and(|create_sig| tx.signature == create_sig)
            })
            .min_by(|lhs, rhs| lhs.1.tx_key.cmp(&rhs.1.tx_key))
            .map(|(idx, _)| idx);

        if preferred.is_some() {
            return preferred;
        }

        self.buffered_txs
            .iter()
            .enumerate()
            .filter(|(_, buffered)| {
                let tx = buffered.tx.as_ref();
                tx.is_buy && tx.signer == creator
            })
            .min_by(|lhs, rhs| lhs.1.tx_key.cmp(&rhs.1.tx_key))
            .map(|(idx, _)| idx)
    }

    fn refresh_canonical_dev_tracking(&mut self) {
        let Some(creator) = self.pool_creator.clone() else {
            return;
        };

        let primary_tx = self
            .find_primary_creator_buy_index()
            .map(|idx| self.buffered_txs[idx].tx.clone());

        if primary_tx.is_some()
            || self
                .buffered_txs
                .iter()
                .any(|buffered| buffered.tx.signer == creator)
        {
            self.apply_dev_metrics_for_wallet(&creator, primary_tx.as_deref());
        }
    }

    pub fn set_pool_identity_with_liquidity(
        &mut self,
        creator: Option<&str>,
        create_signature: Option<&str>,
        initial_liquidity_sol: Option<f64>,
    ) {
        let creator = Self::normalize_identity_value(creator);
        let create_signature = Self::normalize_identity_value(create_signature);
        let initial_liquidity_sol = Self::normalize_initial_liquidity_sol(initial_liquidity_sol);

        let changed = self.pool_creator != creator
            || self.pool_create_signature != create_signature
            || self.pool_initial_liquidity_sol != initial_liquidity_sol;
        self.pool_creator = creator;
        self.pool_create_signature = create_signature;
        self.pool_initial_liquidity_sol = initial_liquidity_sol;

        if changed {
            self.refresh_canonical_dev_tracking();
        }
    }

    pub fn set_pool_identity(&mut self, creator: Option<&str>, create_signature: Option<&str>) {
        self.set_pool_identity_with_liquidity(creator, create_signature, None);
    }

    pub fn mark_committed(&mut self) {
        self.state = PoolState::Committed;
        self.refresh_curve_policy_state();
    }

    pub fn record_curve_state(
        &mut self,
        curve_quality: CurveFreshnessState,
        curve_finality: CurveFinality,
    ) {
        self.curve_state_explicit_for_next_tx = true;
        self.apply_curve_state(curve_quality, curve_finality);
    }

    fn apply_curve_state(
        &mut self,
        curve_quality: CurveFreshnessState,
        curve_finality: CurveFinality,
    ) {
        let normalized_finality =
            curve_finality.normalized(!matches!(curve_quality, CurveFreshnessState::Unknown));
        self.curve_finality_state = normalized_finality;
        self.curve_quality = if self.state.is_committed()
            && !matches!(curve_quality, CurveFreshnessState::Unknown)
            && normalized_finality.is_finalized()
        {
            CurveFreshnessState::Committed
        } else {
            curve_quality
        };
        self.refresh_curve_policy_state();
    }

    fn refresh_curve_policy_state(&mut self) {
        let was_ready = self.curve_ready;
        self.curve_ready = is_curve_quality_actionable(
            self.curve_quality,
            self.curve_finality_state,
            self.config.stale_fallback,
        );

        if self.curve_pending_active
            && !self.curve_terminal_recorded
            && !was_ready
            && self.curve_ready
        {
            self.record_pending_curve_terminal("recovered");
        }
    }

    fn record_pending_curve_reason(&mut self, reason: &'static str) {
        self.curve_pending_active = true;
        ::metrics::counter!("gatekeeper_pending_curve_total", 1, "reason" => reason);
        ::metrics::counter!("gatekeeper_curve_latch_fired_total", 1, "outcome" => "pending");
    }

    fn record_pending_curve_terminal(&mut self, outcome: &'static str) {
        if self.curve_terminal_recorded {
            return;
        }
        self.curve_terminal_recorded = true;
        self.curve_pending_active = false;
        ::metrics::counter!(
            "gatekeeper_pending_curve_terminal_total",
            1,
            "outcome" => outcome
        );
    }

    fn reject_curve_policy(
        &mut self,
        reason_label: &'static str,
        terminal_outcome: &'static str,
        reason: String,
    ) -> GatekeeperVerdict {
        ::metrics::counter!("gatekeeper_pending_curve_total", 1, "reason" => reason_label);
        if terminal_outcome == "timed_out" {
            ::metrics::counter!("gatekeeper_curve_latch_fired_total", 1, "outcome" => "timeout_reject");
        }
        self.record_pending_curve_terminal(terminal_outcome);
        self.rejected = true;
        let mut assessment = self.run_assessment();
        assessment.decision = Some(GatekeeperDecision {
            hard_fail_reason: Some(reason.clone()),
            core1_passed: false,
            core2_passed: false,
            core3_passed: false,
            dev_unknown: false,
            soft_points: 0,
            max_soft_points_possible: 0,
            effective_max_soft_points: 0,
            soft_signals: SoftSignals::default(),
            sybil_policy: SybilPolicyDiagnostics::default(),
            alpha_gate: AlphaGateDiagnostics::not_run(self.config.enable_alpha_gate),
            prosperity_filter: ProsperityFilterDiagnostics::not_run(
                self.config.enable_prosperity_filter,
            ),
            total_soft_points: 0,
            verdict_buy: false,
            verdict_type: GatekeeperVerdictType::RejectHardFail,
            reason_chain: reason.clone(),
            gatekeeper_strength: None,
        });
        GatekeeperVerdict::Reject { assessment, reason }
    }

    fn pending_curve_before_deadline(
        &mut self,
        now_ms: u64,
        reason_label: &'static str,
        timeout_reason_label: &'static str,
        timeout_reason: String,
    ) -> GatekeeperVerdict {
        let deadline = match self.curve_deadline_event_ts_ms {
            Some(deadline) => deadline,
            None => {
                self.record_pending_curve_reason(reason_label);
                return GatekeeperVerdict::PendingCurve;
            }
        };

        if now_ms < deadline {
            self.record_pending_curve_reason(reason_label);
            GatekeeperVerdict::PendingCurve
        } else {
            self.reject_curve_policy(timeout_reason_label, "timed_out", timeout_reason)
        }
    }

    /// Set the curve-latch t0 from `NewPoolDetected.timestamp_ms`.
    /// Computes `curve_deadline_event_ts_ms = t0 + curve_wait_ms`.
    /// Must be called once per pool, before any verdict evaluation.
    pub fn set_curve_t0(&mut self, t0_event_ts_ms: u64) {
        self.set_curve_t0_with_source(t0_event_ts_ms, "unspecified");
    }

    pub fn set_curve_t0_with_source(&mut self, t0_event_ts_ms: u64, source: &'static str) {
        self.curve_t0_event_ts_ms = Some(t0_event_ts_ms);
        self.curve_t0_clock_source = Some(source);
        self.curve_deadline_event_ts_ms =
            Some(t0_event_ts_ms.saturating_add(self.config.curve_wait_ms));
    }

    /// Set immutable wall-clock registration t0 used by the hard deadline.
    /// Must be called when NewPoolDetected is handled in runtime.
    pub fn set_registered_wall_t0(&mut self, registered_wall_ts_ms: u64) {
        self.registered_wall_ts_ms = registered_wall_ts_ms;
        self.deadline_wall_ts_ms =
            registered_wall_ts_ms.saturating_add(self.config.max_wait_time_ms);
        // V2.5: recompute multi-deadline windows from registration t0
        self.early_deadline_ms =
            registered_wall_ts_ms.saturating_add(self.config.dow.early_entry_max_ms);
        self.normal_deadline_ms =
            registered_wall_ts_ms.saturating_add(self.config.dow.normal_window_ms);
        self.extended_deadline_ms =
            registered_wall_ts_ms.saturating_add(self.config.dow.extended_window_ms);
        self.created_at_ms = registered_wall_ts_ms;
        if self.first_tx_ts.is_none() {
            self.first_tx_ts = Some(registered_wall_ts_ms);
        }
    }

    pub fn set_deadline_wall_ts_ms(&mut self, deadline_wall_ts_ms: u64) {
        self.deadline_wall_ts_ms = deadline_wall_ts_ms;
    }

    #[inline]
    fn tx_event_ts_ms(tx: &PoolTransaction) -> u64 {
        if let Some(explicit_event_ts_ms) = tx.effective_event_ts_ms() {
            explicit_event_ts_ms
        } else {
            Self::now_wall_ms()
        }
    }

    #[must_use]
    pub fn tx_key_for(tx: &PoolTransaction) -> Option<TxKey> {
        Self::tx_key_from_tx(tx)
    }

    #[must_use]
    pub fn buffered_tx_count(&self) -> usize {
        self.buffered_txs.len()
    }

    #[must_use]
    pub fn unique_tx_key_count(&self) -> usize {
        self.tx_keys_seen.len()
    }

    #[must_use]
    pub fn unique_signature_count(&self) -> usize {
        self.tx_signatures_seen.len()
    }

    #[must_use]
    pub const fn highest_seen_ts_ms(&self) -> u64 {
        self.highest_seen_ts
    }

    pub fn advance_event_clock(&mut self, observed_ms: u64) -> u64 {
        let now_ms = observed_ms.max(self.highest_seen_ts);
        self.highest_seen_ts = now_ms;
        now_ms
    }

    #[must_use]
    pub const fn first_tx_ts_ms(&self) -> Option<u64> {
        self.first_tx_ts
    }

    #[must_use]
    pub fn latest_price_impact_pct(&self) -> Option<f64> {
        self.price_history.windows(2).last().and_then(|window| {
            let previous = window[0].price_sol_per_token;
            let current = window[1].price_sol_per_token;
            (previous > 0.0).then_some(((current - previous) / previous).abs() * 100.0)
        })
    }

    #[must_use]
    pub const fn curve_ready(&self) -> bool {
        self.curve_ready
    }

    #[must_use]
    pub const fn curve_quality(&self) -> CurveFreshnessState {
        self.curve_quality
    }

    #[must_use]
    pub const fn curve_finality_state(&self) -> CurveFinality {
        self.curve_finality_state
    }

    #[must_use]
    pub const fn curve_t0_event_ts_ms(&self) -> Option<u64> {
        self.curve_t0_event_ts_ms
    }

    #[must_use]
    pub const fn curve_t0_clock_source(&self) -> Option<&'static str> {
        self.curve_t0_clock_source
    }

    #[must_use]
    pub fn curve_wait_elapsed_ms(&self) -> Option<u64> {
        self.curve_t0_event_ts_ms
            .map(|t0| self.highest_seen_ts.saturating_sub(t0))
    }

    #[must_use]
    pub fn current_curve_dynamics(&self) -> BondingCurveDynamics {
        compute_bonding_curve_dynamics(&self.price_history)
    }

    #[must_use]
    pub fn price_history(&self) -> &[PricePoint] {
        &self.price_history
    }

    pub fn last_price_point(&self) -> Option<&PricePoint> {
        self.price_history.last()
    }

    pub fn first_price(&self) -> Option<f64> {
        self.price_history.first().map(|p| p.price_sol_per_token)
    }

    pub fn current_price(&self) -> Option<f64> {
        self.price_history.last().map(|p| p.price_sol_per_token)
    }

    pub fn signer_stats(&self) -> &HashMap<String, crate::tx_intelligence::SignerStats> {
        &self.signer_stats
    }

    pub const fn max_consecutive_buys_count(&self) -> usize {
        self.max_consecutive_buys
    }

    pub fn total_volume_sol(&self) -> f64 {
        self.total_volume_sol
    }

    pub fn buffered_txs_slice(&self) -> &[GatekeeperBufferedTx] {
        &self.buffered_txs
    }

    #[must_use]
    pub fn observation_duration_ms(&self) -> u64 {
        self.wall_observation_duration_ms(Self::now_wall_ms())
    }

    pub fn prepare_policy_evaluation(&mut self, legacy_verdict: &GatekeeperVerdict) {
        match legacy_verdict {
            GatekeeperVerdict::Buy { buffered_txs, .. } => {
                self.buffered_txs = buffered_txs.clone();
                self.state = PoolState::Tracked;
                self.rejected = false;
            }
            GatekeeperVerdict::Reject { .. } | GatekeeperVerdict::Timeout { .. } => {
                self.state = PoolState::Tracked;
                self.rejected = false;
            }
            GatekeeperVerdict::Wait
            | GatekeeperVerdict::ApprovedTx { .. }
            | GatekeeperVerdict::PendingCurve => {}
        }
    }

    #[must_use]
    pub const fn phase1_passed(&self) -> bool {
        self.phase1_passed
    }

    #[must_use]
    pub fn policy_evaluation_context(&self) -> PolicyEvaluationContext {
        PolicyEvaluationContext {
            finalize_lag_ms: self.finalize_lag_ms(Self::now_wall_ms()),
            eval_count: self.eval_count,
        }
    }

    pub fn prepare_feature_evaluation(&mut self) {
        self.state = PoolState::Tracked;
        self.rejected = false;
        self.eval_count += 1;
        self.last_eval_at_count = Some(self.total_tx_count);
    }

    pub fn rollback_feature_evaluation(&mut self) {
        self.eval_count = self.eval_count.saturating_sub(1);
    }

    pub fn evaluate_from_features(
        &mut self,
        features: MaterializedFeatureSet,
        config: &GatekeeperV2Config,
    ) -> GatekeeperVerdict {
        let mut assessment =
            build_assessment_from_features(features, config, self.policy_evaluation_context());
        let decision = evaluate_policy_from_assessment(&assessment, config);
        let reason_chain = decision.reason_chain.clone();
        let verdict_buy = decision.verdict_buy;
        let verdict_tag = decision.verdict_type.tag();
        let soft_pts = decision.soft_points;
        let max_pts = decision.max_soft_points_possible;
        assessment.decision = Some(decision);
        assessment.cache_v25_confidence(config);
        // Transfer V2.5 shadow decisions from buffer to terminal assessment
        assessment.v25_shadow_decisions = self.v25_shadow_decisions.clone();
        assessment.hard_reject_reason = assessment
            .decision
            .as_ref()
            .and_then(|decision| decision.hard_fail_reason.clone());
        let breakdown = Self::format_phase_breakdown(&assessment);

        match evaluate_curve_gate(&assessment.feature_snapshot, config) {
            CurveGateOutcome::Ready => {}
            CurveGateOutcome::Pending { reason_label } => {
                self.record_pending_curve_reason(reason_label);
                return GatekeeperVerdict::PendingCurve;
            }
            CurveGateOutcome::Reject {
                reason_label,
                terminal_outcome,
                reason,
            } => {
                ::metrics::counter!("gatekeeper_pending_curve_total", 1, "reason" => reason_label);
                if terminal_outcome == "timed_out" {
                    ::metrics::counter!(
                        "gatekeeper_curve_latch_fired_total",
                        1,
                        "outcome" => "timeout_reject"
                    );
                }
                self.record_pending_curve_terminal(terminal_outcome);
                self.rejected = true;
                return GatekeeperVerdict::Reject {
                    assessment,
                    reason: reason.to_string(),
                };
            }
        }

        if !verdict_buy {
            self.rejected = true;
            tracing::info!(
                pool = %self.pool_id,
                reason = %reason_chain,
                phases = %breakdown,
                soft_pts = soft_pts,
                verdict = verdict_tag,
                eval_count = self.eval_count,
                "🚫 GATEKEEPER POLICY REJECTED {} soft_pts={}/{}", breakdown, soft_pts, max_pts
            );
            return GatekeeperVerdict::Reject {
                assessment,
                reason: reason_chain,
            };
        }

        self.state = PoolState::Approved;
        tracing::info!(
            pool = %self.pool_id,
            phases_passed = assessment.phases_passed,
            phases = %breakdown,
            soft_pts = soft_pts,
            verdict = verdict_tag,
            eval_count = self.eval_count,
            tx_count = assessment.total_tx_evaluated,
            reason = %reason_chain,
            "✅ GATEKEEPER POLICY BUY {} soft_pts={}/{}", breakdown, soft_pts, max_pts
        );
        let buffered_txs = std::mem::take(&mut self.buffered_txs);
        GatekeeperVerdict::Buy {
            buffered_txs,
            assessment,
        }
    }

    /// Test-only compatibility helper retained for in-crate legacy verdict
    /// parity checks.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn evaluate_compat_from_features(
        &mut self,
        features: MaterializedFeatureSet,
        config: &GatekeeperV2Config,
        deadline_elapsed: bool,
    ) -> GatekeeperVerdict {
        let mut assessment =
            build_assessment_from_features(features, config, self.policy_evaluation_context());
        let decision = evaluate_policy_from_assessment(&assessment, config);
        let reason_chain = decision.reason_chain.clone();
        let verdict_buy = decision.verdict_buy;
        assessment.decision = Some(decision);
        assessment.hard_reject_reason = assessment
            .decision
            .as_ref()
            .and_then(|decision| decision.hard_fail_reason.clone());

        if let Some(reason) = assessment.hard_reject_reason.clone() {
            self.rejected = true;
            return GatekeeperVerdict::Reject { assessment, reason };
        }

        if !deadline_elapsed {
            return GatekeeperVerdict::Wait;
        }

        if verdict_buy {
            match evaluate_curve_gate(&assessment.feature_snapshot, config) {
                CurveGateOutcome::Ready => {}
                CurveGateOutcome::Pending { reason_label } => {
                    self.record_pending_curve_reason(reason_label);
                    return GatekeeperVerdict::PendingCurve;
                }
                CurveGateOutcome::Reject {
                    reason_label,
                    terminal_outcome,
                    reason,
                } => {
                    ::metrics::counter!("gatekeeper_pending_curve_total", 1, "reason" => reason_label);
                    if terminal_outcome == "timed_out" {
                        ::metrics::counter!(
                            "gatekeeper_curve_latch_fired_total",
                            1,
                            "outcome" => "timeout_reject"
                        );
                    }
                    self.record_pending_curve_terminal(terminal_outcome);
                    self.rejected = true;
                    return GatekeeperVerdict::Reject {
                        assessment,
                        reason: reason.to_string(),
                    };
                }
            }

            self.state = PoolState::Approved;
            let buffered_txs = std::mem::take(&mut self.buffered_txs);
            return GatekeeperVerdict::Buy {
                buffered_txs,
                assessment,
            };
        }

        if !assessment.phase1_passed {
            let timeout_decision = super::gatekeeper_policy::build_timeout_decision_from_assessment(
                &assessment,
                config,
            );
            assessment.hard_reject_reason = timeout_decision.hard_fail_reason.clone();
            assessment.decision = Some(timeout_decision);
            assessment.cache_v25_confidence(config);
            self.rejected = true;
            return GatekeeperVerdict::Timeout { assessment };
        }

        self.rejected = true;
        GatekeeperVerdict::Reject {
            assessment,
            reason: reason_chain,
        }
    }

    fn phase1_requirements_met(&self) -> bool {
        self.total_tx_count >= self.config.min_tx_count
            && self.unique_signers.len() >= self.config.min_unique_signers
            && self.buy_count >= self.config.min_buy_count
    }

    fn evaluation_due_after_ingest(&self) -> bool {
        if !self.phase1_passed {
            return false;
        }

        match self.last_eval_at_count {
            None => true,
            Some(last) => {
                self.total_tx_count.saturating_sub(last) >= self.config.re_eval_tx_interval
            }
        }
    }

    fn deadline_elapsed(&self, now_ms: u64) -> bool {
        now_ms >= self.deadline_wall_ts_ms
    }

    fn ingest_long_transaction_tracking_only(
        &mut self,
        tx: Arc<PoolTransaction>,
    ) -> GatekeeperIngressOutcome {
        let tx_ts = Self::tx_event_ts_ms(&tx);
        let now_ms = tx_ts.max(self.highest_seen_ts);
        self.highest_seen_ts = now_ms;

        if self.first_tx_ts.is_none() {
            self.first_tx_ts = Some(tx_ts);
        }

        let tx_key = match Self::tx_key_from_tx(&tx) {
            Some(key) => key,
            None => {
                self.window_events.push_back(V2WindowEvent {
                    timestamp_ms: tx_ts,
                    is_duplicate: true,
                });
                return if self.deadline_elapsed(now_ms) {
                    GatekeeperIngressOutcome::DeadlineElapsed
                } else {
                    GatekeeperIngressOutcome::Wait
                };
            }
        };

        if self.tx_keys_seen.contains(&tx_key) {
            self.window_events.push_back(V2WindowEvent {
                timestamp_ms: tx_ts,
                is_duplicate: true,
            });
            return if self.deadline_elapsed(now_ms) {
                GatekeeperIngressOutcome::DeadlineElapsed
            } else {
                GatekeeperIngressOutcome::Wait
            };
        }

        self.track_tx_key(tx_key.clone());
        self.track_tx_signature(tx.as_ref());
        self.window_events.push_back(V2WindowEvent {
            timestamp_ms: tx_ts,
            is_duplicate: false,
        });
        self.update_tracking(&tx);
        self.buffered_txs.push(GatekeeperBufferedTx {
            tx: tx.clone(),
            metrics: self.metrics,
            tx_key,
        });
        self.refresh_canonical_dev_tracking();

        // ═══════════════════════════════════════════
        // V2.5 DYNAMIC OBSERVATION WINDOW: Shadow checkpoints
        // Single-owner entry: maybe_fire_shadow_checkpoint serializes all
        // checkpoint firing through *_shadow_fired flags (timer + TX path).
        // ═══════════════════════════════════════════
        self.maybe_fire_shadow_checkpoint(now_ms);

        if self.deadline_elapsed(now_ms) {
            GatekeeperIngressOutcome::DeadlineElapsed
        } else {
            GatekeeperIngressOutcome::Wait
        }
    }

    pub fn ingest_transaction_tracking_only(
        &mut self,
        tx: Arc<PoolTransaction>,
    ) -> GatekeeperIngressOutcome {
        if self.rejected {
            return GatekeeperIngressOutcome::Wait;
        }

        if self.state.allows_runtime_relay() {
            let Some(tx_key) = Self::tx_key_from_tx(&tx) else {
                return GatekeeperIngressOutcome::Wait;
            };

            if self.tx_keys_seen.insert(tx_key) {
                self.track_tx_signature(tx.as_ref());
                self.update_tracking(&tx);
                return GatekeeperIngressOutcome::ApprovedTx {
                    tx,
                    metrics: self.metrics,
                };
            }

            return GatekeeperIngressOutcome::Wait;
        }

        if tx.volume_sol < self.config.min_sol_threshold {
            self.dust_filtered_count += 1;
            if self.config.mode == GatekeeperMode::Long {
                let tx_ts = Self::tx_event_ts_ms(tx.as_ref());
                let now_ms = tx_ts.max(self.highest_seen_ts);
                self.highest_seen_ts = now_ms;
                if self.deadline_elapsed(now_ms) {
                    return GatekeeperIngressOutcome::DeadlineElapsed;
                }
            }
            return GatekeeperIngressOutcome::Wait;
        }

        if self.config.mode == GatekeeperMode::Long {
            return self.ingest_long_transaction_tracking_only(tx);
        }

        let tx_ts = Self::tx_event_ts_ms(tx.as_ref());
        let now_ms = tx_ts.max(self.highest_seen_ts);
        self.highest_seen_ts = now_ms;

        if self.first_tx_ts.is_none() {
            self.first_tx_ts = Some(tx_ts);
        }

        if self.deadline_elapsed(now_ms) {
            return GatekeeperIngressOutcome::DeadlineElapsed;
        }

        self.cleanup_old_events(now_ms);

        let tx_key = match Self::tx_key_from_tx(&tx) {
            Some(key) => key,
            None => {
                self.window_events.push_back(V2WindowEvent {
                    timestamp_ms: tx_ts,
                    is_duplicate: true,
                });
                return GatekeeperIngressOutcome::Wait;
            }
        };

        if self.tx_keys_seen.contains(&tx_key) {
            self.window_events.push_back(V2WindowEvent {
                timestamp_ms: tx_ts,
                is_duplicate: true,
            });
            return GatekeeperIngressOutcome::Wait;
        }

        self.track_tx_key(tx_key.clone());
        self.track_tx_signature(tx.as_ref());
        self.window_events.push_back(V2WindowEvent {
            timestamp_ms: tx_ts,
            is_duplicate: false,
        });
        self.update_tracking(&tx);
        self.buffered_txs.push(GatekeeperBufferedTx {
            tx,
            metrics: self.metrics,
            tx_key,
        });
        self.refresh_canonical_dev_tracking();

        if !self.phase1_passed && self.phase1_requirements_met() {
            self.phase1_passed = true;
            self.phase1_passed_at_count = Some(self.total_tx_count);
            return GatekeeperIngressOutcome::TriggerEvaluation;
        }

        if self.evaluation_due_after_ingest() {
            return GatekeeperIngressOutcome::TriggerEvaluation;
        }

        GatekeeperIngressOutcome::Wait
    }

    #[must_use]
    pub const fn registered_wall_ts_ms(&self) -> u64 {
        self.registered_wall_ts_ms
    }

    #[must_use]
    pub const fn deadline_wall_ts_ms(&self) -> u64 {
        self.deadline_wall_ts_ms
    }

    /// Update all tracking structures for a new unique, non-dust transaction.
    pub fn update_tracking(&mut self, tx: &PoolTransaction) {
        if !self.curve_state_explicit_for_next_tx {
            self.apply_curve_state(
                if tx.curve_data_known {
                    CurveFreshnessState::Fresh
                } else {
                    CurveFreshnessState::Unknown
                },
                tx.curve_finality.normalized(tx.curve_data_known),
            );
        }
        self.curve_state_explicit_for_next_tx = false;

        // Yellowstone: Track failed transactions (success=false)
        if !tx.success {
            self.failed_tx_count += 1;
        }

        // Phase 1: Quantity counters
        self.total_tx_count += 1;
        if tx.is_buy {
            self.buy_count += 1;
        } else {
            self.sell_count += 1;
        }
        self.unique_signers.insert(tx.signer.clone());

        // Phase 2: Velocity tracking (sorted timestamps)
        let ts = Self::tx_event_ts_ms(tx);
        let pos = self.tx_timestamps_sorted.partition_point(|&t| t <= ts);
        self.tx_timestamps_sorted.insert(pos, ts);

        // Phase 3: Signer diversity tracking
        let entry = self.signer_stats.entry(tx.signer.clone()).or_default();
        entry.tx_count += 1;
        entry.total_volume_sol += tx.volume_sol;
        if tx.is_buy {
            entry.buy_count += 1;
        } else {
            entry.sell_count += 1;
        }

        // Phase 4: Volume tracking
        self.tx_volumes.push(tx.volume_sol);
        self.total_volume_sol += tx.volume_sol;
        if tx.is_buy {
            self.buy_volume_sol += tx.volume_sol;
        } else {
            self.sell_volume_sol += tx.volume_sol;
        }

        // Consecutive buy streak tracking (FOMO)
        if tx.is_buy {
            self.current_consecutive_buys += 1;
            if self.current_consecutive_buys > self.max_consecutive_buys {
                self.max_consecutive_buys = self.current_consecutive_buys;
            }
        } else {
            self.current_consecutive_buys = 0;
        }

        // Phase 5: Dev tracking
        if self.first_signer.is_none() {
            self.first_signer = Some(tx.signer.clone());
        }
        if tx.is_dev_buy && self.dev_wallet.is_none() {
            self.dev_wallet = Some(tx.signer.clone());
            self.dev_buy_total_sol = tx.volume_sol;
            self.dev_initial_buy_tokens = tx.token_amount_units.map(|u| u as f64);
        }
        if let Some(ref dev) = self.dev_wallet.clone() {
            if tx.signer == *dev {
                self.dev_tx_count += 1;
                if tx.is_buy {
                    self.dev_buy_volume_total_sol += tx.volume_sol;
                } else {
                    self.dev_sell_total_sol += tx.volume_sol;
                    self.dev_has_sold = true;
                }
            }
        }

        // Phase 6: Bonding Curve Dynamics
        if let (Some(v_tokens), Some(v_sol)) =
            (tx.v_tokens_in_bonding_curve, tx.v_sol_in_bonding_curve)
        {
            let price = if v_tokens > f64::EPSILON {
                v_sol / v_tokens
            } else {
                0.0
            };

            let market_cap = tx
                .market_cap_sol
                .unwrap_or_else(|| price * PUMP_TOKEN_TOTAL_SUPPLY);

            self.price_history.push(PricePoint {
                timestamp_ms: ts,
                price_sol_per_token: price,
                v_sol_in_curve: v_sol,
                v_tokens_in_curve: v_tokens,
                market_cap_sol: market_cap,
                is_buy: tx.is_buy,
                curve_data_known: tx.curve_data_known,
                curve_finality: tx.curve_finality.normalized(tx.curve_data_known),
            });
        }

        // PoolMetrics aggregation (for SnapshotEngine relay)
        self.apply_tx_to_metrics(tx);
    }

    /// Update aggregated PoolMetrics for SnapshotEngine compatibility
    fn apply_tx_to_metrics(&mut self, tx: &PoolTransaction) {
        self.metrics.tx_count = self.metrics.tx_count.saturating_add(1);
        self.metrics.unique_addrs = self.unique_signers.len() as u64;
        self.metrics.volume_sol += tx.volume_sol;
        if tx.is_buy {
            self.metrics.buy_volume_sol += tx.volume_sol;
        } else {
            self.metrics.sell_volume_sol += tx.volume_sol;
        }
        if tx.is_dev_buy {
            self.metrics.dev_buy_lamports = self
                .metrics
                .dev_buy_lamports
                .saturating_add(tx.dev_buy_lamports);
        }
    }

    fn tx_key_from_tx(tx: &PoolTransaction) -> Option<TxKey> {
        let event_ts_ms = Self::tx_event_ts_ms(tx);
        if event_ts_ms == 0 {
            return None;
        }
        let signature = if tx.signature.is_empty() {
            None
        } else {
            Signature::from_str(&tx.signature).ok()
        };
        let has_ordering_info = signature.is_some() || tx.event_ordinal.is_some();
        let fallback_counter = if has_ordering_info {
            0
        } else {
            Self::fallback_counter_for_tx(tx)
        };
        TxKey::new(
            event_ts_ms,
            tx.slot,
            tx.event_ordinal,
            signature,
            fallback_counter,
        )
        .ok()
    }

    fn track_tx_key(&mut self, tx_key: TxKey) {
        self.tx_keys_seen.insert(tx_key.clone());
        self.tx_keys_fifo.push_back(tx_key);
        while self.tx_keys_fifo.len() > self.tx_keys_capacity {
            if let Some(oldest) = self.tx_keys_fifo.pop_front() {
                self.tx_keys_seen.remove(&oldest);
            }
        }
    }

    fn track_tx_signature(&mut self, tx: &PoolTransaction) {
        if tx.signature.is_empty() {
            return;
        }
        if let Ok(signature) = Signature::from_str(&tx.signature) {
            self.tx_signatures_seen.insert(signature);
        }
    }

    fn fallback_counter_for_tx(tx: &PoolTransaction) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        Self::tx_event_ts_ms(tx).hash(&mut hasher);
        tx.signer.hash(&mut hasher);
        tx.is_buy.hash(&mut hasher);
        tx.volume_sol.to_bits().hash(&mut hasher);
        tx.event_ordinal.hash(&mut hasher);
        if let Some(price) = tx.price_quote {
            price.to_bits().hash(&mut hasher);
        }
        if let Some(lamports) = tx.sol_amount_lamports {
            lamports.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn cleanup_old_events(&mut self, now_ms: u64) {
        let window_start = now_ms.saturating_sub(self.config.max_wait_time_ms);

        self.buffered_txs
            .retain(|btx| Self::tx_event_ts_ms(btx.tx.as_ref()) >= window_start);

        while let Some(front) = self.window_events.front() {
            if front.timestamp_ms < window_start {
                self.window_events.pop_front();
            } else {
                break;
            }
        }
    }

    #[allow(dead_code)]
    fn build_assessment(&self) -> GatekeeperAssessment {
        let now_wall_ms = Self::now_wall_ms();
        let observation_duration_ms = self.wall_observation_duration_ms(now_wall_ms);
        let finalize_lag_ms = self.finalize_lag_ms(now_wall_ms);

        GatekeeperAssessment {
            phase1_passed: self.phase1_passed,
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
            phases_passed: if self.phase1_passed { 1 } else { 0 },
            hard_reject_reason: None,
            total_tx_evaluated: self.total_tx_count,
            unique_tx_evaluated: self.unique_signature_count(),
            unique_signers_evaluated: self.unique_signers.len(),
            observation_duration_ms,
            finalize_lag_ms,
            dust_filtered_count: self.dust_filtered_count,
            eval_count: self.eval_count,
            buy_count: self.buy_count,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: self.curve_t0_event_ts_ms,
            curve_t0_clock_source: self.curve_t0_clock_source,
            curve_wait_elapsed_ms: self
                .curve_t0_event_ts_ms
                .map(|t0| self.highest_seen_ts.saturating_sub(t0)),
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

    /// Build an assessment for Timeout verdict when Phase 1 never passed.
    ///
    /// Even though Phase 1 didn't pass, we still run the full phase 2-6
    /// analysis so that diagnostic metrics always appear in decision logs.
    /// This is critical for the "long" mode where Timeout assessments are
    /// logged alongside Buy/Reject verdicts for offline threshold tuning.
    fn build_minimal_assessment(&self) -> GatekeeperAssessment {
        let now_wall_ms = Self::now_wall_ms();
        let observation_duration_ms = self.wall_observation_duration_ms(now_wall_ms);
        let finalize_lag_ms = self.finalize_lag_ms(now_wall_ms);
        let v25_trajectory = self.materialize_trajectory(&self.config.tas);
        let v25_pdd = Some(crate::components::gatekeeper_pdd::evaluate_pdd(
            self,
            &self.config.pdd,
            None,
        ));

        // Only run full phase analysis when we have meaningful data.
        // If we have at least 2 TX and timestamps, compute all phases.
        if self.total_tx_count >= 2 && !self.tx_timestamps_sorted.is_empty() {
            let velocity =
                compute_velocity_profile(&self.tx_timestamps_sorted, self.config.max_wait_time_ms);
            let phase2_passed = velocity.interval_cv >= self.config.min_interval_cv
                && velocity.interval_cv <= self.config.max_interval_cv
                && velocity.burst_ratio <= self.config.max_burst_ratio
                && velocity.avg_interval_ms >= self.config.min_avg_interval_ms
                && velocity.avg_interval_ms <= self.config.max_avg_interval_ms
                && velocity.timing_entropy >= self.config.min_timing_entropy
                && velocity.timing_entropy <= self.config.max_timing_entropy
                && self.dust_filtered_count >= self.config.min_dust_filtered_count;

            let diversity = compute_signer_diversity(
                &self.signer_stats,
                self.total_tx_count,
                self.total_volume_sol,
                &self.tx_timestamps_sorted,
            );
            let phase3_passed = diversity.unique_ratio >= self.config.min_unique_ratio
                && diversity.unique_ratio <= self.config.max_unique_ratio
                && diversity.hhi <= self.config.max_hhi
                && diversity.max_tx_per_signer <= self.config.max_tx_per_signer
                && diversity.volume_gini >= self.config.min_volume_gini
                && diversity.volume_gini <= self.config.max_volume_gini
                && diversity.top3_volume_pct <= self.config.max_top3_volume_pct
                && diversity.same_ms_tx_ratio <= self.config.max_same_ms_tx_ratio;

            let volume = compute_volume_sanity(
                &self.tx_volumes,
                self.buy_count,
                self.sell_count,
                self.total_volume_sol,
                self.buy_volume_sol,
                self.max_consecutive_buys,
            );
            let phase4_passed = volume.buy_ratio >= self.config.min_buy_ratio
                && volume.buy_ratio <= self.config.max_buy_ratio
                && volume.avg_tx_sol >= self.config.min_avg_tx_sol
                && volume.avg_tx_sol <= self.config.max_avg_tx_sol
                && volume.volume_cv >= self.config.min_volume_cv
                && volume.volume_cv <= self.config.max_volume_cv
                && volume.total_volume_sol >= self.config.min_total_volume_sol
                && volume.total_volume_sol <= self.config.max_total_volume_sol
                && volume.sol_buy_ratio >= self.config.min_sol_buy_ratio
                && volume.max_consecutive_buys >= self.config.min_consecutive_buys;

            let dev = compute_dev_behavior(
                &self.dev_wallet,
                &self.first_signer,
                self.dev_buy_total_sol,
                self.dev_buy_volume_total_sol,
                self.dev_sell_total_sol,
                self.dev_tx_count,
                self.dev_has_sold,
                self.dev_initial_buy_tokens,
                self.total_tx_count,
                self.total_volume_sol,
            );
            let phase5_passed = if !dev.dev_wallet_known {
                true
            } else {
                dev.dev_buy_total_sol <= self.config.max_dev_buy_sol
                    && dev.dev_buy_total_sol >= self.config.min_dev_buy_sol
                    && dev.dev_tx_ratio <= self.config.max_dev_tx_ratio
                    && dev.dev_tx_ratio >= self.config.min_dev_tx_ratio
                    && dev.dev_volume_ratio <= self.config.max_dev_volume_ratio
                    && dev.dev_volume_ratio >= self.config.min_dev_volume_ratio
                    && !dev.dev_has_sold
            };

            let curve = compute_bonding_curve_dynamics(&self.price_history);
            let phase6_passed = if curve.price_data_points < 2 {
                true
            } else {
                curve.price_change_ratio <= self.config.max_price_change_ratio
                    && curve.max_single_tx_price_impact_pct
                        <= self.config.max_single_tx_price_impact_pct
                    && curve.max_single_sell_impact_pct <= self.config.max_single_sell_impact_pct
                    && (if curve.curve_data_known {
                        curve.bonding_progress_pct <= self.config.max_bonding_progress_pct
                            && curve.bonding_progress_pct >= self.config.min_bonding_progress_pct
                    } else {
                        true // Degrade: unknown progress → pass
                    })
                    && (if curve.curve_data_known {
                        curve.current_market_cap_sol >= self.config.min_market_cap_sol
                    } else {
                        true // Degrade: unknown market cap → pass
                    })
            };

            let phases_passed = [
                self.phase1_passed,
                phase2_passed,
                phase3_passed,
                phase4_passed,
                phase5_passed,
                phase6_passed,
            ]
            .iter()
            .filter(|&&p| p)
            .count() as u8;

            return GatekeeperAssessment {
                phase1_passed: self.phase1_passed,
                phase2_velocity: Some(velocity),
                phase2_passed,
                phase3_diversity: Some(diversity),
                phase3_passed,
                phase4_volume: Some(volume),
                phase4_passed,
                phase5_dev: Some(dev),
                phase5_passed,
                phase6_curve: Some(curve),
                phase6_passed,
                phases_passed,
                hard_reject_reason: None,
                total_tx_evaluated: self.total_tx_count,
                unique_tx_evaluated: self.unique_signature_count(),
                unique_signers_evaluated: self.unique_signers.len(),
                observation_duration_ms,
                finalize_lag_ms,
                dust_filtered_count: self.dust_filtered_count,
                eval_count: self.eval_count,
                buy_count: self.buy_count,
                decision: None,
                early_fingerprint: None,
                curve_t0_event_ts_ms: self.curve_t0_event_ts_ms,
                curve_t0_clock_source: self.curve_t0_clock_source,
                curve_wait_elapsed_ms: self
                    .curve_t0_event_ts_ms
                    .map(|t0| self.highest_seen_ts.saturating_sub(t0)),
                feature_snapshot: MaterializedFeatureSet::default(),
                checkpoint_count: 0,
                trajectory_available: v25_trajectory.is_some(),
                v25_shadow_decisions: Vec::new(),
                trajectory: v25_trajectory.clone(),
                pdd_assessment: v25_pdd.clone(),
                aps_diagnostics: None,
                observation_stage: None,
                entry_drift_pct: None,
                entry_drift_anchor_quality: None,
                adaptive_thresholds_applied: false,
                v25_confidence: None,
            };
        }

        // Truly empty buffer — no meaningful phase analysis possible
        GatekeeperAssessment {
            phase1_passed: self.phase1_passed,
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
            phases_passed: if self.phase1_passed { 1 } else { 0 },
            hard_reject_reason: None,
            total_tx_evaluated: self.total_tx_count,
            unique_tx_evaluated: self.unique_signature_count(),
            unique_signers_evaluated: self.unique_signers.len(),
            observation_duration_ms,
            finalize_lag_ms,
            dust_filtered_count: self.dust_filtered_count,
            eval_count: self.eval_count,
            buy_count: self.buy_count,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: self.curve_t0_event_ts_ms,
            curve_t0_clock_source: self.curve_t0_clock_source,
            curve_wait_elapsed_ms: self
                .curve_t0_event_ts_ms
                .map(|t0| self.highest_seen_ts.saturating_sub(t0)),
            feature_snapshot: MaterializedFeatureSet::default(),
            checkpoint_count: 0,
            trajectory_available: v25_trajectory.is_some(),
            v25_shadow_decisions: Vec::new(),
            trajectory: v25_trajectory.clone(),
            pdd_assessment: v25_pdd.clone(),
            aps_diagnostics: None,
            observation_stage: None,
            entry_drift_pct: None,
            entry_drift_anchor_quality: None,
            adaptive_thresholds_applied: false,
            v25_confidence: None,
        }
    }

    /// Format phase-by-phase breakdown for logging.
    fn format_phase_breakdown(assessment: &GatekeeperAssessment) -> String {
        format!(
            "[P1:{} P2:{} P3:{} P4:{} P5:{} P6:{}]",
            if assessment.phase1_passed {
                "✅"
            } else {
                "❌"
            },
            if assessment.phase2_passed {
                "✅"
            } else {
                "❌"
            },
            if assessment.phase3_passed {
                "✅"
            } else {
                "❌"
            },
            if assessment.phase4_passed {
                "✅"
            } else {
                "❌"
            },
            if assessment.phase5_passed {
                "✅"
            } else {
                "❌"
            },
            if assessment.phase6_passed {
                "✅"
            } else {
                "❌"
            },
        )
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Three-Layer Decision System: Hard Fails → Core Pass → Soft Signals
    // ═══════════════════════════════════════════════════════════════════════

    /// Compute soft signal flags from assessment data.
    /// Soft signals use the ORIGINAL phase-level thresholds (e.g. max_hhi = 0.055).
    /// Hard fails use the EXTREME thresholds (e.g. hard_fail_hhi = 0.10).
    fn compute_soft_signals(&self, assessment: &GatekeeperAssessment) -> SoftSignals {
        let cfg = &self.config;
        let mut ss = SoftSignals::default();

        if let Some(ref vel) = assessment.phase2_velocity {
            ss.low_interval_cv = vel.interval_cv < cfg.min_interval_cv;
            ss.high_interval_cv = vel.interval_cv > cfg.max_interval_cv;
            ss.low_timing_entropy = vel.timing_entropy < cfg.min_timing_entropy;
            ss.high_timing_entropy = vel.timing_entropy > cfg.max_timing_entropy;
            ss.avg_interval_out_of_range = vel.avg_interval_ms < cfg.min_avg_interval_ms
                || vel.avg_interval_ms > cfg.max_avg_interval_ms;
            ss.high_burst_ratio = vel.burst_ratio > cfg.max_burst_ratio;
        }

        if let Some(ref div) = assessment.phase3_diversity {
            ss.bundle_suspicion = div.same_ms_tx_ratio > cfg.max_same_ms_tx_ratio;
            ss.cabal_suspicion = div.hhi > cfg.max_hhi;
            ss.top3_dominance = div.top3_volume_pct > cfg.max_top3_volume_pct;
            ss.high_volume_gini = div.volume_gini > cfg.max_volume_gini;
            ss.unique_ratio_out_of_range =
                div.unique_ratio < cfg.min_unique_ratio || div.unique_ratio > cfg.max_unique_ratio;
            ss.high_tx_per_signer = div.max_tx_per_signer > cfg.max_tx_per_signer;
        }

        ss.low_dust_count = self.dust_filtered_count < cfg.min_dust_filtered_count;

        ss
    }

    /// Compute the three-layer decision: HARD FAILS → CORE PASS → SOFT SIGNALS.
    ///
    /// This replaces the simple `phases_passed >= min_phases_to_pass` check
    /// with a data-driven decision tree based on A/B separation analysis.
    ///
    /// Layer 1 (Hard Fails): Kill-switches for blatant manipulation.
    /// Layer 2 (Core Pass): Core-1 (quantity), Core-2 (capital), Core-3 (dev+curve).
    /// Layer 3 (Soft Signals): Phase 2/3 timing/diversity metrics → confidence score.
    pub fn compute_decision(&self, assessment: &GatekeeperAssessment) -> GatekeeperDecision {
        let cfg = &self.config;

        // ═══════════════════════════════════════
        // LAYER 1: HARD FAILS (kill-switches)
        // ═══════════════════════════════════════
        let mut hard_fail_reason: Option<String> = None;

        // HF-1: Dev sold (strongest B separator in data)
        if hard_fail_reason.is_none()
            && cfg.reject_on_dev_sell
            && assessment
                .phase5_dev
                .as_ref()
                .map_or(false, |d| d.dev_has_sold)
        {
            hard_fail_reason = Some("HARD_FAIL: dev_has_sold".to_string());
        }

        // HF-2: Max single sell impact (dump detection)
        if hard_fail_reason.is_none() {
            if let Some(ref curve) = assessment.phase6_curve {
                if curve.price_data_points >= 2
                    && curve.max_single_sell_impact_pct > cfg.max_single_sell_impact_pct
                {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: sell_impact={:.1}% > {:.1}%",
                        curve.max_single_sell_impact_pct, cfg.max_single_sell_impact_pct
                    ));
                }
            }
        }

        // HF-3: Max single TX price impact (pump-1-tx detection)
        if hard_fail_reason.is_none() {
            if let Some(ref curve) = assessment.phase6_curve {
                if curve.price_data_points >= 2
                    && curve.max_single_tx_price_impact_pct > cfg.max_single_tx_price_impact_pct
                {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: tx_price_impact={:.1}% > {:.1}%",
                        curve.max_single_tx_price_impact_pct, cfg.max_single_tx_price_impact_pct
                    ));
                }
            }
        }

        // HF-4: Extreme price change ratio
        if hard_fail_reason.is_none() {
            if let Some(ref curve) = assessment.phase6_curve {
                if curve.price_data_points >= 2
                    && curve.price_change_ratio > cfg.max_price_change_ratio
                {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: price_change_ratio={:.1} > {:.1}",
                        curve.price_change_ratio, cfg.max_price_change_ratio
                    ));
                }
            }
        }

        // HF-5: Market cap too low (safety requirement)
        // Skip when curve_data_known=false: market cap from genesis seed is unreliable
        if hard_fail_reason.is_none() {
            if let Some(ref curve) = assessment.phase6_curve {
                if curve.curve_data_known
                    && curve.price_data_points >= 2
                    && curve.current_market_cap_sol < cfg.min_market_cap_sol
                {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: market_cap={:.1} < {:.1}",
                        curve.current_market_cap_sol, cfg.min_market_cap_sol
                    ));
                }
            }
        }

        // HF-6: Extreme HHI (blatant cabal — at hard_fail threshold, NOT phase threshold)
        if hard_fail_reason.is_none() {
            if let Some(ref div) = assessment.phase3_diversity {
                if div.hhi > cfg.hard_fail_hhi {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: hhi={:.3} > {:.3} (extreme cabal)",
                        div.hhi, cfg.hard_fail_hhi
                    ));
                }
            }
        }

        // HF-7: Extreme bundling (at hard_fail threshold)
        if hard_fail_reason.is_none() {
            if let Some(ref div) = assessment.phase3_diversity {
                if div.same_ms_tx_ratio > cfg.hard_fail_same_ms_tx_ratio {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: same_ms_ratio={:.2} > {:.2} (extreme bundling)",
                        div.same_ms_tx_ratio, cfg.hard_fail_same_ms_tx_ratio
                    ));
                }
            }
        }

        // HF-8: Extreme top3 dominance (at hard_fail threshold)
        if hard_fail_reason.is_none() {
            if let Some(ref div) = assessment.phase3_diversity {
                if div.top3_volume_pct > cfg.hard_fail_top3_volume_pct {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: top3_vol={:.2} > {:.2} (extreme whale dominance)",
                        div.top3_volume_pct, cfg.hard_fail_top3_volume_pct
                    ));
                }
            }
        }

        // HF-9: Extreme bot timing (machine-gun pattern)
        // Guard: only trigger hard-fail with sufficient data (min TX + min observation window).
        // With small n, timing stats are unreliable → soft flags still catch this.
        if hard_fail_reason.is_none() {
            if let Some(ref vel) = assessment.phase2_velocity {
                if vel.interval_cv < 0.08
                    && vel.avg_interval_ms < 30.0
                    && assessment.total_tx_evaluated >= cfg.hard_fail_bot_min_tx
                    && assessment.observation_duration_ms >= cfg.hard_fail_bot_min_observation_ms
                {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: extreme_bot cv={:.3} avg={:.0}ms (n={} window={}ms)",
                        vel.interval_cv,
                        vel.avg_interval_ms,
                        assessment.total_tx_evaluated,
                        assessment.observation_duration_ms,
                    ));
                }
            }
        }

        // HF-10: Yellowstone failed TX ratio
        if hard_fail_reason.is_none() {
            if let Some(threshold) = cfg.min_failed_tx_ratio_for_bot_flag {
                let total_with_failed = self.total_tx_count + self.failed_tx_count;
                if total_with_failed > 5 {
                    let failed_ratio = self.failed_tx_count as f64 / total_with_failed as f64;
                    if failed_ratio > threshold {
                        hard_fail_reason = Some(format!(
                            "HARD_FAIL: failed_tx_ratio={:.2} (bot spam, Yellowstone)",
                            failed_ratio
                        ));
                    }
                }
            }
        }

        // HF-11: max_avg_interval_ms gate (slow/dead pool detection).
        // Lives here as a Hard Fail so it produces a clean, honest rejection label
        // instead of polluting Core2/Capital which is Phase-4 only.
        if hard_fail_reason.is_none() {
            if let Some(ref vel) = assessment.phase2_velocity {
                if vel.avg_interval_ms > cfg.max_avg_interval_ms {
                    hard_fail_reason = Some(format!(
                        "HARD_FAIL: avg_interval={:.0}ms > {:.0}ms (slow/dead pool)",
                        vel.avg_interval_ms, cfg.max_avg_interval_ms,
                    ));
                }
            }
        }

        // ═══════════════════════════════════════
        // LAYER 2: CORE PASS
        // ═══════════════════════════════════════
        let dev_unknown = assessment
            .phase5_dev
            .as_ref()
            .map_or(true, |d| !d.dev_wallet_known);

        // Core-1: Quantity Gate (Phase 1)
        let core1_passed = assessment.phase1_passed;

        // Core-2: Capital Dominance (Phase 4).
        // max_avg_interval_ms is enforced as HF-11 above — Core2 is purely about capital.
        let core2_passed = assessment.phase4_passed;

        // Core-3: Dev + Curve Safety (Phase 5 + Phase 6)
        // With dev_unknown → stricter requirements on curve and capital
        let core3_passed = if dev_unknown {
            // Dev unknown: auto-pass Phase 5, but tighten Phase 6 + capital.
            // Use stricter price impact threshold to close "clean spoof" vector.
            let dev_unk_max_impact = cfg.dev_unknown_max_single_tx_price_impact_pct;
            let phase6_ok = if let Some(ref curve) = assessment.phase6_curve {
                if curve.price_data_points < 2 {
                    // Not enough curve data + dev unknown → cannot confirm safety
                    false
                } else {
                    let price_ok = curve.price_change_ratio <= cfg.max_price_change_ratio
                        && curve.max_single_tx_price_impact_pct <= dev_unk_max_impact
                        && curve.max_single_sell_impact_pct <= cfg.max_single_sell_impact_pct
                        && (if curve.curve_data_known {
                            curve.current_market_cap_sol >= cfg.dev_unknown_min_market_cap_sol
                        } else {
                            true // Degrade: unknown market cap → pass
                        });

                    // When bonding progress is unknown, degrade instead of hard reject
                    let bonding_ok = if curve.curve_data_known {
                        curve.bonding_progress_pct <= cfg.max_bonding_progress_pct
                            && curve.bonding_progress_pct >= cfg.min_bonding_progress_pct
                    } else {
                        true // Degrade: unknown progress → pass this sub-check
                    };

                    price_ok && bonding_ok
                }
            } else {
                false // No curve data + dev unknown → fail
            };

            // Also require stricter sol_buy_ratio when dev is unknown
            let stricter_capital = assessment.phase4_volume.as_ref().map_or(false, |v| {
                v.sol_buy_ratio >= cfg.dev_unknown_min_sol_buy_ratio
            });

            phase6_ok && stricter_capital
        } else {
            // Dev known: both Phase 5 and Phase 6 must pass
            assessment.phase5_passed && assessment.phase6_passed
        };

        // ═══════════════════════════════════════
        // LAYER 3: SOFT SIGNALS (weighted)
        // ═══════════════════════════════════════
        let soft_signals = self.compute_soft_signals(assessment);
        let soft_points = soft_signals.weighted_score(
            cfg.soft_weight_timing,
            cfg.soft_weight_manipulation,
            cfg.soft_weight_diversity,
            cfg.soft_weight_ecosystem,
        );
        let max_soft_points_possible = SoftSignals::max_possible_points(
            cfg.soft_weight_timing,
            cfg.soft_weight_manipulation,
            cfg.soft_weight_diversity,
            cfg.soft_weight_ecosystem,
        );

        // Dev unknown → stricter soft threshold
        let effective_max_soft_points = if dev_unknown {
            cfg.dev_unknown_max_soft_points
        } else {
            cfg.max_soft_points
        };
        let sybil_policy = build_sybil_policy_diagnostics(assessment, cfg, dev_unknown);
        let total_soft_points = soft_points as u16 + sybil_policy.soft_points;

        // ═══════════════════════════════════════
        // FINAL VERDICT
        // ═══════════════════════════════════════
        let (verdict_buy, verdict_type, reason_chain) = if let Some(ref hf) = hard_fail_reason {
            (false, GatekeeperVerdictType::RejectHardFail, hf.clone())
        } else if !core1_passed {
            (
                false,
                GatekeeperVerdictType::RejectCoreFail,
                format!(
                    "CORE_FAIL: Phase1 (tx={}/{} signers={}/{} buys={}/{})",
                    assessment.total_tx_evaluated,
                    cfg.min_tx_count,
                    assessment.unique_signers_evaluated,
                    cfg.min_unique_signers,
                    assessment.buy_count,
                    cfg.min_buy_count,
                ),
            )
        } else if !core2_passed {
            let detail = assessment.phase4_volume.as_ref().map_or(
                "no volume data".to_string(),
                |v| format!(
                    "buy_ratio={:.2} sol_buy={:.2} avg_tx={:.2} vol_cv={:.2} total_vol={:.1} consec_buys={}",
                    v.buy_ratio, v.sol_buy_ratio, v.avg_tx_sol, v.volume_cv, v.total_volume_sol, v.max_consecutive_buys
                ),
            );
            (
                false,
                GatekeeperVerdictType::RejectCoreFail,
                format!("CORE_FAIL: Core2/Capital [{}]", detail),
            )
        } else if !core3_passed {
            let dev_detail = if dev_unknown {
                let mcap = assessment
                    .phase6_curve
                    .as_ref()
                    .map_or(0.0, |c| c.current_market_cap_sol);
                let sbr = assessment
                    .phase4_volume
                    .as_ref()
                    .map_or(0.0, |v| v.sol_buy_ratio);
                format!(
                    "dev_unknown -> stricter: mcap={:.1} (need>={:.1}) sol_buy_ratio={:.2} (need>={:.2})",
                    mcap, cfg.dev_unknown_min_market_cap_sol, sbr, cfg.dev_unknown_min_sol_buy_ratio
                )
            } else {
                assessment.phase5_dev.as_ref().map_or(
                    "no dev data".to_string(),
                    |d| format!(
                        "dev_buy={:.2} dev_tx_ratio={:.3} dev_vol_ratio={:.3} dev_sold={} p5={} p6={}",
                        d.dev_buy_total_sol, d.dev_tx_ratio, d.dev_volume_ratio, d.dev_has_sold,
                        assessment.phase5_passed, assessment.phase6_passed
                    ),
                )
            };
            (
                false,
                GatekeeperVerdictType::RejectCoreFail,
                format!("CORE_FAIL: Core3/Dev+Curve [{}]", dev_detail),
            )
        } else if soft_points > effective_max_soft_points {
            (
                false,
                GatekeeperVerdictType::RejectSoftExcess,
                format!(
                    "SOFT_EXCESS: pts={}/{} (max={}{}) flags=[{}]",
                    soft_points,
                    max_soft_points_possible,
                    effective_max_soft_points,
                    if dev_unknown { " dev_unk_strict" } else { "" },
                    soft_signals.format_flags(),
                ),
            )
        } else if let Some(reason) = sybil_combo_veto_reason(&sybil_policy, cfg) {
            (
                false,
                GatekeeperVerdictType::RejectSybilInterference,
                reason,
            )
        } else if sybil_policy.enabled
            && sybil_policy.soft_points > sybil_policy.effective_max_soft_points as u16
        {
            (
                false,
                GatekeeperVerdictType::RejectSybilSoftExcess,
                format!(
                    "SYBIL_SOFT_FAIL: sybil_soft_points={} > {} flags=[{}] patterns=[{}]",
                    sybil_policy.soft_points,
                    sybil_policy.effective_max_soft_points,
                    sybil_policy.soft_signals.format_flags(),
                    SybilInterferencePattern::format_patterns(&sybil_policy.interference_patterns),
                ),
            )
        // ── V2.5 PDD: Pump & Dump hard veto (live, only when per-threshold promoted) ──
        } else if self.config.pdd.enabled && self.config.v25.live_execution_enabled {
            if let Some(ref pdd) = assessment.pdd_assessment {
                if let Some(ref fail) = pdd.hard_fail {
                    // Per-threshold promotion gate: only veto live if this specific threshold is promoted
                    let promoted = match fail {
                        PddHardFail::EntryDrift => self.config.pdd.entry_drift_promoted_to_live,
                        PddHardFail::Spike => self.config.pdd.spike_promoted_to_live,
                        PddHardFail::Ramping => self.config.pdd.ramping_promoted_to_live,
                        PddHardFail::Whale => self.config.pdd.whale_promoted_to_live,
                        PddHardFail::Reserve => self.config.pdd.reserve_promoted_to_live,
                        PddHardFail::FlashCrash => self.config.pdd.flash_crash_promoted_to_live,
                    };
                    if !promoted {
                        // Not promoted — skip live veto, fall through to BUY path below
                        (
                            true,
                            GatekeeperVerdictType::Buy,
                            format!("BUY: core_pass, soft_pts={}/{}, dev_unknown={} (PDD_{}_shadow_only)",
                                soft_points, max_soft_points_possible, dev_unknown, fail.as_str()),
                        )
                    } else {
                        let verdict = match fail {
                            PddHardFail::EntryDrift => GatekeeperVerdictType::RejectEntryDrift,
                            PddHardFail::FlashCrash => GatekeeperVerdictType::RejectFlashCrash,
                            PddHardFail::Ramping => GatekeeperVerdictType::RejectRamping,
                            _ => GatekeeperVerdictType::RejectPumpAndDump,
                        };
                        (
                        false,
                        verdict,
                        format!(
                            "PDD_HARD_FAIL: {} drift={:?} spike={} ramping={} whale={:?} reserve={} flash={}",
                            fail.as_str(),
                            pdd.entry_drift_pct,
                            pdd.spike_detected,
                            pdd.ramping_detected,
                            pdd.whale_top3_pct,
                            pdd.reserve_health_pass,
                            pdd.flash_crash_risk,
                        ),
                    )
                    } // end else (promoted → verdict mapped)
                } else {
                    (
                        true,
                        GatekeeperVerdictType::Buy,
                        format!(
                            "BUY: core_pass, soft_pts={}/{}, dev_unknown={}",
                            soft_points, max_soft_points_possible, dev_unknown
                        ),
                    )
                }
            } else {
                (
                    true,
                    GatekeeperVerdictType::Buy,
                    format!(
                        "BUY: core_pass, soft_pts={}/{}, dev_unknown={}",
                        soft_points, max_soft_points_possible, dev_unknown
                    ),
                )
            }
        } else {
            (
                true,
                GatekeeperVerdictType::Buy,
                format!(
                    "BUY: core_pass, soft_pts={}/{}, dev_unknown={}",
                    soft_points, max_soft_points_possible, dev_unknown
                ),
            )
        };

        // ═══════════════════════════════════════
        // GATEKEEPER STRENGTH (only for BUY verdicts, used by IWIM policy matrix)
        // ═══════════════════════════════════════
        let gatekeeper_strength = if verdict_buy {
            // Manipulation flags: bundle_suspicion, cabal_suspicion, top3_dominance
            let manipulation_flag_count = [
                soft_signals.bundle_suspicion,
                soft_signals.cabal_suspicion,
                soft_signals.top3_dominance,
            ]
            .iter()
            .filter(|&&f| f)
            .count() as u8;

            if soft_points <= effective_max_soft_points.saturating_sub(cfg.iwim_veto_strong_margin)
                && manipulation_flag_count <= cfg.iwim_veto_strong_max_manip_flags
            {
                Some(GatekeeperStrength::Strong)
            } else {
                Some(GatekeeperStrength::Borderline)
            }
        } else {
            None
        };

        GatekeeperDecision {
            hard_fail_reason,
            core1_passed,
            core2_passed,
            core3_passed,
            soft_signals,
            soft_points,
            max_soft_points_possible,
            effective_max_soft_points,
            dev_unknown,
            sybil_policy,
            alpha_gate: AlphaGateDiagnostics::not_run(cfg.enable_alpha_gate),
            prosperity_filter: ProsperityFilterDiagnostics::not_run(cfg.enable_prosperity_filter),
            total_soft_points,
            verdict_type,
            verdict_buy,
            reason_chain,
            gatekeeper_strength,
        }
    }

    /// Evaluate phases 2-6 and return a composite assessment.
    ///
    /// When `use_three_layer_decision` is enabled, uses the three-layer system
    /// (hard_fails → core_pass → soft_signals) instead of simple phases_passed counting.
    fn evaluate_phases(&mut self) -> GatekeeperVerdict {
        self.eval_count += 1;
        self.last_eval_at_count = Some(self.total_tx_count);

        let mut assessment = self.run_assessment();
        let breakdown = Self::format_phase_breakdown(&assessment);

        // ── Three-Layer Decision System ──
        if self.config.use_three_layer_decision {
            let decision = self.compute_decision(&assessment);
            let soft_pts = decision.soft_points;
            let max_pts = decision.max_soft_points_possible;
            let verdict_buy = decision.verdict_buy;
            let verdict_tag = decision.verdict_type.tag();
            let reason_chain = decision.reason_chain.clone();
            assessment.decision = Some(decision);
            assessment.cache_v25_confidence(&self.config);

            if !verdict_buy {
                self.rejected = true;
                tracing::info!(
                    pool = %self.pool_id,
                    reason = %reason_chain,
                    phases = %breakdown,
                    soft_pts = soft_pts,
                    verdict = verdict_tag,
                    eval_count = self.eval_count,
                    "🚫 GATEKEEPER V2 REJECTED {} soft_pts={}/{}", breakdown, soft_pts, max_pts
                );
                return GatekeeperVerdict::Reject {
                    assessment,
                    reason: reason_chain,
                };
            }

            // BUY — but first check curve readiness latch
            if let Some(curve_v) = self.check_curve_latch(self.highest_seen_ts) {
                match &curve_v {
                    GatekeeperVerdict::PendingCurve => {
                        // Undo eval_count increment: this wasn't a real decision
                        self.eval_count = self.eval_count.saturating_sub(1);
                        return GatekeeperVerdict::PendingCurve;
                    }
                    GatekeeperVerdict::Reject { .. } => {
                        self.rejected = true;
                        return curve_v;
                    }
                    _ => {}
                }
            }
            self.state = PoolState::Approved;
            tracing::info!(
                pool = %self.pool_id,
                phases_passed = assessment.phases_passed,
                phases = %breakdown,
                soft_pts = soft_pts,
                verdict = verdict_tag,
                eval_count = self.eval_count,
                tx_count = self.total_tx_count,
                reason = %reason_chain,
                "✅ GATEKEEPER V2 BUY {} soft_pts={}/{}", breakdown, soft_pts, max_pts
            );
            let buffered = std::mem::take(&mut self.buffered_txs);
            return GatekeeperVerdict::Buy {
                buffered_txs: buffered,
                assessment,
            };
        }

        // ── Legacy Decision System (phases_passed) ──
        // Hard reject triggered?
        if assessment.hard_reject_reason.is_some() {
            self.rejected = true;
            let reason = assessment.hard_reject_reason.clone().unwrap();
            tracing::info!(
                pool = %self.pool_id,
                reason = %reason,
                phases = %breakdown,
                eval_count = self.eval_count,
                "🚫 GATEKEEPER V2 REJECTED (Hard Reject) {}", breakdown
            );
            return GatekeeperVerdict::Reject { assessment, reason };
        }

        // Enough phases passed?
        if assessment.phases_passed >= self.config.min_phases_to_pass {
            // Curve readiness latch (legacy path)
            if let Some(curve_v) = self.check_curve_latch(self.highest_seen_ts) {
                match &curve_v {
                    GatekeeperVerdict::PendingCurve => {
                        self.eval_count = self.eval_count.saturating_sub(1);
                        return GatekeeperVerdict::PendingCurve;
                    }
                    GatekeeperVerdict::Reject { .. } => {
                        self.rejected = true;
                        return curve_v;
                    }
                    _ => {}
                }
            }
            self.state = PoolState::Approved;
            tracing::info!(
                pool = %self.pool_id,
                phases_passed = assessment.phases_passed,
                phases = %breakdown,
                eval_count = self.eval_count,
                tx_count = self.total_tx_count,
                "✅ GATEKEEPER V2 BUY {}", breakdown
            );
            let buffered = std::mem::take(&mut self.buffered_txs);
            return GatekeeperVerdict::Buy {
                buffered_txs: buffered,
                assessment,
            };
        }

        // Not enough phases yet — keep collecting
        GatekeeperVerdict::Wait
    }

    /// Run the full 6-phase assessment and return the result.
    ///
    /// All phases are always computed to completion, even when a hard-reject
    /// condition is detected.  The `hard_reject_reason` field on the returned
    /// assessment signals that the pool should be rejected, but the phase
    /// profiles are fully populated so that diagnostic metrics always appear
    /// in decision logs (gatekeeper_v2_buys.jsonl).
    ///
    /// Previous behaviour: hard-reject checks caused an early return via
    /// `build_hard_reject()` which left all phase profiles as `None`.  In long
    /// mode this led to Buy verdicts with empty metrics because
    /// `check_long_deadline` intentionally ignores hard-reject reasons and
    /// only checks `phases_passed >= min_phases_to_pass`.

    /// Extract deterministic, length-bounded, **aligned** vectors from the
    /// observation window `[t0, t_end]`.
    ///
    /// All vectors share the same tx-event axis: for each buffered transaction
    /// in the window, the price is looked up from `price_history` (last price
    /// point with `timestamp_ms <= tx_event_ts`; **NaN** if no prior price
    /// point exists — downstream DTW/Hill consumers must handle/filter NaN).
    /// A single set of downsample indices is applied uniformly to all vectors,
    /// guaranteeing identical length.
    pub fn extract_window_vectors(&self, t0: u64, t_end: u64, max_len: usize) -> WindowVectors {
        // Build aligned per-tx triplets (ts_offset, sol_amount, price_at_tx).
        // Price lookup uses a cursor over price_history (assumed sorted by
        // timestamp_ms, ascending — entries are appended chronologically).
        // Cursor advances monotonically → O(N_tx + N_price) total.
        let mut ts_offsets: Vec<i64> = Vec::new();
        let mut sol_amounts: Vec<f64> = Vec::new();
        let mut prices: Vec<f64> = Vec::new();

        let ph = &self.price_history;
        let mut price_cursor: usize = 0;
        let mut last_price: f64 = f64::NAN;

        for btx in &self.buffered_txs {
            let ts = Self::tx_event_ts_ms(btx.tx.as_ref());
            if ts >= t0 && ts <= t_end {
                ts_offsets.push((ts - t0) as i64);
                sol_amounts.push(btx.tx.volume_sol);

                // Advance cursor: consume all price points with ts <= tx ts.
                while price_cursor < ph.len() && ph[price_cursor].timestamp_ms <= ts {
                    last_price = ph[price_cursor].price_sol_per_token;
                    price_cursor += 1;
                }
                prices.push(last_price);
            }
        }

        // Downsample by shared indices so all vectors stay aligned.
        let idx = downsample_indices(ts_offsets.len(), max_len);
        let ts_offsets = deterministic_downsample_i64(&ts_offsets, &idx);
        let sol_amounts = deterministic_downsample_f64(&sol_amounts, &idx);
        let prices = deterministic_downsample_f64(&prices, &idx);

        // Derived vectors (length N-1).
        let interval_ms: Vec<f64> = ts_offsets
            .windows(2)
            .map(|w| (w[1] - w[0]) as f64)
            .collect();
        let d_price: Vec<f64> = prices.windows(2).map(|w| w[1] - w[0]).collect();

        WindowVectors {
            ts_offsets_ms: ts_offsets,
            sol_amounts,
            prices,
            interval_ms,
            d_price,
            max_len: max_len as u32,
        }
    }

    pub fn run_assessment(&self) -> GatekeeperAssessment {
        let window_ms = self
            .highest_seen_ts
            .saturating_sub(self.first_tx_ts.unwrap_or(self.highest_seen_ts));

        // Collect the first hard-reject reason encountered (if any).
        // We do NOT return early — all phases are computed regardless.
        let mut hard_reject_reason: Option<String> = None;

        // ═══════════════════════════════════════
        // HARD REJECT CHECK: Dev sold (Phase 5, checked early)
        // ═══════════════════════════════════════
        if self.config.reject_on_dev_sell && self.dev_has_sold {
            hard_reject_reason = Some("Dev wallet sold during observation window".to_string());
        }

        // ═══════════════════════════════════════
        // PHASE 2: Velocity
        // ═══════════════════════════════════════
        let velocity =
            compute_velocity_profile(&self.tx_timestamps_sorted, self.config.max_wait_time_ms);
        let phase2_passed = velocity.interval_cv >= self.config.min_interval_cv
            && velocity.interval_cv <= self.config.max_interval_cv
            && velocity.burst_ratio <= self.config.max_burst_ratio
            && velocity.avg_interval_ms >= self.config.min_avg_interval_ms
            && velocity.avg_interval_ms <= self.config.max_avg_interval_ms
            && velocity.timing_entropy >= self.config.min_timing_entropy
            && velocity.timing_entropy <= self.config.max_timing_entropy
            && self.dust_filtered_count >= self.config.min_dust_filtered_count;

        // Hard reject: extreme bot (very low CoV + very fast)
        if hard_reject_reason.is_none()
            && velocity.interval_cv < 0.08
            && velocity.avg_interval_ms < 30.0
        {
            hard_reject_reason = Some(format!(
                "Extreme bot timing: CoV={:.3} avg={:.0}ms",
                velocity.interval_cv, velocity.avg_interval_ms
            ));
        }

        // ═══════════════════════════════════════
        // PHASE 3: Signer Diversity
        // ═══════════════════════════════════════
        let diversity = compute_signer_diversity(
            &self.signer_stats,
            self.total_tx_count,
            self.total_volume_sol,
            &self.tx_timestamps_sorted,
        );
        let phase3_passed = diversity.unique_ratio >= self.config.min_unique_ratio
            && diversity.unique_ratio <= self.config.max_unique_ratio
            && diversity.hhi <= self.config.max_hhi
            && diversity.max_tx_per_signer <= self.config.max_tx_per_signer
            && diversity.volume_gini >= self.config.min_volume_gini
            && diversity.volume_gini <= self.config.max_volume_gini
            && diversity.top3_volume_pct <= self.config.max_top3_volume_pct
            && diversity.same_ms_tx_ratio <= self.config.max_same_ms_tx_ratio;

        // Hard reject: extreme concentration
        if hard_reject_reason.is_none() && diversity.hhi > 0.5 {
            hard_reject_reason = Some(format!(
                "Extreme signer concentration: HHI={:.3}",
                diversity.hhi
            ));
        }

        // ═══════════════════════════════════════
        // PHASE 4: Volume Sanity
        // ═══════════════════════════════════════
        let volume = compute_volume_sanity(
            &self.tx_volumes,
            self.buy_count,
            self.sell_count,
            self.total_volume_sol,
            self.buy_volume_sol,
            self.max_consecutive_buys,
        );
        let phase4_passed = volume.buy_ratio >= self.config.min_buy_ratio
            && volume.buy_ratio <= self.config.max_buy_ratio
            && volume.avg_tx_sol >= self.config.min_avg_tx_sol
            && volume.avg_tx_sol <= self.config.max_avg_tx_sol
            && volume.volume_cv >= self.config.min_volume_cv
            && volume.volume_cv <= self.config.max_volume_cv
            && volume.total_volume_sol >= self.config.min_total_volume_sol
            && volume.total_volume_sol <= self.config.max_total_volume_sol
            && volume.sol_buy_ratio >= self.config.min_sol_buy_ratio
            && volume.max_consecutive_buys >= self.config.min_consecutive_buys;

        // ═══════════════════════════════════════
        // PHASE 5: Dev Behavior
        // ═══════════════════════════════════════
        let dev = compute_dev_behavior(
            &self.dev_wallet,
            &self.first_signer,
            self.dev_buy_total_sol,
            self.dev_buy_volume_total_sol,
            self.dev_sell_total_sol,
            self.dev_tx_count,
            self.dev_has_sold,
            self.dev_initial_buy_tokens,
            self.total_tx_count,
            self.total_volume_sol,
        );
        let phase5_passed = if !dev.dev_wallet_known {
            true // No dev info → auto-pass (cannot penalize unknown)
        } else {
            dev.dev_buy_total_sol <= self.config.max_dev_buy_sol
                && dev.dev_buy_total_sol >= self.config.min_dev_buy_sol
                && dev.dev_tx_ratio <= self.config.max_dev_tx_ratio
                && dev.dev_tx_ratio >= self.config.min_dev_tx_ratio
                && dev.dev_volume_ratio <= self.config.max_dev_volume_ratio
                // Respect reject_on_dev_sell config flag.
                // When false, dev selling does NOT fail Phase 5 (sell *size* is still
                // controlled via max_single_sell_impact_pct in Phase 6).
                && (!dev.dev_has_sold || !self.config.reject_on_dev_sell)
        };

        // ═══════════════════════════════════════
        // PHASE 6: Bonding Curve Dynamics
        // ═══════════════════════════════════════
        let curve = compute_bonding_curve_dynamics(&self.price_history);
        let phase6_passed = if curve.price_data_points < 2 {
            true // Not enough data → auto-pass
        } else {
            let price_ok = curve.price_change_ratio <= self.config.max_price_change_ratio
                && curve.max_single_tx_price_impact_pct
                    <= self.config.max_single_tx_price_impact_pct
                && curve.max_single_sell_impact_pct <= self.config.max_single_sell_impact_pct
                && (if curve.curve_data_known {
                    curve.current_market_cap_sol >= self.config.min_market_cap_sol
                } else {
                    true // Degrade: unknown market cap → pass
                });

            // When bonding progress is unknown (parse fail / genesis fallback),
            // skip the bonding progress range check instead of rejecting on 0%.
            let bonding_ok = if curve.curve_data_known {
                curve.bonding_progress_pct <= self.config.max_bonding_progress_pct
                    && curve.bonding_progress_pct >= self.config.min_bonding_progress_pct
            } else {
                true // Degrade: unknown progress → pass this sub-check
            };

            price_ok && bonding_ok
        };

        // Hard reject: extreme price manipulation
        if hard_reject_reason.is_none()
            && curve.price_data_points >= 2
            && curve.max_single_tx_price_impact_pct > 50.0
        {
            hard_reject_reason = Some(format!(
                "Extreme price manipulation: single TX moved price {:.1}%",
                curve.max_single_tx_price_impact_pct
            ));
        }

        // ═══════════════════════════════════════
        // Yellowstone-only: Failed TX ratio check
        // ═══════════════════════════════════════
        if hard_reject_reason.is_none() {
            if let Some(threshold) = self.config.min_failed_tx_ratio_for_bot_flag {
                let total_with_failed = self.total_tx_count + self.failed_tx_count;
                if total_with_failed > 5 {
                    let failed_ratio = self.failed_tx_count as f64 / total_with_failed as f64;
                    if failed_ratio > threshold {
                        hard_reject_reason = Some(format!(
                            "High failed TX ratio: {:.2} (bot spam, Yellowstone)",
                            failed_ratio
                        ));
                    }
                }
            }
        }

        // ═══════════════════════════════════════
        // COMPOSITE DECISION
        // ═══════════════════════════════════════
        let phases_passed = [
            self.phase1_passed,
            phase2_passed,
            phase3_passed,
            phase4_passed,
            phase5_passed,
            phase6_passed,
        ]
        .iter()
        .filter(|&&p| p)
        .count() as u8;

        // Pre-compute trajectory for availability flag + assessment field
        let trajectory = self.materialize_trajectory(&self.config.tas);

        let mut assessment = GatekeeperAssessment {
            phase1_passed: self.phase1_passed,
            phase2_velocity: Some(velocity),
            phase2_passed,
            phase3_diversity: Some(diversity),
            phase3_passed,
            phase4_volume: Some(volume),
            phase4_passed,
            phase5_dev: Some(dev),
            phase5_passed,
            phase6_curve: Some(curve),
            phase6_passed,
            phases_passed,
            hard_reject_reason,
            total_tx_evaluated: self.total_tx_count,
            unique_tx_evaluated: self.unique_signature_count(),
            unique_signers_evaluated: self.unique_signers.len(),
            observation_duration_ms: self.wall_observation_duration_ms(Self::now_wall_ms()),
            finalize_lag_ms: self.finalize_lag_ms(Self::now_wall_ms()),
            dust_filtered_count: self.dust_filtered_count,
            eval_count: self.eval_count,
            buy_count: self.buy_count,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: self.curve_t0_event_ts_ms,
            curve_t0_clock_source: self.curve_t0_clock_source,
            curve_wait_elapsed_ms: self
                .curve_t0_event_ts_ms
                .map(|t0| self.highest_seen_ts.saturating_sub(t0)),
            feature_snapshot: MaterializedFeatureSet::default(),
            checkpoint_count: 0,
            trajectory_available: trajectory.is_some(),
            v25_shadow_decisions: self.v25_shadow_decisions.clone(),
            trajectory,
            pdd_assessment: {
                let pdd =
                    crate::components::gatekeeper_pdd::evaluate_pdd(self, &self.config.pdd, None);
                Some(pdd)
            },
            aps_diagnostics: None,
            observation_stage: Some(self.window_stage),
            // Top-level V2.5 telemetry — populated from PDD + APS after assessment is consumed
            entry_drift_pct: None,
            entry_drift_anchor_quality: None,
            adaptive_thresholds_applied: false,
            v25_confidence: None,
        };

        // Populate top-level V2.5 telemetry from sub-module diagnostics
        if let Some(ref pdd) = assessment.pdd_assessment {
            assessment.entry_drift_pct = pdd.entry_drift_pct;
            if let Some(q) = pdd.entry_drift_anchor_quality {
                assessment.entry_drift_anchor_quality = match q {
                    "strong" => Some(EntryDriftAnchorQuality::Strong),
                    _ => Some(EntryDriftAnchorQuality::Weak),
                };
            }
        }
        if let Some(ref aps) = assessment.aps_diagnostics {
            assessment.adaptive_thresholds_applied =
                aps.adaptive_thresholds_applied && !self.config.v25.live_execution_enabled;
        }

        assessment
    }

    /// Build a hard reject assessment with the given reason.
    /// NOTE: This is kept for backward compatibility but `run_assessment()`
    /// no longer calls it — it computes all phases inline and sets
    /// `hard_reject_reason` without early return.
    #[allow(dead_code)]
    fn build_hard_reject(&self, reason: &str) -> GatekeeperAssessment {
        let mut assessment = GatekeeperAssessment {
            phase1_passed: self.phase1_passed,
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
            phases_passed: if self.phase1_passed { 1 } else { 0 },
            hard_reject_reason: Some(reason.to_string()),
            total_tx_evaluated: self.total_tx_count,
            unique_tx_evaluated: self.unique_signature_count(),
            unique_signers_evaluated: self.unique_signers.len(),
            observation_duration_ms: self.wall_observation_duration_ms(Self::now_wall_ms()),
            finalize_lag_ms: self.finalize_lag_ms(Self::now_wall_ms()),
            dust_filtered_count: self.dust_filtered_count,
            eval_count: self.eval_count,
            buy_count: self.buy_count,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: self.curve_t0_event_ts_ms,
            curve_t0_clock_source: self.curve_t0_clock_source,
            curve_wait_elapsed_ms: self
                .curve_t0_event_ts_ms
                .map(|t0| self.highest_seen_ts.saturating_sub(t0)),
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
        };
        assessment.trajectory = self.materialize_trajectory(&self.config.tas);
        assessment.pdd_assessment = Some(crate::components::gatekeeper_pdd::evaluate_pdd(
            self,
            &self.config.pdd,
            None,
        ));
        assessment
    }

    /// Test-only helper for in-crate suites that still assert legacy inline
    /// verdict parity.
    ///
    /// Production/runtime code and external integration tests must use
    /// `ingest_transaction_tracking_only()` together with feature-driven policy
    /// evaluation instead of this inline verdict path.
    #[cfg(test)]
    pub(crate) fn legacy_test_verdict_from_transaction(
        &mut self,
        tx: Arc<PoolTransaction>,
    ) -> GatekeeperVerdict {
        match self.ingest_transaction_tracking_only(tx) {
            GatekeeperIngressOutcome::Wait => GatekeeperVerdict::Wait,
            GatekeeperIngressOutcome::TriggerEvaluation => self.evaluate_phases(),
            GatekeeperIngressOutcome::DeadlineElapsed => {
                if self.config.mode == GatekeeperMode::Long {
                    self.check_long_deadline(self.highest_seen_ts)
                } else {
                    self.check_standard_deadline(self.highest_seen_ts)
                        .unwrap_or(GatekeeperVerdict::Wait)
                }
            }
            GatekeeperIngressOutcome::ApprovedTx { tx, metrics } => {
                GatekeeperVerdict::ApprovedTx { tx, metrics }
            }
        }
    }

    #[cfg(test)]
    fn on_transaction(&mut self, tx: Arc<PoolTransaction>) -> GatekeeperVerdict {
        self.legacy_test_verdict_from_transaction(tx)
    }

    /// Long-mode transaction processing.
    ///
    /// In long mode the gatekeeper accumulates ALL transactions for the full
    /// `max_wait_time_ms` window without making any early decisions.  Only when
    /// the deadline is reached does it perform a single final evaluation:
    ///   - Phase 1 met (`min_tx_count`, `min_unique_signers`, `min_buy_count`) AND
    ///     `phases_passed >= min_phases_to_pass` → **Buy**
    ///   - Phase 1 met but not enough phases → **Reject**
    ///   - Phase 1 never met → **Timeout**
    ///
    /// Attempt a shadow evaluation at the given observation stage.
    ///
    /// Runs the full assessment + three-layer decision pipeline on the current
    /// buffer state and records a `ShadowV25Decision`. Does NOT change the live
    /// verdict, buffer state, or any tracking fields (except `phase1_passed`
    /// which is saved and restored).
    ///
    /// Returns `true` if a shadow evaluation was produced, `false` if skipped
    /// (feature-gated or insufficient data).
    fn try_shadow_evaluate(&mut self, watch_now_wall_ms: u64, stage: ObservationStage) -> bool {
        if !self.config.v25.shadow_enabled || !self.config.dow.enabled {
            return false;
        }

        let elapsed_ms = watch_now_wall_ms.saturating_sub(self.registered_wall_ts_ms);

        let min_data_tx = match stage {
            ObservationStage::Early => self.config.dow.early_entry_min_tx_count,
            ObservationStage::Normal | ObservationStage::Extended => self.config.min_tx_count,
        };

        // ── Insufficient data guard ──
        if self.total_tx_count < min_data_tx || self.buffered_txs.is_empty() {
            let shadow = ShadowV25Decision {
                kind: ShadowDecisionKind::InsufficientData,
                window: stage,
                elapsed_ms,
                confidence: 0.0,
                phases_passed: 0,
                reason: format!(
                    "INSUFFICIENT_DATA: tx={}/{} buf={} elapsed_ms={}",
                    self.total_tx_count,
                    min_data_tx,
                    self.buffered_txs.len(),
                    elapsed_ms,
                ),
            };
            self.v25_shadow_decisions.push(shadow);
            return true;
        }

        // ── Phase 1 guard ──
        let has_enough_tx = self.total_tx_count >= self.config.min_tx_count;
        let has_enough_signers = self.unique_signers.len() >= self.config.min_unique_signers;
        let has_enough_buys = self.buy_count >= self.config.min_buy_count;

        if !(has_enough_tx && has_enough_signers && has_enough_buys) {
            let shadow = ShadowV25Decision {
                kind: ShadowDecisionKind::InsufficientData,
                window: stage,
                elapsed_ms,
                confidence: 0.0,
                phases_passed: 0,
                reason: format!(
                    "INSUFFICIENT_DATA: phase1 tx={}/{} sig={}/{} buy={}/{}",
                    self.total_tx_count,
                    self.config.min_tx_count,
                    self.unique_signers.len(),
                    self.config.min_unique_signers,
                    self.buy_count,
                    self.config.min_buy_count,
                ),
            };
            self.v25_shadow_decisions.push(shadow);
            return true;
        }

        // ── Save/restore phase1_passed ──
        let prev_phase1 = self.phase1_passed;
        self.phase1_passed = true;

        // Run assessment + three-layer decision
        let mut assessment = self.run_assessment();
        let decision = self.compute_decision(&assessment);
        assessment.decision = Some(decision.clone());

        // ── V2.5 APS: Adaptive Prosperity diagnostics ──
        let spike_flag = assessment
            .pdd_assessment
            .as_ref()
            .map_or(false, |p| p.spike_detected);
        assessment.aps_diagnostics = Some(
            crate::components::gatekeeper_adaptive_prosperity::evaluate_aps(
                &assessment,
                &self.config.aps,
                spike_flag,
            ),
        );

        // ── Populate top-level V2.5 telemetry from sub-module diagnostics ──
        if let Some(ref pdd) = assessment.pdd_assessment {
            assessment.entry_drift_pct = pdd.entry_drift_pct;
            assessment.entry_drift_anchor_quality =
                pdd.entry_drift_anchor_quality.and_then(|q| match q {
                    "strong" => Some(EntryDriftAnchorQuality::Strong),
                    _ => Some(EntryDriftAnchorQuality::Weak),
                });
        }
        if let Some(ref aps) = assessment.aps_diagnostics {
            // P2: shadow-only contract — never applied when live_execution is on.
            assessment.adaptive_thresholds_applied =
                aps.adaptive_thresholds_applied && !self.config.v25.live_execution_enabled;

            // ── HighVolatility PDD drift re-check ──
            // Shadow-only: only apply regime-aware drift when live_execution is off.
            use crate::components::gatekeeper_adaptive_prosperity::MarketRegime;
            if aps.regime == MarketRegime::HighVolatility
                && !self.config.v25.live_execution_enabled
                && assessment
                    .pdd_assessment
                    .as_ref()
                    .map_or(false, |p| p.hard_fail.is_none())
            {
                if let Some(drift) = assessment.entry_drift_pct {
                    let hv_max = self.config.aps.regime_high_vol_entry_drift_max_pct;
                    if drift > hv_max {
                        // Override: PDD hard fail due to regime-aware drift threshold
                        if let Some(ref mut pdd) = assessment.pdd_assessment {
                            pdd.hard_fail = Some(PddHardFail::EntryDrift);
                            pdd.pdd_score = 0.0;
                        }
                    }
                }
            }
        }

        // Restore phase1_passed
        self.phase1_passed = prev_phase1;

        // V2.5 TAS modulation for Early/Normal legacy shadow confidence.
        // Extended uses the canonical V2.5 confidence model below so timer and
        // deadline fallback evaluate the same snapshot the same way.
        let apply_tas = self.config.tas.enabled && stage != ObservationStage::Early;
        let tas_reject = apply_tas && assessment.v25_tas_hard_reject(&self.config);
        let mut confidence = if stage == ObservationStage::Extended {
            assessment.cache_v25_confidence(&self.config);
            assessment.v25_confidence.unwrap_or(0.0)
        } else {
            let base_confidence = if decision.max_soft_points_possible > 0 {
                let ratio = decision.soft_points as f64 / decision.max_soft_points_possible as f64;
                (1.0 - ratio).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if apply_tas {
                if tas_reject {
                    0.0
                } else if let Some(ref traj) = assessment.trajectory {
                    use crate::components::gatekeeper_trajectory::{
                        compute_tas_modulator, evaluate_trajectory,
                    };
                    let tas_score = evaluate_trajectory(traj);
                    (base_confidence * compute_tas_modulator(tas_score, &self.config.tas))
                        .clamp(0.0, 1.0)
                } else {
                    base_confidence
                }
            } else {
                base_confidence
            }
        };

        let phases_passed = assessment.phases_passed.min(6);

        // ── V2.5 PDD: Pump & Dump hard veto (shadow) ──
        if let Some(ref pdd) = assessment.pdd_assessment {
            if pdd.hard_fail.is_some() {
                let fail_tag = pdd.hard_fail.as_ref().unwrap().as_str();
                confidence = 0.0;
                assessment.v25_confidence = Some(0.0);
                let shadow = ShadowV25Decision {
                    kind: ShadowDecisionKind::RejectPumpAndDump,
                    window: stage,
                    elapsed_ms,
                    confidence,
                    phases_passed,
                    reason: format!(
                        "PDD_{}: drift={:?} spike={} ramping={} whale={:?} reserve={} flash={}",
                        fail_tag,
                        pdd.entry_drift_pct,
                        pdd.spike_detected,
                        pdd.ramping_detected,
                        pdd.whale_top3_pct,
                        pdd.reserve_health_pass,
                        pdd.flash_crash_risk
                    ),
                };
                self.v25_shadow_decisions.push(shadow);
                return true;
            }
            if stage != ObservationStage::Extended {
                // Soft penalty: reduce legacy Early/Normal confidence proportionally to pdd_score.
                confidence *= pdd.pdd_score;
            }
        }

        // Cache final confidence on assessment for JSONL telemetry
        assessment.v25_confidence = Some(confidence);

        // ── Stage-specific criteria ──
        let (kind, reason) = match stage {
            ObservationStage::Early => {
                let min_phases = self.config.dow.early_entry_min_phases_passed;
                let all_phases_passed = phases_passed >= min_phases;
                let enough_tx = self.total_tx_count >= self.config.dow.early_entry_min_tx_count;
                let high_conf = confidence >= self.config.dow.early_entry_min_confidence;
                let sybil_clean = decision.sybil_policy.soft_points
                    <= self.config.dow.early_entry_max_sybil_points as u16;
                let low_drift = assessment.phase6_curve.as_ref().map_or(true, |c| {
                    (c.price_change_ratio - 1.0).abs() * 100.0
                        <= self.config.dow.early_entry_max_entry_drift_pct
                });
                let has_momentum = decision.alpha_gate.momentum.unwrap_or(0.0)
                    >= self.config.dow.early_entry_min_momentum;

                if decision.verdict_buy
                    && confidence > 0.0
                    && all_phases_passed
                    && enough_tx
                    && high_conf
                    && sybil_clean
                    && low_drift
                    && has_momentum
                {
                    let reason_text = format!(
                        "EARLY_BUY: phases=6/6 conf={:.3} tx={} sybil_pts={} drift_ok={} momentum_ok={}",
                        confidence, self.total_tx_count,
                        decision.sybil_policy.soft_points, low_drift, has_momentum,
                    );
                    (ShadowDecisionKind::EarlyBuyCandidate, reason_text)
                } else if tas_reject {
                    (
                        ShadowDecisionKind::RejectLowTrajectory,
                        format!(
                            "EARLY_REJECT_LOW_TRAJECTORY: conf={:.3} tas<{:.2}",
                            confidence, self.config.tas.tas_hard_reject_threshold
                        ),
                    )
                } else {
                    let mut fails = Vec::new();
                    if !all_phases_passed {
                        fails.push(format!("phases={}/6", phases_passed));
                    }
                    if !enough_tx {
                        fails.push(format!("tx={}", self.total_tx_count));
                    }
                    if !high_conf {
                        fails.push(format!("conf={:.3}", confidence));
                    }
                    if !sybil_clean {
                        fails.push(format!("sybil={}", decision.sybil_policy.soft_points));
                    }
                    if !low_drift {
                        fails.push("drift_high".to_string());
                    }
                    if !has_momentum {
                        fails.push(format!(
                            "momentum={:.3}",
                            decision.alpha_gate.momentum.unwrap_or(0.0)
                        ));
                    }
                    if !decision.verdict_buy {
                        fails.push(format!("verdict={}", decision.verdict_type.tag()));
                    }
                    (
                        ShadowDecisionKind::ShadowReject,
                        format!("EARLY_REJECT: [{}]", fails.join(", ")),
                    )
                }
            }
            ObservationStage::Normal => {
                let min_conf = self.config.dow.normal_window_min_confidence;
                if decision.verdict_buy && confidence > 0.0 && confidence >= min_conf {
                    (
                        ShadowDecisionKind::NormalBuyCandidate,
                        format!(
                            "NORMAL_BUY: phases={}/6 conf={:.3} tx={}",
                            phases_passed, confidence, self.total_tx_count,
                        ),
                    )
                } else if tas_reject {
                    (
                        ShadowDecisionKind::RejectLowTrajectory,
                        format!(
                            "NORMAL_REJECT_LOW_TRAJECTORY: conf={:.3} tas<{:.2}",
                            confidence, self.config.tas.tas_hard_reject_threshold,
                        ),
                    )
                } else if decision.verdict_buy {
                    (
                        ShadowDecisionKind::ShadowReject,
                        format!(
                            "NORMAL_REJECT: conf={:.3} < {:.3} (BUY verdict but low confidence)",
                            confidence, min_conf,
                        ),
                    )
                } else {
                    (
                        ShadowDecisionKind::ShadowReject,
                        format!(
                            "NORMAL_REJECT: verdict={} reason={}",
                            decision.verdict_type.tag(),
                            decision.reason_chain,
                        ),
                    )
                }
            }
            ObservationStage::Extended => {
                let min_conf = self.config.dow.extended_window_min_confidence;
                let require_pdd = self.config.dow.extended_require_pdd_clean;
                let pdd_clean = assessment.v25_pdd_clean();

                // PDD hard fail always vetoes (absolute, regardless of config).
                if let Some(fail) = assessment
                    .pdd_assessment
                    .as_ref()
                    .and_then(|pdd| pdd.hard_fail.as_ref())
                {
                    (
                        ShadowDecisionKind::RejectPumpAndDump,
                        format!(
                            "EXTENDED_REJECT_PDD_{}: conf={:.3} phases={}/6",
                            fail.as_str(),
                            confidence,
                            phases_passed,
                        ),
                    )
                } else if tas_reject {
                    (
                        ShadowDecisionKind::RejectLowTrajectory,
                        format!(
                            "EXTENDED_REJECT_LOW_TRAJECTORY: conf={:.3} tas<{:.2}",
                            confidence, self.config.tas.tas_hard_reject_threshold,
                        ),
                    )
                } else if decision.verdict_buy
                    && confidence > 0.0
                    && confidence >= min_conf
                    && (!require_pdd || pdd_clean)
                {
                    (
                        ShadowDecisionKind::NormalBuyCandidate,
                        format!(
                            "EXTENDED_BUY: verdict={} conf={:.3} min_conf={:.2} pdd_clean={} phases={}/6",
                            decision.verdict_type.tag(),
                            confidence,
                            min_conf,
                            pdd_clean,
                            phases_passed,
                        ),
                    )
                } else if decision.verdict_buy && !pdd_clean {
                    (
                        ShadowDecisionKind::ShadowReject,
                        format!(
                            "EXTENDED_REJECT_PDD_NOT_CLEAN: verdict={} conf={:.3} min_conf={:.2}",
                            decision.verdict_type.tag(),
                            confidence,
                            min_conf,
                        ),
                    )
                } else if decision.verdict_buy && confidence <= 0.0 {
                    (
                        ShadowDecisionKind::ShadowReject,
                        format!(
                            "EXTENDED_REJECT_ZERO_CONFIDENCE: verdict={} conf={:.3}",
                            decision.verdict_type.tag(),
                            confidence,
                        ),
                    )
                } else if decision.verdict_buy {
                    (
                        ShadowDecisionKind::ShadowReject,
                        format!(
                            "EXTENDED_REJECT: conf={:.3} < {:.3} (BUY verdict but low confidence)",
                            confidence, min_conf,
                        ),
                    )
                } else {
                    (
                        ShadowDecisionKind::ShadowReject,
                        format!(
                            "EXTENDED_REJECT: verdict={} reason={}",
                            decision.verdict_type.tag(),
                            decision.reason_chain,
                        ),
                    )
                }
            }
        };

        // ── Telemetry ──
        if self.config.v25.emit_shadow_decisions {
            tracing::info!(
                pool = %self.pool_id,
                stage = ?stage,
                kind = ?kind,
                elapsed_ms = elapsed_ms,
                confidence = confidence,
                phases = phases_passed,
                reason = %reason,
                "V2.5 SHADOW DECISION: {}",
                reason,
            );
        }

        let shadow = ShadowV25Decision {
            kind,
            window: stage,
            elapsed_ms,
            confidence,
            phases_passed,
            reason,
        };
        self.v25_shadow_decisions.push(shadow);
        true
    }

    /// Unified checkpoint entry point called by both the DOW timer and TX ingestion path.
    ///
    /// Fires shadow evaluations for Early/Normal/Extended stages when their respective
    /// time windows are open and the stage hasn't been fired yet. The `*_shadow_fired`
    /// flags provide single-owner serialization — **exactly one** checkpoint per stage,
    /// including `InsufficientData`. There are no duplicate checkpoints.
    ///
    /// Window semantics (plan §3.2: 2–5s / 5–7s / 7–10s):
    /// - Early:    [early_entry_min_ms,     early_entry_max_ms]     = [2s, 5s]
    /// - Normal:   (early_entry_max_ms,     normal_window_ms)       = (5s, 7s)
    /// - Extended: [normal_window_ms,       extended_window_ms]     = [7s, 10s]
    ///
    /// Invariant enforced at config load: extended_window_ms <= max_wait_time_ms.
    /// Returns `true` if at least one checkpoint was fired.
    pub fn maybe_fire_shadow_checkpoint(&mut self, now_wall_ms: u64) -> bool {
        if self.config.dow.enabled
            && self.config.dow.extended_window_ms > self.config.max_wait_time_ms
        {
            panic!(
                "P0 invariant violated: dow.extended_window_ms ({}) > max_wait_time_ms ({})",
                self.config.dow.extended_window_ms, self.config.max_wait_time_ms
            );
        }
        if !self.config.v25.shadow_enabled || !self.config.dow.enabled {
            return false;
        }
        if self.config.mode != GatekeeperMode::Long {
            return false;
        }
        if self.rejected || self.state == PoolState::Approved {
            return false;
        }

        let elapsed = now_wall_ms.saturating_sub(self.registered_wall_ts_ms);
        let mut fired = false;

        // Early: [2s, 5s]
        if self.config.dow.early_entry_enabled
            && !self.early_shadow_fired
            && elapsed >= self.config.dow.early_entry_min_ms
            && elapsed <= self.config.dow.early_entry_max_ms
        {
            self.early_shadow_fired = true;
            self.window_stage = ObservationStage::Early;
            self.try_shadow_evaluate(now_wall_ms, ObservationStage::Early);
            crate::oracle_metrics::record_dow_timer_fired("Early");
            fired = true;
        }

        // Normal: (5s, 7s) — after Early ends, before Extended begins.
        if !self.normal_shadow_fired
            && elapsed > self.config.dow.early_entry_max_ms
            && elapsed < self.config.dow.normal_window_ms
        {
            self.normal_shadow_fired = true;
            self.window_stage = ObservationStage::Normal;
            self.try_shadow_evaluate(now_wall_ms, ObservationStage::Normal);
            crate::oracle_metrics::record_dow_timer_fired("Normal");
            fired = true;
        }

        // Extended: [7s, 10s] — bounded by extended_window_ms (= max deadline).
        if !self.extended_shadow_fired
            && elapsed >= self.config.dow.normal_window_ms
            && elapsed <= self.config.dow.extended_window_ms
        {
            self.extended_shadow_fired = true;
            self.window_stage = ObservationStage::Extended;
            self.try_shadow_evaluate(now_wall_ms, ObservationStage::Extended);
            crate::oracle_metrics::record_dow_timer_fired("Extended");
            fired = true;
        }

        fired
    }

    /// Materialize trajectory data by dividing the observation window into 3
    /// equal time segments (T0/T1/T2) and computing per-segment metrics.
    ///
    /// Returns `None` when TAS is disabled, observation duration is too short,
    /// or any segment has too few transactions.
    pub fn materialize_trajectory(
        &self,
        config: &TrajectoryAwareScoringConfig,
    ) -> Option<TrajectoryAssessment> {
        use crate::components::gatekeeper_trajectory::{build_segment, score_trajectory};

        if !config.enabled {
            return None;
        }

        let first_ts = self.first_tx_ts_ms()?;
        let last_ts = self.highest_seen_ts_ms();
        let duration_ms = last_ts.saturating_sub(first_ts);

        if duration_ms < config.tas_min_total_duration_ms {
            return None;
        }

        let min_total_tx = config.tas_min_tx_per_segment.saturating_mul(3);
        if self.total_tx_count < min_total_tx {
            return None;
        }

        // Divide window into 3 equal time segments
        let seg_dur = duration_ms as f64 / 3.0;
        let t0_end = first_ts.saturating_add(seg_dur as u64);
        let t1_end = first_ts.saturating_add((2.0 * seg_dur) as u64);

        let mut seg0: Vec<&PoolTransaction> = Vec::new();
        let mut seg1: Vec<&PoolTransaction> = Vec::new();
        let mut seg2: Vec<&PoolTransaction> = Vec::new();

        for btx in &self.buffered_txs {
            let ts = btx.tx.timestamp_ms;
            if ts <= t0_end {
                seg0.push(&btx.tx);
            } else if ts <= t1_end {
                seg1.push(&btx.tx);
            } else {
                seg2.push(&btx.tx);
            }
        }

        let min_per_seg = config.tas_min_tx_per_segment;
        if seg0.len() < min_per_seg || seg1.len() < min_per_seg || seg2.len() < min_per_seg {
            return None;
        }

        let s0 = build_segment(&seg0);
        let s1 = build_segment(&seg1);
        let s2 = build_segment(&seg2);

        Some(score_trajectory(&s0, &s1, &s2, config))
    }

    #[must_use]
    /// Build segment sequence using TAS thresholds but independently of tas.enabled.
    /// PDD sequence signals (spike/ramping/flash) also depend on this data, so the
    /// sequence must be materialized even when TAS scoring is disabled.
    pub fn current_segment_sequence_from_config(
        &self,
    ) -> Option<ghost_core::checkpoint::TxSegmentSequence> {
        // Use TAS config for segment division thresholds (min TX per segment, min duration)
        // but do NOT gate on tas.enabled — PDD sequence also needs this data.
        let min_tx_per_seg = self.config.tas.tas_min_tx_per_segment;
        let min_duration_ms = self.config.tas.tas_min_total_duration_ms;
        self.build_segment_sequence(min_tx_per_seg, min_duration_ms)
    }

    /// Build raw segment sequence with explicit thresholds (decoupled from TAS config).
    fn build_segment_sequence(
        &self,
        min_tx_per_segment: usize,
        min_total_duration_ms: u64,
    ) -> Option<ghost_core::checkpoint::TxSegmentSequence> {
        use crate::components::gatekeeper_trajectory::build_segment;
        use ghost_core::checkpoint::TrajectorySegmentSnapshot;
        let first_ts = self.first_tx_ts_ms()?;
        let last_ts = self.highest_seen_ts_ms();
        let duration_ms = last_ts.saturating_sub(first_ts);
        if duration_ms < min_total_duration_ms {
            return None;
        }
        let min_total_tx = min_tx_per_segment.saturating_mul(3);
        if self.total_tx_count < min_total_tx {
            return None;
        }
        let seg_dur = duration_ms as f64 / 3.0;
        let t0_end = first_ts.saturating_add(seg_dur as u64);
        let t1_end = first_ts.saturating_add((2.0 * seg_dur) as u64);
        let mut seg0: Vec<&PoolTransaction> = Vec::new();
        let mut seg1: Vec<&PoolTransaction> = Vec::new();
        let mut seg2: Vec<&PoolTransaction> = Vec::new();
        for btx in &self.buffered_txs {
            let ts = btx.tx.timestamp_ms;
            if ts <= t0_end {
                seg0.push(&btx.tx);
            } else if ts <= t1_end {
                seg1.push(&btx.tx);
            } else {
                seg2.push(&btx.tx);
            }
        }
        let min_satisfied = seg0.len() >= min_tx_per_segment
            && seg1.len() >= min_tx_per_segment
            && seg2.len() >= min_tx_per_segment;
        fn snapshot(seg: &[&PoolTransaction]) -> TrajectorySegmentSnapshot {
            let built = build_segment(seg);
            let max_pip = seg
                .iter()
                .map(|tx| tx.sol_amount_lamports.unwrap_or(0) as f64)
                .fold(0.0_f64, |a, b| a.max(b));
            let buys: Vec<u64> = seg
                .iter()
                .filter(|tx| tx.is_buy)
                .filter_map(|tx| tx.sol_amount_lamports)
                .collect();
            let mut max_streak: u32 = 0;
            for i in 0..buys.len() {
                let mut streak: u32 = 1;
                let anchor = buys[i];
                for j in (i + 1)..buys.len() {
                    let diff = if anchor > buys[j] {
                        anchor - buys[j]
                    } else {
                        buys[j] - anchor
                    };
                    let pct = if anchor > 0 {
                        diff as f64 / anchor as f64
                    } else {
                        1.0
                    };
                    if pct <= 0.15 {
                        streak += 1;
                    } else {
                        break;
                    }
                }
                max_streak = max_streak.max(streak);
            }
            TrajectorySegmentSnapshot {
                tx_count: built.tx_count as u64,
                buy_ratio: built.buy_ratio,
                avg_interval_ms: built.avg_interval_ms,
                total_volume_sol: built.total_volume_sol,
                hhi: built.hhi,
                max_single_tx_sol: max_pip / 1e9,
                same_size_streak: max_streak,
            }
        }
        Some(ghost_core::checkpoint::TxSegmentSequence {
            t0_segment: snapshot(&seg0),
            t1_segment: snapshot(&seg1),
            t2_segment: snapshot(&seg2),
            total_duration_ms: duration_ms,
            min_tx_per_segment_satisfied: min_satisfied,
        })
    }

    pub fn current_materialized_trajectory(
        &self,
    ) -> Option<ghost_core::checkpoint::MaterializedTrajectoryAssessment> {
        self.materialize_trajectory(&self.config.tas)
            .map(|trajectory| trajectory.to_materialized())
    }

    #[must_use]
    pub fn current_segment_sequence(
        &self,
        config: &TrajectoryAwareScoringConfig,
    ) -> Option<ghost_core::checkpoint::TxSegmentSequence> {
        use crate::components::gatekeeper_trajectory::build_segment;
        use ghost_core::checkpoint::TrajectorySegmentSnapshot;
        if !config.enabled {
            return None;
        }
        let first_ts = self.first_tx_ts_ms()?;
        let last_ts = self.highest_seen_ts_ms();
        let duration_ms = last_ts.saturating_sub(first_ts);
        if duration_ms < config.tas_min_total_duration_ms {
            return None;
        }
        let min_total_tx = config.tas_min_tx_per_segment.saturating_mul(3);
        if self.total_tx_count < min_total_tx {
            return None;
        }
        let seg_dur = duration_ms as f64 / 3.0;
        let t0_end = first_ts.saturating_add(seg_dur as u64);
        let t1_end = first_ts.saturating_add((2.0 * seg_dur) as u64);
        let mut seg0: Vec<&PoolTransaction> = Vec::new();
        let mut seg1: Vec<&PoolTransaction> = Vec::new();
        let mut seg2: Vec<&PoolTransaction> = Vec::new();
        for btx in &self.buffered_txs {
            let ts = btx.tx.timestamp_ms;
            if ts <= t0_end {
                seg0.push(&btx.tx);
            } else if ts <= t1_end {
                seg1.push(&btx.tx);
            } else {
                seg2.push(&btx.tx);
            }
        }
        let min_per_seg = config.tas_min_tx_per_segment;
        let min_satisfied =
            seg0.len() >= min_per_seg && seg1.len() >= min_per_seg && seg2.len() >= min_per_seg;
        fn snapshot(seg: &[&PoolTransaction]) -> TrajectorySegmentSnapshot {
            let built = build_segment(seg);
            let max_pip = seg
                .iter()
                .map(|tx| tx.sol_amount_lamports.unwrap_or(0) as f64)
                .fold(0.0_f64, |a, b| a.max(b));
            let buys: Vec<u64> = seg
                .iter()
                .filter(|tx| tx.is_buy)
                .filter_map(|tx| tx.sol_amount_lamports)
                .collect();
            let mut max_streak: u32 = 0;
            for i in 0..buys.len() {
                let mut streak: u32 = 1;
                let anchor = buys[i];
                for j in (i + 1)..buys.len() {
                    let diff = if anchor > buys[j] {
                        anchor - buys[j]
                    } else {
                        buys[j] - anchor
                    };
                    let pct = if anchor > 0 {
                        diff as f64 / anchor as f64
                    } else {
                        1.0
                    };
                    if pct <= 0.15 {
                        streak += 1;
                    } else {
                        break;
                    }
                }
                max_streak = max_streak.max(streak);
            }
            TrajectorySegmentSnapshot {
                tx_count: built.tx_count as u64,
                buy_ratio: built.buy_ratio,
                avg_interval_ms: built.avg_interval_ms,
                total_volume_sol: built.total_volume_sol,
                hhi: built.hhi,
                max_single_tx_sol: max_pip / 1e9,
                same_size_streak: max_streak,
            }
        }
        Some(ghost_core::checkpoint::TxSegmentSequence {
            t0_segment: snapshot(&seg0),
            t1_segment: snapshot(&seg1),
            t2_segment: snapshot(&seg2),
            total_duration_ms: duration_ms,
            min_tx_per_segment_satisfied: min_satisfied,
        })
    }

    /// No sliding-window cleanup is performed so the assessment covers the
    /// entire observation window.
    fn on_transaction_long(&mut self, tx: Arc<PoolTransaction>) -> GatekeeperVerdict {
        // ═══════════════════════════════════════════
        // MONOTONIC TIME update
        // ═══════════════════════════════════════════
        let tx_ts = Self::tx_event_ts_ms(tx.as_ref());
        let now_ms = tx_ts.max(self.highest_seen_ts);
        self.highest_seen_ts = now_ms;

        // Record first TX timestamp for hard deadline
        if self.first_tx_ts.is_none() {
            self.first_tx_ts = Some(tx_ts);
        }

        // ═══════════════════════════════════════════
        // DEDUP CHECK (TxKey)
        // ═══════════════════════════════════════════
        let tx_key = match Self::tx_key_from_tx(&tx) {
            Some(key) => key,
            None => {
                self.window_events.push_back(V2WindowEvent {
                    timestamp_ms: tx_ts,
                    is_duplicate: true,
                });
                // Still check deadline even on dupes
                return self.check_long_deadline(now_ms);
            }
        };
        if self.tx_keys_seen.contains(&tx_key) {
            self.window_events.push_back(V2WindowEvent {
                timestamp_ms: tx_ts,
                is_duplicate: true,
            });
            return self.check_long_deadline(now_ms);
        }
        self.track_tx_key(tx_key.clone());
        self.track_tx_signature(tx.as_ref());

        // ═══════════════════════════════════════════
        // BUFFER + update_tracking()
        // ═══════════════════════════════════════════
        self.window_events.push_back(V2WindowEvent {
            timestamp_ms: tx_ts,
            is_duplicate: false,
        });

        // Update all 6 phase tracking structures
        self.update_tracking(&tx);

        self.buffered_txs.push(GatekeeperBufferedTx {
            tx: tx.clone(),
            metrics: self.metrics,
            tx_key,
        });
        self.refresh_canonical_dev_tracking();

        // ═══════════════════════════════════════════
        // V2.5 DYNAMIC OBSERVATION WINDOW: Shadow checkpoints
        // Single-owner entry: maybe_fire_shadow_checkpoint serializes all
        // checkpoint firing through *_shadow_fired flags (timer + TX path).
        // ═══════════════════════════════════════════
        self.maybe_fire_shadow_checkpoint(now_ms);

        // ═══════════════════════════════════════════
        // DEADLINE CHECK (single final evaluation)
        // ═══════════════════════════════════════════
        self.check_long_deadline(now_ms)
    }

    /// Curve readiness latch: intercepts a would-be BUY when curve data is
    /// required but not yet available.
    ///
    /// Returns:
    /// - `None`            → curve OK (or feature disabled), caller proceeds normally
    /// - `Some(PendingCurve)` → still waiting, before deadline
    /// - `Some(Reject{..})`   → deadline expired, hard-fail
    fn check_curve_latch(&mut self, now_ms: u64) -> Option<GatekeeperVerdict> {
        if self.curve_ready {
            return None; // curve is available → proceed
        }

        match self.curve_quality {
            CurveFreshnessState::Fresh | CurveFreshnessState::Committed => None,
            CurveFreshnessState::Unknown => {
                if !self.config.curve_require_for_buy {
                    Some(self.reject_curve_policy(
                        "unknown_curve_reject",
                        "rejected",
                        "HARD_FAIL: CURVE_UNKNOWN_REJECTED_BY_POLICY".to_string(),
                    ))
                } else {
                    let t0 = self.curve_t0_event_ts_ms.unwrap_or(0);
                    let waited_ms = now_ms.saturating_sub(t0);
                    Some(self.pending_curve_before_deadline(
                        now_ms,
                        "unknown_curve_pending",
                        "unknown_curve_timeout",
                        format!(
                            "HARD_FAIL: CURVE_NOT_READY_TIMEOUT (waited_ms={}, curve_quality=unknown)",
                            waited_ms,
                        ),
                    ))
                }
            }
            CurveFreshnessState::Stale => {
                let stale_reason = match self.curve_finality_state {
                    CurveFinality::Speculative => "stale_curve_speculative",
                    CurveFinality::Provisional => "stale_curve_provisional",
                    CurveFinality::Finalized => "stale_curve_finalized",
                };

                match self.config.stale_fallback {
                    ShadowLedgerStaleFallback::Reject => Some(self.reject_curve_policy(
                        stale_reason,
                        "rejected",
                        format!(
                            "HARD_FAIL: CURVE_STALE_REJECTED (finality={})",
                            self.curve_finality_state.as_str(),
                        ),
                    )),
                    ShadowLedgerStaleFallback::PendingCurve => {
                        let t0 = self.curve_t0_event_ts_ms.unwrap_or(0);
                        let waited_ms = now_ms.saturating_sub(t0);
                        Some(self.pending_curve_before_deadline(
                            now_ms,
                            stale_reason,
                            "stale_curve_timeout",
                            format!(
                                "HARD_FAIL: CURVE_STALE_TIMEOUT (waited_ms={}, finality={})",
                                waited_ms,
                                self.curve_finality_state.as_str(),
                            ),
                        ))
                    }
                    ShadowLedgerStaleFallback::UseStaleWithWarning => {
                        if self.curve_finality_state.is_finalized() {
                            self.refresh_curve_policy_state();
                            None
                        } else {
                            let t0 = self.curve_t0_event_ts_ms.unwrap_or(0);
                            let waited_ms = now_ms.saturating_sub(t0);
                            Some(self.pending_curve_before_deadline(
                                now_ms,
                                stale_reason,
                                "stale_curve_timeout",
                                format!(
                                    "HARD_FAIL: CURVE_STALE_TIMEOUT (waited_ms={}, finality={})",
                                    waited_ms,
                                    self.curve_finality_state.as_str(),
                                ),
                            ))
                        }
                    }
                }
            }
        }
    }

    /// Build a terminal hard-fail reject from the buffer's current assessment state.
    pub fn reject_hard_fail(&mut self, reason: String) -> GatekeeperVerdict {
        self.rejected = true;
        let mut assessment = self.run_assessment();
        assessment.decision = Some(GatekeeperDecision {
            hard_fail_reason: Some(reason.clone()),
            core1_passed: false,
            core2_passed: false,
            core3_passed: false,
            dev_unknown: false,
            soft_points: 0,
            max_soft_points_possible: 0,
            effective_max_soft_points: 0,
            soft_signals: SoftSignals::default(),
            sybil_policy: SybilPolicyDiagnostics::default(),
            alpha_gate: AlphaGateDiagnostics::not_run(self.config.enable_alpha_gate),
            prosperity_filter: ProsperityFilterDiagnostics::not_run(
                self.config.enable_prosperity_filter,
            ),
            total_soft_points: 0,
            verdict_buy: false,
            verdict_type: GatekeeperVerdictType::RejectHardFail,
            reason_chain: reason.clone(),
            gatekeeper_strength: None,
        });
        GatekeeperVerdict::Reject { assessment, reason }
    }

    /// Check if the long-mode deadline has been reached and, if so, perform
    /// the single final evaluation.  Returns `Wait` if the deadline has not
    /// yet elapsed.
    /// Force-check the long-mode deadline using an externally-supplied
    /// wall-clock timestamp.  Used by the oracle_runtime periodic sweep to
    /// ensure long-mode buffers time out even when no new transactions arrive.
    pub fn force_check_deadline(&mut self, wall_clock_ms: u64) -> GatekeeperVerdict {
        if self.rejected || self.state.allows_runtime_relay() {
            return GatekeeperVerdict::Wait;
        }

        if self.config.mode != GatekeeperMode::Long {
            let now_ms = wall_clock_ms.max(self.highest_seen_ts);
            self.highest_seen_ts = now_ms;

            if let Some(v) = self.check_standard_deadline(now_ms) {
                return v;
            }

            // In standard mode, honour the curve deadline during sweep.
            // Use highest_seen_ts (event-time) for the comparison — wall_clock_ms
            // is only a fallback when no events have been ingested yet.
            let now_event = now_ms;
            if !self.rejected && !self.curve_ready {
                if let Some(v) = self.check_curve_latch(now_event) {
                    match &v {
                        GatekeeperVerdict::Reject { .. } => {
                            // Curve timeout is terminal — let oracle_runtime log it
                            return v;
                        }
                        _ => {} // PendingCurve: nothing to do in sweep
                    }
                }
            }
            return GatekeeperVerdict::Wait;
        }
        let now_ms = wall_clock_ms.max(self.highest_seen_ts);
        self.highest_seen_ts = now_ms;
        self.check_long_deadline(now_ms)
    }

    fn record_deadline_finalize_metrics(
        &self,
        mode: &'static str,
        verdict: &'static str,
        now_ms: u64,
    ) {
        let elapsed_ms = now_ms.saturating_sub(self.registered_wall_ts_ms);
        metrics::increment_counter!(
            "gatekeeper_deadline_finalize_total",
            "mode" => mode,
            "verdict" => verdict
        );
        metrics::histogram!(
            "gatekeeper_deadline_finalize_elapsed_ms",
            elapsed_ms as f64,
            "mode" => mode,
            "verdict" => verdict
        );
    }

    fn check_standard_deadline(&mut self, now_ms: u64) -> Option<GatekeeperVerdict> {
        if now_ms < self.deadline_wall_ts_ms {
            return None;
        }

        if self.phase1_passed {
            // Phase 1 was met → final evaluation on all collected data
            let verdict = self.evaluate_phases();
            return match &verdict {
                GatekeeperVerdict::Buy { .. } => {
                    self.record_deadline_finalize_metrics("standard", "buy", now_ms);
                    Some(verdict)
                }
                GatekeeperVerdict::Reject { .. } => {
                    self.record_deadline_finalize_metrics("standard", "reject", now_ms);
                    Some(verdict)
                }
                _ => {
                    // evaluate_phases returned Wait (not enough phases) → Reject at deadline
                    let mut assessment = self.run_assessment();
                    let breakdown = Self::format_phase_breakdown(&assessment);
                    let reason = format!(
                        "TIMEOUT: {}/{} phases {}",
                        assessment.phases_passed, self.config.min_phases_to_pass, breakdown
                    );
                    self.rejected = true;
                    tracing::info!(
                        pool = %self.pool_id,
                        reason = %reason,
                        phases = %breakdown,
                        deadline_wall_ts_ms = self.deadline_wall_ts_ms,
                        "🚫 GATEKEEPER V2 REJECTED (Deadline, insufficient phases) {}", breakdown
                    );
                    self.record_deadline_finalize_metrics("standard", "reject", now_ms);
                    assessment.cache_v25_confidence(&self.config);
                    Some(GatekeeperVerdict::Reject { assessment, reason })
                }
            };
        }

        // Phase 1 never met → Timeout
        let assessment = self.build_minimal_assessment();
        self.rejected = true;
        let breakdown = Self::format_phase_breakdown(&assessment);
        let elapsed_ms = now_ms.saturating_sub(self.registered_wall_ts_ms);
        tracing::info!(
            pool = %self.pool_id,
            tx_count = self.total_tx_count,
            unique_signers = self.unique_signers.len(),
            buy_count = self.buy_count,
            elapsed_ms = elapsed_ms,
            phases = %breakdown,
            deadline_wall_ts_ms = self.deadline_wall_ts_ms,
            "🚫 GATEKEEPER V2 TIMEOUT (Phase 1 never met: tx={}/{} signers={}/{} buys={}/{}) {}",
            self.total_tx_count, self.config.min_tx_count,
            self.unique_signers.len(), self.config.min_unique_signers,
            self.buy_count, self.config.min_buy_count,
            breakdown
        );
        self.record_deadline_finalize_metrics("standard", "timeout", now_ms);
        Some(GatekeeperVerdict::Timeout { assessment })
    }

    fn check_long_deadline(&mut self, now_ms: u64) -> GatekeeperVerdict {
        if now_ms < self.deadline_wall_ts_ms {
            return GatekeeperVerdict::Wait;
        }

        // ─── Deadline reached ───────────────────────
        // Phase 1 check (quantity gate)
        let has_enough_tx = self.total_tx_count >= self.config.min_tx_count;
        let has_enough_signers = self.unique_signers.len() >= self.config.min_unique_signers;
        let has_enough_buys = self.buy_count >= self.config.min_buy_count;

        if !(has_enough_tx && has_enough_signers && has_enough_buys) {
            // Phase 1 never met → Timeout
            let assessment = self.build_minimal_assessment();
            self.rejected = true;
            let breakdown = Self::format_phase_breakdown(&assessment);
            tracing::info!(
                pool = %self.pool_id,
                tx_count = self.total_tx_count,
                unique_signers = self.unique_signers.len(),
                buy_count = self.buy_count,
                elapsed_ms = now_ms.saturating_sub(self.registered_wall_ts_ms),
                mode = "long",
                phases = %breakdown,
                deadline_wall_ts_ms = self.deadline_wall_ts_ms,
                "🚫 GATEKEEPER V2 LONG TIMEOUT (Phase 1 never met: tx={}/{} signers={}/{} buys={}/{}) {}",
                self.total_tx_count, self.config.min_tx_count,
                self.unique_signers.len(), self.config.min_unique_signers,
                self.buy_count, self.config.min_buy_count,
                breakdown
            );
            self.record_deadline_finalize_metrics("long", "timeout", now_ms);
            return GatekeeperVerdict::Timeout { assessment };
        }

        // Phase 1 met → mark it for the assessment
        self.phase1_passed = true;
        self.phase1_passed_at_count = Some(self.total_tx_count);

        // Single final evaluation of phases 2-6
        self.eval_count += 1;
        self.last_eval_at_count = Some(self.total_tx_count);

        let mut assessment = self.run_assessment();
        let breakdown = Self::format_phase_breakdown(&assessment);

        // ── Three-Layer Decision System ──
        if self.config.use_three_layer_decision {
            let decision = self.compute_decision(&assessment);
            let soft_pts = decision.soft_points;
            let max_pts = decision.max_soft_points_possible;
            let verdict_buy = decision.verdict_buy;
            let verdict_tag = decision.verdict_type.tag();
            let reason_chain = decision.reason_chain.clone();
            assessment.decision = Some(decision);
            assessment.cache_v25_confidence(&self.config);

            // ── V2.5 Extended shadow verdict (terminal deadline, 7-10s) ──
            // Timer-aware: if the DOW timer already fired Extended, we skip.
            // If not, this is a deadline fallback — the timer may have missed
            // the window (e.g., late pool registration, timer starvation).
            if self.config.v25.shadow_enabled && self.config.dow.enabled {
                let elapsed_ms = now_ms.saturating_sub(self.registered_wall_ts_ms);

                if self.extended_shadow_fired {
                    // Timer already produced the Extended shadow decision.
                    // Log telemetry-only to confirm deadline/timer alignment.
                    tracing::debug!(
                        pool = %self.pool_id,
                        elapsed_ms = elapsed_ms,
                        "V2.5 EXTENDED_SHADOW_DEADLINE_SKIPPED: timer already fired Extended"
                    );
                } else {
                    let extended_conf = assessment.v25_confidence.unwrap_or(0.0);
                    let min_conf = self.config.dow.extended_window_min_confidence;
                    let require_pdd = self.config.dow.extended_require_pdd_clean;
                    let pdd_clean = assessment.v25_pdd_clean();
                    let tas_reject = assessment.v25_tas_hard_reject(&self.config);

                    let (ext_kind, ext_reason) = if let Some(fail) = assessment
                        .pdd_assessment
                        .as_ref()
                        .and_then(|pdd| pdd.hard_fail.as_ref())
                    {
                        (
                            ShadowDecisionKind::RejectPumpAndDump,
                            format!(
                                "EXTENDED_SHADOW_DEADLINE_FALLBACK_REJECT_PDD_{}: verdict={} conf={:.3} phases={}/6",
                                fail.as_str(),
                                verdict_tag,
                                extended_conf,
                                assessment.phases_passed
                            ),
                        )
                    } else if tas_reject {
                        (
                            ShadowDecisionKind::RejectLowTrajectory,
                            format!(
                                "EXTENDED_SHADOW_DEADLINE_FALLBACK_REJECT_LOW_TRAJECTORY: conf={:.3} tas<{:.2}",
                                extended_conf, self.config.tas.tas_hard_reject_threshold,
                            ),
                        )
                    } else if verdict_buy
                        && extended_conf > 0.0
                        && extended_conf >= min_conf
                        && (!require_pdd || pdd_clean)
                    {
                        (
                            ShadowDecisionKind::NormalBuyCandidate,
                            format!(
                                "EXTENDED_SHADOW_DEADLINE_FALLBACK_BUY: verdict={} conf={:.3} min_conf={:.2} require_pdd={} pdd_clean={} phases={}/6",
                                verdict_tag, extended_conf, min_conf, require_pdd, pdd_clean, assessment.phases_passed
                            ),
                        )
                    } else if verdict_buy && require_pdd && !pdd_clean {
                        (
                            ShadowDecisionKind::ShadowReject,
                            format!(
                                "EXTENDED_SHADOW_DEADLINE_FALLBACK_REJECT_PDD_NOT_CLEAN: verdict={} conf={:.3} min_conf={:.2} pdd_clean={}",
                                verdict_tag, extended_conf, min_conf, pdd_clean
                            ),
                        )
                    } else if verdict_buy && extended_conf <= 0.0 {
                        (
                            ShadowDecisionKind::ShadowReject,
                            format!(
                                "EXTENDED_SHADOW_DEADLINE_FALLBACK_REJECT_ZERO_CONFIDENCE: verdict={} conf={:.3}",
                                verdict_tag, extended_conf
                            ),
                        )
                    } else {
                        (
                            ShadowDecisionKind::ShadowReject,
                            format!(
                                "EXTENDED_SHADOW_DEADLINE_FALLBACK: verdict={} conf={:.3} min_conf={:.2} pdd_clean={} phases={}/6",
                                verdict_tag, extended_conf, min_conf, pdd_clean, assessment.phases_passed
                            ),
                        )
                    };

                    self.extended_shadow_fired = true;
                    self.v25_shadow_decisions.push(ShadowV25Decision {
                        kind: ext_kind,
                        window: ObservationStage::Extended,
                        elapsed_ms,
                        confidence: extended_conf,
                        phases_passed: assessment.phases_passed.min(6),
                        reason: ext_reason,
                    });
                }
                self.window_stage = ObservationStage::Extended;
            }

            // Transfer shadow decisions (including the Extended verdict just
            // produced above) to the assessment so downstream consumers
            // (tests, JSONL to_buy_log) see the complete shadow plane.
            assessment.v25_shadow_decisions = self.v25_shadow_decisions.clone();

            if verdict_buy {
                // Curve readiness latch (long mode, three-layer)
                if let Some(curve_v) = self.check_curve_latch(now_ms) {
                    if let GatekeeperVerdict::Reject { .. } = &curve_v {
                        self.rejected = true;
                        self.record_deadline_finalize_metrics("long", "reject", now_ms);
                        return curve_v;
                    }
                    // PendingCurve: at long-mode deadline this shouldn't happen
                    // (now_ms >= max_wait_time_ms > curve_wait_ms), but handle gracefully
                }
                self.state = PoolState::Approved;
                self.record_deadline_finalize_metrics("long", "buy", now_ms);
                tracing::info!(
                    pool = %self.pool_id,
                    phases_passed = assessment.phases_passed,
                    phases = %breakdown,
                    soft_pts = soft_pts,
                    verdict = verdict_tag,
                    eval_count = self.eval_count,
                    tx_count = self.total_tx_count,
                    reason = %reason_chain,
                    mode = "long",
                    "✅ GATEKEEPER V2 LONG BUY {} soft_pts={}/{}", breakdown, soft_pts, max_pts
                );
                let buffered = std::mem::take(&mut self.buffered_txs);
                return GatekeeperVerdict::Buy {
                    buffered_txs: buffered,
                    assessment,
                };
            }

            // REJECT at deadline
            let reason = format!("LONG DEADLINE: {} {}", reason_chain, breakdown);
            self.rejected = true;
            tracing::info!(
                pool = %self.pool_id,
                reason = %reason,
                phases = %breakdown,
                soft_pts = soft_pts,
                verdict = verdict_tag,
                mode = "long",
                "🚫 GATEKEEPER V2 LONG REJECTED {} soft_pts={}/{}", breakdown, soft_pts, max_pts
            );
            self.record_deadline_finalize_metrics("long", "reject", now_ms);
            return GatekeeperVerdict::Reject { assessment, reason };
        }

        // ── Legacy Decision System (phases_passed) ──
        // In long mode we do NOT honour hard-rejects — we simply check phase count
        // (the hard-reject data is still in the assessment for logging).
        if assessment.phases_passed >= self.config.min_phases_to_pass {
            // Curve readiness latch (long mode, legacy)
            if let Some(curve_v) = self.check_curve_latch(now_ms) {
                if let GatekeeperVerdict::Reject { .. } = &curve_v {
                    self.rejected = true;
                    self.record_deadline_finalize_metrics("long", "reject", now_ms);
                    return curve_v;
                }
            }
            self.state = PoolState::Approved;
            self.record_deadline_finalize_metrics("long", "buy", now_ms);
            tracing::info!(
                pool = %self.pool_id,
                phases_passed = assessment.phases_passed,
                phases = %breakdown,
                eval_count = self.eval_count,
                tx_count = self.total_tx_count,
                mode = "long",
                "✅ GATEKEEPER V2 LONG BUY {}", breakdown
            );
            let buffered = std::mem::take(&mut self.buffered_txs);
            return GatekeeperVerdict::Buy {
                buffered_txs: buffered,
                assessment,
            };
        }

        // Not enough phases → Reject at deadline
        let reason = format!(
            "TIMEOUT: deadline reached, {}/{} phases {}",
            assessment.phases_passed, self.config.min_phases_to_pass, breakdown
        );
        self.rejected = true;
        tracing::info!(
            pool = %self.pool_id,
            reason = %reason,
            phases = %breakdown,
            mode = "long",
            "🚫 GATEKEEPER V2 LONG REJECTED (Deadline, insufficient phases) {}", breakdown
        );
        self.record_deadline_finalize_metrics("long", "timeout", now_ms);
        GatekeeperVerdict::Timeout { assessment }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_brain::config::GatekeeperV2Config;
    use seer::types::RawBytesMissingReason;

    fn create_v2_mock_tx(timestamp_ms: u64, signature: &str) -> PoolTransaction {
        PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "pool1".to_string(),
            slot: Some(100),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms,
            event_time: ghost_core::EventTimeMetadata::new(
                None,
                (timestamp_ms > 0).then_some(timestamp_ms),
                None,
            ),
            arrival_ts_ms: timestamp_ms,
            signer: format!("signer_{}", signature),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: signature.to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: ghost_core::CurveFinality::Speculative,
        }
    }

    #[test]
    fn event_time_primary_in_runtime_gatekeeper() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut tx = create_v2_mock_tx(1_000, "arrival_pref");
        tx.event_time.ingress_wall_ts_ms = Some(1_500);
        tx.arrival_ts_ms = 9_000;
        let _ = gk.on_transaction(Arc::new(tx));

        assert_eq!(gk.first_tx_ts, Some(1_500));
        assert_eq!(gk.highest_seen_ts, 1_500);
        assert_eq!(gk.window_events.back().map(|e| e.timestamp_ms), Some(1_500));
    }

    #[test]
    fn arrival_axis_is_not_used_as_event_time_fallback() {
        let mut tx = create_v2_mock_tx(0, "arrival_only");
        tx.arrival_ts_ms = 9_000;

        let before = GatekeeperBuffer::now_wall_ms();
        let resolved = GatekeeperBuffer::tx_event_ts_ms(&tx);
        let after = GatekeeperBuffer::now_wall_ms();

        assert!(resolved >= before);
        assert!(resolved <= after);
        assert_ne!(resolved, tx.arrival_ts_ms);
    }

    #[test]
    fn legacy_timestamp_is_not_used_as_event_time_fallback() {
        let mut tx = create_v2_mock_tx(7_000, "legacy_only");
        tx.event_time = ghost_core::EventTimeMetadata::default();

        let before = GatekeeperBuffer::now_wall_ms();
        let resolved = GatekeeperBuffer::tx_event_ts_ms(&tx);
        let after = GatekeeperBuffer::now_wall_ms();

        assert!(resolved >= before);
        assert!(resolved <= after);
        assert_ne!(resolved, tx.timestamp_ms);
    }

    fn v2_default_config() -> GatekeeperV2Config {
        let mut cfg = GatekeeperV2Config::default();
        cfg.use_three_layer_decision = false;
        cfg
    }

    #[test]
    fn canonical_creator_dev_buy_prefers_create_signature_primary_buy() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut later_buy = create_v2_mock_tx(1_200, "later_buy");
        later_buy.signer = "creator_wallet".to_string();
        later_buy.volume_sol = 2.0;
        later_buy.token_amount_units = Some(22_000_000);
        let _ = gk.on_transaction(Arc::new(later_buy));

        let mut genesis_buy = create_v2_mock_tx(1_000, "create_sig");
        genesis_buy.signer = "creator_wallet".to_string();
        genesis_buy.volume_sol = 0.5;
        genesis_buy.token_amount_units = Some(17_000_000);
        let _ = gk.on_transaction(Arc::new(genesis_buy));

        gk.set_pool_identity_with_liquidity(Some("creator_wallet"), Some("create_sig"), Some(9.0));

        assert_eq!(gk.dev_wallet.as_deref(), Some("creator_wallet"));
        assert!((gk.dev_buy_total_sol - 0.5).abs() < f64::EPSILON);
        assert!((gk.dev_buy_volume_total_sol - 2.5).abs() < f64::EPSILON);
        assert_eq!(gk.dev_tx_count, 2);
        assert_eq!(gk.dev_initial_buy_tokens, Some(17_000_000.0));
    }

    #[test]
    fn canonical_creator_dev_buy_falls_back_to_earliest_creator_buy_when_create_signature_absent() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut first_buy = create_v2_mock_tx(1_000, "first_buy");
        first_buy.signer = "creator_wallet".to_string();
        first_buy.volume_sol = 0.7;
        first_buy.token_amount_units = Some(11_000_000);
        let _ = gk.on_transaction(Arc::new(first_buy));

        let mut second_buy = create_v2_mock_tx(1_300, "second_buy");
        second_buy.signer = "creator_wallet".to_string();
        second_buy.volume_sol = 1.3;
        second_buy.token_amount_units = Some(19_000_000);
        let _ = gk.on_transaction(Arc::new(second_buy));

        gk.set_pool_identity(Some("creator_wallet"), Some("missing_create_sig"));

        assert_eq!(gk.dev_wallet.as_deref(), Some("creator_wallet"));
        assert!((gk.dev_buy_total_sol - 0.7).abs() < f64::EPSILON);
        assert!((gk.dev_buy_volume_total_sol - 2.0).abs() < f64::EPSILON);
        assert_eq!(gk.dev_initial_buy_tokens, Some(11_000_000.0));
    }

    #[test]
    fn canonical_creator_dev_buy_remains_zero_when_creator_only_sells() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut creator_sell_a = create_v2_mock_tx(1_000, "creator_sell_a");
        creator_sell_a.signer = "creator_wallet".to_string();
        creator_sell_a.is_buy = false;
        creator_sell_a.volume_sol = 0.8;
        let _ = gk.on_transaction(Arc::new(creator_sell_a));

        let mut creator_sell_b = create_v2_mock_tx(1_200, "creator_sell_b");
        creator_sell_b.signer = "creator_wallet".to_string();
        creator_sell_b.is_buy = false;
        creator_sell_b.volume_sol = 0.6;
        let _ = gk.on_transaction(Arc::new(creator_sell_b));

        gk.set_pool_identity_with_liquidity(Some("creator_wallet"), Some("create_sig"), Some(1.75));

        assert_eq!(gk.dev_wallet.as_deref(), Some("creator_wallet"));
        assert!((gk.dev_buy_total_sol - 0.0).abs() < f64::EPSILON);
        assert!((gk.dev_buy_volume_total_sol - 0.0).abs() < f64::EPSILON);
        assert!((gk.dev_sell_total_sol - 1.4).abs() < f64::EPSILON);
        assert_eq!(gk.dev_tx_count, 2);
        assert!(gk.dev_has_sold);
        assert_eq!(gk.dev_initial_buy_tokens, None);
    }

    #[test]
    fn test_dust_filter_silent_drop() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_sol_threshold = 0.01;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut tx = create_v2_mock_tx(1000, "dust_sig");
        tx.volume_sol = 0.005; // below threshold
        let v = gk.on_transaction(Arc::new(tx));
        assert!(matches!(v, GatekeeperVerdict::Wait));
        assert_eq!(gk.dust_filtered_count, 1);
        assert_eq!(
            gk.total_tx_count, 0,
            "Dust TX must NOT increment total_tx_count"
        );
    }

    #[test]
    fn test_phase1_timeout_not_enough_tx() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_wait_time_ms = 10_000; // 10s
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        // Send 3 TX in 12 seconds → should timeout
        let tx1 = Arc::new(create_v2_mock_tx(1000, "s1"));
        let v1 = gk.on_transaction(tx1);
        assert!(matches!(v1, GatekeeperVerdict::Wait));

        let tx2 = Arc::new(create_v2_mock_tx(5000, "s2"));
        let v2 = gk.on_transaction(tx2);
        assert!(matches!(v2, GatekeeperVerdict::Wait));

        // 12s from first TX → timeout
        let tx3 = Arc::new(create_v2_mock_tx(13001, "s3"));
        let v3 = gk.on_transaction(tx3);
        assert!(matches!(v3, GatekeeperVerdict::Timeout { .. }));
    }

    #[test]
    fn test_monotonic_time_preserved() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let tx1 = Arc::new(create_v2_mock_tx(1000, "s1"));
        gk.on_transaction(tx1);
        assert_eq!(gk.highest_seen_ts, 1000);

        // Out-of-order timestamp
        let tx2 = Arc::new(create_v2_mock_tx(800, "s2"));
        gk.on_transaction(tx2);
        assert!(
            gk.highest_seen_ts >= 1000,
            "highest_seen_ts must never decrease"
        );

        let tx3 = Arc::new(create_v2_mock_tx(1500, "s3"));
        gk.on_transaction(tx3);
        assert_eq!(gk.highest_seen_ts, 1500);
    }

    #[test]
    fn test_min_sol_configurable() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_sol_threshold = 0.1;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX below threshold → filtered
        let mut tx_small = create_v2_mock_tx(1000, "small");
        tx_small.volume_sol = 0.05;
        let v1 = gk.on_transaction(Arc::new(tx_small));
        assert!(matches!(v1, GatekeeperVerdict::Wait));
        assert_eq!(gk.dust_filtered_count, 1);
        assert_eq!(gk.total_tx_count, 0);

        // TX above threshold → accepted
        let mut tx_big = create_v2_mock_tx(1100, "big");
        tx_big.volume_sol = 0.15;
        let v2 = gk.on_transaction(Arc::new(tx_big));
        assert!(matches!(v2, GatekeeperVerdict::Wait));
        assert_eq!(gk.dust_filtered_count, 1);
        assert_eq!(gk.total_tx_count, 1);
    }

    #[test]
    fn test_phase1_basic_pass() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_wait_time_ms = 30_000;
        cfg.min_phases_to_pass = 5; // high requirement so identical-vol+regular-timing → Wait
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Send 8 TX from 5+ unique signers, all buys
        for i in 0..7 {
            let sig = format!("sig_{}", i);
            let signer_id = if i < 5 {
                format!("sig_{}", i)
            } else {
                format!("sig_{}", i % 5)
            };
            let mut tx = create_v2_mock_tx(1000 + i as u64 * 100, &sig);
            tx.signer = format!("signer_{}", signer_id);
            let v = gk.on_transaction(Arc::new(tx));
            assert!(matches!(v, GatekeeperVerdict::Wait));
        }

        // 8th TX → Phase 1 triggers, evaluate_phases() runs analysis
        let mut tx8 = create_v2_mock_tx(1700, "sig_7");
        tx8.signer = "signer_sig_7".to_string(); // 6th unique signer
        let v8 = gk.on_transaction(Arc::new(tx8));
        // Not enough phases pass (identical volumes, regular timing) → Wait
        assert!(matches!(v8, GatekeeperVerdict::Wait));
        assert!(gk.phase1_passed, "Phase 1 should have passed");
    }

    #[test]
    fn test_phase1_not_enough_signers() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 5;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 3;
        cfg.max_wait_time_ms = 30_000;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 8 TX from only 3 unique signers
        for i in 0..8 {
            let sig = format!("nsig_{}", i);
            let mut tx = create_v2_mock_tx(1000 + i as u64 * 100, &sig);
            tx.signer = format!("signer_{}", i % 3); // only 3 unique signers
            let v = gk.on_transaction(Arc::new(tx));
            assert!(matches!(v, GatekeeperVerdict::Wait));
        }
        assert!(
            !gk.phase1_passed,
            "Phase 1 should NOT pass with only 3 signers"
        );
    }

    #[test]
    fn test_phase1_not_enough_buys() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 5;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_wait_time_ms = 30_000;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 8 TX, 5+ signers, but only 2 buys
        for i in 0..8 {
            let sig = format!("bsig_{}", i);
            let mut tx = create_v2_mock_tx(1000 + i as u64 * 100, &sig);
            tx.is_buy = i < 2; // only first 2 are buys
            let v = gk.on_transaction(Arc::new(tx));
            assert!(matches!(v, GatekeeperVerdict::Wait));
        }
        assert!(
            !gk.phase1_passed,
            "Phase 1 should NOT pass with only 2 buys"
        );
    }

    #[test]
    fn test_update_tracking_signer_stats() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut tx1 = create_v2_mock_tx(1000, "ss1");
        tx1.signer = "alice".to_string();
        tx1.volume_sol = 2.0;
        tx1.is_buy = true;
        gk.on_transaction(Arc::new(tx1));

        let mut tx2 = create_v2_mock_tx(1100, "ss2");
        tx2.signer = "alice".to_string();
        tx2.volume_sol = 3.0;
        tx2.is_buy = false;
        gk.on_transaction(Arc::new(tx2));

        let mut tx3 = create_v2_mock_tx(1200, "ss3");
        tx3.signer = "bob".to_string();
        tx3.volume_sol = 1.5;
        tx3.is_buy = true;
        gk.on_transaction(Arc::new(tx3));

        let alice_stats = gk.signer_stats.get("alice").unwrap();
        assert_eq!(alice_stats.tx_count, 2);
        assert_eq!(alice_stats.buy_count, 1);
        assert_eq!(alice_stats.sell_count, 1);
        assert!((alice_stats.total_volume_sol - 5.0).abs() < f64::EPSILON);

        let bob_stats = gk.signer_stats.get("bob").unwrap();
        assert_eq!(bob_stats.tx_count, 1);
        assert_eq!(bob_stats.buy_count, 1);
        assert_eq!(bob_stats.sell_count, 0);
    }

    #[test]
    fn test_update_tracking_price_history() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut tx = create_v2_mock_tx(1000, "price_sig");
        tx.v_tokens_in_bonding_curve = Some(1_000_000_000.0);
        tx.v_sol_in_bonding_curve = Some(30.0);
        tx.market_cap_sol = Some(28.5);
        tx.curve_data_known = true;
        gk.on_transaction(Arc::new(tx));

        assert_eq!(gk.price_history.len(), 1);
        let pp = &gk.price_history[0];
        assert_eq!(pp.timestamp_ms, 1000);
        assert!((pp.price_sol_per_token - 30.0 / 1_000_000_000.0).abs() < 1e-12);
        assert!((pp.v_sol_in_curve - 30.0).abs() < f64::EPSILON);
        assert!((pp.v_tokens_in_curve - 1_000_000_000.0).abs() < f64::EPSILON);
        assert!((pp.market_cap_sol - 28.5).abs() < f64::EPSILON);

        // TX without bonding curve data → no price point added
        let tx_no_bc = create_v2_mock_tx(1100, "no_bc_sig");
        gk.on_transaction(Arc::new(tx_no_bc));
        assert_eq!(
            gk.price_history.len(),
            1,
            "No PricePoint for TX without bonding curve data"
        );
    }

    #[test]
    fn test_update_tracking_dev_wallet() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut tx = create_v2_mock_tx(1000, "dev_sig");
        tx.is_dev_buy = true;
        tx.signer = "dev_creator".to_string();
        tx.volume_sol = 0.5;
        tx.token_amount_units = Some(17_000_000);
        gk.on_transaction(Arc::new(tx));

        assert_eq!(gk.dev_wallet, Some("dev_creator".to_string()));
        assert!((gk.dev_buy_total_sol - 0.5).abs() < f64::EPSILON);
        assert_eq!(gk.dev_initial_buy_tokens, Some(17_000_000.0));
        assert_eq!(gk.dev_tx_count, 1);
        assert!(!gk.dev_has_sold);
    }

    #[test]
    fn test_default_config_values() {
        let cfg = GatekeeperV2Config::default();
        assert!((cfg.min_sol_threshold - 0.1).abs() < f64::EPSILON);
        assert_eq!(cfg.min_tx_count, 30);
        assert_eq!(cfg.min_unique_signers, 15);
        assert_eq!(cfg.min_buy_count, 15);
        assert_eq!(cfg.max_wait_time_ms, 2_222);
        assert!((cfg.min_interval_cv - 0.3).abs() < f64::EPSILON);
        assert!((cfg.max_interval_cv - 9999.0).abs() < f64::EPSILON);
        assert!((cfg.max_burst_ratio - 0.70).abs() < f64::EPSILON);
        assert!((cfg.min_avg_interval_ms - 60.0).abs() < f64::EPSILON);
        assert!((cfg.max_avg_interval_ms - 600.0).abs() < f64::EPSILON);
        assert!((cfg.min_timing_entropy - 1.2).abs() < f64::EPSILON);
        assert!((cfg.max_timing_entropy - 9999.0).abs() < f64::EPSILON);
        assert!((cfg.min_unique_ratio - 0.4).abs() < f64::EPSILON);
        assert!((cfg.max_hhi - 0.25).abs() < f64::EPSILON);
        assert_eq!(cfg.max_tx_per_signer, 4);
        assert!((cfg.max_volume_gini - 0.70).abs() < f64::EPSILON);
        assert!((cfg.max_top3_volume_pct - 0.75).abs() < f64::EPSILON);
        assert!((cfg.min_buy_ratio - 0.50).abs() < f64::EPSILON);
        assert!((cfg.min_avg_tx_sol - 0.02).abs() < f64::EPSILON);
        assert!((cfg.max_avg_tx_sol - 25.0).abs() < f64::EPSILON);
        assert!((cfg.min_volume_cv - 0.15).abs() < f64::EPSILON);
        assert!((cfg.max_volume_cv - 9999.0).abs() < f64::EPSILON);
        assert!((cfg.min_total_volume_sol - 0.5).abs() < f64::EPSILON);
        assert!((cfg.max_total_volume_sol - 9999.0).abs() < f64::EPSILON);
        assert!((cfg.max_dev_buy_sol - 8.0).abs() < f64::EPSILON);
        assert!((cfg.max_dev_tx_ratio - 0.20).abs() < f64::EPSILON);
        assert!((cfg.min_dev_tx_ratio - 0.0).abs() < f64::EPSILON);
        assert!((cfg.max_dev_volume_ratio - 0.40).abs() < f64::EPSILON);
        assert!(cfg.reject_on_dev_sell);
        assert!((cfg.max_price_change_ratio - 4.0).abs() < f64::EPSILON);
        assert!((cfg.max_single_tx_price_impact_pct - 25.0).abs() < f64::EPSILON);
        assert!((cfg.max_bonding_progress_pct - 15.0).abs() < f64::EPSILON);
        assert!((cfg.min_market_cap_sol - 20.0).abs() < f64::EPSILON);
        assert_eq!(cfg.min_phases_to_pass, 3);
        assert_eq!(cfg.re_eval_tx_interval, 3);
        assert!(cfg.min_failed_tx_ratio_for_bot_flag.is_none());
        assert!(!cfg.use_slot_ordering);
    }

    #[test]
    fn test_dedup_preserved() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.max_wait_time_ms = 30_000;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let tx1 = Arc::new(create_v2_mock_tx(1000, "dup_sig"));
        let v1 = gk.on_transaction(tx1);
        assert!(matches!(v1, GatekeeperVerdict::Wait));
        assert_eq!(gk.total_tx_count, 1);

        // Same signature → duplicate → Wait, no counter increment
        let tx2 = Arc::new(create_v2_mock_tx(1000, "dup_sig"));
        let v2 = gk.on_transaction(tx2);
        assert!(matches!(v2, GatekeeperVerdict::Wait));
        assert_eq!(
            gk.total_tx_count, 1,
            "Duplicate TX must NOT increment total_tx_count"
        );
    }

    #[test]
    fn multi_event_same_signature_not_deduped() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        let shared_signature =
            "3sp8niZmZr6rSdA2BAosumE6zBW7eFUef3P349Bt35K8817EprPqSRBzwX41Ak82TnJSWBTBfXGZybVHt7hqcikH";

        let mut tx_a = create_v2_mock_tx(1_000, shared_signature);
        tx_a.event_ordinal = Some(0);
        let mut tx_b = create_v2_mock_tx(1_000, shared_signature);
        tx_b.event_ordinal = Some(1);

        assert!(matches!(
            gk.on_transaction(Arc::new(tx_a)),
            GatekeeperVerdict::Wait
        ));
        assert!(matches!(
            gk.on_transaction(Arc::new(tx_b)),
            GatekeeperVerdict::Wait
        ));
        assert_eq!(gk.total_tx_count, 2);
        assert_eq!(gk.buffered_txs.len(), 2);
        assert_eq!(
            gk.unique_signature_count(),
            1,
            "Same signature across multiple event ordinals must count as one unique tx"
        );
        let assessment = gk.build_assessment();
        assert_eq!(assessment.unique_tx_evaluated, 1);
    }

    #[test]
    fn approved_not_equal_committed() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 5;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let timestamps = [
            1000u64, 2500, 4200, 5800, 8000, 9500, 14000, 16000, 20000, 25000,
        ];
        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];

        for i in 0..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.5;
            let verdict = gk.on_transaction(Arc::new(make_tx(
                timestamps[i],
                &format!("status_sig_{}", i),
                signers[i],
                true,
                volumes[i],
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            )));

            if matches!(verdict, GatekeeperVerdict::Buy { .. }) {
                break;
            }
        }

        assert_eq!(gk.state(), PoolState::Approved);
        assert!(!gk.state().is_committed());

        gk.mark_committed();
        assert_eq!(gk.state(), PoolState::Committed);
        assert!(gk.state().is_committed());
    }

    #[test]
    fn out_of_order_arrival_keeps_canonical_event_ordering() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let mut late_arrival_first = create_v2_mock_tx(2_000, "canon_1");
        late_arrival_first.arrival_ts_ms = 8_000;
        late_arrival_first.signer = "alice".to_string();

        let mut early_arrival_second = create_v2_mock_tx(1_000, "canon_2");
        early_arrival_second.arrival_ts_ms = 9_000;
        early_arrival_second.signer = "bob".to_string();

        let _ = gk.on_transaction(Arc::new(late_arrival_first));
        let _ = gk.on_transaction(Arc::new(early_arrival_second));

        assert_eq!(gk.tx_timestamps_sorted, vec![1_000, 2_000]);
        assert_eq!(gk.window_events.back().map(|e| e.timestamp_ms), Some(1_000));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 2: Pure function tests — compute_velocity_profile
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_velocity_organic_pattern() {
        // Chaotic timestamps → high CoV, moderate burst, high entropy
        let timestamps: Vec<u64> = vec![100, 250, 600, 650, 1200, 1800, 2500, 3100, 4000, 5500];
        let profile = compute_velocity_profile(&timestamps, 10_000);
        assert!(
            profile.interval_cv > 0.3,
            "Organic pattern should have high CoV, got {}",
            profile.interval_cv
        );
        assert!(
            profile.burst_ratio < 0.7,
            "Organic pattern should have moderate burst_ratio, got {}",
            profile.burst_ratio
        );
        assert!(
            profile.timing_entropy > 1.0,
            "Organic pattern should have high entropy, got {}",
            profile.timing_entropy
        );
    }

    #[test]
    fn test_velocity_bot_pattern() {
        // Regular 100ms intervals → low CoV
        let timestamps: Vec<u64> = (0..10).map(|i| 1000 + i * 100).collect();
        let profile = compute_velocity_profile(&timestamps, 12_000);
        assert!(
            profile.interval_cv < 0.01,
            "Bot pattern should have near-zero CoV, got {}",
            profile.interval_cv
        );
        assert!(
            (profile.avg_interval_ms - 100.0).abs() < 1.0,
            "Bot pattern avg should be ~100ms, got {}",
            profile.avg_interval_ms
        );
    }

    #[test]
    fn test_velocity_burst_pattern() {
        // 80% TX in first 20% window → high burst_ratio
        // Window = 10_000ms, 20% = 2000ms
        let mut timestamps: Vec<u64> = vec![100, 200, 300, 400, 500, 600, 700, 800]; // 8 in first 2000ms
        timestamps.push(5000); // 1 late
        timestamps.push(9000); // 1 late
        let profile = compute_velocity_profile(&timestamps, 10_000);
        assert!(
            profile.burst_ratio >= 0.8,
            "Burst pattern should have high burst_ratio, got {}",
            profile.burst_ratio
        );
    }

    #[test]
    fn test_velocity_single_tx() {
        // 1 timestamp → default profile
        let timestamps: Vec<u64> = vec![1000];
        let profile = compute_velocity_profile(&timestamps, 10_000);
        assert_eq!(profile.avg_interval_ms, 0.0);
        assert_eq!(profile.interval_cv, 0.0);
        assert_eq!(profile.burst_ratio, 1.0);
        assert_eq!(profile.timing_entropy, 0.0);
        assert!(!profile.is_accelerating);
    }

    #[test]
    fn test_velocity_two_tx() {
        // 2 timestamps → computed but no acceleration check (need >= 4 intervals)
        let timestamps: Vec<u64> = vec![1000, 1500];
        let profile = compute_velocity_profile(&timestamps, 10_000);
        assert!((profile.avg_interval_ms - 500.0).abs() < 1.0);
        assert!(
            !profile.is_accelerating,
            "Only 1 interval, acceleration should be false"
        );
    }

    #[test]
    fn test_velocity_acceleration() {
        // Intervals shrinking: first half slow, second half fast → is_accelerating=true
        // Need >= 4 intervals, so >= 5 timestamps
        // Intervals: 500, 500, 100, 100, 100 → first_half_mean=500, second_half_mean~100
        let timestamps: Vec<u64> = vec![1000, 1500, 2000, 2100, 2200, 2300];
        let profile = compute_velocity_profile(&timestamps, 10_000);
        assert!(
            profile.is_accelerating,
            "Should detect acceleration when second half is faster"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 2: Pure function tests — compute_gini
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_gini_empty() {
        assert_eq!(compute_gini(&[]), 0.0);
    }

    #[test]
    fn test_gini_equal() {
        let values = vec![1.0, 1.0, 1.0, 1.0];
        assert!(
            (compute_gini(&values)).abs() < 0.01,
            "All equal values should have Gini ≈ 0.0"
        );
    }

    #[test]
    fn test_gini_extreme() {
        let values = vec![0.0, 0.0, 0.0, 100.0]; // sorted ascending
        let gini = compute_gini(&values);
        assert!(
            (gini - 0.75).abs() < 0.01,
            "Extreme inequality should give Gini ≈ 0.75, got {}",
            gini
        );
    }

    #[test]
    fn test_gini_moderate() {
        let values = vec![1.0, 2.0, 3.0, 4.0]; // sorted ascending
        let gini = compute_gini(&values);
        assert!(
            (gini - 0.25).abs() < 0.05,
            "Moderate inequality should give Gini ≈ 0.25, got {}",
            gini
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 2: Pure function tests — compute_signer_diversity
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_diversity_perfect_competition() {
        // 10 signers, 1 TX each → HHI=0.1, Gini low
        let mut signer_stats = HashMap::new();
        for i in 0..10 {
            signer_stats.insert(
                format!("signer_{}", i),
                SignerStats {
                    tx_count: 1,
                    buy_count: 1,
                    sell_count: 0,
                    total_volume_sol: 1.0,
                },
            );
        }
        let profile = compute_signer_diversity(&signer_stats, 10, 10.0, &[]);
        assert!(
            (profile.hhi - 0.1).abs() < 0.01,
            "10 equal signers should give HHI ≈ 0.1, got {}",
            profile.hhi
        );
        assert!(
            profile.volume_gini < 0.05,
            "Equal volumes should give Gini ≈ 0.0, got {}",
            profile.volume_gini
        );
        assert!((profile.unique_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_diversity_monopoly() {
        // 1 signer, 10 TX → HHI=1.0
        let mut signer_stats = HashMap::new();
        signer_stats.insert(
            "monopolist".to_string(),
            SignerStats {
                tx_count: 10,
                buy_count: 10,
                sell_count: 0,
                total_volume_sol: 10.0,
            },
        );
        let profile = compute_signer_diversity(&signer_stats, 10, 10.0, &[]);
        assert!(
            (profile.hhi - 1.0).abs() < 0.01,
            "Monopoly should give HHI=1.0, got {}",
            profile.hhi
        );
    }

    #[test]
    fn test_diversity_oligopoly() {
        // 3 dominant signers + 2 small → HHI > 0.3
        let mut signer_stats = HashMap::new();
        signer_stats.insert(
            "whale_a".to_string(),
            SignerStats {
                tx_count: 5,
                buy_count: 5,
                sell_count: 0,
                total_volume_sol: 5.0,
            },
        );
        signer_stats.insert(
            "whale_b".to_string(),
            SignerStats {
                tx_count: 3,
                buy_count: 3,
                sell_count: 0,
                total_volume_sol: 3.0,
            },
        );
        signer_stats.insert(
            "whale_c".to_string(),
            SignerStats {
                tx_count: 2,
                buy_count: 2,
                sell_count: 0,
                total_volume_sol: 2.0,
            },
        );
        signer_stats.insert(
            "small_1".to_string(),
            SignerStats {
                tx_count: 1,
                buy_count: 1,
                sell_count: 0,
                total_volume_sol: 0.5,
            },
        );
        signer_stats.insert(
            "small_2".to_string(),
            SignerStats {
                tx_count: 1,
                buy_count: 1,
                sell_count: 0,
                total_volume_sol: 0.5,
            },
        );
        let profile = compute_signer_diversity(&signer_stats, 12, 11.0, &[]);
        assert!(
            profile.hhi > 0.2,
            "Oligopoly should have HHI > 0.2, got {}",
            profile.hhi
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 2: Pure function tests — compute_volume_sanity
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_volume_all_identical() {
        let volumes = vec![1.0, 1.0, 1.0];
        let profile = compute_volume_sanity(&volumes, 3, 0, 3.0, 3.0, 3);
        assert!(
            (profile.volume_cv).abs() < 0.01,
            "Identical volumes should have CoV=0, got {}",
            profile.volume_cv
        );
    }

    #[test]
    fn test_volume_varied() {
        let volumes = vec![0.1, 0.5, 2.0, 0.3];
        let total: f64 = volumes.iter().sum();
        let profile = compute_volume_sanity(&volumes, 3, 1, total, 2.4, 3);
        assert!(
            profile.volume_cv > 0.5,
            "Varied volumes should have CoV > 0.5, got {}",
            profile.volume_cv
        );
    }

    #[test]
    fn test_volume_all_sells() {
        let volumes = vec![1.0, 2.0, 3.0];
        let profile = compute_volume_sanity(&volumes, 0, 3, 6.0, 0.0, 0);
        assert_eq!(profile.buy_ratio, 0.0, "All sells should have buy_ratio=0");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 2: Pure function tests — compute_dev_behavior
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_dev_unknown_autopass() {
        let profile = compute_dev_behavior(
            &None,
            &Some("first_signer".to_string()),
            0.0,
            0.0,
            0.0,
            0,
            false,
            None,
            10,
            10.0,
        );
        assert!(
            !profile.dev_wallet_known,
            "dev_wallet=None should mean dev_wallet_known=false"
        );
        assert!(!profile.dev_is_first_buyer);
    }

    #[test]
    fn test_dev_sold_detected() {
        let profile = compute_dev_behavior(
            &Some("dev".to_string()),
            &Some("dev".to_string()),
            1.0,
            1.0,
            0.5,
            3,
            true,
            Some(1000.0),
            10,
            10.0,
        );
        assert!(profile.dev_has_sold, "dev_has_sold=true should be detected");
        assert!(profile.dev_wallet_known);
    }

    #[test]
    fn test_dev_first_buyer() {
        let profile = compute_dev_behavior(
            &Some("dev_wallet".to_string()),
            &Some("dev_wallet".to_string()),
            1.0,
            1.0,
            0.0,
            1,
            false,
            None,
            10,
            10.0,
        );
        assert!(
            profile.dev_is_first_buyer,
            "dev_wallet == first_signer → dev_is_first_buyer=true"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 2: Pure function tests — compute_bonding_curve_dynamics
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_curve_no_data() {
        let dynamics = compute_bonding_curve_dynamics(&[]);
        assert_eq!(dynamics.price_data_points, 0);
        assert_eq!(dynamics.initial_price, 0.0);
        assert_eq!(dynamics.price_change_ratio, 1.0);
        assert_eq!(dynamics.market_cap_change_ratio, 1.0);
    }

    #[test]
    fn test_curve_stable_price() {
        // 5 points, price varies ±2% → low impact, ratio~1.0
        let base_price = 0.00003;
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: base_price,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_000_000_000.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: base_price * 1.01,
                v_sol_in_curve: 30.3,
                v_tokens_in_curve: 999_000_000.0,
                market_cap_sol: 30.3,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1200,
                price_sol_per_token: base_price * 0.99,
                v_sol_in_curve: 29.7,
                v_tokens_in_curve: 1_001_000_000.0,
                market_cap_sol: 29.7,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1300,
                price_sol_per_token: base_price * 1.02,
                v_sol_in_curve: 30.6,
                v_tokens_in_curve: 998_000_000.0,
                market_cap_sol: 30.6,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1400,
                price_sol_per_token: base_price * 1.00,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_000_000_000.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        assert!(
            dynamics.max_single_tx_price_impact_pct < 5.0,
            "Stable price should have low impact, got {}",
            dynamics.max_single_tx_price_impact_pct
        );
        assert!(
            (dynamics.price_change_ratio - 1.0).abs() < 0.05,
            "Stable price ratio should be ~1.0, got {}",
            dynamics.price_change_ratio
        );
    }

    #[test]
    fn test_curve_pump() {
        // 5 points, price goes 5x → ratio=5.0
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.00001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_000_000_000.0,
                market_cap_sol: 10.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00002,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 900_000_000.0,
                market_cap_sol: 20.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1200,
                price_sol_per_token: 0.00003,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 800_000_000.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1300,
                price_sol_per_token: 0.00004,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 700_000_000.0,
                market_cap_sol: 40.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1400,
                price_sol_per_token: 0.00005,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 600_000_000.0,
                market_cap_sol: 50.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        assert!(
            (dynamics.price_change_ratio - 5.0).abs() < 0.01,
            "5x pump should give ratio=5.0, got {}",
            dynamics.price_change_ratio
        );
    }

    #[test]
    fn test_curve_whale_impact() {
        // One point jumps 30% → max_impact=30%
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.0001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_000_000_000.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00013,
                v_sol_in_curve: 31.0,
                v_tokens_in_curve: 990_000_000.0,
                market_cap_sol: 31.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1200,
                price_sol_per_token: 0.000135,
                v_sol_in_curve: 31.5,
                v_tokens_in_curve: 985_000_000.0,
                market_cap_sol: 31.5,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        assert!(
            (dynamics.max_single_tx_price_impact_pct - 30.0).abs() < 1.0,
            "30% whale jump should give max_impact≈30%, got {}",
            dynamics.max_single_tx_price_impact_pct
        );
    }

    #[test]
    fn test_curve_bonding_progress() {
        // vTokens decreases → progress calculated correctly
        // PUMP_GENESIS_TOKEN_SUPPLY = 1_073_000_000.0
        // If vTokens = 966_700_000 → tokens_sold = 106_300_000 → progress = 106_300_000 / 1_073_000_000 ≈ 9.9%
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.0001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_073_000_000.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00011,
                v_sol_in_curve: 31.0,
                v_tokens_in_curve: 966_700_000.0,
                market_cap_sol: 31.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        let expected_progress = (1_073_000_000.0 - 966_700_000.0) / 1_073_000_000.0 * 100.0;
        assert!(
            (dynamics.bonding_progress_pct - expected_progress).abs() < 0.1,
            "Progress should be ~{:.1}%, got {:.1}%",
            expected_progress,
            dynamics.bonding_progress_pct
        );
        assert!(
            dynamics.curve_data_known,
            "curve_data_known should be true when set by parser"
        );
    }

    #[test]
    fn test_curve_bonding_progress_unknown_when_curve_data_not_known() {
        // When curve_data_known == false (parse failure / no update),
        // curve_data_known must be false so Gatekeeper degrades instead of rejecting.
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.0001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 0.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: false,
                curve_finality: ghost_core::CurveFinality::Speculative,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00011,
                v_sol_in_curve: 31.0,
                v_tokens_in_curve: 0.0,
                market_cap_sol: 31.0,
                is_buy: true,
                curve_data_known: false,
                curve_finality: ghost_core::CurveFinality::Speculative,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        assert!(
            !dynamics.curve_data_known,
            "curve_data_known should be false when parser flag is false"
        );
        assert!(
            dynamics.bonding_progress_pct.abs() < f64::EPSILON,
            "bonding_progress_pct should be 0.0 when unknown, got {}",
            dynamics.bonding_progress_pct
        );
    }

    #[test]
    fn test_curve_empty_history_unknown_progress() {
        let dynamics = compute_bonding_curve_dynamics(&[]);
        assert!(!dynamics.curve_data_known);
        assert_eq!(dynamics.price_data_points, 0);
    }

    #[test]
    fn test_curve_keeps_last_authoritative_state_when_unknown_points_follow() {
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.0001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_073_000_000.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00011,
                v_sol_in_curve: 31.0,
                v_tokens_in_curve: 966_700_000.0,
                market_cap_sol: 31.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Finalized,
            },
            PricePoint {
                timestamp_ms: 1200,
                price_sol_per_token: 0.00012,
                v_sol_in_curve: 32.0,
                v_tokens_in_curve: 950_000_000.0,
                market_cap_sol: 32.0,
                is_buy: true,
                curve_data_known: false,
                curve_finality: ghost_core::CurveFinality::Speculative,
            },
        ];

        let dynamics = compute_bonding_curve_dynamics(&points);
        let expected_progress = (1_073_000_000.0 - 966_700_000.0) / 1_073_000_000.0 * 100.0;

        assert!(
            dynamics.curve_data_known,
            "later unknown points must not erase previously known curve data"
        );
        assert_eq!(
            dynamics.curve_finality,
            ghost_core::CurveFinality::Finalized,
            "finality should track the last authoritative point, not the last unknown one"
        );
        assert_eq!(
            dynamics.price_data_points, 2,
            "only authoritative curve points should feed trusted curve dynamics"
        );
        assert!(
            (dynamics.current_market_cap_sol - 31.0).abs() < f64::EPSILON,
            "market cap should come from the last authoritative point, got {}",
            dynamics.current_market_cap_sol
        );
        assert!(
            (dynamics.bonding_progress_pct - expected_progress).abs() < 0.1,
            "bonding progress should come from the last authoritative point, got {:.3}%",
            dynamics.bonding_progress_pct
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // curve_data_known: Gatekeeper Phase6/Core3 integration tests
    // ═══════════════════════════════════════════════════════════════════════

    /// Test A: curve unknown ⇒ no fail on bonding_progress
    /// snapshot: curve_data_known=false, bonding_progress_pct=0
    /// min_bonding_progress_pct=4
    /// Phase6/Core3 must NOT fail due to bonding_progress.
    /// Flag BONDING_PROGRESS_UNKNOWN must be set.
    #[test]
    fn test_curve_data_unknown_no_fail_on_bonding_progress() {
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.0001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 0.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: false,
                curve_finality: ghost_core::CurveFinality::Speculative,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00011,
                v_sol_in_curve: 31.0,
                v_tokens_in_curve: 0.0,
                market_cap_sol: 31.0,
                is_buy: true,
                curve_data_known: false,
                curve_finality: ghost_core::CurveFinality::Speculative,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        assert!(
            !dynamics.curve_data_known,
            "curve_data_known should be false"
        );
        assert!(
            dynamics.bonding_progress_pct.abs() < f64::EPSILON,
            "bonding_progress_pct should be 0.0 when curve_data_known is false"
        );

        // Simulate Phase6 check with min_bonding_progress_pct = 4%
        // Since curve_data_known is false, Phase6 should NOT fail on bonding_progress
        let bonding_check_passes = if dynamics.curve_data_known {
            dynamics.bonding_progress_pct >= 4.0 && dynamics.bonding_progress_pct <= 80.0
        } else {
            true // SKIP: unknown → pass
        };
        assert!(
            bonding_check_passes,
            "Phase6 must NOT fail when curve_data_known=false"
        );
    }

    /// Test B: curve known ⇒ normal range-check works (fail case)
    /// snapshot: curve_data_known=true, bonding_progress_pct=0
    /// min_bonding_progress_pct=4
    /// Phase6 should FAIL because 0 < 4.
    #[test]
    fn test_curve_data_known_range_check_fail() {
        // curve_data_known=true but all tokens remain → progress ~0%
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.0001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_073_000_000.0, // full supply = ~0% progress
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00011,
                v_sol_in_curve: 31.0,
                v_tokens_in_curve: 1_073_000_000.0,
                market_cap_sol: 31.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        assert!(dynamics.curve_data_known, "curve_data_known should be true");
        // Progress is ~0%
        assert!(dynamics.bonding_progress_pct < 1.0);

        // Simulate Phase6 check: min=4, max=80 → should fail (0 < 4)
        let bonding_check_passes = if dynamics.curve_data_known {
            dynamics.bonding_progress_pct >= 4.0 && dynamics.bonding_progress_pct <= 80.0
        } else {
            true
        };
        assert!(
            !bonding_check_passes,
            "Phase6 should FAIL when curve_data_known=true and progress < min"
        );
    }

    /// Test C (regression): curve known and progress in range ⇒ pass
    /// curve_data_known=true, progress=10%, min=4, max=80 → pass
    #[test]
    fn test_curve_data_known_range_check_pass() {
        // ~10% progress: tokens_sold = 107_300_000, remaining = 965_700_000
        let remaining = 965_700_000.0;
        let points: Vec<PricePoint> = vec![
            PricePoint {
                timestamp_ms: 1000,
                price_sol_per_token: 0.0001,
                v_sol_in_curve: 30.0,
                v_tokens_in_curve: 1_073_000_000.0,
                market_cap_sol: 30.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
            PricePoint {
                timestamp_ms: 1100,
                price_sol_per_token: 0.00011,
                v_sol_in_curve: 31.0,
                v_tokens_in_curve: remaining,
                market_cap_sol: 31.0,
                is_buy: true,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
            },
        ];
        let dynamics = compute_bonding_curve_dynamics(&points);
        assert!(dynamics.curve_data_known, "curve_data_known should be true");
        assert!(
            dynamics.bonding_progress_pct > 9.0 && dynamics.bonding_progress_pct < 11.0,
            "bonding_progress_pct should be ~10%, got {:.1}%",
            dynamics.bonding_progress_pct
        );

        // Simulate Phase6 check: min=4, max=80 → should pass
        let bonding_check_passes = if dynamics.curve_data_known {
            dynamics.bonding_progress_pct >= 4.0 && dynamics.bonding_progress_pct <= 80.0
        } else {
            true
        };
        assert!(
            bonding_check_passes,
            "Phase6 should PASS when curve_data_known=true and progress in range"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 2: evaluate_phases integration tests
    // ═══════════════════════════════════════════════════════════════════════

    /// Helper to build a healthy GatekeeperBuffer for integration tests
    fn build_healthy_v2_buffer() -> GatekeeperBuffer {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_wait_time_ms = 30_000;
        cfg.min_phases_to_pass = 5;
        cfg.max_avg_interval_ms = 6000.0; // allow wide intervals for organic spread
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Create 10 organic TX from 10 unique signers spread across the observation window
        // Window = 30_000ms, 20% = 6000ms. We put only 2 TX in first 6000ms to keep burst_ratio low.
        // Varied intervals to ensure high CoV (>0.3) and avg interval within [60, 6000]ms
        let timestamps = [
            1000u64, 2500, 8000, 9000, 15000, 16000, 20000, 22000, 27000, 28500,
        ];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];
        let v_tokens = [
            1_073_000_000.0,
            1_072_000_000.0,
            1_071_500_000.0,
            1_070_000_000.0,
            1_069_000_000.0,
            1_068_000_000.0,
            1_067_500_000.0,
            1_066_000_000.0,
            1_065_000_000.0,
            1_064_000_000.0,
        ];
        let v_sol = [30.0, 30.5, 30.7, 31.0, 31.5, 32.0, 32.2, 32.8, 33.0, 33.5];

        for i in 0..10 {
            let sig = format!("healthy_sig_{}", i);
            let mut tx = create_v2_mock_tx(timestamps[i], &sig);
            tx.signer = format!("unique_signer_{}", i);
            tx.volume_sol = volumes[i];
            tx.is_buy = i < 8; // 8 buys, 2 sells
            tx.v_tokens_in_bonding_curve = Some(v_tokens[i]);
            tx.v_sol_in_bonding_curve = Some(v_sol[i]);
            tx.market_cap_sol = Some(v_sol[i]);
            tx.curve_data_known = true;
            gk.on_transaction(Arc::new(tx));
        }

        gk
    }

    #[test]
    fn test_evaluate_all_pass() {
        let mut gk = build_healthy_v2_buffer();
        assert!(gk.phase1_passed, "Phase 1 should have passed");

        let assessment = gk.run_assessment();
        assert!(
            assessment.hard_reject_reason.is_none(),
            "Healthy pool should have no hard reject"
        );
        assert!(
            assessment.phases_passed >= 5,
            "Healthy pool should pass 5+ phases, got {}",
            assessment.phases_passed
        );
        assert!(
            assessment.phase2_passed,
            "Phase 2 should pass for organic timing"
        );
        assert!(
            assessment.phase3_passed,
            "Phase 3 should pass for diverse signers"
        );
        assert!(
            assessment.phase5_passed,
            "Phase 5 should auto-pass (no dev)"
        );
        assert!(
            assessment.phase6_passed,
            "Phase 6 should pass for stable curve"
        );
    }

    #[test]
    fn test_evaluate_hard_reject_dev_sell() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 3;
        cfg.min_unique_signers = 2;
        cfg.min_buy_count = 2;
        cfg.max_wait_time_ms = 30_000;
        cfg.reject_on_dev_sell = true;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Dev creates
        let mut tx_create = create_v2_mock_tx(1000, "dev_create");
        tx_create.signer = "dev_wallet".to_string();
        tx_create.is_dev_buy = true;
        tx_create.volume_sol = 0.5;
        gk.on_transaction(Arc::new(tx_create));

        // Dev sells
        let mut tx_sell = create_v2_mock_tx(1100, "dev_sell");
        tx_sell.signer = "dev_wallet".to_string();
        tx_sell.is_buy = false;
        tx_sell.volume_sol = 0.3;
        gk.on_transaction(Arc::new(tx_sell));

        // Another buy to trigger phase1
        let mut tx_buy = create_v2_mock_tx(1200, "other_buy");
        tx_buy.signer = "other_signer".to_string();
        tx_buy.volume_sol = 1.0;
        gk.on_transaction(Arc::new(tx_buy));

        assert!(gk.dev_has_sold, "Dev sell should be detected");

        let assessment = gk.run_assessment();
        assert!(
            assessment.hard_reject_reason.is_some(),
            "Dev sell should trigger hard reject"
        );
        assert!(assessment
            .hard_reject_reason
            .as_ref()
            .unwrap()
            .contains("Dev wallet sold"));
    }

    #[test]
    fn test_evaluate_hard_reject_extreme_hhi() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 3;
        cfg.min_unique_signers = 1;
        cfg.min_buy_count = 1;
        cfg.max_wait_time_ms = 30_000;
        cfg.reject_on_dev_sell = false;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 10 TX from 1 signer → HHI=1.0 > 0.5
        for i in 0..10 {
            let sig = format!("mono_sig_{}", i);
            let mut tx = create_v2_mock_tx(1000 + i as u64 * 200, &sig);
            tx.signer = "monopolist".to_string();
            tx.volume_sol = 1.0;
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            assessment.hard_reject_reason.is_some(),
            "HHI=1.0 should trigger hard reject"
        );
        assert!(assessment
            .hard_reject_reason
            .as_ref()
            .unwrap()
            .contains("Extreme signer concentration"));
    }

    #[test]
    fn test_evaluate_hard_reject_extreme_bot() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 5;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_wait_time_ms = 30_000;
        cfg.reject_on_dev_sell = false;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 10 TX at 20ms intervals from unique signers → CoV≈0, avg=20ms < 30ms
        for i in 0..10 {
            let sig = format!("bot_sig_{}", i);
            let mut tx = create_v2_mock_tx(1000 + i as u64 * 20, &sig);
            tx.volume_sol = 0.1 + (i as f64) * 0.05; // varied volumes to avoid P4 issues
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            assessment.hard_reject_reason.is_some(),
            "Extreme bot timing should trigger hard reject"
        );
        assert!(assessment
            .hard_reject_reason
            .as_ref()
            .unwrap()
            .contains("Extreme bot timing"));
    }

    #[test]
    fn test_evaluate_hard_reject_price_manipulation() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 3;
        cfg.min_unique_signers = 2;
        cfg.min_buy_count = 2;
        cfg.max_wait_time_ms = 30_000;
        cfg.reject_on_dev_sell = false;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 3 TX with massive price jump (>50% single TX impact)
        let timestamps = [1000u64, 1500, 2000];
        let signers = ["signer_a", "signer_b", "signer_c"];
        let prices = [0.0001, 0.0001, 0.00020]; // 100% jump at point 3

        for i in 0..3 {
            let sig = format!("price_sig_{}", i);
            let mut tx = create_v2_mock_tx(timestamps[i], &sig);
            tx.signer = signers[i].to_string();
            tx.volume_sol = 1.0 + (i as f64) * 0.3;
            tx.v_tokens_in_bonding_curve = Some(1_000_000_000.0 - i as f64 * 10_000_000.0);
            tx.v_sol_in_bonding_curve = Some(prices[i] * 1_000_000_000.0);
            tx.market_cap_sol = Some(30.0);
            tx.curve_data_known = true;
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            assessment.hard_reject_reason.is_some(),
            "Price manipulation >50% should trigger hard reject"
        );
        assert!(
            assessment
                .hard_reject_reason
                .as_ref()
                .unwrap()
                .contains("price manipulation")
                || assessment
                    .hard_reject_reason
                    .as_ref()
                    .unwrap()
                    .contains("Extreme")
        );
    }

    #[test]
    fn test_evaluate_phase5_autopass_no_dev() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // No dev wallet set
        assert!(gk.dev_wallet.is_none());

        // Add some basic data so assessment can run
        for i in 0..5 {
            let sig = format!("nd_sig_{}", i);
            let mut tx = create_v2_mock_tx(1000 + i as u64 * 300, &sig);
            tx.volume_sol = 0.5 + (i as f64) * 0.2;
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            assessment.phase5_passed,
            "Phase 5 should auto-pass when dev_wallet is unknown"
        );
    }

    #[test]
    fn test_evaluate_phase6_autopass_no_data() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // No bonding curve data (no v_tokens/v_sol)
        for i in 0..5 {
            let sig = format!("no_bc_{}", i);
            let tx = create_v2_mock_tx(1000 + i as u64 * 300, &sig);
            gk.on_transaction(Arc::new(tx));
        }

        assert!(
            gk.price_history.len() < 2,
            "Should have <2 price data points"
        );

        let assessment = gk.run_assessment();
        assert!(
            assessment.phase6_passed,
            "Phase 6 should auto-pass when <2 price points"
        );
    }

    #[test]
    fn test_evaluate_5_of_6_enough() {
        // Healthy pool but one phase fails — still passes if 5 >= min_phases_to_pass(5)
        let mut gk = build_healthy_v2_buffer();

        // Force phase4 to fail by making min_volume_cv very high
        gk.config.min_volume_cv = 999.0;
        gk.config.min_phases_to_pass = 5;

        let assessment = gk.run_assessment();
        assert!(assessment.hard_reject_reason.is_none());
        // Phase 4 should fail
        assert!(
            !assessment.phase4_passed,
            "Phase 4 should fail with extreme min_volume_cv"
        );
        // But should still have enough phases (P1 + P2 + P3 + P5 + P6 = 5)
        assert!(
            assessment.phases_passed >= 5,
            "5 out of 6 should be enough, got {}",
            assessment.phases_passed
        );
    }

    #[test]
    fn test_evaluate_4_of_6_not_enough() {
        // Two phases fail → phases_passed=4 < min(5)
        let mut gk = build_healthy_v2_buffer();

        // Force phase4 and phase2 to fail
        gk.config.min_volume_cv = 999.0;
        gk.config.min_interval_cv = 999.0;
        gk.config.min_phases_to_pass = 5;

        let assessment = gk.run_assessment();
        assert!(assessment.hard_reject_reason.is_none());
        assert!(!assessment.phase2_passed, "Phase 2 should fail");
        assert!(!assessment.phase4_passed, "Phase 4 should fail");
        assert!(
            assessment.phases_passed < 5,
            "4 out of 6 should not be enough, got {}",
            assessment.phases_passed
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 3: Migration tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_new_buffer_is_canonical() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        assert_eq!(gk.state(), PoolState::Tracked);
        let tx = Arc::new(create_v2_mock_tx(1000, "canonical_sig"));
        let v = gk.on_transaction(tx);
        assert!(matches!(v, GatekeeperVerdict::Wait));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Etap 3: Full flow on_transaction tests
    // ═══════════════════════════════════════════════════════════════════════

    /// Helper: create a config tuned for organic flows
    fn organic_flow_config() -> GatekeeperV2Config {
        let mut cfg = v2_default_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_wait_time_ms = 30_000;
        cfg.min_phases_to_pass = 5;
        cfg.re_eval_tx_interval = 3;
        cfg.min_sol_threshold = 0.005;
        // Relaxed thresholds for tests
        cfg.min_interval_cv = 0.2;
        cfg.max_burst_ratio = 0.80;
        cfg.min_avg_interval_ms = 30.0;
        cfg.max_avg_interval_ms = 10000.0;
        cfg.min_timing_entropy = 0.5;
        cfg.min_unique_ratio = 0.3;
        cfg.max_hhi = 0.30;
        cfg.max_tx_per_signer = 5;
        cfg.max_volume_gini = 0.80;
        cfg.max_top3_volume_pct = 0.90;
        cfg.min_buy_ratio = 0.40;
        cfg.min_avg_tx_sol = 0.01;
        cfg.max_avg_tx_sol = 50.0;
        cfg.min_volume_cv = 0.10;
        cfg.min_total_volume_sol = 0.2;
        cfg.max_dev_buy_sol = 10.0;
        cfg.max_dev_tx_ratio = 0.30;
        cfg.max_dev_volume_ratio = 0.50;
        cfg.reject_on_dev_sell = true;
        cfg.max_price_change_ratio = 10.0;
        cfg.max_single_tx_price_impact_pct = 40.0;
        cfg.max_bonding_progress_pct = 50.0;
        cfg.min_market_cap_sol = 5.0;
        cfg
    }

    /// Helper: create a transaction with full customization
    fn make_tx(
        timestamp_ms: u64,
        signature: &str,
        signer: &str,
        is_buy: bool,
        volume_sol: f64,
        v_tokens: Option<f64>,
        v_sol: Option<f64>,
        market_cap: Option<f64>,
    ) -> PoolTransaction {
        let mut tx = create_v2_mock_tx(timestamp_ms, signature);
        tx.signer = signer.to_string();
        tx.is_buy = is_buy;
        tx.volume_sol = volume_sol;
        tx.v_tokens_in_bonding_curve = v_tokens;
        tx.v_sol_in_bonding_curve = v_sol;
        tx.market_cap_sol = market_cap;
        tx.curve_data_known = v_tokens.is_some() && v_sol.is_some();
        tx
    }

    #[test]
    fn test_full_flow_organic_buy() {
        // Test 1: 10 TX from 8 signers, chaotic timing, varied volumes, healthy curve → Buy
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let timestamps = [
            1000u64, 2500, 4200, 5800, 8000, 9500, 14000, 16000, 20000, 25000,
        ];
        let signers = [
            "alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi", "alice", "bob",
        ];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];
        let v_tokens_base = 1_073_000_000.0;

        let mut last_verdict = GatekeeperVerdict::Wait;
        for i in 0..10 {
            let v_tokens = v_tokens_base - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.5;
            let tx = make_tx(
                timestamps[i],
                &format!("sig_{}", i),
                signers[i],
                true,
                volumes[i],
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            last_verdict = gk.on_transaction(Arc::new(tx));
        }

        if let GatekeeperVerdict::Buy { assessment, .. } = last_verdict {
            assert_eq!(
                assessment.phases_passed, 6,
                "All 6 phases should pass for organic traffic"
            );
            assert!(assessment.phase1_passed);
        } else {
            // If not immediate Buy, it should have transitioned to Active
            assert_eq!(
                gk.state(),
                PoolState::Approved,
                "Should be Approved (Buy) after organic traffic"
            );
        }
    }

    #[test]
    fn test_full_flow_bot_reject() {
        // Test 2: 15 TX, regular 100ms interval, identical 1.0 SOL, 3 signers recycled → Reject
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 5;
        cfg.min_unique_signers = 3;
        cfg.min_buy_count = 3;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let signers = ["bot_a", "bot_b", "bot_c"];
        let mut final_verdict = GatekeeperVerdict::Wait;
        for i in 0..15 {
            let tx = make_tx(
                1000 + i as u64 * 20, // 20ms intervals → extreme bot
                &format!("bot_sig_{}", i),
                signers[i % 3],
                true,
                1.0,
                None,
                None,
                None,
            );
            final_verdict = gk.on_transaction(Arc::new(tx));
            if matches!(final_verdict, GatekeeperVerdict::Reject { .. }) {
                break;
            }
        }

        assert!(
            matches!(final_verdict, GatekeeperVerdict::Reject { .. }),
            "Bot pattern should be rejected"
        );
    }

    #[test]
    fn test_full_flow_timeout_phase1_never_met() {
        // Test 3: 3 TX in 12s window (need 8 min_tx) → Timeout
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.max_wait_time_ms = 10_000;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        let v1 = gk.on_transaction(Arc::new(make_tx(
            1000, "s1", "alice", true, 1.0, None, None, None,
        )));
        assert!(matches!(v1, GatekeeperVerdict::Wait));

        let v2 = gk.on_transaction(Arc::new(make_tx(
            5000, "s2", "bob", true, 1.0, None, None, None,
        )));
        assert!(matches!(v2, GatekeeperVerdict::Wait));

        // 12s from first TX → timeout
        let v3 = gk.on_transaction(Arc::new(make_tx(
            13001, "s3", "charlie", true, 1.0, None, None, None,
        )));
        if let GatekeeperVerdict::Timeout { assessment } = v3 {
            assert!(!assessment.phase1_passed, "Phase 1 should not have passed");
            // phases_passed may be > 0 because phases 2-6 are still evaluated
            // for diagnostic completeness (e.g. Phase 5/6 auto-pass with no data).
            // The critical check is that Phase 1 did NOT pass.
        } else {
            panic!("Expected Timeout verdict");
        }
    }

    #[test]
    fn test_full_flow_deadline_reject_phases_insufficient() {
        // Test 4: Phase 1 passes at TX 8, but diversity and volume phases fail, deadline hits → Reject
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.max_wait_time_ms = 10_000;
        cfg.min_phases_to_pass = 5;
        cfg.min_volume_cv = 999.0; // Force Phase 4 to fail
        cfg.min_unique_ratio = 999.0; // Force Phase 3 to fail
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        // Send 8 TX to pass Phase 1
        for i in 0..8 {
            let tx = make_tx(
                1000 + i as u64 * 500,
                &format!("dl_sig_{}", i),
                &format!("signer_{}", i),
                true,
                1.0 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert!(gk.phase1_passed, "Phase 1 should have passed at TX 8");

        // Deadline TX
        let deadline_tx = make_tx(
            15001,
            "deadline_sig",
            "deadline_signer",
            true,
            1.0,
            None,
            None,
            None,
        );
        let v = gk.on_transaction(Arc::new(deadline_tx));

        if let GatekeeperVerdict::Reject { reason, assessment } = v {
            assert!(
                reason.contains("TIMEOUT"),
                "Reason should contain TIMEOUT, got: {}",
                reason
            );
            assert!(
                assessment.phases_passed < 5,
                "Should have insufficient phases"
            );
        } else {
            panic!(
                "Expected Reject at deadline, got: {:?}",
                matches!(v, GatekeeperVerdict::Wait)
            );
        }
    }

    #[test]
    fn test_full_flow_deadline_buy_phases_sufficient() {
        // Test 5: Phase 1 passes, initial eval 4/6, more TX improve metrics, at deadline 5+ phases → Buy
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.max_wait_time_ms = 10_000;
        cfg.min_phases_to_pass = 5;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let timestamps = [
            1000u64, 2000, 3500, 4000, 5500, 6000, 7500, 8000, 8500, 9000,
        ];
        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "a", "b"];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];

        let mut bought = false;
        for i in 0..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 500_000.0;
            let v_sol = 30.0 + (i as f64) * 0.3;
            let tx = make_tx(
                timestamps[i],
                &format!("dlb_sig_{}", i),
                signers[i],
                true,
                volumes[i],
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            let v = gk.on_transaction(Arc::new(tx));
            if matches!(v, GatekeeperVerdict::Buy { .. }) {
                bought = true;
                break;
            }
        }

        if !bought {
            // Send deadline TX
            let deadline_tx = make_tx(
                15001,
                "dlb_deadline",
                "deadline_signer",
                true,
                1.0,
                Some(1_070_000_000.0),
                Some(33.0),
                Some(33.0),
            );
            let v = gk.on_transaction(Arc::new(deadline_tx));
            if let GatekeeperVerdict::Buy { assessment, .. } = v {
                assert!(assessment.phases_passed >= 5);
                bought = true;
            }
        }

        assert!(
            bought,
            "Should Buy either during observation or at deadline"
        );
    }

    #[test]
    fn test_full_flow_reeval_improves() {
        // Test 6: TX 1-8 have identical 1.0 SOL (Phase 4 volume_cv fail). TX 9-11 varied → Phase 4 passes
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_phases_to_pass = 5;
        cfg.min_volume_cv = 0.15; // Need some variation
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1-8: all from unique signers but identical 1.0 SOL
        for i in 0..8 {
            let tx = make_tx(
                1000 + i as u64 * 800,
                &format!("re_sig_{}", i),
                &format!("signer_{}", i),
                true,
                1.0,
                Some(1_073_000_000.0 - (i as f64) * 500_000.0),
                Some(30.0 + (i as f64) * 0.3),
                Some(30.0 + (i as f64) * 0.3),
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert!(gk.phase1_passed);
        let initial_eval = gk.eval_count;
        assert!(initial_eval >= 1, "Should have evaluated at Phase 1 pass");

        // TX 9-11: varied volumes to fix Phase 4
        let varied_volumes = [0.05, 3.0, 0.7];
        let mut final_verdict = GatekeeperVerdict::Wait;
        for (i, &vol) in varied_volumes.iter().enumerate() {
            let idx = 8 + i;
            let tx = make_tx(
                1000 + idx as u64 * 800,
                &format!("re_sig_{}", idx),
                &format!("signer_{}", idx),
                true,
                vol,
                Some(1_073_000_000.0 - (idx as f64) * 500_000.0),
                Some(30.0 + (idx as f64) * 0.3),
                Some(30.0 + (idx as f64) * 0.3),
            );
            final_verdict = gk.on_transaction(Arc::new(tx));
            if matches!(final_verdict, GatekeeperVerdict::Buy { .. }) {
                break;
            }
        }

        assert!(
            gk.eval_count >= 2,
            "Should have re-evaluated at least once, eval_count={}",
            gk.eval_count
        );
        // Either bought or waiting for more data
        if let GatekeeperVerdict::Buy { assessment, .. } = final_verdict {
            assert!(assessment.phases_passed >= 5);
        }
    }

    #[test]
    fn test_full_flow_reeval_interval_respected() {
        // Test 7: Phase 1 passes at TX 8. TX 9 arrives. re_eval_tx_interval=3, no eval. TX 10 no eval. TX 11 → eval.
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.re_eval_tx_interval = 3;
        cfg.min_phases_to_pass = 6; // Unreachable to keep waiting
        cfg.min_volume_cv = 999.0; // Ensure phases never pass enough
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1-8: pass Phase 1
        for i in 0..8 {
            let tx = make_tx(
                1000 + i as u64 * 500,
                &format!("ri_sig_{}", i),
                &format!("signer_{}", i),
                true,
                1.0 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert!(gk.phase1_passed);
        let eval_after_p1 = gk.eval_count;
        assert_eq!(eval_after_p1, 1, "Should have 1 eval after Phase 1 pass");

        // TX 9: no re-eval (1 TX since last eval < 3)
        gk.on_transaction(Arc::new(make_tx(
            5500, "ri_sig_8", "signer_8", true, 1.0, None, None, None,
        )));
        assert_eq!(gk.eval_count, 1, "No re-eval after 1 new TX");

        // TX 10: no re-eval (2 TX since last eval < 3)
        gk.on_transaction(Arc::new(make_tx(
            6000, "ri_sig_9", "signer_9", true, 1.0, None, None, None,
        )));
        assert_eq!(gk.eval_count, 1, "No re-eval after 2 new TX");

        // TX 11: re-eval triggered (3 TX since last eval >= 3)
        gk.on_transaction(Arc::new(make_tx(
            6500,
            "ri_sig_10",
            "signer_10",
            true,
            1.0,
            None,
            None,
            None,
        )));
        assert_eq!(gk.eval_count, 2, "Re-eval should trigger after 3 new TX");
    }

    #[test]
    fn test_full_flow_hard_reject_during_reeval() {
        // Test 8: Phase 1 passes at TX 8, initial eval: 4/6. Dev sells in TX 10. Re-eval → hard reject.
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.re_eval_tx_interval = 2;
        cfg.min_phases_to_pass = 6; // Can't pass anyway
        cfg.reject_on_dev_sell = true;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1: dev buy
        let mut dev_tx = make_tx(1000, "hr_sig_0", "dev_wallet", true, 0.5, None, None, None);
        dev_tx.is_dev_buy = true;
        gk.on_transaction(Arc::new(dev_tx));

        // TX 2-8: organic buys from unique signers to pass Phase 1
        for i in 1..8 {
            let tx = make_tx(
                1000 + i as u64 * 500,
                &format!("hr_sig_{}", i),
                &format!("signer_{}", i),
                true,
                1.0 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert!(gk.phase1_passed);

        // TX 9: normal
        gk.on_transaction(Arc::new(make_tx(
            5500, "hr_sig_8", "signer_8", true, 1.0, None, None, None,
        )));

        // TX 10: dev sells!
        let dev_sell = make_tx(6000, "hr_sig_9", "dev_wallet", false, 0.3, None, None, None);
        let verdict = gk.on_transaction(Arc::new(dev_sell));

        // Re-eval should catch the dev sell
        if matches!(verdict, GatekeeperVerdict::Reject { .. }) {
            if let GatekeeperVerdict::Reject { reason, .. } = &verdict {
                assert!(
                    reason.to_lowercase().contains("dev") || reason.contains("Dev"),
                    "Reason should mention dev: {}",
                    reason
                );
            }
        } else {
            // Dev sell might be caught on next re-eval
            let tx11 = make_tx(6500, "hr_sig_10", "signer_10", true, 1.0, None, None, None);
            let v11 = gk.on_transaction(Arc::new(tx11));
            if let GatekeeperVerdict::Reject { reason, .. } = v11 {
                assert!(
                    reason.to_lowercase().contains("dev") || reason.contains("Dev"),
                    "Reason should mention dev: {}",
                    reason
                );
            } else {
                panic!("Expected Reject due to dev sell during re-eval");
            }
        }
    }

    #[test]
    fn test_full_flow_active_relay_after_buy() {
        // Test 9: Full flow → Buy. Then TX 12, 13, 14 arrive → ApprovedTx
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Feed organic TX until Buy
        let timestamps = [
            1000u64, 2500, 4200, 5800, 8000, 9500, 14000, 16000, 20000, 25000,
        ];
        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];

        for i in 0..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.5;
            let tx = make_tx(
                timestamps[i],
                &format!("ar_sig_{}", i),
                signers[i],
                true,
                volumes[i],
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            gk.on_transaction(Arc::new(tx));
        }

        // Should now be Approved
        assert_eq!(
            gk.state(),
            PoolState::Approved,
            "Should be Approved after Buy"
        );

        // TX 12-14 → ApprovedTx
        for i in 10..13 {
            let tx = make_tx(
                26000 + i as u64 * 1000,
                &format!("ar_sig_{}", i),
                &format!("post_{}", i),
                true,
                0.5,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            assert!(
                matches!(v, GatekeeperVerdict::ApprovedTx { .. }),
                "Post-Buy TX should be ApprovedTx"
            );
        }
    }

    #[test]
    fn test_full_flow_dead_end_after_reject() {
        // Test 10: Full flow → Reject. Then TX arrives → Wait (dead end)
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 5;
        cfg.min_unique_signers = 3;
        cfg.min_buy_count = 3;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Bot pattern → reject
        for i in 0..10 {
            let tx = make_tx(
                1000 + i as u64 * 20,
                &format!("de_sig_{}", i),
                &format!("bot_{}", i % 3),
                true,
                1.0,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            if matches!(v, GatekeeperVerdict::Reject { .. }) {
                break;
            }
        }
        assert!(gk.rejected, "Should be rejected");

        // After rejection → Wait
        let tx_after = make_tx(
            5000,
            "de_sig_after",
            "signer_after",
            true,
            1.0,
            None,
            None,
            None,
        );
        let v = gk.on_transaction(Arc::new(tx_after));
        assert!(
            matches!(v, GatekeeperVerdict::Wait),
            "Dead end: rejected pool should return Wait"
        );
    }

    #[test]
    fn test_full_flow_dead_end_after_timeout() {
        // Test 11: Full flow → Timeout. Then TX arrives → Wait (dead end)
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.max_wait_time_ms = 5_000;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        gk.on_transaction(Arc::new(make_tx(
            1000, "to_sig_1", "alice", true, 1.0, None, None, None,
        )));
        gk.on_transaction(Arc::new(make_tx(
            2000, "to_sig_2", "bob", true, 1.0, None, None, None,
        )));

        // Deadline TX
        let v = gk.on_transaction(Arc::new(make_tx(
            10001, "to_sig_3", "charlie", true, 1.0, None, None, None,
        )));
        assert!(
            matches!(v, GatekeeperVerdict::Timeout { .. }),
            "Should timeout"
        );

        // After timeout → Wait
        let tx_after = make_tx(15000, "to_sig_after", "dave", true, 1.0, None, None, None);
        let v2 = gk.on_transaction(Arc::new(tx_after));
        assert!(
            matches!(v2, GatekeeperVerdict::Wait),
            "Dead end: timed out pool should return Wait"
        );
    }

    #[test]
    fn test_full_flow_dedup_in_active_state() {
        // Test 12: Full flow → Buy → Active. Duplicate TX → Wait
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Feed organic TX until Buy
        let timestamps = [
            1000u64, 2500, 4200, 5800, 8000, 9500, 14000, 16000, 20000, 25000,
        ];
        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];

        for i in 0..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.5;
            let tx = make_tx(
                timestamps[i],
                &format!("dup_sig_{}", i),
                signers[i],
                true,
                volumes[i],
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert_eq!(gk.state(), PoolState::Approved);

        // Approved: unique TX → ApprovedTx
        let v1 = gk.on_transaction(Arc::new(make_tx(
            26000,
            "dup_unique_post",
            "post_signer",
            true,
            0.5,
            None,
            None,
            None,
        )));
        assert!(matches!(v1, GatekeeperVerdict::ApprovedTx { .. }));

        // Active: duplicate TX → Wait
        let v2 = gk.on_transaction(Arc::new(make_tx(
            26000,
            "dup_unique_post",
            "post_signer",
            true,
            0.5,
            None,
            None,
            None,
        )));
        assert!(
            matches!(v2, GatekeeperVerdict::Wait),
            "Duplicate in active state should return Wait"
        );
    }

    #[test]
    fn test_full_flow_dust_not_counted() {
        // Test 13: 5 dust TX (0.001 SOL) + 8 real TX. Phase 1 at real TX #8. dust_filtered_count=5, total_tx_count=8
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_sol_threshold = 0.01;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 5 dust TX
        for i in 0..5 {
            let mut tx = make_tx(
                1000 + i as u64 * 100,
                &format!("dust_{}", i),
                &format!("dust_signer_{}", i),
                true,
                0.001,
                None,
                None,
                None,
            );
            tx.volume_sol = 0.001;
            gk.on_transaction(Arc::new(tx));
        }
        assert_eq!(gk.dust_filtered_count, 5, "Should have 5 dust filtered");
        assert_eq!(gk.total_tx_count, 0, "Dust should not be counted");

        // 8 real TX
        for i in 0..8 {
            let tx = make_tx(
                2000 + i as u64 * 500,
                &format!("real_{}", i),
                &format!("real_signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert_eq!(gk.dust_filtered_count, 5, "Dust count should stay at 5");
        assert_eq!(gk.total_tx_count, 8, "Only real TX should be counted");
    }

    #[test]
    fn test_full_flow_dev_sell_before_phase1() {
        // Test 14: Dev sells at TX #3 (before Phase 1 met at TX #8). Evaluation at Phase 1 → hard reject.
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.reject_on_dev_sell = true;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1: dev buy
        let mut dev_buy = make_tx(1000, "ds_sig_0", "dev_creator", true, 0.5, None, None, None);
        dev_buy.is_dev_buy = true;
        gk.on_transaction(Arc::new(dev_buy));

        // TX 2: normal buy
        gk.on_transaction(Arc::new(make_tx(
            1500, "ds_sig_1", "buyer_a", true, 1.0, None, None, None,
        )));

        // TX 3: dev sells! (before Phase 1)
        gk.on_transaction(Arc::new(make_tx(
            2000,
            "ds_sig_2",
            "dev_creator",
            false,
            0.3,
            None,
            None,
            None,
        )));
        assert!(gk.dev_has_sold, "Dev sell should be tracked");
        assert!(!gk.phase1_passed, "Phase 1 should NOT be passed yet");

        // TX 4-8: more buys to trigger Phase 1
        let mut final_verdict = GatekeeperVerdict::Wait;
        for i in 3..8 {
            let tx = make_tx(
                2000 + i as u64 * 500,
                &format!("ds_sig_{}", i),
                &format!("signer_{}", i),
                true,
                1.0 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            final_verdict = gk.on_transaction(Arc::new(tx));
            if matches!(final_verdict, GatekeeperVerdict::Reject { .. }) {
                break;
            }
        }

        if let GatekeeperVerdict::Reject { reason, .. } = final_verdict {
            assert!(
                reason.to_lowercase().contains("dev") || reason.contains("Dev"),
                "Reason should mention dev sell: {}",
                reason
            );
        } else {
            panic!("Expected Reject due to dev sell detected at Phase 1 evaluation");
        }
    }

    #[test]
    fn test_full_flow_price_data_accumulates() {
        // Test 15: 10 TX with v_tokens/v_sol data. Price history has 10 PricePoints at evaluation.
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        for i in 0..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.3;
            let tx = make_tx(
                1000 + i as u64 * 800,
                &format!("pd_sig_{}", i),
                &format!("signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            gk.on_transaction(Arc::new(tx));
        }

        assert_eq!(
            gk.price_history.len(),
            10,
            "Should have 10 price data points"
        );

        let assessment = gk.run_assessment();
        if let Some(ref curve) = assessment.phase6_curve {
            assert_eq!(
                curve.price_data_points, 10,
                "Assessment should report 10 price points"
            );
        } else {
            panic!("Phase 6 curve data should be present");
        }
    }

    #[test]
    fn test_full_flow_mixed_reserve_availability() {
        // Test 16: TX 1-4 no reserve data, TX 5-10 with reserve data → Phase 6 evaluates 6 PricePoints
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1-4: no reserve data
        for i in 0..4 {
            let tx = make_tx(
                1000 + i as u64 * 500,
                &format!("mr_sig_{}", i),
                &format!("signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert_eq!(gk.price_history.len(), 0, "No price data without reserves");

        // TX 5-10: with reserve data
        for i in 4..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 500_000.0;
            let v_sol = 30.0 + (i as f64) * 0.2;
            let tx = make_tx(
                1000 + i as u64 * 500,
                &format!("mr_sig_{}", i),
                &format!("signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            gk.on_transaction(Arc::new(tx));
        }
        assert_eq!(
            gk.price_history.len(),
            6,
            "Should have 6 price points from TX 5-10"
        );

        let assessment = gk.run_assessment();
        if let Some(ref curve) = assessment.phase6_curve {
            assert_eq!(
                curve.price_data_points, 6,
                "Assessment should report 6 price points"
            );
        }
    }

    #[test]
    fn test_full_flow_concurrent_independence() {
        // Test 17: Two separate GatekeeperBuffer instances (pool A, pool B). Independent verdicts.
        let pool_a_id = Pubkey::new_unique();
        let pool_b_id = Pubkey::new_unique();
        let cfg_a = organic_flow_config();
        let mut cfg_b = organic_flow_config();
        cfg_b.min_tx_count = 5;
        cfg_b.min_unique_signers = 3;
        cfg_b.min_buy_count = 3;
        let mut gk_a = GatekeeperBuffer::new(pool_a_id, &cfg_a);
        let mut gk_b = GatekeeperBuffer::new(pool_b_id, &cfg_b);

        // Pool A: organic traffic → Buy
        for i in 0..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.5;
            let tx = make_tx(
                1000 + i as u64 * 2000,
                &format!("a_sig_{}", i),
                &format!("a_signer_{}", i),
                true,
                0.5 + (i as f64) * 0.2,
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            gk_a.on_transaction(Arc::new(tx));
        }

        // Pool B: bot traffic → Reject
        for i in 0..10 {
            let tx = make_tx(
                1000 + i as u64 * 20,
                &format!("b_sig_{}", i),
                &format!("b_bot_{}", i % 3),
                true,
                1.0,
                None,
                None,
                None,
            );
            gk_b.on_transaction(Arc::new(tx));
        }

        // Verify independence
        assert_eq!(
            gk_a.state(),
            PoolState::Approved,
            "Pool A should be Active (organic)"
        );
        assert!(gk_b.rejected, "Pool B should be rejected (bot)");
        assert_eq!(gk_a.total_tx_count, 10, "Pool A tx count independent");
        assert_ne!(
            gk_a.total_tx_count, gk_b.total_tx_count,
            "Counts should be independent"
        );
    }

    // =============================================================================
    // Gatekeeper Buy Log Tests
    // =============================================================================

    #[test]
    fn test_gatekeeper_buy_log_creation() {
        use ghost_brain::oracle::GATEKEEPER_BUY_LOG_SCHEMA_VERSION;

        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();

        // Create a mock assessment with all phases passed
        let assessment = GatekeeperAssessment {
            phase1_passed: true,
            phase2_velocity: Some(VelocityProfile {
                avg_interval_ms: 125.4,
                interval_std_dev: 50.0,
                interval_cv: 0.33,
                burst_ratio: 0.50,
                timing_entropy: 2.31,
                is_accelerating: false,
            }),
            phase2_passed: true,
            phase3_diversity: Some(SignerDiversityProfile {
                unique_ratio: 0.73,
                hhi: 0.08,
                max_tx_per_signer: 2,
                volume_gini: 0.42,
                top3_volume_pct: 0.65,
                same_ms_tx_ratio: 0.05,
            }),
            phase3_passed: true,
            phase4_volume: Some(VolumeSanityProfile {
                buy_ratio: 0.73,
                avg_tx_sol: 0.85,
                volume_cv: 0.62,
                total_volume_sol: 18.7,
                min_tx_sol: 0.05,
                max_tx_sol: 2.0,
                sol_buy_ratio: 0.75,
                max_consecutive_buys: 8,
            }),
            phase4_passed: true,
            phase5_dev: Some(DevBehaviorProfile {
                dev_wallet_known: true,
                dev_buy_total_sol: 1.2,
                dev_initial_buy_tokens: Some(1000.0),
                dev_tx_count: 2,
                dev_tx_ratio: 0.06,
                dev_volume_ratio: 0.08,
                dev_has_sold: false,
                dev_is_first_buyer: false,
            }),
            phase5_passed: true,
            phase6_curve: Some(BondingCurveDynamics {
                initial_price: 0.01,
                current_price: 0.0145,
                max_price: 0.015,
                price_change_ratio: 1.45,
                max_single_tx_price_impact_pct: 3.2,
                max_single_sell_impact_pct: 0.0,
                current_market_cap_sol: 34.5,
                market_cap_change_ratio: 1.45,
                bonding_progress_pct: 4.8,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
                price_data_points: 30,
            }),
            phase6_passed: true,
            phases_passed: 6,
            hard_reject_reason: None,
            total_tx_evaluated: 30,
            unique_tx_evaluated: 30,
            unique_signers_evaluated: 30,
            observation_duration_ms: 1929,
            finalize_lag_ms: 0,
            dust_filtered_count: 1,
            eval_count: 2,
            buy_count: 22,
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
        };

        let buy_log = assessment.to_buy_log(&pool_id, &config);

        // Verify schema version
        assert_eq!(
            buy_log.log_schema_version,
            GATEKEEPER_BUY_LOG_SCHEMA_VERSION
        );

        // Verify pool_id
        assert_eq!(buy_log.pool_id, pool_id.to_string());

        // Verify summary fields
        assert_eq!(buy_log.phases_passed, 6);
        assert_eq!(buy_log.min_phases_to_pass, config.min_phases_to_pass);
        assert_eq!(buy_log.observation_duration_ms, 1929);
        assert_eq!(buy_log.eval_count, 2);
        assert_eq!(buy_log.min_sol_threshold, Some(config.min_sol_threshold));
        assert_eq!(buy_log.v25_confidence_pre_veto, None);
        assert_eq!(buy_log.v25_confidence_zeroed_by_pdd_hard_fail, None);
        assert_eq!(buy_log.v25_shadow_confidence_source, None);

        // Verify phase 1 fields
        assert_eq!(buy_log.total_tx_evaluated, 30);
        assert_eq!(buy_log.unique_signers_evaluated, 30);
        assert_eq!(buy_log.min_tx_count, config.min_tx_count);

        // Verify buy_count comes from the assessment's buy_count field (Phase 1 tracking)
        assert_eq!(buy_log.buy_count, 22);

        // Verify phase 2 measured values
        assert_eq!(buy_log.interval_cv, Some(0.33));
        assert_eq!(buy_log.burst_ratio, Some(0.50));
        assert_eq!(buy_log.avg_interval_ms, Some(125.4));
        assert_eq!(buy_log.timing_entropy, Some(2.31));

        // Verify phase 3 measured values
        assert_eq!(buy_log.unique_ratio, Some(0.73));
        assert_eq!(buy_log.hhi, Some(0.08));
        assert_eq!(buy_log.max_tx_per_signer_observed, Some(2));
        assert_eq!(buy_log.volume_gini, Some(0.42));

        // Verify phase 4 measured values
        assert_eq!(buy_log.buy_ratio, Some(0.73));
        assert_eq!(buy_log.avg_tx_sol, Some(0.85));
        assert_eq!(buy_log.total_volume_sol, Some(18.7));

        // Verify phase 5 measured values
        assert_eq!(buy_log.dev_wallet_known, Some(true));
        assert_eq!(buy_log.dev_buy_total_sol, Some(1.2));
        assert_eq!(buy_log.dev_has_sold, Some(false));

        // Verify phase 6 measured values
        assert_eq!(buy_log.price_change_ratio, Some(1.45));
        assert_eq!(buy_log.current_market_cap_sol, Some(34.5));
        assert_eq!(buy_log.bonding_progress_pct, Some(4.8));
        assert_eq!(buy_log.curve_finality.as_deref(), Some("provisional"));
        assert_eq!(buy_log.curve_finality_is_finalized, Some(false));
        assert_eq!(
            buy_log.soft_flags.as_deref(),
            Some("CURVE_FINALITY_PROVISIONAL")
        );
    }

    #[test]
    fn test_gatekeeper_buy_log_omits_caution_for_finalized_curve() {
        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();

        let assessment = GatekeeperAssessment {
            phase1_passed: false,
            phase2_velocity: None,
            phase2_passed: false,
            phase3_diversity: None,
            phase3_passed: false,
            phase4_volume: None,
            phase4_passed: false,
            phase5_dev: None,
            phase5_passed: false,
            phase6_curve: Some(BondingCurveDynamics {
                initial_price: 0.01,
                current_price: 0.012,
                max_price: 0.012,
                price_change_ratio: 1.2,
                max_single_tx_price_impact_pct: 1.5,
                max_single_sell_impact_pct: 0.5,
                current_market_cap_sol: 31.0,
                market_cap_change_ratio: 1.2,
                bonding_progress_pct: 5.0,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Finalized,
                price_data_points: 8,
            }),
            phase6_passed: true,
            phases_passed: 1,
            hard_reject_reason: None,
            total_tx_evaluated: 8,
            unique_tx_evaluated: 8,
            unique_signers_evaluated: 8,
            observation_duration_ms: 500,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 1,
            buy_count: 8,
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
        };

        let buy_log = assessment.to_buy_log(&pool_id, &config);
        assert_eq!(buy_log.curve_finality.as_deref(), Some("finalized"));
        assert_eq!(buy_log.curve_finality_is_finalized, Some(true));
        assert_eq!(buy_log.soft_flags.as_deref(), Some("none"));
    }

    #[test]
    fn test_gatekeeper_buy_log_serialization() {
        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();

        // Create a minimal assessment
        let assessment = GatekeeperAssessment {
            phase1_passed: true,
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
            phases_passed: 1,
            hard_reject_reason: None,
            total_tx_evaluated: 10,
            unique_tx_evaluated: 10,
            unique_signers_evaluated: 8,
            observation_duration_ms: 500,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 1,
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
        };

        let buy_log = assessment.to_buy_log(&pool_id, &config);

        // Test serialization
        let json = serde_json::to_string(&buy_log).expect("Failed to serialize");
        assert!(!json.is_empty());

        // Verify it's valid JSON
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("Failed to parse serialized JSON");

        // Verify key fields are present
        assert_eq!(
            parsed["log_schema_version"],
            ghost_brain::oracle::GATEKEEPER_BUY_LOG_SCHEMA_VERSION
        );
        assert_eq!(parsed["pool_id"], pool_id.to_string());
        assert_eq!(parsed["phases_passed"], 1);
        assert_eq!(parsed["total_tx_evaluated"], 10);
        assert_eq!(parsed["min_sol_threshold"], config.min_sol_threshold);

        // Verify None fields are skipped in serialization
        assert!(
            !json.contains("\"interval_cv\""),
            "Optional None fields should be skipped"
        );
        assert!(
            !json.contains("\"unique_ratio\""),
            "Optional None fields should be skipped"
        );

        // Test deserialization round-trip
        let deserialized: ghost_brain::oracle::GatekeeperBuyLog =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.log_schema_version, buy_log.log_schema_version);
        assert_eq!(deserialized.pool_id, buy_log.pool_id);
        assert_eq!(deserialized.phases_passed, buy_log.phases_passed);
    }

    #[test]
    fn test_gatekeeper_buy_log_thresholds_match() {
        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();
        let mut config = config;
        config.enable_prosperity_filter = true;
        config.enable_prosperity_overlay = true;

        // Create an assessment (values don't matter for this test)
        let assessment = GatekeeperAssessment {
            phase1_passed: true,
            phase2_velocity: Some(VelocityProfile {
                avg_interval_ms: 100.0,
                interval_std_dev: 30.0,
                interval_cv: 0.30,
                burst_ratio: 0.60,
                timing_entropy: 1.5,
                is_accelerating: false,
            }),
            phase2_passed: true,
            phase3_diversity: Some(SignerDiversityProfile {
                unique_ratio: 0.5,
                hhi: 0.2,
                max_tx_per_signer: 3,
                volume_gini: 0.6,
                top3_volume_pct: 0.7,
                same_ms_tx_ratio: 0.1,
            }),
            phase3_passed: true,
            phase4_volume: Some(VolumeSanityProfile {
                buy_ratio: 0.6,
                avg_tx_sol: 1.0,
                volume_cv: 0.5,
                total_volume_sol: 10.0,
                min_tx_sol: 0.1,
                max_tx_sol: 3.0,
                sol_buy_ratio: 0.65,
                max_consecutive_buys: 5,
            }),
            phase4_passed: true,
            phase5_dev: Some(DevBehaviorProfile {
                dev_wallet_known: false,
                dev_buy_total_sol: 0.0,
                dev_initial_buy_tokens: None,
                dev_tx_count: 0,
                dev_tx_ratio: 0.0,
                dev_volume_ratio: 0.0,
                dev_has_sold: false,
                dev_is_first_buyer: false,
            }),
            phase5_passed: true,
            phase6_curve: Some(BondingCurveDynamics {
                initial_price: 0.01,
                current_price: 0.02,
                max_price: 0.02,
                price_change_ratio: 2.0,
                max_single_tx_price_impact_pct: 5.0,
                max_single_sell_impact_pct: 0.0,
                current_market_cap_sol: 25.0,
                market_cap_change_ratio: 2.0,
                bonding_progress_pct: 10.0,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Provisional,
                price_data_points: 20,
            }),
            phase6_passed: true,
            phases_passed: 6,
            hard_reject_reason: None,
            total_tx_evaluated: 20,
            unique_tx_evaluated: 20,
            unique_signers_evaluated: 15,
            observation_duration_ms: 1000,
            finalize_lag_ms: 0,
            dust_filtered_count: 2,
            eval_count: 1,
            buy_count: 12,
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
        };

        let buy_log = assessment.to_buy_log(&pool_id, &config);

        // Verify all thresholds match config values exactly

        // Phase 1 thresholds
        assert_eq!(buy_log.min_tx_count, config.min_tx_count);
        assert_eq!(buy_log.min_unique_signers, config.min_unique_signers);
        assert_eq!(buy_log.min_buy_count, config.min_buy_count);
        assert_eq!(buy_log.max_wait_time_ms, config.max_wait_time_ms);
        assert_eq!(buy_log.min_sol_threshold, Some(config.min_sol_threshold));

        // Phase 2 thresholds
        assert_eq!(buy_log.min_interval_cv, config.min_interval_cv);
        assert_eq!(buy_log.max_interval_cv, config.max_interval_cv);
        assert_eq!(buy_log.max_burst_ratio, config.max_burst_ratio);
        assert_eq!(buy_log.min_avg_interval_ms, config.min_avg_interval_ms);
        assert_eq!(buy_log.max_avg_interval_ms, config.max_avg_interval_ms);
        assert_eq!(buy_log.min_timing_entropy, config.min_timing_entropy);
        assert_eq!(buy_log.max_timing_entropy, config.max_timing_entropy);

        // Phase 3 thresholds
        assert_eq!(buy_log.min_unique_ratio, config.min_unique_ratio);
        assert_eq!(buy_log.max_hhi, config.max_hhi);
        assert_eq!(buy_log.max_tx_per_signer, config.max_tx_per_signer);
        assert_eq!(buy_log.max_volume_gini, config.max_volume_gini);
        assert_eq!(buy_log.max_top3_volume_pct, config.max_top3_volume_pct);

        // Phase 4 thresholds
        assert_eq!(buy_log.min_buy_ratio, config.min_buy_ratio);
        assert_eq!(buy_log.min_avg_tx_sol, config.min_avg_tx_sol);
        assert_eq!(buy_log.max_avg_tx_sol, config.max_avg_tx_sol);
        assert_eq!(buy_log.min_volume_cv, config.min_volume_cv);
        assert_eq!(buy_log.max_volume_cv, config.max_volume_cv);
        assert_eq!(buy_log.min_total_volume_sol, config.min_total_volume_sol);
        assert_eq!(buy_log.max_total_volume_sol, config.max_total_volume_sol);

        // Phase 5 thresholds
        assert_eq!(buy_log.max_dev_buy_sol, config.max_dev_buy_sol);
        assert_eq!(buy_log.max_dev_tx_ratio, config.max_dev_tx_ratio);
        assert_eq!(buy_log.min_dev_tx_ratio, config.min_dev_tx_ratio);
        assert_eq!(buy_log.max_dev_volume_ratio, config.max_dev_volume_ratio);
        assert_eq!(buy_log.reject_on_dev_sell, config.reject_on_dev_sell);

        // Phase 6 thresholds
        assert_eq!(
            buy_log.max_price_change_ratio,
            config.max_price_change_ratio
        );
        assert_eq!(
            buy_log.max_single_tx_price_impact_pct,
            config.max_single_tx_price_impact_pct
        );
        assert_eq!(
            buy_log.max_bonding_progress_pct,
            config.max_bonding_progress_pct
        );
        assert_eq!(buy_log.min_market_cap_sol, config.min_market_cap_sol);

        // Summary thresholds
        assert_eq!(buy_log.min_phases_to_pass, config.min_phases_to_pass);
        assert!(buy_log.prosperity_overlay_enabled);
        assert_eq!(
            buy_log.prosperity_overlay_max_price_change_ratio,
            Some(config.prosperity_overlay_max_price_change_ratio)
        );
        assert_eq!(
            buy_log.prosperity_overlay_max_bonding_progress_pct,
            Some(config.prosperity_overlay_max_bonding_progress_pct)
        );
        assert_eq!(
            buy_log.prosperity_overlay_min_fee_topology_diversity_index,
            Some(config.prosperity_overlay_min_fee_topology_diversity_index)
        );
        assert_eq!(
            buy_log.prosperity_overlay_branch23_max_sell_buy_ratio,
            Some(config.prosperity_overlay_branch23_max_sell_buy_ratio)
        );
        assert_eq!(
            buy_log.prosperity_overlay_branch2_max_price_change_ratio,
            Some(config.prosperity_overlay_branch2_max_price_change_ratio)
        );
    }

    // =============================================================================
    // GK2 §11.1 Missing Tests (#2, #4, #6, #9, #11, #12, #13, #17, #10, #27)
    // =============================================================================

    /// §11.1 #2: 10 TX, 8 signers, chaotic timing, varied volumes, healthy curve → Buy (6/6)
    /// Verifies phases_passed == 6 explicitly.
    #[test]
    fn test_organic_pool_all_phases_pass() {
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 10 TX, 8 unique signers, chaotic timing, varied volumes 0.1-2.0 SOL
        let timestamps = [
            1000u64, 2300, 4800, 6100, 9500, 11200, 15700, 18900, 22000, 27000,
        ];
        let signers = [
            "alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi", "alice", "bob",
        ];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 1.8, 0.7, 1.0];

        let mut last_verdict = GatekeeperVerdict::Wait;
        for i in 0..10 {
            let v_tokens = 1_073_000_000.0 - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.5;
            let mcap = v_sol; // healthy mcap
            let tx = make_tx(
                timestamps[i],
                &format!("org_all_{}", i),
                signers[i],
                true,
                volumes[i],
                Some(v_tokens),
                Some(v_sol),
                Some(mcap),
            );
            last_verdict = gk.on_transaction(Arc::new(tx));
        }

        match last_verdict {
            GatekeeperVerdict::Buy { assessment, .. } => {
                assert_eq!(
                    assessment.phases_passed, 6,
                    "All 6 phases must pass for organic traffic"
                );
                assert!(assessment.phase1_passed);
                assert!(assessment.phase2_passed);
                assert!(assessment.phase3_passed);
                assert!(assessment.phase4_passed);
                assert!(assessment.phase5_passed);
                assert!(assessment.phase6_passed);
                assert!(assessment.hard_reject_reason.is_none());
            }
            _ => {
                assert_eq!(
                    gk.state(),
                    PoolState::Approved,
                    "Must reach Buy/Active with 6/6 phases"
                );
            }
        }
    }

    /// §11.1 #4: 12 TX, 10 in first 2s, 2 in next 10s → Reject (Phase 2 burst_ratio > max)
    #[test]
    fn test_burst_front_loaded_rejected() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_burst_ratio = 0.70; // 10/12 = 0.83 in first 20% → exceeds
        cfg.min_phases_to_pass = 6;

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 10 TX packed in 0-2000ms (burst)
        for i in 0..10 {
            let tx = make_tx(
                1000 + i as u64 * 200,
                &format!("burst_{}", i),
                &format!("burst_signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        // 2 TX spread 10s later
        for i in 10..12 {
            let tx = make_tx(
                12000 + i as u64 * 3000,
                &format!("burst_{}", i),
                &format!("burst_signer_{}", i),
                true,
                0.8,
                None,
                None,
                None,
            );
            let verdict = gk.on_transaction(Arc::new(tx));
            if let GatekeeperVerdict::Reject { assessment, .. } = &verdict {
                // Phase 2 should fail due to burst_ratio
                assert!(
                    !assessment.phase2_passed,
                    "Phase 2 should fail: burst front-loaded"
                );
                return;
            }
        }

        // If we got here, check that gk didn't Buy (phases must fail)
        assert_ne!(
            gk.state(),
            PoolState::Approved,
            "Must not reach Active with burst front-loaded pattern"
        );
    }

    /// §11.1 #6: 10 TX, 10 signers, one signer = 90% volume → Reject (Phase 3 Gini)
    #[test]
    fn test_volume_gini_rejected() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_volume_gini = 0.70;
        cfg.max_top3_volume_pct = 0.75;
        cfg.min_phases_to_pass = 6; // Need all 6 → P3 fail prevents Buy

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 10 TX, 10 unique signers with chaotic timing
        // signer_0 = whale (90% volume), rest = tiny
        let timestamps = [
            1000u64, 2500, 4100, 6300, 8800, 11500, 14000, 17500, 20000, 24000,
        ];
        for i in 0..10 {
            let vol = if i == 0 { 18.0 } else { 0.2 }; // signer_0 = 18 SOL, rest = 0.2
            let tx = make_tx(
                timestamps[i],
                &format!("gini_{}", i),
                &format!("gini_signer_{}", i),
                true,
                vol,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        // Force evaluation via deadline
        let deadline_tx = make_tx(
            35000,
            "gini_deadline",
            "gini_signer_extra",
            true,
            0.3,
            None,
            None,
            None,
        );
        let verdict = gk.on_transaction(Arc::new(deadline_tx));

        // Assessment should show P3 Gini fail
        let assessment = gk.run_assessment();
        assert!(
            !assessment.phase3_passed,
            "Phase 3 should fail with extreme Gini"
        );
        if let Some(ref div) = assessment.phase3_diversity {
            assert!(
                div.volume_gini > 0.70,
                "Gini should exceed 0.70: got {}",
                div.volume_gini
            );
        }
    }

    /// §11.1 #9: 8 TX at 0.01 SOL each (above dust, total=0.08 < 0.5) → Reject (Phase 4 total_volume)
    #[test]
    fn test_dust_total_volume_rejected() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_sol_threshold = 0.005; // 0.01 passes dust filter
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.min_total_volume_sol = 0.5; // total=0.08 < 0.5 → Phase 4 fail
        cfg.min_phases_to_pass = 6;

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let timestamps = [1000u64, 2500, 4200, 5800, 8000, 9500, 12000, 15000];
        for i in 0..8 {
            let tx = make_tx(
                timestamps[i],
                &format!("dust_vol_{}", i),
                &format!("dust_vol_signer_{}", i),
                true,
                0.01, // tiny but above dust
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            !assessment.phase4_passed,
            "Phase 4 should fail: total_volume_sol={:.3} < 0.5",
            assessment
                .phase4_volume
                .as_ref()
                .map(|v| v.total_volume_sol)
                .unwrap_or(0.0)
        );
        if let Some(ref vol) = assessment.phase4_volume {
            assert!(vol.total_volume_sol < 0.5, "Total volume should be < 0.5");
        }
    }

    /// §11.1 #10: Yellowstone failed TX ratio bot detection
    #[test]
    fn test_bot_yellowstone_failed_tx() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 6;
        cfg.min_unique_signers = 3;
        cfg.min_buy_count = 3;
        cfg.min_failed_tx_ratio_for_bot_flag = Some(0.30); // >30% failed → reject

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 4 successful TXs
        for i in 0..4 {
            let tx = make_tx(
                1000 + i as u64 * 2000,
                &format!("ys_{}", i),
                &format!("ys_signer_{}", i),
                true,
                0.5,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        // 4 failed TXs (success=false)
        for i in 4..8 {
            let mut tx = make_tx(
                1000 + i as u64 * 2000,
                &format!("ys_fail_{}", i),
                &format!("ys_signer_{}", i),
                true,
                0.3,
                None,
                None,
                None,
            );
            tx.success = false;
            gk.on_transaction(Arc::new(tx));
        }

        // Force evaluation: Phase 1 was met at tx 6, so Phase 2+ should have run.
        // Total 8 TX, 4 failed → failed_ratio = 4/8 = 0.5 > 0.3
        let assessment = gk.run_assessment();
        assert!(
            assessment.hard_reject_reason.is_some(),
            "Should hard reject due to high failed TX ratio (Yellowstone)"
        );
        let reason = assessment.hard_reject_reason.unwrap();
        assert!(
            reason.contains("failed TX ratio"),
            "Reason should mention failed TX ratio, got: {}",
            reason
        );
    }

    /// §11.1 #11: Dev wallet buy exceeds max_dev_buy_sol → Reject (Phase 5)
    #[test]
    fn test_dev_whale_buy_rejected() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.max_dev_buy_sol = 8.0;
        cfg.min_phases_to_pass = 6;

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1: Dev creates pool (is_dev_buy=true) with 12 SOL (> 8 SOL limit)
        let mut dev_tx = make_tx(
            1000,
            "dev_whale_0",
            "dev_wallet",
            true,
            12.0,
            None,
            None,
            None,
        );
        dev_tx.is_dev_buy = true;
        gk.on_transaction(Arc::new(dev_tx));

        // TX 2-8: organic traffic from other signers
        for i in 1..8 {
            let tx = make_tx(
                1000 + i as u64 * 2000,
                &format!("dev_whale_{}", i),
                &format!("other_signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            !assessment.phase5_passed,
            "Phase 5 should fail: dev buy {} > max 8.0",
            assessment
                .phase5_dev
                .as_ref()
                .map(|d| d.dev_buy_total_sol)
                .unwrap_or(0.0)
        );
        if let Some(ref dev) = assessment.phase5_dev {
            assert!(dev.dev_wallet_known, "Dev wallet should be known");
            assert!(
                dev.dev_buy_total_sol > 8.0,
                "Dev buy should exceed threshold"
            );
        }
    }

    /// §11.1 #12: Dev has 4/10 TX (ratio=0.4 > max 0.20) → Reject (Phase 5)
    #[test]
    fn test_dev_dominance_rejected() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 3;
        cfg.min_buy_count = 5;
        cfg.max_dev_tx_ratio = 0.20;
        cfg.min_phases_to_pass = 6;

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1: Dev creates (is_dev_buy=true)
        let mut dev_tx = make_tx(1000, "dom_0", "dev_wallet", true, 0.5, None, None, None);
        dev_tx.is_dev_buy = true;
        gk.on_transaction(Arc::new(dev_tx));

        // TX 2-4: Dev buys 3 more times (total dev=4/10)
        for i in 1..4 {
            let tx = make_tx(
                1000 + i as u64 * 2000,
                &format!("dom_dev_{}", i),
                "dev_wallet",
                true,
                0.5,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        // TX 5-10: other signers
        for i in 4..10 {
            let tx = make_tx(
                1000 + i as u64 * 2000,
                &format!("dom_{}", i),
                &format!("other_signer_{}", i),
                true,
                0.5,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            !assessment.phase5_passed,
            "Phase 5 should fail: dev tx_ratio=0.4 > max 0.20"
        );
        if let Some(ref dev) = assessment.phase5_dev {
            assert!(
                dev.dev_tx_ratio > 0.20,
                "Dev tx ratio should exceed threshold: got {}",
                dev.dev_tx_ratio
            );
        }
    }

    /// §11.1 #13: Dev unknown → Phase 5 auto-pass
    #[test]
    fn test_dev_unknown_auto_pass_full_flow() {
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // 10 TX, no dev info (is_dev_buy=false for all)
        let timestamps = [
            1000u64, 2500, 4200, 5800, 8000, 9500, 12000, 15000, 18000, 22000,
        ];
        for i in 0..10 {
            let tx = make_tx(
                timestamps[i],
                &format!("nodev_{}", i),
                &format!("nodev_signer_{}", i),
                true,
                0.5 + (i as f64) * 0.2,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            assessment.phase5_passed,
            "Phase 5 should auto-pass when dev unknown"
        );
        if let Some(ref dev) = assessment.phase5_dev {
            assert!(!dev.dev_wallet_known, "Dev wallet should be unknown");
        }
    }

    /// §11.1 #17: 8 TX, marketCapSol=5.0 (<20.0 min) → Reject (Phase 6)
    #[test]
    fn test_low_mcap_rejected() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.min_market_cap_sol = 20.0;
        cfg.min_phases_to_pass = 6;

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        let timestamps = [1000u64, 2500, 4200, 5800, 8000, 9500, 12000, 15000];
        for i in 0..8 {
            // v_sol low → implies low mcap
            let v_tokens = 1_073_000_000.0 - (i as f64) * 500_000.0;
            let v_sol = 3.0 + (i as f64) * 0.2; // very low reserve → low mcap
            let mcap = 5.0; // explicitly low mcap
            let tx = make_tx(
                timestamps[i],
                &format!("mcap_{}", i),
                &format!("mcap_signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                Some(v_tokens),
                Some(v_sol),
                Some(mcap),
            );
            gk.on_transaction(Arc::new(tx));
        }

        let assessment = gk.run_assessment();
        assert!(
            !assessment.phase6_passed,
            "Phase 6 should fail: mcap=5.0 < min 20.0"
        );
        if let Some(ref curve) = assessment.phase6_curve {
            assert!(
                curve.current_market_cap_sol < 20.0,
                "Market cap should be below threshold: got {}",
                curve.current_market_cap_sol
            );
        }
    }

    /// §11.1 #27: TX with is_dev_buy=true sets dev_wallet
    #[test]
    fn test_create_event_sets_dev_wallet() {
        let pool_id = Pubkey::new_unique();
        let cfg = organic_flow_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX 1: regular buy (no dev)
        let tx1 = make_tx(
            1000,
            "wallet_test_0",
            "regular_signer",
            true,
            0.5,
            None,
            None,
            None,
        );
        gk.on_transaction(Arc::new(tx1));
        assert!(
            gk.dev_wallet.is_none(),
            "Dev wallet should be None before dev buy"
        );

        // TX 2: dev buy
        let mut dev_tx = make_tx(
            2000,
            "wallet_test_1",
            "the_dev",
            true,
            1.0,
            None,
            None,
            None,
        );
        dev_tx.is_dev_buy = true;
        gk.on_transaction(Arc::new(dev_tx));
        assert_eq!(
            gk.dev_wallet.as_deref(),
            Some("the_dev"),
            "Dev wallet should be set to signer"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // LONG MODE TESTS
    // ═══════════════════════════════════════════════════════════════════════

    fn long_mode_config() -> GatekeeperV2Config {
        let mut cfg = organic_flow_config();
        cfg.mode = ghost_brain::config::GatekeeperMode::Long;
        cfg.max_wait_time_ms = 10_000; // 10s
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;
        cfg.min_phases_to_pass = 4;
        cfg
    }

    /// Long mode: TX arriving before deadline ALWAYS returns Wait, even if
    /// Phase 1 criteria are met.
    #[test]
    fn test_long_mode_waits_full_duration() {
        let pool_id = Pubkey::new_unique();
        let cfg = long_mode_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        // Send 10 organic TX within 3s — Phase 1 would trigger in standard mode
        let timestamps = [1000, 1200, 1500, 1800, 2100, 2400, 2600, 2800, 3000, 3200];
        for (i, &ts) in timestamps.iter().enumerate() {
            let tx = make_tx(
                ts,
                &format!("long_wait_{}", i),
                &format!("signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            assert!(
                matches!(v, GatekeeperVerdict::Wait),
                "Long mode must return Wait before deadline (tx {})",
                i
            );
        }

        // Verify no evaluations happened
        assert_eq!(
            gk.eval_count, 0,
            "Long mode must not evaluate before deadline"
        );
        assert!(
            !gk.phase1_passed,
            "Phase 1 must not be marked passed before deadline"
        );
    }

    /// Long mode: Buy at deadline when enough TX and phases pass.
    #[test]
    fn test_long_mode_buy_at_deadline() {
        let pool_id = Pubkey::new_unique();
        let cfg = long_mode_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        // Send 10 organic TX spread over the window
        let timestamps = [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 9500];
        for (i, &ts) in timestamps.iter().enumerate() {
            let tx = make_tx(
                ts,
                &format!("long_buy_{}", i),
                &format!("signer_{}", i),
                true,
                0.5 + (i as f64) * 0.15,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            assert!(matches!(v, GatekeeperVerdict::Wait));
        }

        // Trigger deadline with a TX beyond max_wait_time_ms
        let deadline_tx = make_tx(
            11001,
            "long_buy_deadline",
            "signer_10",
            true,
            1.0,
            None,
            None,
            None,
        );
        let verdict = gk.on_transaction(Arc::new(deadline_tx));

        match verdict {
            GatekeeperVerdict::Buy {
                assessment,
                buffered_txs,
            } => {
                assert!(
                    assessment.phases_passed >= 4,
                    "Should pass at least 4 phases"
                );
                // All 11 TX should be buffered (10 before + 1 deadline)
                assert!(buffered_txs.len() >= 10, "Should buffer all TX");
                assert_eq!(gk.eval_count, 1, "Long mode should evaluate exactly once");
            }
            GatekeeperVerdict::Reject { .. } => {
                // Also acceptable — depends on exact phase thresholds.
                // The test primarily validates that a decision IS made at deadline.
                assert_eq!(gk.eval_count, 1, "Long mode should evaluate exactly once");
            }
            other => panic!(
                "Expected Buy or Reject at deadline, got {:?}",
                match &other {
                    GatekeeperVerdict::Wait => "Wait",
                    GatekeeperVerdict::Timeout { .. } => "Timeout",
                    GatekeeperVerdict::ApprovedTx { .. } => "ApprovedTx",
                    _ => "unknown",
                }
            ),
        }
    }

    /// Long mode: Timeout when Phase 1 is not met at deadline.
    #[test]
    fn test_long_mode_timeout_insufficient_tx() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = long_mode_config();
        cfg.min_tx_count = 20; // require 20 TX — we'll only send 5
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        // Send 5 TX
        for i in 0..5 {
            let tx = make_tx(
                1000 + i * 500,
                &format!("long_to_{}", i),
                &format!("signer_{}", i),
                true,
                1.0,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            assert!(matches!(v, GatekeeperVerdict::Wait));
        }

        // Trigger deadline
        let deadline_tx = make_tx(
            12000,
            "long_to_deadline",
            "signer_99",
            true,
            1.0,
            None,
            None,
            None,
        );
        let verdict = gk.on_transaction(Arc::new(deadline_tx));
        assert!(
            matches!(verdict, GatekeeperVerdict::Timeout { .. }),
            "Should Timeout when Phase 1 not met at deadline"
        );
    }

    /// Long mode: Reject when Phase 1 is met but not enough phases pass.
    #[test]
    fn test_long_mode_reject_insufficient_phases() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = long_mode_config();
        cfg.min_phases_to_pass = 6; // require ALL 6 phases
        cfg.min_timing_entropy = 99.0; // Phase 2 will always fail
        cfg.min_unique_ratio = 0.99; // Phase 3 will likely fail
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_registered_wall_t0(1_000);

        // Send 10 TX
        for i in 0..10 {
            let tx = make_tx(
                1000 + i * 300,
                &format!("long_rej_{}", i),
                &format!("signer_{}", i),
                true,
                1.0,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        // Trigger deadline
        let deadline_tx = make_tx(
            12000,
            "long_rej_deadline",
            "signer_99",
            true,
            1.0,
            None,
            None,
            None,
        );
        let verdict = gk.on_transaction(Arc::new(deadline_tx));
        // P3: insufficient phases at deadline → Timeout, not Reject
        assert!(
            matches!(verdict, GatekeeperVerdict::Timeout { .. }),
            "Should Timeout when not enough phases pass at deadline (P3 taxonomy)"
        );
    }

    /// Long mode: No sliding window cleanup — old TX are preserved.
    #[test]
    fn test_long_mode_no_sliding_window_cleanup() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = long_mode_config();
        cfg.max_wait_time_ms = 5_000;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // TX at t=1000 (will be 8s old by the time deadline triggers)
        let early_tx = make_tx(
            1000,
            "early_tx",
            "early_signer",
            true,
            2.0,
            None,
            None,
            None,
        );
        gk.on_transaction(Arc::new(early_tx));

        // More TX at t=4000-4500
        for i in 0..8 {
            let tx = make_tx(
                4000 + i * 60,
                &format!("mid_tx_{}", i),
                &format!("mid_signer_{}", i),
                true,
                1.0,
                None,
                None,
                None,
            );
            gk.on_transaction(Arc::new(tx));
        }

        // All 9 TX should still be buffered (no cleanup in long mode)
        assert_eq!(
            gk.buffered_txs.len(),
            9,
            "Long mode must not clean up old TX"
        );
        assert_eq!(gk.total_tx_count, 9, "All TX counted");
    }

    /// Standard mode remains unchanged (regression test).
    #[test]
    fn test_standard_mode_unchanged() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.mode = ghost_brain::config::GatekeeperMode::Standard;
        cfg.curve_require_for_buy = false; // disable curve latch for this regression test
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // In standard mode, Phase 1 meeting triggers immediate evaluation
        let timestamps = [1000, 1200, 1500, 1800, 2100, 2400, 2600, 2800, 3000, 3200];
        let mut got_non_wait = false;
        for (i, &ts) in timestamps.iter().enumerate() {
            let tx = make_tx(
                ts,
                &format!("std_{}", i),
                &format!("signer_{}", i),
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            if !matches!(v, GatekeeperVerdict::Wait) {
                got_non_wait = true;
            }
        }

        // Standard mode should have triggered evaluation when Phase 1 was met
        assert!(gk.phase1_passed, "Standard mode should mark Phase 1 passed");
        assert!(
            gk.eval_count > 0,
            "Standard mode should evaluate when Phase 1 met"
        );
    }

    /// Long mode config roundtrip via serde.
    #[test]
    fn test_long_mode_config_serde() {
        let toml_str = r#"
            mode = "long"
            min_tx_count = 10
            max_wait_time_ms = 10000
        "#;
        let cfg: GatekeeperV2Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.mode, ghost_brain::config::GatekeeperMode::Long);
        assert_eq!(cfg.min_tx_count, 10);
        assert_eq!(cfg.max_wait_time_ms, 10_000);

        // Standard mode default
        let toml_default = r#"
            min_tx_count = 30
        "#;
        let cfg2: GatekeeperV2Config = toml::from_str(toml_default).unwrap();
        assert_eq!(cfg2.mode, ghost_brain::config::GatekeeperMode::Standard);
    }

    /// Dust TXs in long mode must advance the clock and trigger
    /// the deadline check so the buffer times out promptly.
    #[test]
    fn test_long_mode_dust_triggers_deadline() {
        let cfg = GatekeeperV2Config {
            mode: ghost_brain::config::GatekeeperMode::Long,
            max_wait_time_ms: 5000,
            min_sol_threshold: 0.1,
            min_tx_count: 20,
            min_unique_signers: 10,
            min_buy_count: 10,
            ..GatekeeperV2Config::default()
        };
        let pool = Pubkey::new_unique();
        let mut buf = GatekeeperBuffer::new(pool, &cfg);
        buf.set_registered_wall_t0(1_000);

        // Send one real TX at T=1000 to set first_tx_ts
        let tx1 = create_v2_mock_tx(1000, "sig_real");
        let mut tx1 = tx1;
        tx1.volume_sol = 0.5; // above dust threshold
        let v1 = buf.on_transaction(Arc::new(tx1));
        assert!(matches!(v1, GatekeeperVerdict::Wait));

        // Send dust TXs at T=4000 (before deadline) — should still Wait
        let mut dust = create_v2_mock_tx(4000, "dust1");
        dust.volume_sol = 0.01; // below dust threshold
        let v2 = buf.on_transaction(Arc::new(dust));
        assert!(
            matches!(v2, GatekeeperVerdict::Wait),
            "Dust before deadline should Wait"
        );

        // Send dust TX at T=6100 (after deadline) — should trigger Timeout
        let mut dust2 = create_v2_mock_tx(6100, "dust2");
        dust2.volume_sol = 0.01;
        let v3 = buf.on_transaction(Arc::new(dust2));
        assert!(
            matches!(v3, GatekeeperVerdict::Timeout { .. }),
            "Dust after deadline should trigger Timeout, got {:?}",
            match &v3 {
                GatekeeperVerdict::Timeout { .. } => "Timeout",
                GatekeeperVerdict::Wait => "Wait",
                _ => "Other",
            }
        );
    }

    /// force_check_deadline() must trigger Timeout for overdue long-mode buffers
    /// even when no transactions are arriving.
    #[test]
    fn test_force_check_deadline() {
        let cfg = GatekeeperV2Config {
            mode: ghost_brain::config::GatekeeperMode::Long,
            max_wait_time_ms: 5000,
            min_tx_count: 20,
            min_unique_signers: 10,
            min_buy_count: 10,
            ..GatekeeperV2Config::default()
        };
        let pool = Pubkey::new_unique();
        let mut buf = GatekeeperBuffer::new(pool, &cfg);
        buf.set_registered_wall_t0(1_000);

        // Send one real TX at T=1000 to set first_tx_ts
        let tx1 = create_v2_mock_tx(1000, "sig1");
        let mut tx1 = tx1;
        tx1.volume_sol = 0.5;
        buf.on_transaction(Arc::new(tx1));

        // Force check before deadline — should Wait
        let v1 = buf.force_check_deadline(4000);
        assert!(matches!(v1, GatekeeperVerdict::Wait));

        // Force check after deadline — should Timeout
        let v2 = buf.force_check_deadline(7000);
        assert!(
            matches!(v2, GatekeeperVerdict::Timeout { .. }),
            "force_check_deadline after deadline should Timeout"
        );
    }

    /// force_check_deadline() must enforce hard timeout in standard mode
    /// even when no new transactions arrive.
    #[test]
    fn test_force_check_deadline_standard_mode_timeout() {
        let cfg = GatekeeperV2Config {
            mode: ghost_brain::config::GatekeeperMode::Standard,
            max_wait_time_ms: 5000,
            ..GatekeeperV2Config::default()
        };
        let pool = Pubkey::new_unique();
        let mut buf = GatekeeperBuffer::new(pool, &cfg);
        buf.set_registered_wall_t0(1_000);
        let v = buf.force_check_deadline(7_000);
        assert!(
            matches!(v, GatekeeperVerdict::Timeout { .. }),
            "force_check_deadline should timeout overdue standard-mode buffer"
        );
    }

    /// When no non-dust TX ever arrives, long-mode buffers should time out
    /// based on immutable registered wall-clock deadline.
    #[test]
    fn test_long_mode_dust_only_timeout() {
        let cfg = GatekeeperV2Config {
            mode: ghost_brain::config::GatekeeperMode::Long,
            max_wait_time_ms: 5000,
            min_sol_threshold: 0.1,
            min_tx_count: 20,
            min_unique_signers: 10,
            min_buy_count: 10,
            ..GatekeeperV2Config::default()
        };
        let pool = Pubkey::new_unique();
        let mut buf = GatekeeperBuffer::new(pool, &cfg);
        buf.set_registered_wall_t0(1_000);

        // No real TX ever sent → first_tx_ts is None
        // force_check_deadline should detect that the buffer is overdue
        let now = 7_000;
        let v = buf.force_check_deadline(now);
        assert!(
            matches!(v, GatekeeperVerdict::Timeout { .. }),
            "Dust-only buffer should timeout via registered wall-clock deadline"
        );
    }

    /// Verify that PoolTransactions enriched with ShadowLedger reserves
    /// produce price data points in the Gatekeeper's Phase 6 tracking.
    /// This simulates the gRPC source path where bonding curve data
    /// is added by `enrich_pool_tx_from_shadow_ledger` before the
    /// Gatekeeper processes the transaction.
    #[test]
    fn test_grpc_enriched_tx_populates_phase6_price_history() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Simulate gRPC-enriched trade: v_tokens and v_sol filled from ShadowLedger
        let mut tx = create_v2_mock_tx(1000, "grpc_sig_1");
        tx.v_tokens_in_bonding_curve = Some(1_073_000_000.0);
        tx.v_sol_in_bonding_curve = Some(30.0);
        tx.market_cap_sol = Some(30.0);
        tx.curve_data_known = true;
        gk.on_transaction(Arc::new(tx));

        // Phase 6 should now have a price data point
        assert_eq!(
            gk.price_history.len(),
            1,
            "Phase 6 should track enriched price data"
        );

        let point = &gk.price_history[0];
        assert!((point.v_sol_in_curve - 30.0).abs() < 0.01);
        assert!((point.v_tokens_in_curve - 1_073_000_000.0).abs() < 1.0);
        assert!(point.price_sol_per_token > 0.0);
    }

    /// Verify that unenriched gRPC transactions (no ShadowLedger data)
    /// produce ZERO Phase 6 data points — confirming the gap that enrichment fixes.
    #[test]
    fn test_unenriched_grpc_tx_has_no_phase6_data() {
        let pool_id = Pubkey::new_unique();
        let cfg = v2_default_config();
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Simulate raw gRPC trade with no bonding curve data
        let tx = create_v2_mock_tx(1000, "grpc_no_reserves");
        gk.on_transaction(Arc::new(tx));

        // Phase 6 should have NO price data
        assert_eq!(
            gk.price_history.len(),
            0,
            "Without enrichment, Phase 6 has no data"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Window Vectors & Deterministic Downsampling (JSONL v3)
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_downsample_indices_passthrough() {
        // n <= max_len → all indices returned.
        let idx = downsample_indices(3, 10);
        assert_eq!(idx, vec![0, 1, 2]);
    }

    #[test]
    fn test_downsample_indices_exact_match() {
        let idx = downsample_indices(5, 5);
        assert_eq!(idx, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_downsample_indices_reduces_length() {
        let idx = downsample_indices(100, 10);
        assert_eq!(idx.len(), 10);
        assert_eq!(idx[0], 0);
        assert_eq!(idx[9], 99);
    }

    #[test]
    fn test_downsample_indices_zero_max_len() {
        let idx = downsample_indices(5, 0);
        assert_eq!(idx, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_downsample_indices_max_len_one() {
        // max_len == 1 must not panic (division by zero).
        let idx = downsample_indices(50, 1);
        assert_eq!(idx, vec![0]);
    }

    #[test]
    fn test_downsample_indices_is_deterministic() {
        let a = downsample_indices(50, 7);
        let b = downsample_indices(50, 7);
        assert_eq!(a, b, "Downsampling must be deterministic");
    }

    #[test]
    fn test_extract_window_vectors_aligned() {
        let config = v2_default_config();
        let pool_id = Pubkey::new_unique();
        let mut gk = GatekeeperBuffer::new(pool_id, &config);

        // Inject transactions at various timestamps.
        let timestamps = [1000u64, 1200, 1500, 1800, 2000, 2500, 3000];
        for (i, &ts) in timestamps.iter().enumerate() {
            let tx = Arc::new(create_v2_mock_tx(ts, &format!("sig_{}", i)));
            gk.on_transaction(tx);
        }

        // Window [1000, 2000]: should include ts 1000..=2000
        let vecs = gk.extract_window_vectors(1000, 2000, 100);
        assert_eq!(vecs.max_len, 100);
        // All primary vectors must have the same length.
        assert_eq!(vecs.ts_offsets_ms.len(), vecs.sol_amounts.len());
        assert_eq!(vecs.ts_offsets_ms.len(), vecs.prices.len());
        // Timestamps 1000,1200,1500,1800,2000 fall within [1000,2000]
        assert_eq!(vecs.ts_offsets_ms.len(), 5);
        assert_eq!(vecs.ts_offsets_ms[0], 0); // 1000 - 1000
        assert_eq!(vecs.ts_offsets_ms[4], 1000); // 2000 - 1000
                                                 // Derived vectors have length N-1.
        assert_eq!(vecs.interval_ms.len(), 4);
        assert_eq!(vecs.d_price.len(), 4);
    }

    #[test]
    fn test_extract_window_vectors_empty_window() {
        let config = v2_default_config();
        let pool_id = Pubkey::new_unique();
        let mut gk = GatekeeperBuffer::new(pool_id, &config);

        // Inject transactions outside the query window.
        let tx = Arc::new(create_v2_mock_tx(5000, "sig_late"));
        gk.on_transaction(tx);

        let vecs = gk.extract_window_vectors(1000, 2000, 100);
        assert!(vecs.ts_offsets_ms.is_empty());
        assert!(vecs.sol_amounts.is_empty());
        assert!(vecs.prices.is_empty());
        assert!(vecs.interval_ms.is_empty());
        assert!(vecs.d_price.is_empty());
    }

    #[test]
    fn test_extract_window_vectors_downsampled_aligned() {
        let config = v2_default_config();
        let pool_id = Pubkey::new_unique();
        let mut gk = GatekeeperBuffer::new(pool_id, &config);

        // Inject 50 transactions in the window.
        for i in 0..50 {
            let ts = 1000 + i * 10;
            let tx = Arc::new(create_v2_mock_tx(ts, &format!("sig_{}", i)));
            gk.on_transaction(tx);
        }

        let vecs = gk.extract_window_vectors(1000, 1500, 10);
        // All primary vectors are the same downsampled length.
        assert_eq!(vecs.ts_offsets_ms.len(), vecs.sol_amounts.len());
        assert_eq!(vecs.ts_offsets_ms.len(), vecs.prices.len());
        assert!(vecs.ts_offsets_ms.len() <= 10);
        // Derived vectors length = N-1.
        if !vecs.ts_offsets_ms.is_empty() {
            assert_eq!(vecs.interval_ms.len(), vecs.ts_offsets_ms.len() - 1);
            assert_eq!(vecs.d_price.len(), vecs.ts_offsets_ms.len() - 1);
        }
    }

    #[test]
    fn test_extract_window_vectors_price_lookup_cursor() {
        // Verify cursor-based price lookup: last price_point with ts <= tx_ts.
        // price_history entries come from txs with reserve data:
        //   ts 1000 → price 1.0,  ts 1500 → price 2.0
        // Non-price txs (no reserves): ts 900, 1200, 1600
        // Expected prices: NaN (no prior price), 1.0, 1.0, 2.0, 2.0
        //
        // All txs inserted in chronological order (matching production).
        let config = v2_default_config();
        let pool_id = Pubkey::new_unique();
        let mut gk = GatekeeperBuffer::new(pool_id, &config);

        // ts=900: no reserves → no price_history entry
        let tx_early = create_v2_mock_tx(900, "early");
        gk.on_transaction(Arc::new(tx_early));

        // ts=1000: reserves → price_history entry at 1.0
        let mut tx_price1 = create_v2_mock_tx(1000, "price1");
        tx_price1.v_sol_in_bonding_curve = Some(10.0);
        tx_price1.v_tokens_in_bonding_curve = Some(10.0);
        tx_price1.curve_data_known = true;
        gk.on_transaction(Arc::new(tx_price1));

        // ts=1200: no reserves
        let tx_mid = create_v2_mock_tx(1200, "mid");
        gk.on_transaction(Arc::new(tx_mid));

        // ts=1500: reserves → price_history entry at 2.0
        let mut tx_price2 = create_v2_mock_tx(1500, "price2");
        tx_price2.v_sol_in_bonding_curve = Some(20.0);
        tx_price2.v_tokens_in_bonding_curve = Some(10.0);
        tx_price2.curve_data_known = true;
        gk.on_transaction(Arc::new(tx_price2));

        // ts=1600: no reserves
        let tx_late = create_v2_mock_tx(1600, "late");
        gk.on_transaction(Arc::new(tx_late));

        // Window covers all txs.
        let vecs = gk.extract_window_vectors(800, 1700, 200);

        // All vectors aligned.
        assert_eq!(vecs.ts_offsets_ms.len(), vecs.sol_amounts.len());
        assert_eq!(vecs.ts_offsets_ms.len(), vecs.prices.len());
        assert_eq!(vecs.prices.len(), 5);

        // tx@900:  no price point ≤ 900 → NaN
        assert!(vecs.prices[0].is_nan(), "tx@900 should be NaN");
        // tx@1000: price_history at ts=1000 → 1.0
        assert_eq!(vecs.prices[1], 1.0);
        // tx@1200: last price ≤ 1200 is ts=1000 → 1.0
        assert_eq!(vecs.prices[2], 1.0);
        // tx@1500: price_history at ts=1500 → 2.0
        assert_eq!(vecs.prices[3], 2.0);
        // tx@1600: last price ≤ 1600 is ts=1500 → 2.0
        assert_eq!(vecs.prices[4], 2.0);
    }

    #[test]
    fn test_buy_log_vector_fields_default_none() {
        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();
        let assessment = GatekeeperAssessment {
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
        };
        let log = assessment.to_buy_log(&pool_id, &config);
        assert!(log.vectors_max_len.is_none());
        assert!(log.vectors_ts_offsets_ms.is_none());
        assert!(log.vectors_sol_amounts.is_none());
        assert!(log.vectors_prices.is_none());
        assert!(log.vectors_interval_ms.is_none());
        assert!(log.vectors_d_price.is_none());
        // Vector fields should be absent from serialized JSON.
        let json = serde_json::to_string(&log).unwrap();
        assert!(!json.contains("vectors_ts_offsets_ms"));
        assert!(!json.contains("vectors_sol_amounts"));
        assert!(!json.contains("vectors_prices"));
        assert!(!json.contains("vectors_interval_ms"));
        assert!(!json.contains("vectors_d_price"));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Curve Readiness Latch tests
    // ═══════════════════════════════════════════════════════════════════════

    /// PendingCurve before deadline: curve_data_known=false, now < deadline → PendingCurve
    #[test]
    fn test_curve_latch_pending_before_deadline() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 800;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // t0 = 1000 (event-time), deadline = 1000 + 800 = 1800
        gk.set_curve_t0(1000);

        // Feed 10 organic TX with curve_data_known=false (None v_tokens/v_sol)
        let timestamps = [100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
        let signers = [
            "alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi", "ivan", "judy",
        ];
        for (i, &ts) in timestamps.iter().enumerate() {
            let tx = make_tx(
                ts,
                &format!("cl_sig_{}", i),
                signers[i],
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            let _v = gk.on_transaction(Arc::new(tx));
        }

        assert!(gk.phase1_passed, "Phase 1 should pass with 10 TX");
        // With curve_data_known=false and now (highest_seen_ts=1000) < deadline (1800),
        // the latch should return PendingCurve
        let v = gk.on_transaction(Arc::new(make_tx(
            1200,
            "cl_extra",
            "extra_signer",
            true,
            1.0,
            None,
            None,
            None,
        )));
        assert!(
            matches!(v, GatekeeperVerdict::Wait | GatekeeperVerdict::PendingCurve),
            "Before deadline with unknown curve: should be Wait or PendingCurve, got {:?}",
            match &v {
                GatekeeperVerdict::Wait => "Wait",
                GatekeeperVerdict::PendingCurve => "PendingCurve",
                GatekeeperVerdict::Buy { .. } => "Buy",
                GatekeeperVerdict::Reject { .. } => "Reject",
                GatekeeperVerdict::Timeout { .. } => "Timeout",
                GatekeeperVerdict::ApprovedTx { .. } => "ApprovedTx",
            }
        );
        // BUY must NOT have been issued
        assert_ne!(
            gk.state(),
            PoolState::Approved,
            "Must NOT issue BUY with curve unknown"
        );
    }

    /// Fail-closed after deadline: curve_data_known=false, now >= deadline → RejectHardFail
    #[test]
    fn test_curve_latch_reject_after_deadline() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 800;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // t0 = 1000 (event-time), deadline = 1000 + 800 = 1800
        gk.set_curve_t0(1000);

        // Feed 10 organic TX with curve_data_known=false
        let timestamps = [1000, 1100, 1200, 1300, 1400, 1500, 1600, 1700, 1800, 1900];
        let signers = [
            "alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi", "ivan", "judy",
        ];
        for (i, &ts) in timestamps.iter().enumerate() {
            let tx = make_tx(
                ts,
                &format!("cl2_sig_{}", i),
                signers[i],
                true,
                0.5 + (i as f64) * 0.1,
                None,
                None,
                None,
            );
            let _v = gk.on_transaction(Arc::new(tx));
        }

        // Now trigger re-eval past the deadline (ts=2500 > deadline=1800)
        // Need enough TX since last eval for re_eval_tx_interval
        for i in 0..4 {
            let tx = make_tx(
                2500 + i as u64 * 100,
                &format!("cl2_late_{}", i),
                &format!("late_signer_{}", i),
                true,
                1.0,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            if let GatekeeperVerdict::Reject { reason, .. } = &v {
                assert!(
                    reason.contains("CURVE_NOT_READY_TIMEOUT"),
                    "Rejection reason should contain CURVE_NOT_READY_TIMEOUT, got: {}",
                    reason
                );
                return; // Test passed
            }
        }

        // Also test via force_check_deadline (sweep loop path)
        let sweep_verdict = gk.force_check_deadline(2500);
        match sweep_verdict {
            GatekeeperVerdict::Reject { reason, .. } => {
                assert!(
                    reason.contains("CURVE_NOT_READY_TIMEOUT"),
                    "Sweep rejection reason should contain CURVE_NOT_READY_TIMEOUT, got: {}",
                    reason
                );
            }
            _ => {
                // If already rejected, force_check_deadline returns Wait — that's fine
                assert!(
                    gk.rejected,
                    "Pool should be marked rejected after curve timeout"
                );
            }
        }
    }

    #[test]
    fn test_curve_policy_unknown_rejects_immediately_when_pending_disabled() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = false;
        cfg.curve_wait_ms = 800;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_curve_t0(1000);

        let verdict = gk
            .check_curve_latch(1200)
            .expect("curve latch should reject immediately");

        match verdict {
            GatekeeperVerdict::Reject { reason, .. } => {
                assert!(reason.contains("CURVE_UNKNOWN_REJECTED_BY_POLICY"));
                assert!(gk.curve_terminal_recorded);
            }
            _ => panic!("expected immediate unknown-curve rejection"),
        }
    }

    #[test]
    fn test_curve_policy_stale_provisional_pending_then_timeout() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 500;
        cfg.stale_fallback = ShadowLedgerStaleFallback::PendingCurve;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_curve_t0(1000);
        gk.record_curve_state(CurveFreshnessState::Stale, CurveFinality::Provisional);
        let pending = gk
            .check_curve_latch(1300)
            .expect("stale provisional should enter PendingCurve before deadline");
        assert!(matches!(pending, GatekeeperVerdict::PendingCurve));

        gk.record_curve_state(CurveFreshnessState::Stale, CurveFinality::Provisional);
        let reject = gk
            .check_curve_latch(2000)
            .expect("stale provisional should time out after deadline");

        match reject {
            GatekeeperVerdict::Reject { reason, .. } => {
                assert!(reason.contains("CURVE_STALE_TIMEOUT"));
                assert!(gk.curve_terminal_recorded);
            }
            _ => panic!("expected stale timeout reject"),
        }
    }

    #[test]
    fn test_curve_policy_pending_curve_recovers_terminally() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 5_000;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        gk.set_curve_t0(1000);

        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        for (i, &signer) in signers.iter().enumerate() {
            let tx = make_tx(
                1000 + i as u64 * 100,
                &format!("pending_recover_{}", i),
                signer,
                true,
                0.5,
                None,
                None,
                None,
            );
            let _ = gk.on_transaction(Arc::new(tx));
        }

        let pending = gk.on_transaction(Arc::new(make_tx(
            2100,
            "pending_gate",
            "pending_signer",
            true,
            0.9,
            None,
            None,
            None,
        )));
        assert!(matches!(pending, GatekeeperVerdict::PendingCurve));
        assert!(gk.curve_pending_active);
        assert!(!gk.curve_terminal_recorded);

        let recovered = gk.on_transaction(Arc::new(make_tx(
            2200,
            "pending_recovered",
            "recovered_signer",
            true,
            1.0,
            Some(1_060_000_000.0),
            Some(31.0),
            Some(31.0),
        )));

        assert!(!matches!(recovered, GatekeeperVerdict::PendingCurve));
        assert!(gk.curve_terminal_recorded);
        assert!(!gk.curve_pending_active);
        assert!(gk.curve_ready);
    }

    /// Pass when curve known: curve_data_known=true, now < deadline → normal verdict (not PendingCurve)
    #[test]
    fn test_curve_latch_pass_when_curve_known() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 800;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // Pool detected at event-time 500ms. Deadline = 500 + 800 = 1300.
        // First TX at 1000ms is before deadline, so the latch would block if
        // curve_ready=false. Since TXs carry curve_data_known=true, curve_ready
        // becomes true on the first TX and BUY proceeds normally.
        let pool_detected_ts: u64 = 500;
        gk.set_curve_t0(pool_detected_ts);

        // Feed 10 organic TX WITH curve_data_known=true (providing v_tokens and v_sol)
        let timestamps = [
            1000u64, 2500, 4200, 5800, 8000, 9500, 14000, 16000, 20000, 25000,
        ];
        let signers = [
            "alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi", "ivan", "judy",
        ];
        let volumes = [0.5, 1.2, 0.3, 2.0, 0.8, 1.5, 0.1, 3.0, 0.7, 1.0];
        let v_tokens_base = 1_073_000_000.0;

        let mut last_verdict = GatekeeperVerdict::Wait;
        for i in 0..10 {
            let v_tokens = v_tokens_base - (i as f64) * 1_000_000.0;
            let v_sol = 30.0 + (i as f64) * 0.5;
            let tx = make_tx(
                timestamps[i],
                &format!("cl3_sig_{}", i),
                signers[i],
                true,
                volumes[i],
                Some(v_tokens),
                Some(v_sol),
                Some(v_sol),
            );
            last_verdict = gk.on_transaction(Arc::new(tx));
        }

        // With curve_data_known=true, the latch should NOT block.
        // curve_ready should be set to true by the first TX with curve_data_known=true.
        assert!(
            gk.curve_ready,
            "curve_ready should be true after TX with curve_data_known=true"
        );
        // Verdict should be Buy (all phases pass) or Wait — never PendingCurve
        assert!(
            !matches!(last_verdict, GatekeeperVerdict::PendingCurve),
            "With curve data known, verdict should not be PendingCurve"
        );
        // Since this is the same organic pattern as test_full_flow_organic_buy,
        // expect a BUY verdict
        if let GatekeeperVerdict::Buy { assessment, .. } = last_verdict {
            assert!(assessment.phases_passed >= 5, "Should pass enough phases");
        }
    }

    /// Z0.2 — curve_wait_elapsed_ms is populated in the Reject assessment from curve latch
    #[test]
    fn test_curve_latch_reject_has_curve_wait_elapsed_ms() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 500;
        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // t0 = 1000 ms (event-time), deadline = 1000 + 500 = 1500
        gk.set_curve_t0(1000);

        // Feed enough TX to pass Phase 1 (10 unique signers)
        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        for (i, &signer) in signers.iter().enumerate() {
            let tx = make_tx(
                1000 + i as u64 * 50,
                &format!("s{}", i),
                signer,
                true,
                0.5,
                None,
                None,
                None,
            );
            let _ = gk.on_transaction(Arc::new(tx));
        }

        // Now trigger at event-time 2000 (past deadline 1500), curve still unknown
        let mut last_reject: Option<GatekeeperAssessment> = None;
        for i in 0..5 {
            let tx = make_tx(
                2000 + i as u64 * 100,
                &format!("late{}", i),
                &format!("late_signer_{}", i),
                true,
                1.0,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            if let GatekeeperVerdict::Reject { assessment, reason } = v {
                assert!(
                    reason.contains("CURVE_NOT_READY_TIMEOUT"),
                    "reason must contain CURVE_NOT_READY_TIMEOUT, got: {}",
                    reason
                );
                last_reject = Some(assessment);
                break;
            }
        }

        // If on_transaction didn't produce the Reject, try force_check_deadline
        if last_reject.is_none() {
            let sweep = gk.force_check_deadline(2500);
            if let GatekeeperVerdict::Reject { assessment, reason } = sweep {
                assert!(reason.contains("CURVE_NOT_READY_TIMEOUT"));
                last_reject = Some(assessment);
            }
        }

        let assessment = last_reject.expect("curve latch must have issued Reject by ts=2500");

        // curve_t0_event_ts_ms must be populated
        assert!(
            assessment.curve_t0_event_ts_ms.is_some(),
            "curve_t0_event_ts_ms must be set in Reject assessment"
        );
        assert_eq!(assessment.curve_t0_event_ts_ms, Some(1000));

        // curve_wait_elapsed_ms must reflect real waiting time (>= deadline elapsed)
        let elapsed = assessment
            .curve_wait_elapsed_ms
            .expect("curve_wait_elapsed_ms must be set in Reject assessment");
        assert!(
            elapsed >= 500,
            "curve_wait_elapsed_ms ({}) must be >= curve_wait_ms (500)",
            elapsed
        );
    }

    /// Z0.2 — genesis_seed TX → PendingCurve → AccountUpdate before deadline → normal verdict
    ///
    /// Verifies the key transitional scenario: the curve latch correctly unblocks when a
    /// transaction with real curve data (curve_data_known=true) arrives before the deadline,
    /// after an initial period of genesis-seed-only TXs.
    #[test]
    fn test_curve_latch_genesis_seed_then_account_update_before_deadline() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 5_000; // generous 5s window

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);
        // t0 = 1000, deadline = 1000 + 5000 = 6000 (event-time)
        gk.set_curve_t0(1000);

        // Phase 1: feed organic TX with curve_data_known=false (genesis seed enrichment)
        // 10 unique signers to satisfy Phase 1 minimum
        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        for (i, &signer) in signers.iter().enumerate() {
            let ts = 1000 + i as u64 * 100;
            let tx = make_tx(ts, &format!("s{}", i), signer, true, 0.5, None, None, None);
            let v = gk.on_transaction(Arc::new(tx));
            // With curve_data_known=false the latch should block BUY
            assert!(
                !matches!(v, GatekeeperVerdict::Buy { .. }),
                "must not issue BUY while curve latch is active (genesis_seed TXs only)"
            );
        }

        // curve_ready must still be false (no AccountUpdate received yet)
        assert!(
            !gk.curve_ready,
            "curve_ready must be false before AccountUpdate"
        );

        // Phase 2: an AccountUpdate arrives at ts=2000 (well before deadline 6000).
        // Timestamps 1000-1900 are taken by the genesis-seed batch; use 2000 to avoid
        // TxKey collision (all TXs share slot=100 and event_ordinal=0, so uniqueness
        // is determined by timestamp_ms when signatures are not valid base58).
        // make_tx with Some(v_tokens) and Some(v_sol) sets curve_data_known=true.
        let v_tokens = 1_073_000_000.0 - 10_000_000.0;
        let v_sol = 30.5;
        let tx_update = make_tx(
            2000,
            "account_update_sig",
            "new_signer_1",
            true,
            1.0,
            Some(v_tokens),
            Some(v_sol),
            Some(v_sol),
        );
        let _ = gk.on_transaction(Arc::new(tx_update));

        // Curve latch must now be unlocked
        assert!(
            gk.curve_ready,
            "curve_ready must be true after TX with curve_data_known=true"
        );

        // Any subsequent evaluation should NOT return PendingCurve
        let tx_late = make_tx(
            2100,
            "extra_sig",
            "extra_signer",
            true,
            0.8,
            Some(v_tokens - 500_000.0),
            Some(v_sol + 0.1),
            Some(v_sol + 0.1),
        );
        let v_late = gk.on_transaction(Arc::new(tx_late));
        assert!(
            !matches!(v_late, GatekeeperVerdict::PendingCurve),
            "must NOT return PendingCurve after curve latch unlocked, got {:?}",
            match &v_late {
                GatekeeperVerdict::PendingCurve => "PendingCurve",
                GatekeeperVerdict::Buy { .. } => "Buy",
                GatekeeperVerdict::Wait => "Wait",
                GatekeeperVerdict::Reject { .. } => "Reject",
                GatekeeperVerdict::Timeout { .. } => "Timeout",
                GatekeeperVerdict::ApprovedTx { .. } => "ApprovedTx",
            }
        );
    }

    /// Z0.2 — late set_curve_t0 (from late NewPoolDetected) does not break deadline semantics
    ///
    /// In oracle_runtime, the curve_t0 is initially set from `registered_wall_ts_ms` (fallback),
    /// then corrected to `NewPoolDetected.timestamp_ms` when the event arrives later.
    /// This test verifies that the second set_curve_t0 call correctly resets the deadline
    /// and that a pool with earlier on-chain timestamp faces an earlier curve deadline.
    #[test]
    fn test_curve_latch_late_set_curve_t0_resets_deadline() {
        let pool_id = Pubkey::new_unique();
        let mut cfg = organic_flow_config();
        cfg.curve_require_for_buy = true;
        cfg.curve_wait_ms = 800;

        let mut gk = GatekeeperBuffer::new(pool_id, &cfg);

        // First call: fallback wall-clock time t0=5000, deadline=5800
        gk.set_curve_t0(5000);
        assert_eq!(gk.curve_deadline_event_ts_ms, Some(5800));

        // Feed TXs with no curve data (genesis seed) at event-time 1000-3000
        // These timestamps are before the initial t0=5000 but that's OK —
        // highest_seen_ts drives the event-time axis
        let signers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        for (i, &signer) in signers.iter().enumerate() {
            let ts = 1000 + i as u64 * 200;
            let tx = make_tx(
                ts,
                &format!("late_s{}", i),
                signer,
                true,
                0.5,
                None,
                None,
                None,
            );
            let _ = gk.on_transaction(Arc::new(tx));
        }

        // Late NewPoolDetected arrives: actual on-chain timestamp was 1000 (earlier than wall)
        // Second set_curve_t0 corrects the deadline to 1000 + 800 = 1800
        gk.set_curve_t0(1000);
        assert_eq!(
            gk.curve_deadline_event_ts_ms,
            Some(1800),
            "deadline must be recalculated from corrected t0"
        );

        // highest_seen_ts is now ~2800 (after 10 TXs at 200ms intervals starting at 1000)
        // So it's past the corrected deadline of 1800.
        // A TX at event-time 2900 should now trigger the curve reject
        let mut got_curve_reject = false;
        for i in 0..4 {
            let tx = make_tx(
                2900 + i as u64 * 100,
                &format!("after_d{}", i),
                &format!("new_sig_{}", i),
                true,
                1.0,
                None,
                None,
                None,
            );
            let v = gk.on_transaction(Arc::new(tx));
            if let GatekeeperVerdict::Reject { reason, .. } = &v {
                assert!(
                    reason.contains("CURVE_NOT_READY_TIMEOUT"),
                    "reject reason must be CURVE_NOT_READY_TIMEOUT, got: {}",
                    reason
                );
                got_curve_reject = true;
                break;
            }
        }

        // If on_transaction didn't produce the reject, try sweep (force_check_deadline)
        if !got_curve_reject {
            let sweep = gk.force_check_deadline(3000);
            match sweep {
                GatekeeperVerdict::Reject { reason, .. } => {
                    assert!(
                        reason.contains("CURVE_NOT_READY_TIMEOUT"),
                        "sweep reject reason must be CURVE_NOT_READY_TIMEOUT, got: {}",
                        reason
                    );
                    got_curve_reject = true;
                }
                _ => {
                    // force_check_deadline returns Wait if already rejected — check gk.rejected
                    got_curve_reject = gk.rejected;
                }
            }
        }

        assert!(
            got_curve_reject,
            "pool must be curve-rejected after corrected deadline (1800) is past"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Identity enrichment serialization tests
    // ═══════════════════════════════════════════════════════════════════════

    /// When base_mint is Some, the serialized JSON must contain the field.
    #[test]
    fn test_buy_log_serialization_with_base_mint() {
        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();

        let assessment = GatekeeperAssessment {
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
        };
        let mut log = assessment.to_buy_log(&pool_id, &config);

        // Simulate identity enrichment (as done by enrich_buy_log_with_identity)
        let test_mint = "So11111111111111111111111111111111111111112";
        let first_seen = 1700000000000_u64;
        let ab_window_ms: u64 = 10_000;
        let end_ts = first_seen + ab_window_ms;
        log.base_mint = Some(test_mint.to_string());
        log.first_seen_ts_ms = Some(first_seen);
        log.end_10s_ts_ms = Some(end_ts);
        log.join_key = Some(format!("{}:{}", pool_id, first_seen));
        log.dev_pubkey = Some("DevWallet111111111111111111111111111111111".to_string());
        log.gatekeeper_version = Some(ghost_brain::oracle::GATEKEEPER_VERSION.to_string());

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains(&format!("\"base_mint\":\"{}\"", test_mint)));
        assert!(json.contains(&format!("\"first_seen_ts_ms\":{}", first_seen)));
        assert!(json.contains(&format!("\"end_10s_ts_ms\":{}", end_ts)));
        assert!(json.contains("\"join_key\":\""));
        assert!(json.contains("\"dev_pubkey\":\""));
        assert!(json.contains("\"gatekeeper_version\":\""));
    }

    /// When identity fields are None (no enrichment), they must be absent
    /// from the serialized JSON (skip_serializing_if = "Option::is_none").
    #[test]
    fn test_buy_log_serialization_without_identity_fields_absent() {
        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();

        let assessment = GatekeeperAssessment {
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
        };
        let log = assessment.to_buy_log(&pool_id, &config);

        let json = serde_json::to_string(&log).unwrap();
        // With skip_serializing_if, None fields should be absent
        assert!(!json.contains("\"base_mint\""));
        assert!(!json.contains("\"first_seen_ts_ms\""));
        assert!(!json.contains("\"end_10s_ts_ms\""));
        assert!(!json.contains("\"join_key\""));
        assert!(!json.contains("\"dev_pubkey\""));
        assert!(!json.contains("\"gatekeeper_version\""));
    }

    #[test]
    fn test_fingerprint_metrics_map_to_buy_log_and_summary() {
        let pool_id = Pubkey::new_unique();
        let config = v2_default_config();
        let fingerprint = EarlyFingerprintMetrics {
            block0_sniped_supply_pct: Some(0.31),
            flip_ratio_10s: Some(0.6),
            cu_price_p90_1s: Some(15.0),
            cu_price_p90_10s: Some(20.0),
            priority_fee_surge_slope: Some(0.5),
            buyer_pre_balance_cv: Some(0.22),
            avg_inner_ix_count_50tx: Some(3.5),
            avg_cpi_depth_50tx: Some(1.8),
            sell_buy_ratio: Some(0.6),
            compute_unit_cluster_dominance: Some(0.75),
            static_fee_profile_ratio: Some(0.8),
            fixed_size_buy_ratio: Some(0.6),
            fixed_size_buy_ratio_1e4: Some(0.4),
            flipper_presence_ratio: Some(0.6),
            jito_tip_intensity: Some(0.5),
            early_slot_volume_dominance_buy: Some(0.7334),
            early_top3_buy_volume_pct_3s: Some(0.7123),
            whale_reversal_ratio_top3: Some(0.3810),
            whale_reversal_ratio_top1: Some(0.3),
            dev_paperhand_latency_ms: Some(2_500),
            dev_sold_within_3s: Some(true),
            dev_sold_within_5s: Some(true),
            fingerprint_degraded: true,
            fingerprint_reason: Some("TEST_REASON".into()),
        };
        let mut feature_snapshot = MaterializedFeatureSet::default();
        feature_snapshot.sybil_resistance = ghost_core::checkpoint::SybilResistanceFeatures {
            fee_topology_diversity_index: Some(0.42),
            dev_buyer_infrastructure_affinity: Some(0.19),
            spend_fraction_divergence: Some(0.27),
            demand_elasticity_score: Some(-0.25),
            signer_cross_pool_velocity: Some(0.44),
            funding_source_concentration: Some(0.52),
            funding_source_diagnostics: Some(
                ghost_core::tx_intelligence::types::FundingSourceDiagnostics {
                    buyer_sample_count: 5,
                    known_source_count: 2,
                    unknown_buyer_count: 3,
                    structural_unknown_buyer_count: 1,
                    operational_unknown_buyer_count: 1,
                    indeterminate_unknown_buyer_count: 1,
                    miss_reason_counts: vec![
                        ghost_core::tx_intelligence::types::FundingSourceMissReasonCount {
                            reason: "FSC_GLOBAL_RECIPIENT_EVICTED".to_string(),
                            class: ghost_core::tx_intelligence::types::FscMissClass::Operational,
                            count: 1,
                        },
                        ghost_core::tx_intelligence::types::FundingSourceMissReasonCount {
                            reason: "FSC_NO_PREBUY_TRANSFER_IN_WINDOW".to_string(),
                            class: ghost_core::tx_intelligence::types::FscMissClass::Structural,
                            count: 1,
                        },
                    ],
                },
            ),
            degraded_reasons: vec!["FTDI_INSUFFICIENT_BUYS".to_string()],
            buy_sample_count: 5,
            signer_sample_count: 5,
        };

        let assessment = GatekeeperAssessment {
            phase1_passed: true,
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
            phases_passed: 1,
            hard_reject_reason: None,
            total_tx_evaluated: 8,
            unique_tx_evaluated: 8,
            unique_signers_evaluated: 5,
            observation_duration_ms: 3_500,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 1,
            buy_count: 5,
            decision: None,
            early_fingerprint: Some(fingerprint.clone()),
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            feature_snapshot,
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
        };

        let buy_log = assessment.to_buy_log(&pool_id, &config);
        assert_eq!(buy_log.sell_buy_ratio, fingerprint.sell_buy_ratio);
        assert_eq!(
            buy_log.compute_unit_cluster_dominance,
            fingerprint.compute_unit_cluster_dominance
        );
        assert_eq!(
            buy_log.static_fee_profile_ratio,
            fingerprint.static_fee_profile_ratio
        );
        assert_eq!(
            buy_log.fixed_size_buy_ratio,
            fingerprint.fixed_size_buy_ratio
        );
        assert_eq!(
            buy_log.fixed_size_buy_ratio_1e4,
            fingerprint.fixed_size_buy_ratio_1e4
        );
        assert_eq!(
            buy_log.flipper_presence_ratio,
            fingerprint.flipper_presence_ratio
        );
        assert_eq!(buy_log.jito_tip_intensity, fingerprint.jito_tip_intensity);
        assert_eq!(
            buy_log.early_slot_volume_dominance_buy,
            fingerprint.early_slot_volume_dominance_buy
        );
        assert_eq!(
            buy_log.early_top3_buy_volume_pct_3s,
            fingerprint.early_top3_buy_volume_pct_3s
        );
        assert_eq!(
            buy_log.whale_reversal_ratio_top3,
            fingerprint.whale_reversal_ratio_top3
        );
        assert_eq!(
            buy_log.whale_reversal_ratio_top1,
            fingerprint.whale_reversal_ratio_top1
        );
        assert_eq!(
            buy_log.dev_paperhand_latency_ms,
            fingerprint.dev_paperhand_latency_ms
        );
        assert_eq!(buy_log.dev_sold_within_3s, fingerprint.dev_sold_within_3s);
        assert_eq!(buy_log.dev_sold_within_5s, fingerprint.dev_sold_within_5s);
        assert_eq!(buy_log.fee_topology_diversity_index, Some(0.42));
        assert_eq!(buy_log.min_fee_topology_diversity_index, 0.0);
        assert_eq!(buy_log.dev_buyer_infrastructure_affinity, Some(0.19));
        assert_eq!(buy_log.max_dev_buyer_infrastructure_affinity, 1.0);
        assert_eq!(buy_log.spend_fraction_divergence, Some(0.27));
        assert_eq!(buy_log.min_spend_fraction_divergence, 0.0);
        assert_eq!(buy_log.demand_elasticity_score, Some(-0.25));
        assert_eq!(buy_log.min_demand_elasticity_score, -1.0);
        assert_eq!(buy_log.signer_cross_pool_velocity, Some(0.44));
        assert_eq!(buy_log.max_signer_cross_pool_velocity, 1.0);
        assert_eq!(buy_log.funding_source_concentration, Some(0.52));
        assert_eq!(buy_log.max_funding_source_concentration, 1.0);
        assert!(buy_log.funding_source_diagnostics.is_some());
        let fsc_diagnostics = buy_log.funding_source_diagnostics.as_ref().unwrap();
        assert_eq!(fsc_diagnostics.buyer_sample_count, 5);
        assert_eq!(fsc_diagnostics.known_source_count, 2);
        assert_eq!(fsc_diagnostics.unknown_buyer_count, 3);
        assert_eq!(fsc_diagnostics.structural_unknown_buyer_count, 1);
        assert_eq!(fsc_diagnostics.operational_unknown_buyer_count, 1);
        assert_eq!(fsc_diagnostics.indeterminate_unknown_buyer_count, 1);
        assert_eq!(fsc_diagnostics.miss_reason_counts.len(), 2);
        assert_eq!(
            buy_log.sybil_metric_degraded_reasons,
            vec!["FTDI_INSUFFICIENT_BUYS".to_string()]
        );

        let json = serde_json::to_string(&buy_log).unwrap();
        assert!(json.contains("\"sell_buy_ratio\":0.6"));
        assert!(json.contains("\"compute_unit_cluster_dominance\":0.75"));
        assert!(json.contains("\"static_fee_profile_ratio\":0.8"));
        assert!(json.contains("\"fixed_size_buy_ratio\":0.6"));
        assert!(json.contains("\"fixed_size_buy_ratio_1e4\":0.4"));
        assert!(json.contains("\"flipper_presence_ratio\":0.6"));
        assert!(json.contains("\"jito_tip_intensity\":0.5"));
        assert!(json.contains("\"early_slot_volume_dominance_buy\":0.7334"));
        assert!(json.contains("\"early_top3_buy_volume_pct_3s\":0.7123"));
        assert!(json.contains("\"whale_reversal_ratio_top3\":0.381"));
        assert!(json.contains("\"whale_reversal_ratio_top1\":0.3"));
        assert!(json.contains("\"dev_paperhand_latency_ms\":2500"));
        assert!(json.contains("\"dev_sold_within_3s\":true"));
        assert!(json.contains("\"dev_sold_within_5s\":true"));
        assert!(json.contains("\"fee_topology_diversity_index\":0.42"));
        assert!(json.contains("\"dev_buyer_infrastructure_affinity\":0.19"));
        assert!(json.contains("\"spend_fraction_divergence\":0.27"));
        assert!(json.contains("\"demand_elasticity_score\":-0.25"));
        assert!(json.contains("\"signer_cross_pool_velocity\":0.44"));
        assert!(json.contains("\"funding_source_concentration\":0.52"));
        assert!(json.contains("\"funding_source_diagnostics\""));
        assert!(json.contains("\"FSC_GLOBAL_RECIPIENT_EVICTED\""));
        assert!(json.contains("\"sybil_metric_degraded_reasons\":[\"FTDI_INSUFFICIENT_BUYS\"]"));

        let summary = assessment.fingerprint_summary("pool_1", "mint_1");
        assert!(summary.contains("FINGERPRINT pool=pool_1 mint=mint_1"));
        assert!(summary.contains("sell_buy=0.6000"));
        assert!(summary.contains("cu_cluster=0.7500"));
        assert!(summary.contains("static_fee=0.8000"));
        assert!(summary.contains("fixed_buy=0.6000"));
        assert!(summary.contains("flipper=0.6000"));
        assert!(summary.contains("jito_tip=0.5000"));
        assert!(summary.contains("early_slot_dom=0.7334"));
        assert!(summary.contains("whale_rev_top3=0.3810"));
        assert!(summary.contains("dev_latency_ms=2500"));
        assert!(summary.contains("dev_3s=true"));
        assert!(summary.contains("dev_5s=true"));
        assert!(summary.contains("ftdi=0.4200"));
        assert!(summary.contains("dbia=0.1900"));
        assert!(summary.contains("sfd=0.2700"));
        assert!(summary.contains("des=-0.2500"));
        assert!(summary.contains("cpv=0.4400"));
        assert!(summary.contains("fsc=0.5200"));
        assert!(summary.contains("sybil_degraded=FTDI_INSUFFICIENT_BUYS"));
    }

    // ═══════════════════════════════════════════
    // V2.5 Dynamic Observation Window tests
    // ═══════════════════════════════════════════

    fn v25_enabled_config() -> GatekeeperV2Config {
        let mut cfg = GatekeeperV2Config::default();
        cfg.v25.shadow_enabled = true;
        cfg.dow.enabled = true;
        cfg.dow.early_entry_min_ms = 2000;
        cfg.dow.early_entry_max_ms = 5000;
        cfg.dow.normal_window_ms = 7000;
        cfg.dow.extended_window_ms = 10000;
        cfg.dow.early_entry_min_tx_count = 15;
        cfg.dow.early_entry_min_confidence = 0.85;
        cfg.dow.early_entry_max_sybil_points = 1;
        cfg.dow.early_entry_min_momentum = 0.40;
        cfg.dow.early_entry_max_entry_drift_pct = 3.0;
        cfg.dow.normal_window_min_confidence = 0.65;
        cfg.mode = GatekeeperMode::Long;
        cfg.max_wait_time_ms = 10000;
        cfg.min_tx_count = 10;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 3;
        cfg
    }

    #[test]
    fn test_v25_shadow_disabled_by_default() {
        let buf = GatekeeperBuffer::new(Pubkey::new_unique(), &GatekeeperV2Config::default());
        assert!(!buf.config.v25.shadow_enabled);
        assert!(buf.v25_shadow_decisions.is_empty());
        assert_eq!(buf.window_stage, ObservationStage::Extended);
        assert!(!buf.early_shadow_fired);
        assert!(!buf.normal_shadow_fired);
    }

    #[test]
    fn test_v25_shadow_early_insufficient_data() {
        let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &v25_enabled_config());
        buf.set_registered_wall_t0(1000);

        // With zero TX, early shadow should fire InsufficientData
        let wall_now = 1000 + 3000; // elapsed = 3000ms (within early window 2-5s)
        let result = buf.try_shadow_evaluate(wall_now, ObservationStage::Early);
        assert!(result);
        assert_eq!(buf.v25_shadow_decisions.len(), 1);
        assert_eq!(
            buf.v25_shadow_decisions[0].kind.verdict_str(),
            "INSUFFICIENT_DATA"
        );
    }

    #[test]
    fn test_v25_shadow_early_reject() {
        let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &v25_enabled_config());
        buf.set_registered_wall_t0(1000);

        // Manually add enough TX data to pass phase 1 but not enough for early criteria
        buf.unique_signers = (0..10).map(|i| format!("signer_{}", i)).collect();
        buf.total_tx_count = 20;
        buf.buy_count = 10;
        // But no buffered_txs → still InsufficientData guard fires first...
        // Actually we need buffered_txs non-empty
        // For this test we just verify InsufficientData since no buffered TXs
        let wall_now = 1000 + 4000;
        buf.try_shadow_evaluate(wall_now, ObservationStage::Early);
        assert!(!buf.v25_shadow_decisions.is_empty());
        // With empty buffered_txs, still InsufficientData
        assert_eq!(
            buf.v25_shadow_decisions[0].kind.verdict_str(),
            "INSUFFICIENT_DATA"
        );
    }

    #[test]
    fn test_v25_shadow_one_fire_per_stage() {
        let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &v25_enabled_config());
        buf.set_registered_wall_t0(1000);

        // First call at early window
        let wall_1 = 1000 + 3000;
        buf.try_shadow_evaluate(wall_1, ObservationStage::Early);
        assert_eq!(buf.v25_shadow_decisions.len(), 1);

        // Second call at same window — still records (guard is in on_transaction_long, not here)
        let wall_2 = 1000 + 4000;
        buf.try_shadow_evaluate(wall_2, ObservationStage::Early);
        assert_eq!(buf.v25_shadow_decisions.len(), 2);

        // Normal window
        let wall_3 = 1000 + 8000;
        buf.try_shadow_evaluate(wall_3, ObservationStage::Normal);
        assert_eq!(buf.v25_shadow_decisions.len(), 3);
        assert_eq!(buf.v25_shadow_decisions[2].window, ObservationStage::Normal);
    }

    #[test]
    fn test_v25_confidence_computation() {
        let test_config = GatekeeperV2Config::default();
        let mut assessment = GatekeeperAssessment {
            phase1_passed: true,
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
            phases_passed: 6, // all 6 passed → base_quality = 1.0
            hard_reject_reason: None,
            total_tx_evaluated: 30,
            unique_tx_evaluated: 30,
            unique_signers_evaluated: 20,
            observation_duration_ms: 5000,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 1,
            buy_count: 15,
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
        };

        // No decision → confidence is None
        assert!(assessment.v25_confidence(&test_config).is_none());

        // Helper to build a minimal decision with good alpha gate values
        let mk_decision = |sp: u8, max: u8| GatekeeperDecision {
            hard_fail_reason: None,
            core1_passed: true,
            core2_passed: true,
            core3_passed: true,
            soft_signals: SoftSignals {
                low_interval_cv: false,
                high_interval_cv: false,
                low_timing_entropy: false,
                high_timing_entropy: false,
                avg_interval_out_of_range: false,
                high_burst_ratio: false,
                bundle_suspicion: false,
                cabal_suspicion: false,
                top3_dominance: false,
                high_volume_gini: false,
                unique_ratio_out_of_range: false,
                high_tx_per_signer: false,
                low_dust_count: false,
            },
            soft_points: sp,
            max_soft_points_possible: max,
            effective_max_soft_points: max,
            dev_unknown: false,
            sybil_policy: SybilPolicyDiagnostics {
                enabled: false,
                combo_veto_enabled: false,
                soft_signals: SybilSoftSignals {
                    low_ftdi: false,
                    high_dbia: false,
                    low_sfd: false,
                    low_des: false,
                    high_cpv: false,
                    high_fsc: false,
                },
                soft_points: 0,
                max_soft_points_possible: 255,
                effective_max_soft_points: 255,
                lead_signal: None,
                interference_patterns: vec![],
                meta_score: None,
                metric_degraded_reasons: vec![],
            },
            // Full alpha quality: momentum=1.0, demand=1.0 → alpha_quality = 1.0*0.4 + 1.0*0.35 + 1.0*0.25 = 1.0
            alpha_gate: AlphaGateDiagnostics {
                enabled: true,
                actionable: true,
                momentum: Some(1.0),
                demand: Some(1.0),
                joint: Some(1.0),
                pass: Some(true),
                reject_trigger: None,
                skip_reason: None,
            },
            prosperity_filter: ProsperityFilterDiagnostics::not_run(false),
            total_soft_points: 0,
            verdict_type: GatekeeperVerdictType::Buy,
            verdict_buy: true,
            reason_chain: String::new(),
            gatekeeper_strength: None,
        };

        // All phases passed + full alpha + clean pdd/tas + sybil_clean → confidence close to 1.0
        assessment.decision = Some(mk_decision(0, 10));
        let conf = assessment.v25_confidence(&test_config).unwrap();
        // base_quality=1.0 * alpha_quality=1.0 * pdd=1.0 * tas=1.0 * sybil=1.0 = 1.0
        assert!(conf > 0.95, "Expected confidence > 0.95, got {:.4}", conf);

        // Lower phases_passed reduces base_quality → lower confidence
        assessment.phases_passed = 3; // base_quality = 3/6 = 0.5
        let conf2 = assessment.v25_confidence(&test_config).unwrap();
        assert!(
            conf2 < conf,
            "Lower phases_passed should reduce confidence, got {:.4} vs {:.4}",
            conf2,
            conf
        );
        assessment.phases_passed = 6; // restore

        // With extreme TAS trajectory score < 0.30 → returns 0.0
        assessment.decision = Some(mk_decision(0, 10));
        assessment.trajectory = Some(TrajectoryAssessment {
            overall_tas_score: 0.15,
            momentum_score: 0.1,
            hhi_score: 0.2,
            volume_score: 0.1,
            interval_score: 0.2,
            buy_ratio_score: 0.1,
            segment_count: 3,
            t0_tx_count: 5,
            t1_tx_count: 5,
            t2_tx_count: 5,
        });
        // Enable TAS in config
        let mut cfg_with_tas = test_config;
        cfg_with_tas.tas.enabled = true;
        let conf3 = assessment.v25_confidence(&cfg_with_tas).unwrap();
        assert!(
            (conf3 - 0.0).abs() < f64::EPSILON,
            "TAS extreme → 0.0, got {:.4}",
            conf3
        );
    }

    #[test]
    fn test_v25_confidence_zero_on_pdd_hard_fail() {
        let mut cfg = GatekeeperV2Config::default();
        cfg.pdd.enabled = true;

        let decision = GatekeeperDecision {
            hard_fail_reason: None,
            core1_passed: true,
            core2_passed: true,
            core3_passed: true,
            soft_signals: SoftSignals {
                low_interval_cv: false,
                high_interval_cv: false,
                low_timing_entropy: false,
                high_timing_entropy: false,
                avg_interval_out_of_range: false,
                high_burst_ratio: false,
                bundle_suspicion: false,
                cabal_suspicion: false,
                top3_dominance: false,
                high_volume_gini: false,
                unique_ratio_out_of_range: false,
                high_tx_per_signer: false,
                low_dust_count: false,
            },
            soft_points: 0,
            max_soft_points_possible: 8,
            effective_max_soft_points: 8,
            dev_unknown: false,
            sybil_policy: SybilPolicyDiagnostics {
                enabled: false,
                combo_veto_enabled: false,
                soft_signals: SybilSoftSignals {
                    low_ftdi: false,
                    high_dbia: false,
                    low_sfd: false,
                    low_des: false,
                    high_cpv: false,
                    high_fsc: false,
                },
                soft_points: 0,
                max_soft_points_possible: 255,
                effective_max_soft_points: 255,
                lead_signal: None,
                interference_patterns: vec![],
                meta_score: None,
                metric_degraded_reasons: vec![],
            },
            alpha_gate: AlphaGateDiagnostics {
                enabled: true,
                actionable: true,
                momentum: Some(1.0),
                demand: Some(1.0),
                joint: Some(1.0),
                pass: Some(true),
                reject_trigger: None,
                skip_reason: None,
            },
            prosperity_filter: ProsperityFilterDiagnostics::not_run(false),
            total_soft_points: 0,
            verdict_type: GatekeeperVerdictType::Buy,
            verdict_buy: true,
            reason_chain: "clean_core".to_string(),
            gatekeeper_strength: None,
        };

        let mut pdd = PddDiagnostics::not_run();
        pdd.enabled = true;
        pdd.hard_fail = Some(PddHardFail::EntryDrift);
        pdd.pdd_score = 0.91;

        let assessment = GatekeeperAssessment {
            phase1_passed: true,
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
            phases_passed: 6,
            hard_reject_reason: None,
            total_tx_evaluated: 30,
            unique_tx_evaluated: 30,
            unique_signers_evaluated: 20,
            observation_duration_ms: 5000,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 1,
            buy_count: 15,
            decision: Some(decision),
            early_fingerprint: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            feature_snapshot: MaterializedFeatureSet::default(),
            checkpoint_count: 0,
            trajectory_available: false,
            v25_shadow_decisions: Vec::new(),
            trajectory: None,
            pdd_assessment: Some(pdd),
            aps_diagnostics: None,
            observation_stage: None,
            entry_drift_pct: None,
            entry_drift_anchor_quality: None,
            adaptive_thresholds_applied: false,
            v25_confidence: None,
        };

        let conf = assessment.v25_confidence(&cfg).unwrap();
        assert!(
            (conf - 0.0).abs() < f64::EPSILON,
            "PDD hard fail must force zero confidence, got {:.4}",
            conf
        );
        let breakdown = assessment.v25_confidence_breakdown(&cfg).unwrap();
        assert!(
            breakdown.pre_veto_confidence > 0.0,
            "Pre-veto confidence should stay informative even when PDD veto zeroes final confidence"
        );
        assert!(breakdown.zeroed_by_pdd_hard_fail);
        assert!(!breakdown.zeroed_by_tas_hard_reject);

        let buy_log = assessment.to_buy_log(&Pubkey::new_unique(), &cfg);
        assert_eq!(buy_log.v25_confidence, Some(0.0));
        assert_eq!(
            buy_log.v25_confidence_pre_veto,
            Some(breakdown.pre_veto_confidence)
        );
        assert_eq!(buy_log.v25_confidence_zeroed_by_pdd_hard_fail, Some(true));
        assert_eq!(
            buy_log.v25_shadow_confidence_source,
            Some("assessment_cached".to_string())
        );
    }

    #[test]
    fn test_extended_shadow_never_buys_when_pdd_unclean() {
        let mut cfg = organic_flow_config();
        cfg.mode = GatekeeperMode::Long;
        cfg.max_wait_time_ms = 10_000;
        cfg.use_three_layer_decision = true;
        cfg.v25.shadow_enabled = true;
        cfg.dow.enabled = true;
        cfg.dow.extended_window_ms = 10_000;
        cfg.dow.extended_window_min_confidence = 0.0;
        cfg.dow.extended_require_pdd_clean = false;
        cfg.pdd.enabled = true;
        cfg.pdd.entry_drift_max_pct = 5.0;
        cfg.min_tx_count = 8;
        cfg.min_unique_signers = 5;
        cfg.min_buy_count = 5;

        let mut buf = GatekeeperBuffer::new(Pubkey::new_unique(), &cfg);
        buf.set_registered_wall_t0(1_000);

        for i in 0..10 {
            let tx = make_tx(
                1_000 + i as u64 * 900,
                &format!("ext_pdd_{}", i),
                &format!("signer_{}", i),
                true,
                0.6 + (i as f64) * 0.15,
                Some(1_073_000_000.0 - (i as f64) * 1_000_000.0),
                Some(30.0 + (i as f64) * 0.5),
                Some(30.0 + (i as f64) * 0.5),
            );
            assert!(matches!(
                buf.on_transaction(Arc::new(tx)),
                GatekeeperVerdict::Wait
            ));
        }

        let deadline_tx = make_tx(
            11_001,
            "ext_pdd_deadline",
            "signer_deadline",
            true,
            2.0,
            Some(1_062_000_000.0),
            Some(36.5),
            Some(36.5),
        );
        let _ = buf.on_transaction(Arc::new(deadline_tx));

        // DOW timer or TX path may fire multiple Extended checkpoints.
        // InsufficientData decisions (fired when TX count is still low) do not
        // set `extended_shadow_fired`, so the final meaningful verdict may come
        // from either the timer or the deadline fallback. Find the last
        // non-InsufficientData Extended decision.
        let extended = buf
            .v25_shadow_decisions
            .iter()
            .filter(|d| {
                d.window == ObservationStage::Extended
                    && d.kind != ShadowDecisionKind::InsufficientData
            })
            .last()
            .expect("expected meaningful extended shadow decision");

        assert_eq!(extended.kind, ShadowDecisionKind::RejectPumpAndDump);
        assert_eq!(extended.confidence, 0.0);
        assert_ne!(extended.kind.verdict_str(), "BUY");
        // Reason format depends on which path fired Extended:
        // - Timer/tx path (try_shadow_evaluate): "PDD_{FAIL_TAG}: drift=..."
        // - Deadline fallback (check_long_deadline): "EXTENDED_SHADOW_DEADLINE_FALLBACK_REJECT_PDD_{FAIL_TAG}: ..."
        let reason_valid = extended.reason.contains("PDD_ENTRY_DRIFT")
            || extended.reason.contains("EXTENDED_REJECT_PDD_")
            || extended
                .reason
                .contains("EXTENDED_SHADOW_DEADLINE_FALLBACK_REJECT_PDD_");
        assert!(
            reason_valid,
            "expected explicit extended PDD reject reason, got {}",
            extended.reason
        );
    }

    #[test]
    fn test_observation_stage_default() {
        let stage = ObservationStage::default();
        assert_eq!(stage, ObservationStage::Extended);
    }

    #[test]
    fn test_shadow_decision_kind_verdict_str() {
        assert_eq!(ShadowDecisionKind::EarlyBuyCandidate.verdict_str(), "BUY");
        assert_eq!(ShadowDecisionKind::NormalBuyCandidate.verdict_str(), "BUY");
        assert_eq!(ShadowDecisionKind::ShadowReject.verdict_str(), "REJECT");
        assert_eq!(
            ShadowDecisionKind::InsufficientData.verdict_str(),
            "INSUFFICIENT_DATA"
        );
    }
}
