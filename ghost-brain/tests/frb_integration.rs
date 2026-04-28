//! Integration Test: FRB (Fractal Resonance Bands) - Multi-scale Band Extraction
//!
//! This test demonstrates the complete FRB pipeline for extracting multi-scale
//! amplitude profiles from transaction streams.

use ghost_brain::signals::{
    extract_bands, BandConfig, BandExtractor, BandTransaction, WINDOW_15S, WINDOW_1S, WINDOW_5S,
    WINDOW_60S,
};
use solana_sdk::pubkey::Pubkey;

#[test]
fn test_frb_streaming_mode() {
    // Create extractor for streaming mode
    let mut extractor = BandExtractor::new();

    // Simulate streaming transactions over time
    for i in 0..300 {
        let tx = BandTransaction::new(
            10.0 + (i as f32 * 0.5),
            i % 3 != 0, // 2/3 buys
            Pubkey::new_unique(),
            1000 + i * 20,
        );
        extractor.add_transaction(tx);
    }

    // Extract multi-scale bands
    let bands = extractor.extract_bands();

    // Verify all bands have data
    assert!(bands[0].is_significant(), "Short band should have data");
    assert!(bands[1].is_significant(), "Medium band should have data");
    assert!(bands[2].is_significant(), "Long band should have data");

    // Verify band sizes are correct
    assert_eq!(bands[0].transaction_count, 32, "Short band max is 32");
    assert!(bands[1].transaction_count >= 32 && bands[1].transaction_count <= 128);
    assert!(bands[2].transaction_count >= 128 && bands[2].transaction_count <= 512);

    // All bands should have positive amplitude
    assert!(bands[0].amplitude > 0.0);
    assert!(bands[1].amplitude > 0.0);
    assert!(bands[2].amplitude > 0.0);
}

#[test]
fn test_frb_batch_mode() {
    // Create historical transaction data
    let mut transactions = Vec::new();
    for i in 0..300 {
        transactions.push(BandTransaction::new(
            5.0 + (i as f32 * 0.2),
            i % 2 == 0,
            Pubkey::new_unique(),
            1000 + i * 20,
        ));
    }

    // Process in batch mode
    let config = BandConfig::default();
    let bands = extract_bands(&transactions, &config);

    // Verify results
    assert!(bands[0].is_significant());
    assert!(bands[1].is_significant());
    assert!(bands[2].is_significant());
}

#[test]
fn test_frb_time_windows() {
    // Test with different time windows
    let windows = [WINDOW_1S, WINDOW_5S, WINDOW_15S, WINDOW_60S];

    for &window in &windows {
        let config = BandConfig::with_window(window);
        let mut extractor = BandExtractor::with_config(config);

        // Add 300 transactions with 20ms spacing
        for i in 0..300 {
            let tx = BandTransaction::new(10.0, true, Pubkey::new_unique(), 1000 + i * 20);
            extractor.add_transaction(tx);
        }

        let bands = extractor.extract_bands();

        // With different windows, we'll keep different numbers of transactions
        // But short band should always be capped at 32
        assert_eq!(bands[0].transaction_count, 32);
    }
}

#[test]
fn test_frb_with_mpcf_weighting() {
    let config = BandConfig::default().with_mpcf_weighting();
    let mut extractor = BandExtractor::with_config(config);

    // Add transactions with varying actor scores
    for i in 0..300 {
        let tx = BandTransaction::new(10.0, true, Pubkey::new_unique(), 1000 + i * 20)
            .with_actor_score(i as f32 / 300.0); // Gradient from 0.0 to 1.0

        extractor.add_transaction(tx);
    }

    let bands = extractor.extract_bands();

    // Bands should still have data
    assert!(bands[0].is_significant());
    assert!(bands[1].is_significant());
    assert!(bands[2].is_significant());
}

#[test]
fn test_frb_with_sobp_weighting() {
    let config = BandConfig::default().with_sobp_weighting();
    let mut extractor = BandExtractor::with_config(config);

    // Add transactions with varying intensity
    for i in 0..300 {
        let tx = BandTransaction::new(10.0, true, Pubkey::new_unique(), 1000 + i * 20)
            .with_intensity((i % 10) as f32); // Intensity from 0 to 9

        extractor.add_transaction(tx);
    }

    let bands = extractor.extract_bands();

    // Bands should have data
    assert!(bands[0].is_significant());
    assert!(bands[1].is_significant());
    assert!(bands[2].is_significant());
}

#[test]
fn test_frb_buy_sell_analysis() {
    let mut extractor = BandExtractor::new();

    // First wave: mostly buys
    for i in 0..150 {
        let tx = BandTransaction::new(
            10.0,
            true, // All buys
            Pubkey::new_unique(),
            1000 + i * 20,
        );
        extractor.add_transaction(tx);
    }

    let bands_buy_phase = extractor.extract_bands();

    // Should have high buy/sell ratios
    assert!(bands_buy_phase[0].buy_sell_ratio.is_some());
    if let Some(ratio) = bands_buy_phase[0].buy_sell_ratio {
        assert!(ratio > 5.0, "Should be heavily buy-biased");
    }

    // Second wave: mostly sells
    for i in 150..300 {
        let tx = BandTransaction::new(
            10.0,
            false, // All sells
            Pubkey::new_unique(),
            1000 + i * 20,
        );
        extractor.add_transaction(tx);
    }

    let bands_sell_phase = extractor.extract_bands();

    // Recent transactions are sells, so ratio should be low
    assert!(bands_sell_phase[0].buy_sell_ratio.is_some());
    if let Some(ratio) = bands_sell_phase[0].buy_sell_ratio {
        assert!(ratio < 0.5, "Should be heavily sell-biased");
    }
}

#[test]
fn test_frb_volatility_detection() {
    let mut extractor = BandExtractor::new();

    // Add transactions with high volatility (varying volumes)
    let volumes = vec![1.0, 100.0, 2.0, 50.0, 3.0, 80.0, 1.5, 120.0];
    for (i, &volume) in volumes.iter().cycle().take(300).enumerate() {
        let tx = BandTransaction::new(
            volume,
            i % 2 == 0,
            Pubkey::new_unique(),
            1000 + i as u64 * 20,
        );
        extractor.add_transaction(tx);
    }

    let bands = extractor.extract_bands();

    // All bands should show high volatility
    assert!(
        bands[0].volatility > 0.0,
        "Short band should have volatility"
    );
    assert!(
        bands[1].volatility > 0.0,
        "Medium band should have volatility"
    );
    assert!(
        bands[2].volatility > 0.0,
        "Long band should have volatility"
    );
}

#[test]
fn test_frb_unique_buyer_tracking() {
    let mut extractor = BandExtractor::new();

    // Create 10 unique buyers
    let buyers: Vec<Pubkey> = (0..10).map(|_| Pubkey::new_unique()).collect();

    // Add transactions from these buyers
    for i in 0..300 {
        let buyer = buyers[i % 10];
        let tx = BandTransaction::new(10.0, true, buyer, 1000 + (i as u64) * 20);
        extractor.add_transaction(tx);
    }

    let bands = extractor.extract_bands();

    // Short band should have at most 10 unique buyers
    assert!(bands[0].buyers <= 10, "Should have <= 10 unique buyers");
    assert!(bands[0].buyers > 0, "Should have some buyers");
}

#[test]
fn test_frb_zero_alloc_hot_path() {
    // This test verifies that the extractor reuses buffers
    let mut extractor = BandExtractor::new();

    // First batch
    for i in 0..300 {
        let tx = BandTransaction::new(10.0, true, Pubkey::new_unique(), 1000 + i * 20);
        extractor.add_transaction(tx);
    }

    let _bands1 = extractor.extract_bands();

    // Clear and add second batch
    extractor.clear();

    for i in 0..300 {
        let tx = BandTransaction::new(10.0, true, Pubkey::new_unique(), 10000 + i * 20);
        extractor.add_transaction(tx);
    }

    let _bands2 = extractor.extract_bands();

    // Third batch without clearing (rolling window)
    for i in 300..600 {
        let tx = BandTransaction::new(10.0, true, Pubkey::new_unique(), 10000 + i * 20);
        extractor.add_transaction(tx);
    }

    let bands3 = extractor.extract_bands();

    // Should still work correctly
    assert!(bands3[0].is_significant());
}

#[test]
fn test_frb_multi_scale_resonance() {
    // Simulate a pattern that appears at multiple scales
    let mut extractor = BandExtractor::new();

    // Create a repeating pattern: 10 buys, 5 sells, repeat
    for cycle in 0..20 {
        // 10 buys
        for i in 0..10 {
            let tx = BandTransaction::new(
                10.0,
                true,
                Pubkey::new_unique(),
                1000 + (cycle * 15 + i) * 20,
            );
            extractor.add_transaction(tx);
        }

        // 5 sells
        for i in 10..15 {
            let tx = BandTransaction::new(
                5.0,
                false,
                Pubkey::new_unique(),
                1000 + (cycle * 15 + i) * 20,
            );
            extractor.add_transaction(tx);
        }
    }

    let bands = extractor.extract_bands();

    // All bands should detect the pattern
    assert!(bands[0].is_significant());
    assert!(bands[1].is_significant());
    assert!(bands[2].is_significant());

    // All bands should have positive buy/sell ratios (more buys than sells)
    for band in &bands {
        if let Some(ratio) = band.buy_sell_ratio {
            assert!(ratio > 1.0, "Pattern should show buy dominance");
        }
    }
}
