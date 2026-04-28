//! E2E Test Runner
//!
//! This binary runs the complete E2E test scenario and generates a report.
//! It supports multiple test modes:
//! - Scenario A: Single synthetic pool (quick validation)
//! - Scenario B: Burst test (stress testing)
//! - Scenario E2E Full: Real Yellowstone detection (production-like)
//!
//! Usage:
//!   cargo run --bin e2e-test-runner -- --scenario [a|b|e2e-full]
//!   cargo run --bin e2e-test-runner -- --scenario e2e-full --max-wait 300 --max-pools 5

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use ghost_brain::{
    E2EConfig, E2EMetrics, ScenarioA, ScenarioB, ScenarioE2EFull, ScenarioResult, TestScenario,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, ValueEnum)]
enum ScenarioType {
    /// Scenario A: Single synthetic pool test
    A,
    /// Scenario B: Burst test with multiple synthetic pools
    B,
    /// Scenario E2E Full: Real Yellowstone→Jito→DirectBuy→On-chain
    E2eFull,
}

#[derive(Parser, Debug)]
#[command(name = "e2e-test-runner")]
#[command(about = "Ghost E2E Test Runner - Run end-to-end tests and generate reports")]
struct Args {
    /// Test scenario to run
    #[arg(short, long, value_enum, default_value = "e2e-full")]
    scenario: ScenarioType,

    /// Maximum wait time for pool detection (seconds) - only for e2e-full
    #[arg(long, default_value = "300")]
    max_wait: u64,

    /// Maximum number of pools to process - only for e2e-full and scenario-b
    #[arg(long, default_value = "5")]
    max_pools: usize,

    /// Duration for scenario B (seconds)
    #[arg(long, default_value = "60")]
    duration: u64,

    /// Output report file path
    #[arg(short, long, default_value = "docs/testing/E2E_Results.md")]
    output: PathBuf,

    /// Append to existing report instead of overwriting
    #[arg(short, long)]
    append: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose {
        "ghost_e2e=debug,seer=debug,trigger=debug"
    } else {
        "ghost_e2e=info,seer=info,trigger=info"
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("===========================================");
    info!("   Ghost E2E Test Runner");
    info!("===========================================");
    info!("Scenario: {:?}", args.scenario);
    info!("Output: {}", args.output.display());
    info!("Append mode: {}", args.append);
    info!("===========================================");

    // Load configuration
    info!("Loading configuration from environment...");
    let config = E2EConfig::from_env().context("Failed to load configuration")?;
    config
        .validate()
        .context("Configuration validation failed")?;

    info!("Configuration loaded successfully");
    info!("  RPC URL: {}", config.rpc_url);
    info!("  WebSocket URL: {}", config.websocket_url);
    info!("  Mode: Zero-Cost (Direct Pump.fun AMM)");

    // Create metrics
    let metrics = Arc::new(E2EMetrics::new());

    // Run the selected scenario
    info!("");
    info!("===========================================");
    info!("Starting Test Scenario");
    info!("===========================================");

    let result = match args.scenario {
        ScenarioType::A => {
            info!("Running Scenario A: Single Synthetic Pool");
            let scenario = ScenarioA::new(config.clone());
            scenario.run(&config, Arc::clone(&metrics)).await?
        }
        ScenarioType::B => {
            info!("Running Scenario B: Burst Test");
            let scenario = ScenarioB::new(config.clone(), args.max_pools, args.duration);
            scenario.run(&config, Arc::clone(&metrics)).await?
        }
        ScenarioType::E2eFull => {
            info!("Running Scenario E2E Full: Real Yellowstone Detection");
            let scenario = ScenarioE2EFull::new(config.clone(), args.max_wait, args.max_pools);
            scenario.run(&config, Arc::clone(&metrics)).await?
        }
    };

    // Print summary to console
    info!("");
    result.print_summary();

    // Generate and save report
    info!("===========================================");
    info!("Generating Test Report");
    info!("===========================================");

    let report = generate_report(&result, &config, &args)?;
    save_report(&report, &args.output, args.append).await?;

    info!("Report saved to: {}", args.output.display());
    info!("");

    if result.passed {
        info!("===========================================");
        info!("✓ TEST PASSED - All metrics met targets");
        info!("===========================================");
        Ok(())
    } else {
        error!("===========================================");
        error!("✗ TEST FAILED - Some metrics below targets");
        error!("===========================================");
        std::process::exit(1);
    }
}

/// Generate markdown report from test results
fn generate_report(result: &ScenarioResult, config: &E2EConfig, args: &Args) -> Result<String> {
    let now = chrono::Utc::now();
    let date_str = now.format("%Y-%m-%d %H:%M:%S UTC");

    let mut report = String::new();

    // Header
    report.push_str("---\n\n");
    report.push_str(&format!("## Test Run: {}\n\n", date_str));
    report.push_str(&format!("**Scenario**: {}\n\n", result.name));
    report.push_str(&format!(
        "**Status**: {}\n\n",
        if result.passed {
            "✓ PASSED"
        } else {
            "✗ FAILED"
        }
    ));

    // Configuration
    report.push_str("### Configuration\n\n");
    report.push_str("| Parameter | Value |\n");
    report.push_str("|-----------|-------|\n");
    report.push_str(&format!("| RPC URL | `{}` |\n", config.rpc_url));
    report.push_str(&format!("| WebSocket URL | `{}` |\n", config.websocket_url));
    report.push_str("| Mode | Zero-Cost (Direct AMM) |\n");
    report.push_str(&format!(
        "| Pump.fun Enabled | {} |\n",
        config.seer.enable_pumpfun
    ));
    report.push_str(&format!(
        "| Bonk.fun Enabled | {} |\n",
        config.seer.enable_bonkfun
    ));
    report.push_str(&format!(
        "| Oracle Min Score | {} |\n",
        config.oracle.min_score_threshold
    ));
    report.push_str(&format!(
        "| Max Position Size | {} lamports |\n",
        config.features.max_position_size_lamports
    ));
    report.push_str(&format!(
        "| Max Slippage | {:.2}% |\n",
        config.features.max_slippage * 100.0
    ));
    report.push_str(&format!(
        "| Redundancy Factor | N+{} |\n",
        config.trigger.redundancy_factor
    ));
    report.push_str(&format!(
        "| Max Span Slots | {} |\n",
        config.trigger.max_span_slots
    ));
    report.push_str(&format!(
        "| Jito Enabled | {} |\n",
        config.trigger.enable_jito
    ));
    report.push_str(&format!("| Dry Run | {} |\n", config.trigger.dry_run));

    match args.scenario {
        ScenarioType::E2eFull => {
            report.push_str(&format!("| Max Wait Time | {} seconds |\n", args.max_wait));
            report.push_str(&format!("| Max Pools | {} |\n", args.max_pools));
        }
        ScenarioType::B => {
            report.push_str(&format!("| Num Pools | {} |\n", args.max_pools));
            report.push_str(&format!("| Duration | {} seconds |\n", args.duration));
        }
        _ => {}
    }
    report.push_str("\n");

    // Metrics
    report.push_str("### Metrics\n\n");
    report.push_str("| Metric | Target | Actual | Status |\n");
    report.push_str("|--------|--------|--------|--------|\n");
    report.push_str(&format!(
        "| **Land Rate** | ≥ {:.1}% | {:.2}% | {} |\n",
        config.metrics.target_land_rate,
        result.land_rate,
        if result.land_rate >= config.metrics.target_land_rate {
            "✓"
        } else {
            "✗"
        }
    ));
    report.push_str(&format!(
        "| **Inclusion Rate** | ≥ {:.1}% | {:.2}% | {} |\n",
        config.metrics.target_inclusion_rate,
        result.inclusion_rate,
        if result.inclusion_rate >= config.metrics.target_inclusion_rate {
            "✓"
        } else {
            "✗"
        }
    ));
    report.push_str("\n");

    // Latency breakdown
    report.push_str("### Latency Breakdown\n\n");
    report.push_str("| Component | Latency (ms) | Notes |\n");
    report.push_str("|-----------|--------------|-------|\n");

    if let Some(latency) = result.avg_latencies.oracle_scoring {
        report.push_str(&format!(
            "| Oracle Scoring | {:.2} ms | Time to score candidate |\n",
            latency
        ));
    }
    if let Some(latency) = result.avg_latencies.trigger_send {
        report.push_str(&format!(
            "| Trigger Send | {:.2} ms | Time to construct and send TX |\n",
            latency
        ));
    }
    if let Some(latency) = result.avg_latencies.trigger_confirm {
        report.push_str(&format!(
            "| Trigger Confirm | {:.2} ms | Time from send to confirmation |\n",
            latency
        ));
    }
    if let Some(latency) = result.avg_latencies.e2e_total {
        report.push_str(&format!(
            "| **E2E Total** | **{:.2} ms** | Detection to confirmation |\n",
            latency
        ));
    }
    report.push_str("\n");

    // Observations
    if !result.observations.is_empty() {
        report.push_str("### Observations\n\n");
        for obs in &result.observations {
            report.push_str(&format!("- {}\n", obs));
        }
        report.push_str("\n");
    }

    // Conclusion
    report.push_str("### Conclusion\n\n");
    if result.passed {
        report.push_str("✓ **Test PASSED** - All SLA targets were met. The pipeline successfully detected pools, scored candidates, generated swap plans, and executed transactions with acceptable latency.\n\n");
    } else {
        report.push_str("✗ **Test FAILED** - Some SLA targets were not met. Review the metrics above for details on which components need improvement.\n\n");

        if result.land_rate < config.metrics.target_land_rate {
            report.push_str(&format!(
                "- **Land Rate** ({:.2}%) is below target ({:.1}%). The Seer component may need tuning for better pool detection and parsing.\n",
                result.land_rate, config.metrics.target_land_rate
            ));
        }

        if result.inclusion_rate < config.metrics.target_inclusion_rate {
            report.push_str(&format!(
                "- **Inclusion Rate** ({:.2}%) is below target ({:.1}%). The Trigger component may need higher redundancy or better leader selection.\n",
                result.inclusion_rate, config.metrics.target_inclusion_rate
            ));
        }
        report.push_str("\n");
    }

    report.push_str("---\n\n");

    Ok(report)
}

/// Save report to file
async fn save_report(report: &str, path: &PathBuf, append: bool) -> Result<()> {
    use tokio::fs;
    use tokio::io::AsyncWriteExt;

    // Create parent directories if they don't exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create report directory")?;
    }

    if append {
        // Append to existing file
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .context("Failed to open report file for appending")?;

        file.write_all(report.as_bytes())
            .await
            .context("Failed to write report")?;
    } else {
        // Overwrite file
        fs::write(path, report)
            .await
            .context("Failed to write report file")?;
    }

    Ok(())
}
