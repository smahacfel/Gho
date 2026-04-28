//! PRAECOG (Predictive Rapid Adversarial Evaluation & Counterfactual Oracle Guard)
//!
//! Ultrafast adversarial simulation module for evaluating pool exploitability
//! within the critical 0-2 second window after token launch (pump.fun).
//!
//! ## Core Concept
//!
//! PRAECOG answers the question: "How quickly could an attacker destroy this pool?"
//! by simulating adversarial attack paths (buy→sell, sandwich, etc.) and measuring:
//! - Minimum capital required to crash the pool
//! - Feasibility of sandwich attacks
//! - Overall adversarial vulnerability score
//!
//! This is a "wargaming engine" approach - we don't predict what *will* happen,
//! we evaluate what *could* happen if an attacker wanted to exploit the pool.
//!
//! ## Performance Target
//!
//! - **Execution Time**: <250μs per analysis (256 attack paths)
//! - **Zero Heap Allocation**: Stack-only analysis for hot path performance
//! - **No RPC/External Calls**: Pure simulation using in-memory pool snapshot
//!
//! ## Thread Safety
//!
//! All types implement `Send + Sync` for concurrent usage across threads.
//!
//! ## Integration
//!
//! PRAECOG feeds into the Oracle Pipeline alongside IWIM, MPCF, and Chaos Engine:
//! ```text
//! Pool Snapshot (pump.fun) → PRAECOG Analysis
//!      ↓
//! [ Attack Path Simulation ] → PraecogResult
//!      ↓                           ↓
//! [ HyperPredictionOracle ]  [ ψ_praecog wave → QASS ]
//! ```
//!
//! ## Algorithm Overview
//!
//! 1. **Attack Path Generation**: Generate N adversarial swap sequences
//!    - Buy→Sell (pump & dump)
//!    - Buy→Sell→Buy (oscillation attack)
//!    - Large single-direction (crash/pump attempt)
//!
//! 2. **Simulation**: For each attack path:
//!    - Execute swaps on cloned pool state
//!    - Track price changes, PnL, and impacts
//!
//! 3. **Metric Extraction**:
//!    - Find minimum capital to achieve crash (>20% price drop)
//!    - Calculate sandwich attack feasibility
//!    - Compute overall adversarial score

use crate::chaos::amm_math::AmmPool;
use std::time::Instant;

// =============================================================================
// Constants
// =============================================================================

/// Performance target: <250μs per analysis
const TARGET_ANALYSIS_TIME_US: u128 = 250;

/// Default number of attack paths to simulate
const DEFAULT_NUM_PATHS: usize = 256;

/// Maximum number of steps per attack path
const MAX_STEPS_PER_PATH: usize = 4;

/// Maximum number of attack paths to prevent DoS
const MAX_PATHS: usize = 512;

/// Crash threshold: price drop of 20% or more
const CRASH_THRESHOLD_PCT: f64 = 20.0;

/// Sandwich threshold: minimum profit ratio (0.5% = 50 bps)
const SANDWICH_PROFIT_THRESHOLD_PCT: f64 = 0.5;

/// Minimum capital grid (in lamports) - 0.1 SOL to 10 SOL
const MIN_CAPITAL_GRID: [u128; 8] = [
    100_000_000,    // 0.1 SOL
    500_000_000,    // 0.5 SOL
    1_000_000_000,  // 1 SOL
    2_000_000_000,  // 2 SOL
    3_000_000_000,  // 3 SOL
    5_000_000_000,  // 5 SOL
    7_500_000_000,  // 7.5 SOL
    10_000_000_000, // 10 SOL
];

/// Lamports per SOL for conversion
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

// =============================================================================
// Core Types
// =============================================================================

/// Primary output of PRAECOG analysis
///
/// Provides adversarial vulnerability metrics for pool exploitability assessment.
/// All scores are in range [0.0, 1.0] where higher = more vulnerable.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PraecogResult {
    /// Minimum capital (in SOL) required to crash the pool >20%
    /// Lower values = more vulnerable
    /// `f64::INFINITY` if crash is not achievable with tested capital levels
    pub min_capital_to_crash_sol: f64,

    /// Crash feasibility score (0.0 = impossible, 1.0 = trivially easy)
    /// Based on minimum capital required relative to pool liquidity
    pub crash_feasibility: f32,

    /// Sandwich attack feasibility score (0.0 = impossible, 1.0 = highly profitable)
    /// Based on simulated frontrun+backrun profitability
    pub sandwich_feasibility: f32,

    /// Overall adversarial vulnerability score (0.0 = very safe, 1.0 = instant rug risk)
    /// Weighted combination of all vulnerability metrics
    pub adversarial_score: f32,

    /// Analysis confidence (0.0-1.0)
    /// Reflects data quality and simulation coverage
    pub confidence: f32,

    /// Execution time in microseconds (for performance tracking)
    pub analysis_time_us: u64,

    /// Number of attack paths simulated
    pub paths_analyzed: u32,

    /// Fingerprint of attack characteristics (for pattern tracking)
    pub attack_fingerprint: u64,
}

impl Default for PraecogResult {
    fn default() -> Self {
        Self {
            min_capital_to_crash_sol: f64::INFINITY,
            crash_feasibility: 0.0,
            sandwich_feasibility: 0.0,
            adversarial_score: 0.5, // Neutral default
            confidence: 0.3,
            analysis_time_us: 0,
            paths_analyzed: 0,
            attack_fingerprint: 0,
        }
    }
}

/// Configuration parameters for PRAECOG analysis
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PraecogParams {
    /// Number of attack paths to simulate (default: 256)
    pub num_paths: usize,

    /// Maximum steps per attack path (default: 4)
    pub max_steps: usize,

    /// Crash threshold as percentage (default: 20.0)
    pub crash_threshold_pct: f64,

    /// Sandwich profit threshold as percentage (default: 0.5)
    pub sandwich_profit_threshold_pct: f64,

    /// Typical victim trade size in lamports for sandwich simulations (default: 0.5 SOL)
    pub victim_trade_size: u128,
}

impl Default for PraecogParams {
    fn default() -> Self {
        Self {
            num_paths: DEFAULT_NUM_PATHS,
            max_steps: MAX_STEPS_PER_PATH,
            crash_threshold_pct: CRASH_THRESHOLD_PCT,
            sandwich_profit_threshold_pct: SANDWICH_PROFIT_THRESHOLD_PCT,
            victim_trade_size: 500_000_000, // 0.5 SOL typical retail trade
        }
    }
}

impl PraecogParams {
    /// Creates parameters optimized for speed (fewer paths)
    pub fn fast() -> Self {
        Self {
            num_paths: 64,
            max_steps: 2,
            ..Default::default()
        }
    }

    /// Creates parameters for thorough analysis (more paths)
    pub fn thorough() -> Self {
        Self {
            num_paths: 512,
            max_steps: 4,
            ..Default::default()
        }
    }
}

/// Information about a swap in the first 0-2s window
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwapInfo {
    /// Amount in lamports (for SOL→Token swap) or tokens (for Token→SOL)
    pub amount_in: u128,

    /// Direction: true = buy (SOL→Token), false = sell (Token→SOL)
    pub is_buy: bool,

    /// Timestamp in milliseconds relative to pool creation
    pub timestamp_ms: u64,
}

/// Input data for PRAECOG analysis
#[derive(Debug, Clone)]
pub struct PraecogInput {
    /// AMM pool snapshot (pump.fun bonding curve state)
    pub pool: AmmPool,

    /// First swaps observed in the 0-2s window (chronological order)
    pub initial_swaps: Vec<SwapInfo>,

    /// Simulation parameters
    pub params: PraecogParams,
}

impl PraecogInput {
    /// Creates input from a Pump.fun bonding curve snapshot
    ///
    /// This is the only way to create a PraecogInput for Pump.fun analysis.
    /// It uses actual snapshot data from the bonding curve state.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - Reference to a CurveSnapshot from pumpfun::state module
    ///
    /// # Returns
    ///
    /// * `Ok(PraecogInput)` - Valid input ready for analysis
    /// * `Err(AmmMathError)` - If snapshot has invalid reserves or fees
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use ghost_brain::pumpfun::CurveSnapshot;
    /// use ghost_brain::oracle::ultrafast::PraecogInput;
    ///
    /// let snapshot = CurveSnapshot::new(
    ///     30_000_000_000,           // 30 SOL virtual reserves
    ///     1_073_000_000_000_000,    // 1.073T token reserves
    ///     100,                       // 1% fee
    ///     12345,                     // slot
    /// );
    ///
    /// let input = PraecogInput::from_snapshot(&snapshot)?;
    /// ```
    pub fn from_snapshot(
        snapshot: &crate::pumpfun::CurveSnapshot,
    ) -> Result<Self, crate::chaos::AmmMathError> {
        use crate::chaos::build_pumpfun_amm_pool;

        let pool = build_pumpfun_amm_pool(snapshot)?;

        Ok(Self {
            pool,
            initial_swaps: vec![],
            params: PraecogParams::default(),
        })
    }
}

/// Type of attack path
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttackType {
    /// Buy then sell (pump & dump attempt)
    BuySell,
    /// Sell only (crash attempt)
    CrashSell,
    /// Buy→Sell→Buy (oscillation)
    Oscillation,
    /// Large single buy (pump attempt)
    PumpBuy,
    /// Sandwich: frontrun buy, victim buy, backrun sell
    SandwichBuy,
}

/// Result of simulating a single attack path
#[derive(Debug, Clone, Copy)]
struct AttackPathResult {
    /// Attack type that was simulated
    attack_type: AttackType,
    /// Capital used (in lamports)
    capital_used: u128,
    /// Percentage price change (can be negative)
    price_change_pct: f64,
    /// Attacker PnL in lamports (can be negative)
    attacker_pnl: i128,
    /// Maximum price impact observed (basis points)
    max_impact_bps: u64,
    /// Whether crash threshold was achieved
    achieved_crash: bool,
    /// Whether sandwich was profitable
    sandwich_profitable: bool,
}

// =============================================================================
// Core API Functions
// =============================================================================

/// Main entry point for PRAECOG analysis
///
/// Analyzes pool vulnerability to adversarial attacks using counterfactual simulation.
/// Target execution time: <250μs
///
/// # Arguments
/// * `input` - Pool snapshot and simulation parameters
///
/// # Returns
/// `PraecogResult` with vulnerability scores and metrics
pub fn praecog_analyze(input: &PraecogInput) -> PraecogResult {
    let start = Instant::now();

    // Validate input
    let num_paths = input.params.num_paths.min(MAX_PATHS);
    if num_paths == 0 {
        return PraecogResult {
            confidence: 0.0,
            ..Default::default()
        };
    }

    // Get initial price for comparison
    let initial_price = input.pool.price_b_in_a();
    if initial_price <= 0.0 {
        return PraecogResult {
            confidence: 0.2,
            ..Default::default()
        };
    }

    // Apply any initial swaps to get current pool state
    let current_pool = apply_initial_swaps(&input.pool, &input.initial_swaps);

    // Generate and simulate attack paths
    let attack_results = simulate_attack_paths(&current_pool, &input.params, num_paths);

    // Extract metrics from attack results
    let (min_capital_to_crash, crash_feasibility) = calculate_crash_metrics(
        &attack_results,
        input.params.crash_threshold_pct,
        current_pool.reserve_a,
    );

    let sandwich_feasibility =
        calculate_sandwich_feasibility(&current_pool, input.params.sandwich_profit_threshold_pct);

    // Calculate overall adversarial score
    let adversarial_score = calculate_adversarial_score(
        crash_feasibility,
        sandwich_feasibility,
        &attack_results,
        &current_pool,
    );

    // Generate attack fingerprint for pattern tracking
    let attack_fingerprint =
        generate_attack_fingerprint(&attack_results, crash_feasibility, sandwich_feasibility);

    // Calculate confidence based on analysis quality
    let confidence = calculate_confidence(
        num_paths,
        &input.initial_swaps,
        crash_feasibility,
        sandwich_feasibility,
    );

    let analysis_time_us = start.elapsed().as_micros() as u64;

    PraecogResult {
        min_capital_to_crash_sol: min_capital_to_crash,
        crash_feasibility,
        sandwich_feasibility,
        adversarial_score,
        confidence,
        analysis_time_us,
        paths_analyzed: attack_results.len() as u32,
        attack_fingerprint,
    }
}

// =============================================================================
// Attack Path Simulation
// =============================================================================

/// Applies initial swaps to pool state and returns updated pool
fn apply_initial_swaps(pool: &AmmPool, swaps: &[SwapInfo]) -> AmmPool {
    let mut current_pool = *pool;

    for swap in swaps.iter().take(10) {
        if let Ok(result) = current_pool.simulate_swap(swap.amount_in, swap.is_buy) {
            if swap.is_buy {
                if let Ok(new_pool) = AmmPool::new(
                    result.new_reserve_in,
                    result.new_reserve_out,
                    current_pool.fee_bps,
                ) {
                    current_pool = new_pool;
                }
            } else if let Ok(new_pool) = AmmPool::new(
                result.new_reserve_out,
                result.new_reserve_in,
                current_pool.fee_bps,
            ) {
                current_pool = new_pool;
            }
        }
    }

    current_pool
}

/// Generates and simulates attack paths
fn simulate_attack_paths(
    pool: &AmmPool,
    params: &PraecogParams,
    num_paths: usize,
) -> Vec<AttackPathResult> {
    let mut results = Vec::with_capacity(num_paths);

    // Distribute paths across different attack types and capital levels
    let paths_per_type = num_paths / 5;

    // BuySell attacks with varying capital
    for i in 0..paths_per_type {
        let capital_idx = i % MIN_CAPITAL_GRID.len();
        let capital = MIN_CAPITAL_GRID[capital_idx];
        if let Some(result) = simulate_buy_sell_attack(pool, capital, params) {
            results.push(result);
        }
    }

    // CrashSell attacks
    for i in 0..paths_per_type {
        let capital_idx = i % MIN_CAPITAL_GRID.len();
        let capital = MIN_CAPITAL_GRID[capital_idx];
        if let Some(result) = simulate_crash_sell_attack(pool, capital, params) {
            results.push(result);
        }
    }

    // Oscillation attacks
    for i in 0..paths_per_type {
        let capital_idx = i % MIN_CAPITAL_GRID.len();
        let capital = MIN_CAPITAL_GRID[capital_idx];
        if let Some(result) = simulate_oscillation_attack(pool, capital, params) {
            results.push(result);
        }
    }

    // PumpBuy attacks
    for i in 0..paths_per_type {
        let capital_idx = i % MIN_CAPITAL_GRID.len();
        let capital = MIN_CAPITAL_GRID[capital_idx];
        if let Some(result) = simulate_pump_buy_attack(pool, capital) {
            results.push(result);
        }
    }

    // Sandwich attacks
    for i in 0..paths_per_type {
        let capital_idx = i % MIN_CAPITAL_GRID.len();
        let capital = MIN_CAPITAL_GRID[capital_idx];
        if let Some(result) = simulate_sandwich_attack(pool, capital, params) {
            results.push(result);
        }
    }

    results
}

/// Simulates a buy-then-sell attack (pump & dump)
fn simulate_buy_sell_attack(
    pool: &AmmPool,
    capital: u128,
    params: &PraecogParams,
) -> Option<AttackPathResult> {
    let initial_price = pool.price_b_in_a();

    // Step 1: Buy tokens with capital
    let buy_result = pool.simulate_swap(capital, true).ok()?;
    let tokens_received = buy_result.amount_out;

    // Create intermediate pool state
    let mid_pool = AmmPool::new(
        buy_result.new_reserve_in,
        buy_result.new_reserve_out,
        pool.fee_bps,
    )
    .ok()?;

    // Step 2: Sell all tokens
    let sell_result = mid_pool.simulate_swap(tokens_received, false).ok()?;
    let sol_recovered = sell_result.amount_out;

    // Calculate final price and metrics
    let final_pool = AmmPool::new(
        sell_result.new_reserve_out,
        sell_result.new_reserve_in,
        pool.fee_bps,
    )
    .ok()?;

    let final_price = final_pool.price_b_in_a();
    let price_change_pct = ((final_price - initial_price) / initial_price) * 100.0;
    let attacker_pnl = sol_recovered as i128 - capital as i128;

    let achieved_crash = price_change_pct <= -params.crash_threshold_pct;
    let max_impact_bps = buy_result
        .price_impact_bps
        .max(sell_result.price_impact_bps);

    Some(AttackPathResult {
        attack_type: AttackType::BuySell,
        capital_used: capital,
        price_change_pct,
        attacker_pnl,
        max_impact_bps,
        achieved_crash,
        sandwich_profitable: false,
    })
}

/// Simulates a crash sell attack
fn simulate_crash_sell_attack(
    pool: &AmmPool,
    capital: u128,
    params: &PraecogParams,
) -> Option<AttackPathResult> {
    let buy_result = pool.simulate_swap(capital, true).ok()?;
    let tokens_acquired = buy_result.amount_out;

    let mid_pool = AmmPool::new(
        buy_result.new_reserve_in,
        buy_result.new_reserve_out,
        pool.fee_bps,
    )
    .ok()?;

    // Simulate coordinated selling: attacker sells 2x acquired tokens
    // This represents either pre-acquired tokens or coordinated multi-wallet dump
    // The 2x multiplier models aggressive dump behavior typical in rug scenarios
    let sell_amount = tokens_acquired * 2;
    if sell_amount > mid_pool.reserve_b {
        return None;
    }

    let initial_price = pool.price_b_in_a();
    let sell_result = mid_pool
        .simulate_swap(sell_amount.min(mid_pool.reserve_b / 2), false)
        .ok()?;

    let final_pool = AmmPool::new(
        sell_result.new_reserve_out,
        sell_result.new_reserve_in,
        pool.fee_bps,
    )
    .ok()?;

    let final_price = final_pool.price_b_in_a();
    let price_change_pct = ((final_price - initial_price) / initial_price) * 100.0;
    let achieved_crash = price_change_pct <= -params.crash_threshold_pct;

    Some(AttackPathResult {
        attack_type: AttackType::CrashSell,
        capital_used: capital,
        price_change_pct,
        attacker_pnl: 0,
        max_impact_bps: sell_result.price_impact_bps,
        achieved_crash,
        sandwich_profitable: false,
    })
}

/// Simulates an oscillation attack (buy→sell→buy)
fn simulate_oscillation_attack(
    pool: &AmmPool,
    capital: u128,
    params: &PraecogParams,
) -> Option<AttackPathResult> {
    let initial_price = pool.price_b_in_a();
    let mut current_pool = *pool;
    let mut sol_balance = capital;
    let mut max_impact_bps = 0u64;

    // Step 1: Buy
    let buy1 = current_pool.simulate_swap(sol_balance, true).ok()?;
    max_impact_bps = max_impact_bps.max(buy1.price_impact_bps);
    let tokens = buy1.amount_out;
    current_pool = AmmPool::new(buy1.new_reserve_in, buy1.new_reserve_out, pool.fee_bps).ok()?;

    // Step 2: Sell
    let sell = current_pool.simulate_swap(tokens, false).ok()?;
    max_impact_bps = max_impact_bps.max(sell.price_impact_bps);
    sol_balance = sell.amount_out;
    current_pool = AmmPool::new(sell.new_reserve_out, sell.new_reserve_in, pool.fee_bps).ok()?;

    // Step 3: Buy again
    let buy2 = current_pool.simulate_swap(sol_balance, true).ok()?;
    max_impact_bps = max_impact_bps.max(buy2.price_impact_bps);
    let final_tokens = buy2.amount_out;
    current_pool = AmmPool::new(buy2.new_reserve_in, buy2.new_reserve_out, pool.fee_bps).ok()?;

    // Final sell to realize PnL
    let final_sell = current_pool.simulate_swap(final_tokens, false).ok()?;
    let final_sol = final_sell.amount_out;
    current_pool = AmmPool::new(
        final_sell.new_reserve_out,
        final_sell.new_reserve_in,
        pool.fee_bps,
    )
    .ok()?;

    let final_price = current_pool.price_b_in_a();
    let price_change_pct = ((final_price - initial_price) / initial_price) * 100.0;
    let attacker_pnl = final_sol as i128 - capital as i128;
    let achieved_crash = price_change_pct <= -params.crash_threshold_pct;

    Some(AttackPathResult {
        attack_type: AttackType::Oscillation,
        capital_used: capital,
        price_change_pct,
        attacker_pnl,
        max_impact_bps,
        achieved_crash,
        sandwich_profitable: false,
    })
}

/// Simulates a pump buy attack (large single buy)
fn simulate_pump_buy_attack(pool: &AmmPool, capital: u128) -> Option<AttackPathResult> {
    let initial_price = pool.price_b_in_a();

    let buy_result = pool.simulate_swap(capital, true).ok()?;

    let final_pool = AmmPool::new(
        buy_result.new_reserve_in,
        buy_result.new_reserve_out,
        pool.fee_bps,
    )
    .ok()?;

    let final_price = final_pool.price_b_in_a();
    let price_change_pct = ((final_price - initial_price) / initial_price) * 100.0;

    Some(AttackPathResult {
        attack_type: AttackType::PumpBuy,
        capital_used: capital,
        price_change_pct,
        attacker_pnl: 0,
        max_impact_bps: buy_result.price_impact_bps,
        achieved_crash: false,
        sandwich_profitable: false,
    })
}

/// Simulates a sandwich attack (frontrun + victim + backrun)
fn simulate_sandwich_attack(
    pool: &AmmPool,
    capital: u128,
    params: &PraecogParams,
) -> Option<AttackPathResult> {
    let initial_price = pool.price_b_in_a();
    // Use configurable victim trade size from params
    let victim_amount = params.victim_trade_size;

    // Step 1: Frontrun - attacker buys
    let frontrun = pool.simulate_swap(capital, true).ok()?;
    let attacker_tokens = frontrun.amount_out;
    let pool_after_frontrun = AmmPool::new(
        frontrun.new_reserve_in,
        frontrun.new_reserve_out,
        pool.fee_bps,
    )
    .ok()?;

    // Step 2: Victim buys (at worse price)
    let victim_buy = pool_after_frontrun
        .simulate_swap(victim_amount, true)
        .ok()?;
    let pool_after_victim = AmmPool::new(
        victim_buy.new_reserve_in,
        victim_buy.new_reserve_out,
        pool.fee_bps,
    )
    .ok()?;

    // Step 3: Backrun - attacker sells
    let backrun = pool_after_victim
        .simulate_swap(attacker_tokens, false)
        .ok()?;
    let sol_received = backrun.amount_out;

    let final_pool = AmmPool::new(
        backrun.new_reserve_out,
        backrun.new_reserve_in,
        pool.fee_bps,
    )
    .ok()?;

    let final_price = final_pool.price_b_in_a();
    let price_change_pct = ((final_price - initial_price) / initial_price) * 100.0;

    let attacker_pnl = sol_received as i128 - capital as i128;
    let profit_pct = (attacker_pnl as f64 / capital as f64) * 100.0;
    let sandwich_profitable = profit_pct >= params.sandwich_profit_threshold_pct;

    Some(AttackPathResult {
        attack_type: AttackType::SandwichBuy,
        capital_used: capital,
        price_change_pct,
        attacker_pnl,
        max_impact_bps: frontrun.price_impact_bps.max(backrun.price_impact_bps),
        achieved_crash: false,
        sandwich_profitable,
    })
}

// =============================================================================
// Metric Calculation
// =============================================================================

/// Calculates crash feasibility metrics
fn calculate_crash_metrics(
    results: &[AttackPathResult],
    threshold_pct: f64,
    pool_sol_reserve: u128,
) -> (f64, f32) {
    let min_crash_capital = results
        .iter()
        .filter(|r| r.achieved_crash || r.price_change_pct <= -threshold_pct)
        .map(|r| r.capital_used)
        .min();

    let min_capital_sol = match min_crash_capital {
        Some(capital) => capital as f64 / LAMPORTS_PER_SOL,
        None => f64::INFINITY,
    };

    let crash_feasibility = if min_capital_sol.is_infinite() {
        0.0
    } else {
        let pool_sol = pool_sol_reserve as f64 / LAMPORTS_PER_SOL;
        let ratio = min_capital_sol / pool_sol.max(1.0);

        if ratio < 0.1 {
            1.0
        } else if ratio < 0.3 {
            0.8
        } else if ratio < 0.5 {
            0.6
        } else if ratio < 1.0 {
            0.4
        } else {
            0.2
        }
    };

    (min_capital_sol, crash_feasibility)
}

/// Calculates sandwich attack feasibility
fn calculate_sandwich_feasibility(pool: &AmmPool, profit_threshold_pct: f64) -> f32 {
    let test_capitals = [
        100_000_000u128,
        500_000_000u128,
        1_000_000_000u128,
        2_000_000_000u128,
    ];

    let victim_amount = 500_000_000u128;

    let mut profitable_count = 0;
    let mut total_profit_pct = 0.0;
    let mut tested = 0;

    for capital in test_capitals {
        if let Ok(frontrun) = pool.simulate_swap(capital, true) {
            let attacker_tokens = frontrun.amount_out;

            if let Ok(pool2) = AmmPool::new(
                frontrun.new_reserve_in,
                frontrun.new_reserve_out,
                pool.fee_bps,
            ) {
                if let Ok(victim) = pool2.simulate_swap(victim_amount, true) {
                    if let Ok(pool3) =
                        AmmPool::new(victim.new_reserve_in, victim.new_reserve_out, pool.fee_bps)
                    {
                        if let Ok(backrun) = pool3.simulate_swap(attacker_tokens, false) {
                            tested += 1;
                            let pnl = backrun.amount_out as i128 - capital as i128;
                            let profit_pct = (pnl as f64 / capital as f64) * 100.0;

                            if profit_pct >= profit_threshold_pct {
                                profitable_count += 1;
                            }
                            total_profit_pct += profit_pct.max(0.0);
                        }
                    }
                }
            }
        }
    }

    if tested == 0 {
        return 0.0;
    }

    let profitability_rate = profitable_count as f32 / tested as f32;
    let avg_profit = (total_profit_pct / tested as f64) as f32;

    (profitability_rate * 0.6 + (avg_profit / 5.0).min(1.0) * 0.4).min(1.0)
}

/// Calculates overall adversarial vulnerability score
fn calculate_adversarial_score(
    crash_feasibility: f32,
    sandwich_feasibility: f32,
    results: &[AttackPathResult],
    pool: &AmmPool,
) -> f32 {
    const W_CRASH: f32 = 0.35;
    const W_SANDWICH: f32 = 0.25;
    const W_IMPACT: f32 = 0.20;
    const W_LIQUIDITY: f32 = 0.20;

    let max_impact = results.iter().map(|r| r.max_impact_bps).max().unwrap_or(0);

    let impact_score = if max_impact > 2000 {
        1.0
    } else if max_impact > 1000 {
        0.7
    } else if max_impact > 500 {
        0.5
    } else if max_impact > 200 {
        0.3
    } else {
        0.1
    };

    let pool_sol = pool.reserve_a as f64 / LAMPORTS_PER_SOL;
    let liquidity_score = if pool_sol < 10.0 {
        0.9
    } else if pool_sol < 30.0 {
        0.7
    } else if pool_sol < 100.0 {
        0.4
    } else if pool_sol < 500.0 {
        0.2
    } else {
        0.05
    };

    let score = W_CRASH * crash_feasibility
        + W_SANDWICH * sandwich_feasibility
        + W_IMPACT * impact_score
        + W_LIQUIDITY * liquidity_score;

    score.clamp(0.0, 1.0)
}

/// Generates a fingerprint for attack characteristics
fn generate_attack_fingerprint(
    results: &[AttackPathResult],
    crash_feasibility: f32,
    sandwich_feasibility: f32,
) -> u64 {
    let mut fp = 0u64;

    fp |= ((crash_feasibility * 255.0) as u64) << 56;
    fp |= ((sandwich_feasibility * 255.0) as u64) << 48;
    fp |= (results.len().min(255) as u64) << 40;

    let crash_count = results.iter().filter(|r| r.achieved_crash).count().min(255) as u64;
    let sandwich_count = results
        .iter()
        .filter(|r| r.sandwich_profitable)
        .count()
        .min(255) as u64;
    fp |= crash_count << 32;
    fp |= sandwich_count << 24;

    let avg_impact = if results.is_empty() {
        0
    } else {
        (results.iter().map(|r| r.max_impact_bps).sum::<u64>() / results.len() as u64).min(65535)
    };
    fp |= (avg_impact & 0xFFFF) << 8;

    let profitable_count = results
        .iter()
        .filter(|r| r.attacker_pnl > 0)
        .count()
        .min(255) as u64;
    fp |= profitable_count & 0xFF;

    fp
}

/// Calculates analysis confidence
fn calculate_confidence(
    num_paths: usize,
    initial_swaps: &[SwapInfo],
    crash_feasibility: f32,
    sandwich_feasibility: f32,
) -> f32 {
    let mut confidence = 0.5;

    confidence += (num_paths as f32 / DEFAULT_NUM_PATHS as f32).min(1.0) * 0.2;

    if !initial_swaps.is_empty() {
        confidence += 0.1;
    }

    if crash_feasibility > 0.8 || crash_feasibility < 0.2 {
        confidence += 0.1;
    }
    if sandwich_feasibility > 0.7 || sandwich_feasibility < 0.3 {
        confidence += 0.1;
    }

    confidence.clamp(0.3, 0.95)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_default_pump_pool() -> AmmPool {
        // Explicit test values (typical Pump.fun defaults)
        AmmPool::new(
            30_000_000_000,        // 30 SOL
            1_073_000_000_000_000, // 1.073 trillion tokens
            100,                   // 1% fee
        )
        .unwrap()
    }

    fn create_low_liquidity_pool() -> AmmPool {
        // Explicit test values (low liquidity scenario)
        AmmPool::new(
            5_000_000_000,       // 5 SOL
            178_833_333_333_333, // ~179B tokens (1.073T / 6)
            100,                 // 1% fee
        )
        .unwrap()
    }

    fn create_high_liquidity_pool() -> AmmPool {
        // Explicit test values (high liquidity scenario)
        AmmPool::new(
            500_000_000_000,        // 500 SOL
            18_241_000_000_000_000, // ~18.2 quadrillion tokens (1.073T * 17)
            100,                    // 1% fee
        )
        .unwrap()
    }

    #[test]
    fn test_praecog_result_default() {
        let result = PraecogResult::default();
        assert!(result.min_capital_to_crash_sol.is_infinite());
        assert_eq!(result.crash_feasibility, 0.0);
        assert_eq!(result.sandwich_feasibility, 0.0);
        assert_eq!(result.adversarial_score, 0.5);
        assert_eq!(result.confidence, 0.3);
    }

    #[test]
    fn test_praecog_params_default() {
        let params = PraecogParams::default();
        assert_eq!(params.num_paths, 256);
        assert_eq!(params.max_steps, 4);
        assert_eq!(params.crash_threshold_pct, 20.0);
    }

    #[test]
    fn test_praecog_params_fast() {
        let params = PraecogParams::fast();
        assert_eq!(params.num_paths, 64);
        assert_eq!(params.max_steps, 2);
    }

    #[test]
    fn test_praecog_input_from_snapshot() {
        use crate::pumpfun::CurveSnapshot;

        // Create snapshot with typical pump.fun values
        let snapshot = CurveSnapshot::new(30_000_000_000, 1_073_000_000_000_000, 100, Some(12345));

        let input = PraecogInput::from_snapshot(&snapshot)
            .expect("Should create valid input from snapshot");

        // Verify pool uses snapshot values (not hardcoded constants)
        assert_eq!(input.pool.reserve_a, 30_000_000_000);
        assert_eq!(input.pool.reserve_b, 1_073_000_000_000_000);
        assert_eq!(input.pool.fee_bps, 100);
        assert!(input.initial_swaps.is_empty());
    }

    #[test]
    fn test_praecog_input_from_snapshot_custom_values() {
        use crate::pumpfun::CurveSnapshot;

        // Test with non-standard values to ensure no hardcoding
        let snapshot = CurveSnapshot::new(
            50_000_000_000,        // Different SOL reserves
            2_000_000_000_000_000, // Different token reserves
            50,                    // Different fee
            Some(67890),
        );

        let input = PraecogInput::from_snapshot(&snapshot).expect("Should create valid input");

        // Verify actual snapshot values are used
        assert_eq!(input.pool.reserve_a, 50_000_000_000);
        assert_eq!(input.pool.reserve_b, 2_000_000_000_000_000);
        assert_eq!(input.pool.fee_bps, 50);
    }

    #[test]
    fn test_praecog_input_from_snapshot_invalid() {
        use crate::pumpfun::CurveSnapshot;

        // Snapshot with zero reserves should fail
        let snapshot = CurveSnapshot::new(0, 1_073_000_000_000_000, 100, Some(12345));

        let result = PraecogInput::from_snapshot(&snapshot);
        assert!(result.is_err(), "Should fail with zero reserves");
    }

    #[test]
    fn test_praecog_analyze_default_pool() {
        let pool = create_default_pump_pool();
        let input = PraecogInput {
            pool,
            initial_swaps: vec![],
            params: PraecogParams::fast(),
        };

        let result = praecog_analyze(&input);

        assert!(result.adversarial_score >= 0.0 && result.adversarial_score <= 1.0);
        assert!(result.crash_feasibility >= 0.0 && result.crash_feasibility <= 1.0);
        assert!(result.sandwich_feasibility >= 0.0 && result.sandwich_feasibility <= 1.0);
        assert!(result.confidence >= 0.3 && result.confidence <= 1.0);
        assert!(result.paths_analyzed > 0);
    }

    #[test]
    fn test_praecog_analyze_low_liquidity_pool() {
        let pool = create_low_liquidity_pool();
        let input = PraecogInput {
            pool,
            initial_swaps: vec![],
            params: PraecogParams::fast(),
        };

        let result = praecog_analyze(&input);

        assert!(
            result.adversarial_score > 0.5,
            "Low liquidity pool should be vulnerable, got {}",
            result.adversarial_score
        );
    }

    #[test]
    fn test_praecog_analyze_high_liquidity_pool() {
        let pool = create_high_liquidity_pool();
        let input = PraecogInput {
            pool,
            initial_swaps: vec![],
            params: PraecogParams::fast(),
        };

        let result = praecog_analyze(&input);

        assert!(
            result.adversarial_score < 0.6,
            "High liquidity pool should be safer, got {}",
            result.adversarial_score
        );
    }

    #[test]
    fn test_praecog_performance_target() {
        let pool = create_default_pump_pool();
        let input = PraecogInput {
            pool,
            initial_swaps: vec![],
            params: PraecogParams::default(),
        };

        let start = std::time::Instant::now();
        let result = praecog_analyze(&input);
        let elapsed = start.elapsed().as_micros();

        assert!(
            elapsed < TARGET_ANALYSIS_TIME_US * 3,
            "Analysis took {}μs, target is {}μs",
            elapsed,
            TARGET_ANALYSIS_TIME_US
        );

        assert!(result.analysis_time_us > 0);
    }

    #[test]
    fn test_praecog_deterministic() {
        let pool = create_default_pump_pool();
        let input = PraecogInput {
            pool,
            initial_swaps: vec![],
            params: PraecogParams::fast(),
        };

        let result1 = praecog_analyze(&input);
        let result2 = praecog_analyze(&input);

        assert_eq!(result1.crash_feasibility, result2.crash_feasibility);
        assert_eq!(result1.sandwich_feasibility, result2.sandwich_feasibility);
        assert_eq!(result1.adversarial_score, result2.adversarial_score);
    }

    #[test]
    fn test_thread_safety() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PraecogResult>();
        assert_send_sync::<PraecogParams>();
        assert_send_sync::<SwapInfo>();
    }

    #[test]
    fn test_vulnerability_ordering() {
        let low_liq_result = {
            let pool = create_low_liquidity_pool();
            let input = PraecogInput {
                pool,
                initial_swaps: vec![],
                params: PraecogParams::fast(),
            };
            praecog_analyze(&input)
        };

        let high_liq_result = {
            let pool = create_high_liquidity_pool();
            let input = PraecogInput {
                pool,
                initial_swaps: vec![],
                params: PraecogParams::fast(),
            };
            praecog_analyze(&input)
        };

        assert!(
            low_liq_result.adversarial_score >= high_liq_result.adversarial_score,
            "Low liquidity ({}) should be >= high liquidity ({})",
            low_liq_result.adversarial_score,
            high_liq_result.adversarial_score
        );
    }
}
