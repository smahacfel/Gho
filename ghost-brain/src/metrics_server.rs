//! Prometheus Metrics HTTP Server
//!
//! Provides HTTP endpoints for Prometheus metrics scraping and health checks.
//!
//! ## Endpoints
//!
//! - `GET /metrics` - Prometheus metrics in text format
//! - `GET /readyz` - Health check endpoint (200 OK if metrics available)
//! - `GET /healthz` - Alias for /readyz
//!
//! ## Usage
//!
//! ```ignore
//! use ghost_brain::metrics_server::{MetricsServer, MetricsServerConfig};
//! use ghost_brain::metrics::E2EMetrics;
//!
//! let metrics = E2EMetrics::new();
//! let config = MetricsServerConfig::default();
//!
//! // Start server
//! let server = MetricsServer::new(metrics, config);
//! server.run().await?;
//! ```

use crate::metrics::E2EMetrics;
use prometheus::{Counter, Encoder, Gauge, Histogram, HistogramOpts, Registry, TextEncoder};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Default port for metrics server
pub const DEFAULT_METRICS_PORT: u16 = 9091;

/// Configuration for the metrics server
#[derive(Debug, Clone)]
pub struct MetricsServerConfig {
    /// Port to listen on
    pub port: u16,
    /// Bind address (default: 0.0.0.0)
    pub bind_address: String,
}

impl Default for MetricsServerConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_METRICS_PORT,
            bind_address: "0.0.0.0".to_string(),
        }
    }
}

impl MetricsServerConfig {
    /// Create a new config with a specific port
    pub fn with_port(port: u16) -> Self {
        Self {
            port,
            ..Default::default()
        }
    }
}

/// Additional Ghost-specific metrics tracked by the metrics server
#[derive(Clone)]
pub struct GhostMetrics {
    /// RPC latency histogram (milliseconds)
    pub rpc_latency: Histogram,
    /// TPU leaders resolved gauge (1 = resolved, 0 = not resolved)
    pub tpu_leaders_resolved: Gauge,
    /// Validation rejects counter (scams/invalid pools rejected)
    pub validation_rejects: Counter,
}

impl GhostMetrics {
    /// Create new Ghost metrics and register them with the given registry
    pub fn new(registry: &Registry) -> Self {
        let rpc_latency = Histogram::with_opts(
            HistogramOpts::new("ghost_rpc_latency_ms", "RPC call latency in milliseconds").buckets(
                vec![
                    1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0,
                ],
            ),
        )
        .expect("Failed to create rpc_latency histogram");
        registry.register(Box::new(rpc_latency.clone())).ok();

        let tpu_leaders_resolved = Gauge::new(
            "ghost_tpu_leaders_resolved",
            "Whether TPU leaders are resolved (1 = yes, 0 = no)",
        )
        .expect("Failed to create tpu_leaders_resolved gauge");
        registry
            .register(Box::new(tpu_leaders_resolved.clone()))
            .ok();

        let validation_rejects = Counter::new(
            "ghost_validation_rejects_total",
            "Total number of pools/transactions rejected by validation (scams, invalid)",
        )
        .expect("Failed to create validation_rejects counter");
        registry.register(Box::new(validation_rejects.clone())).ok();

        Self {
            rpc_latency,
            tpu_leaders_resolved,
            validation_rejects,
        }
    }

    /// Record an RPC latency measurement
    pub fn observe_rpc_latency(&self, latency_ms: f64) {
        self.rpc_latency.observe(latency_ms);
    }

    /// Set whether TPU leaders are resolved
    pub fn set_tpu_leaders_resolved(&self, resolved: bool) {
        self.tpu_leaders_resolved
            .set(if resolved { 1.0 } else { 0.0 });
    }

    /// Increment validation rejects counter
    pub fn inc_validation_rejects(&self) {
        self.validation_rejects.inc();
    }
}

/// Prometheus metrics HTTP server
pub struct MetricsServer {
    /// E2E metrics instance
    metrics: E2EMetrics,
    /// Ghost-specific metrics
    ghost_metrics: GhostMetrics,
    /// Server configuration
    config: MetricsServerConfig,
    /// Server running state
    is_running: Arc<RwLock<bool>>,
}

impl MetricsServer {
    /// Create a new metrics server
    pub fn new(metrics: E2EMetrics, config: MetricsServerConfig) -> Self {
        let ghost_metrics = GhostMetrics::new(&metrics.registry);
        Self {
            metrics,
            ghost_metrics,
            config,
            is_running: Arc::new(RwLock::new(false)),
        }
    }

    /// Create with default configuration
    pub fn new_default(metrics: E2EMetrics) -> Self {
        Self::new(metrics, MetricsServerConfig::default())
    }

    /// Get a reference to the ghost metrics
    pub fn ghost_metrics(&self) -> &GhostMetrics {
        &self.ghost_metrics
    }

    /// Get a reference to the E2E metrics
    pub fn e2e_metrics(&self) -> &E2EMetrics {
        &self.metrics
    }

    /// Run the metrics server
    ///
    /// This starts an HTTP server that exposes Prometheus metrics.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr: SocketAddr = format!("{}:{}", self.config.bind_address, self.config.port)
            .parse()
            .map_err(|e| format!("Invalid address: {}", e))?;

        info!("📊 Starting Prometheus metrics server on {}", addr);

        // Clone registry for handler
        let registry = Arc::clone(&self.metrics.registry);
        let is_running = Arc::clone(&self.is_running);

        // Set running flag
        *is_running.write().await = true;

        // Create simple HTTP server using tokio
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        info!("📊 Metrics server listening on http://{}/metrics", addr);
        info!("📊 Health check available at http://{}/readyz", addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let registry = Arc::clone(&registry);
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, registry).await {
                            warn!("Error handling connection from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }
    }

    /// Check if the server is running
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }
}

/// Handle an individual HTTP connection
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    registry: Arc<Registry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).await?;

    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..n]);
    let first_line = request.lines().next().unwrap_or("");

    // Parse the request path
    let response = if first_line.starts_with("GET /metrics") {
        // Serialize metrics
        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        let body = String::from_utf8(buffer)?;

        format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/plain; version=0.0.4; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            body.len(),
            body
        )
    } else if first_line.starts_with("GET /readyz") || first_line.starts_with("GET /healthz") {
        // Health check - verify metrics are available
        let metric_families = registry.gather();
        if !metric_families.is_empty() {
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/plain\r\n\
             Content-Length: 2\r\n\
             \r\n\
             OK"
            .to_string()
        } else {
            "HTTP/1.1 503 Service Unavailable\r\n\
             Content-Type: text/plain\r\n\
             Content-Length: 17\r\n\
             \r\n\
             Metrics not ready"
                .to_string()
        }
    } else {
        // 404 for unknown paths
        let body = "Not Found";
        format!(
            "HTTP/1.1 404 Not Found\r\n\
             Content-Type: text/plain\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            body.len(),
            body
        )
    };

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}

/// Start the metrics server in a background task
///
/// Returns a handle that can be used to check if the server is running.
pub fn start_metrics_server(
    metrics: E2EMetrics,
    config: MetricsServerConfig,
) -> Arc<MetricsServer> {
    let server = Arc::new(MetricsServer::new(metrics, config));
    let server_clone = Arc::clone(&server);

    tokio::spawn(async move {
        if let Err(e) = server_clone.run().await {
            error!("Metrics server error: {}", e);
        }
    });

    server
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_server_config_default() {
        let config = MetricsServerConfig::default();
        assert_eq!(config.port, DEFAULT_METRICS_PORT);
        assert_eq!(config.bind_address, "0.0.0.0");
    }

    #[test]
    fn test_metrics_server_config_with_port() {
        let config = MetricsServerConfig::with_port(9999);
        assert_eq!(config.port, 9999);
    }

    #[test]
    fn test_ghost_metrics_creation() {
        let registry = Registry::new();
        let ghost_metrics = GhostMetrics::new(&registry);

        // Test recording metrics
        ghost_metrics.observe_rpc_latency(50.0);
        ghost_metrics.set_tpu_leaders_resolved(true);
        ghost_metrics.inc_validation_rejects();

        // Verify metrics are registered
        let families = registry.gather();
        let names: Vec<&str> = families.iter().map(|f| f.get_name()).collect();

        assert!(names.contains(&"ghost_rpc_latency_ms"));
        assert!(names.contains(&"ghost_tpu_leaders_resolved"));
        assert!(names.contains(&"ghost_validation_rejects_total"));
    }

    #[test]
    fn test_ghost_metrics_values() {
        let registry = Registry::new();
        let ghost_metrics = GhostMetrics::new(&registry);

        ghost_metrics.set_tpu_leaders_resolved(false);
        assert_eq!(ghost_metrics.tpu_leaders_resolved.get(), 0.0);

        ghost_metrics.set_tpu_leaders_resolved(true);
        assert_eq!(ghost_metrics.tpu_leaders_resolved.get(), 1.0);

        ghost_metrics.inc_validation_rejects();
        ghost_metrics.inc_validation_rejects();
        assert_eq!(ghost_metrics.validation_rejects.get(), 2.0);
    }

    #[tokio::test]
    async fn test_metrics_server_creation() {
        let metrics = E2EMetrics::new();
        let config = MetricsServerConfig::with_port(19091); // Use non-standard port for test
        let server = MetricsServer::new(metrics, config);

        assert!(!server.is_running().await);
    }
}
