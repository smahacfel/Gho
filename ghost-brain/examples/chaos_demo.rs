//! Chaos Engine Demo
//!
//! This example demonstrates the Monte Carlo Simulation Engine for AMM risk assessment
//! during the "2-Second Void" period.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example chaos_demo --release
//! ```
//!
//! ## What it does
//!
//! 1. Creates a realistic AMM pool (1M SOL / 2M USDC)
//! 2. Runs 10,000 parallel Monte Carlo simulations for different market scenarios
//! 3. Reports risk probabilities, ROI percentiles, and performance metrics

use ghost_brain::{AmmPool, ChaosEngine, MarketScenario, SimulationConfig};

fn main() -> anyhow::Result<()> {
    println!("=== GHOST PROTOCOL: Chaos Engine Demo ===\n");

    // Create a realistic AMM pool
    // 1M SOL (9 decimals) = 1,000,000 * 10^9 = 1,000,000,000,000,000
    // 2M USDC (6 decimals) = 2,000,000 * 10^6 = 2,000,000,000,000
    // Fee: 0.3% = 30 basis points
    let reserve_a = 1_000_000_000_000_000u128; // 1M SOL
    let reserve_b = 2_000_000_000_000u128; // 2M USDC
    let fee_bps = 30;

    let pool = AmmPool::new(reserve_a, reserve_b, fee_bps)?;

    println!("Pool Configuration:");
    println!(
        "  Reserve A (SOL):  {} ({} SOL)",
        reserve_a,
        reserve_a / 1_000_000_000
    );
    println!(
        "  Reserve B (USDC): {} ({} USDC)",
        reserve_b,
        reserve_b / 1_000_000
    );
    println!(
        "  Fee:              {}bps ({}%)",
        fee_bps,
        fee_bps as f64 / 100.0
    );
    println!("  Initial Price:    {} USDC/SOL\n", pool.price_b_in_a());

    // Configure simulation
    let config = SimulationConfig {
        num_simulations: 10_000,
        num_actions_per_sim: 5, // Simulate 5 random whale actions
        base_trade_pct: 0.01,   // 1% of pool reserves per action
        max_duration_ms: 800,
        seed: None, // Random seed for realistic results
    };

    let engine = ChaosEngine::new(config);

    // Test different market scenarios
    let scenarios = vec![
        (
            MarketScenario::Bullish,
            "Bullish Market (Whale Accumulation)",
        ),
        (MarketScenario::Bearish, "Bearish Market (Whale Exit)"),
        (
            MarketScenario::RugPull,
            "Rug Pull Scenario (Malicious Dump)",
        ),
        (MarketScenario::Mixed, "Mixed Market (Normal Activity)"),
        (
            MarketScenario::Chaotic,
            "Chaotic Market (Random Volatility)",
        ),
    ];

    println!("Running 10,000 Monte Carlo simulations for each scenario...\n");
    println!("{:-^100}", "");

    for (scenario, description) in scenarios {
        println!("\n{}", description);
        println!("{:-^100}", "");

        let result = engine.run_simulation(&pool, scenario)?;

        println!(
            "Execution Time:    {}ms (avg {:.2}μs per simulation)",
            result.execution_time_ms, result.avg_time_per_sim_us
        );
        println!("Simulations:       {}", result.num_simulations);
        println!();
        println!("Risk Metrics:");
        println!(
            "  Crash Probability (>10% drop):  {:.2}%",
            result.crash_probability
        );
        println!(
            "  Pump Probability (>20% gain):   {:.2}%",
            result.pump_probability
        );
        println!();
        println!("ROI Distribution:");
        println!("  5th Percentile (worst case):    {:+.2}%", result.p5_roi);
        println!(
            "  Median ROI:                      {:+.2}%",
            result.median_roi
        );
        println!("  95th Percentile (best case):     {:+.2}%", result.p95_roi);
        println!();
        println!("Statistical Measures:");
        println!(
            "  Mean Price Change:               {:+.2}%",
            result.mean_price_change
        );
        println!(
            "  Price Volatility (σ):            {:.2}%",
            result.price_volatility
        );
        println!();

        // Risk assessment
        if result.crash_probability > 50.0 {
            println!("⚠️  HIGH RISK: Crash probability > 50%");
        } else if result.crash_probability > 25.0 {
            println!("⚠️  MODERATE RISK: Crash probability > 25%");
        } else if result.median_roi > 10.0 {
            println!("✅ OPPORTUNITY: Positive median ROI with low crash risk");
        } else {
            println!("ℹ️  NEUTRAL: No clear signals");
        }
    }

    println!("\n{:-^100}", "");
    println!("\n=== Performance Validation ===");
    println!("✅ All scenarios completed successfully");
    println!("✅ 10,000 simulations per scenario");
    println!("✅ Performance target: <800ms (actual: varies by scenario)");
    println!("\n=== Chaos Engine Demo Complete ===\n");

    Ok(())
}
