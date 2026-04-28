//! Metrics collection and export module

use prometheus::{Histogram, HistogramOpts, IntCounter, IntGauge, Opts, Registry};
use std::time::{Duration, Instant};

/// Global metrics registry
pub struct Metrics {
    registry: Registry,

    // Counters
    pub trades_total: IntCounter,
    pub trades_success: IntCounter,
    pub trades_failed: IntCounter,
    pub candidates_received: IntCounter,
    pub candidates_filtered: IntCounter,

    // Nonce-related counters
    pub nonce_leases_dropped_auto: IntCounter,
    pub nonce_leases_dropped_explicit: IntCounter,
    pub nonce_sequence_errors: IntCounter,
    pub nonce_enforce_paths: IntCounter,

    // Gauges
    pub active_trades: IntGauge,
    pub nonce_pool_size: IntGauge,
    pub rpc_connections: IntGauge,
    pub nonce_active_leases: IntGauge,

    // Histograms
    pub trade_latency: Histogram,
    pub rpc_latency: Histogram,
    pub build_latency: Histogram,
    pub nonce_lease_lifetime: Histogram,

    // Task 5: Additional nonce and tx_builder metrics
    pub acquire_lease_ms: Histogram,
    pub prepare_bundle_ms: Histogram,
    pub build_to_land_ms: Histogram,

    // Task 5: Additional counters for nonce operations
    pub total_acquires: IntCounter,
    pub total_releases: IntCounter,
    pub total_refreshes: IntCounter,
    pub total_failures: IntCounter,
}

impl Metrics {
    /// Create new metrics instance
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        let trades_total = IntCounter::with_opts(Opts::new(
            "trades_total",
            "Total number of trades attempted",
        ))?;

        let trades_success =
            IntCounter::with_opts(Opts::new("trades_success", "Number of successful trades"))?;

        let trades_failed =
            IntCounter::with_opts(Opts::new("trades_failed", "Number of failed trades"))?;

        let candidates_received = IntCounter::with_opts(Opts::new(
            "candidates_received",
            "Number of candidates received from sniffer",
        ))?;

        let candidates_filtered = IntCounter::with_opts(Opts::new(
            "candidates_filtered",
            "Number of candidates filtered out",
        ))?;

        // Nonce-related counters
        let nonce_leases_dropped_auto = IntCounter::with_opts(Opts::new(
            "nonce_leases_dropped_auto",
            "Number of nonce leases auto-released via Drop",
        ))?;

        let nonce_leases_dropped_explicit = IntCounter::with_opts(Opts::new(
            "nonce_leases_dropped_explicit",
            "Number of nonce leases explicitly released",
        ))?;

        let nonce_sequence_errors = IntCounter::with_opts(Opts::new(
            "nonce_sequence_errors",
            "Number of nonce sequence violations (debug/test)",
        ))?;

        let nonce_enforce_paths = IntCounter::with_opts(Opts::new(
            "nonce_enforce_paths",
            "Counter for different code paths in nonce enforcement",
        ))?;

        let active_trades = IntGauge::with_opts(Opts::new(
            "active_trades",
            "Number of trades currently in progress",
        ))?;

        let nonce_pool_size =
            IntGauge::with_opts(Opts::new("nonce_pool_size", "Current nonce pool size"))?;

        let rpc_connections = IntGauge::with_opts(Opts::new(
            "rpc_connections",
            "Number of active RPC connections",
        ))?;

        let nonce_active_leases = IntGauge::with_opts(Opts::new(
            "nonce_active_leases",
            "Number of currently held nonce leases",
        ))?;

        let trade_latency = Histogram::with_opts(
            HistogramOpts::new("trade_latency_seconds", "Trade execution latency")
                .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0]),
        )?;

        let rpc_latency = Histogram::with_opts(
            HistogramOpts::new("rpc_latency_seconds", "RPC call latency")
                .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
        )?;

        let build_latency = Histogram::with_opts(
            HistogramOpts::new("build_latency_seconds", "Transaction build latency")
                .buckets(vec![0.001, 0.005, 0.01, 0.02, 0.05, 0.1]),
        )?;

        let nonce_lease_lifetime = Histogram::with_opts(
            HistogramOpts::new(
                "nonce_lease_lifetime_seconds",
                "Duration nonce leases are held",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0]),
        )?;

        // Task 5: Additional nonce and tx_builder metrics
        let acquire_lease_ms = Histogram::with_opts(
            HistogramOpts::new(
                "acquire_lease_ms",
                "Time to acquire nonce lease in milliseconds",
            )
            .buckets(vec![0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]),
        )?;

        let prepare_bundle_ms = Histogram::with_opts(
            HistogramOpts::new(
                "prepare_bundle_ms",
                "Time to prepare bundle for submission in milliseconds",
            )
            .buckets(vec![1.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0]),
        )?;

        let build_to_land_ms = Histogram::with_opts(
            HistogramOpts::new(
                "build_to_land_ms",
                "Total time from build to transaction landing in milliseconds",
            )
            .buckets(vec![
                10.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0,
            ]),
        )?;

        // Task 5: Additional counters for nonce operations
        let total_acquires = IntCounter::with_opts(Opts::new(
            "total_acquires",
            "Total number of nonce lease acquisitions",
        ))?;

        let total_releases = IntCounter::with_opts(Opts::new(
            "total_releases",
            "Total number of nonce lease releases",
        ))?;

        let total_refreshes = IntCounter::with_opts(Opts::new(
            "total_refreshes",
            "Total number of nonce refreshes",
        ))?;

        let total_failures = IntCounter::with_opts(Opts::new(
            "total_failures",
            "Total number of nonce operation failures",
        ))?;

        // Register all metrics
        registry.register(Box::new(trades_total.clone()))?;
        registry.register(Box::new(trades_success.clone()))?;
        registry.register(Box::new(trades_failed.clone()))?;
        registry.register(Box::new(candidates_received.clone()))?;
        registry.register(Box::new(candidates_filtered.clone()))?;
        registry.register(Box::new(nonce_leases_dropped_auto.clone()))?;
        registry.register(Box::new(nonce_leases_dropped_explicit.clone()))?;
        registry.register(Box::new(nonce_sequence_errors.clone()))?;
        registry.register(Box::new(nonce_enforce_paths.clone()))?;
        registry.register(Box::new(active_trades.clone()))?;
        registry.register(Box::new(nonce_pool_size.clone()))?;
        registry.register(Box::new(rpc_connections.clone()))?;
        registry.register(Box::new(nonce_active_leases.clone()))?;
        registry.register(Box::new(trade_latency.clone()))?;
        registry.register(Box::new(rpc_latency.clone()))?;
        registry.register(Box::new(build_latency.clone()))?;
        registry.register(Box::new(nonce_lease_lifetime.clone()))?;

        // Task 5: Register additional metrics
        registry.register(Box::new(acquire_lease_ms.clone()))?;
        registry.register(Box::new(prepare_bundle_ms.clone()))?;
        registry.register(Box::new(build_to_land_ms.clone()))?;
        registry.register(Box::new(total_acquires.clone()))?;
        registry.register(Box::new(total_releases.clone()))?;
        registry.register(Box::new(total_refreshes.clone()))?;
        registry.register(Box::new(total_failures.clone()))?;

        Ok(Self {
            registry,
            trades_total,
            trades_success,
            trades_failed,
            candidates_received,
            candidates_filtered,
            nonce_leases_dropped_auto,
            nonce_leases_dropped_explicit,
            nonce_sequence_errors,
            nonce_enforce_paths,
            active_trades,
            nonce_pool_size,
            rpc_connections,
            nonce_active_leases,
            trade_latency,
            rpc_latency,
            build_latency,
            nonce_lease_lifetime,
            acquire_lease_ms,
            prepare_bundle_ms,
            build_to_land_ms,
            total_acquires,
            total_releases,
            total_refreshes,
            total_failures,
        })
    }

    /// Get the registry for exporting
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Increment a named counter (for dynamic counter names)
    /// Falls back to a no-op if the counter doesn't exist
    pub fn increment_counter(&self, name: &str) {
        // Map common counter names to their fields
        match name {
            "trades_total" | "buy_attempts_total" => self.trades_total.inc(),
            "trades_success" | "buy_success_total" => self.trades_success.inc(),
            "trades_failed" | "buy_failure_total" => self.trades_failed.inc(),
            "candidates_received" => self.candidates_received.inc(),
            "candidates_filtered" | "buy_attempts_filtered" => self.candidates_filtered.inc(),
            "nonce_leases_dropped_auto" => self.nonce_leases_dropped_auto.inc(),
            "nonce_leases_dropped_explicit" => self.nonce_leases_dropped_explicit.inc(),
            "nonce_sequence_errors" => self.nonce_sequence_errors.inc(),
            "nonce_enforce_paths" => self.nonce_enforce_paths.inc(),
            // For any other counter names that don't map to predefined counters,
            // we silently ignore (could log a warning in the future)
            _ => {
                // Use trades_total as a catch-all for unknown counters
                // This allows the code to compile but may not give accurate metrics
                tracing::debug!("Unknown counter name: {}", name);
            }
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Failed to create metrics")
    }
}

/// Global metrics instance
pub fn metrics() -> &'static Metrics {
    static METRICS: once_cell::sync::Lazy<Metrics> =
        once_cell::sync::Lazy::new(|| Metrics::new().expect("Failed to initialize metrics"));
    &METRICS
}

/// Timer helper for measuring operation duration
pub struct Timer {
    start: Instant,
    histogram_name: Option<String>,
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

impl Timer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            histogram_name: None,
        }
    }

    /// Create a timer with a histogram name for automatic recording
    pub fn with_name(histogram_name: &str) -> Self {
        Self {
            start: Instant::now(),
            histogram_name: Some(histogram_name.to_string()),
        }
    }

    pub fn observe_duration(&self, histogram: &Histogram) {
        let duration = self.start.elapsed();
        histogram.observe(duration.as_secs_f64());
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Finish the timer and record to the associated histogram
    pub fn finish(self) {
        if let Some(name) = self.histogram_name {
            let duration = self.start.elapsed().as_secs_f64();
            // Map histogram names to actual histograms
            match name.as_str() {
                "buy_latency_seconds" | "trade_latency_seconds" => {
                    metrics().trade_latency.observe(duration);
                }
                "rpc_latency_seconds" => {
                    metrics().rpc_latency.observe(duration);
                }
                "build_latency_seconds" => {
                    metrics().build_latency.observe(duration);
                }
                // Task 5: New histogram mappings
                "acquire_lease_ms" => {
                    metrics().acquire_lease_ms.observe(duration * 1000.0);
                }
                "prepare_bundle_ms" => {
                    metrics().prepare_bundle_ms.observe(duration * 1000.0);
                }
                "build_to_land_ms" => {
                    metrics().build_to_land_ms.observe(duration * 1000.0);
                }
                _ => {
                    tracing::debug!("Unknown histogram name: {}", name);
                }
            }
        }
    }
}

/// Task 5: Periodic metrics exporter
///
/// Exports metrics in JSON format at regular intervals (default: 60s)
pub struct MetricsExporter {
    interval: Duration,
}

impl MetricsExporter {
    /// Create a new metrics exporter with the specified interval
    pub fn new(interval: Duration) -> Self {
        Self { interval }
    }

    /// Create a new metrics exporter with default 60s interval
    pub fn default_interval() -> Self {
        Self::new(Duration::from_secs(60))
    }

    /// Export current metrics as JSON string
    pub fn export_json(&self) -> anyhow::Result<String> {
        use prometheus::Encoder;

        let metrics = metrics();
        let encoder = prometheus::TextEncoder::new();
        let metric_families = metrics.registry().gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;

        // Convert to JSON-friendly format
        let text_format = String::from_utf8(buffer)?;

        // Create a simple JSON structure with key metrics
        let json = serde_json::json!({
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            "metrics": {
                "counters": {
                    "trades_total": metrics.trades_total.get(),
                    "trades_success": metrics.trades_success.get(),
                    "trades_failed": metrics.trades_failed.get(),
                    "total_acquires": metrics.total_acquires.get(),
                    "total_releases": metrics.total_releases.get(),
                    "total_refreshes": metrics.total_refreshes.get(),
                    "total_failures": metrics.total_failures.get(),
                    "nonce_leases_dropped_auto": metrics.nonce_leases_dropped_auto.get(),
                    "nonce_leases_dropped_explicit": metrics.nonce_leases_dropped_explicit.get(),
                },
                "gauges": {
                    "active_trades": metrics.active_trades.get(),
                    "nonce_pool_size": metrics.nonce_pool_size.get(),
                    "nonce_active_leases": metrics.nonce_active_leases.get(),
                    "rpc_connections": metrics.rpc_connections.get(),
                },
                "prometheus_format": text_format,
            }
        });

        Ok(serde_json::to_string_pretty(&json)?)
    }

    /// Start periodic export task
    /// Returns a handle that can be used to stop the task
    pub fn start_periodic_export(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.interval);
            loop {
                interval.tick().await;

                match self.export_json() {
                    Ok(json) => {
                        tracing::info!(
                            target: "metrics_export",
                            "Periodic metrics export:\n{}",
                            json
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            target: "metrics_export",
                            "Failed to export metrics: {}",
                            e
                        );
                    }
                }
            }
        })
    }
}
