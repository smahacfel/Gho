//! LIGMA (Liquidity Genesis Manifold Analyzer)
//!
//! This module provides continuous liquidity fingerprint analysis throughout the entire
//! pool lifecycle. It operates on pool state (genesis, explicit ShadowLedger snapshot, or
//! real-time state) to detect retail-friendliness, sniper convexity, and liquidity traps
//! without requiring extensive historical trades.
//!
//! LIGMA runs in **every scoring cycle** to provide continuous protection against sudden
//! liquidity drops (High Slippage Trap) that can occur at any point during analysis (e.g.,
//! at 5s, 6s, or 7s into the scoring window).
//!
//! The hot path targets <1ms per candidate and is AMM-agnostic by operating on
//! constant-product adapters derived from bonding-curve snapshots.

use crate::chaos::amm_math::AmmPool;
use crate::chaos::build_pumpfun_amm_pool;
use crate::fast_pipeline::EnhancedCandidate;
use crate::pumpfun::{
    CurveSnapshot, GENESIS_FEE_BPS, GENESIS_VIRTUAL_SOL_LAMPORTS, GENESIS_VIRTUAL_TOKEN_AMOUNT,
};
use ghost_core::init_pool_parser::AmmType;
use ghost_core::shadow_ledger::LAMPORTS_PER_SOL;
use std::cmp::min;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::time::Instant;

/// Detailed diagnostics for LIGMA simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct LigmaDiagnostics {
    /// Price impact for each tested buy size (basis points).
    pub price_impacts_bps: Vec<f64>,
    /// Round-trip loss (buy then immediate sell) per size (basis points).
    pub round_trip_loss_bps: Vec<f64>,
    /// Tokens received for each simulated buy.
    pub tokens_out: Vec<u128>,
    /// Trade sizes used during simulation (lamports of SOL in).
    pub trade_sizes_lamports: Vec<u128>,
    /// Worst observed trap expressed as (lamports in, loss bps).
    pub worst_trap: (u128, f64),
    /// Source of pool state: "explicit", "candidate", or "genesis".
    pub source: &'static str,
    /// Wall-clock time spent computing the manifold (microseconds).
    pub analysis_time_us: u64,
}

/// LIGMA aggregated result with human- and machine-friendly metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct LigmaResult {
    /// Retail tradability score (0-1): fraction of sizes with low impact and gentle slope.
    pub tradability_score: f64,
    /// Sniper attractiveness (0-1): micro-trade jump and early convexity.
    pub sniper_attractiveness: f64,
    /// Liquidity trap risk (0-1): worst round-trip loss and buy/sell asymmetry.
    pub liquidity_trap_risk: f64,
    /// Phase used for QASS integration: tradability - trap - sniper (clamped to [-1, 1]).
    pub psi_ligma: f64,
    /// Hash of the manifold vector for clustering/topology analytics.
    pub curve_fingerprint: u64,
    /// Baseline price derived from pool reserves (token B per SOL).
    pub baseline_price: f64,
    /// Fraction of retail-friendly trade sizes (impact below threshold).
    pub retail_fraction: f64,
    /// Smallest simulated trade that remained retail-friendly (SOL).
    pub min_tradeable_sol: f64,
    /// Worst observed round-trip loss (basis points).
    pub worst_round_trip_loss_bps: f64,
    /// Mean convexity of the impact curve (positive => sharper early curve).
    pub impact_convexity: f64,
    /// Confidence in the signal based on pool state quality (0-1).
    pub confidence: f64,
    /// Diagnostic payload with raw samples.
    pub diagnostics: LigmaDiagnostics,
}

/// Compute LIGMA metrics for a candidate using explicit pool state or genesis fallback.
///
/// # Arguments
/// * `candidate` - The enhanced candidate to analyze
/// * `explicit_pool_state` - Optional explicit AMM pool state
/// * `amm_type` - Type of AMM (PumpFun, BonkFun, etc.)
/// * `config` - LIGMA configuration with thresholds and parameters
///
/// # Returns
/// `LigmaResult` with tradability, trap risk, and diagnostic metrics
pub fn compute_ligma(
    candidate: &EnhancedCandidate,
    explicit_pool_state: Option<&AmmPool>,
    amm_type: AmmType,
    config: &crate::config::ghost_brain_config::LigmaConfig,
) -> LigmaResult {
    let start = Instant::now();
    let (pool, source, mut confidence) =
        resolve_pool_state(candidate, explicit_pool_state, amm_type);
    let trade_sizes = build_trade_sizes(pool.reserve_a, 10);

    let mut price_impacts = Vec::with_capacity(trade_sizes.len());
    let mut round_trip_losses = Vec::with_capacity(trade_sizes.len());
    let mut tokens_out = Vec::with_capacity(trade_sizes.len());

    let mut retail_hits = 0usize;
    let mut min_tradeable_sol = f64::INFINITY;
    let mut worst_trap = (0u128, 0.0f64);

    for &amount_in in &trade_sizes {
        match simulate_round_trip(&pool, amount_in) {
            Some((impact_bps, loss_bps, token_out)) => {
                price_impacts.push(impact_bps);
                round_trip_losses.push(loss_bps);
                tokens_out.push(token_out);

                if impact_bps <= config.retail_impact_limit_bps {
                    retail_hits += 1;
                    min_tradeable_sol =
                        min_tradeable_sol.min(amount_in as f64 / LAMPORTS_PER_SOL as f64);
                }

                if loss_bps > worst_trap.1 {
                    worst_trap = (amount_in, loss_bps);
                }
            }
            None => {
                // Treat failed simulation as maximum risk.
                price_impacts.push(MAX_BPS);
                round_trip_losses.push(MAX_BPS);
                tokens_out.push(0);
                worst_trap = (amount_in, MAX_BPS);
                confidence *= 0.8;
            }
        }
    }

    let retail_fraction = if trade_sizes.is_empty() {
        0.0
    } else {
        retail_hits as f64 / trade_sizes.len() as f64
    };

    let avg_impact = mean(&price_impacts);
    let avg_loss = mean(&round_trip_losses);
    let convexity = impact_convexity(&price_impacts);

    let tradability_score = (0.6 * retail_fraction
        + 0.4 * (1.0 - normalize_bps(avg_impact, config.soft_impact_bps)))
    .clamp(0.0, 1.0);

    let micro_jump = price_impacts.first().copied().unwrap_or(MAX_BPS);
    let sniper_attractiveness = (0.6 * normalize_bps(micro_jump, config.micro_jump_bps)
        + 0.4 * convexity.max(0.0).min(1.0))
    .clamp(0.0, 1.0);

    let liquidity_trap_risk = (0.6 * normalize_bps(worst_trap.1, config.hard_impact_bps)
        + 0.4
            * normalize_bps(
                (price_impacts.last().unwrap_or(&0.0) - micro_jump).abs(),
                config.hard_impact_bps,
            ))
    .clamp(0.0, 1.0);

    let psi_ligma =
        (tradability_score - liquidity_trap_risk - sniper_attractiveness).clamp(-1.0, 1.0);

    let fingerprint = curve_fingerprint(&[
        tradability_score,
        sniper_attractiveness,
        liquidity_trap_risk,
        psi_ligma,
        convexity,
        retail_fraction,
        pool.price_a_in_b(),
    ]);

    let diagnostics = LigmaDiagnostics {
        price_impacts_bps: price_impacts,
        round_trip_loss_bps: round_trip_losses,
        tokens_out,
        trade_sizes_lamports: trade_sizes,
        worst_trap,
        source,
        analysis_time_us: start.elapsed().as_micros() as u64,
    };

    LigmaResult {
        tradability_score,
        sniper_attractiveness,
        liquidity_trap_risk,
        psi_ligma,
        curve_fingerprint: fingerprint,
        baseline_price: pool.price_a_in_b(),
        retail_fraction,
        min_tradeable_sol: if min_tradeable_sol.is_finite() {
            min_tradeable_sol
        } else {
            0.0
        },
        worst_round_trip_loss_bps: diagnostics.worst_trap.1,
        impact_convexity: convexity,
        confidence: confidence.clamp(0.0, 1.0),
        diagnostics,
    }
}

const MAX_BPS: f64 = 10_000.0;
const RETAIL_IMPACT_LIMIT_BPS: f64 = 700.0;
const SOFT_IMPACT_BPS: f64 = 2_000.0;
const HARD_IMPACT_BPS: f64 = 6_000.0;
const MICRO_JUMP_BPS: f64 = 2_500.0;

fn resolve_pool_state(
    candidate: &EnhancedCandidate,
    explicit_pool_state: Option<&AmmPool>,
    amm_type: AmmType,
) -> (AmmPool, &'static str, f64) {
    if let Some(pool) = explicit_pool_state {
        return (*pool, "explicit", 1.0);
    }

    // Attempt to build from candidate virtual reserves if present.
    if let Some(sol_reserve) = candidate.virtual_sol_reserves {
        let snapshot = CurveSnapshot::new(
            sol_reserve,
            GENESIS_VIRTUAL_TOKEN_AMOUNT,
            GENESIS_FEE_BPS,
            candidate.slot,
        );
        if let Ok(pool) = build_pumpfun_amm_pool(&snapshot) {
            return (pool, "candidate", 0.85);
        }
    }

    // Genesis fallback (AMM-agnostic: constant product adapter).
    let snapshot = CurveSnapshot::new(
        GENESIS_VIRTUAL_SOL_LAMPORTS,
        GENESIS_VIRTUAL_TOKEN_AMOUNT,
        GENESIS_FEE_BPS,
        candidate.slot,
    );
    let pool = build_pumpfun_amm_pool(&snapshot)
        .unwrap_or_else(|_| AmmPool::new(1_000_000, 1_000_000, GENESIS_FEE_BPS).unwrap());

    let confidence = match amm_type {
        AmmType::PumpFun => 0.7,
        AmmType::BonkFun => 0.65,
        AmmType::PumpSwap => 0.7,
    };

    (pool, "genesis", confidence)
}

fn simulate_round_trip(pool: &AmmPool, amount_in: u128) -> Option<(f64, f64, u128)> {
    let buy = pool.simulate_swap(amount_in, true).ok()?;

    let post_buy_pool = AmmPool::new(buy.new_reserve_in, buy.new_reserve_out, pool.fee_bps).ok()?;
    let sell = post_buy_pool.simulate_swap(buy.amount_out, false).ok()?;

    let impact_bps = buy.price_impact_bps as f64;
    let loss_bps = if buy.amount_out > 0 {
        let loss = amount_in.saturating_sub(sell.amount_out) as f64;
        (loss / amount_in as f64) * MAX_BPS
    } else {
        MAX_BPS
    };

    Some((impact_bps, loss_bps, buy.amount_out))
}

fn build_trade_sizes(reserve_sol: u128, points: usize) -> Vec<u128> {
    if points == 0 {
        return Vec::new();
    }

    let max_trade = min(reserve_sol / 4, reserve_sol.saturating_sub(1)).max(1);
    let min_trade = min(reserve_sol / 1_000_000, max_trade).max(1);

    log_space(min_trade, max_trade, points)
}

fn log_space(min_val: u128, max_val: u128, count: usize) -> Vec<u128> {
    if count == 0 || min_val == 0 || max_val == 0 {
        return Vec::new();
    }
    if count == 1 || min_val == max_val {
        return vec![min_val];
    }

    let min_f = min_val as f64;
    let max_f = max_val as f64;
    let step = (max_f.ln() - min_f.ln()) / (count as f64 - 1.0);

    (0..count)
        .map(|i| (min_f.ln() + step * i as f64).exp().round() as u128)
        .map(|v| v.max(1))
        .collect()
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn normalize_bps(value: f64, cap: f64) -> f64 {
    (value / cap).clamp(0.0, 1.0)
}

fn impact_convexity(impacts: &[f64]) -> f64 {
    if impacts.len() < 3 {
        return 0.0;
    }

    let mut accum = 0.0;
    let mut count = 0.0;
    for window in impacts.windows(3) {
        let second = window[2] - 2.0 * window[1] + window[0];
        accum += second;
        count += 1.0;
    }

    let avg_second = accum / count;
    // Scale to 0-1 range (assuming meaningful convexity within +/- 500 bps curvature).
    (avg_second / 500.0).clamp(-1.0, 1.0)
}

fn curve_fingerprint(manifold: &[f64]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for value in manifold {
        // Hash deterministic IEEE bits to avoid NaN surprises.
        hasher.write_u64(value.to_bits());
    }
    hasher.finish()
}
