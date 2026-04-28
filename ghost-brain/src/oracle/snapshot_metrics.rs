//! Snapshot Metrics - Prometheus monitoring for SnapshotEngine
//!
//! This module provides metrics for monitoring ring-buffer state, snapshot operations,
//! and detecting stagnation in the snapshot pipeline.

use prometheus::{IntCounter, IntGauge, Opts, Registry};
use std::sync::Arc;

/// Metrics for SnapshotEngine monitoring
#[derive(Clone)]
pub struct SnapshotMetrics {
    registry: Arc<Registry>,

    /// Current snapshot buffer length (number of snapshots stored)
    pub snapshot_len: IntGauge,

    /// Total number of snapshots pushed to ring buffer
    pub snapshots_pushed_total: IntCounter,

    /// Total number of ring buffer wraps (when buffer is full and overwrites old data)
    pub ring_buffer_wraps_total: IntCounter,

    /// Number of times stagnation was detected (empty buffer for >2000ms)
    pub stagnation_detected_total: IntCounter,

    /// Number of pool initializations
    pub pools_initialized_total: IntCounter,

    /// Number of transaction events processed
    pub tx_events_processed_total: IntCounter,

    /// Number of transaction events dropped for unapproved pools
    pub untracked_tx_dropped_total: IntCounter,

    /// Baseline updates that relied solely on bootstrap data (ignored for anomaly)
    pub baseline_source_bootstrap_total: IntCounter,

    /// Baseline updates using real transaction snapshots
    pub baseline_source_real_total: IntCounter,

    /// Count of bootstrap snapshots created
    pub bootstrap_snapshots_created_total: IntCounter,

    /// Total real samples used across baseline computations
    pub baseline_samples_real_total: IntCounter,

    /// Count of snapshots emitted with valid price
    pub price_valid_total: IntCounter,

    /// Count of snapshots emitted with unknown price
    pub price_unknown_total: IntCounter,

    /// Count of snapshots emitted with invalid price
    pub price_invalid_total: IntCounter,
}

impl SnapshotMetrics {
    /// Create new snapshot metrics collector
    ///
    /// # Arguments
    /// * `registry` - Optional Prometheus registry. If None, creates a new one.
    pub fn new(registry: Option<Arc<Registry>>) -> Self {
        let registry = registry.unwrap_or_else(|| Arc::new(Registry::new()));

        // Create snapshot_len gauge
        let snapshot_len = IntGauge::with_opts(Opts::new(
            "snapshot_engine_buffer_len",
            "Current number of snapshots in the ring buffer",
        ))
        .unwrap();
        registry.register(Box::new(snapshot_len.clone())).unwrap();

        // Create snapshots_pushed counter
        let snapshots_pushed_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_snapshots_pushed_total",
            "Total number of snapshots pushed to ring buffer",
        ))
        .unwrap();
        registry
            .register(Box::new(snapshots_pushed_total.clone()))
            .unwrap();

        // Create ring_buffer_wraps counter
        let ring_buffer_wraps_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_ring_buffer_wraps_total",
            "Total number of ring buffer wraps (overwrites)",
        ))
        .unwrap();
        registry
            .register(Box::new(ring_buffer_wraps_total.clone()))
            .unwrap();

        // Create stagnation_detected counter
        let stagnation_detected_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_stagnation_detected_total",
            "Number of times buffer stagnation was detected (empty >2000ms)",
        ))
        .unwrap();
        registry
            .register(Box::new(stagnation_detected_total.clone()))
            .unwrap();

        // Create pools_initialized counter
        let pools_initialized_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_pools_initialized_total",
            "Total number of pools initialized",
        ))
        .unwrap();
        registry
            .register(Box::new(pools_initialized_total.clone()))
            .unwrap();

        // Create tx_events_processed counter
        let tx_events_processed_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_tx_events_processed_total",
            "Total number of transaction events processed",
        ))
        .unwrap();
        registry
            .register(Box::new(tx_events_processed_total.clone()))
            .unwrap();

        // Create untracked_tx_dropped counter
        let untracked_tx_dropped_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_untracked_tx_dropped_total",
            "Number of transaction events dropped for unapproved pools",
        ))
        .unwrap();
        registry
            .register(Box::new(untracked_tx_dropped_total.clone()))
            .unwrap();

        let baseline_source_bootstrap_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_baseline_source_bootstrap_total",
            "Baseline refresh attempts skipped due to bootstrap-only snapshots",
        ))
        .unwrap();
        registry
            .register(Box::new(baseline_source_bootstrap_total.clone()))
            .unwrap();

        let baseline_source_real_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_baseline_source_real_total",
            "Baseline refresh operations fed by real transaction snapshots",
        ))
        .unwrap();
        registry
            .register(Box::new(baseline_source_real_total.clone()))
            .unwrap();

        let bootstrap_snapshots_created_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_bootstrap_snapshots_created_total",
            "Number of bootstrap snapshots (G0/G1/G2) emitted per pool initialization",
        ))
        .unwrap();
        registry
            .register(Box::new(bootstrap_snapshots_created_total.clone()))
            .unwrap();

        let baseline_samples_real_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_baseline_samples_real_total",
            "Total number of real snapshots used for baseline averaging",
        ))
        .unwrap();
        registry
            .register(Box::new(baseline_samples_real_total.clone()))
            .unwrap();

        let price_valid_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_price_valid_total",
            "Total snapshots emitted with valid price",
        ))
        .unwrap();
        registry
            .register(Box::new(price_valid_total.clone()))
            .unwrap();

        let price_unknown_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_price_unknown_total",
            "Total snapshots emitted with unknown price (no data yet)",
        ))
        .unwrap();
        registry
            .register(Box::new(price_unknown_total.clone()))
            .unwrap();

        let price_invalid_total = IntCounter::with_opts(Opts::new(
            "snapshot_engine_price_invalid_total",
            "Total snapshots emitted with invalid price (critical error)",
        ))
        .unwrap();
        registry
            .register(Box::new(price_invalid_total.clone()))
            .unwrap();

        Self {
            registry,
            snapshot_len,
            snapshots_pushed_total,
            ring_buffer_wraps_total,
            stagnation_detected_total,
            pools_initialized_total,
            tx_events_processed_total,
            untracked_tx_dropped_total,
            baseline_source_bootstrap_total,
            baseline_source_real_total,
            bootstrap_snapshots_created_total,
            baseline_samples_real_total,
            price_valid_total,
            price_unknown_total,
            price_invalid_total,
        }
    }

    /// Get the Prometheus registry for exporting metrics
    pub fn registry(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }

    /// Record a snapshot push operation
    ///
    /// # Arguments
    /// * `new_len` - The new length of the ring buffer after push
    /// * `wrapped` - Whether the push caused a wrap (overwrite of oldest snapshot)
    pub fn record_snapshot_push(&self, new_len: usize, wrapped: bool) {
        self.snapshot_len.set(new_len as i64);
        self.snapshots_pushed_total.inc();
        if wrapped {
            self.ring_buffer_wraps_total.inc();
        }
    }

    /// Record a stagnation detection event
    pub fn record_stagnation_detected(&self) {
        self.stagnation_detected_total.inc();
    }

    /// Record a pool initialization
    pub fn record_pool_initialized(&self) {
        self.pools_initialized_total.inc();
    }

    /// Record a transaction event processed
    pub fn record_tx_event_processed(&self) {
        self.tx_events_processed_total.inc();
    }

    /// Record a dropped transaction from an unapproved pool
    pub fn record_untracked_tx_dropped(&self) {
        self.untracked_tx_dropped_total.inc();
    }

    /// Record a bootstrap snapshot creation (G0/G1/G2)
    pub fn record_bootstrap_snapshots_created(&self, count: usize) {
        if count > 0 {
            self.bootstrap_snapshots_created_total.inc_by(count as u64);
        }
    }

    /// Record that baseline refresh was skipped due to bootstrap-only data
    pub fn record_baseline_source_bootstrap(&self) {
        self.baseline_source_bootstrap_total.inc();
    }

    /// Record that baseline was refreshed from real snapshots
    pub fn record_baseline_source_real(&self, sample_size: usize) {
        self.baseline_source_real_total.inc();
        if sample_size > 0 {
            self.baseline_samples_real_total.inc_by(sample_size as u64);
        }
    }

    /// Update snapshot buffer length directly (for cases where we just need to report current state)
    pub fn set_snapshot_len(&self, len: usize) {
        self.snapshot_len.set(len as i64);
    }

    /// Record price state outcome for emitted snapshot
    pub fn record_price_state(&self, state: ghost_core::shadow_ledger::types::PriceState) {
        use ghost_core::shadow_ledger::types::PriceState;
        match state {
            PriceState::Valid => self.price_valid_total.inc(),
            PriceState::Unknown => self.price_unknown_total.inc(),
            PriceState::Invalid => self.price_invalid_total.inc(),
        }
    }
}

impl Default for SnapshotMetrics {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_metrics_creation() {
        let metrics = SnapshotMetrics::new(None);

        // Verify initial state
        assert_eq!(metrics.snapshot_len.get(), 0);
        assert_eq!(metrics.snapshots_pushed_total.get(), 0);
        assert_eq!(metrics.ring_buffer_wraps_total.get(), 0);
        assert_eq!(metrics.stagnation_detected_total.get(), 0);
    }

    #[test]
    fn test_record_snapshot_push() {
        let metrics = SnapshotMetrics::new(None);

        // Record first push (no wrap)
        metrics.record_snapshot_push(1, false);
        assert_eq!(metrics.snapshot_len.get(), 1);
        assert_eq!(metrics.snapshots_pushed_total.get(), 1);
        assert_eq!(metrics.ring_buffer_wraps_total.get(), 0);

        // Record second push with wrap
        metrics.record_snapshot_push(2, true);
        assert_eq!(metrics.snapshot_len.get(), 2);
        assert_eq!(metrics.snapshots_pushed_total.get(), 2);
        assert_eq!(metrics.ring_buffer_wraps_total.get(), 1);
    }

    #[test]
    fn test_record_stagnation() {
        let metrics = SnapshotMetrics::new(None);

        metrics.record_stagnation_detected();
        assert_eq!(metrics.stagnation_detected_total.get(), 1);

        metrics.record_stagnation_detected();
        assert_eq!(metrics.stagnation_detected_total.get(), 2);
    }

    #[test]
    fn test_record_pool_and_tx_events() {
        let metrics = SnapshotMetrics::new(None);

        metrics.record_pool_initialized();
        assert_eq!(metrics.pools_initialized_total.get(), 1);

        metrics.record_tx_event_processed();
        metrics.record_tx_event_processed();
        assert_eq!(metrics.tx_events_processed_total.get(), 2);
    }
}
