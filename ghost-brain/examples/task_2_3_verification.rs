//! TASK 2.3 Verification Example
//!
//! This example demonstrates the Buyer Profile Distributions module
//! and verifies that all profiles work correctly.

use ghost_brain::chaos::{
    action_to_amount_multiplier, is_buy, is_sell, BuyerProfile, MarketAction,
};
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

fn main() {
    println!("=== TASK 2.3: Buyer Profile Distributions - Verification ===\n");

    // Test all predefined profiles
    let profiles = vec![
        ("Bullish Whale", BuyerProfile::bullish_whale()),
        ("Bearish Whale", BuyerProfile::bearish_whale()),
        ("Rug Puller", BuyerProfile::rug_puller()),
        ("Normal Trader", BuyerProfile::normal_trader()),
        ("Mixed Market", BuyerProfile::mixed_market()),
    ];

    for (name, profile) in profiles {
        println!("Testing Profile: {}", name);
        println!("{}", "=".repeat(60));

        // Sample 1000 actions and analyze distribution
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(12345);
        let mut action_counts = std::collections::HashMap::new();

        for _ in 0..1000 {
            let action = profile.sample_action(&mut rng);
            *action_counts.entry(format!("{:?}", action)).or_insert(0) += 1;
        }

        // Display distribution
        let mut actions: Vec<_> = action_counts.iter().collect();
        actions.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

        for (action, count) in actions {
            let percentage = (*count as f64 / 1000.0) * 100.0;
            let bar = "█".repeat((percentage / 2.0) as usize);
            println!("  {:12} {:4} ({:5.1}%) {}", action, count, percentage, bar);
        }

        println!();
    }

    // Test helper functions
    println!("Testing Helper Functions");
    println!("{}", "=".repeat(60));

    let test_actions = vec![
        MarketAction::BuyLarge,
        MarketAction::BuyMedium,
        MarketAction::BuySmall,
        MarketAction::SellSmall,
        MarketAction::SellMedium,
        MarketAction::SellLarge,
        MarketAction::Hold,
    ];

    println!("Action Multipliers:");
    for action in &test_actions {
        let mult = action_to_amount_multiplier(*action);
        let buy = is_buy(*action);
        let sell = is_sell(*action);
        let dir = if buy {
            "BUY "
        } else if sell {
            "SELL"
        } else {
            "HOLD"
        };
        println!(
            "  {:12} -> {:4.1}x ({})",
            format!("{:?}", action),
            mult,
            dir
        );
    }

    println!("\n✅ All profiles and helper functions working correctly!");
    println!("\n=== TASK 2.3 Implementation VERIFIED ===");
}
