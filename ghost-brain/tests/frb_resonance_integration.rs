//! Integration Test: FRB Resonance Detection and Signal Classification
//!
//! This test demonstrates the complete FRB Part 2 & 3 pipeline:
//! - Multi-scale band extraction (Part 1)
//! - Resonance detection and coherence calculation (Part 2)
//! - Signal classification (Part 3)

use ghost_brain::signals::{
    analyze_resonance, BandConfig, BandExtractor, BandTransaction, FrbSignal, ResonanceAnalyzer,
    WINDOW_15S, WINDOW_5S, WINDOW_60S,
};
use solana_sdk::pubkey::Pubkey;

/// Create test transaction helper
fn create_tx(volume: f32, is_buy: bool, timestamp_ms: u64) -> BandTransaction {
    BandTransaction::new(volume, is_buy, Pubkey::new_unique(), timestamp_ms)
}

#[test]
fn test_scenario_trend_continuation() {
    // Scenario: Organic trend with strong multi-scale resonance
    // All bands active, high coherence, increasing momentum

    let mut extractor = BandExtractor::with_config(BandConfig::with_window(WINDOW_5S));
    let analyzer = ResonanceAnalyzer::new();

    // Simulate organic buying across 300 transactions
    // Increasing volume and consistent buyers
    for i in 0..300 {
        let volume = 5.0 + (i as f32 * 0.1); // Gradually increasing
        let is_buy = i % 5 != 0; // 80% buys
        let timestamp = 1000 + i * 15; // 15ms spacing = 4.5s total

        extractor.add_transaction(create_tx(volume, is_buy, timestamp));
    }

    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // Should classify as RES_CONTINUE or high-confidence transition
    // Note: actual resonance may vary based on transaction distribution
    assert!(
        result.signal == FrbSignal::ResContinue || result.signal == FrbSignal::ResTransition,
        "Expected RES_CONTINUE or RES_TRANSITION for organic trend, got {:?}",
        result.signal
    );

    // Moderate to high resonance score
    assert!(
        result.resonance_score > 0.4,
        "Expected moderate-high resonance score, got {}",
        result.resonance_score
    );

    // High trend likelihood
    assert!(
        result.trend_likelihood > 0.5,
        "Expected high trend likelihood, got {}",
        result.trend_likelihood
    );

    // All bands should be significant
    assert!(result.band_profiles[0].is_significant());
    assert!(result.band_profiles[1].is_significant());
    assert!(result.band_profiles[2].is_significant());

    println!("✓ Trend Continuation Test:");
    println!("  Resonance Score: {:.3}", result.resonance_score);
    println!("  Trend Likelihood: {:.3}", result.trend_likelihood);
    println!(
        "  Coherence Map: [{:.3}, {:.3}, {:.3}]",
        result.coherence_map[0], result.coherence_map[1], result.coherence_map[2]
    );
}

#[test]
fn test_scenario_bot_spike() {
    // Scenario: Bot-driven spike with no multi-scale support
    // Only short band active, few unique buyers, low coherence

    let mut extractor = BandExtractor::with_config(BandConfig::with_window(WINDOW_5S));
    let analyzer = ResonanceAnalyzer::new();

    // Simulate bot activity: 50 rapid transactions from same few wallets
    let bot_wallet1 = Pubkey::new_unique();
    let bot_wallet2 = Pubkey::new_unique();

    for i in 0..50 {
        let volume = 10.0; // Consistent volume (bot-like)
        let wallet = if i % 2 == 0 { bot_wallet1 } else { bot_wallet2 };
        let timestamp = 1000 + i * 10; // 10ms spacing = 0.5s total

        let tx = BandTransaction::new(volume, true, wallet, timestamp);
        extractor.add_transaction(tx);
    }

    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // Should classify as RES_FAKE or low resonance signal
    // Note: with only 2 wallets, this should be detected as bot-like
    assert!(
        result.signal == FrbSignal::ResFake || result.signal == FrbSignal::ResTransition,
        "Expected RES_FAKE or RES_TRANSITION for bot spike, got {:?}",
        result.signal
    );

    // Low to medium resonance score
    assert!(
        result.resonance_score < 0.6,
        "Expected low-medium resonance score, got {}",
        result.resonance_score
    );

    // Only short band should be significant
    assert!(result.band_profiles[0].is_significant());
    assert!(!result.band_profiles[1].is_significant() || !result.band_profiles[2].is_significant());

    // Few unique buyers
    assert!(
        result.band_profiles[0].buyers < 5,
        "Expected few buyers in bot scenario, got {}",
        result.band_profiles[0].buyers
    );

    println!("✓ Bot Spike Test:");
    println!("  Resonance Score: {:.3}", result.resonance_score);
    println!("  Signal: {:?}", result.signal);
    println!("  Short Band Buyers: {}", result.band_profiles[0].buyers);
}

#[test]
fn test_scenario_fake_pump() {
    // Scenario: Fake pump with short-term activity but no sustained support
    // Short and medium bands active, but long band weak/absent

    let mut extractor = BandExtractor::with_config(BandConfig::with_window(WINDOW_5S));
    let analyzer = ResonanceAnalyzer::new();

    // Phase 1: Build up some history (150 small transactions)
    for i in 0..150 {
        let volume = 2.0 + (i as f32 * 0.01);
        let is_buy = i % 2 == 0;
        let timestamp = 1000 + i * 20;

        extractor.add_transaction(create_tx(volume, is_buy, timestamp));
    }

    // Phase 2: Sudden spike (100 large buy transactions)
    for i in 0..100 {
        let volume = 15.0; // Large volume
        let timestamp = 4000 + i * 8; // 8ms spacing = rapid

        extractor.add_transaction(create_tx(volume, true, timestamp));
    }

    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // Could be RES_TRANSITION or RES_FAKE depending on coherence
    // Both are valid for a fake pump scenario
    assert!(
        result.signal == FrbSignal::ResFake || result.signal == FrbSignal::ResTransition,
        "Expected RES_FAKE or RES_TRANSITION for fake pump, got {:?}",
        result.signal
    );

    // Medium resonance at best
    assert!(
        result.resonance_score < 0.7,
        "Expected medium-low resonance, got {}",
        result.resonance_score
    );

    println!("✓ Fake Pump Test:");
    println!("  Resonance Score: {:.3}", result.resonance_score);
    println!("  Signal: {:?}", result.signal);
    println!(
        "  Coherence Map: [{:.3}, {:.3}, {:.3}]",
        result.coherence_map[0], result.coherence_map[1], result.coherence_map[2]
    );
}

#[test]
fn test_scenario_transitional_phase() {
    // Scenario: Market in transition - some resonance but not dominant
    // Short and medium bands active, long band building

    let mut extractor = BandExtractor::with_config(BandConfig::with_window(WINDOW_5S));
    let analyzer = ResonanceAnalyzer::new();

    // Simulate transitional activity: 200 transactions with moderate pattern
    for i in 0..200 {
        let volume = 5.0 + (i as f32 * 0.05);
        let is_buy = i % 3 != 0; // 67% buys
        let timestamp = 1000 + i * 20; // 20ms spacing

        extractor.add_transaction(create_tx(volume, is_buy, timestamp));
    }

    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // Should classify as RES_TRANSITION
    assert_eq!(
        result.signal,
        FrbSignal::ResTransition,
        "Expected RES_TRANSITION for transitional phase, got {:?}",
        result.signal
    );

    // Medium resonance
    assert!(
        result.resonance_score >= 0.3 && result.resonance_score < 0.7,
        "Expected medium resonance (0.3-0.7), got {}",
        result.resonance_score
    );

    // Short and medium bands should be active
    assert!(result.band_profiles[0].is_significant());
    assert!(result.band_profiles[1].is_significant());

    println!("✓ Transitional Phase Test:");
    println!("  Resonance Score: {:.3}", result.resonance_score);
    println!("  Trend Likelihood: {:.3}", result.trend_likelihood);
    println!("  Signal: {:?}", result.signal);
}

#[test]
fn test_scenario_distribution_phase() {
    // Scenario: Distribution (selling) across multiple scales
    // All bands active but sell-heavy

    let mut extractor = BandExtractor::with_config(BandConfig::with_window(WINDOW_5S));
    let analyzer = ResonanceAnalyzer::new();

    // Simulate coordinated selling across 300 transactions
    for i in 0..300 {
        let volume = 8.0 + (i as f32 * 0.08);
        let is_buy = i % 5 == 0; // 80% sells
        let timestamp = 1000 + i * 15;

        extractor.add_transaction(create_tx(volume, is_buy, timestamp));
    }

    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // All bands should be significant
    assert!(result.band_profiles[0].is_significant());
    assert!(result.band_profiles[1].is_significant());
    assert!(result.band_profiles[2].is_significant());

    // Buy/sell ratio should indicate selling
    for profile in &result.band_profiles {
        if let Some(ratio) = profile.buy_sell_ratio {
            assert!(ratio < 1.0, "Expected sell-heavy ratio, got {}", ratio);
        }
    }

    // Can still have good resonance during distribution
    // The signal classification should account for sell pressure
    println!("✓ Distribution Phase Test:");
    println!("  Resonance Score: {:.3}", result.resonance_score);
    println!("  Signal: {:?}", result.signal);
    println!(
        "  Buy/Sell Ratios: [{:?}, {:?}, {:?}]",
        result.band_profiles[0].buy_sell_ratio,
        result.band_profiles[1].buy_sell_ratio,
        result.band_profiles[2].buy_sell_ratio
    );
}

#[test]
fn test_analyze_resonance_helper() {
    // Test the convenience helper function

    let mut extractor = BandExtractor::new();

    // Add test transactions
    for i in 0..150 {
        let volume = 5.0 + (i as f32 * 0.1);
        let is_buy = i % 3 != 0;
        let timestamp = 1000 + i * 25;

        extractor.add_transaction(create_tx(volume, is_buy, timestamp));
    }

    let bands = extractor.extract_bands();
    let result = analyze_resonance(bands);

    // Should produce valid result
    assert!(result.is_significant());
    assert!(result.resonance_score >= 0.0 && result.resonance_score <= 1.0);
    assert!(result.trend_likelihood >= 0.0 && result.trend_likelihood <= 1.0);
    assert_ne!(result.signal, FrbSignal::ResHold);

    println!("✓ Helper Function Test:");
    println!("  Result is significant: {}", result.is_significant());
    println!("  Signal: {:?}", result.signal);
}

#[test]
fn test_coherence_with_mpcf_weighting() {
    // Test resonance detection with MPCF actor score weighting

    let config = BandConfig::with_window(WINDOW_5S).with_mpcf_weighting();
    let mut extractor = BandExtractor::with_config(config);
    let analyzer = ResonanceAnalyzer::new();

    // Add transactions with varying actor scores
    for i in 0..200 {
        let volume = 10.0;
        let is_buy = true;
        let timestamp = 1000 + i * 20;
        let actor_score = if i < 100 { 0.9 } else { 0.2 }; // Human-like then bot-like

        let tx = create_tx(volume, is_buy, timestamp).with_actor_score(actor_score);
        extractor.add_transaction(tx);
    }

    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // Should detect pattern change in recent bands
    assert!(result.is_significant());

    println!("✓ MPCF Weighting Test:");
    println!("  Resonance Score: {:.3}", result.resonance_score);
    println!("  Signal: {:?}", result.signal);
}

#[test]
fn test_coherence_with_sobp_weighting() {
    // Test resonance detection with SOBP intensity weighting

    let config = BandConfig::with_window(WINDOW_5S).with_sobp_weighting();
    let mut extractor = BandExtractor::with_config(config);
    let analyzer = ResonanceAnalyzer::new();

    // Add transactions with varying intensity
    for i in 0..200 {
        let volume = 10.0;
        let is_buy = true;
        let timestamp = 1000 + i * 20;
        let intensity = 2.0 + (i as f32 * 0.01); // Increasing intensity

        let tx = create_tx(volume, is_buy, timestamp).with_intensity(intensity);
        extractor.add_transaction(tx);
    }

    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // Should show momentum with increasing intensity
    assert!(result.is_significant());
    assert!(result.trend_likelihood > 0.4);

    println!("✓ SOBP Weighting Test:");
    println!("  Resonance Score: {:.3}", result.resonance_score);
    println!("  Trend Likelihood: {:.3}", result.trend_likelihood);
}

#[test]
fn test_multi_timeframe_resonance() {
    // Test resonance across different time windows

    let windows = [WINDOW_5S, WINDOW_15S, WINDOW_60S];

    for &window in &windows {
        let config = BandConfig::with_window(window);
        let mut extractor = BandExtractor::with_config(config);
        let analyzer = ResonanceAnalyzer::new();

        // Add sufficient transactions
        for i in 0..300 {
            let volume = 5.0 + (i as f32 * 0.1);
            let is_buy = i % 3 != 0;
            let timestamp = 1000 + i * 20;

            extractor.add_transaction(create_tx(volume, is_buy, timestamp));
        }

        let bands = extractor.extract_bands();
        let result = analyzer.analyze(bands);

        // All time windows should produce valid results
        assert!(result.resonance_score >= 0.0 && result.resonance_score <= 1.0);
        assert!(result.is_significant());

        println!("✓ Timeframe Test ({}ms window):", window);
        println!("  Resonance Score: {:.3}", result.resonance_score);
    }
}
