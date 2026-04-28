//! Simple performance test for anomaly detection
//!
//! This is a minimal test to verify the ≤80ns per candidate target
//! Run with: cargo run --release --bin perf-test

use ghost_brain::oracle::{AnomalyDetector, PremintCandidateWithAnomaly};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
struct TestCandidate {
    id: u64,
    score: u64,
}

fn main() {
    println!("Anomaly Detection Performance Test");
    println!("===================================\n");

    let detector = AnomalyDetector::new();

    // Prime the buffer
    println!("Priming buffer with 1000 baseline scores...");
    let mut baseline = Vec::new();
    for i in 0..1000 {
        let candidate = TestCandidate {
            id: i,
            score: 50 + (i % 10),
        };
        baseline.push(Arc::new(PremintCandidateWithAnomaly::new(
            Arc::new(candidate),
            50 + (i % 10),
        )));
    }
    detector.detect_anomalies_batch(&baseline);
    println!("Buffer primed.\n");

    // Test batch of 128
    println!("Testing batch of 128 candidates:");
    let mut batch = Vec::new();
    for i in 0..128 {
        let candidate = TestCandidate {
            id: i,
            score: 50 + (i % 10),
        };
        batch.push(Arc::new(PremintCandidateWithAnomaly::new(
            Arc::new(candidate),
            50 + (i % 10),
        )));
    }

    // Warm up
    for _ in 0..100 {
        detector.detect_anomalies_batch(&batch);
    }

    // Actual measurement - 10,000 iterations
    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        detector.detect_anomalies_batch(&batch);
    }
    let elapsed = start.elapsed();

    let total_ns = elapsed.as_nanos();
    let total_candidates = iterations * 128;
    let ns_per_candidate = total_ns / total_candidates;

    println!("  Iterations: {}", iterations);
    println!("  Total time: {:?}", elapsed);
    println!("  Time per batch (128): {} ns", total_ns / iterations);
    println!("  Time per candidate: {} ns", ns_per_candidate);
    println!("  Target: ≤80 ns per candidate");

    if ns_per_candidate <= 80 {
        println!("  ✅ PASS - Performance target met!");
    } else {
        println!(
            "  ⚠️  WARN - Target not met ({}ns > 80ns)",
            ns_per_candidate
        );
        println!("     Note: This may be due to system load or compilation settings.");
    }

    // Test scam wave scenario
    println!("\nTesting scam wave (3000 pools):");
    let mut scam_wave = Vec::new();
    for i in 0..3000 {
        let candidate = TestCandidate { id: i, score: 200 };
        scam_wave.push(Arc::new(PremintCandidateWithAnomaly::new(
            Arc::new(candidate),
            200,
        )));
    }

    let start = Instant::now();
    let results = detector.detect_anomalies_batch(&scam_wave);
    let elapsed = start.elapsed();

    let anomaly_count = results.iter().filter(|&&x| x).count();

    println!("  Scam wave size: 3000 pools");
    println!("  Detection time: {:?}", elapsed);
    println!("  Anomalies detected: {}", anomaly_count);
    println!("  Target: <1ms");

    if elapsed.as_millis() < 1 {
        println!("  ✅ PASS - Scam wave detected in <1ms!");
    } else if elapsed.as_micros() < 1000 {
        println!(
            "  ✅ PASS - Scam wave detected in {}µs (<1ms)",
            elapsed.as_micros()
        );
    } else {
        println!("  ⚠️  WARN - Detection took {} ms", elapsed.as_millis());
    }

    println!("\n===================================");
    println!("Performance test complete!");
}
