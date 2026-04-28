//! EventWriter — JSONL file writer with rotation and periodic flushing.
//!
//! # File naming
//!
//! `{output_dir}/{run_id}_{date}_{seq}.jsonl`
//!
//! # Rotation
//!
//! A new file is created when:
//! - `rotation_interval_ms` has elapsed since the current file was opened, OR
//! - The current file exceeds `max_file_size_bytes`.
//!
//! # Flushing
//!
//! The internal buffer is flushed to disk every `flush_interval_ms` or when
//! `flush()` is called explicitly. On `Drop`, remaining events are flushed.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};

use super::schema::ExecutionEvent;

// ─── Config ─────────────────────────────────────────────────────────────────

/// Configuration for EventWriter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventWriterConfig {
    /// Output directory for JSONL files.
    pub output_dir: String,
    /// Time-based rotation interval (ms). Default: 300_000 (5 min).
    pub rotation_interval_ms: u64,
    /// Flush buffer to disk interval (ms). Default: 1_000 (1 sec).
    pub flush_interval_ms: u64,
    /// Maximum file size before rotation (bytes). Default: 50 MB.
    pub max_file_size_bytes: u64,
    /// Whether to enable AEM tick events.
    pub enable_aem_ticks: bool,
    /// Whether to enable optional events (stress changed, oracle stale, etc).
    pub enable_optional_events: bool,
}

impl Default for EventWriterConfig {
    fn default() -> Self {
        Self {
            output_dir: "datasets/events".to_string(),
            rotation_interval_ms: 300_000,   // 5 min
            flush_interval_ms: 1_000,        // 1 sec
            max_file_size_bytes: 50_000_000, // 50 MB
            enable_aem_ticks: true,
            enable_optional_events: false,
        }
    }
}

// ─── EventWriter ────────────────────────────────────────────────────────────

/// Writes `ExecutionEvent`s to JSONL files with automatic rotation.
pub struct EventWriter {
    config: EventWriterConfig,
    run_id: String,
    /// Current file being written to.
    current_writer: Option<BufWriter<File>>,
    /// Path of the current file.
    current_path: Option<PathBuf>,
    /// When the current file was opened (Unix ms).
    file_opened_at_ms: u64,
    /// Bytes written to the current file.
    current_file_bytes: u64,
    /// Sequence counter for file naming.
    file_seq: u32,
    /// Event buffer (for batch writing).
    buffer: VecDeque<String>,
    /// Total events written (across all files).
    total_events_written: u64,
    /// Last flush timestamp (Unix ms).
    last_flush_ms: u64,
}

impl EventWriter {
    /// Create a new EventWriter. Creates the output directory if needed.
    pub fn new(config: EventWriterConfig, run_id: String) -> std::io::Result<Self> {
        // Ensure output directory exists
        fs::create_dir_all(&config.output_dir)?;

        info!(
            output_dir = %config.output_dir,
            run_id = %run_id,
            rotation_ms = config.rotation_interval_ms,
            "EventWriter initialized"
        );

        let now_ms = Self::now_ms();

        let mut writer = Self {
            config,
            run_id,
            current_writer: None,
            current_path: None,
            file_opened_at_ms: now_ms,
            current_file_bytes: 0,
            file_seq: 0,
            buffer: VecDeque::with_capacity(256),
            total_events_written: 0,
            last_flush_ms: now_ms,
        };

        // Open first file
        writer.rotate(now_ms)?;

        Ok(writer)
    }

    /// Write a single event. The event is serialized to JSON and buffered.
    /// Call `maybe_flush()` periodically or `flush()` explicitly.
    pub fn write_event(&mut self, event: &ExecutionEvent) -> std::io::Result<()> {
        // Filter based on config
        if !self.should_write(event) {
            return Ok(());
        }

        let json = serde_json::to_string(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        self.buffer.push_back(json);

        // Check if we should auto-flush
        let now = Self::now_ms();
        if now.saturating_sub(self.last_flush_ms) >= self.config.flush_interval_ms {
            self.flush_internal(now)?;
        }

        Ok(())
    }

    /// Write multiple events at once.
    pub fn write_events(&mut self, events: &[ExecutionEvent]) -> std::io::Result<()> {
        for event in events {
            self.write_event(event)?;
        }
        Ok(())
    }

    /// Flush the buffer to disk and rotate file if needed.
    pub fn flush(&mut self) -> std::io::Result<()> {
        let now = Self::now_ms();
        self.flush_internal(now)
    }

    /// Returns the total number of events written across all files.
    pub fn total_events_written(&self) -> u64 {
        self.total_events_written
    }

    /// Returns the current output file path.
    pub fn current_file_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }

    /// Returns the number of buffered events waiting to be flushed.
    pub fn buffered_count(&self) -> usize {
        self.buffer.len()
    }

    // ── Internal ────────────────────────────────────────────────────────

    fn should_write(&self, event: &ExecutionEvent) -> bool {
        // Filter optional events
        if event.kind.is_optional() && !self.config.enable_optional_events {
            return false;
        }
        // Filter AEM ticks
        if matches!(event.kind, super::schema::EventKind::AemTick(_))
            && !self.config.enable_aem_ticks
        {
            return false;
        }
        true
    }

    fn flush_internal(&mut self, now_ms: u64) -> std::io::Result<()> {
        if self.buffer.is_empty() {
            self.last_flush_ms = now_ms;
            return Ok(());
        }

        // Check if rotation is needed
        let needs_rotate = now_ms.saturating_sub(self.file_opened_at_ms)
            >= self.config.rotation_interval_ms
            || self.current_file_bytes >= self.config.max_file_size_bytes;

        if needs_rotate {
            self.rotate(now_ms)?;
        }

        // Write buffered events
        let writer = self.current_writer.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "no active file writer")
        })?;

        while let Some(json_line) = self.buffer.pop_front() {
            let line_bytes = json_line.len() as u64 + 1; // +1 for newline
            writer.write_all(json_line.as_bytes())?;
            writer.write_all(b"\n")?;
            self.current_file_bytes += line_bytes;
            self.total_events_written += 1;
        }

        writer.flush()?;
        self.last_flush_ms = now_ms;

        debug!(
            total = self.total_events_written,
            file_bytes = self.current_file_bytes,
            "EventWriter flushed"
        );

        Ok(())
    }

    fn rotate(&mut self, now_ms: u64) -> std::io::Result<()> {
        // Flush remaining buffer to old file before rotating
        if let Some(ref mut w) = self.current_writer {
            while let Some(json_line) = self.buffer.pop_front() {
                w.write_all(json_line.as_bytes())?;
                w.write_all(b"\n")?;
                self.total_events_written += 1;
            }
            w.flush()?;
        }

        // Generate new filename
        let date = Self::format_date(now_ms);
        let filename = format!(
            "{}_{}_{}_{:04}.jsonl",
            "exec", self.run_id, date, self.file_seq
        );
        let path = Path::new(&self.config.output_dir).join(&filename);

        // Open new file
        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        info!(
            path = %path.display(),
            seq = self.file_seq,
            "EventWriter rotated to new file"
        );

        self.current_writer = Some(BufWriter::new(file));
        self.current_path = Some(path);
        self.file_opened_at_ms = now_ms;
        self.current_file_bytes = 0;
        self.file_seq += 1;

        Ok(())
    }

    fn format_date(now_ms: u64) -> String {
        // Simple date format: YYYYMMDD_HHMMSS
        let secs = now_ms / 1000;
        let days_since_epoch = secs / 86400;
        let time_of_day = secs % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        // Approximate date calculation (good enough for file naming)
        let mut year = 1970u32;
        let mut remaining_days = days_since_epoch as u32;
        loop {
            let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                366
            } else {
                365
            };
            if remaining_days < days_in_year {
                break;
            }
            remaining_days -= days_in_year;
            year += 1;
        }

        let days_in_months: [u32; 12] = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut month = 1u32;
        for &dim in &days_in_months {
            if remaining_days < dim {
                break;
            }
            remaining_days -= dim;
            month += 1;
        }
        let day = remaining_days + 1;

        format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}",
            year, month, day, hours, minutes, seconds
        )
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

impl Drop for EventWriter {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            error!(error = %e, "EventWriter: failed to flush on drop");
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::schema::*;
    use crate::execution::backend::{FillStatus, Lane, OrderSide};
    use tempfile::TempDir;

    fn make_test_event(kind: EventKind) -> ExecutionEvent {
        let mut env = EventEnvelope::new(
            "test-run".to_string(),
            Lane::Paper,
            "mint_pool_123".to_string(),
            1700000000000,
        );
        env.order_id = Some("ord-1".to_string());
        ExecutionEvent::new(env, kind)
    }

    fn make_candidate_event() -> ExecutionEvent {
        make_test_event(EventKind::Candidate(CandidatePayload {
            mcap_snapshot: Some(50000.0),
            price_snapshot: Some(0.001),
            gatekeeper_verdict: "PASS".to_string(),
            gatekeeper_flags: vec!["ok".to_string()],
            source: "grpc".to_string(),
        }))
    }

    #[test]
    fn test_writer_creates_output_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("events_out");

        let config = EventWriterConfig {
            output_dir: dir.to_str().unwrap().to_string(),
            ..Default::default()
        };

        let writer = EventWriter::new(config, "run-1".to_string()).unwrap();
        assert!(dir.exists());
        assert!(writer.current_file_path().is_some());
    }

    #[test]
    fn test_writer_writes_jsonl() {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            ..Default::default()
        };

        let mut writer = EventWriter::new(config, "run-2".to_string()).unwrap();

        let event = make_candidate_event();
        writer.write_event(&event).unwrap();
        writer.flush().unwrap();

        assert_eq!(writer.total_events_written(), 1);

        // Read back the file
        let path = writer.current_file_path().unwrap().to_owned();
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        // Verify valid JSON
        let _: ExecutionEvent = serde_json::from_str(lines[0]).unwrap();
    }

    #[test]
    fn test_writer_multiple_events() {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            ..Default::default()
        };

        let mut writer = EventWriter::new(config, "run-3".to_string()).unwrap();

        for i in 0..10 {
            let mut event = make_candidate_event();
            event.envelope.event_time_ms = 1700000000000 + i * 1000;
            writer.write_event(&event).unwrap();
        }
        writer.flush().unwrap();

        assert_eq!(writer.total_events_written(), 10);
    }

    #[test]
    fn test_writer_filters_optional_events() {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            enable_optional_events: false, // OFF
            ..Default::default()
        };

        let mut writer = EventWriter::new(config, "run-4".to_string()).unwrap();

        // Write a mandatory event
        let mandatory = make_candidate_event();
        writer.write_event(&mandatory).unwrap();

        // Write an optional event
        let optional = make_test_event(EventKind::OracleStale(OracleStalePayload {
            stale_age_ms: 2000,
            threshold_ms: 1500,
        }));
        writer.write_event(&optional).unwrap();

        writer.flush().unwrap();

        // Only mandatory should be written
        assert_eq!(writer.total_events_written(), 1);
    }

    #[test]
    fn test_writer_filters_aem_ticks() {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            enable_aem_ticks: false, // OFF
            ..Default::default()
        };

        let mut writer = EventWriter::new(config, "run-5".to_string()).unwrap();

        let tick = make_test_event(EventKind::AemTick(AemTickPayload {
            regime_key: "k".to_string(),
            regime_tag: "t".to_string(),
            features_summary: serde_json::json!({}),
            rollout_mode: "Shadow".to_string(),
            hard_safety_state: None,
            drawdown_pct: 0.0,
            unrealized_pnl_pct: 0.0,
        }));
        writer.write_event(&tick).unwrap();

        writer.flush().unwrap();
        assert_eq!(writer.total_events_written(), 0);
    }

    #[test]
    fn test_writer_rotation_by_size() {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            max_file_size_bytes: 100,       // Very small — force rotation
            rotation_interval_ms: u64::MAX, // Don't rotate by time
            flush_interval_ms: 0,
            ..Default::default()
        };

        let mut writer = EventWriter::new(config, "run-6".to_string()).unwrap();

        // Write enough events to trigger rotation
        for _ in 0..5 {
            let event = make_candidate_event();
            writer.write_event(&event).unwrap();
            writer.flush().unwrap();
        }

        // Should have rotated at least once
        assert!(
            writer.file_seq >= 2,
            "Expected rotation, seq = {}",
            writer.file_seq
        );
        assert_eq!(writer.total_events_written(), 5);
    }

    #[test]
    fn test_format_date() {
        // 2026-02-24 21:30:00 UTC approximately
        // 1772061000 seconds since epoch * 1000
        let date = EventWriter::format_date(1772061000000);
        assert!(date.starts_with("2026")); // Year check
    }

    #[test]
    fn test_writer_drop_flushes() {
        let tmp = TempDir::new().unwrap();
        let config = EventWriterConfig {
            output_dir: tmp.path().to_str().unwrap().to_string(),
            flush_interval_ms: u64::MAX, // Never auto-flush
            ..Default::default()
        };

        let path;
        {
            let mut writer = EventWriter::new(config, "run-7".to_string()).unwrap();
            path = writer.current_file_path().unwrap().to_owned();
            let event = make_candidate_event();
            writer.write_event(&event).unwrap();
            // Don't call flush — Drop should handle it
        }

        // File should have content after Drop
        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.is_empty(), "Drop should flush remaining events");
    }
}
