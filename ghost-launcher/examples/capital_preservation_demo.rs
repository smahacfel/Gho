//! Example demonstrating the Capital Preservation Suite
//!
//! This example shows how to use the Bulkhead (safety) and tip-guard modules
//! to protect capital when building and submitting transactions.
//!
//! Run with: cargo run --example capital_preservation_demo

use ghost_launcher::components::trigger::{
    safety::{calculate_safe_trade_amount, check_emergency_floor, validate_trade, SafetyConfig},
    tip_guard::{calculate_safe_tip, get_fallback_tip, validate_tip, TipGuardConfig},
};

fn main() {
    println!("🛡️  Capital Preservation Suite Demo\n");
    println!("====================================\n");

    // Initialize configurations
    let safety_config = SafetyConfig {
        emergency_floor_sol: 0.05,
        position_size_buffer_sol: 0.02,
        max_position_size_sol: 0.1,
    };

    let tip_guard_config = TipGuardConfig {
        max_tip_absolute_sol: 0.04,
        fallback_tip_sol: 0.001,
    };

    println!("📊 Configuration:");
    println!(
        "  Emergency Floor: {} SOL",
        safety_config.emergency_floor_sol
    );
    println!(
        "  Position Buffer: {} SOL",
        safety_config.position_size_buffer_sol
    );
    println!(
        "  Max Position Size: {} SOL",
        safety_config.max_position_size_sol
    );
    println!(
        "  Max Tip Absolute: {} SOL",
        tip_guard_config.max_tip_absolute_sol
    );
    println!(
        "  Fallback Tip: {} SOL\n",
        tip_guard_config.fallback_tip_sol
    );

    // Scenario 1: Normal trading conditions
    println!("📈 Scenario 1: Normal Trading Conditions");
    println!("----------------------------------------");
    demo_scenario_normal(&safety_config, &tip_guard_config);

    // Scenario 2: Low balance warning
    println!("\n⚠️  Scenario 2: Low Balance Warning");
    println!("----------------------------------------");
    demo_scenario_low_balance(&safety_config, &tip_guard_config);

    // Scenario 3: Critical balance - emergency floor triggered
    println!("\n🚨 Scenario 3: Critical Balance (Emergency Floor)");
    println!("----------------------------------------");
    demo_scenario_critical_balance(&safety_config, &tip_guard_config);

    // Scenario 4: Aggressive tip calculation
    println!("\n💰 Scenario 4: Aggressive Tip Calculation");
    println!("----------------------------------------");
    demo_scenario_aggressive_tip(&safety_config, &tip_guard_config);

    // Scenario 5: API failure fallback
    println!("\n❌ Scenario 5: Jito API Failure");
    println!("----------------------------------------");
    demo_scenario_api_failure(&tip_guard_config);

    println!("\n✅ Demo completed successfully!");
}

fn demo_scenario_normal(safety_config: &SafetyConfig, tip_guard_config: &TipGuardConfig) {
    let current_balance = 1.0; // 1 SOL

    println!("  Current Balance: {} SOL", current_balance);

    // Check emergency floor
    match check_emergency_floor(current_balance, safety_config) {
        Ok(()) => println!("  ✅ Emergency floor check PASSED"),
        Err(e) => println!("  ❌ Emergency floor check FAILED: {}", e),
    }

    // Calculate safe trade amount
    let safe_amount = calculate_safe_trade_amount(current_balance, safety_config);
    println!("  💵 Safe trade amount: {} SOL", safe_amount);

    // Validate proposed trade
    let proposed_trade = 0.1;
    match validate_trade(proposed_trade, current_balance, safety_config) {
        Ok(()) => println!("  ✅ Trade validation PASSED for {} SOL", proposed_trade),
        Err(e) => println!("  ❌ Trade validation FAILED: {}", e),
    }

    // Calculate safe tip
    let calculated_tip = 0.01; // 1% tip from algorithm
    let safe_tip = calculate_safe_tip(calculated_tip, proposed_trade, tip_guard_config);
    println!(
        "  🎁 Safe tip amount: {} SOL (calculated: {} SOL)",
        safe_tip, calculated_tip
    );

    // Validate tip
    if validate_tip(safe_tip, proposed_trade, tip_guard_config) {
        println!("  ✅ Tip validation PASSED");
    } else {
        println!("  ❌ Tip validation FAILED");
    }
}

fn demo_scenario_low_balance(safety_config: &SafetyConfig, tip_guard_config: &TipGuardConfig) {
    let current_balance = 0.08; // Low but above emergency floor

    println!("  Current Balance: {} SOL", current_balance);

    // Check emergency floor
    match check_emergency_floor(current_balance, safety_config) {
        Ok(()) => println!("  ✅ Emergency floor check PASSED"),
        Err(e) => println!("  ❌ Emergency floor check FAILED: {}", e),
    }

    // Calculate safe trade amount
    let safe_amount = calculate_safe_trade_amount(current_balance, safety_config);
    println!(
        "  💵 Safe trade amount: {} SOL (limited by balance)",
        safe_amount
    );

    // Try to trade more than safe amount
    let proposed_trade = 0.05;
    match validate_trade(proposed_trade, current_balance, safety_config) {
        Ok(()) => println!("  ✅ Trade validation PASSED for {} SOL", proposed_trade),
        Err(e) => println!("  ❌ Trade validation FAILED: {}", e),
    }

    // Calculate tip for smaller safe amount
    let calculated_tip = 0.005;
    let safe_tip = calculate_safe_tip(calculated_tip, safe_amount, tip_guard_config);
    println!("  🎁 Safe tip amount: {} SOL", safe_tip);
}

fn demo_scenario_critical_balance(
    safety_config: &SafetyConfig,
    _tip_guard_config: &TipGuardConfig,
) {
    let current_balance = 0.04; // Below emergency floor!

    println!("  Current Balance: {} SOL (CRITICAL!)", current_balance);

    // Check emergency floor - this will fail
    match check_emergency_floor(current_balance, safety_config) {
        Ok(()) => println!("  ✅ Emergency floor check PASSED"),
        Err(e) => {
            println!("  🚨 Emergency floor check FAILED: {}", e);
            println!("  🚨 BOT WOULD SHUTDOWN TO PREVENT COMPLETE DEPLETION");
        }
    }

    // Safe amount would be 0
    let safe_amount = calculate_safe_trade_amount(current_balance, safety_config);
    println!(
        "  💵 Safe trade amount: {} SOL (NO TRADING ALLOWED)",
        safe_amount
    );
}

fn demo_scenario_aggressive_tip(_safety_config: &SafetyConfig, tip_guard_config: &TipGuardConfig) {
    let current_balance = 1.0;
    let proposed_trade = 0.1;

    println!("  Current Balance: {} SOL", current_balance);
    println!("  Proposed Trade: {} SOL", proposed_trade);

    // Aggressive algorithm calculates 0.5 SOL tip (way too high!)
    let calculated_tip = 0.5;
    println!(
        "  ⚠️  Algorithm calculated tip: {} SOL (50% of balance!)",
        calculated_tip
    );

    // TipGuard reduces it to safe levels
    let safe_tip = calculate_safe_tip(calculated_tip, proposed_trade, tip_guard_config);
    println!("  🛡️  TipGuard reduced tip to: {} SOL", safe_tip);
    println!(
        "  📉 Reduction: {}%",
        ((calculated_tip - safe_tip) / calculated_tip * 100.0)
    );

    // Validate the safe tip
    if validate_tip(safe_tip, proposed_trade, tip_guard_config) {
        println!("  ✅ Safe tip validation PASSED");
    }

    // Show why it was capped
    let absolute_cap = tip_guard_config.max_tip_absolute_sol;
    println!("\n  📊 Cap Analysis:");
    println!("    Absolute cap: {} SOL", absolute_cap);
    println!("    Trade size: {} SOL", proposed_trade);
    println!("    Final tip: {} SOL (capped by absolute limit)", safe_tip);
}

fn demo_scenario_api_failure(tip_guard_config: &TipGuardConfig) {
    println!("  ❌ Simulating Jito API failure...");

    // When API fails, use fallback
    let fallback_tip = get_fallback_tip(tip_guard_config);
    println!("  🔄 Using fallback tip: {} SOL", fallback_tip);
    println!("  ✅ Transaction can proceed with conservative tip");
    println!("  💡 Better to use low tip than to fail completely");
}
