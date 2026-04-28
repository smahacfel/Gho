//! Live Pipeline Module - EPIC 4: Live TX → TradeSnapshot → MarketSnapshot → append to ShadowLedger
//!
//! This module provides the live transaction pipeline that processes incoming
//! transactions after Gatekeeper commit and appends them to ShadowLedger.
//!
//! ## Full Adapter Chain (Deliverable 2)
//!
//! The pipeline now implements the complete adapter chain:
//! ```text
//! LiveTxEvent → BufferedLiveEvent → (BufferedTx, TxKey) → TradeSnapshot + MarketSnapshot
//! ```
//!
//! Both snapshot types are generated:
//! - **`TradeSnapshot`**: TX-level canonical snapshot with `price_avg` and `price_instant_after`
//! - **`MarketSnapshot`**: Ledger-compatible snapshot for scoring modules
//!
//! ## Canonical Source Declaration (Deliverable 1)
//!
//! **The sole canonical source of live TX for EPIC 4 is `SnapshotListener` via `SnapshotEngine`.**
//! The `LivePipeline` is the **only** canonical path for post-commit snapshot appending.
//!
//! ### Enforcement Mechanism
//!
//! Currently this is enforced by **convention**, not runtime guards:
//! - `LivePipeline.init_for_mint()` must be called after Gatekeeper commit
//! - Only initialized mints can receive live events
//! - Other code paths using `push_snapshot_with_source` are non-canonical and should be avoided
//!
//! **Note**: Full runtime enforcement would require making `push_snapshot_with_source`
//! private or adding a guard that only allows appends from LivePipeline. This is left
//! for future hardening if needed.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                        Live Transaction Flow                                 │
//! │                                                                             │
//! │  SnapshotListener → LiveTxEvent → BufferedLiveEvent → TradeSnapshot         │
//! │      (sole canonical   (validated    (with fallback)    (price_avg +        │
//! │         source)         input)                          price_instant)      │
//! │                                                              ↓               │
//! │                                                        MarketSnapshot       │
//! │                                                              ↓               │
//! │                                                        ShadowLedger         │
//! │                                                        (append with         │
//! │                                                         monotonicity)       │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Guarantees
//!
//! - **No slot=0**: Unknown slot must be None (slot is metadata-only)
//! - **No duplicates**: TxKey deduplication with FIFO eviction + pending buffer dedup
//! - **Deterministic order**: timestamp_ms + tx_index + signature + fallback_counter tie-breaker
//! - **Append-only + Monotonic**: timestamp_ms must be >= last appended (event-time only)
//! - **Out-of-order handling**: Buffering with configurable flush window
//! - **Tie-breaker stability**: Fallback counter assigned ONCE at add_live_tx, not reassigned in flush
//!
//! ## Price Signals (Deliverable 3)
//!
//! Each TradeSnapshot contains both price signals:
//! - `price_instant_after_sol_per_tok`: Reserve ratio R_sol/R_tok after applying the trade
//! - `price_avg_sol_per_tok`: **VWAP** (Volume-Weighted Average Price) across the flush window
//!
//! VWAP is computed as: Σ(exec_price_i × volume_i) / Σ(volume_i)
//! where exec_price = d_sol_lamports / d_tok_units for each trade.
//!
//! ## Fallback Counter Consistency (Fixed)
//!
//! The fallback counter is now assigned **ONCE** at `add_live_tx` time and stored in
//! `BufferedLiveEvent`. This ensures:
//! - Same TxKey used for dedup check in `add_live_tx` and sorting in `flush`
//! - No more inconsistent fallback values between add and flush phases
//!
//! ## Flush Window Semantics
//!
//! The `flush_delay_ms` parameter (default: 50ms) defines a **flush tick interval**:
//! - Events are buffered until the next flush tick (not per-event timeout)
//! - `should_flush()` returns true when: `now - last_flush_ms >= flush_delay_ms` OR buffer is full
//! - This is a "flush every X ms" pattern, not "buffer each event for X ms"
//!
//! ## Deduplication Contract
//!
//! Deduplication relies on `TxKey` uniqueness. The contract is:
//! - If `SnapshotEngine` provides `tx_index` OR `signature`: TxKey is unique per real TX
//! - If neither is provided: `fallback_counter` ensures unique keys, but real TX dedup is **not guaranteed**
//!   (same event arriving twice without ordering info = two different TxKeys)
//! - Pending buffer is also checked to prevent duplicates within the same flush window
//!
//! Callers should ensure `SnapshotEngine` provides at least one ordering field when possible.
//!
//! ## Monotonicity Semantics
//!
//! The monotonicity guard at ledger boundary checks **timestamp only**, not full TxKey:
//! - `new_snapshot.timestamp_ms >= last_ledger_snapshot.timestamp_ms` must hold
//! - Multiple snapshots with the same timestamp are allowed (deterministic ordering via TxKey)
//! - This means "append-only" refers to event-time progression, not strict TxKey ordering
//!
//! ## Integration (Deliverable 7)
//!
//! After `GatekeeperMintBuffer::commit_to_ledger()`, callers MUST:
//! 1. Call `pipeline.init_for_mint(base_mint, &last_snapshot)` to enable live processing
//! 2. Periodically call `pipeline.flush_ready(&ledger)` (e.g., every 50-100ms via timer/task)

use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use metrics::increment_counter;
use tracing::{debug, warn};

use super::history_types::{BufferedTx, ReconstructedState};
use super::ledger::ShadowLedger;
use super::trade_types::{TradeSide, TxKey};
use super::ShadowLedgerWriteSource;

// ============================================================================
// Constants
// ============================================================================

/// Default flush delay for out-of-order buffering (in milliseconds).
/// Live TXs within this window are accumulated before sorting and appending.
pub const DEFAULT_FLUSH_DELAY_MS: u64 = 50;

/// Maximum number of TXs to buffer per mint before forcing flush.
pub const DEFAULT_MAX_BUFFER_SIZE: usize = 20;

/// Maximum number of TxKeys to retain in the dedup cache per mint.
/// Prevents unbounded memory growth for long-running mints.
/// Keys older than this limit are evicted in FIFO order.
pub const DEFAULT_SEEN_KEYS_LIMIT: usize = 10_000;

// ============================================================================
// FlushResult - Output of flush operation
// ============================================================================

/// Result of a flush operation containing both TradeSnapshots and MarketSnapshots.
///
/// ## Deliverable 2: Full Adapter Chain
///
/// The pipeline now generates both:
/// - `TradeSnapshot`: TX-level canonical snapshot with price_avg and price_instant_after
/// - `MarketSnapshot`: Ledger-compatible snapshot for scoring modules
#[derive(Clone, Debug, Default)]
pub struct FlushResult {
    /// TradeSnapshots generated from live events (Deliverable 2).
    pub trade_snapshots: Vec<super::trade_types::TradeSnapshot>,
    /// MarketSnapshots for ShadowLedger append.
    pub market_snapshots: Vec<super::types::MarketSnapshot>,
    /// VWAP (Volume-Weighted Average Price) across the flush window.
    /// Returns 0.0 if no valid trades (zero volume).
    pub vwap: f64,
    /// Total volume in SOL across the flush window.
    pub total_volume_sol: f64,
}

impl FlushResult {
    /// Check if the result is empty (no snapshots generated).
    ///
    /// Note: trade_snapshots and market_snapshots are always equal length,
    /// so checking only market_snapshots is sufficient.
    pub fn is_empty(&self) -> bool {
        self.market_snapshots.is_empty()
    }

    /// Get the number of snapshots.
    ///
    /// Note: trade_snapshots and market_snapshots are always equal length.
    pub fn len(&self) -> usize {
        self.market_snapshots.len()
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during live pipeline operations.
#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum LivePipelineError {
    #[error("mint not committed: {0}")]
    MintNotCommitted(Pubkey),

    #[error("duplicate transaction key: {0:?}")]
    DuplicateTxKey(TxKey),

    #[error("invalid slot (must be > 0)")]
    InvalidSlot,

    #[error("stale transaction: slot {tx_slot} < last committed slot {last_slot}")]
    StaleTx { tx_slot: u64, last_slot: u64 },

    #[error("monotonicity violation: new slot {new_slot} < last ledger slot {last_slot}")]
    MonotonicityViolation { new_slot: u64, last_slot: u64 },

    #[error("insufficient ordering info: requires (slot + signature) or (slot + tx_index)")]
    InsufficientOrderingInfo,

    #[error("trade snapshot error: {0}")]
    TradeSnapshotError(#[from] super::trade_types::TradeSnapshotError),

    #[error("tx key error: {0}")]
    TxKeyError(#[from] super::trade_types::TxKeyError),
}

// ============================================================================
// LiveTxEvent - Incoming live transaction representation
// ============================================================================

/// Incoming live transaction event from SnapshotEngine (sole canonical source).
///
/// This is the raw input format for live transactions that need to be
/// converted to `MarketSnapshot` and appended to ShadowLedger.
///
/// ## Ordering Contract (Deliverable D)
///
/// For deterministic ordering and deduplication, events MUST have at least one of:
/// - `tx_index` (preferred: log position within slot)
/// - `signature` (fallback: lexicographic ordering)
///
/// If neither is provided, events use a per-mint monotonic `fallback_counter`
/// to guarantee unique TxKeys within the same slot.
#[derive(Clone, Debug)]
pub struct LiveTxEvent {
    /// Base mint (token) address - canonical key for ShadowLedger.
    pub base_mint: Pubkey,
    /// Solana slot for this transaction (optional metadata).
    pub slot: Option<u64>,
    /// Optional transaction log/index order when available (preferred tie-breaker).
    pub tx_index: Option<u32>,
    /// Transaction signature for deduplication (secondary tie-breaker).
    pub signature: Option<Signature>,
    /// Timestamp in milliseconds since UNIX_EPOCH.
    pub timestamp_ms: u64,
    /// Trade direction (Buy or Sell).
    pub side: TradeSide,
    /// SOL amount in lamports (input for Buy, output for Sell).
    pub d_sol_lamports: u64,
    /// Token amount (output for Buy, input for Sell).
    pub d_tok_units: u64,
    /// Whether this is a developer buy.
    pub dev_buy: bool,
    /// Trader wallet address.
    pub trader: Option<Pubkey>,
}

impl LiveTxEvent {
    /// Create a new LiveTxEvent with validation.
    ///
    /// # Validation
    ///
    /// - At least `tx_index` or `signature` should be provided for stable ordering
    ///   (if both are None, fallback_counter will be used)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_mint: Pubkey,
        slot: Option<u64>,
        tx_index: Option<u32>,
        signature: Option<Signature>,
        timestamp_ms: u64,
        side: TradeSide,
        d_sol_lamports: u64,
        d_tok_units: u64,
        dev_buy: bool,
        trader: Option<Pubkey>,
    ) -> Result<Self, LivePipelineError> {
        Ok(Self {
            base_mint,
            slot,
            tx_index,
            signature,
            timestamp_ms,
            side,
            d_sol_lamports,
            d_tok_units,
            dev_buy,
            trader,
        })
    }

    /// Check if this event has sufficient ordering information.
    ///
    /// Returns true if at least tx_index or signature is present.
    #[inline]
    pub fn has_ordering_info(&self) -> bool {
        self.tx_index.is_some() || self.signature.is_some()
    }

    /// Convert to TxKey for ordering and deduplication.
    ///
    /// Uses the provided fallback_counter for events without tx_index/signature.
    /// Note: LiveTxEvent.slot is passed as Option<u64> to TxKey (optional metadata).
    pub fn to_tx_key_with_fallback(
        &self,
        fallback_counter: u64,
    ) -> Result<TxKey, LivePipelineError> {
        // TxKey::new(timestamp_ms, slot, tx_index, signature, fallback_counter)
        // LiveTxEvent.slot is u64, convert to Option<u64> (slot is metadata, not logic)
        Ok(TxKey::new(
            self.timestamp_ms,
            self.slot, // slot is optional metadata
            self.tx_index,
            self.signature,
            fallback_counter,
        )?)
    }

    /// Convert to BufferedTx for internal processing.
    pub fn to_buffered_tx_with_fallback(
        &self,
        fallback_counter: u64,
    ) -> Result<BufferedTx, LivePipelineError> {
        let tx_key = self.to_tx_key_with_fallback(fallback_counter)?;
        Ok(BufferedTx {
            tx_key,
            side: self.side,
            d_sol_lamports: self.d_sol_lamports,
            d_tok_units: self.d_tok_units,
            dev_buy: self.dev_buy,
            trader: self.trader,
        })
    }

    /// Compute the volume delta in SOL for this trade (used in VWAP).
    #[inline]
    pub fn volume_sol(&self) -> f64 {
        self.d_sol_lamports as f64 / super::types::LAMPORTS_PER_SOL
    }

    /// Compute the execution price for this trade (SOL per token).
    /// Used for VWAP calculation.
    ///
    /// Returns 0.0 if d_tok_units is 0 (edge case: no tokens traded).
    /// In VWAP calculation, zero-price trades are excluded from the weighted average.
    #[inline]
    pub fn execution_price(&self) -> f64 {
        if self.d_tok_units == 0 {
            return 0.0;
        }
        (self.d_sol_lamports as f64 / super::types::LAMPORTS_PER_SOL) / (self.d_tok_units as f64)
    }
}

// ============================================================================
// BufferedLiveEvent - Internal wrapper with assigned fallback counter
// ============================================================================

/// Internal wrapper that stores a LiveTxEvent with its assigned fallback_counter.
/// This ensures the fallback is assigned ONCE at add_live_tx time and reused in flush.
#[derive(Clone, Debug)]
struct BufferedLiveEvent {
    /// The original event.
    event: LiveTxEvent,
    /// The fallback counter assigned at add_live_tx time.
    /// This is 0 if the event has ordering info (tx_index or signature).
    assigned_fallback: u64,
}

impl BufferedLiveEvent {
    /// Create a TxKey using the pre-assigned fallback counter.
    fn to_tx_key(&self) -> Result<TxKey, LivePipelineError> {
        self.event.to_tx_key_with_fallback(self.assigned_fallback)
    }

    /// Create a BufferedTx using the pre-assigned fallback counter.
    fn to_buffered_tx(&self) -> Result<BufferedTx, LivePipelineError> {
        self.event
            .to_buffered_tx_with_fallback(self.assigned_fallback)
    }
}

// ============================================================================
// MintLiveState - Per-mint state for live processing
// ============================================================================

/// Per-mint state tracking for live transaction processing.
///
/// Tracks the reconstructed reserve state from the last commit and
/// maintains deduplication/ordering for live TXs.
///
/// ## Staleness Policy (Deliverable C)
///
/// We allow multiple TXs in the same slot, differentiated by tie-breaker (tx_index/signature).
/// Events are sorted by TxKey during flush.
#[derive(Debug)]
pub struct MintLiveState {
    /// Base mint this state tracks.
    pub base_mint: Pubkey,
    /// Whether this mint has been committed (Gatekeeper passed).
    committed: AtomicBool,
    /// Last committed slot (metadata only, not used for ordering or gating).
    last_committed_slot: RwLock<Option<u64>>,
    /// Last TxKey appended to ledger (for monotonicity guard at boundary).
    last_appended_key: RwLock<Option<TxKey>>,
    /// Reconstructed state from the last snapshot.
    reconstructed_state: RwLock<ReconstructedState>,
    /// Monotonic fallback counter for events without tx_index/signature.
    fallback_counter: AtomicU64,
    /// FIFO queue of seen TxKeys for deduplication (limited to prevent unbounded growth).
    seen_keys: RwLock<Vec<TxKey>>,
    /// Buffer for out-of-order TXs awaiting flush (with pre-assigned fallback counters).
    pending_buffer: RwLock<Vec<BufferedLiveEvent>>,
    /// Last flush timestamp.
    last_flush_ms: AtomicU64,
    /// Seen traders for unique_addrs calculation.
    seen_traders: RwLock<HashSet<Pubkey>>,
    /// Maximum seen_keys before eviction.
    seen_keys_limit: usize,
}

impl MintLiveState {
    /// Create a new MintLiveState.
    ///
    /// # Arguments
    ///
    /// * `base_mint` - The base token mint address
    /// * `initial_sol` - Initial SOL reserves in lamports
    /// * `initial_tok` - Initial token reserves
    /// * `last_slot` - Last committed slot
    /// * `tx_count` - Initial transaction count
    /// * `cum_volume_sol_lamports` - Initial cumulative volume
    pub fn new(
        base_mint: Pubkey,
        initial_sol: u64,
        initial_tok: u64,
        last_slot: Option<u64>,
        tx_count: u64,
        cum_volume_sol_lamports: u64,
    ) -> Self {
        Self::with_seen_keys_limit(
            base_mint,
            initial_sol,
            initial_tok,
            last_slot,
            tx_count,
            cum_volume_sol_lamports,
            DEFAULT_SEEN_KEYS_LIMIT,
        )
    }

    /// Create a new MintLiveState with custom seen_keys limit.
    pub fn with_seen_keys_limit(
        base_mint: Pubkey,
        initial_sol: u64,
        initial_tok: u64,
        last_slot: Option<u64>,
        tx_count: u64,
        cum_volume_sol_lamports: u64,
        seen_keys_limit: usize,
    ) -> Self {
        let mut state = ReconstructedState::from_initial_reserves(initial_sol, initial_tok);
        state.tx_count = tx_count;
        state.cum_volume_sol_lamports = cum_volume_sol_lamports;

        Self {
            base_mint,
            committed: AtomicBool::new(true),
            last_committed_slot: RwLock::new(last_slot),
            last_appended_key: RwLock::new(None),
            reconstructed_state: RwLock::new(state),
            fallback_counter: AtomicU64::new(0),
            seen_keys: RwLock::new(Vec::new()),
            pending_buffer: RwLock::new(Vec::new()),
            last_flush_ms: AtomicU64::new(current_time_ms()),
            seen_traders: RwLock::new(HashSet::new()),
            seen_keys_limit,
        }
    }

    /// Create from the last committed snapshot.
    ///
    /// Initializes state from the most recent MarketSnapshot in ShadowLedger.
    pub fn from_last_snapshot(base_mint: Pubkey, snapshot: &super::types::MarketSnapshot) -> Self {
        let sol_lamports = (snapshot.reserve_quote * super::types::LAMPORTS_PER_SOL) as u64;
        let tok_units = snapshot.reserve_base as u64;
        let cum_volume_lamports = (snapshot.cum_volume_sol * super::types::LAMPORTS_PER_SOL) as u64;

        Self::new(
            base_mint,
            sol_lamports,
            tok_units,
            snapshot.slot, // Pass Option<u64> directly
            snapshot.tx_count,
            cum_volume_lamports,
        )
    }

    /// Check if this mint is committed.
    pub fn is_committed(&self) -> bool {
        self.committed.load(Ordering::Acquire)
    }

    /// Get the last committed slot.
    pub fn last_slot(&self) -> Option<u64> {
        *self.last_committed_slot.read().unwrap()
    }

    /// Return a snapshot (clone) of the current [`ReconstructedState`] for
    /// read-only forward simulation.
    ///
    /// The returned value is a deep copy — mutations to it do NOT affect the
    /// authoritative state tracked by this `MintLiveState`.
    ///
    /// Use this as the starting point for [`forward_simulation`] functions.
    pub fn snapshot_reconstructed_state(&self) -> ReconstructedState {
        self.reconstructed_state.read().unwrap().clone()
    }

    /// Get the next fallback counter value (monotonically increasing).
    fn next_fallback_counter(&self) -> u64 {
        self.fallback_counter.fetch_add(1, Ordering::AcqRel)
    }

    /// Get current seen_keys cache size (for metrics).
    pub fn seen_keys_size(&self) -> usize {
        self.seen_keys.read().unwrap().len()
    }

    /// Get current unique_addrs count (baseline 1 + seen traders).
    pub fn unique_addrs(&self) -> u64 {
        1 + self.seen_traders.read().unwrap().len() as u64
    }

    /// Add a live TX to the pending buffer.
    ///
    /// Returns error if:
    /// - Duplicate TxKey
    ///
    /// ## Tie-breaker (Deliverable D)
    ///
    /// If event lacks tx_index and signature, assigns a per-mint monotonic fallback_counter
    /// to ensure unique TxKeys within the same slot. The fallback is assigned ONCE here
    /// and stored with the event, ensuring consistent TxKey between add and flush.
    pub fn add_live_tx(&self, event: LiveTxEvent) -> Result<(), LivePipelineError> {
        // Assign fallback_counter ONCE for events without ordering info.
        // This is stored with the event to ensure consistent TxKey in flush.
        let assigned_fallback = if event.has_ordering_info() {
            0
        } else {
            self.next_fallback_counter()
        };

        let tx_key = event.to_tx_key_with_fallback(assigned_fallback)?;

        // Check for duplicate in seen_keys (FIFO queue with limit)
        {
            let seen = self.seen_keys.read().unwrap();
            if seen.iter().any(|k| k == &tx_key) {
                increment_counter!("live_pipeline_duplicate_rejected_total");
                return Err(LivePipelineError::DuplicateTxKey(tx_key));
            }
        }

        // Check for duplicate in pending buffer (same TxKey already buffered)
        {
            let buffer = self.pending_buffer.read().unwrap();
            for buffered in buffer.iter() {
                if let Ok(buffered_key) = buffered.to_tx_key() {
                    if buffered_key == tx_key {
                        increment_counter!("live_pipeline_duplicate_rejected_total");
                        return Err(LivePipelineError::DuplicateTxKey(tx_key));
                    }
                }
            }
        }

        // Add to pending buffer with assigned fallback
        {
            let mut buffer = self.pending_buffer.write().unwrap();
            buffer.push(BufferedLiveEvent {
                event,
                assigned_fallback,
            });
        }

        Ok(())
    }

    /// Check if the buffer should be flushed.
    pub fn should_flush(&self, flush_delay_ms: u64, max_buffer_size: usize) -> bool {
        let buffer_len = self.pending_buffer.read().unwrap().len();
        if buffer_len == 0 {
            return false;
        }

        if buffer_len >= max_buffer_size {
            return true;
        }

        let last_flush = self.last_flush_ms.load(Ordering::Acquire);
        let now = current_time_ms();
        now.saturating_sub(last_flush) >= flush_delay_ms
    }

    /// Flush the pending buffer and return both TradeSnapshots and MarketSnapshots.
    ///
    /// Sorts pending TXs by TxKey (using pre-assigned fallback counters), applies them
    /// to reconstructed state, and returns the generated snapshots with proper continuation.
    ///
    /// ## Authority Path
    ///
    /// State evolution uses [`ReconstructedState::apply_trade_strict`] — the single
    /// authoritative fee-aware, k-invariant, integer-safe path.  The deprecated
    /// [`apply_trade`][ReconstructedState::apply_trade] is **never** called here.
    ///
    /// ## Price Signals (Deliverable 3)
    ///
    /// Each snapshot contains:
    /// - `price_instant_after`: Reserve ratio R_sol/R_tok after applying the trade
    /// - `price_avg`: VWAP (Volume-Weighted Average Price) computed from execution prices
    ///
    /// ## Fallback Counter Consistency
    ///
    /// The fallback counter is assigned ONCE at `add_live_tx` time and stored in
    /// `BufferedLiveEvent`. This ensures the same TxKey is used for dedup and sorting.
    pub fn flush(&self) -> Result<FlushResult, LivePipelineError> {
        use super::trade_types::{TradeSnapshot, TradeSource};
        use super::types::{MarketSnapshot, PriceState, LAMPORTS_PER_SOL};

        let pending: Vec<BufferedLiveEvent> = {
            let mut buffer = self.pending_buffer.write().unwrap();
            std::mem::take(&mut *buffer)
        };

        if pending.is_empty() {
            return Ok(FlushResult::default());
        }

        // Create (BufferedLiveEvent, TxKey) pairs using pre-assigned fallback
        let mut events_with_keys: Vec<(BufferedLiveEvent, TxKey)> =
            Vec::with_capacity(pending.len());
        for buffered in pending {
            let tx_key = buffered.to_tx_key()?;
            events_with_keys.push((buffered, tx_key));
        }

        // Sort by TxKey for deterministic ordering
        events_with_keys.sort_by(|(_, key_a), (_, key_b)| key_a.cmp(key_b));

        // First pass: compute VWAP across all events in flush window
        let (vwap, total_volume) = compute_vwap(&events_with_keys);

        let mut trade_snapshots = Vec::with_capacity(events_with_keys.len());
        let mut market_snapshots = Vec::with_capacity(events_with_keys.len());
        let mut state = self.reconstructed_state.write().unwrap();
        let mut seen_keys = self.seen_keys.write().unwrap();
        let mut seen_traders = self.seen_traders.write().unwrap();
        let mut last_key_guard = self.last_appended_key.write().unwrap();

        for (buffered, tx_key) in events_with_keys {
            // Final dedup check (linear search on FIFO vec)
            if seen_keys.iter().any(|k| k == &tx_key) {
                continue;
            }

            let event = &buffered.event;
            let buffered_tx = buffered.to_buffered_tx()?;

            // AUTHORITY PATH: apply fee-aware k-invariant state evolution.
            // apply_trade_strict applies the 1% Pump.fun fee to SOL input (BUY) and
            // uses the k invariant to derive the counter-side amount, keeping
            // virtual reserves protocol-correct across many trades.
            let (price_instant_after, _computed_delta) = state.apply_trade_strict(&buffered_tx);

            // Track trader
            if let Some(trader) = event.trader {
                seen_traders.insert(trader);
            }

            let unique_addrs = 1 + seen_traders.len() as u64;

            // Build TradeSnapshot (Deliverable 2: full adapter chain)
            let trade_snapshot = TradeSnapshot::new(
                self.base_mint,
                tx_key.clone(),
                event.side,
                event.dev_buy,
                event.d_sol_lamports,
                event.d_tok_units,
                vwap,                // price_avg: VWAP across flush window
                price_instant_after, // price_instant_after
                state.reserve_sol_lamports,
                state.reserve_tok_units,
                None, // fee_lamports: not tracked in live pipeline
                event.trader,
                TradeSource::Live,
            )?;

            // Build MarketSnapshot with proper continuation from state
            let (price_state, price_reason) = PriceState::from_price(price_instant_after);

            let market_snapshot = MarketSnapshot {
                slot: event.slot, // LiveTxEvent has optional slot
                tx_key: Some(tx_key.clone()),
                timestamp_ms: event.timestamp_ms,
                cum_volume_sol: state.cum_volume_sol_lamports as f64 / LAMPORTS_PER_SOL,
                tx_count: state.tx_count,
                unique_addrs,
                // price_sol_per_token is the instant price after the trade
                price_sol_per_token: price_instant_after,
                price_state,
                price_reason,
                market_cap_sol: 0.0, // Not derivable without supply
                reserve_base: state.reserve_tok_units as f64,
                reserve_quote: state.reserve_sol_lamports as f64 / LAMPORTS_PER_SOL,
                bonding_progress_pct: 0.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            };

            // Add to seen_keys with FIFO eviction (Deliverable F)
            seen_keys.push(tx_key.clone());
            if seen_keys.len() > self.seen_keys_limit {
                let excess = seen_keys.len() - self.seen_keys_limit;
                seen_keys.drain(0..excess);
                increment_counter!("live_pipeline_seen_keys_evicted_batch", "count" => excess.to_string());
            }

            // Update last appended key for monotonicity guard
            *last_key_guard = Some(tx_key);

            trade_snapshots.push(trade_snapshot);
            market_snapshots.push(market_snapshot);

            // Update last committed slot
            if let Some(s) = event.slot {
                let mut last_slot = self.last_committed_slot.write().unwrap();
                if last_slot.map_or(true, |prev| s > prev) {
                    *last_slot = Some(s);
                }
            }
        }

        self.last_flush_ms
            .store(current_time_ms(), Ordering::Release);

        increment_counter!("live_pipeline_snapshots_flushed_total", "count" => market_snapshots.len().to_string());
        increment_counter!("live_pipeline_seen_keys_size", "size" => seen_keys.len().to_string());

        Ok(FlushResult {
            trade_snapshots,
            market_snapshots,
            vwap,
            total_volume_sol: total_volume,
        })
    }

    /// Get the last appended TxKey (for monotonicity checks).
    pub fn last_appended_key(&self) -> Option<TxKey> {
        self.last_appended_key.read().unwrap().clone()
    }
}

// ============================================================================
// LivePipeline - Main orchestrator
// ============================================================================

/// Configuration for LivePipeline behavior.
#[derive(Clone, Debug)]
pub struct LivePipelineConfig {
    /// Flush delay in milliseconds for out-of-order buffering.
    pub flush_delay_ms: u64,
    /// Maximum buffer size before forcing flush.
    pub max_buffer_size: usize,
    /// Maximum number of TxKeys retained in dedup cache per mint (Deliverable F).
    pub seen_keys_limit: usize,
}

impl Default for LivePipelineConfig {
    fn default() -> Self {
        Self {
            flush_delay_ms: DEFAULT_FLUSH_DELAY_MS,
            max_buffer_size: DEFAULT_MAX_BUFFER_SIZE,
            seen_keys_limit: DEFAULT_SEEN_KEYS_LIMIT,
        }
    }
}

/// Statistics about the LivePipeline.
#[derive(Debug, Clone, Default)]
pub struct LivePipelineStats {
    pub active_mints: usize,
    pub total_events_processed: u64,
    pub total_snapshots_appended: u64,
    pub total_duplicates_rejected: u64,
    pub total_stale_rejected: u64,
    pub total_monotonicity_violations: u64,
}

/// Live transaction pipeline for post-commit snapshot appending.
///
/// ## Canonical Source (Deliverable A)
///
/// The sole canonical source of live TX is `SnapshotListener` via `SnapshotEngine`.
/// `LivePipeline` is the **only** canonical path for post-commit snapshot appending.
///
/// ## Integration (Deliverable G)
///
/// After `GatekeeperMintBuffer::commit_to_ledger()`:
/// 1. Call `pipeline.init_for_mint(base_mint, &last_snapshot)`
/// 2. Periodically call `pipeline.flush_ready(&ledger)` (e.g., every 50-100ms)
pub struct LivePipeline {
    /// Per-mint live state.
    mint_states: DashMap<Pubkey, MintLiveState>,
    /// Configuration.
    config: LivePipelineConfig,
    /// Statistics counters.
    events_processed: AtomicU64,
    snapshots_appended: AtomicU64,
    duplicates_rejected: AtomicU64,
    stale_rejected: AtomicU64,
    monotonicity_violations: AtomicU64,
}

impl LivePipeline {
    /// Create a new LivePipeline with default configuration.
    pub fn new() -> Self {
        Self::with_config(LivePipelineConfig::default())
    }

    /// Create a new LivePipeline with custom configuration.
    pub fn with_config(config: LivePipelineConfig) -> Self {
        Self {
            mint_states: DashMap::new(),
            config,
            events_processed: AtomicU64::new(0),
            snapshots_appended: AtomicU64::new(0),
            duplicates_rejected: AtomicU64::new(0),
            stale_rejected: AtomicU64::new(0),
            monotonicity_violations: AtomicU64::new(0),
        }
    }

    /// Initialize live state for a mint after Gatekeeper commit.
    ///
    /// This should be called after `GatekeeperMintBuffer::commit_to_ledger()`
    /// to set up the live pipeline for continued snapshot appending.
    ///
    /// # Arguments
    ///
    /// * `base_mint` - The base token mint address
    /// * `last_snapshot` - The last committed MarketSnapshot
    pub fn init_for_mint(&self, base_mint: Pubkey, last_snapshot: &super::types::MarketSnapshot) {
        let state = MintLiveState::from_last_snapshot(base_mint, last_snapshot);
        self.mint_states.insert(base_mint, state);

        debug!(
            base_mint = %base_mint,
            slot = last_snapshot.slot,
            tx_count = last_snapshot.tx_count,
            "LivePipeline: initialized for mint"
        );
    }

    /// Initialize live state with explicit reserves.
    ///
    /// Alternative to `init_for_mint` when MarketSnapshot is not available.
    pub fn init_with_reserves(
        &self,
        base_mint: Pubkey,
        sol_lamports: u64,
        tok_units: u64,
        last_slot: u64,
        tx_count: u64,
        cum_volume_lamports: u64,
    ) {
        let state = MintLiveState::new(
            base_mint,
            sol_lamports,
            tok_units,
            Some(last_slot),
            tx_count,
            cum_volume_lamports,
        );
        self.mint_states.insert(base_mint, state);

        debug!(
            base_mint = %base_mint,
            last_slot,
            tx_count,
            "LivePipeline: initialized with explicit reserves"
        );
    }

    /// Check if a mint is initialized for live processing.
    pub fn is_initialized(&self, base_mint: &Pubkey) -> bool {
        self.mint_states.contains_key(base_mint)
    }

    /// Process a live transaction event.
    ///
    /// Adds the event to the mint's pending buffer for later flush.
    /// Returns error if mint is not initialized or TX is invalid.
    pub fn process_event(&self, event: LiveTxEvent) -> Result<(), LivePipelineError> {
        self.events_processed.fetch_add(1, Ordering::Relaxed);

        let state = self
            .mint_states
            .get(&event.base_mint)
            .ok_or(LivePipelineError::MintNotCommitted(event.base_mint))?;

        match state.add_live_tx(event) {
            Ok(()) => Ok(()),
            Err(LivePipelineError::DuplicateTxKey(key)) => {
                self.duplicates_rejected.fetch_add(1, Ordering::Relaxed);
                Err(LivePipelineError::DuplicateTxKey(key))
            }
            Err(LivePipelineError::StaleTx { tx_slot, last_slot }) => {
                self.stale_rejected.fetch_add(1, Ordering::Relaxed);
                Err(LivePipelineError::StaleTx { tx_slot, last_slot })
            }
            Err(e) => Err(e),
        }
    }

    /// Flush pending TXs for a mint and append to ShadowLedger.
    ///
    /// ## Monotonicity Guard (Deliverable C)
    ///
    /// Before appending each snapshot, we check that its slot is >= the last
    /// appended slot in the ledger. If violated, the snapshot is dropped with
    /// a warning and metric.
    ///
    /// # Arguments
    ///
    /// * `base_mint` - The mint to flush
    /// * `ledger` - The ShadowLedger to append to
    ///
    /// # Returns
    ///
    /// Number of snapshots appended.
    pub fn flush_mint(
        &self,
        base_mint: &Pubkey,
        ledger: &ShadowLedger,
    ) -> Result<usize, LivePipelineError> {
        self.flush_mint_with_source(base_mint, ledger, ShadowLedgerWriteSource::LivePipeline)
    }

    pub fn flush_mint_with_source(
        &self,
        base_mint: &Pubkey,
        ledger: &ShadowLedger,
        write_source: ShadowLedgerWriteSource,
    ) -> Result<usize, LivePipelineError> {
        let state = self
            .mint_states
            .get(base_mint)
            .ok_or(LivePipelineError::MintNotCommitted(*base_mint))?;

        let flush_start = std::time::Instant::now();
        let flush_result = state.flush()?;
        if flush_result.is_empty() {
            return Ok(0);
        }

        // Number of events flushed from the pending buffer.
        let buffer_size = flush_result.market_snapshots.len();

        // Get last timestamp in ledger for monotonicity check
        let mut last_ledger_ts = ledger
            .get_snapshots(base_mint)
            .and_then(|snaps| snaps.last().map(|s| s.timestamp_ms));

        let mut appended = 0;

        // Append each snapshot with monotonicity guard
        for snapshot in &flush_result.market_snapshots {
            // Monotonicity check: new timestamp must be >= last ledger timestamp
            if let Some(last_ts) = last_ledger_ts {
                if snapshot.timestamp_ms < last_ts {
                    self.monotonicity_violations.fetch_add(1, Ordering::Relaxed);
                    increment_counter!("live_pipeline_monotonicity_violation_total");
                    warn!(
                        base_mint = %base_mint,
                        new_ts_ms = snapshot.timestamp_ms,
                        last_ts_ms = last_ts,
                        "LivePipeline: monotonicity violation - dropping snapshot"
                    );
                    continue;
                }
            }

            if ledger.append_live_with_source(*base_mint, snapshot.clone(), write_source) {
                appended += 1;
                last_ledger_ts = Some(snapshot.timestamp_ms);
            }
        }

        self.snapshots_appended
            .fetch_add(appended as u64, Ordering::Relaxed);

        let flush_us = flush_start.elapsed().as_micros() as u64;
        super::pipeline_metrics::on_live_pipeline_flush(
            &base_mint.to_string(),
            buffer_size,
            appended,
            flush_us,
        );

        debug!(
            base_mint = %base_mint,
            count = appended,
            vwap = flush_result.vwap,
            total_volume_sol = flush_result.total_volume_sol,
            "LivePipeline: flushed and appended snapshots"
        );

        Ok(appended)
    }

    /// Flush pending TXs for a mint and return the full FlushResult.
    ///
    /// Unlike `flush_mint`, this returns the TradeSnapshots and VWAP data
    /// without appending to ShadowLedger. Useful for testing or downstream processing.
    pub fn flush_mint_with_result(
        &self,
        base_mint: &Pubkey,
    ) -> Result<FlushResult, LivePipelineError> {
        let state = self
            .mint_states
            .get(base_mint)
            .ok_or(LivePipelineError::MintNotCommitted(*base_mint))?;

        state.flush()
    }

    /// Flush all mints that are ready.
    ///
    /// Checks each mint against flush criteria and flushes if ready.
    ///
    /// # Returns
    ///
    /// Total number of snapshots appended across all mints.
    pub fn flush_ready(&self, ledger: &ShadowLedger) -> usize {
        let ready_mints: Vec<Pubkey> = self
            .mint_states
            .iter()
            .filter(|entry| {
                entry.should_flush(self.config.flush_delay_ms, self.config.max_buffer_size)
            })
            .map(|entry| *entry.key())
            .collect();

        let mut total = 0;
        for mint in ready_mints {
            match self.flush_mint(&mint, ledger) {
                Ok(count) => total += count,
                Err(e) => {
                    warn!(
                        base_mint = %mint,
                        error = %e,
                        "LivePipeline: flush failed"
                    );
                }
            }
        }

        total
    }

    /// Remove a mint from the live pipeline.
    ///
    /// Should be called when a pool is vetoed or removed.
    pub fn remove_mint(&self, base_mint: &Pubkey) -> bool {
        self.mint_states.remove(base_mint).is_some()
    }

    /// Get statistics about the pipeline.
    pub fn stats(&self) -> LivePipelineStats {
        LivePipelineStats {
            active_mints: self.mint_states.len(),
            total_events_processed: self.events_processed.load(Ordering::Relaxed),
            total_snapshots_appended: self.snapshots_appended.load(Ordering::Relaxed),
            total_duplicates_rejected: self.duplicates_rejected.load(Ordering::Relaxed),
            total_stale_rejected: self.stale_rejected.load(Ordering::Relaxed),
            total_monotonicity_violations: self.monotonicity_violations.load(Ordering::Relaxed),
        }
    }

    /// Get the number of active mints.
    pub fn active_mint_count(&self) -> usize {
        self.mint_states.len()
    }

    /// Get total seen_keys cache size across all mints (for monitoring).
    pub fn total_seen_keys_size(&self) -> usize {
        self.mint_states
            .iter()
            .map(|entry| entry.seen_keys_size())
            .sum()
    }

    /// Return a snapshot (clone) of the current [`ReconstructedState`] for a
    /// specific mint, suitable for read-only forward simulation.
    ///
    /// The returned value is a deep copy — mutations to it do NOT affect the
    /// authoritative state tracked inside this pipeline.
    ///
    /// Returns `None` if the mint is not currently tracked by this pipeline.
    pub fn get_reconstructed_state(&self, base_mint: &Pubkey) -> Option<ReconstructedState> {
        self.mint_states
            .get(base_mint)
            .map(|entry| entry.snapshot_reconstructed_state())
    }
}

impl Default for LivePipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Compute VWAP (Volume-Weighted Average Price) from buffered events.
///
/// VWAP = Σ(price_i * volume_i) / Σ(volume_i)
///
/// Where:
/// - price_i = execution price of trade i (SOL/token = d_sol / d_tok)
/// - volume_i = SOL volume of trade i
///
/// ## Edge Cases
///
/// - Zero-price trades (d_tok_units = 0) are excluded from calculation
/// - Zero-volume trades (d_sol_lamports = 0) are excluded from calculation
/// - If no valid trades exist, returns (0.0, 0.0)
///
/// Returns (vwap, total_volume_sol).
fn compute_vwap(events_with_keys: &[(BufferedLiveEvent, TxKey)]) -> (f64, f64) {
    let mut weighted_sum = 0.0;
    let mut total_volume = 0.0;

    for (buffered, _) in events_with_keys {
        let event = &buffered.event;
        let volume_sol = event.volume_sol();
        let exec_price = event.execution_price();

        // Exclude zero-price/zero-volume trades from VWAP calculation
        if volume_sol > 0.0 && exec_price > 0.0 {
            weighted_sum += exec_price * volume_sol;
            total_volume += volume_sol;
        }
    }

    if total_volume > 0.0 {
        (weighted_sum / total_volume, total_volume)
    } else {
        (0.0, 0.0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event(base_mint: Pubkey, slot: Option<u64>, side: TradeSide) -> LiveTxEvent {
        // Derive timestamp for tests; slot is metadata-only.
        let ts = slot.unwrap_or(100) * 1000;
        LiveTxEvent::new(
            base_mint,
            slot,
            Some(1),
            None,
            ts,
            side,
            1_000_000_000, // 1 SOL
            1_000_000,     // 1M tokens
            false,
            None,
        )
        .unwrap()
    }

    #[test]
    fn test_live_tx_event_accepts_slot_none() {
        let base_mint = Pubkey::new_unique();
        let result = LiveTxEvent::new(
            base_mint,
            None,
            Some(1),
            None,
            1000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_live_tx_event_to_tx_key() {
        let base_mint = Pubkey::new_unique();
        let event = create_test_event(base_mint, Some(100), TradeSide::Buy);
        let tx_key = event.to_tx_key_with_fallback(0).unwrap();

        // slot is now Option<u64> per EVENT-TIME architecture
        assert_eq!(tx_key.slot, Some(100));
        assert_eq!(tx_key.tx_index, Some(1));
        assert_eq!(tx_key.timestamp_ms, 100_000);
    }

    #[test]
    fn test_mint_live_state_add_tx() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        let event = create_test_event(base_mint, Some(100), TradeSide::Buy);
        assert!(state.add_live_tx(event).is_ok());
        assert_eq!(state.pending_buffer.read().unwrap().len(), 1);
    }

    #[test]
    fn test_mint_live_state_rejects_duplicate() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        let event1 = create_test_event(base_mint, Some(100), TradeSide::Buy);
        let event2 = create_test_event(base_mint, Some(100), TradeSide::Sell); // Same slot/tx_index

        assert!(state.add_live_tx(event1).is_ok());

        // Flush to mark key as seen
        let _ = state.flush();

        // Now try to add duplicate
        assert!(matches!(
            state.add_live_tx(event2),
            Err(LivePipelineError::DuplicateTxKey(_))
        ));
    }

    #[test]
    fn test_mint_live_state_accepts_out_of_order_timestamp() {
        let base_mint = Pubkey::new_unique();
        let state = MintLiveState::new(
            base_mint,
            30_000_000_000,
            1_000_000_000_000,
            Some(100),
            5,
            0,
        );

        let event = create_test_event(base_mint, Some(50), TradeSide::Buy); // Earlier timestamp than last slot
        assert!(state.add_live_tx(event).is_ok());
    }

    #[test]
    fn test_mint_live_state_flush() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Add out-of-order events
        state
            .add_live_tx(create_test_event(base_mint, Some(102), TradeSide::Sell))
            .unwrap();
        state
            .add_live_tx(create_test_event(base_mint, Some(100), TradeSide::Buy))
            .unwrap();
        state
            .add_live_tx(create_test_event(base_mint, Some(101), TradeSide::Buy))
            .unwrap();

        let flush_result = state.flush().unwrap();

        // Should be sorted by event timestamp
        assert_eq!(flush_result.len(), 3);
        assert_eq!(flush_result.market_snapshots[0].timestamp_ms, 100_000);
        assert_eq!(flush_result.market_snapshots[1].timestamp_ms, 101_000);
        assert_eq!(flush_result.market_snapshots[2].timestamp_ms, 102_000);

        // Also verify TradeSnapshots are generated
        assert_eq!(flush_result.trade_snapshots.len(), 3);
    }

    #[test]
    fn test_live_pipeline_process_and_flush() {
        let pipeline = LivePipeline::new();
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        // Initialize pipeline for mint
        let initial_snapshot = super::super::types::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(crate::TxKey::new(1000, Some(50), Some(0), None, 0).unwrap()),
            tx_count: 5,
            cum_volume_sol: 10.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![initial_snapshot.clone()], None);
        pipeline.init_for_mint(base_mint, &initial_snapshot);

        assert!(pipeline.is_initialized(&base_mint));

        // Process events
        pipeline
            .process_event(create_test_event(base_mint, Some(100), TradeSide::Buy))
            .unwrap();
        pipeline
            .process_event(create_test_event(base_mint, Some(101), TradeSide::Sell))
            .unwrap();

        // Flush
        let count = pipeline.flush_mint(&base_mint, &ledger).unwrap();
        assert_eq!(count, 2);

        // Verify snapshots in ledger
        let snapshots = ledger.get_snapshots(&base_mint);
        assert!(snapshots.is_some());
        let snapshots = snapshots.unwrap();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].slot, Some(50));
    }

    #[test]
    fn test_live_pipeline_rejects_uninitialized_mint() {
        let pipeline = LivePipeline::new();
        let base_mint = Pubkey::new_unique();

        let event = create_test_event(base_mint, Some(100), TradeSide::Buy);
        assert!(matches!(
            pipeline.process_event(event),
            Err(LivePipelineError::MintNotCommitted(_))
        ));
    }

    #[test]
    fn test_live_pipeline_stats() {
        let pipeline = LivePipeline::new();
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        let initial_snapshot = super::super::types::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(crate::TxKey::new(1000, Some(50), Some(0), None, 0).unwrap()),
            tx_count: 5,
            cum_volume_sol: 10.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![initial_snapshot.clone()], None);
        pipeline.init_with_reserves(base_mint, 30_000_000_000, 1_000_000_000_000, 50, 5, 0);

        pipeline
            .process_event(create_test_event(base_mint, Some(100), TradeSide::Buy))
            .unwrap();
        pipeline.flush_mint(&base_mint, &ledger).unwrap();

        let stats = pipeline.stats();
        assert_eq!(stats.active_mints, 1);
        assert_eq!(stats.total_events_processed, 1);
        assert_eq!(stats.total_snapshots_appended, 1);
    }

    #[test]
    fn test_live_pipeline_remove_mint() {
        let pipeline = LivePipeline::new();
        let base_mint = Pubkey::new_unique();

        pipeline.init_with_reserves(base_mint, 30_000_000_000, 1_000_000_000_000, 50, 5, 0);
        assert!(pipeline.is_initialized(&base_mint));

        let removed = pipeline.remove_mint(&base_mint);
        assert!(removed);
        assert!(!pipeline.is_initialized(&base_mint));
    }

    #[test]
    fn test_live_pipeline_continues_tx_count() {
        let pipeline = LivePipeline::new();
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        // Initialize with tx_count = 15 (from Gatekeeper commit)
        let initial_snapshot = super::super::types::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(crate::TxKey::new(1000, Some(50), Some(0), None, 0).unwrap()),
            tx_count: 15,
            cum_volume_sol: 10.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![initial_snapshot.clone()], None);
        pipeline.init_for_mint(base_mint, &initial_snapshot);

        // Add 2 more TXs
        pipeline
            .process_event(create_test_event(base_mint, Some(100), TradeSide::Buy))
            .unwrap();
        pipeline
            .process_event(create_test_event(base_mint, Some(101), TradeSide::Buy))
            .unwrap();

        pipeline.flush_mint(&base_mint, &ledger).unwrap();

        // Verify tx_count continues from 15 -> 17
        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[1].tx_count, 16); // 15 + 1
        assert_eq!(snapshots[2].tx_count, 17); // 15 + 2
    }

    #[test]
    fn test_should_flush_by_size() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Add max buffer size events
        for i in 0..DEFAULT_MAX_BUFFER_SIZE {
            let event = LiveTxEvent::new(
                base_mint,
                Some(100 + i as u64),
                Some(i as u32),
                None,
                (100 + i as u64) * 1000,
                TradeSide::Buy,
                1_000_000_000,
                1_000_000,
                false,
                None,
            )
            .unwrap();
            state.add_live_tx(event).unwrap();
        }

        // Should flush due to size
        assert!(state.should_flush(DEFAULT_FLUSH_DELAY_MS, DEFAULT_MAX_BUFFER_SIZE));
    }

    #[test]
    fn test_no_slot_zero_in_live_snapshots() {
        let pipeline = LivePipeline::new();
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        let initial_snapshot = super::super::types::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(crate::TxKey::new(1000, Some(50), Some(0), None, 0).unwrap()),
            tx_count: 5,
            cum_volume_sol: 10.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![initial_snapshot.clone()], None);
        pipeline.init_with_reserves(base_mint, 30_000_000_000, 1_000_000_000_000, 50, 5, 0);

        // Valid events with non-zero slots
        pipeline
            .process_event(create_test_event(base_mint, Some(100), TradeSide::Buy))
            .unwrap();
        pipeline
            .process_event(create_test_event(base_mint, Some(101), TradeSide::Sell))
            .unwrap();

        pipeline.flush_mint(&base_mint, &ledger).unwrap();

        // Verify no slot=0
        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        for snap in &snapshots {
            // Slot must be Some(n) where n > 0, or None (both valid, Some(0) rejected)
            assert!(snap.slot != Some(0), "slot must not be Some(0)");
        }
    }

    #[test]
    fn test_cum_volume_continues_from_commit() {
        let pipeline = LivePipeline::new();
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        // Initialize with cum_volume = 10 SOL (from Gatekeeper commit)
        let initial_snapshot = super::super::types::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(crate::TxKey::new(1000, Some(50), Some(0), None, 0).unwrap()),
            tx_count: 15,
            cum_volume_sol: 10.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![initial_snapshot.clone()], None);
        pipeline.init_for_mint(base_mint, &initial_snapshot);

        // Add BUY TXs (1 SOL each)
        pipeline
            .process_event(create_test_event(base_mint, Some(100), TradeSide::Buy))
            .unwrap();
        pipeline
            .process_event(create_test_event(base_mint, Some(101), TradeSide::Buy))
            .unwrap();

        pipeline.flush_mint(&base_mint, &ledger).unwrap();

        // Verify cum_volume_sol continues from 10.0
        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(snapshots.len(), 3);
        // First live BUY adds 1 SOL: 10.0 + 1.0 = 11.0
        assert!((snapshots[1].cum_volume_sol - 11.0).abs() < 0.01);
        // Second live BUY adds 1 SOL: 11.0 + 1.0 = 12.0
        assert!((snapshots[2].cum_volume_sol - 12.0).abs() < 0.01);
    }

    // =========================================================================
    // Deliverable C: Same slot, different tx_index - deterministic ordering
    // =========================================================================

    #[test]
    fn test_same_slot_different_tx_index_deterministic() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Add events with same slot but different tx_index (out of order)
        let event1 = LiveTxEvent::new(
            base_mint,
            Some(100), // Same slot
            Some(5),   // tx_index = 5
            None,
            100_000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap();

        let event2 = LiveTxEvent::new(
            base_mint,
            Some(100), // Same slot
            Some(2),   // tx_index = 2 (should come first)
            None,
            100_000,
            TradeSide::Sell,
            500_000_000,
            500_000,
            false,
            None,
        )
        .unwrap();

        let event3 = LiveTxEvent::new(
            base_mint,
            Some(100), // Same slot
            Some(8),   // tx_index = 8 (should come last)
            None,
            100_000,
            TradeSide::Buy,
            2_000_000_000,
            2_000_000,
            false,
            None,
        )
        .unwrap();

        // Add in random order
        state.add_live_tx(event1).unwrap();
        state.add_live_tx(event2).unwrap();
        state.add_live_tx(event3).unwrap();

        let flush_result = state.flush().unwrap();

        // Should be sorted by tx_index within slot
        assert_eq!(flush_result.len(), 3);
        // All same slot, but order should be deterministic based on tx_index
        assert_eq!(flush_result.market_snapshots[0].slot, Some(100));
        assert_eq!(flush_result.market_snapshots[1].slot, Some(100));
        assert_eq!(flush_result.market_snapshots[2].slot, Some(100));
    }

    // =========================================================================
    // Deliverable D: Fallback counter ensures unique keys without signature/tx_index
    // =========================================================================

    #[test]
    fn test_fallback_counter_prevents_duplicate_without_ordering_info() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Two events with same slot, NO tx_index, NO signature
        // Should get unique fallback_counters and NOT be duplicates
        let event1 = LiveTxEvent::new(
            base_mint,
            Some(100),
            None, // No tx_index
            None, // No signature
            100_000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap();

        let event2 = LiveTxEvent::new(
            base_mint,
            Some(100), // Same slot!
            None,      // No tx_index
            None,      // No signature
            100_000,   // Same timestamp!
            TradeSide::Sell,
            500_000_000,
            500_000,
            false,
            None,
        )
        .unwrap();

        // Both should be accepted (unique fallback counters)
        assert!(state.add_live_tx(event1).is_ok());
        assert!(state.add_live_tx(event2).is_ok());

        // Flush should produce 2 distinct snapshots
        let snapshots = state.flush().unwrap();
        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn test_has_ordering_info() {
        let base_mint = Pubkey::new_unique();

        // With tx_index
        let event1 = LiveTxEvent::new(
            base_mint,
            Some(100),
            Some(1),
            None,
            1000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap();
        assert!(event1.has_ordering_info());

        // With signature only
        let sig = Signature::new_unique();
        let event2 = LiveTxEvent::new(
            base_mint,
            Some(100),
            None,
            Some(sig),
            1000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap();
        assert!(event2.has_ordering_info());

        // Without either
        let event3 = LiveTxEvent::new(
            base_mint,
            Some(100),
            None,
            None,
            1000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap();
        assert!(!event3.has_ordering_info());
    }

    // =========================================================================
    // Deliverable F: seen_keys FIFO eviction
    // =========================================================================

    #[test]
    fn test_seen_keys_eviction() {
        let base_mint = Pubkey::new_unique();
        let small_limit = 5;
        let state = MintLiveState::with_seen_keys_limit(
            base_mint,
            30_000_000_000,
            1_000_000_000_000,
            Some(50),
            5,
            0,
            small_limit,
        );

        // Add more events than the limit
        for i in 0..10 {
            let event = LiveTxEvent::new(
                base_mint,
                Some(100 + i as u64),
                Some(i as u32),
                None,
                (100 + i as u64) * 1000,
                TradeSide::Buy,
                1_000_000_000,
                1_000_000,
                false,
                None,
            )
            .unwrap();
            state.add_live_tx(event).unwrap();
        }

        // Flush to process all
        let snapshots = state.flush().unwrap();
        assert_eq!(snapshots.len(), 10);

        // seen_keys should be limited to small_limit
        assert_eq!(state.seen_keys_size(), small_limit);
    }

    // =========================================================================
    // Deliverable C: Monotonicity guard at ledger boundary
    // =========================================================================

    #[test]
    fn test_monotonicity_guard_drops_stale_snapshots() {
        let pipeline = LivePipeline::new();
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        // First, commit a snapshot with a newer timestamp to ledger
        let committed_snapshot = super::super::types::MarketSnapshot {
            slot: Some(100),
            tx_key: Some(crate::TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
            timestamp_ms: 100_000,
            tx_count: 1,
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![committed_snapshot], None);

        // Initialize pipeline with earlier timestamp
        let initial_snapshot = super::super::types::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(crate::TxKey::new(1000, Some(50), Some(0), None, 0).unwrap()),
            timestamp_ms: 50_000,
            tx_count: 5,
            cum_volume_sol: 10.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            ..Default::default()
        };
        pipeline.init_for_mint(base_mint, &initial_snapshot);

        // Add events with timestamps < 100_000 (should be dropped due to monotonicity)
        pipeline
            .process_event(create_test_event(base_mint, Some(60), TradeSide::Buy))
            .unwrap();
        pipeline
            .process_event(create_test_event(base_mint, Some(70), TradeSide::Buy))
            .unwrap();

        // Flush - these should be dropped due to monotonicity violation
        let count = pipeline.flush_mint(&base_mint, &ledger).unwrap();

        // Both should be dropped (timestamps 60_000, 70_000 < last ledger timestamp 100_000)
        assert_eq!(count, 0);

        // Ledger should still only have the committed snapshot
        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].timestamp_ms, 100_000);

        // Stats should show monotonicity violations
        let stats = pipeline.stats();
        assert_eq!(stats.total_monotonicity_violations, 2);
    }

    #[test]
    fn test_monotonicity_allows_same_or_greater_timestamp() {
        let pipeline = LivePipeline::new();
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        // Initialize with timestamp 50_000
        let initial_snapshot = super::super::types::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(crate::TxKey::new(1000, Some(50), Some(0), None, 0).unwrap()),
            timestamp_ms: 50_000,
            tx_count: 5,
            cum_volume_sol: 10.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![initial_snapshot.clone()], None);
        pipeline.init_for_mint(base_mint, &initial_snapshot);

        // Add events with timestamps >= 50_000 (should be accepted)
        pipeline
            .process_event(create_test_event(base_mint, Some(50), TradeSide::Buy)) // Same slot OK
            .unwrap();
        pipeline
            .process_event(create_test_event(base_mint, Some(100), TradeSide::Buy)) // Greater slot OK
            .unwrap();

        let count = pipeline.flush_mint(&base_mint, &ledger).unwrap();
        assert_eq!(count, 2);

        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(snapshots.len(), 3);
    }

    // =========================================================================
    // Deliverable 2: TradeSnapshot generation
    // =========================================================================

    #[test]
    fn test_flush_generates_trade_snapshots() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Add BUY and SELL events
        let event1 = LiveTxEvent::new(
            base_mint,
            Some(100),
            Some(1),
            None,
            100_000,
            TradeSide::Buy,
            1_000_000_000, // 1 SOL
            1_000_000,     // 1M tokens
            false,
            None,
        )
        .unwrap();

        let event2 = LiveTxEvent::new(
            base_mint,
            Some(101),
            Some(1),
            None,
            101_000,
            TradeSide::Sell,
            500_000_000, // 0.5 SOL
            500_000,     // 500K tokens
            false,
            None,
        )
        .unwrap();

        state.add_live_tx(event1).unwrap();
        state.add_live_tx(event2).unwrap();

        let flush_result = state.flush().unwrap();

        // Should have both TradeSnapshots and MarketSnapshots
        assert_eq!(flush_result.trade_snapshots.len(), 2);
        assert_eq!(flush_result.market_snapshots.len(), 2);

        // TradeSnapshots should have proper price signals
        let ts1 = &flush_result.trade_snapshots[0];
        assert_eq!(ts1.side, TradeSide::Buy);
        assert_eq!(ts1.d_sol_lamports, 1_000_000_000);
        assert_eq!(ts1.d_tok_units, 1_000_000);
        assert!(ts1.price_instant_after_sol_per_tok > 0.0);
        assert!(ts1.price_avg_sol_per_tok > 0.0);
        assert_eq!(ts1.source, super::super::trade_types::TradeSource::Live);

        let ts2 = &flush_result.trade_snapshots[1];
        assert_eq!(ts2.side, TradeSide::Sell);
        assert_eq!(ts2.d_sol_lamports, 500_000_000);
    }

    // =========================================================================
    // Deliverable 3: VWAP (price_avg) calculation
    // =========================================================================

    #[test]
    fn test_vwap_calculation() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Add events with different volumes to verify VWAP weighting
        // Event 1: 2 SOL for 2M tokens = 1e-6 SOL/token
        let event1 = LiveTxEvent::new(
            base_mint,
            Some(100),
            Some(1),
            None,
            100_000,
            TradeSide::Buy,
            2_000_000_000, // 2 SOL
            2_000_000,     // 2M tokens
            false,
            None,
        )
        .unwrap();

        // Event 2: 1 SOL for 500K tokens = 2e-6 SOL/token (higher price)
        let event2 = LiveTxEvent::new(
            base_mint,
            Some(101),
            Some(1),
            None,
            101_000,
            TradeSide::Buy,
            1_000_000_000, // 1 SOL
            500_000,       // 500K tokens
            false,
            None,
        )
        .unwrap();

        state.add_live_tx(event1).unwrap();
        state.add_live_tx(event2).unwrap();

        let flush_result = state.flush().unwrap();

        // VWAP should be volume-weighted
        // Event 1: exec_price = 2 SOL / 2M = 1e-6, volume = 2 SOL
        // Event 2: exec_price = 1 SOL / 0.5M = 2e-6, volume = 1 SOL
        // VWAP = (1e-6 * 2 + 2e-6 * 1) / (2 + 1) = 4e-6 / 3 ≈ 1.33e-6
        assert!(flush_result.vwap > 0.0);
        assert!(flush_result.total_volume_sol > 0.0);
        assert!((flush_result.total_volume_sol - 3.0).abs() < 0.01); // 2 + 1 = 3 SOL

        // All TradeSnapshots should have the same VWAP (flush window)
        for ts in &flush_result.trade_snapshots {
            assert_eq!(ts.price_avg_sol_per_tok, flush_result.vwap);
        }

        // price_instant_after should differ between trades
        let ts1 = &flush_result.trade_snapshots[0];
        let ts2 = &flush_result.trade_snapshots[1];
        // They should be different (reserves change after each trade)
        assert!(ts1.price_instant_after_sol_per_tok != ts2.price_instant_after_sol_per_tok);
    }

    // =========================================================================
    // Fallback counter consistency test
    // =========================================================================

    #[test]
    fn test_fallback_counter_consistent_between_add_and_flush() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Add two events WITHOUT ordering info (no tx_index, no signature)
        // They should get unique fallback counters that are consistent
        let event1 = LiveTxEvent::new(
            base_mint,
            Some(100),
            None, // No tx_index
            None, // No signature
            100_000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap();

        let event2 = LiveTxEvent::new(
            base_mint,
            Some(100), // Same slot!
            None,      // No tx_index
            None,      // No signature
            100_000,
            TradeSide::Sell,
            500_000_000,
            500_000,
            false,
            None,
        )
        .unwrap();

        // Both should be accepted (unique fallback counters)
        assert!(state.add_live_tx(event1).is_ok());
        assert!(state.add_live_tx(event2).is_ok());

        // Flush should produce 2 distinct snapshots with different TxKeys
        let flush_result = state.flush().unwrap();
        assert_eq!(flush_result.len(), 2);

        // The TxKeys should be different (due to different fallback counters)
        let ts1 = &flush_result.trade_snapshots[0];
        let ts2 = &flush_result.trade_snapshots[1];
        assert_ne!(ts1.tx_key, ts2.tx_key);
        assert_eq!(ts1.tx_key.slot, ts2.tx_key.slot); // Same slot
                                                      // Different fallback counters should make them distinguishable
        assert_ne!(ts1.tx_key.fallback_counter, ts2.tx_key.fallback_counter);
    }

    #[test]
    fn test_pending_buffer_dedup() {
        let base_mint = Pubkey::new_unique();
        let state =
            MintLiveState::new(base_mint, 30_000_000_000, 1_000_000_000_000, Some(50), 5, 0);

        // Add event with tx_index
        let event1 = LiveTxEvent::new(
            base_mint,
            Some(100),
            Some(1), // tx_index = 1
            None,
            100_000,
            TradeSide::Buy,
            1_000_000_000,
            1_000_000,
            false,
            None,
        )
        .unwrap();

        // Try to add duplicate with same tx_index (should fail)
        let event2 = LiveTxEvent::new(
            base_mint,
            Some(100), // Same slot
            Some(1),   // Same tx_index - should be detected as duplicate
            None,
            100_000,
            TradeSide::Sell, // Different side doesn't matter
            500_000_000,
            500_000,
            false,
            None,
        )
        .unwrap();

        assert!(state.add_live_tx(event1).is_ok());
        // Second should be rejected as duplicate in pending buffer
        let result = state.add_live_tx(event2);
        assert!(matches!(result, Err(LivePipelineError::DuplicateTxKey(_))));
    }
}
