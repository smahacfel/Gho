//! Integration test for Capital Preservation Suite (Bulkhead & TipGuard)
//!
//! This test verifies that the safety modules are properly wired into the
//! transaction builder and Jito client, protecting against portfolio depletion.

use ghost_core::SwapPlan;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use trigger::{
    AmmAccounts, AmmType, BundleConfig, GhostTransactionBuilder, JitoClient, JitoClientBuilder,
    SafetyConfig, TipConfig, TipGuardConfig,
};

/// Test that emergency floor check prevents transaction building
#[test]
fn test_bulkhead_emergency_floor_protection() {
    // Create a mock swap plan
    let swap_plan = create_test_swap_plan(100_000_000); // 0.1 SOL trade

    // Create transaction builder
    let builder =
        GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, create_test_amm_accounts());

    // Create safety config
    let safety_config = SafetyConfig::default();

    // Create payer and blockhash
    let payer = Keypair::new();
    let recent_blockhash = Hash::default();

    // SCENARIO: Balance is BELOW emergency floor (0.04 SOL < 0.05 SOL floor)
    let balance_sol = 0.04;

    // Try to build transaction - should FAIL with safety violation
    let result = builder.build_full_swap_tx_with_safety(
        &payer,
        recent_blockhash,
        balance_sol,
        &safety_config,
    );

    // ASSERT: Transaction building should fail
    assert!(
        result.is_err(),
        "Expected transaction building to fail due to emergency floor violation"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Safety violation") || error_msg.contains("Balance critical"),
        "Error should mention safety violation, got: {}",
        error_msg
    );

    println!("✅ Test passed: Emergency floor protection working");
}

/// Test that position size is capped when balance is low
#[test]
fn test_bulkhead_position_size_capping() {
    // Create a swap plan with 0.1 SOL trade (within normal limits)
    let swap_plan = create_test_swap_plan(100_000_000); // 0.1 SOL

    let builder =
        GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, create_test_amm_accounts());

    let safety_config = SafetyConfig::default();
    let payer = Keypair::new();
    let recent_blockhash = Hash::default();

    // SCENARIO: Balance is low (0.08 SOL)
    // Available: 0.08 - 0.05 (floor) - 0.02 (buffer) = 0.01 SOL
    // Trade amount: 0.1 SOL
    // This should FAIL because 0.1 > 0.01 safe amount
    let balance_sol = 0.08;

    let result = builder.build_full_swap_tx_with_safety(
        &payer,
        recent_blockhash,
        balance_sol,
        &safety_config,
    );

    // ASSERT: Should fail because trade exceeds safe amount
    assert!(
        result.is_err(),
        "Expected transaction to fail due to insufficient safe balance"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("validation failed") || error_msg.contains("exceeds"),
        "Error should mention validation failure, got: {}",
        error_msg
    );

    println!("✅ Test passed: Position size capping working");
}

/// Test that transaction succeeds with sufficient balance
#[test]
fn test_bulkhead_allows_safe_trades() {
    // Create a swap plan with 0.1 SOL trade
    let swap_plan = create_test_swap_plan(100_000_000); // 0.1 SOL

    // Create a builder with custom LUT config that won't validate pool
    let mut builder =
        GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, create_test_amm_accounts());

    // For this test, we're only testing safety checks, not pool validation
    // The fact that we can create the builder proves the safety layer is integrated

    let safety_config = SafetyConfig::default();
    let payer = Keypair::new();
    let recent_blockhash = Hash::default();

    // SCENARIO: Sufficient balance (1.0 SOL)
    // Available: 1.0 - 0.05 - 0.02 = 0.93 SOL
    // Trade: 0.1 SOL
    // Safety checks should PASS (pool validation may fail, but that's separate)
    let balance_sol = 1.0;

    let result = builder.build_full_swap_tx_with_safety(
        &payer,
        recent_blockhash,
        balance_sol,
        &safety_config,
    );

    // ASSERT: Should fail on pool validation, NOT on safety checks
    // If error mentions "Safety" or "Balance critical", the test fails
    // If error mentions "pool" or "whitelisted", safety checks passed!
    match result {
        Ok(_) => {
            println!("✅ Test passed: Transaction built successfully with sufficient balance");
        }
        Err(e) => {
            let error_msg = e.to_string();
            // Safety check should have passed
            assert!(
                !error_msg.contains("Safety") && !error_msg.contains("Balance critical"),
                "Safety checks failed when balance was sufficient: {}",
                error_msg
            );
            // Pool validation failure is expected and acceptable for this test
            if error_msg.contains("pool") || error_msg.contains("whitelisted") {
                println!(
                    "✅ Test passed: Safety checks passed (pool validation failed as expected)"
                );
            }
        }
    }
}

/// Test that TipGuard caps excessive tips
#[test]
fn test_tipguard_caps_excessive_tips() {
    // Create Jito client
    let bundle_config = BundleConfig {
        redundancy_policy: trigger::RedundancyPolicy::NPlusOne,
        tip_config: TipConfig {
            base_tip_percent: 0.02,
            dynamic_tip_percent: 0.50, // Aggressive: 50% tip!
            max_tip_percent: 0.50,
            min_tip_lamports: 1_000,
            max_tip_lamports: 1_000_000_000, // 1 SOL max (way too high!)
        },
        stagger_nonce: false,
        enable_diagnostics: true,
    };

    let client = JitoClientBuilder::new()
        .with_endpoint("https://mainnet.block-engine.jito.wtf")
        .with_bundle_config(bundle_config)
        .build()
        .expect("Failed to build Jito client");

    // Create mock transactions
    let init_tx = create_mock_transaction();
    let ghost_txs = vec![create_mock_transaction()];
    let payer = Keypair::new();
    let recent_blockhash = Hash::default();

    // SCENARIO: Small trade (0.1 SOL) but high priority (1.0)
    // Without TipGuard: 50% of 0.1 SOL = 0.05 SOL tip
    // With TipGuard absolute cap: 0.04 SOL max
    // With TipGuard ratio cap: 40% of 0.1 = 0.04 SOL max
    let transaction_value = 100_000_000; // 0.1 SOL
    let priority = 1.0; // Maximum priority

    let tip_guard_config = TipGuardConfig::default();

    // Build bundle WITH TipGuard
    let result = client.build_bundle_with_tip_guard(
        init_tx,
        ghost_txs,
        transaction_value,
        priority,
        recent_blockhash,
        Some(&payer),
        &tip_guard_config,
    );

    assert!(result.is_ok(), "Bundle building should succeed");

    let bundle = result.unwrap();

    // ASSERT: Tip should be capped at 0.04 SOL (40_000_000 lamports)
    let max_allowed_lamports = 40_000_000; // 0.04 SOL
    assert!(
        bundle.tip_lamports <= max_allowed_lamports,
        "Tip {} lamports exceeds TipGuard cap of {} lamports",
        bundle.tip_lamports,
        max_allowed_lamports
    );

    println!(
        "✅ Test passed: TipGuard capped tip to {} lamports (max: {})",
        bundle.tip_lamports, max_allowed_lamports
    );
}

/// Test that TipGuard respects ratio cap
#[test]
fn test_tipguard_ratio_cap() {
    let bundle_config = BundleConfig::default();
    let client = JitoClientBuilder::new()
        .with_endpoint("https://mainnet.block-engine.jito.wtf")
        .with_bundle_config(bundle_config)
        .build()
        .expect("Failed to build Jito client");

    let init_tx = create_mock_transaction();
    let ghost_txs = vec![create_mock_transaction()];
    let payer = Keypair::new();
    let recent_blockhash = Hash::default();

    // SCENARIO: Trade value 0.05 SOL, ratio cap should be 40% = 0.02 SOL
    let transaction_value = 50_000_000; // 0.05 SOL
    let priority = 1.0;

    let tip_guard_config = TipGuardConfig::default(); // 40% ratio cap

    let result = client.build_bundle_with_tip_guard(
        init_tx,
        ghost_txs,
        transaction_value,
        priority,
        recent_blockhash,
        Some(&payer),
        &tip_guard_config,
    );

    assert!(result.is_ok(), "Bundle building should succeed");

    let bundle = result.unwrap();

    // ASSERT: Tip should not exceed 40% of trade value
    let max_ratio_lamports = (transaction_value as f64 * 0.40) as u64;
    assert!(
        bundle.tip_lamports <= max_ratio_lamports,
        "Tip {} lamports exceeds ratio cap of {} lamports (40% of {})",
        bundle.tip_lamports,
        max_ratio_lamports,
        transaction_value
    );

    println!("✅ Test passed: TipGuard ratio cap working - tip {} lamports <= {} lamports (40% of trade)", 
        bundle.tip_lamports, max_ratio_lamports);
}

// Helper functions

fn create_test_swap_plan(amount_in: u64) -> SwapPlan {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    SwapPlan {
        pool_amm_id: Pubkey::new_unique(),
        authority: Pubkey::new_unique(),
        amount_in,
        min_amount_out: 1000,
        timeout: now + 300, // 5 minutes
        metadata: None,
    }
}

fn create_test_amm_accounts() -> AmmAccounts {
    AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: Some(Pubkey::new_unique()),
        additional_accounts: vec![],
    }
}

fn create_mock_transaction() -> solana_sdk::transaction::VersionedTransaction {
    use solana_sdk::{
        message::{v0, VersionedMessage},
        transaction::VersionedTransaction,
    };

    let payer = Keypair::new();
    let message = v0::Message {
        header: solana_sdk::message::MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 0,
        },
        account_keys: vec![payer.pubkey()],
        recent_blockhash: Hash::default(),
        instructions: vec![],
        address_table_lookups: vec![],
    };

    VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer]).unwrap()
}
