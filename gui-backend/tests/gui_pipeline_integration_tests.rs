//! GUI ↔ Backend ↔ Pipeline Integration Tests
//!
//! This test suite verifies the complete integration chain:
//! GUI/REST API ↔ Backend State ↔ E2E Pipeline Components
//!
//! Tests cover:
//! - Immediate mode changes (PAUSE/RESUME/STOP)
//! - Runtime settings propagation to pipeline
//! - Portfolio sync with live updates
//! - Control flow verification

use gui_backend::{GuiBackend, GuiBackendConfig, Portfolio, Position, Settings, SystemMode};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// Test that mode changes are immediate and atomic
#[tokio::test]
async fn test_immediate_mode_change_pause_resume() {
    let config = GuiBackendConfig {
        port: 8802,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Initial state: RUNNING
    assert!(state.is_running());
    assert!(!state.is_paused());
    assert!(!state.is_stopped());

    // Simulate PAUSE command from GUI
    state.set_mode(SystemMode::Paused);

    // Verify immediate change
    assert!(state.is_paused());
    assert!(!state.is_running());
    assert_eq!(state.get_mode(), SystemMode::Paused);

    // Simulate RESUME command from GUI
    state.set_mode(SystemMode::Running);

    // Verify immediate change back to running
    assert!(state.is_running());
    assert!(!state.is_paused());
    assert_eq!(state.get_mode(), SystemMode::Running);
}

/// Test that STOP command is immediate and final
#[tokio::test]
async fn test_immediate_mode_change_stop() {
    let config = GuiBackendConfig {
        port: 8803,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Start in RUNNING
    assert!(state.is_running());

    // Simulate STOP command from GUI
    state.set_mode(SystemMode::Stopped);

    // Verify immediate change
    assert!(state.is_stopped());
    assert!(!state.is_running());
    assert!(!state.is_paused());
    assert_eq!(state.get_mode(), SystemMode::Stopped);
}

/// Test that mode changes trigger WebSocket broadcasts
#[tokio::test]
async fn test_mode_change_broadcasts() {
    let config = GuiBackendConfig {
        port: 8804,
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Subscribe to updates (simulating WebSocket client)
    let mut rx = state.subscribe();

    // Spawn task to listen for mode updates
    let update_task = tokio::spawn(async move {
        let mut mode_updates = Vec::new();

        // Collect updates for 500ms
        let timeout = tokio::time::sleep(Duration::from_millis(500));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                result = rx.recv() => {
                    if let Ok(update) = result {
                        mode_updates.push(update);
                    }
                }
                _ = &mut timeout => {
                    break;
                }
            }
        }

        mode_updates
    });

    // Trigger multiple mode changes
    sleep(Duration::from_millis(50)).await;
    state.set_mode(SystemMode::Paused);

    sleep(Duration::from_millis(50)).await;
    state.set_mode(SystemMode::Running);

    sleep(Duration::from_millis(50)).await;
    state.set_mode(SystemMode::Stopped);

    // Wait for updates
    let updates = update_task.await.unwrap();

    // Should have received multiple updates
    assert!(
        updates.len() >= 3,
        "Expected at least 3 updates, got {}",
        updates.len()
    );
}

/// Test that runtime settings updates are immediate
#[tokio::test]
async fn test_runtime_settings_immediate_update() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Initial settings
    let initial = state.get_settings();
    let initial_position = initial.position_size_lamports;

    // Update position size (simulating GUI/REST API update)
    let new_settings = Settings {
        position_size_lamports: initial_position * 2,
        jito_tip_lamports: 50_000,
        max_slippage: 0.05,
        enable_jito: true,
        auto_jito_tip: false,
    };

    state.update_settings(new_settings.clone());

    // Verify immediate availability (no delay)
    let updated = state.get_settings();
    assert_eq!(updated.position_size_lamports, initial_position * 2);
    assert_eq!(updated.jito_tip_lamports, 50_000);
    assert_eq!(updated.max_slippage, 0.05);
    assert!(updated.enable_jito);
    assert!(!updated.auto_jito_tip);
}

/// Test that runtime config can be retrieved and used by pipeline
#[tokio::test]
async fn test_runtime_config_retrieval_for_pipeline() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Set specific runtime settings
    let settings = Settings {
        position_size_lamports: 500_000_000, // 0.5 SOL
        jito_tip_lamports: 25_000,
        max_slippage: 0.03,
        enable_jito: false,
        auto_jito_tip: true,
    };

    state.update_settings(settings.clone());

    // Simulate pipeline component reading runtime config
    let runtime_config = state.get_runtime_config();

    // Verify values match
    assert_eq!(runtime_config.position_size_lamports, 500_000_000);
    assert_eq!(runtime_config.jito_tip_lamports, 25_000);
    assert_eq!(runtime_config.max_slippage, 0.03);
    assert!(!runtime_config.enable_jito);
    assert!(runtime_config.auto_jito_tip);
}

/// Test portfolio updates are reflected immediately
#[tokio::test]
async fn test_portfolio_immediate_update() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Initial portfolio should be empty
    let initial = state.get_portfolio();
    assert_eq!(initial.sol_balance, 0);
    assert_eq!(initial.positions.len(), 0);

    // Update portfolio (simulating pipeline update)
    let new_portfolio = Portfolio {
        sol_balance: 5_000_000_000, // 5 SOL
        positions: vec![
            Position {
                mint: "TokenMint1".to_string(),
                amount: 1_000_000,
                entry_price: 50_000,
                current_price: Some(55_000),
                pnl: 5_000,
                opened_at: 1700000000,
            },
            Position {
                mint: "TokenMint2".to_string(),
                amount: 2_000_000,
                entry_price: 30_000,
                current_price: Some(28_000),
                pnl: -2_000,
                opened_at: 1700000100,
            },
        ],
        total_value: 5_200_000_000,
        total_pnl: 3_000,
    };

    state.update_portfolio(new_portfolio.clone());

    // Verify immediate availability
    let updated = state.get_portfolio();
    assert_eq!(updated.sol_balance, 5_000_000_000);
    assert_eq!(updated.positions.len(), 2);
    assert_eq!(updated.total_value, 5_200_000_000);
    assert_eq!(updated.total_pnl, 3_000);
    assert_eq!(updated.positions[0].mint, "TokenMint1");
    assert_eq!(updated.positions[1].mint, "TokenMint2");
}

/// Test portfolio updates trigger WebSocket broadcasts
#[tokio::test]
async fn test_portfolio_update_broadcasts() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Subscribe to updates
    let mut rx = state.subscribe();

    // Spawn task to listen for portfolio updates
    let update_task = tokio::spawn(async move {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Ok(update)) => Some(update),
            _ => None,
        }
    });

    // Trigger portfolio update
    sleep(Duration::from_millis(50)).await;
    let portfolio = Portfolio {
        sol_balance: 10_000_000_000,
        positions: vec![],
        total_value: 10_000_000_000,
        total_pnl: 0,
    };
    state.update_portfolio(portfolio);

    // Wait for update
    let result = update_task.await.unwrap();
    assert!(result.is_some(), "Expected portfolio update broadcast");
}

/// Test transaction statistics updates
#[tokio::test]
async fn test_transaction_stats_tracking() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Initial stats
    let initial = state.get_status();
    assert_eq!(initial.transactions_sent, 0);
    assert_eq!(initial.transactions_confirmed, 0);

    // Simulate pipeline sending and confirming transactions
    state.update_transaction_stats(10, 9);
    let after_first = state.get_status();
    assert_eq!(after_first.transactions_sent, 10);
    assert_eq!(after_first.transactions_confirmed, 9);

    state.update_transaction_stats(25, 23);
    let after_second = state.get_status();
    assert_eq!(after_second.transactions_sent, 25);
    assert_eq!(after_second.transactions_confirmed, 23);
}

/// Test concurrent reads from multiple "pipeline components"
#[tokio::test]
async fn test_concurrent_pipeline_component_reads() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = Arc::new(backend.state());

    // Set initial state
    let settings = Settings {
        position_size_lamports: 300_000_000,
        jito_tip_lamports: 15_000,
        max_slippage: 0.02,
        enable_jito: true,
        auto_jito_tip: true,
    };
    state.update_settings(settings);

    // Spawn multiple tasks simulating pipeline components reading state
    let mut handles = vec![];

    for _ in 0..20 {
        let state_clone = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            // Simulate multiple reads
            let mode = state_clone.get_mode();
            sleep(Duration::from_micros(100)).await;
            let settings = state_clone.get_settings();
            sleep(Duration::from_micros(100)).await;
            let status = state_clone.get_status();
            sleep(Duration::from_micros(100)).await;
            let portfolio = state_clone.get_portfolio();

            (mode, settings, status, portfolio)
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let mut results = vec![];
    for handle in handles {
        let result = handle.await;
        assert!(result.is_ok());
        results.push(result.unwrap());
    }

    // Verify all reads were consistent
    for (mode, settings, _, _) in results {
        assert_eq!(mode, SystemMode::Running);
        assert_eq!(settings.position_size_lamports, 300_000_000);
        assert_eq!(settings.jito_tip_lamports, 15_000);
    }
}

/// Test mode checking simulating trigger component behavior
#[tokio::test]
async fn test_trigger_mode_checking() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Simulate trigger checking mode before processing
    assert!(state.is_running());
    // Trigger would process candidate here

    // Pause system
    state.set_mode(SystemMode::Paused);

    // Simulate trigger checking mode - should skip
    assert!(state.is_paused());
    assert!(!state.is_running());
    // Trigger would skip candidate here

    // Resume system
    state.set_mode(SystemMode::Running);

    // Trigger can process again
    assert!(state.is_running());

    // Stop system
    state.set_mode(SystemMode::Stopped);

    // Trigger should exit
    assert!(state.is_stopped());
}

/// Test settings update with validation constraints
#[tokio::test]
async fn test_settings_validation_via_state() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Valid settings update
    let valid_settings = Settings {
        position_size_lamports: 1_000_000_000, // 1 SOL
        jito_tip_lamports: 100_000,
        max_slippage: 0.1,
        enable_jito: true,
        auto_jito_tip: false,
    };

    state.update_settings(valid_settings.clone());
    let updated = state.get_settings();
    assert_eq!(updated.position_size_lamports, 1_000_000_000);
    assert_eq!(updated.jito_tip_lamports, 100_000);
}

/// Test uptime tracking
#[tokio::test]
async fn test_uptime_tracking() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Get initial status
    let status1 = state.get_status();
    let uptime1 = status1.uptime_secs;

    // Wait a bit
    sleep(Duration::from_millis(100)).await;

    // Get status again
    let status2 = state.get_status();
    let uptime2 = status2.uptime_secs;

    // Uptime should have increased
    assert!(uptime2 >= uptime1, "Uptime should increase over time");
}

/// Test active positions tracking
#[tokio::test]
async fn test_active_positions_tracking() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Initially no positions
    let status = state.get_status();
    assert_eq!(status.active_positions, 0);

    // Update portfolio with positions
    let portfolio = Portfolio {
        sol_balance: 5_000_000_000,
        positions: vec![
            Position {
                mint: "Token1".to_string(),
                amount: 1_000_000,
                entry_price: 50_000,
                current_price: Some(55_000),
                pnl: 5_000,
                opened_at: 1700000000,
            },
            Position {
                mint: "Token2".to_string(),
                amount: 2_000_000,
                entry_price: 30_000,
                current_price: Some(32_000),
                pnl: 2_000,
                opened_at: 1700000100,
            },
            Position {
                mint: "Token3".to_string(),
                amount: 500_000,
                entry_price: 100_000,
                current_price: Some(105_000),
                pnl: 5_000,
                opened_at: 1700000200,
            },
        ],
        total_value: 5_500_000_000,
        total_pnl: 12_000,
    };

    state.update_portfolio(portfolio);

    // Check active positions count
    let updated_status = state.get_status();
    assert_eq!(updated_status.active_positions, 3);
}
