//! Example demonstrating the Telemetry Recorder with Watchdog integration
//!
//! This example shows how to use the telemetry system to log Watchdog decisions
//! and internal task results to JSONL format.
//!
//! Run with:
//! ```bash
//! cargo run --example telemetry_demo
//! ```
//!
//! Output will be written to `logs/telemetry_demo.jsonl`

use ghost_brain::guardian::types::{WatchdogConfig, WatchdogSignal};
use ghost_brain::guardian::watchdog::run_watchdog;
use ghost_brain::telemetry::{TelemetryConfig, TelemetryRecorder};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("ghost_brain=info")
        .init();

    println!("🔬 Telemetry Recorder Demo");
    println!("{}", "=".repeat(60));

    // Create telemetry config
    let telemetry_config = TelemetryConfig {
        log_path: PathBuf::from("logs/telemetry_demo.jsonl"),
        channel_buffer_size: 100,
        enabled: true,
    };

    println!("📝 Telemetry output: {:?}", telemetry_config.log_path);

    // Create telemetry recorder
    let telemetry = TelemetryRecorder::new(telemetry_config.clone()).await?;
    println!("✅ Telemetry recorder initialized");

    // Create watchdog config
    let mut watchdog_config = WatchdogConfig::default();
    watchdog_config.enable_parallel_tasks = true;
    watchdog_config.max_void_duration_ms = 1000;
    watchdog_config.min_qass_score = 0.5;

    println!("\n🐕 Starting Watchdog with telemetry...");

    // Create signal channel
    let (tx, rx) = mpsc::channel(10);

    // Spawn watchdog in background
    let watchdog_handle = tokio::spawn(async move {
        run_watchdog(watchdog_config, rx, Some(telemetry), None, None, None).await
    });

    // Give time for internal tasks to spawn
    sleep(Duration::from_millis(50)).await;

    // Simulate slot update with good QASS score
    println!("📊 Sending slot update (QASS=0.85)...");
    tx.send(WatchdogSignal::SlotUpdate {
        slot: 123456,
        timestamp_ms: 1733414400000,
        qass_score: Some(0.85),
    })
    .await?;

    sleep(Duration::from_millis(50)).await;

    // Simulate external result
    println!("🔍 Sending external result (success)...");
    tx.send(WatchdogSignal::ExternalResult {
        success: true,
        data: "Demo external validation passed".to_string(),
    })
    .await?;

    // Wait for watchdog to complete
    let decision = watchdog_handle.await??;
    println!("\n⚖️  Watchdog decision: {:?}", decision);

    // Give time for async telemetry writes to complete
    sleep(Duration::from_millis(200)).await;

    println!(
        "\n📂 Telemetry log written to: {:?}",
        telemetry_config.log_path
    );
    println!("\n💡 View logs with:");
    println!("   cat logs/telemetry_demo.jsonl | jq '.'");
    println!("\n✨ Demo complete!");

    Ok(())
}
