//! Custom logging formatters and utilities for Ghost Launcher
//!
//! This module provides specialized logging formatters for different log targets:
//! - Oracle Decision Formatter: For `ghost_brain::oracle` targets with enhanced formatting
//! - Standard Formatter: For system logs

use std::fmt;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::{format::Writer, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// Custom formatter for Oracle decision logs
///
/// Provides a clean, structured format specifically designed for Oracle decisions:
/// ```text
/// [2025-12-14T11:25:10.798Z] [INFO] [ghost_brain::oracle]
///   Timestamp: 2025-12-14T11:25:10.798Z
///   Chaos ROI: 85.3%
///   Resonance: 0.72
///   Transaction Metrics: { count: 15, volume: 2.5 SOL }
///   Score: 87/100
///   Message: Oracle decision made for pool ABC123...
/// ```
pub struct OracleDecisionFormatter {
    pub use_ansi: bool,
}

impl OracleDecisionFormatter {
    pub fn new(use_ansi: bool) -> Self {
        Self { use_ansi }
    }
}

impl<S, N> FormatEvent<S, N> for OracleDecisionFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();

        // Format timestamp
        let now = chrono::Utc::now();
        let timestamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        // Format level with optional ANSI colors
        let level_str = if self.use_ansi {
            match *metadata.level() {
                Level::ERROR => format!("\x1b[31m{}\x1b[0m", "ERROR"),
                Level::WARN => format!("\x1b[33m{}\x1b[0m", "WARN"),
                Level::INFO => format!("\x1b[32m{}\x1b[0m", "INFO"),
                Level::DEBUG => format!("\x1b[36m{}\x1b[0m", "DEBUG"),
                Level::TRACE => format!("\x1b[35m{}\x1b[0m", "TRACE"),
            }
        } else {
            metadata.level().to_string()
        };

        // Write header line
        writeln!(
            writer,
            "[{}] [{}] [{}]",
            timestamp,
            level_str,
            metadata.target()
        )?;

        // Format fields if available
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

/// Standard formatter for system logs
///
/// Provides a concise single-line format for system logs:
/// ```text
/// 2025-12-14T11:25:10.798Z INFO ghost_launcher: Starting Ghost Launcher
/// ```
pub struct StandardFormatter {
    pub use_ansi: bool,
}

impl StandardFormatter {
    pub fn new(use_ansi: bool) -> Self {
        Self { use_ansi }
    }
}

impl<S, N> FormatEvent<S, N> for StandardFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();

        // Format timestamp
        let now = chrono::Utc::now();
        let timestamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        // Format level with optional ANSI colors
        let level_str = if self.use_ansi {
            match *metadata.level() {
                Level::ERROR => format!("\x1b[31m{:5}\x1b[0m", "ERROR"),
                Level::WARN => format!("\x1b[33m{:5}\x1b[0m", "WARN"),
                Level::INFO => format!("\x1b[32m{:5}\x1b[0m", "INFO"),
                Level::DEBUG => format!("\x1b[36m{:5}\x1b[0m", "DEBUG"),
                Level::TRACE => format!("\x1b[35m{:5}\x1b[0m", "TRACE"),
            }
        } else {
            format!("{:5}", metadata.level())
        };

        write!(
            writer,
            "{} {} {}: ",
            timestamp,
            level_str,
            metadata.target()
        )?;

        // Format fields
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}
