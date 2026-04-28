//! PaperBackend + PaperBroker — simulated execution with no on-chain transactions.
//!
//! ## Architecture
//!
//! `PaperBackend` implements `ExecutionBackend` and delegates to `PaperBroker`.
//! The broker maintains an order queue with configurable fill delay, jitter,
//! slippage, and failure injection.
//!
//! Fill prices are resolved against [ExecutableQuoteProvider::lookup_nearest]
//! at the simulated fill time — not at the submit time.

use async_trait::async_trait;
use rand::prelude::*;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::execution::backend::*;
use crate::quotes::provider::ExecutableQuoteProvider;

// ─── Config ─────────────────────────────────────────────────────────────────

/// Slippage model for paper fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlippageModel {
    /// No slippage — fill at exact quote price.
    Off,
    /// Apply fixed basis points of slippage.
    FixedBps,
    /// Use the quote's price_impact_pct field.
    ImpactFromQuote,
}

impl Default for SlippageModel {
    fn default() -> Self {
        Self::Off
    }
}

/// Stress injection mode for paper execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StressInjectionMode {
    Off,
    Rules,
    Random,
}

impl Default for StressInjectionMode {
    fn default() -> Self {
        Self::Off
    }
}

/// Configuration for the paper broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperBrokerConfig {
    /// Minimum fill delay in ms.
    pub fill_delay_ms_min: u64,
    /// Maximum fill delay in ms.
    pub fill_delay_ms_max: u64,
    /// Additional jitter in ms (uniform [0, jitter_ms]).
    pub jitter_ms: u64,
    /// Maximum quote age before rejecting as stale.
    pub max_quote_age_ms: u64,
    /// Slippage model.
    pub slippage_model: SlippageModel,
    /// Fixed slippage in bps (used when slippage_model = FixedBps).
    pub slippage_bps_fixed: u64,
    /// Probability of simulated failure [0.0, 1.0].
    pub fail_prob: f64,
    /// Stress injection mode.
    pub stress_injection: StressInjectionMode,
    /// Maximum open positions in paper mode.
    pub max_open_positions_paper: usize,
    /// Candidate sampling rate (1.0 = all, 0.05 = 5%).
    pub candidate_sampling: f64,
    /// RNG seed for reproducibility (0 = system entropy).
    pub rng_seed: u64,
    /// Stress rules config.
    pub stress_rules: StressRulesConfig,
}

impl Default for PaperBrokerConfig {
    fn default() -> Self {
        Self {
            fill_delay_ms_min: 200,
            fill_delay_ms_max: 400,
            jitter_ms: 50,
            max_quote_age_ms: 1500,
            slippage_model: SlippageModel::Off,
            slippage_bps_fixed: 0,
            fail_prob: 0.0,
            stress_injection: StressInjectionMode::Off,
            max_open_positions_paper: 10,
            candidate_sampling: 1.0,
            rng_seed: 0,
            stress_rules: StressRulesConfig::default(),
        }
    }
}

/// Stress injection rule thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressRulesConfig {
    pub parallel_exits_med_threshold: u32,
    pub parallel_exits_high_threshold: u32,
    pub oracle_stale_inject_above_ms: u64,
    pub random_stress_probability: f64,
}

impl Default for StressRulesConfig {
    fn default() -> Self {
        Self {
            parallel_exits_med_threshold: 2,
            parallel_exits_high_threshold: 4,
            oracle_stale_inject_above_ms: 1200,
            random_stress_probability: 0.0,
        }
    }
}

/// Represents a stress bucket transition between two calls.
#[derive(Debug, Clone)]
pub struct StressTransition {
    pub previous_bucket: StressBucket,
    pub new_bucket: StressBucket,
}

// ─── Paper Order ────────────────────────────────────────────────────────────

/// A pending paper order in the broker queue.
#[derive(Debug, Clone)]
pub struct PaperOrder {
    pub order_id: OrderId,
    pub side: OrderSide,
    pub candidate_id: CandidateId,
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub position_id: Option<PositionId>,
    pub fraction_bps: Option<u16>,
    pub quote_ref: QuoteId,
    pub command_ref: Option<CommandId>,
    pub submitted_at_ms: u64,
    pub scheduled_fill_at_ms: u64,
    pub planned_delay_ms: u64,
    pub entry_amount_lamports: u64,
    pub min_tokens_out: u64,
}

// ─── Paper Position ─────────────────────────────────────────────────────────

/// A tracked paper position.
#[derive(Debug, Clone)]
pub struct PaperPosition {
    pub position_id: PositionId,
    pub candidate_id: CandidateId,
    pub base_mint: Pubkey,
    pub pool_amm_id: Pubkey,
    pub entry_price: f64,
    pub entry_qty: u64,
    pub remaining_qty: u64,
    pub epoch: u64,
    pub opened_at_ms: u64,
}

// ─── PaperBroker ────────────────────────────────────────────────────────────

/// Simulated order broker with configurable delay, slippage, and failure.
pub struct PaperBroker {
    config: PaperBrokerConfig,
    /// Pending orders awaiting fill.
    order_queue: VecDeque<PaperOrder>,
    /// Filled orders waiting to be consumed by the owning lifecycle/order poller.
    completed_fills: HashMap<OrderId, FillEvent>,
    /// Open paper positions.
    positions: HashMap<PositionId, PaperPosition>,
    /// Quote provider reference.
    quote_provider: Arc<RwLock<ExecutableQuoteProvider>>,
    /// Seeded RNG for deterministic behavior.
    rng: StdRng,
    /// Order ID counter.
    next_order_id: u64,
    /// Position ID counter.
    next_position_id: u64,
    /// Count of concurrent exit orders pending.
    concurrent_exits: u32,
    /// Previous stress bucket per position (for detecting transitions).
    previous_stress_buckets: HashMap<PositionId, StressBucket>,
}

impl PaperBroker {
    pub fn new(
        config: PaperBrokerConfig,
        quote_provider: Arc<RwLock<ExecutableQuoteProvider>>,
    ) -> Self {
        let rng = if config.rng_seed == 0 {
            StdRng::from_entropy()
        } else {
            StdRng::seed_from_u64(config.rng_seed)
        };

        info!(
            delay = format!("{}..{}ms", config.fill_delay_ms_min, config.fill_delay_ms_max),
            jitter = config.jitter_ms,
            slippage = ?config.slippage_model,
            fail_prob = config.fail_prob,
            stress = ?config.stress_injection,
            max_positions = config.max_open_positions_paper,
            "PaperBroker initialized"
        );

        Self {
            config,
            order_queue: VecDeque::new(),
            completed_fills: HashMap::new(),
            positions: HashMap::new(),
            quote_provider,
            rng,
            next_order_id: 1,
            next_position_id: 1,
            concurrent_exits: 0,
            previous_stress_buckets: HashMap::new(),
        }
    }

    fn generate_order_id(&mut self) -> OrderId {
        let id = self.next_order_id;
        self.next_order_id += 1;
        format!("paper-{}", id)
    }

    pub fn reserve_entry_order_id(&mut self) -> OrderId {
        self.generate_order_id()
    }

    fn generate_position_id(&mut self) -> PositionId {
        let id = self.next_position_id;
        self.next_position_id += 1;
        format!("paper-pos-{}", id)
    }

    fn compute_fill_delay(&mut self) -> u64 {
        let base = self
            .rng
            .gen_range(self.config.fill_delay_ms_min..=self.config.fill_delay_ms_max);
        let jitter = if self.config.jitter_ms > 0 {
            self.rng.gen_range(0..=self.config.jitter_ms)
        } else {
            0
        };
        base + jitter
    }

    fn should_simulate_failure(&mut self) -> bool {
        if self.config.fail_prob <= 0.0 {
            return false;
        }
        self.rng.gen_bool(self.config.fail_prob.clamp(0.0, 1.0))
    }

    fn apply_slippage(&self, base_price: f64, price_impact_pct: f64) -> f64 {
        match self.config.slippage_model {
            SlippageModel::Off => base_price,
            SlippageModel::FixedBps => {
                let slippage_multiplier = 1.0 + (self.config.slippage_bps_fixed as f64 / 10_000.0);
                base_price * slippage_multiplier
            }
            SlippageModel::ImpactFromQuote => {
                let slippage_multiplier = 1.0 + (price_impact_pct / 100.0);
                base_price * slippage_multiplier
            }
        }
    }

    /// Submit an entry order. Returns order_id if accepted.
    pub fn submit_entry(
        &mut self,
        candidate: &CandidateRef,
        quote_ref: QuoteId,
        now_ms: u64,
    ) -> Result<OrderId, ExecutionError> {
        let order_id = self.reserve_entry_order_id();
        self.submit_entry_with_order_id(order_id, candidate, quote_ref, now_ms)
    }

    pub fn submit_entry_with_order_id(
        &mut self,
        order_id: OrderId,
        candidate: &CandidateRef,
        quote_ref: QuoteId,
        now_ms: u64,
    ) -> Result<OrderId, ExecutionError> {
        // Position limit check
        if self.positions.len() >= self.config.max_open_positions_paper {
            return Err(ExecutionError::PositionLimitReached);
        }

        // Candidate sampling gate
        if self.config.candidate_sampling < 1.0 {
            let roll: f64 = self.rng.gen();
            if roll > self.config.candidate_sampling {
                return Err(ExecutionError::SimulatedFailure {
                    reason: format!(
                        "candidate_sampling filter: roll={:.3} > threshold={:.3}",
                        roll, self.config.candidate_sampling
                    ),
                });
            }
        }

        let delay = self.compute_fill_delay();
        let scheduled_fill = now_ms + delay;

        let order = PaperOrder {
            order_id: order_id.clone(),
            side: OrderSide::Entry,
            candidate_id: candidate.candidate_id.clone(),
            pool_amm_id: candidate.pool_amm_id,
            base_mint: candidate.base_mint,
            position_id: None,
            fraction_bps: None,
            quote_ref,
            command_ref: None,
            submitted_at_ms: now_ms,
            scheduled_fill_at_ms: scheduled_fill,
            planned_delay_ms: delay,
            entry_amount_lamports: candidate.entry_amount_lamports,
            min_tokens_out: candidate.min_tokens_out,
        };

        debug!(
            order_id = %order_id,
            mint = %candidate.base_mint,
            delay_ms = delay,
            scheduled_fill = scheduled_fill,
            "PaperBroker: entry order queued"
        );

        self.order_queue.push_back(order);
        Ok(order_id)
    }

    /// Submit an exit order. Returns order_id if accepted.
    pub fn submit_exit(
        &mut self,
        position_id: &PositionId,
        fraction_bps: u16,
        quote_ref: QuoteId,
        command_ref: Option<CommandId>,
        now_ms: u64,
    ) -> Result<OrderId, ExecutionError> {
        // Verify position exists
        if !self.positions.contains_key(position_id) {
            return Err(ExecutionError::TransactionFailed(format!(
                "paper position not found: {}",
                position_id
            )));
        }

        // Clone position data before mutable borrows (order_id gen, delay computation)
        let (pos_candidate_id, pos_pool_amm_id, pos_base_mint) = {
            let pos = &self.positions[position_id];
            (pos.candidate_id.clone(), pos.pool_amm_id, pos.base_mint)
        };
        let order_id = self.generate_order_id();
        let delay = self.compute_fill_delay();
        let scheduled_fill = now_ms + delay;

        let order = PaperOrder {
            order_id: order_id.clone(),
            side: OrderSide::Exit,
            candidate_id: pos_candidate_id,
            pool_amm_id: pos_pool_amm_id,
            base_mint: pos_base_mint,
            position_id: Some(position_id.clone()),
            fraction_bps: Some(fraction_bps),
            quote_ref,
            command_ref,
            submitted_at_ms: now_ms,
            scheduled_fill_at_ms: scheduled_fill,
            planned_delay_ms: delay,
            entry_amount_lamports: 0,
            min_tokens_out: 0,
        };

        debug!(
            order_id = %order_id,
            position_id = %position_id,
            fraction_bps = fraction_bps,
            delay_ms = delay,
            "PaperBroker: exit order queued"
        );

        self.concurrent_exits += 1;
        self.order_queue.push_back(order);
        Ok(order_id)
    }

    /// Poll for orders whose scheduled fill time has passed.
    /// Resolves fill price from the quote nearest to the fill time.
    pub async fn poll_fills(&mut self, now_ms: u64) -> Vec<FillEvent> {
        self.settle_due_orders(now_ms).await;
        self.completed_fills.drain().map(|(_, fill)| fill).collect()
    }

    /// Poll and consume the fill for a single order without stealing fills that belong to
    /// concurrently running lifecycles.
    pub async fn take_fill_for_order(&mut self, order_id: &str, now_ms: u64) -> Option<FillEvent> {
        if let Some(fill) = self.completed_fills.remove(order_id) {
            return Some(fill);
        }

        self.settle_due_orders(now_ms).await;
        self.completed_fills.remove(order_id)
    }

    async fn settle_due_orders(&mut self, now_ms: u64) {
        let mut remaining = VecDeque::new();

        while let Some(order) = self.order_queue.pop_front() {
            if now_ms >= order.scheduled_fill_at_ms {
                let fill = self.resolve_fill(order, now_ms).await;
                self.completed_fills.insert(fill.order_id.clone(), fill);
            } else {
                remaining.push_back(order);
            }
        }

        self.order_queue = remaining;
    }

    async fn resolve_fill(&mut self, order: PaperOrder, now_ms: u64) -> FillEvent {
        let latency = now_ms.saturating_sub(order.submitted_at_ms);

        // Check simulated failure
        if self.should_simulate_failure() {
            warn!(
                order_id = %order.order_id,
                "PaperBroker: simulated failure"
            );
            if order.side == OrderSide::Exit {
                self.concurrent_exits = self.concurrent_exits.saturating_sub(1);
            }
            return FillEvent {
                order_id: order.order_id,
                position_id: order.position_id,
                side: order.side,
                status: FillStatus::Failed,
                fill_price: 0.0,
                fill_qty: 0,
                quote_id_used: order.quote_ref,
                fill_time_ms: now_ms,
                latency_ms: latency,
                lane: Lane::Paper,
            };
        }

        // Resolve fill price from quote provider
        let provider = self.quote_provider.read().await;
        let fill_time = order.scheduled_fill_at_ms;

        // Check if quote is stale
        if provider.is_stale(&order.quote_ref, fill_time) {
            warn!(
                order_id = %order.order_id,
                quote_id = %order.quote_ref,
                "PaperBroker: quote stale at fill time"
            );
            if order.side == OrderSide::Exit {
                self.concurrent_exits = self.concurrent_exits.saturating_sub(1);
            }
            return FillEvent {
                order_id: order.order_id,
                position_id: order.position_id,
                side: order.side,
                status: FillStatus::Stale,
                fill_price: 0.0,
                fill_qty: 0,
                quote_id_used: order.quote_ref,
                fill_time_ms: now_ms,
                latency_ms: latency,
                lane: Lane::Paper,
            };
        }

        // Get the nearest quote to the fill time
        let (fill_price, fill_qty, quote_id_used) =
            if let Some(nearest) = provider.lookup_nearest(&order.pool_amm_id, fill_time) {
                let base_price = nearest.price_sol_per_token;
                let adjusted_price = self.apply_slippage(base_price, nearest.price_impact_pct);

                let qty = match order.side {
                    OrderSide::Entry => {
                        // BUY: tokens received for entry_amount_lamports
                        if adjusted_price > 0.0 {
                            (order.entry_amount_lamports as f64 / adjusted_price) as u64
                        } else {
                            0
                        }
                    }
                    OrderSide::Exit => {
                        // SELL: fraction of remaining position
                        if let Some(ref pos_id) = order.position_id {
                            if let Some(pos) = self.positions.get(pos_id) {
                                let frac = order.fraction_bps.unwrap_or(10_000) as f64 / 10_000.0;
                                (pos.remaining_qty as f64 * frac) as u64
                            } else {
                                0
                            }
                        } else {
                            0
                        }
                    }
                };
                (adjusted_price, qty, nearest.quote_id.clone())
            } else {
                // No quote available — fill at order's quote ref price or fail
                warn!(
                    order_id = %order.order_id,
                    "PaperBroker: no quote available at fill time, using order quote ref"
                );
                if let Some(quote) = provider.get_by_id(&order.quote_ref) {
                    (
                        quote.price_sol_per_token,
                        order.min_tokens_out,
                        order.quote_ref.clone(),
                    )
                } else {
                    if order.side == OrderSide::Exit {
                        self.concurrent_exits = self.concurrent_exits.saturating_sub(1);
                    }
                    return FillEvent {
                        order_id: order.order_id,
                        position_id: order.position_id,
                        side: order.side,
                        status: FillStatus::Failed,
                        fill_price: 0.0,
                        fill_qty: 0,
                        quote_id_used: order.quote_ref,
                        fill_time_ms: now_ms,
                        latency_ms: latency,
                        lane: Lane::Paper,
                    };
                }
            };
        drop(provider);

        // Process: update position state
        match order.side {
            OrderSide::Entry => {
                let position_id = self.generate_position_id();
                let pos = PaperPosition {
                    position_id: position_id.clone(),
                    candidate_id: order.candidate_id.clone(),
                    base_mint: order.base_mint,
                    pool_amm_id: order.pool_amm_id,
                    entry_price: fill_price,
                    entry_qty: fill_qty,
                    remaining_qty: fill_qty,
                    epoch: 1,
                    opened_at_ms: now_ms,
                };

                info!(
                    position_id = %position_id,
                    entry_price = fill_price,
                    qty = fill_qty,
                    "PaperBroker: position opened"
                );

                self.positions.insert(position_id.clone(), pos);

                FillEvent {
                    order_id: order.order_id,
                    position_id: Some(position_id),
                    side: OrderSide::Entry,
                    status: FillStatus::Filled,
                    fill_price,
                    fill_qty,
                    quote_id_used,
                    fill_time_ms: now_ms,
                    latency_ms: latency,
                    lane: Lane::Paper,
                }
            }
            OrderSide::Exit => {
                self.concurrent_exits = self.concurrent_exits.saturating_sub(1);
                let position_id = order.position_id.clone().unwrap_or_default();
                let is_closed;

                // Update position remaining qty
                if let Some(pos) = self.positions.get_mut(&position_id) {
                    pos.remaining_qty = pos.remaining_qty.saturating_sub(fill_qty);
                    is_closed = pos.remaining_qty == 0;
                } else {
                    is_closed = false;
                }

                if is_closed {
                    if let Some(closed_pos) = self.positions.remove(&position_id) {
                        info!(
                            position_id = %position_id,
                            duration_ms = now_ms.saturating_sub(closed_pos.opened_at_ms),
                            "PaperBroker: position closed"
                        );
                    }
                }

                FillEvent {
                    order_id: order.order_id,
                    position_id: Some(position_id),
                    side: OrderSide::Exit,
                    status: FillStatus::Filled,
                    fill_price,
                    fill_qty,
                    quote_id_used,
                    fill_time_ms: now_ms,
                    latency_ms: latency,
                    lane: Lane::Paper,
                }
            }
        }
    }

    /// Get execution stress snapshot based on injection rules.
    /// Returns the snapshot and an optional `StressTransition` if the bucket changed.
    pub fn get_execution_stress(
        &mut self,
        position_id: &PositionId,
    ) -> (ExecutionStressSnapshot, Option<StressTransition>) {
        // Get oracle stale age from quote provider
        let oracle_stale_age_ms = self
            .quote_provider
            .try_read()
            .map(|qp| qp.stale_age_ms())
            .unwrap_or(0);

        let snapshot = match self.config.stress_injection {
            StressInjectionMode::Off => ExecutionStressSnapshot {
                oracle_stale_age_ms,
                ..Default::default()
            },
            StressInjectionMode::Rules => {
                let bucket = if self.concurrent_exits
                    >= self.config.stress_rules.parallel_exits_high_threshold
                {
                    StressBucket::High
                } else if self.concurrent_exits
                    >= self.config.stress_rules.parallel_exits_med_threshold
                {
                    StressBucket::Med
                } else {
                    StressBucket::Low
                };

                ExecutionStressSnapshot {
                    requeue_count: 0,
                    send_fail_count: 0,
                    relax_count: 0,
                    oracle_stale_age_ms,
                    last_sell_attempt_age_ms: None,
                    stress_bucket: bucket,
                    concurrent_exits_count: self.concurrent_exits,
                    injected: true,
                }
            }
            StressInjectionMode::Random => {
                let roll: f64 = self.rng.gen();
                let bucket = if roll < self.config.stress_rules.random_stress_probability {
                    // Pick random bucket
                    match self.rng.gen_range(0..3) {
                        0 => StressBucket::Low,
                        1 => StressBucket::Med,
                        _ => StressBucket::High,
                    }
                } else {
                    StressBucket::Low
                };

                ExecutionStressSnapshot {
                    stress_bucket: bucket,
                    concurrent_exits_count: self.concurrent_exits,
                    oracle_stale_age_ms,
                    injected: true,
                    ..Default::default()
                }
            }
        };

        // Detect stress bucket transition
        let previous = self
            .previous_stress_buckets
            .get(position_id)
            .copied()
            .unwrap_or(StressBucket::Low);
        let current = snapshot.stress_bucket;
        let transition = if previous != current {
            self.previous_stress_buckets
                .insert(position_id.clone(), current);
            Some(StressTransition {
                previous_bucket: previous,
                new_bucket: current,
            })
        } else {
            None
        };

        (snapshot, transition)
    }

    /// Number of open paper positions.
    pub fn open_positions_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of pending orders in the queue.
    pub fn pending_orders_count(&self) -> usize {
        self.order_queue.len()
    }

    /// Get a reference to an open paper position.
    pub fn get_position(&self, position_id: &PositionId) -> Option<&PaperPosition> {
        self.positions.get(position_id)
    }
}

// ─── PaperBackend ───────────────────────────────────────────────────────────

/// PaperBackend wraps PaperBroker to implement ExecutionBackend.
///
/// All execution is simulated — zero on-chain transactions.
pub struct PaperBackend {
    broker: Arc<RwLock<PaperBroker>>,
}

impl PaperBackend {
    pub fn new(broker: PaperBroker) -> Self {
        Self {
            broker: Arc::new(RwLock::new(broker)),
        }
    }

    /// Access the underlying broker (for testing / inspection).
    pub fn broker(&self) -> &Arc<RwLock<PaperBroker>> {
        &self.broker
    }

    /// Retrieve current stress snapshot and optional bucket transition.
    pub fn get_execution_stress_with_transition(
        &self,
        position_id: &PositionId,
    ) -> (ExecutionStressSnapshot, Option<StressTransition>) {
        if let Ok(mut broker) = self.broker.try_write() {
            broker.get_execution_stress(position_id)
        } else {
            (ExecutionStressSnapshot::default(), None)
        }
    }

    pub async fn reserve_entry_order_id(&self) -> OrderId {
        self.broker.write().await.reserve_entry_order_id()
    }

    pub async fn submit_prepared_entry(
        &self,
        attempt: ExecutionAttemptContext,
    ) -> Result<OrderId, ExecutionError> {
        let prepared = attempt.prepared;
        self.broker.write().await.submit_entry_with_order_id(
            prepared.order_id,
            &prepared.candidate,
            prepared.quote.quote_id,
            prepared.submit_time_ms,
        )
    }
}

#[async_trait]
impl ExecutionBackend for PaperBackend {
    async fn submit_entry(
        &self,
        candidate: &CandidateRef,
        quote_ref: QuoteId,
        _position_epoch: u64,
    ) -> Result<OrderId, ExecutionError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.broker
            .write()
            .await
            .submit_entry(candidate, quote_ref, now_ms)
    }

    async fn submit_exit(
        &self,
        position_id: &PositionId,
        fraction_bps: u16,
        quote_ref: QuoteId,
        command_ref: Option<CommandId>,
    ) -> Result<OrderId, ExecutionError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.broker.write().await.submit_exit(
            position_id,
            fraction_bps,
            quote_ref,
            command_ref,
            now_ms,
        )
    }

    async fn poll_fills(&self, now_ms: u64) -> Vec<FillEvent> {
        self.broker.write().await.poll_fills(now_ms).await
    }

    fn get_execution_stress(&self, position_id: &PositionId) -> ExecutionStressSnapshot {
        self.get_execution_stress_with_transition(position_id).0
    }

    fn lane(&self) -> Lane {
        Lane::Paper
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quotes::provider::{QuoteProviderConfig, QuoteSource};

    fn make_quote_provider() -> Arc<RwLock<ExecutableQuoteProvider>> {
        Arc::new(RwLock::new(ExecutableQuoteProvider::new(
            QuoteProviderConfig {
                max_quote_age_ms: 5000, // generous for tests
                ring_buffer_size: 64,
                generation_interval_ms: 100,
                stale_warning_threshold_ms: 3000,
            },
        )))
    }

    fn make_broker(qp: Arc<RwLock<ExecutableQuoteProvider>>) -> PaperBroker {
        PaperBroker::new(
            PaperBrokerConfig {
                fill_delay_ms_min: 100,
                fill_delay_ms_max: 200,
                jitter_ms: 10,
                max_quote_age_ms: 5000,
                rng_seed: 42, // deterministic
                ..Default::default()
            },
            qp,
        )
    }

    fn test_candidate() -> CandidateRef {
        CandidateRef {
            candidate_id: "test-candidate-1".to_string(),
            base_mint: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            entry_amount_lamports: 10_000_000,
            min_tokens_out: 1000,
        }
    }

    #[tokio::test]
    async fn test_paper_entry_fill() {
        let qp = make_quote_provider();
        let candidate = test_candidate();

        // Generate a quote
        {
            let mut provider = qp.write().await;
            provider.generate_quote(
                &candidate.pool_amm_id,
                &candidate.base_mint,
                1000,
                Some(100),
                0.01, // 0.01 SOL/token
                1_000_000,
                100_000_000,
                0.5,
                QuoteSource::BondingCurve,
            );
        }

        let mut broker = make_broker(qp);
        let quote_ref = "100_1000_0".to_string();

        // Submit entry
        let order_id = broker.submit_entry(&candidate, quote_ref, 1000).unwrap();
        assert!(order_id.starts_with("paper-"));
        assert_eq!(broker.pending_orders_count(), 1);

        // Poll before fill time → no fills
        let fills = broker.poll_fills(1050).await;
        assert!(fills.is_empty());
        assert_eq!(broker.pending_orders_count(), 1);

        // Poll after fill time (1000 + ~150 delay ≈ 1200+)
        let fills = broker.poll_fills(1500).await;
        assert_eq!(fills.len(), 1);
        let fill = &fills[0];
        assert_eq!(fill.side, OrderSide::Entry);
        assert_eq!(fill.status, FillStatus::Filled);
        assert!(fill.fill_price > 0.0);
        assert!(fill.fill_qty > 0);
        assert_eq!(fill.lane, Lane::Paper);

        // Position should be opened
        assert_eq!(broker.open_positions_count(), 1);
    }

    #[tokio::test]
    async fn test_paper_exit_and_close() {
        let qp = make_quote_provider();
        let candidate = test_candidate();

        // Setup quote
        {
            let mut provider = qp.write().await;
            provider.generate_quote(
                &candidate.pool_amm_id,
                &candidate.base_mint,
                1000,
                Some(100),
                0.01,
                1_000_000,
                100_000_000,
                0.0,
                QuoteSource::BondingCurve,
            );
        }

        let mut broker = make_broker(qp);
        let quote_ref = "100_1000_0".to_string();

        // Submit + fill entry
        broker
            .submit_entry(&candidate, quote_ref.clone(), 1000)
            .unwrap();
        let fills = broker.poll_fills(1500).await;
        assert_eq!(fills.len(), 1);
        let pos_id = fills[0].position_id.clone().unwrap();

        // Submit full exit
        broker
            .submit_exit(&pos_id, 10_000, quote_ref, None, 2000)
            .unwrap();

        // Fill exit
        let fills = broker.poll_fills(2500).await;
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].side, OrderSide::Exit);
        assert_eq!(fills[0].status, FillStatus::Filled);

        // Position should be closed
        assert_eq!(broker.open_positions_count(), 0);
    }

    #[tokio::test]
    async fn test_paper_position_limit() {
        let qp = make_quote_provider();
        let candidate = test_candidate();

        // Setup quote
        {
            let mut provider = qp.write().await;
            provider.generate_quote(
                &candidate.pool_amm_id,
                &candidate.base_mint,
                1000,
                None,
                0.01,
                1_000_000,
                100_000_000,
                0.0,
                QuoteSource::BondingCurve,
            );
        }

        let mut broker = PaperBroker::new(
            PaperBrokerConfig {
                fill_delay_ms_min: 10,
                fill_delay_ms_max: 20,
                jitter_ms: 0,
                max_open_positions_paper: 2,
                rng_seed: 42,
                ..Default::default()
            },
            qp,
        );

        let qr = "0_1000_0".to_string();

        // Fill 2 entries
        broker.submit_entry(&candidate, qr.clone(), 1000).unwrap();
        broker.submit_entry(&candidate, qr.clone(), 1000).unwrap();
        broker.poll_fills(2000).await;
        assert_eq!(broker.open_positions_count(), 2);

        // 3rd should fail
        let result = broker.submit_entry(&candidate, qr, 2000);
        assert!(matches!(result, Err(ExecutionError::PositionLimitReached)));
    }

    #[tokio::test]
    async fn test_paper_fill_timing() {
        let qp = make_quote_provider();
        let candidate = test_candidate();

        {
            let mut provider = qp.write().await;
            provider.generate_quote(
                &candidate.pool_amm_id,
                &candidate.base_mint,
                1000,
                None,
                0.01,
                1_000_000,
                100_000_000,
                0.0,
                QuoteSource::BondingCurve,
            );
        }

        let mut broker = PaperBroker::new(
            PaperBrokerConfig {
                fill_delay_ms_min: 200,
                fill_delay_ms_max: 400,
                jitter_ms: 50,
                rng_seed: 42,
                ..Default::default()
            },
            qp,
        );

        let qr = "0_1000_0".to_string();
        broker.submit_entry(&candidate, qr, 1000).unwrap();

        // Not filled at 1100 (min delay is 200)
        let fills = broker.poll_fills(1100).await;
        assert!(fills.is_empty());

        // Should be filled by 1500 (max 400 + 50 jitter = 450)
        let fills = broker.poll_fills(1500).await;
        assert_eq!(fills.len(), 1);
    }

    #[tokio::test]
    async fn test_paper_slippage_fixed_bps() {
        let qp = make_quote_provider();
        let candidate = test_candidate();

        {
            let mut provider = qp.write().await;
            provider.generate_quote(
                &candidate.pool_amm_id,
                &candidate.base_mint,
                1000,
                None,
                0.01, // base price
                1_000_000,
                100_000_000,
                0.0,
                QuoteSource::BondingCurve,
            );
        }

        let mut broker = PaperBroker::new(
            PaperBrokerConfig {
                fill_delay_ms_min: 10,
                fill_delay_ms_max: 20,
                jitter_ms: 0,
                slippage_model: SlippageModel::FixedBps,
                slippage_bps_fixed: 100, // 1%
                rng_seed: 42,
                ..Default::default()
            },
            qp,
        );

        let qr = "0_1000_0".to_string();
        broker.submit_entry(&candidate, qr, 1000).unwrap();
        let fills = broker.poll_fills(2000).await;

        let fill_price = fills[0].fill_price;
        // 0.01 * 1.01 = 0.0101
        assert!(
            (fill_price - 0.0101).abs() < 0.0001,
            "Expected ~0.0101, got {}",
            fill_price
        );
    }

    #[tokio::test]
    async fn test_paper_stress_injection_rules() {
        let qp = make_quote_provider();

        let mut broker = PaperBroker::new(
            PaperBrokerConfig {
                stress_injection: StressInjectionMode::Rules,
                stress_rules: StressRulesConfig {
                    parallel_exits_med_threshold: 2,
                    parallel_exits_high_threshold: 4,
                    ..Default::default()
                },
                rng_seed: 42,
                ..Default::default()
            },
            qp,
        );

        let pos_id = "test-pos".to_string();

        // No concurrent exits → Low
        // No concurrent exits → Low (first call, no transition since default is Low)
        let (stress, transition) = broker.get_execution_stress(&pos_id);
        assert_eq!(stress.stress_bucket, StressBucket::Low);
        assert!(stress.injected);
        assert!(transition.is_none()); // No transition (Low → Low)

        // Simulate 2 concurrent exits → Med (transition Low → Med)
        broker.concurrent_exits = 2;
        let (stress, transition) = broker.get_execution_stress(&pos_id);
        assert_eq!(stress.stress_bucket, StressBucket::Med);
        assert!(transition.is_some());
        let t = transition.unwrap();
        assert_eq!(t.previous_bucket, StressBucket::Low);
        assert_eq!(t.new_bucket, StressBucket::Med);

        // Simulate 5 concurrent exits → High (transition Med → High)
        broker.concurrent_exits = 5;
        let (stress, transition) = broker.get_execution_stress(&pos_id);
        assert_eq!(stress.stress_bucket, StressBucket::High);
        assert!(transition.is_some());
        let t = transition.unwrap();
        assert_eq!(t.previous_bucket, StressBucket::Med);
        assert_eq!(t.new_bucket, StressBucket::High);

        // Same bucket again → no transition
        let (stress, transition) = broker.get_execution_stress(&pos_id);
        assert_eq!(stress.stress_bucket, StressBucket::High);
        assert!(transition.is_none());
    }

    #[tokio::test]
    async fn test_paper_backend_lane() {
        let qp = make_quote_provider();
        let broker = make_broker(qp);
        let backend = PaperBackend::new(broker);
        assert_eq!(backend.lane(), Lane::Paper);
    }
}
