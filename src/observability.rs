//! Observability module - OpenTelemetry integration for distributed tracing and metrics
//!
//! This module provides comprehensive observability for the H-5N1P3R system using:
//! - OpenTelemetry for distributed tracing
//! - Prometheus for metrics collection
//! - OTLP exporter for sending traces to Jaeger or other backends
//!
//! # Features
//! - Automatic span creation for all major system flows
//! - Context propagation across async boundaries
//! - Prometheus metrics endpoint for monitoring
//! - Jaeger trace export for visualization
//!
//! # Usage
//! ```no_run
//! use h_5n1p3r::observability::init_observability;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Initialize with default settings (Jaeger on localhost:4317)
//!     let _guard = init_observability(None).expect("Failed to initialize observability");
//!     
//!     // Your application code here
//!     
//!     // Guard will flush traces on drop
//! }
//! ```

use anyhow::{Context, Result};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::{
    runtime,
    trace::{Config, RandomIdGenerator, Sampler},
    Resource,
};
use prometheus::{Encoder, TextEncoder};
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Configuration for the observability system
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Service name for traces and metrics
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Environment (dev, staging, prod)
    pub environment: String,
    /// OTLP endpoint for traces (e.g., "http://localhost:4317" for Jaeger)
    pub otlp_endpoint: String,
    /// Whether to enable trace export
    pub enable_tracing: bool,
    /// Whether to enable metrics export
    pub enable_metrics: bool,
    /// Sampling ratio (0.0 to 1.0)
    pub sampling_ratio: f64,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            service_name: "h-5n1p3r".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            environment: std::env::var("ENVIRONMENT").unwrap_or_else(|_| "dev".to_string()),
            otlp_endpoint: std::env::var("OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_string()),
            enable_tracing: std::env::var("ENABLE_TRACING")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            enable_metrics: std::env::var("ENABLE_METRICS")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            sampling_ratio: std::env::var("TRACE_SAMPLING_RATIO")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.0),
        }
    }
}

/// Guard that ensures proper cleanup of OpenTelemetry resources
pub struct ObservabilityGuard;

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        // Shutdown the tracer provider to flush remaining spans
        global::shutdown_tracer_provider();
        tracing::info!("OpenTelemetry tracer provider shutdown complete");
    }
}

/// Initialize the observability system with tracing and metrics
///
/// This function sets up:
/// - OpenTelemetry tracer with OTLP export to Jaeger
/// - Prometheus metrics registry
/// - Tracing subscriber with OpenTelemetry layer
///
/// # Arguments
/// * `config` - Optional configuration. If None, uses default configuration
///
/// # Returns
/// A guard that will flush traces when dropped
///
/// # Example
/// ```no_run
/// use h_5n1p3r::observability::{init_observability, ObservabilityConfig};
///
/// let config = ObservabilityConfig {
///     service_name: "my-service".to_string(),
///     otlp_endpoint: "http://jaeger:4317".to_string(),
///     ..Default::default()
/// };
/// let _guard = init_observability(Some(config)).unwrap();
/// ```
pub fn init_observability(config: Option<ObservabilityConfig>) -> Result<ObservabilityGuard> {
    let config = config.unwrap_or_default();

    tracing::info!(
        "Initializing observability: service={}, version={}, environment={}, otlp_endpoint={}",
        config.service_name,
        config.service_version,
        config.environment,
        config.otlp_endpoint
    );

    // Create resource with service information
    let resource = Resource::new(vec![
        KeyValue::new("service.name", config.service_name.clone()),
        KeyValue::new("service.version", config.service_version.clone()),
        KeyValue::new("deployment.environment", config.environment.clone()),
    ]);

    if config.enable_metrics {
        init_metrics(&resource)?;
    }

    if config.enable_tracing {
        init_tracing(&config, resource)?;
    }

    // Set up tracing subscriber with OpenTelemetry layer
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true);

    if config.enable_tracing {
        let telemetry_layer = tracing_opentelemetry::layer();

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(telemetry_layer)
            .try_init()
            .context("Failed to initialize tracing subscriber")?;
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .try_init()
            .context("Failed to initialize tracing subscriber")?;
    }

    tracing::info!("Observability initialized successfully");

    Ok(ObservabilityGuard)
}

/// Initialize OpenTelemetry tracing with OTLP exporter
fn init_tracing(config: &ObservabilityConfig, resource: Resource) -> Result<()> {
    // Configure OTLP exporter for traces
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(&config.otlp_endpoint)
        .with_protocol(Protocol::Grpc)
        .with_timeout(Duration::from_secs(3));

    // Build pipeline with exporter
    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            Config::default()
                .with_sampler(Sampler::TraceIdRatioBased(config.sampling_ratio))
                .with_id_generator(RandomIdGenerator::default())
                .with_resource(resource),
        )
        .install_batch(runtime::Tokio)
        .context("Failed to install OTLP tracer")?;

    // Set as global tracer provider
    global::set_tracer_provider(tracer_provider);

    tracing::info!(
        "OpenTelemetry tracing initialized with endpoint: {}",
        config.otlp_endpoint
    );

    Ok(())
}

/// Initialize Prometheus metrics
fn init_metrics(resource: &Resource) -> Result<()> {
    // Create Prometheus exporter
    let exporter = opentelemetry_prometheus::exporter()
        .with_registry(prometheus::Registry::new())
        .build()
        .context("Failed to create Prometheus exporter")?;

    tracing::info!("Prometheus metrics initialized");

    Ok(())
}

/// Get Prometheus metrics in text format
///
/// This function can be used to expose metrics via an HTTP endpoint
///
/// # Example
/// ```no_run
/// use h_5n1p3r::observability::get_metrics;
///
/// let metrics = get_metrics();
/// // Serve via HTTP endpoint
/// ```
pub fn get_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();

    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        tracing::error!("Failed to encode metrics: {}", e);
        return String::new();
    }

    String::from_utf8(buffer).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.service_name, "h-5n1p3r");
        assert!(config.enable_tracing);
        assert!(config.enable_metrics);
        assert_eq!(config.sampling_ratio, 1.0);
    }

    #[test]
    fn test_custom_config() {
        let config = ObservabilityConfig {
            service_name: "test-service".to_string(),
            service_version: "1.0.0".to_string(),
            environment: "test".to_string(),
            otlp_endpoint: "http://test:4317".to_string(),
            enable_tracing: false,
            enable_metrics: true,
            sampling_ratio: 0.5,
        };

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, "1.0.0");
        assert_eq!(config.environment, "test");
        assert!(!config.enable_tracing);
        assert!(config.enable_metrics);
        assert_eq!(config.sampling_ratio, 0.5);
    }

    #[test]
    fn test_get_metrics() {
        // Just ensure it doesn't panic
        let metrics = get_metrics();
        assert!(metrics.is_empty() || metrics.contains("# HELP"));
    }
}
