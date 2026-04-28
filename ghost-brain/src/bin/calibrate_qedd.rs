//! QEDD Calibration Tool
//!
//! Offline calibration of QEDD model parameters using ridge regression.
//!
//! Usage:
//!   ghost-brain-calibrate-qedd [OPTIONS]
//!
//! Options:
//!   --dataset-dir <PATH>    Path to datasets directory [default: datasets/dry_run]
//!   --output-dir <PATH>     Output directory for reports [default: calibration_output]
//!   --alpha <FLOAT>         Ridge regularization parameter [default: 0.1]
//!   --train-ratio <FLOAT>   Train/test split ratio [default: 0.8]
//!   --export-json           Export report to JSON
//!   --export-csv            Export report to CSV
//!   --export-parquet        Export report to Parquet
//!   --help                  Print help information

use anyhow::{Context, Result};
use clap::Parser;
use ghost_brain::calibration::{
    export_csv, export_json, export_parquet, DatasetLoader, ReportGenerator, RidgeCalibrator,
};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "ghost-brain-calibrate-qedd")]
#[command(about = "Calibrate QEDD model parameters offline")]
struct Args {
    /// Path to datasets directory
    #[arg(long, default_value = "datasets/dry_run")]
    dataset_dir: PathBuf,

    /// Output directory for reports
    #[arg(long, default_value = "calibration_output")]
    output_dir: PathBuf,

    /// Ridge regularization parameter (alpha)
    #[arg(long, default_value = "0.1")]
    alpha: f64,

    /// Train/test split ratio
    #[arg(long, default_value = "0.8")]
    train_ratio: f64,

    /// Export report to JSON
    #[arg(long)]
    export_json: bool,

    /// Export report to CSV
    #[arg(long)]
    export_csv: bool,

    /// Export report to Parquet
    #[arg(long)]
    export_parquet: bool,
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

    tracing::info!("=== QEDD Calibration Tool ===");
    tracing::info!("Dataset directory: {:?}", args.dataset_dir);
    tracing::info!("Output directory: {:?}", args.output_dir);
    tracing::info!("Ridge alpha: {}", args.alpha);
    tracing::info!("Train/test ratio: {}", args.train_ratio);

    // Create output directory
    std::fs::create_dir_all(&args.output_dir).context("Failed to create output directory")?;

    // Load datasets
    tracing::info!("Loading datasets from {:?}...", args.dataset_dir);
    let loader = DatasetLoader::new(&args.dataset_dir);
    let file_count = loader.count_files();
    tracing::info!("Found {} files in dataset directory", file_count);

    let data = loader.load_all().context("Failed to load datasets")?;

    if data.is_empty() {
        tracing::warn!(
            "No data points loaded. Please add dataset files to {:?}",
            args.dataset_dir
        );
        tracing::info!("Creating sample dataset...");
        create_sample_dataset(&args.dataset_dir)?;
        tracing::info!("Sample dataset created. Please re-run the calibration tool.");
        return Ok(());
    }

    tracing::info!("Loaded {} data points", data.len());

    // Calibrate parameters
    tracing::info!("Fitting QEDD parameters using ridge regression...");
    let calibrator = RidgeCalibrator::new(args.alpha, args.train_ratio);
    let calibration_result = calibrator.fit(&data).context("Failed to fit parameters")?;

    tracing::info!("Calibration complete!");
    tracing::info!("  λ_base: {:.4}", calibration_result.lambda_base);
    tracing::info!("  α (SOBP drop): {:.4}", calibration_result.alpha_sobp_drop);
    tracing::info!("  β (outflow): {:.4}", calibration_result.beta_outflow);
    tracing::info!("  γ (resonance): {:.4}", calibration_result.gamma_resonance);
    tracing::info!("  δ (deviation): {:.4}", calibration_result.delta_deviation);
    tracing::info!("  Train R²: {:.4}", calibration_result.train_r2);
    tracing::info!("  Train MSE: {:.4}", calibration_result.train_mse);

    // Generate report
    tracing::info!("Generating calibration report...");
    let config = calibration_result.to_qedd_config();
    let report = ReportGenerator::generate(calibration_result, &data, &config)
        .context("Failed to generate report")?;

    // Print text summary
    println!("\n{}", report.to_text_summary());

    // Export reports
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");

    if args.export_json {
        let json_path = args
            .output_dir
            .join(format!("calibration_{}.json", timestamp));
        tracing::info!("Exporting to JSON: {:?}", json_path);
        export_json(&report, &json_path).context("Failed to export JSON")?;
    }

    if args.export_csv {
        let csv_path = args
            .output_dir
            .join(format!("calibration_{}.csv", timestamp));
        tracing::info!("Exporting to CSV: {:?}", csv_path);
        export_csv(&report, &csv_path).context("Failed to export CSV")?;
    }

    if args.export_parquet {
        let parquet_path = args
            .output_dir
            .join(format!("calibration_{}.parquet", timestamp));
        tracing::info!("Exporting to Parquet: {:?}", parquet_path);
        export_parquet(&report, &parquet_path).context("Failed to export Parquet")?;
    }

    tracing::info!(
        "Calibration complete! Results saved to {:?}",
        args.output_dir
    );

    Ok(())
}

/// Create a sample dataset for testing
fn create_sample_dataset(dataset_dir: &PathBuf) -> Result<()> {
    use ghost_brain::calibration::DataPoint;
    use ghost_brain::signals::MarketSignals;

    std::fs::create_dir_all(dataset_dir)?;

    let mut data_points = Vec::new();

    // Generate 50 sample data points with varying market conditions
    for i in 0..50 {
        let signals = if i % 3 == 0 {
            MarketSignals::mock_hype() // Good conditions
        } else if i % 3 == 1 {
            MarketSignals::mock_stable() // Moderate conditions
        } else {
            MarketSignals::mock_rug() // Bad conditions
        };

        let mut dp = DataPoint::new(i * 1000, signals);

        // Set survival based on conditions
        dp.survived_1s = Some(i % 3 != 2); // Rugs don't survive
        dp.survived_5s = Some(i % 3 != 2);
        dp.survived_30s = Some(i % 3 == 0); // Only hype survives long
        dp.survived_60s = Some(i % 3 == 0);

        data_points.push(dp);
    }

    // Save to JSON
    let json_path = dataset_dir.join("sample_data.json");
    let json = serde_json::to_string_pretty(&data_points)?;
    std::fs::write(json_path, json)?;

    Ok(())
}
