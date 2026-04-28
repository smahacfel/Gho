//! WHF Part 1: Flowfield Construction & Extraction Example
//!
//! This example demonstrates how to use the flowfield extractor to build
//! dynamic market flow fields from transaction streams.

use ghost_brain::chaos::{FlowDirection, FlowTransaction, FlowfieldConfig, FlowfieldExtractor};
use solana_sdk::pubkey::Pubkey;

fn main() {
    println!("═══════════════════════════════════════════════════════");
    println!("  WHF Part 1: Flowfield Construction & Extraction Demo");
    println!("═══════════════════════════════════════════════════════\n");

    // Scenario 1: Normal Trading Activity
    println!("📊 Scenario 1: Normal Trading Activity");
    println!("─────────────────────────────────────────────────────");
    demonstrate_normal_trading();

    println!("\n");

    // Scenario 2: Accumulation Phase
    println!("📈 Scenario 2: Accumulation Phase (Smart Money Entry)");
    println!("─────────────────────────────────────────────────────");
    demonstrate_accumulation();

    println!("\n");

    // Scenario 3: Distribution Phase
    println!("📉 Scenario 3: Distribution Phase (Exit Wave)");
    println!("─────────────────────────────────────────────────────");
    demonstrate_distribution();

    println!("\n");

    // Scenario 4: Time Series Analysis
    println!("📉 Scenario 4: Time Series Flow Analysis");
    println!("─────────────────────────────────────────────────────");
    demonstrate_time_series();
}

fn demonstrate_normal_trading() {
    let mut extractor = FlowfieldExtractor::new();

    let mut time = 1_000_000u64;

    // Simulate balanced buy/sell activity
    for i in 0..20 {
        let wallet = Pubkey::new_unique();
        let slot = 100_000 + (i / 2);
        let is_buy = i % 2 == 0;
        let volume = 5.0 + (i as f32 * 0.5);

        extractor.process_transaction(FlowTransaction {
            slot,
            wallet,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        });

        time += 500; // 500ms between transactions
    }

    let agg = extractor.get_aggregate_flow();

    println!("  Total Buy Volume:    {:.2} SOL", agg.buy);
    println!("  Total Sell Volume:   {:.2} SOL", agg.sell);
    println!("  Net Flow:            {:.2} SOL", agg.net);
    println!("  Unique Wallets:      {}", agg.wallets);
    println!("  Flow Direction:      {:?}", agg.flow_direction());

    if let Some(ratio) = agg.buy_sell_ratio() {
        println!("  Buy/Sell Ratio:      {:.2}", ratio);
    }

    println!("\n  Interpretation:");
    match agg.flow_direction() {
        FlowDirection::Accumulation => println!("  ✅ Net buying pressure detected"),
        FlowDirection::Distribution => println!("  ⚠️  Net selling pressure detected"),
        FlowDirection::Neutral => println!("  ℹ️  Balanced market activity"),
    }

    println!("\n  Slots in Window:     {}", extractor.window_slot_count());
    println!(
        "  Total Transactions:  {}",
        extractor.window_transaction_count()
    );
}

fn demonstrate_accumulation() {
    // Use shorter window for this demo
    let config = FlowfieldConfig::with_window(20_000);
    let mut extractor = FlowfieldExtractor::with_config(config);

    let mut time = 2_000_000u64;

    // Simulate strong accumulation: many wallets buying
    println!("  Simulating 15 wallets entering positions...\n");

    for i in 0..15 {
        let wallet = Pubkey::new_unique();
        let slot = 200_000 + i;

        // Mostly buys with increasing volume
        let is_buy = i < 12; // 12 buys, 3 sells
        let volume = if is_buy {
            10.0 + (i as f32 * 2.0) // Increasing buy size
        } else {
            5.0 // Small sells
        };

        extractor.process_transaction(FlowTransaction {
            slot,
            wallet,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        });

        time += 800;
    }

    let agg = extractor.get_aggregate_flow();

    println!("  Total Buy Volume:    {:.2} SOL", agg.buy);
    println!("  Total Sell Volume:   {:.2} SOL", agg.sell);
    println!("  Net Flow:            {:.2} SOL", agg.net);
    println!("  Unique Wallets:      {}", agg.wallets);

    if let Some(ratio) = agg.buy_sell_ratio() {
        println!("  Buy/Sell Ratio:      {:.2}x", ratio);
    }

    println!("\n  Interpretation:");
    if agg.net > 50.0 {
        println!("  🚀 STRONG ACCUMULATION DETECTED");
        println!("  💡 Smart money is entering positions");
        if let Some(ratio) = agg.buy_sell_ratio() {
            if ratio > 3.0 {
                println!("  📊 Buy pressure {}x stronger than sells", ratio.round());
            }
        }
    }

    // Show per-slot breakdown
    println!("\n  Per-Slot Flow Analysis:");
    let slot_flows = extractor.get_all_slot_flows();
    for (slot, flow) in slot_flows.iter().take(5) {
        println!(
            "    Slot {}: Buy={:.1}, Sell={:.1}, Net={:.1}",
            slot, flow.buy, flow.sell, flow.net
        );
    }
    if slot_flows.len() > 5 {
        println!("    ... and {} more slots", slot_flows.len() - 5);
    }
}

fn demonstrate_distribution() {
    let mut extractor = FlowfieldExtractor::new();

    let mut time = 3_000_000u64;

    // Simulate distribution: many wallets exiting
    println!("  Simulating 20 wallets exiting positions...\n");

    for i in 0..20 {
        let wallet = Pubkey::new_unique();
        let slot = 300_000 + (i / 3); // Multiple txs per slot

        // Mostly sells
        let is_buy = i < 5; // 5 buys, 15 sells
        let volume = if !is_buy {
            8.0 + (i as f32 * 1.5) // Increasing sell size
        } else {
            3.0 // Small buys
        };

        extractor.process_transaction(FlowTransaction {
            slot,
            wallet,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        });

        time += 600;
    }

    let agg = extractor.get_aggregate_flow();

    println!("  Total Buy Volume:    {:.2} SOL", agg.buy);
    println!("  Total Sell Volume:   {:.2} SOL", agg.sell);
    println!("  Net Flow:            {:.2} SOL", agg.net);
    println!("  Unique Wallets:      {}", agg.wallets);

    if let Some(ratio) = agg.buy_sell_ratio() {
        println!("  Buy/Sell Ratio:      {:.2}x", ratio);
    }

    println!("\n  Interpretation:");
    if agg.net < -50.0 {
        println!("  🔴 STRONG DISTRIBUTION DETECTED");
        println!("  ⚠️  Wallets are exiting positions");
        println!("  💸 Capital is flowing out");

        if agg.sell > agg.buy * 3.0 {
            println!(
                "  📊 Sell pressure dominates ({}x stronger)",
                (agg.sell / agg.buy.max(0.1)).round()
            );
        }
    }

    // Show wallet-level analysis
    println!("\n  Active Wallets in Window:");
    let wallet_flows = extractor.get_all_wallet_flows();
    let mut sellers = 0;
    let mut buyers = 0;

    for (_, flow) in &wallet_flows {
        if flow.sell > flow.buy {
            sellers += 1;
        } else if flow.buy > flow.sell {
            buyers += 1;
        }
    }

    println!("    Sellers:  {} wallets", sellers);
    println!("    Buyers:   {} wallets", buyers);
    println!(
        "    Ratio:    {:.1}:1 (sellers:buyers)",
        sellers as f32 / buyers.max(1) as f32
    );
}

fn demonstrate_time_series() {
    let mut extractor = FlowfieldExtractor::new();

    let mut time = 4_000_000u64;

    println!("  Creating flow time series over 10 slots...\n");

    // Create a pattern: accumulation -> distribution -> accumulation
    let pattern = [
        (true, 15.0),  // Strong buy
        (true, 12.0),  // Buy
        (true, 10.0),  // Buy
        (false, 8.0),  // Sell
        (false, 11.0), // Strong sell
        (false, 13.0), // Strong sell
        (false, 9.0),  // Sell
        (true, 11.0),  // Buy
        (true, 14.0),  // Strong buy
        (true, 16.0),  // Strong buy
    ];

    for (i, (is_buy, volume)) in pattern.iter().enumerate() {
        let slot = 400_000 + i as u64;
        let wallet = Pubkey::new_unique();

        extractor.process_transaction(FlowTransaction {
            slot,
            wallet,
            is_buy: *is_buy,
            volume_sol: *volume,
            timestamp_ms: time,
        });

        time += 1000;
    }

    println!("  Flow Vector Time Series F(t):");
    println!("  ───────────────────────────────────────────────");
    println!("  Slot       | Buy   | Sell  | Net    | Direction");
    println!("  ───────────────────────────────────────────────");

    let slot_flows = extractor.get_all_slot_flows();
    for (slot, flow) in &slot_flows {
        let direction_symbol = match flow.flow_direction() {
            FlowDirection::Accumulation => "↗️ ",
            FlowDirection::Distribution => "↘️ ",
            FlowDirection::Neutral => "→ ",
        };

        println!(
            "  {:9} | {:5.1} | {:5.1} | {:6.1} | {}",
            slot, flow.buy, flow.sell, flow.net, direction_symbol
        );
    }

    println!("\n  Pattern Analysis:");
    let agg = extractor.get_aggregate_flow();

    // Detect the overall trend
    let first_third = &slot_flows[0..3];
    let second_third = &slot_flows[3..7];
    let last_third = &slot_flows[7..10];

    let first_net: f32 = first_third.iter().map(|(_, f)| f.net).sum();
    let second_net: f32 = second_third.iter().map(|(_, f)| f.net).sum();
    let last_net: f32 = last_third.iter().map(|(_, f)| f.net).sum();

    println!(
        "  Phase 1 (slots 0-2):   Net = {:6.1} SOL ({})",
        first_net,
        if first_net > 0.0 {
            "Accumulation"
        } else {
            "Distribution"
        }
    );
    println!(
        "  Phase 2 (slots 3-6):   Net = {:6.1} SOL ({})",
        second_net,
        if second_net > 0.0 {
            "Accumulation"
        } else {
            "Distribution"
        }
    );
    println!(
        "  Phase 3 (slots 7-9):   Net = {:6.1} SOL ({})",
        last_net,
        if last_net > 0.0 {
            "Accumulation"
        } else {
            "Distribution"
        }
    );

    println!("\n  Overall Window:");
    println!("    Total Net Flow: {:.1} SOL", agg.net);
    println!("    Trend: {:?}", agg.flow_direction());

    if first_net > 0.0 && second_net < 0.0 && last_net > 0.0 {
        println!("\n  💡 Pattern Detected: Accumulation → Distribution → Accumulation");
        println!("     This could indicate: Shake-out followed by re-entry");
    }
}
