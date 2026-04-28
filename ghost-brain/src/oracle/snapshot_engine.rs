//! Snapshot Engine - High-performance market snapshot system for SCR/ULVF/POVC
//!
//! This module provides a centralized, thread-safe system for managing market snapshots
//! across multiple pools. It supports:
//! - Per-pool state management with ring buffers
//! - Bootstrap initialization with synthetic snapshots (g0, g1, g2) for in-engine state only
//! - Real-time accumulation and snapshot emission based on transaction events
//! - Zero-allocation hot path for maximum throughput
//!
//! Designed to feed HyperOracle (SCR/ULVF/POVC) and future modules like HyperPrediction,
//! SSMI, ULVFExtended, and SCRExtended.
//!
//! Time-axis contract (live/scoring path): slot is metadata-only and must remain `Option<u64>`
//! end-to-end. Missing slot must stay `None`; `slot == 0` is invalid and normalized to `None`.

use metrics::{histogram, increment_counter};
use parking_lot::{Mutex, RwLock};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use super::snapshot_metrics::SnapshotMetrics;
use ghost_core::shadow_ledger::types::{
    MarketSnapshot as GhostCoreMarketSnapshot, PriceReason, PriceState,
};
use ghost_core::shadow_ledger::{ShadowLedger, TxKey};
use ghost_core::{coverage_audit, pipeline_coverage, PipelineCoverageStage};
use ghost_core::{EventSemanticEnvelope, EventTimeMetadata};
use seer::types::RawBytesMissingReason;

const MIN_RESERVE_THRESHOLD: f64 = 1e-6;
const SEEN_TX_KEYS_CAPACITY: usize = 4096;
const INACTIVE_TX_BUFFER_CAPACITY: usize = 1024;
const INACTIVE_TX_BUFFER_TTL_MS: u64 = 20_000;

/// Canonical price derivation used across the pipeline (SOL per base token).
///
/// Returns (price, PriceState) where:
/// - **Valid**: Price derived from valid reserves or fallback
/// - **Unknown**: No data available (reserves are 0 or missing)
/// - **Invalid**: Price is NaN, Inf, or negative when data should be complete
///
/// Priority:
/// 1) Valid reserves (finite, > MIN_RESERVE_THRESHOLD) → reserve_quote / reserve_base (quote is SOL) → Valid
/// 2) Else valid fallback price (>0, finite) → Valid
/// 3) Else if fallback is NaN/Inf/negative → Invalid
/// 4) Else → Unknown (no data yet)
pub fn derive_price_canonical(
    reserve_base: f64,
    reserve_quote: f64,
    fallback_price: f64,
) -> (f64, PriceState, Option<PriceReason>) {
    let reserve_reason = if reserve_base.is_finite()
        && reserve_quote.is_finite()
        && (reserve_base <= MIN_RESERVE_THRESHOLD || reserve_quote <= MIN_RESERVE_THRESHOLD)
    {
        Some(if reserve_base == 0.0 && reserve_quote == 0.0 {
            PriceReason::MissingReserves
        } else {
            PriceReason::ZeroOrNearZeroReserves
        })
    } else if !reserve_base.is_finite() || !reserve_quote.is_finite() {
        Some(PriceReason::NonFinite)
    } else {
        None
    };

    if reserve_base.is_finite()
        && reserve_quote.is_finite()
        && reserve_base > MIN_RESERVE_THRESHOLD
        && reserve_quote > MIN_RESERVE_THRESHOLD
    {
        let price = reserve_quote / reserve_base;
        if price.is_finite() && price > 0.0 {
            return (price, PriceState::Valid, None);
        }

        if price.is_nan() || price.is_infinite() {
            return (price, PriceState::Invalid, Some(PriceReason::NonFinite));
        }

        return (price, PriceState::Invalid, Some(PriceReason::NonPositive));
    }

    if fallback_price.is_finite() && fallback_price > 0.0 {
        return (
            fallback_price,
            PriceState::Valid,
            Some(PriceReason::FallbackUsed),
        );
    }

    if fallback_price.is_nan() || fallback_price.is_infinite() {
        return (
            fallback_price,
            PriceState::Invalid,
            Some(PriceReason::NonFinite),
        );
    }

    if fallback_price < 0.0 {
        return (
            fallback_price,
            PriceState::Invalid,
            Some(PriceReason::NonPositive),
        );
    }

    (
        fallback_price,
        PriceState::Unknown,
        Some(reserve_reason.unwrap_or(PriceReason::MissingPriceData)),
    )
}

/// Callback for sending data integrity violations to Guardian
pub type IntegrityViolationCallback = Arc<dyn Fn(IntegrityViolation) + Send + Sync>;

/// Data integrity violation detected by SnapshotEngine
#[derive(Debug, Clone)]
pub struct IntegrityViolation {
    /// Source of the violation (e.g., "SnapshotEngine")
    pub source: String,
    /// Severity level (HardAbort or SoftSync)
    pub severity: IntegritySeverity,
    /// Detailed description
    pub details: String,
    /// Pool that triggered the violation
    pub pool_pubkey: Pubkey,
    /// Timestamp when detected (milliseconds)
    pub timestamp_ms: u64,
}

/// Severity level for data integrity violations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegritySeverity {
    /// Soft violation - jitter, duplicate transaction
    SoftSync,
    /// Hard violation - reorg detected, critical state mismatch
    HardAbort,
}

/// Data source type for truth distinction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    /// Soft truth: WebSocket/Helius/Geyser - fast but potentially unreliable
    SoftTruth,
    /// Hard truth: Direct Solana RPC/Blockstore - slower but authoritative
    HardTruth,
}

/// Configuration for resynchronization behavior
#[derive(Debug, Clone, Copy)]
pub struct ResyncConfig {
    /// Resynchronization interval in slots (default: 10)
    pub resync_interval_slots: u64,
    /// Maximum acceptable volume deviation (as ratio, default: 2.0 = 200%)
    pub max_volume_deviation: f64,
    /// Minimum volume threshold for sanity check (SOL, default: 0.001)
    pub min_volume_threshold_sol: f64,
    /// Enable token graph validation
    pub enable_token_graph_validation: bool,
    /// Enable mint supply validation
    pub enable_mint_supply_validation: bool,
}

/// Extended market snapshot with full state information
///
/// This structure captures a comprehensive view of market state at a specific point in time.
/// All fields are designed to be lightweight and copy-friendly for efficient storage in ring buffers.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct MarketSnapshot {
    /// Timestamp in epoch milliseconds
    pub timestamp_ms: u64,
    /// Provenance of `timestamp_ms` on the event axis.
    #[serde(default)]
    pub event_ts_source: EventTsSource,
    /// Solana slot number (Metadata only)
    pub slot: Option<u64>,

    // Basic market intensity metrics
    /// Cumulative volume in SOL since pool initialization
    pub cum_volume_sol: f64,
    /// Cumulative number of transactions since pool initialization
    pub tx_count: u64,
    /// Number of unique addresses in the window
    pub unique_addrs: usize,

    // Directional flow
    /// Cumulative buy volume in SOL since pool initialization
    pub cum_buy_volume_sol: f64,
    /// Cumulative sell volume in SOL since pool initialization
    pub cum_sell_volume_sol: f64,

    // Window-only metrics (event-time interval)
    /// Number of transactions in the current window
    pub window_tx_count: u32,
    /// Total volume in SOL within the window
    pub window_volume_sol: f32,
    /// Buy volume in SOL within the window
    pub window_buy_volume_sol: f32,
    /// Sell volume in SOL within the window
    pub window_sell_volume_sol: f32,

    // Liquidity state (if available)
    /// Base token reserves (e.g., the token being traded)
    pub reserve_base: f64,
    /// Quote reserves (denominated in SOL)
    pub reserve_quote: f64,
    /// Price of token in quote currency (SOL per base token = reserve_quote / reserve_base)
    pub price_quote: f64,
    /// Epistemic state of price data (Valid, Unknown, or Invalid)
    pub price_state: PriceState,
    /// Reason for Unknown/Invalid price
    pub price_reason: Option<PriceReason>,

    // Additional heuristics
    /// Developer buy activity in lamports (optional)
    pub dev_buy_lamports: u64,

    /// Data source type for truth distinction (0 = SoftTruth, 1 = HardTruth)
    /// Stored as u8 to maintain Copy trait
    pub data_source: u8,
}

impl MarketSnapshot {
    /// Get data source as enum
    pub fn get_data_source(&self) -> DataSource {
        match self.data_source {
            1 => DataSource::HardTruth,
            _ => DataSource::SoftTruth,
        }
    }

    /// Set data source from enum
    pub fn set_data_source(&mut self, source: DataSource) {
        self.data_source = match source {
            DataSource::HardTruth => 1,
            DataSource::SoftTruth => 0,
        };
    }

    /// Backward compatibility helper: Check if price is valid for use
    ///
    /// This provides compatibility with old code that checked `price_valid: bool`.
    /// New code should use `price_state.is_valid()` directly.
    #[inline]
    pub fn price_valid(&self) -> bool {
        self.price_state.is_valid()
    }

    #[inline]
    pub fn decision_event_ts_ms(&self) -> Option<u64> {
        self.event_ts_source.decision_event_ts_ms(self.timestamp_ms)
    }

    /// Convert to ghost_core::MarketSnapshot for ShadowLedger storage
    ///
    /// Maps snapshot_engine fields to ghost_core fields, computing derived values
    /// where necessary. Fields not available in snapshot_engine are set to 0.0.
    pub fn to_ghost_core_snapshot(&self) -> GhostCoreMarketSnapshot {
        // Calculate market cap only from observable data without heuristic constants
        let market_cap_sol = if self.price_state == PriceState::Valid
            && self.price_quote.is_finite()
            && self.price_quote > 0.0
            && self.reserve_base > 0.0
        {
            self.price_quote * self.reserve_base
        } else {
            0.0
        };

        GhostCoreMarketSnapshot {
            slot: self.slot,
            tx_key: None,
            timestamp_ms: self.timestamp_ms,
            cum_volume_sol: self.cum_volume_sol,
            tx_count: self.tx_count,
            unique_addrs: self.unique_addrs as u64,
            price_sol_per_token: self.price_quote,
            price_state: self.price_state,
            price_reason: self.price_reason,
            market_cap_sol,
            reserve_base: self.reserve_base,
            reserve_quote: self.reserve_quote,
            // Bonding progress is unavailable in snapshot_engine; avoid heuristic estimates.
            bonding_progress_pct: 0.0,
            // Derivatives are not available from snapshot_engine data
            // These would need to be computed by comparing multiple snapshots
            d_price_d_volume: 0.0,
            d_price_d_liquidity: 0.0,
            d_price_d_slippage: 0.0,
        }
    }
}

/// Accumulator for pool state and lifetime counters.
///
/// SnapshotEngine keeps current reserves/price plus cumulative counters that
/// are monotonic over the pool lifetime; interval windows are handled separately.
#[derive(Debug, Default, Clone)]
pub struct PoolAccumulators {
    pub reserve_base: f64,
    pub reserve_quote: f64,
    pub price_quote: f64,
    pub cum_tx_count: u64,
    pub cum_volume_sol: f64,
    pub cum_buy_volume_sol: f64,
    pub cum_sell_volume_sol: f64,
}

impl PoolAccumulators {
    /// Reset interval (no-op: cumulative counters are lifetime totals)
    pub fn reset_interval(&mut self) {
        // no-op
    }
}

/// Lightweight transaction record for SOBP/MPCF processing
///
/// Contains essential transaction data needed for per-transaction analysis.
/// Designed to be copy-friendly for efficient storage in ring buffers.
#[derive(Debug, Clone)]
pub struct TransactionRecord {
    /// Solana slot number (metadata only)
    pub slot: Option<u64>,
    /// Transaction signature (for deduplication)
    pub signature: String,
    /// Primary signer (wallet address)
    pub signer: Pubkey,
    /// Volume in SOL
    pub sol_amount: f64,
    /// True if buy, false if sell
    pub is_buy: bool,
    /// True if this is a developer buy
    pub is_dev_buy: bool,
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Explicit provenance for event/ingest time axes.
    pub event_time: EventTimeMetadata,
    /// Source of canonical event timestamp.
    pub event_ts_source: EventTsSource,
    /// Monotonic ingestion sequence used as deterministic tie-breaker.
    pub seq_no: u64,
    /// MPCF payload bytes (optional, for actor classification)
    ///
    /// Contains aggregated instruction data for MPCF entropy analysis.
    /// NOT complete serialized transaction bytes - just instruction payloads.
    /// Only stored if payload size is reasonable (<8KB)
    pub raw_bytes: Option<Vec<u8>>,
    /// Updated price quote from the transaction (used for SOBP early window fallback)
    pub price_quote: Option<f64>,
    /// Reason why MPCF payload is missing (if raw_bytes is None)
    ///
    /// - ProviderDoesNotSupport: WebSocket/Helius don't provide raw bytes
    /// - DroppedUpstream: Bytes were available upstream but lost in pipeline (regression)
    /// - Unknown: gRPC source or cannot determine reason
    pub raw_bytes_missing_reason: RawBytesMissingReason,
}

/// Source used to derive the stored tx/snapshot timestamp.
///
/// Only `Event` and `IngressWall` are decision-eligible. `LegacyCompat`,
/// `Arrival`, and `Wallclock` remain valid for buffering and observability,
/// but they must not outrank or evict decision-eligible transactions from
/// ordering/retention paths that feed the decision event-axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTsSource {
    Event,
    IngressWall,
    LegacyCompat,
    Arrival,
    Wallclock,
}

impl EventTsSource {
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::IngressWall => "ingress_wall",
            Self::LegacyCompat => "legacy_compat",
            Self::Arrival => "arrival",
            Self::Wallclock => "wallclock",
        }
    }

    #[inline]
    pub const fn is_decision_event_source(self) -> bool {
        matches!(self, Self::Event | Self::IngressWall)
    }

    #[inline]
    pub const fn decision_event_ts_ms(self, timestamp_ms: u64) -> Option<u64> {
        if self.is_decision_event_source() && timestamp_ms > 0 {
            Some(timestamp_ms)
        } else {
            None
        }
    }
}

#[inline]
fn resolve_decision_tx_event_timestamp(event: &TxEvent) -> Option<(u64, EventTsSource)> {
    if let Some(chain_ts_ms) = event.event_time.chain_event_ts_ms.filter(|ts| *ts > 0) {
        Some((chain_ts_ms, EventTsSource::Event))
    } else if let Some(ingress_wall_ts_ms) =
        event.event_time.ingress_wall_ts_ms.filter(|ts| *ts > 0)
    {
        Some((ingress_wall_ts_ms, EventTsSource::IngressWall))
    } else {
        None
    }
}

#[inline]
fn resolve_tx_event_timestamp(event: &TxEvent) -> (u64, EventTsSource) {
    if let Some((decision_ts_ms, source)) = resolve_decision_tx_event_timestamp(event) {
        (decision_ts_ms, source)
    } else if event.timestamp_ms > 0 {
        (event.timestamp_ms, EventTsSource::LegacyCompat)
    } else if let Some(arrival_ts) = event.arrival_time_ms {
        (arrival_ts, EventTsSource::Arrival)
    } else {
        (
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            EventTsSource::Wallclock,
        )
    }
}

impl Default for EventTsSource {
    fn default() -> Self {
        Self::LegacyCompat
    }
}

impl TransactionRecord {
    /// Create from TxEvent
    ///
    /// Extracts MPCF payload from event for actor classification
    pub fn from_tx_event(event: &TxEvent) -> Self {
        let (timestamp_ms, event_ts_source) = resolve_tx_event_timestamp(event);
        Self {
            slot: event.slot,
            signature: event.signature.clone().unwrap_or_default(),
            signer: event.signer,
            sol_amount: event.volume_sol,
            is_buy: event.is_buy,
            is_dev_buy: event.is_dev_buy,
            timestamp_ms,
            event_time: event.event_time,
            event_ts_source,
            seq_no: 0,
            price_quote: event.price_quote,
            raw_bytes: event.raw_data.clone(), // MPCF payload from upstream
            raw_bytes_missing_reason: event.raw_data_missing_reason,
        }
    }

    pub fn from_tx_event_with_ts(
        event: &TxEvent,
        event_ts_ms: u64,
        event_ts_source: EventTsSource,
        seq_no: u64,
    ) -> Self {
        Self {
            slot: event.slot,
            signature: event.signature.clone().unwrap_or_default(),
            signer: event.signer,
            sol_amount: event.volume_sol,
            is_buy: event.is_buy,
            is_dev_buy: event.is_dev_buy,
            timestamp_ms: event_ts_ms,
            event_time: event.event_time,
            event_ts_source,
            seq_no,
            price_quote: event.price_quote,
            raw_bytes: event.raw_data.clone(),
            raw_bytes_missing_reason: event.raw_data_missing_reason,
        }
    }

    #[inline]
    pub fn decision_event_ts_ms(&self) -> Option<u64> {
        self.event_ts_source.decision_event_ts_ms(self.timestamp_ms)
    }
}

#[derive(Debug, Clone)]
struct BufferedInactiveTx {
    event: TxEvent,
    buffered_at_ms: u64,
}

impl BufferedInactiveTx {
    fn new(event: &TxEvent, buffered_at_ms: u64) -> Self {
        Self {
            event: event.clone(),
            buffered_at_ms,
        }
    }
}

/// Group of events accumulated for the next snapshot
///
/// Agreguje zdarzenia w oparciu o czas (flow), a nie sloty.
/// Służy do deterministycznego wyznaczania momentu emisji snapshota.
#[derive(Debug, Clone)]
pub struct SnapshotGroup {
    /// Timestamp of the first event in this group (ms)
    pub first_event_ts_ms: u64,
    /// Timestamp of the last event in this group (ms)
    pub last_event_ts_ms: u64,
    /// Source of the last event timestamp in this group.
    pub last_event_ts_source: EventTsSource,
    /// Number of events in this group
    pub event_count: usize,

    // Flow metrics accumulated for this group
    pub accum_volume_sol: f64,
    pub accum_buy_volume_sol: f64,
    pub accum_sell_volume_sol: f64,

    // Unique signers tracking for this group
    pub unique_signers: HashSet<Pubkey>,
}

impl SnapshotGroup {
    /// Create new SnapshotGroup starting at specific event time
    pub fn new(first_ts_ms: u64, first_ts_source: EventTsSource) -> Self {
        Self {
            first_event_ts_ms: first_ts_ms,
            last_event_ts_ms: first_ts_ms,
            last_event_ts_source: first_ts_source,
            event_count: 0,
            accum_volume_sol: 0.0,
            accum_buy_volume_sol: 0.0,
            accum_sell_volume_sol: 0.0,
            unique_signers: HashSet::new(),
        }
    }

    /// Check if snapshot should be created based on time window
    ///
    /// RULE: Snapshot creation condition
    /// A snapshot MUST be created if and only if:
    /// snapshot_ts_ms - current_group.first_event_ts_ms >= snapshot_interval_ms
    pub fn should_create_snapshot(&self, min_window_ms: u64) -> bool {
        self.last_event_ts_ms.saturating_sub(self.first_event_ts_ms) >= min_window_ms
    }

    /// Add event to group
    pub fn add_event(&mut self, ev: &TxEvent, event_ts_ms: u64, event_ts_source: EventTsSource) {
        self.last_event_ts_ms = event_ts_ms;
        self.last_event_ts_source = event_ts_source;
        self.event_count += 1;
        self.accum_volume_sol += ev.volume_sol;

        if ev.is_buy {
            self.accum_buy_volume_sol += ev.volume_sol;
        } else {
            self.accum_sell_volume_sol += ev.volume_sol;
        }

        self.unique_signers.insert(ev.signer);
    }
}

/// Fixed-size ring buffer for market snapshots
///
/// Provides O(1) push operations with no allocations after initialization.
/// Uses power-of-2 capacity for efficient modulo via bitwise AND.
#[derive(Debug, Clone)]
pub struct RingSnapshots {
    buf: Vec<MarketSnapshot>,
    head: usize,
    size: usize,
    capacity: usize,

    // Transaction buffer for SOBP/MPCF processing
    /// Recent transactions for SOBP/MPCF processing.
    ///
    /// Decision-eligible records (`Event` / `IngressWall`) are retained by the
    /// event-axis window. Storage-only clocks (`LegacyCompat` / `Arrival` /
    /// `Wallclock`) may remain in the buffer for observability, but they must
    /// not evict decision-eligible transactions from the decision path.
    transaction_buffer: Vec<TransactionRecord>,
    /// Maximum transaction buffer size
    max_tx_buffer: usize,
}

impl RingSnapshots {
    /// Create a new ring buffer with the specified capacity
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of snapshots to store (should be power of 2 for efficiency)
    pub fn new(capacity: usize) -> Self {
        debug!(capacity = capacity, "Creating new RingSnapshots buffer");
        Self {
            buf: vec![MarketSnapshot::default(); capacity],
            head: 0,
            size: 0,
            capacity,
            transaction_buffer: Vec::with_capacity(128),
            max_tx_buffer: 128,
        }
    }

    /// Push a transaction into the buffer
    ///
    /// Maintains a rolling window of recent transactions for SOBP/MPCF analysis.
    /// The time-based trim follows only decision-eligible event clocks, and
    /// storage-only records are preferentially dropped under capacity pressure.
    pub fn push_transaction(&mut self, tx: TransactionRecord) {
        // Add new transaction
        self.transaction_buffer.push(tx);

        self.trim_transaction_buffer_to_capacity();
        self.trim_transaction_buffer_by_decision_time();
        self.trim_transaction_buffer_to_capacity();
    }

    /// Get recent transactions for SOBP/MPCF processing
    pub fn get_transactions(&self) -> &[TransactionRecord] {
        &self.transaction_buffer
    }

    /// Get transactions within a specific time window
    pub fn get_transactions_since(&self, cutoff_ms: u64) -> Vec<TransactionRecord> {
        self.transaction_buffer
            .iter()
            .filter(|t| t.timestamp_ms >= cutoff_ms)
            .cloned()
            .collect()
    }

    fn trim_transaction_buffer_to_capacity(&mut self) {
        while self.transaction_buffer.len() > self.max_tx_buffer {
            if let Some(idx) = self
                .transaction_buffer
                .iter()
                .position(|tx| tx.decision_event_ts_ms().is_none())
            {
                self.transaction_buffer.remove(idx);
            } else {
                self.transaction_buffer.remove(0);
            }
        }
    }

    fn trim_transaction_buffer_by_decision_time(&mut self) {
        let Some(latest_decision_ts_ms) = self
            .transaction_buffer
            .iter()
            .rev()
            .find_map(TransactionRecord::decision_event_ts_ms)
        else {
            return;
        };

        let cutoff_ts = latest_decision_ts_ms.saturating_sub(10_000);
        self.transaction_buffer.retain(|tx| {
            tx.decision_event_ts_ms()
                .map_or(true, |decision_ts_ms| decision_ts_ms >= cutoff_ts)
        });
    }

    /// Push a new snapshot into the ring buffer with strict time-based retention
    ///
    /// # Arguments
    /// * `snapshot` - New snapshot to push
    /// * `current_ts_ms` - Current event time (for retention check)
    /// * `drop_age_ms` - Max age of snapshots to retain
    ///
    /// # Retention Rule
    /// Drop snapshot if and only if: now_ts_ms - snapshot_ts_ms > drop_age_ms
    pub fn push(&mut self, snapshot: MarketSnapshot, current_ts_ms: u64, drop_age_ms: u64) -> bool {
        // Enforce retention BEFORE pushing new snapshot (or after? Rule says "inside push")
        // "Retention MUST be enforced inside RingSnapshots::push()"
        // "while oldest snapshot violates rule -> drop it"

        while let Some(oldest) = self.peek_oldest() {
            if current_ts_ms.saturating_sub(oldest.timestamp_ms) > drop_age_ms {
                self.drop_oldest();
            } else {
                break;
            }
        }

        let wrapped = self.size == self.capacity;

        self.buf[self.head] = snapshot;
        self.head = self.head.wrapping_add(1) % self.capacity;
        if self.size < self.capacity {
            self.size = self.size.saturating_add(1);
        }

        debug!(
            head = self.head,
            tail = if self.size < self.capacity {
                0
            } else {
                self.head
            },
            len = self.size,
            capacity = self.capacity,
            wrapped = wrapped,
            snapshot_ts_ms = snapshot.timestamp_ms,
            "RingSnapshots::push"
        );

        wrapped
    }

    /// Peek at the oldest snapshot
    fn peek_oldest(&self) -> Option<&MarketSnapshot> {
        if self.size == 0 {
            return None;
        }
        // Start index calculation:
        // If wrapped: (head) is oldest? No, head is insertion point (next write).
        // Elements are at: [head-size .. head-1] (modulo capacity).
        //
        // Example: cap=4, size=2, head=2. Indices: 0, 1. Oldest is 0.
        // (2 + 4 - 2) % 4 = 0. Correct.
        // Example: cap=4, size=4, head=1. Indices: 1, 2, 3, 0. Oldest is 1. (head is at 1, overwriting 1 next).
        // Actually head points to potentially old data if full.
        // Wait.
        // If size < capacity: oldest is at 0? No, oldest is at (head - size).
        // Example: head=2, size=2. Oldest at 0.
        // Example: head=0 (wrapped once), size=4. Oldest at 0?
        //
        // Formula: (head + capacity - size) % capacity
        let idx = (self.head + self.capacity - self.size) % self.capacity;
        Some(&self.buf[idx])
    }

    /// Remove the oldest snapshot
    fn drop_oldest(&mut self) {
        if self.size > 0 {
            self.size -= 1;
            // Note: We don't change head. Head is strictly for NEW inserts.
            // Reducing size effectively moves the logical "tail" forward.
        }
    }

    /// Get the most recent snapshot
    pub fn latest(&self) -> Option<&MarketSnapshot> {
        if self.size == 0 {
            debug!(
                head = self.head,
                len = self.size,
                "RingSnapshots::latest - buffer empty"
            );
            return None;
        }
        let idx = if self.head == 0 {
            self.size.checked_sub(1).unwrap_or(0)
        } else {
            self.head.checked_sub(1).unwrap_or(0)
        };

        debug!(
            head = self.head,
            len = self.size,
            latest_idx = idx,
            latest_ts_ms = self.buf[idx].timestamp_ms,
            "RingSnapshots::latest"
        );

        Some(&self.buf[idx])
    }

    /// Get the most recent snapshot mutably.
    pub fn latest_mut(&mut self) -> Option<&mut MarketSnapshot> {
        if self.size == 0 {
            return None;
        }
        let idx = match self.head.checked_sub(1) {
            Some(idx) => idx,
            None => self.size - 1,
        };
        self.buf.get_mut(idx)
    }

    /// Get the last N snapshots (most recent first)
    ///
    /// Returns fewer than N if the buffer doesn't contain that many snapshots yet.
    pub fn get_last_n(&self, n: usize) -> Vec<MarketSnapshot> {
        let count = n.min(self.size);
        let mut result = Vec::with_capacity(count);

        for i in 0..count {
            let offset = i.saturating_add(1);
            let idx = if self.head >= offset {
                self.head.checked_sub(offset).unwrap_or(0)
            } else {
                self.capacity
                    .saturating_add(self.head)
                    .checked_sub(offset)
                    .unwrap_or(0)
            };
            result.push(self.buf[idx]);
        }

        debug!(
            head = self.head,
            len = self.size,
            requested = n,
            returned = count,
            "RingSnapshots::get_last_n"
        );

        result
    }

    /// Get the number of snapshots currently stored
    pub fn len(&self) -> usize {
        self.size
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

/// Per-pool state containing snapshots and accumulators
///
/// Each pool maintains its own independent state, allowing parallel updates
/// without contention across pools.
#[derive(Debug)]
pub struct PoolState {
    /// Pool's on-chain public key
    pub pool_pubkey: Pubkey,
    /// Ring buffer of historical snapshots
    pub snapshots: RingSnapshots,
    /// Current accumulation window
    pub accum: PoolAccumulators,
    /// Timestamp of last emitted snapshot
    pub last_snapshot_ts_ms: u64,
    /// Slot of last emitted snapshot (Metadata)
    pub last_snapshot_slot: Option<u64>,
    /// Timestamp of last activity (any snapshot push or event)
    pub last_activity_ts_ms: u64,
    /// Timestamp of last observed transaction event (event-time or arrival-time)
    pub last_event_ts_ms: u64,
    /// Source of latest canonical event timestamp.
    pub last_event_ts_source: EventTsSource,
    /// Monotonic ingestion counter for deterministic ordering.
    pub ingest_seq_no: u64,
    /// Token mint for this pool (for mint supply validation)
    pub token_mint: Option<Pubkey>,
    /// Last known mint supply (for detecting anomalies)
    pub last_known_mint_supply: Option<u64>,
    /// Active SnapshotGroup for accumulating events
    pub current_group: Option<SnapshotGroup>,
    /// EPIC 3/4: Flag indicating this mint has been committed to LivePipeline
    /// When true, SnapshotEngine stops processing TX for this mint (LivePipeline takes over)
    pub committed_to_live_pipeline: bool,
    /// Dedup cache for transaction keys (event-time primary)
    pub seen_tx_keys: HashSet<TxKey>,
    /// Enrichment quality per TxKey so same-event richer duplicates can upgrade state.
    pub seen_tx_key_quality: HashMap<TxKey, u8>,
    /// FIFO for bounded dedup cache eviction
    pub seen_tx_keys_fifo: VecDeque<TxKey>,
    /// Capacity for dedup cache
    pub seen_tx_keys_capacity: usize,
}

impl PoolState {
    /// Create a new pool state
    pub fn new(pool_pubkey: Pubkey, snapshot_capacity: usize) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            pool_pubkey,
            snapshots: RingSnapshots::new(snapshot_capacity),
            accum: PoolAccumulators::default(),
            last_snapshot_ts_ms: 0,
            last_snapshot_slot: None,
            last_activity_ts_ms: now_ms,
            // Event-time must be sourced from ingestion events, never seeded with wall-clock.
            last_event_ts_ms: 0,
            last_event_ts_source: EventTsSource::LegacyCompat,
            ingest_seq_no: 0,
            token_mint: None,
            last_known_mint_supply: None,
            current_group: None,
            committed_to_live_pipeline: false, // EPIC 3/4: Initially false
            seen_tx_keys: HashSet::with_capacity(SEEN_TX_KEYS_CAPACITY),
            seen_tx_key_quality: HashMap::with_capacity(SEEN_TX_KEYS_CAPACITY),
            seen_tx_keys_fifo: VecDeque::with_capacity(SEEN_TX_KEYS_CAPACITY),
            seen_tx_keys_capacity: SEEN_TX_KEYS_CAPACITY,
        }
    }

    /// Set token mint for this pool (for validation purposes)
    pub fn set_token_mint(&mut self, mint: Pubkey) {
        self.token_mint = Some(mint);
    }

    /// Update mint supply (for supply validation)
    pub fn update_mint_supply(&mut self, supply: u64) {
        self.last_known_mint_supply = Some(supply);
    }

    #[inline]
    pub fn canonical_event_now_ts_ms(&self) -> u64 {
        self.last_event_ts_ms
    }
}

/// Event representing pool initialization
///
/// Captured from WebSocket/Geyser when a new pool is created.
#[derive(Debug, Clone)]
pub struct InitPoolEvent {
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub slot: Option<u64>,
    pub timestamp_ms: u64,
    pub initial_liquidity_sol: f64,
    pub initial_reserve_base: f64,
    pub initial_reserve_quote: f64,
    pub initial_price_quote: f64,
}

/// Event representing a transaction affecting a pool
///
/// Normalized transaction data extracted from WebSocket/Geyser logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolLifecycle {
    Active,
}

impl Default for PoolLifecycle {
    fn default() -> Self {
        PoolLifecycle::Active
    }
}

/// Metryki poola policzone wyłącznie przez Gatekeeper.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct PoolMetrics {
    pub tx_count: u64,
    pub unique_addrs: u64,
    pub volume_sol: f64,
    pub buy_volume_sol: f64,
    pub sell_volume_sol: f64,
    pub dev_buy_lamports: u64,
}

/// Live cumulative counters for a pool (independent of snapshot emission).
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveCounters {
    /// Latest observed storage/order timestamp (ms).
    /// Decision code must gate this through `event_ts_source` and use only
    /// explicit epoch-like sources for event-axis evaluation.
    pub now_ts_ms: u64,
    /// Cumulative number of transactions since pool initialization
    pub cum_tx_count: u64,
    /// Cumulative volume in SOL since pool initialization
    pub cum_volume_sol: f64,
    /// Cumulative buy volume in SOL since pool initialization
    pub cum_buy_volume_sol: f64,
    /// Cumulative sell volume in SOL since pool initialization
    pub cum_sell_volume_sol: f64,
    /// Transaction buffer length (SOBP/MPCF)
    pub tx_buffer_len: usize,
    /// Snapshot ring size
    pub snapshot_count: usize,
    /// Source of latest canonical event-time timestamp.
    pub event_ts_source: EventTsSource,
}

#[derive(Debug, Clone)]
pub struct TxEvent {
    /// Cross-source semantic envelope carried through canonical ingest.
    pub semantic: EventSemanticEnvelope,
    /// Pool this transaction affects
    pub pool_amm_id: Pubkey,
    /// Base mint (token address) - PRIMARY KEY for snapshot storage
    pub base_mint: Pubkey,
    /// Aktualny stan poola (musi być ACTIVE)
    pub pool_state: PoolLifecycle,
    /// Metryki poola dostarczone przez Gatekeeper
    pub metrics: PoolMetrics,
    /// Slot number (metadata only)
    pub slot: Option<u64>,
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Explicit provenance for event/ingest time axes.
    pub event_time: EventTimeMetadata,
    /// Primary signer (for unique address counting)
    pub signer: Pubkey,
    /// True if this is a buy, false if sell
    pub is_buy: bool,
    /// Volume in SOL (or quote currency)
    pub volume_sol: f64,
    /// Updated base reserve (if available)
    pub reserve_base: Option<f64>,
    /// Updated quote reserve (if available)
    pub reserve_quote: Option<f64>,
    /// Updated price (if available)
    pub price_quote: Option<f64>,
    /// True if this is a developer buy
    pub is_dev_buy: bool,
    /// Developer buy amount in lamports
    pub dev_buy_lamports: u64,
    /// Transaction signature (for duplicate detection)
    pub signature: Option<String>,
    /// Intra-transaction event ordinal (0-based index within the same signature).
    ///
    /// Two events with the same signature but different event_ordinal values are
    /// DISTINCT canonical trades (e.g. multi-trade tx) and MUST NOT be deduped.
    /// This becomes `tx_index` in `TxKey` to disambiguate same-signature events.
    pub event_ordinal: Option<u32>,
    /// Block time from on-chain (Unix timestamp in seconds)
    pub block_time: Option<i64>,
    /// System arrival time in milliseconds (when event was received)
    pub arrival_time_ms: Option<u64>,
    /// Data source (soft truth vs hard truth)
    pub data_source: DataSource,
    /// Intra-slot offset in milliseconds (for precise timestamp correction)
    pub intra_slot_offset_ms: Option<u64>,
    /// MPCF payload bytes for actor classification
    ///
    /// Contains aggregated instruction data for MPCF entropy analysis:
    /// - Entropy analysis (bot detection)
    /// - ISS (Instruction Set Signature) variance
    /// - Byte-level fingerprinting
    ///
    /// NOTE: This is NOT complete serialized transaction bytes - just instruction payloads.
    /// If None, system falls back to heuristic-based classification.
    pub raw_data: Option<Vec<u8>>,
    /// Reason why MPCF payload is missing (if raw_data is None)
    ///
    /// Provides epistemic clarity for telemetry:
    /// - ProviderDoesNotSupport: WebSocket/Helius sources (explicit)
    /// - DroppedUpstream: Bug/regression (bytes were available upstream but lost)
    /// - Unknown: gRPC source or cannot determine reason (use source label to distinguish)
    pub raw_data_missing_reason: RawBytesMissingReason,
}

/// Canonical registry of pools approved for tracking
#[derive(Clone, Default)]
pub struct ApprovedPools {
    inner: Arc<RwLock<HashSet<Pubkey>>>,
}

impl ApprovedPools {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn is_approved(&self, pool: &Pubkey) -> bool {
        let guard = self.inner.read();
        guard.contains(pool)
    }

    pub fn insert(&self, pool: Pubkey) -> bool {
        let mut guard = self.inner.write();
        let inserted = guard.insert(pool);
        if inserted {
            debug!(pool = %pool, "ApprovedPools: pool approved");
        }
        inserted
    }

    pub fn remove(&self, pool: &Pubkey) -> bool {
        let mut guard = self.inner.write();
        guard.remove(pool)
    }
}

/// Global snapshot engine managing all pools
///
/// This is the primary interface for snapshot management. It maintains a thread-safe
/// map of all active pools and their states. The design ensures:
/// - Read operations don't block each other (RwLock for pools map)
/// - Updates to different pools don't contend (per-pool Mutex)
/// - Minimal allocations on the hot path
pub struct SnapshotEngine {
    /// Map of pool pubkey to pool state (Arc<Mutex> for fine-grained locking)
    pools: RwLock<HashMap<Pubkey, Arc<Mutex<PoolState>>>>,
    /// Ring buffer capacity for each pool
    snapshot_capacity: usize,
    /// Minimum interval in milliseconds between snapshot emissions
    snapshot_interval_ms: u64,
    /// Metrics collector for monitoring
    metrics: Option<Arc<SnapshotMetrics>>,
    /// Stagnation threshold in milliseconds (default: 2000ms)
    stagnation_threshold_ms: u64,
    /// Callback for sending integrity violations to Guardian
    integrity_callback: Option<IntegrityViolationCallback>,
    /// Maximum jitter tolerance in milliseconds (default: 1500ms)
    max_jitter_ms: u64,
    /// Resynchronization configuration
    resync_config: ResyncConfig,
    /// Optional ShadowLedger reference for real-time snapshot synchronization
    /// When set, every snapshot emission is immediately synced to ShadowLedger
    /// to ensure PredictionEngine sees live market data
    shadow_ledger: Option<Arc<ShadowLedger>>,
    /// Optional registry of approved pools (defense against untracked/legacy pollution)
    approved_pools: Arc<RwLock<Option<Arc<ApprovedPools>>>>,
    /// Track unapproved pools already logged to avoid log spam
    untracked_pools_logged: RwLock<HashSet<Pubkey>>,
    /// Pools aktywowane przez Gatekeeper (SnapshotEngine działa tylko dla ACTIVE)
    active_pools: RwLock<HashSet<Pubkey>>,
    /// Pending InitPoolEvent for pools not yet ACTIVE
    pending_inits: RwLock<HashMap<Pubkey, InitPoolEvent>>,
    /// Transactions received before pool activation (replayed after mark_pool_active)
    pending_inactive_txs: RwLock<HashMap<Pubkey, VecDeque<BufferedInactiveTx>>>,
    /// Per-pool overflow drops while buffering inactive transactions
    inactive_overflow_drops: RwLock<HashMap<Pubkey, u64>>,
    /// Per-pool cap for inactive transaction buffer
    inactive_tx_buffer_capacity: usize,
    /// TTL for inactive transaction buffer entries
    inactive_tx_buffer_ttl_ms: u64,
}

impl Default for ResyncConfig {
    fn default() -> Self {
        Self {
            resync_interval_slots: 10,
            max_volume_deviation: 2.0,
            min_volume_threshold_sol: 0.001,
            enable_token_graph_validation: true,
            enable_mint_supply_validation: true,
        }
    }
}

impl SnapshotEngine {
    #[inline]
    fn normalize_slot_metadata(
        slot: Option<u64>,
        origin: &str,
        pool: Pubkey,
        base_mint: Pubkey,
    ) -> Option<u64> {
        match slot {
            Some(0) => {
                increment_counter!("slot_contract_violation_total");
                warn!(
                    pool = %pool,
                    base_mint = %base_mint,
                    origin = origin,
                    "SLOT_CONTRACT_VIOLATION: received slot=0, normalizing to None"
                );
                None
            }
            other => other,
        }
    }

    fn fallback_counter_from_event(ev: &TxEvent, event_ts_ms: u64) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        event_ts_ms.hash(&mut hasher);
        ev.signer.hash(&mut hasher);
        ev.is_buy.hash(&mut hasher);
        ev.volume_sol.to_bits().hash(&mut hasher);
        if let Some(price) = ev.price_quote {
            price.to_bits().hash(&mut hasher);
        }
        ev.dev_buy_lamports.hash(&mut hasher);
        // event_ordinal included so that two events from the same tx with no signature
        // but different ordinals still get distinct fallback counters.
        if let Some(ord) = ev.event_ordinal {
            ord.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn tx_key_from_event(ev: &TxEvent, event_ts_ms: u64) -> Option<TxKey> {
        if event_ts_ms == 0 {
            return None;
        }
        let signature = ev.signature.as_ref().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Signature::from_str(s).ok()
            }
        });
        let has_ordering_info = signature.is_some();
        let fallback_counter = if has_ordering_info {
            0
        } else {
            Self::fallback_counter_from_event(ev, event_ts_ms)
        };
        TxKey::new(
            event_ts_ms,
            ev.slot,
            ev.event_ordinal,
            signature,
            fallback_counter,
        )
        .ok()
    }

    #[inline]
    fn tx_event_enrichment_score(ev: &TxEvent) -> u8 {
        let mut score = 0u8;
        if ev.reserve_base.is_some() {
            score = score.saturating_add(1);
        }
        if ev.reserve_quote.is_some() {
            score = score.saturating_add(1);
        }
        if ev.price_quote.is_some() {
            score = score.saturating_add(1);
        }
        if matches!(ev.data_source, DataSource::HardTruth) {
            score = score.saturating_add(1);
        }
        score
    }

    fn apply_duplicate_enrichment(
        &self,
        ps: &mut PoolState,
        ev: &TxEvent,
        event_ts_ms: u64,
        event_ts_source: EventTsSource,
        normalized_slot: Option<u64>,
    ) {
        if let Some(rb) = ev.reserve_base {
            ps.accum.reserve_base = rb;
        }
        if let Some(rq) = ev.reserve_quote {
            ps.accum.reserve_quote = rq;
        }
        if let Some(price) = ev.price_quote {
            ps.accum.price_quote = price;
        }

        let Some(snap) = ps.snapshots.latest_mut() else {
            return;
        };

        let (price_quote, price_state, price_reason) = Self::derive_price(
            ps.accum.reserve_base,
            ps.accum.reserve_quote,
            ps.accum.price_quote,
        );
        snap.timestamp_ms = event_ts_ms;
        snap.slot = normalized_slot.or(snap.slot);
        snap.reserve_base = ps.accum.reserve_base;
        snap.reserve_quote = ps.accum.reserve_quote;
        snap.price_quote = price_quote;
        snap.price_state = price_state;
        snap.price_reason = price_reason;
        snap.dev_buy_lamports = ev.metrics.dev_buy_lamports;
        snap.event_ts_source = event_ts_source;
        snap.set_data_source(ev.data_source);

        ps.last_snapshot_ts_ms = snap.timestamp_ms;
        ps.last_snapshot_slot = snap.slot;
        ps.last_activity_ts_ms = Self::now_ms();

        if let Some(ref metrics) = self.metrics {
            metrics.record_price_state(snap.price_state);
        }

        increment_counter!("snapshotengine_duplicate_enrichment_total");
    }
    #[inline]
    fn price_from_reserves(reserve_base: f64, reserve_quote: f64) -> Option<f64> {
        if reserve_base.is_finite()
            && reserve_quote.is_finite()
            && reserve_base > MIN_RESERVE_THRESHOLD
            && reserve_quote > MIN_RESERVE_THRESHOLD
        {
            let price = reserve_quote / reserve_base;
            if price.is_finite() && price > 0.0 {
                return Some(price);
            }
        }
        None
    }

    #[inline]
    fn derive_price(
        reserve_base: f64,
        reserve_quote: f64,
        fallback_price: f64,
    ) -> (f64, PriceState, Option<PriceReason>) {
        derive_price_canonical(reserve_base, reserve_quote, fallback_price)
    }

    /// Create a new snapshot engine
    ///
    /// # Arguments
    /// * `snapshot_capacity` - Number of snapshots to retain per pool (e.g., 128 or 256)
    /// * `snapshot_interval_ms` - Minimum time between snapshots in milliseconds (e.g., 200)
    pub fn new(snapshot_capacity: usize, snapshot_interval_ms: u64) -> Self {
        Self::with_metrics(snapshot_capacity, snapshot_interval_ms, None, None)
    }

    /// Create a new snapshot engine with metrics and custom stagnation threshold
    ///
    /// # Arguments
    /// * `snapshot_capacity` - Number of snapshots to retain per pool
    /// * `snapshot_interval_ms` - Minimum time between snapshots in milliseconds
    /// * `metrics` - Optional metrics collector for monitoring
    /// * `stagnation_threshold_ms` - Optional stagnation threshold (default: 2000ms)
    pub fn with_metrics(
        snapshot_capacity: usize,
        snapshot_interval_ms: u64,
        metrics: Option<Arc<SnapshotMetrics>>,
        stagnation_threshold_ms: Option<u64>,
    ) -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            snapshot_capacity,
            snapshot_interval_ms,
            metrics,
            stagnation_threshold_ms: stagnation_threshold_ms.unwrap_or(2000),
            integrity_callback: None,
            max_jitter_ms: 1500,
            resync_config: ResyncConfig::default(),
            shadow_ledger: None,
            approved_pools: Arc::new(RwLock::new(None)),
            untracked_pools_logged: RwLock::new(HashSet::new()),
            active_pools: RwLock::new(HashSet::new()),
            pending_inits: RwLock::new(HashMap::new()),
            pending_inactive_txs: RwLock::new(HashMap::new()),
            inactive_overflow_drops: RwLock::new(HashMap::new()),
            inactive_tx_buffer_capacity: INACTIVE_TX_BUFFER_CAPACITY,
            inactive_tx_buffer_ttl_ms: INACTIVE_TX_BUFFER_TTL_MS,
        }
    }

    /// Attach ShadowLedger for read-side integrations.
    ///
    /// SnapshotEngine no longer performs normal-path writes to ShadowLedger.
    /// Canonical history remains Gatekeeper-owned; this handle exists only so
    /// callers/tests can share the same ledger instance for external commits
    /// and subsequent readback/validation.
    ///
    /// # Arguments
    /// * `shadow_ledger` - Shared reference to ShadowLedger
    ///
    /// # Example
    /// ```ignore
    /// let mut engine = SnapshotEngine::new(128, 200);
    /// engine.set_shadow_ledger(Arc::clone(&shadow_ledger));
    /// ```
    pub fn set_shadow_ledger(&mut self, shadow_ledger: Arc<ShadowLedger>) {
        self.shadow_ledger = Some(shadow_ledger);
        debug!("ShadowLedger attached to SnapshotEngine for read-side coordination");
    }

    /// Configure inactive-pool buffering policy.
    ///
    /// This controls the per-pool ring buffer used for `TX_BUFFERED_INACTIVE`
    /// and subsequent replay on activation.
    pub fn set_inactive_tx_buffer_policy(&mut self, capacity: usize, ttl_ms: u64) {
        self.inactive_tx_buffer_capacity = capacity.max(1);
        self.inactive_tx_buffer_ttl_ms = ttl_ms.max(1);
    }

    /// Set the approved pools registry used to gate transaction processing
    pub fn set_approved_pools(&self, approved_pools: Arc<ApprovedPools>) {
        *self.approved_pools.write() = Some(approved_pools);
    }

    /// Mark pool as tracked for SnapshotEngine ingest.
    ///
    /// This is intentionally earlier than Gatekeeper approval: SnapshotEngine
    /// needs to observe the pool during the observation window, while approval
    /// gating is handled separately via `ApprovedPools` and LivePipeline commit.
    pub fn track_pool(&self, pool_pubkey: Pubkey) {
        self.active_pools.write().insert(pool_pubkey);
        let pending = { self.pending_inits.write().remove(&pool_pubkey) };
        if let Some(init_event) = pending {
            self.handle_initialize_pool_event(&init_event);
        }

        let buffered = { self.pending_inactive_txs.write().remove(&pool_pubkey) };
        if let Some(mut buffered_txs) = buffered {
            if !buffered_txs.is_empty() {
                let dropped_overflow = self
                    .inactive_overflow_drops
                    .write()
                    .remove(&pool_pubkey)
                    .unwrap_or(0);
                let mut replay_batch: Vec<(usize, BufferedInactiveTx)> =
                    buffered_txs.drain(..).enumerate().collect();
                replay_batch.sort_by(|(left_idx, left), (right_idx, right)| {
                    match (
                        Self::inactive_replay_tx_key(&left.event),
                        Self::inactive_replay_tx_key(&right.event),
                    ) {
                        (Some(left_key), Some(right_key)) => left_key.cmp(&right_key),
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, None) => left_idx.cmp(right_idx),
                    }
                });

                let replay_count = replay_batch.len();
                for (_, buffered_tx) in replay_batch {
                    self.handle_tx_event(&buffered_tx.event);
                }

                increment_counter!("snapshotengine_replayed_inactive_total");
                pipeline_coverage().increment(
                    PipelineCoverageStage::SnapshotEngineReplayed,
                    replay_count as u64,
                );
                ::metrics::counter!(
                    "ghost_pipeline_stage_total",
                    replay_count as u64,
                    "stage" => "snapshot_engine_replayed"
                );
                info!(
                    "BUFFER_REPLAY pool={} replayed={} dropped_overflow={}",
                    pool_pubkey, replay_count, dropped_overflow
                );
            }
        }
        self.inactive_overflow_drops.write().remove(&pool_pubkey);
    }

    /// Backward-compatible alias used by existing call sites/tests.
    pub fn mark_pool_active(&self, pool_pubkey: Pubkey) {
        self.track_pool(pool_pubkey);
    }

    /// Remove pool state and deactivate it (Gatekeeper DROP).
    pub fn remove_pool(&self, pool_pubkey: Pubkey) {
        self.active_pools.write().remove(&pool_pubkey);
        self.pools.write().remove(&pool_pubkey);
        self.untracked_pools_logged.write().remove(&pool_pubkey);
        self.pending_inits.write().remove(&pool_pubkey);
        self.pending_inactive_txs.write().remove(&pool_pubkey);
        self.inactive_overflow_drops.write().remove(&pool_pubkey);
    }

    /// Set resynchronization configuration
    pub fn set_resync_config(&mut self, config: ResyncConfig) {
        self.resync_config = config;
    }

    /// Set the integrity violation callback
    ///
    /// This callback will be invoked whenever a data integrity violation is detected.
    /// It should send the violation to the Guardian watchdog for appropriate action.
    pub fn set_integrity_callback(&mut self, callback: IntegrityViolationCallback) {
        self.integrity_callback = Some(callback);
    }

    /// Set the maximum jitter tolerance in milliseconds
    ///
    /// If arrival_time - corrected_timestamp exceeds this value, a SoftSync violation is triggered.
    /// Default: 1500ms
    pub fn set_max_jitter_ms(&mut self, max_jitter_ms: u64) {
        self.max_jitter_ms = max_jitter_ms;
    }

    /// Get current timestamp in milliseconds
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[inline]
    fn event_time_or_now(ev: &TxEvent) -> u64 {
        resolve_tx_event_timestamp(ev).0
    }

    #[inline]
    fn decision_event_time(ev: &TxEvent) -> Option<u64> {
        resolve_decision_tx_event_timestamp(ev).map(|(ts_ms, _)| ts_ms)
    }

    #[inline]
    fn inactive_replay_tx_key(ev: &TxEvent) -> Option<TxKey> {
        Self::decision_event_time(ev)
            .and_then(|decision_ts_ms| Self::tx_key_from_event(ev, decision_ts_ms))
    }

    fn trim_inactive_queue_by_buffered_age(
        queue: &mut VecDeque<BufferedInactiveTx>,
        now_ms: u64,
        ttl_ms: u64,
    ) {
        while let Some(front) = queue.front() {
            if now_ms.saturating_sub(front.buffered_at_ms) > ttl_ms {
                queue.pop_front();
            } else {
                break;
            }
        }
    }

    fn pop_inactive_overflow_candidate(
        queue: &mut VecDeque<BufferedInactiveTx>,
    ) -> Option<BufferedInactiveTx> {
        if let Some(idx) = queue
            .iter()
            .position(|buffered| Self::decision_event_time(&buffered.event).is_none())
        {
            queue.remove(idx)
        } else {
            queue.pop_front()
        }
    }

    fn buffer_inactive_tx(&self, ev: &TxEvent, reason: &'static str) {
        let now_ms = Self::now_ms();
        let event_ts = Self::event_time_or_now(ev);
        let decision_event_ts = Self::decision_event_time(ev);
        let signature = ev.signature.as_deref().unwrap_or("unknown");
        let mut dropped_overflow = 0u64;
        let pool_id = ev.pool_amm_id.to_string();

        let mut inactive = self.pending_inactive_txs.write();
        let queue = inactive.entry(ev.pool_amm_id).or_insert_with(VecDeque::new);

        Self::trim_inactive_queue_by_buffered_age(queue, now_ms, self.inactive_tx_buffer_ttl_ms);

        if let Some(candidate_key) = Self::tx_key_from_event(ev, event_ts) {
            if queue.iter().any(|existing| {
                let existing_ts = Self::event_time_or_now(&existing.event);
                Self::tx_key_from_event(&existing.event, existing_ts)
                    .as_ref()
                    .is_some_and(|existing_key| existing_key == &candidate_key)
            }) {
                increment_counter!("snapshotengine_buffered_inactive_dedup_total");
                if let Some(signature) = ev.signature.as_deref() {
                    coverage_audit().record_runtime_filtered(
                        &pool_id,
                        signature,
                        "buffered_inactive_duplicate",
                    );
                }
                return;
            }
        }

        queue.push_back(BufferedInactiveTx::new(ev, now_ms));
        while queue.len() > self.inactive_tx_buffer_capacity {
            if let Some(dropped) = Self::pop_inactive_overflow_candidate(queue) {
                if let Some(signature) = dropped.event.signature.as_deref() {
                    coverage_audit().record_runtime_filtered(
                        &dropped.event.pool_amm_id.to_string(),
                        signature,
                        "buffered_inactive_overflow",
                    );
                }
            }
            dropped_overflow = dropped_overflow.saturating_add(1);
        }
        if dropped_overflow > 0 {
            let mut overflow = self.inactive_overflow_drops.write();
            let counter = overflow.entry(ev.pool_amm_id).or_insert(0);
            *counter = counter.saturating_add(dropped_overflow);
            increment_counter!(
                "snapshotengine_buffered_inactive_overflow_total",
                "pool" => ev.pool_amm_id.to_string()
            );
        }

        increment_counter!("snapshotengine_buffered_inactive_total");
        pipeline_coverage().increment(PipelineCoverageStage::SnapshotEngineBuffered, 1);
        increment_counter!("ghost_pipeline_stage_total", "stage" => "snapshot_engine_buffered");
        info!(
            "TX_BUFFERED_INACTIVE sig={} pool={} mint={} reason={} buffered={} ttl_ms={}",
            signature,
            ev.pool_amm_id,
            ev.base_mint,
            reason,
            queue.len(),
            self.inactive_tx_buffer_ttl_ms
        );

        if decision_event_ts.is_some_and(|decision_ts| {
            now_ms.saturating_sub(decision_ts) > self.inactive_tx_buffer_ttl_ms
        }) {
            increment_counter!(
                "snapshotengine_buffered_inactive_stale_total",
                "reason" => "arrived_after_ttl"
            );
        }
    }

    /// Check for stagnation across all pools
    ///
    /// This should be called periodically to detect pools with no activity.
    /// Returns the number of stagnant pools detected.
    pub fn check_stagnation(&self) -> usize {
        let now = Self::now_ms();
        let pools = self.pools.read();
        let mut stagnant_count = 0;

        for (pool_pubkey, pool_state) in pools.iter() {
            let ps = pool_state.lock();
            let time_since_activity = now.saturating_sub(ps.last_activity_ts_ms);

            // Check if buffer is empty and hasn't had activity for > threshold
            if ps.snapshots.is_empty() && time_since_activity > self.stagnation_threshold_ms {
                warn!(
                    pool = %pool_pubkey,
                    time_since_activity_ms = time_since_activity,
                    threshold_ms = self.stagnation_threshold_ms,
                    "Stagnation detected: empty ring-buffer for extended period"
                );

                if let Some(ref metrics) = self.metrics {
                    metrics.record_stagnation_detected();
                }

                stagnant_count += 1;
            }
        }

        stagnant_count
    }

    /// Get or create a pool state
    ///
    /// This is the primary entry point for ensuring a pool exists in the system.
    /// Uses RwLock upgrade pattern to avoid write lock contention when pool already exists.
    pub fn get_or_create_pool_state(&self, pool_pubkey: Pubkey) -> Arc<Mutex<PoolState>> {
        // Fast path: try read lock first
        {
            let pools = self.pools.read();
            if let Some(state) = pools.get(&pool_pubkey) {
                return Arc::clone(state);
            }
        }

        // Slow path: need to create
        let mut pools = self.pools.write();
        // Double-check after acquiring write lock (another thread might have created it)
        pools
            .entry(pool_pubkey)
            .or_insert_with(|| {
                Arc::new(Mutex::new(PoolState::new(
                    pool_pubkey,
                    self.snapshot_capacity,
                )))
            })
            .clone()
    }

    /// Check whether a pool is currently tracked by the SnapshotEngine
    pub fn has_pool(&self, pool_pubkey: &Pubkey) -> bool {
        let pools = self.pools.read();
        pools.contains_key(pool_pubkey)
    }

    /// Return the number of currently active pools
    pub fn active_pool_count(&self) -> usize {
        self.active_pools.read().len()
    }

    /// Bootstrap pool state from initialization event
    ///
    /// Creates three synthetic snapshots (g0, g1, g2) to provide initial data
    /// for SCR/ULVF/POVC calculations immediately after pool creation.
    ///
    /// - g0: Initial state (zero activity)
    /// - g1: Minimal synthetic activity (1 tx, small volume)
    /// - g2: Slightly more activity (2 tx, slightly more volume)
    pub fn handle_initialize_pool_event(&self, init: &InitPoolEvent) {
        if !self.active_pools.read().contains(&init.pool_amm_id) {
            self.pending_inits
                .write()
                .insert(init.pool_amm_id, init.clone());
            debug!(
                pool = %init.pool_amm_id,
                base_mint = %init.base_mint,
                "Buffered InitPoolEvent for inactive pool"
            );
            return;
        }

        let t0 = init.timestamp_ms;
        let snapshot_slot = Self::normalize_slot_metadata(
            init.slot,
            "snapshot_engine.handle_initialize_pool_event",
            init.pool_amm_id,
            init.base_mint,
        );

        let pool_state = self.get_or_create_pool_state(init.pool_amm_id);
        let mut ps = pool_state.lock();
        ps.last_event_ts_ms = t0;
        let (price_quote, price_state, price_reason) = Self::derive_price(
            init.initial_reserve_base,
            init.initial_reserve_quote,
            init.initial_price_quote,
        );

        // g0: Initial snapshot (zero activity)
        let mut g0 = MarketSnapshot {
            timestamp_ms: t0,
            event_ts_source: EventTsSource::LegacyCompat,
            slot: snapshot_slot,
            cum_volume_sol: 0.0,
            tx_count: 0,
            unique_addrs: 0,
            cum_buy_volume_sol: 0.0,
            cum_sell_volume_sol: 0.0,
            window_tx_count: 0,
            window_volume_sol: 0.0,
            window_buy_volume_sol: 0.0,
            window_sell_volume_sol: 0.0,
            reserve_base: init.initial_reserve_base,
            reserve_quote: init.initial_reserve_quote,
            price_quote,
            price_state,
            price_reason,
            dev_buy_lamports: 0,
            data_source: 0, // SoftTruth by default for bootstrap
        };
        g0.set_data_source(DataSource::SoftTruth);

        // g1: Bootstrap snapshot (zero-volume for baseline hygiene)
        let mut g1 = MarketSnapshot {
            timestamp_ms: t0.saturating_add(1),
            event_ts_source: EventTsSource::LegacyCompat,
            slot: snapshot_slot,
            cum_volume_sol: 0.0,
            tx_count: 0,
            unique_addrs: 0,
            cum_buy_volume_sol: 0.0,
            cum_sell_volume_sol: 0.0,
            window_tx_count: 0,
            window_volume_sol: 0.0,
            window_buy_volume_sol: 0.0,
            window_sell_volume_sol: 0.0,
            reserve_base: init.initial_reserve_base,
            reserve_quote: init.initial_reserve_quote,
            price_quote,
            price_state,
            price_reason,
            dev_buy_lamports: 0,
            data_source: 0,
        };
        g1.set_data_source(DataSource::SoftTruth);

        // g2: Second bootstrap snapshot (zero-volume for baseline hygiene)
        let mut g2 = MarketSnapshot {
            timestamp_ms: t0.saturating_add(2),
            event_ts_source: EventTsSource::LegacyCompat,
            slot: snapshot_slot,
            cum_volume_sol: 0.0,
            tx_count: 0,
            unique_addrs: 0,
            cum_buy_volume_sol: 0.0,
            cum_sell_volume_sol: 0.0,
            window_tx_count: 0,
            window_volume_sol: 0.0,
            window_buy_volume_sol: 0.0,
            window_sell_volume_sol: 0.0,
            reserve_base: init.initial_reserve_base,
            reserve_quote: init.initial_reserve_quote,
            price_quote,
            price_state,
            price_reason,
            dev_buy_lamports: 0,
            data_source: 0,
        };
        g2.set_data_source(DataSource::SoftTruth);

        // Retention for bootstrap: 60s
        let drop_age_ms = 60_000;

        let wrapped0 = ps.snapshots.push(g0, t0, drop_age_ms);
        let wrapped1 = ps.snapshots.push(g1, t0.saturating_add(1), drop_age_ms);
        let wrapped2 = ps.snapshots.push(g2, t0.saturating_add(2), drop_age_ms);

        ps.last_snapshot_ts_ms = t0.saturating_add(2);
        ps.last_snapshot_slot = snapshot_slot;
        ps.last_activity_ts_ms = Self::now_ms();
        ps.accum.reset_interval();

        // Bootstrap snapshots stay local to SnapshotEngine. Canonical ShadowLedger
        // history must be committed only by Gatekeeper to preserve single-writer
        // semantics and allow later canonical replacement.

        // Update metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_pool_initialized();
            metrics.record_snapshot_push(ps.snapshots.len(), wrapped0);
            metrics.record_snapshot_push(ps.snapshots.len(), wrapped1);
            metrics.record_snapshot_push(ps.snapshots.len(), wrapped2);
            metrics.record_price_state(g0.price_state);
            metrics.record_price_state(g1.price_state);
            metrics.record_price_state(g2.price_state);
            metrics.record_bootstrap_snapshots_created(3);
        }

        info!(
            pool = %init.pool_amm_id,
            snapshot_count = ps.snapshots.len(),
            bootstrap_snapshots = 3,
            bootstrap_volume_sol = (g0.cum_volume_sol + g1.cum_volume_sol + g2.cum_volume_sol),
            excluded_from_baseline = true,
            "Created bootstrap snapshots: baseline-safe zero volume"
        );
    }

    pub fn handle_tx_event(&self, ev: &TxEvent) {
        struct IngestLatencyGuard {
            started_at: std::time::Instant,
        }
        impl Drop for IngestLatencyGuard {
            fn drop(&mut self) {
                histogram!(
                    "snapshotengine_ingest_hot_path_ms",
                    self.started_at.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
        let _ingest_latency_guard = IngestLatencyGuard {
            started_at: std::time::Instant::now(),
        };
        if let Some(arrival_ts) = ev.arrival_time_ms {
            histogram!(
                "snapshotengine_arrival_to_ingest_start_ms",
                Self::now_ms().saturating_sub(arrival_ts) as f64
            );
        }
        increment_counter!("ghost_pipeline_stage_total", "stage" => "snapshot_engine_received");
        pipeline_coverage().increment(PipelineCoverageStage::SnapshotEngineReceived, 1);

        if ev.pool_state != PoolLifecycle::Active {
            if let Some(signature) = ev.signature.as_deref().filter(|sig| !sig.is_empty()) {
                coverage_audit().record_runtime_filtered(
                    &ev.pool_amm_id.to_string(),
                    signature,
                    "non_active_pool_lifecycle",
                );
            }
            pipeline_coverage().increment(PipelineCoverageStage::SnapshotEngineFiltered, 1);
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "snapshot_engine_filtered",
                "reason" => "non_active_pool_lifecycle"
            );
            increment_counter!("snapshotengine_ignored_zombie_total");
            warn!(
                "TX_IGNORED_ZOMBIE sig={} pool={} mint={} reason=NON_ACTIVE_POOL_LIFECYCLE",
                ev.signature.as_deref().unwrap_or("unknown"),
                ev.pool_amm_id,
                ev.base_mint
            );
            return;
        }

        if !self.active_pools.read().contains(&ev.pool_amm_id) {
            if let Some(signature) = ev.signature.as_deref().filter(|sig| !sig.is_empty()) {
                coverage_audit().record_runtime_filtered(
                    &ev.pool_amm_id.to_string(),
                    signature,
                    "pool_not_active_buffered",
                );
            }
            self.buffer_inactive_tx(ev, "pool_not_active");
            return;
        }

        // --- 1. Get Pool State ---
        let pool_state = self.get_or_create_pool_state(ev.pool_amm_id);
        let mut ps = pool_state.lock();

        let (event_ts_ms, event_ts_source) = resolve_tx_event_timestamp(ev);
        if matches!(event_ts_source, EventTsSource::Wallclock)
            && ev.event_time.is_empty()
            && ev.timestamp_ms == 0
            && ev.arrival_time_ms.is_none()
        {
            increment_counter!(
                "event_time_contract_violation_total",
                "origin" => "snapshot_engine.handle_tx_event",
                "reason" => "missing_timestamp_and_arrival"
            );
            warn!(
                pool = %ev.pool_amm_id,
                base_mint = %ev.base_mint,
                slot = ?ev.slot,
                "EVENT_TIME_CONTRACT_VIOLATION: missing timestamp_ms and arrival_time_ms; using wallclock fallback"
            );
        }

        let signature_str =
            ev.signature
                .as_ref()
                .and_then(|s| if s.is_empty() { None } else { Some(s.as_str()) });

        let normalized_slot = Self::normalize_slot_metadata(
            ev.slot,
            "snapshot_engine.handle_tx_event",
            ev.pool_amm_id,
            ev.base_mint,
        );

        // event_ordinal (intra-tx index) distinguishes multiple distinct trades within
        // the same signature. It becomes tx_index in TxKey so that:
        //   (sig="A", ordinal=0)  and  (sig="A", ordinal=1)
        // produce DIFFERENT TxKeys and are never collapsed into one.
        let event_ordinal = ev.event_ordinal;

        let tx_key = if event_ts_ms == 0 {
            if let Some(signature) = signature_str {
                coverage_audit().record_runtime_filtered(
                    &ev.pool_amm_id.to_string(),
                    signature,
                    "invalid_event_timestamp_zero",
                );
            }
            warn!(
                pool = %ev.pool_amm_id,
                base_mint = %ev.base_mint,
                timestamp_ms = event_ts_ms,
                "Invalid event timestamp for TxKey; dropping tx event"
            );
            return;
        } else if let Some(sig) = signature_str {
            if let Ok(parsed) = Signature::from_str(sig) {
                TxKey::new(event_ts_ms, normalized_slot, event_ordinal, Some(parsed), 0).ok()
            } else {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                sig.hash(&mut hasher);
                if let Some(ord) = event_ordinal {
                    ord.hash(&mut hasher);
                }
                let fallback_counter = hasher.finish();
                TxKey::new(
                    event_ts_ms,
                    normalized_slot,
                    event_ordinal,
                    None,
                    fallback_counter,
                )
                .ok()
            }
        } else {
            let fallback_counter = Self::fallback_counter_from_event(ev, event_ts_ms);
            TxKey::new(
                event_ts_ms,
                normalized_slot,
                event_ordinal,
                None,
                fallback_counter,
            )
            .ok()
        };

        let tx_key = match tx_key {
            Some(key) => key,
            None => {
                if let Some(signature) = signature_str {
                    coverage_audit().record_runtime_filtered(
                        &ev.pool_amm_id.to_string(),
                        signature,
                        "invalid_tx_key",
                    );
                }
                warn!(
                    pool = %ev.pool_amm_id,
                    base_mint = %ev.base_mint,
                    timestamp_ms = event_ts_ms,
                    "Invalid event timestamp for TxKey; dropping tx event"
                );
                return;
            }
        };

        let enrichment_score = Self::tx_event_enrichment_score(ev);
        if let Some(previous_score) = ps.seen_tx_key_quality.get(&tx_key).copied() {
            if enrichment_score > previous_score {
                ps.seen_tx_key_quality
                    .insert(tx_key.clone(), enrichment_score);
                self.apply_duplicate_enrichment(
                    &mut ps,
                    ev,
                    event_ts_ms,
                    event_ts_source,
                    normalized_slot,
                );
                debug!(
                    pool = %ev.pool_amm_id,
                    base_mint = %ev.base_mint,
                    "Duplicate tx_key upgraded SnapshotEngine state with richer context"
                );
                return;
            }
            pipeline_coverage().increment(PipelineCoverageStage::SnapshotEngineFiltered, 1);
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "snapshot_engine_filtered",
                "reason" => "duplicate_tx_key"
            );
            if let Some(signature) = signature_str {
                coverage_audit().record_runtime_filtered(
                    &ev.pool_amm_id.to_string(),
                    signature,
                    "duplicate_tx_key",
                );
            }
            debug!(
                pool = %ev.pool_amm_id,
                base_mint = %ev.base_mint,
                "Duplicate tx_key ignored in SnapshotEngine"
            );
            return;
        }
        ps.seen_tx_keys.insert(tx_key.clone());
        ps.seen_tx_key_quality
            .insert(tx_key.clone(), enrichment_score);
        pipeline_coverage().increment(PipelineCoverageStage::SnapshotEngineAccepted, 1);
        increment_counter!("ghost_pipeline_stage_total", "stage" => "snapshot_engine_accepted");
        if let Some(signature) = signature_str {
            let pool_id = ev.pool_amm_id.to_string();
            coverage_audit().record_runtime_accepted(&pool_id, signature);
        }
        ps.seen_tx_keys_fifo.push_back(tx_key);
        if ps.seen_tx_keys_fifo.len() > ps.seen_tx_keys_capacity {
            if let Some(oldest) = ps.seen_tx_keys_fifo.pop_front() {
                ps.seen_tx_keys.remove(&oldest);
                ps.seen_tx_key_quality.remove(&oldest);
            }
        }

        let prev_last_event_ts_ms = ps.last_event_ts_ms;
        let event_ts_regression = event_ts_ms < prev_last_event_ts_ms;
        if event_ts_regression {
            increment_counter!(
                "event_time_regression_total",
                "origin" => "snapshot_engine.handle_tx_event"
            );
        }
        ps.last_event_ts_ms = prev_last_event_ts_ms.max(event_ts_ms);
        ps.last_event_ts_source = event_ts_source;
        ps.ingest_seq_no = ps.ingest_seq_no.saturating_add(1);
        let seq_no = ps.ingest_seq_no;
        ps.last_activity_ts_ms = Self::now_ms();

        // --- 2. LivePipeline Check ---
        // C5: Single-writer after commit
        // If this mint has been committed to LivePipeline, SnapshotEngine stops writing to ShadowLedger
        // It can still process for in-memory signals, but NO writes to ledger
        if ps.committed_to_live_pipeline {
            // Process for in-memory state only (no ledger write)
            debug!(
                pool = %ev.pool_amm_id,
                base_mint = %ev.base_mint,
                "Mint committed to LivePipeline - SnapshotEngine processing for signals only (no ledger write)"
            );
        }

        // --- 3. Update Reserves/Price State (PoolAccumulators) ---
        if let Some(rb) = ev.reserve_base {
            ps.accum.reserve_base = rb;
        }
        if let Some(rq) = ev.reserve_quote {
            ps.accum.reserve_quote = rq;
        }
        if let Some(price) = ev.price_quote {
            ps.accum.price_quote = price;
        }

        // --- 3.5. Cumulative Counters (Pool Lifetime) ---
        ps.accum.cum_tx_count = ps.accum.cum_tx_count.saturating_add(1);
        ps.accum.cum_volume_sol += ev.volume_sol;
        if ev.is_buy {
            ps.accum.cum_buy_volume_sol += ev.volume_sol;
        } else {
            ps.accum.cum_sell_volume_sol += ev.volume_sol;
        }

        // --- 4. Transaction Buffer (SOBP/MPCF) ---
        let tx_record =
            TransactionRecord::from_tx_event_with_ts(ev, event_ts_ms, event_ts_source, seq_no);
        ps.snapshots.push_transaction(tx_record);

        debug!(
            pool = %ev.pool_amm_id,
            tx_buffer_len = ps.snapshots.get_transactions().len(),
            event_ts_source = event_ts_source.as_str(),
            event_ts_regression = event_ts_regression,
            canonical_last_event_ts_ms = ps.last_event_ts_ms,
            "Transaction recorded in buffer for SOBP/MPCF"
        );

        // --- 5. Grouping Logic (Event-Time) ---
        // Initialize group if None
        if ps.current_group.is_none() {
            ps.current_group = Some(SnapshotGroup::new(event_ts_ms, event_ts_source));
        }

        // Add event to current group (Scoped mutable borrow)
        {
            let group = ps.current_group.as_mut().expect("Group initialized");
            group.add_event(ev, event_ts_ms, event_ts_source);
        }

        // --- 6. Window Check ---
        let should_emit = ps
            .current_group
            .as_ref()
            .expect("Group initialized")
            .should_create_snapshot(self.snapshot_interval_ms);

        // Emit snapshot every event; reset group only when interval window closes.
        // Use immutable borrow for accum, which is safe because we ended the mutable borrow of group
        let (price_quote, price_state, price_reason) = Self::derive_price(
            ps.accum.reserve_base,
            ps.accum.reserve_quote,
            ps.accum.price_quote,
        );

        if ps.accum.reserve_base <= MIN_RESERVE_THRESHOLD
            || ps.accum.reserve_quote <= MIN_RESERVE_THRESHOLD
        {
            warn!(
                pool = %ev.pool_amm_id,
                base_mint = %ev.base_mint,
                reserve_base = ps.accum.reserve_base,
                reserve_quote = ps.accum.reserve_quote,
                "Snapshot emitted with missing/zero reserves (price state will be Unknown/Invalid)"
            );
        }

        // Create Snapshot from Group Data
        let (
            accum_volume_sol,
            event_count,
            unique_signers_len,
            accum_buy_volume_sol,
            accum_sell_volume_sol,
            snapshot_ts_ms,
            snapshot_ts_source,
        ) = {
            let group = ps.current_group.as_ref().expect("Group initialized");
            (
                group.accum_volume_sol,
                group.event_count,
                group.unique_signers.len(),
                group.accum_buy_volume_sol,
                group.accum_sell_volume_sol,
                group.last_event_ts_ms,
                group.last_event_ts_source,
            )
        };

        let snapshot_slot = normalized_slot;
        let mut snap = MarketSnapshot {
            timestamp_ms: snapshot_ts_ms,
            event_ts_source: snapshot_ts_source,
            slot: snapshot_slot, // Metadata ONLY
            cum_volume_sol: ps.accum.cum_volume_sol,
            tx_count: ps.accum.cum_tx_count,
            unique_addrs: unique_signers_len,
            cum_buy_volume_sol: ps.accum.cum_buy_volume_sol,
            cum_sell_volume_sol: ps.accum.cum_sell_volume_sol,
            window_tx_count: event_count as u32,
            window_volume_sol: accum_volume_sol as f32,
            window_buy_volume_sol: accum_buy_volume_sol as f32,
            window_sell_volume_sol: accum_sell_volume_sol as f32,
            reserve_base: ps.accum.reserve_base,
            reserve_quote: ps.accum.reserve_quote,
            price_quote,
            price_state,
            price_reason,
            dev_buy_lamports: ev.metrics.dev_buy_lamports,
            data_source: 0,
        };
        snap.set_data_source(ev.data_source);

        // --- 7. Push with Retention ---
        // RULE: Drop if now_ts_ms - snapshot_ts_ms > drop_age_ms
        let drop_age_ms = 60_000;

        let wrapped = ps.snapshots.push(snap, event_ts_ms, drop_age_ms);

        ps.last_snapshot_ts_ms = snapshot_ts_ms;
        ps.last_snapshot_slot = snapshot_slot;
        ps.last_activity_ts_ms = Self::now_ms();

        // --- 8. ShadowLedger Sync ---
        // Pre-commit boundary contract (PR-3b):
        // SnapshotEngine maintains LOCAL soft-truth state only (ring buffer above).
        // ShadowLedger is written exclusively by:
        //   - Gatekeeper commit loop (canonical history)
        //   - LivePipeline.flush_mint() (post-commit live events)
        // SnapshotEngine MUST NOT attempt pre-commit writes to ShadowLedger.
        //
        // Detect committed state lazily to flip the committed_to_live_pipeline flag
        // so downstream callers can check ps.committed_to_live_pipeline if needed.
        if let Some(ref ledger) = self.shadow_ledger {
            if !ps.committed_to_live_pipeline && ledger.is_committed(&ev.base_mint) {
                ps.committed_to_live_pipeline = true;
                info!(
                    pool = %ev.pool_amm_id,
                    base_mint = %ev.base_mint,
                    "Detected committed ShadowLedger history; SnapshotEngine operating in read-only mode for this pool"
                );
            }
        }
        // No ShadowLedger write here — pre-commit state lives exclusively in the ring buffer.

        // --- 9. Metrics & Logging ---
        ps.accum.reset_interval(); // Reserves accumulators don't really reset, but ok.

        if let Some(ref metrics) = self.metrics {
            metrics.record_snapshot_push(ps.snapshots.len(), wrapped);
            metrics.record_price_state(snap.price_state);
        }

        debug!(
            pool = %ev.pool_amm_id,
            snapshot_ts_ms = snap.timestamp_ms,
            tx_count = snap.tx_count,
            volume_sol = snap.cum_volume_sol,
            snapshot_count = ps.snapshots.len(),
            wrapped = wrapped,
            signature = ev.signature.as_deref().unwrap_or("N/A"),
            base_mint = %ev.base_mint,
            signer = %ev.signer,
            is_buy = ev.is_buy,
            tx_volume_sol = ev.volume_sol,
            reserve_base = ev.reserve_base.unwrap_or(0.0),
            reserve_quote = ev.reserve_quote.unwrap_or(0.0),
            price_quote = ev.price_quote.unwrap_or(0.0),
            slot = ?snapshot_slot,
            is_dev_buy = ev.is_dev_buy,
            "Snapshot emitted"
        );

        // --- 10. Reset Group ---
        if should_emit {
            // RULE: Discard completely.
            ps.current_group = None;
        }
    }

    /// Get the latest snapshot for a pool
    pub fn get_latest_snapshot(&self, pool_pubkey: &Pubkey) -> Option<MarketSnapshot> {
        let pools = self.pools.read();
        let ps = pools.get(pool_pubkey)?;
        let ps = ps.lock();
        ps.snapshots.latest().copied()
    }

    /// Get the latest two snapshots as a pair (for ULVF calculations)
    ///
    /// Returns (t0, t1) where t1 is more recent than t0.
    pub fn latest_pair(&self, pool_pubkey: &Pubkey) -> Option<(MarketSnapshot, MarketSnapshot)> {
        let pools = self.pools.read();
        let ps = pools.get(pool_pubkey)?;
        let ps = ps.lock();
        let snaps = ps.snapshots.get_last_n(2);
        if snaps.len() < 2 {
            return None;
        }
        // get_last_n returns most recent first, so [0] is t1, [1] is t0
        Some((snaps[1], snaps[0]))
    }

    /// Get the last N snapshots for a pool (most recent first)
    pub fn last_n(&self, pool_pubkey: &Pubkey, n: usize) -> Vec<MarketSnapshot> {
        let pools = self.pools.read();
        let ps = match pools.get(pool_pubkey) {
            Some(p) => p,
            None => return Vec::new(),
        };
        let ps = ps.lock();
        ps.snapshots.get_last_n(n)
    }

    /// Get live cumulative counters for a pool (independent of snapshot emission)
    pub fn get_live_counters(&self, pool_pubkey: Pubkey) -> Option<LiveCounters> {
        let pools = self.pools.read();
        let ps = pools.get(&pool_pubkey)?;
        let ps = ps.lock();
        let canonical_event_now_ts_ms = ps.canonical_event_now_ts_ms();
        Some(LiveCounters {
            // Contract: never derive this from activity/wall-clock timestamps.
            now_ts_ms: canonical_event_now_ts_ms,
            cum_tx_count: ps.accum.cum_tx_count,
            cum_volume_sol: ps.accum.cum_volume_sol,
            cum_buy_volume_sol: ps.accum.cum_buy_volume_sol,
            cum_sell_volume_sol: ps.accum.cum_sell_volume_sol,
            tx_buffer_len: ps.snapshots.get_transactions().len(),
            snapshot_count: ps.snapshots.len(),
            event_ts_source: ps.last_event_ts_source,
        })
    }

    /// Get recent transactions for a pool for SOBP/MPCF processing
    ///
    /// Returns all transactions in the buffer (up to 128 transactions or 10 seconds)
    pub fn get_transactions(&self, pool_pubkey: &Pubkey) -> Vec<TransactionRecord> {
        let pools = self.pools.read();
        let ps = match pools.get(pool_pubkey) {
            Some(p) => p,
            None => return Vec::new(),
        };
        let ps = ps.lock();
        ps.snapshots.get_transactions().to_vec()
    }

    /// Get transactions since a specific timestamp
    ///
    /// Useful for getting transactions within a specific cycle window
    pub fn get_transactions_since(
        &self,
        pool_pubkey: &Pubkey,
        cutoff_ms: u64,
    ) -> Vec<TransactionRecord> {
        let pools = self.pools.read();
        let ps = match pools.get(pool_pubkey) {
            Some(p) => p,
            None => return Vec::new(),
        };
        let ps = ps.lock();
        ps.snapshots.get_transactions_since(cutoff_ms)
    }

    /// Get metrics collector (if enabled)
    pub fn metrics(&self) -> Option<Arc<SnapshotMetrics>> {
        self.metrics.as_ref().map(Arc::clone)
    }

    /// Cross-check volume: Compare Shadow Ledger state with real transaction
    ///
    /// If Shadow Ledger indicates a pool is empty/inactive but we receive a
    /// significant transaction (> threshold), this indicates state desynchronization.
    ///
    /// # Arguments
    /// * `pool_pubkey` - Pool to check
    /// * `shadow_ledger_reserves` - Current reserves from Shadow Ledger (base, quote)
    /// * `transaction_volume_sol` - Volume of current transaction in SOL
    /// * `volume_threshold_sol` - Threshold for "significant" transaction (default: 1.0 SOL)
    pub fn check_volume_cross_check(
        &self,
        pool_pubkey: &Pubkey,
        shadow_ledger_reserves: (f64, f64),
        transaction_volume_sol: f64,
        volume_threshold_sol: f64,
    ) {
        let (shadow_base, shadow_quote) = shadow_ledger_reserves;

        // Check if Shadow Ledger thinks pool is empty/near-empty
        let shadow_is_empty = shadow_base < 0.001 && shadow_quote < 0.001;

        // Check if we're seeing significant real transaction
        let has_significant_tx = transaction_volume_sol > volume_threshold_sol;

        if shadow_is_empty && has_significant_tx {
            warn!(
                pool = %pool_pubkey,
                shadow_base = shadow_base,
                shadow_quote = shadow_quote,
                tx_volume_sol = transaction_volume_sol,
                "STATE DESYNCHRONIZATION: Shadow Ledger empty but significant transaction detected"
            );

            // Send HardAbort violation - trading on stale state is dangerous
            if let Some(ref callback) = self.integrity_callback {
                callback(IntegrityViolation {
                    source: "SnapshotEngine".to_string(),
                    severity: IntegritySeverity::HardAbort,
                    details: format!(
                        "Shadow Ledger reports empty pool but transaction {} SOL detected (threshold: {} SOL)",
                        transaction_volume_sol, volume_threshold_sol
                    ),
                    pool_pubkey: *pool_pubkey,
                    timestamp_ms: Self::now_ms(),
                });
            }
        }
    }

    /// Validate data consistency between soft truth and hard truth sources
    ///
    /// This method should be called when receiving hard truth data to verify
    /// that soft truth predictions match reality.
    ///
    /// # Arguments
    /// * `pool_pubkey` - Pool to validate
    /// * `hard_truth_snapshot` - Authoritative snapshot from blockstore/RPC
    /// * `tolerance` - Acceptable deviation ratio (e.g., 0.1 = 10%)
    ///
    /// # Returns
    /// `true` if data is consistent, `false` if anomaly detected
    pub fn validate_soft_vs_hard_truth(
        &self,
        pool_pubkey: &Pubkey,
        hard_truth_snapshot: &MarketSnapshot,
        tolerance: f64,
    ) -> bool {
        let pools = self.pools.read();
        let ps = match pools.get(pool_pubkey) {
            Some(p) => p,
            None => return true, // No soft truth to compare
        };
        let ps = ps.lock();

        // Find most recent soft truth snapshot closest in event-time (slot is metadata-only)
        let hard_ts_ms = hard_truth_snapshot.timestamp_ms;
        if hard_ts_ms == 0 {
            return true;
        }
        let soft_snapshots = ps.snapshots.get_last_n(10);
        let matching_soft = soft_snapshots
            .iter()
            .filter(|s| {
                s.get_data_source() == DataSource::SoftTruth
                    // Hard-truth alignment must never be driven by ingress/arrival/wallclock
                    // timestamps, otherwise chain truth gets compared against ingest time.
                    && matches!(s.event_ts_source, EventTsSource::Event)
                    && s.timestamp_ms > 0
            })
            .min_by_key(|s| {
                if hard_ts_ms >= s.timestamp_ms {
                    hard_ts_ms - s.timestamp_ms
                } else {
                    s.timestamp_ms - hard_ts_ms
                }
            });

        if let Some(soft) = matching_soft {
            // Compare volumes
            let volume_diff_ratio = if soft.cum_volume_sol > 0.0 {
                (hard_truth_snapshot.cum_volume_sol - soft.cum_volume_sol).abs()
                    / soft.cum_volume_sol
            } else {
                0.0
            };

            if volume_diff_ratio > tolerance {
                warn!(
                    pool = %pool_pubkey,
                    soft_volume = soft.cum_volume_sol,
                    hard_volume = hard_truth_snapshot.cum_volume_sol,
                    diff_ratio = volume_diff_ratio,
                    slot = hard_truth_snapshot.slot,
                    "SOFT vs HARD TRUTH mismatch detected"
                );

                if let Some(ref callback) = self.integrity_callback {
                    callback(IntegrityViolation {
                        source: "SnapshotEngine".to_string(),
                        severity: IntegritySeverity::HardAbort,
                        details: format!(
                            "Soft/Hard truth mismatch: soft volume {:.3} SOL vs hard {:.3} SOL (diff: {:.1}%)",
                            soft.cum_volume_sol, hard_truth_snapshot.cum_volume_sol, volume_diff_ratio * 100.0
                        ),
                        pool_pubkey: *pool_pubkey,
                        timestamp_ms: Self::now_ms(),
                    });
                }

                return false;
            }
        }

        true
    }

    /// Get statistics about data sources for a pool
    ///
    /// Returns (soft_truth_count, hard_truth_count) from recent snapshots
    pub fn get_data_source_stats(&self, pool_pubkey: &Pubkey, n: usize) -> (usize, usize) {
        let recent = self.last_n(pool_pubkey, n);
        let soft_count = recent
            .iter()
            .filter(|s| s.get_data_source() == DataSource::SoftTruth)
            .count();
        let hard_count = recent
            .iter()
            .filter(|s| s.get_data_source() == DataSource::HardTruth)
            .count();
        (soft_count, hard_count)
    }

    /// Mark a pool as committed to LivePipeline (BLOCKER FIX)
    ///
    /// After commit, SnapshotEngine will stop writing to ShadowLedger for this pool.
    /// This enforces the single-writer contract: only LivePipeline writes post-commit.
    ///
    /// # Arguments
    /// * `pool_amm_id` - The specific pool AMM ID to mark as committed
    pub fn mark_pool_committed(&self, pool_amm_id: Pubkey) {
        let pools = self.pools.read();

        // Mark ONLY the specific pool that was committed
        if let Some(pool_state) = pools.get(&pool_amm_id) {
            let mut ps = pool_state.lock();
            ps.committed_to_live_pipeline = true;
            info!(
                pool = %pool_amm_id,
                "Marked pool as committed - SnapshotEngine will skip ShadowLedger writes for this pool"
            );
        } else {
            warn!(
                pool = %pool_amm_id,
                "Attempted to mark non-existent pool as committed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn mk_metrics(tx_count: u64, unique_addrs: u64, volume_sol: f64) -> PoolMetrics {
        PoolMetrics {
            tx_count,
            unique_addrs,
            volume_sol,
            dev_buy_lamports: 0,
            ..Default::default()
        }
    }

    fn test_pubkey(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    #[test]
    fn test_tx_event_with_raw_data() {
        let raw_bytes = vec![1, 2, 3, 4, 5];

        let event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: test_pubkey(1),
            base_mint: test_pubkey(2),
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 1.5),
            slot: Some(12345),
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(3),
            is_buy: true,
            volume_sol: 1.5,
            reserve_base: Some(1000.0),
            reserve_quote: Some(100.0),
            price_quote: Some(0.1),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("sig123".to_string()),
            event_ordinal: None,
            block_time: Some(1700000000),
            arrival_time_ms: Some(1700000000050),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: Some(raw_bytes.clone()),
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        assert_eq!(event.raw_data, Some(raw_bytes));
    }

    #[test]
    fn test_transaction_record_extracts_raw_bytes() {
        let raw_bytes = vec![0xDE, 0xAD, 0xBE, 0xEF];

        let event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: test_pubkey(10),
            base_mint: test_pubkey(11),
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 2.0),
            slot: Some(100),
            timestamp_ms: 999999,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(12),
            is_buy: true,
            volume_sol: 2.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("test".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: Some(raw_bytes.clone()),
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        let record = TransactionRecord::from_tx_event(&event);

        assert_eq!(record.raw_bytes, Some(raw_bytes));
        assert_eq!(record.slot, Some(100));
        assert_eq!(record.signature, "test");
        assert_eq!(record.signer, test_pubkey(12));
        assert_eq!(record.sol_amount, 2.0);
        assert_eq!(record.is_buy, true);
        assert_eq!(record.timestamp_ms, 999999);
        assert_eq!(record.event_ts_source, EventTsSource::LegacyCompat);
    }

    #[test]
    fn test_transaction_record_without_raw_bytes() {
        let event = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: test_pubkey(20),
            base_mint: test_pubkey(21),
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 1.0),
            slot: Some(200),
            timestamp_ms: 888888,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(22),
            is_buy: false,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        let record = TransactionRecord::from_tx_event(&event);

        assert_eq!(record.raw_bytes, None);
        assert_eq!(record.slot, Some(200));
        assert_eq!(record.signature, "");
        assert_eq!(record.is_buy, false);
        assert_eq!(record.event_ts_source, EventTsSource::LegacyCompat);
    }

    #[test]
    fn test_event_ts_source_decision_event_ts_ms_rejects_storage_only_sources() {
        assert_eq!(
            EventTsSource::Event.decision_event_ts_ms(1_000),
            Some(1_000)
        );
        assert_eq!(
            EventTsSource::IngressWall.decision_event_ts_ms(2_000),
            Some(2_000)
        );
        assert_eq!(
            EventTsSource::LegacyCompat.decision_event_ts_ms(3_000),
            None
        );
        assert_eq!(EventTsSource::Arrival.decision_event_ts_ms(4_000), None);
        assert_eq!(EventTsSource::Wallclock.decision_event_ts_ms(5_000), None);
    }

    #[test]
    fn test_ring_snapshots_creation() {
        let ring = RingSnapshots::new(128);
        assert_eq!(ring.capacity, 128);
        assert_eq!(ring.size, 0);
        assert!(ring.is_empty());
    }

    fn make_transaction_record_for_buffer(
        ts: u64,
        event_ts_source: EventTsSource,
        seq_no: u64,
    ) -> TransactionRecord {
        TransactionRecord {
            slot: Some(seq_no + 1),
            signature: format!("sig-{seq_no}"),
            signer: test_pubkey((seq_no as u8).saturating_add(30)),
            sol_amount: 1.0,
            is_buy: true,
            is_dev_buy: false,
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            event_ts_source,
            seq_no,
            price_quote: None,
            raw_bytes: None,
            raw_bytes_missing_reason: RawBytesMissingReason::Unknown,
        }
    }

    #[test]
    fn test_ring_snapshots_push_and_latest() {
        let mut ring = RingSnapshots::new(4);

        let snap1 = MarketSnapshot {
            timestamp_ms: 1000,
            slot: Some(100),
            cum_volume_sol: 10.0,
            ..Default::default()
        };

        ring.push(snap1.clone(), snap1.timestamp_ms, 60_000);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.latest().unwrap().timestamp_ms, 1000);

        let snap2 = MarketSnapshot {
            timestamp_ms: 2000,
            slot: Some(200),
            cum_volume_sol: 20.0,
            ..Default::default()
        };

        ring.push(snap2.clone(), snap2.timestamp_ms, 60_000);
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.latest().unwrap().timestamp_ms, 2000);
    }

    #[test]
    fn test_ring_snapshots_wrapping() {
        let mut ring = RingSnapshots::new(3);

        for i in 0..5 {
            let snap = MarketSnapshot {
                timestamp_ms: (i + 1) * 1000,
                slot: Some((i + 1) * 100),
                ..Default::default()
            };
            ring.push(snap.clone(), snap.timestamp_ms, 60_000);
        }

        // Ring has capacity 3, so we should only have the last 3
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.latest().unwrap().timestamp_ms, 5000);

        let last_3 = ring.get_last_n(3);
        assert_eq!(last_3.len(), 3);
        assert_eq!(last_3[0].timestamp_ms, 5000); // Most recent
        assert_eq!(last_3[1].timestamp_ms, 4000);
        assert_eq!(last_3[2].timestamp_ms, 3000);
    }

    #[test]
    fn test_ring_snapshots_get_last_n() {
        let mut ring = RingSnapshots::new(10);

        for i in 0..5 {
            let snap = MarketSnapshot {
                timestamp_ms: (i + 1) * 1000,
                ..Default::default()
            };
            ring.push(snap.clone(), snap.timestamp_ms, 60_000);
        }

        let last_3 = ring.get_last_n(3);
        assert_eq!(last_3.len(), 3);
        assert_eq!(last_3[0].timestamp_ms, 5000);
        assert_eq!(last_3[1].timestamp_ms, 4000);
        assert_eq!(last_3[2].timestamp_ms, 3000);

        // Request more than available
        let last_10 = ring.get_last_n(10);
        assert_eq!(last_10.len(), 5);
    }

    #[test]
    fn test_transaction_buffer_age_trim_ignores_storage_only_clocks() {
        let mut ring = RingSnapshots::new(8);

        ring.push_transaction(make_transaction_record_for_buffer(
            5_000,
            EventTsSource::IngressWall,
            1,
        ));
        ring.push_transaction(make_transaction_record_for_buffer(
            50_000,
            EventTsSource::LegacyCompat,
            2,
        ));

        let kept = ring.get_transactions();
        assert_eq!(kept.len(), 2);
        assert!(
            kept.iter()
                .any(|tx| tx.decision_event_ts_ms() == Some(5_000)),
            "later storage-only timestamp must not age-trim an earlier decision tx"
        );
        assert!(
            kept.iter()
                .any(|tx| tx.event_ts_source == EventTsSource::LegacyCompat),
            "storage-only tx should remain observable while not defining decision retention"
        );
    }

    #[test]
    fn test_transaction_buffer_capacity_prefers_dropping_storage_only_sources() {
        let mut ring = RingSnapshots::new(8);
        ring.max_tx_buffer = 2;

        ring.push_transaction(make_transaction_record_for_buffer(
            1_000,
            EventTsSource::Event,
            1,
        ));
        ring.push_transaction(make_transaction_record_for_buffer(
            20_000,
            EventTsSource::LegacyCompat,
            2,
        ));
        ring.push_transaction(make_transaction_record_for_buffer(
            30_000,
            EventTsSource::LegacyCompat,
            3,
        ));

        let kept = ring.get_transactions();
        assert_eq!(kept.len(), 2);
        assert!(
            kept.iter()
                .any(|tx| tx.decision_event_ts_ms() == Some(1_000)),
            "storage-only txs must not evict decision-eligible txs under capacity pressure"
        );
        assert_eq!(
            kept.iter()
                .filter(|tx| tx.decision_event_ts_ms().is_none())
                .count(),
            1,
            "only one storage-only tx should survive once the buffer reaches capacity"
        );
    }

    #[test]
    fn test_pool_accumulators_reset() {
        let mut accum = PoolAccumulators::default();
        accum.reserve_base = 100.0;
        accum.reserve_quote = 50.0;
        accum.price_quote = 0.5;

        accum.reset_interval();

        assert_eq!(accum.reserve_base, 100.0);
        assert_eq!(accum.reserve_quote, 50.0);
        assert_eq!(accum.price_quote, 0.5);
    }

    #[test]
    fn test_snapshot_engine_creation() {
        let engine = SnapshotEngine::new(128, 200);
        assert_eq!(engine.snapshot_capacity, 128);
        assert_eq!(engine.snapshot_interval_ms, 200);
    }

    #[test]
    fn test_snapshot_engine_get_or_create() {
        let engine = SnapshotEngine::new(128, 200);
        let pool_key = test_pubkey(1);

        let state1 = engine.get_or_create_pool_state(pool_key);
        let state2 = engine.get_or_create_pool_state(pool_key);

        // Should return the same Arc
        assert!(Arc::ptr_eq(&state1, &state2));
    }

    #[test]
    fn test_bootstrap_from_init_pool_event() {
        let engine = SnapshotEngine::new(128, 200);

        let init_event = InitPoolEvent {
            pool_amm_id: test_pubkey(1),
            base_mint: test_pubkey(2),
            quote_mint: test_pubkey(3),
            slot: Some(1000),
            timestamp_ms: 5000,
            initial_liquidity_sol: 10.0,
            initial_reserve_base: 1000000.0,
            initial_reserve_quote: 10.0,
            initial_price_quote: 0.00001,
        };

        engine.mark_pool_active(init_event.pool_amm_id);

        engine.handle_initialize_pool_event(&init_event);

        // Check that 3 snapshots were created (g0, g1, g2)
        let snapshots = engine.last_n(&test_pubkey(1), 10);
        assert_eq!(snapshots.len(), 3);

        // Verify g0
        assert_eq!(snapshots[2].timestamp_ms, 5000);
        assert_eq!(snapshots[2].cum_volume_sol, 0.0);
        assert_eq!(snapshots[2].tx_count, 0);

        // Verify g1
        assert_eq!(snapshots[1].timestamp_ms, 5001);
        assert_eq!(snapshots[1].cum_volume_sol, 0.0);
        assert_eq!(snapshots[1].tx_count, 0);

        // Verify g2
        assert_eq!(snapshots[0].timestamp_ms, 5002);
        assert_eq!(snapshots[0].cum_volume_sol, 0.0);
        assert_eq!(snapshots[0].tx_count, 0);
    }

    #[test]
    fn test_handle_tx_event_accumulation() {
        let engine = SnapshotEngine::new(128, 1000); // 1 second interval

        let pool_key = test_pubkey(1);
        engine.mark_pool_active(pool_key);

        let tx1 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 5.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: Some(1000.0),
            reserve_quote: Some(100.0),
            price_quote: Some(0.1),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx1);

        // Snapshot should be emitted on first ACTIVE event
        let snapshots = engine.last_n(&pool_key, 10);
        assert_eq!(snapshots.len(), 1);

        let tx2 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 2,
                unique_addrs: 2,
                volume_sol: 8.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 3.0,
                dev_buy_lamports: 0,
            },
            slot: Some(101),
            timestamp_ms: 1500, // 500ms later - still within interval
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(11),
            is_buy: false,
            volume_sol: 3.0,
            reserve_base: Some(1003.0),
            reserve_quote: Some(97.0),
            price_quote: Some(0.097),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx2);

        // Snapshot emitted for every event
        let snapshots = engine.last_n(&pool_key, 10);
        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn test_handle_tx_event_writes_shadow_ledger_when_active() {
        let mut engine = SnapshotEngine::new(16, 0);
        let ledger = Arc::new(ShadowLedger::new());
        engine.set_shadow_ledger(Arc::clone(&ledger));
        let pool_key = test_pubkey(10);
        let base_mint = test_pubkey(11);
        engine.mark_pool_active(pool_key);

        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(1),
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(12),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(1.0),
            reserve_quote: Some(1.0),
            price_quote: Some(1.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: Some(1),
            arrival_time_ms: Some(1),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        // SnapshotEngine should process events when pool is ACTIVE
        engine.handle_tx_event(&tx);
        let commit_snap = MarketSnapshot {
            slot: Some(1),
            timestamp_ms: 1,
            price_state: PriceState::Unknown,
            price_reason: Some(PriceReason::MissingReserves),
            ..Default::default()
        };
        ledger.commit_history(base_mint, vec![commit_snap.to_ghost_core_snapshot()], None);
        let tx2 = TxEvent {
            timestamp_ms: 2,
            slot: Some(2),
            ..tx.clone()
        };
        let tx3 = TxEvent {
            timestamp_ms: 3,
            slot: Some(3),
            ..tx.clone()
        };
        engine.handle_tx_event(&tx2); // sets baseline
        engine.handle_tx_event(&tx3); // emits snapshot with interval 0

        assert_eq!(ledger.snapshot_count(), 1);
        assert!(ledger.get_snapshots(&base_mint).is_some());
        assert!(ledger.get_snapshots(&pool_key).is_none());
    }

    #[test]
    fn test_initialize_pool_does_not_commit_bootstrap_history_to_shadow_ledger() {
        let mut engine = SnapshotEngine::new(16, 0);
        let ledger = Arc::new(ShadowLedger::new());
        engine.set_shadow_ledger(Arc::clone(&ledger));

        let init_event = make_init_event(252, 100);
        engine.mark_pool_active(init_event.pool_amm_id);
        engine.handle_initialize_pool_event(&init_event);

        assert!(
            ledger.get_snapshots(&init_event.base_mint).is_none(),
            "bootstrap snapshots must remain local until Gatekeeper commits canonical history"
        );
        assert!(
            !ledger.is_committed(&init_event.base_mint),
            "bootstrap must not mark mint as committed"
        );
    }

    #[test]
    fn test_committed_shadow_ledger_disables_snapshotengine_writes() {
        let mut engine = SnapshotEngine::new(16, 0);
        let ledger = Arc::new(ShadowLedger::new());
        engine.set_shadow_ledger(Arc::clone(&ledger));

        let init_event = make_init_event(240, 100);
        let base_mint = init_event.base_mint;
        let pool = init_event.pool_amm_id;
        engine.mark_pool_active(pool);
        engine.handle_initialize_pool_event(&init_event);

        let committed_snapshot = MarketSnapshot {
            slot: Some(111),
            timestamp_ms: 1_111,
            tx_count: 1,
            unique_addrs: 1,
            cum_volume_sol: 1.0,
            ..Default::default()
        };
        ledger.commit_history(
            base_mint,
            vec![committed_snapshot.to_ghost_core_snapshot()],
            None,
        );

        let tx = make_tx_event(240, 2_000, Some(112));
        engine.handle_tx_event(&tx);

        let snapshots = ledger
            .get_snapshots(&base_mint)
            .expect("committed snapshots should remain available");
        assert_eq!(
            snapshots.len(),
            1,
            "SnapshotEngine must not append after ShadowLedger is committed"
        );
    }

    #[test]
    fn test_handle_tx_event_snapshot_emission() {
        let engine = SnapshotEngine::new(128, 100); // 100ms interval

        let pool_key = test_pubkey(1);
        engine.mark_pool_active(pool_key);

        let tx1 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 5.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: Some(1000.0),
            reserve_quote: Some(100.0),
            price_quote: Some(0.1),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx1);

        let tx2 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 2,
                unique_addrs: 2,
                volume_sol: 8.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 3.0,
                dev_buy_lamports: 0,
            },
            slot: Some(101),
            timestamp_ms: 1150, // 150ms later - exceeds interval
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(11),
            is_buy: false,
            volume_sol: 3.0,
            reserve_base: Some(1003.0),
            reserve_quote: Some(97.0),
            price_quote: Some(0.097),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx2);

        // Snapshot is emitted for each event
        let snapshots = engine.last_n(&pool_key, 10);
        assert_eq!(snapshots.len(), 2);

        let latest = &snapshots[0];
        assert_eq!(latest.timestamp_ms, 1150);
        assert_eq!(latest.tx_count, 2);
        assert_eq!(latest.cum_volume_sol, 8.0); // 5.0 + 3.0
        assert_eq!(latest.cum_buy_volume_sol, 5.0);
        assert_eq!(latest.cum_sell_volume_sol, 3.0);
        assert_eq!(latest.unique_addrs, 2);
    }

    #[test]
    fn test_snapshot_price_prefers_reserves_over_event_price() {
        let engine = SnapshotEngine::new(16, 0);
        let pool_key = test_pubkey(30);
        engine.mark_pool_active(pool_key);

        let tx1 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(10),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(31),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(1_000.0),
            reserve_quote: Some(120.0),
            price_quote: Some(42.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx1);

        let tx2 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(11),
            timestamp_ms: 1_010,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(32),
            is_buy: false,
            volume_sol: 0.5,
            reserve_base: Some(1_002.0),
            reserve_quote: Some(119.0),
            price_quote: Some(0.01),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx2);

        let snapshots = engine.last_n(&pool_key, 1);
        assert_eq!(snapshots.len(), 1);
        let snap = snapshots[0];
        let expected_price = 119.0 / 1_002.0;
        assert!((snap.price_quote - expected_price).abs() < 1e-12);
        assert_eq!(snap.price_state, PriceState::Valid);
    }

    #[test]
    fn test_snapshot_marks_invalid_price_when_reserves_missing() {
        let metrics = Arc::new(SnapshotMetrics::new(None));
        let engine = SnapshotEngine::with_metrics(8, 0, Some(metrics.clone()), None);
        let pool_key = test_pubkey(31);
        engine.mark_pool_active(pool_key);

        let tx1 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(5),
            timestamp_ms: 500,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(33),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(0.0),
            reserve_quote: Some(0.0),
            price_quote: Some(0.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx1);

        let tx2 = TxEvent {
            timestamp_ms: 510,
            slot: Some(6),
            ..tx1
        };
        engine.handle_tx_event(&tx2);

        let snapshots = engine.last_n(&pool_key, 2);
        assert_eq!(snapshots.len(), 2);
        let snap = snapshots[0];
        assert_eq!(snap.price_state, PriceState::Unknown);
        assert_eq!(snap.price_reason, Some(PriceReason::MissingReserves));
        assert_eq!(metrics.price_unknown_total.get(), 2);
    }

    #[test]
    fn test_to_ghost_core_snapshot_maps_fields_without_heuristics() {
        let snap = MarketSnapshot {
            timestamp_ms: 1234,
            slot: Some(42),
            cum_volume_sol: 10.5,
            tx_count: 3,
            unique_addrs: 7,
            reserve_base: 2.0,
            reserve_quote: 4.0,
            price_quote: 2.0,
            price_state: PriceState::Unknown,
            price_reason: Some(PriceReason::MissingReserves),
            ..Default::default()
        };

        let gc = snap.to_ghost_core_snapshot();

        assert_eq!(gc.timestamp_ms, 1234);
        assert_eq!(gc.slot, Some(42));
        assert_eq!(gc.cum_volume_sol, 10.5);
        assert_eq!(gc.tx_count, 3);
        assert_eq!(gc.unique_addrs, 7);
        assert_eq!(gc.price_sol_per_token, 2.0);
        assert_eq!(gc.price_state, PriceState::Unknown);
        assert_eq!(gc.price_reason, Some(PriceReason::MissingReserves));
        assert_eq!(gc.reserve_base, 2.0);
        assert_eq!(gc.reserve_quote, 4.0);
        assert_eq!(gc.market_cap_sol, 0.0);
        assert_eq!(gc.bonding_progress_pct, 0.0);
    }

    #[test]
    fn test_to_ghost_core_snapshot_preserves_monotonic_fields() {
        let s1 = MarketSnapshot {
            timestamp_ms: 1_000,
            slot: Some(10),
            cum_volume_sol: 1.5,
            tx_count: 2,
            unique_addrs: 3,
            reserve_base: 10.0,
            reserve_quote: 5.0,
            price_quote: 0.5,
            price_state: PriceState::Valid,
            price_reason: None,
            ..Default::default()
        };

        let s2 = MarketSnapshot {
            timestamp_ms: 1_500,
            slot: Some(12),
            cum_volume_sol: 3.0,
            tx_count: 5,
            unique_addrs: 6,
            reserve_base: 11.0,
            reserve_quote: 5.5,
            price_quote: 0.5,
            price_state: PriceState::Valid,
            price_reason: None,
            ..Default::default()
        };

        let g1 = s1.to_ghost_core_snapshot();
        let g2 = s2.to_ghost_core_snapshot();

        assert!(g2.timestamp_ms > g1.timestamp_ms);
        assert!(g2.timestamp_ms > g1.timestamp_ms);
        assert!(g2.cum_volume_sol > g1.cum_volume_sol);
        assert!(g2.tx_count > g1.tx_count);
        assert!(g2.unique_addrs > g1.unique_addrs);
        assert_eq!(g1.price_state, PriceState::Valid);
        assert_eq!(g2.price_state, PriceState::Valid);
    }

    #[test]
    fn test_fallback_price_valid_when_reserves_missing() {
        let (price, state, reason) = derive_price_canonical(0.0, 0.0, 0.42);
        assert_eq!(price, 0.42);
        assert_eq!(state, PriceState::Valid);
        assert_eq!(reason, Some(PriceReason::FallbackUsed));
    }

    #[test]
    fn test_snapshot_emits_even_on_duplicate_events() {
        let engine = SnapshotEngine::new(16, 0);
        let pool_key = test_pubkey(9);
        engine.mark_pool_active(pool_key);

        let base_tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(42),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(10.0),
            reserve_quote: Some(5.0),
            price_quote: Some(0.5),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("sig_base".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        // First event emits a snapshot
        engine.handle_tx_event(&base_tx);
        assert_eq!(engine.last_n(&pool_key, 4).len(), 1);

        // Identical event should emit once, then be deduped on repeat
        let emit_tx = TxEvent {
            signature: Some("sig_emit".to_string()),
            ..base_tx
        };
        engine.handle_tx_event(&emit_tx);
        let snaps = engine.last_n(&pool_key, 4);
        assert_eq!(snaps.len(), 2);

        engine.handle_tx_event(&emit_tx);
        let snaps_after = engine.last_n(&pool_key, 4);
        assert_eq!(snaps_after.len(), 2);
    }

    #[test]
    fn test_latest_pair() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(1);
        engine.mark_pool_active(pool_key);

        // Not enough snapshots
        assert!(engine.latest_pair(&pool_key).is_none());

        // Add some events to generate snapshots
        for i in 0..3 {
            let tx = TxEvent {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                pool_amm_id: pool_key,
                base_mint: pool_key,
                pool_state: PoolLifecycle::Active,
                metrics: PoolMetrics::default(),
                slot: Some(100 + i),
                timestamp_ms: 1000 + (i * 150), // Exceeds 100ms interval
                event_time: ghost_core::EventTimeMetadata::default(),
                signer: test_pubkey(10 + i as u8),
                is_buy: true,
                volume_sol: 5.0 * (i as f64 + 1.0),
                reserve_base: None,
                reserve_quote: None,
                price_quote: None,
                is_dev_buy: false,
                dev_buy_lamports: 0,
                signature: None,
                event_ordinal: None,
                block_time: None,
                arrival_time_ms: None,
                data_source: DataSource::SoftTruth,
                intra_slot_offset_ms: None,
                raw_data: None,
                raw_data_missing_reason: RawBytesMissingReason::Unknown,
            };
            engine.handle_tx_event(&tx);
        }

        let pair = engine.latest_pair(&pool_key);
        assert!(pair.is_some());

        let (t0, t1) = pair.unwrap();
        // t0 should be older than t1
        assert!(t0.timestamp_ms < t1.timestamp_ms);
        assert_eq!(t1.timestamp_ms, 1300); // Most recent
        assert_eq!(t0.timestamp_ms, 1150); // Second most recent
    }

    #[test]
    fn test_multiple_pools_independent() {
        let engine = SnapshotEngine::new(128, 100);

        let pool1 = test_pubkey(1);
        let pool2 = test_pubkey(2);
        engine.mark_pool_active(pool1);
        engine.mark_pool_active(pool2);

        // Events for pool 1
        let tx1 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool1,
            base_mint: pool1,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 10.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx1);

        let tx2 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool1,
            base_mint: pool1,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 2,
                unique_addrs: 2,
                volume_sol: 15.0,
                buy_volume_sol: 15.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(101),
            timestamp_ms: 1200,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(11),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx2);

        // Events for pool 2
        let tx3 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool2,
            base_mint: pool2,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(200),
            timestamp_ms: 2000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(20),
            is_buy: true,
            volume_sol: 20.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx3);

        let tx4 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool2,
            base_mint: pool2,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 2,
                unique_addrs: 2,
                volume_sol: 35.0,
                buy_volume_sol: 20.0,
                sell_volume_sol: 15.0,
                dev_buy_lamports: 0,
            },
            slot: Some(201),
            timestamp_ms: 2250,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(21),
            is_buy: false,
            volume_sol: 15.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx4);

        // Verify pool 1
        let pool1_snaps = engine.last_n(&pool1, 10);
        assert_eq!(pool1_snaps.len(), 2);
        assert_eq!(pool1_snaps[0].cum_volume_sol, 15.0); // 10 + 5

        // Verify pool 2
        let pool2_snaps = engine.last_n(&pool2, 10);
        assert_eq!(pool2_snaps.len(), 2);
        assert_eq!(pool2_snaps[0].cum_volume_sol, 35.0); // 20 + 15
        assert_eq!(pool2_snaps[0].cum_buy_volume_sol, 20.0);
        assert_eq!(pool2_snaps[0].cum_sell_volume_sol, 15.0);
    }

    // ========== LEGACY VERIFICATION TESTS (NO-OP) ==========

    #[test]
    fn test_reorg_detection_noop_in_snapshot_engine() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(1);
        engine.mark_pool_active(pool_key);

        // Track violations
        let violations = Arc::new(Mutex::new(Vec::new()));
        let violations_clone = Arc::clone(&violations);

        // Set up callback
        let mut engine_mut = engine;
        engine_mut.set_integrity_callback(Arc::new(move |violation| {
            violations_clone.lock().push(violation);
        }));

        // First transaction at slot 100
        let tx1 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 5.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("sig1".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine_mut.handle_tx_event(&tx1);

        // Second transaction at slot 99 (reorg!)
        let tx2 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 2,
                unique_addrs: 2,
                volume_sol: 8.0,
                buy_volume_sol: 8.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            slot: Some(99), // Slot regression
            timestamp_ms: 1100,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(11),
            is_buy: true,
            volume_sol: 3.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("sig2".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine_mut.handle_tx_event(&tx2);

        // SnapshotEngine no longer performs reorg detection
        let viols = violations.lock();
        assert!(viols.is_empty());
    }

    #[test]
    fn test_duplicate_transaction_noop_in_snapshot_engine() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(1);
        engine.mark_pool_active(pool_key);

        // Track violations
        let violations = Arc::new(Mutex::new(Vec::new()));
        let violations_clone = Arc::clone(&violations);

        // Set up callback
        let mut engine_mut = engine;
        engine_mut.set_integrity_callback(Arc::new(move |violation| {
            violations_clone.lock().push(violation);
        }));

        // First transaction
        let tx1 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 5.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("duplicate_sig".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine_mut.handle_tx_event(&tx1);

        // Duplicate transaction (same signature)
        let tx2 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 2,
                unique_addrs: 2,
                volume_sol: 8.0,
                buy_volume_sol: 8.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot: Some(101),
            timestamp_ms: 1100,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(11),
            is_buy: true,
            volume_sol: 3.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("duplicate_sig".to_string()), // Duplicate!
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine_mut.handle_tx_event(&tx2);

        let live = engine_mut
            .get_live_counters(pool_key)
            .expect("live counters");
        assert_eq!(live.cum_tx_count, 1);

        // SnapshotEngine performs duplicate detection (no new violations expected)
        let viols = violations.lock();
        assert!(viols.is_empty());
    }

    #[test]
    fn test_live_counters_now_ts_ms_uses_event_time_not_activity_time() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(60);
        engine.mark_pool_active(pool_key);

        // Use an old event timestamp so activity time diverges from event-time.
        let tx = make_tx_event(60, 1_000, Some(200));
        engine.handle_tx_event(&tx);

        let live = engine.get_live_counters(pool_key).expect("live counters");
        assert_eq!(
            live.now_ts_ms, 1_000,
            "now_ts_ms must track last_event_ts_ms (event-time), not last_activity_ts_ms"
        );
    }

    #[test]
    fn test_live_counters_default_event_ts_source_is_legacy_compat() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(65);
        engine.mark_pool_active(pool_key);
        engine.get_or_create_pool_state(pool_key);

        let live = engine.get_live_counters(pool_key).expect("live counters");
        assert_eq!(live.now_ts_ms, 0);
        assert_eq!(live.event_ts_source, EventTsSource::LegacyCompat);
    }

    #[test]
    fn test_handle_tx_event_uses_wallclock_fallback_when_event_time_missing() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(61);
        engine.mark_pool_active(pool_key);

        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100),
            timestamp_ms: 0,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(62),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("missing_event_time".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx);

        let live = engine.get_live_counters(pool_key).expect("live counters");
        assert_eq!(
            live.cum_tx_count, 1,
            "event must be ingested via wallclock fallback"
        );
        assert!(
            live.now_ts_ms > 0,
            "event-time axis must be set from fallback"
        );
        assert_eq!(live.event_ts_source, EventTsSource::Wallclock);
    }

    #[test]
    fn test_handle_tx_event_labels_legacy_timestamp_as_legacy_compat() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(63);
        engine.mark_pool_active(pool_key);

        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100),
            timestamp_ms: 123_456,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(64),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("legacy_compat".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx);

        let live = engine.get_live_counters(pool_key).expect("live counters");
        assert_eq!(live.now_ts_ms, 123_456);
        assert_eq!(live.event_ts_source, EventTsSource::LegacyCompat);
    }

    #[test]
    fn test_emitted_snapshot_preserves_last_event_ts_source() {
        let engine = SnapshotEngine::new(128, 0);
        let pool_key = test_pubkey(66);
        engine.mark_pool_active(pool_key);

        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(101),
            timestamp_ms: 0,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(9_000), None),
            signer: test_pubkey(67),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("snapshot_source".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: Some(12_000),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        engine.handle_tx_event(&tx);

        let snapshot = engine
            .get_latest_snapshot(&pool_key)
            .expect("latest snapshot");
        assert_eq!(snapshot.timestamp_ms, 9_000);
        assert_eq!(snapshot.event_ts_source, EventTsSource::IngressWall);
    }

    #[test]
    fn test_validate_soft_vs_hard_truth_ignores_non_chain_soft_snapshots() {
        let engine = SnapshotEngine::new(16, 100);
        let pool_key = test_pubkey(68);
        let pool_state = engine.get_or_create_pool_state(pool_key);
        let mut ps = pool_state.lock();

        let mut ingress_soft = MarketSnapshot {
            timestamp_ms: 1_000,
            event_ts_source: EventTsSource::IngressWall,
            cum_volume_sol: 100.0,
            tx_count: 1,
            unique_addrs: 1,
            ..Default::default()
        };
        ingress_soft.set_data_source(DataSource::SoftTruth);
        ps.snapshots
            .push(ingress_soft, ingress_soft.timestamp_ms, 60_000);

        let mut chain_soft = MarketSnapshot {
            timestamp_ms: 1_020,
            event_ts_source: EventTsSource::Event,
            cum_volume_sol: 5.0,
            tx_count: 1,
            unique_addrs: 1,
            ..Default::default()
        };
        chain_soft.set_data_source(DataSource::SoftTruth);
        ps.snapshots
            .push(chain_soft, chain_soft.timestamp_ms, 60_000);
        drop(ps);

        let mut hard_truth = MarketSnapshot {
            timestamp_ms: 1_005,
            cum_volume_sol: 5.0,
            ..Default::default()
        };
        hard_truth.set_data_source(DataSource::HardTruth);

        assert!(
            engine.validate_soft_vs_hard_truth(&pool_key, &hard_truth, 0.10),
            "validation must ignore nearer ingress-wall soft snapshots and compare only against chain-timed soft truth"
        );
    }

    #[test]
    fn test_last_event_ts_monotonic_even_with_out_of_order_timestamps() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(70);
        engine.mark_pool_active(pool_key);

        let tx_newer = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics::default(),
            slot: Some(100),
            timestamp_ms: 2_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(71),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("mono-newer".to_string()),
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine.handle_tx_event(&tx_newer);

        let tx_older = TxEvent {
            timestamp_ms: 1_000,
            signature: Some("mono-older".to_string()),
            signer: test_pubkey(72),
            ..tx_newer.clone()
        };
        engine.handle_tx_event(&tx_older);

        let live = engine.get_live_counters(pool_key).expect("live counters");
        assert_eq!(
            live.now_ts_ms, 2_000,
            "monotonic contract: last_event_ts_ms must not regress"
        );
    }

    #[test]
    fn test_excessive_jitter_noop_in_snapshot_engine() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(1);
        engine.mark_pool_active(pool_key);

        // Track violations
        let violations = Arc::new(Mutex::new(Vec::new()));
        let violations_clone = Arc::clone(&violations);

        // Set up callback
        let mut engine_mut = engine;
        engine_mut.set_integrity_callback(Arc::new(move |violation| {
            violations_clone.lock().push(violation);
        }));

        // Transaction with excessive jitter
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 5.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("sig1".to_string()),
            event_ordinal: None,
            block_time: Some(1),         // 1 second Unix timestamp
            arrival_time_ms: Some(5000), // 5 seconds in ms = 4 second jitter!
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine_mut.handle_tx_event(&tx);

        // SnapshotEngine does not perform jitter checks
        let viols = violations.lock();
        assert!(viols.is_empty());
    }

    #[test]
    fn test_normal_jitter_noop_in_snapshot_engine() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(1);

        // Track violations
        let violations = Arc::new(Mutex::new(Vec::new()));
        let violations_clone = Arc::clone(&violations);

        // Set up callback
        let mut engine_mut = engine;
        engine_mut.set_integrity_callback(Arc::new(move |violation| {
            violations_clone.lock().push(violation);
        }));

        // Transaction with acceptable jitter
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_key,
            base_mint: pool_key,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 5.0,
                buy_volume_sol: 5.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot: Some(100),
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(10),
            is_buy: true,
            volume_sol: 5.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("sig1".to_string()),
            event_ordinal: None,
            block_time: Some(1),         // 1 second Unix timestamp
            arrival_time_ms: Some(1500), // 1.5 seconds in ms = 500ms jitter (OK)
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        engine_mut.handle_tx_event(&tx);

        // SnapshotEngine does not perform jitter checks
        let viols = violations.lock();
        assert!(viols.is_empty());
    }

    #[test]
    fn test_volume_cross_check_detects_desync() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(1);

        // Track violations
        let violations = Arc::new(Mutex::new(Vec::new()));
        let violations_clone = Arc::clone(&violations);

        // Set up callback
        let mut engine_mut = engine;
        engine_mut.set_integrity_callback(Arc::new(move |violation| {
            violations_clone.lock().push(violation);
        }));

        // Shadow Ledger reports empty pool
        let shadow_reserves = (0.0, 0.0);

        // But we see a 5 SOL transaction
        engine_mut.check_volume_cross_check(&pool_key, shadow_reserves, 5.0, 1.0);

        // Verify HardAbort violation was triggered
        let viols = violations.lock();
        assert_eq!(viols.len(), 1);
        assert_eq!(viols[0].severity, IntegritySeverity::HardAbort);
        assert!(viols[0].details.contains("Shadow Ledger"));
    }

    #[test]
    fn test_volume_cross_check_no_violation_when_consistent() {
        let engine = SnapshotEngine::new(128, 100);
        let pool_key = test_pubkey(1);

        // Track violations
        let violations = Arc::new(Mutex::new(Vec::new()));
        let violations_clone = Arc::clone(&violations);

        // Set up callback
        let mut engine_mut = engine;
        engine_mut.set_integrity_callback(Arc::new(move |violation| {
            violations_clone.lock().push(violation);
        }));

        // Shadow Ledger reports active pool
        let shadow_reserves = (100.0, 50.0);

        // Transaction is consistent with active pool
        engine_mut.check_volume_cross_check(&pool_key, shadow_reserves, 5.0, 1.0);

        // Verify no violation was triggered
        let viols = violations.lock();
        assert_eq!(viols.len(), 0);
    }

    #[test]
    fn test_unapproved_tx_event_is_processed_when_active() {
        let metrics = Arc::new(SnapshotMetrics::new(None));
        let engine = SnapshotEngine::with_metrics(8, 100, Some(metrics.clone()), None);
        let approved = Arc::new(ApprovedPools::new());
        engine.set_approved_pools(approved);

        let pool_id = test_pubkey(90);
        let base_mint = test_pubkey(91);
        engine.mark_pool_active(pool_id);
        let tx = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_id,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 1.0,
                buy_volume_sol: 1.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(92),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("sig".to_string()),
            event_ordinal: None,
            block_time: Some(1),
            arrival_time_ms: Some(2),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        };

        engine.handle_tx_event(&tx);

        assert!(engine.has_pool(&pool_id));
        assert!(engine.get_transactions(&pool_id).len() > 0);
    }

    #[test]
    fn test_handle_tx_event_keeps_decision_tx_when_later_legacy_compat_arrives() {
        let engine = SnapshotEngine::new(16, 0);
        let pool_seed = 210;
        let pool = test_pubkey(pool_seed);
        engine.mark_pool_active(pool);

        let mut decision_tx = make_tx_event(pool_seed, 5_000, Some(1));
        decision_tx.signature = Some("decision".to_string());
        decision_tx.event_time.ingress_wall_ts_ms = Some(5_000);

        let mut compat_tx = make_tx_event(pool_seed, 50_000, Some(2));
        compat_tx.signature = Some("compat".to_string());

        engine.handle_tx_event(&decision_tx);
        engine.handle_tx_event(&compat_tx);

        let buffered = engine.get_transactions(&pool);
        assert_eq!(buffered.len(), 2);
        assert!(
            buffered.iter().any(|tx| {
                tx.signature == "decision"
                    && tx.event_ts_source == EventTsSource::IngressWall
                    && tx.decision_event_ts_ms() == Some(5_000)
            }),
            "legacy-compat timestamps must not evict prior decision txs from the buffer"
        );
        assert!(
            buffered.iter().any(|tx| {
                tx.signature == "compat" && tx.event_ts_source == EventTsSource::LegacyCompat
            }),
            "legacy-compat tx should remain visible for non-decision observability"
        );
    }

    #[test]
    fn test_inactive_tx_is_buffered_and_replayed_on_activation() {
        let engine = SnapshotEngine::new(16, 0);
        let tx = make_tx_event(10, 1_000, Some(1));
        let pool = tx.pool_amm_id;

        // Pool is not active yet: event must not be dropped, only buffered.
        engine.handle_tx_event(&tx);
        assert!(
            !engine.has_pool(&pool),
            "Pool state must not be created before activation"
        );
        assert!(
            engine.get_transactions(&pool).is_empty(),
            "No transactions should be visible before replay"
        );

        // Activation should replay buffered tx into normal pipeline.
        engine.mark_pool_active(pool);
        let replayed = engine.get_transactions(&pool);
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].signature, tx.signature.unwrap_or_default());
    }

    fn make_init_event(pool_seed: u8, ts: u64) -> InitPoolEvent {
        InitPoolEvent {
            pool_amm_id: test_pubkey(pool_seed),
            base_mint: test_pubkey(pool_seed.saturating_add(1)),
            quote_mint: test_pubkey(pool_seed.saturating_add(2)),
            slot: Some(1),
            timestamp_ms: ts,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1_000.0,
            initial_reserve_quote: 1.0,
            initial_price_quote: 0.001,
        }
    }

    fn make_tx_event(pool: u8, ts: u64, slot: Option<u64>) -> TxEvent {
        TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: test_pubkey(pool),
            base_mint: test_pubkey(pool.saturating_add(1)),
            pool_state: PoolLifecycle::Active,
            metrics: PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: 1.0,
                buy_volume_sol: 1.0,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
                ..Default::default()
            },
            slot,
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(pool.saturating_add(3)),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: None,
            event_ordinal: None,
            block_time: None,
            arrival_time_ms: None,
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        }
    }

    #[test]
    fn test_handle_tx_event_auto_tracks_active_pool() {
        let engine = SnapshotEngine::new(16, 0);
        let tx = make_tx_event(249, 1_000, Some(10));
        let pool = tx.pool_amm_id;

        assert!(!engine.active_pools.read().contains(&pool));
        engine.handle_tx_event(&tx);

        assert!(engine.active_pools.read().contains(&pool));
        assert_eq!(engine.last_n(&pool, 10).len(), 1);
        assert!(
            !engine.pending_inactive_txs.read().contains_key(&pool),
            "active tx should self-heal instead of buffering as pool_not_active"
        );
    }

    #[test]
    fn test_bootstrap_snapshots_zero_volume() {
        let engine = SnapshotEngine::new(16, 0);
        let init_event = make_init_event(250, 10);
        engine.mark_pool_active(init_event.pool_amm_id);
        engine.handle_initialize_pool_event(&init_event);

        let pools = engine.pools.read();
        let ps = pools.get(&init_event.pool_amm_id).unwrap().lock();
        let snaps = ps.snapshots.get_last_n(3);

        assert_eq!(snaps.len(), 3);
        for snap in snaps {
            assert_eq!(snap.cum_volume_sol, 0.0);
            assert_eq!(snap.tx_count, 0);
            assert_eq!(snap.cum_buy_volume_sol, 0.0);
            assert_eq!(snap.cum_sell_volume_sol, 0.0);
            assert_eq!(snap.window_volume_sol, 0.0);
            assert_eq!(snap.window_buy_volume_sol, 0.0);
            assert_eq!(snap.window_sell_volume_sol, 0.0);
            assert_eq!(snap.window_tx_count, 0);
        }
    }

    #[test]
    fn test_baseline_ignores_bootstrap_and_starts_with_real_snapshots() {
        let mut engine = SnapshotEngine::new(16, 0);
        engine.set_resync_config(ResyncConfig {
            resync_interval_slots: 1,
            max_volume_deviation: 2.0,
            min_volume_threshold_sol: 0.001,
            enable_token_graph_validation: false,
            enable_mint_supply_validation: false,
        });

        let init_event = make_init_event(251, 100);
        engine.mark_pool_active(init_event.pool_amm_id);
        engine.handle_initialize_pool_event(&init_event);

        // First real tx - baseline should initialize immediately
        let tx1 = make_tx_event(251, 200, Some(2));
        engine.handle_tx_event(&tx1);

        // Second tx emits first real snapshot (baseline can refine on resync)
        let tx2 = make_tx_event(251, 201, Some(3));
        engine.handle_tx_event(&tx2);

        // Third tx triggers resync that sees the real snapshot
        let tx3 = make_tx_event(251, 202, Some(4));
        engine.handle_tx_event(&tx3);

        let pools = engine.pools.read();
        let ps = pools.get(&init_event.pool_amm_id).unwrap().lock();

        let recent = ps.snapshots.get_last_n(4);
        let real_snapshot = recent
            .iter()
            .find(|s| s.tx_count > 0 && s.cum_volume_sol > 0.0)
            .expect("expected real snapshot");
        assert!(real_snapshot.timestamp_ms >= 200);
    }

    #[test]
    fn test_handle_initialize_pool_event_accepts_missing_slot_metadata() {
        // Missing slot metadata should be accepted
        let engine = SnapshotEngine::new(16, 0);
        let pool = test_pubkey(42);
        let mint = test_pubkey(43);
        let quote_mint = test_pubkey(44);
        engine.mark_pool_active(pool);

        let init_event = InitPoolEvent {
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint,
            slot: None, // Unknown slot metadata
            timestamp_ms: 1700000000000,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1000.0,
            initial_reserve_quote: 100.0,
            initial_price_quote: 0.1,
        };

        // Call handle_initialize_pool_event
        engine.handle_initialize_pool_event(&init_event);

        // Verify that pool state was created and slot metadata is None
        let pools = engine.pools.read();
        let ps = pools
            .get(&pool)
            .expect("pool state should be created")
            .lock();
        let latest = ps
            .snapshots
            .latest()
            .expect("bootstrap snapshot should exist");
        assert!(latest.slot.is_none(), "missing slot should remain None");
    }

    #[test]
    fn test_handle_initialize_pool_event_accepts_valid_slot() {
        // EPIC 5: Test that valid slot bootstrap events are accepted
        let engine = SnapshotEngine::new(16, 0);
        let pool = test_pubkey(44);
        let mint = test_pubkey(45);
        let quote_mint = test_pubkey(46);
        engine.mark_pool_active(pool);

        let init_event = InitPoolEvent {
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint,
            slot: Some(12345), // VALID: slot > 0
            timestamp_ms: 1700000000000,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1000.0,
            initial_reserve_quote: 100.0,
            initial_price_quote: 0.1,
        };

        // Call handle_initialize_pool_event
        engine.handle_initialize_pool_event(&init_event);

        // Verify that pool state was created
        let pools = engine.pools.read();
        assert!(
            pools.get(&pool).is_some(),
            "Pool state should be created for valid slot bootstrap event"
        );

        // Verify bootstrap snapshots were created
        let ps = pools.get(&pool).unwrap().lock();
        assert_eq!(
            ps.snapshots.len(),
            3,
            "Should have 3 bootstrap snapshots (g0, g1, g2)"
        );
        assert_eq!(
            ps.last_snapshot_slot,
            Some(12345),
            "Last snapshot slot should match init event"
        );
    }

    #[test]
    fn test_handle_initialize_pool_event_normalizes_zero_slot_to_none() {
        let engine = SnapshotEngine::new(16, 0);
        let pool = test_pubkey(47);
        let mint = test_pubkey(48);
        let quote_mint = test_pubkey(49);
        engine.mark_pool_active(pool);

        let init_event = InitPoolEvent {
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint,
            slot: Some(0),
            timestamp_ms: 1700000000000,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1000.0,
            initial_reserve_quote: 100.0,
            initial_price_quote: 0.1,
        };

        engine.handle_initialize_pool_event(&init_event);

        let pools = engine.pools.read();
        let ps = pools
            .get(&pool)
            .expect("pool state should be created")
            .lock();
        assert_eq!(ps.last_snapshot_slot, None, "slot=0 must normalize to None");
    }

    #[test]
    fn test_handle_tx_event_normalizes_zero_slot_to_none() {
        let engine = SnapshotEngine::new(16, 0);
        let pool = test_pubkey(50);
        engine.mark_pool_active(pool);
        let tx = make_tx_event(50, 1700000000100, Some(0));

        engine.handle_tx_event(&tx);

        let pools = engine.pools.read();
        let ps = pools
            .get(&pool)
            .expect("pool state should be created")
            .lock();
        let latest = ps.snapshots.latest().expect("snapshot should exist");
        assert_eq!(latest.slot, None, "slot=0 must normalize to None");
        assert_eq!(ps.last_snapshot_slot, None, "slot=0 must normalize to None");
    }

    // =========================================================================
    // PR-3b contract tests
    // =========================================================================

    /// Two events with the SAME signature but DIFFERENT event_ordinal are distinct
    /// canonical trades (e.g. multi-trade tx). SnapshotEngine must NOT drop the second.
    #[test]
    fn snapshot_engine_does_not_drop_second_trade_same_signature() {
        let engine = SnapshotEngine::new(64, 0); // 0ms interval → emit every event
        let pool = test_pubkey(200);
        engine.mark_pool_active(pool);
        let base_mint = test_pubkey(201);
        let ts = 1_700_000_000_000u64;

        // ordinal=0 — first canonical trade in the tx
        let ev0 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 1.0),
            slot: Some(500),
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(202),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(1_000.0),
            reserve_quote: Some(1.0),
            price_quote: Some(0.001),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("SameSignatureABC".to_string()),
            event_ordinal: Some(0),
            block_time: Some((ts / 1000) as i64),
            arrival_time_ms: Some(ts + 50),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        // ordinal=1 — SECOND canonical trade in the SAME tx (same signature)
        let ev1 = TxEvent {
            event_ordinal: Some(1),
            metrics: mk_metrics(2, 1, 2.0),
            volume_sol: 1.0,
            ..ev0.clone()
        };

        engine.handle_tx_event(&ev0);
        engine.handle_tx_event(&ev1);

        let snaps = engine.last_n(&pool, 10);
        assert!(
            snaps.len() >= 2,
            "Both trades (ordinal=0 and ordinal=1) must be accepted — got {}",
            snaps.len()
        );
    }

    /// SnapshotListener is the single-ingress authority. A second call from a different
    /// path with the same TxKey (same sig + same ordinal) must be deduped; the second
    /// *distinct* event (different ordinal) must NOT be deduped.
    #[test]
    fn single_ingress_wins() {
        let engine = SnapshotEngine::new(64, 0);
        let pool = test_pubkey(203);
        engine.mark_pool_active(pool);
        let base_mint = test_pubkey(204);
        let ts = 1_700_000_100_000u64;

        let ev = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 1.5),
            slot: Some(600),
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(205),
            is_buy: true,
            volume_sol: 1.5,
            reserve_base: Some(900.0),
            reserve_quote: Some(1.5),
            price_quote: Some(0.0015),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("SingleIngressSig".to_string()),
            event_ordinal: Some(0),
            block_time: Some((ts / 1000) as i64),
            arrival_time_ms: Some(ts + 30),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        // Authoritative path (SnapshotListener)
        engine.handle_tx_event(&ev);
        let count_after_first = engine.last_n(&pool, 20).len();
        assert!(count_after_first >= 1, "First ingress must be accepted");

        // Duplicate call with same key — simulates a competing path (oracle_runtime)
        engine.handle_tx_event(&ev);
        let count_after_dup = engine.last_n(&pool, 20).len();
        assert_eq!(
            count_after_dup, count_after_first,
            "Duplicate TxKey must be deduped — snapshot count must not grow (was {}, got {})",
            count_after_first, count_after_dup
        );
    }

    #[test]
    fn pool_not_active_gate_buffers_until_track_pool() {
        let mut engine = SnapshotEngine::new(64, 0);
        engine.set_inactive_tx_buffer_policy(64, u64::MAX / 4);
        let pool = test_pubkey(230);
        let base_mint = test_pubkey(231);
        let ts = SnapshotEngine::now_ms();
        let shared_sig = Signature::new_unique().to_string();

        let ev0 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 1.0),
            slot: Some(650),
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(232),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(950.0),
            reserve_quote: Some(1.0),
            price_quote: Some(1.0 / 950.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(shared_sig.clone()),
            event_ordinal: Some(0),
            block_time: Some((ts / 1000) as i64),
            arrival_time_ms: Some(ts + 20),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        let ev1 = TxEvent {
            metrics: mk_metrics(2, 1, 2.0),
            volume_sol: 1.0,
            reserve_base: Some(940.0),
            reserve_quote: Some(1.1),
            price_quote: Some(1.1 / 940.0),
            event_ordinal: Some(1),
            ..ev0.clone()
        };

        engine.handle_tx_event(&ev0);
        engine.handle_tx_event(&ev1);

        assert!(
            engine.last_n(&pool, 10).is_empty(),
            "untracked pool must not accept tx directly before track_pool"
        );
        {
            let pending = engine.pending_inactive_txs.read();
            let queue = pending
                .get(&pool)
                .expect("recoverable txs must be buffered behind pool_not_active gate");
            assert_eq!(
                queue.len(),
                2,
                "pool_not_active buffer must retain both legal events under the same signature"
            );
        }

        engine.track_pool(pool);

        {
            let pending = engine.pending_inactive_txs.read();
            assert!(
                !pending.contains_key(&pool),
                "buffer must be drained once track_pool authorizes replay"
            );
        }
        let snaps = engine.last_n(&pool, 10);
        assert!(
            snaps.len() >= 2,
            "both buffered ordinals must replay after track_pool (got {})",
            snaps.len()
        );
    }

    #[test]
    fn inactive_buffer_ttl_ignores_storage_only_event_timestamps() {
        let mut engine = SnapshotEngine::new(64, 0);
        engine.set_inactive_tx_buffer_policy(64, 10_000);

        let mut decision = make_tx_event(231, 5_000, Some(1));
        let pool = decision.pool_amm_id;
        decision.signature = Some("inactive-decision".to_string());
        decision.event_time.ingress_wall_ts_ms = Some(5_000);

        let mut compat = make_tx_event(231, 50_000, Some(2));
        compat.signature = Some("inactive-compat".to_string());

        engine.buffer_inactive_tx(&decision, "pool_not_active");
        engine.buffer_inactive_tx(&compat, "pool_not_active");

        let pending = engine.pending_inactive_txs.read();
        let queue = pending
            .get(&pool)
            .expect("inactive queue should retain both buffered events");
        assert_eq!(queue.len(), 2);
        assert!(
            queue.iter().any(|buffered| {
                buffered.event.signature.as_deref() == Some("inactive-decision")
                    && buffered.event.event_time.ingress_wall_ts_ms == Some(5_000)
            }),
            "storage-only raw timestamps must not age-trim earlier decision txs"
        );
    }

    #[test]
    fn inactive_buffer_overflow_prefers_dropping_storage_only_events() {
        let mut engine = SnapshotEngine::new(64, 0);
        engine.set_inactive_tx_buffer_policy(2, u64::MAX / 4);

        let mut decision = make_tx_event(232, 1_000, Some(1));
        let pool = decision.pool_amm_id;
        decision.signature = Some("decision".to_string());
        decision.event_time.ingress_wall_ts_ms = Some(1_000);

        let mut compat_a = make_tx_event(232, 20_000, Some(2));
        compat_a.signature = Some("compat-a".to_string());

        let mut compat_b = make_tx_event(232, 30_000, Some(3));
        compat_b.signature = Some("compat-b".to_string());

        engine.buffer_inactive_tx(&decision, "pool_not_active");
        engine.buffer_inactive_tx(&compat_a, "pool_not_active");
        engine.buffer_inactive_tx(&compat_b, "pool_not_active");

        let pending = engine.pending_inactive_txs.read();
        let queue = pending
            .get(&pool)
            .expect("inactive queue should keep buffered events");
        assert_eq!(queue.len(), 2);
        assert!(
            queue
                .iter()
                .any(|buffered| buffered.event.signature.as_deref() == Some("decision")),
            "storage-only events must be evicted before decision-eligible txs under overflow"
        );
        assert_eq!(
            queue
                .iter()
                .filter(|buffered| SnapshotEngine::decision_event_time(&buffered.event).is_none())
                .count(),
            1,
            "only one storage-only event should remain once the queue is at capacity"
        );
    }

    #[test]
    fn track_pool_replay_does_not_let_storage_only_timestamps_outrank_decision_tx() {
        let mut engine = SnapshotEngine::new(64, 0);
        engine.set_inactive_tx_buffer_policy(64, u64::MAX / 4);

        let mut decision = make_tx_event(233, 5_000, Some(1));
        let pool = decision.pool_amm_id;
        decision.signature = Some("decision".to_string());
        decision.event_time.ingress_wall_ts_ms = Some(5_000);
        decision.reserve_base = Some(990.0);
        decision.reserve_quote = Some(1.1);
        decision.price_quote = Some(1.1 / 990.0);

        let mut compat = make_tx_event(233, 50_000, Some(2));
        compat.signature = Some("compat".to_string());
        compat.reserve_base = Some(995.0);
        compat.reserve_quote = Some(1.05);
        compat.price_quote = Some(1.05 / 995.0);

        engine.buffer_inactive_tx(&decision, "pool_not_active");
        engine.buffer_inactive_tx(&compat, "pool_not_active");
        engine.track_pool(pool);

        let latest = engine
            .last_n(&pool, 10)
            .into_iter()
            .next()
            .expect("replayed queue should materialize snapshots");
        assert_eq!(
            latest.reserve_base, 990.0,
            "storage-only buffered timestamps must not replay after and outrank decision txs"
        );
        assert_eq!(latest.event_ts_source, EventTsSource::IngressWall);
    }

    /// An enriched event for the SAME canonical trade (same sig + same ordinal) must
    /// upgrade local state without being counted as a second trade.
    #[test]
    fn enriched_tx_path_does_not_lose_to_poorer_duplicate_path() {
        let engine = SnapshotEngine::new(64, 0);
        let pool = test_pubkey(206);
        engine.mark_pool_active(pool);
        let base_mint = test_pubkey(207);
        let ts = 1_700_000_200_000u64;

        // Poor event: no reserves, ordinal=0
        let poor = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 2.0),
            slot: Some(700),
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(208),
            is_buy: true,
            volume_sol: 2.0,
            reserve_base: None, // poor — no reserves
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some("EnrichedSig".to_string()),
            event_ordinal: Some(0),
            block_time: Some((ts / 1000) as i64),
            arrival_time_ms: Some(ts + 10),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };

        // Enriched event: SAME canonical identity, richer context arrives later.
        let enriched = TxEvent {
            reserve_base: Some(800.0),
            reserve_quote: Some(2.0),
            price_quote: Some(0.0025),
            data_source: DataSource::HardTruth,
            ..poor.clone()
        };

        engine.handle_tx_event(&poor);
        let count_after_poor = engine.last_n(&pool, 20).len();
        let poor_latest = engine
            .last_n(&pool, 20)
            .into_iter()
            .next()
            .expect("poor event should produce an initial snapshot");
        assert_eq!(poor_latest.reserve_base, 0.0);
        assert_eq!(poor_latest.reserve_quote, 0.0);
        assert_eq!(poor_latest.price_quote, 0.0);

        engine.handle_tx_event(&enriched);
        let snaps_after_enriched = engine.last_n(&pool, 20);
        let latest = snaps_after_enriched
            .first()
            .expect("enriched duplicate should leave a latest snapshot");
        assert!(
            snaps_after_enriched.len() == count_after_poor,
            "Enriched duplicate must upgrade SnapshotEngine state without adding a second snapshot"
        );
        assert_eq!(
            latest.tx_count, 1,
            "enrichment must not count as a second trade"
        );
        assert_eq!(
            latest.cum_volume_sol, 2.0,
            "enrichment must preserve cumulative volume for the canonical trade"
        );
        assert_eq!(latest.reserve_base, 800.0);
        assert_eq!(latest.reserve_quote, 2.0);
        assert_eq!(latest.price_quote, 0.0025);
        assert_eq!(latest.get_data_source(), DataSource::HardTruth);
    }

    /// PR-3b pre-commit boundary: SnapshotEngine MUST NOT write to ShadowLedger.
    /// ShadowLedger must have exactly the externally-committed entries — no more.
    #[test]
    fn snapshot_engine_precommit_does_not_write_shadow_ledger() {
        use ghost_core::shadow_ledger::types::{
            MarketSnapshot as GhostCoreMarketSnapshot, PriceState,
        };
        use ghost_core::shadow_ledger::ShadowLedger;

        let shadow_ledger = Arc::new(ShadowLedger::new());
        let mut engine = SnapshotEngine::new(64, 0);
        engine.set_shadow_ledger(Arc::clone(&shadow_ledger));

        let pool = test_pubkey(209);
        let base_mint = test_pubkey(210);
        let ts = 1_700_000_300_000u64;

        // External commit: exactly 1 canonical snapshot (simulates Gatekeeper commit)
        let seed = GhostCoreMarketSnapshot {
            slot: Some(800),
            timestamp_ms: ts,
            price_sol_per_token: 0.001,
            price_state: PriceState::Valid,
            reserve_base: 1_000.0,
            reserve_quote: 1.0,
            market_cap_sol: 1.0,
            ..Default::default()
        };
        shadow_ledger.commit_history(base_mint, vec![seed], None);

        let ledger_before = shadow_ledger
            .get_snapshots(&base_mint)
            .expect("ledger should have seed");
        assert_eq!(
            ledger_before.len(),
            1,
            "ShadowLedger starts with 1 committed snapshot"
        );

        engine.mark_pool_active(pool);

        // Bootstrap
        let init = InitPoolEvent {
            pool_amm_id: pool,
            base_mint,
            quote_mint: test_pubkey(211),
            slot: Some(800),
            timestamp_ms: ts,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1_000.0,
            initial_reserve_quote: 1.0,
            initial_price_quote: 0.001,
        };
        engine.handle_initialize_pool_event(&init);

        // Several tx events
        for i in 1u64..=5 {
            let ev = TxEvent {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                pool_amm_id: pool,
                base_mint,
                pool_state: PoolLifecycle::Active,
                metrics: mk_metrics(i, i, i as f64),
                slot: Some(800 + i),
                timestamp_ms: ts + i * 1000,
                event_time: ghost_core::EventTimeMetadata::default(),
                signer: test_pubkey(212),
                is_buy: true,
                volume_sol: i as f64,
                reserve_base: Some(1_000.0 - i as f64 * 10.0),
                reserve_quote: Some(1.0 + i as f64 * 0.1),
                price_quote: Some(0.001 + i as f64 * 0.0001),
                is_dev_buy: false,
                dev_buy_lamports: 0,
                signature: Some(format!("precommit_sig_{}", i)),
                event_ordinal: None,
                block_time: Some(((ts + i * 1000) / 1000) as i64),
                arrival_time_ms: Some(ts + i * 1000 + 50),
                data_source: DataSource::SoftTruth,
                intra_slot_offset_ms: None,
                raw_data: None,
                raw_data_missing_reason: RawBytesMissingReason::Unknown,
            };
            engine.handle_tx_event(&ev);
        }

        // LOCAL ring buffer must have data
        let local_snaps = engine.last_n(&pool, 20);
        assert!(
            !local_snaps.is_empty(),
            "LOCAL ring buffer must have snapshots"
        );

        // ShadowLedger must NOT have grown — still exactly 1 committed snapshot
        let ledger_after = shadow_ledger
            .get_snapshots(&base_mint)
            .expect("ledger should still have seed");
        assert_eq!(
            ledger_after.len(),
            1,
            "ShadowLedger must not receive pre-commit snapshots from SnapshotEngine (PR-3b), got {}",
            ledger_after.len()
        );
    }

    /// pool_not_active gate is recoverable: init events and buffered tx events with the
    /// same signature but different ordinals replay after activation without dropping the
    /// second legal trade, and replay ordering follows TxKey.
    #[test]
    fn pool_not_active_buffers_and_replays_recoverably() {
        let engine = SnapshotEngine::new(64, 0);
        let pool = test_pubkey(213);
        let base_mint = test_pubkey(214);
        let ts_base = SnapshotEngine::now_ms();
        let shared_sig = Signature::new_unique().to_string();

        let init = InitPoolEvent {
            pool_amm_id: pool,
            base_mint,
            quote_mint: test_pubkey(215),
            slot: Some(900),
            timestamp_ms: ts_base,
            initial_liquidity_sol: 1.0,
            initial_reserve_base: 1_000.0,
            initial_reserve_quote: 1.0,
            initial_price_quote: 0.001,
        };
        engine.handle_initialize_pool_event(&init);

        {
            let pending = engine.pending_inits.read();
            assert!(
                pending.contains_key(&pool),
                "Init event must be buffered in pending_inits before mark_pool_active"
            );
        }
        assert!(
            engine.last_n(&pool, 10).is_empty(),
            "Ring buffer must be empty before mark_pool_active"
        );

        let buffered0 = TxEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool,
            base_mint,
            pool_state: PoolLifecycle::Active,
            metrics: mk_metrics(1, 1, 1.0),
            slot: Some(901),
            timestamp_ms: ts_base + 500,
            event_time: ghost_core::EventTimeMetadata::default(),
            signer: test_pubkey(216),
            is_buy: true,
            volume_sol: 1.0,
            reserve_base: Some(990.0),
            reserve_quote: Some(1.1),
            price_quote: Some(1.1 / 990.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Some(shared_sig.clone()),
            event_ordinal: Some(0),
            block_time: Some(((ts_base + 500) / 1000) as i64),
            arrival_time_ms: Some(ts_base + 520),
            data_source: DataSource::SoftTruth,
            intra_slot_offset_ms: None,
            raw_data: None,
            raw_data_missing_reason: RawBytesMissingReason::Unknown,
        };
        let buffered1 = TxEvent {
            metrics: mk_metrics(2, 1, 3.0),
            volume_sol: 2.0,
            reserve_base: Some(970.0),
            reserve_quote: Some(1.3),
            price_quote: Some(1.3 / 970.0),
            event_ordinal: Some(1),
            ..buffered0.clone()
        };

        engine.buffer_inactive_tx(&buffered0, "pool_not_active");
        engine.buffer_inactive_tx(&buffered1, "pool_not_active");

        {
            let pending = engine.pending_inactive_txs.read();
            let queue = pending.get(&pool).expect("inactive txs must be buffered");
            assert_eq!(
                queue.len(),
                2,
                "inactive buffer must keep both legal trades under the same signature"
            );
        }

        engine.track_pool(pool);

        {
            let pending = engine.pending_inits.read();
            assert!(
                !pending.contains_key(&pool),
                "pending_inits must be cleared after track_pool"
            );
        }
        {
            let pending = engine.pending_inactive_txs.read();
            assert!(
                !pending.contains_key(&pool),
                "pending_inactive_txs must be drained after track_pool replay"
            );
        }

        let snaps_after_replay = engine.last_n(&pool, 10);
        assert!(
            snaps_after_replay.len() > 3,
            "track_pool must replay buffered tx events on top of bootstrap snapshots"
        );

        let latest = snaps_after_replay
            .first()
            .expect("latest snapshot must exist after replay");
        assert_eq!(
            latest.tx_count, 2,
            "both buffered trades must replay; second legal trade must not be dropped"
        );
        assert_eq!(latest.cum_volume_sol, 3.0);
        assert_eq!(
            latest.reserve_base, 970.0,
            "replay must preserve TxKey ordering so ordinal=1 becomes the latest state"
        );
        assert_eq!(latest.reserve_quote, 1.3);
        assert_eq!(
            latest.timestamp_ms,
            ts_base + 500,
            "replayed multi-event tx keeps canonical event timestamp"
        );

        let tx_records = engine.get_transactions(&pool);
        assert_eq!(
            tx_records.len(),
            2,
            "transaction buffer must retain both replayed events under the same signature"
        );
        assert!(
            tx_records.iter().all(|tx| tx.signature == shared_sig),
            "replayed records must preserve original signature for correlation"
        );
        assert_eq!(
            tx_records[0].sol_amount + tx_records[1].sol_amount,
            3.0,
            "transaction buffer must retain both replayed events"
        );
    }
}
