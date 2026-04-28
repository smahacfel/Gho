//! Enhanced demo of the Guardian module with Supervisor Loop
//!
//! This example demonstrates the TASK 1.2 implementation with:
//! - Parallel task spawning (Chaos Engine, Gene Mapper)
//! - Slot updates with QASS evaluation
//! - Internal task result handling
//! - External Hunter result handling
//! - Timeout scenarios

use ghost_brain::guardian::{run_watchdog, WatchdogConfig, WatchdogDecision, WatchdogSignal};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with more detailed output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("=== Guardian Module Enhanced Demo (TASK 1.2) ===\n");

    // Scenario 1: Successful execution with all checks passing
    println!("--- Scenario 1: All Checks Pass ---");
    demo_success_scenario().await?;

    println!("\n--- Scenario 2: QASS Score Too Low ---");
    demo_qass_abort_scenario().await?;

    println!("\n--- Scenario 3: High Internal Risk ---");
    demo_internal_risk_scenario().await?;

    println!("\n--- Scenario 4: Timeout ---");
    demo_timeout_scenario().await?;

    println!("\n=== Demo Complete ===");

    Ok(())
}

async fn demo_success_scenario() -> anyhow::Result<()> {
    let mut config = WatchdogConfig::default();
    config.enable_parallel_tasks = true;
    println!("Config: {:?}\n", config);

    let (tx, rx) = mpsc::channel(10);

    // Spawn watchdog task
    let watchdog_task = tokio::spawn(async move {
        // Updated signature: run_watchdog(config, rx, telemetry, pool, scenario, slot_history)
        run_watchdog(config, rx, None, None, None, None).await
    });

    // Give parallel tasks time to spawn
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    // Send slot update with good QASS score
    println!("Sending SlotUpdate with QASS score 0.85...");
    tx.send(WatchdogSignal::SlotUpdate {
        slot: 123456,
        timestamp_ms: 1000,
        qass_score: Some(0.85),
    })
    .await?;

    // Wait a bit
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Send external success
    println!("Sending ExternalResult (success)...");
    tx.send(WatchdogSignal::ExternalResult {
        success: true,
        data: "Hunter analysis complete - token safe".to_string(),
    })
    .await?;

    // Wait for watchdog to complete
    let decision = watchdog_task.await??;

    println!("\nWatchdog decision: {:?}", decision);
    match decision {
        WatchdogDecision::Proceed => println!("✅ All checks passed - PROCEEDING with trade"),
        WatchdogDecision::Abort => println!("❌ Checks failed - ABORTING trade"),
        WatchdogDecision::Timeout => println!("⏱️  Analysis timed out"),
    }

    Ok(())
}

async fn demo_qass_abort_scenario() -> anyhow::Result<()> {
    let mut config = WatchdogConfig::default();
    config.min_qass_score = 0.7;
    config.enable_parallel_tasks = false; // Disable for simpler test
    println!("Config: min_qass_score = {}\n", config.min_qass_score);

    let (tx, rx) = mpsc::channel(10);

    let watchdog_task =
        tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

    // Send slot update with low QASS score
    println!("Sending SlotUpdate with low QASS score 0.3...");
    tx.send(WatchdogSignal::SlotUpdate {
        slot: 123457,
        timestamp_ms: 2000,
        qass_score: Some(0.3), // Below threshold!
    })
    .await?;

    let decision = watchdog_task.await??;

    println!("\nWatchdog decision: {:?}", decision);
    match decision {
        WatchdogDecision::Proceed => println!("✅ Proceeding"),
        WatchdogDecision::Abort => println!("❌ ABORTED due to low QASS score"),
        WatchdogDecision::Timeout => println!("⏱️  Timeout"),
    }

    Ok(())
}

async fn demo_internal_risk_scenario() -> anyhow::Result<()> {
    let mut config = WatchdogConfig::default();
    config.max_internal_risk_score = 0.5;
    config.enable_parallel_tasks = false; // Manual control
    println!(
        "Config: max_internal_risk_score = {}\n",
        config.max_internal_risk_score
    );

    let (tx, rx) = mpsc::channel(10);

    let watchdog_task =
        tokio::spawn(async move { run_watchdog(config, rx, None, None, None, None).await });

    // Simulate internal task reporting high risk
    println!("Sending InternalTaskResult with high risk score 0.9...");
    tx.send(WatchdogSignal::InternalTaskResult {
        task_name: "Chaos Engine".to_string(),
        risk_detected: true,
        risk_score: 0.9, // Very high risk!
        details: "Monte Carlo simulation shows 90% probability of rug pull".to_string(),
    })
    .await?;

    let decision = watchdog_task.await??;

    println!("\nWatchdog decision: {:?}", decision);
    match decision {
        WatchdogDecision::Proceed => println!("✅ Proceeding"),
        WatchdogDecision::Abort => println!("❌ ABORTED due to high internal risk"),
        WatchdogDecision::Timeout => println!("⏱️  Timeout"),
    }

    Ok(())
}

async fn demo_timeout_scenario() -> anyhow::Result<()> {
    let mut config = WatchdogConfig::default();
    config.max_void_duration_ms = 200; // Short timeout
    config.enable_parallel_tasks = false; // Disable for clean test
    println!(
        "Config: max_void_duration_ms = {}ms\n",
        config.max_void_duration_ms
    );

    let (_tx, rx) = mpsc::channel(10);

    println!("Waiting for timeout (no signals sent)...");
    let decision = run_watchdog(config, rx, None, None, None, None).await?;

    println!("\nWatchdog decision: {:?}", decision);
    match decision {
        WatchdogDecision::Proceed => println!("✅ Proceeding"),
        WatchdogDecision::Abort => println!("❌ Aborted"),
        WatchdogDecision::Timeout => println!("⏱️  TIMEOUT - external APIs too slow"),
    }

    Ok(())
}
