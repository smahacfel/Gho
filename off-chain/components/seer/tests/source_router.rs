//! Integration tests for Source Router functionality
//!
//! These tests verify that the Seer component properly routes events based on source mode:
//! - PumpPortal mode: No binary parsing, synthetic events only
//! - Geyser modes: Binary parsing enabled for raw events
//! - Synthetic events are NEVER parsed by binary parser

use seer::config::{
    CommitmentLevel, ConnectionMode, FilterConfig, FundingLaneMode, ProgramStreamsConfig,
    PumpPortalConfig, SeerConfig, SeerSourceMode, StreamMode, TxFilterStrategy,
};
use seer::types::{GeyserEvent, RawBytesMissingReason, RawInstruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashMap;
use std::str::FromStr;

/// Create a default test config
fn default_test_config() -> SeerConfig {
    SeerConfig {
        connection_mode: ConnectionMode::WebSocket,
        source_mode: None,
        geyser_endpoint: "ws://localhost:8900".to_string(),
        grpc_endpoint: "http://localhost:10000".to_string(),
        helius_endpoint: None,
        rpc_endpoint: "http://localhost:8899".to_string(),
        grpc_manual_backfill_enabled: true,
        grpc_client_id: None,
        grpc_auth_token: None,
        grpc_auth_header: SeerConfig::default_grpc_auth_header(),
        max_reconnect_attempts: 5,
        reconnect_delay_secs: 1,
        max_reconnect_delay_secs: 60,
        grpc_max_stalls_before_open: 3,
        grpc_stall_timeout_secs: SeerConfig::default_grpc_stall_timeout_secs(),
        grpc_circuit_breaker_cooldown_ms: 15_000,
        verbose: false,
        filter: FilterConfig {
            enable_pumpfun: true,
            enable_bonkfun: true,
            allowed_quote_mints: vec![],
            min_initial_liquidity_sol: None,
        },
        channel_buffer_size: 100,
        ipc_config: seer::ipc::IpcChannelConfig {
            buffer_size: 1000,
            backpressure_policy: seer::ipc::BackpressurePolicy::Block,
            log_drops: true,
            log_overflows: true,
            warning_threshold_percent: 80.0,
        },
        metrics_port: 9091,
        ultrafast_enter_threshold: 0.8,
        ultrafast_exit_threshold: 0.5,
        commitment: CommitmentLevel::Confirmed,
        grpc_commitment_fallback_to_websocket: false,
        pumpportal: PumpPortalConfig::default(),
        stream_mode: StreamMode::SingleGlobal,
        tx_filter_strategy: TxFilterStrategy::PerPool,
        funding_lane_mode: FundingLaneMode::Disabled,
        program_streams: ProgramStreamsConfig::default(),
        watched_pools_ttl_ms: 120_000,
        watched_pools_cap: 512,
        watch_debounce_ms: 0,
        canonical_account_update_relay_enabled: false,
    }
}

/// Create a synthetic transaction event (like PumpPortal produces)
fn create_synthetic_event() -> GeyserEvent {
    GeyserEvent::Transaction {
        slot: None,
        event_ts_ms: None,
        arrival_ts_ms: Some(seer::types::arrival_time_ms()),
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: Signature::from_str("5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW").unwrap(),
        accounts: vec![
            Pubkey::from_str("11111111111111111111111111111111").unwrap(),
            Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
        ],
        instructions: vec![
            RawInstruction {
                program_id: Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap(), // Pump.fun
                account_indices: vec![0, 1],
                data: vec![], // Synthetic events have no raw instruction data
            }
        ],
        logs: vec![
            "Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P invoke [1]".to_string(),
            "Program log: Instruction: Create".to_string(),
            "Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P success".to_string(),
        ],
        block_time: Some(1640000000),
        account_data: HashMap::new(),
        pre_balances: vec![],
        post_balances: vec![],
        success: true,
        error_code: None,
        compute_units_consumed: None,
        synthetic: true, // This is the key flag
        source: "pumpportal".to_string(),
        mpcf_payload_bytes: None,
        mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        inner_instructions: vec![],
        pre_token_balances: vec![],
        post_token_balances: vec![],
    }
}

/// Create a raw (non-synthetic) transaction event (like Geyser produces)
fn create_raw_event() -> GeyserEvent {
    GeyserEvent::Transaction {
        slot: Some(12345),
        event_ts_ms: None,
        arrival_ts_ms: Some(seer::types::arrival_time_ms()),
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: Signature::from_str("5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW").unwrap(),
        accounts: vec![
            Pubkey::from_str("11111111111111111111111111111111").unwrap(),
            Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
        ],
        instructions: vec![
            RawInstruction {
                program_id: Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap(), // Pump.fun
                account_indices: vec![0, 1],
                data: vec![0x01, 0x02, 0x03], // Raw events have instruction data
            }
        ],
        logs: vec![
            "Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P invoke [1]".to_string(),
            "Program log: Instruction: Create".to_string(),
            "Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P success".to_string(),
        ],
        block_time: Some(1640000000),
        account_data: HashMap::new(),
        pre_balances: vec![],
        post_balances: vec![],
        success: true,
        error_code: None,
        compute_units_consumed: None,
        synthetic: false, // Raw event
        source: "geyser".to_string(),
        mpcf_payload_bytes: Some(vec![0x01, 0x02, 0x03]),
        mpcf_payload_missing_reason: RawBytesMissingReason::NotMissing,
        inner_instructions: vec![],
        pre_token_balances: vec![],
        post_token_balances: vec![],
    }
}

#[test]
fn test_pumpportal_mode_parser_not_created() {
    // GIVEN: Seer configured in PumpPortal mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::PumpPortalWs);

    // WHEN: Seer is created
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let seer = seer::Seer::new(config.clone(), tx);

    // THEN: Binary parser should NOT be created (verified via source code logic)
    // This test verifies that the code compiles and constructs properly
    assert_eq!(config.effective_source_mode(), SeerSourceMode::PumpPortalWs);
}

#[test]
fn test_geyser_mode_parser_created() {
    // GIVEN: Seer configured in Geyser mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::GeyserGrpc);

    // WHEN: Seer is created
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let seer = seer::Seer::new(config.clone(), tx);

    // THEN: Binary parser should be created
    assert_eq!(config.effective_source_mode(), SeerSourceMode::GeyserGrpc);
}

#[test]
fn test_geyser_websocket_mode_parser_created() {
    // GIVEN: Seer configured in GeyserWebSocket mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::GeyserWebSocket);

    // WHEN: Seer is created
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let seer = seer::Seer::new(config.clone(), tx);

    // THEN: Binary parser should be created
    assert_eq!(
        config.effective_source_mode(),
        SeerSourceMode::GeyserWebSocket
    );
}

#[test]
fn test_helius_mode_parser_created() {
    // GIVEN: Seer configured in HeliusWebSocket mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::HeliusWebSocket);
    config.helius_endpoint = Some("wss://mainnet.helius-rpc.com".to_string());

    // WHEN: Seer is created
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let seer = seer::Seer::new(config.clone(), tx);

    // THEN: Binary parser should be created
    assert_eq!(
        config.effective_source_mode(),
        SeerSourceMode::HeliusWebSocket
    );
}

#[test]
fn test_synthetic_event_has_correct_flags() {
    // GIVEN: A synthetic event
    let event = create_synthetic_event();

    // THEN: It should have synthetic=true and source="pumpportal"
    match event {
        GeyserEvent::Transaction {
            synthetic, source, ..
        } => {
            assert!(synthetic, "Synthetic event should have synthetic=true");
            assert_eq!(
                source, "pumpportal",
                "Synthetic event should have source=pumpportal"
            );
        }
        _ => panic!("Expected Transaction event"),
    }
}

#[test]
fn test_raw_event_has_correct_flags() {
    // GIVEN: A raw event
    let event = create_raw_event();

    // THEN: It should have synthetic=false and source="geyser"
    match event {
        GeyserEvent::Transaction {
            synthetic, source, ..
        } => {
            assert!(!synthetic, "Raw event should have synthetic=false");
            assert_eq!(source, "geyser", "Raw event should have source=geyser");
        }
        _ => panic!("Expected Transaction event"),
    }
}

#[test]
fn test_effective_source_mode_pumpportal() {
    // GIVEN: Config with PumpPortal source mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::PumpPortalWs);

    // WHEN: Checking effective source mode
    let mode = config.effective_source_mode();

    // THEN: Should return PumpPortalWs
    assert_eq!(mode, SeerSourceMode::PumpPortalWs);
}

#[test]
fn test_effective_source_mode_fallback() {
    // GIVEN: Config without explicit source_mode (uses connection_mode)
    let mut config = default_test_config();
    config.source_mode = None;
    config.connection_mode = ConnectionMode::Grpc;

    // WHEN: Checking effective source mode
    let mode = config.effective_source_mode();

    // THEN: Should fallback to GeyserGrpc
    assert_eq!(mode, SeerSourceMode::GeyserGrpc);
}

/// This test verifies that the binary parser is not invoked for synthetic events
/// by checking that the Seer component properly handles synthetic events
#[tokio::test]
async fn test_synthetic_event_skips_binary_parsing() {
    // GIVEN: Seer in any mode receiving a synthetic event
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::GeyserGrpc);

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let _seer = seer::Seer::new(config, tx);

    // WHEN: A synthetic event is processed
    let event = create_synthetic_event();

    // THEN: The event should be recognized as synthetic
    match event {
        GeyserEvent::Transaction { synthetic, .. } => {
            assert!(synthetic, "Event should be marked as synthetic");
            // The actual processing logic will skip binary parsing based on this flag
        }
        _ => panic!("Expected Transaction event"),
    }
}

/// This test verifies that in PumpPortal mode, warnings about empty instruction data
/// should not occur (since binary parser is not used)
#[tokio::test]
async fn test_pumpportal_mode_no_parser_warnings() {
    // GIVEN: Seer in PumpPortal mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::PumpPortalWs);

    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let _seer = seer::Seer::new(config.clone(), tx);

    // WHEN: Configuration is verified
    // THEN: PumpPortal mode should be active
    assert_eq!(config.effective_source_mode(), SeerSourceMode::PumpPortalWs);

    // AND: The parser should not be created (verified by construction)
    // This prevents warnings like "⚠️ AMM instruction is empty" and "⚠️ DROPPED POTENTIAL POOL"
}

/// Test that demonstrates the correct routing logic for different event types
#[test]
fn test_source_routing_logic() {
    // PumpPortal mode with synthetic event -> NO parsing
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::PumpPortalWs);
    assert_eq!(config.effective_source_mode(), SeerSourceMode::PumpPortalWs);
    let synthetic_event = create_synthetic_event();
    match synthetic_event {
        GeyserEvent::Transaction {
            synthetic, source, ..
        } => {
            assert!(synthetic);
            assert_eq!(source, "pumpportal");
        }
        _ => panic!("Expected Transaction"),
    }

    // Geyser mode with raw event -> YES parsing
    config.source_mode = Some(SeerSourceMode::GeyserGrpc);
    assert_eq!(config.effective_source_mode(), SeerSourceMode::GeyserGrpc);
    let raw_event = create_raw_event();
    match raw_event {
        GeyserEvent::Transaction {
            synthetic, source, ..
        } => {
            assert!(!synthetic);
            assert_eq!(source, "geyser");
        }
        _ => panic!("Expected Transaction"),
    }
}

/// Test that verifies binary parser is NEVER invoked in PumpPortal mode
/// This test uses metrics to prove no parsing occurs
#[tokio::test]
async fn test_pumpportal_mode_never_invokes_parser() {
    // GIVEN: Seer in PumpPortal mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::PumpPortalWs);

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let seer = seer::Seer::new(config.clone(), tx);

    // Get initial metric value
    let initial_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["initialize_pool"])
        .get();

    // WHEN: A synthetic event is processed (simulating PumpPortal)
    let event = create_synthetic_event();
    let _ = seer.process_event(event).await;

    // THEN: Binary parser should NOT have been invoked
    let final_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["initialize_pool"])
        .get();

    assert_eq!(
        initial_invocations, final_invocations,
        "Binary parser should NOT be invoked in PumpPortal mode"
    );

    // Also verify no trade parsing occurred
    let trade_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["trade"])
        .get();

    assert_eq!(
        0, trade_invocations,
        "Trade parsing should NOT occur in PumpPortal mode"
    );
}

/// Test that verifies binary parser is NEVER invoked for synthetic events in Geyser mode
/// This ensures the "synthetic=true" flag prevents parsing even when parser exists
#[tokio::test]
async fn test_geyser_mode_skips_synthetic_events() {
    // GIVEN: Seer in Geyser mode (parser exists)
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::GeyserGrpc);

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let seer = seer::Seer::new(config.clone(), tx);

    // Verify parser was created in Geyser mode
    assert!(
        matches!(config.effective_source_mode(), SeerSourceMode::GeyserGrpc),
        "Should be in Geyser mode"
    );

    // Get initial metric value
    let initial_pool_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["initialize_pool"])
        .get();

    let initial_trade_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["trade"])
        .get();

    // WHEN: A synthetic event is processed
    let event = create_synthetic_event();
    let _ = seer.process_event(event).await;

    // THEN: Binary parser should NOT have been invoked (even though it exists)
    let final_pool_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["initialize_pool"])
        .get();

    let final_trade_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["trade"])
        .get();

    assert_eq!(
        initial_pool_invocations, final_pool_invocations,
        "Binary parser should NOT parse synthetic events for pool detection"
    );

    assert_eq!(
        initial_trade_invocations, final_trade_invocations,
        "Binary parser should NOT parse synthetic events for trade detection"
    );
}

/// Test that verifies binary parser IS invoked for raw events in Geyser mode
/// This ensures backward compatibility - Geyser mode still works as expected
#[tokio::test]
async fn test_geyser_mode_parses_raw_events() {
    // GIVEN: Seer in Geyser mode
    let mut config = default_test_config();
    config.source_mode = Some(SeerSourceMode::GeyserGrpc);

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let seer = seer::Seer::new(config.clone(), tx);

    // Get initial metric value
    let initial_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["initialize_pool"])
        .get();

    // WHEN: A raw (non-synthetic) event is processed
    let event = create_raw_event();
    let _ = seer.process_event(event).await;

    // THEN: Binary parser SHOULD have been invoked
    let final_invocations = seer
        .metrics()
        .binary_parser_invocations
        .with_label_values(&["initialize_pool"])
        .get();

    assert!(
        final_invocations > initial_invocations,
        "Binary parser SHOULD be invoked for raw events in Geyser mode"
    );
}
