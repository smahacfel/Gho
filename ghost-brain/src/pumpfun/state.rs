//! Pump.fun Curve State Cache and Early Swap Ring Buffer
//!
//! This module provides real-time caching of Pump.fun bonding curve state snapshots
//! and a ring buffer for early swap events (T<2s window). These are used for
//! on-chain predictive analysis by Oracle, Chaos, Praecog, and MESA components.
//!
//! ## Architecture
//!
//! - **PumpCurveStateCache**: Event-driven cache storing curve state by bonding_curve pubkey
//! - **EarlySwapRingBuffer**: Fixed-size (32 elements) FIFO buffer with 2-second TTL
//! - **Zero-heap design**: Ring buffer uses fixed array for minimal allocation overhead
//!
//! ## Performance Target
//!
//! - Insert operations: <50μs (hot path)
//! - Cache lookups: O(1) with DashMap
//! - Memory: ~8KB per curve (snapshot + buffer)

use dashmap::{mapref::entry::Entry, DashMap};
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Size of the early swap ring buffer (must be power of 2 for efficient modulo)
pub const EARLY_SWAP_BUFFER_SIZE: usize = 32;

/// Time-to-live for swap events in milliseconds (2 seconds)
pub const SWAP_EVENT_TTL_MS: u64 = 2000;

/// Default genesis virtual SOL reserves (30 SOL in lamports)
pub const GENESIS_VIRTUAL_SOL_LAMPORTS: u64 = 30_000_000_000;

/// Default genesis virtual token reserves (~1.073B tokens)
pub const GENESIS_VIRTUAL_TOKEN_AMOUNT: u128 = 1_073_000_000_000_000;

/// Default fee basis points for genesis snapshot (1%)
pub const GENESIS_FEE_BPS: u16 = 100;

/// Heapless container for early swap events (zero allocation in hot path)
#[derive(Debug, Clone)]
pub struct EarlySwapEvents {
    /// Fixed-size array of events
    pub events: [Option<EarlySwapEvent>; EARLY_SWAP_BUFFER_SIZE],

    /// Number of valid events in the array
    pub len: u8,
}

impl EarlySwapEvents {
    /// Create empty events container
    pub fn new() -> Self {
        Self {
            events: [None; EARLY_SWAP_BUFFER_SIZE],
            len: 0,
        }
    }

    /// Check if there are no events
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get slice of valid events
    pub fn as_slice(&self) -> &[Option<EarlySwapEvent>] {
        &self.events[..self.len as usize]
    }

    /// Iterate over valid events
    pub fn iter(&self) -> impl Iterator<Item = &EarlySwapEvent> {
        self.events[..self.len as usize]
            .iter()
            .filter_map(|e| e.as_ref())
    }
}

impl Default for EarlySwapEvents {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of a Pump.fun bonding curve state
///
/// Contains virtual and real reserves, fees, and timing metadata.
/// All monetary values are in lamports for precision.
#[derive(Debug, Clone)]
pub struct CurveSnapshot {
    /// Virtual SOL reserves in lamports
    pub virtual_sol_reserves_lamports: u64,

    /// Virtual token reserves (native token amount)
    pub virtual_token_reserves: u128,

    /// Real SOL reserves in lamports (if available from on-chain account)
    pub real_sol_reserves_lamports: Option<u64>,

    /// Real token reserves (if available from on-chain account)
    pub real_token_reserves: Option<u128>,

    /// Fee in basis points (configurable, typically 100 = 1%)
    pub fee_bps: u16,

    /// Slot when this snapshot was captured (metadata only)
    pub last_update_slot: Option<u64>,

    /// Unix timestamp in milliseconds when snapshot was captured
    pub last_update_ts: u64,
}

impl CurveSnapshot {
    /// Create a new curve snapshot
    pub fn new(
        virtual_sol_reserves_lamports: u64,
        virtual_token_reserves: u128,
        fee_bps: u16,
        slot: Option<u64>,
    ) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            virtual_sol_reserves_lamports,
            virtual_token_reserves,
            real_sol_reserves_lamports: None,
            real_token_reserves: None,
            fee_bps,
            last_update_slot: slot,
            last_update_ts: now_ms,
        }
    }

    /// Update real reserves (from on-chain account data)
    pub fn with_real_reserves(mut self, sol_lamports: u64, token_amount: u128) -> Self {
        self.real_sol_reserves_lamports = Some(sol_lamports);
        self.real_token_reserves = Some(token_amount);
        self
    }

    /// Check if reserves are non-zero (invariant check)
    pub fn has_valid_reserves(&self) -> bool {
        self.virtual_sol_reserves_lamports > 0 && self.virtual_token_reserves > 0
    }
}

/// Early swap event for predictive analysis
///
/// Captures swap direction, amount, and timing for short-term pattern detection.
#[derive(Debug, Clone, Copy)]
pub struct EarlySwapEvent {
    /// Input amount in lamports (SOL) or token native units
    pub amount_in: u64,

    /// True if buy (SOL -> Token), false if sell (Token -> SOL)
    pub is_buy: bool,

    /// Unix timestamp in milliseconds
    pub timestamp_ms: u64,

    /// Slot when swap occurred
    pub slot: u64,
}

impl EarlySwapEvent {
    /// Create a new swap event
    pub fn new(amount_in: u64, is_buy: bool, slot: u64) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            amount_in,
            is_buy,
            timestamp_ms: now_ms,
            slot,
        }
    }

    /// Check if event is still valid (within TTL)
    pub fn is_valid(&self, current_ts_ms: u64) -> bool {
        current_ts_ms.saturating_sub(self.timestamp_ms) < SWAP_EVENT_TTL_MS
    }
}

/// Fixed-size ring buffer for early swap events
///
/// Uses a fixed array (zero-heap) with FIFO semantics and automatic TTL expiration.
/// Thread-safe through internal atomic operations.
#[derive(Debug)]
pub struct EarlySwapRingBuffer {
    /// Fixed-size buffer (stack-allocated, zero-heap)
    buffer: [Option<EarlySwapEvent>; EARLY_SWAP_BUFFER_SIZE],

    /// Write position (head)
    head: usize,

    /// Number of valid entries
    count: usize,
}

impl EarlySwapRingBuffer {
    /// Create a new empty ring buffer
    pub fn new() -> Self {
        Self {
            buffer: [None; EARLY_SWAP_BUFFER_SIZE],
            head: 0,
            count: 0,
        }
    }

    /// Push a new swap event (FIFO, overwrites oldest)
    pub fn push(&mut self, event: EarlySwapEvent) {
        self.buffer[self.head] = Some(event);
        self.head = (self.head + 1) % EARLY_SWAP_BUFFER_SIZE;
        if self.count < EARLY_SWAP_BUFFER_SIZE {
            self.count += 1;
        }
    }

    /// Get all valid events (within TTL) - heapless version
    pub fn get_valid_events(&self) -> EarlySwapEvents {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut result = EarlySwapEvents::new();
        let mut idx = 0u8;

        for &opt_event in self.buffer.iter() {
            if let Some(event) = opt_event {
                if event.is_valid(now_ms) && (idx as usize) < EARLY_SWAP_BUFFER_SIZE {
                    result.events[idx as usize] = Some(event);
                    idx += 1;
                }
            }
        }

        result.len = idx;
        result
    }

    /// Get number of events in buffer (may include expired)
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.buffer = [None; EARLY_SWAP_BUFFER_SIZE];
        self.head = 0;
        self.count = 0;
    }
}

impl Default for EarlySwapRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Cached state for a single bonding curve
#[derive(Debug)]
struct CurveState {
    /// Current snapshot of reserves and fees (None if not yet received)
    snapshot: Option<CurveSnapshot>,

    /// Ring buffer of recent swap events
    swaps: parking_lot::Mutex<EarlySwapRingBuffer>,
}

/// Telemetry metrics for cache performance
#[derive(Debug, Default)]
pub struct CacheMetrics {
    /// Total number of snapshot updates
    pub snapshot_updates: AtomicU64,

    /// Total number of swap events recorded
    pub swap_events: AtomicU64,

    /// Number of cache hits
    pub cache_hits: AtomicU64,

    /// Number of cache misses
    pub cache_misses: AtomicU64,

    /// Number of snapshot updates with valid (non-zero) reserves
    pub valid_reserves_updates: AtomicU64,
}

impl CacheMetrics {
    /// Calculate cache hit rate as percentage
    pub fn hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;

        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    }

    /// Get total number of swap events in all buffers
    pub fn total_swaps(&self) -> u64 {
        self.swap_events.load(Ordering::Relaxed)
    }
}

/// Event-driven cache for Pump.fun bonding curve states
///
/// Provides O(1) lookups and updates for curve snapshots and swap events.
/// Thread-safe and optimized for high-frequency updates from gRPC streams.
pub struct PumpCurveStateCache {
    /// Cache storage (bonding_curve -> state)
    cache: Arc<DashMap<Pubkey, CurveState>>,

    /// Telemetry metrics
    metrics: Arc<CacheMetrics>,
}

impl PumpCurveStateCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            metrics: Arc::new(CacheMetrics::default()),
        }
    }

    /// Update curve snapshot (event-driven from gRPC)
    ///
    /// Creates or replaces the snapshot for the given bonding curve.
    /// Returns true if this is a new curve, false if updating existing.
    pub fn update_snapshot(&self, bonding_curve: Pubkey, snapshot: CurveSnapshot) -> bool {
        self.metrics
            .snapshot_updates
            .fetch_add(1, Ordering::Relaxed);

        if snapshot.has_valid_reserves() {
            self.metrics
                .valid_reserves_updates
                .fetch_add(1, Ordering::Relaxed);
        }

        let is_new = !self.cache.contains_key(&bonding_curve);

        self.cache
            .entry(bonding_curve)
            .and_modify(|state| state.snapshot = Some(snapshot.clone()))
            .or_insert_with(|| CurveState {
                snapshot: Some(snapshot),
                swaps: parking_lot::Mutex::new(EarlySwapRingBuffer::new()),
            });

        is_new
    }

    /// Add swap event to ring buffer
    ///
    /// Appends the event to the curve's swap buffer. Creates cache entry if needed.
    /// Note: Swaps can be recorded even before snapshot arrives (snapshot will be None).
    pub fn update_swap(&self, bonding_curve: Pubkey, event: EarlySwapEvent) {
        self.metrics.swap_events.fetch_add(1, Ordering::Relaxed);

        let entry = self.cache.entry(bonding_curve).or_insert_with(|| {
            // Create entry with no snapshot yet (swaps can arrive before snapshot)
            CurveState {
                snapshot: None,
                swaps: parking_lot::Mutex::new(EarlySwapRingBuffer::new()),
            }
        });

        entry.swaps.lock().push(event);
    }

    /// Check if cache contains an entry for the given bonding curve
    pub fn contains(&self, bonding_curve: &Pubkey) -> bool {
        self.cache.contains_key(bonding_curve)
    }

    /// Inject initial (genesis) snapshot for new token to avoid cold start
    ///
    /// Does nothing if an entry already exists for the bonding curve.
    pub fn inject_genesis(&self, bonding_curve: &Pubkey, slot: u64) {
        let genesis_snapshot = CurveSnapshot {
            virtual_sol_reserves_lamports: GENESIS_VIRTUAL_SOL_LAMPORTS,
            virtual_token_reserves: GENESIS_VIRTUAL_TOKEN_AMOUNT,
            real_sol_reserves_lamports: Some(GENESIS_VIRTUAL_SOL_LAMPORTS),
            real_token_reserves: Some(GENESIS_VIRTUAL_TOKEN_AMOUNT),
            fee_bps: GENESIS_FEE_BPS,
            last_update_slot: Some(slot),
            last_update_ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };

        if let Entry::Vacant(entry) = self.cache.entry(*bonding_curve) {
            entry.insert(CurveState {
                snapshot: Some(genesis_snapshot),
                swaps: parking_lot::Mutex::new(EarlySwapRingBuffer::new()),
            });
            tracing::debug!(
                "💉 GENESIS INJECTED for {}: {} lamports / {} tokens",
                bonding_curve,
                GENESIS_VIRTUAL_SOL_LAMPORTS,
                GENESIS_VIRTUAL_TOKEN_AMOUNT
            );
        }
    }

    /// Get curve snapshot
    ///
    /// Returns None if curve not in cache or snapshot not yet received.
    pub fn get_snapshot(&self, bonding_curve: &Pubkey) -> Option<CurveSnapshot> {
        if let Some(entry) = self.cache.get(bonding_curve) {
            self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
            entry.snapshot.clone()
        } else {
            self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Get valid early swap events (within TTL) - heapless
    ///
    /// Returns empty container if curve not in cache or no valid swaps.
    pub fn get_early_swaps(&self, bonding_curve: &Pubkey) -> EarlySwapEvents {
        if let Some(entry) = self.cache.get(bonding_curve) {
            self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
            entry.swaps.lock().get_valid_events()
        } else {
            self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
            EarlySwapEvents::new()
        }
    }

    /// Get both snapshot and swaps atomically
    ///
    /// Returns None if curve not in cache OR snapshot not yet received.
    /// More efficient than separate calls for both.
    pub fn get_state(&self, bonding_curve: &Pubkey) -> Option<(CurveSnapshot, EarlySwapEvents)> {
        if let Some(entry) = self.cache.get(bonding_curve) {
            self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
            if let Some(snapshot) = entry.snapshot.clone() {
                let swaps = entry.swaps.lock().get_valid_events();
                Some((snapshot, swaps))
            } else {
                // Curve exists but snapshot not yet received
                None
            }
        } else {
            self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Get cache metrics
    pub fn metrics(&self) -> &CacheMetrics {
        &self.metrics
    }

    /// Get number of cached curves
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clear all cached data
    pub fn clear(&self) {
        self.cache.clear();
    }
}

impl Default for PumpCurveStateCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_curve_snapshot_creation() {
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));

        assert_eq!(snapshot.virtual_sol_reserves_lamports, 1_000_000_000);
        assert_eq!(snapshot.virtual_token_reserves, 1_000_000_000_000);
        assert_eq!(snapshot.fee_bps, 100);
        assert_eq!(snapshot.last_update_slot, Some(12345));
        assert!(snapshot.has_valid_reserves());
    }

    #[test]
    fn test_curve_snapshot_with_real_reserves() {
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345))
            .with_real_reserves(2_000_000_000, 2_000_000_000_000);

        assert_eq!(snapshot.real_sol_reserves_lamports, Some(2_000_000_000));
        assert_eq!(snapshot.real_token_reserves, Some(2_000_000_000_000));
    }

    #[test]
    fn test_early_swap_event_creation() {
        let event = EarlySwapEvent::new(500_000_000, true, 12345);

        assert_eq!(event.amount_in, 500_000_000);
        assert!(event.is_buy);
        assert_eq!(event.slot, 12345);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(event.is_valid(now));
    }

    #[test]
    fn test_early_swap_event_ttl() {
        let mut event = EarlySwapEvent::new(500_000_000, true, 12345);

        // Manually set old timestamp (3 seconds ago)
        event.timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 3000;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(!event.is_valid(now));
    }

    #[test]
    fn test_ring_buffer_push() {
        let mut buffer = EarlySwapRingBuffer::new();

        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());

        buffer.push(EarlySwapEvent::new(100, true, 1));
        buffer.push(EarlySwapEvent::new(200, false, 2));

        assert_eq!(buffer.len(), 2);
        assert!(!buffer.is_empty());

        let events = buffer.get_valid_events();
        assert_eq!(events.len, 2);
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let mut buffer = EarlySwapRingBuffer::new();

        // Fill buffer beyond capacity
        for i in 0..(EARLY_SWAP_BUFFER_SIZE + 5) {
            buffer.push(EarlySwapEvent::new(i as u64, true, i as u64));
        }

        assert_eq!(buffer.len(), EARLY_SWAP_BUFFER_SIZE);

        let events = buffer.get_valid_events();
        assert!(events.len as usize <= EARLY_SWAP_BUFFER_SIZE);
    }

    #[test]
    fn test_ring_buffer_ttl_filtering() {
        let mut buffer = EarlySwapRingBuffer::new();

        // Add events with old timestamps
        for i in 0..5 {
            let mut event = EarlySwapEvent::new(i, true, i);
            event.timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
                - 3000; // 3 seconds ago
            buffer.push(event);
        }

        // Add fresh events
        for i in 5..10 {
            buffer.push(EarlySwapEvent::new(i, true, i));
        }

        let valid_events = buffer.get_valid_events();
        // Only fresh events should be valid
        assert_eq!(valid_events.len, 5);
    }

    #[test]
    fn test_cache_update_snapshot() {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));

        let is_new = cache.update_snapshot(curve, snapshot.clone());
        assert!(is_new);

        let retrieved = cache.get_snapshot(&curve).unwrap();
        assert_eq!(retrieved.virtual_sol_reserves_lamports, 1_000_000_000);
        assert_eq!(retrieved.fee_bps, 100);
    }

    #[test]
    fn test_cache_update_swap() {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        let event = EarlySwapEvent::new(500_000_000, true, 12345);
        cache.update_swap(curve, event);

        let swaps = cache.get_early_swaps(&curve);
        assert_eq!(swaps.len, 1);
        let first_swap = swaps.iter().next().unwrap();
        assert_eq!(first_swap.amount_in, 500_000_000);
        assert!(first_swap.is_buy);
    }

    #[test]
    fn test_cache_get_state() {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
        cache.update_snapshot(curve, snapshot);

        let event = EarlySwapEvent::new(500_000_000, true, 12345);
        cache.update_swap(curve, event);

        let (snap, swaps) = cache.get_state(&curve).unwrap();
        assert_eq!(snap.virtual_sol_reserves_lamports, 1_000_000_000);
        assert_eq!(swaps.len, 1);
    }

    #[test]
    fn test_cache_metrics() {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
        cache.update_snapshot(curve, snapshot);

        cache.update_swap(curve, EarlySwapEvent::new(100, true, 1));
        cache.update_swap(curve, EarlySwapEvent::new(200, false, 2));

        let metrics = cache.metrics();
        assert_eq!(metrics.snapshot_updates.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.swap_events.load(Ordering::Relaxed), 2);

        // Trigger cache hit
        cache.get_snapshot(&curve);
        assert_eq!(metrics.cache_hits.load(Ordering::Relaxed), 1);

        // Trigger cache miss
        let other_curve = Pubkey::new_unique();
        cache.get_snapshot(&other_curve);
        assert_eq!(metrics.cache_misses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_cache_metrics_hit_rate() {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
        cache.update_snapshot(curve, snapshot);

        // 3 hits
        cache.get_snapshot(&curve);
        cache.get_snapshot(&curve);
        cache.get_snapshot(&curve);

        // 1 miss
        let other_curve = Pubkey::new_unique();
        cache.get_snapshot(&other_curve);

        let hit_rate = cache.metrics().hit_rate();
        assert!((hit_rate - 75.0).abs() < 0.1); // 3/4 = 75%
    }

    #[test]
    fn test_cache_clear() {
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
        cache.update_snapshot(curve, snapshot);

        assert_eq!(cache.len(), 1);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_swap_before_snapshot() {
        // Critical test: swaps can arrive before snapshot
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        // Add swap events BEFORE snapshot
        cache.update_swap(curve, EarlySwapEvent::new(100_000_000, true, 1));
        cache.update_swap(curve, EarlySwapEvent::new(200_000_000, false, 2));

        // Snapshot should NOT exist yet
        assert!(
            cache.get_snapshot(&curve).is_none(),
            "Snapshot should be None before update_snapshot"
        );

        // But swaps should be available
        let swaps = cache.get_early_swaps(&curve);
        assert_eq!(
            swaps.len, 2,
            "Swaps should be available even without snapshot"
        );

        // get_state should return None (no snapshot yet)
        assert!(
            cache.get_state(&curve).is_none(),
            "get_state should return None without snapshot"
        );

        // Now add snapshot
        let snapshot = CurveSnapshot::new(1_000_000_000, 1_000_000_000_000, 100, Some(12345));
        cache.update_snapshot(curve, snapshot);

        // Now snapshot should exist
        let retrieved = cache
            .get_snapshot(&curve)
            .expect("Snapshot should exist after update");
        assert_eq!(retrieved.fee_bps, 100);

        // And get_state should work
        let (snap, swaps) = cache
            .get_state(&curve)
            .expect("get_state should work with snapshot");
        assert_eq!(snap.virtual_sol_reserves_lamports, 1_000_000_000);
        assert_eq!(swaps.len, 2);
    }

    #[test]
    fn test_no_magic_fee_without_snapshot() {
        // Ensure NO magic number 100 appears without actual snapshot data
        let cache = PumpCurveStateCache::new();
        let curve = Pubkey::new_unique();

        // Add swap only (no snapshot)
        cache.update_swap(curve, EarlySwapEvent::new(100_000_000, true, 1));

        // get_snapshot MUST return None (not a dummy with fee=100)
        assert!(
            cache.get_snapshot(&curve).is_none(),
            "No dummy snapshot should be created"
        );
    }
}
