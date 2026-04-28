//! Main entry point for the H-5N1P3R system
//! Slim orchestrator that initializes and coordinates all system components.

use anyhow::Result;
use h_5n1p3r::observability::{init_observability, ObservabilityConfig};
use h_5n1p3r::oracle::{MarketRegime, TransactionMonitor};
use h_5n1p3r::{ChannelFactory, OrchestratorConfig, ShutdownCoordinator, SystemInitializer};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize observability first (before regular logging)
    let _observability_guard = init_observability(Some(ObservabilityConfig {
        enable_tracing: std::env::var("ENABLE_TRACING")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false), // Disabled by default to avoid dependency on Jaeger
        ..Default::default()
    }))
    .unwrap_or_else(|e| {
        eprintln!("Warning: Failed to initialize observability: {}", e);
        eprintln!("Continuing without OpenTelemetry tracing...");
        h_5n1p3r::observability::ObservabilityGuard
    });

    let config = match OrchestratorConfig::from_toml_file("config.toml") {
        Ok(c) => {
            tracing::info!("Loaded config from config.toml");
            c
        }
        Err(_) => {
            tracing::info!("Using default config");
            OrchestratorConfig::default()
        }
    };

    tracing::info!("Starting H-5N1P3R Oracle System");

    let mut shutdown = ShutdownCoordinator::new();
    let factory = ChannelFactory::new(config.channel_buffer_size);
    let ledger_ch = factory.create_decision_ledger_channels();
    let ooda_ch = factory.create_ooda_channels();
    let oracle_ch = factory.create_oracle_channels();

    let (ledger, db_pool, storage) = SystemInitializer::init_pillar_one(
        ledger_ch.decision_record_receiver,
        ledger_ch.outcome_update_receiver,
    )
    .await?;

    let rpc = Arc::new(RpcClient::new_with_timeout(
        config.rpc_url.clone(),
        Duration::from_secs(config.rpc_timeout_secs),
    ));

    let tx_monitor = TransactionMonitor::new(
        storage,
        ledger_ch.outcome_update_sender.clone(),
        config.tx_monitor.check_interval_ms,
        rpc,
        config.wallet_pubkey.clone(),
    );

    let (perf_monitor, optimizer) = SystemInitializer::init_pillar_two(
        db_pool,
        ooda_ch.perf_report_sender,
        ooda_ch.perf_report_receiver,
        ooda_ch.opt_params_sender,
        &config,
    );

    // Initialize Pillar Three first to get regime state for oracle
    let (regime_detector, regime) = SystemInitializer::init_pillar_three(&config);

    let oracle = SystemInitializer::init_oracle(
        oracle_ch.candidate_receiver,
        oracle_ch.oracle_scored_sender,
        regime.clone(),
    )?;

    start_components(
        &mut shutdown,
        ledger,
        tx_monitor,
        perf_monitor,
        optimizer,
        regime_detector,
        oracle,
        ledger_ch.monitor_tx_receiver,
        ooda_ch.opt_params_receiver,
        oracle_ch.oracle_scored_receiver,
        regime,
    );

    tracing::info!("All components started - awaiting shutdown signal");
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down gracefully");
    shutdown.shutdown().await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn start_components(
    shutdown: &mut ShutdownCoordinator,
    ledger: h_5n1p3r::oracle::DecisionLedger,
    monitor: h_5n1p3r::oracle::TransactionMonitor,
    perf: h_5n1p3r::oracle::PerformanceMonitor,
    opt: h_5n1p3r::oracle::StrategyOptimizer,
    regime_det: h_5n1p3r::oracle::MarketRegimeDetector,
    oracle: Arc<h_5n1p3r::oracle::PredictiveOracle>,
    mon_rx: tokio::sync::mpsc::Receiver<h_5n1p3r::oracle::MonitoredTransaction>,
    mut opt_rx: tokio::sync::mpsc::Receiver<h_5n1p3r::oracle::OptimizedParameters>,
    mut score_rx: tokio::sync::mpsc::Receiver<h_5n1p3r::oracle::quantum_oracle::ScoredCandidate>,
    regime: Arc<RwLock<MarketRegime>>,
) {
    shutdown.add_handle(tokio::spawn(async move {
        ledger.run().await;
    }));
    shutdown.add_handle(tokio::spawn(async move {
        monitor.run(mon_rx).await;
    }));
    shutdown.add_handle(tokio::spawn(async move {
        perf.run().await;
    }));
    shutdown.add_handle(tokio::spawn(async move {
        opt.run().await;
    }));
    shutdown.add_handle(tokio::spawn(async move {
        regime_det.run().await;
    }));

    shutdown.add_handle(tokio::spawn(async move {
        while let Some(params) = opt_rx.recv().await {
            tracing::info!(
                "OODA: Parameters updated. Regime: {:?}",
                *regime.read().await
            );
            if let Err(e) = oracle
                .update_config(params.new_weights, params.new_thresholds)
                .await
            {
                tracing::error!("Hot-swap failed: {}", e);
            }
        }
    }));

    shutdown.add_handle(tokio::spawn(async move {
        while let Some(s) = score_rx.recv().await {
            tracing::debug!("Scored: {} -> {}", s.mint, s.predicted_score);
        }
    }));
}
