//! Jito Batch Processor - Bundle Submission and Revolver Integration
//!
//! This module handles:
//! - Converting SwapPlans to SwapIntents
//! - Jito bundle batch submission with redundancy
//! - CRITICAL: Creating Revolver magazines after successful Jito execution
//! - Magazine loading for exit strategy (TP/SL bullets)

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::events::EventEmitter;
use crate::execution::backend::{
    CandidateRef, EntryStalePolicy, EntryTimingSource, ExecutionAttemptContext, ExecutionBackend,
    FillStatus as ExecFillStatus, Lane, MIN_SHADOW_TX_BUILD_COMPENSATION_MS,
};
use crate::execution::live::LiveBackend;
use crate::guardian::post_buy::{
    engine::{PositionEventContext, PositionJoinMetadata},
    MonitoringEngine,
};
use crate::jito_bundle::{JitoBundleExecutor, SwapIntent};
use crate::leader_predictor::LeaderPredictor;
use crate::metrics::E2EMetrics;
use crate::oracle::SnapshotEngine;
use crate::quotes::provider::ExecutableQuoteProvider;

use ghost_core::swap_plan::SwapPlan;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use trigger::{
    create_magazine_after_buy, register_shot_context, DirectBuyBuilder, MagazineConfig, Revolver,
};

use super::E2EPipeline;

impl E2EPipeline {
    /// Process a batch of SwapPlans through Jito Bundle Executor
    ///
    /// This helper function is called from the Jito batching loop to submit
    /// a batch of SwapPlans as Jito bundles with redundancy.
    ///
    /// CRITICAL: After successful submission, creates Revolver magazines for exit strategy.
    pub(super) async fn process_jito_batch(
        batch: &[SwapPlan],
        executor: &Arc<JitoBundleExecutor>,
        tracking_id_counter: &mut u64,
        leader_predictor: &Option<Arc<LeaderPredictor>>,
        redundancy_factor: u32,
        metrics: &Arc<E2EMetrics>,
        payer: &Keypair,
        rpc_client: &Arc<solana_client::nonblocking::rpc_client::RpcClient>,
        revolver: &Arc<RwLock<Revolver>>,
        post_buy_guardian: &Option<Arc<MonitoringEngine>>,
        live_backend: Option<&Arc<LiveBackend>>,
        event_emitter: Option<Arc<EventEmitter>>,
        snapshot_engine: &Arc<SnapshotEngine>,
        quote_provider: &Arc<tokio::sync::RwLock<ExecutableQuoteProvider>>,
        quote_max_age_ms: u64,
    ) {
        if batch.is_empty() {
            return;
        }

        let start = Instant::now();
        let mut prepared_attempts: Vec<ExecutionAttemptContext> = Vec::with_capacity(batch.len());

        // Convert SwapPlans to SwapIntents
        let mut intents: Vec<Arc<SwapIntent>> = Vec::new();

        for swap_plan in batch {
            let current_slot = rpc_client.get_slot().await.ok();
            let predicted_slot = if let Some(ref predictor) = leader_predictor {
                if let Some((_, slot)) = predictor.find_nearest_leader() {
                    Some(slot)
                } else {
                    Some(predictor.current_slot().saturating_add(1))
                }
            } else {
                None
            };

            // Create SwapIntent with unique tracking ID
            *tracking_id_counter += 1;

            let priority = if let Some(ref metadata) = swap_plan.metadata {
                match metadata.risk_level {
                    ghost_core::swap_plan::RiskLevel::Low => 1.0,
                    ghost_core::swap_plan::RiskLevel::Medium => 0.75,
                    ghost_core::swap_plan::RiskLevel::High => 0.5,
                    ghost_core::swap_plan::RiskLevel::VeryHigh => 0.25,
                }
            } else {
                0.75
            };

            let token_mint = swap_plan
                .metadata
                .as_ref()
                .map(|m| m.token_mint)
                .unwrap_or_else(|| Pubkey::default());

            let submit_time_ms = EventEmitter::now_ms();
            let candidate_id = Self::candidate_id_for_swap_plan(swap_plan);
            let prepared_quote = Self::resolve_quote_ref_with_provider(
                quote_provider,
                snapshot_engine,
                swap_plan,
                &token_mint,
                submit_time_ms,
                quote_max_age_ms,
                current_slot,
                EntryStalePolicy::EmitWarning,
            )
            .await;
            let candidate_ref = CandidateRef {
                candidate_id: candidate_id.clone(),
                base_mint: token_mint,
                pool_amm_id: swap_plan.pool_amm_id,
                entry_amount_lamports: swap_plan.amount_in,
                min_tokens_out: swap_plan.min_amount_out,
            };
            let order_id = if let Some(backend) = live_backend {
                backend.reserve_entry_order_id()
            } else {
                format!("entry-jito-{}", *tracking_id_counter)
            };
            let prepared_entry = Self::build_prepared_entry_execution(
                order_id,
                candidate_ref,
                1,
                prepared_quote,
                submit_time_ms,
                EntryTimingSource::LiveJito,
                predicted_slot,
            );
            let prepared_attempt = ExecutionAttemptContext::new(
                prepared_entry,
                current_slot,
                MIN_SHADOW_TX_BUILD_COMPENSATION_MS,
            );
            let intent = Arc::new(SwapIntent::new(
                swap_plan.authority,
                swap_plan.pool_amm_id,
                swap_plan.amount_in,
                swap_plan.min_amount_out,
                swap_plan.timeout,
                priority,
                prepared_attempt
                    .timing
                    .predicted_slot
                    .unwrap_or_else(|| predicted_slot.unwrap_or(1)),
                token_mint,
                *tracking_id_counter,
            ));
            if let Some(backend) = live_backend {
                if let Err(e) = backend
                    .submit_prepared_entry(prepared_attempt.clone())
                    .await
                {
                    warn!(
                        target: "jito_batch",
                        "LiveBackend submit_prepared_entry failed for candidate {}: {}",
                        candidate_id, e
                    );
                }
            }
            prepared_attempts.push(prepared_attempt);

            intents.push(intent);
        }

        info!(
            target: "jito_batch",
            "Submitting batch of {} intents with redundancy N+{}",
            intents.len(), redundancy_factor
        );

        if let Some(ref emitter) = event_emitter {
            for (idx, _swap_plan) in batch.iter().enumerate() {
                if let Some(attempt) = prepared_attempts.get(idx) {
                    let prepared = &attempt.prepared;
                    Self::emit_prepared_entry_submitted(
                        emitter,
                        prepared,
                        Some(serde_json::json!({
                            "path": "jito_bundle",
                            "redundancy_factor": redundancy_factor,
                            "quote_price_ref": prepared.quote.quote_price_ref,
                            "timing_source": prepared.timing_source.as_str(),
                            "timing_path": attempt.timing.timing_path.as_str(),
                            "planned_settle_time_ms": attempt.timing.planned_settle_time_ms,
                            "compensation_ms": attempt.timing.compensation_ms,
                            "price_source": prepared.quote.price_source.as_str(),
                        })),
                    );
                }
            }
        }

        // Execute batch through Jito
        match executor
            .trigger_batch_jito(&intents, redundancy_factor)
            .await
        {
            Ok(results) => {
                let bundle_count = results.len();
                let total_txs: usize = results.iter().map(|r| r.tx_count).sum();
                let total_tip: u64 = results.iter().map(|r| r.total_tip).sum();

                info!(
                    target: "jito_batch",
                    "✅ Jito batch submitted: {} bundles, {} total transactions, {} lamports in tips",
                    bundle_count, total_txs, total_tip
                );

                // Update metrics
                metrics.trigger_txs_sent.inc_by(intents.len() as f64);
                metrics
                    .trigger_send_latency
                    .observe(start.elapsed().as_millis() as f64);
                metrics.jito_bundles_submitted.inc_by(bundle_count as f64);
                metrics.jito_total_tips_paid.inc_by(total_tip as f64);
                metrics
                    .jito_bundle_latency
                    .observe(start.elapsed().as_millis() as f64);

                // In production, track confirmations via Yellowstone
                // For now, we assume success after a delay
                tokio::time::sleep(Duration::from_secs(2)).await;
                metrics.trigger_txs_confirmed.inc_by(intents.len() as f64);
                metrics.jito_bundles_confirmed.inc_by(bundle_count as f64);
                metrics
                    .trigger_confirm_latency
                    .observe(start.elapsed().as_millis() as f64);

                if let Some(backend) = live_backend {
                    for (idx, swap_plan) in batch.iter().enumerate() {
                        if let Some(attempt) = prepared_attempts.get(idx) {
                            let prepared = &attempt.prepared;
                            let fill_time_ms = EventEmitter::now_ms();
                            let fill_price = Self::effective_fill_price(
                                swap_plan.amount_in,
                                swap_plan.min_amount_out,
                            );
                            let fill_status = if prepared.quote.is_stale {
                                ExecFillStatus::Stale
                            } else {
                                ExecFillStatus::Unknown
                            };
                            if let Err(e) = backend
                                .complete_order(
                                    &prepared.order_id,
                                    fill_status,
                                    fill_price,
                                    swap_plan.min_amount_out,
                                    prepared.quote.quote_id.clone(),
                                    fill_time_ms,
                                    None,
                                )
                                .await
                            {
                                warn!(
                                    target: "jito_batch",
                                    "LiveBackend complete_order failed for {}: {}",
                                    prepared.order_id, e
                                );
                            }
                        }
                    }
                }

                if let Some(ref emitter) = event_emitter {
                    let polled = if let Some(backend) = live_backend {
                        backend.poll_fills(EventEmitter::now_ms()).await
                    } else {
                        Vec::new()
                    };
                    let mut fills_by_order = std::collections::HashMap::new();
                    for fill in polled {
                        fills_by_order.insert(fill.order_id.clone(), fill);
                    }
                    for (idx, swap_plan) in batch.iter().enumerate() {
                        if let Some(attempt) = prepared_attempts.get(idx) {
                            let prepared = &attempt.prepared;
                            let fallback_fill_time_ms = EventEmitter::now_ms();
                            let fallback_latency_ms =
                                fallback_fill_time_ms.saturating_sub(prepared.submit_time_ms);
                            let fallback_fill_price = Self::effective_fill_price(
                                swap_plan.amount_in,
                                swap_plan.min_amount_out,
                            );
                            let fallback_fill_status = if prepared.quote.is_stale {
                                ExecFillStatus::Stale
                            } else {
                                ExecFillStatus::Unknown
                            };
                            let fill = fills_by_order.remove(&prepared.order_id);
                            Self::emit_prepared_entry_filled(
                                emitter,
                                prepared,
                                fill.as_ref()
                                    .map(|f| f.quote_id_used.as_str())
                                    .unwrap_or(&prepared.quote.quote_id),
                                fill.as_ref()
                                    .map(|f| f.fill_time_ms)
                                    .unwrap_or(fallback_fill_time_ms),
                                fill.as_ref()
                                    .map(|f| f.fill_price)
                                    .unwrap_or(fallback_fill_price),
                                fill.as_ref()
                                    .map(|f| f.fill_qty)
                                    .unwrap_or(swap_plan.min_amount_out),
                                fill.as_ref()
                                    .map(|f| f.status)
                                    .unwrap_or(fallback_fill_status),
                                fill.as_ref()
                                    .map(|f| f.latency_ms)
                                    .unwrap_or(fallback_latency_ms),
                            );
                            if prepared.quote.is_stale
                                && matches!(
                                    prepared.quote.stale_policy,
                                    EntryStalePolicy::EmitWarning
                                )
                            {
                                Self::emit_quote_stale_events(
                                    emitter,
                                    prepared.candidate_id(),
                                    &prepared.order_id,
                                    &prepared.quote.quote_id,
                                    prepared.quote.slot,
                                    fill.as_ref()
                                        .map(|f| f.fill_time_ms)
                                        .unwrap_or(fallback_fill_time_ms),
                                    prepared.quote.stale_age_ms,
                                    quote_max_age_ms,
                                );
                            }
                        }
                    }
                }

                // Log Jito-specific stats
                let stats = executor.get_stats();
                info!(
                    target: "jito_stats",
                    "Jito Executor Stats: total_intents={}, total_bundles={}, total_txs={}, total_tips={} lamports, inclusion_rate={:.2}%",
                    stats.total_intents, stats.total_bundles, stats.total_transactions,
                    stats.total_tip_paid, stats.inclusion_rate * 100.0
                );

                // === CRITICAL FIX: CREATE REVOLVER MAGAZINES FOR JITO POSITIONS ===
                // After successful Jito submission, create exit strategy (magazines) for each position
                // This ensures that tokens bought via Jito have TP/SL bullets ready
                info!(
                    target: "jito_revolver",
                    "Creating Revolver magazines for {} Jito positions",
                    batch.len()
                );

                for (idx, swap_plan) in batch.iter().enumerate() {
                    // Extract token mint from metadata
                    let token_mint = if let Some(ref metadata) = swap_plan.metadata {
                        metadata.token_mint
                    } else {
                        warn!(
                            "Jito position missing metadata (pool={}). Skipping magazine creation - NO EXIT STRATEGY!",
                            swap_plan.pool_amm_id
                        );
                        continue;
                    };

                    let position_size = swap_plan.amount_in;

                    // Calculate entry price from swap plan
                    let entry_price = if swap_plan.min_amount_out > 0 {
                        (swap_plan.amount_in as f64 / swap_plan.min_amount_out as f64 * 1_000_000.0)
                            as u64
                    } else {
                        1000_u64
                    };

                    info!(
                        target: "jito_revolver",
                        "Spawning magazine creation for Jito position: mint={}, size={}, price={}",
                        token_mint, position_size, entry_price
                    );

                    // Clone data for background task
                    let payer_bytes = payer.to_bytes();
                    let rpc_client_clone = Arc::clone(rpc_client);
                    let revolver_clone = Arc::clone(revolver);

                    // Use Pump.fun program ID for magazine config
                    let pump_program_id = DirectBuyBuilder::pump_program_id();

                    // Spawn magazine creation in background (non-blocking)
                    tokio::spawn(async move {
                        let payer_bg = match Keypair::from_bytes(&payer_bytes) {
                            Ok(kp) => kp,
                            Err(e) => {
                                error!("Failed to reconstruct keypair for Jito magazine: {}", e);
                                return;
                            }
                        };

                        let magazine_config = MagazineConfig::default_targets(pump_program_id);

                        match create_magazine_after_buy(
                            &payer_bg,
                            token_mint,
                            position_size,
                            entry_price,
                            &magazine_config,
                            &rpc_client_clone,
                        )
                        .await
                        {
                            Ok(bullets) => {
                                info!(
                                    target: "jito_revolver",
                                    "✅ Created {} SELL bullets for Jito position: mint={}",
                                    bullets.len(), token_mint
                                );

                                // Load magazine into Revolver
                                {
                                    let mut revolver_guard = revolver_clone.write().await;
                                    revolver_guard.load_magazine(token_mint, bullets);

                                    let active_positions = revolver_guard.get_active_mints().len();
                                    info!(
                                        target: "jito_revolver",
                                        "✅ Magazine loaded into Revolver. Active positions: {}",
                                        active_positions
                                    );
                                }

                                info!(
                                    target: "jito_revolver",
                                    "✅ Jito position lifecycle armed: BUY complete, SELL bullets ready"
                                );
                            }
                            Err(e) => {
                                error!(
                                    target: "jito_revolver",
                                    "❌ Failed to create magazine for Jito position {}: {}. NO EXIT STRATEGY!",
                                    token_mint, e
                                );
                            }
                        }
                    });

                    // === POSTBUY GUARDIAN: Register position for monitoring ===
                    if let Some(ref guardian) = post_buy_guardian {
                        let entry_price_f64 = if swap_plan.min_amount_out > 0 {
                            Some(swap_plan.amount_in as f64 / swap_plan.min_amount_out as f64)
                        } else {
                            None
                        };

                        let context =
                            prepared_attempts
                                .get(idx)
                                .map(|attempt| PositionEventContext {
                                    join_metadata: PositionJoinMetadata::default(),
                                    candidate_id: attempt.prepared.candidate_id().to_string(),
                                    entry_order_id: attempt.prepared.order_id.clone(),
                                    quote_id: attempt.prepared.quote.quote_id.clone(),
                                    slot: attempt.prepared.quote.slot,
                                    lane: event_emitter
                                        .as_ref()
                                        .map(|emitter| emitter.lane())
                                        .unwrap_or(Lane::Live),
                                    position_id: None,
                                    position_epoch: None,
                                });
                        let candidate_for_context = context
                            .as_ref()
                            .map(|ctx| ctx.candidate_id.clone())
                            .unwrap_or_else(|| Self::candidate_id_for_swap_plan(swap_plan));

                        let registered = guardian.register_position_with_context(
                            swap_plan.pool_amm_id,
                            token_mint,
                            swap_plan.pool_amm_id, // bonding_curve ≈ pool_amm_id on Pump.fun
                            entry_price_f64,
                            Some(swap_plan.amount_in),
                            None,
                            context,
                        );

                        if let Some(registered_position) = registered {
                            register_shot_context(
                                token_mint,
                                candidate_for_context,
                                registered_position.position_id.clone(),
                            );
                            info!(
                                target: "jito_revolver",
                                "🛡️ PostBuyGuardian: Jito position registered — mint={} pool={}",
                                token_mint, swap_plan.pool_amm_id
                            );
                        }
                    }
                }
            }
            Err(e) => {
                if let Some(backend) = live_backend {
                    for attempt in &prepared_attempts {
                        let prepared = &attempt.prepared;
                        let fill_time_ms = EventEmitter::now_ms();
                        let fill_status = if prepared.quote.is_stale {
                            ExecFillStatus::Stale
                        } else {
                            ExecFillStatus::Failed
                        };
                        if let Err(err) = backend
                            .complete_order(
                                &prepared.order_id,
                                fill_status,
                                0.0,
                                0,
                                prepared.quote.quote_id.clone(),
                                fill_time_ms,
                                None,
                            )
                            .await
                        {
                            warn!(
                                target: "jito_batch",
                                "LiveBackend complete_order failed for {}: {}",
                                prepared.order_id, err
                            );
                        }
                    }
                }
                if let Some(ref emitter) = event_emitter {
                    let polled = if let Some(backend) = live_backend {
                        backend.poll_fills(EventEmitter::now_ms()).await
                    } else {
                        Vec::new()
                    };
                    let mut fills_by_order = std::collections::HashMap::new();
                    for fill in polled {
                        fills_by_order.insert(fill.order_id.clone(), fill);
                    }
                    for attempt in &prepared_attempts {
                        let prepared = &attempt.prepared;
                        let fallback_fill_time_ms = EventEmitter::now_ms();
                        let fallback_fill_status = if prepared.quote.is_stale {
                            ExecFillStatus::Stale
                        } else {
                            ExecFillStatus::Failed
                        };
                        let fill = fills_by_order.remove(&prepared.order_id);
                        Self::emit_prepared_entry_filled(
                            emitter,
                            prepared,
                            fill.as_ref()
                                .map(|f| f.quote_id_used.as_str())
                                .unwrap_or(&prepared.quote.quote_id),
                            fill.as_ref()
                                .map(|f| f.fill_time_ms)
                                .unwrap_or(fallback_fill_time_ms),
                            fill.as_ref().map(|f| f.fill_price).unwrap_or(0.0),
                            fill.as_ref().map(|f| f.fill_qty).unwrap_or(0),
                            fill.as_ref()
                                .map(|f| f.status)
                                .unwrap_or(fallback_fill_status),
                            fill.as_ref().map(|f| f.latency_ms).unwrap_or(
                                fallback_fill_time_ms.saturating_sub(prepared.submit_time_ms),
                            ),
                        );
                        if prepared.quote.is_stale
                            && matches!(prepared.quote.stale_policy, EntryStalePolicy::EmitWarning)
                        {
                            Self::emit_quote_stale_events(
                                emitter,
                                prepared.candidate_id(),
                                &prepared.order_id,
                                &prepared.quote.quote_id,
                                prepared.quote.slot,
                                fill.as_ref()
                                    .map(|f| f.fill_time_ms)
                                    .unwrap_or(fallback_fill_time_ms),
                                prepared.quote.stale_age_ms,
                                quote_max_age_ms,
                            );
                        }
                    }
                }
                error!(
                    target: "jito_batch",
                    "❌ Failed to submit Jito batch: {}. Batch size: {}", e, intents.len()
                );
                metrics.trigger_txs_failed.inc_by(intents.len() as f64);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventValidator, EventWriterConfig};
    use crate::quotes::provider::{ExecutableQuoteProvider, QuoteProviderConfig};
    use ghost_core::swap_plan::SwapPlan;
    use solana_sdk::pubkey::Pubkey;
    use tempfile::tempdir;

    fn make_swap_plan(seed: u64) -> SwapPlan {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        SwapPlan::new(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            1_000_000 + seed * 1_000,
            500_000 + seed * 500,
            now_secs + 60,
        )
    }

    #[tokio::test]
    async fn test_live_smoke_gate_jito_same_inputs_same_decisions_events_only_appended() {
        let batch = vec![make_swap_plan(1), make_swap_plan(2), make_swap_plan(3)];

        let keypair = Arc::new(Keypair::new());
        let executor = Arc::new(JitoBundleExecutor::new(
            "https://amsterdam.mainnet.block-engine.jito.wtf".to_string(),
            Arc::clone(&keypair),
        ));
        let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
            "http://127.0.0.1:8899".to_string(),
        ));
        let revolver = Arc::new(RwLock::new(Revolver::new()));
        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let quote_provider = Arc::new(tokio::sync::RwLock::new(ExecutableQuoteProvider::new(
            QuoteProviderConfig::default(),
        )));

        // Baseline: live path without event emitter.
        let mut tracking_baseline = 0_u64;
        let baseline_metrics = Arc::new(E2EMetrics::new());
        E2EPipeline::process_jito_batch(
            &batch,
            &executor,
            &mut tracking_baseline,
            &None,
            2,
            &baseline_metrics,
            keypair.as_ref(),
            &rpc_client,
            &revolver,
            &None,
            None,
            None,
            &snapshot_engine,
            &quote_provider,
            1500,
        )
        .await;
        let baseline_stats = executor.get_stats();
        let baseline_sent = baseline_metrics.trigger_txs_sent.get();
        let baseline_confirmed = baseline_metrics.trigger_txs_confirmed.get();
        let baseline_failed = baseline_metrics.trigger_txs_failed.get();

        // Instrumented: same inputs + emitter enabled.
        executor.reset_stats();
        let mut tracking_instrumented = 0_u64;
        let instrumented_metrics = Arc::new(E2EMetrics::new());
        let temp_dir = tempdir().expect("temp dir");
        let writer_cfg = EventWriterConfig {
            output_dir: temp_dir.path().to_string_lossy().to_string(),
            enable_optional_events: true,
            ..Default::default()
        };
        let emitter = Arc::new(
            EventEmitter::new(
                writer_cfg,
                "smoke-live-run".to_string(),
                crate::execution::Lane::Live,
            )
            .expect("create emitter"),
        );
        E2EPipeline::process_jito_batch(
            &batch,
            &executor,
            &mut tracking_instrumented,
            &None,
            2,
            &instrumented_metrics,
            keypair.as_ref(),
            &rpc_client,
            &revolver,
            &None,
            None,
            Some(Arc::clone(&emitter)),
            &snapshot_engine,
            &quote_provider,
            1500,
        )
        .await;
        let instrumented_stats = executor.get_stats();
        let instrumented_sent = instrumented_metrics.trigger_txs_sent.get();
        let instrumented_confirmed = instrumented_metrics.trigger_txs_confirmed.get();
        let instrumented_failed = instrumented_metrics.trigger_txs_failed.get();

        // Smoke gate: same inputs -> same decisions/outcomes, no new runtime errors.
        assert_eq!(tracking_baseline, tracking_instrumented);
        assert_eq!(
            baseline_stats.total_intents,
            instrumented_stats.total_intents
        );
        assert_eq!(
            baseline_stats.total_bundles,
            instrumented_stats.total_bundles
        );
        assert_eq!(
            baseline_stats.total_transactions,
            instrumented_stats.total_transactions
        );
        assert_eq!(
            baseline_stats.total_tip_paid,
            instrumented_stats.total_tip_paid
        );
        assert_eq!(baseline_sent, instrumented_sent);
        assert_eq!(baseline_confirmed, instrumented_confirmed);
        assert_eq!(baseline_failed, instrumented_failed);

        // Events must be appended only in instrumented run and pass schema/invariant validator.
        emitter.flush().expect("flush emitter");
        let total_events = emitter.total_events_written();
        assert!(total_events > 0, "instrumented run must emit events");
        let event_file = emitter
            .shared_writer()
            .lock()
            .unwrap()
            .current_file_path()
            .map(|p| p.to_path_buf())
            .expect("event file path");
        let validation = EventValidator::validate_jsonl(&event_file).expect("validate events");
        assert!(
            validation.invariant_violations.is_empty(),
            "smoke event invariants failed: {:?}",
            validation.invariant_violations
        );
    }
}
