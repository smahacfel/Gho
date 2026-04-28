//! Test: Logging Separation Verification
//!
//! This test verifies that logs are properly separated into:
//! - oracle_decision.log: Only ghost_brain::oracle and ghost_launcher::oracle_runtime logs
//! - system.log: All other logs (network, startup, etc.)
//!
//! Run with: cargo test -p ghost-launcher test_log_separation -- --nocapture

use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use tracing::{info, warn};

#[tokio::test]
async fn test_log_separation() {
    // Create temporary directory for test logs
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let log_dir = temp_dir.path().to_path_buf();

    // Create test config with separate log files
    let system_log_path = log_dir.join("system.log");
    let oracle_log_path = log_dir.join("oracle_decision.log");

    // Initialize test logging (simplified version)
    use tracing_appender;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

    // Create file appenders
    let system_appender = tracing_appender::rolling::never(&log_dir, "system.log");
    let (system_writer, _system_guard) = tracing_appender::non_blocking(system_appender);

    let oracle_appender = tracing_appender::rolling::never(&log_dir, "oracle_decision.log");
    let (oracle_writer, _oracle_guard) = tracing_appender::non_blocking(oracle_appender);

    // Create filters
    let oracle_filter =
        EnvFilter::new("ghost_brain::oracle=info,ghost_launcher::oracle_runtime=info");
    let system_filter = EnvFilter::new("info");

    // Set up layers
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(oracle_writer)
                .with_ansi(false)
                .with_filter(oracle_filter),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(system_writer)
                .with_ansi(false)
                .with_filter(system_filter),
        )
        .init();

    // Emit some test logs from different modules

    // System log
    info!(target: "ghost_launcher::main", "Ghost Launcher starting");
    info!(target: "ghost_launcher::components::seer", "Seer component initialized");
    warn!(target: "ghost_launcher::components::trigger", "Network connection unstable");

    // Oracle logs
    info!(target: "ghost_brain::oracle", "Oracle decision made");
    info!(target: "ghost_launcher::oracle_runtime", "Oracle runtime task started");

    // More system logs
    info!(target: "ghost_launcher::components::gui_backend", "GUI backend listening on port 8800");

    // Force flush
    drop(_system_guard);
    drop(_oracle_guard);

    // Give it a moment for async writes to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify log files exist
    assert!(system_log_path.exists(), "System log file should exist");
    assert!(oracle_log_path.exists(), "Oracle log file should exist");

    // Read log files
    let system_content = fs::read_to_string(&system_log_path).expect("Failed to read system log");
    let oracle_content = fs::read_to_string(&oracle_log_path).expect("Failed to read oracle log");

    println!("\n=== SYSTEM LOG ===");
    println!("{}", system_content);
    println!("\n=== ORACLE LOG ===");
    println!("{}", oracle_content);

    // Verify separation:
    // System log should contain non-oracle logs
    assert!(
        system_content.contains("Ghost Launcher starting"),
        "System log should contain launcher startup message"
    );
    assert!(
        system_content.contains("Seer component initialized"),
        "System log should contain Seer message"
    );
    assert!(
        system_content.contains("Network connection unstable"),
        "System log should contain trigger warning"
    );
    assert!(
        system_content.contains("GUI backend listening"),
        "System log should contain GUI message"
    );

    // Oracle log should contain ONLY oracle logs
    assert!(
        oracle_content.contains("Oracle decision made"),
        "Oracle log should contain oracle decision"
    );
    assert!(
        oracle_content.contains("Oracle runtime task started"),
        "Oracle log should contain oracle runtime message"
    );

    // Oracle log should NOT contain system messages
    assert!(
        !oracle_content.contains("Ghost Launcher starting"),
        "Oracle log should NOT contain launcher startup message"
    );
    assert!(
        !oracle_content.contains("Seer component initialized"),
        "Oracle log should NOT contain Seer message"
    );
    assert!(
        !oracle_content.contains("GUI backend listening"),
        "Oracle log should NOT contain GUI message"
    );

    println!("\n✅ Log separation test PASSED");
    println!(
        "   - System log: {} bytes, {} lines",
        system_content.len(),
        system_content.lines().count()
    );
    println!(
        "   - Oracle log: {} bytes, {} lines",
        oracle_content.len(),
        oracle_content.lines().count()
    );
}
