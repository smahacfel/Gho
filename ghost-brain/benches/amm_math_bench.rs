use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ghost_brain::{AmmPool, BatchSwapInput};

fn bench_amm_operations(c: &mut Criterion) {
    let pool = AmmPool::new(1_000_000_000, 2_000_000_000, 30).unwrap();
    let amount_in = 1_000_000u128;

    // Benchmark checked swap
    c.bench_function("simulate_swap_checked", |b| {
        b.iter(|| black_box(pool.simulate_swap(black_box(amount_in), black_box(true))))
    });

    // Benchmark unchecked swap
    c.bench_function("simulate_swap_unchecked", |b| {
        b.iter(|| unsafe {
            black_box(pool.simulate_swap_unchecked(black_box(amount_in), black_box(true)))
        })
    });

    // Benchmark minimal output
    c.bench_function("get_swap_output_only", |b| {
        b.iter(|| black_box(pool.get_swap_output_only(black_box(amount_in), black_box(true))))
    });

    // Benchmark fast path
    c.bench_function("get_amount_out_fast", |b| {
        b.iter(|| {
            black_box(pool.get_amount_out_fast(
                black_box(pool.reserve_a),
                black_box(pool.reserve_b),
                black_box(amount_in),
            ))
        })
    });

    // Benchmark batch processing
    let batch_input = BatchSwapInput {
        amounts_in: [1_000_000; 8],
    };

    c.bench_function("simulate_batch_8x", |b| {
        b.iter(|| black_box(pool.simulate_batch(black_box(&batch_input), black_box(true))))
    });

    // Benchmark individual swaps (for comparison with batch)
    c.bench_function("simulate_swap_8x_individual", |b| {
        b.iter(|| {
            for &amount in &batch_input.amounts_in {
                black_box(pool.get_amount_out_fast(
                    black_box(pool.reserve_a),
                    black_box(pool.reserve_b),
                    black_box(amount),
                ));
            }
        })
    });
}

fn bench_different_pool_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_sizes");

    let sizes = vec![
        ("small", 1_000_000u128, 1_000_000u128),
        ("medium", 100_000_000u128, 100_000_000u128),
        ("large", 10_000_000_000u128, 10_000_000_000u128),
        ("huge", 1_000_000_000_000u128, 1_000_000_000_000u128),
    ];

    for (name, reserve_a, reserve_b) in sizes {
        let pool = AmmPool::new(reserve_a, reserve_b, 30).unwrap();
        let amount_in = reserve_a / 1000; // 0.1% of pool

        group.bench_with_input(
            BenchmarkId::new("checked", name),
            &(pool, amount_in),
            |b, (p, a)| b.iter(|| black_box(p.simulate_swap(black_box(*a), black_box(true)))),
        );

        group.bench_with_input(
            BenchmarkId::new("unchecked", name),
            &(pool, amount_in),
            |b, (p, a)| {
                b.iter(|| unsafe {
                    black_box(p.simulate_swap_unchecked(black_box(*a), black_box(true)))
                })
            },
        );
    }

    group.finish();
}

fn bench_different_fees(c: &mut Criterion) {
    let mut group = c.benchmark_group("fee_rates");

    let fees = vec![
        ("0bps", 0u16),
        ("30bps", 30u16),     // 0.3% standard
        ("100bps", 100u16),   // 1%
        ("500bps", 500u16),   // 5%
        ("1000bps", 1000u16), // 10%
    ];

    for (name, fee_bps) in fees {
        let pool = AmmPool::new(1_000_000_000, 2_000_000_000, fee_bps).unwrap();
        let amount_in = 1_000_000u128;

        group.bench_with_input(
            BenchmarkId::new("unchecked", name),
            &(pool, amount_in),
            |b, (p, a)| {
                b.iter(|| unsafe {
                    black_box(p.simulate_swap_unchecked(black_box(*a), black_box(true)))
                })
            },
        );
    }

    group.finish();
}

fn bench_monte_carlo_simulation(c: &mut Criterion) {
    let pool = AmmPool::new(1_000_000_000, 2_000_000_000, 30).unwrap();

    // Simulate 10,000 swaps (typical Monte Carlo run)
    c.bench_function("monte_carlo_10k_checked", |b| {
        b.iter(|| {
            let mut sum = 0u128;
            for i in 1..=10_000 {
                let amount = 100_000 + (i % 1000) * 1000;
                if let Ok(result) = pool.simulate_swap(amount, true) {
                    sum = sum.wrapping_add(result.amount_out);
                }
            }
            black_box(sum)
        })
    });

    c.bench_function("monte_carlo_10k_unchecked", |b| {
        b.iter(|| {
            let mut sum = 0u128;
            for i in 1..=10_000 {
                let amount = 100_000 + (i % 1000) * 1000;
                let result = unsafe { pool.simulate_swap_unchecked(amount, true) };
                sum = sum.wrapping_add(result.amount_out);
            }
            black_box(sum)
        })
    });

    c.bench_function("monte_carlo_10k_batch", |b| {
        b.iter(|| {
            let mut sum = 0u128;
            for batch_idx in 0..1250 {
                // 10,000 / 8 = 1,250 batches
                let mut amounts = [0u128; 8];
                for i in 0..8 {
                    let idx = batch_idx * 8 + i;
                    amounts[i] = 100_000 + ((idx % 1000) * 1000) as u128;
                }

                let batch_input = BatchSwapInput {
                    amounts_in: amounts,
                };
                let batch_output = pool.simulate_batch(&batch_input, true);

                for &amount_out in &batch_output.amounts_out {
                    sum = sum.wrapping_add(amount_out);
                }
            }
            black_box(sum)
        })
    });
}

criterion_group!(
    benches,
    bench_amm_operations,
    bench_different_pool_sizes,
    bench_different_fees,
    bench_monte_carlo_simulation
);
criterion_main!(benches);
