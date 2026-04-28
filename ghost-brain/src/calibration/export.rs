//! Export Functions
//!
//! Export calibration results and reports to various formats (JSON, CSV, Parquet).

use crate::calibration::reports::CalibrationReport;
use anyhow::{Context, Result};
use std::path::Path;

/// Export calibration report to JSON
pub fn export_json<P: AsRef<Path>>(report: &CalibrationReport, path: P) -> Result<()> {
    let json =
        serde_json::to_string_pretty(report).context("Failed to serialize report to JSON")?;

    std::fs::write(path.as_ref(), json).context("Failed to write JSON file")?;

    Ok(())
}

/// Export calibration report to CSV
pub fn export_csv<P: AsRef<Path>>(report: &CalibrationReport, path: P) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path.as_ref()).context("Failed to create CSV writer")?;

    // Write header
    wtr.write_record(&["parameter", "value"])?;

    // Write parameters
    wtr.write_record(&["lambda_base", &report.calibration.lambda_base.to_string()])?;
    wtr.write_record(&[
        "alpha_sobp_drop",
        &report.calibration.alpha_sobp_drop.to_string(),
    ])?;
    wtr.write_record(&["beta_outflow", &report.calibration.beta_outflow.to_string()])?;
    wtr.write_record(&[
        "gamma_resonance",
        &report.calibration.gamma_resonance.to_string(),
    ])?;
    wtr.write_record(&[
        "delta_deviation",
        &report.calibration.delta_deviation.to_string(),
    ])?;

    // Write metrics
    wtr.write_record(&["train_r2", &report.calibration.train_r2.to_string()])?;
    wtr.write_record(&["train_mse", &report.calibration.train_mse.to_string()])?;

    if let Some(test_r2) = report.calibration.test_r2 {
        wtr.write_record(&["test_r2", &test_r2.to_string()])?;
    }
    if let Some(test_mse) = report.calibration.test_mse {
        wtr.write_record(&["test_mse", &test_mse.to_string()])?;
    }

    // Write AUC metrics
    if let Some(auc) = report.auc_1s {
        wtr.write_record(&["auc_1s", &auc.to_string()])?;
    }
    if let Some(auc) = report.auc_5s {
        wtr.write_record(&["auc_5s", &auc.to_string()])?;
    }
    if let Some(auc) = report.auc_30s {
        wtr.write_record(&["auc_30s", &auc.to_string()])?;
    }
    if let Some(auc) = report.auc_60s {
        wtr.write_record(&["auc_60s", &auc.to_string()])?;
    }

    wtr.flush()?;
    Ok(())
}

/// Export calibration report to Parquet
pub fn export_parquet<P: AsRef<Path>>(report: &CalibrationReport, path: P) -> Result<()> {
    use arrow::array::{Float32Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::arrow_writer::ArrowWriter;
    use parquet::file::properties::WriterProperties;
    use std::fs::File;
    use std::sync::Arc;

    // Define schema
    let schema = Schema::new(vec![
        Field::new("parameter", DataType::Utf8, false),
        Field::new("value", DataType::Float32, true),
    ]);

    // Build arrays
    let mut params = Vec::new();
    let mut values = Vec::new();

    params.push("lambda_base");
    values.push(Some(report.calibration.lambda_base));

    params.push("alpha_sobp_drop");
    values.push(Some(report.calibration.alpha_sobp_drop));

    params.push("beta_outflow");
    values.push(Some(report.calibration.beta_outflow));

    params.push("gamma_resonance");
    values.push(Some(report.calibration.gamma_resonance));

    params.push("delta_deviation");
    values.push(Some(report.calibration.delta_deviation));

    params.push("train_r2");
    values.push(Some(report.calibration.train_r2));

    params.push("train_mse");
    values.push(Some(report.calibration.train_mse));

    if let Some(test_r2) = report.calibration.test_r2 {
        params.push("test_r2");
        values.push(Some(test_r2));
    }

    if let Some(test_mse) = report.calibration.test_mse {
        params.push("test_mse");
        values.push(Some(test_mse));
    }

    if let Some(auc) = report.auc_1s {
        params.push("auc_1s");
        values.push(Some(auc));
    }

    if let Some(auc) = report.auc_5s {
        params.push("auc_5s");
        values.push(Some(auc));
    }

    if let Some(auc) = report.auc_30s {
        params.push("auc_30s");
        values.push(Some(auc));
    }

    if let Some(auc) = report.auc_60s {
        params.push("auc_60s");
        values.push(Some(auc));
    }

    let param_array = StringArray::from(params);
    let value_array = Float32Array::from(values);

    // Create record batch
    let batch = RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![Arc::new(param_array), Arc::new(value_array)],
    )
    .context("Failed to create record batch")?;

    // Write to Parquet file
    let file = File::create(path.as_ref()).context("Failed to create Parquet file")?;

    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, Arc::new(schema), Some(props))
        .context("Failed to create Parquet writer")?;

    writer
        .write(&batch)
        .context("Failed to write batch to Parquet")?;

    writer.close().context("Failed to close Parquet writer")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::fitting::CalibrationResult;
    use crate::calibration::reports::CalibrationReport;

    fn create_test_report() -> CalibrationReport {
        let calibration = CalibrationResult {
            alpha_sobp_drop: 0.3,
            beta_outflow: 0.25,
            gamma_resonance: 0.15,
            delta_deviation: 0.20,
            lambda_base: 0.5,
            train_r2: 0.85,
            test_r2: Some(0.80),
            train_mse: 0.05,
            test_mse: Some(0.06),
            n_train: 80,
            n_test: Some(20),
        };

        CalibrationReport {
            calibration,
            auc_1s: Some(0.92),
            auc_5s: Some(0.88),
            auc_30s: Some(0.85),
            auc_60s: Some(0.82),
            residuals_mean: 0.01,
            residuals_std: 0.15,
            residuals_min: -0.5,
            residuals_max: 0.6,
            n_total_samples: 100,
            n_survived: 60,
            n_rugged: 40,
            generated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_export_json() {
        let report = create_test_report();
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_calibration.json");

        let result = export_json(&report, &path);
        assert!(result.is_ok());

        // Verify file exists and can be read back
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("alpha_sobp_drop"));

        // Clean up
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_export_csv() {
        let report = create_test_report();
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_calibration.csv");

        let result = export_csv(&report, &path);
        assert!(result.is_ok());

        // Verify file exists
        assert!(path.exists());

        // Clean up
        let _ = std::fs::remove_file(path);
    }
}
