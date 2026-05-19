//! Pipeline Execution - Direct Trigger with Gatekeeper Logic
//!
//! This module handles:
//! - Direct AMM interaction (Zero-Cost mode)
//! - Gatekeeper logic (position limit enforcement)
//! - Traffic Light logic (Leader Predictor integration for BUY)
//! - Jito batch mode vs Standard mode routing
//! - Magazine creation for Standard mode
//! - Leapfrog TPU injection for direct transaction sending
//! - **AOE (Atomic Optimistic Execution)**: Single-transaction swaps
//! - **Ghost Intelligence**: Post-buy analysis for HODL vs Panic Sell decisions
//! - **HyperOracle**: Early market analysis (T+2s) with SCR, ULVF, POVC

use anyhow::Result;
use ghost_core::swap_plan::SwapPlan;
use solana_sdk::signer::keypair::Keypair;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use trigger::{set_shot_event_sink, DirectBuyBuilder, ShotEvent, ShotEventSink, ShotEventStage};
use uuid::Uuid;

use crate::events::{
    CandidatePayload, EntryFilledPayload, EntrySubmittedPayload, EventEmitter, EventKind,
    ExecutionEvent, OracleStalePayload, PositionOpenedPayload,
};
use crate::execution::backend::{
    CandidateRef, EntryPriceSource, EntryStalePolicy, EntryTimingSource, ExecutionAttemptContext,
    ExecutionBackend, ExecutionMode, FillStatus as ExecFillStatus, Lane, OrderSide,
    PreparedEntryExecution, PreparedQuoteRef, MIN_SHADOW_TX_BUILD_COMPENSATION_MS,
};
use crate::execution::live::LiveBackend;
use crate::execution::paper::{PaperBackend, PaperBroker};
use crate::execution::shadow::ShadowBackend;
use crate::guardian::post_buy::engine::{PositionEventContext, PositionJoinMetadata};
use crate::oracle::MarketSnapshot;
use crate::quotes::provider::{ExecutableQuoteProvider, QuoteSource};

use super::E2EPipeline;

struct PipelineShotTelemetrySink {
    emitter: Arc<EventEmitter>,
}

struct PreparedSwapAttempt {
    attempt: ExecutionAttemptContext,
    estimated_tokens: u64,
    slippage_bps: u64,
}

impl ShotEventSink for PipelineShotTelemetrySink {
    fn on_shot(&self, event: ShotEvent) {
        let (Some(candidate_id), Some(position_id)) =
            (event.candidate_id.clone(), event.position_id.clone())
        else {
            return;
        };

        match event.stage {
            ShotEventStage::Submitted => {
                self.emitter.emit_exit_submitted(
                    &candidate_id,
                    &position_id,
                    &event.order_id,
                    event.fraction_bps,
                    None,
                );
            }
            ShotEventStage::Filled => {
                let status = if event.error.is_none() {
                    ExecFillStatus::Confirmed
                } else {
                    ExecFillStatus::Failed
                };
                self.emitter.emit_exit_filled(
                    &candidate_id,
                    &position_id,
                    &event.order_id,
                    event.observed_price as f64,
                    0,
                    0.0,
                    status,
                    event.fraction_bps < 10_000,
                    0,
                );
            }
        }
    }
}

impl E2EPipeline {
    /// Start Direct Trigger execution (Zero-Cost mode - Direct AMM interaction)
    pub(super) async fn start_trigger(
        &self,
        mut swap_plan_rx: mpsc::Receiver<SwapPlan>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        info!("Starting Direct Trigger component (Zero-Cost mode)");

        let payer = Arc::new(Keypair::from_bytes(&self.payer.to_bytes()).unwrap());

        // Create RPC client for transaction sending and magazine creation
        let rpc_client = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
            self.config.rpc_url.clone(),
        ));

        let metrics = Arc::clone(&self.metrics);
        let execution_mode = self.config.execution.execution_mode;
        let redundancy_factor = self.config.trigger.redundancy_factor;
        let gui_state = self.gui_state.clone();
        let revolver = Arc::clone(&self.revolver);
        let max_concurrent_positions = self.config.trigger.max_concurrent_positions.unwrap_or(3);
        let leader_predictor = self.leader_predictor.clone();
        let jito_executor = self.jito_executor.clone();
        let enable_jito = self.config.trigger.enable_jito;
        let event_emitter = self.execution_event_emitter.clone();
        let secondary_emitter = self.execution_event_emitter_paper.clone();
        let quote_max_age_ms = self.config.execution.quotes.max_quote_age_ms;
        let shadow_quote_max_age_ms = self.config.execution.shadow.max_quote_age_ms;
        let shadow_execution_config = self.config.execution.shadow.clone();
        let dual_mode = matches!(execution_mode, ExecutionMode::Dual);
        let quote_provider = Arc::new(RwLock::new(ExecutableQuoteProvider::new(
            self.config.execution.quotes.clone(),
        )));
        let live_config = crate::execution::live::LiveBackendConfig {
            rpc_client: rpc_client.clone(),
            payer: Arc::new(self.payer.insecure_clone()),
            jito_executor: self.jito_executor.clone(),
            enable_jito,
            redundancy_factor: self.config.trigger.redundancy_factor as usize,
            revolver: self.revolver.clone(),
            leader_predictor: self.leader_predictor.clone(),
            leader_resolver: self.leader_resolver.clone(),
            leapfrog_config: if self.config.trigger.enable_leapfrog {
                Some((self.config.trigger.leapfrog_redundancy as usize, false))
            } else {
                None
            },
            shadow_ledger: self.shadow_ledger.clone(),
            metrics: self.metrics.clone(),
            event_emitter: self.execution_event_emitter.clone(),
            post_buy_guardian: self.post_buy_guardian.clone(),
            quote_provider: Arc::clone(&quote_provider),
            snapshot_engine: self.snapshot_engine.clone(),
            quote_max_age_ms,
        };

        let (live_backend, sim_backend, shadow_backend) = match execution_mode {
            ExecutionMode::Live => (Some(Arc::new(LiveBackend::new(live_config))), None, None),
            ExecutionMode::Paper => (
                None,
                Some(Arc::new(PaperBackend::new(PaperBroker::new(
                    self.config.execution.paper.clone(),
                    Arc::clone(&quote_provider),
                )))),
                None,
            ),
            ExecutionMode::Shadow => (
                None,
                None,
                Some(Arc::new(ShadowBackend::new(
                    shadow_execution_config.clone(),
                ))),
            ),
            ExecutionMode::Dual => (
                Some(Arc::new(LiveBackend::new(live_config))),
                Some(Arc::new(PaperBackend::new(PaperBroker::new(
                    self.config.execution.paper.clone(),
                    Arc::clone(&quote_provider),
                )))),
                None,
            ),
        };

        // Leapfrog configuration
        let enable_leapfrog = self.config.trigger.enable_leapfrog;
        let leapfrog_redundancy = self.config.trigger.leapfrog_redundancy;
        let leapfrog_use_quic = self.config.trigger.leapfrog_use_quic;

        // Leapfrog infrastructure components
        let leader_resolver = self.leader_resolver.clone();
        let rpc_url = self.config.rpc_url.clone();

        // Shared payer for Leapfrog transactions (Arc for safe cross-task sharing)
        let leapfrog_payer = Arc::clone(&payer);

        // Shadow Ledger for zero-latency price calculations and slippage protection
        // Used in DirectBuy to calculate precise min_tokens_out from simulate_buy()
        let shadow_ledger = Arc::clone(&self.shadow_ledger);

        // Ghost Intelligence components for post-buy analysis
        let profiler = Arc::clone(&self.profiler);
        let cluster_hunter = Arc::clone(&self.cluster_hunter);
        let vision_critic = Arc::clone(&self.vision_critic);

        // HyperOracle for early market analysis (T+2s window)
        let hyper_oracle = Arc::clone(&self.hyper_oracle);

        // SnapshotEngine for real-time market snapshot data
        let snapshot_engine = Arc::clone(&self.snapshot_engine);

        // PostBuy Guardian for real-time position monitoring
        let post_buy_guardian = self.post_buy_guardian.clone();
        if let (Some(guardian), Some(shadow_backend)) =
            (post_buy_guardian.as_ref(), shadow_backend.as_ref())
        {
            guardian.attach_shadow_backend(Arc::clone(shadow_backend));
        }

        info!("Execution mode (SSOT): {}", execution_mode);

        info!(
            "Gatekeeper enabled: max_concurrent_positions = {}",
            max_concurrent_positions
        );
        if let Some(ref emitter) = event_emitter {
            let sink: Arc<dyn ShotEventSink> = Arc::new(PipelineShotTelemetrySink {
                emitter: Arc::clone(emitter),
            });
            set_shot_event_sink(Some(sink));
        } else {
            set_shot_event_sink(None);
        }

        let handle = tokio::spawn(async move {
            match execution_mode {
                ExecutionMode::Paper => {
                    info!("PAPER MODE ENABLED - using PaperBackend lane (no on-chain sends)");
                    let Some(sim_backend) = sim_backend else {
                        error!("Paper mode selected but PaperBackend was not initialized");
                        return;
                    };

                    while let Some(swap_plan) = swap_plan_rx.recv().await {
                        if let Some(ref state) = gui_state {
                            if state.is_stopped() {
                                warn!("System is STOPPED - exiting paper trigger loop");
                                break;
                            }

                            if state.is_paused() {
                                info!("System is PAUSED - skipping swap plan (paper)");
                                continue;
                            }
                        }

                        let active_count = revolver.read().await.get_active_mints().len();
                        if active_count >= max_concurrent_positions {
                            warn!(
                                target: "gatekeeper",
                                "⛔ STOP: Position limit reached ({}/{}). Skipping opportunity: pool={}",
                                active_count, max_concurrent_positions, swap_plan.pool_amm_id
                            );
                            metrics.buy_init_failures.inc();
                            continue;
                        }

                        if let Some(ref emitter) = event_emitter {
                            Self::emit_candidate_pass(emitter, &swap_plan, "gatekeeper_paper_pass");
                        }

                        match Self::process_paper_swap_plan(
                            &swap_plan,
                            &sim_backend,
                            &quote_provider,
                            &snapshot_engine,
                            &event_emitter,
                            &post_buy_guardian,
                            quote_max_age_ms,
                            1,
                        )
                        .await
                        {
                            Ok(latency_ms) => {
                                metrics.buy_intents_initialized.inc();
                                metrics.trigger_txs_sent.inc();
                                metrics.trigger_txs_confirmed.inc();
                                metrics.trigger_send_latency.observe(latency_ms as f64);
                                metrics.trigger_confirm_latency.observe(latency_ms as f64);
                            }
                            Err(err) => {
                                warn!(error = %err, "PaperBackend entry processing failed");
                                metrics.buy_init_failures.inc();
                                metrics.trigger_txs_failed.inc();
                            }
                        }
                    }

                    warn!("Paper trigger loop ended");
                }
                ExecutionMode::Shadow => {
                    info!(
                        "SHADOW MODE ENABLED - using ShadowBackend synthetic settlement on prepared-entry truth"
                    );
                    let Some(shadow_backend) = shadow_backend else {
                        error!("Shadow mode selected but ShadowBackend was not initialized");
                        return;
                    };

                    while let Some(swap_plan) = swap_plan_rx.recv().await {
                        if let Some(ref state) = gui_state {
                            if state.is_stopped() {
                                warn!("System is STOPPED - exiting shadow trigger loop");
                                break;
                            }

                            if state.is_paused() {
                                info!("System is PAUSED - skipping swap plan (shadow)");
                                continue;
                            }
                        }

                        let active_count = shadow_backend.position_count().await;
                        if active_count >= max_concurrent_positions {
                            warn!(
                                target: "gatekeeper",
                                "⛔ STOP: Shadow position limit reached ({}/{}). Skipping opportunity: pool={}",
                                active_count, max_concurrent_positions, swap_plan.pool_amm_id
                            );
                            metrics.buy_init_failures.inc();
                            continue;
                        }

                        if let Some(ref emitter) = event_emitter {
                            Self::emit_candidate_pass(
                                emitter,
                                &swap_plan,
                                "gatekeeper_shadow_pass",
                            );
                        }

                        match Self::process_shadow_swap_plan(
                            &swap_plan,
                            &shadow_backend,
                            &quote_provider,
                            &snapshot_engine,
                            &shadow_ledger,
                            &rpc_client,
                            &event_emitter,
                            &post_buy_guardian,
                            shadow_quote_max_age_ms,
                            shadow_execution_config.stale_policy,
                            shadow_execution_config.tx_build_compensation_ms,
                            enable_jito,
                            1,
                        )
                        .await
                        {
                            Ok(latency_ms) => {
                                metrics.buy_intents_initialized.inc();
                                metrics.trigger_txs_sent.inc();
                                metrics.trigger_txs_confirmed.inc();
                                metrics.trigger_send_latency.observe(latency_ms as f64);
                                metrics.trigger_confirm_latency.observe(latency_ms as f64);
                            }
                            Err(err) => {
                                warn!(error = %err, "ShadowBackend entry processing failed");
                                metrics.buy_init_failures.inc();
                                metrics.trigger_txs_failed.inc();
                            }
                        }
                    }

                    warn!("Shadow trigger loop ended");
                }
                ExecutionMode::Live | ExecutionMode::Dual => {
                    info!("LIVE/DUAL MODE ENABLED - starting live trigger loop");

                    while let Some(swap_plan) = swap_plan_rx.recv().await {
                        // Check system mode before processing
                        if let Some(ref state) = gui_state {
                            if state.is_stopped() {
                                warn!("System is STOPPED - exiting trigger loop");
                                break;
                            }

                            if state.is_paused() {
                                info!("System is PAUSED - skipping swap plan");
                                continue;
                            }
                        }

                        let start = Instant::now();

                        info!(
                            "Evaluating SwapPlan: pool={}, amount_in={}",
                            swap_plan.pool_amm_id, swap_plan.amount_in
                        );

                        // Gatekeeper check: enforce position limit
                        {
                            let active_count = revolver.read().await.get_active_mints().len();

                            if active_count >= max_concurrent_positions {
                                warn!(
                                    target: "gatekeeper",
                                    "⛔ STOP: Position limit reached ({}/{}). Skipping opportunity: pool={}",
                                    active_count, max_concurrent_positions, swap_plan.pool_amm_id
                                );
                                metrics.buy_init_failures.inc();
                                continue; // Skip this swap, return to listening
                            }

                            info!(
                                target: "gatekeeper",
                                "✅ Gatekeeper passed: {}/{} positions active",
                                active_count, max_concurrent_positions
                            );
                            if let Some(ref emitter) = event_emitter {
                                Self::emit_candidate_pass(
                                    emitter,
                                    &swap_plan,
                                    "gatekeeper_live_pass",
                                );
                            }
                            if dual_mode {
                                if let Some(ref emitter) = secondary_emitter {
                                    Self::emit_candidate_pass(
                                        emitter,
                                        &swap_plan,
                                        "gatekeeper_live_pass",
                                    );
                                }
                            }
                        }
                        // =========================================

                        // === HYPER ORACLE: Early Market Analysis (T+2s Window) ===
                        // This block implements the HyperOracle pre-buy filtering using:
                        // 1. SCR (Slot-Coherence Resonance) - Detects bot activity via FFT
                        // 2. ULVF (Ultra-Early Liquidity Vector Field) - Detects wash trading via Curl/Divergence
                        // 3. POVC (Projected Orderflow Vector Collapse) - Predicts trajectory via PCA
                        //
                        // Uses real-time data from SnapshotEngine for accurate market analysis
                        {
                            // Try to get real market snapshots from SnapshotEngine
                            let (t0_snapshot, t1_snapshot) = match snapshot_engine
                                .latest_pair(&swap_plan.pool_amm_id)
                            {
                                Some((t0, t1)) => {
                                    // Use HyperOracle's conversion method for consistency
                                    use crate::oracle::HyperOracle;
                                    let t0_ho = HyperOracle::convert_snapshot(&t0);
                                    let t1_ho = HyperOracle::convert_snapshot(&t1);

                                    debug!(
                                        target: "hyper_oracle",
                                        "Using real SnapshotEngine data: pool={}, t0={:.3} SOL, t1={:.3} SOL, Δt={}ms",
                                        swap_plan.pool_amm_id, t0.cum_volume_sol, t1.cum_volume_sol,
                                        t1.timestamp_ms.saturating_sub(t0.timestamp_ms)
                                    );

                                    (t0_ho, t1_ho)
                                }
                                None => {
                                    // Fallback: Create minimal snapshots based on current context
                                    // This happens when the pool is very new and hasn't accumulated snapshots yet
                                    debug!(
                                        target: "hyper_oracle",
                                        "No SnapshotEngine data for pool={}, using minimal fallback snapshots",
                                        swap_plan.pool_amm_id
                                    );

                                    let current_ts = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                        as u64;

                                    let t0_snapshot = MarketSnapshot {
                                        tx_key: None,
                                        timestamp_ms: current_ts.saturating_sub(2000),
                                        volume_sol: swap_plan.amount_in as f64 / 1_000_000_000.0,
                                        tx_count: 1,
                                        unique_addrs: 1,
                                    };

                                    let t1_snapshot = MarketSnapshot {
                                        tx_key: None,
                                        timestamp_ms: current_ts,
                                        volume_sol: t0_snapshot.volume_sol * 1.2,
                                        tx_count: 2,
                                        unique_addrs: 2,
                                    };

                                    (t0_snapshot, t1_snapshot)
                                }
                            };

                            // SCR requires at least 8 timestamps for FFT analysis to detect periodic bot patterns
                            // This represents a 2-second window with 250ms intervals (standard trading bot frequency)
                            const SCR_TIMESTAMP_COUNT: usize = 8;

                            // 1. SCR (Bot Detection via FFT) - ADAPTIVE LOGIC
                            // Require minimum 4 real data points to run FFT analysis.
                            // Without sufficient data, return neutral score (0.0) to avoid false positives.
                            const MIN_SCR_SAMPLES: usize = 4;

                            let recent_snaps =
                                snapshot_engine.last_n(&swap_plan.pool_amm_id, SCR_TIMESTAMP_COUNT);
                            let scr_score = if recent_snaps.len() >= MIN_SCR_SAMPLES {
                                // Sufficient data - compute real SCR score
                                let timestamps: Vec<u64> =
                                    recent_snaps.iter().map(|s| s.timestamp_ms).collect();
                                hyper_oracle.calculate_scr(&timestamps)
                            } else {
                                // Insufficient data: Return neutral score (benefit of the doubt).
                                // Synthetic timestamps would create artificial 4Hz signal that FFT would
                                // interpret as bot activity, causing false positives on new pools.
                                debug!(
                                    target: "hyper_oracle",
                                    "Not enough data for SCR ({} samples), skipping resonance check - pool={}",
                                    recent_snaps.len(), swap_plan.pool_amm_id
                                );
                                0.0
                            };

                            if scr_score > 0.85 {
                                warn!(
                                    target: "hyper_oracle",
                                    "🚨 HYPER REJECT: SCR Resonance {:.2} (Botnet Attack) - pool={}",
                                    scr_score, swap_plan.pool_amm_id
                                );
                                metrics.buy_init_failures.inc();
                                continue;
                            }

                            // 2. ULVF (Liquidity Physics)
                            let (divergence, curl) =
                                hyper_oracle.calculate_ulvf(&t0_snapshot, &t1_snapshot);

                            // Curl > 15.0 indicates wash trading (artificial rotation)
                            if curl > 15.0 {
                                warn!(
                                    target: "hyper_oracle",
                                    "🚨 HYPER REJECT: Liquidity Vortex (Curl {:.2}) - Wash Trading - pool={}",
                                    curl, swap_plan.pool_amm_id
                                );
                                metrics.buy_init_failures.inc();
                                continue;
                            }

                            // Divergence < 0.3 indicates stagnation (no fresh capital)
                            if divergence < 0.3 {
                                warn!(
                                    target: "hyper_oracle",
                                    "🚨 HYPER REJECT: Low Divergence {:.2} (Stagnation) - pool={}",
                                    divergence, swap_plan.pool_amm_id
                                );
                                metrics.buy_init_failures.inc();
                                continue;
                            }

                            // 3. POVC (Trajectory Prediction)
                            let trend_cluster = hyper_oracle.calculate_povc(&t1_snapshot);
                            match trend_cluster {
                                0 => {
                                    // Dump Cluster - reject
                                    warn!(
                                        target: "hyper_oracle",
                                        "🚨 HYPER REJECT: POVC predicts DUMP trajectory - pool={}",
                                        swap_plan.pool_amm_id
                                    );
                                    metrics.buy_init_failures.inc();
                                    continue;
                                }
                                1 => {
                                    // Organic Hype - proceed with confidence
                                    info!(
                                        target: "hyper_oracle",
                                        "✅ HYPER CONFIRM: Organic Growth Cluster Detected. EXECUTE BUY. pool={}",
                                        swap_plan.pool_amm_id
                                    );
                                }
                                _ => {
                                    // Noise Cluster - proceed with caution
                                    info!(
                                        target: "hyper_oracle",
                                        "⚠️ HYPER WARN: Noise Cluster. Proceed with caution (Tight Stop). pool={}",
                                        swap_plan.pool_amm_id
                                    );
                                    // Note: In a full implementation, this would set a tighter SL in Revolver
                                }
                            }

                            debug!(
                                target: "hyper_oracle",
                                "HyperOracle metrics: SCR={:.3}, Divergence={:.3}, Curl={:.3}, Cluster={}",
                                scr_score, divergence, curl, trend_cluster
                            );
                        }
                        // =========================================

                        if dual_mode {
                            if let (Some(sim_backend), Some(sim_emitter)) =
                                (sim_backend.as_ref(), secondary_emitter.as_ref())
                            {
                                let swap_plan_clone = swap_plan.clone();
                                let sim_backend_clone = Arc::clone(sim_backend);
                                let quote_provider_clone = Arc::clone(&quote_provider);
                                let snapshot_engine_clone = Arc::clone(&snapshot_engine);
                                let sim_emitter_clone = Some(Arc::clone(sim_emitter));

                                tokio::spawn(async move {
                                    let shadow_result = Self::process_paper_swap_plan(
                                        &swap_plan_clone,
                                        &sim_backend_clone,
                                        &quote_provider_clone,
                                        &snapshot_engine_clone,
                                        &sim_emitter_clone,
                                        &None,
                                        quote_max_age_ms,
                                        1,
                                    )
                                    .await;
                                    if let Err(e) = shadow_result {
                                        warn!(
                                            error = %e,
                                            "Dual mode: paper lane standard-shadow processing failed"
                                        );
                                    }
                                });
                            }
                        }

                        let backend = live_backend
                            .as_ref()
                            .expect("LiveBackend not initialized in Live/Dual mode");
                        let prepared_attempt = match Self::prepare_execution_attempt(
                            &swap_plan,
                            backend.reserve_entry_order_id(),
                            &quote_provider,
                            &snapshot_engine,
                            &shadow_ledger,
                            &rpc_client,
                            quote_max_age_ms,
                            EntryStalePolicy::EmitWarning,
                            if enable_jito {
                                EntryTimingSource::LiveJito
                            } else {
                                EntryTimingSource::LiveStandard
                            },
                            shadow_execution_config.tx_build_compensation_ms,
                            1,
                        )
                        .await
                        {
                            Ok(attempt) => attempt,
                            Err(error) => {
                                error!("{error}. Skipping transaction.");
                                metrics.buy_init_failures.inc();
                                continue;
                            }
                        };

                        info!(
                            target: "live_buy",
                            "📦 Submitting Live Entry: mint={}, sol_in={}, estimated_tokens={}, min_accept={} (slippage={}bps)",
                            prepared_attempt.attempt.prepared.candidate.base_mint,
                            swap_plan.amount_in,
                            prepared_attempt.estimated_tokens,
                            prepared_attempt.attempt.prepared.candidate.min_tokens_out,
                            prepared_attempt.slippage_bps
                        );

                        if let Some(ref emitter) = event_emitter {
                            Self::emit_prepared_entry_submitted(
                                emitter,
                                &prepared_attempt.attempt.prepared,
                                Some(serde_json::json!({
                                    "path": "live",
                                    "slippage_bps": prepared_attempt.slippage_bps,
                                    "quote_price_ref": prepared_attempt.attempt.prepared.quote.quote_price_ref,
                                    "timing_source": prepared_attempt.attempt.prepared.timing_source.as_str(),
                                    "timing_path": prepared_attempt.attempt.timing.timing_path.as_str(),
                                    "planned_settle_time_ms": prepared_attempt.attempt.timing.planned_settle_time_ms,
                                    "compensation_ms": prepared_attempt.attempt.timing.compensation_ms,
                                    "price_source": prepared_attempt.attempt.prepared.quote.price_source.as_str(),
                                })),
                            );
                        }

                        match backend
                            .submit_prepared_entry(prepared_attempt.attempt)
                            .await
                        {
                            Ok(entry_order_id) => {
                                info!("⭐ Live Entry Submitted ID: {}", entry_order_id);
                                metrics.buy_intents_initialized.inc();
                                metrics.trigger_txs_sent.inc();
                                metrics
                                    .trigger_send_latency
                                    .observe(start.elapsed().as_millis() as f64);
                            }
                            Err(e) => {
                                error!("❌ Live Backend Entry Failed: {}", e);
                                metrics.buy_init_failures.inc();
                                metrics.trigger_txs_failed.inc();
                            }
                        }
                    }

                    warn!("Live trigger task ended");
                }
            }
        });

        Ok(handle)
    }

    /// Legacy paper/compat execution path.
    ///
    /// Canonical shadow entry settlement is introduced separately via ShadowBackend in PR-3.
    async fn prepare_execution_attempt(
        swap_plan: &SwapPlan,
        order_id: String,
        quote_provider: &Arc<RwLock<ExecutableQuoteProvider>>,
        snapshot_engine: &Arc<crate::oracle::SnapshotEngine>,
        shadow_ledger: &Arc<ghost_core::shadow_ledger::ShadowLedger>,
        rpc_client: &Arc<solana_client::nonblocking::rpc_client::RpcClient>,
        quote_max_age_ms: u64,
        stale_policy: EntryStalePolicy,
        timing_source: EntryTimingSource,
        compensation_ms: u64,
        position_epoch: u64,
    ) -> std::result::Result<PreparedSwapAttempt, String> {
        if swap_plan.amount_in == 0 {
            return Err(format!(
                "Invalid SwapPlan: amount_in is zero. Pool={}, authority={}",
                swap_plan.pool_amm_id, swap_plan.authority
            ));
        }

        let token_mint = swap_plan
            .metadata
            .as_ref()
            .map(|metadata| metadata.token_mint)
            .ok_or_else(|| {
                format!(
                    "Cannot process SwapPlan: metadata missing token_mint. Pool={}",
                    swap_plan.pool_amm_id
                )
            })?;

        const SNIPE_SLIPPAGE_BPS: u64 = 50;
        let slippage_bps = SNIPE_SLIPPAGE_BPS;
        let current_slot = rpc_client.get_slot().await.ok();
        let (estimated_tokens, simulated_min_tokens) = match shadow_ledger
            .simulate_buy_with_slippage(
                &token_mint,
                swap_plan.amount_in,
                current_slot,
                slippage_bps,
            ) {
            Ok(sim) => (sim.tokens_out, sim.min_tokens_out),
            Err(error) => {
                debug!(
                    target: "shadow_slippage",
                    "Shadow Ledger miss for mint={}: {}. Falling back to estimation.",
                    token_mint, error
                );
                DirectBuyBuilder::estimate_tokens_out(
                    swap_plan.amount_in,
                    trigger::direct_buy_builder::DEFAULT_SLIPPAGE_TOLERANCE,
                )
            }
        };

        let final_min_tokens = if swap_plan.min_amount_out > 0 {
            swap_plan.min_amount_out
        } else {
            simulated_min_tokens
        };
        let submit_time_ms = EventEmitter::now_ms();
        let candidate_ref = CandidateRef {
            candidate_id: Self::candidate_id_for_swap_plan(swap_plan),
            base_mint: token_mint,
            pool_amm_id: swap_plan.pool_amm_id,
            entry_amount_lamports: swap_plan.amount_in,
            min_tokens_out: final_min_tokens,
        };
        let prepared_quote = Self::resolve_quote_ref_with_provider(
            quote_provider,
            snapshot_engine,
            swap_plan,
            &token_mint,
            submit_time_ms,
            quote_max_age_ms,
            current_slot,
            stale_policy,
        )
        .await;
        let prepared = Self::build_prepared_entry_execution(
            order_id,
            candidate_ref,
            position_epoch,
            prepared_quote,
            submit_time_ms,
            timing_source,
            None,
        );
        Ok(PreparedSwapAttempt {
            attempt: ExecutionAttemptContext::new(prepared, current_slot, compensation_ms),
            estimated_tokens,
            slippage_bps,
        })
    }

    async fn process_shadow_swap_plan(
        swap_plan: &SwapPlan,
        shadow_backend: &Arc<ShadowBackend>,
        quote_provider: &Arc<RwLock<ExecutableQuoteProvider>>,
        snapshot_engine: &Arc<crate::oracle::SnapshotEngine>,
        shadow_ledger: &Arc<ghost_core::shadow_ledger::ShadowLedger>,
        rpc_client: &Arc<solana_client::nonblocking::rpc_client::RpcClient>,
        event_emitter: &Option<Arc<EventEmitter>>,
        post_buy_guardian: &Option<Arc<crate::guardian::post_buy::MonitoringEngine>>,
        quote_max_age_ms: u64,
        stale_policy: EntryStalePolicy,
        compensation_ms: u64,
        enable_jito: bool,
        position_epoch: u64,
    ) -> std::result::Result<u64, String> {
        let prepared_attempt = Self::prepare_execution_attempt(
            swap_plan,
            shadow_backend.reserve_entry_order_id(),
            quote_provider,
            snapshot_engine,
            shadow_ledger,
            rpc_client,
            quote_max_age_ms,
            stale_policy,
            if enable_jito {
                EntryTimingSource::LiveJito
            } else {
                EntryTimingSource::LiveStandard
            },
            compensation_ms,
            position_epoch,
        )
        .await?;

        let order_id = shadow_backend
            .submit_prepared_entry(prepared_attempt.attempt.clone())
            .await
            .map_err(|error| format!("shadow submit_entry failed: {}", error))?;

        if let Some(emitter) = event_emitter.as_ref() {
            Self::emit_prepared_entry_submitted(
                emitter,
                &prepared_attempt.attempt.prepared,
                Some(serde_json::json!({
                    "path": "shadow",
                    "backend": "ShadowBackend",
                    "quote_price_ref": prepared_attempt.attempt.prepared.quote.quote_price_ref,
                    "timing_source": prepared_attempt.attempt.prepared.timing_source.as_str(),
                    "timing_path": prepared_attempt.attempt.timing.timing_path.as_str(),
                    "planned_settle_time_ms": prepared_attempt.attempt.timing.planned_settle_time_ms,
                    "compensation_ms": prepared_attempt.attempt.timing.compensation_ms,
                    "price_source": prepared_attempt.attempt.prepared.quote.price_source.as_str(),
                })),
            );
        }

        let poll_deadline_ms = prepared_attempt
            .attempt
            .timing
            .planned_settle_time_ms
            .saturating_add(1_000);
        loop {
            let poll_now_ms = EventEmitter::now_ms();
            let fills = shadow_backend.poll_fills(poll_now_ms).await;
            if let Some(fill) = fills.into_iter().find(|fill| fill.order_id == order_id) {
                if let Some(emitter) = event_emitter.as_ref() {
                    Self::emit_prepared_entry_filled(
                        emitter,
                        &prepared_attempt.attempt.prepared,
                        &fill.quote_id_used,
                        fill.fill_time_ms,
                        fill.fill_price,
                        fill.fill_qty,
                        fill.status,
                        fill.latency_ms,
                    );
                    if prepared_attempt.attempt.prepared.quote.is_stale
                        && matches!(
                            prepared_attempt.attempt.prepared.quote.stale_policy,
                            EntryStalePolicy::EmitWarning
                        )
                    {
                        Self::emit_quote_stale_events(
                            emitter,
                            prepared_attempt.attempt.prepared.candidate_id(),
                            &order_id,
                            &fill.quote_id_used,
                            prepared_attempt.attempt.prepared.quote.slot,
                            fill.fill_time_ms,
                            prepared_attempt.attempt.prepared.quote.stale_age_ms,
                            quote_max_age_ms,
                        );
                    }
                }

                match fill.status {
                    ExecFillStatus::Stale => {
                        return Err(format!("shadow fill stale for order {}", order_id));
                    }
                    ExecFillStatus::Failed => {
                        return Err(format!("shadow fill failed for order {}", order_id));
                    }
                    ExecFillStatus::Filled | ExecFillStatus::Confirmed => {
                        let Some(position_id) = fill.position_id.as_ref() else {
                            return Err(format!(
                                "shadow fill missing position_id for successful order {}",
                                order_id
                            ));
                        };
                        let mut registered_by_guardian = false;
                        if let Some(guardian) = post_buy_guardian.as_ref() {
                            let registered = guardian.register_position_with_context(
                                swap_plan.pool_amm_id,
                                prepared_attempt.attempt.prepared.candidate.base_mint,
                                swap_plan.pool_amm_id,
                                Some(fill.fill_price),
                                Some(
                                    prepared_attempt
                                        .attempt
                                        .prepared
                                        .candidate
                                        .entry_amount_lamports,
                                ),
                                Some(fill.fill_qty),
                                Some(PositionEventContext {
                                    join_metadata: PositionJoinMetadata::default(),
                                    candidate_id: prepared_attempt
                                        .attempt
                                        .prepared
                                        .candidate_id()
                                        .to_string(),
                                    entry_order_id: order_id.clone(),
                                    quote_id: fill.quote_id_used.clone(),
                                    slot: prepared_attempt.attempt.prepared.quote.slot,
                                    lane: Lane::Shadow,
                                    position_id: Some(position_id.clone()),
                                    position_epoch: Some(position_epoch),
                                }),
                            );
                            registered_by_guardian = registered.is_some();
                        }

                        if !registered_by_guardian {
                            if let Some(emitter) = event_emitter.as_ref() {
                                let mut env = emitter.make_envelope_at(
                                    &prepared_attempt.attempt.prepared.candidate_id().to_string(),
                                    fill.fill_time_ms,
                                );
                                env.position_id = Some(position_id.clone());
                                env.position_epoch = Some(position_epoch);
                                env.order_id = Some(order_id.clone());
                                env.quote_id = Some(fill.quote_id_used.clone());
                                env.slot = prepared_attempt.attempt.prepared.quote.slot;
                                emitter.emit_raw(ExecutionEvent::new(
                                    env,
                                    EventKind::PositionOpened(PositionOpenedPayload {
                                        entry_price: fill.fill_price,
                                        entry_time_ms: fill.fill_time_ms,
                                        epoch_id: position_epoch,
                                        size_tokens: fill.fill_qty,
                                        size_sol: prepared_attempt
                                            .attempt
                                            .prepared
                                            .candidate
                                            .entry_amount_lamports,
                                    }),
                                ));
                            }
                        }
                        return Ok(fill.latency_ms);
                    }
                    ExecFillStatus::Sent | ExecFillStatus::Unknown => {}
                }
            }

            if poll_now_ms >= poll_deadline_ms {
                return Err(format!("shadow fill timeout for order {}", order_id));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub(super) async fn process_paper_swap_plan(
        swap_plan: &SwapPlan,
        paper_backend: &Arc<PaperBackend>,
        quote_provider: &Arc<RwLock<ExecutableQuoteProvider>>,
        snapshot_engine: &Arc<crate::oracle::SnapshotEngine>,
        event_emitter: &Option<Arc<EventEmitter>>,
        post_buy_guardian: &Option<Arc<crate::guardian::post_buy::MonitoringEngine>>,
        quote_max_age_ms: u64,
        position_epoch: u64,
    ) -> std::result::Result<u64, String> {
        let token_mint = swap_plan
            .metadata
            .as_ref()
            .map(|m| m.token_mint)
            .ok_or_else(|| {
                format!(
                    "Cannot process paper SwapPlan: metadata missing token_mint. Pool={}",
                    swap_plan.pool_amm_id
                )
            })?;
        let candidate_id = Self::candidate_id_for_swap_plan(swap_plan);
        let submit_time_ms = EventEmitter::now_ms();
        let prepared_quote = Self::resolve_quote_ref_with_provider(
            quote_provider,
            snapshot_engine,
            swap_plan,
            &token_mint,
            submit_time_ms,
            quote_max_age_ms,
            None,
            EntryStalePolicy::Reject,
        )
        .await;

        let candidate_ref = CandidateRef {
            candidate_id: candidate_id.clone(),
            base_mint: token_mint,
            pool_amm_id: swap_plan.pool_amm_id,
            entry_amount_lamports: swap_plan.amount_in,
            min_tokens_out: swap_plan.min_amount_out,
        };
        let prepared_entry = Self::build_prepared_entry_execution(
            paper_backend.reserve_entry_order_id().await,
            candidate_ref,
            position_epoch,
            prepared_quote,
            submit_time_ms,
            EntryTimingSource::PaperBroker,
            None,
        );

        let order_id = paper_backend
            .submit_prepared_entry(ExecutionAttemptContext::new(
                prepared_entry.clone(),
                None,
                MIN_SHADOW_TX_BUILD_COMPENSATION_MS,
            ))
            .await
            .map_err(|e| format!("paper submit_entry failed: {}", e))?;

        if let Some(emitter) = event_emitter.as_ref() {
            Self::emit_prepared_entry_submitted(
                emitter,
                &prepared_entry,
                Some(serde_json::json!({
                    "path": "paper",
                    "backend": "PaperBackend",
                    "quote_price_ref": prepared_entry.quote.quote_price_ref,
                    "timing_source": prepared_entry.timing_source.as_str(),
                    "price_source": prepared_entry.quote.price_source.as_str(),
                })),
            );
        }

        let poll_deadline = Instant::now() + Duration::from_millis(5000);
        loop {
            let poll_now_ms = EventEmitter::now_ms();
            let fills = paper_backend.poll_fills(poll_now_ms).await;
            if let Some(fill) = fills.into_iter().find(|f| f.order_id == order_id) {
                let fill_status = match fill.status {
                    ExecFillStatus::Filled => ExecFillStatus::Filled,
                    ExecFillStatus::Failed => ExecFillStatus::Failed,
                    ExecFillStatus::Stale => ExecFillStatus::Stale,
                    ExecFillStatus::Sent => ExecFillStatus::Sent,
                    ExecFillStatus::Confirmed => ExecFillStatus::Confirmed,
                    ExecFillStatus::Unknown => ExecFillStatus::Unknown,
                };
                if let Some(emitter) = event_emitter.as_ref() {
                    Self::emit_prepared_entry_filled(
                        emitter,
                        &prepared_entry,
                        &fill.quote_id_used,
                        fill.fill_time_ms,
                        fill.fill_price,
                        fill.fill_qty,
                        fill_status,
                        fill.latency_ms,
                    );
                }

                if matches!(fill.status, ExecFillStatus::Stale) {
                    if let Some(emitter) = event_emitter.as_ref() {
                        let stale_age_ms = {
                            let qp = quote_provider.read().await;
                            qp.get_by_id(&fill.quote_id_used)
                                .map(|q| fill.fill_time_ms.saturating_sub(q.timestamp_ms))
                                .unwrap_or(u64::MAX)
                        };
                        Self::emit_quote_stale_events(
                            emitter,
                            prepared_entry.candidate_id(),
                            &order_id,
                            &fill.quote_id_used,
                            prepared_entry.quote.slot,
                            fill.fill_time_ms,
                            stale_age_ms,
                            quote_max_age_ms,
                        );
                    }
                    return Err(format!("paper fill stale for order {}", order_id));
                }

                if matches!(fill.status, ExecFillStatus::Failed) {
                    return Err(format!("paper fill failed for order {}", order_id));
                }

                if matches!(
                    fill.status,
                    ExecFillStatus::Filled | ExecFillStatus::Confirmed
                ) {
                    if let Some(position_id) = fill.position_id.as_ref() {
                        let (stress_snapshot, transition) =
                            paper_backend.get_execution_stress_with_transition(position_id);
                        if let (Some(emitter), Some(transition)) =
                            (event_emitter.as_ref(), transition)
                        {
                            emitter.emit_stress_changed(
                                &candidate_id,
                                position_id,
                                transition.previous_bucket,
                                transition.new_bucket,
                                &stress_snapshot,
                            );
                        }
                    }

                    let mut registered_by_guardian = false;
                    if let Some(guardian) = post_buy_guardian.as_ref() {
                        let _ = guardian.register_position_with_context(
                            swap_plan.pool_amm_id,
                            token_mint,
                            swap_plan.pool_amm_id,
                            Some(fill.fill_price),
                            Some(swap_plan.amount_in),
                            Some(fill.fill_qty),
                            Some(PositionEventContext {
                                join_metadata: PositionJoinMetadata::default(),
                                candidate_id: candidate_id.clone(),
                                entry_order_id: order_id.clone(),
                                quote_id: fill.quote_id_used.clone(),
                                slot: prepared_entry.quote.slot,
                                lane: event_emitter
                                    .as_ref()
                                    .map(|emitter| emitter.lane())
                                    .unwrap_or(Lane::Paper),
                                position_id: fill.position_id.clone(),
                                position_epoch: Some(position_epoch),
                            }),
                        );
                        registered_by_guardian = true;
                    }

                    if !registered_by_guardian {
                        if let (Some(emitter), Some(position_id)) =
                            (event_emitter.as_ref(), fill.position_id.as_ref())
                        {
                            let mut env =
                                emitter.make_envelope_at(&candidate_id, fill.fill_time_ms);
                            env.position_id = Some(position_id.clone());
                            env.position_epoch = Some(position_epoch);
                            env.order_id = Some(order_id.clone());
                            env.quote_id = Some(fill.quote_id_used.clone());
                            env.slot = prepared_entry.quote.slot;
                            emitter.emit_raw(ExecutionEvent::new(
                                env,
                                EventKind::PositionOpened(PositionOpenedPayload {
                                    entry_price: fill.fill_price,
                                    entry_time_ms: fill.fill_time_ms,
                                    epoch_id: position_epoch,
                                    size_tokens: fill.fill_qty,
                                    size_sol: swap_plan.amount_in,
                                }),
                            ));
                        }
                    }
                    return Ok(fill.latency_ms);
                }

                return Err(format!(
                    "paper fill unresolved status {:?} for order {}",
                    fill.status, order_id
                ));
            }

            if Instant::now() >= poll_deadline {
                return Err(format!("paper fill timeout for order {}", order_id));
            }

            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn complete_live_entry_and_emit(
        live_backend: Option<&Arc<LiveBackend>>,
        event_emitter: &Option<Arc<EventEmitter>>,
        prepared: &PreparedEntryExecution,
        fill_time_ms: u64,
        status: ExecFillStatus,
        fill_price: f64,
        fill_qty: u64,
        quote_max_age_ms: u64,
    ) {
        let latency_ms = fill_time_ms.saturating_sub(prepared.submit_time_ms);

        if let Some(backend) = live_backend {
            if let Err(e) = backend
                .complete_order(
                    &prepared.order_id,
                    status,
                    fill_price,
                    fill_qty,
                    prepared.quote.quote_id.clone(),
                    fill_time_ms,
                    None,
                )
                .await
            {
                warn!(
                    order_id = %prepared.order_id,
                    error = %e,
                    "LiveBackend complete_order failed; falling back to direct emission"
                );
            } else {
                let fills = backend.poll_fills(fill_time_ms).await;
                if let Some(fill) = fills.into_iter().find(|f| f.order_id == prepared.order_id) {
                    if let Some(emitter) = event_emitter.as_ref() {
                        Self::emit_prepared_entry_filled(
                            emitter,
                            prepared,
                            &fill.quote_id_used,
                            fill.fill_time_ms,
                            fill.fill_price,
                            fill.fill_qty,
                            fill.status,
                            fill.latency_ms,
                        );
                        if prepared.quote.is_stale
                            && matches!(prepared.quote.stale_policy, EntryStalePolicy::EmitWarning)
                        {
                            Self::emit_quote_stale_events(
                                emitter,
                                prepared.candidate_id(),
                                &fill.order_id,
                                &fill.quote_id_used,
                                prepared.quote.slot,
                                fill.fill_time_ms,
                                prepared.quote.stale_age_ms,
                                quote_max_age_ms,
                            );
                        }
                    }
                    return;
                }
            }
        }

        if let Some(emitter) = event_emitter.as_ref() {
            Self::emit_prepared_entry_filled(
                emitter,
                prepared,
                &prepared.quote.quote_id,
                fill_time_ms,
                fill_price,
                fill_qty,
                status,
                latency_ms,
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
                    fill_time_ms,
                    prepared.quote.stale_age_ms,
                    quote_max_age_ms,
                );
            }
        }
    }

    pub(super) fn candidate_id_for_swap_plan(swap_plan: &SwapPlan) -> String {
        let mint = swap_plan
            .metadata
            .as_ref()
            .map(|m| m.token_mint.to_string())
            .unwrap_or_else(|| "unknown_mint".to_string());
        let creation_ref = swap_plan
            .metadata
            .as_ref()
            .map(|m| m.created_at.max(0) as u64)
            .unwrap_or_else(|| swap_plan.timeout.max(0) as u64);
        format!(
            "{}_{}_{}_{}_{}",
            mint,
            swap_plan.pool_amm_id,
            creation_ref,
            swap_plan.amount_in,
            swap_plan.min_amount_out,
        )
    }

    pub(crate) fn build_prepared_entry_execution(
        order_id: String,
        candidate: CandidateRef,
        position_epoch: u64,
        quote: PreparedQuoteRef,
        submit_time_ms: u64,
        timing_source: EntryTimingSource,
        predicted_slot: Option<u64>,
    ) -> PreparedEntryExecution {
        PreparedEntryExecution {
            order_id,
            candidate,
            submit_time_ms,
            position_epoch,
            quote,
            timing_source,
            predicted_slot,
        }
    }

    pub(crate) async fn resolve_quote_ref_with_provider(
        quote_provider: &Arc<RwLock<ExecutableQuoteProvider>>,
        snapshot_engine: &Arc<crate::oracle::SnapshotEngine>,
        swap_plan: &SwapPlan,
        base_mint: &solana_sdk::pubkey::Pubkey,
        now_ms: u64,
        max_quote_age_ms: u64,
        fallback_slot: Option<u64>,
        stale_policy: EntryStalePolicy,
    ) -> PreparedQuoteRef {
        let latest_snapshot = snapshot_engine.get_latest_snapshot(&swap_plan.pool_amm_id);
        let quote_slot = latest_snapshot.and_then(|s| s.slot).or(fallback_slot);
        let fallback_price =
            Self::effective_fill_price(swap_plan.amount_in, swap_plan.min_amount_out).max(1e-12);
        let (quote_price, price_source) = latest_snapshot
            .filter(|s| s.price_valid() && s.price_quote.is_finite() && s.price_quote > 0.0)
            .map(|s| (s.price_quote, EntryPriceSource::SnapshotEngine))
            .unwrap_or((fallback_price, EntryPriceSource::EffectiveFillFallback));
        let reserve_quote = latest_snapshot
            .map(|s| s.reserve_quote.max(0.0) as u64)
            .unwrap_or(0);
        let reserve_base = latest_snapshot
            .map(|s| s.reserve_base.max(0.0) as u64)
            .unwrap_or(0);
        let quote_ts_ms = latest_snapshot.map(|s| s.timestamp_ms).unwrap_or(now_ms);

        let quote_id = {
            let mut qp = quote_provider.write().await;
            qp.generate_quote(
                &swap_plan.pool_amm_id,
                base_mint,
                quote_ts_ms,
                quote_slot,
                quote_price,
                reserve_quote,
                reserve_base,
                0.0,
                QuoteSource::External,
            )
        };
        let stale_age_ms = now_ms.saturating_sub(quote_ts_ms);
        let is_stale = stale_age_ms > max_quote_age_ms;
        PreparedQuoteRef {
            quote_id,
            quote_ts_ms,
            slot: quote_slot,
            quote_price_ref: Some(quote_price),
            price_source,
            is_stale,
            stale_age_ms,
            stale_policy,
        }
    }

    pub(super) fn new_live_order_id(prefix: &str) -> String {
        format!("{}-{}", prefix, Uuid::new_v4())
    }

    pub(super) fn effective_fill_price(amount_lamports: u64, qty_tokens: u64) -> f64 {
        if qty_tokens == 0 {
            return 0.0;
        }
        amount_lamports as f64 / qty_tokens as f64
    }

    pub(super) fn emit_candidate_pass(emitter: &EventEmitter, swap_plan: &SwapPlan, source: &str) {
        let candidate_id = Self::candidate_id_for_swap_plan(swap_plan);
        let price_snapshot = if swap_plan.min_amount_out > 0 {
            Some(swap_plan.amount_in as f64 / swap_plan.min_amount_out as f64)
        } else {
            None
        };
        emitter.emit_raw(ExecutionEvent::new(
            emitter.make_envelope_at(&candidate_id, EventEmitter::now_ms()),
            EventKind::Candidate(CandidatePayload {
                mcap_snapshot: None,
                price_snapshot,
                gatekeeper_verdict: "PASS".to_string(),
                gatekeeper_flags: vec!["position_limit_ok".to_string()],
                source: source.to_string(),
            }),
        ));
    }

    pub(super) fn emit_entry_submitted(
        emitter: &EventEmitter,
        candidate_id: &str,
        order_id: &str,
        quote_id: &str,
        slot: Option<u64>,
        event_time_ms: u64,
        amount_lamports: u64,
        min_tokens_out: u64,
        send_params: Option<serde_json::Value>,
    ) {
        let mut env = emitter.make_envelope_at(&candidate_id.to_string(), event_time_ms);
        env.order_id = Some(order_id.to_string());
        env.quote_id = Some(quote_id.to_string());
        env.slot = slot;
        emitter.emit_raw(ExecutionEvent::new(
            env,
            EventKind::EntrySubmitted(EntrySubmittedPayload {
                side: OrderSide::Entry,
                planned_delay_ms: None,
                send_params,
                amount_lamports,
                min_tokens_out,
            }),
        ));
    }

    pub(super) fn emit_prepared_entry_submitted(
        emitter: &EventEmitter,
        prepared: &PreparedEntryExecution,
        send_params: Option<serde_json::Value>,
    ) {
        Self::emit_entry_submitted(
            emitter,
            prepared.candidate_id(),
            &prepared.order_id,
            &prepared.quote.quote_id,
            prepared.quote.slot,
            prepared.submit_time_ms,
            prepared.candidate.entry_amount_lamports,
            prepared.candidate.min_tokens_out,
            send_params,
        );
    }

    pub(crate) fn emit_entry_filled(
        emitter: &EventEmitter,
        candidate_id: &str,
        order_id: &str,
        quote_id: &str,
        slot: Option<u64>,
        fill_time_ms: u64,
        fill_price_effective: f64,
        fill_qty: u64,
        status: ExecFillStatus,
        latency_ms: u64,
    ) {
        let mut env = emitter.make_envelope_at(&candidate_id.to_string(), fill_time_ms);
        env.order_id = Some(order_id.to_string());
        env.quote_id = Some(quote_id.to_string());
        env.slot = slot;
        emitter.emit_raw(ExecutionEvent::new(
            env,
            EventKind::EntryFilled(EntryFilledPayload {
                fill_time_ms,
                fill_price_effective,
                fill_qty,
                quote_id_used: quote_id.to_string(),
                status,
                latency_ms,
            }),
        ));
    }

    pub(crate) fn emit_prepared_entry_filled(
        emitter: &EventEmitter,
        prepared: &PreparedEntryExecution,
        quote_id: &str,
        fill_time_ms: u64,
        fill_price_effective: f64,
        fill_qty: u64,
        status: ExecFillStatus,
        latency_ms: u64,
    ) {
        Self::emit_entry_filled(
            emitter,
            prepared.candidate_id(),
            &prepared.order_id,
            quote_id,
            prepared.quote.slot,
            fill_time_ms,
            fill_price_effective,
            fill_qty,
            status,
            latency_ms,
        );
    }

    pub(super) fn emit_quote_stale_events(
        emitter: &EventEmitter,
        candidate_id: &str,
        order_id: &str,
        quote_id: &str,
        slot: Option<u64>,
        event_time_ms: u64,
        stale_age_ms: u64,
        threshold_ms: u64,
    ) {
        let mut stale_env = emitter.make_envelope_at(&candidate_id.to_string(), event_time_ms);
        stale_env.order_id = Some(order_id.to_string());
        stale_env.quote_id = Some(quote_id.to_string());
        stale_env.slot = slot;
        emitter.emit_raw(ExecutionEvent::new(
            stale_env,
            EventKind::OracleStale(OracleStalePayload {
                stale_age_ms,
                threshold_ms,
            }),
        ));
    }

    /// Start panic signal monitoring task
    ///
    /// This task monitors all critical signals (LIGMA, QEDD, PARADOX, CLUSTER)
    /// and triggers execute_hard_kill when any signal is received.
    ///
    /// **THIS TASK IS THE NERVOUS SYSTEM - IT RUNS CONTINUOUSLY**
    pub(super) fn start_panic_monitor(&self) -> tokio::task::JoinHandle<()> {
        let panic_executor = self.panic_executor.clone();
        let panic_signals = self.panic_signals.clone();

        tokio::spawn(async move {
            if panic_executor.is_none() {
                warn!("Panic Executor not initialized - panic monitoring disabled");
                return;
            }

            let executor = panic_executor.unwrap();

            info!("🚨 Panic Monitor started - listening for critical signals");

            // Clone receivers for use in select!
            let mut ligma_veto_rx = panic_signals.ligma_veto_rx.write().await;
            let mut qedd_survival_rx = panic_signals.qedd_survival_rx.write().await;
            let mut paradox_anomaly_rx = panic_signals.paradox_anomaly_rx.write().await;
            let mut cluster_cabal_rx = panic_signals.cluster_cabal_rx.write().await;

            loop {
                tokio::select! {
                    // LIGMA veto: Liquidity trap or PSI imbalance
                    Some((mint, amount)) = ligma_veto_rx.recv() => {
                        error!(
                            "🚨🚨🚨 LIGMA VETO RECEIVED: mint={}, amount={}",
                            mint, amount
                        );
                        executor.execute_hard_kill(
                            mint,
                            amount,
                            trigger::KillReason::LigmaVeto,
                        ).await;
                        // Never reaches here - process terminates
                    }

                    // QEDD survival: Survival probability < 0.5
                    Some((mint, amount)) = qedd_survival_rx.recv() => {
                        error!(
                            "🚨🚨🚨 QEDD SURVIVAL PANIC: mint={}, amount={}",
                            mint, amount
                        );
                        executor.execute_hard_kill(
                            mint,
                            amount,
                            trigger::KillReason::QeddSurvival,
                        ).await;
                        // Never reaches here - process terminates
                    }

                    // PARADOX anomaly: HFT manipulation detected
                    Some((mint, amount)) = paradox_anomaly_rx.recv() => {
                        error!(
                            "🚨🚨🚨 PARADOX ANOMALY DETECTED: mint={}, amount={}",
                            mint, amount
                        );
                        executor.execute_hard_kill(
                            mint,
                            amount,
                            trigger::KillReason::ParadoxAnomaly,
                        ).await;
                        // Never reaches here - process terminates
                    }

                    // CLUSTER cabal: Cabal distribution detected
                    Some((mint, amount)) = cluster_cabal_rx.recv() => {
                        error!(
                            "🚨🚨🚨 CLUSTER CABAL SIGNAL: mint={}, amount={}",
                            mint, amount
                        );
                        executor.execute_hard_kill(
                            mint,
                            amount,
                            trigger::KillReason::ClusterCabal,
                        ).await;
                        // Never reaches here - process terminates
                    }

                    else => {
                        // All channels closed - this should never happen in production
                        warn!("All panic signal channels closed - exiting monitor");
                        break;
                    }
                }
            }

            error!("🚨 Panic Monitor ended unexpectedly");
        })
    }
}
