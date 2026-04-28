//! Integration tests for enhanced candidate scoring with contextual analysis

use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::oracle_scoring::{score_enhanced, RiskLevel};
use ghost_core::{
    analyze_transaction, calculate_vanity_score, AmmType, EnhancedAnalysis, TransactionData,
};
use solana_sdk::pubkey::Pubkey;

#[test]
fn test_enhanced_candidate_scoring_flow() {
    // === STEP 1: Simulate transaction data ===

    let accounts = vec![
        Pubkey::new_unique(), // Payer/dev
        Pubkey::new_unique(), // Pool
        Pubkey::new_unique(), // Mint
        Pubkey::new_unique(), // Bonding curve
    ];

    let base_mint = accounts[2];

    // No instructions for simplicity (just testing the flow)
    let tx_data = TransactionData {
        accounts: &accounts,
        num_required_signatures: 1,
        instructions: &[],
    };

    // === STEP 2: Analyze transaction to get enhanced data ===

    let analysis = analyze_transaction(&tx_data, &base_mint, AmmType::PumpFun)
        .expect("Analysis should succeed");

    // === STEP 3: Build EnhancedCandidate ===

    let candidate = EnhancedCandidate::new_with_fields(
        // Hot fields
        Some(12345),                 // slot
        1234567890,                  // timestamp
        10.0,                        // initial_liquidity_sol
        analysis.dev_buy_sol,        // dev_buy_sol
        Some(0.05),                  // bonding_curve_progress
        analysis.vanity_score,       // vanity_score
        analysis.metadata_len_score, // metadata_len_score
        analysis.has_dev_buy,        // has_dev_buy
        analysis.mint_auth_disabled, // mint_auth_disabled
        // Shadow fields
        None, // expected_price
        None, // shadow_bonding_progress
        None, // virtual_sol_reserves
        None, // shadow_market_cap
        // Cold fields
        accounts[1], // pool_amm_id
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap(), // amm_program_id
        accounts[2], // base_mint
        Pubkey::new_unique(), // quote_mint
        accounts[3], // bonding_curve
        "test_sig".to_string(), // signature
        Some(1_000_000_000), // token_total_supply
    );

    // === STEP 4: Score the enhanced candidate ===

    let scored = score_enhanced(&candidate, 70);

    println!("\n=== Enhanced Scoring Test Results ===");
    println!("Vanity Score: {}", candidate.vanity_score);
    println!("Has Dev Buy: {}", candidate.has_dev_buy);
    println!("Dev Buy SOL: {}", candidate.dev_buy_sol);
    println!("Mint Auth Disabled: {}", candidate.mint_auth_disabled);
    println!("Metadata Score: {}", candidate.metadata_len_score);
    println!("Final Score: {}", scored.score);
    println!("Risk Level: {:?}", scored.risk_level);
    println!("Passed: {}", scored.passed);

    // Verify scoring works
    assert!(scored.score <= 100);
}

#[test]
fn test_enhanced_scoring_with_dev_buy_simulation() {
    // Simulate a scenario where dev buys atomically

    let base_mint = Pubkey::new_unique();

    let analysis = EnhancedAnalysis {
        vanity_score: 45,         // Some vanity
        has_dev_buy: true,        // Dev bought!
        dev_buy_sol: 6.5,         // Significant investment
        mint_auth_disabled: true, // Authority disabled
        metadata_len_score: 70,   // Good metadata
    };

    let candidate = EnhancedCandidate::new_with_fields(
        // Hot fields
        Some(12345),                 // slot
        1234567890,                  // timestamp
        15.0,                        // initial_liquidity_sol
        analysis.dev_buy_sol,        // dev_buy_sol
        Some(0.03),                  // bonding_curve_progress
        analysis.vanity_score,       // vanity_score
        analysis.metadata_len_score, // metadata_len_score
        analysis.has_dev_buy,        // has_dev_buy
        analysis.mint_auth_disabled, // mint_auth_disabled
        // Shadow fields
        None, // expected_price
        None, // shadow_bonding_progress
        None, // virtual_sol_reserves
        None, // shadow_market_cap
        // Cold fields
        Pubkey::new_unique(), // pool_amm_id
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap(), // amm_program_id
        base_mint,            // base_mint
        Pubkey::new_unique(), // quote_mint
        Pubkey::new_unique(), // bonding_curve
        "test_sig".to_string(), // signature
        Some(500_000_000),    // token_total_supply
    );

    let scored = score_enhanced(&candidate, 70);

    println!("\n=== Dev Buy Scenario Results ===");
    println!("Final Score: {}", scored.score);
    println!("Risk Level: {:?}", scored.risk_level);
    println!("Passed: {}", scored.passed);

    // With all positive signals, should pass
    assert!(
        scored.passed,
        "Expected to pass with dev buy and good signals"
    );
    assert!(
        scored.score >= 80,
        "Expected high score with optimal conditions"
    );
    assert!(matches!(
        scored.risk_level,
        RiskLevel::Low | RiskLevel::Medium
    ));
}

#[test]
fn test_enhanced_scoring_scam_detection() {
    // Simulate a scam scenario

    let base_mint = Pubkey::new_unique();

    let analysis = EnhancedAnalysis {
        vanity_score: 0,    // Random address
        has_dev_buy: false, // No dev buy (red flag)
        dev_buy_sol: 0.0,
        mint_auth_disabled: false, // Authority still active (red flag)
        metadata_len_score: 15,    // Poor metadata
    };

    let candidate = EnhancedCandidate::new_with_fields(
        // Hot fields
        Some(12345),                 // slot
        1234567890,                  // timestamp
        0.5,                         // initial_liquidity_sol
        analysis.dev_buy_sol,        // dev_buy_sol
        Some(0.95),                  // bonding_curve_progress
        analysis.vanity_score,       // vanity_score
        analysis.metadata_len_score, // metadata_len_score
        analysis.has_dev_buy,        // has_dev_buy
        analysis.mint_auth_disabled, // mint_auth_disabled
        // Shadow fields
        None, // expected_price
        None, // shadow_bonding_progress
        None, // virtual_sol_reserves
        None, // shadow_market_cap
        // Cold fields
        Pubkey::new_unique(), // pool_amm_id
        "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
            .parse()
            .unwrap(), // amm_program_id (Raydium)
        base_mint,            // base_mint
        Pubkey::new_unique(), // quote_mint
        Pubkey::new_unique(), // bonding_curve
        "test_sig".to_string(), // signature
        Some(50_000_000_000), // token_total_supply
    );

    let scored = score_enhanced(&candidate, 70);

    println!("\n=== Scam Detection Results ===");
    println!("Final Score: {}", scored.score);
    println!("Risk Level: {:?}", scored.risk_level);
    println!("Passed: {}", scored.passed);

    // Should be detected as scam
    assert!(!scored.passed, "Expected to fail scam detection");
    assert_eq!(scored.risk_level, RiskLevel::VeryHigh);
    assert_eq!(
        scored.score, 0,
        "Raydium with active mint auth should be hard rejected"
    );
}

#[test]
fn test_enhanced_scoring_memetic_liquidity_bonus() {
    // Test the memetic liquidity value detection

    let base_mint = Pubkey::new_unique();
    let pool_amm_id = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let bonding_curve = Pubkey::new_unique();

    let candidate1 = EnhancedCandidate::new_with_fields(
        // Hot fields
        Some(12345), // slot
        1234567890,  // timestamp
        4.2069,      // initial_liquidity_sol - Memetic value!
        0.0,         // dev_buy_sol
        Some(0.05),  // bonding_curve_progress
        30,          // vanity_score
        50,          // metadata_len_score
        false,       // has_dev_buy
        true,        // mint_auth_disabled
        // Shadow fields
        None, // expected_price
        None, // shadow_bonding_progress
        None, // virtual_sol_reserves
        None, // shadow_market_cap
        // Cold fields
        pool_amm_id, // pool_amm_id
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap(), // amm_program_id
        base_mint,   // base_mint
        quote_mint,  // quote_mint
        bonding_curve, // bonding_curve
        "test_sig".to_string(), // signature
        Some(1_000_000_000), // token_total_supply
    );

    let candidate2 = EnhancedCandidate::new_with_fields(
        // Hot fields
        Some(12345), // slot
        1234567890,  // timestamp
        4.0,         // initial_liquidity_sol - Round number (lazy script)
        0.0,         // dev_buy_sol
        Some(0.05),  // bonding_curve_progress
        30,          // vanity_score
        50,          // metadata_len_score
        false,       // has_dev_buy
        true,        // mint_auth_disabled
        // Shadow fields
        None, // expected_price
        None, // shadow_bonding_progress
        None, // virtual_sol_reserves
        None, // shadow_market_cap
        // Cold fields
        pool_amm_id, // pool_amm_id
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap(), // amm_program_id
        base_mint,   // base_mint
        quote_mint,  // quote_mint
        bonding_curve, // bonding_curve
        "test_sig".to_string(), // signature
        Some(1_000_000_000), // token_total_supply
    );

    let scored1 = score_enhanced(&candidate1, 70);
    let scored2 = score_enhanced(&candidate2, 70);

    println!("\n=== Memetic Liquidity Test ===");
    println!("Memetic (4.2069) Score: {}", scored1.score);
    println!("Lazy (4.0) Score: {}", scored2.score);

    // Memetic value should get bonus vs lazy script penalty
    assert!(
        scored1.score > scored2.score,
        "Memetic value should score higher than lazy script value"
    );
}

#[test]
fn test_vanity_score_integration() {
    // Test vanity score calculation as part of the flow

    let mint1 = Pubkey::new_unique(); // Random
    let vanity1 = calculate_vanity_score(&mint1);

    println!("\n=== Vanity Score Test ===");
    println!("Random mint vanity score: {}", vanity1);

    // Random mints should have relatively low vanity scores
    assert!(vanity1 < 60, "Random mint should have low vanity score");

    // Score should be in valid range
    assert!(vanity1 <= 100);
}

#[test]
fn test_performance_scoring_speed() {
    // Quick performance check for scoring
    use std::time::Instant;

    let candidate = EnhancedCandidate::new_with_fields(
        // Hot fields
        Some(12345), // slot
        1234567890,  // timestamp
        10.0,        // initial_liquidity_sol
        2.5,         // dev_buy_sol
        Some(0.05),  // bonding_curve_progress
        30,          // vanity_score
        60,          // metadata_len_score
        true,        // has_dev_buy
        true,        // mint_auth_disabled
        // Shadow fields
        None, // expected_price
        None, // shadow_bonding_progress
        None, // virtual_sol_reserves
        None, // shadow_market_cap
        // Cold fields
        Pubkey::new_unique(), // pool_amm_id
        "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap(), // amm_program_id
        Pubkey::new_unique(), // base_mint
        Pubkey::new_unique(), // quote_mint
        Pubkey::new_unique(), // bonding_curve
        "test_sig".to_string(), // signature
        Some(1_000_000_000),  // token_total_supply
    );

    // Warm up
    for _ in 0..100 {
        let _ = score_enhanced(&candidate, 70);
    }

    // Measure
    let iterations = 10_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = score_enhanced(&candidate, 70);
    }

    let elapsed = start.elapsed();
    let avg_micros = elapsed.as_micros() / iterations as u128;

    println!("\n=== Performance Test ===");
    println!("Iterations: {}", iterations);
    println!("Total time: {:?}", elapsed);
    println!("Average time per score: {} μs", avg_micros);

    // Target is <20ms, so we should be well under that even in debug mode
    // In practice, each score should be <1μs in release mode
    assert!(
        avg_micros < 1000,
        "Scoring should be very fast (<1ms), got {} μs",
        avg_micros
    );
}
