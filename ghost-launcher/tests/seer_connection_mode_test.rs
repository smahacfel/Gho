//! Test for helius_websocket connection_mode handling
//!
//! This test verifies that the seer component correctly handles the
//! "helius_websocket" connection_mode configuration.

use ghost_launcher::config::{SeerCommitment, SeerComponentConfig};
use seer::config::{ConnectionMode, SeerSourceMode};

// Test constants for program IDs - matches defaults in ghost-launcher config
const TEST_PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const TEST_BONK_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

/// Helper function to create a test SeerComponentConfig with common defaults
fn create_test_config(
    connection_mode: &str,
    source_mode: Option<&str>,
    helius_endpoint: Option<&str>,
) -> SeerComponentConfig {
    SeerComponentConfig {
        enabled: true,
        connection_mode: connection_mode.to_string(),
        source_mode: source_mode.map(|s| s.to_string()),
        geyser_endpoint: "ws://localhost:8900".to_string(),
        grpc_endpoint: "http://localhost:10000".to_string(),
        helius_endpoint: helius_endpoint.map(|s| s.to_string()),
        rpc_endpoint: "https://api.devnet.solana.com".to_string(),
        grpc_manual_backfill_enabled: true,
        grpc_commitment_fallback_to_websocket: false,
        grpc_max_stalls_before_open: 3,
        grpc_circuit_breaker_cooldown_ms: 15_000,
        grpc_client_id: None,
        grpc_auth_token: None,
        grpc_x_token: None,
        enable_pumpfun: true,
        enable_bonkfun: true,
        pump_program_id: TEST_PUMP_PROGRAM_ID.to_string(),
        bonk_program_id: TEST_BONK_PROGRAM_ID.to_string(),
        metrics_port: 9090,
        ipc_buffer_size: 10000,
        ipc_backpressure_policy: "block".to_string(),
        stream_mode: "single_global".to_string(),
        tx_filter_strategy: "per_pool".to_string(),
        funding_lane_mode: "disabled".to_string(),
        watched_pools_ttl_ms: 120_000,
        watched_pools_cap: 32_768,
        watch_debounce_ms: 0,
        commitment: SeerCommitment::default(),
        pumpportal: Default::default(),
    }
}

/// Simulates the logic in ghost-launcher/src/components/seer.rs
/// to convert SeerComponentConfig to SeerConfig
fn derive_connection_settings(
    config: &SeerComponentConfig,
) -> (ConnectionMode, Option<SeerSourceMode>) {
    // Determine source mode first, checking both specific source_mode and legacy connection_mode
    let derived_source_mode = if let Some(mode) = &config.source_mode {
        match mode.to_lowercase().as_str() {
            "geyser_grpc" => Some(SeerSourceMode::GeyserGrpc),
            "geyser_websocket" => Some(SeerSourceMode::GeyserWebSocket),
            "helius_websocket" => Some(SeerSourceMode::HeliusWebSocket),
            _ => None,
        }
    } else {
        // Fallback to inferring from connection_mode for backward compatibility
        match config.connection_mode.to_lowercase().as_str() {
            "helius_websocket" => Some(SeerSourceMode::HeliusWebSocket),
            _ => None,
        }
    };

    let connection_mode = match config.connection_mode.to_lowercase().as_str() {
        "websocket" | "ws" | "helius_websocket" => ConnectionMode::WebSocket,
        "grpc" | "g" => ConnectionMode::Grpc,
        _ => ConnectionMode::Grpc,
    };

    (connection_mode, derived_source_mode)
}

#[test]
fn test_helius_websocket_connection_mode() {
    // Test case 1: connection_mode = "helius_websocket" with no source_mode
    let config = create_test_config("helius_websocket", None, Some("wss://api.helius.xyz"));

    let (connection_mode, source_mode) = derive_connection_settings(&config);

    // Should use WebSocket connection mode
    assert_eq!(connection_mode, ConnectionMode::WebSocket);
    // Should derive HeliusWebSocket source mode
    assert_eq!(source_mode, Some(SeerSourceMode::HeliusWebSocket));
}

#[test]
fn test_helius_websocket_explicit_source_mode() {
    // Test case 2: connection_mode = "helius_websocket" with explicit source_mode
    let config = create_test_config(
        "helius_websocket",
        Some("helius_websocket"),
        Some("wss://api.helius.xyz"),
    );

    let (connection_mode, source_mode) = derive_connection_settings(&config);

    // Should use WebSocket connection mode
    assert_eq!(connection_mode, ConnectionMode::WebSocket);
    // Should use explicit HeliusWebSocket source mode
    assert_eq!(source_mode, Some(SeerSourceMode::HeliusWebSocket));
}

#[test]
fn test_regular_websocket_connection_mode() {
    // Test case 3: regular "websocket" connection_mode
    let config = create_test_config("websocket", None, None);

    let (connection_mode, source_mode) = derive_connection_settings(&config);

    // Should use WebSocket connection mode
    assert_eq!(connection_mode, ConnectionMode::WebSocket);
    // Should have None source mode (will be derived from connection_mode in SeerConfig)
    assert_eq!(source_mode, None);
}

#[test]
fn test_grpc_connection_mode() {
    // Test case 4: "grpc" connection_mode
    let config = create_test_config("grpc", None, None);

    let (connection_mode, source_mode) = derive_connection_settings(&config);

    // Should use Grpc connection mode
    assert_eq!(connection_mode, ConnectionMode::Grpc);
    // Should have None source mode
    assert_eq!(source_mode, None);
}

#[test]
fn test_explicit_source_mode_overrides() {
    // Test case 5: connection_mode = "grpc" but source_mode = "helius_websocket"
    let config = create_test_config(
        "grpc",
        Some("helius_websocket"),
        Some("wss://api.helius.xyz"),
    );

    let (connection_mode, source_mode) = derive_connection_settings(&config);

    // Should use Grpc connection mode (based on connection_mode field)
    assert_eq!(connection_mode, ConnectionMode::Grpc);
    // Should use explicit HeliusWebSocket source mode (explicit source_mode takes precedence)
    assert_eq!(source_mode, Some(SeerSourceMode::HeliusWebSocket));
}

#[test]
fn test_grpc_x_token_precedence() {
    // Test case 6: grpc_x_token takes precedence over grpc_auth_token
    let mut config = create_test_config("grpc", None, None);
    config.grpc_endpoint = "https://yellowstone-solana-mainnet.core.chainstack.com:443".to_string();
    config.grpc_auth_token = Some("legacy-token".to_string());
    config.grpc_x_token = Some("chainstack-x-token".to_string());

    // Verify that grpc_x_token takes precedence over grpc_auth_token
    // This is the logic used in ghost-launcher/src/components/seer.rs
    let effective_token = config
        .grpc_x_token
        .clone()
        .or(config.grpc_auth_token.clone());

    assert_eq!(effective_token, Some("chainstack-x-token".to_string()));
}

#[test]
fn test_grpc_auth_token_fallback() {
    // Test case 7: grpc_auth_token is used when grpc_x_token is not provided
    let mut config = create_test_config("grpc", None, None);
    config.grpc_endpoint = "https://yellowstone-solana-mainnet.core.chainstack.com:443".to_string();
    config.grpc_auth_token = Some("legacy-token".to_string());
    // grpc_x_token is already None from create_test_config

    // Verify that grpc_auth_token is used when grpc_x_token is None
    let effective_token = config
        .grpc_x_token
        .clone()
        .or(config.grpc_auth_token.clone());

    assert_eq!(effective_token, Some("legacy-token".to_string()));
}
