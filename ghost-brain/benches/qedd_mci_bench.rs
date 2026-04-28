//! Benchmark for QEDD and MCI performance
//!
//! Goal: Verify p99 < 5ms and zero allocations

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ghost_brain::config::mci_config::MciConfig;
use ghost_brain::config::qedd_config::QeddConfig;
use ghost_brain::mci::MciEngine;
use ghost_brain::qedd::QeddEngine;
use ghost_brain::signals::MarketSignals;

fn bench_qedd_computation(c: &mut Criterion) {
    let config = QeddConfig::default();
    let engine = QeddEngine::new(config);

    // Test with different market scenarios
    let signals_mock = MarketSignals::mock();
    let signals_hype = MarketSignals::mock_hype();
    let signals_rug = MarketSignals::mock_rug();
    let signals_stable = MarketSignals::mock_stable();

    let mut group = c.benchmark_group("qedd_compute");

    group.bench_function("mock", |b| {
        b.iter(|| {
            let result = engine.compute_qedd_sync(black_box(&signals_mock));
            black_box(result);
        });
    });

    group.bench_function("hype_scenario", |b| {
        b.iter(|| {
            let result = engine.compute_qedd_sync(black_box(&signals_hype));
            black_box(result);
        });
    });

    group.bench_function("rug_scenario", |b| {
        b.iter(|| {
            let result = engine.compute_qedd_sync(black_box(&signals_rug));
            black_box(result);
        });
    });

    group.bench_function("stable_scenario", |b| {
        b.iter(|| {
            let result = engine.compute_qedd_sync(black_box(&signals_stable));
            black_box(result);
        });
    });

    group.finish();
}

fn bench_mci_computation(c: &mut Criterion) {
    let config = MciConfig::default();
    let engine = MciEngine::new(config);

    // Test with different market scenarios
    let signals_mock = MarketSignals::mock();
    let signals_hype = MarketSignals::mock_hype();
    let signals_rug = MarketSignals::mock_rug();
    let signals_stable = MarketSignals::mock_stable();

    let mut group = c.benchmark_group("mci_compute");

    group.bench_function("mock", |b| {
        b.iter(|| {
            let result = engine.compute_mci(black_box(&signals_mock));
            black_box(result);
        });
    });

    group.bench_function("hype_scenario", |b| {
        b.iter(|| {
            let result = engine.compute_mci(black_box(&signals_hype));
            black_box(result);
        });
    });

    group.bench_function("rug_scenario", |b| {
        b.iter(|| {
            let result = engine.compute_mci(black_box(&signals_rug));
            black_box(result);
        });
    });

    group.bench_function("stable_scenario", |b| {
        b.iter(|| {
            let result = engine.compute_mci(black_box(&signals_stable));
            black_box(result);
        });
    });

    group.finish();
}

fn bench_qedd_with_cancellation(c: &mut Criterion) {
    let config = QeddConfig::default();
    let engine = QeddEngine::new(config);
    let signals = MarketSignals::mock();

    // Without cancellation token
    c.bench_function("qedd_no_cancel", |b| {
        b.iter(|| {
            let result = engine.compute_qedd(black_box(&signals), None);
            black_box(result);
        });
    });

    // With active cancellation token (not cancelled)
    let token = tokio_util::sync::CancellationToken::new();
    c.bench_function("qedd_with_token", |b| {
        b.iter(|| {
            let result = engine.compute_qedd(black_box(&signals), Some(&token));
            black_box(result);
        });
    });
}

fn bench_combined_qedd_mci(c: &mut Criterion) {
    let qedd_config = QeddConfig::default();
    let mci_config = MciConfig::default();
    let qedd_engine = QeddEngine::new(qedd_config);
    let mci_engine = MciEngine::new(mci_config);
    let signals = MarketSignals::mock();

    c.bench_function("qedd_mci_combined", |b| {
        b.iter(|| {
            let qedd_result = qedd_engine.compute_qedd_sync(black_box(&signals));
            let mci_result = mci_engine.compute_mci(black_box(&signals));
            black_box((qedd_result, mci_result));
        });
    });
}

criterion_group!(
    benches,
    bench_qedd_computation,
    bench_mci_computation,
    bench_qedd_with_cancellation,
    bench_combined_qedd_mci
);
criterion_main!(benches);
