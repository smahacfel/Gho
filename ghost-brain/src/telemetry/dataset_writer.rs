//! Dataset Writer for QEDD and MCI Results
//!
//! This module provides JSONL recording of QEDD and MCI computation results
//! for analysis and training purposes.
//!
//! ## TASK 3: QEDD and MCI Dataset Recording
//! Creates individual JSONL files for:
//! - datasets/.../qedd.json - QEDD computation results
//! - datasets/.../mci.json - MCI computation results
//!
//! ## Usage Example
//!
//! ```no_run
//! use ghost_brain::telemetry::DatasetWriter;
//! use ghost_brain::oracle::HyperPredictionOracle;
//!
//! #[tokio::main]
//! async fn main() {
//!     let oracle = HyperPredictionOracle::new(70);
//!     let writer = DatasetWriter::new();
//!     
//!     // After scoring a candidate...
//!     // let result = oracle.score_candidate(&candidate, ...).unwrap();
//!     
//!     // Write QEDD and MCI results to dataset
//!     // if let (Some(qedd), Some(mci)) = (&result.qedd_result, &result.mci_result) {
//!     //     let session_id = format!("slot_{}", candidate.slot);
//!     //     writer.write_both(&session_id, qedd, mci).await.unwrap();
//!     // }
//! }
//! ```
//!
//! ## File Format
//!
//! Each file contains one JSON object per line (JSONL format):
//! ```json
//! {"lambda_now":0.75,"survival_1s":0.47,"survival_5s":0.08,...}
//! {"lambda_now":0.82,"survival_1s":0.43,"survival_5s":0.06,...}
//! ```
//!
//! This format allows for easy streaming and processing with tools like `jq`, pandas, or custom parsers.

use crate::models::mci_result;
use crate::models::mci_result::MciResult;
use crate::models::qedd_result;
use crate::models::qedd_result::QeddResult;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tracing::{debug, error};

/// Default base directory for dataset files
pub const DEFAULT_DATASET_DIR: &str = "./datasets";

/// Dataset writer for QEDD and MCI results
pub struct DatasetWriter {
    base_dir: PathBuf,
}

impl DatasetWriter {
    /// Create a new dataset writer with the default base directory
    pub fn new() -> Self {
        Self {
            base_dir: PathBuf::from(DEFAULT_DATASET_DIR),
        }
    }

    /// Create a new dataset writer with a custom base directory
    pub fn with_base_dir(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    /// Write QEDD result to JSONL file
    ///
    /// Creates/appends to: datasets/{session_id}/qedd.json
    ///
    /// # Arguments
    /// * `session_id` - Unique identifier for this dataset session (e.g., slot number or timestamp)
    /// * `qedd` - QEDD computation result to record
    ///
    /// # Returns
    /// * `Result<PathBuf>` - Path to the written file or error
    pub async fn write_qedd(&self, session_id: &str, qedd: &QeddResult) -> Result<PathBuf> {
        let dir = self.base_dir.join(session_id);
        self.ensure_dir(&dir).await?;

        let file_path = dir.join("qedd.json");
        self.write_jsonl(&file_path, qedd).await?;

        debug!("Wrote QEDD result to: {:?}", file_path);
        Ok(file_path)
    }

    /// Write MCI result to JSONL file
    ///
    /// Creates/appends to: datasets/{session_id}/mci.json
    ///
    /// # Arguments
    /// * `session_id` - Unique identifier for this dataset session (e.g., slot number or timestamp)
    /// * `mci` - MCI computation result to record
    ///
    /// # Returns
    /// * `Result<PathBuf>` - Path to the written file or error
    pub async fn write_mci(&self, session_id: &str, mci: &MciResult) -> Result<PathBuf> {
        let dir = self.base_dir.join(session_id);
        self.ensure_dir(&dir).await?;

        let file_path = dir.join("mci.json");
        self.write_jsonl(&file_path, mci).await?;

        debug!("Wrote MCI result to: {:?}", file_path);
        Ok(file_path)
    }

    /// Write both QEDD and MCI results to their respective files
    ///
    /// This is a convenience method that writes both results atomically.
    ///
    /// # Arguments
    /// * `session_id` - Unique identifier for this dataset session
    /// * `qedd` - QEDD computation result to record
    /// * `mci` - MCI computation result to record
    ///
    /// # Returns
    /// * `Result<(PathBuf, PathBuf)>` - Tuple of (qedd_path, mci_path) or error
    pub async fn write_both(
        &self,
        session_id: &str,
        qedd: &QeddResult,
        mci: &MciResult,
    ) -> Result<(PathBuf, PathBuf)> {
        let qedd_path = self.write_qedd(session_id, qedd).await?;
        let mci_path = self.write_mci(session_id, mci).await?;
        Ok((qedd_path, mci_path))
    }

    /// Ensure the directory exists
    async fn ensure_dir(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)
            .await
            .with_context(|| format!("Failed to create dataset directory: {:?}", dir))
    }

    /// Write data to JSONL file (one JSON object per line)
    async fn write_jsonl<T: serde::Serialize>(&self, file_path: &Path, data: &T) -> Result<()> {
        // Open file in append mode
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .await
            .with_context(|| format!("Failed to open dataset file: {:?}", file_path))?;

        // Serialize to JSON
        let json =
            serde_json::to_string(data).with_context(|| "Failed to serialize data to JSON")?;

        // Write JSON line
        let line = format!("{}\n", json);
        file.write_all(line.as_bytes())
            .await
            .with_context(|| "Failed to write to dataset file")?;

        // Flush to ensure data is written
        file.flush()
            .await
            .with_context(|| "Failed to flush dataset file")?;

        Ok(())
    }
}

impl Default for DatasetWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs;

    #[tokio::test]
    async fn test_dataset_writer_creation() {
        let writer = DatasetWriter::new();
        assert_eq!(writer.base_dir, PathBuf::from(DEFAULT_DATASET_DIR));

        let custom_path = std::env::temp_dir().join("test_datasets");
        let custom_writer = DatasetWriter::with_base_dir(&custom_path);
        assert_eq!(custom_writer.base_dir, custom_path);
    }

    #[tokio::test]
    async fn test_write_qedd() {
        let test_dir = std::env::temp_dir().join("test_qedd_write");
        let writer = DatasetWriter::with_base_dir(&test_dir);

        let qedd = QeddResult {
            lambda_now: 0.75,
            survival_1s: 0.47,
            survival_5s: 0.08,
            survival_30s: 0.0001,
            survival_60s: 0.00001,
            #[allow(deprecated)]
            features_used: vec!["lambda_base".to_string(), "sobp_drop".to_string()],
            features_flags: qedd_result::FEATURE_LAMBDA_BASE | qedd_result::FEATURE_SOBP_DROP,
            computation_ms: 5,
        };

        let result = writer.write_qedd("test_session_1", &qedd).await;
        assert!(result.is_ok());

        let file_path = result.unwrap();
        assert!(file_path.exists());

        // Read and verify content
        let content = fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("lambda_now"));
        assert!(content.contains("0.75"));

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir).await;
    }

    #[tokio::test]
    async fn test_write_mci() {
        let test_dir = std::env::temp_dir().join("test_mci_write");
        let writer = DatasetWriter::with_base_dir(&test_dir);

        let mci = MciResult {
            mci: 0.82,
            dc: 0.90,
            sc: 0.75,
            #[allow(deprecated)]
            features_used: vec!["qass_alignment".to_string(), "flow_magnitude".to_string()],
            features_flags: mci_result::FEATURE_QASS_ALIGNMENT | mci_result::FEATURE_FLOW_MAGNITUDE,
            computation_ms: 3,
        };

        let result = writer.write_mci("test_session_2", &mci).await;
        assert!(result.is_ok());

        let file_path = result.unwrap();
        assert!(file_path.exists());

        // Read and verify content
        let content = fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("mci"));
        assert!(content.contains("0.82"));

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir).await;
    }

    #[tokio::test]
    async fn test_write_both() {
        let test_dir = std::env::temp_dir().join("test_both_write");
        let writer = DatasetWriter::with_base_dir(&test_dir);

        let qedd = QeddResult {
            lambda_now: 0.65,
            survival_1s: 0.52,
            survival_5s: 0.12,
            survival_30s: 0.001,
            survival_60s: 0.0001,
            #[allow(deprecated)]
            features_used: vec!["all".to_string()],
            features_flags: qedd_result::FEATURE_LAMBDA_BASE
                | qedd_result::FEATURE_SOBP_DROP
                | qedd_result::FEATURE_OUTFLOW
                | qedd_result::FEATURE_RESONANCE_RISK
                | qedd_result::FEATURE_DEVIATION_RISK,
            computation_ms: 4,
        };

        let mci = MciResult {
            mci: 0.78,
            dc: 0.85,
            sc: 0.72,
            #[allow(deprecated)]
            features_used: vec!["all".to_string()],
            features_flags: mci_result::FEATURE_QASS_ALIGNMENT
                | mci_result::FEATURE_FLOW_MAGNITUDE
                | mci_result::FEATURE_MPCF_ENTROPY
                | mci_result::FEATURE_SOBP_STABILITY
                | mci_result::FEATURE_COMBINED_ENTROPY
                | mci_result::FEATURE_DEVIATION_RISK,
            computation_ms: 2,
        };

        let result = writer.write_both("test_session_3", &qedd, &mci).await;
        assert!(result.is_ok());

        let (qedd_path, mci_path) = result.unwrap();
        assert!(qedd_path.exists());
        assert!(mci_path.exists());

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir).await;
    }

    #[tokio::test]
    async fn test_append_multiple_entries() {
        let test_dir = std::env::temp_dir().join("test_append");
        let writer = DatasetWriter::with_base_dir(&test_dir);

        let qedd1 = QeddResult {
            lambda_now: 0.5,
            survival_1s: 0.6,
            survival_5s: 0.2,
            survival_30s: 0.01,
            survival_60s: 0.001,
            #[allow(deprecated)]
            features_used: vec![],
            features_flags: 0,
            computation_ms: 1,
        };

        let qedd2 = QeddResult {
            lambda_now: 0.8,
            survival_1s: 0.4,
            survival_5s: 0.1,
            survival_30s: 0.001,
            survival_60s: 0.0001,
            #[allow(deprecated)]
            features_used: vec![],
            features_flags: 0,
            computation_ms: 2,
        };

        // Write first entry
        let path1 = writer
            .write_qedd("test_append_session", &qedd1)
            .await
            .unwrap();

        // Write second entry (should append)
        let path2 = writer
            .write_qedd("test_append_session", &qedd2)
            .await
            .unwrap();

        assert_eq!(path1, path2);

        // Read and verify both entries are present
        let content = fs::read_to_string(&path1).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "Should have 2 JSONL entries");

        // Verify each line is valid JSON
        for line in lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("lambda_now").is_some());
        }

        // Cleanup
        let _ = fs::remove_dir_all(test_dir).await;
    }
}
