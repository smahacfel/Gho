//! Prometheus metrics for Seer monitoring
//!
//! This module defines and exports Prometheus metrics for monitoring Seer performance
//! and meeting the Land Rate SLA (≥95%).

use prometheus::{
    register_counter_vec, register_gauge, register_gauge_vec, register_histogram_vec,
    register_int_counter_vec, register_int_gauge, register_int_gauge_vec, CounterVec, Gauge,
    GaugeVec, HistogramVec, IntCounterVec, IntGauge, IntGaugeVec,
};
use std::sync::Once;

static INIT: Once = Once::new();

/// Metrics collection for Seer module
#[derive(Clone)]
pub struct SeerMetrics {
    /// Total number of InitializePool events detected from stream
    pub initialize_pool_detected: IntCounterVec,

    /// Number of successfully parsed InitializePool events
    pub initialize_pool_parsed_success: IntCounterVec,

    /// Number of failed parse attempts
    pub initialize_pool_parsed_failed: IntCounterVec,

    /// Number of candidates forwarded to Oracle
    pub candidate_forwarded_to_oracle: IntCounterVec,

    /// Latency from event detection to CandidatePool creation (milliseconds)
    pub processing_latency: HistogramVec,

    /// Total number of events received from Geyser stream
    pub geyser_events_received: IntCounterVec,

    /// Total number of events received, labeled by source and synthetic flag
    pub events_received: IntCounterVec,

    /// Number of WebSocket reconnections
    pub websocket_reconnections: IntCounterVec,

    /// Number of non-buffered trade outcomes, keyed by canonical outcome label.
    ///
    /// Historically this vector only tracked filtered events, but it now also
    /// carries forwarded / expired / IPC-failed outcomes so telemetry can use a
    /// single shared label vocabulary across live, replay and final drops.
    pub events_filtered: CounterVec,

    /// Number of events buffered pending a curve→mint mapping resolution.
    ///
    /// Semantically distinct from `events_filtered`: a buffered trade is NOT
    /// dropped — it will be replayed once `register_curve_mapping` fires.
    pub events_buffered: CounterVec,

    /// Number of pool/create events filtered before candidate forwarding.
    pub pool_events_filtered: CounterVec,

    /// Dev buy parsing anomalies (sanity failures)
    pub dev_buy_anomaly_total: IntCounterVec,

    /// gRPC connection status (1=connected, 0=disconnected)
    pub grpc_connection_status: IntGauge,

    /// Helius adapter land rate (percentage of events successfully published)
    pub helius_land_rate: Gauge,

    /// Helius WebSocket messages received
    pub helius_ws_messages_received: IntCounterVec,

    /// Helius log notifications received
    pub helius_log_notifications_received: IntCounterVec,

    /// Helius raw log notifications (all)
    pub helius_ws_logs_notifications_total: IntCounterVec,

    /// Helius log notifications after prefilter
    pub helius_ws_logs_notifications_prefilter_passed: IntCounterVec,

    /// Helius RPC fetch attempts
    pub helius_rpc_fetch_attempted_total: IntCounterVec,

    /// Helius RPC fetch successes
    pub helius_rpc_fetch_success_total: IntCounterVec,

    /// Helius events published to stream
    pub helius_events_published: IntCounterVec,

    /// Helius events dropped/filtered
    pub helius_events_dropped: IntCounterVec,

    /// Latency from on-chain mint time to detection (milliseconds)
    pub mint_to_detection_latency: HistogramVec,

    /// Counter for detections exceeding latency SLO
    pub late_detection_total: IntCounterVec,

    /// Total binary parser invocations (pool detection + trade parsing)
    pub binary_parser_invocations: IntCounterVec,

    /// Coverage ratio: percentage of parsed trades that result in an emitted pooltx
    pub seer_coverage_ratio: Gauge,

    /// End-to-end coverage ratio from chain denominator to canonical ShadowLedger history.
    pub seer_ledger_coverage_ratio: Gauge,

    /// Current total number of forwarded tx signatures whose source was RPC fallback.
    pub rpc_fallback_forwarded_signatures_total: IntGauge,

    /// Current total number of forwarded TradeEvent objects whose source was RPC fallback.
    pub rpc_fallback_forwarded_events_total: IntGauge,

    /// Share of forwarded tx signatures that came from RPC fallback (%).
    pub rpc_fallback_signature_share_pct: Gauge,

    /// Share of forwarded TradeEvent objects that came from RPC fallback (%).
    pub rpc_fallback_event_share_pct: Gauge,

    /// Stage totals for the gRPC -> parse -> forward pipeline.
    pub pipeline_stage_total: IntGaugeVec,

    /// Stage ratios against the currently known chain denominator.
    pub pipeline_stage_ratio: GaugeVec,

    // ── resolve-path telemetry ──────────────────────────────────────────────
    /// Current number of queued + active curve→mint RPC resolve tasks.
    pub curve_resolve_pending: IntGauge,

    /// Time spent waiting to acquire a semaphore permit before an RPC resolve (ms).
    pub curve_resolve_semaphore_wait_ms: HistogramVec,

    /// End-to-end latency of a single `resolve_curve_mint_via_rpc` call (ms).
    pub curve_resolve_rpc_latency_ms: HistogramVec,

    /// Number of curve→mint resolves that returned no result.
    pub curve_resolve_failure_total: IntCounterVec,

    /// Number of pending trades that expired while sitting in the buffer
    /// waiting for a curve→mint mapping (covers both buffer-enqueue eviction
    /// and replay-drain TTL expiry).
    pub pending_trade_expired_while_buffered_total: IntCounterVec,
}

impl SeerMetrics {
    /// Create a new metrics instance and register all metrics with Prometheus
    pub fn new() -> Self {
        INIT.call_once(|| {
            // This ensures metrics are only registered once
        });

        // Try to register metrics, but don't panic if already registered (for tests)
        let initialize_pool_detected = register_int_counter_vec!(
            "seer_initialize_pool_detected_total",
            "Total number of InitializePool events detected from stream",
            &["amm_program"]
        )
        .unwrap_or_else(|_| {
            // Already registered, retrieve it
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_initialize_pool_detected_total",
                    "Total number of InitializePool events detected from stream",
                ),
                &["amm_program"],
            )
            .unwrap()
        });

        let initialize_pool_parsed_success = register_int_counter_vec!(
            "seer_initialize_pool_parsed_success_total",
            "Number of successfully parsed InitializePool events",
            &["amm_program"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_initialize_pool_parsed_success_total",
                    "Number of successfully parsed InitializePool events",
                ),
                &["amm_program"],
            )
            .unwrap()
        });

        let initialize_pool_parsed_failed = register_int_counter_vec!(
            "seer_initialize_pool_parsed_failed_total",
            "Number of failed InitializePool parse attempts",
            &["amm_program", "reason"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_initialize_pool_parsed_failed_total",
                    "Number of failed InitializePool parse attempts",
                ),
                &["amm_program", "reason"],
            )
            .unwrap()
        });

        let candidate_forwarded_to_oracle = register_int_counter_vec!(
            "seer_candidate_forwarded_to_oracle_total",
            "Number of candidates successfully forwarded to Oracle",
            &["amm_program"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_candidate_forwarded_to_oracle_total",
                    "Number of candidates successfully forwarded to Oracle",
                ),
                &["amm_program"],
            )
            .unwrap()
        });

        let processing_latency = register_histogram_vec!(
            "seer_latency_ms",
            "Latency from event detection to CandidatePool creation (milliseconds)",
            &["amm_program"],
            vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0]
        )
        .unwrap_or_else(|_| {
            prometheus::HistogramVec::new(
                prometheus::HistogramOpts::new(
                    "seer_latency_ms",
                    "Latency from event detection to CandidatePool creation (milliseconds)",
                ),
                &["amm_program"],
            )
            .unwrap()
        });

        let mint_to_detection_latency = register_histogram_vec!(
            "seer_mint_to_detection_ms",
            "Latency from mint/block timestamp to detection (milliseconds)",
            &["amm_program", "source"],
            vec![10.0, 25.0, 50.0, 100.0, 200.0, 300.0, 500.0, 750.0, 1000.0, 2000.0]
        )
        .unwrap_or_else(|_| {
            prometheus::HistogramVec::new(
                prometheus::HistogramOpts::new(
                    "seer_mint_to_detection_ms",
                    "Latency from mint/block timestamp to detection (milliseconds)",
                ),
                &["amm_program", "source"],
            )
            .unwrap()
        });

        let geyser_events_received = register_int_counter_vec!(
            "seer_geyser_events_received_total",
            "Total number of events received from Geyser stream",
            &["event_type"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_geyser_events_received_total",
                    "Total number of events received from Geyser stream",
                ),
                &["event_type"],
            )
            .unwrap()
        });

        let events_received = register_int_counter_vec!(
            "seer_events_received_total",
            "Total number of events received, labeled by source and synthetic flag",
            &["source", "event_type"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_events_received_total",
                    "Total number of events received, labeled by source and synthetic flag",
                ),
                &["source", "event_type"],
            )
            .unwrap()
        });

        let websocket_reconnections = register_int_counter_vec!(
            "seer_websocket_reconnections_total",
            "Number of WebSocket reconnection attempts",
            &["status"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_websocket_reconnections_total",
                    "Number of WebSocket reconnection attempts",
                ),
                &["status"],
            )
            .unwrap()
        });

        let events_filtered = register_counter_vec!(
            "seer_events_filtered_total",
            "Number of non-buffered trade outcomes keyed by canonical outcome label",
            &["outcome"]
        )
        .unwrap_or_else(|_| {
            prometheus::CounterVec::new(
                prometheus::Opts::new(
                    "seer_events_filtered_total",
                    "Number of non-buffered trade outcomes keyed by canonical outcome label",
                ),
                &["outcome"],
            )
            .unwrap()
        });

        let events_buffered = register_counter_vec!(
            "seer_events_buffered_total",
            "Number of trade events buffered keyed by canonical outcome label",
            &["outcome"]
        )
        .unwrap_or_else(|_| {
            prometheus::CounterVec::new(
                prometheus::Opts::new(
                    "seer_events_buffered_total",
                    "Number of trade events buffered keyed by canonical outcome label",
                ),
                &["outcome"],
            )
            .unwrap()
        });

        let pool_events_filtered = register_counter_vec!(
            "seer_pool_events_filtered_total",
            "Number of pool/create events filtered before candidate forwarding",
            &["reason"]
        )
        .unwrap_or_else(|_| {
            prometheus::CounterVec::new(
                prometheus::Opts::new(
                    "seer_pool_events_filtered_total",
                    "Number of pool/create events filtered before candidate forwarding",
                ),
                &["reason"],
            )
            .unwrap()
        });

        let dev_buy_anomaly_total = register_int_counter_vec!(
            "seer_dev_buy_anomaly_total",
            "Sanity check anomalies for dev buy amounts",
            &["reason"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_dev_buy_anomaly_total",
                    "Sanity check anomalies for dev buy amounts",
                ),
                &["reason"],
            )
            .unwrap()
        });

        let late_detection_total = register_int_counter_vec!(
            "seer_late_detection_total",
            "Number of pools detected later than latency SLO",
            &["amm_program", "source"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_late_detection_total",
                    "Number of pools detected later than latency SLO",
                ),
                &["amm_program", "source"],
            )
            .unwrap()
        });

        let grpc_connection_status = register_int_gauge!(
            "seer_grpc_connection_status",
            "gRPC connection status (1=connected, 0=disconnected)"
        )
        .unwrap_or_else(|_| {
            IntGauge::new(
                "seer_grpc_connection_status",
                "gRPC connection status (1=connected, 0=disconnected)",
            )
            .unwrap()
        });

        let helius_land_rate = register_gauge!(
            "seer_helius_land_rate_percent",
            "Helius adapter land rate as percentage (events published / log notifications received)"
        )
        .unwrap_or_else(|_| {
            Gauge::new(
                "seer_helius_land_rate_percent",
                "Helius adapter land rate as percentage (events published / log notifications received)",
            )
            .expect("Failed to create helius_land_rate gauge metric")
        });

        let helius_ws_messages_received = register_int_counter_vec!(
            "seer_helius_ws_messages_received_total",
            "Total WebSocket messages received by Helius adapter",
            &["status"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_ws_messages_received_total",
                    "Total WebSocket messages received by Helius adapter",
                ),
                &["status"],
            )
            .unwrap()
        });

        let helius_log_notifications_received = register_int_counter_vec!(
            "seer_helius_log_notifications_received_total",
            "Total log notifications received by Helius adapter",
            &["status"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_log_notifications_received_total",
                    "Total log notifications received by Helius adapter",
                ),
                &["status"],
            )
            .unwrap()
        });

        let helius_events_published = register_int_counter_vec!(
            "seer_helius_events_published_total",
            "Total events successfully published to stream by Helius adapter",
            &["status"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_events_published_total",
                    "Total events successfully published to stream by Helius adapter",
                ),
                &["status"],
            )
            .unwrap()
        });

        let helius_events_dropped = register_int_counter_vec!(
            "seer_helius_events_dropped_total",
            "Total events dropped/filtered by Helius adapter",
            &["reason"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_events_dropped_total",
                    "Total events dropped/filtered by Helius adapter",
                ),
                &["reason"],
            )
            .unwrap()
        });

        let helius_ws_logs_notifications_total = register_int_counter_vec!(
            "seer_helius_ws_logs_notifications_total",
            "Raw WebSocket log notifications received",
            &["stage"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_ws_logs_notifications_total",
                    "Raw WebSocket log notifications received",
                ),
                &["stage"],
            )
            .unwrap()
        });

        let helius_ws_logs_notifications_prefilter_passed = register_int_counter_vec!(
            "seer_helius_ws_logs_notifications_prefilter_passed_total",
            "WebSocket log notifications that passed prefilter",
            &["stage"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_ws_logs_notifications_prefilter_passed_total",
                    "WebSocket log notifications that passed prefilter",
                ),
                &["stage"],
            )
            .unwrap()
        });

        let helius_rpc_fetch_attempted_total = register_int_counter_vec!(
            "seer_helius_rpc_fetch_attempted_total",
            "Number of RPC fetch attempts",
            &["status"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_rpc_fetch_attempted_total",
                    "Number of RPC fetch attempts",
                ),
                &["status"],
            )
            .unwrap()
        });

        let helius_rpc_fetch_success_total = register_int_counter_vec!(
            "seer_helius_rpc_fetch_success_total",
            "Number of successful RPC fetches",
            &["status"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_helius_rpc_fetch_success_total",
                    "Number of successful RPC fetches",
                ),
                &["status"],
            )
            .unwrap()
        });

        let binary_parser_invocations = register_int_counter_vec!(
            "seer_binary_parser_invocations_total",
            "Total binary parser invocations (pool detection + trade parsing)",
            &["parse_type"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_binary_parser_invocations_total",
                    "Total binary parser invocations (pool detection + trade parsing)",
                ),
                &["parse_type"],
            )
            .unwrap()
        });

        let seer_coverage_ratio = register_gauge!(
            "seer_coverage_ratio_percent",
            "Percentage of trade-candidate transactions that the parser decodes into at least one trade"
        )
        .unwrap_or_else(|_| {
            Gauge::new(
                "seer_coverage_ratio_percent",
                "Percentage of trade-candidate transactions that the parser decodes into at least one trade",
            )
            .expect("Failed to create seer_coverage_ratio gauge metric")
        });

        let seer_ledger_coverage_ratio = register_gauge!(
            "seer_ledger_coverage_ratio_percent",
            "Percentage of tracked chain-truth transactions represented in canonical ShadowLedger history"
        )
        .unwrap_or_else(|_| {
            Gauge::new(
                "seer_ledger_coverage_ratio_percent",
                "Percentage of tracked chain-truth transactions represented in canonical ShadowLedger history",
            )
            .expect("Failed to create seer_ledger_coverage_ratio gauge metric")
        });

        let rpc_fallback_forwarded_signatures_total = register_int_gauge!(
            "seer_rpc_fallback_forwarded_signatures_total",
            "Current total number of forwarded tx signatures whose source was grpc_backfill"
        )
        .unwrap_or_else(|_| {
            IntGauge::new(
                "seer_rpc_fallback_forwarded_signatures_total",
                "Current total number of forwarded tx signatures whose source was grpc_backfill",
            )
            .unwrap()
        });

        let rpc_fallback_forwarded_events_total = register_int_gauge!(
            "seer_rpc_fallback_forwarded_events_total",
            "Current total number of forwarded TradeEvent objects whose source was grpc_backfill"
        )
        .unwrap_or_else(|_| {
            IntGauge::new(
                "seer_rpc_fallback_forwarded_events_total",
                "Current total number of forwarded TradeEvent objects whose source was grpc_backfill",
            )
            .unwrap()
        });

        let rpc_fallback_signature_share_pct = register_gauge!(
            "seer_rpc_fallback_signature_share_percent",
            "Share of forwarded tx signatures that came from grpc_backfill"
        )
        .unwrap_or_else(|_| {
            Gauge::new(
                "seer_rpc_fallback_signature_share_percent",
                "Share of forwarded tx signatures that came from grpc_backfill",
            )
            .expect("Failed to create rpc_fallback_signature_share_pct gauge")
        });

        let rpc_fallback_event_share_pct = register_gauge!(
            "seer_rpc_fallback_event_share_percent",
            "Share of forwarded TradeEvent objects that came from grpc_backfill"
        )
        .unwrap_or_else(|_| {
            Gauge::new(
                "seer_rpc_fallback_event_share_percent",
                "Share of forwarded TradeEvent objects that came from grpc_backfill",
            )
            .expect("Failed to create rpc_fallback_event_share_pct gauge")
        });

        let pipeline_stage_total = register_int_gauge_vec!(
            "seer_pipeline_stage_total",
            "Stage totals for the Seer gRPC coverage pipeline",
            &["stage"]
        )
        .unwrap_or_else(|_| {
            IntGaugeVec::new(
                prometheus::Opts::new(
                    "seer_pipeline_stage_total",
                    "Stage totals for the Seer gRPC coverage pipeline",
                ),
                &["stage"],
            )
            .unwrap()
        });

        let pipeline_stage_ratio = register_gauge_vec!(
            "seer_pipeline_stage_ratio_percent",
            "Stage ratios against the current chain denominator",
            &["stage"]
        )
        .unwrap_or_else(|_| {
            GaugeVec::new(
                prometheus::Opts::new(
                    "seer_pipeline_stage_ratio_percent",
                    "Stage ratios against the current chain denominator",
                ),
                &["stage"],
            )
            .unwrap()
        });

        // ── resolve-path telemetry ──────────────────────────────────────────

        let curve_resolve_pending = register_int_gauge!(
            "seer_curve_resolve_pending",
            "Current number of queued + active curve→mint RPC resolve tasks"
        )
        .unwrap_or_else(|_| {
            IntGauge::new(
                "seer_curve_resolve_pending",
                "Current number of queued + active curve→mint RPC resolve tasks",
            )
            .unwrap()
        });

        let curve_resolve_semaphore_wait_ms = register_histogram_vec!(
            "seer_curve_resolve_semaphore_wait_ms",
            "Time spent waiting to acquire a semaphore permit before an RPC resolve (ms)",
            &[],
            vec![0.1, 0.5, 1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0]
        )
        .unwrap_or_else(|_| {
            prometheus::HistogramVec::new(
                prometheus::HistogramOpts::new(
                    "seer_curve_resolve_semaphore_wait_ms",
                    "Time spent waiting to acquire a semaphore permit before an RPC resolve (ms)",
                ),
                &[],
            )
            .unwrap()
        });

        let curve_resolve_rpc_latency_ms = register_histogram_vec!(
            "seer_curve_resolve_rpc_latency_ms",
            "End-to-end latency of a single resolve_curve_mint_via_rpc call (ms)",
            &[],
            vec![10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0]
        )
        .unwrap_or_else(|_| {
            prometheus::HistogramVec::new(
                prometheus::HistogramOpts::new(
                    "seer_curve_resolve_rpc_latency_ms",
                    "End-to-end latency of a single resolve_curve_mint_via_rpc call (ms)",
                ),
                &[],
            )
            .unwrap()
        });

        let curve_resolve_failure_total = register_int_counter_vec!(
            "seer_curve_resolve_failure_total",
            "Number of curve→mint resolves that returned no result",
            &[]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_curve_resolve_failure_total",
                    "Number of curve→mint resolves that returned no result",
                ),
                &[],
            )
            .unwrap()
        });

        let pending_trade_expired_while_buffered_total = register_int_counter_vec!(
            "seer_pending_trade_expired_while_buffered_total",
            "Number of pending trades that expired while buffered waiting for a curve→mint mapping",
            &["reason"]
        )
        .unwrap_or_else(|_| {
            prometheus::IntCounterVec::new(
                prometheus::Opts::new(
                    "seer_pending_trade_expired_while_buffered_total",
                    "Number of pending trades that expired while buffered waiting for a curve→mint mapping",
                ),
                &["reason"],
            )
            .unwrap()
        });

        Self {
            initialize_pool_detected,
            initialize_pool_parsed_success,
            initialize_pool_parsed_failed,
            candidate_forwarded_to_oracle,
            processing_latency,
            geyser_events_received,
            events_received,
            websocket_reconnections,
            events_filtered,
            events_buffered,
            pool_events_filtered,
            dev_buy_anomaly_total,
            grpc_connection_status,
            helius_land_rate,
            helius_ws_messages_received,
            helius_log_notifications_received,
            helius_ws_logs_notifications_total,
            helius_ws_logs_notifications_prefilter_passed,
            helius_rpc_fetch_attempted_total,
            helius_rpc_fetch_success_total,
            helius_events_published,
            helius_events_dropped,
            mint_to_detection_latency,
            late_detection_total,
            binary_parser_invocations,
            seer_coverage_ratio,
            seer_ledger_coverage_ratio,
            rpc_fallback_forwarded_signatures_total,
            rpc_fallback_forwarded_events_total,
            rpc_fallback_signature_share_pct,
            rpc_fallback_event_share_pct,
            pipeline_stage_total,
            pipeline_stage_ratio,
            curve_resolve_pending,
            curve_resolve_semaphore_wait_ms,
            curve_resolve_rpc_latency_ms,
            curve_resolve_failure_total,
            pending_trade_expired_while_buffered_total,
        }
    }

    /// Calculate and return the current Land Rate (parse success rate)
    ///
    /// Land Rate = (parsed_success / detected) * 100
    /// Target: ≥ 95%
    pub fn calculate_land_rate(&self, amm_program: &str) -> f64 {
        let detected = self
            .initialize_pool_detected
            .with_label_values(&[amm_program])
            .get() as f64;

        if detected == 0.0 {
            return 100.0; // No events yet
        }

        let parsed = self
            .initialize_pool_parsed_success
            .with_label_values(&[amm_program])
            .get() as f64;

        (parsed / detected) * 100.0
    }
}

impl Default for SeerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = SeerMetrics::new();
        assert_eq!(metrics.calculate_land_rate("pumpfun"), 100.0);
    }

    #[test]
    fn test_land_rate_calculation() {
        let metrics = SeerMetrics::new();

        // Simulate 100 detected events
        for _ in 0..100 {
            metrics
                .initialize_pool_detected
                .with_label_values(&["pumpfun"])
                .inc();
        }

        // Simulate 96 successfully parsed (96% land rate)
        for _ in 0..96 {
            metrics
                .initialize_pool_parsed_success
                .with_label_values(&["pumpfun"])
                .inc();
        }

        let land_rate = metrics.calculate_land_rate("pumpfun");
        assert_eq!(land_rate, 96.0);
        assert!(land_rate >= 95.0); // Meets SLA
    }
}
