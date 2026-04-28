//! Watchdog types for the Guardian module

use serde::{Deserialize, Serialize};

/// Severity level for data integrity violations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegritySeverity {
    /// Jitter > 400ms, duplicate transaction, missing slot.
    /// Action: Warning, lower Confidence, force SoftResync.
    SoftSync,

    /// Detected slot regression (Reorg), conflicting data (SL vs Geyser),
    /// or timestamp manipulation > 2s.
    /// Action: IMMEDIATE ABORT (Hard Kill).
    HardAbort,
}

/// Signals that the watchdog can receive
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatchdogSignal {
    /// Slot has been updated - triggers re-evaluation of fast metrics (QASS)
    SlotUpdate {
        slot: u64,
        timestamp_ms: u64,
        /// Optional QASS score for this slot (if available)
        qass_score: Option<f64>,
    },
    /// External result received (e.g., from Hunter/Oracle)
    ExternalResult { success: bool, data: String },
    /// Internal task completed (Chaos Engine, Gene Mapper, etc.)
    InternalTaskResult {
        task_name: String,
        risk_detected: bool,
        risk_score: f64,
        details: String,
    },
    /// Critical failure detected
    CriticalFailure { reason: String },
    /// Data integrity violation detected
    DataIntegrityViolation {
        /// Source of the violation (e.g., "SnapshotEngine", "Seer")
        source: String,
        /// Severity level of the violation
        severity: IntegritySeverity,
        /// Detailed description (e.g., "Slot regression: 100 -> 99")
        details: String,
        /// Timestamp in milliseconds when violation was detected
        timestamp_ms: u64,
    },
    /// Network stress detected by Paradox Sensor (EchoScanner)
    NetworkStress {
        /// Market tension score (0.0 - 100.0+)
        tension: f64,
        /// Jitter in milliseconds
        jitter_ms: f64,
        /// Packet density (packets per second)
        density_bps: f64,
        /// Whether HFT anomaly is detected
        anomaly_detected: bool,
        /// Timestamp in milliseconds when stress was detected
        timestamp_ms: u64,
    },
    /// Paradox State (Enhanced) - Full predictive analysis from Paradox Sensor
    ParadoxState {
        /// Market tension score (0.0 - 100.0)
        tension: f64,
        /// Derivative - direction of tension changes (-1.0 to +1.0)
        derivative: f64,
        /// Phase sync - bot synchronization strength (0.0 to 1.0)
        phase_sync: f64,
        /// Paradox Decision Score (0.0 - 100.0)
        pds_score: f64,
        /// Echo spike detection flag
        is_echo_spike: bool,
        /// Whether HFT anomaly is detected
        anomaly_detected: bool,
        /// Timestamp in milliseconds when state was calculated
        timestamp_ms: u64,
    },
}

/// Decisions the watchdog can make
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchdogDecision {
    /// Proceed with execution
    Proceed,
    /// Abort the operation
    Abort,
    /// Operation timed out
    Timeout,
}

/// Configuration for the watchdog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// Maximum duration to wait in void state (milliseconds)
    pub max_void_duration_ms: u64,
    /// Threshold for critical failures
    pub critical_failure_threshold: u32,
    /// Threshold for retry attempts
    pub retry_threshold: u32,
    /// Minimum QASS score to proceed (abort if below this)
    pub min_qass_score: f64,
    /// Maximum risk score from internal tasks (abort if above this)
    pub max_internal_risk_score: f64,
    /// Enable parallel task execution (Chaos, Gene)
    pub enable_parallel_tasks: bool,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            max_void_duration_ms: 2000, // 2 seconds
            critical_failure_threshold: 3,
            retry_threshold: 5,
            min_qass_score: 0.5,           // Neutral threshold
            max_internal_risk_score: 0.75, // 75% risk threshold
            enable_parallel_tasks: true,
        }
    }
}

/// Context maintained by the watchdog during execution
#[derive(Debug, Clone, Default)]
pub struct WatchdogContext {
    /// Latest QASS score
    pub latest_qass_score: Option<f64>,
    /// Latest slot number
    pub latest_slot: Option<u64>,
    /// Accumulated risk scores from internal tasks
    pub internal_risk_scores: Vec<(String, f64)>,
    /// Whether external result has arrived
    pub external_result_received: bool,
    /// Number of internal tasks completed
    pub internal_tasks_completed: u32,
    /// Number of internal tasks spawned
    pub internal_tasks_spawned: u32,
    /// Counter for data integrity violations
    pub integrity_violations_count: u32,
    /// Timestamp of last integrity violation (milliseconds)
    pub last_violation_ts: Option<u64>,
    /// Timestamp of first integrity violation in current window (milliseconds)
    pub first_violation_ts_in_window: Option<u64>,
    /// Internal risk score accumulated from soft violations and penalties
    pub internal_risk_score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integrity_severity_variants() {
        // Test that all severity variants exist and are distinct
        let soft = IntegritySeverity::SoftSync;
        let hard = IntegritySeverity::HardAbort;

        assert_ne!(soft, hard);
        assert_eq!(soft, IntegritySeverity::SoftSync);
        assert_eq!(hard, IntegritySeverity::HardAbort);
    }

    #[test]
    fn test_integrity_severity_serialization() {
        // Test JSON serialization/deserialization
        let soft = IntegritySeverity::SoftSync;
        let json = serde_json::to_string(&soft).unwrap();
        let deserialized: IntegritySeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(soft, deserialized);

        let hard = IntegritySeverity::HardAbort;
        let json = serde_json::to_string(&hard).unwrap();
        let deserialized: IntegritySeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(hard, deserialized);
    }

    #[test]
    fn test_data_integrity_violation_signal() {
        // Test creating and serializing a DataIntegrityViolation signal
        let signal = WatchdogSignal::DataIntegrityViolation {
            source: "SnapshotEngine".to_string(),
            severity: IntegritySeverity::SoftSync,
            details: "Jitter detected: 450ms".to_string(),
            timestamp_ms: 1234567890,
        };

        // Verify it can be serialized and deserialized
        let json = serde_json::to_string(&signal).unwrap();
        let deserialized: WatchdogSignal = serde_json::from_str(&json).unwrap();

        match deserialized {
            WatchdogSignal::DataIntegrityViolation {
                source,
                severity,
                details,
                timestamp_ms,
            } => {
                assert_eq!(source, "SnapshotEngine");
                assert_eq!(severity, IntegritySeverity::SoftSync);
                assert_eq!(details, "Jitter detected: 450ms");
                assert_eq!(timestamp_ms, 1234567890);
            }
            _ => panic!("Expected DataIntegrityViolation variant"),
        }
    }

    #[test]
    fn test_data_integrity_violation_hard_abort() {
        // Test HardAbort severity scenario
        let signal = WatchdogSignal::DataIntegrityViolation {
            source: "Seer".to_string(),
            severity: IntegritySeverity::HardAbort,
            details: "Slot regression detected: 100 -> 99".to_string(),
            timestamp_ms: 9876543210,
        };

        let json = serde_json::to_string(&signal).unwrap();
        let deserialized: WatchdogSignal = serde_json::from_str(&json).unwrap();

        match deserialized {
            WatchdogSignal::DataIntegrityViolation { severity, .. } => {
                assert_eq!(severity, IntegritySeverity::HardAbort);
            }
            _ => panic!("Expected DataIntegrityViolation variant"),
        }
    }

    #[test]
    fn test_watchdog_context_integrity_fields() {
        // Test that WatchdogContext has integrity tracking fields
        let mut context = WatchdogContext::default();

        // Verify default values
        assert_eq!(context.integrity_violations_count, 0);
        assert_eq!(context.last_violation_ts, None);

        // Simulate tracking violations
        context.integrity_violations_count = 3;
        context.last_violation_ts = Some(1234567890);

        assert_eq!(context.integrity_violations_count, 3);
        assert_eq!(context.last_violation_ts, Some(1234567890));
    }

    #[test]
    fn test_watchdog_context_tracks_multiple_violations() {
        // Simulate accumulating violations over time
        let mut context = WatchdogContext::default();

        // First violation
        context.integrity_violations_count += 1;
        context.last_violation_ts = Some(1000);
        assert_eq!(context.integrity_violations_count, 1);

        // Second violation
        context.integrity_violations_count += 1;
        context.last_violation_ts = Some(2000);
        assert_eq!(context.integrity_violations_count, 2);

        // Third violation
        context.integrity_violations_count += 1;
        context.last_violation_ts = Some(3000);
        assert_eq!(context.integrity_violations_count, 3);
        assert_eq!(context.last_violation_ts, Some(3000));
    }

    #[test]
    fn test_all_watchdog_signal_variants_serialize() {
        // Ensure all variants including new DataIntegrityViolation serialize correctly
        let signals = vec![
            WatchdogSignal::SlotUpdate {
                slot: 123,
                timestamp_ms: 1000,
                qass_score: Some(0.8),
            },
            WatchdogSignal::ExternalResult {
                success: true,
                data: "test".to_string(),
            },
            WatchdogSignal::InternalTaskResult {
                task_name: "test".to_string(),
                risk_detected: false,
                risk_score: 0.1,
                details: "none".to_string(),
            },
            WatchdogSignal::CriticalFailure {
                reason: "test".to_string(),
            },
            WatchdogSignal::DataIntegrityViolation {
                source: "test".to_string(),
                severity: IntegritySeverity::SoftSync,
                details: "test".to_string(),
                timestamp_ms: 1000,
            },
            WatchdogSignal::NetworkStress {
                tension: 85.0,
                jitter_ms: 2.5,
                density_bps: 450.0,
                anomaly_detected: true,
                timestamp_ms: 1000,
            },
            WatchdogSignal::ParadoxState {
                tension: 87.5,
                derivative: 0.5,
                phase_sync: 0.8,
                pds_score: 85.0,
                is_echo_spike: true,
                anomaly_detected: true,
                timestamp_ms: 1000,
            },
        ];

        for signal in signals {
            let json = serde_json::to_string(&signal).unwrap();
            let _: WatchdogSignal = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_network_stress_signal() {
        // Test creating and serializing a NetworkStress signal from Paradox Sensor
        let signal = WatchdogSignal::NetworkStress {
            tension: 87.5,
            jitter_ms: 2.14,
            density_bps: 456.0,
            anomaly_detected: true,
            timestamp_ms: 1234567890,
        };

        // Verify it can be serialized and deserialized
        let json = serde_json::to_string(&signal).unwrap();
        let deserialized: WatchdogSignal = serde_json::from_str(&json).unwrap();

        match deserialized {
            WatchdogSignal::NetworkStress {
                tension,
                jitter_ms,
                density_bps,
                anomaly_detected,
                timestamp_ms,
            } => {
                assert!((tension - 87.5).abs() < 0.01);
                assert!((jitter_ms - 2.14).abs() < 0.01);
                assert!((density_bps - 456.0).abs() < 0.01);
                assert!(anomaly_detected);
                assert_eq!(timestamp_ms, 1234567890);
            }
            _ => panic!("Expected NetworkStress variant"),
        }
    }

    #[test]
    fn test_network_stress_signal_high_tension() {
        // Test high tension scenario (> 80) for Guardian blocking
        let signal = WatchdogSignal::NetworkStress {
            tension: 92.3,
            jitter_ms: 1.8,
            density_bps: 520.0,
            anomaly_detected: true,
            timestamp_ms: 9876543210,
        };

        let json = serde_json::to_string(&signal).unwrap();
        let deserialized: WatchdogSignal = serde_json::from_str(&json).unwrap();

        match deserialized {
            WatchdogSignal::NetworkStress {
                tension,
                anomaly_detected,
                ..
            } => {
                assert!(tension > 80.0, "High tension scenario");
                assert!(anomaly_detected, "Anomaly should be detected");
            }
            _ => panic!("Expected NetworkStress variant"),
        }
    }
}
