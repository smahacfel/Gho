//! Scoring Engine Benchmarks - Task 5 (Zadanie 5)
//!
//! Performance benchmarks for the Ghost Predator scoring engine.
//!
//! ## Target Metrics (from CONSOLIDATED_TASKS_SCORING_ENGINE.md)
//! - Cycle time: ≤400ms
//! - Total scoring time: ≤4800ms (without Gatekeeper)
//! - GUNSHOT response: <1ms
//! - Memory per pool: <1MB
//! - Concurrent pools: >100

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ghost_brain::oracle::predator_strategy::{
    calculate_quality_early_stage, calculate_quality_for_cycle, calculate_quality_full_analysis,
    calculate_weighted_geometric_mean, get_gunshot_threshold, CYCLE_WEIGHTS,
};
use ghost_brain::oracle::survivor_score::{SurvivorScoreCalculator, SurvivorScoreInput};
use ghost_brain::oracle::tcf::{
    observation_from_ghost_signals, MarketObservation, TrendCohesionField,
};

// =============================================================================
// SurvivorScore Calculation Benchmarks
// =============================================================================

fn benchmark_survivor_score_calculation(c: &mut Criterion) {
    let calculator = SurvivorScoreCalculator::new();
    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.75),
        iwim_threat_score: Some(0.2),
        cluster_risk_score: Some(0.15),
        sobp_momentum: Some(0.6),
        qman_score: Some(0.7),
        chaos_pump_prob: Some(0.65),
        mpcf_organic_ratio: Some(0.8),
        mesa_organic_likeness: Some(0.75),
        scr_bot_score: Some(0.2),
        unique_wallet_ratio: Some(0.7),
        mesa_wash_likeness: Some(0.25),
        qman_exit_signal: false,
        price_crash_detected: false,
        paradox_anomaly: false,
        ligma_tradability_score: Some(0.8),
        ligma_psi: Some(0.5),
        ligma_liquidity_trap_risk: Some(0.15),
        tx_count: Some(25),
        ..Default::default()
    };

    c.bench_function("survivor_score_calculate", |b| {
        b.iter(|| black_box(calculator.calculate(black_box(&input))))
    });
}

fn benchmark_survivor_score_with_iwim(c: &mut Criterion) {
    let calculator = SurvivorScoreCalculator::new();
    let input = SurvivorScoreInput {
        qedd_survival_60s: Some(0.75),
        iwim_threat_score: Some(0.2),
        cluster_risk_score: Some(0.15),
        sobp_momentum: Some(0.6),
        qman_score: Some(0.7),
        chaos_pump_prob: Some(0.65),
        mpcf_organic_ratio: Some(0.8),
        mesa_organic_likeness: Some(0.75),
        scr_bot_score: Some(0.2),
        unique_wallet_ratio: Some(0.7),
        mesa_wash_likeness: Some(0.25),
        ..Default::default()
    };

    c.bench_function("survivor_score_calculate_with_iwim", |b| {
        b.iter(|| black_box(calculator.calculate_with_iwim(black_box(&input))))
    });
}

fn benchmark_survivor_score_minimal_input(c: &mut Criterion) {
    let calculator = SurvivorScoreCalculator::new();
    let input = SurvivorScoreInput::default();

    c.bench_function("survivor_score_minimal_input", |b| {
        b.iter(|| black_box(calculator.calculate(black_box(&input))))
    });
}

// =============================================================================
// Weighted Geometric Mean Benchmarks
// =============================================================================

fn benchmark_weighted_geometric_mean(c: &mut Criterion) {
    let scores: Vec<f32> = vec![
        75.0, 78.0, 80.0, 82.0, 83.0, 85.0, 86.0, 87.0, 88.0, 89.0, 90.0, 91.0,
    ];

    c.bench_function("weighted_geometric_mean_12scores", |b| {
        b.iter(|| black_box(calculate_weighted_geometric_mean(black_box(&scores))))
    });
}

fn benchmark_weighted_geometric_mean_uniform(c: &mut Criterion) {
    let scores: Vec<f32> = vec![80.0; 12];

    c.bench_function("weighted_geometric_mean_uniform", |b| {
        b.iter(|| black_box(calculate_weighted_geometric_mean(black_box(&scores))))
    });
}

// =============================================================================
// GUNSHOT Threshold Benchmarks
// =============================================================================

fn benchmark_gunshot_threshold_lookup(c: &mut Criterion) {
    c.bench_function("gunshot_threshold_lookup", |b| {
        let mut cycle = 0usize;
        b.iter(|| {
            let threshold = get_gunshot_threshold(black_box(cycle));
            cycle = (cycle + 1) % 12;
            black_box(threshold)
        })
    });
}

fn benchmark_gunshot_check(c: &mut Criterion) {
    c.bench_function("gunshot_check_single", |b| {
        let score = 87.0;
        let cycle = 6;
        b.iter(|| {
            let threshold = get_gunshot_threshold(black_box(cycle));
            let triggered = black_box(score) >= threshold;
            black_box(triggered)
        })
    });
}

// =============================================================================
// Quality Formula Benchmarks
// =============================================================================

fn benchmark_quality_early_stage(c: &mut Criterion) {
    c.bench_function("quality_early_stage", |b| {
        b.iter(|| {
            black_box(calculate_quality_early_stage(
                black_box(0.8),
                black_box(0.7),
                black_box(0.6),
            ))
        })
    });
}

fn benchmark_quality_full_analysis(c: &mut Criterion) {
    c.bench_function("quality_full_analysis", |b| {
        b.iter(|| {
            black_box(calculate_quality_full_analysis(
                black_box(0.8),
                black_box(0.7),
                black_box(0.2),
                black_box(0.6),
            ))
        })
    });
}

fn benchmark_quality_for_cycle(c: &mut Criterion) {
    c.bench_function("quality_for_cycle_12iterations", |b| {
        b.iter(|| {
            for cycle in 0..12 {
                black_box(calculate_quality_for_cycle(
                    black_box(cycle),
                    black_box(0.8),
                    black_box(0.7),
                    Some(black_box(0.2)),
                    black_box(0.6),
                ));
            }
        })
    });
}

// =============================================================================
// TCF Benchmarks
// =============================================================================

fn benchmark_tcf_update(c: &mut Criterion) {
    let mut tcf = TrendCohesionField::new();
    let observation = MarketObservation::new(0.1, 0.1, 0.6, 0.3, 0.7, 0.5, 0.2);

    c.bench_function("tcf_single_update", |b| {
        b.iter(|| black_box(tcf.update(black_box(&observation))))
    });
}

fn benchmark_tcf_from_signals(c: &mut Criterion) {
    c.bench_function("tcf_observation_from_signals", |b| {
        b.iter(|| {
            black_box(observation_from_ghost_signals(
                black_box(0.15),
                black_box(0.30),
                black_box(0.65),
                black_box(0.8),
                black_box(0.45),
                black_box(0.25),
            ))
        })
    });
}

fn benchmark_tcf_full_cycle(c: &mut Criterion) {
    c.bench_function("tcf_13_cycle_updates", |b| {
        b.iter(|| {
            let mut tcf = TrendCohesionField::new();
            for i in 0..13 {
                let obs = MarketObservation::new(
                    0.1 + 0.02 * i as f64,
                    0.1 + 0.01 * i as f64,
                    0.6,
                    0.3,
                    0.7,
                    0.5,
                    0.2,
                );
                black_box(tcf.update(&obs));
            }
            black_box(tcf.get_tcf_score())
        })
    });
}

// =============================================================================
// Single Cycle Scoring Benchmark (Target: <400ms)
// =============================================================================

fn benchmark_single_cycle_scoring(c: &mut Criterion) {
    let calculator = SurvivorScoreCalculator::new();

    c.bench_function("single_cycle_scoring", |b| {
        b.iter(|| {
            // Simulate one cycle of scoring
            let input = SurvivorScoreInput {
                qedd_survival_60s: Some(0.75),
                sobp_momentum: Some(0.6),
                mpcf_organic_ratio: Some(0.8),
                cluster_risk_score: Some(0.2),
                ligma_tradability_score: Some(0.7),
                ..Default::default()
            };

            let result = calculator.calculate(black_box(&input));
            let score = result.score as f32;

            // TCF update
            let obs = observation_from_ghost_signals(0.1, 0.2, 0.6, 0.7, 0.5, 0.2);

            // TCF modulation (simplified - no state)
            let tcf_score = 0.7;
            let modulated = score * (0.6 + 0.4 * tcf_score as f32);

            // Gunshot check
            let threshold = get_gunshot_threshold(5);
            let _ = modulated >= threshold;

            black_box(modulated)
        })
    });
}

// =============================================================================
// Full Scoring Loop Benchmark (Target: <4800ms for 12 cycles)
// =============================================================================

fn benchmark_full_scoring_loop(c: &mut Criterion) {
    c.bench_function("full_scoring_loop_12cycles", |b| {
        b.iter(|| {
            let calculator = SurvivorScoreCalculator::new();
            let mut tcf = TrendCohesionField::new();
            let mut scores: Vec<f32> = Vec::with_capacity(12);

            for cycle_idx in 0..12 {
                // Cycle scoring
                let input = SurvivorScoreInput {
                    qedd_survival_60s: Some(0.7 + 0.01 * cycle_idx as f32),
                    sobp_momentum: Some(0.5 + 0.02 * cycle_idx as f32),
                    mpcf_organic_ratio: Some(0.7),
                    cluster_risk_score: Some(0.2),
                    ..Default::default()
                };

                let result = calculator.calculate(&input);
                let base_score = result.score as f32;

                // TCF update
                let obs = observation_from_ghost_signals(
                    0.05 + 0.01 * cycle_idx as f64,
                    0.03 + 0.01 * cycle_idx as f64,
                    0.6,
                    0.7,
                    0.5,
                    0.2,
                );
                let tcf_result = tcf.update(&obs);

                // Modulation
                let modulated = if tcf_result.cliff_detected {
                    base_score * 0.6
                } else {
                    base_score * (0.6 + 0.4 * tcf_result.tcf_score as f32)
                };

                // Gunshot check
                let threshold = get_gunshot_threshold(cycle_idx);
                if modulated >= threshold {
                    // Would break here in real code
                }

                scores.push(modulated);
            }

            // Final Verdict
            black_box(calculate_weighted_geometric_mean(&scores))
        })
    });
}

// =============================================================================
// Memory Efficiency Benchmarks
// =============================================================================

fn benchmark_calculator_creation(c: &mut Criterion) {
    c.bench_function("survivor_calculator_creation", |b| {
        b.iter(|| black_box(SurvivorScoreCalculator::new()))
    });
}

fn benchmark_tcf_creation(c: &mut Criterion) {
    c.bench_function("tcf_creation", |b| {
        b.iter(|| black_box(TrendCohesionField::new()))
    });
}

fn benchmark_input_creation(c: &mut Criterion) {
    c.bench_function("survivor_input_creation", |b| {
        b.iter(|| {
            black_box(SurvivorScoreInput {
                qedd_survival_60s: Some(0.75),
                iwim_threat_score: Some(0.2),
                cluster_risk_score: Some(0.15),
                sobp_momentum: Some(0.6),
                qman_score: Some(0.7),
                chaos_pump_prob: Some(0.65),
                mpcf_organic_ratio: Some(0.8),
                mesa_organic_likeness: Some(0.75),
                scr_bot_score: Some(0.2),
                unique_wallet_ratio: Some(0.7),
                mesa_wash_likeness: Some(0.25),
                qman_exit_signal: false,
                price_crash_detected: false,
                paradox_anomaly: false,
                ligma_tradability_score: Some(0.8),
                ligma_psi: Some(0.5),
                ligma_liquidity_trap_risk: Some(0.15),
                tx_count: Some(25),
                ..Default::default()
            })
        })
    });
}

// =============================================================================
// Concurrent Pool Scoring Benchmark (Target: >100 pools)
// =============================================================================

fn benchmark_concurrent_pool_scoring(c: &mut Criterion) {
    c.bench_function("concurrent_100_pool_scoring", |b| {
        b.iter(|| {
            // Simulate scoring 100 pools
            let calculator = SurvivorScoreCalculator::new();

            for pool_id in 0..100 {
                let input = SurvivorScoreInput {
                    qedd_survival_60s: Some(0.6 + 0.003 * pool_id as f32),
                    sobp_momentum: Some(0.5 + 0.002 * pool_id as f32),
                    mpcf_organic_ratio: Some(0.7),
                    ..Default::default()
                };

                let result = calculator.calculate(&input);
                black_box(result.score);
            }
        })
    });
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(
    survivor_score_benches,
    benchmark_survivor_score_calculation,
    benchmark_survivor_score_with_iwim,
    benchmark_survivor_score_minimal_input,
);

criterion_group!(
    geometric_mean_benches,
    benchmark_weighted_geometric_mean,
    benchmark_weighted_geometric_mean_uniform,
);

criterion_group!(
    gunshot_benches,
    benchmark_gunshot_threshold_lookup,
    benchmark_gunshot_check,
);

criterion_group!(
    quality_benches,
    benchmark_quality_early_stage,
    benchmark_quality_full_analysis,
    benchmark_quality_for_cycle,
);

criterion_group!(
    tcf_benches,
    benchmark_tcf_update,
    benchmark_tcf_from_signals,
    benchmark_tcf_full_cycle,
);

criterion_group!(
    cycle_benches,
    benchmark_single_cycle_scoring,
    benchmark_full_scoring_loop,
);

criterion_group!(
    memory_benches,
    benchmark_calculator_creation,
    benchmark_tcf_creation,
    benchmark_input_creation,
);

criterion_group!(concurrent_benches, benchmark_concurrent_pool_scoring,);

criterion_main!(
    survivor_score_benches,
    geometric_mean_benches,
    gunshot_benches,
    quality_benches,
    tcf_benches,
    cycle_benches,
    memory_benches,
    concurrent_benches,
);
