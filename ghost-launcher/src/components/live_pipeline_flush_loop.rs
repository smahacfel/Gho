//! Live Pipeline Flush Loop - Periodic Flush to ShadowLedger
//!
//! This module provides a background task that periodically flushes buffered
//! live transactions from LivePipeline to ShadowLedger.
//!
//! ## Responsibilities
//!
//! - Periodically call flush_ready() on LivePipeline
//! - Process flush results (TradeSnapshots and MarketSnapshots)
//! - Emit metrics for events processed and flushes
//! - Handle errors gracefully with logging
//!
//! ## Integration
//!
//! This loop runs as a background task spawned from main.rs and ensures
//! that live transactions are continuously appended to ShadowLedger with
//! proper ordering and deduplication.

use ghost_core::shadow_ledger::{LivePipeline, ShadowLedger};
use metrics::increment_counter;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Configuration for the LivePipeline flush loop.
#[derive(Clone, Debug)]
pub struct LivePipelineFlushLoopConfig {
    /// How often to flush buffers (in milliseconds).
    pub flush_interval_ms: u64,
}

impl Default for LivePipelineFlushLoopConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 50, // Flush every 50ms (matches LivePipeline default)
        }
    }
}

/// Run the LivePipeline flush loop.
///
/// This task periodically flushes buffered live transactions from the
/// LivePipeline to ShadowLedger, ensuring continuous snapshot updates
/// with deterministic ordering.
///
/// # Arguments
///
/// * `live_pipeline` - Pipeline for post-commit live transaction processing
/// * `shadow_ledger` - Ledger for storing snapshots
/// * `mut shutdown_rx` - Shutdown signal receiver
/// * `config` - Loop configuration
pub async fn run(
    live_pipeline: Arc<LivePipeline>,
    shadow_ledger: Arc<ShadowLedger>,
    mut shutdown_rx: broadcast::Receiver<()>,
    config: LivePipelineFlushLoopConfig,
) {
    info!(
        "💧 LivePipelineFlushLoop: Starting (flush_interval={}ms)",
        config.flush_interval_ms
    );

    let mut interval = tokio::time::interval(Duration::from_millis(config.flush_interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut total_flushes = 0u64;
    let mut total_snapshots_appended = 0u64;
    let mut last_log = std::time::Instant::now();

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                let stats = live_pipeline.stats();
                info!("💧 LivePipelineFlushLoop: Shutdown signal received");
                info!("   Total flushes: {}, Total events: {}, Total snapshots: {}",
                    total_flushes, stats.total_events_processed, total_snapshots_appended);
                break;
            }
            _ = interval.tick() => {
                let flush_started = std::time::Instant::now();
                // Flush all ready buffers.
                // NOTE: `flush_ready()` returns total snapshots appended across all mints.
                let appended_snapshots = live_pipeline.flush_ready(&shadow_ledger);
                metrics::histogram!(
                    "live_pipeline_flush_latency_ms",
                    flush_started.elapsed().as_secs_f64() * 1000.0
                );
                if appended_snapshots > 0 {
                    total_flushes += 1;
                    total_snapshots_appended += appended_snapshots as u64;

                    debug!(
                        "💧 LivePipelineFlushLoop: Appended {} snapshots (total_snapshots_appended={})",
                        appended_snapshots,
                        total_snapshots_appended
                    );

                    increment_counter!("live_pipeline_flush_total");
                    for _ in 0..appended_snapshots {
                        increment_counter!("live_pipeline_snapshots_appended_total");
                    }
                }

                // Log stats periodically
                if last_log.elapsed().as_secs() >= 60 {
                    let stats = live_pipeline.stats();
                    info!(
                        "💧 LivePipelineFlushLoop: Stats - flushes={}, events={}, snapshots={}, active_mints={}",
                        total_flushes,
                        stats.total_events_processed,
                        total_snapshots_appended,
                        stats.active_mints
                    );
                    last_log = std::time::Instant::now();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_flush_loop_starts_and_stops() {
        let pipeline = Arc::new(LivePipeline::new());
        let ledger = Arc::new(ShadowLedger::new());
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let config = LivePipelineFlushLoopConfig {
            flush_interval_ms: 25,
        };

        let pipeline_clone = Arc::clone(&pipeline);
        let ledger_clone = Arc::clone(&ledger);

        let handle = tokio::spawn(async move {
            run(pipeline_clone, ledger_clone, shutdown_rx, config).await;
        });

        // Let it run for a bit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Shutdown
        handle.abort();
    }
}
