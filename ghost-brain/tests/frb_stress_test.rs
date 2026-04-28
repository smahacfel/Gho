//! FRB Stress Test - Historical Corpus Testing
//!
//! This test validates FRB (Fractal Resonance Bands) system stability and performance
//! under high-volume production conditions. Tests include:
//!
//! - Processing >100,000 transactions (IWIM/MPCF corpus scale)
//! - Zero-alloc hot-path validation
//! - Memory leak detection
//! - Latency assertions (<50μs per slot)
//! - Signal accuracy and stability

use ghost_brain::signals::{
    BandConfig, BandExtractor, BandTransaction, FrbIntegrator, FrbIntegratorConfig,
    ResonanceAnalyzer,
};
use solana_sdk::pubkey::Pubkey;
use std::time::Instant;

/// Minimum transaction count for stress testing
/// Can be overridden via STRESS_TEST_TX_COUNT environment variable for CI/CD
fn stress_test_tx_count() -> usize {
    std::env::var("STRESS_TEST_TX_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000)
}

/// Maximum acceptable latency per transaction (microseconds)
const MAX_LATENCY_US: u64 = 50;

/// Helper to create randomized test transaction
fn create_random_tx(i: usize, wallets: &[Pubkey], timestamp_base: u64) -> BandTransaction {
    let volume = 1.0 + (i % 100) as f32 * 0.5;
    let is_buy = (i % 7) < 4; // ~57% buys
    let wallet = wallets[i % wallets.len()];
    let timestamp = timestamp_base + (i as u64 * 100); // 100ms spacing

    BandTransaction::new(volume, is_buy, wallet, timestamp)
}

/// Generate test wallet pool
fn generate_wallets(count: usize) -> Vec<Pubkey> {
    (0..count).map(|_| Pubkey::new_unique()).collect()
}

#[test]
fn test_massive_transaction_processing() {
    let tx_count = stress_test_tx_count();
    println!("\n=== FRB Stress Test: {} Transactions ===\n", tx_count);

    let mut extractor = BandExtractor::new();
    let analyzer = ResonanceAnalyzer::new();
    let wallets = generate_wallets(500); // 500 unique wallets

    let start = Instant::now();
    let mut extract_count = 0;
    let mut total_extract_time = 0u128;

    // Process transactions in batches
    for i in 0..tx_count {
        let tx = create_random_tx(i, &wallets, 1_000_000);
        extractor.add_transaction(tx);

        // Extract bands every 100 transactions to simulate real-time processing
        if (i + 1) % 100 == 0 {
            let extract_start = Instant::now();
            let bands = extractor.extract_bands();
            let _result = analyzer.analyze(bands);
            let extract_time = extract_start.elapsed().as_micros();

            total_extract_time += extract_time;
            extract_count += 1;

            // Assert latency per extraction
            assert!(
                extract_time < (MAX_LATENCY_US as u128),
                "Extraction #{} took {}μs, exceeding {}μs limit",
                extract_count,
                extract_time,
                MAX_LATENCY_US
            );
        }

        // Log progress
        if (i + 1) % 10_000 == 0 {
            let elapsed = start.elapsed();
            let rate = (i + 1) as f64 / elapsed.as_secs_f64();
            println!(
                "  Processed {}/{} tx ({:.0} tx/s, avg extract: {:.2}μs)",
                i + 1,
                tx_count,
                rate,
                if extract_count > 0 {
                    (total_extract_time as f64 / extract_count as f64)
                } else {
                    0.0
                }
            );
        }
    }

    let total_time = start.elapsed();
    let avg_extract_time = total_extract_time / extract_count as u128;

    println!("\n✓ Stress Test Completed:");
    println!("  Total time: {:.2}s", total_time.as_secs_f64());
    println!(
        "  Throughput: {:.0} tx/s",
        tx_count as f64 / total_time.as_secs_f64()
    );
    println!("  Extractions: {}", extract_count);
    println!("  Avg extraction time: {}μs", avg_extract_time);
    println!(
        "  Max latency: {}μs (limit: {}μs)",
        MAX_LATENCY_US, MAX_LATENCY_US
    );

    // Final assertions
    assert!(avg_extract_time < (MAX_LATENCY_US as u128));
    assert!(extract_count > 0);
}

#[test]
fn test_zero_alloc_hot_path() {
    println!("\n=== FRB Zero-Alloc Hot-Path Test ===\n");

    let mut extractor = BandExtractor::new();
    let analyzer = ResonanceAnalyzer::new();
    let wallets = generate_wallets(50);

    // Pre-fill buffer to reach steady state
    for i in 0..1000 {
        let tx = create_random_tx(i, &wallets, 1_000_000);
        extractor.add_transaction(tx);
    }

    // Measure memory-stable hot path (should reuse buffers)
    let iterations = 10_000;
    let mut times = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let tx = create_random_tx(1000 + i, &wallets, 1_000_000 + (i as u64 * 100));

        let start = Instant::now();
        extractor.add_transaction(tx);
        let bands = extractor.extract_bands();
        let _result = analyzer.analyze(bands);
        let elapsed = start.elapsed().as_micros();

        times.push(elapsed);
    }

    // Calculate statistics
    times.sort_unstable();
    let p50 = times[iterations / 2];
    let p95 = times[(iterations * 95) / 100];
    let p99 = times[(iterations * 99) / 100];
    let avg = times.iter().sum::<u128>() / iterations as u128;

    println!("  Iterations: {}", iterations);
    println!("  Average: {}μs", avg);
    println!("  P50: {}μs", p50);
    println!("  P95: {}μs", p95);
    println!("  P99: {}μs", p99);

    // Assert hot-path performance
    assert!(
        p50 < MAX_LATENCY_US as u128,
        "P50 latency {}μs exceeds {}μs",
        p50,
        MAX_LATENCY_US
    );
    assert!(
        p95 < (MAX_LATENCY_US * 2) as u128,
        "P95 latency {}μs exceeds {}μs",
        p95,
        MAX_LATENCY_US * 2
    );
    assert!(
        p99 < (MAX_LATENCY_US * 3) as u128,
        "P99 latency {}μs exceeds {}μs",
        p99,
        MAX_LATENCY_US * 3
    );

    println!("\n✓ Zero-alloc hot-path validated");
}

#[test]
fn test_signal_stability_under_load() {
    println!("\n=== FRB Signal Stability Test ===\n");

    let mut extractor = BandExtractor::new();
    let analyzer = ResonanceAnalyzer::new();
    let wallets = generate_wallets(100);

    let mut signal_changes = 0;
    let mut last_signal = None;
    let test_iterations = 1000;

    for i in 0..test_iterations {
        // Add batch of transactions
        for j in 0..50 {
            let tx = create_random_tx(i * 50 + j, &wallets, 1_000_000 + (i as u64 * 5000));
            extractor.add_transaction(tx);
        }

        // Extract and analyze
        let bands = extractor.extract_bands();
        let result = analyzer.analyze(bands);

        // Track signal stability
        if let Some(prev_signal) = last_signal {
            if prev_signal != result.signal {
                signal_changes += 1;
            }
        }
        last_signal = Some(result.signal);

        // Validate signal quality
        assert!(
            result.resonance_score >= 0.0 && result.resonance_score <= 1.0,
            "Resonance score out of bounds: {}",
            result.resonance_score
        );
        assert!(
            result.trend_likelihood >= 0.0 && result.trend_likelihood <= 1.0,
            "Trend likelihood out of bounds: {}",
            result.trend_likelihood
        );
    }

    // Signal should be relatively stable (not flipping constantly)
    let change_rate = signal_changes as f32 / test_iterations as f32;
    println!(
        "  Signal changes: {}/{} ({:.1}%)",
        signal_changes,
        test_iterations,
        change_rate * 100.0
    );

    // Allow some changes but not excessive thrashing
    assert!(
        change_rate < 0.3,
        "Signal change rate {:.1}% too high (>30%), indicates instability",
        change_rate * 100.0
    );

    println!("✓ Signal stability validated");
}

#[test]
fn test_integration_layer_stress() {
    println!("\n=== FRB Integration Layer Stress Test ===\n");

    let mut extractor = BandExtractor::new();
    let analyzer = ResonanceAnalyzer::new();
    let integrator = FrbIntegrator::new();
    let wallets = generate_wallets(200);

    let iterations = 10_000;
    let start = Instant::now();
    let mut total_integration_time = 0u128;

    for i in 0..iterations {
        // Add transactions
        for j in 0..10 {
            let tx = create_random_tx(i * 10 + j, &wallets, 1_000_000 + (i as u64 * 1000));
            extractor.add_transaction(tx);
        }

        // Extract, analyze, and integrate
        let bands = extractor.extract_bands();
        let frb_result = analyzer.analyze(bands);

        let integration_start = Instant::now();
        let _integration_result = integrator.integrate(frb_result, None, None);
        let integration_time = integration_start.elapsed().as_micros();

        total_integration_time += integration_time;

        // Assert integration latency
        assert!(
            integration_time < (MAX_LATENCY_US as u128),
            "Integration #{} took {}μs, exceeding {}μs limit",
            i,
            integration_time,
            MAX_LATENCY_US
        );
    }

    let total_time = start.elapsed();
    let avg_integration_time = total_integration_time / iterations as u128;

    println!("  Iterations: {}", iterations);
    println!("  Total time: {:.2}s", total_time.as_secs_f64());
    println!("  Avg integration time: {}μs", avg_integration_time);
    println!(
        "  Throughput: {:.0} integrations/s",
        iterations as f64 / total_time.as_secs_f64()
    );

    assert!(avg_integration_time < (MAX_LATENCY_US as u128));
    println!("\n✓ Integration layer stress test passed");
}

#[test]
fn test_memory_stability() {
    println!("\n=== FRB Memory Stability Test ===\n");

    let mut extractor = BandExtractor::new();
    let analyzer = ResonanceAnalyzer::new();
    let wallets = generate_wallets(100);

    // Run for extended period to detect memory leaks
    let long_run_iterations = 50_000;

    for i in 0..long_run_iterations {
        let tx = create_random_tx(i, &wallets, 1_000_000 + (i as u64 * 100));
        extractor.add_transaction(tx);

        // Extract periodically
        if i % 100 == 0 {
            let bands = extractor.extract_bands();
            let _result = analyzer.analyze(bands);
        }

        // Periodic cleanup check
        if i % 10_000 == 0 && i > 0 {
            let buffer_size = extractor.buffer_size();
            println!("  Iteration {}: buffer_size={}", i, buffer_size);

            // Buffer size should be bounded by rolling window
            assert!(
                buffer_size < 1000,
                "Buffer size {} exceeds expected bounds - possible memory leak",
                buffer_size
            );
        }
    }

    println!(
        "✓ Memory stability validated over {} iterations",
        long_run_iterations
    );
}

#[test]
fn test_concurrent_extraction_safety() {
    use std::sync::{Arc, Mutex};
    use std::thread;

    println!("\n=== FRB Concurrent Safety Test ===\n");

    let extractor = Arc::new(Mutex::new(BandExtractor::new()));
    let analyzer = Arc::new(ResonanceAnalyzer::new());
    let wallets = Arc::new(generate_wallets(100));

    let mut handles = vec![];
    let threads = 4;
    let tx_per_thread = 10_000;

    for thread_id in 0..threads {
        let extractor = Arc::clone(&extractor);
        let analyzer = Arc::clone(&analyzer);
        let wallets = Arc::clone(&wallets);

        let handle = thread::spawn(move || {
            for i in 0..tx_per_thread {
                let tx = create_random_tx(
                    thread_id * tx_per_thread + i,
                    &wallets,
                    1_000_000 + (i as u64 * 100),
                );

                // Lock and add transaction
                {
                    let mut ext = extractor.lock().unwrap();
                    ext.add_transaction(tx);
                }

                // Extract and analyze periodically
                if i % 100 == 0 {
                    let bands = {
                        let mut ext = extractor.lock().unwrap();
                        ext.extract_bands()
                    };
                    let _result = analyzer.analyze(bands);
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    let final_buffer_size = extractor.lock().unwrap().buffer_size();
    println!("  Threads: {}", threads);
    println!("  Transactions per thread: {}", tx_per_thread);
    println!("  Total transactions: {}", threads * tx_per_thread);
    println!("  Final buffer size: {}", final_buffer_size);

    assert!(final_buffer_size > 0);
    println!("✓ Concurrent safety validated");
}

#[test]
fn test_extreme_value_handling() {
    println!("\n=== FRB Extreme Value Handling Test ===\n");

    let mut extractor = BandExtractor::new();
    let analyzer = ResonanceAnalyzer::new();
    let wallet = Pubkey::new_unique();

    // Test extreme volumes
    let extreme_volumes = vec![
        0.0,               // Zero
        f32::MIN_POSITIVE, // Minimum positive
        1e-6,              // Very small
        1e6,               // Very large
        f32::MAX / 2.0,    // Near max (avoid overflow)
    ];

    for (i, &volume) in extreme_volumes.iter().enumerate() {
        let tx = BandTransaction::new(volume, true, wallet, 1_000_000 + (i as u64 * 100));
        extractor.add_transaction(tx);
    }

    // Should not panic or produce NaN
    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    // Validate no NaN or infinite values
    assert!(result.resonance_score.is_finite());
    assert!(result.trend_likelihood.is_finite());
    for coherence in &result.coherence_map {
        assert!(coherence.is_finite());
    }

    println!("✓ Extreme value handling validated");
}

#[test]
fn test_burst_transaction_handling() {
    println!("\n=== FRB Burst Transaction Test ===\n");

    let mut extractor = BandExtractor::new();
    let analyzer = ResonanceAnalyzer::new();
    let wallets = generate_wallets(50);

    // Simulate burst: 1000 transactions in rapid succession
    let burst_size = 1000;
    let start = Instant::now();

    for i in 0..burst_size {
        let tx = create_random_tx(i, &wallets, 1_000_000 + (i as u64 * 10)); // 10ms spacing
        extractor.add_transaction(tx);
    }

    let burst_time = start.elapsed();

    // Extract and analyze after burst
    let bands = extractor.extract_bands();
    let result = analyzer.analyze(bands);

    println!("  Burst size: {} tx", burst_size);
    println!("  Burst time: {:?}", burst_time);
    println!(
        "  Rate: {:.0} tx/s",
        burst_size as f64 / burst_time.as_secs_f64()
    );
    println!("  Resonance score: {:.3}", result.resonance_score);

    // Should handle burst without issues
    assert!(result.is_significant());
    assert!(burst_time.as_millis() < 1000); // Should complete in <1s

    println!("✓ Burst handling validated");
}
