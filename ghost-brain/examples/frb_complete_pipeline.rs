//! FRB Complete Pipeline Example
//!
//! This example demonstrates the complete FRB (Fractal Resonance Bands) pipeline:
//! - Part 1: Multi-scale band extraction
//! - Part 2: Resonance detection and coherence calculation
//! - Part 3: Signal classification and interpretation
//!
//! Run with: cargo run --example frb_complete_pipeline

use ghost_brain::signals::{
    BandConfig, BandExtractor, BandTransaction, FrbSignal, ResonanceAnalyzer, WINDOW_5S,
};
use solana_sdk::pubkey::Pubkey;

fn main() {
    println!("=== FRB Complete Pipeline Example ===\n");

    // Scenario 1: Organic Trend with Multi-Scale Support
    println!("📈 Scenario 1: Organic Trend Analysis");
    println!("─────────────────────────────────────");

    let mut extractor = BandExtractor::with_config(BandConfig::with_window(WINDOW_5S));
    let analyzer = ResonanceAnalyzer::new();

    // Simulate 250 organic transactions with increasing volume
    for i in 0..250 {
        let volume = 5.0 + (i as f32 * 0.1);
        let is_buy = i % 4 != 0; // 75% buys
        let timestamp = 1000 + i * 20; // 20ms spacing

        let tx = BandTransaction::new(volume, is_buy, Pubkey::new_unique(), timestamp);
        extractor.add_transaction(tx);
    }

    // Part 1: Extract multi-scale bands
    let bands = extractor.extract_bands();

    println!("Band Profiles:");
    println!(
        "  Short  (8-32 tx):   amplitude={:.2}, buyers={}, volatility={:.2}",
        bands[0].amplitude, bands[0].buyers, bands[0].volatility
    );
    println!(
        "  Medium (32-128 tx): amplitude={:.2}, buyers={}, volatility={:.2}",
        bands[1].amplitude, bands[1].buyers, bands[1].volatility
    );
    println!(
        "  Long   (128+ tx):   amplitude={:.2}, buyers={}, volatility={:.2}",
        bands[2].amplitude, bands[2].buyers, bands[2].volatility
    );

    // Part 2 & 3: Analyze resonance and classify signal
    let result = analyzer.analyze(bands);

    println!("\nResonance Analysis:");
    println!("  Overall Score: {:.3}", result.resonance_score);
    println!("  Coherence Map:");
    println!("    Short-Medium: {:.3}", result.coherence_map[0]);
    println!("    Medium-Long:  {:.3}", result.coherence_map[1]);
    println!("    Short-Long:   {:.3}", result.coherence_map[2]);
    println!("  Trend Likelihood: {:.3}", result.trend_likelihood);

    println!("\nSignal Classification:");
    println!("  Signal: {:?}", result.signal);
    println!("  Description: {}", result.signal.description());
    println!("  Strength: {:.1}", result.signal.strength());

    interpret_signal(&result);

    println!("\n");

    // Scenario 2: Bot Spike Detection
    println!("🤖 Scenario 2: Bot Spike Detection");
    println!("──────────────────────────────────");

    let mut extractor2 = BandExtractor::new();
    let analyzer2 = ResonanceAnalyzer::new();

    // Simulate bot activity: 40 rapid transactions from same 2 wallets
    let bot_wallet1 = Pubkey::new_unique();
    let bot_wallet2 = Pubkey::new_unique();

    for i in 0..40 {
        let volume = 10.0; // Constant volume (bot-like)
        let wallet = if i % 2 == 0 { bot_wallet1 } else { bot_wallet2 };
        let timestamp = 1000 + i * 10; // 10ms spacing

        let tx = BandTransaction::new(volume, true, wallet, timestamp);
        extractor2.add_transaction(tx);
    }

    let bands2 = extractor2.extract_bands();
    let result2 = analyzer2.analyze(bands2);

    println!("Band Profiles:");
    println!(
        "  Short band: amplitude={:.2}, buyers={} (BOT ALERT: <3 buyers)",
        result2.band_profiles[0].amplitude, result2.band_profiles[0].buyers
    );
    println!(
        "  Medium band: significant={}",
        result2.band_profiles[1].is_significant()
    );
    println!(
        "  Long band: significant={}",
        result2.band_profiles[2].is_significant()
    );

    println!("\nResonance Analysis:");
    println!("  Overall Score: {:.3} (LOW)", result2.resonance_score);
    println!("  Signal: {:?}", result2.signal);

    interpret_signal(&result2);

    println!("\n");

    // Scenario 3: Transitional Phase
    println!("⚡ Scenario 3: Transitional Phase");
    println!("─────────────────────────────────");

    let mut extractor3 = BandExtractor::new();
    let analyzer3 = ResonanceAnalyzer::new();

    // Simulate early trend formation: 120 transactions
    for i in 0..120 {
        let volume = 7.0 + (i as f32 * 0.05);
        let is_buy = i % 3 != 0; // 67% buys
        let timestamp = 1000 + i * 25;

        let tx = BandTransaction::new(volume, is_buy, Pubkey::new_unique(), timestamp);
        extractor3.add_transaction(tx);
    }

    let bands3 = extractor3.extract_bands();
    let result3 = analyzer3.analyze(bands3);

    println!("Resonance Analysis:");
    println!("  Overall Score: {:.3}", result3.resonance_score);
    println!("  Trend Likelihood: {:.3}", result3.trend_likelihood);
    println!("  Signal: {:?}", result3.signal);

    interpret_signal(&result3);

    println!("\n");

    // Scenario 4: Using Custom Configuration
    println!("⚙️  Scenario 4: Custom Configuration");
    println!("────────────────────────────────────");

    use ghost_brain::signals::FrbResonanceConfig;

    let custom_config = FrbResonanceConfig::with_thresholds(0.75, 0.25);
    let analyzer4 = ResonanceAnalyzer::with_config(custom_config);

    println!("Custom thresholds: continue=0.75, fake=0.25");
    println!("Re-analyzing Scenario 1 with custom config:");

    let result4 = analyzer4.analyze(result.band_profiles.clone());
    println!("  Signal: {:?}", result4.signal);
    println!("  (Compare with default config: {:?})", result.signal);
}

fn interpret_signal(result: &ghost_brain::signals::FrbResult) {
    println!("\nInterpretation:");

    match result.signal {
        FrbSignal::ResContinue => {
            println!("  ✅ Strong multi-scale resonance detected");
            println!("  ✅ All bands show coordinated activity");
            println!("  💡 Recommendation: Trend likely to continue");
            println!("  💡 Action: Consider holding or entering position");
        }
        FrbSignal::ResFake => {
            println!("  ⚠️  Weak resonance - potential bot activity");
            println!("  ⚠️  Limited buyer participation");
            println!("  💡 Recommendation: Avoid this pump");
            println!("  💡 Action: Exit or do not enter");
        }
        FrbSignal::ResTransition => {
            println!("  ⏳ Transitional phase detected");
            println!("  ⏳ Partial band activation");
            println!("  💡 Recommendation: Monitor for full alignment");
            println!("  💡 Action: Wait for confirmation");
        }
        FrbSignal::ResHold => {
            println!("  ⏸️  Insufficient signal");
            println!("  💡 Recommendation: Wait for clearer pattern");
            println!("  💡 Action: No action recommended");
        }
    }

    // Risk assessment based on coherence
    let avg_coherence =
        (result.coherence_map[0] + result.coherence_map[1] + result.coherence_map[2]) / 3.0;
    if avg_coherence > 0.7 {
        println!("  📊 Risk Level: LOW (high cross-scale alignment)");
    } else if avg_coherence > 0.4 {
        println!("  📊 Risk Level: MEDIUM");
    } else {
        println!("  📊 Risk Level: HIGH (low alignment)");
    }
}
