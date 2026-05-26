//! Seer main entry point
//!
//! This is the standalone executable for the Seer module.

use seer::{config::SeerConfig, ipc::*, Seer};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "seer=info,warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Seer - Real-time InitializePool Detector");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    // Load configuration (in production, this would come from a config file or environment)
    let config = load_config();

    info!("Configuration loaded:");
    info!("  Source mode: {:?}", config.source_mode);
    info!(
        "  Effective source mode: {:?}",
        config.effective_source_mode()
    );
    info!("  Connection mode (legacy): {:?}", config.connection_mode);
    info!("  Geyser endpoint: {}", config.geyser_endpoint);
    info!("  gRPC endpoint: {}", config.grpc_endpoint);
    info!("  RPC endpoint: {}", config.rpc_endpoint);
    info!("  Commitment: {}", config.commitment.as_str());
    info!(
        "  gRPC→WebSocket fallback enabled: {}",
        config.grpc_commitment_fallback_to_websocket
    );
    info!(
        "  gRPC circuit breaker: max_stalls_before_open={} cooldown_ms={}",
        config.grpc_max_stalls_before_open, config.grpc_circuit_breaker_cooldown_ms
    );
    info!("  Pump.fun enabled: {}", config.filter.enable_pumpfun);
    info!("  Bonk.fun enabled: {}", config.filter.enable_bonkfun);
    info!("  IPC buffer size: {}", config.ipc_config.buffer_size);
    info!(
        "  IPC backpressure policy: {:?}",
        config.ipc_config.backpressure_policy
    );

    // Log PumpPortal config when in PumpPortal mode
    if matches!(
        config.effective_source_mode(),
        seer::config::SeerSourceMode::PumpPortalWs
    ) {
        info!("PumpPortal WebSocket Mode:");
        info!("  WS URL: {}", config.pumpportal.ws_url);
        info!("  Max active mints: {}", config.pumpportal.max_active_mints);
        info!(
            "  Subscription batch size: {}",
            config.pumpportal.subscription_batch_size
        );
        info!(
            "  Reconnect base delay: {}s",
            config.pumpportal.reconnect_base_delay_secs
        );
        info!(
            "  Reconnect max delay: {}s",
            config.pumpportal.reconnect_max_delay_secs
        );
        info!("  Stats window: {}s", config.pumpportal.stats_window_secs);
    }

    // Create IPC channel for candidate forwarding to Trigger
    let (ipc_sender, mut ipc_receiver, ipc_metrics) = create_ipc_channel(config.ipc_config.clone());

    // Create Seer instance with IPC sender
    let seer = Arc::new(Seer::new_with_ipc(config.clone(), ipc_sender));

    // Start metrics server
    let metrics_port = config.metrics_port;
    tokio::spawn(async move {
        if let Err(e) = start_metrics_server(metrics_port).await {
            error!("Metrics server error: {}", e);
        }
    });

    // Start Seer in background
    let seer_handle = {
        let seer = Arc::clone(&seer);
        tokio::spawn(async move {
            loop {
                info!("Starting Seer event processing loop");
                match Arc::clone(&seer).run().await {
                    Ok(()) => {
                        info!("Seer event loop ended normally");
                        break;
                    }
                    Err(e) => {
                        error!("Seer error: {}. Restarting in 10 seconds...", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    }
                }
            }
        })
    };

    // Process candidates from Seer
    // In production, this would forward to Trigger via IPC
    tokio::spawn(async move {
        info!("Starting IPC event processing loop");
        while let Some(seer_event) = ipc_receiver.recv().await {
            match seer_event {
                SeerEvent::PoolDetected(event) => {
                    info!(
                        "Received Pool IPC event: pool={}, amm={}, priority={:?}, seq={}, queue_utilization={:.1}%",
                        event.candidate.pool_amm_id,
                        event.candidate.amm_program_id,
                        event.priority,
                        event.sequence_number,
                        ipc_metrics.calculate_queue_utilization(10000) // Use config buffer size
                    );

                    // Log IPC metrics periodically
                    if event.sequence_number % 100 == 0 {
                        let drop_rate = ipc_metrics.calculate_drop_rate();
                        let queue_util = ipc_metrics.calculate_queue_utilization(10000);
                        info!(
                            "IPC Metrics: queue_util={:.1}%, drop_rate={:.2}%, sent={}, received={}, dropped={}",
                            queue_util,
                            drop_rate,
                            ipc_metrics.events_sent.get(),
                            ipc_metrics.events_received.get(),
                            ipc_metrics.events_dropped.get()
                        );

                        if drop_rate > 1.0 {
                            warn!(
                                "IPC drop rate is high: {:.2}% - consider increasing buffer size or adjusting backpressure policy",
                                drop_rate
                            );
                        }
                    }

                    // TODO: Forward to Trigger for transaction building and execution
                    // For now, this demonstrates the IPC layer working
                    // In integrated deployment, Trigger would receive via this channel
                }

                SeerEvent::Trade(trade_event) => {
                    info!(
                        "Received Trade IPC event: {} on pool={}, mint={}, amount={}, seq={}",
                        if trade_event.trade.is_buy {
                            "BUY"
                        } else {
                            "SELL"
                        },
                        trade_event.trade.pool_amm_id,
                        trade_event.trade.mint,
                        trade_event.trade.amount,
                        trade_event.sequence_number
                    );
                }

                SeerEvent::FundingTransfer(_) => {}

                // AccountUpdate events are consumed by OracleRuntime via the
                // ghost-launcher event bus; nothing to do in the standalone binary.
                SeerEvent::AccountUpdate(_) => {}

                // Execution account evidence is forwarded by ghost-launcher in
                // integrated runtime; standalone Seer has no consumer.
                SeerEvent::ExecutionAccountEvidence(_) => {}
            }
        }
        info!("IPC event processing loop ended");
    });

    // Wait for Seer to complete
    seer_handle.await?;

    info!("Seer shutting down");
    Ok(())
}

/// Load configuration from environment or use defaults
fn load_config() -> SeerConfig {
    // In production, this would load from a config file or environment variables
    // For now, use defaults with some overrides from environment

    let mut config = SeerConfig::default();

    // Override source mode (NEW - supports PumpPortalWs)
    if let Ok(mode) = std::env::var("SEER_SOURCE_MODE") {
        config.source_mode = match mode.to_lowercase().as_str() {
            "geyser_grpc" => Some(seer::config::SeerSourceMode::GeyserGrpc),
            "geyser_websocket" => Some(seer::config::SeerSourceMode::GeyserWebSocket),
            "helius_websocket" => Some(seer::config::SeerSourceMode::HeliusWebSocket),
            "pump_portal_ws" => Some(seer::config::SeerSourceMode::PumpPortalWs),
            _ => {
                warn!("Unknown SEER_SOURCE_MODE '{}', will use default", mode);
                config.source_mode
            }
        };
    }

    // Override connection mode (legacy)
    if let Ok(mode) = std::env::var("SEER_CONNECTION_MODE") {
        config.connection_mode = match mode.to_lowercase().as_str() {
            "websocket" | "ws" => seer::config::ConnectionMode::WebSocket,
            "grpc" | "g" => seer::config::ConnectionMode::Grpc,
            _ => config.connection_mode,
        };
    }

    // Override with environment variables if present
    if let Ok(endpoint) = std::env::var("SEER_GEYSER_ENDPOINT") {
        config.geyser_endpoint = endpoint;
    }

    if let Ok(endpoint) = std::env::var("SEER_GRPC_ENDPOINT") {
        config.grpc_endpoint = endpoint;
    }

    if let Ok(endpoint) = std::env::var("SEER_RPC_ENDPOINT") {
        config.rpc_endpoint = endpoint;
    }

    // gRPC-specific configuration
    if let Ok(client_id) = std::env::var("SEER_GRPC_CLIENT_ID") {
        config.grpc_client_id = Some(client_id);
    }

    if let Ok(auth_token) = std::env::var("SEER_GRPC_AUTH_TOKEN") {
        config.grpc_auth_token = Some(auth_token);
    }

    // Commitment configuration
    if let Ok(commitment) = std::env::var("SEER_COMMITMENT") {
        config.commitment = match commitment.to_lowercase().as_str() {
            "processed" | "mempool" => seer::config::CommitmentLevel::Mempool,
            "finalized" => seer::config::CommitmentLevel::Finalized,
            "confirmed" => seer::config::CommitmentLevel::Confirmed,
            _ => config.commitment,
        };
    }

    if let Ok(fallback) = std::env::var("SEER_GRPC_WS_FALLBACK") {
        match fallback.parse() {
            Ok(flag) => config.grpc_commitment_fallback_to_websocket = flag,
            Err(_) => warn!(
                "Invalid SEER_GRPC_WS_FALLBACK value '{}', defaulting to {}",
                fallback, config.grpc_commitment_fallback_to_websocket
            ),
        }
    }

    if let Ok(max_delay) = std::env::var("SEER_MAX_RECONNECT_DELAY_SECS") {
        config.max_reconnect_delay_secs = max_delay.parse().unwrap_or(300);
    }

    if let Ok(max_stalls) = std::env::var("SEER_GRPC_MAX_STALLS_BEFORE_OPEN") {
        config.grpc_max_stalls_before_open = max_stalls.parse().unwrap_or(3);
    }

    if let Ok(cooldown_ms) = std::env::var("SEER_GRPC_CIRCUIT_BREAKER_COOLDOWN_MS") {
        config.grpc_circuit_breaker_cooldown_ms = cooldown_ms.parse().unwrap_or(15_000);
    }

    if let Ok(verbose) = std::env::var("SEER_VERBOSE") {
        config.verbose = verbose.parse().unwrap_or(false);
    }

    if let Ok(port) = std::env::var("SEER_METRICS_PORT") {
        config.metrics_port = port.parse().unwrap_or(9090);
    }

    // IPC configuration
    if let Ok(buffer_size) = std::env::var("SEER_IPC_BUFFER_SIZE") {
        config.ipc_config.buffer_size = buffer_size.parse().unwrap_or(10000);
    }

    if let Ok(policy) = std::env::var("SEER_IPC_BACKPRESSURE_POLICY") {
        config.ipc_config.backpressure_policy = match policy.to_lowercase().as_str() {
            "block" => BackpressurePolicy::Block,
            "dropoldest" | "drop_oldest" => BackpressurePolicy::DropOldest,
            "dropnew" | "drop_new" => BackpressurePolicy::DropNew,
            "dropbypriority" | "drop_by_priority" => BackpressurePolicy::DropByPriority,
            _ => BackpressurePolicy::Block,
        };
    }

    if let Ok(log_drops) = std::env::var("SEER_IPC_LOG_DROPS") {
        config.ipc_config.log_drops = log_drops.parse().unwrap_or(true);
    }

    if let Ok(log_overflows) = std::env::var("SEER_IPC_LOG_OVERFLOWS") {
        config.ipc_config.log_overflows = log_overflows.parse().unwrap_or(true);
    }

    if let Ok(threshold) = std::env::var("SEER_IPC_WARNING_THRESHOLD_PERCENT") {
        config.ipc_config.warning_threshold_percent = threshold.parse().unwrap_or(80.0);
    }

    // PumpPortal configuration
    if let Ok(ws_url) = std::env::var("PUMPPORTAL_WS_URL") {
        config.pumpportal.ws_url = ws_url;
    }

    if let Ok(max_mints) = std::env::var("PUMPPORTAL_MAX_ACTIVE_MINTS") {
        config.pumpportal.max_active_mints = max_mints.parse().unwrap_or(100);
    }

    if let Ok(batch_size) = std::env::var("PUMPPORTAL_SUBSCRIPTION_BATCH_SIZE") {
        config.pumpportal.subscription_batch_size = batch_size.parse().unwrap_or(10);
    }

    if let Ok(base_delay) = std::env::var("PUMPPORTAL_RECONNECT_BASE_DELAY_SECS") {
        config.pumpportal.reconnect_base_delay_secs = base_delay.parse().unwrap_or(5);
    }

    if let Ok(max_delay) = std::env::var("PUMPPORTAL_RECONNECT_MAX_DELAY_SECS") {
        config.pumpportal.reconnect_max_delay_secs = max_delay.parse().unwrap_or(300);
    }

    if let Ok(stats_window) = std::env::var("PUMPPORTAL_STATS_WINDOW_SECS") {
        config.pumpportal.stats_window_secs = stats_window.parse().unwrap_or(900);
    }

    config
}

/// Start Prometheus metrics HTTP server
async fn start_metrics_server(port: u16) -> anyhow::Result<()> {
    use prometheus::{Encoder, TextEncoder};
    use std::net::SocketAddr;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;

    info!(
        "Prometheus metrics server listening on http://{}/metrics",
        addr
    );

    loop {
        let (mut stream, _) = listener.accept().await?;

        tokio::spawn(async move {
            let mut buffer = vec![0; 1024];
            if stream.read(&mut buffer).await.is_err() {
                return;
            }

            // Simple HTTP response with metrics
            let encoder = TextEncoder::new();
            let metric_families = prometheus::gather();
            let mut metrics_buffer = Vec::new();

            if encoder
                .encode(&metric_families, &mut metrics_buffer)
                .is_err()
            {
                return;
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
                metrics_buffer.len(),
                String::from_utf8_lossy(&metrics_buffer)
            );

            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}
