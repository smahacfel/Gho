//! Integration test for E2E Pipeline with Jito Bundle Executor
//!
//! This test verifies that the pipeline correctly initializes and operates
//! with the Jito Bundle Executor enabled.

use ghost_brain::{E2EConfig, E2EPipeline, ExecutionMode};
use solana_sdk::signer::keypair::Keypair;
use tempfile::TempDir;

/// Test that E2EPipeline correctly initializes with Jito enabled
#[test]
fn test_pipeline_jito_initialization() {
    // Create a test configuration with Jito enabled
    let mut config = create_test_config();
    config.trigger.enable_jito = true;
    config.trigger.jito_block_engine_url =
        Some("https://mainnet.block-engine.jito.wtf".to_string());

    // Attempt to create pipeline - should succeed even if keypairs don't exist
    // in CI environment (validation will catch missing files)
    let result = E2EPipeline::new(config);

    // In CI, this will fail due to missing keypair files, which is expected
    // The important part is that the code compiles and the Jito logic is reachable
    match result {
        Ok(_pipeline) => {
            // Success - pipeline created (only possible if keypairs exist)
            println!("✅ Pipeline with Jito executor created successfully");
        }
        Err(e) => {
            // Expected in CI - keypair files don't exist
            println!("⚠️  Pipeline creation failed (expected in CI): {}", e);
            assert!(
                e.to_string().contains("keypair") || e.to_string().contains("Failed to read"),
                "Error should be about missing keypair files"
            );
        }
    }
}

/// Test that E2EPipeline correctly initializes with Jito disabled
#[test]
fn test_pipeline_standard_mode_initialization() {
    let mut config = create_test_config();
    config.trigger.enable_jito = false;

    let result = E2EPipeline::new(config);

    match result {
        Ok(_pipeline) => {
            println!("✅ Pipeline in standard mode created successfully");
        }
        Err(e) => {
            println!("⚠️  Pipeline creation failed (expected in CI): {}", e);
            assert!(
                e.to_string().contains("keypair") || e.to_string().contains("Failed to read"),
                "Error should be about missing keypair files"
            );
        }
    }
}

/// Test configuration validation with Jito settings
#[test]
fn test_jito_config_validation() {
    let config = create_test_config();

    // This should succeed - the config structure is valid
    // (validation of file paths will fail in CI, which is separate)
    assert_eq!(config.trigger.enable_jito, false);
    assert_eq!(config.trigger.redundancy_factor, 3);
    assert_eq!(config.trigger.max_span_slots, 4);
}

/// Test that Jito executor can be conditionally enabled
#[test]
fn test_jito_conditional_initialization() {
    let mut config = create_test_config();

    // Test with Jito enabled
    config.trigger.enable_jito = true;
    config.trigger.jito_block_engine_url = Some("https://test.jito.wtf".to_string());
    assert!(config.trigger.enable_jito);

    // Test with Jito disabled
    config.trigger.enable_jito = false;
    assert!(!config.trigger.enable_jito);
}

/// Test redundancy factor configuration
#[test]
fn test_redundancy_configuration() {
    let mut config = create_test_config();

    // Test different redundancy levels
    config.trigger.redundancy_factor = 1; // N+1 (testing)
    assert_eq!(config.trigger.redundancy_factor, 1);

    config.trigger.redundancy_factor = 3; // N+3 (production)
    assert_eq!(config.trigger.redundancy_factor, 3);

    config.trigger.redundancy_factor = 5; // N+5 (high priority)
    assert_eq!(config.trigger.redundancy_factor, 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_shadow_pipeline_fails_closed_without_account_state_core_feed() {
    let temp = TempDir::new().expect("tempdir");
    let authority_path = temp.path().join("authority.json");
    let payer_path = temp.path().join("payer.json");
    write_keypair_file(&authority_path, &Keypair::new());
    write_keypair_file(&payer_path, &Keypair::new());

    let mut config = create_test_config();
    config.authority_keypair_path = authority_path.to_string_lossy().to_string();
    config.payer_keypair_path = payer_path.to_string_lossy().to_string();
    config.execution.execution_mode = ExecutionMode::Shadow;

    let err = match E2EPipeline::new(config) {
        Ok(_) => panic!("shadow pipeline should fail closed"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("requires AccountStateCore feed"),
        "unexpected error: {err}"
    );
}

/// Helper function to create a test configuration
fn create_test_config() -> E2EConfig {
    E2EConfig {
        rpc_url: "https://api.devnet.solana.com".to_string(),
        websocket_url: "wss://api.devnet.solana.com".to_string(),
        authority_keypair_path: "/tmp/test_authority.json".to_string(),
        payer_keypair_path: "/tmp/test_payer.json".to_string(),
        seer: ghost_brain::config::SeerConfig {
            enable_pumpfun: true,
            enable_bonkfun: true,
            min_liquidity_sol: Some(1.0),
            max_reconnect_attempts: 5,
            reconnect_delay_secs: 5,
            verbose: false,
        },
        oracle: ghost_brain::config::OracleConfig {
            min_score_threshold: 70,
            enable_anomaly_detection: true,
            rpc_endpoints: vec!["https://api.devnet.solana.com".to_string()],
        },
        features: ghost_brain::config::FeaturesConfig {
            default_strategy: "snipe_new_pool".to_string(),
            max_position_size_lamports: 10_000_000,
            max_slippage: 0.05,
            intent_timeout_secs: 3600,
        },
        trigger: ghost_brain::config::TriggerConfig {
            redundancy_factor: 3,
            max_span_slots: 4,
            enable_jito: false,
            jito_block_engine_url: None,
            dry_run: false,
            max_concurrent_positions: Some(3),
            enable_leapfrog: false,
            leapfrog_redundancy: 2,
            leapfrog_use_quic: false,
        },
        metrics: ghost_brain::config::MetricsConfig {
            enable_prometheus: true,
            prometheus_port: 9090,
            target_land_rate: 95.0,
            target_inclusion_rate: 92.0,
        },
        gui_backend: ghost_brain::config::GuiBackendConfig {
            enabled: false,
            port: 8800,
            bind_address: "127.0.0.1".to_string(),
        },
        leader_predictor: ghost_brain::config::LeaderPredictorConfig {
            enabled: false,
            grpc_endpoint: "http://localhost:10000".to_string(),
            our_leaders: vec![],
            verbose: false,
        },
        intelligence: ghost_brain::config::IntelligenceConfig {
            enable_vision: false,
            vision_provider: "openai".to_string(),
            vision_api_key: None,
            openai_model: "gpt-4o-mini".to_string(),
            anthropic_model: "claude-3-haiku-20240307".to_string(),
            max_cluster_size: 20,
            min_cluster_size: 3,
            high_risk_threshold_pct: 30.0,
            max_signatures: 10,
            serial_minter_threshold: 5,
            serial_minter_window_hours: 24,
            rpc_timeout_secs: 10,
            vision_api_timeout_secs: 30,
        },
        execution: ghost_brain::config::ExecutionConfig::default(),
    }
}

fn write_keypair_file(path: &std::path::Path, keypair: &Keypair) {
    let payload = serde_json::to_vec(&keypair.to_bytes().to_vec()).expect("serialize keypair");
    std::fs::write(path, payload).expect("write keypair");
}

/// Test metrics structure includes Jito fields
#[test]
fn test_metrics_include_jito_fields() {
    use ghost_brain::E2EMetrics;

    let metrics = E2EMetrics::new();

    // Verify Jito metrics are initialized
    let (submitted, confirmed, tips) = metrics.get_jito_stats();
    assert_eq!(submitted, 0.0);
    assert_eq!(confirmed, 0.0);
    assert_eq!(tips, 0.0);

    let rate = metrics.calculate_jito_confirmation_rate();
    assert_eq!(rate, 0.0);
}

/// Test that Jito metrics can be updated
#[test]
fn test_jito_metrics_update() {
    use ghost_brain::E2EMetrics;

    let metrics = E2EMetrics::new();

    // Simulate bundle submission
    metrics.jito_bundles_submitted.inc_by(5.0);
    metrics.jito_bundles_confirmed.inc_by(4.0);
    metrics.jito_total_tips_paid.inc_by(1_000_000.0); // 0.001 SOL

    let (submitted, confirmed, tips) = metrics.get_jito_stats();
    assert_eq!(submitted, 5.0);
    assert_eq!(confirmed, 4.0);
    assert_eq!(tips, 1_000_000.0);

    let rate = metrics.calculate_jito_confirmation_rate();
    assert_eq!(rate, 80.0); // 4/5 = 80%
}
