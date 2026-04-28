# QEDD Calibration & Backtesting Tools

This directory contains tools for offline calibration and backtesting of the QEDD (Quantum Entropy-Driven Decay) model.

## Tools

### 1. `ghost-brain-calibrate-qedd`

Calibrate QEDD model parameters using ridge regression on historical market data.

**Usage:**
```bash
cargo run --bin ghost-brain-calibrate-qedd -- [OPTIONS]
```

**Options:**
- `--dataset-dir <PATH>` - Path to datasets directory (default: `datasets/dry_run`)
- `--output-dir <PATH>` - Output directory for reports (default: `calibration_output`)
- `--alpha <FLOAT>` - Ridge regularization parameter (default: `0.1`)
- `--train-ratio <FLOAT>` - Train/test split ratio (default: `0.8`)
- `--export-json` - Export report to JSON
- `--export-csv` - Export report to CSV
- `--export-parquet` - Export report to Parquet

**Example:**
```bash
# Calibrate with default settings and export to all formats
cargo run --bin ghost-brain-calibrate-qedd -- \
  --dataset-dir datasets/dry_run \
  --output-dir calibration_output \
  --export-json \
  --export-csv \
  --export-parquet
```

**Output:**
- Fitted parameters: λ_base, α (alpha_sobp_drop), β (beta_outflow), γ (gamma_resonance), δ (delta_deviation)
- Model performance metrics: R², MSE for train and test sets
- AUC scores for survival predictions at different time horizons
- Residual statistics
- Dataset summary

### 2. `ghost-brain-backtest-qedd`

Run backtests on historical data using calibrated (or default) QEDD parameters.

**Usage:**
```bash
cargo run --bin ghost-brain-backtest-qedd -- [OPTIONS]
```

**Options:**
- `--dataset-dir <PATH>` - Path to datasets directory (default: `datasets/dry_run`)
- `--config-file <PATH>` - Path to calibrated config JSON file (optional)
- `--output-dir <PATH>` - Output directory for backtest results (default: `backtest_output`)
- `--export-json` - Export results to JSON
- `--export-csv` - Export results to CSV
- `--export-parquet` - Export results to Parquet

**Example:**
```bash
# Backtest with calibrated parameters
cargo run --bin ghost-brain-backtest-qedd -- \
  --dataset-dir datasets/dry_run \
  --config-file calibration_output/calibration_20231201_120000.json \
  --output-dir backtest_output \
  --export-json \
  --export-csv
```

**Output:**
- Predicted lambda and survival probabilities for each data point
- Comparison with actual outcomes (if available)
- Accuracy metrics at different time horizons
- False positive and false negative rates
- Abort rate analysis

## Dataset Format

Datasets should be placed in `datasets/dry_run/` or a custom directory. Supported formats:

### JSON Format
```json
[
  {
    "timestamp_ms": 1000000,
    "signals": {
      "volume": { "current": 100000.0, "ma": 95000.0, "std_dev": 5000.0 },
      "price": { "current": 1.0, "momentum": 0.05, "volatility": 0.02 },
      "orderbook": { "spread": 0.001, "depth": 50000.0, "imbalance": 0.55 },
      "time": { "timestamp_ms": 1000000, "time_since_last_trade_ms": 100 },
      "sobp": { "current": 1.5, "drop": 0.1, "ma": 1.4 },
      "flow": { "outflow": 0.3, "qass_alignment": 0.7, "magnitude": 0.8 },
      "resonance": { "risk": 0.2, "cv": 0.6, "sample_count": 32 },
      "deviation": { "risk": 0.15, "coherence_loss": 0.1, "anomaly_magnitude": 0.2 },
      "entropy": { "ssmi": 0.65, "mpcf": 0.70, "combined": 0.675 }
    },
    "survived_1s": true,
    "survived_5s": true,
    "survived_30s": false,
    "survived_60s": false,
    "actual_price_1s": 1.05,
    "actual_price_5s": 1.12,
    "actual_price_30s": 0.95,
    "actual_price_60s": 0.88
  }
]
```

### CSV Format
CSV files should have columns matching the `DataPoint` structure. Nested structures are flattened with dot notation (e.g., `signals.sobp.drop`).

### Parquet Format
Parquet files with the same schema as JSON are supported for efficient storage of large datasets.

## Sample Data Generation

If no datasets are found, the calibration tool will automatically generate sample data for testing purposes. This creates a `sample_data.json` file with 50 data points representing various market conditions (hype, stable, rug scenarios).

## Ridge Regression Calibration

The calibration tool uses ridge regression to fit the QEDD hazard rate formula:

```
λ(t) = λ_base + α * SOBP_drop + β * outflow + γ * resonance_risk + δ * deviation_risk
```

Where:
- **λ_base**: Base hazard rate (intercept)
- **α (alpha_sobp_drop)**: Coefficient for SOBP drop contribution
- **β (beta_outflow)**: Coefficient for capital outflow contribution
- **γ (gamma_resonance)**: Coefficient for bot activity/resonance risk contribution
- **δ (delta_deviation)**: Coefficient for market deviation risk contribution

The ridge regularization parameter (alpha) helps prevent overfitting by adding a penalty term for large coefficient values.

## Interpretation of Results

### Calibration Metrics

- **R² (Coefficient of Determination)**: Ranges from -∞ to 1.0
  - 1.0 = perfect fit
  - 0.0 = model performs as well as predicting the mean
  - < 0.0 = model performs worse than predicting the mean (may indicate poor model fit or unsuitable data)

- **MSE (Mean Squared Error)**: Lower is better
  - Measures average squared difference between predictions and actual values

- **AUC (Area Under ROC Curve)**: Ranges from 0.0 to 1.0
  - 1.0 = perfect classifier
  - 0.5 = random classifier
  - Measures ability to distinguish between survived and rugged tokens

### Backtest Metrics

- **Accuracy**: Percentage of correct survival predictions
- **False Positive Rate**: Predicted rug but token survived (missed opportunities)
- **False Negative Rate**: Predicted survive but token rugged (dangerous cases)
- **Abort Rate**: Percentage of tokens that would have triggered trading abort

## Integration with Live Trading

After calibration, use the fitted parameters in your QEDD configuration:

```rust
use ghost_brain::config::qedd_config::QeddConfig;

let mut config = QeddConfig::default();
config.lambda_base = 0.5;
config.alpha_sobp_drop = 0.3;
config.beta_outflow = 0.25;
config.gamma_resonance = 0.15;
config.delta_deviation = 0.20;

let engine = QeddEngine::new(config);
```

Or load from calibrated JSON:
```rust
let config_str = std::fs::read_to_string("calibration_output/calibration.json")?;
let report: CalibrationReport = serde_json::from_str(&config_str)?;
let config = report.calibration.to_qedd_config();
```

## Advanced Usage

### Custom Ridge Regularization

Experiment with different alpha values to control overfitting:

```bash
# Lower alpha (0.01) = less regularization, may overfit
cargo run --bin ghost-brain-calibrate-qedd -- --alpha 0.01

# Higher alpha (1.0) = more regularization, may underfit
cargo run --bin ghost-brain-calibrate-qedd -- --alpha 1.0
```

### Custom Train/Test Split

Adjust the train/test split ratio:

```bash
# 90% train, 10% test
cargo run --bin ghost-brain-calibrate-qedd -- --train-ratio 0.9

# 70% train, 30% test (more rigorous validation)
cargo run --bin ghost-brain-calibrate-qedd -- --train-ratio 0.7
```

## Troubleshooting

### Very Negative R² Values

If you see extremely negative R² values (e.g., -1e26), this indicates:
- The model is not a good fit for the data
- The data may not have enough variance in the target variable
- Consider collecting more diverse data or adjusting the model

### Low AUC Scores

AUC scores close to 0.5 indicate the model is no better than random guessing:
- Check that your dataset has sufficient samples of both survived and rugged tokens
- Ensure signal quality is good (not all zeros or constants)
- Consider feature engineering or collecting additional signals

### Compilation Issues

If you encounter compilation errors related to `linfa` or `arrow`:
```bash
# Clean and rebuild
cargo clean
cargo build --release
```

## Testing

Run the calibration module tests:
```bash
cargo test -p ghost-brain --lib calibration
```

Run QEDD tests to ensure integration:
```bash
cargo test -p ghost-brain --lib qedd
```

## Performance Considerations

- **Dataset Size**: The tools can handle datasets from a few samples to millions of records
- **Memory Usage**: Large Parquet files are read in batches to minimize memory footprint
- **Disk Space**: Keep calibration and backtest outputs small; old reports can be deleted after analysis

## Future Enhancements

Potential improvements for future versions:
- Cross-validation with k-folds
- Hyperparameter grid search
- Feature importance analysis
- Time-series specific validation (walk-forward)
- Integration with MLflow or similar experiment tracking
- Real-time calibration updates
- Ensemble methods (random forest, gradient boosting)
