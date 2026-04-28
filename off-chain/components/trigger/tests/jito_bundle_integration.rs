//! Integration test demonstrating Jito bundle building and submission
//!
//! This test showcases the complete workflow of building and submitting
//! Jito bundles with various configurations and priority levels.

use solana_sdk::{
    hash::Hash,
    message::{v0, VersionedMessage},
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use trigger::{
    BundleBuilder, BundleConfig, JitoClient, JitoClientBuilder, RedundancyPolicy, TipConfig,
};

/// Helper function to create a dummy versioned transaction
fn create_test_transaction() -> VersionedTransaction {
    let payer = Keypair::new();
    let message = v0::Message::try_compile(&payer.pubkey(), &[], &[], Hash::default()).unwrap();

    VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer]).unwrap()
}

#[tokio::test]
async fn test_jito_bundle_basic_workflow() {
    // Create a basic bundle configuration
    let bundle_config = BundleConfig::default();

    // Initialize Jito client with dry-run mode for testing
    let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", bundle_config);
    jito_client.set_dry_run(true); // Enable dry-run mode for tests
    let bundle_builder = BundleBuilder::new(jito_client);

    // Create test transactions
    let init_pool_tx = create_test_transaction();
    let ghost_tx = create_test_transaction();

    // Build and submit bundle with medium priority
    let result = bundle_builder
        .build_and_submit_single(
            init_pool_tx,
            ghost_tx,
            1_000_000_000, // 1 SOL transaction value
            0.5,           // Medium priority (50% between base and dynamic)
            Hash::default(),
        )
        .await;

    // Verify the bundle was created successfully
    assert!(result.is_ok());
    let (_signature, diagnostics) = result.unwrap();

    // Validate diagnostics
    assert_eq!(diagnostics.transaction_count, 2); // InitPool + 1 Ghost TX
    assert_eq!(diagnostics.tip_lamports, 35_000_000); // 3.5% tip (base 2% + 50% of 3% range)
    assert_eq!(diagnostics.redundancy_count, 4); // N+3 default
    assert!(diagnostics.nonce_staggered);
}

#[tokio::test]
async fn test_jito_bundle_multiple_ghost_txs() {
    // Create bundle configuration
    let bundle_config = BundleConfig::default();
    let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", bundle_config);
    jito_client.set_dry_run(true); // Enable dry-run mode for tests
    let bundle_builder = BundleBuilder::new(jito_client);

    // Create InitializePool transaction and multiple Ghost transactions
    let init_pool_tx = create_test_transaction();
    let ghost_txs = vec![
        create_test_transaction(),
        create_test_transaction(),
        create_test_transaction(),
    ];

    // Build and submit bundle
    let result = bundle_builder
        .build_and_submit(
            init_pool_tx,
            ghost_txs,
            5_000_000_000, // 5 SOL transaction value
            0.0,           // Base priority
            Hash::default(),
        )
        .await;

    assert!(result.is_ok());
    let (_signature, diagnostics) = result.unwrap();

    // Verify bundle structure
    assert_eq!(diagnostics.transaction_count, 4); // InitPool + 3 Ghost TXs
    assert_eq!(diagnostics.tip_lamports, 100_000_000); // 2% of 5 SOL = 0.1 SOL
    assert_eq!(diagnostics.priority_factor, 0.0);
}

#[tokio::test]
async fn test_jito_bundle_high_priority() {
    // Create bundle configuration with custom tip config
    let tip_config = TipConfig::new(
        0.02,        // 2% base
        0.05,        // 5% dynamic
        0.05,        // 5% max
        10_000,      // min tip
        100_000_000, // max tip (0.1 SOL)
    );

    let bundle_config = BundleConfig::new(
        RedundancyPolicy::NPlusThree,
        tip_config,
        true, // stagger nonce
        true, // enable diagnostics
    );

    let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", bundle_config);
    jito_client.set_dry_run(true); // Enable dry-run mode for tests
    let bundle_builder = BundleBuilder::new(jito_client);

    // Create transactions
    let init_pool_tx = create_test_transaction();
    let ghost_tx = create_test_transaction();

    // Submit with maximum priority
    let result = bundle_builder
        .build_and_submit_single(
            init_pool_tx,
            ghost_tx,
            1_000_000_000, // 1 SOL
            1.0,           // Maximum priority (100%)
            Hash::default(),
        )
        .await;

    assert!(result.is_ok());
    let (_signature, diagnostics) = result.unwrap();

    // At max priority, should use dynamic tip (5%)
    assert_eq!(diagnostics.tip_lamports, 50_000_000); // 5% of 1 SOL
    assert_eq!(diagnostics.priority_factor, 1.0);
    assert_eq!(diagnostics.tip_percent, 5.0);
}

#[tokio::test]
async fn test_jito_bundle_with_n_plus_five_redundancy() {
    // Create configuration with maximum redundancy
    let bundle_config = BundleConfig::new(
        RedundancyPolicy::NPlusFive,
        TipConfig::default(),
        true,
        true,
    );

    let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", bundle_config);
    jito_client.set_dry_run(true); // Enable dry-run mode for tests
    let bundle_builder = BundleBuilder::new(jito_client);

    // Verify redundancy configuration
    assert_eq!(
        bundle_builder.bundle_config().redundancy_policy,
        RedundancyPolicy::NPlusFive
    );

    // Create and submit bundle
    let init_pool_tx = create_test_transaction();
    let ghost_tx = create_test_transaction();

    let result = bundle_builder
        .build_and_submit_single(init_pool_tx, ghost_tx, 1_000_000_000, 0.5, Hash::default())
        .await;

    assert!(result.is_ok());
    let (_signature, diagnostics) = result.unwrap();

    // Should submit 6 bundles (N+5)
    assert_eq!(diagnostics.redundancy_count, 6);
}

#[tokio::test]
async fn test_jito_bundle_with_n_plus_one_redundancy() {
    // Create configuration with minimal redundancy
    let bundle_config = BundleConfig::new(
        RedundancyPolicy::NPlusOne,
        TipConfig::default(),
        false, // disable nonce staggering
        false, // disable diagnostics
    );

    let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", bundle_config);
    jito_client.set_dry_run(true); // Enable dry-run mode for tests
    let bundle_builder = BundleBuilder::new(jito_client);

    let init_pool_tx = create_test_transaction();
    let ghost_tx = create_test_transaction();

    let result = bundle_builder
        .build_and_submit_single(init_pool_tx, ghost_tx, 1_000_000_000, 0.5, Hash::default())
        .await;

    assert!(result.is_ok());
    let (_signature, diagnostics) = result.unwrap();

    // Should submit 2 bundles (N+1)
    assert_eq!(diagnostics.redundancy_count, 2);
    assert!(!diagnostics.nonce_staggered);
}

#[tokio::test]
async fn test_jito_bundle_tip_capping() {
    // Test that tips are properly capped at max limits
    let tip_config = TipConfig::new(
        0.02,       // 2% base
        0.05,       // 5% dynamic
        0.05,       // 5% max
        10_000,     // min tip
        50_000_000, // max tip (0.05 SOL) - lower than default
    );

    let bundle_config = BundleConfig::new(RedundancyPolicy::NPlusThree, tip_config, true, true);

    let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", bundle_config);
    jito_client.set_dry_run(true); // Enable dry-run mode for tests
    let bundle_builder = BundleBuilder::new(jito_client);

    let init_pool_tx = create_test_transaction();
    let ghost_tx = create_test_transaction();

    // Submit with max priority on high value transaction
    let result = bundle_builder
        .build_and_submit_single(
            init_pool_tx,
            ghost_tx,
            10_000_000_000, // 10 SOL - would give 0.5 SOL tip at 5%
            1.0,            // Max priority
            Hash::default(),
        )
        .await;

    assert!(result.is_ok());
    let (_signature, diagnostics) = result.unwrap();

    // Tip should be capped at max_tip_lamports
    assert_eq!(diagnostics.tip_lamports, 50_000_000); // Capped at 0.05 SOL
}

#[tokio::test]
async fn test_jito_bundle_min_tip() {
    // Test minimum tip enforcement
    let bundle_config = BundleConfig::default();
    let mut jito_client = JitoClient::new("https://test.jito.wtf/api/v1", bundle_config);
    jito_client.set_dry_run(true); // Enable dry-run mode for tests
    let bundle_builder = BundleBuilder::new(jito_client);

    let init_pool_tx = create_test_transaction();
    let ghost_tx = create_test_transaction();

    // Submit with very small transaction value
    let result = bundle_builder
        .build_and_submit_single(
            init_pool_tx,
            ghost_tx,
            100, // Very small value
            0.0, // Base priority
            Hash::default(),
        )
        .await;

    assert!(result.is_ok());
    let (_signature, diagnostics) = result.unwrap();

    // Tip should be at minimum
    assert_eq!(diagnostics.tip_lamports, 10_000); // Min tip
}

#[tokio::test]
async fn test_jito_client_builder_pattern() {
    // Test the builder pattern for client creation
    let client = JitoClientBuilder::new()
        .with_endpoint("https://custom.jito.endpoint")
        .with_redundancy_policy(RedundancyPolicy::NPlusFive)
        .with_tip_config(TipConfig::new(0.03, 0.06, 0.06, 15_000, 150_000_000))
        .with_diagnostics(true)
        .build();

    assert!(client.is_ok());
    let client = client.unwrap();

    // Verify configuration
    let config = client.bundle_config();
    assert_eq!(config.redundancy_policy, RedundancyPolicy::NPlusFive);
    assert_eq!(config.tip_config.base_tip_percent, 0.03);
    assert_eq!(config.tip_config.dynamic_tip_percent, 0.06);
    assert!(config.enable_diagnostics);
}

#[test]
fn test_tip_calculation_examples() {
    // Document tip calculation behavior with examples
    let tip_config = TipConfig::default();

    // Example 1: 1 SOL swap at base priority (0%)
    let tip1 = tip_config.calculate_tip(1_000_000_000, 0.0);
    assert_eq!(tip1, 20_000_000); // 2% = 0.02 SOL

    // Example 2: 1 SOL swap at 25% priority
    let tip2 = tip_config.calculate_tip(1_000_000_000, 0.25);
    assert_eq!(tip2, 27_500_000); // 2.75% = 0.0275 SOL

    // Example 3: 1 SOL swap at 50% priority
    let tip3 = tip_config.calculate_tip(1_000_000_000, 0.5);
    assert_eq!(tip3, 35_000_000); // 3.5% = 0.035 SOL

    // Example 4: 1 SOL swap at 75% priority
    let tip4 = tip_config.calculate_tip(1_000_000_000, 0.75);
    assert_eq!(tip4, 42_500_000); // 4.25% = 0.0425 SOL

    // Example 5: 1 SOL swap at max priority (100%)
    let tip5 = tip_config.calculate_tip(1_000_000_000, 1.0);
    assert_eq!(tip5, 50_000_000); // 5% = 0.05 SOL
}
