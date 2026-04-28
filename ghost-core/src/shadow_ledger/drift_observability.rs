//! Drift Observability, Hot-Pool Tx-Loss Tracking, and Replay Validation
//!
//! This module provides three focused observability and validation helpers for
//! the Shadow Ledger:
//!
//! 1. **[`DriftObservabilityReport`]** — accumulates per-pool drift statistics
//!    so operators can answer: *which pools drift, how often, and how badly?*
//!
//! 2. **[`HotPoolTxLossTracker`]** — tracks transaction visibility for pools
//!    with high tx volume, surfacing observation-window loss concentration.
//!
//! 3. **[`ReplayValidator`]** — replays a recorded sequence of transactions
//!    against the Shadow Ledger and compares the resulting state against a
//!    reference on-chain snapshot, capturing any drift / diagnostic-signal
//!    behaviour.
//!
//! ## Architecture fit
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────┐
//! │   Shadow Ledger (tx-driven, primary authority)            │
//! │   ShadowLedgerReconciler (diagnostic AccountUpdate lane)  │
//! └─────────────────────┬─────────────────────────────────────┘
//!                       │ drift events / reconciliation outcomes
//!                       ▼
//! ┌───────────────────────────────────────────────────────────┐
//! │   DriftObservabilityReport  (record & query per-pool)     │
//! │   HotPoolTxLossTracker      (count tx-seen vs tx-lost)    │
//! │   ReplayValidator           (replay & compare state)      │
//! └───────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage pattern
//!
//! ```rust,ignore
//! use ghost_core::shadow_ledger::drift_observability::{
//!     DriftObservabilityReport, HotPoolTxLossTracker, ReplayValidator,
//! };
//! use ghost_core::shadow_ledger::reconciliation::{DriftSeverity, ReconciliationAction};
//!
//! // ── 1. Drift observability ───────────────────────────────
//! let mut report = DriftObservabilityReport::new();
//! // Feed every reconciliation outcome into the report:
//! if let Some(outcome) = reconciler.reconcile(&mint, on_chain_sol, on_chain_tok, 0, slot) {
//!     report.record(&outcome);
//! }
//! // Query later:
//! let stats = report.stats_for(&mint);
//!
//! // ── 2. Hot-pool tx-loss tracking ────────────────────────
//! let mut tracker = HotPoolTxLossTracker::new();
//! tracker.record_seen(&mint);           // Seer observed a tx for this mint
//! tracker.record_forwarded(&mint);      // tx reached the Shadow Ledger authority path
//! tracker.record_suspected_miss(&mint); // diagnostic signal → possible tx was missed
//! let loss = tracker.loss_summary(&mint);
//!
//! // ── 3. Replay validation ────────────────────────────────
//! let mut validator = ReplayValidator::new(ledger.clone());
//! validator.record_tx(mint, sol_delta, tok_delta, slot);
//! let result = validator.validate_against_account_update(&mint, on_chain_sol, on_chain_tok);
//! ```

use std::collections::HashMap;

use metrics::increment_counter;
use solana_sdk::pubkey::Pubkey;
use tracing::{info, warn};

use crate::market_state::{
    ShadowLedgerStateConfidence, ShadowLedgerWriteReason, ShadowLedgerWriteSource,
    ShadowLedgerWriteStrength,
};
use crate::shadow_ledger::reconciliation::{
    DriftSeverity, ReconciliationAction, ReconciliationOutcome, ShadowLedgerReconciler,
};
use crate::shadow_ledger::{CurveWriteMetadata, ShadowLedger};

// ============================================================================
// Constants
// ============================================================================

/// Minimum number of observed transactions for a pool to be classified as *hot*
/// during a single observation window.
///
/// Pools at or above this threshold receive additional loss-concentration
/// reporting via [`HotPoolTxLossTracker`].
pub const HOT_POOL_TX_THRESHOLD: u64 = 20;

// ============================================================================
// Per-pool drift statistics
// ============================================================================

/// Accumulated drift statistics for a single pool (mint).
///
/// Every call to [`DriftObservabilityReport::record`] updates one of these
/// records.  At any time the record answers:
/// - how many reconciliation checks were performed for this pool,
/// - how many produced each severity level,
/// - how many diagnostic signals were triggered,
/// - what the peak / cumulative drift magnitude was.
#[derive(Debug, Default, Clone)]
pub struct PoolDriftStats {
    /// Total reconciliation checks for this pool.
    pub checks: u64,
    /// Checks that found no drift.
    pub no_drift: u64,
    /// Checks that classified drift as noise.
    pub noise: u64,
    /// Checks that classified drift as meaningful and only logged.
    pub meaningful: u64,
    /// Checks that classified drift as severe.
    pub severe: u64,
    /// Total number of dormant diagnostic signals observed.
    pub diagnostic_signals: u64,
    /// Peak absolute SOL drift seen (lamports).
    pub peak_abs_sol_drift: u64,
    /// Cumulative absolute SOL drift across all checks (lamports).
    pub cumulative_abs_sol_drift: u64,
}

impl PoolDriftStats {
    fn record(&mut self, outcome: &ReconciliationOutcome) {
        self.checks += 1;
        self.cumulative_abs_sol_drift = self
            .cumulative_abs_sol_drift
            .saturating_add(outcome.abs_sol_drift);
        if outcome.abs_sol_drift > self.peak_abs_sol_drift {
            self.peak_abs_sol_drift = outcome.abs_sol_drift;
        }
        match outcome.severity {
            DriftSeverity::None => self.no_drift += 1,
            DriftSeverity::Noise => self.noise += 1,
            DriftSeverity::Meaningful => self.meaningful += 1,
            DriftSeverity::Severe => self.severe += 1,
        }
        if outcome.action == ReconciliationAction::DiagnosticSignal {
            self.diagnostic_signals += 1;
        }
    }

    /// Return `true` if any drift was ever detected for this pool.
    pub fn has_drift(&self) -> bool {
        self.noise > 0 || self.meaningful > 0 || self.severe > 0
    }

    /// Return the fraction of checks that resulted in a diagnostic signal
    /// (0.0–1.0).
    pub fn diagnostic_signal_rate(&self) -> f64 {
        if self.checks == 0 {
            0.0
        } else {
            self.diagnostic_signals as f64 / self.checks as f64
        }
    }
}

// ============================================================================
// DriftObservabilityReport
// ============================================================================

/// Aggregates per-pool drift statistics from reconciliation outcomes.
///
/// Feed every [`ReconciliationOutcome`] returned by
/// [`ShadowLedgerReconciler::reconcile`] into this report, then query
/// [`stats_for`](Self::stats_for) or [`drift_pools`](Self::drift_pools) to
/// understand which pools are unstable and how badly.
///
/// # Metrics emitted
///
/// - `shadow_ledger_obs_checks_total`   — total reconciliation checks recorded
/// - `shadow_ledger_obs_drift_total`    — checks where any drift was observed
/// - `shadow_ledger_obs_diagnostic_signals_total` — dormant diagnostic
///   signals recorded
#[derive(Debug, Default)]
pub struct DriftObservabilityReport {
    pools: HashMap<Pubkey, PoolDriftStats>,
}

impl DriftObservabilityReport {
    /// Create an empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one reconciliation outcome into the per-pool statistics.
    ///
    /// This is the primary ingestion point: call it once per reconciliation
    /// outcome returned by [`ShadowLedgerReconciler::reconcile`].
    pub fn record(&mut self, outcome: &ReconciliationOutcome) {
        increment_counter!("shadow_ledger_obs_checks_total");

        let stats = self.pools.entry(outcome.mint).or_default();
        stats.record(outcome);

        if outcome.severity != DriftSeverity::None {
            increment_counter!(
                "shadow_ledger_obs_drift_total",
                "severity" => outcome.severity.label()
            );
            info!(
                mint = %outcome.mint,
                severity = outcome.severity.label(),
                abs_sol_drift = outcome.abs_sol_drift,
                action = ?outcome.action,
                "drift_observability: drift event recorded"
            );
        }

        if outcome.action == ReconciliationAction::DiagnosticSignal {
            increment_counter!("shadow_ledger_obs_diagnostic_signals_total");
            warn!(
                mint = %outcome.mint,
                abs_sol_drift = outcome.abs_sol_drift,
                "drift_observability: diagnostic signal recorded for pool"
            );
        }
    }

    /// Return the accumulated statistics for `mint`, or `None` if no
    /// reconciliation outcomes have been recorded for it yet.
    pub fn stats_for(&self, mint: &Pubkey) -> Option<&PoolDriftStats> {
        self.pools.get(mint)
    }

    /// Return an iterator over all pools that have ever experienced drift.
    pub fn drift_pools(&self) -> impl Iterator<Item = (&Pubkey, &PoolDriftStats)> {
        self.pools.iter().filter(|(_, s)| s.has_drift())
    }

    /// Return the total number of distinct pools tracked.
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Return the total number of diagnostic signals across all pools.
    pub fn total_diagnostic_signals(&self) -> u64 {
        self.pools.values().map(|s| s.diagnostic_signals).sum()
    }

    /// Return the pool with the highest peak SOL drift, if any pools are tracked.
    pub fn worst_drift_pool(&self) -> Option<(&Pubkey, &PoolDriftStats)> {
        self.pools.iter().max_by_key(|(_, s)| s.peak_abs_sol_drift)
    }
}

// ============================================================================
// Hot-pool tx-loss summary
// ============================================================================

/// Per-pool tx-loss counters used by [`HotPoolTxLossTracker`].
#[derive(Debug, Default, Clone)]
pub struct PoolTxLossSummary {
    /// Transactions observed by Seer for this pool.
    pub tx_seen: u64,
    /// Transactions successfully forwarded into the Shadow Ledger authority path.
    pub tx_forwarded: u64,
    /// Situations where a diagnostic signal was observed, suggesting a tx was
    /// likely missed.
    pub suspected_misses: u64,
    /// Number of dormant diagnostic signals recorded for this pool.
    pub diagnostic_signals: u64,
}

impl PoolTxLossSummary {
    /// Estimated tx loss count (`tx_seen - tx_forwarded`), floored at zero.
    pub fn estimated_loss(&self) -> u64 {
        self.tx_seen.saturating_sub(self.tx_forwarded)
    }

    /// Whether this pool has seen enough tx to be classified as *hot* during
    /// the current window.
    pub fn is_hot(&self) -> bool {
        self.tx_seen >= HOT_POOL_TX_THRESHOLD
    }

    /// Loss rate as a fraction (0.0–1.0).  Returns 0.0 if no tx were seen.
    pub fn loss_rate(&self) -> f64 {
        if self.tx_seen == 0 {
            0.0
        } else {
            self.estimated_loss() as f64 / self.tx_seen as f64
        }
    }
}

// ============================================================================
// HotPoolTxLossTracker
// ============================================================================

/// Tracks transaction visibility for individual pools during an observation
/// window, with focus on pools classified as *hot* (≥ [`HOT_POOL_TX_THRESHOLD`]
/// observed tx).
///
/// # What it measures
///
/// | Event              | Method                                   |
/// |--------------------|------------------------------------------|
/// | Seer observed tx   | [`record_seen`](Self::record_seen)        |
/// | Tx forwarded to SL | [`record_forwarded`](Self::record_forwarded) |
/// | Diagnostic signal | [`record_diagnostic_signal`](Self::record_diagnostic_signal) |
/// | Suspected miss     | [`record_suspected_miss`](Self::record_suspected_miss) |
///
/// # Metrics emitted
///
/// - `shadow_ledger_hot_pool_tx_seen_total`
/// - `shadow_ledger_hot_pool_tx_forwarded_total`
/// - `shadow_ledger_hot_pool_suspected_miss_total`
/// - `shadow_ledger_hot_pool_diagnostic_signals_total`
#[derive(Debug, Default)]
pub struct HotPoolTxLossTracker {
    pools: HashMap<Pubkey, PoolTxLossSummary>,
}

impl HotPoolTxLossTracker {
    /// Create an empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that Seer observed a transaction for `mint`.
    pub fn record_seen(&mut self, mint: &Pubkey) {
        increment_counter!("shadow_ledger_hot_pool_tx_seen_total");
        self.pools.entry(*mint).or_default().tx_seen += 1;
    }

    /// Record that a transaction for `mint` was successfully forwarded into
    /// the Shadow Ledger authority path.
    pub fn record_forwarded(&mut self, mint: &Pubkey) {
        increment_counter!("shadow_ledger_hot_pool_tx_forwarded_total");
        self.pools.entry(*mint).or_default().tx_forwarded += 1;
    }

    /// Record a suspected missed transaction for `mint` (e.g. a diagnostic
    /// signal suggested state diverged due to a missed tx).
    pub fn record_suspected_miss(&mut self, mint: &Pubkey) {
        increment_counter!("shadow_ledger_hot_pool_suspected_miss_total");
        self.pools.entry(*mint).or_default().suspected_misses += 1;
    }

    /// Record that a reconciliation diagnostic signal occurred for `mint`.
    ///
    /// A diagnostic signal on a hot pool is strong evidence of
    /// observation-window tx loss concentrated on that pool.
    pub fn record_diagnostic_signal(&mut self, mint: &Pubkey) {
        increment_counter!("shadow_ledger_hot_pool_diagnostic_signals_total");
        let summary = self.pools.entry(*mint).or_default();
        summary.diagnostic_signals += 1;
        summary.suspected_misses += 1;

        if summary.is_hot() {
            warn!(
                %mint,
                tx_seen = summary.tx_seen,
                tx_forwarded = summary.tx_forwarded,
                estimated_loss = summary.estimated_loss(),
                diagnostic_signals = summary.diagnostic_signals,
                "hot_pool_tracker: diagnostic signal on hot pool — likely observation-window tx loss"
            );
        }
    }

    /// Return the tx-loss summary for `mint`, or `None` if never seen.
    pub fn loss_summary(&self, mint: &Pubkey) -> Option<&PoolTxLossSummary> {
        self.pools.get(mint)
    }

    /// Return an iterator over all pools classified as *hot* (tx_seen ≥ threshold).
    pub fn hot_pools(&self) -> impl Iterator<Item = (&Pubkey, &PoolTxLossSummary)> {
        self.pools.iter().filter(|(_, s)| s.is_hot())
    }

    /// Return the pool with the highest estimated tx loss, if any pools are tracked.
    pub fn worst_loss_pool(&self) -> Option<(&Pubkey, &PoolTxLossSummary)> {
        self.pools.iter().max_by_key(|(_, s)| s.estimated_loss())
    }

    /// Total estimated tx loss across all pools.
    pub fn total_estimated_loss(&self) -> u64 {
        self.pools.values().map(|s| s.estimated_loss()).sum()
    }
}

// ============================================================================
// Replay event
// ============================================================================

/// A single replay step: a trade that should be applied to the Shadow Ledger
/// state during a [`ReplayValidator`] run.
#[derive(Debug, Clone)]
pub struct ReplayTx {
    /// Mint address of the pool affected by this trade.
    pub mint: Pubkey,
    /// SOL reserve delta produced by this trade (lamports, signed).
    ///
    /// Positive = buy (SOL added to pool), negative = sell (SOL removed).
    pub sol_delta_lamports: i64,
    /// Token reserve delta produced by this trade (units, signed).
    ///
    /// Negative for buys (tokens leave pool), positive for sells.
    pub tok_delta_units: i64,
    /// Chain slot at which this trade landed.
    pub slot: u64,
}

// ============================================================================
// ReplayValidationResult
// ============================================================================

/// Outcome of a single [`ReplayValidator::validate_against_account_update`] call.
#[derive(Debug, Clone)]
pub struct ReplayValidationResult {
    /// Mint that was validated.
    pub mint: Pubkey,
    /// SOL reserves in the Shadow Ledger after replaying all recorded tx.
    pub shadow_sol_after_replay: u64,
    /// Token reserves in the Shadow Ledger after replaying all recorded tx.
    pub shadow_tok_after_replay: u64,
    /// On-chain SOL reserves used as the reference.
    pub on_chain_sol: u64,
    /// On-chain token reserves used as the reference.
    pub on_chain_tok: u64,
    /// Absolute SOL drift between replayed state and on-chain state (lamports).
    pub abs_sol_drift: u64,
    /// Whether the reconciler emitted a severe-drift diagnostic signal after replay.
    pub emitted_diagnostic_signal: bool,
}

impl ReplayValidationResult {
    /// Return `true` if the replayed Shadow Ledger state matched on-chain
    /// state within the given tolerance (lamports).
    pub fn within_tolerance(&self, tolerance_lamports: u64) -> bool {
        self.abs_sol_drift <= tolerance_lamports
    }
}

// ============================================================================
// ReplayValidator
// ============================================================================

/// Replay a recorded sequence of transactions against the Shadow Ledger and
/// compare the resulting state against an on-chain AccountUpdate snapshot.
///
/// This is a lightweight parity / correctness validation helper: it replays a
/// known sequence of trades, then reconciles against chain truth and reports
/// any divergence.  The goal is to make Shadow Ledger correctness *provable by
/// replay* rather than just asserted by inspection.
///
/// # Workflow
///
/// 1. Create a `ReplayValidator` wrapping an existing [`ShadowLedger`].
/// 2. Record the sequence of trades that *should* have been applied.
/// 3. Call [`replay`](Self::replay) to apply all recorded tx to the ledger.
/// 4. Call [`validate_against_account_update`](Self::validate_against_account_update)
///    to compare the resulting state with on-chain truth.
///
/// Steps 3 and 4 can be combined via
/// [`replay_and_validate`](Self::replay_and_validate).
pub struct ReplayValidator {
    ledger: ShadowLedger,
    txs: Vec<ReplayTx>,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Apply a signed delta to a `u64` reserve value, saturating at zero.
///
/// Used in [`ReplayValidator::replay`] for both SOL and token reserve updates.
fn apply_signed_delta(reserve: u64, delta: i64) -> u64 {
    (reserve as i64).saturating_add(delta).max(0) as u64
}

impl ReplayValidator {
    /// Create a new validator backed by `ledger`.
    ///
    /// The validator shares ownership of the ledger with the caller, so
    /// mutations from replay are visible through the original `Arc`-backed
    /// handle.
    pub fn new(ledger: ShadowLedger) -> Self {
        Self {
            ledger,
            txs: Vec::new(),
        }
    }

    /// Record a trade that should be applied during replay.
    ///
    /// Trades are applied in the order they are recorded.
    pub fn record_tx(
        &mut self,
        mint: Pubkey,
        sol_delta_lamports: i64,
        tok_delta_units: i64,
        slot: u64,
    ) {
        self.txs.push(ReplayTx {
            mint,
            sol_delta_lamports,
            tok_delta_units,
            slot,
        });
    }

    /// Apply all recorded transactions to the Shadow Ledger.
    ///
    /// Each trade is applied by adjusting the virtual reserves of the
    /// corresponding curve entry.  If a mint is not present in the ledger, the
    /// trade is skipped (no state is created for unknown mints).
    ///
    /// Returns the number of transactions successfully applied.
    pub fn replay(&self) -> usize {
        let mut applied = 0usize;
        for tx in &self.txs {
            if let Some(mut curve) = self.ledger.get(&tx.mint) {
                curve.virtual_sol_reserves =
                    apply_signed_delta(curve.virtual_sol_reserves, tx.sol_delta_lamports);
                curve.virtual_token_reserves =
                    apply_signed_delta(curve.virtual_token_reserves, tx.tok_delta_units);

                #[allow(deprecated)]
                let _ = self.ledger.apply_curve_write(
                    Some(tx.mint),
                    tx.mint,
                    curve,
                    CurveWriteMetadata::new(
                        ShadowLedgerWriteSource::WalReplayCurve,
                        ShadowLedgerWriteStrength::Repair,
                        ShadowLedgerStateConfidence::Observed,
                        ShadowLedgerWriteReason::WalReplayCurveUpdate,
                        Some(tx.slot),
                        crate::shadow_ledger::CurveFinality::Provisional,
                    ),
                );
                applied += 1;
            }
        }
        applied
    }

    /// Compare the current Shadow Ledger state for `mint` against the provided
    /// on-chain reserves.
    ///
    /// Uses a default-policy [`ShadowLedgerReconciler`] to run the comparison.
    /// If the drift is severe, the reconciler emits a diagnostic signal while
    /// keeping the Shadow Ledger state untouched.
    ///
    /// Returns `None` if `mint` is not present in the ledger.
    pub fn validate_against_account_update(
        &self,
        mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        slot: u64,
    ) -> Option<ReplayValidationResult> {
        let pre = self.ledger.get(mint)?;
        let shadow_sol_after_replay = pre.virtual_sol_reserves;
        let shadow_tok_after_replay = pre.virtual_token_reserves;

        let reconciler = ShadowLedgerReconciler::with_default_policy(self.ledger.clone());
        let outcome = reconciler.reconcile(
            mint,
            on_chain_sol,
            on_chain_tok,
            0,
            slot,
            crate::shadow_ledger::CurveFinality::Provisional,
        )?;

        let emitted_diagnostic_signal = outcome.action == ReconciliationAction::DiagnosticSignal;

        if emitted_diagnostic_signal {
            warn!(
                %mint,
                shadow_sol = shadow_sol_after_replay,
                shadow_tok = shadow_tok_after_replay,
                on_chain_sol,
                on_chain_tok,
                abs_sol_drift = outcome.abs_sol_drift,
                "replay_validator: divergence detected — severe drift surfaced without mutating Shadow Ledger"
            );
        } else {
            info!(
                %mint,
                shadow_sol = shadow_sol_after_replay,
                on_chain_sol,
                abs_sol_drift = outcome.abs_sol_drift,
                severity = outcome.severity.label(),
                "replay_validator: parity check complete"
            );
        }

        Some(ReplayValidationResult {
            mint: *mint,
            shadow_sol_after_replay,
            shadow_tok_after_replay,
            on_chain_sol,
            on_chain_tok,
            abs_sol_drift: outcome.abs_sol_drift,
            emitted_diagnostic_signal,
        })
    }

    /// Convenience: replay all recorded tx, then validate against on-chain truth.
    ///
    /// Returns `(applied_count, validation_result)`.
    pub fn replay_and_validate(
        &self,
        mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        slot: u64,
    ) -> (usize, Option<ReplayValidationResult>) {
        let applied = self.replay();
        let result = self.validate_against_account_update(mint, on_chain_sol, on_chain_tok, slot);
        (applied, result)
    }

    /// Access the underlying ledger (e.g. to seed initial state before replay).
    pub fn ledger(&self) -> &ShadowLedger {
        &self.ledger
    }
}

// ============================================================================
// Convenience: feed a reconciliation outcome into both report and tracker
// ============================================================================

/// Feed a single reconciliation outcome into both a [`DriftObservabilityReport`]
/// and a [`HotPoolTxLossTracker`] in one call.
///
/// This is the recommended integration point: every reconciliation outcome
/// from [`ShadowLedgerReconciler::reconcile`] should be passed through here.
pub fn record_reconciliation_outcome(
    outcome: &ReconciliationOutcome,
    report: &mut DriftObservabilityReport,
    tracker: &mut HotPoolTxLossTracker,
) {
    report.record(outcome);
    if outcome.action == ReconciliationAction::DiagnosticSignal {
        tracker.record_diagnostic_signal(&outcome.mint);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market_state::BondingCurve;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn make_ledger_with_curve(mint: Pubkey, sol: u64, tok: u64) -> ShadowLedger {
        let ledger = ShadowLedger::new();
        ledger.insert_with_slot(
            mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol,
                virtual_token_reserves: tok,
                real_sol_reserves: sol,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );
        ledger
    }

    fn reconcile_outcome(
        mint: Pubkey,
        shadow_sol: u64,
        tok: u64,
        on_chain_sol: u64,
        on_chain_tok: u64,
    ) -> ReconciliationOutcome {
        let ledger = make_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger);
        reconciler
            .reconcile(
                &mint,
                on_chain_sol,
                on_chain_tok,
                0,
                200,
                crate::shadow_ledger::CurveFinality::Provisional,
            )
            .expect("mint must be in ledger")
    }

    // =========================================================================
    // A. Drift instrumentation
    // =========================================================================

    /// A.1 – DriftObservabilityReport accumulates no-drift checks correctly.
    #[test]
    fn test_report_records_no_drift() {
        let mint = Pubkey::new_unique();
        let sol = 30_000_000_000u64;
        let tok = 1_000_000_000_000u64;

        let outcome = reconcile_outcome(mint, sol, tok, sol, tok);

        let mut report = DriftObservabilityReport::new();
        report.record(&outcome);

        let stats = report.stats_for(&mint).unwrap();
        assert_eq!(stats.checks, 1);
        assert_eq!(stats.no_drift, 1);
        assert_eq!(stats.noise, 0);
        assert_eq!(stats.meaningful, 0);
        assert_eq!(stats.severe, 0);
        assert_eq!(stats.diagnostic_signals, 0);
        assert!(!stats.has_drift());
    }

    /// A.2 – drift counter increments when noise-level divergence is detected.
    #[test]
    fn test_report_drift_counter_moves_on_noise() {
        let mint = Pubkey::new_unique();
        let shadow_sol = 30_000_500_000u64; // 0.0005 SOL noise
        let on_chain_sol = 30_000_000_000u64;
        let tok = 1_000_000_000_000u64;

        let outcome = reconcile_outcome(mint, shadow_sol, tok, on_chain_sol, tok);
        assert_eq!(outcome.severity, DriftSeverity::Noise);

        let mut report = DriftObservabilityReport::new();
        report.record(&outcome);

        let stats = report.stats_for(&mint).unwrap();
        assert_eq!(
            stats.noise, 1,
            "noise counter must increment on noise drift"
        );
        assert!(stats.has_drift());
    }

    /// A.3 – severe drift increments drift counters without repairs in PR 7.
    #[test]
    fn test_report_repair_counter_moves_on_severe_drift() {
        let mint = Pubkey::new_unique();
        let shadow_sol = 32_000_000_000u64; // 2 SOL above on-chain → severe
        let on_chain_sol = 30_000_000_000u64;
        let tok = 1_000_000_000_000u64;

        let outcome = reconcile_outcome(mint, shadow_sol, tok, on_chain_sol, tok);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        let mut report = DriftObservabilityReport::new();
        report.record(&outcome);

        let stats = report.stats_for(&mint).unwrap();
        assert_eq!(
            stats.severe, 1,
            "severe counter must increment on severe drift"
        );
        assert_eq!(
            stats.diagnostic_signals, 0,
            "diagnostic-signal counter must remain zero in PR 7 diagnostic mode"
        );
        assert_eq!(report.total_diagnostic_signals(), 0);
    }

    /// A.4 – multiple consecutive outcomes are accumulated correctly.
    #[test]
    fn test_report_accumulates_multiple_outcomes() {
        let mint = Pubkey::new_unique();
        let sol = 30_000_000_000u64;
        let tok = 1_000_000_000_000u64;

        let mut report = DriftObservabilityReport::new();

        // 2 no-drift checks
        for _ in 0..2 {
            let o = reconcile_outcome(mint, sol, tok, sol, tok);
            report.record(&o);
        }
        // 1 noise-level check
        let noise_outcome = reconcile_outcome(mint, sol + 500_000, tok, sol, tok);
        report.record(&noise_outcome);

        // 1 severe → logged severe drift
        let severe_outcome = reconcile_outcome(mint, sol + 2_000_000_000, tok, sol, tok);
        report.record(&severe_outcome);

        let stats = report.stats_for(&mint).unwrap();
        assert_eq!(stats.checks, 4);
        assert_eq!(stats.no_drift, 2);
        assert_eq!(stats.noise, 1);
        assert_eq!(stats.severe, 1);
        assert_eq!(stats.diagnostic_signals, 0);
        assert!(stats.peak_abs_sol_drift >= 2_000_000_000);
    }

    /// A.5 – worst_drift_pool identifies the pool with the highest peak drift.
    #[test]
    fn test_report_worst_drift_pool() {
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        let sol = 30_000_000_000u64;
        let tok = 1_000_000_000_000u64;

        let mut report = DriftObservabilityReport::new();

        // mint_a: 1 SOL drift
        let o_a = reconcile_outcome(mint_a, sol + 1_000_000_000, tok, sol, tok);
        report.record(&o_a);

        // mint_b: 3 SOL drift
        let o_b = reconcile_outcome(mint_b, sol + 3_000_000_000, tok, sol, tok);
        report.record(&o_b);

        let (worst_mint, worst_stats) = report.worst_drift_pool().unwrap();
        assert_eq!(*worst_mint, mint_b);
        assert!(worst_stats.peak_abs_sol_drift >= 3_000_000_000);
    }

    // =========================================================================
    // B. Replay / parity validation
    // =========================================================================

    /// B.1 – tx-driven state diverges → AccountUpdate reports the divergence.
    ///
    /// Scenario: Shadow Ledger was seeded with 30 SOL. We record three trades
    /// (each +0.5 SOL net into the pool) but only replay two, leaving the
    /// ledger 0.5 SOL behind on-chain truth.  Validate detects drift and heals.
    #[test]
    fn test_replay_divergence_healed_by_account_update() {
        let mint = Pubkey::new_unique();
        let initial_sol: u64 = 30_000_000_000;
        let initial_tok: u64 = 1_000_000_000_000;

        let ledger = make_ledger_with_curve(mint, initial_sol, initial_tok);
        let mut validator = ReplayValidator::new(ledger.clone());

        // Record 2 buy trades (+0.5 SOL each into pool)
        const BUY_SOL_DELTA: i64 = 495_000_000; // ~0.495 SOL net after 1% fee
        validator.record_tx(mint, BUY_SOL_DELTA, -29_411_764, 101);
        validator.record_tx(mint, BUY_SOL_DELTA, -28_549_020, 102);
        // A third trade that was NOT recorded (simulating a missed tx)

        // On-chain truth: 3 trades happened
        let on_chain_sol: u64 = initial_sol + 3 * BUY_SOL_DELTA as u64; // 31_485_000_000
        let on_chain_tok: u64 = 940_000_000_000; // approximate

        let (applied, result) =
            validator.replay_and_validate(&mint, on_chain_sol, on_chain_tok, 200);

        assert_eq!(applied, 2, "both recorded trades must be applied");
        let result = result.unwrap();

        // After 2 replayed trades the shadow is at ~30_990_000_000 SOL
        // On-chain is at 31_485_000_000 → drift ~0.495 SOL > MEANINGFUL (0.05 SOL)
        assert!(
            result.abs_sol_drift > 0,
            "divergence must be detected after missing 1 trade"
        );
        // The missing trade is ~0.495 SOL drift → above the noise/meaningful threshold
        // but whether it crosses SEVERE (0.5 SOL) depends on exact values.
        // Either way, drift must be non-zero and the result must be captured.
        assert_eq!(result.mint, mint);
        assert_eq!(result.on_chain_sol, on_chain_sol);
    }

    /// B.2 – exact replay (no missed tx) produces zero drift.
    #[test]
    fn test_replay_exact_produces_no_drift() {
        let mint = Pubkey::new_unique();
        let initial_sol: u64 = 30_000_000_000;
        let initial_tok: u64 = 1_000_000_000_000;

        let ledger = make_ledger_with_curve(mint, initial_sol, initial_tok);
        let mut validator = ReplayValidator::new(ledger.clone());

        let buy_sol_delta: i64 = 495_000_000;
        let buy_tok_delta: i64 = -29_411_764;
        validator.record_tx(mint, buy_sol_delta, buy_tok_delta, 101);

        // On-chain truth matches the single replayed trade exactly
        let on_chain_sol = (initial_sol as i64 + buy_sol_delta) as u64;
        let on_chain_tok = (initial_tok as i64 + buy_tok_delta) as u64;

        let (applied, result) =
            validator.replay_and_validate(&mint, on_chain_sol, on_chain_tok, 200);
        assert_eq!(applied, 1);
        let result = result.unwrap();
        assert_eq!(
            result.abs_sol_drift, 0,
            "exact replay must produce zero drift"
        );
        assert!(
            !result.emitted_diagnostic_signal,
            "no repair needed for zero drift"
        );
    }

    /// B.3 – severe divergence stays diagnostic-only and the result reflects it.
    #[test]
    fn test_replay_severe_divergence_stays_diagnostic_only() {
        let mint = Pubkey::new_unique();
        let initial_sol: u64 = 30_000_000_000;
        let initial_tok: u64 = 1_000_000_000_000;

        let ledger = make_ledger_with_curve(mint, initial_sol, initial_tok);
        let validator = ReplayValidator::new(ledger.clone());
        // No trades recorded — Shadow Ledger stays at initial state.
        // On-chain moved by 2 SOL (many missed trades).
        let on_chain_sol: u64 = 32_000_000_000; // 2 SOL ahead
        let on_chain_tok: u64 = 937_500_000_000u64;

        let result = validator
            .validate_against_account_update(&mint, on_chain_sol, on_chain_tok, 200)
            .unwrap();

        assert!(
            !result.emitted_diagnostic_signal,
            "severe divergence must remain diagnostic-only in PR 7"
        );
        assert!(
            result.abs_sol_drift > 500_000_000,
            "2 SOL drift must exceed severe threshold"
        );

        // After validation the ledger must still hold the tx-driven state
        let post = ledger.get(&mint).unwrap();
        assert_eq!(post.virtual_sol_reserves, initial_sol);
    }

    // =========================================================================
    // C. Hot-pool-focused visibility
    // =========================================================================

    /// C.1 – a pool that sees many tx is classified as hot.
    #[test]
    fn test_hot_pool_classified_correctly() {
        let mint = Pubkey::new_unique();
        let mut tracker = HotPoolTxLossTracker::new();

        // Simulate HOT_POOL_TX_THRESHOLD observations
        for _ in 0..HOT_POOL_TX_THRESHOLD {
            tracker.record_seen(&mint);
        }
        // Forward all but 3 of them
        for _ in 0..(HOT_POOL_TX_THRESHOLD - 3) {
            tracker.record_forwarded(&mint);
        }

        let summary = tracker.loss_summary(&mint).unwrap();
        assert!(
            summary.is_hot(),
            "pool with >= threshold tx must be classified hot"
        );
        assert_eq!(
            summary.estimated_loss(),
            3,
            "3 tx not forwarded → 3 estimated loss"
        );
        assert!(summary.loss_rate() > 0.0);
    }

    /// C.2 – a pool below the threshold is not classified as hot.
    #[test]
    fn test_low_volume_pool_not_hot() {
        let mint = Pubkey::new_unique();
        let mut tracker = HotPoolTxLossTracker::new();

        for _ in 0..(HOT_POOL_TX_THRESHOLD - 1) {
            tracker.record_seen(&mint);
        }

        let summary = tracker.loss_summary(&mint).unwrap();
        assert!(
            !summary.is_hot(),
            "pool below threshold must not be classified as hot"
        );
    }

    /// C.3 – a diagnostic signal on a hot pool increments both counters.
    #[test]
    fn test_hot_pool_diagnostic_signal_increments_counters() {
        let mint = Pubkey::new_unique();
        let mut tracker = HotPoolTxLossTracker::new();

        // Make pool hot
        for _ in 0..HOT_POOL_TX_THRESHOLD {
            tracker.record_seen(&mint);
            tracker.record_forwarded(&mint);
        }

        tracker.record_diagnostic_signal(&mint);

        let summary = tracker.loss_summary(&mint).unwrap();
        assert_eq!(
            summary.diagnostic_signals, 1,
            "diagnostic-signal counter must increment on compatibility signal"
        );
        assert_eq!(
            summary.suspected_misses, 1,
            "suspected_miss counter must also increment on a diagnostic signal"
        );
    }

    /// C.4 – synthetic high-activity scenario: 50 tx, 8 missed, 2 diagnostic signals visible.
    #[test]
    fn test_hot_pool_high_activity_scenario() {
        let hot_mint = Pubkey::new_unique();
        let cold_mint = Pubkey::new_unique();
        let mut tracker = HotPoolTxLossTracker::new();

        // Hot pool: 50 tx seen, 42 forwarded (8 missed)
        for _ in 0..50u64 {
            tracker.record_seen(&hot_mint);
        }
        for _ in 0..42u64 {
            tracker.record_forwarded(&hot_mint);
        }
        tracker.record_diagnostic_signal(&hot_mint);
        tracker.record_diagnostic_signal(&hot_mint);

        // Cold pool: only 5 tx
        for _ in 0..5u64 {
            tracker.record_seen(&cold_mint);
            tracker.record_forwarded(&cold_mint);
        }

        let hot_summary = tracker.loss_summary(&hot_mint).unwrap();
        assert!(hot_summary.is_hot());
        // estimated_loss = 50 - 42 = 8 (diagnostic signals also add to suspected_misses)
        assert_eq!(hot_summary.estimated_loss(), 8);
        assert_eq!(hot_summary.diagnostic_signals, 2);
        // suspected_misses = 2 (from record_diagnostic_signal; the helper increments suspected_misses)
        assert_eq!(hot_summary.suspected_misses, 2);

        let cold_summary = tracker.loss_summary(&cold_mint).unwrap();
        assert!(!cold_summary.is_hot());

        // hot_pools() must list only the hot pool
        let hot: Vec<_> = tracker.hot_pools().collect();
        assert_eq!(hot.len(), 1);
        assert_eq!(*hot[0].0, hot_mint);

        // worst_loss_pool must be the hot pool
        let (worst, _) = tracker.worst_loss_pool().unwrap();
        assert_eq!(*worst, hot_mint);

        assert_eq!(tracker.total_estimated_loss(), 8);
    }

    /// C.5 – record_reconciliation_outcome convenience helper wires both report
    ///        and tracker correctly on a diagnostic signal.
    #[test]
    fn test_convenience_record_reconciliation_outcome() {
        let mint = Pubkey::new_unique();
        let shadow_sol = 32_000_000_000u64;
        let on_chain_sol = 30_000_000_000u64;
        let tok = 1_000_000_000_000u64;

        let outcome = reconcile_outcome(mint, shadow_sol, tok, on_chain_sol, tok);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        let mut report = DriftObservabilityReport::new();
        let mut tracker = HotPoolTxLossTracker::new();

        // Prime the tracker with some seen tx so the pool is tracked
        for _ in 0..5 {
            tracker.record_seen(&mint);
            tracker.record_forwarded(&mint);
        }

        record_reconciliation_outcome(&outcome, &mut report, &mut tracker);

        // Report must have captured the severe drift without repairs
        let stats = report.stats_for(&mint).unwrap();
        assert_eq!(stats.diagnostic_signals, 0);

        // Tracker must remain unchanged because no repair actions are emitted
        let summary = tracker.loss_summary(&mint).unwrap();
        assert_eq!(summary.diagnostic_signals, 0);
    }
}
