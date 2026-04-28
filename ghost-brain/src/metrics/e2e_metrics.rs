//! End-to-end pipeline metrics
//!
//! Tracks Land Rate, Inclusion Rate, and latency metrics for the entire pipeline.

use prometheus::{
    Counter, CounterVec, Gauge, Histogram, HistogramOpts, HistogramVec, Opts, Registry,
};
use std::sync::Arc;

/// Histogram buckets for QASS latency in nanoseconds
/// Tuned for expected QASS performance: 100ns to 25μs
/// Based on target <1μs for superposition_score() hot path
const QASS_LATENCY_BUCKETS_NS: [f64; 8] = [
    100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 25000.0,
];

/// E2E Pipeline metrics
#[derive(Clone)]
pub struct E2EMetrics {
    /// Registry for Prometheus metrics
    pub registry: Arc<Registry>,

    // Seer metrics (Land Rate)
    /// Total InitializePool events detected by Seer
    pub seer_pools_detected: CounterVec,

    /// Total InitializePool events successfully parsed
    pub seer_pools_parsed: CounterVec,

    /// Seer processing latency (ms)
    pub seer_latency: HistogramVec,

    // Oracle metrics
    /// Total candidates scored by Oracle
    pub oracle_candidates_scored: Counter,

    /// Oracle scoring latency (ms)
    pub oracle_latency: Histogram,

    /// Oracle score distribution
    pub oracle_score_histogram: Histogram,

    // Features metrics
    /// Total SwapPlans created
    pub features_plans_created: Counter,

    /// SwapPlans rejected (below threshold)
    pub features_plans_rejected: Counter,

    // Direct Buy metrics
    /// Total buy intents initialized
    pub buy_intents_initialized: Counter,

    /// Buy initialization failures
    pub buy_init_failures: Counter,

    // Trigger metrics (Inclusion Rate)
    /// Total transactions sent
    pub trigger_txs_sent: Counter,

    /// Total transactions confirmed
    pub trigger_txs_confirmed: Counter,

    /// Total transactions failed
    pub trigger_txs_failed: Counter,

    /// Trigger send latency (ms)
    pub trigger_send_latency: Histogram,

    /// Trigger confirmation latency (ms)
    pub trigger_confirm_latency: Histogram,

    // Jito-specific metrics
    /// Total Jito bundles submitted
    pub jito_bundles_submitted: Counter,

    /// Total Jito bundles confirmed
    pub jito_bundles_confirmed: Counter,

    /// Total tips paid to Jito (in lamports)
    pub jito_total_tips_paid: Counter,

    /// Jito bundle submission latency (ms)
    pub jito_bundle_latency: Histogram,

    // End-to-end metrics
    /// Total end-to-end latency (detection to confirmation)
    pub e2e_total_latency: Histogram,

    /// Current Land Rate gauge (%)
    pub land_rate: Gauge,

    /// Current Inclusion Rate gauge (%)
    pub inclusion_rate: Gauge,

    /// SLA violations counter
    pub sla_violations: CounterVec,

    // Leapfrog-specific metrics
    /// Total leapfrog packets sent
    pub leapfrog_packets_sent: Counter,

    // QASS-specific metrics
    /// QASS scoring latency (nanoseconds)
    pub qass_latency_ns: Histogram,
    /// QASS score distribution
    pub qass_score_histogram: Histogram,
    /// QASS confidence distribution
    pub qass_confidence_histogram: Histogram,
    /// QASS analysis count by validity (valid/invalid)
    pub qass_analyses_total: CounterVec,
    /// QASS dominant wave distribution
    pub qass_dominant_waves: CounterVec,
}

impl E2EMetrics {
    /// Create a new E2E metrics instance
    pub fn new() -> Self {
        let registry = Arc::new(Registry::new());

        let seer_pools_detected = CounterVec::new(
            Opts::new(
                "ghost_seer_pools_detected_total",
                "Total InitializePool events detected by Seer",
            ),
            &["amm_program"],
        )
        .unwrap();
        registry
            .register(Box::new(seer_pools_detected.clone()))
            .unwrap();

        let seer_pools_parsed = CounterVec::new(
            Opts::new(
                "ghost_seer_pools_parsed_total",
                "Total InitializePool events successfully parsed",
            ),
            &["amm_program"],
        )
        .unwrap();
        registry
            .register(Box::new(seer_pools_parsed.clone()))
            .unwrap();

        let seer_latency = HistogramVec::new(
            HistogramOpts::new(
                "ghost_seer_latency_ms",
                "Seer processing latency in milliseconds",
            ),
            &["amm_program"],
        )
        .unwrap();
        registry.register(Box::new(seer_latency.clone())).unwrap();

        let oracle_candidates_scored = Counter::new(
            "ghost_oracle_candidates_scored_total",
            "Total candidates scored by Oracle",
        )
        .unwrap();
        registry
            .register(Box::new(oracle_candidates_scored.clone()))
            .unwrap();

        let oracle_latency = Histogram::with_opts(HistogramOpts::new(
            "ghost_oracle_latency_ms",
            "Oracle scoring latency in milliseconds",
        ))
        .unwrap();
        registry.register(Box::new(oracle_latency.clone())).unwrap();

        let oracle_score_histogram = Histogram::with_opts(
            HistogramOpts::new("ghost_oracle_score", "Distribution of Oracle scores").buckets(
                vec![
                    0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0,
                ],
            ),
        )
        .unwrap();
        registry
            .register(Box::new(oracle_score_histogram.clone()))
            .unwrap();

        let features_plans_created = Counter::new(
            "ghost_features_plans_created_total",
            "Total SwapPlans created by Features",
        )
        .unwrap();
        registry
            .register(Box::new(features_plans_created.clone()))
            .unwrap();

        let features_plans_rejected = Counter::new(
            "ghost_features_plans_rejected_total",
            "SwapPlans rejected (below threshold)",
        )
        .unwrap();
        registry
            .register(Box::new(features_plans_rejected.clone()))
            .unwrap();

        let buy_intents_initialized = Counter::new(
            "ghost_buy_intents_initialized_total",
            "Total buy intents initialized",
        )
        .unwrap();
        registry
            .register(Box::new(buy_intents_initialized.clone()))
            .unwrap();

        let buy_init_failures = Counter::new(
            "ghost_buy_init_failures_total",
            "Buy initialization failures",
        )
        .unwrap();
        registry
            .register(Box::new(buy_init_failures.clone()))
            .unwrap();

        let trigger_txs_sent = Counter::new(
            "ghost_trigger_txs_sent_total",
            "Total transactions sent by Trigger",
        )
        .unwrap();
        registry
            .register(Box::new(trigger_txs_sent.clone()))
            .unwrap();

        let trigger_txs_confirmed = Counter::new(
            "ghost_trigger_txs_confirmed_total",
            "Total transactions confirmed",
        )
        .unwrap();
        registry
            .register(Box::new(trigger_txs_confirmed.clone()))
            .unwrap();

        let trigger_txs_failed = Counter::new(
            "ghost_trigger_txs_failed_total",
            "Total transactions failed",
        )
        .unwrap();
        registry
            .register(Box::new(trigger_txs_failed.clone()))
            .unwrap();

        let trigger_send_latency = Histogram::with_opts(HistogramOpts::new(
            "ghost_trigger_send_latency_ms",
            "Trigger send latency in milliseconds",
        ))
        .unwrap();
        registry
            .register(Box::new(trigger_send_latency.clone()))
            .unwrap();

        let trigger_confirm_latency = Histogram::with_opts(HistogramOpts::new(
            "ghost_trigger_confirm_latency_ms",
            "Trigger confirmation latency in milliseconds",
        ))
        .unwrap();
        registry
            .register(Box::new(trigger_confirm_latency.clone()))
            .unwrap();

        // Jito-specific metrics
        let jito_bundles_submitted = Counter::new(
            "ghost_jito_bundles_submitted_total",
            "Total Jito bundles submitted",
        )
        .unwrap();
        registry
            .register(Box::new(jito_bundles_submitted.clone()))
            .unwrap();

        let jito_bundles_confirmed = Counter::new(
            "ghost_jito_bundles_confirmed_total",
            "Total Jito bundles confirmed",
        )
        .unwrap();
        registry
            .register(Box::new(jito_bundles_confirmed.clone()))
            .unwrap();

        let jito_total_tips_paid = Counter::new(
            "ghost_jito_total_tips_paid_lamports",
            "Total tips paid to Jito in lamports",
        )
        .unwrap();
        registry
            .register(Box::new(jito_total_tips_paid.clone()))
            .unwrap();

        let jito_bundle_latency = Histogram::with_opts(HistogramOpts::new(
            "ghost_jito_bundle_latency_ms",
            "Jito bundle submission latency in milliseconds",
        ))
        .unwrap();
        registry
            .register(Box::new(jito_bundle_latency.clone()))
            .unwrap();

        let e2e_total_latency = Histogram::with_opts(HistogramOpts::new(
            "ghost_e2e_total_latency_ms",
            "Total end-to-end latency in milliseconds",
        ))
        .unwrap();
        registry
            .register(Box::new(e2e_total_latency.clone()))
            .unwrap();

        let land_rate =
            Gauge::new("ghost_land_rate_percent", "Current Land Rate percentage").unwrap();
        registry.register(Box::new(land_rate.clone())).unwrap();

        let inclusion_rate = Gauge::new(
            "ghost_inclusion_rate_percent",
            "Current Inclusion Rate percentage",
        )
        .unwrap();
        registry.register(Box::new(inclusion_rate.clone())).unwrap();

        let sla_violations = CounterVec::new(
            Opts::new("ghost_sla_violations_total", "SLA violations by type"),
            &["violation_type"],
        )
        .unwrap();
        registry.register(Box::new(sla_violations.clone())).unwrap();

        let leapfrog_packets_sent = Counter::new(
            "ghost_leapfrog_packets_sent_total",
            "Total leapfrog packets sent to TPU leaders",
        )
        .unwrap();
        registry
            .register(Box::new(leapfrog_packets_sent.clone()))
            .unwrap();

        // QASS-specific metrics
        let qass_latency_ns = Histogram::with_opts(
            HistogramOpts::new(
                "ghost_qass_latency_ns",
                "QASS scoring latency in nanoseconds",
            )
            // Buckets tuned for expected QASS performance: 100ns to 25μs
            // Based on target <1μs for superposition_score() hot path
            .buckets(QASS_LATENCY_BUCKETS_NS.to_vec()),
        )
        .unwrap();
        registry
            .register(Box::new(qass_latency_ns.clone()))
            .unwrap();

        let qass_score_histogram = Histogram::with_opts(
            HistogramOpts::new("ghost_qass_score", "Distribution of QASS scores (0-100)").buckets(
                vec![
                    0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0,
                ],
            ),
        )
        .unwrap();
        registry
            .register(Box::new(qass_score_histogram.clone()))
            .unwrap();

        let qass_confidence_histogram = Histogram::with_opts(
            HistogramOpts::new(
                "ghost_qass_confidence",
                "Distribution of QASS confidence (0.0-1.0)",
            )
            .buckets(vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]),
        )
        .unwrap();
        registry
            .register(Box::new(qass_confidence_histogram.clone()))
            .unwrap();

        let qass_analyses_total = CounterVec::new(
            Opts::new(
                "ghost_qass_analyses_total",
                "Total QASS analyses by validity status",
            ),
            &["validity"],
        )
        .unwrap();
        registry
            .register(Box::new(qass_analyses_total.clone()))
            .unwrap();

        let qass_dominant_waves = CounterVec::new(
            Opts::new(
                "ghost_qass_dominant_waves_total",
                "Count of times each wave was dominant in QASS analysis",
            ),
            &["wave_name"],
        )
        .unwrap();
        registry
            .register(Box::new(qass_dominant_waves.clone()))
            .unwrap();

        Self {
            registry,
            seer_pools_detected,
            seer_pools_parsed,
            seer_latency,
            oracle_candidates_scored,
            oracle_latency,
            oracle_score_histogram,
            features_plans_created,
            features_plans_rejected,
            buy_intents_initialized,
            buy_init_failures,
            trigger_txs_sent,
            trigger_txs_confirmed,
            trigger_txs_failed,
            trigger_send_latency,
            trigger_confirm_latency,
            jito_bundles_submitted,
            jito_bundles_confirmed,
            jito_total_tips_paid,
            jito_bundle_latency,
            e2e_total_latency,
            land_rate,
            inclusion_rate,
            sla_violations,
            leapfrog_packets_sent,
            qass_latency_ns,
            qass_score_histogram,
            qass_confidence_histogram,
            qass_analyses_total,
            qass_dominant_waves,
        }
    }

    /// Calculate and update Land Rate
    ///
    /// Land Rate = (parsed_success / detected_total) * 100
    pub fn update_land_rate(&self, amm_program: &str) -> f64 {
        let detected = self
            .seer_pools_detected
            .with_label_values(&[amm_program])
            .get();
        let parsed = self
            .seer_pools_parsed
            .with_label_values(&[amm_program])
            .get();

        if detected == 0.0 {
            return 0.0;
        }

        let rate = (parsed / detected) * 100.0;
        self.land_rate.set(rate);
        rate
    }

    /// Calculate and update Inclusion Rate
    ///
    /// Inclusion Rate = (confirmed / sent) * 100
    pub fn update_inclusion_rate(&self) -> f64 {
        let sent = self.trigger_txs_sent.get();
        let confirmed = self.trigger_txs_confirmed.get();

        if sent == 0.0 {
            return 0.0;
        }

        let rate = (confirmed / sent) * 100.0;
        self.inclusion_rate.set(rate);
        rate
    }

    /// Check if Land Rate meets SLA (≥ 95%)
    pub fn check_land_rate_sla(&self, amm_program: &str, target: f64) -> bool {
        let rate = self.update_land_rate(amm_program);
        if rate < target {
            self.sla_violations.with_label_values(&["land_rate"]).inc();
            false
        } else {
            true
        }
    }

    /// Check if Inclusion Rate meets SLA (≥ 92%)
    pub fn check_inclusion_rate_sla(&self, target: f64) -> bool {
        let rate = self.update_inclusion_rate();
        if rate < target {
            self.sla_violations
                .with_label_values(&["inclusion_rate"])
                .inc();
            false
        } else {
            true
        }
    }

    /// Get Jito bundle statistics
    ///
    /// Returns (bundles_submitted, bundles_confirmed, total_tips_paid)
    pub fn get_jito_stats(&self) -> (f64, f64, f64) {
        let submitted = self.jito_bundles_submitted.get();
        let confirmed = self.jito_bundles_confirmed.get();
        let tips_paid = self.jito_total_tips_paid.get();
        (submitted, confirmed, tips_paid)
    }

    /// Calculate Jito bundle confirmation rate
    pub fn calculate_jito_confirmation_rate(&self) -> f64 {
        let submitted = self.jito_bundles_submitted.get();
        if submitted == 0.0 {
            return 0.0;
        }
        let confirmed = self.jito_bundles_confirmed.get();
        (confirmed / submitted) * 100.0
    }

    /// Get a summary of current metrics
    pub fn get_summary(&self) -> MetricsSummary {
        MetricsSummary {
            land_rate_pumpfun: self.update_land_rate("pumpfun"),
            land_rate_bonkfun: self.update_land_rate("bonkfun"),
            inclusion_rate: self.update_inclusion_rate(),
            pools_detected_pumpfun: self
                .seer_pools_detected
                .with_label_values(&["pumpfun"])
                .get(),
            pools_detected_bonkfun: self
                .seer_pools_detected
                .with_label_values(&["bonkfun"])
                .get(),
            pools_parsed_pumpfun: self.seer_pools_parsed.with_label_values(&["pumpfun"]).get(),
            pools_parsed_bonkfun: self.seer_pools_parsed.with_label_values(&["bonkfun"]).get(),
            candidates_scored: self.oracle_candidates_scored.get(),
            plans_created: self.features_plans_created.get(),
            plans_rejected: self.features_plans_rejected.get(),
            intents_initialized: self.buy_intents_initialized.get(),
            init_failures: self.buy_init_failures.get(),
            txs_sent: self.trigger_txs_sent.get(),
            txs_confirmed: self.trigger_txs_confirmed.get(),
            txs_failed: self.trigger_txs_failed.get(),
        }
    }
}

/// Summary of E2E metrics
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub land_rate_pumpfun: f64,
    pub land_rate_bonkfun: f64,
    pub inclusion_rate: f64,
    pub pools_detected_pumpfun: f64,
    pub pools_detected_bonkfun: f64,
    pub pools_parsed_pumpfun: f64,
    pub pools_parsed_bonkfun: f64,
    pub candidates_scored: f64,
    pub plans_created: f64,
    pub plans_rejected: f64,
    pub intents_initialized: f64,
    pub init_failures: f64,
    pub txs_sent: f64,
    pub txs_confirmed: f64,
    pub txs_failed: f64,
}

impl Default for E2EMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = E2EMetrics::new();
        assert_eq!(metrics.land_rate.get(), 0.0);
        assert_eq!(metrics.inclusion_rate.get(), 0.0);
    }

    #[test]
    fn test_land_rate_calculation() {
        let metrics = E2EMetrics::new();

        // No data yet
        assert_eq!(metrics.update_land_rate("pumpfun"), 0.0);

        // Add some data
        metrics
            .seer_pools_detected
            .with_label_values(&["pumpfun"])
            .inc_by(100.0);
        metrics
            .seer_pools_parsed
            .with_label_values(&["pumpfun"])
            .inc_by(95.0);

        let rate = metrics.update_land_rate("pumpfun");
        assert_eq!(rate, 95.0);
    }

    #[test]
    fn test_inclusion_rate_calculation() {
        let metrics = E2EMetrics::new();

        // No data yet
        assert_eq!(metrics.update_inclusion_rate(), 0.0);

        // Add some data
        metrics.trigger_txs_sent.inc_by(100.0);
        metrics.trigger_txs_confirmed.inc_by(92.0);

        let rate = metrics.update_inclusion_rate();
        assert_eq!(rate, 92.0);
    }

    #[test]
    fn test_sla_checks() {
        let metrics = E2EMetrics::new();

        // Below SLA
        metrics
            .seer_pools_detected
            .with_label_values(&["pumpfun"])
            .inc_by(100.0);
        metrics
            .seer_pools_parsed
            .with_label_values(&["pumpfun"])
            .inc_by(90.0);
        assert!(!metrics.check_land_rate_sla("pumpfun", 95.0));

        // Above SLA
        metrics
            .seer_pools_parsed
            .with_label_values(&["pumpfun"])
            .inc_by(6.0);
        assert!(metrics.check_land_rate_sla("pumpfun", 95.0));
    }
}
