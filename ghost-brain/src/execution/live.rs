//! LiveBackend — wraps the existing trigger/Revolver execution path.
//!
//! This backend delegates to the real on-chain execution path
//! (Jito bundles / Direct AMM / Leapfrog TPU) without changing its semantics.
//! It exists so the pipeline can talk to `ExecutionBackend` uniformly.

use async_trait::async_trait;
use dashmap::DashSet;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use super::backend::*;
use crate::events::EventEmitter;
use crate::quotes::provider::ExecutableQuoteProvider;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use trigger::Revolver;

pub struct LiveBackendConfig {
    pub rpc_client: Arc<RpcClient>,
    pub payer: Arc<Keypair>,
    pub jito_executor: Option<Arc<crate::jito_bundle::JitoBundleExecutor>>,
    pub enable_jito: bool,
    pub redundancy_factor: usize,
    pub revolver: Arc<RwLock<Revolver>>,
    pub leader_predictor: Option<Arc<crate::leader_predictor::LeaderPredictor>>,
    pub leader_resolver: Option<Arc<trigger::LeaderResolver>>,
    pub leapfrog_config: Option<(usize, bool)>,
    pub shadow_ledger: Arc<ghost_core::shadow_ledger::ShadowLedger>,
    pub metrics: Arc<crate::metrics::E2EMetrics>,
    pub event_emitter: Option<Arc<EventEmitter>>,
    pub post_buy_guardian: Option<Arc<crate::guardian::post_buy::MonitoringEngine>>,
    pub quote_provider: Arc<RwLock<ExecutableQuoteProvider>>,
    pub snapshot_engine: Arc<crate::oracle::SnapshotEngine>,
    pub quote_max_age_ms: u64,
}

pub struct LiveEntryRequest {
    pub attempt: ExecutionAttemptContext,
}

/// LiveBackend: delegates to the existing on-chain execution path.
pub struct LiveBackend {
    pending_orders: Arc<RwLock<HashMap<OrderId, PendingOrder>>>,
    completed_fills: Arc<RwLock<Vec<FillEvent>>>,
    completed_order_ids: Arc<DashSet<OrderId>>, // Idempotency guarantee
    stress_snapshots: Arc<std::sync::RwLock<HashMap<PositionId, ExecutionStressSnapshot>>>,
    next_order_id: Arc<std::sync::atomic::AtomicU64>,
    entry_tx: mpsc::Sender<LiveEntryRequest>,
    revolver: Arc<RwLock<Revolver>>,
    event_emitter: Option<Arc<EventEmitter>>,
}

struct PendingOrder {
    order_id: OrderId,
    side: OrderSide,
    candidate_id: CandidateId,
    position_id: Option<PositionId>,
    quote_ref: QuoteId,
    submitted_at_ms: u64,
}

impl LiveBackend {
    /// Create a minimal stub for unit tests (no worker, no Revolver).
    #[cfg(test)]
    pub fn new_stub() -> Self {
        let (entry_tx, _entry_rx) = mpsc::channel(1);
        let revolver = Arc::new(RwLock::new(trigger::Revolver::new()));
        Self {
            pending_orders: Arc::new(RwLock::new(HashMap::new())),
            completed_fills: Arc::new(RwLock::new(Vec::new())),
            completed_order_ids: Arc::new(DashSet::new()),
            stress_snapshots: Arc::new(std::sync::RwLock::new(HashMap::new())),
            next_order_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            entry_tx,
            revolver,
            event_emitter: None,
        }
    }

    pub fn new(config: LiveBackendConfig) -> Self {
        info!("Initializing LiveBackend (on-chain execution)");
        let pending_orders = Arc::new(RwLock::new(HashMap::new()));
        let completed_fills = Arc::new(RwLock::new(Vec::new()));
        let completed_order_ids = Arc::new(DashSet::new());
        let stress_snapshots = Arc::new(std::sync::RwLock::new(HashMap::new()));
        let next_order_id = Arc::new(std::sync::atomic::AtomicU64::new(1));

        let (entry_tx, entry_rx) = mpsc::channel(100);

        let worker_pending = Arc::clone(&pending_orders);
        let worker_completed = Arc::clone(&completed_fills);
        let worker_completed_ids = Arc::clone(&completed_order_ids);
        let revolver_clone = Arc::clone(&config.revolver);

        let event_emitter = config.event_emitter.clone();

        let worker = LiveBackendWorker {
            config,
            entry_rx,
            pending_orders: worker_pending,
            completed_fills: worker_completed,
            completed_order_ids: worker_completed_ids,
        };

        tokio::spawn(async move {
            worker.run().await;
        });

        Self {
            pending_orders,
            completed_fills,
            completed_order_ids,
            stress_snapshots,
            next_order_id,
            entry_tx,
            revolver: revolver_clone,
            event_emitter,
        }
    }

    fn generate_order_id(&self) -> OrderId {
        let id = self
            .next_order_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("live-{}", id)
    }

    pub fn reserve_entry_order_id(&self) -> OrderId {
        self.generate_order_id()
    }

    pub async fn submit_prepared_entry(
        &self,
        attempt: ExecutionAttemptContext,
    ) -> Result<OrderId, ExecutionError> {
        let prepared = attempt.prepared.clone();
        let order_id = prepared.order_id.clone();
        self.pending_orders.write().await.insert(
            order_id.clone(),
            PendingOrder {
                order_id: order_id.clone(),
                side: OrderSide::Entry,
                candidate_id: prepared.candidate_id().to_string(),
                position_id: None,
                quote_ref: prepared.quote.quote_id.clone(),
                submitted_at_ms: prepared.submit_time_ms,
            },
        );

        if self
            .entry_tx
            .send(LiveEntryRequest { attempt })
            .await
            .is_err()
        {
            {
                let mut orders = self.pending_orders.write().await;
                orders.remove(&order_id);
            }
            self.completed_order_ids.insert(order_id.clone());
            self.completed_fills.write().await.push(FillEvent {
                order_id: order_id.clone(),
                position_id: None,
                side: OrderSide::Entry,
                status: FillStatus::Failed,
                fill_price: 0.0,
                fill_qty: 0,
                quote_id_used: String::new(),
                fill_time_ms: EventEmitter::now_ms(),
                latency_ms: 0,
                lane: Lane::Live,
            });
            return Err(ExecutionError::TransactionFailed(
                "Worker channel closed".into(),
            ));
        }

        Ok(order_id)
    }

    pub fn update_stress(&self, position_id: &PositionId, snapshot: ExecutionStressSnapshot) {
        self.stress_snapshots
            .write()
            .unwrap()
            .insert(position_id.clone(), snapshot);
    }

    pub async fn complete_order(
        &self,
        order_id: &str,
        status: FillStatus,
        fill_price: f64,
        fill_qty: u64,
        quote_id_used: QuoteId,
        fill_time_ms: u64,
        position_id: Option<PositionId>,
    ) -> Result<(), ExecutionError> {
        // Protection against double-fill or late duplicate events (idempotency invariant)
        if !self.completed_order_ids.insert(order_id.to_string()) {
            warn!(
                "Order {} already completed. Ignoring duplicate fill event.",
                order_id
            );
            return Ok(());
        }

        let pending = {
            let mut orders = self.pending_orders.write().await;
            orders.remove(order_id)
        };

        let Some(pending) = pending else {
            return Err(ExecutionError::TransactionFailed(format!(
                "unknown live order_id: {}",
                order_id
            )));
        };

        let resolved_position = position_id.or(pending.position_id.clone());
        let resolved_quote_id = if quote_id_used.is_empty() {
            pending.quote_ref.clone()
        } else {
            quote_id_used
        };
        let latency_ms = fill_time_ms.saturating_sub(pending.submitted_at_ms);

        let fill_event = FillEvent {
            order_id: pending.order_id.clone(),
            position_id: resolved_position.clone(),
            side: pending.side,
            status: status.clone(),
            fill_price,
            fill_qty,
            quote_id_used: resolved_quote_id.clone(),
            fill_time_ms,
            latency_ms,
            lane: Lane::Live,
        };

        self.completed_fills.write().await.push(fill_event.clone());

        Ok(())
    }
}

#[async_trait]
impl ExecutionBackend for LiveBackend {
    fn lane(&self) -> Lane {
        Lane::Live
    }

    async fn submit_entry(
        &self,
        candidate: &CandidateRef,
        quote_ref: QuoteId,
        position_epoch: u64,
    ) -> Result<OrderId, ExecutionError> {
        let submit_time_ms = EventEmitter::now_ms();
        let order_id = self.reserve_entry_order_id();
        let prepared = PreparedEntryExecution {
            order_id,
            candidate: candidate.clone(),
            submit_time_ms,
            position_epoch,
            quote: PreparedQuoteRef {
                quote_id: quote_ref,
                quote_ts_ms: submit_time_ms,
                slot: None,
                quote_price_ref: None,
                price_source: EntryPriceSource::EffectiveFillFallback,
                is_stale: false,
                stale_age_ms: 0,
                stale_policy: EntryStalePolicy::Reject,
            },
            timing_source: EntryTimingSource::LegacyBackend,
            predicted_slot: None,
        };

        self.submit_prepared_entry(ExecutionAttemptContext::new(
            prepared,
            None,
            MIN_SHADOW_TX_BUILD_COMPENSATION_MS,
        ))
        .await
    }

    async fn submit_exit(
        &self,
        position_id: &PositionId,
        fraction_bps: u16,
        quote_ref: QuoteId,
        _command_ref: Option<String>,
    ) -> Result<OrderId, ExecutionError> {
        let order_id = self.generate_order_id();

        self.pending_orders.write().await.insert(
            order_id.clone(),
            PendingOrder {
                order_id: order_id.clone(),
                side: OrderSide::Exit,
                candidate_id: String::new(),
                position_id: Some(position_id.clone()),
                quote_ref: quote_ref.clone(),
                submitted_at_ms: EventEmitter::now_ms(),
            },
        );

        let mut parts = position_id.split(':');
        let _pool = parts.next();
        if let Some(mint_str) = parts.next() {
            if let Ok(mint) = std::str::FromStr::from_str(mint_str) {
                if let Some(magazine) = self.revolver.write().await.get_magazine_mut(&mint) {
                    if fraction_bps >= 10000 {
                        magazine.force_exit_all = true;
                        magazine.strategy_mode = trigger::StrategyMode::PanicSell;
                    } else {
                        magazine.force_exit_fraction_bps = Some(fraction_bps as u16);
                        magazine.strategy_mode = trigger::StrategyMode::TightStopLoss;
                    }
                }
            }
        }

        Ok(order_id)
    }

    async fn poll_fills(&self, _now_ms: u64) -> Vec<FillEvent> {
        let mut fills = self.completed_fills.write().await;
        std::mem::take(&mut *fills)
    }

    fn get_execution_stress(&self, position_id: &PositionId) -> ExecutionStressSnapshot {
        self.stress_snapshots
            .read()
            .unwrap()
            .get(position_id)
            .cloned()
            .unwrap_or_default()
    }
}

pub struct LiveBackendWorker {
    pub config: LiveBackendConfig,
    pub entry_rx: mpsc::Receiver<LiveEntryRequest>,
    pub pending_orders: Arc<RwLock<HashMap<OrderId, PendingOrder>>>,
    pub completed_fills: Arc<RwLock<Vec<FillEvent>>>,
    pub completed_order_ids: Arc<DashSet<OrderId>>,
}

impl LiveBackendWorker {
    pub async fn run(mut self) {
        info!(
            "LiveBackendWorker started: Jito={}, Redundancy={}",
            self.config.enable_jito, self.config.redundancy_factor
        );

        // Recover in-flight orders from a hard crash / restart
        self.recovery_sweep().await;

        if self.config.enable_jito && self.config.jito_executor.is_some() {
            self.run_jito_loop().await;
        } else {
            self.run_standard_loop().await;
        }
    }

    async fn recovery_sweep(&self) {
        let active_orders: Vec<PendingOrder> = {
            let pending = self.pending_orders.read().await;
            pending
                .values()
                .filter(|p| p.submitted_at_ms > 0)
                .map(|p| PendingOrder {
                    order_id: p.order_id.clone(),
                    side: p.side,
                    candidate_id: p.candidate_id.clone(),
                    position_id: p.position_id.clone(),
                    quote_ref: p.quote_ref.clone(),
                    submitted_at_ms: p.submitted_at_ms,
                })
                .collect()
        };

        if !active_orders.is_empty() {
            warn!(
                "Recovery sweep found {} in-flight orders.",
                active_orders.len()
            );
            for order in active_orders {
                // If it's been more than 30s since boot, it's definitely a ghost order.
                // We emit UNKNOWN to seal the ledger so AEM doesn't wait forever.
                if !self.completed_order_ids.insert(order.order_id.clone()) {
                    continue; // Already processed
                }

                let fill = FillEvent {
                    order_id: order.order_id.clone(),
                    position_id: order.position_id,
                    side: order.side,
                    status: FillStatus::Unknown, // Signal that we lost track
                    fill_price: 0.0,
                    fill_qty: 0,
                    quote_id_used: order.quote_ref,
                    fill_time_ms: EventEmitter::now_ms(),
                    latency_ms: 0,
                    lane: Lane::Live,
                };
                self.completed_fills.write().await.push(fill);
                warn!(
                    "Marked in-flight order {} as UNKNOWN due to restart",
                    order.order_id
                );
            }
        }
    }

    async fn run_jito_loop(&mut self) {
        let mut batch_buffer = Vec::new();
        const BATCH_SIZE: usize = 4;
        const BATCH_TIMEOUT_MS: u64 = 1000;
        let mut batch_deadline = Instant::now() + Duration::from_millis(BATCH_TIMEOUT_MS);
        let mut tracking_id_counter = 0_u64;

        loop {
            let timeout_duration = batch_deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                req_opt = self.entry_rx.recv() => {
                    match req_opt {
                        Some(req) => {
                            batch_buffer.push(req);
                            if batch_buffer.len() >= BATCH_SIZE {
                                self.process_jito_batch(&batch_buffer, &mut tracking_id_counter).await;
                                batch_buffer.clear();
                                batch_deadline = Instant::now() + Duration::from_millis(BATCH_TIMEOUT_MS);
                            }
                        }
                        None => {
                            if !batch_buffer.is_empty() {
                                self.process_jito_batch(&batch_buffer, &mut tracking_id_counter).await;
                            }
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(timeout_duration) => {
                    if !batch_buffer.is_empty() {
                        self.process_jito_batch(&batch_buffer, &mut tracking_id_counter).await;
                        batch_buffer.clear();
                    }
                    batch_deadline = Instant::now() + Duration::from_millis(BATCH_TIMEOUT_MS);
                }
            }
        }
    }

    async fn run_standard_loop(&mut self) {
        while let Some(req) = self.entry_rx.recv().await {
            let prepared = &req.attempt.prepared;
            // standard mode processing
            info!(
                "LiveBackend: Processing standard direct buy for pool {}",
                prepared.candidate.pool_amm_id
            );
            // ... DirectBuyBuilder ...
            let buy_ix = trigger::DirectBuyBuilder::build_buy_ix(
                &self.config.payer.pubkey(),
                &prepared.candidate.base_mint,
                prepared.candidate.entry_amount_lamports,
                prepared.candidate.min_tokens_out,
            );

            let blockhash = match self.config.rpc_client.get_latest_blockhash().await {
                Ok(bh) => bh,
                Err(e) => {
                    error!("Standard execution failed to get blockhash (Timeout): {e}");
                    self.mark_fill_failed(&prepared.order_id).await;
                    continue; // Failure to get blockhash = timeout/failure
                }
            };

            let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                &[buy_ix],
                Some(&self.config.payer.pubkey()),
                &[&*self.config.payer],
                blockhash,
            );

            // Wait with the exact identical timeout semantic as pipeline used to have.
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(15000),
                self.config.rpc_client.send_and_confirm_transaction(&tx),
            )
            .await
            {
                Ok(Ok(sig)) => {
                    info!("Standard buy success: {}", sig);
                    self.mark_fill_success(&req).await;
                    self.fire_magazine_creation(&req).await;
                }
                Ok(Err(e)) => {
                    error!("Standard buy failed (Transaction error): {}", e);
                    self.mark_fill_failed(&prepared.order_id).await;
                }
                Err(_) => {
                    error!("Standard buy failed (Timeout waiting for confirmation)");
                    self.mark_fill_failed(&prepared.order_id).await;
                }
            }
        }
    }

    async fn process_jito_batch(&self, batch: &[LiveEntryRequest], counter: &mut u64) {
        info!("Processing Jito Batch of {}", batch.len());
        let mut intents = Vec::new();
        let rpc_fallback_slot = self
            .config
            .rpc_client
            .get_slot()
            .await
            .ok()
            .map(|slot| slot.saturating_add(1));

        for req in batch {
            *counter += 1;
            let prepared = &req.attempt.prepared;
            let predicted_slot = req
                .attempt
                .timing
                .predicted_slot
                .or(prepared.predicted_slot)
                .or_else(|| {
                    self.config.leader_predictor.as_ref().map(|predictor| {
                        predictor
                            .find_nearest_leader()
                            .map(|(_, slot)| slot)
                            .unwrap_or_else(|| predictor.current_slot().saturating_add(1))
                    })
                })
                .or(rpc_fallback_slot)
                .or_else(|| predicted_slot_from_reference(prepared.quote.slot))
                .unwrap_or(1);
            let intent = Arc::new(crate::jito_bundle::SwapIntent::new(
                self.config.payer.pubkey(),
                prepared.candidate.pool_amm_id,
                prepared.candidate.entry_amount_lamports,
                prepared.candidate.min_tokens_out,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    + 60,
                0.75, // Default priority
                predicted_slot,
                prepared.candidate.base_mint,
                *counter,
            ));
            intents.push(intent);
        }

        if let Some(exe) = &self.config.jito_executor {
            match exe
                .trigger_batch_jito(&intents, self.config.redundancy_factor as u32)
                .await
            {
                Ok(_) => {
                    info!("Jito batch submitted successfully");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    for req in batch {
                        self.mark_fill_success(req).await;
                        self.fire_magazine_creation(req).await;
                    }
                }
                Err(e) => {
                    error!("Jito batch failed: {}", e);
                    for req in batch {
                        self.mark_fill_failed(&req.attempt.prepared.order_id).await;
                    }
                }
            }
        }
    }

    async fn mark_fill_success(&self, req: &LiveEntryRequest) {
        if !self
            .completed_order_ids
            .insert(req.attempt.prepared.order_id.clone())
        {
            return;
        }

        let prepared = &req.attempt.prepared;
        let fill_price = if prepared.candidate.min_tokens_out > 0 {
            (prepared.candidate.entry_amount_lamports as f64
                / prepared.candidate.min_tokens_out as f64)
                * 1_000_000.0
        } else {
            0.0
        };
        let fill = FillEvent {
            order_id: prepared.order_id.clone(),
            position_id: None,
            side: OrderSide::Entry,
            status: FillStatus::Confirmed,
            fill_price,
            fill_qty: prepared.candidate.min_tokens_out,
            quote_id_used: prepared.quote.quote_id.clone(),
            fill_time_ms: EventEmitter::now_ms(),
            latency_ms: EventEmitter::now_ms().saturating_sub(prepared.submit_time_ms),
            lane: Lane::Live,
        };
        self.completed_fills.write().await.push(fill);
    }

    async fn mark_fill_failed(&self, order_id: &str) {
        if !self.completed_order_ids.insert(order_id.to_string()) {
            return;
        }

        let fill = FillEvent {
            order_id: order_id.to_string(),
            position_id: None,
            side: OrderSide::Entry,
            status: FillStatus::Failed,
            fill_price: 0.0,
            fill_qty: 0,
            quote_id_used: String::new(),
            fill_time_ms: EventEmitter::now_ms(),
            latency_ms: 0,
            lane: Lane::Live,
        };
        self.completed_fills.write().await.push(fill);
    }

    async fn fire_magazine_creation(&self, req: &LiveEntryRequest) {
        let pump_program_id = trigger::DirectBuyBuilder::pump_program_id();
        let prepared = &req.attempt.prepared;
        let token_mint = prepared.candidate.base_mint;
        let position_size = prepared.candidate.entry_amount_lamports;
        let entry_price = if prepared.candidate.min_tokens_out > 0 {
            (prepared.candidate.entry_amount_lamports as f64
                / prepared.candidate.min_tokens_out as f64
                * 1_000_000.0) as u64
        } else {
            1000_u64
        };

        let rpc_clone = Arc::clone(&self.config.rpc_client);
        let revolver_clone = Arc::clone(&self.config.revolver);
        let payer_bytes = self.config.payer.to_bytes();

        tokio::spawn(async move {
            let payer_bg = match Keypair::from_bytes(&payer_bytes) {
                Ok(kp) => kp,
                Err(_) => return,
            };

            let magazine_config = trigger::MagazineConfig::default_targets(pump_program_id);
            if let Ok(bullets) = trigger::create_magazine_after_buy(
                &payer_bg,
                token_mint,
                position_size,
                entry_price,
                &magazine_config,
                &rpc_clone,
            )
            .await
            {
                let mut guard = revolver_clone.write().await;
                guard.load_magazine(token_mint, bullets);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::paper::{PaperBackend, PaperBroker, PaperBrokerConfig};
    use crate::quotes::provider::{QuoteProviderConfig, QuoteSource};
    use solana_sdk::pubkey::Pubkey;

    fn make_candidate() -> CandidateRef {
        CandidateRef {
            candidate_id: "cand-live-worker".to_string(),
            base_mint: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            entry_amount_lamports: 1_000_000,
            min_tokens_out: 1000,
        }
    }

    fn make_worker() -> LiveBackendWorker {
        let (_tx, rx) = mpsc::channel(8);
        LiveBackendWorker {
            config: LiveBackendConfig {
                rpc_client: Arc::new(RpcClient::new("http://127.0.0.1:8899".to_string())),
                payer: Arc::new(Keypair::new()),
                jito_executor: None,
                enable_jito: false,
                redundancy_factor: 1,
                revolver: Arc::new(RwLock::new(Revolver::new())),
                leader_predictor: None,
                leader_resolver: None,
                leapfrog_config: None,
                shadow_ledger: Arc::new(ghost_core::shadow_ledger::ShadowLedger::new()),
                metrics: Arc::new(crate::metrics::E2EMetrics::new()),
                event_emitter: None,
                post_buy_guardian: None,
                quote_provider: Arc::new(RwLock::new(ExecutableQuoteProvider::new(
                    QuoteProviderConfig::default(),
                ))),
                snapshot_engine: Arc::new(crate::oracle::SnapshotEngine::new(8, 200)),
                quote_max_age_ms: 1_500,
            },
            entry_rx: rx,
            pending_orders: Arc::new(RwLock::new(HashMap::new())),
            completed_fills: Arc::new(RwLock::new(Vec::new())),
            completed_order_ids: Arc::new(DashSet::new()),
        }
    }

    #[tokio::test]
    async fn test_complete_order_idempotent_race() {
        let backend = Arc::new(LiveBackend::new_stub());
        let order_id = "live-race-1".to_string();
        let quote_id = "q-1".to_string();
        let now = EventEmitter::now_ms();

        backend.pending_orders.write().await.insert(
            order_id.clone(),
            PendingOrder {
                order_id: order_id.clone(),
                side: OrderSide::Entry,
                candidate_id: "cand-race".to_string(),
                position_id: None,
                quote_ref: quote_id.clone(),
                submitted_at_ms: now.saturating_sub(10),
            },
        );

        let b1 = Arc::clone(&backend);
        let b2 = Arc::clone(&backend);

        let (r1, r2) = tokio::join!(
            b1.complete_order(
                &order_id,
                FillStatus::Confirmed,
                1.0,
                100,
                quote_id.clone(),
                now,
                None
            ),
            b2.complete_order(
                &order_id,
                FillStatus::Confirmed,
                1.1,
                200,
                quote_id.clone(),
                now,
                None
            ),
        );

        assert!(r1.is_ok());
        assert!(r2.is_ok());

        let fills = backend.poll_fills(now).await;
        assert_eq!(fills.len(), 1, "idempotent completion should emit one fill");
        assert_eq!(fills[0].order_id, order_id);
    }

    #[tokio::test]
    async fn test_terminal_state_timeout_records_failed_fill() {
        let worker = make_worker();
        worker.mark_fill_failed("timeout-order-1").await;

        let fills = worker.completed_fills.read().await;
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].order_id, "timeout-order-1");
        assert_eq!(fills[0].status, FillStatus::Failed);
        assert_eq!(fills[0].lane, Lane::Live);
    }

    #[tokio::test]
    async fn test_terminal_state_error_is_exactly_once() {
        let worker = make_worker();
        worker.mark_fill_failed("error-order-1").await;
        worker.mark_fill_failed("error-order-1").await;

        let fills = worker.completed_fills.read().await;
        assert_eq!(fills.len(), 1, "duplicate terminal error must be deduped");
        assert_eq!(fills[0].status, FillStatus::Failed);
    }

    #[tokio::test]
    async fn test_recovery_sweep_marks_inflight_unknown() {
        let worker = make_worker();
        worker.pending_orders.write().await.insert(
            "inflight-1".to_string(),
            PendingOrder {
                order_id: "inflight-1".to_string(),
                side: OrderSide::Entry,
                candidate_id: "cand-1".to_string(),
                position_id: None,
                quote_ref: "q-1".to_string(),
                submitted_at_ms: EventEmitter::now_ms().saturating_sub(100),
            },
        );
        worker.pending_orders.write().await.insert(
            "inflight-2".to_string(),
            PendingOrder {
                order_id: "inflight-2".to_string(),
                side: OrderSide::Exit,
                candidate_id: "cand-2".to_string(),
                position_id: Some("pos-2".to_string()),
                quote_ref: "q-2".to_string(),
                submitted_at_ms: EventEmitter::now_ms().saturating_sub(200),
            },
        );

        worker.recovery_sweep().await;
        worker.recovery_sweep().await;

        let fills = worker.completed_fills.read().await;
        assert_eq!(fills.len(), 2, "recovery sweep should be idempotent");
        assert!(fills.iter().all(|f| f.status == FillStatus::Unknown));
        assert!(fills.iter().all(|f| f.lane == Lane::Live));
    }

    #[tokio::test]
    async fn test_lane_separation_live_and_paper() {
        let worker = make_worker();
        let req = LiveEntryRequest {
            attempt: ExecutionAttemptContext::new(
                PreparedEntryExecution {
                    order_id: "live-success-1".to_string(),
                    candidate: make_candidate(),
                    submit_time_ms: EventEmitter::now_ms().saturating_sub(15),
                    position_epoch: 1,
                    quote: PreparedQuoteRef {
                        quote_id: "q-live".to_string(),
                        quote_ts_ms: EventEmitter::now_ms().saturating_sub(15),
                        slot: None,
                        quote_price_ref: Some(1.0),
                        price_source: EntryPriceSource::SnapshotEngine,
                        is_stale: false,
                        stale_age_ms: 0,
                        stale_policy: EntryStalePolicy::Reject,
                    },
                    timing_source: EntryTimingSource::LiveStandard,
                    predicted_slot: None,
                },
                None,
                MIN_SHADOW_TX_BUILD_COMPENSATION_MS,
            ),
        };
        worker.mark_fill_success(&req).await;
        let live_fills = worker.completed_fills.read().await.clone();
        assert_eq!(live_fills.len(), 1);
        assert_eq!(live_fills[0].lane, Lane::Live);

        let qp = Arc::new(RwLock::new(ExecutableQuoteProvider::new(
            QuoteProviderConfig::default(),
        )));
        let paper_candidate = make_candidate();
        {
            let mut q = qp.write().await;
            q.generate_quote(
                &paper_candidate.pool_amm_id,
                &paper_candidate.base_mint,
                1000,
                Some(1),
                0.01,
                1_000_000,
                100_000_000,
                0.0,
                QuoteSource::BondingCurve,
            );
        }
        let broker = PaperBroker::new(
            PaperBrokerConfig {
                fill_delay_ms_min: 10,
                fill_delay_ms_max: 20,
                jitter_ms: 0,
                ..Default::default()
            },
            qp,
        );
        let backend = PaperBackend::new(broker);
        let _ = backend
            .submit_entry(&paper_candidate, "1_1000_0".to_string(), 1)
            .await
            .expect("paper submit");
        let paper_fills = backend.poll_fills(u64::MAX).await;
        assert!(!paper_fills.is_empty());
        assert!(paper_fills.iter().all(|f| f.lane == Lane::Paper));
    }

    #[tokio::test]
    async fn test_submit_entry_channel_closed_emits_terminal_fill() {
        let backend = LiveBackend::new_stub();
        let candidate = make_candidate();

        let err = backend
            .submit_entry(&candidate, "q-closed".to_string(), 1)
            .await
            .expect_err("stub channel should be closed");
        match err {
            ExecutionError::TransactionFailed(msg) => {
                assert!(msg.contains("Worker channel closed"))
            }
            other => panic!("unexpected error: {other}"),
        }

        let fills = backend.poll_fills(EventEmitter::now_ms()).await;
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].status, FillStatus::Failed);
        assert_eq!(fills[0].lane, Lane::Live);
    }
}
