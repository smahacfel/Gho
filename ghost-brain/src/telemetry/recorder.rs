//! Telemetry Recorder for logging Watchdog decisions and performance metrics
//!
//! This module implements TASK 4.2 from the T<2S COMPONENTS IMPL PLAN.
//! It provides async JSONL logging of:
//! - Watchdog decisions (Proceed, Abort, Timeout)
//! - Performance metrics (void_latency_ms, chaos_sims_per_sec)
//! - Context data (QASS scores, risk scores, task completion)
//!
//! ## Design Principles
//! 1. **Async Writing**: Non-blocking writes via tokio::fs
//! 2. **Structured Logging**: JSONL format for easy parsing/analysis
//! 3. **Minimal Overhead**: Buffered writes, fire-and-forget
//! 4. **Fail-Safe**: Logging errors should not crash the system

use crate::guardian::types::{WatchdogDecision, WatchdogSignal};
use crate::oracle::hyper_prediction::HyperPredictionResult;
use anyhow::{Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Event types that can be logged by the telemetry system
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum TelemetryEvent {
    /// Watchdog decision event
    Decision {
        /// Timestamp in milliseconds since Unix epoch
        timestamp_ms: u64,
        /// The decision made (Proceed, Abort, Timeout)
        decision: WatchdogDecision,
        /// Time spent in the void (milliseconds)
        void_latency_ms: u64,
        /// Latest QASS score if available
        qass_score: Option<f64>,
        /// Internal risk scores from tasks
        internal_risk_scores: Vec<(String, f64)>,
        /// Number of internal tasks completed
        internal_tasks_completed: u32,
        /// Number of internal tasks spawned
        internal_tasks_spawned: u32,
        /// Whether external result was received
        external_result_received: bool,
    },

    /// Internal task completion event
    TaskCompletion {
        /// Timestamp in milliseconds since Unix epoch
        timestamp_ms: u64,
        /// Task name (e.g., "Chaos Engine", "Gene Mapper")
        task_name: String,
        /// Whether risk was detected
        risk_detected: bool,
        /// Risk score (0.0-1.0)
        risk_score: f64,
        /// Additional details
        details: String,
    },

    /// Performance metrics event
    Metrics {
        /// Timestamp in milliseconds since Unix epoch
        timestamp_ms: u64,
        /// Chaos Engine simulations per second (if available)
        chaos_sims_per_sec: Option<f64>,
        /// Gene Mapper analysis time in microseconds
        gene_analysis_us: Option<u64>,
        /// Watchdog loop iteration time in microseconds
        loop_iteration_us: Option<u64>,
    },

    /// Error event
    Error {
        /// Timestamp in milliseconds since Unix epoch
        timestamp_ms: u64,
        /// Error message
        message: String,
        /// Error context/source
        context: String,
    },

    /// Data Quality Alarm event - Automatic alerts for invalid/anomalous data
    DataQualityAlarm {
        /// Timestamp in milliseconds since Unix epoch
        timestamp_ms: u64,
        /// Alarm source (e.g., "SnapshotEngine", "SCRExtended")
        source: String,
        /// Severity (SoftSync or HardAbort)
        severity: String,
        /// Alarm type (e.g., "DuplicateTransaction", "ExcessiveJitter", "VolumeAnomaly")
        alarm_type: String,
        /// Pool pubkey if applicable
        pool_pubkey: Option<String>,
        /// Detailed description
        details: String,
        /// Metric values associated with alarm
        metrics: serde_json::Value,
    },

    /// HyperPrediction Scoring event - Full JSONL output with all subcomponents
    /// This event captures the complete scoring pipeline result including:
    /// - Raw transaction data
    /// - All Oracle submodule results (QASS, SSMI, MPCF, IWIM, etc.)
    /// - InsufficientData flags for each submodule
    /// - QEDD and MCI results
    /// - Final scoring decision
    #[serde(untagged)]
    HyperPredictionScoring {
        /// Candidate identifier (pool AMM ID)
        candidate_id: String,
        /// Timestamp in ISO 8601 format
        timestamp: String,
        /// Raw transaction data for this candidate
        #[serde(skip_serializing_if = "Vec::is_empty")]
        txs: Vec<serde_json::Value>,
        /// QASS (Quantum Amplitude Superposition Scoring) result
        #[serde(skip_serializing_if = "Option::is_none")]
        qass: Option<serde_json::Value>,
        /// SSMI (Sub-Slot Microentropy Index) result
        #[serde(skip_serializing_if = "Option::is_none")]
        ssmi: Option<serde_json::Value>,
        /// MPCF (Micro-Payload Cognitive Fingerprint) result
        #[serde(skip_serializing_if = "Option::is_none")]
        mpcf: Option<serde_json::Value>,
        /// IWIM (Initial Wallet Intent Mapping) result
        #[serde(skip_serializing_if = "Option::is_none")]
        iwim: Option<serde_json::Value>,
        /// SOBP (Shadow Oracle Bonding Progress) data
        #[serde(skip_serializing_if = "Option::is_none")]
        sobp: Option<serde_json::Value>,
        /// QOFSV (Quantum Oracle Field Superposition Vector) - mapped from POVC
        #[serde(skip_serializing_if = "Option::is_none")]
        qofsv: Option<serde_json::Value>,
        /// QEDD (Quantum Entropy-Driven Decay) result
        #[serde(skip_serializing_if = "Option::is_none")]
        qedd: Option<serde_json::Value>,
        /// MCI (Market Coherence Index) result
        #[serde(skip_serializing_if = "Option::is_none")]
        mci: Option<serde_json::Value>,
        /// Gene Mapper security analysis result
        #[serde(skip_serializing_if = "Option::is_none")]
        gene_mapper: Option<serde_json::Value>,
        /// SCR (Slot-Coherence Resonance) score for bot detection
        #[serde(skip_serializing_if = "Option::is_none")]
        scr: Option<serde_json::Value>,
        /// ULVF (Ultra-Early Liquidity Vector Field) data
        #[serde(skip_serializing_if = "Option::is_none")]
        ulvf: Option<serde_json::Value>,
        /// Chaos Engine Monte Carlo simulation result
        #[serde(skip_serializing_if = "Option::is_none")]
        chaos: Option<serde_json::Value>,
        /// Resonance Detector result (bot vs human pattern detection)
        #[serde(skip_serializing_if = "Option::is_none")]
        resonance: Option<serde_json::Value>,
        /// External Hunter/Oracle score
        #[serde(skip_serializing_if = "Option::is_none")]
        hunter_score: Option<u8>,
        /// Final combined score (0-100) - initial scoring
        final_score_initial: u8,
        /// Final score after follow-up analysis (if available)
        #[serde(skip_serializing_if = "Option::is_none")]
        final_score_followup: Option<u8>,
        /// Whether the candidate passed threshold
        passed: bool,
        /// Risk level assessment
        risk_level: String,
        /// Processing time in microseconds
        processing_time_us: u64,
        /// Base score before QASS modification
        base_score: u8,
        /// Human-readable interpretation
        interpretation: String,
    },
}

impl TelemetryEvent {
    /// Get the current timestamp in milliseconds since Unix epoch
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Create a decision event from watchdog context
    pub fn decision(
        decision: WatchdogDecision,
        void_latency_ms: u64,
        qass_score: Option<f64>,
        internal_risk_scores: Vec<(String, f64)>,
        internal_tasks_completed: u32,
        internal_tasks_spawned: u32,
        external_result_received: bool,
    ) -> Self {
        Self::Decision {
            timestamp_ms: Self::now_ms(),
            decision,
            void_latency_ms,
            qass_score,
            internal_risk_scores,
            internal_tasks_completed,
            internal_tasks_spawned,
            external_result_received,
        }
    }

    /// Create a task completion event from a signal
    pub fn task_completion(
        task_name: String,
        risk_detected: bool,
        risk_score: f64,
        details: String,
    ) -> Self {
        Self::TaskCompletion {
            timestamp_ms: Self::now_ms(),
            task_name,
            risk_detected,
            risk_score,
            details,
        }
    }

    /// Create a metrics event
    pub fn metrics(
        chaos_sims_per_sec: Option<f64>,
        gene_analysis_us: Option<u64>,
        loop_iteration_us: Option<u64>,
    ) -> Self {
        Self::Metrics {
            timestamp_ms: Self::now_ms(),
            chaos_sims_per_sec,
            gene_analysis_us,
            loop_iteration_us,
        }
    }

    /// Create an error event
    pub fn error(message: String, context: String) -> Self {
        Self::Error {
            timestamp_ms: Self::now_ms(),
            message,
            context,
        }
    }

    /// Create a HyperPrediction scoring event from the full scoring result
    ///
    /// This captures all subcomponents with their InsufficientData status
    /// indicated by None values. The output is designed for comprehensive
    /// JSONL logging of the entire scoring pipeline.
    ///
    /// # Arguments
    /// * `candidate_id` - Pool AMM ID or candidate identifier
    /// * `result` - Full HyperPrediction scoring result
    /// * `txs` - Raw transaction data (if available)
    pub fn hyper_prediction_scoring(
        candidate_id: &str,
        result: &HyperPredictionResult,
        txs: Vec<serde_json::Value>,
    ) -> Self {
        // Convert timestamp to ISO 8601 format
        let timestamp = chrono::Utc::now().to_rfc3339();

        // QASS is deprecated - use None for backward compatibility
        let qass: Option<serde_json::Value> = None;

        let ssmi = result
            .ssmi_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        let mpcf = result
            .mpcf_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        let iwim = result
            .iwim_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        // SOBP: Shadow Oracle Bonding Progress
        let sobp = if result.shadow_progress.is_some() || result.shadow_price_ratio.is_some() {
            Some(json!({
                "progress_pct": result.shadow_progress,
                "price_ratio": result.shadow_price_ratio,
            }))
        } else {
            None
        };

        // QOFSV: Map from POVC cluster data
        let qofsv = result.povc_cluster.map(|cluster| {
            json!({
                "povc_cluster": cluster,
                "cluster_name": match cluster {
                    0 => "Dump",
                    1 => "Organic Hype",
                    2 => "Bot Noise",
                    _ => "Unknown",
                },
            })
        });

        let qedd = result
            .qedd_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        let mci = result
            .mci_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        let gene_mapper = result
            .gene_safety_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        // SCR: Slot-Coherence Resonance
        let scr = result.scr_score.map(|score| {
            json!({
                "score": score,
            })
        });

        // ULVF: Ultra-Early Liquidity Vector Field
        let ulvf = if result.ulvf_divergence.is_some() || result.ulvf_curl.is_some() {
            Some(json!({
                "divergence": result.ulvf_divergence,
                "curl": result.ulvf_curl,
            }))
        } else {
            None
        };

        let chaos = result
            .chaos_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        let resonance = result
            .resonance_result
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok());

        Self::HyperPredictionScoring {
            candidate_id: candidate_id.to_string(),
            timestamp,
            txs,
            qass,
            ssmi,
            mpcf,
            iwim,
            sobp,
            qofsv,
            qedd,
            mci,
            gene_mapper,
            scr,
            ulvf,
            chaos,
            resonance,
            hunter_score: result.hunter_score,
            final_score_initial: result.score,
            final_score_followup: None, // For future follow-up scoring
            passed: result.passed,
            risk_level: format!("{:?}", result.risk_level),
            processing_time_us: result.processing_time_us,
            base_score: result.base_score,
            interpretation: result.interpretation.clone(),
        }
    }
}

/// Configuration for the telemetry recorder
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Path to the log file
    pub log_path: PathBuf,
    /// Buffer size for the event channel
    pub channel_buffer_size: usize,
    /// Whether to enable telemetry (can be disabled for testing)
    pub enabled: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_path: PathBuf::from("logs/decisions.jsonl"),
            channel_buffer_size: 100,
            enabled: true,
        }
    }
}

/// Async telemetry recorder for logging events to JSONL
pub struct TelemetryRecorder {
    tx: mpsc::Sender<TelemetryEvent>,
    config: TelemetryConfig,
}

impl TelemetryRecorder {
    /// Creates a new telemetry recorder and spawns the async writer task
    ///
    /// # Arguments
    /// * `config` - Configuration for the recorder
    ///
    /// # Returns
    /// * `Result<Self>` - The recorder instance or an error
    pub async fn new(config: TelemetryConfig) -> Result<Self> {
        if !config.enabled {
            info!("Telemetry disabled by configuration");
            let (tx, _rx) = mpsc::channel(1);
            return Ok(Self { tx, config });
        }

        // Ensure log directory exists
        if let Some(parent) = config.log_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create log directory")?;
        }

        let (tx, rx) = mpsc::channel(config.channel_buffer_size);

        // Spawn async writer task
        let log_path = config.log_path.clone();
        tokio::spawn(async move {
            if let Err(e) = run_writer_task(log_path, rx).await {
                error!("Telemetry writer task failed: {}", e);
            }
        });

        info!("Telemetry recorder initialized: {:?}", config.log_path);

        Ok(Self { tx, config })
    }

    /// Creates a recorder with default configuration
    pub async fn default_config() -> Result<Self> {
        Self::new(TelemetryConfig::default()).await
    }

    /// Logs an event (non-blocking, fire-and-forget)
    ///
    /// # Arguments
    /// * `event` - The event to log
    pub fn log(&self, event: TelemetryEvent) {
        if !self.config.enabled {
            return;
        }

        // Try to send, but don't block if channel is full
        if let Err(e) = self.tx.try_send(event) {
            // Only log errors occasionally to avoid log spam
            if rand::thread_rng().gen::<u8>() < 10 {
                warn!("Telemetry channel full or closed: {}", e);
            }
        }
    }

    /// Logs a decision event
    pub fn log_decision(
        &self,
        decision: WatchdogDecision,
        void_latency_ms: u64,
        qass_score: Option<f64>,
        internal_risk_scores: Vec<(String, f64)>,
        internal_tasks_completed: u32,
        internal_tasks_spawned: u32,
        external_result_received: bool,
    ) {
        let event = TelemetryEvent::decision(
            decision,
            void_latency_ms,
            qass_score,
            internal_risk_scores,
            internal_tasks_completed,
            internal_tasks_spawned,
            external_result_received,
        );
        self.log(event);
    }

    /// Logs a task completion event from a signal
    pub fn log_task_completion(&self, signal: &WatchdogSignal) {
        if let WatchdogSignal::InternalTaskResult {
            task_name,
            risk_detected,
            risk_score,
            details,
        } = signal
        {
            let event = TelemetryEvent::task_completion(
                task_name.clone(),
                *risk_detected,
                *risk_score,
                details.clone(),
            );
            self.log(event);
        }
    }

    /// Logs a metrics event
    pub fn log_metrics(
        &self,
        chaos_sims_per_sec: Option<f64>,
        gene_analysis_us: Option<u64>,
        loop_iteration_us: Option<u64>,
    ) {
        let event =
            TelemetryEvent::metrics(chaos_sims_per_sec, gene_analysis_us, loop_iteration_us);
        self.log(event);
    }

    /// Logs an error event
    pub fn log_error(&self, message: String, context: String) {
        let event = TelemetryEvent::error(message, context);
        self.log(event);
    }

    /// Logs a data quality alarm event
    ///
    /// This is automatically called when data integrity violations are detected
    /// by components like SnapshotEngine, SCRExtended, etc.
    ///
    /// # Arguments
    /// * `source` - Component that detected the alarm (e.g., "SnapshotEngine")
    /// * `severity` - Severity level ("SoftSync" or "HardAbort")
    /// * `alarm_type` - Type of alarm (e.g., "DuplicateTransaction", "ExcessiveJitter")
    /// * `pool_pubkey` - Optional pool pubkey if applicable
    /// * `details` - Detailed description of the alarm
    /// * `metrics` - Associated metric values as JSON
    pub fn log_data_quality_alarm(
        &self,
        source: String,
        severity: String,
        alarm_type: String,
        pool_pubkey: Option<String>,
        details: String,
        metrics: serde_json::Value,
    ) {
        let event = TelemetryEvent::DataQualityAlarm {
            timestamp_ms: TelemetryEvent::now_ms(),
            source,
            severity,
            alarm_type,
            pool_pubkey,
            details,
            metrics,
        };
        self.log(event);
    }

    /// Logs a HyperPrediction scoring event with immediate flush
    ///
    /// This method is specifically designed for full JSONL scoring output.
    /// It logs the complete scoring result including all subcomponents and
    /// their InsufficientData status (indicated by None values).
    ///
    /// # Arguments
    /// * `candidate_id` - Pool AMM ID or candidate identifier
    /// * `result` - Full HyperPrediction scoring result
    /// * `txs` - Raw transaction data (if available)
    ///
    /// # Immediate Flush
    /// Unlike other log methods, this triggers an immediate flush to ensure
    /// scoring data is committed to disk without delay.
    pub fn log_hyper_prediction_scoring(
        &self,
        candidate_id: &str,
        result: &HyperPredictionResult,
        txs: Vec<serde_json::Value>,
    ) {
        if !self.config.enabled {
            return;
        }

        let event = TelemetryEvent::hyper_prediction_scoring(candidate_id, result, txs);

        // Try to send with immediate flush flag
        if let Err(e) = self.tx.try_send(event) {
            // Only log errors occasionally to avoid log spam
            if rand::thread_rng().gen::<u8>() < 10 {
                warn!("Telemetry channel full or closed: {}", e);
            }
        }
    }
}

/// Async writer task that processes events and writes to JSONL
async fn run_writer_task(log_path: PathBuf, mut rx: mpsc::Receiver<TelemetryEvent>) -> Result<()> {
    // Open file in append mode
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await
        .with_context(|| format!("Failed to open log file: {:?}", log_path))?;

    info!("Telemetry writer task started: {:?}", log_path);

    let mut events_written = 0u64;
    let mut scoring_events_written = 0u64;

    while let Some(event) = rx.recv().await {
        // Check if this is a scoring event (requires immediate flush)
        let is_scoring_event = matches!(event, TelemetryEvent::HyperPredictionScoring { .. });

        // Serialize event to JSON
        match serde_json::to_string(&event) {
            Ok(json) => {
                // Write JSONL line (JSON + newline)
                let line = format!("{}\n", json);

                if let Err(e) = file.write_all(line.as_bytes()).await {
                    error!("Failed to write telemetry event: {}", e);
                    continue;
                }

                events_written += 1;

                // Immediate flush for scoring events or every 10 regular events
                if is_scoring_event {
                    scoring_events_written += 1;
                    if let Err(e) = file.flush().await {
                        error!("Failed to flush telemetry file after scoring event: {}", e);
                    } else {
                        debug!("Immediately flushed scoring event to disk");
                    }
                } else if events_written % 10 == 0 {
                    if let Err(e) = file.flush().await {
                        error!("Failed to flush telemetry file: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Failed to serialize telemetry event: {}", e);
            }
        }
    }

    // Final flush before closing
    let _ = file.flush().await;

    info!(
        "Telemetry writer task stopped. Total events: {}, Scoring events: {}",
        events_written, scoring_events_written
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardian::types::WatchdogDecision;
    use tokio::fs;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_telemetry_recorder_creation() {
        let config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_telemetry_create.jsonl"),
            channel_buffer_size: 10,
            enabled: true,
        };

        let recorder = TelemetryRecorder::new(config.clone()).await.unwrap();
        assert!(recorder.config.enabled);

        // Cleanup
        let _ = fs::remove_file(config.log_path).await;
    }

    #[tokio::test]
    async fn test_telemetry_disabled() {
        let config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_telemetry_disabled.jsonl"),
            channel_buffer_size: 10,
            enabled: false,
        };

        let recorder = TelemetryRecorder::new(config.clone()).await.unwrap();

        // Log some events
        recorder.log_decision(
            WatchdogDecision::Proceed,
            100,
            Some(0.8),
            vec![],
            0,
            0,
            true,
        );

        sleep(Duration::from_millis(50)).await;

        // File should not be created when disabled
        assert!(!config.log_path.exists());
    }

    #[tokio::test]
    async fn test_log_decision_event() {
        let config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_telemetry_decision.jsonl"),
            channel_buffer_size: 10,
            enabled: true,
        };

        let recorder = TelemetryRecorder::new(config.clone()).await.unwrap();

        // Log a decision
        recorder.log_decision(
            WatchdogDecision::Proceed,
            150,
            Some(0.85),
            vec![("Chaos Engine".to_string(), 0.2)],
            2,
            2,
            true,
        );

        // Give time for async write
        sleep(Duration::from_millis(100)).await;

        // Read and verify
        let content = fs::read_to_string(&config.log_path).await.unwrap();
        assert!(content.contains("Decision"));
        assert!(content.contains("Proceed"));
        assert!(content.contains("void_latency_ms"));
        assert!(content.contains("150"));

        // Cleanup
        let _ = fs::remove_file(config.log_path).await;
    }

    #[tokio::test]
    async fn test_log_task_completion() {
        let config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_telemetry_task.jsonl"),
            channel_buffer_size: 10,
            enabled: true,
        };

        let recorder = TelemetryRecorder::new(config.clone()).await.unwrap();

        // Create a signal
        let signal = WatchdogSignal::InternalTaskResult {
            task_name: "Gene Mapper".to_string(),
            risk_detected: true,
            risk_score: 0.6,
            details: "Pattern detected".to_string(),
        };

        recorder.log_task_completion(&signal);

        // Give time for async write
        sleep(Duration::from_millis(100)).await;

        // Read and verify
        let content = fs::read_to_string(&config.log_path).await.unwrap();
        assert!(content.contains("TaskCompletion"));
        assert!(content.contains("Gene Mapper"));
        assert!(content.contains("0.6"));

        // Cleanup
        let _ = fs::remove_file(config.log_path).await;
    }

    #[tokio::test]
    async fn test_log_metrics() {
        let config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_telemetry_metrics.jsonl"),
            channel_buffer_size: 10,
            enabled: true,
        };

        let recorder = TelemetryRecorder::new(config.clone()).await.unwrap();

        recorder.log_metrics(Some(12500.0), Some(450), Some(120));

        // Give time for async write
        sleep(Duration::from_millis(100)).await;

        // Read and verify
        let content = fs::read_to_string(&config.log_path).await.unwrap();
        assert!(content.contains("Metrics"));
        assert!(content.contains("chaos_sims_per_sec"));
        assert!(content.contains("12500"));

        // Cleanup
        let _ = fs::remove_file(config.log_path).await;
    }

    #[tokio::test]
    async fn test_multiple_events_jsonl_format() {
        let config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_telemetry_multiple.jsonl"),
            channel_buffer_size: 10,
            enabled: true,
        };

        let recorder = TelemetryRecorder::new(config.clone()).await.unwrap();

        // Log multiple events
        recorder.log_decision(
            WatchdogDecision::Proceed,
            100,
            Some(0.9),
            vec![],
            0,
            0,
            true,
        );

        recorder.log_metrics(Some(10000.0), None, None);

        recorder.log_error("Test error".to_string(), "test context".to_string());

        // Give time for async writes
        sleep(Duration::from_millis(150)).await;

        // Read and verify JSONL format
        let content = fs::read_to_string(&config.log_path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();

        // Should have 3 lines (one per event)
        assert_eq!(lines.len(), 3);

        // Each line should be valid JSON
        for line in lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("event_type").is_some());
            assert!(parsed.get("timestamp_ms").is_some());
        }

        // Cleanup
        let _ = fs::remove_file(config.log_path).await;
    }

    #[tokio::test]
    async fn test_log_hyper_prediction_scoring() {
        let config = TelemetryConfig {
            log_path: PathBuf::from("/tmp/test_telemetry_scoring.jsonl"),
            channel_buffer_size: 10,
            enabled: true,
        };

        let recorder = TelemetryRecorder::new(config.clone()).await.unwrap();

        // Create a mock HyperPredictionResult
        use crate::oracle::hyper_prediction::HyperPredictionResult;
        use crate::oracle::RiskLevel;

        let result = HyperPredictionResult {
            score: 75,
            passed: true,
            risk_level: RiskLevel::Low,
            analysis_phase: crate::oracle::hyper_prediction::AnalysisPhase::FullAnalysis,
            analysis_started_at: std::time::Instant::now(),
            ssmi_result: None, // InsufficientData
            mpcf_result: None,
            iwim_result: None,
            praecog_result: None,
            mesa_result: None,
            scr_score: Some(0.25),
            ulvf_divergence: Some(0.15),
            ulvf_curl: Some(0.08),
            povc_cluster: Some(1), // Organic Hype
            shadow_progress: Some(45),
            shadow_price_ratio: Some(1.25),
            base_score: 70,
            processing_time_us: 1500000, // 1.5s
            interpretation: "Strong candidate with low bot activity".to_string(),
            chaos_result: None,
            resonance_result: None,
            gene_safety_result: None,
            hunter_score: Some(80),
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
            fallback_tracker: crate::config::FallbackTracker::new(),
        };

        // Create mock transaction data
        let txs = vec![
            serde_json::json!({
                "signature": "sig1",
                "slot": 12345,
                "timestamp_ms": 1700000000000u64,
                "signer": "wallet1",
                "is_buy": true,
                "volume_sol": 0.5,
            }),
            serde_json::json!({
                "signature": "sig2",
                "slot": 12346,
                "timestamp_ms": 1700000001000u64,
                "signer": "wallet2",
                "is_buy": true,
                "volume_sol": 1.2,
            }),
        ];

        // Log the scoring event
        recorder.log_hyper_prediction_scoring("pool123", &result, txs);

        // Give time for async write
        sleep(Duration::from_millis(200)).await;

        // Read and verify
        let content = fs::read_to_string(&config.log_path).await.unwrap();

        // Parse the JSONL
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();

        // Verify structure
        assert_eq!(parsed["candidate_id"], "pool123");
        assert!(parsed["timestamp"].is_string());
        assert_eq!(parsed["final_score_initial"], 75);
        assert_eq!(parsed["passed"], true);
        assert_eq!(parsed["risk_level"], "Low");
        assert_eq!(parsed["base_score"], 70);

        // Verify transactions are present
        assert!(parsed["txs"].is_array());
        assert_eq!(parsed["txs"].as_array().unwrap().len(), 2);

        // Verify InsufficientData fields are null
        assert!(parsed["ssmi"].is_null());
        assert!(parsed["mpcf"].is_null());
        assert!(parsed["iwim"].is_null());
        assert!(parsed["qass"].is_null());

        // Verify present fields
        assert!(parsed["scr"].is_object());
        assert!(parsed["ulvf"].is_object());
        assert!(parsed["sobp"].is_object());
        assert!(parsed["qofsv"].is_object());
        assert_eq!(parsed["hunter_score"], 80);

        // Verify QOFSV mapping
        assert_eq!(parsed["qofsv"]["povc_cluster"], 1);
        assert_eq!(parsed["qofsv"]["cluster_name"], "Organic Hype");

        // Cleanup
        let _ = fs::remove_file(config.log_path).await;
    }
}
