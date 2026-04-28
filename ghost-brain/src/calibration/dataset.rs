//! Dataset Loading
//!
//! Load and parse datasets from various formats for calibration and backtesting.

use crate::signals::MarketSignals;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A single data point for calibration/backtesting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,

    /// Market signals at this point in time
    pub signals: MarketSignals,

    /// Actual outcome: price at future time horizon (optional for backtesting)
    pub actual_price_1s: Option<f64>,
    pub actual_price_5s: Option<f64>,
    pub actual_price_30s: Option<f64>,
    pub actual_price_60s: Option<f64>,

    /// Whether the token survived (did not rug) at each horizon
    pub survived_1s: Option<bool>,
    pub survived_5s: Option<bool>,
    pub survived_30s: Option<bool>,
    pub survived_60s: Option<bool>,
}

impl DataPoint {
    /// Create a new data point with signals only
    pub fn new(timestamp_ms: u64, signals: MarketSignals) -> Self {
        Self {
            timestamp_ms,
            signals,
            actual_price_1s: None,
            actual_price_5s: None,
            actual_price_30s: None,
            actual_price_60s: None,
            survived_1s: None,
            survived_5s: None,
            survived_30s: None,
            survived_60s: None,
        }
    }
}

/// Dataset loader for various file formats
pub struct DatasetLoader {
    /// Root directory for datasets (e.g., "datasets/dry_run")
    root_dir: PathBuf,
}

impl DatasetLoader {
    /// Create a new dataset loader
    pub fn new<P: AsRef<Path>>(root_dir: P) -> Self {
        Self {
            root_dir: root_dir.as_ref().to_path_buf(),
        }
    }

    /// Load all datasets from the root directory recursively
    pub fn load_all(&self) -> Result<Vec<DataPoint>> {
        let mut all_data = Vec::new();

        // Walk through all subdirectories
        for entry in WalkDir::new(&self.root_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip directories
            if !path.is_file() {
                continue;
            }

            // Try to load based on file extension
            if let Some(ext) = path.extension() {
                match ext.to_str() {
                    Some("json") => {
                        if let Ok(data) = self.load_json(path) {
                            all_data.extend(data);
                        }
                    }
                    Some("csv") => {
                        if let Ok(data) = self.load_csv(path) {
                            all_data.extend(data);
                        }
                    }
                    Some("parquet") => {
                        if let Ok(data) = self.load_parquet(path) {
                            all_data.extend(data);
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(all_data)
    }

    /// Load dataset from JSON file
    pub fn load_json<P: AsRef<Path>>(&self, path: P) -> Result<Vec<DataPoint>> {
        let content = std::fs::read_to_string(path.as_ref()).context("Failed to read JSON file")?;

        // Try to parse as array of data points
        if let Ok(data) = serde_json::from_str::<Vec<DataPoint>>(&content) {
            return Ok(data);
        }

        // Try to parse as single data point
        if let Ok(data) = serde_json::from_str::<DataPoint>(&content) {
            return Ok(vec![data]);
        }

        anyhow::bail!("Failed to parse JSON as DataPoint or Vec<DataPoint>")
    }

    /// Load dataset from CSV file
    pub fn load_csv<P: AsRef<Path>>(&self, path: P) -> Result<Vec<DataPoint>> {
        let mut reader =
            csv::Reader::from_path(path.as_ref()).context("Failed to open CSV file")?;

        let mut data_points = Vec::new();

        for result in reader.deserialize() {
            let record: DataPoint = result.context("Failed to deserialize CSV record")?;
            data_points.push(record);
        }

        Ok(data_points)
    }

    /// Load dataset from Parquet file
    pub fn load_parquet<P: AsRef<Path>>(&self, path: P) -> Result<Vec<DataPoint>> {
        // Note: Full Parquet to DataPoint conversion is not yet implemented
        // This would require mapping Arrow schema to nested Rust structures
        anyhow::bail!(
            "Parquet loading is not fully implemented. Please convert your Parquet files to JSON or CSV format. \
             Path: {:?}", path.as_ref()
        )
    }

    /// Get count of files in the dataset directory
    pub fn count_files(&self) -> usize {
        WalkDir::new(&self.root_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::MarketSignals;

    #[test]
    fn test_datapoint_creation() {
        let signals = MarketSignals::mock();
        let dp = DataPoint::new(1000000, signals);
        assert_eq!(dp.timestamp_ms, 1000000);
        assert!(dp.actual_price_1s.is_none());
    }

    #[test]
    fn test_dataset_loader_creation() {
        let loader = DatasetLoader::new("datasets/dry_run");
        assert!(loader.root_dir.to_str().is_some());
    }
}
