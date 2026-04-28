//! E2E Integration tests for Revolver scenarios
//!
//! These tests validate the Revolver E2E workflow in the ghost-e2e context.

use ghost_brain::config::{
    E2EConfig, ExecutionConfig, FeaturesConfig, GuiBackendConfig, IntelligenceConfig,
    LeaderPredictorConfig, MetricsConfig, OracleConfig, SeerConfig, TriggerConfig,
};
use ghost_brain::metrics::E2EMetrics;
use ghost_brain::scenarios::scenario_revolver::ScenarioRevolver;
use ghost_brain::scenarios::TestScenario;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Helper function to create a test configuration
fn create_test_config() -> E2EConfig {
    E2EConfig {
        rpc_url: "http://localhost:8899".to_string(),
        websocket_url: "ws://localhost:8900".to_string(),
        authority_keypair_path: "test".to_string(),
        payer_keypair_path: "test".to_string(),
        seer: SeerConfig {
            enable_pumpfun: true,
            enable_bonkfun: true,
            min_liquidity_sol: Some(5.0),
            max_reconnect_attempts: 3,
            reconnect_delay_secs: 5,
            verbose: false,
        },
        oracle: OracleConfig {
            min_score_threshold: 70,
            enable_anomaly_detection: false,
            rpc_endpoints: vec![],
        },
        features: FeaturesConfig {
            default_strategy: "test".to_string(),
            max_position_size_lamports: 1000000,
            max_slippage: 0.05,
            intent_timeout_secs: 3600,
        },
        trigger: TriggerConfig {
            redundancy_factor: 3,
            max_span_slots: 4,
            enable_jito: false,
            jito_block_engine_url: None,
            dry_run: true,
            max_concurrent_positions: Some(3),
            enable_leapfrog: false,
            leapfrog_redundancy: 2,
            leapfrog_use_quic: false,
        },
        metrics: MetricsConfig {
            enable_prometheus: false,
            prometheus_port: 9090,
            target_land_rate: 100.0,
            target_inclusion_rate: 92.0,
        },
        gui_backend: GuiBackendConfig {
            enabled: false,
            port: 8080,
            bind_address: "localhost".to_string(),
        },
        leader_predictor: LeaderPredictorConfig {
            enabled: false,
            grpc_endpoint: "test".to_string(),
            our_leaders: vec![],
            verbose: false,
        },
        intelligence: IntelligenceConfig {
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
        execution: ExecutionConfig::default(),
    }
}

#[tokio::test]
async fn test_revolver_scenario_full_flow() {
    // Initialize tracing for test visibility
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    // Create test config
    let config = create_test_config();
    let metrics = Arc::new(E2EMetrics::new());

    // Create and run scenario
    let scenario = ScenarioRevolver::new(config.clone());
    let result = scenario.run(&config, metrics).await;

    // Verify results
    assert!(result.is_ok(), "Scenario should complete without errors");
    let result = result.unwrap();

    println!("\n=== Revolver E2E Test Results ===");
    println!("Passed: {}", result.passed);
    println!("Land Rate: {:.2}%", result.land_rate);
    println!("Inclusion Rate: {:.2}%", result.inclusion_rate);
    println!("Observations:");
    for obs in &result.observations {
        println!("  - {}", obs);
    }

    // Assert success criteria
    assert!(result.passed, "Scenario should pass all internal tests");
    assert_eq!(result.land_rate, 100.0, "Land rate should be 100%");
    assert_eq!(
        result.inclusion_rate, 100.0,
        "Inclusion rate should be 100%"
    );
    assert!(
        !result.observations.is_empty(),
        "Should have observations recorded"
    );
}

#[tokio::test]
async fn test_revolver_bullet_creation() {
    // Test that bullets are created with non-empty tx_bytes
    use ghost_brain::scenarios::scenario_revolver::{MockBlockhashProvider, MockSellTxBuilder};
    use solana_sdk::pubkey::Pubkey;
    use trigger::{Bullet, PriceTarget};

    let mint = Pubkey::new_unique();
    let position_size = 1_000_000_u64;
    let entry_price = 500_u64;

    let blockhash_provider = MockBlockhashProvider::new();
    let tx_builder = MockSellTxBuilder::new(mint, position_size);

    // Create a bullet
    let target = PriceTarget::new(2.0, 5000).unwrap();
    let target_price = target.calculate_target_price(entry_price);
    let tx_bytes = tx_builder
        .build_sell_tx(
            target_price,
            target.position_fraction_bps,
            &blockhash_provider.get_blockhash(),
        )
        .unwrap();

    let bullet = Bullet::new(tx_bytes.clone(), target_price, target.position_fraction_bps).unwrap();

    // Verify bullet properties
    assert!(
        !bullet.tx_bytes.is_empty(),
        "Bullet should have non-empty tx_bytes"
    );
    assert_eq!(bullet.target_price, target_price);
    assert_eq!(bullet.position_fraction_bps, 5000);
}

#[tokio::test]
async fn test_revolver_bullet_refresh() {
    // Test that bullets can be refreshed with new blockhash
    use ghost_brain::scenarios::scenario_revolver::{MockBlockhashProvider, MockSellTxBuilder};
    use solana_sdk::pubkey::Pubkey;
    use trigger::Bullet;

    let mint = Pubkey::new_unique();
    let position_size = 1_000_000_u64;
    let target_price = 1000_u64;
    let fraction_bps = 2500_u16;

    let mut blockhash_provider = MockBlockhashProvider::new();
    let tx_builder = MockSellTxBuilder::new(mint, position_size);

    // Create initial bullet
    let initial_hash = blockhash_provider.get_blockhash();
    let tx_bytes = tx_builder
        .build_sell_tx(target_price, fraction_bps, &initial_hash)
        .unwrap();
    let mut bullet = Bullet::new(tx_bytes, target_price, fraction_bps).unwrap();

    let initial_tx = String::from_utf8_lossy(&bullet.tx_bytes).to_string();
    assert!(initial_tx.contains("MockBlockhash1111"));

    // Refresh with new blockhash
    blockhash_provider.update_blockhash("MockBlockhash9999999999999999999999999999".to_string());
    let new_hash = blockhash_provider.get_blockhash();
    let new_tx_bytes = tx_builder
        .build_sell_tx(target_price, fraction_bps, &new_hash)
        .unwrap();

    bullet.update_tx(new_tx_bytes);

    // Verify update
    let updated_tx = String::from_utf8_lossy(&bullet.tx_bytes).to_string();
    assert!(updated_tx.contains("MockBlockhash9999"));
    assert!(!updated_tx.contains("MockBlockhash1111"));
}

#[tokio::test]
async fn test_revolver_bullet_firing_logic() {
    // Test that bullets fire at correct price thresholds
    use ghost_brain::scenarios::scenario_revolver::MockSellTxBuilder;
    use solana_sdk::pubkey::Pubkey;
    use trigger::{Bullet, Revolver};

    let mint = Pubkey::new_unique();
    let mut revolver = Revolver::new();

    let tx_builder = MockSellTxBuilder::new(mint, 1_000_000);

    // Create bullets at different price points
    let bullets = vec![
        Bullet::new(
            tx_builder.build_sell_tx(2000, 2500, "hash1").unwrap(),
            2000, // 2x target
            2500,
        )
        .unwrap(),
        Bullet::new(
            tx_builder.build_sell_tx(3000, 2500, "hash2").unwrap(),
            3000, // 3x target
            2500,
        )
        .unwrap(),
        Bullet::new(
            tx_builder.build_sell_tx(5000, 5000, "hash3").unwrap(),
            5000, // 5x target
            5000,
        )
        .unwrap(),
    ];

    revolver.load_magazine(mint, bullets);
    assert_eq!(revolver.total_bullet_count(), 3);

    // Test price below all targets
    let token_revolver = revolver.get_revolver(&mint).unwrap();
    let indices = token_revolver.check_targets(1500);
    assert_eq!(indices.len(), 0, "No bullets should trigger at 1.5x");

    // Test price at 2.5x - should trigger first bullet
    let indices = token_revolver.check_targets(2500);
    assert_eq!(indices.len(), 1, "One bullet should trigger at 2.5x");
    assert_eq!(indices[0], 0, "First bullet (2x) should trigger");

    // Test price at 10x - should trigger all remaining bullets
    let indices = token_revolver.check_targets(10000);
    assert_eq!(indices.len(), 3, "All bullets should trigger at 10x");
}

#[tokio::test]
async fn test_revolver_magazine_depletion() {
    // Test that bullets are removed after firing
    use ghost_brain::scenarios::scenario_revolver::{MockSellTxBuilder, MockTpuClient};
    use solana_sdk::pubkey::Pubkey;
    use trigger::{Bullet, Revolver};

    let mint = Pubkey::new_unique();
    let mut revolver = Revolver::new();
    let tpu_client = MockTpuClient::new();

    let tx_builder = MockSellTxBuilder::new(mint, 1_000_000);

    // Create and load bullets
    let bullets = vec![
        Bullet::new(
            tx_builder.build_sell_tx(2000, 5000, "hash1").unwrap(),
            2000,
            5000,
        )
        .unwrap(),
        Bullet::new(
            tx_builder.build_sell_tx(5000, 5000, "hash2").unwrap(),
            5000,
            5000,
        )
        .unwrap(),
    ];

    revolver.load_magazine(mint, bullets);
    assert_eq!(revolver.total_bullet_count(), 2);

    // Fire first bullet
    let token_revolver = revolver.get_revolver_mut(&mint).unwrap();
    let indices = token_revolver.check_targets(3000); // Should fire 2x bullet
    let fired_bullets = token_revolver.take_bullets(&indices);

    assert_eq!(fired_bullets.len(), 1);
    assert_eq!(revolver.total_bullet_count(), 1, "One bullet should remain");

    // Simulate sending
    for bullet in fired_bullets {
        let _sig = tpu_client.send_transaction(&bullet.tx_bytes).await.unwrap();
    }

    assert_eq!(tpu_client.get_sent_count().await, 1);

    // Fire remaining bullet
    let token_revolver = revolver.get_revolver_mut(&mint).unwrap();
    let indices = token_revolver.check_targets(10000);
    let fired_bullets = token_revolver.take_bullets(&indices);

    assert_eq!(fired_bullets.len(), 1);
    assert_eq!(revolver.total_bullet_count(), 0, "Magazine should be empty");
}
