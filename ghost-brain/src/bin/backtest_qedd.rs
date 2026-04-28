//! QEDD Backtesting Tool
//!
//! Run backtests on historical data using calibrated QEDD parameters.
//!
//! Usage:
//!   ghost-brain-backtest-qedd [OPTIONS]
//!
//! Options:
//!   --dataset-dir <PATH>     Path to datasets directory [default: datasets/dry_run]
//!   --config-file <PATH>     Path to calibrated config JSON file
//!   --output-dir <PATH>      Output directory for backtest results [default: backtest_output]
//!   --export-json            Export results to JSON
//!   --export-csv             Export results to CSV
//!   --export-parquet         Export results to Parquet
//!   --help                   Print help information

use anyhow::{Context, Result};
use clap::Parser;
use ghost_brain::calibration::DatasetLoader;
use ghost_brain::config::qedd_config::QeddConfig;
use ghost_brain::qedd::QeddEngine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "ghost-brain-backtest-qedd")]
#[command(about = "Backtest QEDD model on historical data")]
struct Args {
    /// Path to datasets directory
    #[arg(long, default_value = "datasets/dry_run")]
    dataset_dir: PathBuf,

    /// Path to calibrated config JSON file (optional, uses defaults if not provided)
    #[arg(long)]
    config_file: Option<PathBuf>,

    /// Output directory for backtest results
    #[arg(long, default_value = "backtest_output")]
    output_dir: PathBuf,

    /// Export results to JSON
    #[arg(long)]
    export_json: bool,

    /// Export results to CSV
    #[arg(long)]
    export_csv: bool,

    /// Export results to Parquet
    #[arg(long)]
    export_parquet: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BacktestResult {
    /// Timestamp of the data point
    timestamp_ms: u64,

    /// Predicted lambda
    predicted_lambda: f32,

    /// Predicted survival probabilities
    predicted_survival_1s: f32,
    predicted_survival_5s: f32,
    predicted_survival_30s: f32,
    predicted_survival_60s: f32,

    /// Actual outcomes (if available)
    actual_survived_1s: Option<bool>,
    actual_survived_5s: Option<bool>,
    actual_survived_30s: Option<bool>,
    actual_survived_60s: Option<bool>,

    /// Actual price changes (if available)
    actual_price_1s: Option<f64>,
    actual_price_5s: Option<f64>,
    actual_price_30s: Option<f64>,
    actual_price_60s: Option<f64>,

    /// Decision: would we abort trading based on this prediction?
    would_abort: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BacktestSummary {
    /// Total number of data points
    total_samples: usize,

    /// Correctly predicted survivals at each horizon
    correct_1s: usize,
    correct_5s: usize,
    correct_30s: usize,
    correct_60s: usize,

    /// Accuracy metrics
    accuracy_1s: Option<f32>,
    accuracy_5s: Option<f32>,
    accuracy_30s: Option<f32>,
    accuracy_60s: Option<f32>,

    /// False positive rate (predicted rug, but survived)
    false_positive_rate: f32,

    /// False negative rate (predicted survive, but rugged)
    false_negative_rate: f32,

    /// Average predicted lambda
    avg_predicted_lambda: f32,

    /// Number of times we would have aborted
    abort_count: usize,

    /// Percentage of aborts
    abort_rate: f32,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args = Args::parse();

    tracing::info!("=== QEDD Backtesting Tool ===");
    tracing::info!("Dataset directory: {:?}", args.dataset_dir);
    tracing::info!("Output directory: {:?}", args.output_dir);

    // Create output directory
    std::fs::create_dir_all(&args.output_dir).context("Failed to create output directory")?;

    // Load configuration
    let config = if let Some(config_path) = &args.config_file {
        tracing::info!("Loading config from {:?}", config_path);
        let config_str =
            std::fs::read_to_string(config_path).context("Failed to read config file")?;
        serde_json::from_str(&config_str).context("Failed to parse config JSON")?
    } else {
        tracing::info!("Using default QEDD configuration");
        QeddConfig::default()
    };

    tracing::info!("QEDD Config:");
    tracing::info!("  λ_base: {:.4}", config.lambda_base);
    tracing::info!("  α: {:.4}", config.alpha_sobp_drop);
    tracing::info!("  β: {:.4}", config.beta_outflow);
    tracing::info!("  γ: {:.4}", config.gamma_resonance);
    tracing::info!("  δ: {:.4}", config.delta_deviation);
    tracing::info!("  Abort threshold: {:.4}", config.lambda_abort_threshold);

    // Load datasets
    tracing::info!("Loading datasets from {:?}...", args.dataset_dir);
    let loader = DatasetLoader::new(&args.dataset_dir);
    let data = loader.load_all().context("Failed to load datasets")?;

    if data.is_empty() {
        tracing::warn!(
            "No data points loaded. Please add dataset files to {:?}",
            args.dataset_dir
        );
        return Ok(());
    }

    tracing::info!("Loaded {} data points", data.len());

    // Run backtest
    tracing::info!("Running backtest...");
    let engine = QeddEngine::new(config.clone());
    let mut results = Vec::new();

    for data_point in &data {
        let qedd_result = engine.compute_qedd_sync(&data_point.signals);

        let would_abort = qedd_result.should_abort(config.lambda_abort_threshold);

        results.push(BacktestResult {
            timestamp_ms: data_point.timestamp_ms,
            predicted_lambda: qedd_result.lambda_now,
            predicted_survival_1s: qedd_result.survival_1s,
            predicted_survival_5s: qedd_result.survival_5s,
            predicted_survival_30s: qedd_result.survival_30s,
            predicted_survival_60s: qedd_result.survival_60s,
            actual_survived_1s: data_point.survived_1s,
            actual_survived_5s: data_point.survived_5s,
            actual_survived_30s: data_point.survived_30s,
            actual_survived_60s: data_point.survived_60s,
            actual_price_1s: data_point.actual_price_1s,
            actual_price_5s: data_point.actual_price_5s,
            actual_price_30s: data_point.actual_price_30s,
            actual_price_60s: data_point.actual_price_60s,
            would_abort,
        });
    }

    // Compute summary statistics
    let summary = compute_summary(&results);

    // Print summary
    println!("\n=== Backtest Summary ===\n");
    println!("Total samples: {}", summary.total_samples);
    println!("Abort rate: {:.2}%", summary.abort_rate * 100.0);
    println!("Average predicted λ: {:.4}", summary.avg_predicted_lambda);
    println!();

    if let Some(acc) = summary.accuracy_1s {
        println!("1s horizon accuracy: {:.2}%", acc * 100.0);
    }
    if let Some(acc) = summary.accuracy_5s {
        println!("5s horizon accuracy: {:.2}%", acc * 100.0);
    }
    if let Some(acc) = summary.accuracy_30s {
        println!("30s horizon accuracy: {:.2}%", acc * 100.0);
    }
    if let Some(acc) = summary.accuracy_60s {
        println!("60s horizon accuracy: {:.2}%", acc * 100.0);
    }
    println!();
    println!(
        "False positive rate: {:.2}%",
        summary.false_positive_rate * 100.0
    );
    println!(
        "False negative rate: {:.2}%",
        summary.false_negative_rate * 100.0
    );

    // Export results
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");

    if args.export_json {
        let json_path = args.output_dir.join(format!("backtest_{}.json", timestamp));
        tracing::info!("Exporting to JSON: {:?}", json_path);
        let json = serde_json::to_string_pretty(&results)?;
        std::fs::write(&json_path, json)?;

        let summary_path = args
            .output_dir
            .join(format!("backtest_summary_{}.json", timestamp));
        let summary_json = serde_json::to_string_pretty(&summary)?;
        std::fs::write(&summary_path, summary_json)?;
    }

    if args.export_csv {
        let csv_path = args.output_dir.join(format!("backtest_{}.csv", timestamp));
        tracing::info!("Exporting to CSV: {:?}", csv_path);
        let mut wtr = csv::Writer::from_path(&csv_path)?;

        for result in &results {
            wtr.serialize(result)?;
        }
        wtr.flush()?;
    }

    if args.export_parquet {
        tracing::info!("Parquet export for backtest results not yet implemented");
    }

    tracing::info!("Backtest complete! Results saved to {:?}", args.output_dir);

    Ok(())
}

fn compute_summary(results: &[BacktestResult]) -> BacktestSummary {
    let total_samples = results.len();

    let mut correct_1s = 0;
    let mut correct_5s = 0;
    let mut correct_30s = 0;
    let mut correct_60s = 0;

    let mut total_with_1s = 0;
    let mut total_with_5s = 0;
    let mut total_with_30s = 0;
    let mut total_with_60s = 0;

    let mut false_positives = 0;
    let mut false_negatives = 0;
    let mut total_with_outcomes = 0;

    let mut sum_lambda = 0.0f32;
    let mut abort_count = 0;

    for result in results {
        sum_lambda += result.predicted_lambda;

        if result.would_abort {
            abort_count += 1;
        }

        // Check 1s predictions
        if let Some(actual) = result.actual_survived_1s {
            total_with_1s += 1;
            let predicted = result.predicted_survival_1s > 0.5;
            if predicted == actual {
                correct_1s += 1;
            }

            total_with_outcomes += 1;
            // False positive: predicted rug (survival < 0.5), but actually survived
            // False negative: predicted survive (survival >= 0.5), but actually rugged
            if !predicted && actual {
                false_positives += 1; // Predicted rug, actually survived (missed opportunity)
            } else if predicted && !actual {
                false_negatives += 1; // Predicted survive, actually rugged (dangerous case)
            }
        }

        // Check 5s predictions
        if let Some(actual) = result.actual_survived_5s {
            total_with_5s += 1;
            let predicted = result.predicted_survival_5s > 0.5;
            if predicted == actual {
                correct_5s += 1;
            }
        }

        // Check 30s predictions
        if let Some(actual) = result.actual_survived_30s {
            total_with_30s += 1;
            let predicted = result.predicted_survival_30s > 0.5;
            if predicted == actual {
                correct_30s += 1;
            }
        }

        // Check 60s predictions
        if let Some(actual) = result.actual_survived_60s {
            total_with_60s += 1;
            let predicted = result.predicted_survival_60s > 0.5;
            if predicted == actual {
                correct_60s += 1;
            }
        }
    }

    let accuracy_1s = if total_with_1s > 0 {
        Some(correct_1s as f32 / total_with_1s as f32)
    } else {
        None
    };

    let accuracy_5s = if total_with_5s > 0 {
        Some(correct_5s as f32 / total_with_5s as f32)
    } else {
        None
    };

    let accuracy_30s = if total_with_30s > 0 {
        Some(correct_30s as f32 / total_with_30s as f32)
    } else {
        None
    };

    let accuracy_60s = if total_with_60s > 0 {
        Some(correct_60s as f32 / total_with_60s as f32)
    } else {
        None
    };

    let false_positive_rate = if total_with_outcomes > 0 {
        false_positives as f32 / total_with_outcomes as f32
    } else {
        0.0
    };

    let false_negative_rate = if total_with_outcomes > 0 {
        false_negatives as f32 / total_with_outcomes as f32
    } else {
        0.0
    };

    let avg_predicted_lambda = sum_lambda / total_samples as f32;
    let abort_rate = abort_count as f32 / total_samples as f32;

    BacktestSummary {
        total_samples,
        correct_1s,
        correct_5s,
        correct_30s,
        correct_60s,
        accuracy_1s,
        accuracy_5s,
        accuracy_30s,
        accuracy_60s,
        false_positive_rate,
        false_negative_rate,
        avg_predicted_lambda,
        abort_count,
        abort_rate,
    }
}
