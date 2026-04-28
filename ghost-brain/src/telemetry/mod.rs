//! Telemetry module for logging and metrics
//!
//! This module provides async telemetry recording for the Ghost Brain system.
//! It implements TASK 4.2 from the T<2S COMPONENTS IMPL PLAN.

pub mod dataset_writer;
pub mod recorder;

pub use dataset_writer::{DatasetWriter, DEFAULT_DATASET_DIR};
pub use recorder::{TelemetryConfig, TelemetryEvent, TelemetryRecorder};
