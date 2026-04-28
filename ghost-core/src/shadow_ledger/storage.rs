//! Shadow Ledger Storage Layer - Thread-Safe Persistence for Curves and Snapshots
//!
//! This module provides the core storage abstraction for the Shadow Ledger,
//! handling all DashMap operations for both bonding curves and snapshot buffers.
//!
//! ## Module Structure
//!
//! - **SnapshotStorage trait**: Interface for snapshot buffer operations
//! - **CurveStorage trait**: Interface for bonding curve persistence
//! - **DashMapSnapshotStorage**: Concrete snapshot storage implementation
//! - **DashMapCurveStorage**: Concrete curve storage implementation  
//! - **ShardedCurveStorage**: Horizontally-scaled curve storage with Pubkey prefix sharding
//!
//! ## Design Principles
//!
//! - **Encapsulation**: DashMap details hidden behind clean trait/API
//! - **Testability**: Storage operations can be mocked for unit testing
//! - **Separation of Concerns**: Storage logic separate from simulation/eviction logic
//! - **Scalability**: Sharding support for horizontal scaling under high load
//! - **Lazy Loading Foundation**: DirtyCurveMarker for future disk-backed eviction
//!
//! ## Thread Safety
//!
//! All storage implementations are thread-safe and designed for concurrent access:
//! - Lock-free reads via DashMap's fine-grained sharding
//! - Atomic updates without global locking
//! - Near-linear scalability with thread count
//!
//! ## Usage
//!
//! ```ignore
//! use ghost_core::shadow_ledger::storage::{CurveStorage, DashMapCurveStorage};
//!
//! let storage = DashMapCurveStorage::new();
//! storage.insert_with_slot(mint, curve, slot);
//! let curve = storage.get(&mint);
//! ```

use dashmap::DashMap;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::types::SnapshotBuffer;
use crate::market_state::{BondingCurve, ShadowBondingCurve};

// ============================================================================
// Storage Trait - Interface for Snapshot Storage Operations
// ============================================================================

/// Trait defining the interface for snapshot storage operations.
///
/// This trait abstracts the underlying storage implementation (DashMap),
/// allowing for clean separation between storage and eviction logic.
///
/// # Thread Safety
///
/// All implementations must be thread-safe and support concurrent access.
pub trait SnapshotStorage: Send + Sync {
    /// Get the number of stored snapshot buffers.
    fn len(&self) -> usize;

    /// Check if the storage is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if a snapshot buffer exists for the given mint.
    fn contains(&self, mint: &Pubkey) -> bool;

    /// Get a copy of the snapshot buffer for the given mint.
    fn get(&self, mint: &Pubkey) -> Option<SnapshotBuffer>;

    /// Insert a snapshot buffer for the given mint.
    fn insert(&self, mint: Pubkey, buffer: SnapshotBuffer);

    /// Remove a snapshot buffer for the given mint.
    ///
    /// # Returns
    ///
    /// The removed buffer if it existed.
    fn remove(&self, mint: &Pubkey) -> Option<SnapshotBuffer>;

    /// Clear all snapshot buffers.
    fn clear(&self);

    /// Iterate over all entries and collect keys matching a predicate.
    ///
    /// This method enables eviction logic to identify stale entries without
    /// exposing the underlying DashMap iteration details.
    ///
    /// # Arguments
    ///
    /// * `predicate` - A closure that takes a reference to a SnapshotBuffer and returns
    ///                 true if the entry should be included in the result
    ///
    /// # Returns
    ///
    /// Vector of mint addresses (Pubkeys) for entries matching the predicate.
    fn filter_keys<F>(&self, predicate: F) -> Vec<Pubkey>
    where
        F: Fn(&SnapshotBuffer) -> bool;

    /// Get all mint addresses currently stored.
    fn get_all_mints(&self) -> Vec<Pubkey>;

    /// Iterate over all entries and collect detailed information.
    ///
    /// This method enables diagnostics and statistics gathering without
    /// exposing the underlying DashMap.
    ///
    /// # Arguments
    ///
    /// * `collector` - A closure that takes (mint, buffer) and returns an optional result
    ///
    /// # Returns
    ///
    /// Vector of collected results.
    fn collect<T, F>(&self, collector: F) -> Vec<T>
    where
        T: Send,
        F: Fn(&Pubkey, &SnapshotBuffer) -> Option<T>;
}

// ============================================================================
// Curve Info for Aggressive Eviction
// ============================================================================

/// Information about a curve for aggressive eviction decisions.
///
/// This struct contains curve-specific data used by the aggressive
/// eviction algorithm to make eviction decisions.
#[derive(Clone, Debug, Default)]
pub struct CurveInfo {
    /// Current bonding progress percentage (0-100).
    pub bonding_progress: u64,

    /// Slot when the curve was last updated.
    pub last_updated_slot: u64,

    /// Cumulative volume in SOL (for LRU ranking).
    pub cum_volume_sol: f64,

    /// Transaction count (for LRU ranking).
    pub tx_count: u64,
}

/// Trait for storage that can provide curve information.
///
/// This trait extends SnapshotStorage with the ability to retrieve
/// curve-specific data for aggressive eviction decisions.
pub trait CurveAwareStorage: SnapshotStorage {
    /// Get curve information for a specific mint.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to look up
    ///
    /// # Returns
    ///
    /// CurveInfo if the curve exists, None otherwise.
    fn get_curve_info(&self, mint: &Pubkey) -> Option<CurveInfo>;

    /// Collect curve information for all mints.
    ///
    /// # Arguments
    ///
    /// * `collector` - A closure that takes (mint, curve_info) and returns an optional result
    ///
    /// # Returns
    ///
    /// Vector of collected results.
    fn collect_with_curve_info<T, F>(&self, collector: F) -> Vec<T>
    where
        T: Send,
        F: Fn(&Pubkey, &SnapshotBuffer, Option<&CurveInfo>) -> Option<T>;
}

// ============================================================================
// DashMap Implementation - Concrete Storage Using DashMap
// ============================================================================

/// DashMap-based implementation of SnapshotStorage.
///
/// This struct wraps an `Arc<DashMap>` and provides thread-safe,
/// lock-free snapshot storage operations.
///
/// # Performance
///
/// DashMap provides:
/// - Lock-free reads via fine-grained sharding
/// - Atomic updates without global locking
/// - Near-linear scalability with thread count
#[derive(Clone)]
pub struct DashMapSnapshotStorage {
    inner: Arc<DashMap<Pubkey, SnapshotBuffer>>,
}

impl DashMapSnapshotStorage {
    /// Create a new empty storage.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Create a new storage with pre-allocated capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Number of entries to pre-allocate space for
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(DashMap::with_capacity(capacity)),
        }
    }

    /// Create a storage wrapper around an existing Arc<DashMap>.
    ///
    /// This is useful for integrating with existing code that already
    /// has a DashMap instance.
    pub fn from_arc(inner: Arc<DashMap<Pubkey, SnapshotBuffer>>) -> Self {
        Self { inner }
    }

    /// Get the underlying Arc<DashMap> for direct access.
    ///
    /// # Warning
    ///
    /// This method is provided for backward compatibility and should be
    /// avoided in new code. Prefer using the trait methods instead.
    pub fn inner(&self) -> Arc<DashMap<Pubkey, SnapshotBuffer>> {
        Arc::clone(&self.inner)
    }
}

impl Default for DashMapSnapshotStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotStorage for DashMapSnapshotStorage {
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn contains(&self, mint: &Pubkey) -> bool {
        self.inner.contains_key(mint)
    }

    fn get(&self, mint: &Pubkey) -> Option<SnapshotBuffer> {
        self.inner.get(mint).map(|entry| entry.value().clone())
    }

    fn insert(&self, mint: Pubkey, buffer: SnapshotBuffer) {
        self.inner.insert(mint, buffer);
    }

    fn remove(&self, mint: &Pubkey) -> Option<SnapshotBuffer> {
        self.inner.remove(mint).map(|(_, v)| v)
    }

    fn clear(&self) {
        self.inner.clear();
    }

    fn filter_keys<F>(&self, predicate: F) -> Vec<Pubkey>
    where
        F: Fn(&SnapshotBuffer) -> bool,
    {
        self.inner
            .iter()
            .filter(|entry| predicate(entry.value()))
            .map(|entry| *entry.key())
            .collect()
    }

    fn get_all_mints(&self) -> Vec<Pubkey> {
        self.inner.iter().map(|entry| *entry.key()).collect()
    }

    fn collect<T, F>(&self, collector: F) -> Vec<T>
    where
        T: Send,
        F: Fn(&Pubkey, &SnapshotBuffer) -> Option<T>,
    {
        self.inner
            .iter()
            .filter_map(|entry| collector(entry.key(), entry.value()))
            .collect()
    }
}

// ============================================================================
// Curve Storage Trait - Interface for Bonding Curve Persistence
// ============================================================================

/// Trait defining the interface for bonding curve storage operations.
///
/// This trait abstracts the underlying storage implementation (DashMap),
/// allowing for clean separation between storage and simulation logic.
///
/// # Thread Safety
///
/// All implementations must be thread-safe and support concurrent access.
/// Operations should be atomic where noted.
///
/// # Lifecycle Operations
///
/// This trait provides all the lifecycle management operations for curves:
/// - `insert_with_slot` / `insert` - Add or update a curve
/// - `get` / `get_shadow` - Retrieve curve state
/// - `remove` - Remove a single curve  
/// - `clear` - Remove all curves
/// - `contains` / `len` / `is_empty` - Query storage state
/// - `get_all_mints` - Enumerate all stored mints
pub trait CurveStorage: Send + Sync {
    /// Get the number of stored bonding curves.
    fn len(&self) -> usize;

    /// Check if the storage is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if a bonding curve exists for the given mint.
    fn contains(&self, mint: &Pubkey) -> bool;

    /// Get a copy of the bonding curve for the given mint.
    ///
    /// Returns only the curve state, without slot tracking metadata.
    fn get(&self, mint: &Pubkey) -> Option<BondingCurve>;

    /// Get a copy of the full ShadowBondingCurve for the given mint.
    ///
    /// Includes slot tracking metadata for staleness detection.
    fn get_shadow(&self, mint: &Pubkey) -> Option<ShadowBondingCurve>;

    /// Insert a bonding curve with slot tracking.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address (token Pubkey)
    /// * `curve` - The bonding curve state to store
    /// * `slot` - The slot number when this state was observed
    ///
    /// # Returns
    ///
    /// The previous curve state if one existed.
    fn insert_with_slot(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
    ) -> Option<ShadowBondingCurve>;

    /// Insert a bonding curve without slot tracking (uses slot 0).
    ///
    /// Prefer `insert_with_slot` when slot information is available.
    fn insert(&self, mint: Pubkey, curve: BondingCurve) -> Option<ShadowBondingCurve> {
        self.insert_with_slot(mint, curve, 0)
    }

    /// Remove a bonding curve for the given mint.
    ///
    /// # Returns
    ///
    /// The removed curve state if it existed.
    fn remove(&self, mint: &Pubkey) -> Option<ShadowBondingCurve>;

    /// Clear all bonding curves.
    fn clear(&self);

    /// Get all mint addresses currently stored.
    fn get_all_mints(&self) -> Vec<Pubkey>;

    /// Iterate over all entries and collect results matching a predicate.
    ///
    /// # Arguments
    ///
    /// * `predicate` - A closure that returns true for entries to include
    ///
    /// # Returns
    ///
    /// Vector of mint addresses for entries matching the predicate.
    fn filter_keys<F>(&self, predicate: F) -> Vec<Pubkey>
    where
        F: Fn(&ShadowBondingCurve) -> bool;

    /// Collect curve information for all mints.
    ///
    /// # Arguments
    ///
    /// * `collector` - A closure that transforms (mint, curve) to optional result
    ///
    /// # Returns
    ///
    /// Vector of collected results.
    fn collect<T, F>(&self, collector: F) -> Vec<T>
    where
        T: Send,
        F: Fn(&Pubkey, &ShadowBondingCurve) -> Option<T>;
}

// ============================================================================
// DashMap Curve Storage - Concrete Implementation
// ============================================================================

/// DashMap-based implementation of CurveStorage.
///
/// This struct wraps an `Arc<DashMap>` and provides thread-safe,
/// lock-free bonding curve storage operations.
///
/// # Performance
///
/// DashMap provides:
/// - Lock-free reads via fine-grained sharding
/// - Atomic updates without global locking
/// - Near-linear scalability with thread count
///
/// # Memory Overhead
///
/// - DashMap overhead: ~64 bytes per entry
/// - Pubkey: 32 bytes
/// - ShadowBondingCurve: 64 bytes
/// - **Total per entry: ~160 bytes**
#[derive(Clone)]
pub struct DashMapCurveStorage {
    inner: Arc<DashMap<Pubkey, ShadowBondingCurve>>,
}

impl DashMapCurveStorage {
    /// Create a new empty curve storage.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Create a new storage with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(DashMap::with_capacity(capacity)),
        }
    }

    /// Create a storage wrapper around an existing Arc<DashMap>.
    ///
    /// Useful for integrating with existing code that already has a DashMap.
    pub fn from_arc(inner: Arc<DashMap<Pubkey, ShadowBondingCurve>>) -> Self {
        Self { inner }
    }

    /// Get the underlying Arc<DashMap> for direct access.
    ///
    /// # Warning
    ///
    /// This method is provided for backward compatibility and should be
    /// avoided in new code. Prefer using the trait methods instead.
    pub fn inner(&self) -> Arc<DashMap<Pubkey, ShadowBondingCurve>> {
        Arc::clone(&self.inner)
    }
}

impl Default for DashMapCurveStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl CurveStorage for DashMapCurveStorage {
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn contains(&self, mint: &Pubkey) -> bool {
        self.inner.contains_key(mint)
    }

    fn get(&self, mint: &Pubkey) -> Option<BondingCurve> {
        self.inner.get(mint).map(|entry| entry.value().curve)
    }

    fn get_shadow(&self, mint: &Pubkey) -> Option<ShadowBondingCurve> {
        self.inner.get(mint).map(|entry| *entry.value())
    }

    fn insert_with_slot(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
    ) -> Option<ShadowBondingCurve> {
        let shadow_curve = ShadowBondingCurve::new(curve, slot);
        self.inner.insert(mint, shadow_curve)
    }

    fn remove(&self, mint: &Pubkey) -> Option<ShadowBondingCurve> {
        self.inner.remove(mint).map(|(_, v)| v)
    }

    fn clear(&self) {
        self.inner.clear();
    }

    fn get_all_mints(&self) -> Vec<Pubkey> {
        self.inner.iter().map(|entry| *entry.key()).collect()
    }

    fn filter_keys<F>(&self, predicate: F) -> Vec<Pubkey>
    where
        F: Fn(&ShadowBondingCurve) -> bool,
    {
        self.inner
            .iter()
            .filter(|entry| predicate(entry.value()))
            .map(|entry| *entry.key())
            .collect()
    }

    fn collect<T, F>(&self, collector: F) -> Vec<T>
    where
        T: Send,
        F: Fn(&Pubkey, &ShadowBondingCurve) -> Option<T>,
    {
        self.inner
            .iter()
            .filter_map(|entry| collector(entry.key(), entry.value()))
            .collect()
    }
}

// ============================================================================
// Sharded Curve Storage - Horizontal Scaling via Pubkey Prefix Sharding
// ============================================================================

/// Number of shards for ShardedCurveStorage (16 shards = 4-bit prefix)
pub const DEFAULT_SHARD_COUNT: usize = 16;

/// Sharded curve storage for improved scalability under high load.
///
/// This implementation partitions curves across multiple DashMaps based on
/// the first nibble (4 bits) of the Pubkey, providing:
/// - Reduced contention under concurrent access
/// - Better cache locality per shard
/// - Independent scaling per partition
///
/// # Sharding Strategy
///
/// Curves are distributed to shards based on `pubkey.as_ref()[0] % shard_count`.
/// With 16 shards, this uses the first 4 bits of the Pubkey.
///
/// # Performance
///
/// - Each shard maintains its own lock set
/// - Concurrent operations on different shards don't contend
/// - Ideal for workloads with 10,000+ curves
#[derive(Clone)]
pub struct ShardedCurveStorage {
    shards: Vec<Arc<DashMap<Pubkey, ShadowBondingCurve>>>,
    shard_count: usize,
}

impl ShardedCurveStorage {
    /// Create a new sharded storage with the default shard count (16).
    pub fn new() -> Self {
        Self::with_shard_count(DEFAULT_SHARD_COUNT)
    }

    /// Create a new sharded storage with a custom shard count.
    ///
    /// # Arguments
    ///
    /// * `shard_count` - Number of shards (should be a power of 2 for optimal distribution)
    pub fn with_shard_count(shard_count: usize) -> Self {
        let shards = (0..shard_count).map(|_| Arc::new(DashMap::new())).collect();
        Self {
            shards,
            shard_count,
        }
    }

    /// Create a new sharded storage with pre-allocated capacity per shard.
    pub fn with_capacity(capacity_per_shard: usize) -> Self {
        let shards = (0..DEFAULT_SHARD_COUNT)
            .map(|_| Arc::new(DashMap::with_capacity(capacity_per_shard)))
            .collect();
        Self {
            shards,
            shard_count: DEFAULT_SHARD_COUNT,
        }
    }

    /// Get the shard index for a given pubkey.
    ///
    /// # Sharding Strategy
    ///
    /// Uses the first byte of the Pubkey modulo shard count. This works well when:
    /// - Pubkeys are cryptographically random (typical for Solana addresses)
    /// - The number of shards is a power of 2 (for even distribution)
    ///
    /// For workloads with non-random Pubkey patterns, consider using a hash function.
    #[inline]
    fn shard_index(&self, mint: &Pubkey) -> usize {
        // Simple modulo on first byte provides O(1) shard lookup.
        // This assumes Pubkeys have uniform distribution in their first byte,
        // which holds for cryptographically generated addresses.
        (mint.as_ref()[0] as usize) % self.shard_count
    }

    /// Get the shard for a given pubkey.
    #[inline]
    fn get_shard(&self, mint: &Pubkey) -> &Arc<DashMap<Pubkey, ShadowBondingCurve>> {
        &self.shards[self.shard_index(mint)]
    }

    /// Get the number of shards.
    pub fn shard_count(&self) -> usize {
        self.shard_count
    }

    /// Get the number of entries in each shard (for diagnostics).
    pub fn shard_sizes(&self) -> Vec<usize> {
        self.shards.iter().map(|s| s.len()).collect()
    }
}

impl Default for ShardedCurveStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl CurveStorage for ShardedCurveStorage {
    fn len(&self) -> usize {
        self.shards.iter().map(|s| s.len()).sum()
    }

    fn contains(&self, mint: &Pubkey) -> bool {
        self.get_shard(mint).contains_key(mint)
    }

    fn get(&self, mint: &Pubkey) -> Option<BondingCurve> {
        self.get_shard(mint).get(mint).map(|e| e.value().curve)
    }

    fn get_shadow(&self, mint: &Pubkey) -> Option<ShadowBondingCurve> {
        self.get_shard(mint).get(mint).map(|e| *e.value())
    }

    fn insert_with_slot(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
    ) -> Option<ShadowBondingCurve> {
        let shadow_curve = ShadowBondingCurve::new(curve, slot);
        self.get_shard(&mint).insert(mint, shadow_curve)
    }

    fn remove(&self, mint: &Pubkey) -> Option<ShadowBondingCurve> {
        self.get_shard(mint).remove(mint).map(|(_, v)| v)
    }

    fn clear(&self) {
        for shard in &self.shards {
            shard.clear();
        }
    }

    fn get_all_mints(&self) -> Vec<Pubkey> {
        self.shards
            .iter()
            .flat_map(|s| s.iter().map(|e| *e.key()))
            .collect()
    }

    fn filter_keys<F>(&self, predicate: F) -> Vec<Pubkey>
    where
        F: Fn(&ShadowBondingCurve) -> bool,
    {
        self.shards
            .iter()
            .flat_map(|s| {
                s.iter()
                    .filter(|e| predicate(e.value()))
                    .map(|e| *e.key())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn collect<T, F>(&self, collector: F) -> Vec<T>
    where
        T: Send,
        F: Fn(&Pubkey, &ShadowBondingCurve) -> Option<T>,
    {
        self.shards
            .iter()
            .flat_map(|s| {
                s.iter()
                    .filter_map(|e| collector(e.key(), e.value()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

// ============================================================================
// Dirty Curve Marker - Foundation for Lazy Loading / Disk Eviction
// ============================================================================

/// Estimated serialized size for a ShadowBondingCurve when written to disk.
///
/// This accounts for typical JSON serialization overhead. Actual size may vary
/// depending on the serialization format used (JSON, LMDB, bincode, etc.).
/// The value is an estimate based on:
/// - BondingCurve fields (8 u64 values, 1 u8) → ~100-200 bytes JSON
/// - ShadowBondingCurve slot tracking (1 u64) → ~20-30 bytes JSON  
/// - Total with formatting: ~256 bytes
pub const ESTIMATED_SERIALIZED_CURVE_SIZE: usize = 256;

/// Marker for curves that have been evicted to disk.
///
/// This struct provides the foundation for lazy loading / dirty curve eviction.
/// When memory pressure is high, curves can be serialized to disk and replaced
/// with a DirtyCurveMarker. The curve can be reloaded on-demand when accessed.
///
/// # Future Implementation
///
/// The full lazy loading implementation would:
/// 1. Serialize stale/unused curves to JSON/LMDB on disk
/// 2. Replace the curve entry with a DirtyCurveMarker
/// 3. On access, reload the curve from disk transparently
/// 4. Track access patterns for LRU-based decisions
///
/// # Current Status
///
/// This is a foundation struct. The actual disk I/O and transparent reload
/// functionality would be implemented in a future iteration.
#[derive(Clone, Debug)]
pub struct DirtyCurveMarker {
    /// Path or key where the curve is stored on disk.
    pub storage_path: String,

    /// Timestamp when the curve was evicted to disk (ms since UNIX epoch).
    pub evicted_at_ms: u64,

    /// Last known slot when the curve was active.
    pub last_known_slot: u64,

    /// Estimated size of the serialized curve in bytes.
    /// This is an estimate; actual disk usage may vary by format.
    pub serialized_size: usize,
}

/// Get the current time in milliseconds, with fallback behavior.
///
/// Returns current time as ms since UNIX epoch. If system clock is misconfigured
/// (before 1970), returns 1 to ensure staleness calculations behave predictably.
#[inline]
fn current_time_ms_safe() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1) // Return 1 instead of 0 to avoid edge cases with age=0
}

impl DirtyCurveMarker {
    /// Create a new dirty curve marker.
    ///
    /// # Arguments
    ///
    /// * `storage_path` - Path or key where the curve will be stored
    /// * `last_known_slot` - Slot number when the curve was last active
    pub fn new(storage_path: String, last_known_slot: u64) -> Self {
        Self {
            storage_path,
            evicted_at_ms: current_time_ms_safe(),
            last_known_slot,
            serialized_size: ESTIMATED_SERIALIZED_CURVE_SIZE,
        }
    }

    /// Create a marker with a custom serialized size estimate.
    ///
    /// Use this when you know the actual serialization format and can provide
    /// a more accurate size estimate.
    pub fn with_size(storage_path: String, last_known_slot: u64, serialized_size: usize) -> Self {
        Self {
            storage_path,
            evicted_at_ms: current_time_ms_safe(),
            last_known_slot,
            serialized_size,
        }
    }

    /// Check if this marker is stale and should be cleaned up entirely.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Maximum age before the marker is considered stale
    pub fn is_stale(&self, max_age_ms: u64) -> bool {
        current_time_ms_safe().saturating_sub(self.evicted_at_ms) > max_age_ms
    }
}

/// Registry for tracking dirty (disk-evicted) curves.
///
/// This struct maintains a map of mint addresses to their DirtyCurveMarker,
/// enabling efficient lookup when a curve needs to be reloaded from disk.
#[derive(Clone)]
pub struct DirtyCurveRegistry {
    markers: Arc<DashMap<Pubkey, DirtyCurveMarker>>,
    eviction_count: Arc<AtomicU64>,
    reload_count: Arc<AtomicU64>,
}

impl DirtyCurveRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            markers: Arc::new(DashMap::new()),
            eviction_count: Arc::new(AtomicU64::new(0)),
            reload_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Check if a curve has been evicted to disk.
    pub fn is_dirty(&self, mint: &Pubkey) -> bool {
        self.markers.contains_key(mint)
    }

    /// Get the marker for a dirty curve.
    pub fn get_marker(&self, mint: &Pubkey) -> Option<DirtyCurveMarker> {
        self.markers.get(mint).map(|e| e.value().clone())
    }

    /// Register a curve as dirty (evicted to disk).
    pub fn mark_dirty(&self, mint: Pubkey, marker: DirtyCurveMarker) {
        self.markers.insert(mint, marker);
        self.eviction_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove a dirty marker after the curve has been reloaded.
    pub fn mark_clean(&self, mint: &Pubkey) -> Option<DirtyCurveMarker> {
        let result = self.markers.remove(mint).map(|(_, v)| v);
        if result.is_some() {
            self.reload_count.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    /// Get the number of dirty curves.
    pub fn dirty_count(&self) -> usize {
        self.markers.len()
    }

    /// Get eviction statistics.
    pub fn stats(&self) -> (u64, u64) {
        (
            self.eviction_count.load(Ordering::Relaxed),
            self.reload_count.load(Ordering::Relaxed),
        )
    }

    /// Clear all dirty markers (e.g., on shutdown or full reset).
    pub fn clear(&self) {
        self.markers.clear();
    }
}

impl Default for DirtyCurveRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shadow_ledger::types::MarketSnapshot;

    fn create_test_buffer(created_at_ms: u64) -> SnapshotBuffer {
        SnapshotBuffer::with_timestamp(vec![MarketSnapshot::new(1000)], created_at_ms)
    }

    fn create_test_curve() -> BondingCurve {
        BondingCurve {
            discriminator: 0x1234567890abcdef,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 24_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        }
    }

    #[test]
    fn test_storage_new_is_empty() {
        let storage = DashMapSnapshotStorage::new();
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_storage_with_capacity() {
        let storage = DashMapSnapshotStorage::with_capacity(100);
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_storage_insert_and_get() {
        let storage = DashMapSnapshotStorage::new();
        let mint = Pubkey::new_unique();
        let buffer = create_test_buffer(1000);

        storage.insert(mint, buffer.clone());

        assert!(storage.contains(&mint));
        assert_eq!(storage.len(), 1);

        let retrieved = storage.get(&mint);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().created_at_ms, 1000);
    }

    #[test]
    fn test_storage_remove() {
        let storage = DashMapSnapshotStorage::new();
        let mint = Pubkey::new_unique();
        let buffer = create_test_buffer(1000);

        storage.insert(mint, buffer);
        assert!(storage.contains(&mint));

        let removed = storage.remove(&mint);
        assert!(removed.is_some());
        assert!(!storage.contains(&mint));
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_storage_clear() {
        let storage = DashMapSnapshotStorage::new();

        for _ in 0..10 {
            storage.insert(Pubkey::new_unique(), create_test_buffer(1000));
        }

        assert_eq!(storage.len(), 10);

        storage.clear();

        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_storage_filter_keys() {
        let storage = DashMapSnapshotStorage::new();

        // Insert buffers with different ages
        let old_mint = Pubkey::new_unique();
        let new_mint = Pubkey::new_unique();

        storage.insert(old_mint, create_test_buffer(1000));
        storage.insert(new_mint, create_test_buffer(2000));

        // Filter for buffers with created_at_ms > 1500
        let filtered = storage.filter_keys(|buffer| buffer.created_at_ms > 1500);

        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains(&new_mint));
        assert!(!filtered.contains(&old_mint));
    }

    #[test]
    fn test_storage_get_all_mints() {
        let storage = DashMapSnapshotStorage::new();

        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();
        let mint3 = Pubkey::new_unique();

        storage.insert(mint1, create_test_buffer(1000));
        storage.insert(mint2, create_test_buffer(2000));
        storage.insert(mint3, create_test_buffer(3000));

        let all_mints = storage.get_all_mints();

        assert_eq!(all_mints.len(), 3);
        assert!(all_mints.contains(&mint1));
        assert!(all_mints.contains(&mint2));
        assert!(all_mints.contains(&mint3));
    }

    #[test]
    fn test_storage_collect() {
        let storage = DashMapSnapshotStorage::new();

        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();

        storage.insert(mint1, create_test_buffer(1000));
        storage.insert(mint2, create_test_buffer(2000));

        // Collect ages as u64
        let ages: Vec<u64> = storage.collect(|_, buffer| Some(buffer.created_at_ms));

        assert_eq!(ages.len(), 2);
        assert!(ages.contains(&1000));
        assert!(ages.contains(&2000));
    }

    #[test]
    fn test_storage_clone_shares_data() {
        let storage1 = DashMapSnapshotStorage::new();
        let mint = Pubkey::new_unique();

        storage1.insert(mint, create_test_buffer(1000));

        let storage2 = storage1.clone();

        // Both should see the same data
        assert!(storage2.contains(&mint));

        // Inserting in one should be visible in the other
        let mint2 = Pubkey::new_unique();
        storage2.insert(mint2, create_test_buffer(2000));
        assert!(storage1.contains(&mint2));
    }

    #[test]
    fn test_storage_from_arc() {
        let map: Arc<DashMap<Pubkey, SnapshotBuffer>> = Arc::new(DashMap::new());
        let mint = Pubkey::new_unique();
        map.insert(mint, create_test_buffer(1000));

        let storage = DashMapSnapshotStorage::from_arc(map);

        assert!(storage.contains(&mint));
        assert_eq!(storage.len(), 1);
    }

    // =========================================================================
    // CurveStorage Tests
    // =========================================================================

    #[test]
    fn test_curve_storage_new_is_empty() {
        let storage = DashMapCurveStorage::new();
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_curve_storage_with_capacity() {
        let storage = DashMapCurveStorage::with_capacity(100);
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_curve_storage_insert_and_get() {
        let storage = DashMapCurveStorage::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve();

        // Insert should return None for new entry
        assert!(storage.insert_with_slot(mint, curve, 1000).is_none());
        assert_eq!(storage.len(), 1);

        // Get should return the inserted curve
        let retrieved = storage.get(&mint);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.virtual_token_reserves, 1_000_000_000_000);
    }

    #[test]
    fn test_curve_storage_get_shadow() {
        let storage = DashMapCurveStorage::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve();

        storage.insert_with_slot(mint, curve, 1000);

        let shadow = storage.get_shadow(&mint);
        assert!(shadow.is_some());
        let shadow = shadow.unwrap();
        assert_eq!(shadow.last_updated_slot, 1000);
        assert_eq!(shadow.curve.virtual_token_reserves, 1_000_000_000_000);
    }

    #[test]
    fn test_curve_storage_insert_replaces() {
        let storage = DashMapCurveStorage::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve();

        // First insert
        assert!(storage.insert_with_slot(mint, curve, 1000).is_none());

        // Second insert should return previous
        let previous = storage.insert_with_slot(mint, curve, 2000);
        assert!(previous.is_some());
        assert_eq!(previous.unwrap().last_updated_slot, 1000);

        // New slot should be stored
        let current = storage.get_shadow(&mint).unwrap();
        assert_eq!(current.last_updated_slot, 2000);
    }

    #[test]
    fn test_curve_storage_remove() {
        let storage = DashMapCurveStorage::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve();

        storage.insert(mint, curve);
        assert!(storage.contains(&mint));

        let removed = storage.remove(&mint);
        assert!(removed.is_some());
        assert!(!storage.contains(&mint));
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_curve_storage_clear() {
        let storage = DashMapCurveStorage::new();

        for _ in 0..10 {
            storage.insert(Pubkey::new_unique(), create_test_curve());
        }

        assert_eq!(storage.len(), 10);

        storage.clear();

        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_curve_storage_get_all_mints() {
        let storage = DashMapCurveStorage::new();

        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();
        let mint3 = Pubkey::new_unique();

        storage.insert(mint1, create_test_curve());
        storage.insert(mint2, create_test_curve());
        storage.insert(mint3, create_test_curve());

        let all_mints = storage.get_all_mints();

        assert_eq!(all_mints.len(), 3);
        assert!(all_mints.contains(&mint1));
        assert!(all_mints.contains(&mint2));
        assert!(all_mints.contains(&mint3));
    }

    #[test]
    fn test_curve_storage_filter_keys() {
        let storage = DashMapCurveStorage::new();

        let old_mint = Pubkey::new_unique();
        let new_mint = Pubkey::new_unique();

        storage.insert_with_slot(old_mint, create_test_curve(), 1000);
        storage.insert_with_slot(new_mint, create_test_curve(), 2000);

        // Filter for curves with slot > 1500
        let filtered = storage.filter_keys(|curve| curve.last_updated_slot > 1500);

        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains(&new_mint));
        assert!(!filtered.contains(&old_mint));
    }

    #[test]
    fn test_curve_storage_collect() {
        let storage = DashMapCurveStorage::new();

        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();

        storage.insert_with_slot(mint1, create_test_curve(), 1000);
        storage.insert_with_slot(mint2, create_test_curve(), 2000);

        // Collect slots
        let slots: Vec<u64> = storage.collect(|_, curve| Some(curve.last_updated_slot));

        assert_eq!(slots.len(), 2);
        assert!(slots.contains(&1000));
        assert!(slots.contains(&2000));
    }

    #[test]
    fn test_curve_storage_clone_shares_data() {
        let storage1 = DashMapCurveStorage::new();
        let mint = Pubkey::new_unique();

        storage1.insert(mint, create_test_curve());

        let storage2 = storage1.clone();

        // Both should see the same data
        assert!(storage2.contains(&mint));

        // Inserting in one should be visible in the other
        let mint2 = Pubkey::new_unique();
        storage2.insert(mint2, create_test_curve());
        assert!(storage1.contains(&mint2));
    }

    // =========================================================================
    // ShardedCurveStorage Tests
    // =========================================================================

    #[test]
    fn test_sharded_storage_new_is_empty() {
        let storage = ShardedCurveStorage::new();
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
        assert_eq!(storage.shard_count(), DEFAULT_SHARD_COUNT);
    }

    #[test]
    fn test_sharded_storage_with_shard_count() {
        let storage = ShardedCurveStorage::with_shard_count(8);
        assert_eq!(storage.shard_count(), 8);
        assert!(storage.is_empty());
    }

    #[test]
    fn test_sharded_storage_insert_and_get() {
        let storage = ShardedCurveStorage::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve();

        assert!(storage.insert_with_slot(mint, curve, 1000).is_none());
        assert_eq!(storage.len(), 1);

        let retrieved = storage.get(&mint);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().virtual_token_reserves, 1_000_000_000_000);
    }

    #[test]
    fn test_sharded_storage_distribution() {
        let storage = ShardedCurveStorage::with_shard_count(16);

        // Insert entries with different prefixes to test distribution
        // We explicitly create pubkeys with different first bytes
        for i in 0..160u8 {
            let mut bytes = [0u8; 32];
            bytes[0] = i % 16; // Ensure we hit all shards
            bytes[1] = i;
            let mint = Pubkey::new_from_array(bytes);
            storage.insert(mint, create_test_curve());
        }

        assert_eq!(storage.len(), 160);

        // Check that entries are distributed across shards
        let shard_sizes = storage.shard_sizes();
        assert_eq!(shard_sizes.len(), 16);

        // All entries should be accounted for
        let total: usize = shard_sizes.iter().sum();
        assert_eq!(total, 160);

        // With our explicit distribution, all shards should have entries
        let non_empty_shards = shard_sizes.iter().filter(|&&s| s > 0).count();
        assert_eq!(
            non_empty_shards, 16,
            "Expected all 16 shards to have entries, got {}",
            non_empty_shards
        );
    }

    #[test]
    fn test_sharded_storage_remove() {
        let storage = ShardedCurveStorage::new();
        let mint = Pubkey::new_unique();

        storage.insert(mint, create_test_curve());
        assert!(storage.contains(&mint));

        let removed = storage.remove(&mint);
        assert!(removed.is_some());
        assert!(!storage.contains(&mint));
    }

    #[test]
    fn test_sharded_storage_clear() {
        let storage = ShardedCurveStorage::new();

        for _ in 0..100 {
            storage.insert(Pubkey::new_unique(), create_test_curve());
        }

        assert_eq!(storage.len(), 100);

        storage.clear();

        assert_eq!(storage.len(), 0);

        // Verify all shards are empty
        for size in storage.shard_sizes() {
            assert_eq!(size, 0);
        }
    }

    #[test]
    fn test_sharded_storage_get_all_mints() {
        let storage = ShardedCurveStorage::new();

        let mints: Vec<Pubkey> = (0..50).map(|_| Pubkey::new_unique()).collect();
        for mint in &mints {
            storage.insert(*mint, create_test_curve());
        }

        let all_mints = storage.get_all_mints();
        assert_eq!(all_mints.len(), 50);

        for mint in &mints {
            assert!(all_mints.contains(mint));
        }
    }

    #[test]
    fn test_sharded_storage_filter_keys() {
        let storage = ShardedCurveStorage::new();

        let old_mints: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();
        let new_mints: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();

        for mint in &old_mints {
            storage.insert_with_slot(*mint, create_test_curve(), 1000);
        }
        for mint in &new_mints {
            storage.insert_with_slot(*mint, create_test_curve(), 2000);
        }

        let filtered = storage.filter_keys(|curve| curve.last_updated_slot > 1500);

        assert_eq!(filtered.len(), 5);
        for mint in &new_mints {
            assert!(filtered.contains(mint));
        }
        for mint in &old_mints {
            assert!(!filtered.contains(mint));
        }
    }

    // =========================================================================
    // DirtyCurveMarker and DirtyCurveRegistry Tests
    // =========================================================================

    #[test]
    fn test_dirty_curve_marker_new() {
        let marker = DirtyCurveMarker::new("/tmp/curve_123.json".to_string(), 1000);

        assert_eq!(marker.storage_path, "/tmp/curve_123.json");
        assert_eq!(marker.last_known_slot, 1000);
        assert!(marker.evicted_at_ms > 0);
        assert_eq!(marker.serialized_size, ESTIMATED_SERIALIZED_CURVE_SIZE);
    }

    #[test]
    fn test_dirty_curve_marker_with_size() {
        let marker = DirtyCurveMarker::with_size("/tmp/curve_123.json".to_string(), 1000, 512);

        assert_eq!(marker.storage_path, "/tmp/curve_123.json");
        assert_eq!(marker.last_known_slot, 1000);
        assert!(marker.evicted_at_ms > 0);
        assert_eq!(marker.serialized_size, 512);
    }

    #[test]
    fn test_dirty_curve_marker_staleness() {
        // Create a marker with old timestamp
        let mut marker = DirtyCurveMarker::new("/tmp/test.json".to_string(), 1000);
        marker.evicted_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 10_000; // 10 seconds ago

        // Should be stale at 5 second threshold
        assert!(marker.is_stale(5_000));

        // Should not be stale at 15 second threshold
        assert!(!marker.is_stale(15_000));
    }

    #[test]
    fn test_dirty_curve_registry_new() {
        let registry = DirtyCurveRegistry::new();
        assert_eq!(registry.dirty_count(), 0);
        assert_eq!(registry.stats(), (0, 0));
    }

    #[test]
    fn test_dirty_curve_registry_mark_dirty() {
        let registry = DirtyCurveRegistry::new();
        let mint = Pubkey::new_unique();
        let marker = DirtyCurveMarker::new("/tmp/test.json".to_string(), 1000);

        registry.mark_dirty(mint, marker.clone());

        assert!(registry.is_dirty(&mint));
        assert_eq!(registry.dirty_count(), 1);

        let retrieved = registry.get_marker(&mint);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().last_known_slot, 1000);

        let (evicted, _) = registry.stats();
        assert_eq!(evicted, 1);
    }

    #[test]
    fn test_dirty_curve_registry_mark_clean() {
        let registry = DirtyCurveRegistry::new();
        let mint = Pubkey::new_unique();
        let marker = DirtyCurveMarker::new("/tmp/test.json".to_string(), 1000);

        registry.mark_dirty(mint, marker);
        assert!(registry.is_dirty(&mint));

        let removed = registry.mark_clean(&mint);
        assert!(removed.is_some());
        assert!(!registry.is_dirty(&mint));
        assert_eq!(registry.dirty_count(), 0);

        let (evicted, reloaded) = registry.stats();
        assert_eq!(evicted, 1);
        assert_eq!(reloaded, 1);
    }

    #[test]
    fn test_dirty_curve_registry_clear() {
        let registry = DirtyCurveRegistry::new();

        for i in 0..10 {
            let mint = Pubkey::new_unique();
            let marker = DirtyCurveMarker::new(format!("/tmp/curve_{}.json", i), i as u64);
            registry.mark_dirty(mint, marker);
        }

        assert_eq!(registry.dirty_count(), 10);

        registry.clear();

        assert_eq!(registry.dirty_count(), 0);
    }

    // =========================================================================
    // Concurrency and Atomicity Tests
    // =========================================================================

    #[test]
    fn test_curve_storage_concurrent_inserts() {
        use std::thread;

        let storage = Arc::new(DashMapCurveStorage::new());
        let mut handles = vec![];

        // Spawn 10 threads, each inserting 100 curves
        for _ in 0..10 {
            let storage = Arc::clone(&storage);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    storage.insert(Pubkey::new_unique(), create_test_curve());
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // All 1000 entries should be present
        assert_eq!(storage.len(), 1000);
    }

    #[test]
    fn test_sharded_storage_concurrent_inserts() {
        use std::thread;

        let storage = Arc::new(ShardedCurveStorage::new());
        let mut handles = vec![];

        // Spawn 10 threads, each inserting 100 curves
        for _ in 0..10 {
            let storage = Arc::clone(&storage);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    storage.insert(Pubkey::new_unique(), create_test_curve());
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // All 1000 entries should be present
        assert_eq!(storage.len(), 1000);
    }

    #[test]
    fn test_curve_storage_concurrent_read_write() {
        use std::thread;

        let storage = Arc::new(DashMapCurveStorage::new());

        // Pre-populate with some entries
        let mints: Vec<Pubkey> = (0..100).map(|_| Pubkey::new_unique()).collect();
        for mint in &mints {
            storage.insert(*mint, create_test_curve());
        }

        let mut handles = vec![];

        // Readers
        for _ in 0..5 {
            let storage = Arc::clone(&storage);
            let mints = mints.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    for mint in &mints {
                        let _ = storage.get(mint);
                    }
                }
            }));
        }

        // Writers
        for _ in 0..5 {
            let storage = Arc::clone(&storage);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    storage.insert(Pubkey::new_unique(), create_test_curve());
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have 100 + 500 = 600 entries
        assert_eq!(storage.len(), 600);
    }

    #[test]
    fn test_curve_storage_concurrent_insert_remove() {
        use std::thread;

        let storage = Arc::new(DashMapCurveStorage::new());

        // Pre-populate
        let mints: Vec<Pubkey> = (0..100)
            .map(|_| {
                let mint = Pubkey::new_unique();
                storage.insert(mint, create_test_curve());
                mint
            })
            .collect();

        let mut handles = vec![];

        // Removers (remove half)
        {
            let storage = Arc::clone(&storage);
            let remove_mints = mints[..50].to_vec();
            handles.push(thread::spawn(move || {
                for mint in remove_mints {
                    storage.remove(&mint);
                }
            }));
        }

        // Inserters (insert 50 new)
        {
            let storage = Arc::clone(&storage);
            handles.push(thread::spawn(move || {
                for _ in 0..50 {
                    storage.insert(Pubkey::new_unique(), create_test_curve());
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have 100 - 50 + 50 = 100 entries
        assert_eq!(storage.len(), 100);
    }

    #[test]
    fn test_sharded_storage_consistent_after_operations() {
        let storage = ShardedCurveStorage::new();

        let mints: Vec<Pubkey> = (0..1000).map(|_| Pubkey::new_unique()).collect();

        // Insert all
        for mint in &mints {
            storage.insert(*mint, create_test_curve());
        }
        assert_eq!(storage.len(), 1000);

        // Remove half
        for mint in &mints[..500] {
            storage.remove(mint);
        }
        assert_eq!(storage.len(), 500);

        // Verify remaining are accessible
        for mint in &mints[500..] {
            assert!(storage.contains(mint));
        }
        for mint in &mints[..500] {
            assert!(!storage.contains(mint));
        }

        // Verify get_all_mints consistency
        let all_mints = storage.get_all_mints();
        assert_eq!(all_mints.len(), 500);
    }
}
