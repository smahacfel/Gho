//! Simple performance validation test for QEDD and MCI
//!
//! This test validates that QEDD and MCI meet the p99 < 5ms requirement

#[cfg(test)]
mod perf_tests {
    use ghost_brain::config::mci_config::MciConfig;
    use ghost_brain::config::qedd_config::QeddConfig;
    use ghost_brain::mci::MciEngine;
    use ghost_brain::qedd::QeddEngine;
    use ghost_brain::signals::MarketSignals;
    use std::time::Instant;

    #[test]
    fn test_qedd_performance() {
        let config = QeddConfig::default();
        let engine = QeddEngine::new(config);
        let signals = MarketSignals::mock();

        // Warm up
        for _ in 0..10 {
            let _ = engine.compute_qedd_sync(&signals);
        }

        // Measure 1000 iterations
        // Note: Vec allocation here is for measurement only, not part of the hot path being tested
        let iterations = 1000;
        let mut timings: Vec<u128> = Vec::with_capacity(iterations);

        for _ in 0..iterations {
            let start = Instant::now();
            let _result = engine.compute_qedd_sync(&signals);
            let elapsed = start.elapsed().as_micros();
            timings.push(elapsed);
        }

        timings.sort_unstable();

        let p50 = timings[iterations / 2];
        let p99 = timings[(iterations * 99) / 100];
        let p999 = timings[(iterations * 999) / 1000];
        let mean: u128 = timings.iter().sum::<u128>() / iterations as u128;

        println!("QEDD Performance (μs):");
        println!("  Mean: {}", mean);
        println!("  p50:  {}", p50);
        println!("  p99:  {}", p99);
        println!("  p999: {}", p999);

        // Goal: p99 < 5ms = 5000μs
        assert!(p99 < 5000, "QEDD p99 latency {} μs exceeds 5ms goal", p99);
        println!("✓ QEDD meets p99 < 5ms requirement ({} μs)", p99);
    }

    #[test]
    fn test_mci_performance() {
        let config = MciConfig::default();
        let engine = MciEngine::new(config);
        let signals = MarketSignals::mock();

        // Warm up
        for _ in 0..10 {
            let _ = engine.compute_mci(&signals);
        }

        // Measure 1000 iterations
        // Note: Vec allocation here is for measurement only, not part of the hot path being tested
        let iterations = 1000;
        let mut timings: Vec<u128> = Vec::with_capacity(iterations);

        for _ in 0..iterations {
            let start = Instant::now();
            let _result = engine.compute_mci(&signals);
            let elapsed = start.elapsed().as_micros();
            timings.push(elapsed);
        }

        timings.sort_unstable();

        let p50 = timings[iterations / 2];
        let p99 = timings[(iterations * 99) / 100];
        let p999 = timings[(iterations * 999) / 1000];
        let mean: u128 = timings.iter().sum::<u128>() / iterations as u128;

        println!("MCI Performance (μs):");
        println!("  Mean: {}", mean);
        println!("  p50:  {}", p50);
        println!("  p99:  {}", p99);
        println!("  p999: {}", p999);

        // Goal: p99 < 5ms = 5000μs
        assert!(p99 < 5000, "MCI p99 latency {} μs exceeds 5ms goal", p99);
        println!("✓ MCI meets p99 < 5ms requirement ({} μs)", p99);
    }

    #[test]
    fn test_combined_qedd_mci_performance() {
        let qedd_config = QeddConfig::default();
        let mci_config = MciConfig::default();
        let qedd_engine = QeddEngine::new(qedd_config);
        let mci_engine = MciEngine::new(mci_config);
        let signals = MarketSignals::mock();

        // Warm up
        for _ in 0..10 {
            let _ = qedd_engine.compute_qedd_sync(&signals);
            let _ = mci_engine.compute_mci(&signals);
        }

        // Measure 1000 iterations
        // Note: Vec allocation here is for measurement only, not part of the hot path being tested
        let iterations = 1000;
        let mut timings: Vec<u128> = Vec::with_capacity(iterations);

        for _ in 0..iterations {
            let start = Instant::now();
            let _qedd_result = qedd_engine.compute_qedd_sync(&signals);
            let _mci_result = mci_engine.compute_mci(&signals);
            let elapsed = start.elapsed().as_micros();
            timings.push(elapsed);
        }

        timings.sort_unstable();

        let p50 = timings[iterations / 2];
        let p99 = timings[(iterations * 99) / 100];
        let p999 = timings[(iterations * 999) / 1000];
        let mean: u128 = timings.iter().sum::<u128>() / iterations as u128;

        println!("Combined QEDD+MCI Performance (μs):");
        println!("  Mean: {}", mean);
        println!("  p50:  {}", p50);
        println!("  p99:  {}", p99);
        println!("  p999: {}", p999);

        // Goal: Combined should still be well under 5ms
        assert!(
            p99 < 5000,
            "Combined QEDD+MCI p99 latency {} μs exceeds 5ms goal",
            p99
        );
        println!(
            "✓ Combined QEDD+MCI meets p99 < 5ms requirement ({} μs)",
            p99
        );
    }
}
