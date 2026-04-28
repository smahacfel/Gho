//! Integration tests for Ghost Transaction Builder
//!
//! These tests validate the complete flow of building, pre-signing,
//! and validating Ghost transactions with various configurations.

use ghost_core::SwapPlanBuilder;
use solana_sdk::{hash::Hash, pubkey::Pubkey, signature::Keypair, signer::Signer};
use trigger::{AmmAccounts, AmmType, GhostTransactionBuilder, LutConfig};

/// Test complete flow: build swap plan -> create builder -> presign -> validate
/// Uses a mock LUT to ensure transaction size stays under 400 bytes
#[test]
fn test_complete_presign_flow() {
    // Setup
    let payer = Keypair::new();
    let config = LutConfig::new();

    // Create a valid swap plan
    let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(5_000_000) // 5M lamports = 0.005 SOL
        .min_amount_out(4_500_000) // 10% slippage tolerance
        .timeout_seconds(300) // 5 minutes
        .with_score(85)
        .with_strategy("pump_fun_snipe")
        .build()
        .expect("Valid swap plan");

    // Create AMM accounts
    let amm_accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: Some(Pubkey::new_unique()),
        additional_accounts: vec![],
    };

    // Create a mock LUT address
    let mock_lut_key = Pubkey::new_unique();

    // Create mock LUT account with static addresses
    let mock_lut_account = GhostTransactionBuilder::create_mock_lut_account(mock_lut_key);

    // Build Ghost transaction with mock LUT
    let builder = GhostTransactionBuilder::new(swap_plan.clone(), AmmType::PumpFun, amm_accounts)
        .with_static_lut(mock_lut_account);

    // Pre-sign the transaction
    let blockhash = Hash::default();
    let presigned = builder
        .presign_initialize_intent_tx(&payer, blockhash)
        .expect("Pre-signing should succeed");

    // Validate presigned transaction
    assert!(presigned.size_bytes > 0);
    // With LUT compression, transaction should be under 400 bytes (target: 250-300 bytes)
    assert!(
        presigned.size_bytes < 400,
        "Transaction size {} bytes exceeds limit of 400 bytes. With LUT, expected ~250-300 bytes",
        presigned.size_bytes
    );
    assert_eq!(presigned.blockhash, blockhash);
    assert!(!presigned.signature().is_empty());

    // Check validity
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(presigned.is_valid(now));

    println!("✓ Complete presign flow successful");
    println!(
        "  Transaction size: {} bytes (with LUT compression)",
        presigned.size_bytes
    );
    println!("  Signature: {}", presigned.signature());
}

/// Test building transactions for both Pump.fun and Bonk.fun
#[test]
fn test_multiple_amm_types() {
    let payer = Keypair::new();
    let config = LutConfig::new();

    // Test Pump.fun
    let pump_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(1_000_000)
        .min_amount_out(900_000)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let pump_accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: None,
        additional_accounts: vec![],
    };

    let pump_builder = GhostTransactionBuilder::new(pump_plan, AmmType::PumpFun, pump_accounts);

    let pump_tx = pump_builder
        .build_initialize_intent_tx(&payer, Hash::default())
        .expect("Pump.fun transaction should build");

    // Test Bonk.fun
    let bonk_plan = SwapPlanBuilder::new(payer.pubkey(), config.bonk_fun.program_id)
        .amount_in(1_000_000)
        .min_amount_out(900_000)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let bonk_accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: None,
        additional_accounts: vec![],
    };

    let bonk_builder = GhostTransactionBuilder::new(bonk_plan, AmmType::BonkFun, bonk_accounts);

    let bonk_tx = bonk_builder
        .build_initialize_intent_tx(&payer, Hash::default())
        .expect("Bonk.fun transaction should build");

    // Both transactions should be valid
    assert!(!pump_tx.signatures.is_empty());
    assert!(!bonk_tx.signatures.is_empty());

    println!("✓ Both AMM types build successfully");
}

/// Test validation catches invalid swap plans
#[test]
fn test_validation_errors() {
    let payer = Keypair::new();
    let config = LutConfig::new();

    // Test case 1: Amount too small
    let invalid_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(100) // Below minimum of 1000
        .min_amount_out(90)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: None,
        additional_accounts: vec![],
    };

    let builder = GhostTransactionBuilder::new(invalid_plan, AmmType::PumpFun, accounts.clone());
    let result = builder.build_initialize_intent_tx(&payer, Hash::default());
    assert!(result.is_err(), "Should reject amount below minimum");

    // Test case 2: Zero min_amount_out
    let invalid_plan2 = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(1_000_000)
        .min_amount_out(0) // Zero output
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let builder2 = GhostTransactionBuilder::new(invalid_plan2, AmmType::PumpFun, accounts.clone());
    let result2 = builder2.build_initialize_intent_tx(&payer, Hash::default());
    assert!(result2.is_err(), "Should reject zero min_amount_out");

    // Test case 3: Invalid pool (not whitelisted)
    let invalid_plan3 = SwapPlanBuilder::new(payer.pubkey(), Pubkey::new_unique())
        .amount_in(1_000_000)
        .min_amount_out(900_000)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let builder3 = GhostTransactionBuilder::new(invalid_plan3, AmmType::PumpFun, accounts);
    let result3 = builder3.build_initialize_intent_tx(&payer, Hash::default());
    assert!(result3.is_err(), "Should reject non-whitelisted pool");

    println!("✓ Validation correctly rejects invalid inputs");
}

/// Test LUT address retrieval
#[test]
fn test_lut_address_management() {
    let payer = Keypair::new();
    let config = LutConfig::new();

    let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(1_000_000)
        .min_amount_out(900_000)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: None,
        additional_accounts: vec![],
    };

    let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, accounts);

    // Check LUT addresses
    let lut_addresses = builder.get_lut_addresses();
    assert!(!lut_addresses.is_empty(), "Should have LUT addresses");
    assert!(
        lut_addresses.len() >= 10,
        "Should have at least 10 addresses"
    );

    // Verify key addresses are included
    let lut_set: std::collections::HashSet<_> = lut_addresses.iter().collect();
    assert!(
        lut_set.contains(&config.pump_fun.program_id),
        "Should include AMM program"
    );
    assert!(
        lut_set.contains(&config.mints.sol),
        "Should include SOL mint"
    );
    assert!(
        lut_set.contains(&config.system_programs.token_program),
        "Should include token program"
    );

    println!("✓ LUT addresses properly managed");
    println!("  Total LUT addresses: {}", lut_addresses.len());
}

/// Test transaction validity window
#[test]
fn test_presigned_validity_window() {
    let payer = Keypair::new();
    let config = LutConfig::new();

    let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(1_000_000)
        .min_amount_out(900_000)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: None,
        additional_accounts: vec![],
    };

    let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, accounts);
    let presigned = builder
        .presign_initialize_intent_tx(&payer, Hash::default())
        .expect("Presigning should succeed");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Should be valid now
    assert!(presigned.is_valid(now));
    assert!(presigned.is_valid(now + 30)); // Still valid after 30 seconds
    assert!(presigned.is_valid(now + 59)); // Still valid after 59 seconds
    assert!(!presigned.is_valid(now + 61)); // Invalid after 61 seconds
    assert!(!presigned.is_valid(now + 120)); // Invalid after 2 minutes

    println!("✓ Presigned transaction validity window working correctly");
}

/// Test building full swap transaction (initialize + execute)
#[test]
fn test_full_swap_transaction() {
    let payer = Keypair::new();
    let config = LutConfig::new();

    let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(1_000_000)
        .min_amount_out(900_000)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: Some(Pubkey::new_unique()),
        additional_accounts: vec![],
    };

    let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, accounts);

    // Build full swap transaction (both initialize and execute)
    let full_tx = builder
        .build_full_swap_tx(&payer, Hash::default())
        .expect("Full swap transaction should build");

    assert!(!full_tx.signatures.is_empty());

    // Full transaction should be larger than initialize-only
    let init_tx = builder
        .build_initialize_intent_tx(&payer, Hash::default())
        .expect("Initialize transaction should build");

    let full_size = bincode::serialize(&full_tx).unwrap().len();
    let init_size = bincode::serialize(&init_tx).unwrap().len();

    assert!(
        full_size > init_size,
        "Full transaction should be larger than initialize-only"
    );

    println!("✓ Full swap transaction builds successfully");
    println!("  Initialize-only size: {} bytes", init_size);
    println!("  Full swap size: {} bytes", full_size);
}

/// Test that LUT compression significantly reduces transaction size
#[test]
fn test_lut_compression_size_reduction() {
    let payer = Keypair::new();
    let config = LutConfig::new();

    let swap_plan = SwapPlanBuilder::new(payer.pubkey(), config.pump_fun.program_id)
        .amount_in(1_000_000)
        .min_amount_out(900_000)
        .timeout_seconds(3600)
        .build()
        .unwrap();

    let amm_accounts = AmmAccounts {
        pool: Pubkey::new_unique(),
        amm_program_id: None,
        bonding_curve: Some(Pubkey::new_unique()),
        additional_accounts: vec![],
    };

    // Build transaction WITHOUT LUT
    let builder_no_lut =
        GhostTransactionBuilder::new(swap_plan.clone(), AmmType::PumpFun, amm_accounts.clone());

    let tx_no_lut = builder_no_lut
        .build_initialize_intent_tx(&payer, Hash::default())
        .expect("Transaction without LUT should build");
    let size_no_lut = bincode::serialize(&tx_no_lut).unwrap().len();

    // Build transaction WITH LUT
    let mock_lut_key = Pubkey::new_unique();
    let mock_lut_account = GhostTransactionBuilder::create_mock_lut_account(mock_lut_key);

    let builder_with_lut = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts)
        .with_static_lut(mock_lut_account);

    let tx_with_lut = builder_with_lut
        .build_initialize_intent_tx(&payer, Hash::default())
        .expect("Transaction with LUT should build");
    let size_with_lut = bincode::serialize(&tx_with_lut).unwrap().len();

    // LUT should significantly reduce transaction size
    println!("✓ LUT compression test results:");
    println!("  Without LUT: {} bytes", size_no_lut);
    println!("  With LUT:    {} bytes", size_with_lut);
    println!(
        "  Savings:     {} bytes ({:.1}% reduction)",
        size_no_lut - size_with_lut,
        (1.0 - (size_with_lut as f64 / size_no_lut as f64)) * 100.0
    );

    // With LUT, size should be under 400 bytes
    assert!(
        size_with_lut < 400,
        "Transaction with LUT should be under 400 bytes, got {} bytes",
        size_with_lut
    );

    // LUT should save at least 100 bytes
    assert!(
        size_no_lut > size_with_lut + 100,
        "LUT should save at least 100 bytes, but only saved {} bytes",
        size_no_lut.saturating_sub(size_with_lut)
    );
}

/// Test that get_static_lut_addresses returns the expected addresses
#[test]
fn test_static_lut_addresses() {
    use solana_sdk::system_program;
    use std::str::FromStr;

    // Token Program ID constant (well-known SPL Token program)
    const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    // Pump.fun Program ID constant
    const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    let addresses = LutConfig::get_static_lut_addresses();

    // Should have at least 7 addresses (system programs + Pump.fun specific)
    assert!(
        addresses.len() >= 7,
        "Expected at least 7 static LUT addresses, got {}",
        addresses.len()
    );

    // Verify key addresses are included using SDK constants where available
    let system_program_id = system_program::id();
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).unwrap();

    assert!(
        addresses.contains(&system_program_id),
        "Should contain System Program"
    );
    assert!(
        addresses.contains(&token_program),
        "Should contain Token Program"
    );
    assert!(
        addresses.contains(&pump_program),
        "Should contain Pump.fun Program"
    );

    println!("✓ Static LUT addresses verified");
    println!("  Total addresses: {}", addresses.len());
    for (i, addr) in addresses.iter().enumerate() {
        println!("    [{}] {}", i, addr);
    }
}
