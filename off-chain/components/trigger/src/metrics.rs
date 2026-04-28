//! Metrics for Trigger module
//!
//! Prometheus metrics for monitoring transaction sending performance,
//! inclusion rate, and latency.

use prometheus::{
    Counter, Gauge, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Registry,
};

/// Metrics collector for Trigger module
#[derive(Clone)]
pub struct TriggerMetrics {
    /// Total transactions sent
    pub transactions_sent: IntCounter,

    /// Total transactions confirmed
    pub transactions_confirmed: IntCounter,

    /// Total transactions failed
    pub transactions_failed: IntCounter,

    /// Inclusion rate (percentage)
    pub inclusion_rate: Gauge,

    /// Transaction send latency (milliseconds)
    pub send_latency: Histogram,

    /// Transaction confirmation latency (milliseconds)
    pub confirmation_latency: Histogram,

    /// Currently pending transactions
    pub pending_transactions: IntGauge,

    /// Total bytes sent
    pub bytes_sent: Counter,

    /// Redundancy sends (N+3 counter)
    pub redundancy_sends: IntCounter,

    /// Jito bundle submissions
    pub jito_bundles_submitted: IntCounter,

    /// Jito bundle successes
    pub jito_bundles_successful: IntCounter,

    /// Jito bundle rejection reasons
    pub jito_bundle_rejection_reason: IntCounterVec,

    /// Total bullets fired successfully
    pub bullet_fired_total: IntCounter,

    /// Total bullets failed due to not being ready
    pub bullet_failed_not_ready_total: IntCounter,

    /// Leader resolution latency (milliseconds)
    pub leader_resolution_latency: Histogram,

    /// Leader schedule cache refresh count
    pub leader_schedule_refreshes: IntCounter,

    /// Cluster nodes cache refresh count
    pub cluster_nodes_refreshes: IntCounter,

    /// Localhost addresses filtered count
    pub localhost_addresses_filtered: IntCounter,

    /// Leapfrog sends (transactions sent via leapfrog strategy)
    pub leapfrog_sends_total: IntCounter,
}

impl TriggerMetrics {
    /// Create a new metrics instance
    pub fn new() -> Self {
        Self {
            transactions_sent: IntCounter::new(
                "trigger_transactions_sent_total",
                "Total number of transactions sent",
            )
            .expect("metric creation"),

            transactions_confirmed: IntCounter::new(
                "trigger_transactions_confirmed_total",
                "Total number of transactions confirmed",
            )
            .expect("metric creation"),

            transactions_failed: IntCounter::new(
                "trigger_transactions_failed_total",
                "Total number of transactions failed",
            )
            .expect("metric creation"),

            inclusion_rate: Gauge::new(
                "trigger_inclusion_rate",
                "Transaction inclusion rate (0.0 - 1.0)",
            )
            .expect("metric creation"),

            send_latency: Histogram::with_opts(
                HistogramOpts::new(
                    "trigger_send_latency_ms",
                    "Transaction send latency in milliseconds",
                )
                .buckets(vec![
                    1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0,
                ]),
            )
            .expect("metric creation"),

            confirmation_latency: Histogram::with_opts(
                HistogramOpts::new(
                    "trigger_confirmation_latency_ms",
                    "Transaction confirmation latency in milliseconds",
                )
                .buckets(vec![100.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0]),
            )
            .expect("metric creation"),

            pending_transactions: IntGauge::new(
                "trigger_pending_transactions",
                "Number of currently pending transactions",
            )
            .expect("metric creation"),

            bytes_sent: Counter::new("trigger_bytes_sent_total", "Total bytes sent via TPU")
                .expect("metric creation"),

            redundancy_sends: IntCounter::new(
                "trigger_redundancy_sends_total",
                "Total number of redundant sends (N+3)",
            )
            .expect("metric creation"),

            jito_bundles_submitted: IntCounter::new(
                "trigger_jito_bundles_submitted_total",
                "Total Jito bundles submitted",
            )
            .expect("metric creation"),

            jito_bundles_successful: IntCounter::new(
                "trigger_jito_bundles_successful_total",
                "Total successful Jito bundles",
            )
            .expect("metric creation"),

            jito_bundle_rejection_reason: IntCounterVec::new(
                prometheus::Opts::new(
                    "trigger_jito_bundle_rejection_reason",
                    "Jito bundle rejection reasons",
                ),
                &["reason"],
            )
            .expect("metric creation"),

            bullet_fired_total: IntCounter::new(
                "trigger_bullet_fired_total",
                "Total number of revolver bullets fired successfully",
            )
            .expect("metric creation"),

            bullet_failed_not_ready_total: IntCounter::new(
                "trigger_bullet_failed_not_ready_total",
                "Total number of bullets that failed to fire due to not being ready",
            )
            .expect("metric creation"),

            leader_resolution_latency: Histogram::with_opts(
                HistogramOpts::new(
                    "trigger_leader_resolution_latency_ms",
                    "Leader resolution latency in milliseconds",
                )
                .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0]),
            )
            .expect("metric creation"),

            leader_schedule_refreshes: IntCounter::new(
                "trigger_leader_schedule_refreshes_total",
                "Total number of leader schedule cache refreshes",
            )
            .expect("metric creation"),

            cluster_nodes_refreshes: IntCounter::new(
                "trigger_cluster_nodes_refreshes_total",
                "Total number of cluster nodes cache refreshes",
            )
            .expect("metric creation"),

            localhost_addresses_filtered: IntCounter::new(
                "trigger_localhost_addresses_filtered_total",
                "Total number of localhost addresses filtered out",
            )
            .expect("metric creation"),

            leapfrog_sends_total: IntCounter::new(
                "trigger_leapfrog_sends_total",
                "Total number of transactions sent via leapfrog strategy",
            )
            .expect("metric creation"),
        }
    }

    /// Register metrics with a Prometheus registry
    pub fn register(&self, registry: &Registry) -> Result<(), prometheus::Error> {
        registry.register(Box::new(self.transactions_sent.clone()))?;
        registry.register(Box::new(self.transactions_confirmed.clone()))?;
        registry.register(Box::new(self.transactions_failed.clone()))?;
        registry.register(Box::new(self.inclusion_rate.clone()))?;
        registry.register(Box::new(self.send_latency.clone()))?;
        registry.register(Box::new(self.confirmation_latency.clone()))?;
        registry.register(Box::new(self.pending_transactions.clone()))?;
        registry.register(Box::new(self.bytes_sent.clone()))?;
        registry.register(Box::new(self.redundancy_sends.clone()))?;
        registry.register(Box::new(self.jito_bundles_submitted.clone()))?;
        registry.register(Box::new(self.jito_bundles_successful.clone()))?;
        registry.register(Box::new(self.jito_bundle_rejection_reason.clone()))?;
        registry.register(Box::new(self.bullet_fired_total.clone()))?;
        registry.register(Box::new(self.bullet_failed_not_ready_total.clone()))?;
        registry.register(Box::new(self.leader_resolution_latency.clone()))?;
        registry.register(Box::new(self.leader_schedule_refreshes.clone()))?;
        registry.register(Box::new(self.cluster_nodes_refreshes.clone()))?;
        registry.register(Box::new(self.localhost_addresses_filtered.clone()))?;
        registry.register(Box::new(self.leapfrog_sends_total.clone()))?;
        Ok(())
    }

    /// Record a transaction send
    pub fn record_send(&self, bytes: usize, redundancy_count: usize) {
        self.transactions_sent.inc();
        self.bytes_sent.inc_by(bytes as f64);
        self.redundancy_sends.inc_by(redundancy_count as u64);
        self.pending_transactions.inc();
    }

    /// Record a transaction confirmation
    pub fn record_confirmation(&self, send_latency_ms: f64, confirmation_latency_ms: f64) {
        self.transactions_confirmed.inc();
        self.pending_transactions.dec();
        self.send_latency.observe(send_latency_ms);
        self.confirmation_latency.observe(confirmation_latency_ms);
        self.update_inclusion_rate();
    }

    /// Record a transaction failure
    pub fn record_failure(&self) {
        self.transactions_failed.inc();
        self.pending_transactions.dec();
        self.update_inclusion_rate();
    }

    /// Update the inclusion rate metric
    fn update_inclusion_rate(&self) {
        let sent = self.transactions_sent.get() as f64;
        let confirmed = self.transactions_confirmed.get() as f64;

        if sent > 0.0 {
            let rate = confirmed / sent;
            self.inclusion_rate.set(rate);
        }
    }

    /// Record a Jito bundle submission
    pub fn record_jito_bundle(&self, success: bool) {
        self.jito_bundles_submitted.inc();
        if success {
            self.jito_bundles_successful.inc();
        }
    }

    /// Record a Jito bundle rejection with reason
    pub fn record_jito_bundle_rejection(&self, reason: &str) {
        self.jito_bundles_submitted.inc();
        self.jito_bundle_rejection_reason
            .with_label_values(&[reason])
            .inc();
    }

    /// Record a leader resolution operation
    pub fn record_leader_resolution(&self, latency_ms: f64) {
        self.leader_resolution_latency.observe(latency_ms);
    }

    /// Record a leader schedule refresh
    pub fn record_leader_schedule_refresh(&self) {
        self.leader_schedule_refreshes.inc();
    }

    /// Record a cluster nodes refresh
    pub fn record_cluster_nodes_refresh(&self) {
        self.cluster_nodes_refreshes.inc();
    }

    /// Record localhost addresses filtered
    pub fn record_localhost_filtered(&self, count: u64) {
        self.localhost_addresses_filtered.inc_by(count);
    }

    /// Record a leapfrog send
    pub fn record_leapfrog_send(&self) {
        self.leapfrog_sends_total.inc();
    }

    /// Get current inclusion rate
    pub fn get_inclusion_rate(&self) -> f64 {
        self.inclusion_rate.get()
    }

    /// Get summary statistics
    pub fn get_summary(&self) -> MetricsSummary {
        MetricsSummary {
            transactions_sent: self.transactions_sent.get(),
            transactions_confirmed: self.transactions_confirmed.get(),
            transactions_failed: self.transactions_failed.get(),
            inclusion_rate: self.inclusion_rate.get(),
            pending_transactions: self.pending_transactions.get(),
        }
    }
}

impl Default for TriggerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of metrics for reporting
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub transactions_sent: u64,
    pub transactions_confirmed: u64,
    pub transactions_failed: u64,
    pub inclusion_rate: f64,
    pub pending_transactions: i64,
}

impl std::fmt::Display for MetricsSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Sent: {}, Confirmed: {}, Failed: {}, Inclusion Rate: {:.2}%, Pending: {}",
            self.transactions_sent,
            self.transactions_confirmed,
            self.transactions_failed,
            self.inclusion_rate * 100.0,
            self.pending_transactions
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = TriggerMetrics::new();
        assert_eq!(metrics.transactions_sent.get(), 0);
        assert_eq!(metrics.transactions_confirmed.get(), 0);
    }

    #[test]
    fn test_record_send() {
        let metrics = TriggerMetrics::new();
        metrics.record_send(200, 3);

        assert_eq!(metrics.transactions_sent.get(), 1);
        assert_eq!(metrics.bytes_sent.get(), 200.0);
        assert_eq!(metrics.redundancy_sends.get(), 3);
        assert_eq!(metrics.pending_transactions.get(), 1);
    }

    #[test]
    fn test_record_confirmation() {
        let metrics = TriggerMetrics::new();

        // Send then confirm
        metrics.record_send(200, 3);
        metrics.record_confirmation(10.0, 500.0);

        assert_eq!(metrics.transactions_confirmed.get(), 1);
        assert_eq!(metrics.pending_transactions.get(), 0);
    }

    #[test]
    fn test_inclusion_rate_calculation() {
        let metrics = TriggerMetrics::new();

        // Send 10 transactions
        for _ in 0..10 {
            metrics.record_send(200, 3);
        }

        // Confirm 9 of them
        for _ in 0..9 {
            metrics.record_confirmation(10.0, 500.0);
        }

        // Fail 1
        metrics.record_failure();

        let rate = metrics.get_inclusion_rate();
        assert!((rate - 0.9).abs() < 0.01); // Should be 90%
    }

    #[test]
    fn test_summary() {
        let metrics = TriggerMetrics::new();
        metrics.record_send(200, 3);
        metrics.record_confirmation(10.0, 500.0);

        let summary = metrics.get_summary();
        assert_eq!(summary.transactions_sent, 1);
        assert_eq!(summary.transactions_confirmed, 1);
        assert_eq!(summary.pending_transactions, 0);
    }

    #[test]
    fn test_jito_metrics() {
        let metrics = TriggerMetrics::new();

        metrics.record_jito_bundle(true);
        metrics.record_jito_bundle(false);

        assert_eq!(metrics.jito_bundles_submitted.get(), 2);
        assert_eq!(metrics.jito_bundles_successful.get(), 1);
    }
}
