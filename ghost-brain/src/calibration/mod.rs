//! Calibration Module
//!
//! Tools for offline calibration and backtesting of QEDD models.

pub mod dataset;
pub mod export;
pub mod fitting;
pub mod reports;

pub use dataset::{DataPoint, DatasetLoader};
pub use export::{export_csv, export_json, export_parquet};
pub use fitting::{CalibrationResult, RidgeCalibrator};
pub use reports::{CalibrationReport, ReportGenerator};
