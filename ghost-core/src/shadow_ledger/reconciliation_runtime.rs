//! Reconciliation Runtime — explicit production loop for Shadow Ledger observability
//!
//! # Purpose
//!
//! This module operationalises the Shadow Ledger + AccountUpdate model into a
//! production-ready runtime loop.  It wires together three previously-separate
//! building blocks:
//!
//! - [`ShadowLedgerReconciler`] — compares Shadow Ledger state with on-chain truth
//!   and classifies divergences without mutating state.
//! - [`DriftObservabilityReport`] — accumulates per-pool drift counters that
//!   operators can inspect in real time.
//! - [`HotPoolTxLossTracker`] — correlates hot-pool tx volume with severe drift
//!   pressure so ingest hardening can be measured.
//!
//! # Architecture fit
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────┐
//! │         Seer → GatekeeperMintBuffer → commit → ShadowLedger        │
//! │                   (tx-driven primary authority path)               │
//! └────────────────────────────────────┬───────────────────────────────┘
//!                                      │  AccountUpdate arrives
//!                                      ▼
//! ┌────────────────────────────────────────────────────────────────────┐
//! │              ReconciliationRuntime                                 │
//! │                                                                    │
//! │  process_account_update(mint, sol, tok, complete, slot, finality)  │
//! │    ├─► ShadowLedgerReconciler::reconcile()  (compare + log drift)  │
//! │    ├─► DriftObservabilityReport::record()   (per-pool counters)    │
//! │    └─► suppress unexpected legacy write-like actions               │
//! │                                                                    │
//! │  run_cycle(fetch_fn)                                               │
//! │    ├─► iterate registered pools (bounded)                          │
//! │    ├─► call fetch_fn per pool                                      │
//! │    └─► process_account_update() for each pool with data            │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Authority model — invariants
//!
//! - **Shadow Ledger remains tx-driven** for simulation/replay evolution.
//! - **AccountUpdate is diagnostic-only here** — reconciliation never overwrites.
//! - This runtime **does not create a competing state engine**: it observes and correlates.
//!
//! # Usage
//!
//! ```rust,ignore
//! use ghost_core::shadow_ledger::{ShadowLedger, reconciliation_runtime::ReconciliationRuntime};
//!
//! let ledger = ShadowLedger::new();
//! let mut runtime = ReconciliationRuntime::new(ledger.clone());
//!
//! // Register pools that should be periodically reconciled.
//! runtime.register_pool(mint_a);
//! runtime.register_pool(mint_b);
//!
//! // Record every Seer-observed tx for hot-pool pressure correlation.
//! runtime.record_tx_seen(&mint_a);
//! runtime.record_tx_forwarded(&mint_a);
//!
//! // Event-driven: call whenever an AccountUpdate arrives for a pool.
//! if let Some(outcome) = runtime.process_account_update(
//!     &mint_a,
//!     on_chain_sol,
//!     on_chain_tok,
//!     0,
//!     slot,
//!     ghost_core::CurveFinality::Provisional,
//! ) {
//!     // outcome.action == Logged if drift was severe
//! }
//!
//! // Scheduled: run a reconciliation cycle over all registered pools.
//! runtime.run_cycle(|mint| {
//!     // Return the latest on-chain data for this pool, or None to skip.
//!     fetch_on_chain_state(mint)
//! });
//!
//! // Inspect runtime health.
//! let status = runtime.status();
//! println!("drifting pools: {}", status.total_drifting_pools);
//! ```

use metrics::{histogram, increment_counter};
use solana_sdk::pubkey::Pubkey;
use tracing::{info, warn};

use crate::shadow_ledger::drift_observability::{
    DriftObservabilityReport, HotPoolTxLossTracker, PoolDriftStats, PoolTxLossSummary,
};
use crate::shadow_ledger::reconciliation::{
    DriftPolicy, ReconciliationAction, ReconciliationOutcome, ShadowLedgerReconciler,
};
use crate::shadow_ledger::{CurveFinality, ShadowLedger};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for [`ReconciliationRuntime`].
///
/// All limits are explicit to avoid unbounded memory growth or runaway cycles.
#[derive(Debug, Clone)]
pub struct ReconciliationRuntimeConfig {
    /// Maximum number of pools tracked in the active-pool registry.
    ///
    /// Once this limit is reached, [`ReconciliationRuntime::register_pool`]
    /// returns `false` and the new pool is not added.  The operator should
    /// either evict stale pools or raise the limit for their workload.
    ///
    /// Default: 1 000
    pub max_pools: usize,

    /// Maximum number of pools reconciled in a single [`run_cycle`] call.
    ///
    /// Limits the per-cycle cost when the pool registry is large.
    /// Pools are visited in registration order; remaining pools are deferred
    /// to the next cycle.
    ///
    /// Default: 100
    pub max_pools_per_cycle: usize,

    /// Alert threshold for emitting WARN + counter on large drift.
    ///
    /// This threshold is independent from any legacy action compatibility ballast.
    /// It can be set below `severe_lamports` when operators want alerts before
    /// the severe-drift threshold is crossed.
    ///
    /// Default: 50_000_000 lamports (same as meaningful threshold, 0.05 SOL).
    pub drift_alert_threshold_lamports: u64,
}

impl Default for ReconciliationRuntimeConfig {
    fn default() -> Self {
        Self {
            max_pools: 1_000,
            max_pools_per_cycle: 100,
            drift_alert_threshold_lamports:
                crate::shadow_ledger::reconciliation::MEANINGFUL_THRESHOLD_LAMPORTS,
        }
    }
}

// ============================================================================
// Status snapshot
// ============================================================================

/// Point-in-time health summary for [`ReconciliationRuntime`].
///
/// Returned by [`ReconciliationRuntime::status`].  Useful for dashboards,
/// alerting, and deciding whether to increase reconciliation frequency.
#[derive(Debug, Clone)]
pub struct ReconciliationRuntimeStatus {
    /// Number of pools currently in the active-pool registry.
    pub registered_pools: usize,
    /// Total reconciliation checks performed since the runtime was created.
    pub total_checks: u64,
    /// Diagnostic counter retained in the status shape.
    ///
    /// Monitoring-only reconciliation suppresses legacy write-like actions, so this
    /// should remain `0`.
    pub total_diagnostic_signals: u64,
    /// Number of distinct pools that have experienced any drift.
    pub total_drifting_pools: usize,
    /// Mint of the pool with the highest peak SOL drift, if any.
    pub worst_drift_mint: Option<Pubkey>,
    /// Peak absolute SOL drift seen across all pools (lamports).
    pub worst_drift_lamports: u64,
    /// Number of pools currently classified as *hot*
    /// (≥ [`HOT_POOL_TX_THRESHOLD`] observed tx).
    pub total_hot_pools: usize,
    /// Estimated total tx loss across all tracked pools.
    pub total_estimated_tx_loss: u64,
    /// Number of completed reconciliation cycles.
    pub cycle_count: u64,
    /// Number of times drift exceeded the configured alert threshold.
    pub total_critical_drift_alerts: u64,
    /// Configured alert threshold currently in use.
    pub drift_alert_threshold_lamports: u64,
}

// ============================================================================
// ReconciliationRuntime — main production loop
// ============================================================================

/// Explicit production reconciliation / observability loop for the Shadow Ledger.
///
/// This struct is *the* integration point described in the problem statement:
///
/// > *"Shadow Ledger now runs with an explicit runtime reconciliation/observability
/// > loop: tx-driven state advances quickly, AccountUpdate surfaces drift, and
/// > drift behaviour is visible per pool in production."*
///
/// ## What it does
///
/// 1. Maintains a **bounded registry** of pools whose reconciliation is actively
///    tracked.
/// 2. Accepts **event-driven AccountUpdate signals** via
///    [`process_account_update`](Self::process_account_update) and immediately
///    reconciles + records the outcome.
/// 3. Supports a **scheduled cycle** via [`run_cycle`](Self::run_cycle) which
///    iterates the registry and reconciles each pool whose on-chain data is
///    provided by the caller.
/// 4. Exposes a unified [`status`](Self::status) for operational health.
///
/// ## Thread safety
///
/// `ReconciliationRuntime` is not `Sync` by itself.  The underlying
/// [`ShadowLedger`] is `Arc`-backed and fully thread-safe; the runtime struct
/// itself is designed to be owned by a single coordination task.  Wrap it in a
/// `Mutex` if you need to share it across threads.
pub struct ReconciliationRuntime {
    reconciler: ShadowLedgerReconciler,
    report: DriftObservabilityReport,
    tracker: HotPoolTxLossTracker,
    /// Ordered registry of pools under active reconciliation.
    registered_pools: Vec<Pubkey>,
    /// Offset into `registered_pools` for round-robin cycle scheduling.
    cycle_offset: usize,
    config: ReconciliationRuntimeConfig,
    total_checks: u64,
    cycle_count: u64,
    critical_drift_alerts: u64,
}

impl ReconciliationRuntime {
    // ── Constructors ─────────────────────────────────────────────────────────

    /// Create a new runtime backed by `ledger` with default production settings.
    pub fn new(ledger: ShadowLedger) -> Self {
        Self::with_config(ledger, ReconciliationRuntimeConfig::default())
    }

    /// Create a new runtime backed by `ledger` with a custom [`ReconciliationRuntimeConfig`].
    pub fn with_config(ledger: ShadowLedger, config: ReconciliationRuntimeConfig) -> Self {
        Self {
            reconciler: ShadowLedgerReconciler::with_default_policy(ledger),
            report: DriftObservabilityReport::new(),
            tracker: HotPoolTxLossTracker::new(),
            registered_pools: Vec::new(),
            cycle_offset: 0,
            config,
            total_checks: 0,
            cycle_count: 0,
            critical_drift_alerts: 0,
        }
    }

    /// Create a new runtime backed by `ledger` with a custom [`DriftPolicy`].
    pub fn with_policy(ledger: ShadowLedger, policy: DriftPolicy) -> Self {
        Self {
            reconciler: ShadowLedgerReconciler::new(ledger, policy),
            report: DriftObservabilityReport::new(),
            tracker: HotPoolTxLossTracker::new(),
            registered_pools: Vec::new(),
            cycle_offset: 0,
            config: ReconciliationRuntimeConfig::default(),
            total_checks: 0,
            cycle_count: 0,
            critical_drift_alerts: 0,
        }
    }

    // ── Pool registry ─────────────────────────────────────────────────────────

    /// Register `mint` for periodic reconciliation tracking.
    ///
    /// Returns `true` if the pool was added, `false` if:
    /// - the pool was already registered (idempotent, no duplicate), or
    /// - the registry is at [`ReconciliationRuntimeConfig::max_pools`] capacity.
    pub fn register_pool(&mut self, mint: Pubkey) -> bool {
        if self.registered_pools.contains(&mint) {
            return true; // idempotent
        }
        if self.registered_pools.len() >= self.config.max_pools {
            warn!(
                %mint,
                cap = self.config.max_pools,
                "reconciliation_runtime: pool registry at capacity — not registered"
            );
            increment_counter!("shadow_ledger_runtime_registry_overflow_total");
            return false;
        }
        self.registered_pools.push(mint);
        increment_counter!("shadow_ledger_runtime_pools_registered_total");
        true
    }

    /// Remove `mint` from the active-pool registry.
    ///
    /// Has no effect if the pool was not registered.
    pub fn unregister_pool(&mut self, mint: &Pubkey) {
        self.registered_pools.retain(|m| m != mint);
    }

    /// Number of pools currently in the active-pool registry.
    pub fn registered_pool_count(&self) -> usize {
        self.registered_pools.len()
    }

    /// Select the next bounded round-robin batch of registered pools.
    ///
    /// Advances the internal cycle offset exactly like [`run_cycle`], but
    /// leaves fetching/execution to the caller. This is useful for async
    /// schedulers that must fetch on-chain state outside of this sync core.
    pub fn next_cycle_batch(&mut self) -> Vec<Pubkey> {
        self.cycle_count += 1;
        increment_counter!("shadow_ledger_runtime_cycles_total");

        let pool_count = self.registered_pools.len();
        if pool_count == 0 {
            return Vec::new();
        }

        let cap = self.config.max_pools_per_cycle.min(pool_count);
        let start = self.cycle_offset % pool_count;
        let mut batch = Vec::with_capacity(cap);
        for i in 0..cap {
            let idx = (start + i) % pool_count;
            batch.push(self.registered_pools[idx]);
        }

        self.cycle_offset = (start + cap) % pool_count;
        batch
    }

    // ── Tx-visibility recording (hot-pool pressure correlation) ──────────────

    /// Record that Seer observed a transaction for `mint`.
    ///
    /// Call this on every tx that Seer parses for a pool, regardless of
    /// whether it passes the Shadow Ledger authority path.  This is the
    /// baseline for hot-pool pressure measurement.
    pub fn record_tx_seen(&mut self, mint: &Pubkey) {
        self.tracker.record_seen(mint);
    }

    /// Record that a transaction for `mint` was successfully forwarded into
    /// the Shadow Ledger authority path (Gatekeeper → commit).
    ///
    /// Call this every time a tx successfully reaches the commit path.  The
    /// difference between [`record_tx_seen`](Self::record_tx_seen) and this
    /// counter is the estimated tx loss for this pool.
    pub fn record_tx_forwarded(&mut self, mint: &Pubkey) {
        self.tracker.record_forwarded(mint);
    }

    // ── Event-driven reconciliation ───────────────────────────────────────────

    /// Process an AccountUpdate for `mint`.
    ///
    /// This is the **primary integration point** for event-driven reconciliation.
    /// Call it every time an on-chain AccountUpdate arrives for a pool that the
    /// Shadow Ledger tracks.
    ///
    /// ## What it does
    ///
    /// 1. Calls [`ShadowLedgerReconciler::reconcile`] — compares Shadow Ledger
    ///    state with the provided on-chain reserves and logs severe drift.
    /// 2. Feeds the outcome into [`DriftObservabilityReport`] — updates per-pool
    ///    drift counters (checks, noise, meaningful, severe, legacy compat, peak drift).
    /// 3. If a legacy write-like action ever reappears, suppresses it back to
    ///    monitoring-only semantics and emits an explicit compatibility metric.
    ///
    /// Returns the [`ReconciliationOutcome`] for the caller's inspection, or
    /// `None` if `mint` is not tracked by the Shadow Ledger.
    ///
    /// ## Authority model
    ///
    /// AccountUpdate is **diagnostic-only** here. The Shadow Ledger tx-driven path
    /// remains untouched; this call only measures drift and records observability.
    pub fn process_account_update(
        &mut self,
        mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        on_chain_complete: u8,
        slot: u64,
        curve_finality: CurveFinality,
    ) -> Option<ReconciliationOutcome> {
        let mut outcome = self.reconciler.reconcile(
            mint,
            on_chain_sol,
            on_chain_tok,
            on_chain_complete,
            slot,
            curve_finality,
        )?;

        if outcome.action == ReconciliationAction::DiagnosticSignal {
            increment_counter!(
                "shadow_ledger_runtime_unexpected_reconciliation_action_total",
                "action" => "suppressed_diagnostic_signal"
            );
            warn!(
                %mint,
                abs_sol_drift = outcome.abs_sol_drift,
                slot,
                "reconciliation_runtime: suppressed unexpected legacy write-like reconciliation action in monitoring-only runtime"
            );
            outcome.action = ReconciliationAction::Logged;
        }

        self.total_checks += 1;
        increment_counter!("shadow_ledger_runtime_account_updates_processed_total");
        histogram!(
            "shadow_ledger_reconciliation_drift_lamports",
            outcome.abs_sol_drift as f64
        );

        if outcome.abs_sol_drift > self.config.drift_alert_threshold_lamports {
            self.critical_drift_alerts = self.critical_drift_alerts.saturating_add(1);
            increment_counter!("shadow_ledger_reconciliation_critical_drift_total");
            warn!(
                %mint,
                abs_sol_drift = outcome.abs_sol_drift,
                threshold_lamports = self.config.drift_alert_threshold_lamports,
                severity = outcome.severity.label(),
                "reconciliation_runtime: drift exceeded alert threshold"
            );
        }

        // Feed outcome into drift observability
        self.report.record(&outcome);
        Some(outcome)
    }

    // ── Scheduled cycle ───────────────────────────────────────────────────────

    /// Run a bounded reconciliation cycle over the active-pool registry.
    ///
    /// `fetch` is called once per pool visited in this cycle.  It should return
    /// the latest on-chain reserves for the pool, or `None` to skip that pool
    /// (e.g. if no fresh AccountUpdate data is available).
    ///
    /// ## Bounding
    ///
    /// At most [`ReconciliationRuntimeConfig::max_pools_per_cycle`] pools are
    /// visited per call, using round-robin scheduling across cycles to ensure all
    /// registered pools are eventually covered even when the registry is larger
    /// than the per-cycle cap.
    ///
    /// ## Returns
    ///
    /// The number of pools that were actually reconciled in this cycle (i.e.
    /// fetch returned `Some` and the mint was present in the Shadow Ledger).
    pub fn run_cycle<F>(&mut self, fetch: F) -> usize
    where
        F: Fn(&Pubkey) -> Option<(u64, u64, u8, u64)>,
    {
        let cycle_started = std::time::Instant::now();
        let batch = self.next_cycle_batch();
        if batch.is_empty() {
            histogram!("shadow_ledger_reconciliation_cycle_ms", 0.0);
            return 0;
        }

        let mut reconciled = 0usize;
        for mint in &batch {
            if let Some((sol, tok, complete, slot)) = fetch(&mint) {
                if self
                    .process_account_update(
                        mint,
                        sol,
                        tok,
                        complete,
                        slot,
                        CurveFinality::Provisional,
                    )
                    .is_some()
                {
                    reconciled += 1;
                }
            }
        }

        if reconciled > 0 {
            info!(
                cycle = self.cycle_count,
                pools_visited = batch.len(),
                pools_reconciled = reconciled,
                "reconciliation_runtime: cycle complete"
            );
        }

        histogram!(
            "shadow_ledger_reconciliation_cycle_ms",
            cycle_started.elapsed().as_secs_f64() * 1000.0
        );

        reconciled
    }

    // ── Status / observability accessors ─────────────────────────────────────

    /// Return a point-in-time health summary of the reconciliation runtime.
    ///
    /// Suitable for dashboards, alerting thresholds, and operational health checks.
    pub fn status(&self) -> ReconciliationRuntimeStatus {
        let drifting_pools = self.report.drift_pools().count();
        let (worst_mint, worst_lamports) = self
            .report
            .worst_drift_pool()
            .map(|(m, s)| (Some(*m), s.peak_abs_sol_drift))
            .unwrap_or((None, 0));

        let hot_pools = self.tracker.hot_pools().count();
        let total_tx_loss = self.tracker.total_estimated_loss();

        ReconciliationRuntimeStatus {
            registered_pools: self.registered_pools.len(),
            total_checks: self.total_checks,
            total_diagnostic_signals: self.report.total_diagnostic_signals(),
            total_drifting_pools: drifting_pools,
            worst_drift_mint: worst_mint,
            worst_drift_lamports: worst_lamports,
            total_hot_pools: hot_pools,
            total_estimated_tx_loss: total_tx_loss,
            cycle_count: self.cycle_count,
            total_critical_drift_alerts: self.critical_drift_alerts,
            drift_alert_threshold_lamports: self.config.drift_alert_threshold_lamports,
        }
    }

    /// Read-only access to the per-pool drift statistics.
    pub fn drift_report(&self) -> &DriftObservabilityReport {
        &self.report
    }

    /// Read-only access to the hot-pool tx-loss tracker.
    pub fn loss_tracker(&self) -> &HotPoolTxLossTracker {
        &self.tracker
    }

    /// Per-pool drift stats for `mint`, or `None` if no outcomes recorded yet.
    pub fn pool_drift_stats(&self, mint: &Pubkey) -> Option<&PoolDriftStats> {
        self.report.stats_for(mint)
    }

    /// Per-pool tx-loss summary for `mint`, or `None` if no tx were seen.
    pub fn pool_loss_summary(&self, mint: &Pubkey) -> Option<&PoolTxLossSummary> {
        self.tracker.loss_summary(mint)
    }

    /// Total number of completed reconciliation cycles.
    pub fn cycle_count(&self) -> u64 {
        self.cycle_count
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market_state::BondingCurve;
    use crate::shadow_ledger::reconciliation::{DriftSeverity, ReconciliationAction};
    use crate::shadow_ledger::ShadowLedger;

    // ── Helpers ──────────────────────────────────────────────────────────────

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

    const SOL: u64 = 30_000_000_000;
    const TOK: u64 = 1_000_000_000_000;
    const TEST_FINALITY: CurveFinality = CurveFinality::Provisional;

    // =========================================================================
    // A. Runtime reconciliation loop tests
    // =========================================================================

    /// A.1 – reconciliation is triggered through the actual runtime integration path.
    ///
    /// Verifies that `process_account_update` calls through to the reconciler and
    /// returns an outcome from the real runtime path.
    #[test]
    fn test_runtime_reconciliation_triggered_through_integration_path() {
        let mint = Pubkey::new_unique();
        let ledger = make_ledger_with_curve(mint, SOL, TOK);
        let mut runtime = ReconciliationRuntime::new(ledger);

        // Process an AccountUpdate with matching state (no drift expected)
        let outcome = runtime.process_account_update(&mint, SOL, TOK, 0, 1, TEST_FINALITY);
        assert!(
            outcome.is_some(),
            "reconciliation must return an outcome for a known mint"
        );
        let outcome = outcome.unwrap();
        assert_eq!(
            outcome.severity,
            DriftSeverity::None,
            "no drift expected when shadow matches on-chain"
        );
        assert_eq!(outcome.action, ReconciliationAction::NoAction);
        assert_eq!(
            runtime.status().total_checks,
            1,
            "total_checks must increment after reconciliation"
        );
    }

    /// A.2 – relevant pools are checked via `run_cycle`.
    ///
    /// Registers two pools, runs a cycle with on-chain data available for both,
    /// and verifies both were reconciled.
    #[test]
    fn test_runtime_cycle_checks_registered_pools() {
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();

        let ledger = ShadowLedger::new();
        for &mint in &[mint_a, mint_b] {
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: SOL,
                    virtual_token_reserves: TOK,
                    real_sol_reserves: SOL,
                    real_token_reserves: TOK,
                    token_total_supply: TOK,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            );
        }

        let mut runtime = ReconciliationRuntime::new(ledger);
        assert!(runtime.register_pool(mint_a));
        assert!(runtime.register_pool(mint_b));

        let reconciled = runtime.run_cycle(|_mint| Some((SOL, TOK, 0, 200)));
        assert_eq!(
            reconciled, 2,
            "both registered pools must be reconciled in cycle"
        );
        assert_eq!(
            runtime.cycle_count(),
            1,
            "cycle count must increment after run_cycle"
        );
    }

    /// A.3 – severe drift through the runtime path is logged without state rewrite.
    ///
    /// This is the full integration path: AccountUpdate arrives, runtime detects
    /// severe drift, and the runtime remains diagnostic-only.
    #[test]
    fn test_runtime_severe_drift_is_logged_through_integration_path() {
        let mint = Pubkey::new_unique();
        // Shadow ledger is 2 SOL above chain (well beyond 0.5 SOL severe threshold)
        let shadow_sol = SOL + 2_000_000_000;
        let ledger = make_ledger_with_curve(mint, shadow_sol, TOK);
        let shadow_ledger_clone = ledger.clone();
        let mut runtime = ReconciliationRuntime::new(ledger);

        let outcome = runtime
            .process_account_update(&mint, SOL, TOK, 0, 150, TEST_FINALITY)
            .expect("must return outcome for known mint");

        assert_eq!(
            outcome.severity,
            DriftSeverity::Severe,
            "2 SOL drift must be classified as severe"
        );
        assert_eq!(
            outcome.action,
            ReconciliationAction::Logged,
            "severe drift must remain diagnostic-only through runtime path"
        );

        // Verify the Shadow Ledger state was not mutated
        let retained_curve = shadow_ledger_clone.get(&mint).unwrap();
        assert_eq!(
            retained_curve.virtual_sol_reserves, shadow_sol,
            "Shadow Ledger must preserve tx-driven state after diagnostic-only reconcile"
        );

        // Verify the runtime surfaces drift without recording repairs
        let status = runtime.status();
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "PR 7 runtime must not record diagnostic signals"
        );
    }

    // =========================================================================
    // B. Observability integration tests
    // =========================================================================

    /// B.1 – real runtime reconciliation outcomes feed drift observability.
    ///
    /// Verifies that per-pool drift stats are populated via the actual runtime
    /// path, not just via direct report.record() calls.
    #[test]
    fn test_runtime_observability_fed_by_real_reconciliation_outcomes() {
        let mint = Pubkey::new_unique();
        // 0.6 SOL drift (above noise, above meaningful, above severe)
        let shadow_sol = SOL + 600_000_000;
        let ledger = make_ledger_with_curve(mint, shadow_sol, TOK);
        let mut runtime = ReconciliationRuntime::new(ledger);

        // Run reconciliation through the runtime path
        runtime
            .process_account_update(&mint, SOL, TOK, 0, 1, TEST_FINALITY)
            .unwrap();

        // Per-pool drift stats must be visible through the runtime's observability
        let stats = runtime
            .pool_drift_stats(&mint)
            .expect("pool drift stats must exist after reconciliation");
        assert_eq!(stats.checks, 1, "check counter must reflect runtime call");
        assert_eq!(stats.severe, 1, "severe counter must be set");
        assert_eq!(
            stats.diagnostic_signals, 0,
            "diagnostic-signal counter must stay zero in PR 7"
        );
        assert!(
            stats.peak_abs_sol_drift >= 600_000_000,
            "peak drift must record the measured drift"
        );
    }

    /// B.2 – severe drift remains observable while diagnostic-signal counters stay zero.
    ///
    /// Sends multiple outcomes through the runtime and verifies that the
    /// accumulated diagnostic-signal count matches reality.
    #[test]
    fn test_runtime_repair_count_stays_zero() {
        let mint = Pubkey::new_unique();
        let ledger = ShadowLedger::new();

        let mut runtime = ReconciliationRuntime::new(ledger.clone());

        // Helper to insert a fresh drifted curve state
        let insert_drifted = |ledger: &ShadowLedger, sol_offset: u64| {
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: SOL + sol_offset,
                    virtual_token_reserves: TOK,
                    real_sol_reserves: SOL + sol_offset,
                    real_token_reserves: TOK,
                    token_total_supply: TOK,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            );
        };

        // Two severe-drift AccountUpdates → should still produce zero repairs
        for i in 0u64..2 {
            insert_drifted(&ledger, 2_000_000_000); // always 2 SOL above chain
            runtime
                .process_account_update(&mint, SOL, TOK, 0, 200 + i, TEST_FINALITY)
                .unwrap();
        }

        let status = runtime.status();
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "diagnostic-only runtime must not accumulate diagnostic signals"
        );

        let stats = runtime.pool_drift_stats(&mint).unwrap();
        assert_eq!(stats.checks, 2);
        assert_eq!(stats.diagnostic_signals, 0);
    }

    // =========================================================================
    // C. Hot-pool correlation tests
    // =========================================================================

    /// C.1 – a high-activity pool can be observed as hot even without diagnostic signals.
    ///
    /// Records enough tx to classify the pool as hot, then triggers severe drift,
    /// and verifies the hot-pool view remains visible from the runtime path.
    #[test]
    fn test_runtime_hot_pool_observed_as_hot_without_repairs() {
        use crate::shadow_ledger::drift_observability::HOT_POOL_TX_THRESHOLD;

        let mint = Pubkey::new_unique();
        // Shadow is severely drifted from the start
        let shadow_sol = SOL + 2_000_000_000;
        let ledger = make_ledger_with_curve(mint, shadow_sol, TOK);
        let mut runtime = ReconciliationRuntime::new(ledger);

        // Flood tx observations — classify the pool as hot
        for _ in 0..HOT_POOL_TX_THRESHOLD {
            runtime.record_tx_seen(&mint);
        }
        // Some tx reach the authority path, some don't (simulating partial loss)
        for _ in 0..HOT_POOL_TX_THRESHOLD / 2 {
            runtime.record_tx_forwarded(&mint);
        }

        // AccountUpdate triggers severe drift logging
        let outcome = runtime
            .process_account_update(&mint, SOL, TOK, 0, 300, TEST_FINALITY)
            .unwrap();
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        // Verify: pool is hot
        let loss = runtime
            .pool_loss_summary(&mint)
            .expect("loss summary must exist for observed pool");
        assert!(
            loss.is_hot(),
            "pool must be classified as hot after {} tx seen",
            HOT_POOL_TX_THRESHOLD
        );

        // PR 7: no diagnostic pressure should be emitted into the hot-pool tracker.
        assert_eq!(
            loss.diagnostic_signals, 0,
            "diagnostic-signal counter must stay zero in PR 7"
        );
        assert_eq!(
            loss.suspected_misses, 0,
            "diagnostic-only severe drift must not synthesize suspected misses"
        );

        // Verify: estimated tx loss is visible
        let status = runtime.status();
        assert_eq!(
            status.total_hot_pools, 1,
            "runtime status must reflect one hot pool"
        );
        assert!(
            status.total_estimated_tx_loss > 0,
            "estimated tx loss must be positive when some tx were not forwarded"
        );
    }

    /// C.2 – ingest hardening scenario: all tx forwarded → zero loss, zero repair pressure.
    ///
    /// Simulates the target state where ingest hardening is working: all observed
    /// tx are forwarded and no diagnostic signals are triggered.
    #[test]
    fn test_runtime_zero_loss_when_all_tx_forwarded_and_no_drift() {
        use crate::shadow_ledger::drift_observability::HOT_POOL_TX_THRESHOLD;

        let mint = Pubkey::new_unique();
        let ledger = make_ledger_with_curve(mint, SOL, TOK);
        let mut runtime = ReconciliationRuntime::new(ledger);

        // Simulate a hot-pool burst with perfect ingest (all tx forwarded)
        let burst = HOT_POOL_TX_THRESHOLD * 3;
        for _ in 0..burst {
            runtime.record_tx_seen(&mint);
            runtime.record_tx_forwarded(&mint);
        }

        // AccountUpdate with matching state (no drift)
        runtime
            .process_account_update(&mint, SOL, TOK, 0, 400, TEST_FINALITY)
            .unwrap();

        let loss = runtime.pool_loss_summary(&mint).unwrap();
        assert!(loss.is_hot(), "pool must be hot after burst");
        assert_eq!(
            loss.estimated_loss(),
            0,
            "zero estimated loss when all tx are forwarded"
        );
        assert_eq!(
            loss.diagnostic_signals, 0,
            "no diagnostic signals when drift is zero"
        );

        let status = runtime.status();
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "no diagnostic signals expected in the ideal ingest scenario"
        );
        assert_eq!(
            status.total_estimated_tx_loss, 0,
            "zero total tx loss when ingest hardening is working"
        );
    }

    /// C.3 – runtime records hot-pool repair correlation via actual runtime path.
    ///
    /// Verifies that `run_cycle` also correctly feeds hot-pool correlation when
    /// a cycle-triggered reconciliation results in a repair.
    #[test]
    fn test_runtime_cycle_records_hot_pool_repair_correlation() {
        use crate::shadow_ledger::drift_observability::HOT_POOL_TX_THRESHOLD;

        let mint = Pubkey::new_unique();
        let shadow_sol = SOL + 2_000_000_000;
        let ledger = make_ledger_with_curve(mint, shadow_sol, TOK);
        let mut runtime = ReconciliationRuntime::new(ledger);
        runtime.register_pool(mint);

        // Classify as hot
        for _ in 0..HOT_POOL_TX_THRESHOLD {
            runtime.record_tx_seen(&mint);
        }

        // Cycle delivers on-chain truth (SOL, not shadow_sol) → severe drift log
        let reconciled = runtime.run_cycle(|_| Some((SOL, TOK, 0, 500)));
        assert_eq!(reconciled, 1);

        let loss = runtime.pool_loss_summary(&mint).unwrap();
        assert!(loss.is_hot());
        assert_eq!(
            loss.diagnostic_signals, 0,
            "diagnostic-only cycle path must not track diagnostic signals"
        );
    }

    // =========================================================================
    // D. Non-regression tests
    // =========================================================================

    /// D.1 – Shadow Ledger remains primary authority.
    ///
    /// When there is no drift, process_account_update must not modify state.
    /// Tx-driven state is preserved as-is.
    #[test]
    fn test_runtime_shadow_ledger_remains_primary_authority() {
        let mint = Pubkey::new_unique();
        let ledger = make_ledger_with_curve(mint, SOL, TOK);
        let shadow_clone = ledger.clone();
        let mut runtime = ReconciliationRuntime::new(ledger);

        // AccountUpdate matches tx-driven state perfectly
        let outcome = runtime
            .process_account_update(&mint, SOL, TOK, 0, 1, TEST_FINALITY)
            .unwrap();
        assert_eq!(outcome.action, ReconciliationAction::NoAction);

        // Ledger must still hold the original tx-driven values
        let curve = shadow_clone.get(&mint).unwrap();
        assert_eq!(
            curve.virtual_sol_reserves, SOL,
            "Shadow Ledger must preserve tx-driven state when no drift is detected"
        );
    }

    /// D.2 – AccountUpdate remains corrective only (small drift does not overwrite).
    ///
    /// Noise and meaningful drift must not cause the runtime to overwrite Shadow
    /// Ledger state — that would make AccountUpdate primary, which violates the
    /// architecture.
    #[test]
    fn test_runtime_account_update_corrective_only_small_drift_not_overwritten() {
        let mint = Pubkey::new_unique();
        // Noise-level drift: 0.0003 SOL
        let shadow_sol = SOL + 300_000;
        let ledger = make_ledger_with_curve(mint, shadow_sol, TOK);
        let shadow_clone = ledger.clone();
        let mut runtime = ReconciliationRuntime::new(ledger);

        let outcome = runtime
            .process_account_update(&mint, SOL, TOK, 0, 1, TEST_FINALITY)
            .unwrap();
        assert_ne!(outcome.action, ReconciliationAction::DiagnosticSignal);

        // Shadow Ledger must still hold the tx-driven (slightly-drifted) state
        let curve = shadow_clone.get(&mint).unwrap();
        assert_eq!(
            curve.virtual_sol_reserves, shadow_sol,
            "tx-driven state must be preserved for noise-level AccountUpdate"
        );
    }

    /// D.3 – runtime does not introduce a competing state engine.
    ///
    /// Verifies that the runtime does not create new ledger entries for mints
    /// that the Shadow Ledger does not track.  AccountUpdate for unknown mints
    /// is a no-op.
    #[test]
    fn test_runtime_no_competing_state_engine_for_unknown_mints() {
        let ledger = ShadowLedger::new();
        let shadow_clone = ledger.clone();
        let mut runtime = ReconciliationRuntime::new(ledger);

        let unknown_mint = Pubkey::new_unique();
        let result = runtime.process_account_update(&unknown_mint, SOL, TOK, 0, 1, TEST_FINALITY);

        assert!(
            result.is_none(),
            "unknown mint must return None — runtime must not create competing state"
        );
        assert_eq!(
            shadow_clone.len(),
            0,
            "Shadow Ledger must remain empty after AccountUpdate for unknown mint"
        );
    }

    /// D.4 – run_cycle with no registered pools is a safe no-op.
    #[test]
    fn test_runtime_cycle_with_no_pools_is_safe_noop() {
        let ledger = ShadowLedger::new();
        let mut runtime = ReconciliationRuntime::new(ledger);

        let reconciled = runtime.run_cycle(|_| Some((SOL, TOK, 0, 1)));
        assert_eq!(reconciled, 0);
        assert_eq!(runtime.cycle_count(), 1, "cycle count must still increment");
    }

    // =========================================================================
    // E. Pool registry tests
    // =========================================================================

    /// E.1 – pool registry is bounded; registration beyond cap returns false.
    #[test]
    fn test_runtime_pool_registry_bounded() {
        let ledger = ShadowLedger::new();
        let config = ReconciliationRuntimeConfig {
            max_pools: 2,
            max_pools_per_cycle: 10,
            ..Default::default()
        };
        let mut runtime = ReconciliationRuntime::with_config(ledger, config);

        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        let mint_c = Pubkey::new_unique();

        assert!(runtime.register_pool(mint_a));
        assert!(runtime.register_pool(mint_b));
        assert!(
            !runtime.register_pool(mint_c),
            "registration beyond cap must return false"
        );
        assert_eq!(
            runtime.registered_pool_count(),
            2,
            "registry must not exceed max_pools"
        );
    }

    /// E.2 – re-registering a pool is idempotent.
    #[test]
    fn test_runtime_pool_registration_idempotent() {
        let ledger = ShadowLedger::new();
        let mut runtime = ReconciliationRuntime::new(ledger);
        let mint = Pubkey::new_unique();

        assert!(runtime.register_pool(mint));
        assert!(runtime.register_pool(mint)); // second call: must not duplicate
        assert_eq!(
            runtime.registered_pool_count(),
            1,
            "duplicate registration must not increase pool count"
        );
    }

    /// E.3 – unregister_pool removes the pool from the registry.
    #[test]
    fn test_runtime_pool_unregister_removes_pool() {
        let ledger = ShadowLedger::new();
        let mut runtime = ReconciliationRuntime::new(ledger);
        let mint = Pubkey::new_unique();

        runtime.register_pool(mint);
        assert_eq!(runtime.registered_pool_count(), 1);
        runtime.unregister_pool(&mint);
        assert_eq!(
            runtime.registered_pool_count(),
            0,
            "unregister_pool must remove the pool"
        );
    }

    /// E.4 – run_cycle respects max_pools_per_cycle cap.
    #[test]
    fn test_runtime_cycle_respects_per_cycle_cap() {
        let ledger = ShadowLedger::new();
        let config = ReconciliationRuntimeConfig {
            max_pools: 10,
            max_pools_per_cycle: 3,
            ..Default::default()
        };
        let mut runtime = ReconciliationRuntime::with_config(ledger.clone(), config);

        // Register 5 pools, insert minimal curve state for each
        let mints: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();
        for &mint in &mints {
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: SOL,
                    virtual_token_reserves: TOK,
                    real_sol_reserves: SOL,
                    real_token_reserves: TOK,
                    token_total_supply: TOK,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            );
            runtime.register_pool(mint);
        }

        let reconciled = runtime.run_cycle(|_| Some((SOL, TOK, 0, 100)));
        assert_eq!(
            reconciled, 3,
            "cycle must only reconcile max_pools_per_cycle (3) pools"
        );
    }

    /// E.5 – round-robin scheduling covers all pools across consecutive cycles.
    #[test]
    fn test_runtime_cycle_round_robin_covers_all_pools() {
        let ledger = ShadowLedger::new();
        let config = ReconciliationRuntimeConfig {
            max_pools: 10,
            max_pools_per_cycle: 2,
            ..Default::default()
        };
        let mut runtime = ReconciliationRuntime::with_config(ledger.clone(), config);

        let mints: Vec<Pubkey> = (0..4).map(|_| Pubkey::new_unique()).collect();
        for &mint in &mints {
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: SOL,
                    virtual_token_reserves: TOK,
                    real_sol_reserves: SOL,
                    real_token_reserves: TOK,
                    token_total_supply: TOK,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            );
            runtime.register_pool(mint);
        }

        // 2 pools per cycle, 4 pools total → 2 full cycles cover everything
        let r1 = runtime.run_cycle(|_| Some((SOL, TOK, 0, 100)));
        let r2 = runtime.run_cycle(|_| Some((SOL, TOK, 0, 101)));
        assert_eq!(r1 + r2, 4, "two cycles must cover all 4 pools");
    }

    // =========================================================================
    // F. Status and health tests
    // =========================================================================

    /// F.1 – status reflects accurate cumulative counters.
    #[test]
    fn test_runtime_status_reflects_cumulative_counters() {
        let mint = Pubkey::new_unique();
        let ledger = ShadowLedger::new();

        let mut runtime = ReconciliationRuntime::new(ledger.clone());

        let insert_drifted = |sol: u64| {
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: sol,
                    virtual_token_reserves: TOK,
                    real_sol_reserves: sol,
                    real_token_reserves: TOK,
                    token_total_supply: TOK,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            )
        };

        // 1 no-drift check
        insert_drifted(SOL);
        runtime
            .process_account_update(&mint, SOL, TOK, 0, 1, TEST_FINALITY)
            .unwrap();

        // 1 severe-drift check → diagnostic-only drift log
        insert_drifted(SOL + 2_000_000_000);
        runtime
            .process_account_update(&mint, SOL, TOK, 0, 2, TEST_FINALITY)
            .unwrap();

        let status = runtime.status();
        assert_eq!(status.total_checks, 2, "two checks must be recorded");
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "PR 7 runtime must not record diagnostic signals"
        );
        assert_eq!(
            status.total_drifting_pools, 1,
            "one pool must show drift history"
        );
        assert!(
            status.worst_drift_lamports >= 2_000_000_000,
            "worst drift must reflect the severe event"
        );
    }

    /// F.2 – status worst_drift_mint identifies the highest-drift pool.
    #[test]
    fn test_runtime_status_worst_drift_mint_identified() {
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();

        // mint_a has lower drift; mint_b has higher drift — worst_drift_mint must point to mint_b.
        const DRIFT_A: u64 = 1_000_000_000; // 1 SOL above chain
        const DRIFT_B: u64 = 3_000_000_000; // 3 SOL above chain (should be identified as worst)

        let make_runtime_with_two_drifted_pools = || -> ReconciliationRuntime {
            let ledger = ShadowLedger::new();
            for (&mint, &drift) in [mint_a, mint_b].iter().zip([DRIFT_A, DRIFT_B].iter()) {
                ledger.insert_with_slot(
                    mint,
                    BondingCurve {
                        discriminator: 0,
                        virtual_sol_reserves: SOL + drift,
                        virtual_token_reserves: TOK,
                        real_sol_reserves: SOL + drift,
                        real_token_reserves: TOK,
                        token_total_supply: TOK,
                        complete: 0,
                        _padding: [0u8; 7],
                    },
                    100,
                );
            }
            let mut runtime = ReconciliationRuntime::new(ledger);
            runtime
                .process_account_update(&mint_a, SOL, TOK, 0, 1, TEST_FINALITY)
                .unwrap();
            runtime
                .process_account_update(&mint_b, SOL, TOK, 0, 2, TEST_FINALITY)
                .unwrap();
            runtime
        };

        let runtime = make_runtime_with_two_drifted_pools();
        let status = runtime.status();
        assert_eq!(
            status.worst_drift_mint,
            Some(mint_b),
            "worst drift pool must be the one with highest peak drift (DRIFT_B = {} > DRIFT_A = {})",
            DRIFT_B,
            DRIFT_A,
        );
    }

    #[test]
    fn test_next_cycle_batch_round_robins_registered_pools() {
        let ledger = ShadowLedger::new();
        let mut runtime = ReconciliationRuntime::with_config(
            ledger,
            ReconciliationRuntimeConfig {
                max_pools: 8,
                max_pools_per_cycle: 2,
                ..Default::default()
            },
        );
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        let mint_c = Pubkey::new_unique();
        runtime.register_pool(mint_a);
        runtime.register_pool(mint_b);
        runtime.register_pool(mint_c);

        let first = runtime.next_cycle_batch();
        let second = runtime.next_cycle_batch();
        let third = runtime.next_cycle_batch();

        assert_eq!(first, vec![mint_a, mint_b]);
        assert_eq!(second, vec![mint_c, mint_a]);
        assert_eq!(third, vec![mint_b, mint_c]);
    }

    #[test]
    fn test_runtime_critical_drift_alert_counter_increments_without_repair() {
        let mint = Pubkey::new_unique();
        let ledger = make_ledger_with_curve(mint, 30_000_000_000, 1_000_000_000_000);
        let mut runtime = ReconciliationRuntime::with_config(
            ledger.clone(),
            ReconciliationRuntimeConfig {
                drift_alert_threshold_lamports: 50_000_000,
                ..Default::default()
            },
        );

        let outcome = runtime
            .process_account_update(
                &mint,
                30_500_000_001,
                1_000_000_000_000,
                0,
                101,
                TEST_FINALITY,
            )
            .expect("known mint must reconcile");

        assert_eq!(outcome.severity, DriftSeverity::Severe);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        let status = runtime.status();
        assert_eq!(status.total_diagnostic_signals, 0);
        assert_eq!(status.total_critical_drift_alerts, 1);
        assert_eq!(status.drift_alert_threshold_lamports, 50_000_000);

        let retained_curve = ledger.get(&mint).expect("retained curve must exist");
        assert_eq!(retained_curve.virtual_sol_reserves, 30_000_000_000);
    }
}
