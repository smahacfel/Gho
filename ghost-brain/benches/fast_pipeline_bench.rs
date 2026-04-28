use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ghost_brain::fast_pipeline::{
    pop_candidate, push_candidate, recycle_candidate, CacheLinePadding, EnhancedCandidate,
    PremintCandidate, CACHE_LINE_SIZE,
};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Benchmark for pushing candidates to the queue (with Hot/Cold padding)
fn bench_push_candidate(c: &mut Criterion) {
    c.bench_function("push_candidate", |b| {
        b.iter(|| {
            let result = push_candidate(|candidate| {
                // Hot data (first cache line)
                candidate.slot = black_box(12345);
                candidate.timestamp = black_box(1234567890);
                candidate.liquidity_sol = black_box(10.5);
                candidate.base_score = black_box(85);

                // Cold data (after cache barrier)
                candidate.pool_amm_id = black_box(Pubkey::new_unique());
                candidate.amm_program_id = black_box(Pubkey::new_unique());
                candidate.signature = black_box("test_signature".to_string());
            });
            black_box(result)
        });
    });
}

/// Benchmark for popping candidates from the queue
fn bench_pop_candidate(c: &mut Criterion) {
    // Pre-fill the queue with 1000 candidates
    for i in 0..1000 {
        push_candidate(|candidate| {
            candidate.slot = i;
            candidate.base_score = 85;
        })
        .ok();
    }

    c.bench_function("pop_candidate", |b| {
        b.iter(|| {
            let result = pop_candidate();
            // Re-push if we got one to keep the queue full
            if let Some(arc) = result.clone() {
                push_candidate(|candidate| {
                    candidate.slot = arc.slot;
                    candidate.base_score = arc.base_score;
                })
                .ok();
            }
            black_box(result)
        });
    });
}

/// Benchmark for the full push-pop-recycle cycle
fn bench_full_cycle(c: &mut Criterion) {
    c.bench_function("full_cycle", |b| {
        b.iter(|| {
            // Push
            push_candidate(|candidate| {
                candidate.slot = black_box(12345);
                candidate.base_score = black_box(85);
            })
            .ok();

            // Pop
            if let Some(arc) = pop_candidate() {
                // Recycle
                recycle_candidate(arc);
            }
        });
    });
}

/// Benchmark for batch processing at different batch sizes
fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_processing");

    for batch_size in [16, 32, 64, 128, 256].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &batch_size| {
                // Pre-fill queue
                for i in 0..1000 {
                    push_candidate(|candidate| {
                        candidate.slot = i;
                        candidate.base_score = 85;
                    })
                    .ok();
                }

                b.iter(|| {
                    let mut batch = Vec::with_capacity(batch_size);

                    // Collect batch
                    for _ in 0..batch_size {
                        if let Some(arc) = pop_candidate() {
                            batch.push(arc);
                        } else {
                            break;
                        }
                    }

                    // Process batch - accessing HOT data only (cache-efficient)
                    for candidate in &batch {
                        black_box(candidate.slot * 2);
                    }

                    // Recycle batch
                    for arc in batch.drain(..) {
                        let slot = arc.slot;
                        recycle_candidate(arc);
                        // Re-push to keep queue full
                        push_candidate(|candidate| {
                            candidate.slot = slot;
                            candidate.base_score = 85;
                        })
                        .ok();
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark for measuring detect-to-score latency
/// This simulates the time from Seer detection to Oracle scoring
fn bench_detect_to_score_latency(c: &mut Criterion) {
    c.bench_function("detect_to_score_latency", |b| {
        b.iter(|| {
            let start = Instant::now();

            // Simulate Seer detection: push candidate
            push_candidate(|candidate| {
                candidate.slot = black_box(12345);
                candidate.liquidity_sol = black_box(10.5);
                candidate.base_score = black_box(85);
                candidate.pool_amm_id = black_box(Pubkey::new_unique());
            })
            .ok();

            // Simulate Oracle scoring: pop and score (HOT PATH - only hot data)
            if let Some(arc) = pop_candidate() {
                // Scoring operation uses only hot data (first cache line, no false sharing)
                let score = (arc.base_score as f64 * arc.liquidity_sol * 0.1) as u8;
                black_box(score);

                recycle_candidate(arc);
            }

            let elapsed = start.elapsed();
            black_box(elapsed)
        });
    });
}

/// Benchmark for memory leak test - process 50M pool operations
fn bench_memory_leak_test(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_leak");
    group.sample_size(10); // Reduce sample size for long-running test
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("50M_operations", |b| {
        b.iter(|| {
            // Process 1M candidates per iteration (scaled down for benchmarking)
            for i in 0..1_000_000 {
                push_candidate(|candidate| {
                    candidate.slot = i;
                    candidate.base_score = (i % 100) as u8;
                })
                .ok();

                if let Some(arc) = pop_candidate() {
                    recycle_candidate(arc);
                }
            }
        });
    });

    group.finish();
}

/// Benchmark: Hot-only access pattern (simulates scoring loop)
///
/// This benchmark measures cache efficiency by accessing ONLY hot data,
/// which should be in the first cache line and not cause false sharing.
fn bench_hot_only_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_efficiency");

    // Create batch of candidates
    let batch_size = 128;
    let mut candidates: Vec<PremintCandidate> = Vec::with_capacity(batch_size);
    for i in 0..batch_size {
        let mut c = PremintCandidate::default();
        c.slot = i as u64;
        c.liquidity_sol = 10.0 + (i as f64) * 0.1;
        c.base_score = 50 + (i % 50) as u8;
        candidates.push(c);
    }

    group.bench_function("hot_only_scoring", |b| {
        b.iter(|| {
            let mut score_sum = 0u64;
            for candidate in &candidates {
                // Access only hot data (first cache line) - should be cache-efficient
                let score = (candidate.base_score as u64) * (candidate.liquidity_sol as u64);
                score_sum += score;
            }
            black_box(score_sum);
        });
    });

    group.bench_function("hot_and_cold_mixed", |b| {
        b.iter(|| {
            let mut score_sum = 0u64;
            for candidate in &candidates {
                // Access both hot and cold data - crosses cache line barrier
                let score = (candidate.base_score as u64) * (candidate.liquidity_sol as u64);
                // Cold access (after cache barrier)
                let _pool_id = &candidate.pool_amm_id;
                score_sum += score;
            }
            black_box(score_sum);
        });
    });

    group.finish();
}

/// Benchmark: Verify cache line separation doesn't degrade performance
fn bench_cache_separation_overhead(c: &mut Criterion) {
    use std::mem;

    let mut group = c.benchmark_group("cache_separation_verification");

    // Print sizes for reference
    println!(
        "PremintCandidate size: {} bytes",
        mem::size_of::<PremintCandidate>()
    );
    println!(
        "EnhancedCandidate size: {} bytes",
        mem::size_of::<EnhancedCandidate>()
    );
    println!(
        "CacheLinePadding size: {} bytes",
        mem::size_of::<CacheLinePadding>()
    );

    group.bench_function("create_premint_candidate", |b| {
        b.iter(|| {
            let mut c = PremintCandidate::default();
            c.slot = black_box(12345);
            c.base_score = black_box(85);
            c.liquidity_sol = black_box(10.5);
            black_box(c);
        });
    });

    group.bench_function("create_enhanced_candidate", |b| {
        b.iter(|| {
            let mut c = EnhancedCandidate::default();
            c.slot = black_box(12345);
            c.vanity_score = black_box(50);
            c.initial_liquidity_sol = black_box(10.5);
            c.expected_price = Some(black_box(0.001));
            black_box(c);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_push_candidate,
    bench_pop_candidate,
    bench_full_cycle,
    bench_batch_processing,
    bench_detect_to_score_latency,
    bench_memory_leak_test,
    bench_hot_only_access,
    bench_cache_separation_overhead,
);
criterion_main!(benches);
