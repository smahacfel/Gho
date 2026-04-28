//! FRB Metrics - Prometheus Monitoring for FRB Integration
//!
//! This module provides Prometheus metrics for monitoring FRB (Fractal Resonance Bands)
//! performance, signal quality, and false-positive rate in production.
//!
//! ## Metrics Categories
//!
//! 1. **Performance Metrics**
//!    - Latency: band extraction, resonance analysis, integration
//!    - Throughput: transactions/second, signals/second
//!
//! 2. **Signal Quality Metrics**
//!    - Resonance score distribution
//!    - Coherence map values
//!    - Trend likelihood distribution
//!    - Signal type counts
//!
//! 3. **Integration Metrics**
//!    - QOFSV coherence boost distribution
//!    - WHF validation success rate
//!    - QMAN enhancement confidence
//!    - Integration confidence distribution
//!
//! 4. **Anomaly Detection Metrics**
//!    - False positive rate (estimated)
//!    - Bot manipulation detection count
//!    - Wash trading detection count
//!    - Signal contradiction rate

use crate::signals::{FrbIntegrationResult, FrbResult, FrbSignal};
use prometheus::{
    Encoder, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};
use std::sync::Arc;

/// FRB Metrics collection
#[derive(Clone)]
pub struct FrbMetrics {
    registry: Arc<Registry>,

    // Performance metrics
    band_extraction_duration: Histogram,
    resonance_analysis_duration: Histogram,
    integration_duration: Histogram,
    e2e_pipeline_duration: Histogram,

    // Throughput metrics
    transactions_processed: IntCounter,
    bands_extracted: IntCounter,
    signals_generated: IntCounter,

    // Signal quality metrics
    resonance_score: Histogram,
    coherence_short_medium: Histogram,
    coherence_medium_long: Histogram,
    coherence_short_long: Histogram,
    trend_likelihood: Histogram,

    // Signal type counters
    signal_counts: IntCounterVec,

    // Integration metrics
    qofsv_coherence_boost: Histogram,
    qofsv_amplitude_multiplier: Histogram,
    whf_validation_success: IntCounter,
    whf_validation_failure: IntCounter,
    qman_confidence_boost: Histogram,
    integration_confidence: Histogram,

    // Anomaly detection metrics
    bot_manipulation_detected: IntCounter,
    wash_trading_detected: IntCounter,
    false_positive_suspected: IntCounter,
    signal_contradictions: IntCounter,

    // Active state gauges
    active_short_band_buyers: IntGauge,
    active_medium_band_buyers: IntGauge,
    active_long_band_buyers: IntGauge,
    buffer_size: IntGauge,
}

impl FrbMetrics {
    /// Create new FRB metrics collector
    pub fn new() -> Self {
        let registry = Arc::new(Registry::new());

        // Performance metrics (in microseconds)
        let band_extraction_duration = Histogram::with_opts(
            HistogramOpts::new(
                "frb_band_extraction_duration_us",
                "Time to extract band profiles (microseconds)",
            )
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0]),
        )
        .unwrap();
        registry
            .register(Box::new(band_extraction_duration.clone()))
            .unwrap();

        let resonance_analysis_duration = Histogram::with_opts(
            HistogramOpts::new(
                "frb_resonance_analysis_duration_us",
                "Time to analyze resonance (microseconds)",
            )
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0]),
        )
        .unwrap();
        registry
            .register(Box::new(resonance_analysis_duration.clone()))
            .unwrap();

        let integration_duration = Histogram::with_opts(
            HistogramOpts::new(
                "frb_integration_duration_us",
                "Time for FRB integration (microseconds)",
            )
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0]),
        )
        .unwrap();
        registry
            .register(Box::new(integration_duration.clone()))
            .unwrap();

        let e2e_pipeline_duration = Histogram::with_opts(
            HistogramOpts::new(
                "frb_e2e_pipeline_duration_us",
                "End-to-end pipeline latency (microseconds)",
            )
            .buckets(vec![10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0]),
        )
        .unwrap();
        registry
            .register(Box::new(e2e_pipeline_duration.clone()))
            .unwrap();

        // Throughput metrics
        let transactions_processed = IntCounter::new(
            "frb_transactions_processed_total",
            "Total transactions processed by FRB",
        )
        .unwrap();
        registry
            .register(Box::new(transactions_processed.clone()))
            .unwrap();

        let bands_extracted = IntCounter::new(
            "frb_bands_extracted_total",
            "Total band extractions performed",
        )
        .unwrap();
        registry
            .register(Box::new(bands_extracted.clone()))
            .unwrap();

        let signals_generated =
            IntCounter::new("frb_signals_generated_total", "Total FRB signals generated").unwrap();
        registry
            .register(Box::new(signals_generated.clone()))
            .unwrap();

        // Signal quality metrics
        let resonance_score = Histogram::with_opts(
            HistogramOpts::new(
                "frb_resonance_score",
                "FRB resonance score distribution (0.0-1.0)",
            )
            .buckets(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]),
        )
        .unwrap();
        registry
            .register(Box::new(resonance_score.clone()))
            .unwrap();

        let coherence_short_medium = Histogram::with_opts(
            HistogramOpts::new(
                "frb_coherence_short_medium",
                "Coherence between short and medium bands (0.0-1.0)",
            )
            .buckets(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]),
        )
        .unwrap();
        registry
            .register(Box::new(coherence_short_medium.clone()))
            .unwrap();

        let coherence_medium_long = Histogram::with_opts(
            HistogramOpts::new(
                "frb_coherence_medium_long",
                "Coherence between medium and long bands (0.0-1.0)",
            )
            .buckets(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]),
        )
        .unwrap();
        registry
            .register(Box::new(coherence_medium_long.clone()))
            .unwrap();

        let coherence_short_long = Histogram::with_opts(
            HistogramOpts::new(
                "frb_coherence_short_long",
                "Coherence between short and long bands (0.0-1.0)",
            )
            .buckets(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]),
        )
        .unwrap();
        registry
            .register(Box::new(coherence_short_long.clone()))
            .unwrap();

        let trend_likelihood = Histogram::with_opts(
            HistogramOpts::new(
                "frb_trend_likelihood",
                "FRB trend likelihood distribution (0.0-1.0)",
            )
            .buckets(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]),
        )
        .unwrap();
        registry
            .register(Box::new(trend_likelihood.clone()))
            .unwrap();

        // Signal type counters
        let signal_counts = IntCounterVec::new(
            Opts::new("frb_signal_type_total", "Count of each FRB signal type"),
            &["signal_type"],
        )
        .unwrap();
        registry.register(Box::new(signal_counts.clone())).unwrap();

        // Integration metrics
        let qofsv_coherence_boost = Histogram::with_opts(
            HistogramOpts::new(
                "frb_qofsv_coherence_boost",
                "QOFSV coherence boost factor distribution",
            )
            .buckets(vec![0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8]),
        )
        .unwrap();
        registry
            .register(Box::new(qofsv_coherence_boost.clone()))
            .unwrap();

        let qofsv_amplitude_multiplier = Histogram::with_opts(
            HistogramOpts::new(
                "frb_qofsv_amplitude_multiplier",
                "QOFSV amplitude multiplier distribution",
            )
            .buckets(vec![0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5]),
        )
        .unwrap();
        registry
            .register(Box::new(qofsv_amplitude_multiplier.clone()))
            .unwrap();

        let whf_validation_success = IntCounter::new(
            "frb_whf_validation_success_total",
            "Count of successful WHF validations",
        )
        .unwrap();
        registry
            .register(Box::new(whf_validation_success.clone()))
            .unwrap();

        let whf_validation_failure = IntCounter::new(
            "frb_whf_validation_failure_total",
            "Count of failed WHF validations (contradictions)",
        )
        .unwrap();
        registry
            .register(Box::new(whf_validation_failure.clone()))
            .unwrap();

        let qman_confidence_boost = Histogram::with_opts(
            HistogramOpts::new(
                "frb_qman_confidence_boost",
                "QMAN confidence boost distribution",
            )
            .buckets(vec![0.0, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30]),
        )
        .unwrap();
        registry
            .register(Box::new(qman_confidence_boost.clone()))
            .unwrap();

        let integration_confidence = Histogram::with_opts(
            HistogramOpts::new(
                "frb_integration_confidence",
                "Overall integration confidence (0.0-1.0)",
            )
            .buckets(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]),
        )
        .unwrap();
        registry
            .register(Box::new(integration_confidence.clone()))
            .unwrap();

        // Anomaly detection metrics
        let bot_manipulation_detected = IntCounter::new(
            "frb_bot_manipulation_detected_total",
            "Count of bot manipulation detections",
        )
        .unwrap();
        registry
            .register(Box::new(bot_manipulation_detected.clone()))
            .unwrap();

        let wash_trading_detected = IntCounter::new(
            "frb_wash_trading_detected_total",
            "Count of wash trading detections",
        )
        .unwrap();
        registry
            .register(Box::new(wash_trading_detected.clone()))
            .unwrap();

        let false_positive_suspected = IntCounter::new(
            "frb_false_positive_suspected_total",
            "Count of suspected false positive signals",
        )
        .unwrap();
        registry
            .register(Box::new(false_positive_suspected.clone()))
            .unwrap();

        let signal_contradictions = IntCounter::new(
            "frb_signal_contradictions_total",
            "Count of FRB/WHF signal contradictions",
        )
        .unwrap();
        registry
            .register(Box::new(signal_contradictions.clone()))
            .unwrap();

        // Active state gauges
        let active_short_band_buyers = IntGauge::new(
            "frb_active_short_band_buyers",
            "Current number of unique buyers in short band",
        )
        .unwrap();
        registry
            .register(Box::new(active_short_band_buyers.clone()))
            .unwrap();

        let active_medium_band_buyers = IntGauge::new(
            "frb_active_medium_band_buyers",
            "Current number of unique buyers in medium band",
        )
        .unwrap();
        registry
            .register(Box::new(active_medium_band_buyers.clone()))
            .unwrap();

        let active_long_band_buyers = IntGauge::new(
            "frb_active_long_band_buyers",
            "Current number of unique buyers in long band",
        )
        .unwrap();
        registry
            .register(Box::new(active_long_band_buyers.clone()))
            .unwrap();

        let buffer_size =
            IntGauge::new("frb_buffer_size", "Current size of transaction buffer").unwrap();
        registry.register(Box::new(buffer_size.clone())).unwrap();

        Self {
            registry,
            band_extraction_duration,
            resonance_analysis_duration,
            integration_duration,
            e2e_pipeline_duration,
            transactions_processed,
            bands_extracted,
            signals_generated,
            resonance_score,
            coherence_short_medium,
            coherence_medium_long,
            coherence_short_long,
            trend_likelihood,
            signal_counts,
            qofsv_coherence_boost,
            qofsv_amplitude_multiplier,
            whf_validation_success,
            whf_validation_failure,
            qman_confidence_boost,
            integration_confidence,
            bot_manipulation_detected,
            wash_trading_detected,
            false_positive_suspected,
            signal_contradictions,
            active_short_band_buyers,
            active_medium_band_buyers,
            active_long_band_buyers,
            buffer_size,
        }
    }

    /// Render metrics in Prometheus text format
    ///
    /// Returns metrics in Prometheus exposition format, or an empty string on error.
    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();

        let mut buffer = Vec::new();
        if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
            eprintln!("Failed to encode metrics: {}", e);
            return String::new();
        }

        String::from_utf8(buffer).unwrap_or_else(|e| {
            eprintln!("Failed to convert metrics to UTF-8: {}", e);
            String::new()
        })
    }
}

/// FRB Metrics Reporter - Convenient API for recording metrics
pub struct FrbMetricsReporter {
    metrics: FrbMetrics,
}

impl FrbMetricsReporter {
    /// Create new metrics reporter
    pub fn new(metrics: FrbMetrics) -> Self {
        Self { metrics }
    }

    /// Record transaction processing
    pub fn record_transaction(&self) {
        self.metrics.transactions_processed.inc();
    }

    /// Record band extraction with duration
    pub fn record_band_extraction(&self, duration_us: f64) {
        self.metrics.bands_extracted.inc();
        self.metrics.band_extraction_duration.observe(duration_us);
    }

    /// Record resonance analysis with duration
    pub fn record_resonance_analysis(&self, duration_us: f64) {
        self.metrics
            .resonance_analysis_duration
            .observe(duration_us);
    }

    /// Record FRB result metrics
    pub fn record_frb_result(&self, result: &FrbResult) {
        self.metrics.signals_generated.inc();
        self.metrics
            .resonance_score
            .observe(result.resonance_score as f64);
        self.metrics
            .coherence_short_medium
            .observe(result.coherence_map[0] as f64);
        self.metrics
            .coherence_medium_long
            .observe(result.coherence_map[1] as f64);
        self.metrics
            .coherence_short_long
            .observe(result.coherence_map[2] as f64);
        self.metrics
            .trend_likelihood
            .observe(result.trend_likelihood as f64);

        // Update signal type counter
        let signal_label = match result.signal {
            FrbSignal::ResContinue => "continue",
            FrbSignal::ResFake => "fake",
            FrbSignal::ResTransition => "transition",
            FrbSignal::ResHold => "hold",
        };
        self.metrics
            .signal_counts
            .with_label_values(&[signal_label])
            .inc();

        // Update buyer counts
        self.metrics
            .active_short_band_buyers
            .set(result.band_profiles[0].buyers as i64);
        self.metrics
            .active_medium_band_buyers
            .set(result.band_profiles[1].buyers as i64);
        self.metrics
            .active_long_band_buyers
            .set(result.band_profiles[2].buyers as i64);

        // Detect anomalies
        if result.signal == FrbSignal::ResFake {
            if result.band_profiles[0].buyers < 3 {
                self.metrics.bot_manipulation_detected.inc();
            }
        }
    }

    /// Record integration result metrics
    pub fn record_integration(&self, integration: &FrbIntegrationResult, duration_us: f64) {
        self.metrics.integration_duration.observe(duration_us);
        self.metrics
            .integration_confidence
            .observe(integration.integration_confidence as f64);

        // QOFSV metrics
        self.metrics
            .qofsv_coherence_boost
            .observe(integration.qofsv_enhancement.coherence_boost as f64);
        self.metrics
            .qofsv_amplitude_multiplier
            .observe(integration.qofsv_enhancement.amplitude_multiplier as f64);

        // WHF validation metrics
        if let Some(ref whf) = integration.whf_validation {
            if whf.is_valid {
                self.metrics.whf_validation_success.inc();
            } else {
                self.metrics.whf_validation_failure.inc();
                self.metrics.signal_contradictions.inc();

                // Check for specific patterns
                if !whf.risk_flags.is_empty() {
                    self.metrics.false_positive_suspected.inc();
                }
            }
        }

        // QMAN enhancement metrics
        if let Some(ref qman) = integration.qman_enhancement {
            self.metrics
                .qman_confidence_boost
                .observe(qman.confidence_boost as f64);
        }

        // Count warnings
        if !integration.warnings.is_empty() {
            for warning in &integration.warnings {
                if warning.contains("wash") || warning.contains("Wash") {
                    self.metrics.wash_trading_detected.inc();
                } else if warning.contains("bot") || warning.contains("Bot") {
                    self.metrics.bot_manipulation_detected.inc();
                }
            }
        }
    }

    /// Record end-to-end pipeline duration
    pub fn record_e2e_duration(&self, duration_us: f64) {
        self.metrics.e2e_pipeline_duration.observe(duration_us);
    }

    /// Update buffer size gauge
    pub fn update_buffer_size(&self, size: usize) {
        self.metrics.buffer_size.set(size as i64);
    }

    /// Get false positive rate estimate
    ///
    /// Returns (false_positives, total_signals) for external calculation
    pub fn get_false_positive_stats(&self) -> (u64, u64) {
        let false_positives = self.metrics.false_positive_suspected.get();
        let total_signals = self.metrics.signals_generated.get();
        (false_positives, total_signals)
    }
}

impl Default for FrbMetrics {
    fn default() -> Self {
        Self::new()
    }
}
