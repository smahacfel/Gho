//! Example: WHF Signal API for Launcher/Trading Bot
//!
//! This example demonstrates how to use the complete WHF (Wallet Harmonic Field)
//! pipeline to generate trading signals for automated trading systems.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example whf_launcher_api
//! ```

use ghost_brain::chaos::{
    FlowTransaction, FlowfieldExtractor, HarmonicFieldAnalyzer, WhfSignalConfig, WhfSignalDetector,
    WhfSignalType,
};
use ghost_brain::oracle::wallet_energy_tracker::WalletEnergyTracker;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

fn main() {
    println!("=== WHF Part 3: Signal API for Launcher/Trading Bot ===\n");

    // Initialize the three components of WHF pipeline
    let mut flowfield = FlowfieldExtractor::new();
    let mut field_analyzer = HarmonicFieldAnalyzer::new(128);
    let signal_detector = WhfSignalDetector::new();
    let wallet_tracker = WalletEnergyTracker::new();

    println!("📊 WHF Pipeline Initialized:");
    println!("   - Flowfield Extractor (Part 1)");
    println!("   - Harmonic Field Analyzer (Part 2)");
    println!("   - Signal Detector (Part 3)");
    println!();

    // Simulate different market scenarios
    demonstrate_organic_expansion(
        &mut flowfield,
        &mut field_analyzer,
        &signal_detector,
        &wallet_tracker,
    );
    println!("\n{}\n", "=".repeat(60));

    demonstrate_wash_trading(
        &mut flowfield,
        &mut field_analyzer,
        &signal_detector,
        &wallet_tracker,
    );
    println!("\n{}\n", "=".repeat(60));

    demonstrate_trend_decay(
        &mut flowfield,
        &mut field_analyzer,
        &signal_detector,
        &wallet_tracker,
    );
    println!("\n{}\n", "=".repeat(60));

    demonstrate_bot_manipulation(
        &mut flowfield,
        &mut field_analyzer,
        &signal_detector,
        &wallet_tracker,
    );
    println!("\n{}\n", "=".repeat(60));

    demonstrate_custom_config();
    println!("\n{}\n", "=".repeat(60));

    println!("✅ All scenarios demonstrated successfully!");
    println!("\n## Integration with Launcher/Bot:");
    println!("   1. Stream transactions via Geyser/WebSocket");
    println!("   2. Process through Flowfield + WEST");
    println!("   3. Analyze with HarmonicFieldAnalyzer");
    println!("   4. Detect signals with WhfSignalDetector");
    println!("   5. Execute trades based on signal.trigger_level");
}

fn demonstrate_organic_expansion(
    flowfield: &mut FlowfieldExtractor,
    analyzer: &mut HarmonicFieldAnalyzer,
    detector: &WhfSignalDetector,
    tracker: &WalletEnergyTracker,
) {
    println!("Scenario 1: ORGANIC EXPANSION");
    println!("Description: Natural market growth with diverse participants\n");

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let mut time = 1000000u64;

    // Simulate organic buying activity
    for i in 0..12 {
        let wallet = Pubkey::new_unique();
        let volume = 8.0 + (i as f32 * 1.5);
        let is_buy = i < 10; // Mostly buying

        let flow_tx = FlowTransaction {
            slot: 1000 + (i as u64),
            wallet,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);
        tracker.process_transaction(pool, wallet, token, is_buy, volume as f64, time);

        time += 2500 + (i as u64 * 800); // Random intervals
    }

    let signal = analyze_and_report(flowfield, analyzer, detector, tracker, time);

    if signal.signal_type == WhfSignalType::OrganicExpansion {
        println!("✅ SIGNAL: ORGANIC_EXPANSION");
        println!("   Action Recommendation:");
        if signal.trigger_level > 0.8 {
            println!("   🚀 STRONG BUY - Increase position size");
        } else if signal.trigger_level > 0.6 {
            println!("   📈 BUY - Standard position");
        } else if signal.trigger_level > 0.3 {
            println!("   ⏰ PREPARE - Watch for entry");
        } else {
            println!("   👁️  MONITOR - Weak signal");
        }
    }
}

fn demonstrate_wash_trading(
    flowfield: &mut FlowfieldExtractor,
    analyzer: &mut HarmonicFieldAnalyzer,
    detector: &WhfSignalDetector,
    tracker: &WalletEnergyTracker,
) {
    println!("Scenario 2: WASH TRADING");
    println!("Description: Circular trading to inflate volume\n");

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let bot1 = Pubkey::new_unique();
    let bot2 = Pubkey::new_unique();
    let mut time = 2000000u64;

    // Alternating buy-sell between two bots
    for i in 0..16 {
        let is_buy = i % 2 == 0;
        let wallet = if is_buy { bot1 } else { bot2 };
        let volume = 12.0;

        let flow_tx = FlowTransaction {
            slot: 2000 + (i as u64),
            wallet,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);
        tracker.process_transaction(pool, wallet, token, is_buy, volume as f64, time);

        time += 1000; // Regular intervals
    }

    let signal = analyze_and_report(flowfield, analyzer, detector, tracker, time);

    if signal.signal_type == WhfSignalType::WashTrading {
        println!("⚠️  SIGNAL: WASH_TRADING");
        println!("   Action Recommendation:");
        println!("   🚫 AVOID - Do not enter position");
        println!(
            "   ⚡ High curl detected: {:.2}",
            signal.harmonic_analysis.curl
        );
        println!(
            "   📊 Near-zero net flow: {:.2}",
            signal.flow_metrics.net_flow
        );
    }
}

fn demonstrate_trend_decay(
    flowfield: &mut FlowfieldExtractor,
    analyzer: &mut HarmonicFieldAnalyzer,
    detector: &WhfSignalDetector,
    tracker: &WalletEnergyTracker,
) {
    println!("Scenario 3: TREND DECAY");
    println!("Description: Distribution phase with selling pressure\n");

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let mut time = 3000000u64;

    // Heavy selling from multiple wallets
    for i in 0..10 {
        let wallet = Pubkey::new_unique();
        let volume = 15.0 + (i as f32 * 2.5);

        let flow_tx = FlowTransaction {
            slot: 3000 + (i as u64),
            wallet,
            is_buy: false,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);
        tracker.process_transaction(pool, wallet, token, false, volume as f64, time);

        time += 1800 + (i as u64 * 400);
    }

    let signal = analyze_and_report(flowfield, analyzer, detector, tracker, time);

    if signal.signal_type == WhfSignalType::TrendDecay {
        println!("📉 SIGNAL: TREND_DECAY");
        println!("   Action Recommendation:");
        if signal.trigger_level > 0.8 {
            println!("   🚨 URGENT EXIT - Sell immediately");
        } else if signal.trigger_level > 0.6 {
            println!("   📉 EXIT - Close position");
        } else {
            println!("   ⚠️  REDUCE - Decrease exposure");
        }
    }
}

fn demonstrate_bot_manipulation(
    flowfield: &mut FlowfieldExtractor,
    analyzer: &mut HarmonicFieldAnalyzer,
    detector: &WhfSignalDetector,
    tracker: &WalletEnergyTracker,
) {
    println!("Scenario 4: BOT MANIPULATION");
    println!("Description: Coordinated trading with perfect periodicity\n");

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let bot = Pubkey::new_unique();
    let mut time = 4000000u64;

    // Bot trading with perfect timing
    for i in 0..25 {
        let is_buy = i % 4 != 3;
        let volume = 10.0;

        let flow_tx = FlowTransaction {
            slot: 4000 + (i as u64),
            wallet: bot,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);
        tracker.process_transaction(pool, bot, token, is_buy, volume as f64, time);

        time += 600; // Perfect 600ms intervals
    }

    let signal = analyze_and_report(flowfield, analyzer, detector, tracker, time);

    if signal.signal_type == WhfSignalType::BotManipulation {
        println!("🤖 SIGNAL: BOT_MANIPULATION");
        println!("   Action Recommendation:");
        println!("   ⚠️  CAUTION - Bot activity detected");
        println!(
            "   📊 High resonance: {:.2}",
            signal.harmonic_analysis.resonance_score
        );
        println!("   💡 Consider: Follow bot trend or avoid");
    }
}

fn demonstrate_custom_config() {
    println!("Scenario 5: CUSTOM CONFIGURATION");
    println!("Description: Using custom thresholds for signal detection\n");

    // Create custom configuration
    let custom_config = WhfSignalConfig {
        wash_trading_curl_threshold: 0.5,        // Lower threshold
        bot_resonance_threshold: 0.6,            // Lower threshold
        accumulation_divergence_threshold: 0.15, // More sensitive
        distribution_divergence_threshold: -0.15,
        organic_curl_threshold: 0.35,
        organic_resonance_threshold: 0.25,
        min_significant_net_flow: 0.5, // Lower minimum
        min_wallet_count: 2,           // Lower minimum
    };

    let detector = WhfSignalDetector::with_config(custom_config);

    println!("✅ Custom Configuration Created:");
    println!("   - More sensitive to wash trading (curl >= 0.5)");
    println!("   - More sensitive to bot activity (resonance >= 0.6)");
    println!("   - Lower minimum requirements");
    println!("\n   Use case: High-frequency trading with tighter risk controls");
}

fn analyze_and_report(
    flowfield: &mut FlowfieldExtractor,
    analyzer: &mut HarmonicFieldAnalyzer,
    detector: &WhfSignalDetector,
    tracker: &WalletEnergyTracker,
    time: u64,
) -> ghost_brain::chaos::WhfSignal {
    // Get flow data
    let aggregate_flow = flowfield.get_aggregate_flow();

    // Extract wallet data
    let wallet_cache = tracker.get_wallet_cache_snapshot();
    let mut wallet_data = HashMap::new();
    for (pk, particle) in wallet_cache.iter() {
        wallet_data.insert(
            *pk,
            (
                particle.energy as f64,
                particle.current_token,
                particle.last_update_ms,
            ),
        );
    }

    // Perform harmonic analysis
    let harmonic_analysis = analyzer.analyze(&wallet_data, time);

    // Detect signal
    let signal = detector.detect_signal(&harmonic_analysis, &aggregate_flow);

    // Report metrics
    println!("Harmonic Indicators:");
    println!("   Curl (wash trading):     {:.3}", harmonic_analysis.curl);
    println!(
        "   Divergence (flow):       {:.3}",
        harmonic_analysis.divergence
    );
    println!(
        "   Resonance (bot activity): {:.3}",
        harmonic_analysis.resonance_score
    );
    println!();
    println!("Flow Metrics:");
    println!(
        "   Buy Volume:    {:.2} SOL",
        signal.flow_metrics.buy_volume
    );
    println!(
        "   Sell Volume:   {:.2} SOL",
        signal.flow_metrics.sell_volume
    );
    println!("   Net Flow:      {:.2} SOL", signal.flow_metrics.net_flow);
    println!("   Wallet Count:  {}", signal.flow_metrics.wallet_count);
    println!();
    println!("Signal Output:");
    println!("   Type:          {:?}", signal.signal_type);
    println!("   Confidence:    {:.2}", signal.confidence);
    println!("   Trigger Level: {:.2}", signal.trigger_level);
    println!("   Reason:        {}", signal.reason);
    println!();

    signal
}
