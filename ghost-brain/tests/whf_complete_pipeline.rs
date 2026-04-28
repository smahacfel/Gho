//! Integration Test: Complete WHF Pipeline (Part 1 + Part 2 + Part 3)
//!
//! This test demonstrates the full WHF (Wallet Harmonic Field) pipeline:
//! - Part 1: Flowfield construction from transactions
//! - Part 2: Harmonic field analysis (curl, divergence, resonance)
//! - Part 3: Signal detection for launcher/trading bot

use ghost_brain::chaos::{
    FlowTransaction, FlowfieldExtractor, HarmonicFieldAnalyzer, WhfSignalDetector, WhfSignalType,
};
use ghost_brain::oracle::wallet_energy_tracker::WalletEnergyTracker;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

#[test]
fn test_complete_whf_pipeline_organic_expansion() {
    // Setup all three components
    let mut flowfield = FlowfieldExtractor::new();
    let mut analyzer = HarmonicFieldAnalyzer::new(128);
    let detector = WhfSignalDetector::new();
    let tracker = WalletEnergyTracker::new();

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let mut time = 1000000u64;

    // First, establish baseline with initial small activity
    for i in 0..5 {
        let wallet = Pubkey::new_unique();
        let volume = 2.0;

        let flow_tx = FlowTransaction {
            slot: 1000 + (i as u64),
            wallet,
            is_buy: true,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);
        tracker.process_transaction(pool, wallet, token, true, volume as f64, time);

        time += 5000;
    }

    // Get initial state
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
    analyzer.analyze(&wallet_data, time);

    // Now simulate organic buying activity with diverse participants
    for i in 0..15 {
        let wallet = Pubkey::new_unique();
        let volume = 5.0 + (i as f32 * 2.0);

        let flow_tx = FlowTransaction {
            slot: 2000 + (i as u64),
            wallet,
            is_buy: true,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);
        tracker.process_transaction(pool, wallet, token, true, volume as f64, time);

        time += 3000 + (i as u64 * 500); // Random intervals
    }

    // Get flow data
    let aggregate_flow = flowfield.get_aggregate_flow();

    // Get wallet data for field analysis
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

    // Perform field analysis
    let harmonic_analysis = analyzer.analyze(&wallet_data, time);

    // Detect signal
    let signal = detector.detect_signal(&harmonic_analysis, &aggregate_flow);

    // With random intervals and diverse wallets, should detect organic or at least not bot manipulation
    // The key is positive divergence and sufficient net flow
    assert!(
        harmonic_analysis.divergence > 0.0,
        "Should have positive divergence"
    );
    assert!(aggregate_flow.net > 0.0, "Should have positive net flow");
    assert!(
        aggregate_flow.wallets >= 3,
        "Should have sufficient wallets"
    );

    // Should be either organic expansion or hold (but not wash trading or trend decay)
    assert_ne!(signal.signal_type, WhfSignalType::WashTrading);
    assert_ne!(signal.signal_type, WhfSignalType::TrendDecay);

    println!("✅ Organic Expansion Test:");
    println!("   Signal Type: {:?}", signal.signal_type);
    println!("   Confidence: {:.2}", signal.confidence);
    println!("   Trigger Level: {:.2}", signal.trigger_level);
    println!("   Reason: {}", signal.reason);
}

#[test]
fn test_complete_whf_pipeline_wash_trading() {
    let mut flowfield = FlowfieldExtractor::new();
    let mut analyzer = HarmonicFieldAnalyzer::new(128);
    let detector = WhfSignalDetector::new();
    let tracker = WalletEnergyTracker::new();

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let bot1 = Pubkey::new_unique();
    let bot2 = Pubkey::new_unique();
    let mut time = 1000000u64;

    // Establish baseline with some initial buying from both bots
    for _i in 0..5 {
        tracker.process_transaction(pool, bot1, token, true, 5.0, time);
        time += 2000;
        tracker.process_transaction(pool, bot2, token, true, 5.0, time);
        time += 2000;
    }

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
    analyzer.analyze(&wallet_data, time);

    // Simulate wash trading: alternating buy-sell between same two bots
    // This creates velocity changes which should increase curl
    for i in 0..30 {
        let is_buy = i % 2 == 0;
        let wallet = if i % 2 == 0 { bot1 } else { bot2 };
        let volume = 10.0;

        let flow_tx = FlowTransaction {
            slot: 2000 + (i as u64),
            wallet,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);

        tracker.process_transaction(pool, wallet, token, is_buy, volume as f64, time);

        time += 1000; // Regular intervals (bot-like)
    }

    let aggregate_flow = flowfield.get_aggregate_flow();

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

    let harmonic_analysis = analyzer.analyze(&wallet_data, time);
    let signal = detector.detect_signal(&harmonic_analysis, &aggregate_flow);

    // Key characteristic: Near-zero net flow despite high total volume
    let total_volume = aggregate_flow.buy + aggregate_flow.sell;
    let net_ratio = aggregate_flow.net.abs() / (total_volume + 0.01);

    println!("⚠️  Wash Trading Pattern Test:");
    println!("   Curl: {:.2}", harmonic_analysis.curl);
    println!("   Net Flow: {:.2}", aggregate_flow.net);
    println!("   Total Volume: {:.2}", total_volume);
    println!("   Net Ratio: {:.2}", net_ratio);
    println!("   Signal Type: {:?}", signal.signal_type);

    // Verify near-zero net flow (main wash trading characteristic)
    assert!(
        net_ratio < 0.15,
        "Should have near-zero net flow ratio: {:.2}",
        net_ratio
    );
    assert!(total_volume > 100.0, "Should have significant total volume");

    // Should not be classified as organic expansion (wash trading has balanced flow)
    assert_ne!(
        signal.signal_type,
        WhfSignalType::OrganicExpansion,
        "Should not be organic expansion with balanced wash trading flow"
    );
}

#[test]
fn test_complete_whf_pipeline_trend_decay() {
    let mut flowfield = FlowfieldExtractor::new();
    let mut analyzer = HarmonicFieldAnalyzer::new(128);
    let detector = WhfSignalDetector::new();
    let tracker = WalletEnergyTracker::new();

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let mut time = 1000000u64;
    let mut wallets = Vec::new();

    // Phase 1: Establish baseline with accumulation (buying)
    for _i in 0..10 {
        let wallet = Pubkey::new_unique();
        wallets.push(wallet);

        let flow_tx = FlowTransaction {
            slot: 1000,
            wallet,
            is_buy: true,
            volume_sol: 20.0,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);
        tracker.process_transaction(pool, wallet, token, true, 20.0, time);
        time += 2000;
    }

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
    analyzer.analyze(&wallet_data, time);

    // Phase 2: Distribution phase - same wallets now selling
    for wallet in &wallets {
        let volume = 25.0; // Selling more than they bought

        let flow_tx = FlowTransaction {
            slot: 3000,
            wallet: *wallet,
            is_buy: false,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);

        tracker.process_transaction(pool, *wallet, token, false, volume as f64, time);

        time += 1500;
    }

    // Additional new sellers (panic sellers)
    for _i in 0..10 {
        let wallet = Pubkey::new_unique();
        let volume = 30.0;

        let flow_tx = FlowTransaction {
            slot: 4000,
            wallet,
            is_buy: false,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);

        tracker.process_transaction(pool, wallet, token, false, volume as f64, time);

        time += 1200;
    }

    let aggregate_flow = flowfield.get_aggregate_flow();

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

    let harmonic_analysis = analyzer.analyze(&wallet_data, time);
    let signal = detector.detect_signal(&harmonic_analysis, &aggregate_flow);

    println!("📉 Trend Decay Pattern Test:");
    println!("   Divergence: {:.2}", harmonic_analysis.divergence);
    println!("   Net Flow: {:.2}", aggregate_flow.net);
    println!("   Buy Volume: {:.2}", aggregate_flow.buy);
    println!("   Sell Volume: {:.2}", aggregate_flow.sell);
    println!("   Signal Type: {:?}", signal.signal_type);

    // Should have strong selling pressure
    assert!(
        aggregate_flow.net < -50.0,
        "Should have strong negative net flow: {:.2}",
        aggregate_flow.net
    );
    assert!(
        aggregate_flow.sell > aggregate_flow.buy,
        "Sell should exceed buy"
    );
    assert!(
        aggregate_flow.wallets >= 3,
        "Should have sufficient wallets: {}",
        aggregate_flow.wallets
    );

    // Should detect trend decay or at least not organic expansion
    assert_ne!(
        signal.signal_type,
        WhfSignalType::OrganicExpansion,
        "Should not be organic with heavy selling"
    );
}

#[test]
fn test_complete_whf_pipeline_bot_manipulation() {
    let mut flowfield = FlowfieldExtractor::new();
    let mut analyzer = HarmonicFieldAnalyzer::new(128);
    let detector = WhfSignalDetector::new();
    let tracker = WalletEnergyTracker::new();

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let bot = Pubkey::new_unique();
    let mut time = 1000000u64;

    // Establish baseline
    for i in 0..5 {
        tracker.process_transaction(pool, bot, token, true, 5.0, time);
        time += 3000;
    }

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
    analyzer.analyze(&wallet_data, time);

    // Simulate bot trading with perfect periodicity
    let mut timestamps = Vec::new();
    for i in 0..40 {
        let is_buy = i % 3 == 0;
        let volume = 8.0;

        let flow_tx = FlowTransaction {
            slot: 4000 + (i as u64),
            wallet: bot,
            is_buy,
            volume_sol: volume,
            timestamp_ms: time,
        };
        flowfield.process_transaction(flow_tx);

        tracker.process_transaction(pool, bot, token, is_buy, volume as f64, time);
        timestamps.push(time);

        time += 500; // Perfect 500ms intervals
    }

    // Update timestamp buffer for resonance calculation
    analyzer.update_timestamp_buffer(timestamps);

    let aggregate_flow = flowfield.get_aggregate_flow();

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

    let harmonic_analysis = analyzer.analyze(&wallet_data, time);
    let signal = detector.detect_signal(&harmonic_analysis, &aggregate_flow);

    // Should detect high resonance (periodic pattern)
    assert!(
        harmonic_analysis.resonance_score > 0.4,
        "Should have elevated resonance: {:.2}",
        harmonic_analysis.resonance_score
    );

    // Should detect bot manipulation OR at least not organic expansion
    if signal.signal_type == WhfSignalType::BotManipulation {
        println!("✅ Bot Manipulation detected directly");
    } else {
        println!("⚠️  Bot-like pattern detected via high resonance");
        assert_ne!(
            signal.signal_type,
            WhfSignalType::OrganicExpansion,
            "Should not classify as organic with high resonance"
        );
    }

    println!("🤖 Bot Manipulation Pattern Detected:");
    println!("   Signal Type: {:?}", signal.signal_type);
    println!("   Resonance: {:.2}", harmonic_analysis.resonance_score);
    println!("   Trigger Level: {:.2} (caution)", signal.trigger_level);
    println!("   Reason: {}", signal.reason);
}

#[test]
fn test_whf_signal_serialization() {
    let mut flowfield = FlowfieldExtractor::new();
    let mut analyzer = HarmonicFieldAnalyzer::new(128);
    let detector = WhfSignalDetector::new();
    let tracker = WalletEnergyTracker::new();

    let pool = Pubkey::new_unique();
    let token = Pubkey::new_unique();
    let wallet = Pubkey::new_unique();
    let time = 1000000u64;

    // Generate a simple signal
    let flow_tx = FlowTransaction {
        slot: 1000,
        wallet,
        is_buy: true,
        volume_sol: 50.0,
        timestamp_ms: time,
    };
    flowfield.process_transaction(flow_tx);
    tracker.process_transaction(pool, wallet, token, true, 50.0, time);

    let aggregate_flow = flowfield.get_aggregate_flow();
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

    let harmonic_analysis = analyzer.analyze(&wallet_data, time);
    let signal = detector.detect_signal(&harmonic_analysis, &aggregate_flow);

    // Test JSON serialization
    let json = serde_json::to_string(&signal).expect("Should serialize to JSON");
    assert!(json.contains("signal_type"));
    assert!(json.contains("confidence"));
    assert!(json.contains("trigger_level"));

    // Test deserialization
    let _deserialized: ghost_brain::chaos::WhfSignal =
        serde_json::from_str(&json).expect("Should deserialize from JSON");

    println!("✅ Signal serialization test passed");
    println!("   JSON: {}", json);
}
