//! Integration tests for SELL Business Logic
//!
//! These tests validate the complete flow of:
//! 1. Defining TP/panic targets based on entry price
//! 2. Building SELL transactions with min_sol_output
//! 3. Integrating with price oracle
//! 4. Firing bullets when price targets are reached

use solana_sdk::{hash::Hash, pubkey::Pubkey, signature::Keypair};
use std::collections::HashMap;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::net::UdpSocket;
use trigger::{
    AmmProtocol, Bullet, PositionPriceTargets, PriceFeedIntegration, PriceOracleProvider, Result,
    Revolver, SellTxBuilder, SellTxConfig, TpPanicConfig, TriggerError,
};

/// Mock price oracle for testing
struct MockPriceOracle {
    prices: StdRwLock<HashMap<Pubkey, u64>>,
}

impl MockPriceOracle {
    fn new() -> Self {
        Self {
            prices: StdRwLock::new(HashMap::new()),
        }
    }

    fn set_price(&self, mint: Pubkey, price: u64) {
        self.prices.write().unwrap().insert(mint, price);
    }
}

#[async_trait::async_trait]
impl PriceOracleProvider for MockPriceOracle {
    async fn get_current_price(&self, mint: &Pubkey) -> Result<u64> {
        self.prices
            .read()
            .unwrap()
            .get(mint)
            .copied()
            .ok_or_else(|| TriggerError::Other(format!("No price for mint: {}", mint)))
    }
}

#[test]
fn test_tp_panic_target_calculation() {
    // Test case 1: Default configuration
    let entry_price = 1_000_000_000_000; // 1 SOL per token (1e9-scaled)
    let config = TpPanicConfig::default();
    let targets = PositionPriceTargets::new(entry_price, config);

    // Verify TP1 = entry * 1.2
    assert_eq!(targets.tp1_target_price, 1_200_000_000_000);
    // Verify TP2 = entry * 2.0
    assert_eq!(targets.tp2_target_price, 2_000_000_000_000);
    // Verify panic = entry * 0.8
    assert_eq!(targets.panic_target_price, 800_000_000_000);

    println!("✓ TP/Panic targets calculated correctly:");
    println!("  Entry:  {} lamports", targets.entry_price);
    println!("  TP1:    {} lamports (+20%)", targets.tp1_target_price);
    println!("  TP2:    {} lamports (+100%)", targets.tp2_target_price);
    println!("  Panic:  {} lamports (-20%)", targets.panic_target_price);
}

#[test]
fn test_min_sol_output_calculation() {
    // Test calculating min_sol_output with 1e9-scaled price contract
    let token_amount = 1_000_000; // 1 whole token in raw units
    let target_price = 1_000_000_000_000; // 1 SOL per token (1e9-scaled)
    let slippage_bps = 100; // 1% slippage

    let min_output =
        SellTxBuilder::calculate_min_output(token_amount, target_price, slippage_bps).unwrap();

    // Expected: 1 SOL * 0.99 = 0.99 SOL
    assert_eq!(min_output, 990_000_000);

    println!("✓ Min SOL output calculation:");
    println!("  Token amount:  {}", token_amount);
    println!("  Target price:  {} lamports/token", target_price);
    println!("  Slippage:      {}%", slippage_bps as f64 / 100.0);
    println!("  Min output:    {} lamports", min_output);
}

#[tokio::test]
async fn test_sell_tx_building() {
    // Create builder
    let payer = Keypair::new();
    let config = SellTxConfig::default();
    let builder = SellTxBuilder::new(payer, config);

    // Build SELL transaction
    let mint = Pubkey::new_unique();
    let amount_in = 1_000_000; // 1M tokens to sell
    let min_sol_output = 990_000; // Minimum SOL expected
    let blockhash = Hash::default();

    let result = builder
        .build_signed_sell_tx(
            mint,
            Some(Pubkey::new_unique()),
            amount_in,
            min_sol_output,
            blockhash,
            AmmProtocol::PumpFun,
        )
        .await;

    assert!(result.is_ok());
    let tx_bytes = result.unwrap();
    assert!(!tx_bytes.is_empty());

    println!("✓ SELL transaction built successfully:");
    println!("  Mint:          {}", mint);
    println!("  Amount in:     {} tokens", amount_in);
    println!("  Min SOL out:   {} lamports", min_sol_output);
    println!("  TX size:       {} bytes", tx_bytes.len());
}

#[tokio::test]
async fn test_complete_sell_flow() {
    // Simulate a complete BUY -> TP1 -> TP2 scenario

    // 1. BUY executed at entry price
    let entry_price = 1_000_000_000_000; // 1 SOL per token (1e9-scaled)
    let position_size = 10_000_000; // 10M tokens
    let mint = Pubkey::new_unique();

    println!("\n=== Complete SELL Flow Test ===");
    println!("1. BUY executed:");
    println!("   Entry price:   {} lamports/token", entry_price);
    println!("   Position size: {} tokens", position_size);

    // 2. Calculate TP targets
    let config = TpPanicConfig::default();
    let targets = PositionPriceTargets::new(entry_price, config.clone());

    println!("\n2. TP targets calculated:");
    println!(
        "   TP1 target:    {} lamports (+20%)",
        targets.tp1_target_price
    );
    println!(
        "   TP2 target:    {} lamports (+100%)",
        targets.tp2_target_price
    );
    println!(
        "   Panic target:  {} lamports (-20%)",
        targets.panic_target_price
    );

    // 3. Create bullets for each target
    let mut revolver = Revolver::new();
    let payer = Keypair::new();
    let sell_config = SellTxConfig::default();
    let builder = SellTxBuilder::new(payer, sell_config);

    // Create TP1 bullet (25% of position)
    let tp1_amount = position_size / 4;
    let tp1_min_output = SellTxBuilder::calculate_min_output(
        tp1_amount,
        targets.tp1_target_price,
        100, // 1% slippage
    )
    .unwrap();
    let tp1_tx = builder
        .build_signed_sell_tx(
            mint,
            Some(Pubkey::new_unique()),
            tp1_amount,
            tp1_min_output,
            Hash::default(),
            AmmProtocol::PumpFun,
        )
        .await
        .unwrap();
    let tp1_bullet = Bullet::new(tp1_tx, targets.tp1_target_price, 2500).unwrap();

    // Create TP2 bullet (remaining 75%)
    let tp2_amount = position_size * 3 / 4;
    let tp2_min_output =
        SellTxBuilder::calculate_min_output(tp2_amount, targets.tp2_target_price, 100).unwrap();
    let tp2_tx = builder
        .build_signed_sell_tx(
            mint,
            Some(Pubkey::new_unique()),
            tp2_amount,
            tp2_min_output,
            Hash::default(),
            AmmProtocol::PumpFun,
        )
        .await
        .unwrap();
    let tp2_bullet = Bullet::new(tp2_tx, targets.tp2_target_price, 7500).unwrap();

    // Load bullets into revolver
    revolver.load_magazine(mint, vec![tp1_bullet, tp2_bullet]);

    println!("\n3. Bullets created and loaded:");
    println!(
        "   TP1 bullet: {} tokens @ {} lamports",
        tp1_amount, targets.tp1_target_price
    );
    println!(
        "   TP2 bullet: {} tokens @ {} lamports",
        tp2_amount, targets.tp2_target_price
    );

    // 4. Simulate price movement to TP1
    let oracle = Arc::new(MockPriceOracle::new());
    let current_price = targets.tp1_target_price + 100_000_000; // Price rises to TP1+
    oracle.set_price(mint, current_price);

    println!("\n4. Price rises to TP1:");
    println!("   Current price: {} lamports", current_price);
    println!("   PnL: +{:.2}%", targets.get_pnl_percentage(current_price));

    // 5. Try to fire bullets via price feed
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let leader_addr = "127.0.0.1:8001".parse().unwrap();
    let price_feed = PriceFeedIntegration::new(oracle.clone(), socket.clone(), leader_addr);

    let fired_count = price_feed
        .try_fire_revolver_for_price(&mut revolver, mint, current_price)
        .await
        .unwrap();

    println!("\n5. Bullets fired:");
    println!("   Fired count: {}", fired_count);
    println!("   Remaining bullets: {}", revolver.total_bullet_count());

    // Should fire TP1 bullet only
    assert_eq!(fired_count, 1);
    assert_eq!(revolver.total_bullet_count(), 1); // TP2 remains

    // 6. Simulate price movement to TP2
    let current_price = targets.tp2_target_price + 200_000_000;
    oracle.set_price(mint, current_price);

    println!("\n6. Price rises to TP2:");
    println!("   Current price: {} lamports", current_price);
    println!("   PnL: +{:.2}%", targets.get_pnl_percentage(current_price));

    let fired_count = price_feed
        .try_fire_revolver_for_price(&mut revolver, mint, current_price)
        .await
        .unwrap();

    println!("\n7. Final bullets fired:");
    println!("   Fired count: {}", fired_count);
    println!("   Remaining bullets: {}", revolver.total_bullet_count());

    // Should fire TP2 bullet
    assert_eq!(fired_count, 1);
    assert_eq!(revolver.total_bullet_count(), 0); // All bullets fired

    println!("\n✓ Complete SELL flow successful!");
    println!("  Position fully closed at TP1 and TP2");
}

#[tokio::test]
async fn test_panic_sell_calculation() {
    // Test panic/stop loss price calculation and validation
    // Note: Current Bullet::should_fire() uses >= for take-profit bullets.
    // For panic/stop-loss, custom logic would be needed to fire when price <= target.

    let entry_price = 1_000_000_000_000;
    let position_size = 10_000_000;
    let mint = Pubkey::new_unique();

    println!("\n=== Panic SELL Calculation Test ===");
    println!("Entry price:   {} lamports/token", entry_price);
    println!("Position size: {} tokens", position_size);

    // Calculate targets
    let config = TpPanicConfig::default();
    let targets = PositionPriceTargets::new(entry_price, config);

    println!(
        "Panic target:  {} lamports (-20%)",
        targets.panic_target_price
    );

    // Build panic transaction with proper min_sol_output
    let payer = Keypair::new();
    let builder = SellTxBuilder::new(payer, SellTxConfig::default());

    let panic_min_output = SellTxBuilder::calculate_min_output(
        position_size,
        targets.panic_target_price,
        200, // 2% slippage for panic sells
    )
    .unwrap();

    println!("\nPanic bullet configuration:");
    println!("  Amount:        {} tokens", position_size);
    println!("  Target price:  {} lamports", targets.panic_target_price);
    println!(
        "  Min SOL out:   {} lamports (with 2% slippage)",
        panic_min_output
    );

    let panic_tx = builder
        .build_signed_sell_tx(
            mint,
            Some(Pubkey::new_unique()),
            position_size,
            panic_min_output,
            Hash::default(),
            AmmProtocol::PumpFun,
        )
        .await
        .unwrap();

    assert!(!panic_tx.is_empty());

    // Test panic detection
    let current_price = targets.panic_target_price - 50_000_000; // Price drops below panic
    println!("\nSimulated price drop:");
    println!("  Current price: {} lamports", current_price);
    println!("  PnL: {:.2}%", targets.get_pnl_percentage(current_price));
    println!(
        "  Panic triggered: {}",
        targets.has_hit_panic(current_price)
    );

    assert!(targets.has_hit_panic(current_price));

    println!("\n✓ Panic SELL calculation and detection working correctly!");
    println!("  Note: Actual firing would require separate logic for stop-loss bullets");
    println!("        (current Bullet::should_fire uses >= for take-profit only)");
}

#[test]
fn test_aggressive_vs_conservative_strategies() {
    let entry_price = 1_000_000_000_000;

    // Conservative strategy
    let conservative = TpPanicConfig::conservative();
    let conservative_targets = PositionPriceTargets::new(entry_price, conservative);

    println!("\n=== Strategy Comparison ===");
    println!("Entry price: {} lamports\n", entry_price);

    println!("Conservative (lower risk):");
    println!(
        "  TP1:   {} lamports (+10%)",
        conservative_targets.tp1_target_price
    );
    println!(
        "  TP2:   {} lamports (+50%)",
        conservative_targets.tp2_target_price
    );
    println!(
        "  Panic: {} lamports (-10%)",
        conservative_targets.panic_target_price
    );

    // Aggressive strategy
    let aggressive = TpPanicConfig::aggressive();
    let aggressive_targets = PositionPriceTargets::new(entry_price, aggressive);

    println!("\nAggressive (higher risk):");
    println!(
        "  TP1:   {} lamports (+50%)",
        aggressive_targets.tp1_target_price
    );
    println!(
        "  TP2:   {} lamports (+200%)",
        aggressive_targets.tp2_target_price
    );
    println!(
        "  Panic: {} lamports (-30%)",
        aggressive_targets.panic_target_price
    );

    // Verify conservative has tighter targets
    assert!(conservative_targets.tp1_target_price < aggressive_targets.tp1_target_price);
    assert!(conservative_targets.panic_target_price > aggressive_targets.panic_target_price);
}
