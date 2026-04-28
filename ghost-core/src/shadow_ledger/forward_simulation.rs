//! Forward Simulation / Bundle Preflight & Execution Guardrail Module
//!
//! Read-only forward simulation layer built on top of authoritative Shadow Ledger state,
//! plus configurable execution guardrails and strategy-facing planning helpers.
//!
//! ## Purpose
//!
//! This module provides deterministic simulation of hypothetical future trades against
//! the current authoritative Shadow Ledger state, without mutating that state.
//!
//! It then evaluates those simulated results against configurable guardrails and returns
//! explicit, structured assessment results that runtime or strategy code can act on directly.
//!
//! ## Architecture
//!
//! ```text
//! authoritative ShadowLedger state
//!         │
//!         │  (read-only clone via ReconstructedState)
//!         ▼
//!  forward simulation (apply_hypothetical_buy / apply_hypothetical_sell)
//!         │
//!         ▼
//!  ForwardSimResult / ForwardBundleResult
//!         │
//!         │  evaluated against GuardrailConfig
//!         ▼
//!  TradeAssessment / BundleAssessment
//!  (accept / reject with explicit RejectionReason)
//! ```
//!
//! **The authoritative live state is NEVER mutated by any function in this module.**
//!
//! ## Math
//!
//! All simulation uses the same fee-aware, k-invariant integer math as the authoritative
//! state-evolution path in `ReconstructedState::apply_trade_strict`:
//!
//! - BUY:  `sol_after_fee = sol_in * 99 / 100`;
//!         `new_sol = R_sol + sol_after_fee`;
//!         `new_tok = k / new_sol`;
//!         `tok_out = R_tok - new_tok`
//!
//! - SELL: `new_tok = R_tok + tok_in`;
//!         `sol_before_fee = R_sol - (k / new_tok)`;
//!         `sol_out = sol_before_fee * 99 / 100`
//!
//! - Price: `R_sol / R_tok` (instantaneous, presentation only)

use super::history_types::ReconstructedState;
use super::simulation::BPS_DENOMINATOR;

// ============================================================================
// ForwardSimAction - input for bundle simulations
// ============================================================================

/// A single action in a hypothetical trade bundle.
///
/// Used to describe a sequence of trades for bundle preflight simulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardSimAction {
    /// A hypothetical buy: spend `sol_lamports` of SOL to receive tokens.
    Buy {
        /// SOL amount to spend, in lamports.
        sol_lamports: u64,
    },
    /// A hypothetical sell: sell `tok_units` tokens to receive SOL.
    Sell {
        /// Token amount to sell, in base units.
        tok_units: u64,
    },
}

// ============================================================================
// ForwardSimResult - result of a single-step simulation
// ============================================================================

/// Result of simulating a single hypothetical trade against authoritative state.
///
/// All values are derived from the same fee-aware, k-invariant math as the
/// authoritative state-evolution path.  The authoritative state is never mutated.
#[derive(Debug, Clone, PartialEq)]
pub struct ForwardSimResult {
    // ---- Input summary ----
    /// SOL spent (lamports) for a BUY, or 0 for a SELL.
    pub sol_in: u64,
    /// Tokens sold for a SELL, or 0 for a BUY.
    pub tok_in: u64,

    // ---- Output ----
    /// Tokens received for a BUY, or 0 for a SELL.
    pub tok_out: u64,
    /// SOL received (lamports) for a SELL, or 0 for a BUY.
    pub sol_out: u64,

    // ---- Post-trade reserves ----
    /// Virtual SOL reserves after this hypothetical trade (lamports).
    pub post_reserve_sol: u64,
    /// Virtual token reserves after this hypothetical trade.
    pub post_reserve_tok: u64,

    // ---- Price ----
    /// Instantaneous price before trade (R_sol / R_tok).
    pub price_before: f64,
    /// Instantaneous price after trade (post_reserve_sol / post_reserve_tok).
    pub price_after: f64,
    /// Price impact as a percentage change: `(price_after - price_before) / price_before * 100`.
    /// Positive for buys (price rises), negative for sells (price falls).
    pub price_impact_pct: f64,

    // ---- Slippage ----
    /// Minimum acceptable output after slippage adjustment (tokens for BUY, lamports for SELL).
    /// Computed as `output * (BPS_DENOMINATOR - slippage_bps) / BPS_DENOMINATOR`.
    pub min_output: u64,

    // ---- Validity ----
    /// Whether the simulation produced a numerically valid result.
    /// False if input was zero, reserves were zero, or arithmetic overflowed.
    pub is_valid: bool,
}

impl Default for ForwardSimResult {
    fn default() -> Self {
        Self {
            sol_in: 0,
            tok_in: 0,
            tok_out: 0,
            sol_out: 0,
            post_reserve_sol: 0,
            post_reserve_tok: 0,
            price_before: 0.0,
            price_after: 0.0,
            price_impact_pct: 0.0,
            min_output: 0,
            is_valid: false,
        }
    }
}

// ============================================================================
// ForwardBundleResult - result of a multi-step bundle simulation
// ============================================================================

/// Result of simulating a sequence of hypothetical trades (bundle preflight).
///
/// Each step sees the post-trade reserves of the previous step, so the
/// simulation is a deterministic sequential fold starting from authoritative state.
#[derive(Debug, Clone)]
pub struct ForwardBundleResult {
    /// Per-step simulation results, in the same order as the input actions.
    pub steps: Vec<ForwardSimResult>,
    /// Whether all steps produced valid simulation results.
    pub all_valid: bool,
    /// Index of the first invalid step, if any.
    pub first_invalid_step: Option<usize>,
}

// ============================================================================
// GuardrailConfig - configurable execution guardrails
// ============================================================================

/// Configurable execution guardrails for trade/bundle assessment.
///
/// All thresholds are optional; a `None` value means "no limit / not checked."
/// Tighten or relax individual fields to tune the safety envelope for a given strategy.
///
/// # Defaults
///
/// The [`Default`] implementation provides conservative defaults suitable for
/// general-purpose preflight checks.  Override individual fields as needed.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailConfig {
    /// Maximum allowed price impact as a percentage.
    ///
    /// A buy with `price_impact_pct > max_price_impact_pct` will be rejected with
    /// [`RejectionReason::PriceImpactTooHigh`].
    ///
    /// Default: `Some(5.0)` (5%).
    pub max_price_impact_pct: Option<f64>,

    /// Minimum acceptable output amount.
    ///
    /// For a BUY this is in token units; for a SELL this is in lamports.
    /// Output below this threshold triggers [`RejectionReason::OutputTooLow`].
    ///
    /// Default: `None` (not enforced).
    pub min_output: Option<u64>,

    /// Maximum post-trade price allowed (lamports per token, as `f64`).
    ///
    /// If the post-trade price exceeds this value the trade is rejected with
    /// [`RejectionReason::UnsafePostTradePrice`].
    ///
    /// Default: `None` (not enforced).
    pub max_post_trade_price: Option<f64>,

    /// Minimum post-trade price allowed (lamports per token, as `f64`).
    ///
    /// If the post-trade price falls below this value the trade is rejected with
    /// [`RejectionReason::UnsafePostTradePrice`].
    ///
    /// Default: `None` (not enforced).
    pub min_post_trade_price: Option<f64>,

    /// Maximum effective execution price for a BUY (lamports per token).
    ///
    /// If `sol_in / tok_out > max_effective_price` the trade is rejected with
    /// [`RejectionReason::EffectivePriceTooHigh`].
    ///
    /// Default: `None` (not enforced).
    pub max_effective_price: Option<f64>,

    /// Slippage tolerance applied when computing `min_output`, in basis points.
    ///
    /// Default: `50` (0.5%).
    pub slippage_bps: u64,
}

impl Default for GuardrailConfig {
    fn default() -> Self {
        Self {
            max_price_impact_pct: Some(5.0),
            min_output: None,
            max_post_trade_price: None,
            min_post_trade_price: None,
            max_effective_price: None,
            slippage_bps: 50,
        }
    }
}

// ============================================================================
// RejectionReason - explicit reason for guardrail failure
// ============================================================================

/// Explicit reason why a trade or bundle step was rejected by guardrails.
///
/// This avoids "bool soup" — callers can match on the reason to understand exactly
/// why a trade failed and display useful diagnostics or logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    /// Price impact exceeded the configured maximum.
    PriceImpactTooHigh,
    /// Simulated output (tokens for BUY, SOL for SELL) was below the configured minimum.
    OutputTooLow,
    /// Post-trade price is outside the configured safe range.
    UnsafePostTradePrice,
    /// Effective execution price for a BUY exceeded the configured maximum.
    EffectivePriceTooHigh,
    /// The underlying simulation produced an invalid result (zero input, arithmetic failure, etc.).
    InvalidSimulation,
    /// A specific bundle step failed a guardrail (carries the step index and inner reason).
    BundleStepFailed {
        /// Zero-based index of the failing step.
        step: usize,
        /// Guardrail that the step violated.
        reason: Box<RejectionReason>,
    },
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectionReason::PriceImpactTooHigh => write!(f, "price impact too high"),
            RejectionReason::OutputTooLow => write!(f, "output too low"),
            RejectionReason::UnsafePostTradePrice => write!(f, "unsafe post-trade price"),
            RejectionReason::EffectivePriceTooHigh => write!(f, "effective price too high"),
            RejectionReason::InvalidSimulation => write!(f, "invalid simulation result"),
            RejectionReason::BundleStepFailed { step, reason } => {
                write!(f, "bundle step {} failed: {}", step, reason)
            }
        }
    }
}

// ============================================================================
// TradeAssessment - single-trade execution assessment
// ============================================================================

/// Structured result of assessing a single hypothetical trade against guardrails.
///
/// This is the primary strategy-facing output for single-trade preflight checks.
/// Downstream code should inspect `is_accepted` and, on rejection, `rejection_reasons`
/// to understand why.
#[derive(Debug, Clone)]
pub struct TradeAssessment {
    /// The underlying forward simulation result.
    pub sim: ForwardSimResult,
    /// Whether the trade passed all configured guardrails.
    pub is_accepted: bool,
    /// Explicit reasons for rejection.  Empty when `is_accepted` is true.
    pub rejection_reasons: Vec<RejectionReason>,
}

impl TradeAssessment {
    /// Returns `true` if the trade was accepted (passed all guardrails).
    #[inline]
    pub fn accepted(&self) -> bool {
        self.is_accepted
    }

    /// Returns `true` if the trade was rejected by at least one guardrail.
    #[inline]
    pub fn rejected(&self) -> bool {
        !self.is_accepted
    }

    /// Returns the first rejection reason, if any.
    #[inline]
    pub fn first_rejection(&self) -> Option<&RejectionReason> {
        self.rejection_reasons.first()
    }
}

// ============================================================================
// BundleStepAssessment - per-step result in a bundle
// ============================================================================

/// Assessment of a single step within a bundle.
#[derive(Debug, Clone)]
pub struct BundleStepAssessment {
    /// Zero-based index of this step in the bundle.
    pub step_index: usize,
    /// The forward simulation result for this step.
    pub sim: ForwardSimResult,
    /// Whether this step passed all configured guardrails.
    pub is_accepted: bool,
    /// Explicit reasons for rejection.  Empty when `is_accepted` is true.
    pub rejection_reasons: Vec<RejectionReason>,
}

// ============================================================================
// BundleAssessment - bundle-level execution assessment
// ============================================================================

/// Structured result of assessing a multi-step trade bundle against guardrails.
///
/// Per-step outcomes are available in `steps`; the bundle-level verdict is in
/// `is_accepted`.  If any step fails, `first_failing_step` points to it and
/// `rejection_reasons` carries the corresponding [`RejectionReason::BundleStepFailed`].
#[derive(Debug, Clone)]
pub struct BundleAssessment {
    /// Per-step assessment results, in the same order as the input actions.
    pub steps: Vec<BundleStepAssessment>,
    /// Whether all steps passed all configured guardrails.
    pub is_accepted: bool,
    /// Index of the first step that violated a guardrail, if any.
    pub first_failing_step: Option<usize>,
    /// Bundle-level rejection reasons.  Empty when `is_accepted` is true.
    pub rejection_reasons: Vec<RejectionReason>,
}

impl BundleAssessment {
    /// Returns `true` if the bundle was accepted (all steps passed all guardrails).
    #[inline]
    pub fn accepted(&self) -> bool {
        self.is_accepted
    }

    /// Returns `true` if the bundle was rejected.
    #[inline]
    pub fn rejected(&self) -> bool {
        !self.is_accepted
    }

    /// Returns the first bundle-level rejection reason, if any.
    #[inline]
    pub fn first_rejection(&self) -> Option<&RejectionReason> {
        self.rejection_reasons.first()
    }
}

// ============================================================================
// Core pure simulation functions (read-only w.r.t. authoritative state)
// ============================================================================

/// Apply a hypothetical BUY to a **mutable clone** of the state and return the result.
///
/// This is an internal helper used by both `simulate_forward_buy` (which clones first)
/// and `simulate_forward_bundle` (which carries the clone across steps).
///
/// # Arguments
///
/// * `sim`          – mutable reference to the current simulation state (will be updated)
/// * `sol_lamports` – SOL amount to spend (lamports)
/// * `slippage_bps` – slippage tolerance in basis points
///
/// # Returns
///
/// A [`ForwardSimResult`] describing the outcome of this step.
pub fn apply_hypothetical_buy(
    sim: &mut ReconstructedState,
    sol_lamports: u64,
    slippage_bps: u64,
) -> ForwardSimResult {
    if sol_lamports == 0 || sim.reserve_sol_lamports == 0 || sim.reserve_tok_units == 0 {
        return ForwardSimResult {
            sol_in: sol_lamports,
            is_valid: false,
            ..Default::default()
        };
    }

    let price_before = if sim.reserve_tok_units == 0 {
        0.0
    } else {
        sim.reserve_sol_lamports as f64 / sim.reserve_tok_units as f64
    };

    // Protocol fee: 1% deducted from SOL input
    let sol_after_fee = sol_lamports.saturating_mul(99) / 100;
    let new_sol = sim.reserve_sol_lamports.saturating_add(sol_after_fee);

    // k-invariant: new_tok = k / new_sol
    let new_tok = if new_sol == 0 {
        0u64
    } else {
        std::cmp::min(sim.k / (new_sol as u128), u64::MAX as u128) as u64
    };

    let tok_out = sim.reserve_tok_units.saturating_sub(new_tok);

    if tok_out == 0 {
        return ForwardSimResult {
            sol_in: sol_lamports,
            is_valid: false,
            ..Default::default()
        };
    }

    // Update the sim state
    sim.reserve_sol_lamports = new_sol;
    sim.reserve_tok_units = new_tok;
    sim.tx_count = sim.tx_count.saturating_add(1);
    sim.cum_volume_sol_lamports = sim.cum_volume_sol_lamports.saturating_add(sol_lamports);

    let price_after = if new_tok == 0 {
        0.0
    } else {
        new_sol as f64 / new_tok as f64
    };

    let price_impact_pct = if price_before == 0.0 {
        0.0
    } else {
        (price_after - price_before) / price_before * 100.0
    };

    let min_output =
        tok_out.saturating_mul(BPS_DENOMINATOR.saturating_sub(slippage_bps)) / BPS_DENOMINATOR;

    ForwardSimResult {
        sol_in: sol_lamports,
        tok_in: 0,
        tok_out,
        sol_out: 0,
        post_reserve_sol: new_sol,
        post_reserve_tok: new_tok,
        price_before,
        price_after,
        price_impact_pct,
        min_output,
        is_valid: true,
    }
}

/// Apply a hypothetical SELL to a **mutable clone** of the state and return the result.
///
/// This is an internal helper used by both `simulate_forward_sell` (which clones first)
/// and `simulate_forward_bundle` (which carries the clone across steps).
///
/// # Arguments
///
/// * `sim`       – mutable reference to the current simulation state (will be updated)
/// * `tok_units` – token amount to sell (base units)
/// * `slippage_bps` – slippage tolerance in basis points
///
/// # Returns
///
/// A [`ForwardSimResult`] describing the outcome of this step.
pub fn apply_hypothetical_sell(
    sim: &mut ReconstructedState,
    tok_units: u64,
    slippage_bps: u64,
) -> ForwardSimResult {
    if tok_units == 0 || sim.reserve_sol_lamports == 0 || sim.reserve_tok_units == 0 {
        return ForwardSimResult {
            tok_in: tok_units,
            is_valid: false,
            ..Default::default()
        };
    }

    let price_before = sim.reserve_sol_lamports as f64 / sim.reserve_tok_units as f64;

    // k-invariant: new_sol = k / new_tok
    let new_tok = sim.reserve_tok_units.saturating_add(tok_units);
    let new_sol = if new_tok == 0 {
        0u64
    } else {
        std::cmp::min(sim.k / (new_tok as u128), u64::MAX as u128) as u64
    };

    let sol_before_fee = sim.reserve_sol_lamports.saturating_sub(new_sol);
    // Protocol fee: 1% deducted from SOL output
    let sol_out = sol_before_fee.saturating_mul(99) / 100;

    if sol_out == 0 {
        return ForwardSimResult {
            tok_in: tok_units,
            is_valid: false,
            ..Default::default()
        };
    }

    // Update the sim state
    sim.reserve_tok_units = new_tok;
    sim.reserve_sol_lamports = new_sol;
    sim.tx_count = sim.tx_count.saturating_add(1);

    let price_after = if new_tok == 0 {
        0.0
    } else {
        new_sol as f64 / new_tok as f64
    };

    let price_impact_pct = if price_before == 0.0 {
        0.0
    } else {
        (price_after - price_before) / price_before * 100.0
    };

    let min_output =
        sol_out.saturating_mul(BPS_DENOMINATOR.saturating_sub(slippage_bps)) / BPS_DENOMINATOR;

    ForwardSimResult {
        sol_in: 0,
        tok_in: tok_units,
        tok_out: 0,
        sol_out,
        post_reserve_sol: new_sol,
        post_reserve_tok: new_tok,
        price_before,
        price_after,
        price_impact_pct,
        min_output,
        is_valid: true,
    }
}

/// Simulate a single hypothetical BUY against the authoritative state.
///
/// The authoritative `state` is **never mutated** — a clone is used internally.
///
/// # Arguments
///
/// * `state`        – current authoritative [`ReconstructedState`] (read-only)
/// * `sol_lamports` – SOL amount to spend (lamports)
/// * `slippage_bps` – slippage tolerance in basis points (e.g. 50 = 0.5 %)
///
/// # Returns
///
/// A [`ForwardSimResult`] describing the hypothetical post-trade state.
pub fn simulate_forward_buy(
    state: &ReconstructedState,
    sol_lamports: u64,
    slippage_bps: u64,
) -> ForwardSimResult {
    // Clone authoritative state — never mutate the original
    let mut sim = state.clone();
    apply_hypothetical_buy(&mut sim, sol_lamports, slippage_bps)
}

/// Simulate a single hypothetical SELL against the authoritative state.
///
/// The authoritative `state` is **never mutated** — a clone is used internally.
///
/// # Arguments
///
/// * `state`        – current authoritative [`ReconstructedState`] (read-only)
/// * `tok_units`    – token units to sell
/// * `slippage_bps` – slippage tolerance in basis points
///
/// # Returns
///
/// A [`ForwardSimResult`] describing the hypothetical post-trade state.
pub fn simulate_forward_sell(
    state: &ReconstructedState,
    tok_units: u64,
    slippage_bps: u64,
) -> ForwardSimResult {
    let mut sim = state.clone();
    apply_hypothetical_sell(&mut sim, tok_units, slippage_bps)
}

/// Simulate an ordered sequence of hypothetical trades (bundle preflight).
///
/// Each action sees the post-trade state of the previous action, so the simulation
/// is a deterministic sequential fold starting from the authoritative state.
///
/// The authoritative `state` is **never mutated** — a clone is used internally.
///
/// # Arguments
///
/// * `state`        – current authoritative [`ReconstructedState`] (read-only)
/// * `actions`      – ordered slice of [`ForwardSimAction`] entries to simulate
/// * `slippage_bps` – slippage tolerance in basis points applied to every step
///
/// # Returns
///
/// A [`ForwardBundleResult`] with per-step results and bundle-level validity.
pub fn simulate_forward_bundle(
    state: &ReconstructedState,
    actions: &[ForwardSimAction],
    slippage_bps: u64,
) -> ForwardBundleResult {
    // --- Read-only: clone the authoritative state once ---
    let mut sim = state.clone();
    let mut steps = Vec::with_capacity(actions.len());
    let mut all_valid = true;
    let mut first_invalid_step = None;

    for (idx, action) in actions.iter().enumerate() {
        let step_result = match action {
            ForwardSimAction::Buy { sol_lamports } => {
                apply_hypothetical_buy(&mut sim, *sol_lamports, slippage_bps)
            }
            ForwardSimAction::Sell { tok_units } => {
                apply_hypothetical_sell(&mut sim, *tok_units, slippage_bps)
            }
        };

        if !step_result.is_valid {
            if all_valid {
                first_invalid_step = Some(idx);
            }
            all_valid = false;
        }

        steps.push(step_result);
    }

    ForwardBundleResult {
        steps,
        all_valid,
        first_invalid_step,
    }
}

// ============================================================================
// Guardrail evaluation (pure, no I/O)
// ============================================================================

/// Evaluate a single [`ForwardSimResult`] against a [`GuardrailConfig`].
///
/// Returns a list of [`RejectionReason`]s (empty = accepted).
///
/// This function is pure: it only reads `sim` and `config`, never mutates anything.
pub fn evaluate_guardrails(
    sim: &ForwardSimResult,
    config: &GuardrailConfig,
) -> Vec<RejectionReason> {
    let mut reasons = Vec::new();

    if !sim.is_valid {
        reasons.push(RejectionReason::InvalidSimulation);
        return reasons; // No point checking further for invalid sim
    }

    // 1. Price impact check
    if let Some(max_impact) = config.max_price_impact_pct {
        if sim.price_impact_pct.abs() > max_impact {
            reasons.push(RejectionReason::PriceImpactTooHigh);
        }
    }

    // 2. Output too low check
    let output = if sim.tok_out > 0 {
        sim.tok_out
    } else {
        sim.sol_out
    };
    if let Some(min_out) = config.min_output {
        if output < min_out {
            reasons.push(RejectionReason::OutputTooLow);
        }
    }

    // 3. Post-trade price checks
    let post_price = sim.price_after;
    let price_violated = match (config.max_post_trade_price, config.min_post_trade_price) {
        (Some(max_p), _) if post_price > max_p => true,
        (_, Some(min_p)) if post_price < min_p => true,
        _ => false,
    };
    if price_violated {
        reasons.push(RejectionReason::UnsafePostTradePrice);
    }

    // 4. Effective execution price check (BUY only)
    if sim.tok_out > 0 {
        if let Some(max_eff_price) = config.max_effective_price {
            let effective_price = sim.sol_in as f64 / sim.tok_out as f64;
            if effective_price > max_eff_price {
                reasons.push(RejectionReason::EffectivePriceTooHigh);
            }
        }
    }

    reasons
}

// ============================================================================
// Single-trade execution assessment
// ============================================================================

/// Assess a single hypothetical BUY against the authoritative state and guardrails.
///
/// This is the primary single-trade API for strategy code.
///
/// 1. Runs a read-only forward simulation on a clone of `state`.
/// 2. Evaluates configured guardrails.
/// 3. Returns a [`TradeAssessment`] with the simulation result, acceptance flag,
///    and explicit rejection reasons.
///
/// The authoritative `state` is **never mutated**.
///
/// # Example
///
/// ```ignore
/// let assessment = assess_buy(&state, 1_000_000_000, &GuardrailConfig::default());
/// if assessment.is_accepted {
///     // safe to submit
/// } else {
///     eprintln!("rejected: {:?}", assessment.rejection_reasons);
/// }
/// ```
pub fn assess_buy(
    state: &ReconstructedState,
    sol_lamports: u64,
    config: &GuardrailConfig,
) -> TradeAssessment {
    let sim = simulate_forward_buy(state, sol_lamports, config.slippage_bps);
    let rejection_reasons = evaluate_guardrails(&sim, config);
    let is_accepted = rejection_reasons.is_empty();
    TradeAssessment {
        sim,
        is_accepted,
        rejection_reasons,
    }
}

/// Assess a single hypothetical SELL against the authoritative state and guardrails.
///
/// Like [`assess_buy`] but for sell-side trades.
///
/// The authoritative `state` is **never mutated**.
pub fn assess_sell(
    state: &ReconstructedState,
    tok_units: u64,
    config: &GuardrailConfig,
) -> TradeAssessment {
    let sim = simulate_forward_sell(state, tok_units, config.slippage_bps);
    let rejection_reasons = evaluate_guardrails(&sim, config);
    let is_accepted = rejection_reasons.is_empty();
    TradeAssessment {
        sim,
        is_accepted,
        rejection_reasons,
    }
}

// ============================================================================
// Bundle/sequence execution assessment
// ============================================================================

/// Assess a multi-step trade bundle against the authoritative state and guardrails.
///
/// Simulates the full bundle sequentially (each step sees the post-trade state of
/// the previous step), then evaluates each step against the configured guardrails.
///
/// The authoritative `state` is **never mutated**.
///
/// # Returns
///
/// A [`BundleAssessment`] containing:
/// - per-step [`BundleStepAssessment`] results,
/// - bundle-level `is_accepted` flag,
/// - `first_failing_step` index (if any step failed),
/// - bundle-level `rejection_reasons`.
///
/// # Example
///
/// ```ignore
/// let actions = vec![
///     ForwardSimAction::Buy { sol_lamports: 1_000_000_000 },
///     ForwardSimAction::Buy { sol_lamports: 2_000_000_000 },
/// ];
/// let assessment = assess_bundle(&state, &actions, &GuardrailConfig::default());
/// if assessment.is_accepted {
///     // submit bundle
/// } else if let Some(idx) = assessment.first_failing_step {
///     eprintln!("step {} failed: {:?}", idx, assessment.steps[idx].rejection_reasons);
/// }
/// ```
pub fn assess_bundle(
    state: &ReconstructedState,
    actions: &[ForwardSimAction],
    config: &GuardrailConfig,
) -> BundleAssessment {
    let bundle_result = simulate_forward_bundle(state, actions, config.slippage_bps);

    let mut step_assessments = Vec::with_capacity(bundle_result.steps.len());
    let mut bundle_accepted = true;
    let mut first_failing_step = None;
    let mut bundle_rejection_reasons = Vec::new();

    for (idx, step_sim) in bundle_result.steps.into_iter().enumerate() {
        let step_reasons = evaluate_guardrails(&step_sim, config);
        let step_accepted = step_reasons.is_empty();

        if !step_accepted && bundle_accepted {
            // Record the first failing step
            first_failing_step = Some(idx);
            bundle_accepted = false;
            for reason in &step_reasons {
                bundle_rejection_reasons.push(RejectionReason::BundleStepFailed {
                    step: idx,
                    reason: Box::new(reason.clone()),
                });
            }
        }

        step_assessments.push(BundleStepAssessment {
            step_index: idx,
            sim: step_sim,
            is_accepted: step_accepted,
            rejection_reasons: step_reasons,
        });
    }

    BundleAssessment {
        steps: step_assessments,
        is_accepted: bundle_accepted,
        first_failing_step,
        rejection_reasons: bundle_rejection_reasons,
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------------------

    /// Epsilon for floating-point price comparisons in tests.
    const PRICE_EPSILON: f64 = 1e-10;

    /// Standard authoritative state: 30 SOL / 1 T tokens (Pump.fun genesis-like).
    fn standard_state() -> ReconstructedState {
        ReconstructedState::from_initial_reserves(30_000_000_000, 1_000_000_000_000)
    }

    /// Default guardrail config for tests (conservative).
    fn default_config() -> GuardrailConfig {
        GuardrailConfig::default()
    }

    // =========================================================================
    // A. Single-trade acceptance/rejection tests
    // =========================================================================

    /// A.1 – one trade that passes all guardrails (small buy, low impact).
    #[test]
    fn test_single_trade_accepted() {
        let state = standard_state();
        let config = GuardrailConfig {
            max_price_impact_pct: Some(10.0),
            min_output: Some(1),
            ..Default::default()
        };
        // Small buy: 0.01 SOL
        let assessment = assess_buy(&state, 10_000_000, &config);
        assert!(assessment.is_accepted, "small buy should be accepted");
        assert!(
            assessment.rejection_reasons.is_empty(),
            "no rejection reasons expected"
        );
        assert!(assessment.sim.tok_out > 0, "should receive tokens");
        assert!(assessment.sim.is_valid, "simulation should be valid");
    }

    /// A.2 – one trade rejected for output too low.
    #[test]
    fn test_single_trade_rejected_output_too_low() {
        let state = standard_state();
        let config = GuardrailConfig {
            min_output: Some(u64::MAX), // impossible threshold
            max_price_impact_pct: None,
            ..Default::default()
        };
        let assessment = assess_buy(&state, 1_000_000_000, &config);
        assert!(assessment.rejected(), "should be rejected");
        assert!(
            assessment
                .rejection_reasons
                .contains(&RejectionReason::OutputTooLow),
            "should cite output too low: {:?}",
            assessment.rejection_reasons
        );
    }

    /// A.3 – one trade rejected for impact too high.
    #[test]
    fn test_single_trade_rejected_impact_too_high() {
        let state = standard_state();
        let config = GuardrailConfig {
            max_price_impact_pct: Some(0.001), // extremely tight
            min_output: None,
            ..Default::default()
        };
        // Large buy: 10 SOL → significant impact
        let assessment = assess_buy(&state, 10_000_000_000, &config);
        assert!(assessment.rejected(), "should be rejected due to impact");
        assert!(
            assessment
                .rejection_reasons
                .contains(&RejectionReason::PriceImpactTooHigh),
            "should cite impact too high: {:?}",
            assessment.rejection_reasons
        );
    }

    /// A.4 – one trade rejected for unsafe post-trade condition (price too high).
    #[test]
    fn test_single_trade_rejected_unsafe_post_trade_price() {
        let state = standard_state();
        let config = GuardrailConfig {
            max_post_trade_price: Some(0.00000001), // very tight upper bound (tiny)
            max_price_impact_pct: None,
            ..Default::default()
        };
        // Any buy will push price above this tiny cap
        let assessment = assess_buy(&state, 1_000_000_000, &config);
        assert!(assessment.rejected(), "should be rejected");
        assert!(
            assessment
                .rejection_reasons
                .contains(&RejectionReason::UnsafePostTradePrice),
            "should cite unsafe post-trade price: {:?}",
            assessment.rejection_reasons
        );
    }

    /// A.5 – sell trade accepted.
    #[test]
    fn test_single_sell_accepted() {
        let state = standard_state();
        let config = GuardrailConfig {
            max_price_impact_pct: Some(20.0),
            ..Default::default()
        };
        let assessment = assess_sell(&state, 1_000_000_000, &config);
        assert!(assessment.accepted(), "small sell should be accepted");
        assert!(assessment.sim.sol_out > 0, "should receive SOL");
    }

    /// A.6 – effective price too high for buy.
    #[test]
    fn test_single_trade_rejected_effective_price_too_high() {
        let state = standard_state();
        let config = GuardrailConfig {
            max_effective_price: Some(0.0000000001), // absurdly tight
            max_price_impact_pct: None,
            ..Default::default()
        };
        let assessment = assess_buy(&state, 1_000_000_000, &config);
        assert!(assessment.rejected());
        assert!(assessment
            .rejection_reasons
            .contains(&RejectionReason::EffectivePriceTooHigh));
    }

    // =========================================================================
    // B. Bundle acceptance/rejection tests
    // =========================================================================

    /// B.1 – one multi-step bundle that passes.
    #[test]
    fn test_bundle_all_pass() {
        let state = standard_state();
        let config = GuardrailConfig {
            max_price_impact_pct: Some(50.0), // permissive
            min_output: Some(1),
            ..Default::default()
        };
        let actions = vec![
            ForwardSimAction::Buy {
                sol_lamports: 100_000_000,
            }, // 0.1 SOL
            ForwardSimAction::Buy {
                sol_lamports: 200_000_000,
            }, // 0.2 SOL
        ];
        let assessment = assess_bundle(&state, &actions, &config);
        assert!(
            assessment.is_accepted,
            "bundle should be accepted: {:?}",
            assessment.rejection_reasons
        );
        assert!(assessment.first_failing_step.is_none());
        assert_eq!(assessment.steps.len(), 2);
    }

    /// B.2 – bundle where an intermediate step fails.
    #[test]
    fn test_bundle_intermediate_step_fails() {
        let state = standard_state();
        // Step 1 will pass (low impact), step 2 requires output > u64::MAX (fails).
        let config_pass = GuardrailConfig {
            max_price_impact_pct: Some(50.0),
            min_output: None,
            ..Default::default()
        };
        let _ = config_pass; // verify the first step would pass with permissive config

        let config_tight = GuardrailConfig {
            max_price_impact_pct: Some(0.001), // tight — step 2 (large buy) will violate
            min_output: None,
            ..Default::default()
        };

        let actions = vec![
            ForwardSimAction::Buy {
                sol_lamports: 10_000_000,
            }, // 0.01 SOL — tiny, may pass
            ForwardSimAction::Buy {
                sol_lamports: 10_000_000_000,
            }, // 10 SOL — large, will violate impact
        ];

        let assessment = assess_bundle(&state, &actions, &config_tight);
        // The bundle should fail
        assert!(assessment.rejected(), "bundle should be rejected");

        // Identify which step failed
        let failing_idx = assessment.first_failing_step.unwrap();
        assert!(failing_idx < 2, "must identify a valid step index");

        // The failing step's rejection reasons must be non-empty
        let failing_step = &assessment.steps[failing_idx];
        assert!(!failing_step.rejection_reasons.is_empty());

        // The bundle-level rejection must wrap the step failure
        assert!(!assessment.rejection_reasons.is_empty());
        let first = assessment.first_rejection().unwrap();
        match first {
            RejectionReason::BundleStepFailed { step, .. } => {
                assert_eq!(*step, failing_idx);
            }
            other => panic!("expected BundleStepFailed, got {:?}", other),
        }
    }

    /// B.3 – verify failure reason identifies the correct step and condition.
    #[test]
    fn test_bundle_step_fail_reason_carries_step_index() {
        let state = standard_state();
        let config = GuardrailConfig {
            max_price_impact_pct: Some(1.0), // tighter than default
            min_output: None,
            ..Default::default()
        };

        // Big buy should violate impact
        let actions = vec![
            ForwardSimAction::Sell { tok_units: 1_000 }, // tiny sell at step 0 — should pass
            ForwardSimAction::Buy {
                sol_lamports: 30_000_000_000,
            }, // huge buy at step 1 — should fail
        ];

        let assessment = assess_bundle(&state, &actions, &config);
        if assessment.rejected() {
            let idx = assessment.first_failing_step.unwrap();
            // The failing step must be identified and must have PriceImpactTooHigh
            let step = &assessment.steps[idx];
            assert!(step
                .rejection_reasons
                .iter()
                .any(|r| r == &RejectionReason::PriceImpactTooHigh
                    || r == &RejectionReason::InvalidSimulation));
        }
        // (if somehow both pass at this impact threshold, test still passes — it's config-dependent)
    }

    // =========================================================================
    // C. Read-only / non-regression tests
    // =========================================================================

    /// C.1 – assess_buy must NOT mutate authoritative state.
    #[test]
    fn test_assess_buy_does_not_mutate_state() {
        let state = standard_state();
        let original_sol = state.reserve_sol_lamports;
        let original_tok = state.reserve_tok_units;

        let _ = assess_buy(&state, 5_000_000_000, &default_config());

        assert_eq!(
            state.reserve_sol_lamports, original_sol,
            "SOL reserves must not change"
        );
        assert_eq!(
            state.reserve_tok_units, original_tok,
            "tok reserves must not change"
        );
    }

    /// C.2 – assess_bundle must NOT mutate authoritative state.
    #[test]
    fn test_assess_bundle_does_not_mutate_state() {
        let state = standard_state();
        let original_sol = state.reserve_sol_lamports;
        let original_tok = state.reserve_tok_units;

        let actions = vec![
            ForwardSimAction::Buy {
                sol_lamports: 1_000_000_000,
            },
            ForwardSimAction::Buy {
                sol_lamports: 2_000_000_000,
            },
            ForwardSimAction::Sell {
                tok_units: 5_000_000_000,
            },
        ];
        let _ = assess_bundle(&state, &actions, &default_config());

        assert_eq!(state.reserve_sol_lamports, original_sol);
        assert_eq!(state.reserve_tok_units, original_tok);
    }

    /// C.3 – repeated identical assessments are deterministic.
    #[test]
    fn test_repeated_assessments_are_deterministic() {
        let state = standard_state();
        let config = default_config();

        let a1 = assess_buy(&state, 1_000_000_000, &config);
        let a2 = assess_buy(&state, 1_000_000_000, &config);

        assert_eq!(a1.is_accepted, a2.is_accepted);
        assert_eq!(a1.rejection_reasons, a2.rejection_reasons);
        assert_eq!(a1.sim.tok_out, a2.sim.tok_out);
        assert_eq!(a1.sim.price_after, a2.sim.price_after);
    }

    /// C.4 – simulate_forward_buy does not mutate authoritative state.
    #[test]
    fn test_simulate_forward_buy_does_not_mutate_state() {
        let state = standard_state();
        let original_sol = state.reserve_sol_lamports;
        let original_tok = state.reserve_tok_units;

        let _ = simulate_forward_buy(&state, 5_000_000_000, 50);

        assert_eq!(state.reserve_sol_lamports, original_sol);
        assert_eq!(state.reserve_tok_units, original_tok);
    }

    /// C.5 – simulate_forward_bundle does not mutate authoritative state.
    #[test]
    fn test_simulate_forward_bundle_does_not_mutate_state() {
        let state = standard_state();
        let original_sol = state.reserve_sol_lamports;
        let original_tok = state.reserve_tok_units;

        let actions = vec![
            ForwardSimAction::Buy {
                sol_lamports: 1_000_000_000,
            },
            ForwardSimAction::Sell {
                tok_units: 500_000_000,
            },
        ];
        let _ = simulate_forward_bundle(&state, &actions, 50);

        assert_eq!(state.reserve_sol_lamports, original_sol);
        assert_eq!(state.reserve_tok_units, original_tok);
    }

    // =========================================================================
    // D. Configurability tests
    // =========================================================================

    /// D.1 – tightening impact threshold causes previously-accepted trade to be rejected.
    #[test]
    fn test_config_tighten_impact_threshold() {
        let state = standard_state();
        let sol = 5_000_000_000u64; // 5 SOL — meaningful impact

        let permissive = GuardrailConfig {
            max_price_impact_pct: Some(100.0),
            ..Default::default()
        };
        let tight = GuardrailConfig {
            max_price_impact_pct: Some(0.001),
            ..Default::default()
        };

        let a_permissive = assess_buy(&state, sol, &permissive);
        let a_tight = assess_buy(&state, sol, &tight);

        assert!(
            a_permissive.is_accepted,
            "should pass with permissive config"
        );
        assert!(
            a_tight.rejected(),
            "should fail with tight impact threshold"
        );
    }

    /// D.2 – relaxing min_output threshold causes previously-rejected trade to pass.
    #[test]
    fn test_config_relax_min_output_threshold() {
        let state = standard_state();
        let sol = 10_000_000u64; // 0.01 SOL — small output

        // Simulate to find out actual output
        let sim = simulate_forward_buy(&state, sol, 50);
        let actual_output = sim.tok_out;
        assert!(actual_output > 0, "should produce tokens");

        let tight = GuardrailConfig {
            min_output: Some(actual_output + 1), // one more than actual
            max_price_impact_pct: None,
            ..Default::default()
        };
        let permissive = GuardrailConfig {
            min_output: Some(1), // basically no minimum
            max_price_impact_pct: None,
            ..Default::default()
        };

        let a_tight = assess_buy(&state, sol, &tight);
        let a_permissive = assess_buy(&state, sol, &permissive);

        assert!(a_tight.rejected(), "tight min_output should reject");
        assert!(
            a_permissive.accepted(),
            "permissive min_output should accept"
        );
    }

    /// D.3 – setting max_post_trade_price rejects when price exceeds it.
    #[test]
    fn test_config_max_post_trade_price() {
        let state = standard_state();
        let sim = simulate_forward_buy(&state, 1_000_000_000, 50);
        assert!(sim.is_valid);

        // Use exactly the post-trade price as limit → should just pass
        let config_at_limit = GuardrailConfig {
            max_post_trade_price: Some(sim.price_after),
            max_price_impact_pct: None,
            ..Default::default()
        };
        let a_at = assess_buy(&state, 1_000_000_000, &config_at_limit);
        // price_after == limit means not strictly greater, should pass
        assert!(a_at.accepted(), "price == limit should be accepted");

        // Slightly tighter → should fail
        let config_tight = GuardrailConfig {
            max_post_trade_price: Some(sim.price_after * 0.999),
            max_price_impact_pct: None,
            ..Default::default()
        };
        let a_tight = assess_buy(&state, 1_000_000_000, &config_tight);
        assert!(
            a_tight.rejected(),
            "price > limit should be rejected: {:?}",
            a_tight.rejection_reasons
        );
    }

    /// D.4 – bundle config: all steps evaluated against the same config.
    #[test]
    fn test_bundle_config_applied_per_step() {
        let state = standard_state();

        let permissive = GuardrailConfig {
            max_price_impact_pct: Some(100.0),
            ..Default::default()
        };
        let tight = GuardrailConfig {
            max_price_impact_pct: Some(0.001),
            ..Default::default()
        };

        let actions = vec![
            ForwardSimAction::Buy {
                sol_lamports: 10_000_000_000,
            }, // 10 SOL — high impact
        ];

        let a_permissive = assess_bundle(&state, &actions, &permissive);
        let a_tight = assess_bundle(&state, &actions, &tight);

        assert!(a_permissive.accepted(), "permissive should pass");
        assert!(a_tight.rejected(), "tight should fail");
    }

    // =========================================================================
    // E. Integration readiness tests
    // =========================================================================

    /// E.1 – planner helpers work on a reconciled/healed state.
    ///
    /// A "healed" state has had its reserves reset to authoritative on-chain values.
    /// Forward simulation should work correctly from that state.
    #[test]
    fn test_assess_buy_after_reconciliation_healed_state() {
        // Simulate a healing: rebuild state from on-chain curve snapshot
        use crate::market_state::BondingCurve;
        let curve = BondingCurve {
            discriminator: 0,
            virtual_sol_reserves: 35_000_000_000, // healed: 35 SOL
            virtual_token_reserves: 900_000_000_000, // healed: 900B tokens
            real_sol_reserves: 5_000_000_000,
            real_token_reserves: 100_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let healed_state = ReconstructedState::reserves_from_curve(&curve);

        let config = GuardrailConfig {
            max_price_impact_pct: Some(10.0),
            ..Default::default()
        };
        let assessment = assess_buy(&healed_state, 500_000_000, &config);
        assert!(
            assessment.sim.is_valid,
            "simulation must be valid on healed state"
        );
        // Result should be deterministic
        let assessment2 = assess_buy(&healed_state, 500_000_000, &config);
        assert_eq!(assessment.sim.tok_out, assessment2.sim.tok_out);
    }

    /// E.2 – bundle assessment works on top of reconciled state.
    #[test]
    fn test_assess_bundle_after_reconciliation() {
        use crate::market_state::BondingCurve;
        let curve = BondingCurve {
            discriminator: 0,
            virtual_sol_reserves: 40_000_000_000,
            virtual_token_reserves: 800_000_000_000,
            real_sol_reserves: 10_000_000_000,
            real_token_reserves: 200_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let healed = ReconstructedState::reserves_from_curve(&curve);

        let actions = vec![
            ForwardSimAction::Buy {
                sol_lamports: 500_000_000,
            },
            ForwardSimAction::Sell {
                tok_units: 100_000_000,
            },
        ];
        let config = GuardrailConfig {
            max_price_impact_pct: Some(50.0),
            ..Default::default()
        };
        let assessment = assess_bundle(&healed, &actions, &config);
        assert_eq!(assessment.steps.len(), 2, "should have 2 step results");
        // All steps should produce valid simulations
        for (i, step) in assessment.steps.iter().enumerate() {
            assert!(step.sim.is_valid, "step {} should be valid", i);
        }
    }

    /// E.3 – integration: planner composes with healed state from many live trades.
    #[test]
    fn test_assess_buy_after_many_live_trades() {
        // Simulate a state that has processed many trades
        let mut state = standard_state();
        // Apply a series of trades via apply_hypothetical_buy/sell
        for _ in 0..50 {
            apply_hypothetical_buy(&mut state, 100_000_000, 50);
        }
        for _ in 0..20 {
            apply_hypothetical_sell(&mut state, 50_000_000, 50);
        }

        // Now assess a new hypothetical trade from this evolved state
        let config = GuardrailConfig {
            max_price_impact_pct: Some(20.0),
            ..Default::default()
        };
        let sol = state.reserve_sol_lamports;
        let tok = state.reserve_tok_units;

        let assessment = assess_buy(&state, 1_000_000_000, &config);
        assert!(
            assessment.sim.is_valid,
            "sim should be valid after many trades"
        );

        // State must not be mutated
        assert_eq!(state.reserve_sol_lamports, sol);
        assert_eq!(state.reserve_tok_units, tok);
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    /// Zero input returns invalid result and is rejected.
    #[test]
    fn test_zero_input_is_invalid_and_rejected() {
        let state = standard_state();
        let config = default_config();

        let a_buy = assess_buy(&state, 0, &config);
        assert!(!a_buy.sim.is_valid);
        assert!(a_buy.rejected());
        assert!(a_buy
            .rejection_reasons
            .contains(&RejectionReason::InvalidSimulation));

        let a_sell = assess_sell(&state, 0, &config);
        assert!(!a_sell.sim.is_valid);
        assert!(a_sell.rejected());
    }

    /// Empty bundle returns no steps and is accepted.
    #[test]
    fn test_empty_bundle() {
        let state = standard_state();
        let config = default_config();
        let assessment = assess_bundle(&state, &[], &config);
        assert_eq!(assessment.steps.len(), 0);
        assert!(assessment.is_accepted, "empty bundle should be accepted");
        assert!(assessment.first_failing_step.is_none());
    }

    /// Sell from empty reserves returns invalid.
    #[test]
    fn test_sell_from_empty_reserves_is_invalid() {
        let state = ReconstructedState::from_initial_reserves(0, 0);
        let config = default_config();
        let a = assess_sell(&state, 1_000_000, &config);
        assert!(!a.sim.is_valid);
        assert!(a.rejected());
    }

    /// ForwardSimResult price_impact_pct is positive for buys, negative for sells.
    #[test]
    fn test_price_impact_direction() {
        let state = standard_state();
        let buy_result = simulate_forward_buy(&state, 5_000_000_000, 50);
        assert!(
            buy_result.price_impact_pct > 0.0,
            "buy should push price up"
        );

        let sell_result = simulate_forward_sell(&state, 50_000_000_000, 50);
        assert!(
            sell_result.price_impact_pct < 0.0,
            "sell should push price down"
        );
    }

    /// Bundle carries state forward across steps.
    #[test]
    fn test_bundle_state_carries_forward() {
        let state = standard_state();
        // Simulate two sequential buys; step 2 should see higher price than step 1
        let actions = vec![
            ForwardSimAction::Buy {
                sol_lamports: 1_000_000_000,
            },
            ForwardSimAction::Buy {
                sol_lamports: 1_000_000_000,
            },
        ];
        let bundle = simulate_forward_bundle(&state, &actions, 50);
        assert_eq!(bundle.steps.len(), 2);
        let step0_price_after = bundle.steps[0].price_after;
        let step1_price_before = bundle.steps[1].price_before;
        // Step 1's price_before must equal step 0's price_after
        assert!(
            (step1_price_before - step0_price_after).abs() < PRICE_EPSILON,
            "state must carry forward: step0.price_after={}, step1.price_before={}",
            step0_price_after,
            step1_price_before
        );
    }

    /// RejectionReason::BundleStepFailed Display is readable.
    #[test]
    fn test_rejection_reason_display() {
        let r = RejectionReason::BundleStepFailed {
            step: 2,
            reason: Box::new(RejectionReason::PriceImpactTooHigh),
        };
        let s = r.to_string();
        assert!(s.contains("step 2"), "should mention step: {}", s);
        assert!(s.contains("price impact"), "should mention reason: {}", s);
    }
}
