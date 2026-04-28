//! Benchmark for PumpCurveStateCache hot-path operations
//!
//! Target: <50μs for insert operations

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ghost_brain::pumpfun::{CurveSnapshot, EarlySwapEvent, PumpCurveStateCache};
use solana_sdk::pubkey::Pubkey;

/// Benchmark snapshot insertion (hot path)
fn bench_snapshot_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_insert");

    group.bench_function("single_curve", |b| {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();
        let mut slot = 0u64;

        b.iter(|| {
            let snapshot = CurveSnapshot::new(
                black_box(1_000_000_000),
                black_box(1_000_000_000_000),
                black_box(100),
                black_box(slot),
            );
            cache.update_snapshot(black_box(curve), snapshot);
            slot += 1;
        });
    });

    group.bench_function("multiple_curves", |b| {
        let cache = PumpCurveStateCache::new();
        let curves: Vec<Pubkey> = (0..100).map(|_| Pubkey::new_unique()).collect();
        let mut idx = 0;

        b.iter(|| {
            let curve = curves[idx % curves.len()];
            let snapshot = CurveSnapshot::new(
                black_box(1_000_000_000),
                black_box(1_000_000_000_000),
                black_box(100),
                black_box(idx as u64),
            );
            cache.update_snapshot(black_box(curve), snapshot);
            idx += 1;
        });
    });

    group.finish();
}

/// Benchmark swap event insertion
fn bench_swap_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("swap_insert");

    group.bench_function("single_curve", |b| {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();
        let mut slot = 0u64;

        b.iter(|| {
            let event =
                EarlySwapEvent::new(black_box(100_000_000), black_box(true), black_box(slot));
            cache.update_swap(black_box(curve), event);
            slot += 1;
        });
    });

    group.bench_function("multiple_curves", |b| {
        let cache = PumpCurveStateCache::new();
        let curves: Vec<Pubkey> = (0..100).map(|_| Pubkey::new_unique()).collect();
        let mut idx = 0;

        b.iter(|| {
            let curve = curves[idx % curves.len()];
            let event = EarlySwapEvent::new(
                black_box(100_000_000),
                black_box(idx % 2 == 0),
                black_box(idx as u64),
            );
            cache.update_swap(black_box(curve), event);
            idx += 1;
        });
    });

    group.finish();
}

/// Benchmark snapshot retrieval
fn bench_snapshot_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_get");

    group.bench_function("cache_hit", |b| {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, 1);
        cache.update_snapshot(curve, snapshot);

        b.iter(|| {
            black_box(cache.get_snapshot(black_box(&curve)));
        });
    });

    group.bench_function("cache_miss", |b| {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        b.iter(|| {
            black_box(cache.get_snapshot(black_box(&curve)));
        });
    });

    group.finish();
}

/// Benchmark early swaps retrieval
fn bench_swaps_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("swaps_get");

    for swap_count in [10, 20, 32].iter() {
        group.bench_with_input(
            BenchmarkId::new("valid_swaps", swap_count),
            swap_count,
            |b, &count| {
                let cache = PumpCurveStateCache::new();
                let curve = Pubkey::new_unique();

                // Fill buffer with swaps
                for i in 0..count {
                    let event = EarlySwapEvent::new(i as u64 * 100_000_000, i % 2 == 0, i as u64);
                    cache.update_swap(curve, event);
                }

                b.iter(|| {
                    black_box(cache.get_early_swaps(black_box(&curve)));
                });
            },
        );
    }

    group.finish();
}

/// Benchmark combined state retrieval
fn bench_state_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_get");

    group.bench_function("snapshot_and_swaps", |b| {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        // Setup snapshot
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, 1);
        cache.update_snapshot(curve, snapshot);

        // Add swaps
        for i in 0..20 {
            let event = EarlySwapEvent::new(i * 100_000_000, i % 2 == 0, i);
            cache.update_swap(curve, event);
        }

        b.iter(|| {
            black_box(cache.get_state(black_box(&curve)));
        });
    });

    group.finish();
}

/// Benchmark cache under load (mixed operations)
fn bench_mixed_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_operations");

    group.bench_function("realistic_workload", |b| {
        let cache = PumpCurveStateCache::new();
        let curves: Vec<Pubkey> = (0..10).map(|_| Pubkey::new_unique()).collect();
        let mut op_count = 0u64;

        b.iter(|| {
            let curve_idx = (op_count as usize) % curves.len();
            let curve = curves[curve_idx];

            match op_count % 4 {
                // 25% snapshot updates
                0 => {
                    let snapshot =
                        CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, op_count);
                    cache.update_snapshot(curve, snapshot);
                }
                // 50% swap inserts
                1 | 2 => {
                    let event = EarlySwapEvent::new(100_000_000, op_count % 2 == 0, op_count);
                    cache.update_swap(curve, event);
                }
                // 25% reads
                _ => {
                    black_box(cache.get_state(&curve));
                }
            }

            op_count += 1;
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_snapshot_insert,
    bench_swap_insert,
    bench_snapshot_get,
    bench_swaps_get,
    bench_state_get,
    bench_mixed_operations,
);
criterion_main!(benches);
