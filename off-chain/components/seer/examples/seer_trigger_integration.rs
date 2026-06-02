//! Example: Integrated Seer→Trigger with IPC
//!
//! This example demonstrates how to run Seer and Trigger together
//! with the IPC layer for communication.
//!
//! Run with:
//! ```
//! cargo run --example seer_trigger_integration
//! ```

use seer::config::SeerConfig;
use seer::ipc::*;
use seer::Seer;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use trigger::{
    ipc_integration::{
        CandidatePool as TriggerCandidatePool, DetectedPoolEvent as TriggerDetectedPoolEvent,
        EventPriority as TriggerEventPriority,
    },
    IpcEventProcessor, ProcessorConfig,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("=== Seer→Trigger IPC Integration Example ===");

    // Configure IPC channel
    let ipc_config = IpcChannelConfig {
        buffer_size: 1000,
        backpressure_policy: BackpressurePolicy::Block,
        log_drops: true,
        log_overflows: true,
        warning_threshold_percent: 80.0,
    };

    info!("IPC Configuration:");
    info!("  Buffer size: {}", ipc_config.buffer_size);
    info!(
        "  Backpressure policy: {:?}",
        ipc_config.backpressure_policy
    );

    // Create IPC channel
    let (ipc_sender, mut ipc_receiver, ipc_metrics) = create_ipc_channel(ipc_config.clone());

    // Configure and create Seer
    let mut seer_config = SeerConfig::default();
    seer_config.ipc_config = ipc_config.clone();

    info!("Creating Seer instance...");
    let seer = Arc::new(Seer::new_with_ipc(seer_config, ipc_sender));

    // Configure Trigger event processor
    let processor_config = ProcessorConfig {
        prioritize_by_priority: true,
        target_latency_ms: 100,
        skip_stale_events: true,
        stale_threshold_secs: 5,
        max_concurrent_positions: 3,
    };
    let event_processor = Arc::new(IpcEventProcessor::new(processor_config));

    info!("Starting Seer in background...");
    // Start Seer in background
    let seer_handle = {
        let seer = Arc::clone(&seer);
        tokio::spawn(async move {
            loop {
                match Arc::clone(&seer).run().await {
                    Ok(()) => {
                        info!("Seer completed normally");
                        break;
                    }
                    Err(e) => {
                        error!("Seer error: {}, restarting in 10s...", e);
                        sleep(Duration::from_secs(10)).await;
                    }
                }
            }
        })
    };

    info!("Starting Trigger event processing loop...");
    // Start Trigger event processing
    let ipc_metrics_clone = ipc_metrics.clone();
    let ipc_config_for_trigger = ipc_config.clone();

    let trigger_handle = tokio::spawn(async move {
        let mut event_count = 0u64;

        while let Some(event) = ipc_receiver.recv().await {
            event_count += 1;

            match &event {
                SeerEvent::PoolDetected(evt) => {
                    info!(
                        "Trigger received event #{}: pool={}, priority={:?}, seq={}",
                        event_count, evt.candidate.pool_amm_id, evt.priority, evt.sequence_number
                    );

                    let trigger_evt = TriggerDetectedPoolEvent {
                        candidate: TriggerCandidatePool {
                            slot: evt.candidate.slot,
                            signature: evt.candidate.signature.clone(),
                            amm_program_id: evt.candidate.amm_program_id,
                            pool_amm_id: evt.candidate.pool_amm_id,
                            base_mint: evt.candidate.base_mint,
                            quote_mint: evt.candidate.quote_mint,
                            bonding_curve: evt.candidate.bonding_curve,
                            creator: evt.candidate.creator,
                            timestamp: evt.candidate.timestamp,
                            bonding_curve_progress: evt.candidate.bonding_curve_progress,
                            initial_liquidity_sol: evt.candidate.initial_liquidity_sol,
                            token_total_supply: evt.candidate.token_total_supply,
                            block_time: evt.candidate.block_time,
                        },
                        detected_at: evt.detected_at,
                        sequence_number: evt.sequence_number,
                        priority: match evt.priority {
                            seer::ipc::EventPriority::High => TriggerEventPriority::High,
                            seer::ipc::EventPriority::Normal => TriggerEventPriority::Normal,
                            seer::ipc::EventPriority::Low => TriggerEventPriority::Low,
                        },
                    };

                    match event_processor.process_event(trigger_evt).await {
                        Ok(result) => {
                            info!("Event processed successfully: {:?}", result);
                        }
                        Err(e) => {
                            error!("Failed to process event: {}", e);
                        }
                    }
                }
                SeerEvent::Trade(trade_evt) => {
                    info!(
                        "Trigger received trade event #{}: mint={}, is_buy={}, seq={}",
                        event_count,
                        trade_evt.trade.mint,
                        trade_evt.trade.is_buy,
                        trade_evt.sequence_number
                    );
                }
                SeerEvent::FundingTransfer(funding_evt) => {
                    info!(
                        "Trigger received funding transfer #{}: source={} recipient={} lamports={} seq={}",
                        event_count,
                        funding_evt.transfer.source_wallet,
                        funding_evt.transfer.recipient_wallet,
                        funding_evt.transfer.lamports,
                        funding_evt.sequence_number
                    );
                }
                SeerEvent::AccountUpdate(account_evt) => {
                    info!(
                        "Trigger received account update #{}: mint={}, curve={}, finality={}",
                        event_count,
                        account_evt.base_mint,
                        account_evt.bonding_curve,
                        account_evt.curve_finality.as_str()
                    );
                }
                SeerEvent::ExecutionAccountEvidence(evidence_evt) => {
                    info!(
                        "Trigger received execution account evidence #{}: account={}, role={}, status={}",
                        event_count,
                        evidence_evt.evidence.account_pubkey,
                        evidence_evt.evidence.role.label(),
                        evidence_evt.evidence.status.as_str()
                    );
                }
            }

            // Log IPC metrics periodically
            if event_count % 10 == 0 {
                let queue_util = ipc_metrics_clone
                    .calculate_queue_utilization(ipc_config_for_trigger.buffer_size);
                let drop_rate = ipc_metrics_clone.calculate_drop_rate();

                info!(
                    "IPC Metrics after {} events: queue={:.1}%, drops={:.2}%, latency_p50={:.1}ms",
                    event_count,
                    queue_util,
                    drop_rate,
                    0.0 // Would need to calculate from histogram
                );
            }
        }

        info!("Trigger event processing loop ended");
    });

    // Wait for both components
    info!("Running integrated system. Press Ctrl+C to stop.");

    tokio::select! {
        _ = seer_handle => {
            info!("Seer stopped");
        }
        _ = trigger_handle => {
            info!("Trigger stopped");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }

    info!("Shutting down...");

    // Print final metrics
    info!("=== Final IPC Metrics ===");
    info!("  Events sent: {}", ipc_metrics.events_sent.get());
    info!("  Events received: {}", ipc_metrics.events_received.get());
    info!("  Events dropped: {}", ipc_metrics.events_dropped.get());
    info!("  Drop rate: {:.2}%", ipc_metrics.calculate_drop_rate());
    info!("  Max queue length: {}", ipc_metrics.queue_length_max.get());

    Ok(())
}
