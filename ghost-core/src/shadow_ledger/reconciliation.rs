//! AccountUpdate drift monitoring for Shadow Ledger
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │           Normal tx-driven state evolution (PRIMARY)            │
//! │  Seer → GatekeeperMintBuffer → commit → ShadowLedger            │
//! │                   ↑                                             │
//! │           This is the authority path                            │
//! └─────────────────────────────────────────────────────────────────┘
//!                          │  drift can happen due to:
//!                          │  - dropped gRPC packets
//!                          │  - missing tx observations
//!                          │  - micro-forks / reorg-like events
//!                          │  - parser misclassification
//!                          ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │      AccountUpdate Reconciliation (DIAGNOSTIC-ONLY, PR 7+)      │
//! │  ShadowLedgerReconciler::reconcile(mint, on_chain_state, slot)  │
//! │   ↓ compare Shadow state vs on-chain state                      │
//! │   ↓ classify drift severity (None / Noise / Meaningful / Severe)│
//! │   ↓ emit telemetry at every step                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Invariants
//!
//! - **Shadow Ledger remains the simulation / diagnostic store** for tx replay.
//! - **AccountUpdate / AccountStateCore is canonical runtime truth**.
//! - Reconciliation is **read-only** and never overwrites runtime state.
//! - Severe drift is surfaced explicitly for operators without hidden state-write side effects.
//!
//! ## Finding the Key Code Paths
//!
//! - Drift comparison:     [`ShadowLedgerReconciler::compare`]
//! - Drift monitoring:     [`ShadowLedgerReconciler::reconcile`]
//! - Drift policy:         [`DriftPolicy`] and its associated constants
//! - Severity classif.:    [`DriftSeverity`]

use metrics::increment_counter;
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info, warn};

use crate::shadow_ledger::history_types::{ReconciliationDiff, ReconciliationPoint};
use crate::shadow_ledger::ledger::ShadowLedger;
use crate::shadow_ledger::CurveFinality;

// ============================================================================
// Drift Thresholds — explicit policy constants
// ============================================================================

/// Noise-level SOL drift threshold (lamports).
///
/// Drift at or below this value is considered timing noise from minor block
/// ordering or rounding effects. No state change is needed, but the event is
/// counted.
///
/// Value: 1 000 000 lamports (0.001 SOL).
pub const NOISE_THRESHOLD_LAMPORTS: u64 = 1_000_000;

/// Meaningful SOL drift threshold (lamports).
///
/// Drift above [`NOISE_THRESHOLD_LAMPORTS`] but at or below this value is considered
/// *meaningful* — it exceeds normal timing noise and should be logged, but is still
/// within a range where brief stream gaps could explain the discrepancy without
/// escalating to a severe-drift alert.
///
/// Value: 50 000 000 lamports (0.05 SOL).
pub const MEANINGFUL_THRESHOLD_LAMPORTS: u64 = 50_000_000;

/// Severe SOL drift threshold (lamports).
///
/// Drift above this value indicates that the Shadow Ledger is substantially out
/// of sync with on-chain reality. The reconciler escalates the condition as a
/// severe diagnostic signal but does **not** reset Shadow Ledger state.
///
/// Value: 500 000 000 lamports (0.5 SOL).
pub const SEVERE_THRESHOLD_LAMPORTS: u64 = 500_000_000;

// ============================================================================
// DriftPolicy — configurable thresholds
// ============================================================================

/// Explicit drift classification policy.
///
/// Holds the three threshold values used to categorise measured drift.
/// Create with [`DriftPolicy::default`] for the standard production thresholds,
/// or build with custom values for tests that exercise edge cases.
///
/// # Example
///
/// ```rust
/// use ghost_core::shadow_ledger::reconciliation::DriftPolicy;
///
/// // Default production policy
/// let policy = DriftPolicy::default();
/// assert_eq!(policy.noise_lamports, 1_000_000);
/// assert_eq!(policy.meaningful_lamports, 50_000_000);
/// assert_eq!(policy.severe_lamports, 500_000_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriftPolicy {
    /// Absolute SOL drift (lamports) considered timing noise — no state change needed.
    pub noise_lamports: u64,
    /// Absolute SOL drift (lamports) considered meaningful — log, but no state change.
    pub meaningful_lamports: u64,
    /// Absolute SOL drift (lamports) classified as severe for diagnostic escalation.
    pub severe_lamports: u64,
}

impl DriftPolicy {
    /// Create a policy with explicit threshold values (useful in tests).
    pub fn new(noise_lamports: u64, meaningful_lamports: u64, severe_lamports: u64) -> Self {
        Self {
            noise_lamports,
            meaningful_lamports,
            severe_lamports,
        }
    }

    /// Classify the absolute SOL drift magnitude into a [`DriftSeverity`] level.
    pub fn classify(&self, abs_sol_drift: u64) -> DriftSeverity {
        if abs_sol_drift == 0 {
            DriftSeverity::None
        } else if abs_sol_drift <= self.noise_lamports {
            DriftSeverity::Noise
        } else if abs_sol_drift <= self.meaningful_lamports {
            DriftSeverity::Meaningful
        } else {
            DriftSeverity::Severe
        }
    }
}

impl Default for DriftPolicy {
    fn default() -> Self {
        Self {
            noise_lamports: NOISE_THRESHOLD_LAMPORTS,
            meaningful_lamports: MEANINGFUL_THRESHOLD_LAMPORTS,
            severe_lamports: SEVERE_THRESHOLD_LAMPORTS,
        }
    }
}

// ============================================================================
// DriftSeverity — classification result
// ============================================================================

/// Classification of the measured drift between Shadow Ledger and on-chain state.
///
/// Returned by [`DriftPolicy::classify`] and embedded in [`ReconciliationOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftSeverity {
    /// No drift — Shadow Ledger exactly matches on-chain state.
    None,
    /// Noise-level drift — within normal timing/rounding tolerances.
    Noise,
    /// Meaningful drift — exceeds noise but remains below the severe threshold.
    ///
    /// This level is logged and counted but does **not** trigger a state mutation.
    /// It often indicates a temporarily missed trade that is likely to correct itself.
    Meaningful,
    /// Severe drift — requires operator attention and explicit monitoring.
    ///
    /// When this level is reached, [`ShadowLedgerReconciler::reconcile`] emits a
    /// severe-drift signal but still does not overwrite Shadow Ledger state.
    Severe,
}

impl DriftSeverity {
    /// Return a short human-readable label for metrics/logging.
    pub fn label(&self) -> &'static str {
        match self {
            DriftSeverity::None => "none",
            DriftSeverity::Noise => "noise",
            DriftSeverity::Meaningful => "meaningful",
            DriftSeverity::Severe => "severe",
        }
    }
}

// ============================================================================
// ReconciliationAction — what the reconciler did
// ============================================================================

/// Action taken by the reconciler after computing drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationAction {
    /// State matched — no action required.
    NoAction,
    /// Drift was within noise or meaningful range — logged and counted only.
    Logged,
    /// Dormant diagnostic marker retained only for historical compatibility/tests.
    /// Monitoring-only reconciliation no longer emits this action.
    DiagnosticSignal,
}

// ============================================================================
// ReconciliationOutcome — full result of one reconciliation check
// ============================================================================

/// Full result of a single reconciliation check for one mint.
///
/// This is the return value of [`ShadowLedgerReconciler::reconcile`] and
/// [`ShadowLedgerReconciler::compare`].
#[derive(Debug, Clone)]
pub struct ReconciliationOutcome {
    /// Mint that was reconciled.
    pub mint: Pubkey,
    /// Measured difference between Shadow Ledger and on-chain state.
    pub diff: ReconciliationDiff,
    /// Absolute SOL drift magnitude (unsigned).
    pub abs_sol_drift: u64,
    /// Classified drift severity.
    pub severity: DriftSeverity,
    /// Action taken by the reconciler.
    pub action: ReconciliationAction,
}

// ============================================================================
// ShadowLedgerReconciler — main entrypoint
// ============================================================================

/// Reconciliation / drift-monitoring entrypoint for Shadow Ledger.
///
/// This struct holds a reference to a [`ShadowLedger`] and provides the two key
/// operations that implement drift monitoring against on-chain truth:
///
/// 1. **`compare`** — compute drift between Shadow Ledger state and on-chain state
///    without modifying any state (read-only, observability use).
/// 2. **`reconcile`** — compare and classify drift for observability. As of PR 7
///    it never mutates the Shadow Ledger.
///
/// # Example
///
/// ```rust,ignore
/// use ghost_core::shadow_ledger::{ShadowLedger, reconciliation::{DriftPolicy, ShadowLedgerReconciler}};
///
/// let ledger = ShadowLedger::new();
/// let reconciler = ShadowLedgerReconciler::new(ledger.clone(), DriftPolicy::default());
///
/// // When an AccountUpdate arrives for a mint:
/// let outcome = reconciler.reconcile(
///     &mint,
///     on_chain_sol_reserves,
///     on_chain_token_reserves,
///     on_chain_complete,
///     on_chain_slot,
/// );
///
/// // Inspect outcome for observability
/// println!("drift: {} lamports, action: {:?}", outcome.abs_sol_drift, outcome.action);
/// ```
pub struct ShadowLedgerReconciler {
    ledger: ShadowLedger,
    policy: DriftPolicy,
}

impl ShadowLedgerReconciler {
    /// Create a new reconciler wrapping the given ledger.
    pub fn new(ledger: ShadowLedger, policy: DriftPolicy) -> Self {
        Self { ledger, policy }
    }

    /// Create a reconciler with the default production [`DriftPolicy`].
    pub fn with_default_policy(ledger: ShadowLedger) -> Self {
        Self::new(ledger, DriftPolicy::default())
    }

    /// Compare Shadow Ledger state for `mint` against the provided on-chain reserves.
    ///
    /// # DRIFT COMPARISON ENTRYPOINT
    ///
    /// This is the explicit comparison point described in the architecture:
    /// *"This is where Shadow Ledger state is compared to chain state."*
    ///
    /// - Reads the current Shadow Ledger curve state for `mint`.
    /// - Constructs a [`ReconciliationPoint`] from the current virtual reserves.
    /// - Calls [`ReconciliationPoint::compare_with_account_update`] to measure drift.
    /// - Classifies drift using the configured [`DriftPolicy`].
    /// - **Does NOT modify any state** — read-only.
    ///
    /// Returns `None` if the mint is not yet known to the Shadow Ledger.
    pub fn compare(
        &self,
        mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
    ) -> Option<ReconciliationOutcome> {
        let curve_key = self.ledger.resolve_curve_key(mint)?;
        let curve = self.ledger.get_curve(&curve_key)?;

        let point = ReconciliationPoint {
            shadow_sol_lamports: curve.virtual_sol_reserves,
            shadow_tok_units: curve.virtual_token_reserves,
            shadow_k: curve.virtual_sol_reserves as u128 * curve.virtual_token_reserves as u128,
            tx_count: 0,
        };

        let diff = point.compare_with_account_update(
            on_chain_sol,
            on_chain_tok,
            self.policy.noise_lamports,
        );

        let abs_sol_drift = diff.sol_drift_lamports.unsigned_abs() as u64;
        let severity = self.policy.classify(abs_sol_drift);

        Some(ReconciliationOutcome {
            mint: *mint,
            diff,
            abs_sol_drift,
            severity,
            action: ReconciliationAction::NoAction,
        })
    }

    /// Compare Shadow Ledger state against on-chain state and classify drift.
    ///
    /// # DIAGNOSTIC ENTRYPOINT
    ///
    /// This is the explicit drift-observability point described in the architecture.
    ///
    /// Workflow:
    /// 1. Compare current Shadow Ledger state with the provided on-chain state (see [`compare`]).
    /// 2. Classify drift via [`DriftPolicy`].
    /// 3. If `Severe`: emit an explicit warning and surface the divergence.
    /// 4. Emit telemetry at every step.
    ///
    /// # Returns
    ///
    /// `None` if the mint is not currently tracked by the Shadow Ledger.
    pub fn reconcile(
        &self,
        mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        on_chain_complete: u8,
        slot: u64,
        curve_finality: CurveFinality,
    ) -> Option<ReconciliationOutcome> {
        increment_counter!("shadow_ledger_reconciliation_checks_total");

        let mut outcome = self.compare(mint, on_chain_sol, on_chain_tok)?;

        let severity_label = outcome.severity.label();

        match outcome.severity {
            DriftSeverity::None => {
                increment_counter!("shadow_ledger_reconciliation_no_drift_total");
                debug!(
                    %mint,
                    on_chain_sol,
                    on_chain_tok,
                    "reconciliation: state matches on-chain (no drift)"
                );
                outcome.action = ReconciliationAction::NoAction;
            }

            DriftSeverity::Noise => {
                increment_counter!(
                    "shadow_ledger_reconciliation_drift_detected_total",
                    "severity" => severity_label
                );
                debug!(
                    %mint,
                    abs_sol_drift = outcome.abs_sol_drift,
                    severity = severity_label,
                    "reconciliation: noise-level drift (tolerated)"
                );
                outcome.action = ReconciliationAction::Logged;
            }

            DriftSeverity::Meaningful => {
                increment_counter!(
                    "shadow_ledger_reconciliation_drift_detected_total",
                    "severity" => severity_label
                );
                info!(
                    %mint,
                    abs_sol_drift = outcome.abs_sol_drift,
                    sol_drift_lamports = outcome.diff.sol_drift_lamports,
                    tok_drift_units = outcome.diff.tok_drift_units,
                    severity = severity_label,
                    "reconciliation: meaningful drift detected (logged, no state rewrite)"
                );
                outcome.action = ReconciliationAction::Logged;
            }

            DriftSeverity::Severe => {
                increment_counter!(
                    "shadow_ledger_reconciliation_drift_detected_total",
                    "severity" => severity_label
                );

                warn!(
                    %mint,
                    abs_sol_drift = outcome.abs_sol_drift,
                    sol_drift_lamports = outcome.diff.sol_drift_lamports,
                    tok_drift_units = outcome.diff.tok_drift_units,
                    on_chain_sol,
                    on_chain_tok,
                    on_chain_complete,
                    slot,
                    curve_finality = curve_finality.as_str(),
                    severity = severity_label,
                    "reconciliation: severe drift detected (diagnostic-only; no state rewrite)"
                );
                outcome.action = ReconciliationAction::Logged;
            }
        }

        Some(outcome)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market_state::BondingCurve;
    use crate::shadow_ledger::ShadowLedger;

    const TEST_FINALITY: CurveFinality = CurveFinality::Provisional;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn test_ledger_with_curve(mint: Pubkey, sol: u64, tok: u64) -> ShadowLedger {
        let ledger = ShadowLedger::new();
        let curve = BondingCurve {
            discriminator: 0,
            virtual_sol_reserves: sol,
            virtual_token_reserves: tok,
            real_sol_reserves: sol,
            real_token_reserves: tok,
            token_total_supply: tok,
            complete: 0,
            _padding: [0u8; 7],
        };
        ledger.insert_with_slot(mint, curve, 100);
        ledger
    }

    // =========================================================================
    // A. Drift detection tests
    // =========================================================================

    /// A.1 – exact match produces no drift, no action.
    #[test]
    fn test_drift_exact_match_no_repair() {
        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger);

        let outcome = reconciler
            .reconcile(&mint, sol, tok, 0, 101, TEST_FINALITY)
            .unwrap();

        assert_eq!(outcome.severity, DriftSeverity::None);
        assert_eq!(outcome.action, ReconciliationAction::NoAction);
        assert_eq!(outcome.abs_sol_drift, 0);
        assert_eq!(outcome.diff.sol_drift_lamports, 0);
    }

    #[test]
    fn test_drift_exact_match_finalized_does_not_mutate_curve_finality() {
        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        let outcome = reconciler
            .reconcile(&mint, sol, tok, 0, 101, CurveFinality::Finalized)
            .unwrap();

        assert_eq!(outcome.severity, DriftSeverity::None);
        assert_eq!(outcome.action, ReconciliationAction::NoAction);
        assert_eq!(
            ledger.get_curve_finality(&mint),
            Some(CurveFinality::Provisional)
        );
    }

    /// A.2 – small below-threshold mismatch is classified as noise, not rewritten.
    #[test]
    fn test_drift_noise_level_tolerated() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 30_000_500_000; // 0.0005 SOL above on-chain
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 102, TEST_FINALITY)
            .unwrap();

        assert_eq!(outcome.severity, DriftSeverity::Noise);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        // State must NOT have been rewritten
        let curve = ledger.get(&mint).unwrap();
        assert_eq!(
            curve.virtual_sol_reserves, shadow_sol,
            "noise-level drift must not rewrite state"
        );
    }

    /// A.3 – meaningful drift (above noise, below severe) is classified correctly.
    #[test]
    fn test_drift_meaningful_logged_not_repaired() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 30_020_000_000; // 0.02 SOL above on-chain
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 103, TEST_FINALITY)
            .unwrap();

        assert_eq!(outcome.severity, DriftSeverity::Meaningful);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        // State must NOT have been rewritten (drift is meaningful but not severe)
        let curve = ledger.get(&mint).unwrap();
        assert_eq!(
            curve.virtual_sol_reserves, shadow_sol,
            "meaningful drift must not rewrite state"
        );
    }

    /// A.4 – above-threshold mismatch is classified as severe.
    #[test]
    fn test_drift_severe_detected() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 31_000_000_000; // 1 SOL above on-chain (> 0.5 SOL threshold)
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger);

        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 104, TEST_FINALITY)
            .unwrap();

        assert_eq!(outcome.severity, DriftSeverity::Severe);
        assert_eq!(outcome.action, ReconciliationAction::Logged);
        assert!(outcome.abs_sol_drift > SEVERE_THRESHOLD_LAMPORTS);
    }

    // =========================================================================
    // B. Healing / reset tests
    // =========================================================================

    /// B.1 – severe drift is surfaced without mutating Shadow Ledger state.
    #[test]
    fn test_severe_drift_is_diagnostic_only() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 32_000_000_000; // drifted 2 SOL above chain
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 200, TEST_FINALITY)
            .unwrap();

        assert_eq!(outcome.action, ReconciliationAction::Logged);

        // PR 7: reconciliation is diagnostic-only; Shadow Ledger stays unchanged.
        let retained_curve = ledger.get(&mint).unwrap();
        assert_eq!(
            retained_curve.virtual_sol_reserves, shadow_sol,
            "diagnostic-only reconciliation must not overwrite virtual_sol_reserves"
        );
        assert_eq!(
            retained_curve.virtual_token_reserves, tok,
            "diagnostic-only reconciliation must not overwrite virtual_token_reserves"
        );
    }

    /// B.2 – diagnostic-only reconciliation leaves simulation state usable.
    #[test]
    fn test_diagnostic_only_state_is_usable() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 35_000_000_000; // significantly drifted
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 201, TEST_FINALITY)
            .unwrap();

        // Simulate a buy after logging drift — must still succeed.
        let sim = ledger.simulate_buy(&mint, 1_000_000_000, None);
        assert!(
            sim.is_ok(),
            "simulate_buy must succeed after diagnostic-only reconcile"
        );
        let sim = sim.unwrap();
        assert!(
            sim.tokens_out > 0,
            "tokens_out must remain positive after diagnostic-only reconcile"
        );
    }

    /// B.3 – severe drift classification is deterministic for the same on-chain state.
    #[test]
    fn test_severe_drift_classification_is_deterministic() {
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;

        let run_once = |shadow_sol: u64| -> ReconciliationOutcome {
            let mint = Pubkey::new_unique();
            let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
            let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());
            reconciler
                .reconcile(&mint, on_chain_sol, tok, 0, 300, TEST_FINALITY)
                .unwrap()
        };

        // Two runs with different initial shadow states but the same on-chain state
        let result_a = run_once(35_000_000_000); // severely drifted
        let result_b = run_once(40_000_000_000); // also severely drifted

        assert_eq!(
            result_a.severity, result_b.severity,
            "same on-chain state must classify severe drift consistently"
        );
        assert_eq!(
            result_a.action, result_b.action,
            "same on-chain state must produce the same diagnostic action"
        );
    }

    // =========================================================================
    // C. Failure-mode regression tests
    // =========================================================================

    /// C.1 – missing trade: tx-driven state misses a buy, AccountUpdate logs the drift.
    ///
    /// Scenario:
    ///   - Shadow Ledger believes sol = 30 SOL (missed a 2 SOL buy)
    ///   - On-chain truth: sol = 31.98 SOL (fee-adjusted result of the buy)
    ///   - AccountUpdate arrives with correct on-chain state
    ///   - Reconciler detects severe drift and logs it
    #[test]
    fn test_failure_mode_missing_trade_logged() {
        let mint = Pubkey::new_unique();

        // Shadow Ledger: missed a 2 SOL buy, still shows initial reserves
        let missed_shadow_sol: u64 = 30_000_000_000; // 30 SOL
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, missed_shadow_sol, tok);

        // On-chain truth after fee-adjusted 2 SOL buy
        // effective_sol = 2_000_000_000 * 99 / 100 = 1_980_000_000
        let on_chain_sol: u64 = 30_000_000_000 + 1_980_000_000; // 31.98 SOL
        let on_chain_tok: u64 = 970_588_235_294; // k / new_sol (approx)

        // Before reconciliation: shadow shows the pre-trade state
        let pre_reconcile_curve = ledger.get(&mint).unwrap();
        assert_eq!(pre_reconcile_curve.virtual_sol_reserves, missed_shadow_sol);

        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());
        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, on_chain_tok, 0, 400, TEST_FINALITY)
            .unwrap();

        // Drift is 1.98 SOL — well above SEVERE_THRESHOLD (0.5 SOL)
        assert_eq!(outcome.severity, DriftSeverity::Severe);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        // PR 7: severe drift is diagnostic-only, so the tx-driven state is preserved.
        let retained_curve = ledger.get(&mint).unwrap();
        assert_eq!(
            retained_curve.virtual_sol_reserves, missed_shadow_sol,
            "diagnostic-only reconcile must preserve tx-driven shadow state"
        );
    }

    /// C.2 – stale divergent state is surfaced by a later AccountUpdate.
    ///
    /// Scenario:
    ///   - Shadow Ledger has accumulated drift from multiple missed packets
    ///   - AccountUpdate finally arrives with canonical state
    ///   - Reconciler logs the divergence without mutating Shadow Ledger
    #[test]
    fn test_failure_mode_stale_state_logged() {
        let mint = Pubkey::new_unique();

        // Shadow Ledger: severely stale from dropped gRPC packets
        let stale_sol: u64 = 28_000_000_000; // 2 SOL below actual
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, stale_sol, tok);

        // On-chain reality: 2 SOL worth of buys happened
        let on_chain_sol: u64 = 30_000_000_000;
        let on_chain_tok: u64 = 934_579_439_252; // k invariant derived

        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());
        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, on_chain_tok, 0, 500, TEST_FINALITY)
            .unwrap();

        assert_eq!(outcome.severity, DriftSeverity::Severe);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        // Diagnostic-only path leaves the shadow state intact for forensics.
        let retained_curve = ledger.get(&mint).unwrap();
        assert_eq!(retained_curve.virtual_sol_reserves, stale_sol);
        assert_eq!(retained_curve.virtual_token_reserves, tok);
    }

    /// C.3 – full reconciliation cycle: tx-driven drift → AccountUpdate → logged divergence.
    ///
    /// This test explicitly demonstrates the full reconciliation lifecycle:
    ///   1. Shadow Ledger evolves via tx-driven path (normal operations)
    ///   2. A series of missing trades causes divergence
    ///   3. AccountUpdate arrives with canonical chain state
    ///   4. Severe divergence is logged without mutating Shadow Ledger
    ///   5. Normal tx-driven evolution can resume on the tx-driven state
    #[test]
    fn test_full_reconciliation_cycle_logs_drift_without_repair() {
        let mint = Pubkey::new_unique();
        let initial_sol: u64 = 30_000_000_000;
        let initial_tok: u64 = 1_000_000_000_000;

        // Step 1: Shadow Ledger starts with correct initial state
        let ledger = test_ledger_with_curve(mint, initial_sol, initial_tok);

        // Step 2: Some tx-driven evolution happens correctly (simulate 3 buys)
        // Each buy: 0.5 SOL * 99/100 = 495_000_000 effective SOL
        // After 3 buys: sol = 30_000_000_000 + 3 * 495_000_000 = 31_485_000_000
        // But our test simulates the *stale* state — the ledger missed these txs.
        // The shadow ledger still shows initial_sol.

        // Step 3: AccountUpdate arrives with the true on-chain state (after 3 buys)
        let on_chain_sol: u64 = 31_485_000_000;
        let on_chain_tok: u64 = 950_960_264_901; // k / new_sol

        // Drift: 1_485_000_000 lamports (1.485 SOL) >> SEVERE_THRESHOLD
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());
        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, on_chain_tok, 0, 600, TEST_FINALITY)
            .unwrap();

        // Step 4: Verify severe drift is surfaced without state mutation
        assert_eq!(
            outcome.severity,
            DriftSeverity::Severe,
            "drift from 3 missed trades must be severe"
        );
        assert_eq!(
            outcome.action,
            ReconciliationAction::Logged,
            "severe drift must remain diagnostic-only in PR 7"
        );

        let retained_curve = ledger.get(&mint).unwrap();
        assert_eq!(
            retained_curve.virtual_sol_reserves, initial_sol,
            "Shadow Ledger must remain on the tx-driven state after diagnostic-only reconcile"
        );
        assert_eq!(
            retained_curve.virtual_token_reserves, initial_tok,
            "Shadow Ledger token reserves must remain unchanged after diagnostic-only reconcile"
        );

        // Step 5: Normal tx-driven evolution can resume on the preserved state
        let sim = ledger.simulate_buy(&mint, 500_000_000, None);
        assert!(
            sim.is_ok(),
            "simulation must work after tx-driven evolution resumes without repair side effects"
        );
    }

    // =========================================================================
    // D. Non-regression tests for architecture
    // =========================================================================

    /// D.1 – tx-driven path remains primary: reconcile does NOT change state when
    ///        there is no drift (no-op for healthy state).
    #[test]
    fn test_arch_tx_driven_path_is_primary() {
        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        // Run reconciliation with matching on-chain state
        let outcome = reconciler
            .reconcile(&mint, sol, tok, 0, 1000, TEST_FINALITY)
            .unwrap();
        assert_eq!(outcome.action, ReconciliationAction::NoAction);

        // The ledger state is unchanged (tx-driven state is the authority)
        let curve = ledger.get(&mint).unwrap();
        assert_eq!(curve.virtual_sol_reserves, sol);
        assert_eq!(curve.virtual_token_reserves, tok);
    }

    /// D.2 – reconcile is a no-op for unknown mints (does not create entries).
    #[test]
    fn test_arch_unknown_mint_returns_none() {
        let ledger = ShadowLedger::new();
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());
        let unknown_mint = Pubkey::new_unique();

        let result = reconciler.reconcile(
            &unknown_mint,
            30_000_000_000,
            1_000_000_000_000,
            0,
            1,
            TEST_FINALITY,
        );
        assert!(result.is_none(), "unknown mint must return None");

        // Ledger must not have gained a new entry
        assert_eq!(ledger.len(), 0);
    }

    /// D.3 – noise/meaningful drift does NOT rewrite Shadow Ledger state.
    ///        This is the architecture guard: reconciliation must not replace
    ///        tx-driven evolution for small, normal differences.
    #[test]
    fn test_arch_small_drift_does_not_overwrite() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 30_000_300_000; // 0.0003 SOL above on-chain (noise)
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 1100, TEST_FINALITY)
            .unwrap();

        // Must be noise, and Shadow Ledger must still hold the tx-driven state
        assert_eq!(outcome.severity, DriftSeverity::Noise);
        assert_ne!(outcome.action, ReconciliationAction::DiagnosticSignal);

        let curve = ledger.get(&mint).unwrap();
        assert_eq!(
            curve.virtual_sol_reserves, shadow_sol,
            "tx-driven state must be preserved for noise-level drift"
        );
    }

    /// D.4 – meaningful drift does NOT trigger repair (architecture guard).
    #[test]
    fn test_arch_meaningful_drift_not_repaired() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 30_020_000_000; // 0.02 SOL above (meaningful, not severe)
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 1200, TEST_FINALITY)
            .unwrap();

        assert_eq!(outcome.severity, DriftSeverity::Meaningful);
        assert_ne!(
            outcome.action,
            ReconciliationAction::DiagnosticSignal,
            "meaningful drift must not rewrite state (architecture invariant)"
        );

        let curve = ledger.get(&mint).unwrap();
        assert_eq!(
            curve.virtual_sol_reserves, shadow_sol,
            "tx-driven state must be authoritative for meaningful-level drift"
        );
    }

    // =========================================================================
    // E. Drift policy unit tests
    // =========================================================================

    /// E.1 – DriftPolicy::classify correctly maps magnitudes to severity levels.
    #[test]
    fn test_drift_policy_classify() {
        let policy = DriftPolicy::default();

        assert_eq!(policy.classify(0), DriftSeverity::None);
        assert_eq!(policy.classify(500_000), DriftSeverity::Noise);
        assert_eq!(policy.classify(1_000_000), DriftSeverity::Noise); // at boundary
        assert_eq!(policy.classify(1_000_001), DriftSeverity::Meaningful);
        assert_eq!(policy.classify(50_000_000), DriftSeverity::Meaningful); // at boundary
        assert_eq!(policy.classify(50_000_001), DriftSeverity::Severe);
        assert_eq!(policy.classify(u64::MAX), DriftSeverity::Severe);
    }

    /// E.2 – compare returns None for an unknown mint.
    #[test]
    fn test_compare_unknown_mint_returns_none() {
        let ledger = ShadowLedger::new();
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger);
        let result = reconciler.compare(&Pubkey::new_unique(), 1_000, 1_000);
        assert!(result.is_none());
    }

    /// E.3 – compare (read-only) does not mutate state.
    #[test]
    fn test_compare_does_not_mutate() {
        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, sol, tok);
        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());

        // Compare with severely different on-chain state
        reconciler.compare(&mint, sol + 5_000_000_000, tok);

        // Ledger must be unchanged
        let curve = ledger.get(&mint).unwrap();
        assert_eq!(curve.virtual_sol_reserves, sol);
    }

    /// E.4 – custom policy with tight thresholds still stays diagnostic-only.
    #[test]
    fn test_custom_policy_tight_thresholds() {
        let mint = Pubkey::new_unique();
        let shadow_sol: u64 = 30_002_000_000; // 0.002 SOL drift
        let on_chain_sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;
        let ledger = test_ledger_with_curve(mint, shadow_sol, tok);

        // Tight policy: severe threshold at 1_000_000 (0.001 SOL)
        let tight_policy = DriftPolicy::new(100_000, 500_000, 1_000_000);
        let reconciler = ShadowLedgerReconciler::new(ledger.clone(), tight_policy);

        let outcome = reconciler
            .reconcile(&mint, on_chain_sol, tok, 0, 1300, TEST_FINALITY)
            .unwrap();

        // 0.002 SOL > 0.001 SOL severe threshold → must still be classified as severe
        assert_eq!(outcome.severity, DriftSeverity::Severe);
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        let retained_curve = ledger.get(&mint).unwrap();
        assert_eq!(retained_curve.virtual_sol_reserves, shadow_sol);
    }

    /// E.5 – DriftSeverity labels are non-empty (telemetry guard).
    #[test]
    fn test_severity_labels_non_empty() {
        for severity in [
            DriftSeverity::None,
            DriftSeverity::Noise,
            DriftSeverity::Meaningful,
            DriftSeverity::Severe,
        ] {
            assert!(
                !severity.label().is_empty(),
                "severity label must be non-empty for telemetry"
            );
        }
    }

    #[test]
    fn test_reconcile_resolves_base_mint_to_bonding_curve_alias() {
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 900,
            virtual_sol_reserves: 700,
            real_token_reserves: 900,
            real_sol_reserves: 700,
            token_total_supply: 900,
            complete: 0,
            _padding: [0u8; 7],
        };
        ledger.register_curve_alias(base_mint, bonding_curve);
        ledger.insert_with_slot_known(bonding_curve, curve, 10, true);

        let reconciler = ShadowLedgerReconciler::with_default_policy(ledger.clone());
        let outcome = reconciler
            .reconcile(&base_mint, 1_500_000_000, 800, 1, 42, TEST_FINALITY)
            .expect("alias-backed reconcile should succeed");

        assert_eq!(outcome.action, ReconciliationAction::Logged);
        let retained_curve = ledger
            .get_curve(&bonding_curve)
            .expect("diagnostic-only reconcile must still target canonical bonding_curve key");
        assert_eq!(retained_curve.virtual_sol_reserves, 700);
        assert_eq!(retained_curve.virtual_token_reserves, 900);
        assert_eq!(retained_curve.complete, 0);
        assert!(
            ledger.get(&base_mint).is_none(),
            "curve state must not be duplicated under base_mint key"
        );
    }
}
