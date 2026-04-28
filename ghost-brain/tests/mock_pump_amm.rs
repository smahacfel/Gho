//! Mock Pump.fun AMM Tests for Devnet E2E
//!
//! This module provides mock testing for DirectBuyBuilder against Pump.fun AMM
//! without requiring actual on-chain deployment on Devnet.
//!
//! ## Purpose
//! Since Pump.fun may not have a deployment on Devnet, this mock test validates:
//! 1. DirectBuyBuilder instruction building (byte-level)
//! 2. Discriminator verification (SHA256("global:buy")[..8])
//! 3. Account structure and ordering
//! 4. Data serialization format
//!
//! ## Test Flow
//! 1. Build raw buy instruction using DirectBuyBuilder
//! 2. Verify instruction structure matches Pump.fun IDL expectations
//! 3. Validate account ordering and permissions
//! 4. Confirm data layout (discriminator + amount + max_sol_cost)

use solana_sdk::pubkey::Pubkey;
use trigger::DirectBuyBuilder;

/// Mock Pump.fun program for testing instruction deserialization
mod mock_pump_program {
    use solana_sdk::pubkey::Pubkey;

    /// Expected buy discriminator from Pump.fun IDL
    /// This is SHA256("global:buy")[..8]
    pub const EXPECTED_BUY_DISCRIMINATOR: [u8; 8] =
        [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];

    /// Expected number of accounts for buy instruction
    pub const EXPECTED_ACCOUNT_COUNT: usize = 12;

    /// Account indices for Pump.fun buy instruction
    pub mod account_indices {
        pub const GLOBAL: usize = 0;
        pub const FEE_RECIPIENT: usize = 1;
        pub const MINT: usize = 2;
        pub const BONDING_CURVE: usize = 3;
        pub const ASSOCIATED_BONDING_CURVE: usize = 4;
        pub const ASSOCIATED_USER: usize = 5;
        pub const USER: usize = 6;
        pub const SYSTEM_PROGRAM: usize = 7;
        pub const TOKEN_PROGRAM: usize = 8;
        pub const RENT: usize = 9;
        pub const ASSOC_TOKEN_PROGRAM: usize = 10;
        pub const EVENT_AUTHORITY: usize = 11;
    }

    /// Deserialize buy instruction data (mock)
    ///
    /// Layout: [discriminator (8 bytes), amount (8 bytes), max_sol_cost (8 bytes)]
    pub fn deserialize_buy_data(data: &[u8]) -> Result<(u64, u64), &'static str> {
        if data.len() != 24 {
            return Err("Invalid data length: expected 24 bytes");
        }

        // Check discriminator
        let discriminator = &data[0..8];
        if discriminator != EXPECTED_BUY_DISCRIMINATOR {
            return Err("Invalid discriminator: does not match SHA256('global:buy')[..8]");
        }

        // Parse amount (tokens to receive) - little-endian u64
        let amount = u64::from_le_bytes(data[8..16].try_into().unwrap());

        // Parse max_sol_cost (max SOL to pay) - little-endian u64
        let max_sol_cost = u64::from_le_bytes(data[16..24].try_into().unwrap());

        Ok((amount, max_sol_cost))
    }

    /// Validate account structure for buy instruction
    pub fn validate_accounts(
        accounts: &[solana_sdk::instruction::AccountMeta],
        expected_payer: &Pubkey,
    ) -> Result<(), &'static str> {
        if accounts.len() != EXPECTED_ACCOUNT_COUNT {
            return Err("Invalid account count");
        }

        // User (index 6) should be signer and writable
        if !accounts[account_indices::USER].is_signer {
            return Err("User account must be signer");
        }
        if !accounts[account_indices::USER].is_writable {
            return Err("User account must be writable");
        }
        if accounts[account_indices::USER].pubkey != *expected_payer {
            return Err("User account pubkey mismatch");
        }

        // System program (index 7) should be read-only
        if accounts[account_indices::SYSTEM_PROGRAM].is_signer {
            return Err("System program should not be signer");
        }
        if accounts[account_indices::SYSTEM_PROGRAM].is_writable {
            return Err("System program should not be writable");
        }

        // Bonding curve (index 3) should be writable
        if !accounts[account_indices::BONDING_CURVE].is_writable {
            return Err("Bonding curve must be writable");
        }

        // Associated accounts should be writable
        if !accounts[account_indices::ASSOCIATED_BONDING_CURVE].is_writable {
            return Err("Associated bonding curve must be writable");
        }
        if !accounts[account_indices::ASSOCIATED_USER].is_writable {
            return Err("Associated user must be writable");
        }

        Ok(())
    }
}

/// Test that DirectBuyBuilder produces a valid Pump.fun buy instruction
#[test]
fn test_direct_buy_builder_produces_valid_instruction() {
    let payer = Pubkey::new_unique();
    let token_mint = Pubkey::new_unique();
    let amount_sol_in = 1_000_000_000u64; // 1 SOL
    let min_tokens_out = 24_000_000u64; // 24M tokens

    // Build the instruction
    let ix = DirectBuyBuilder::build_buy_ix(&payer, &token_mint, amount_sol_in, min_tokens_out);

    // Validate program ID
    assert_eq!(
        ix.program_id,
        DirectBuyBuilder::pump_program_id(),
        "Program ID should match Pump.fun program"
    );

    // Validate account count
    assert_eq!(
        ix.accounts.len(),
        mock_pump_program::EXPECTED_ACCOUNT_COUNT,
        "Should have exactly 12 accounts for Pump.fun buy"
    );

    // Validate account structure
    mock_pump_program::validate_accounts(&ix.accounts, &payer)
        .expect("Account structure should be valid");

    // Validate data can be deserialized
    let (amount, max_sol_cost) = mock_pump_program::deserialize_buy_data(&ix.data)
        .expect("Buy data should deserialize correctly");

    assert_eq!(amount, min_tokens_out, "Amount should match min_tokens_out");
    assert_eq!(
        max_sol_cost, amount_sol_in,
        "Max SOL cost should match amount_sol_in"
    );
}

/// Test discriminator verification
#[test]
fn test_discriminator_matches_pump_fun_idl() {
    // Verify that DirectBuyBuilder's discriminator matches expected value
    assert!(
        DirectBuyBuilder::verify_discriminator(),
        "Discriminator should match SHA256('global:buy')[..8]"
    );

    // Also verify the raw bytes
    let discriminator = DirectBuyBuilder::get_discriminator();
    assert_eq!(
        discriminator,
        mock_pump_program::EXPECTED_BUY_DISCRIMINATOR,
        "Discriminator bytes should match exactly"
    );
}

/// Test PDA derivation for bonding curve
#[test]
fn test_bonding_curve_pda_derivation() {
    let token_mint = Pubkey::new_unique();

    // Derive bonding curve PDA
    let (bonding_curve, bump) = DirectBuyBuilder::derive_bonding_curve(&token_mint);

    // Verify it's a valid PDA (bump should be valid)
    assert!(bump <= 255, "Bump should be valid");
    assert_ne!(
        bonding_curve,
        Pubkey::default(),
        "Bonding curve should not be default pubkey"
    );

    // Verify deterministic derivation
    let (bonding_curve2, bump2) = DirectBuyBuilder::derive_bonding_curve(&token_mint);
    assert_eq!(
        bonding_curve, bonding_curve2,
        "PDA derivation should be deterministic"
    );
    assert_eq!(bump, bump2, "Bump derivation should be deterministic");
}

/// Test global state PDA derivation
#[test]
fn test_global_pda_derivation() {
    let (global, bump) = DirectBuyBuilder::derive_global();

    assert!(bump <= 255, "Bump should be valid");
    assert_ne!(
        global,
        Pubkey::default(),
        "Global should not be default pubkey"
    );

    // Verify deterministic
    let (global2, bump2) = DirectBuyBuilder::derive_global();
    assert_eq!(global, global2, "Global PDA should be deterministic");
    assert_eq!(bump, bump2, "Bump should be deterministic");
}

/// Test token estimation with various inputs
#[test]
fn test_token_estimation_edge_cases() {
    // Zero input should produce zero output
    let (est, min) = DirectBuyBuilder::estimate_tokens_out(0, 0.2);
    assert_eq!(est, 0);
    assert_eq!(min, 0);

    // Very small input (1 lamport)
    // DirectBuyBuilder uses TOKENS_PER_SOL_ESTIMATE (30M tokens/SOL) constant
    // for initial estimation. With 1 lamport input, result is essentially 0.
    let (est, _min) = DirectBuyBuilder::estimate_tokens_out(1, 0.2);
    assert!(est <= 1);

    // Large input (1000 SOL)
    let (est, min) = DirectBuyBuilder::estimate_tokens_out(1_000_000_000_000, 0.2);
    assert!(est > 0);
    assert!(min > 0);
    assert!(
        min < est,
        "Min should be less than estimate due to slippage"
    );

    // Zero slippage
    let (est, min) = DirectBuyBuilder::estimate_tokens_out(1_000_000_000, 0.0);
    assert_eq!(est, min, "With zero slippage, min should equal estimate");

    // 100% slippage (extreme case)
    let (est, min) = DirectBuyBuilder::estimate_tokens_out(1_000_000_000, 1.0);
    assert!(est > 0);
    assert_eq!(min, 0, "With 100% slippage, min should be 0");
}

/// Test instruction data layout
#[test]
fn test_instruction_data_layout() {
    let payer = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let amount_sol = 500_000_000u64; // 0.5 SOL
    let min_tokens = 12_000_000u64; // 12M tokens

    let ix = DirectBuyBuilder::build_buy_ix(&payer, &mint, amount_sol, min_tokens);

    // Verify total data length
    assert_eq!(ix.data.len(), 24, "Instruction data should be 24 bytes");

    // Extract and verify discriminator (bytes 0-7)
    let discriminator: [u8; 8] = ix.data[0..8].try_into().unwrap();
    assert_eq!(discriminator, mock_pump_program::EXPECTED_BUY_DISCRIMINATOR);

    // Extract and verify amount (bytes 8-15, little-endian)
    let extracted_tokens = u64::from_le_bytes(ix.data[8..16].try_into().unwrap());
    assert_eq!(extracted_tokens, min_tokens);

    // Extract and verify max_sol_cost (bytes 16-23, little-endian)
    let extracted_sol = u64::from_le_bytes(ix.data[16..24].try_into().unwrap());
    assert_eq!(extracted_sol, amount_sol);
}

/// Test multiple mints produce different PDAs
#[test]
fn test_different_mints_different_pdas() {
    let mint1 = Pubkey::new_unique();
    let mint2 = Pubkey::new_unique();

    let (bc1, _) = DirectBuyBuilder::derive_bonding_curve(&mint1);
    let (bc2, _) = DirectBuyBuilder::derive_bonding_curve(&mint2);

    assert_ne!(
        bc1, bc2,
        "Different mints should produce different bonding curve PDAs"
    );
}

/// Test E2E scenario simulation
#[tokio::test]
async fn test_e2e_devnet_simulation() {
    // Simulate a complete E2E flow for Devnet testing

    // Step 1: Simulate pool detection
    let detected_pool = Pubkey::new_unique(); // Mock pool address
    let token_mint = Pubkey::new_unique(); // Token being bought

    // Step 2: Simulate Oracle scoring (mock score)
    let oracle_score = 85u8;
    let min_score_threshold = 70u8;
    assert!(
        oracle_score >= min_score_threshold,
        "Pool should pass Oracle threshold"
    );

    // Step 3: Simulate SwapPlan creation
    let amount_sol = 100_000_000u64; // 0.1 SOL
    let (estimated_tokens, min_tokens) = DirectBuyBuilder::estimate_tokens_out(
        amount_sol,
        trigger::direct_buy_builder::DEFAULT_SLIPPAGE_TOLERANCE,
    );

    println!("E2E Simulation:");
    println!("  Pool: {}", detected_pool);
    println!("  Token: {}", token_mint);
    println!("  Oracle Score: {}/100", oracle_score);
    println!("  Amount SOL: {} lamports", amount_sol);
    println!("  Estimated Tokens: {}", estimated_tokens);
    println!("  Min Tokens (20% slippage): {}", min_tokens);

    // Step 4: Build Direct Buy instruction
    let payer = Pubkey::new_unique();
    let buy_ix = DirectBuyBuilder::build_buy_ix(&payer, &token_mint, amount_sol, min_tokens);

    println!("  Instruction Built:");
    println!("    Program: {}", buy_ix.program_id);
    println!("    Accounts: {}", buy_ix.accounts.len());
    println!("    Data: {} bytes", buy_ix.data.len());

    // Step 5: Verify instruction is valid
    assert_eq!(buy_ix.accounts.len(), 12);
    assert_eq!(buy_ix.data.len(), 24);
    assert!(DirectBuyBuilder::verify_discriminator());

    // Step 6: Mock metrics update
    let metrics = ghost_brain::E2EMetrics::new();
    metrics.buy_intents_initialized.inc();
    metrics.trigger_txs_sent.inc();

    // Simulate confirmation
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    metrics.trigger_txs_confirmed.inc();

    println!("  Metrics:");
    println!("    Buy Intents: {}", metrics.buy_intents_initialized.get());
    println!("    Txs Sent: {}", metrics.trigger_txs_sent.get());
    println!("    Txs Confirmed: {}", metrics.trigger_txs_confirmed.get());

    // Verify metrics are incremented
    assert_eq!(metrics.buy_intents_initialized.get(), 1.0);
    assert_eq!(metrics.trigger_txs_sent.get(), 1.0);
    assert_eq!(metrics.trigger_txs_confirmed.get(), 1.0);

    println!("✓ E2E Devnet Simulation Complete");
}

/// Test that buy_intents_initialized and buy_init_failures are properly incremented
#[test]
fn test_prometheus_metrics_incremented() {
    let metrics = ghost_brain::E2EMetrics::new();

    // Initial state
    assert_eq!(metrics.buy_intents_initialized.get(), 0.0);
    assert_eq!(metrics.buy_init_failures.get(), 0.0);

    // Simulate successful buy
    metrics.buy_intents_initialized.inc();
    assert_eq!(metrics.buy_intents_initialized.get(), 1.0);

    // Simulate failed buy
    metrics.buy_init_failures.inc();
    assert_eq!(metrics.buy_init_failures.get(), 1.0);

    // Simulate batch success
    metrics.buy_intents_initialized.inc_by(5.0);
    assert_eq!(metrics.buy_intents_initialized.get(), 6.0);

    // Simulate batch failure
    metrics.buy_init_failures.inc_by(2.0);
    assert_eq!(metrics.buy_init_failures.get(), 3.0);
}

/// Test inclusion rate calculation after Direct Buy
#[test]
fn test_inclusion_rate_after_direct_buy() {
    let metrics = ghost_brain::E2EMetrics::new();

    // Simulate 10 transactions sent
    for _ in 0..10 {
        metrics.trigger_txs_sent.inc();
    }

    // Simulate 9 confirmed (90% inclusion rate)
    for _ in 0..9 {
        metrics.trigger_txs_confirmed.inc();
    }

    // Calculate inclusion rate
    let inclusion_rate = metrics.update_inclusion_rate();

    assert!(
        (inclusion_rate - 90.0).abs() < 0.01,
        "Inclusion rate should be 90%, got {}",
        inclusion_rate
    );
}
