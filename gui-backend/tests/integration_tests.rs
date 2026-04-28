//! Integration tests for GUI backend API

use gui_backend::{GuiBackend, GuiBackendConfig, Settings, SystemMode};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_backend_state_management() {
    // Create backend
    let config = GuiBackendConfig {
        port: 8801, // Use different port to avoid conflicts
        enabled: true,
        bind_address: "127.0.0.1".to_string(),
    };

    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Test initial state
    assert!(state.is_running());
    assert!(!state.is_paused());
    assert!(!state.is_stopped());

    // Test mode transitions
    state.set_mode(SystemMode::Paused);
    assert!(state.is_paused());
    assert_eq!(state.get_mode(), SystemMode::Paused);

    state.set_mode(SystemMode::Running);
    assert!(state.is_running());

    state.set_mode(SystemMode::Stopped);
    assert!(state.is_stopped());
}

#[tokio::test]
async fn test_settings_update() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Get initial settings
    let initial = state.get_settings();
    assert_eq!(initial.position_size_lamports, 100_000_000);

    // Update settings
    let new_settings = Settings {
        position_size_lamports: 200_000_000,
        jito_tip_lamports: 20_000,
        max_slippage: 0.02,
        enable_jito: true,
        auto_jito_tip: false,
    };

    state.update_settings(new_settings.clone());

    // Verify update
    let updated = state.get_settings();
    assert_eq!(updated.position_size_lamports, 200_000_000);
    assert_eq!(updated.jito_tip_lamports, 20_000);
    assert_eq!(updated.max_slippage, 0.02);
    assert!(updated.enable_jito);
}

#[tokio::test]
async fn test_transaction_stats_update() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Initial stats should be zero
    let initial_status = state.get_status();
    assert_eq!(initial_status.transactions_sent, 0);
    assert_eq!(initial_status.transactions_confirmed, 0);

    // Update stats
    state.update_transaction_stats(100, 95);

    // Verify update
    let updated_status = state.get_status();
    assert_eq!(updated_status.transactions_sent, 100);
    assert_eq!(updated_status.transactions_confirmed, 95);
}

#[tokio::test]
async fn test_broadcast_channel() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = backend.state();

    // Subscribe to updates
    let mut rx = state.subscribe();

    // Spawn task to listen for updates
    let update_task = tokio::spawn(async move {
        match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Ok(update)) => Some(update),
            _ => None,
        }
    });

    // Trigger an update
    sleep(Duration::from_millis(100)).await;
    state.set_mode(SystemMode::Paused);

    // Wait for update
    let result = update_task.await.unwrap();
    assert!(result.is_some());
}

#[tokio::test]
async fn test_concurrent_access() {
    let config = GuiBackendConfig::default();
    let backend = GuiBackend::new(config);
    let state = Arc::new(backend.state());

    // Spawn multiple tasks that access state concurrently
    let mut handles = vec![];

    for i in 0..10 {
        let state_clone = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            let settings = Settings {
                position_size_lamports: 100_000_000 + (i * 10_000_000),
                jito_tip_lamports: 10_000,
                max_slippage: 0.01,
                enable_jito: false,
                auto_jito_tip: true,
            };
            state_clone.update_settings(settings);
            sleep(Duration::from_millis(10)).await;
            state_clone.get_settings()
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        let result = handle.await;
        assert!(result.is_ok());
    }

    // Verify state is still consistent
    let final_settings = state.get_settings();
    assert!(final_settings.position_size_lamports >= 100_000_000);
}
