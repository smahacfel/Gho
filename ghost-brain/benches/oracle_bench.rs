use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ghost_brain::oracle::{AnomalyDetector, PremintCandidateWithAnomaly};
use ghost_brain::oracle::{ScoringWeights, SimpleOracle};
use seer::types::CandidatePool;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

fn create_bench_candidate() -> CandidatePool {
    CandidatePool {
        semantic: ghost_core::EventSemanticEnvelope::default(),
        slot: Some(12345),
        event_ts_ms: Some(1_234_567_890_000),
        event_time: ghost_core::EventTimeMetadata::default(),
        signature: "5".repeat(88),
        amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
            .parse()
            .unwrap(),
        pool_amm_id: Pubkey::new_unique(),
        base_mint: Pubkey::new_unique(),
        quote_mint: Pubkey::new_unique(),
        bonding_curve: Pubkey::new_unique(),
        creator: Pubkey::new_unique(),
        timestamp: 1234567890,
        bonding_curve_progress: Some(0.05),
        initial_liquidity_sol: Some(10.0),
        token_total_supply: Some(1_000_000_000),
        block_time: Some(1234567890),
    }
}

fn benchmark_weighted_scoring(c: &mut Criterion) {
    let oracle = SimpleOracle::new(70);
    let candidate = create_bench_candidate();

    c.bench_function("calculate_weighted_score", |b| {
        b.iter(|| {
            // We use a runtime to run the async function
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async { oracle.score_candidate(black_box(&candidate)).await.unwrap() })
        });
    });
}

fn benchmark_weighted_scoring_sync(c: &mut Criterion) {
    // For the pure scoring calculation (without async overhead)
    let weights = ScoringWeights::default();
    let candidate = create_bench_candidate();

    c.bench_function("weighted_score_hot_path", |b| {
        b.iter(|| {
            // Inline version of calculate_weighted_score for benchmarking
            let mut score: u8 = 50;

            if let Some(liquidity_sol) = black_box(candidate.initial_liquidity_sol) {
                let liquidity_weight = weights.get(ScoringWeights::LIQUIDITY_IDX);
                if liquidity_sol >= 10.0 {
                    score = score.saturating_add(liquidity_weight as u8);
                } else if liquidity_sol >= 5.0 {
                    score = score.saturating_add((liquidity_weight * 0.5) as u8);
                } else if liquidity_sol < 1.0 {
                    score = score.saturating_sub(liquidity_weight as u8);
                }
            } else {
                score = score.saturating_sub(20);
            }

            if let Some(progress) = black_box(candidate.bonding_curve_progress) {
                if progress < 0.1 {
                    let early_bonus = weights.get(ScoringWeights::BONDING_EARLY_BONUS_IDX);
                    score = score.saturating_add(early_bonus as u8);
                } else if progress > 0.8 {
                    let late_penalty = weights.get(ScoringWeights::BONDING_LATE_PENALTY_IDX);
                    score = score.saturating_sub(late_penalty as u8);
                } else if progress > 0.5 {
                    let late_penalty = weights.get(ScoringWeights::BONDING_LATE_PENALTY_IDX);
                    score = score.saturating_sub((late_penalty * 0.4) as u8);
                }
            }

            if let Some(supply) = black_box(candidate.token_total_supply) {
                let supply_weight = weights.get(ScoringWeights::SUPPLY_CAP_BONUS_IDX);
                if supply >= 100_000_000 && supply <= 1_000_000_000 {
                    score = score.saturating_add(supply_weight as u8);
                } else if supply > 10_000_000_000 {
                    score = score.saturating_sub(supply_weight as u8);
                }
            }

            black_box(score.min(100))
        });
    });
}

fn benchmark_weights_access(c: &mut Criterion) {
    let weights = ScoringWeights::default();

    c.bench_function("weights_get_operation", |b| {
        b.iter(|| black_box(weights.get(ScoringWeights::LIQUIDITY_IDX)));
    });
}

fn benchmark_cache_line_alignment(c: &mut Criterion) {
    c.bench_function("weights_creation", |b| {
        b.iter(|| black_box(ScoringWeights::default()));
    });
}

// === Anomaly Detection Benchmarks ===

// Test candidate type for benchmarking
#[derive(Debug, Clone)]
struct BenchCandidate {
    id: u64,
    score: u64,
}

fn create_bench_anomaly_candidate(score: u64) -> Arc<PremintCandidateWithAnomaly<BenchCandidate>> {
    let candidate = BenchCandidate { id: score, score };
    Arc::new(PremintCandidateWithAnomaly::new(Arc::new(candidate), score))
}

fn benchmark_anomaly_detection_batch_128(c: &mut Criterion) {
    let detector = AnomalyDetector::new();

    // Prime the buffer with baseline data
    let mut baseline = Vec::new();
    for i in 0..1000 {
        baseline.push(create_bench_anomaly_candidate(50 + (i % 10)));
    }
    detector.detect_anomalies_batch(&baseline);

    // Create test batch of 128
    let mut batch = Vec::new();
    for i in 0..128 {
        batch.push(create_bench_anomaly_candidate(50 + (i % 10)));
    }

    c.bench_function("anomaly_detection_batch_128", |b| {
        b.iter(|| detector.detect_anomalies_batch(black_box(&batch)));
    });
}

fn benchmark_anomaly_detection_single(c: &mut Criterion) {
    let detector = AnomalyDetector::new();

    // Prime the buffer
    let mut baseline = Vec::new();
    for i in 0..1000 {
        baseline.push(create_bench_anomaly_candidate(50 + (i % 10)));
    }
    detector.detect_anomalies_batch(&baseline);

    // Single candidate
    let single = vec![create_bench_anomaly_candidate(55)];

    c.bench_function("anomaly_detection_single", |b| {
        b.iter(|| detector.detect_anomalies_batch(black_box(&single)));
    });
}

fn benchmark_ring_buffer_push(c: &mut Criterion) {
    use ghost_brain::oracle::RingBuffer;

    let buffer = RingBuffer::new(16384);

    c.bench_function("ring_buffer_push", |b| {
        let mut counter = 0u64;
        b.iter(|| {
            buffer.push(black_box(counter));
            counter = counter.wrapping_add(1);
        });
    });
}

fn benchmark_ema_calculation(c: &mut Criterion) {
    use ghost_brain::oracle::RingBuffer;

    let buffer = RingBuffer::new(16384);

    // Fill buffer with test data
    for i in 0..16384 {
        buffer.push(50 + (i % 20));
    }

    c.bench_function("ema_calculation", |b| {
        b.iter(|| black_box(buffer.calculate_ema(0.1)));
    });
}

fn benchmark_statistics_calculation(c: &mut Criterion) {
    use ghost_brain::oracle::RingBuffer;

    let buffer = RingBuffer::new(16384);

    // Fill buffer with test data
    for i in 0..16384 {
        buffer.push(50 + (i % 20));
    }

    c.bench_function("statistics_calculation", |b| {
        b.iter(|| {
            let mean = buffer.calculate_mean();
            let stddev = buffer.calculate_stddev(mean);
            black_box((mean, stddev))
        });
    });
}

fn benchmark_anomaly_scam_wave_3000(c: &mut Criterion) {
    let detector = AnomalyDetector::new();

    // Prime with normal activity
    let mut baseline = Vec::new();
    for i in 0..200 {
        baseline.push(create_bench_anomaly_candidate(50 + (i % 10)));
    }
    detector.detect_anomalies_batch(&baseline);

    // Create scam wave
    let mut scam_wave = Vec::new();
    for _ in 0..3000 {
        scam_wave.push(create_bench_anomaly_candidate(200));
    }

    c.bench_function("anomaly_scam_wave_3000", |b| {
        b.iter(|| detector.detect_anomalies_batch(black_box(&scam_wave)));
    });
}

// === SSMI (Sub-Slot Microentropy Index) Benchmarks ===

fn benchmark_ssmi_analyze_100tx(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::SubSlotMicroentropy;

    let ssmi = SubSlotMicroentropy::new();
    let timestamps: Vec<u64> = (0..100).map(|i| i * 10 + (i % 7) * 3).collect();

    c.bench_function("ssmi_analyze_100tx", |b| {
        b.iter(|| ssmi.analyze(black_box(&timestamps)));
    });
}

fn benchmark_ssmi_analyze_1000tx(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::SubSlotMicroentropy;

    let ssmi = SubSlotMicroentropy::new();
    let timestamps: Vec<u64> = (0..1000).map(|i| i * 10 + (i % 13) * 5).collect();

    c.bench_function("ssmi_analyze_1000tx", |b| {
        b.iter(|| ssmi.analyze(black_box(&timestamps)));
    });
}

fn benchmark_ssmi_shannon_entropy(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::SubSlotMicroentropy;

    let ssmi = SubSlotMicroentropy::new();
    let timestamps: Vec<u64> = (0..500).map(|i| i * 20 + (i % 11) * 7).collect();

    c.bench_function("ssmi_shannon_entropy_500tx", |b| {
        b.iter(|| ssmi.calculate_shannon_entropy(black_box(&timestamps)));
    });
}

fn benchmark_ssmi_ar_correlation(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::SubSlotMicroentropy;

    let ssmi = SubSlotMicroentropy::new();
    let timestamps: Vec<u64> = (0..500).map(|i| i * 20 + (i % 11) * 7).collect();

    c.bench_function("ssmi_ar_correlation_500tx", |b| {
        b.iter(|| ssmi.calculate_ar_correlation(black_box(&timestamps)));
    });
}

fn benchmark_ssmi_configurable_128bins(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::ssmi::SubSlotMicroentropyConfigurable;

    let ssmi = SubSlotMicroentropyConfigurable::<128>::new();
    let timestamps: Vec<u64> = (0..500).map(|i| i * 10 + (i % 7) * 3).collect();

    c.bench_function("ssmi_128bins_analyze_500tx", |b| {
        b.iter(|| ssmi.analyze(black_box(&timestamps)));
    });
}

// === QASS (Quantum-Style Amplitude Superposition Scoring) Benchmarks ===

fn benchmark_qass_superposition_4waves(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::{HeuristicWave, QuantumAmplitudeScorer};

    let scorer = QuantumAmplitudeScorer::default();
    let waves = vec![
        HeuristicWave::new("ψ_ssmi", 0.7, 0.5, 0.8),
        HeuristicWave::new("ψ_scr", 0.6, 0.3, 0.9),
        HeuristicWave::new("ψ_ulvf", 0.8, 0.6, 0.75),
        HeuristicWave::new("ψ_povc", 0.7, 0.4, 0.8),
    ];

    c.bench_function("qass_superposition_4waves", |b| {
        b.iter(|| scorer.superposition_score(black_box(&waves)));
    });
}

fn benchmark_qass_superposition_8waves(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::{HeuristicWave, QuantumAmplitudeScorer};

    let scorer = QuantumAmplitudeScorer::default();
    let waves = vec![
        HeuristicWave::new("ψ_ssmi", 0.85, 0.8, 0.9),
        HeuristicWave::new("ψ_scr", 0.7, 0.3, 0.85),
        HeuristicWave::new("ψ_ulvf", 0.8, 0.7, 0.8),
        HeuristicWave::new("ψ_povc", 0.95, 0.9, 0.85),
        HeuristicWave::new("ψ_cluster", 0.9, 0.5, 0.9),
        HeuristicWave::new("ψ_profiler", 0.75, 0.6, 0.85),
        HeuristicWave::new("ψ_vision", 0.8, 0.7, 0.6),
        HeuristicWave::new("ψ_shadow", 0.9, 0.7, 0.9),
    ];

    c.bench_function("qass_superposition_8waves", |b| {
        b.iter(|| scorer.superposition_score(black_box(&waves)));
    });
}

fn benchmark_qass_full_score_8waves(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::{HeuristicWave, QuantumAmplitudeScorer};

    let scorer = QuantumAmplitudeScorer::default();
    let waves = vec![
        HeuristicWave::new("ψ_ssmi", 0.85, 0.8, 0.9),
        HeuristicWave::new("ψ_scr", 0.7, 0.3, 0.85),
        HeuristicWave::new("ψ_ulvf", 0.8, 0.7, 0.8),
        HeuristicWave::new("ψ_povc", 0.95, 0.9, 0.85),
        HeuristicWave::new("ψ_cluster", 0.9, 0.5, 0.9),
        HeuristicWave::new("ψ_profiler", 0.75, 0.6, 0.85),
        HeuristicWave::new("ψ_vision", 0.8, 0.7, 0.6),
        HeuristicWave::new("ψ_shadow", 0.9, 0.7, 0.9),
    ];

    c.bench_function("qass_full_score_8waves", |b| {
        b.iter(|| scorer.score(black_box(&waves)));
    });
}

fn benchmark_qass_generic_16waves(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::{HeuristicWave, QuantumAmplitudeScorerN};

    let scorer: QuantumAmplitudeScorerN<16> = QuantumAmplitudeScorerN::new();
    let waves: Vec<HeuristicWave> = (0..16)
        .map(|i| {
            let name = match i {
                0 => "ψ_0",
                1 => "ψ_1",
                2 => "ψ_2",
                3 => "ψ_3",
                4 => "ψ_4",
                5 => "ψ_5",
                6 => "ψ_6",
                7 => "ψ_7",
                8 => "ψ_8",
                9 => "ψ_9",
                10 => "ψ_10",
                11 => "ψ_11",
                12 => "ψ_12",
                13 => "ψ_13",
                14 => "ψ_14",
                _ => "ψ_15",
            };
            HeuristicWave::new(name, 0.5 + (i as f64 * 0.03), 0.1 * i as f64, 0.8)
        })
        .collect();

    c.bench_function("qass_generic_16waves", |b| {
        b.iter(|| scorer.score(black_box(&waves)));
    });
}

// === MPCF (Micro-Payload Cognitive Fingerprint) Benchmarks ===

fn benchmark_mpcf_infer_128bytes(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::mpcf_infer;

    // Create synthetic transaction payload (128 bytes)
    let payload: Vec<u8> = (0..128).map(|i| ((i * 13 + 7) % 256) as u8).collect();

    c.bench_function("mpcf_infer_128bytes", |b| {
        b.iter(|| mpcf_infer(black_box(&payload)));
    });
}

fn benchmark_mpcf_infer_512bytes(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::mpcf_infer;

    // Create synthetic transaction payload (512 bytes - typical tx size)
    let payload: Vec<u8> = (0..512).map(|i| ((i * 17 + 11) % 256) as u8).collect();

    c.bench_function("mpcf_infer_512bytes", |b| {
        b.iter(|| mpcf_infer(black_box(&payload)));
    });
}

fn benchmark_mpcf_infer_1024bytes(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::mpcf_infer;

    // Create synthetic transaction payload (1024 bytes)
    let payload: Vec<u8> = (0..1024).map(|i| ((i * 19 + 13) % 256) as u8).collect();

    c.bench_function("mpcf_infer_1024bytes", |b| {
        b.iter(|| mpcf_infer(black_box(&payload)));
    });
}

fn benchmark_mpcf_infer_4096bytes(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::mpcf_infer;

    // Create synthetic transaction payload (4096 bytes - max size)
    let payload: Vec<u8> = (0..4096).map(|i| ((i * 23 + 17) % 256) as u8).collect();

    c.bench_function("mpcf_infer_4096bytes", |b| {
        b.iter(|| mpcf_infer(black_box(&payload)));
    });
}

fn benchmark_mpcf_bot_pattern(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::mpcf_infer;

    // Create bot-like payload: low entropy, regular pattern
    let mut payload = vec![0u8; 512];
    for i in 0..512 {
        payload[i] = if i % 2 == 0 { 0x00 } else { 0xFF };
    }

    c.bench_function("mpcf_bot_pattern_512bytes", |b| {
        b.iter(|| mpcf_infer(black_box(&payload)));
    });
}

fn benchmark_mpcf_human_pattern(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::mpcf_infer;

    // Create human-like payload: high entropy, diverse bytes
    let payload: Vec<u8> = (0..512).map(|i| ((i * 29 + 31) % 256) as u8).collect();

    c.bench_function("mpcf_human_pattern_512bytes", |b| {
        b.iter(|| mpcf_infer(black_box(&payload)));
    });
}

fn benchmark_mpcf_batch_10k(c: &mut Criterion) {
    use ghost_brain::oracle::ultrafast::mpcf_infer;

    // Prepare 10,000 transaction payloads
    let payloads: Vec<Vec<u8>> = (0..10000)
        .map(|i| (0..512).map(|j| ((i + j * 7) % 256) as u8).collect())
        .collect();

    c.bench_function("mpcf_batch_10k_txs", |b| {
        b.iter(|| {
            for payload in &payloads {
                black_box(mpcf_infer(payload));
            }
        });
    });
}

// =============================================================================
// IWIM (Initial Wallet Intent Mapping) Benchmarks
// =============================================================================

use ghost_brain::oracle::ultrafast::iwim::{iwim_analyze, IwimInput};

/// Generate organic creator pattern for benchmarking
fn create_organic_creator_input() -> IwimInput {
    IwimInput {
        creator_pubkey: [1u8; 32],
        init_slot: Some(10000),
        time_window_ms: 2000,
        transactions: vec![
            vec![0x01; 100], // InitializeMint
            vec![0x02; 120], // InitializeMetadata
            vec![0x03; 150], // InitializePool
        ],
        init_timestamp_ms: Some(1000000),
        synthetic: false,
        pool_id: None,
    }
}

/// Generate rug pull pattern (high IAPP)
fn create_rug_iapp_input() -> IwimInput {
    IwimInput {
        creator_pubkey: [2u8; 32],
        init_slot: Some(10001),
        time_window_ms: 2000,
        transactions: vec![
            vec![0x01; 100], // InitializeMint
            vec![0x02; 120], // InitializeMetadata
            vec![0x03; 150], // InitializePool
            vec![0x04; 80],  // CreateTokenAccount #1
            vec![0x05; 80],  // CreateTokenAccount #2
            vec![0x06; 80],  // CreateTokenAccount #3
        ],
        init_timestamp_ms: Some(1000100),
        synthetic: false,
        pool_id: None,
    }
}

/// Generate sybil burst pattern
fn create_sybil_burst_input() -> IwimInput {
    IwimInput {
        creator_pubkey: [3u8; 32],
        init_slot: Some(10002),
        time_window_ms: 2000,
        transactions: vec![
            vec![0x01; 80],  // CreateAccount
            vec![0x02; 80],  // CreateAccount
            vec![0x03; 80],  // CreateAccount
            vec![0x04; 100], // InitializeMint
            vec![0x05; 120], // InitializeMetadata
            vec![0x06; 150], // InitializePool
        ],
        init_timestamp_ms: Some(1000200),
        synthetic: false,
        pool_id: None,
    }
}

fn benchmark_iwim_organic_creator(c: &mut Criterion) {
    let input = create_organic_creator_input();

    c.bench_function("iwim_organic_creator", |b| {
        b.iter(|| black_box(iwim_analyze(black_box(&input)).unwrap()));
    });
}

fn benchmark_iwim_rug_iapp(c: &mut Criterion) {
    let input = create_rug_iapp_input();

    c.bench_function("iwim_rug_iapp_pattern", |b| {
        b.iter(|| black_box(iwim_analyze(black_box(&input)).unwrap()));
    });
}

fn benchmark_iwim_sybil_burst(c: &mut Criterion) {
    let input = create_sybil_burst_input();

    c.bench_function("iwim_sybil_burst_pattern", |b| {
        b.iter(|| black_box(iwim_analyze(black_box(&input)).unwrap()));
    });
}

fn benchmark_iwim_batch_100(c: &mut Criterion) {
    // Generate 100 mixed inputs
    let inputs: Vec<IwimInput> = (0..100)
        .map(|i| match i % 3 {
            0 => create_organic_creator_input(),
            1 => create_rug_iapp_input(),
            _ => create_sybil_burst_input(),
        })
        .collect();

    c.bench_function("iwim_batch_100_mixed", |b| {
        b.iter(|| {
            for input in &inputs {
                black_box(iwim_analyze(black_box(input)).unwrap());
            }
        });
    });
}

criterion_group!(
    benches,
    benchmark_weighted_scoring,
    benchmark_weighted_scoring_sync,
    benchmark_weights_access,
    benchmark_cache_line_alignment,
    benchmark_anomaly_detection_batch_128,
    benchmark_anomaly_detection_single,
    benchmark_ring_buffer_push,
    benchmark_ema_calculation,
    benchmark_statistics_calculation,
    benchmark_anomaly_scam_wave_3000,
    benchmark_ssmi_analyze_100tx,
    benchmark_ssmi_analyze_1000tx,
    benchmark_ssmi_shannon_entropy,
    benchmark_ssmi_ar_correlation,
    benchmark_ssmi_configurable_128bins,
    benchmark_qass_superposition_4waves,
    benchmark_qass_superposition_8waves,
    benchmark_qass_full_score_8waves,
    benchmark_qass_generic_16waves,
    benchmark_mpcf_infer_128bytes,
    benchmark_mpcf_infer_512bytes,
    benchmark_mpcf_infer_1024bytes,
    benchmark_mpcf_infer_4096bytes,
    benchmark_mpcf_bot_pattern,
    benchmark_mpcf_human_pattern,
    benchmark_mpcf_batch_10k,
    benchmark_iwim_organic_creator,
    benchmark_iwim_rug_iapp,
    benchmark_iwim_sybil_burst,
    benchmark_iwim_batch_100,
);
criterion_main!(benches);
