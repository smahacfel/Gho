//! Example demonstrating Full HyperPrediction JSONL Telemetry
//!
//! This example shows how the telemetry system records complete scoring results
//! with all subcomponents (QASS, SSMI, MPCF, IWIM, SOBP, QOFSV, QEDD, MCI, etc.)
//! in JSONL format with immediate flush to disk.
//!
//! Run with:
//! ```bash
//! cargo run --example hyper_prediction_telemetry_demo
//! ```
//!
//! Output will be written to `logs/hyper_prediction_scoring.jsonl`

use ghost_brain::config::FallbackTracker;
use ghost_brain::oracle::hyper_prediction::{AnalysisPhase, HyperPredictionResult};
use ghost_brain::oracle::RiskLevel;
use ghost_brain::telemetry::{TelemetryConfig, TelemetryRecorder};
use std::path::PathBuf;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("ghost_brain=info")
        .init();

    println!("🔬 HyperPrediction JSONL Telemetry Demo");
    println!("{}", "=".repeat(60));

    // Create telemetry config
    let telemetry_config = TelemetryConfig {
        log_path: PathBuf::from("logs/hyper_prediction_scoring.jsonl"),
        channel_buffer_size: 100,
        enabled: true,
    };

    println!("📝 Telemetry output: {:?}", telemetry_config.log_path);

    // Create telemetry recorder
    let telemetry = TelemetryRecorder::new(telemetry_config.clone()).await?;
    println!("✅ Telemetry recorder initialized");

    println!("\n🎯 Scenario 1: Full data available (all submodules succeeded)");
    println!("{}", "-".repeat(60));

    // Create a mock HyperPredictionResult with all components populated
    let full_result = HyperPredictionResult {
        score: 85,
        passed: true,
        risk_level: RiskLevel::Low,
        analysis_phase: AnalysisPhase::FullAnalysis,
        analysis_started_at: std::time::Instant::now(),
        ssmi_result: None, // Note: would be Some in real scenario
        mpcf_result: None,
        iwim_result: None,
        praecog_result: None,
        mesa_result: None,
        scr_score: Some(0.15), // Low bot activity
        ulvf_divergence: Some(0.08),
        ulvf_curl: Some(0.05),
        povc_cluster: Some(1), // Organic Hype
        shadow_progress: Some(65),
        shadow_price_ratio: Some(1.45),
        base_score: 80,
        processing_time_us: 1_800_000, // 1.8s
        interpretation: "Strong organic growth with minimal bot activity".to_string(),
        chaos_result: None,
        resonance_result: None,
        gene_safety_result: None,
        hunter_score: Some(88),
        qedd_result: None,
        mci_result: None,
        qman_result: None,
        ligma_result: None,
        cluster_result: None,
        paradox_state: None,
        should_delay_entry: false,
        recommended_delay_ms: 0,
        second_wave_result: None,
        survivor_score_result: None,
        fractal_verdict: None,
        tcf_result: None,
        fallback_tracker: FallbackTracker::new(),
    };

    // Create mock transaction data
    let txs_full = vec![
        serde_json::json!({
            "signature": "sig_abc123",
            "slot": 245_123_456,
            "timestamp_ms": 1733500000000u64,
            "signer": "wallet_human_1",
            "is_buy": true,
            "volume_sol": 2.5,
        }),
        serde_json::json!({
            "signature": "sig_def456",
            "slot": 245_123_457,
            "timestamp_ms": 1733500001000u64,
            "signer": "wallet_human_2",
            "is_buy": true,
            "volume_sol": 1.8,
        }),
        serde_json::json!({
            "signature": "sig_ghi789",
            "slot": 245_123_458,
            "timestamp_ms": 1733500002500u64,
            "signer": "wallet_human_3",
            "is_buy": true,
            "volume_sol": 0.9,
        }),
    ];

    // Log the scoring event
    telemetry.log_hyper_prediction_scoring("pool_amm_abc123xyz", &full_result, txs_full);

    println!("✅ Logged full scoring result");

    // Give time for async write and flush
    sleep(Duration::from_millis(100)).await;

    println!("\n🎯 Scenario 2: Partial data (InsufficientData for some modules)");
    println!("{}", "-".repeat(60));

    // Create a result with many None values (InsufficientData)
    let partial_result = HyperPredictionResult {
        score: 45,
        passed: false,
        risk_level: RiskLevel::High,
        analysis_phase: AnalysisPhase::EarlyStage,
        analysis_started_at: std::time::Instant::now(),
        ssmi_result: None, // InsufficientData
        mpcf_result: None, // InsufficientData
        iwim_result: None, // InsufficientData
        praecog_result: None,
        mesa_result: None,
        scr_score: Some(0.75), // High bot activity
        ulvf_divergence: Some(0.42),
        ulvf_curl: None,       // InsufficientData
        povc_cluster: Some(2), // Bot Noise
        shadow_progress: None, // InsufficientData
        shadow_price_ratio: None,
        base_score: 40,
        processing_time_us: 950_000, // 0.95s (fast decision due to lack of data)
        interpretation: "Insufficient data with high bot signals - rejected".to_string(),
        chaos_result: None,
        resonance_result: None,
        gene_safety_result: None,
        hunter_score: None, // External API timeout
        qedd_result: None,
        mci_result: None,
        qman_result: None,
        ligma_result: None,
        cluster_result: None,
        paradox_state: None,
        should_delay_entry: false,
        recommended_delay_ms: 0,
        second_wave_result: None,
        survivor_score_result: None,
        fractal_verdict: None,
        tcf_result: None,
        fallback_tracker: FallbackTracker::new(),
    };

    let txs_partial = vec![serde_json::json!({
        "signature": "sig_bot_001",
        "slot": 245_123_500,
        "timestamp_ms": 1733500010000u64,
        "signer": "bot_wallet_1",
        "is_buy": true,
        "volume_sol": 5.0,
    })];

    telemetry.log_hyper_prediction_scoring("pool_amm_def456uvw", &partial_result, txs_partial);

    println!("✅ Logged partial scoring result (many InsufficientData fields)");

    // Give time for async write and flush
    sleep(Duration::from_millis(100)).await;

    println!("\n🎯 Scenario 3: Timeout/Pipeline failure");
    println!("{}", "-".repeat(60));

    let timeout_result = HyperPredictionResult {
        score: 0,
        passed: false,
        risk_level: RiskLevel::VeryHigh,
        analysis_phase: AnalysisPhase::EarlyStage,
        analysis_started_at: std::time::Instant::now(),
        ssmi_result: None,
        mpcf_result: None,
        iwim_result: None,
        praecog_result: None,
        mesa_result: None,
        scr_score: None, // Timeout
        ulvf_divergence: None,
        ulvf_curl: None,
        povc_cluster: None,
        shadow_progress: None,
        shadow_price_ratio: None,
        base_score: 0,
        processing_time_us: 2_100_000, // Exceeded 2s timeout
        interpretation: "Pipeline timeout - candidate skipped".to_string(),
        chaos_result: None,
        resonance_result: None,
        gene_safety_result: None,
        hunter_score: None,
        qedd_result: None,
        mci_result: None,
        qman_result: None,
        ligma_result: None,
        cluster_result: None,
        paradox_state: None,
        should_delay_entry: false,
        recommended_delay_ms: 0,
        second_wave_result: None,
        survivor_score_result: None,
        fractal_verdict: None,
        tcf_result: None,
        fallback_tracker: FallbackTracker::new(),
    };

    telemetry.log_hyper_prediction_scoring("pool_amm_timeout_xyz", &timeout_result, vec![]);

    println!("✅ Logged timeout result (all components failed)");

    // Give time for async write and flush
    sleep(Duration::from_millis(200)).await;

    println!(
        "\n📂 Telemetry log written to: {:?}",
        telemetry_config.log_path
    );
    println!("\n💡 View logs with:");
    println!("   cat logs/hyper_prediction_scoring.jsonl | jq '.'");
    println!("\n💡 View specific fields:");
    println!("   cat logs/hyper_prediction_scoring.jsonl | jq '.candidate_id, .final_score_initial, .passed, .risk_level'");
    println!("   cat logs/hyper_prediction_scoring.jsonl | jq 'select(.qass != null)'");
    println!("\n✨ Demo complete!");
    println!("\n📊 Summary:");
    println!("  - 3 scoring events logged");
    println!("  - Immediate flush to disk after each event");
    println!("  - InsufficientData represented as null in JSON");
    println!(
        "  - All 8+ subcomponents tracked (QASS, SSMI, MPCF, IWIM, SOBP, QOFSV, QEDD, MCI, etc.)"
    );

    Ok(())
}
