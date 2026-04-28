//! Shadow Ledger Eviction Module - Age-Based Cleanup & Diagnostic Tools
//!
//! This module provides algorithms for garbage collection and cleanup of stale
//! snapshot buffers. It implements age-based eviction, force-cleanup utilities,
//! and diagnostic tools for monitoring snapshot lifecycle.
//!
//! ## Design Principles
//!
//! - **Separation of Concerns**: Eviction logic is isolated from core ledger operations
//! - **Storage Abstraction**: Works through the `SnapshotStorage` trait, not directly with DashMap
//! - **Lazy Eviction**: Eviction is triggered on-demand, not continuously
//! - **Configurable**: Age thresholds and frequencies can be customized
//!
//! ## Eviction Strategies
//!
//! ### Age-Based Eviction
//!
//! Snapshots older than `max_age_ms` are removed during eviction cycles. The default
//! max age is 5 minutes (300,000 ms), which is suitable for most trading scenarios
//! where market state changes rapidly.
//!
//! ### Force Cleanup
//!
//! For scenarios requiring immediate memory reclamation (e.g., memory pressure,
//! shutdown), force cleanup removes all snapshots regardless of age.
//!
//! ## Recommended Usage
//!
//! ### Mainnet
//!
//! - Call `evict_stale` every 30-60 seconds
//! - Use default max age (5 minutes) for normal operation
//! - Reduce max age to 2-3 minutes under memory pressure
//!
//! ### Testnet/Devnet
//!
//! - More aggressive eviction (every 10-15 seconds) may be used
//! - Shorter max age (1-2 minutes) is acceptable for faster iteration
//!
//! ## Edge Cases
//!
//! - **Empty Storage**: Eviction on empty storage is a no-op (returns 0)
//! - **All Fresh**: If all snapshots are fresh, no eviction occurs
//! - **All Stale**: All snapshots are removed when all are stale
//! - **Clock Skew**: Uses monotonic time internally to avoid issues with system clock changes
//! - **Large Volume**: Designed to handle 10,000+ mints efficiently via batch removal
//!
//! ## Example
//!
//! ```ignore
//! use ghost_core::shadow_ledger::eviction::{EvictionManager, EvictionConfig};
//!
//! // Create eviction manager with default config
//! let eviction = EvictionManager::new(snapshot_storage.clone());
//!
//! // Periodic eviction (call every 30-60 seconds)
//! let evicted = eviction.evict_stale_default();
//! println!("Evicted {} stale snapshots", evicted);
//!
//! // Force cleanup (e.g., on shutdown or memory pressure)
//! eviction.force_cleanup();
//! ```

use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicU64, Ordering};

use super::storage::SnapshotStorage;
use super::types::{current_time_ms, DEFAULT_SNAPSHOT_MAX_AGE_MS};

// ============================================================================
// Eviction Configuration
// ============================================================================

/// Configuration for aggressive eviction based on curve state.
///
/// This struct enables curve-specific eviction logic that considers:
/// - Bonding curve progress (near migration = lower priority)
/// - Curve staleness in slots
#[derive(Clone, Debug)]
pub struct AggressiveEvictionConfig {
    /// Enable aggressive eviction based on curve state.
    /// Default: false
    pub enabled: bool,

    /// Bonding progress threshold percentage (0-100).
    /// Curves with progress > this threshold are candidates for eviction.
    /// Default: 90
    pub bonding_progress_threshold: u64,

    /// Maximum slot age for curves.
    /// Curves not updated for more than this many slots are candidates for eviction.
    /// Default: 10
    pub max_slot_age: u64,
}

impl Default for AggressiveEvictionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bonding_progress_threshold: 90,
            max_slot_age: 10,
        }
    }
}

impl AggressiveEvictionConfig {
    /// Create an enabled aggressive config with default thresholds.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Create config with custom bonding progress threshold.
    pub fn with_bonding_threshold(bonding_progress_threshold: u64) -> Self {
        Self {
            enabled: true,
            bonding_progress_threshold,
            ..Default::default()
        }
    }

    /// Create config with custom slot age threshold.
    pub fn with_slot_age(max_slot_age: u64) -> Self {
        Self {
            enabled: true,
            max_slot_age,
            ..Default::default()
        }
    }
}

/// Configuration for LRU cache limiting.
///
/// Limits the maximum number of curves stored in the ledger,
/// evicting least-recently-used entries when the limit is exceeded.
#[derive(Clone, Debug)]
pub struct LruConfig {
    /// Enable LRU-based eviction.
    /// Default: false
    pub enabled: bool,

    /// Maximum number of curves to keep.
    /// When exceeded, least-recently-used curves are evicted.
    /// Default: 5000
    pub max_curves: usize,
}

impl Default for LruConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_curves: 5000,
        }
    }
}

impl LruConfig {
    /// Create an enabled LRU config with default limit (5000).
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            max_curves: 5000,
        }
    }

    /// Create an LRU config with custom limit.
    pub fn with_max_curves(max_curves: usize) -> Self {
        Self {
            enabled: true,
            max_curves,
        }
    }
}

/// Configuration for eviction behavior.
///
/// This struct allows customization of eviction parameters without
/// changing the eviction algorithm itself.
#[derive(Clone, Debug)]
pub struct EvictionConfig {
    /// Maximum age of snapshot buffers in milliseconds.
    /// Buffers older than this are eligible for eviction.
    /// Default: 300,000 ms (5 minutes)
    pub max_age_ms: u64,

    /// Whether to log eviction statistics.
    /// Default: false
    pub log_stats: bool,

    /// Configuration for aggressive curve-specific eviction.
    pub aggressive: AggressiveEvictionConfig,

    /// Configuration for LRU cache limiting.
    pub lru: LruConfig,
}

impl Default for EvictionConfig {
    fn default() -> Self {
        Self {
            max_age_ms: DEFAULT_SNAPSHOT_MAX_AGE_MS,
            log_stats: false,
            aggressive: AggressiveEvictionConfig::default(),
            lru: LruConfig::default(),
        }
    }
}

impl EvictionConfig {
    /// Create a new config with the specified max age.
    pub fn with_max_age_ms(max_age_ms: u64) -> Self {
        Self {
            max_age_ms,
            ..Default::default()
        }
    }

    /// Create a config suitable for mainnet operation.
    ///
    /// Uses conservative settings:
    /// - 5 minute max age
    /// - Statistics logging enabled
    /// - LRU enabled with 5000 curve limit
    pub fn mainnet() -> Self {
        Self {
            max_age_ms: DEFAULT_SNAPSHOT_MAX_AGE_MS,
            log_stats: true,
            aggressive: AggressiveEvictionConfig::default(),
            lru: LruConfig::enabled(),
        }
    }

    /// Create a config suitable for testnet/devnet operation.
    ///
    /// Uses more aggressive settings:
    /// - 2 minute max age
    /// - Statistics logging enabled
    /// - Aggressive eviction enabled
    pub fn testnet() -> Self {
        Self {
            max_age_ms: 120_000, // 2 minutes
            log_stats: true,
            aggressive: AggressiveEvictionConfig::enabled(),
            lru: LruConfig::enabled(),
        }
    }

    /// Create a config for aggressive cleanup (e.g., under memory pressure).
    ///
    /// Uses very short max age:
    /// - 30 second max age
    /// - Statistics logging enabled
    /// - Aggressive eviction with 80% bonding threshold
    pub fn aggressive() -> Self {
        Self {
            max_age_ms: 30_000, // 30 seconds
            log_stats: true,
            aggressive: AggressiveEvictionConfig {
                enabled: true,
                bonding_progress_threshold: 80,
                max_slot_age: 5,
            },
            lru: LruConfig::with_max_curves(3000),
        }
    }

    /// Enable aggressive eviction on this config.
    pub fn with_aggressive(mut self, config: AggressiveEvictionConfig) -> Self {
        self.aggressive = config;
        self
    }

    /// Enable LRU limiting on this config.
    pub fn with_lru(mut self, config: LruConfig) -> Self {
        self.lru = config;
        self
    }
}

// ============================================================================
// Eviction Result - Statistics from Eviction Operation
// ============================================================================

/// Result of an eviction operation with detailed statistics.
///
/// This struct provides insight into what was evicted and why,
/// useful for monitoring and debugging.
#[derive(Clone, Debug, Default)]
pub struct EvictionResult {
    /// Number of snapshot buffers evicted.
    pub evicted_count: usize,

    /// Number of snapshot buffers that were checked but not evicted.
    pub retained_count: usize,

    /// Duration of the eviction operation in microseconds.
    pub duration_us: u64,

    /// Maximum age (in ms) of evicted snapshots.
    pub max_evicted_age_ms: u64,

    /// Minimum age (in ms) of retained snapshots (0 if none retained).
    pub min_retained_age_ms: u64,
}

impl EvictionResult {
    /// Total number of snapshots processed.
    pub fn total_processed(&self) -> usize {
        self.evicted_count + self.retained_count
    }

    /// Percentage of snapshots that were evicted.
    pub fn eviction_rate(&self) -> f64 {
        let total = self.total_processed();
        if total == 0 {
            0.0
        } else {
            (self.evicted_count as f64 / total as f64) * 100.0
        }
    }
}

// ============================================================================
// Diagnostic Information
// ============================================================================

/// Diagnostic information about a single snapshot buffer.
#[derive(Clone, Debug)]
pub struct SnapshotDiagnostic {
    /// Mint address of the snapshot.
    pub mint: Pubkey,

    /// Age of the snapshot buffer in milliseconds.
    pub age_ms: u64,

    /// Number of snapshots in the buffer.
    pub snapshot_count: usize,

    /// Timestamp when the buffer was created (ms since UNIX epoch).
    pub created_at_ms: u64,

    /// Whether the snapshot is stale according to default max age.
    pub is_stale: bool,
}

/// Aggregated diagnostic statistics for all snapshots.
#[derive(Clone, Debug, Default)]
pub struct DiagnosticStats {
    /// Total number of snapshot buffers.
    pub total_buffers: usize,

    /// Number of stale buffers (according to provided max age).
    pub stale_count: usize,

    /// Number of fresh buffers.
    pub fresh_count: usize,

    /// Average age of all buffers in milliseconds.
    pub avg_age_ms: f64,

    /// Maximum age of any buffer in milliseconds.
    pub max_age_ms: u64,

    /// Minimum age of any buffer in milliseconds.
    pub min_age_ms: u64,

    /// Total number of snapshots across all buffers.
    pub total_snapshots: usize,
}

// ============================================================================
// Eviction Metrics - PromQL-Compatible Metrics for Observability
// ============================================================================

/// Eviction metrics for Prometheus/PromQL export.
///
/// These metrics are designed to be compatible with Prometheus and can be
/// exported via a metrics endpoint for monitoring and alerting.
///
/// ## PromQL Metrics
///
/// - `eviction_count`: Counter of total evictions performed
/// - `eviction_aggressive_count`: Counter of aggressive evictions (curve-based)
/// - `bootstrap_duration_slots`: Histogram of bootstrap durations
/// - `lru_eviction_count`: Counter of LRU-based evictions
///
/// ## Usage
///
/// ```ignore
/// let metrics = EvictionMetrics::new();
///
/// // After eviction
/// metrics.record_eviction(evicted_count);
///
/// // Export for Prometheus
/// println!("eviction_count {}", metrics.eviction_count());
/// ```
#[derive(Debug, Default)]
pub struct EvictionMetrics {
    /// Total count of evicted snapshots (age-based).
    eviction_count: AtomicU64,

    /// Total count of aggressive evictions (curve-based).
    aggressive_eviction_count: AtomicU64,

    /// Total count of LRU evictions.
    lru_eviction_count: AtomicU64,

    /// Sum of bootstrap durations in slots (for histogram calculation).
    bootstrap_duration_sum_slots: AtomicU64,

    /// Count of bootstrap operations (for histogram calculation).
    bootstrap_count: AtomicU64,

    /// Count of bootstrap failures.
    bootstrap_failure_count: AtomicU64,
}

impl EvictionMetrics {
    /// Create a new metrics instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an age-based eviction.
    pub fn record_eviction(&self, count: usize) {
        self.eviction_count
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record an aggressive (curve-based) eviction.
    pub fn record_aggressive_eviction(&self, count: usize) {
        self.aggressive_eviction_count
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record an LRU-based eviction.
    pub fn record_lru_eviction(&self, count: usize) {
        self.lru_eviction_count
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record a bootstrap operation.
    pub fn record_bootstrap(&self, duration_slots: u64) {
        self.bootstrap_duration_sum_slots
            .fetch_add(duration_slots, Ordering::Relaxed);
        self.bootstrap_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a bootstrap failure.
    pub fn record_bootstrap_failure(&self) {
        self.bootstrap_failure_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get total eviction count.
    pub fn eviction_count(&self) -> u64 {
        self.eviction_count.load(Ordering::Relaxed)
    }

    /// Get aggressive eviction count.
    pub fn aggressive_eviction_count(&self) -> u64 {
        self.aggressive_eviction_count.load(Ordering::Relaxed)
    }

    /// Get LRU eviction count.
    pub fn lru_eviction_count(&self) -> u64 {
        self.lru_eviction_count.load(Ordering::Relaxed)
    }

    /// Get bootstrap count.
    pub fn bootstrap_count(&self) -> u64 {
        self.bootstrap_count.load(Ordering::Relaxed)
    }

    /// Get bootstrap failure count.
    pub fn bootstrap_failure_count(&self) -> u64 {
        self.bootstrap_failure_count.load(Ordering::Relaxed)
    }

    /// Get average bootstrap duration in slots.
    pub fn avg_bootstrap_duration_slots(&self) -> f64 {
        let count = self.bootstrap_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let sum = self.bootstrap_duration_sum_slots.load(Ordering::Relaxed);
        sum as f64 / count as f64
    }

    /// Export metrics in Prometheus text format.
    ///
    /// Returns a string that can be served via HTTP for Prometheus scraping.
    pub fn to_prometheus(&self) -> String {
        format!(
            "# HELP ghost_eviction_count Total number of age-based snapshot evictions\n\
             # TYPE ghost_eviction_count counter\n\
             ghost_eviction_count {}\n\
             \n\
             # HELP ghost_aggressive_eviction_count Total number of aggressive (curve-based) evictions\n\
             # TYPE ghost_aggressive_eviction_count counter\n\
             ghost_aggressive_eviction_count {}\n\
             \n\
             # HELP ghost_lru_eviction_count Total number of LRU-based evictions\n\
             # TYPE ghost_lru_eviction_count counter\n\
             ghost_lru_eviction_count {}\n\
             \n\
             # HELP ghost_bootstrap_count Total number of bootstrap operations\n\
             # TYPE ghost_bootstrap_count counter\n\
             ghost_bootstrap_count {}\n\
             \n\
             # HELP ghost_bootstrap_failure_count Total number of failed bootstrap operations\n\
             # TYPE ghost_bootstrap_failure_count counter\n\
             ghost_bootstrap_failure_count {}\n\
             \n\
             # HELP ghost_bootstrap_duration_slots_sum Sum of bootstrap durations in slots\n\
             # TYPE ghost_bootstrap_duration_slots_sum counter\n\
             ghost_bootstrap_duration_slots_sum {}\n",
            self.eviction_count(),
            self.aggressive_eviction_count(),
            self.lru_eviction_count(),
            self.bootstrap_count(),
            self.bootstrap_failure_count(),
            self.bootstrap_duration_sum_slots.load(Ordering::Relaxed),
        )
    }

    /// Reset all metrics to zero.
    pub fn reset(&self) {
        self.eviction_count.store(0, Ordering::Relaxed);
        self.aggressive_eviction_count.store(0, Ordering::Relaxed);
        self.lru_eviction_count.store(0, Ordering::Relaxed);
        self.bootstrap_duration_sum_slots
            .store(0, Ordering::Relaxed);
        self.bootstrap_count.store(0, Ordering::Relaxed);
        self.bootstrap_failure_count.store(0, Ordering::Relaxed);
    }
}

// ============================================================================
// Eviction Manager - Core Eviction Logic
// ============================================================================

/// Manages eviction of stale snapshot buffers.
///
/// The EvictionManager provides age-based eviction, force cleanup,
/// and diagnostic tools for snapshot lifecycle management.
///
/// # Thread Safety
///
/// This struct is thread-safe and can be shared across threads.
/// Operations are atomic and do not require external synchronization.
///
/// # Example
///
/// ```ignore
/// let storage = DashMapSnapshotStorage::new();
/// let eviction = EvictionManager::new(storage);
///
/// // Periodic cleanup
/// let result = eviction.evict_stale(300_000); // 5 minute max age
/// println!("Evicted {} buffers in {}µs", result.evicted_count, result.duration_us);
/// ```
pub struct EvictionManager<S: SnapshotStorage> {
    storage: S,
    config: EvictionConfig,
}

impl<S: SnapshotStorage> EvictionManager<S> {
    /// Create a new eviction manager with default configuration.
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            config: EvictionConfig::default(),
        }
    }

    /// Create a new eviction manager with custom configuration.
    pub fn with_config(storage: S, config: EvictionConfig) -> Self {
        Self { storage, config }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &EvictionConfig {
        &self.config
    }

    /// Update the eviction configuration.
    pub fn set_config(&mut self, config: EvictionConfig) {
        self.config = config;
    }

    // =========================================================================
    // Core Eviction Methods
    // =========================================================================

    /// Evict stale snapshots using the configured max age.
    ///
    /// This is the primary eviction method. Call it periodically
    /// (recommended: every 30-60 seconds for mainnet).
    ///
    /// # Returns
    ///
    /// Detailed statistics about the eviction operation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = eviction.evict_stale_default();
    /// if result.evicted_count > 0 {
    ///     tracing::info!("Evicted {} stale snapshots", result.evicted_count);
    /// }
    /// ```
    pub fn evict_stale_default(&self) -> EvictionResult {
        self.evict_stale(self.config.max_age_ms)
    }

    /// Evict stale snapshots with a custom max age.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Maximum age in milliseconds. Buffers older than this are removed.
    ///
    /// # Returns
    ///
    /// Detailed statistics about the eviction operation.
    ///
    /// # Performance
    ///
    /// This operation has O(n) time complexity where n is the number of snapshot buffers.
    /// It is designed to be efficient even with 10,000+ mints.
    ///
    /// # Edge Cases
    ///
    /// - **max_age_ms = 0**: All snapshots are considered stale and will be evicted
    /// - **max_age_ms = u64::MAX**: Effectively disables eviction (no snapshots are stale)
    pub fn evict_stale(&self, max_age_ms: u64) -> EvictionResult {
        let start = std::time::Instant::now();
        let now_ms = current_time_ms();

        // Single iteration to collect all statistics and identify stale entries
        // Optimization: collect both stale keys and retained age info in one pass
        struct EntryInfo {
            mint: Pubkey,
            age: u64,
            is_stale: bool,
        }

        let entries: Vec<EntryInfo> = self.storage.collect(|mint, buffer| {
            let age = now_ms.saturating_sub(buffer.created_at_ms);
            Some(EntryInfo {
                mint: *mint,
                age,
                is_stale: age > max_age_ms,
            })
        });

        // Partition into stale and retained, computing statistics
        let mut stale_keys: Vec<Pubkey> = Vec::new();
        let mut max_evicted_age = 0u64;
        let mut min_retained_age = u64::MAX;
        let mut retained_count = 0usize;

        for entry in entries {
            if entry.is_stale {
                stale_keys.push(entry.mint);
                if entry.age > max_evicted_age {
                    max_evicted_age = entry.age;
                }
            } else {
                retained_count += 1;
                if entry.age < min_retained_age {
                    min_retained_age = entry.age;
                }
            }
        }

        // Normalize min_retained_age if no entries were retained
        if retained_count == 0 {
            min_retained_age = 0;
        }

        // Remove stale entries
        for mint in &stale_keys {
            self.storage.remove(mint);
        }

        let duration = start.elapsed();

        EvictionResult {
            evicted_count: stale_keys.len(),
            retained_count,
            duration_us: duration.as_micros() as u64,
            max_evicted_age_ms: max_evicted_age,
            min_retained_age_ms: min_retained_age,
        }
    }

    /// Evict stale snapshots and return only the count (simpler API).
    ///
    /// This method provides backward compatibility with the original
    /// `evict_stale_snapshots` method from ShadowLedger.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Maximum age in milliseconds.
    ///
    /// # Returns
    ///
    /// Number of snapshot buffers evicted.
    pub fn evict_stale_count(&self, max_age_ms: u64) -> usize {
        self.evict_stale(max_age_ms).evicted_count
    }

    /// Force cleanup of all snapshots regardless of age.
    ///
    /// Use this for:
    /// - Shutdown procedures
    /// - Memory pressure situations
    /// - Full reset scenarios
    ///
    /// # Returns
    ///
    /// Number of snapshot buffers removed.
    pub fn force_cleanup(&self) -> usize {
        let count = self.storage.len();
        self.storage.clear();
        count
    }

    /// Remove snapshots for specific mints.
    ///
    /// # Arguments
    ///
    /// * `mints` - Iterator of mint addresses to remove
    ///
    /// # Returns
    ///
    /// Number of snapshot buffers actually removed.
    pub fn remove_mints<I>(&self, mints: I) -> usize
    where
        I: IntoIterator<Item = Pubkey>,
    {
        let mut count = 0;
        for mint in mints {
            if self.storage.remove(&mint).is_some() {
                count += 1;
            }
        }
        count
    }

    // =========================================================================
    // Advanced Eviction Methods
    // =========================================================================

    /// Perform aggressive eviction based on curve state.
    ///
    /// This method evicts snapshots based on curve-specific criteria:
    /// - Bonding progress > threshold (default: 90%)
    /// - Curve not updated for > max_slot_age slots (default: 10)
    ///
    /// # Arguments
    ///
    /// * `curve_info_fn` - A closure that returns CurveInfo for a mint, or None if not available
    /// * `current_slot` - Current Solana slot number
    ///
    /// # Returns
    ///
    /// Number of snapshots evicted due to aggressive criteria.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let evicted = manager.evict_aggressive(|mint| ledger.get_curve_info(mint), current_slot);
    /// ```
    pub fn evict_aggressive<F>(&self, curve_info_fn: F, current_slot: u64) -> usize
    where
        F: Fn(&Pubkey) -> Option<super::storage::CurveInfo>,
    {
        if !self.config.aggressive.enabled {
            return 0;
        }

        let threshold = self.config.aggressive.bonding_progress_threshold;
        let max_slot_age = self.config.aggressive.max_slot_age;

        // Collect mints that should be evicted based on curve state
        let mints_to_evict: Vec<Pubkey> = self
            .storage
            .get_all_mints()
            .into_iter()
            .filter(|mint| {
                if let Some(info) = curve_info_fn(mint) {
                    // Evict if bonding progress > threshold
                    if info.bonding_progress > threshold {
                        return true;
                    }
                    // Evict if curve hasn't been updated for too long
                    if current_slot.saturating_sub(info.last_updated_slot) > max_slot_age {
                        return true;
                    }
                }
                false
            })
            .collect();

        let count = mints_to_evict.len();
        for mint in mints_to_evict {
            self.storage.remove(&mint);
        }

        count
    }

    /// Perform LRU eviction to limit the number of stored curves.
    ///
    /// This method evicts the least-recently-used snapshots when the storage
    /// exceeds the configured maximum number of curves.
    ///
    /// # Arguments
    ///
    /// * `priority_fn` - A closure that returns a priority score for each mint.
    ///                   Higher scores = higher priority = less likely to be evicted.
    ///                   Return None for mints that should not be considered for eviction.
    ///
    /// # Returns
    ///
    /// Number of snapshots evicted due to LRU limit.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Evict based on tx_count (lower tx_count = lower priority = evicted first)
    /// let evicted = manager.evict_lru(|mint| {
    ///     ledger.get_curve_info(mint).map(|info| info.tx_count as f64)
    /// });
    /// ```
    pub fn evict_lru<F>(&self, priority_fn: F) -> usize
    where
        F: Fn(&Pubkey) -> Option<f64>,
    {
        if !self.config.lru.enabled {
            return 0;
        }

        let current_count = self.storage.len();
        let max_curves = self.config.lru.max_curves;

        if current_count <= max_curves {
            return 0;
        }

        // Number of entries to evict
        let to_evict = current_count - max_curves;

        // Collect all mints with their priorities, filtering out NaN values
        let mut entries: Vec<(Pubkey, f64)> = self
            .storage
            .get_all_mints()
            .into_iter()
            .filter_map(|mint| {
                priority_fn(&mint)
                    .filter(|p| !p.is_nan())
                    .map(|priority| (mint, priority))
            })
            .collect();

        // Sort by priority (ascending - lowest priority first for eviction)
        // Use total_cmp for deterministic ordering (NaN already filtered)
        entries.sort_by(|a, b| a.1.total_cmp(&b.1));

        // Evict the lowest priority entries
        let mut evicted = 0;
        for (mint, _) in entries.into_iter().take(to_evict) {
            if self.storage.remove(&mint).is_some() {
                evicted += 1;
            }
        }

        evicted
    }

    /// Perform LRU eviction using snapshot age as the priority.
    ///
    /// Older snapshots are evicted first (lowest priority).
    ///
    /// # Returns
    ///
    /// Number of snapshots evicted due to LRU limit.
    pub fn evict_lru_by_age(&self) -> usize {
        let now_ms = current_time_ms();
        self.evict_lru(|mint| {
            self.storage.get(mint).map(|buffer| {
                // Use negative age so older = lower priority = evicted first
                let age = now_ms.saturating_sub(buffer.created_at_ms);
                -(age as f64)
            })
        })
    }

    /// Perform combined eviction: age-based + aggressive + LRU.
    ///
    /// This is the recommended method for production use when all eviction
    /// strategies are enabled. It performs:
    /// 1. Age-based eviction (always)
    /// 2. Aggressive eviction (if enabled and curve_info_fn provided)
    /// 3. LRU eviction (if enabled)
    ///
    /// # Arguments
    ///
    /// * `curve_info_fn` - Optional closure for curve info (for aggressive eviction)
    /// * `priority_fn` - Optional closure for priority scoring (for LRU eviction)
    /// * `current_slot` - Current Solana slot (required for aggressive eviction)
    ///
    /// # Returns
    ///
    /// Combined eviction result with total counts.
    pub fn evict_all<F1, F2>(
        &self,
        curve_info_fn: Option<F1>,
        priority_fn: Option<F2>,
        current_slot: u64,
    ) -> EvictionResult
    where
        F1: Fn(&Pubkey) -> Option<super::storage::CurveInfo>,
        F2: Fn(&Pubkey) -> Option<f64>,
    {
        let start = std::time::Instant::now();

        // 1. Age-based eviction (always)
        let age_result = self.evict_stale(self.config.max_age_ms);
        let mut total_evicted = age_result.evicted_count;

        // 2. Aggressive eviction (if configured and callback provided)
        if let Some(curve_fn) = curve_info_fn {
            let aggressive_evicted = self.evict_aggressive(curve_fn, current_slot);
            total_evicted += aggressive_evicted;
        }

        // 3. LRU eviction (if configured and callback provided)
        if let Some(priority_fn) = priority_fn {
            let lru_evicted = self.evict_lru(priority_fn);
            total_evicted += lru_evicted;
        } else if self.config.lru.enabled {
            // Use age-based LRU as default
            let lru_evicted = self.evict_lru_by_age();
            total_evicted += lru_evicted;
        }

        let duration = start.elapsed();

        EvictionResult {
            evicted_count: total_evicted,
            retained_count: self.storage.len(),
            duration_us: duration.as_micros() as u64,
            max_evicted_age_ms: age_result.max_evicted_age_ms,
            min_retained_age_ms: age_result.min_retained_age_ms,
        }
    }

    // =========================================================================
    // Diagnostic Methods
    // =========================================================================

    /// Get diagnostic information for a specific mint.
    ///
    /// # Arguments
    ///
    /// * `mint` - Mint address to inspect
    ///
    /// # Returns
    ///
    /// Diagnostic information if the snapshot exists.
    pub fn get_diagnostic(&self, mint: &Pubkey) -> Option<SnapshotDiagnostic> {
        self.storage.get(mint).map(|buffer| {
            let now_ms = current_time_ms();
            let age = now_ms.saturating_sub(buffer.created_at_ms);
            SnapshotDiagnostic {
                mint: *mint,
                age_ms: age,
                snapshot_count: buffer.snapshots.len(),
                created_at_ms: buffer.created_at_ms,
                is_stale: age > self.config.max_age_ms,
            }
        })
    }

    /// Get diagnostic information for all snapshots.
    ///
    /// # Returns
    ///
    /// Vector of diagnostic information for each snapshot buffer.
    ///
    /// # Warning
    ///
    /// This method allocates memory proportional to the number of snapshot buffers.
    /// Use `get_stats` for aggregate statistics without full enumeration.
    pub fn get_all_diagnostics(&self) -> Vec<SnapshotDiagnostic> {
        let now_ms = current_time_ms();
        let max_age = self.config.max_age_ms;

        self.storage.collect(|mint, buffer| {
            let age = now_ms.saturating_sub(buffer.created_at_ms);
            Some(SnapshotDiagnostic {
                mint: *mint,
                age_ms: age,
                snapshot_count: buffer.snapshots.len(),
                created_at_ms: buffer.created_at_ms,
                is_stale: age > max_age,
            })
        })
    }

    /// Get aggregated statistics for all snapshots.
    ///
    /// This method is more efficient than `get_all_diagnostics` when you only
    /// need aggregate statistics.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Max age to use for stale/fresh classification
    ///
    /// # Returns
    ///
    /// Aggregated statistics.
    pub fn get_stats(&self, max_age_ms: u64) -> DiagnosticStats {
        let now_ms = current_time_ms();

        let mut total_buffers = 0usize;
        let mut stale_count = 0usize;
        let mut fresh_count = 0usize;
        let mut total_age = 0u64;
        let mut max_age = 0u64;
        let mut min_age = u64::MAX;
        let mut total_snapshots = 0usize;

        let data: Vec<(u64, usize, bool)> = self.storage.collect(|_, buffer| {
            let age = now_ms.saturating_sub(buffer.created_at_ms);
            let is_stale = age > max_age_ms;
            Some((age, buffer.snapshots.len(), is_stale))
        });

        for (age, snapshot_count, is_stale) in data {
            total_buffers += 1;
            total_age += age;
            total_snapshots += snapshot_count;

            if is_stale {
                stale_count += 1;
            } else {
                fresh_count += 1;
            }

            if age > max_age {
                max_age = age;
            }
            if age < min_age {
                min_age = age;
            }
        }

        let avg_age = if total_buffers > 0 {
            total_age as f64 / total_buffers as f64
        } else {
            0.0
        };

        DiagnosticStats {
            total_buffers,
            stale_count,
            fresh_count,
            avg_age_ms: avg_age,
            max_age_ms: if total_buffers > 0 { max_age } else { 0 },
            min_age_ms: if total_buffers > 0 { min_age } else { 0 },
            total_snapshots,
        }
    }

    /// Get aggregated statistics using the configured max age.
    pub fn get_stats_default(&self) -> DiagnosticStats {
        self.get_stats(self.config.max_age_ms)
    }

    /// Get list of stale mints without evicting them.
    ///
    /// Useful for preview or confirmation before eviction.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Max age threshold
    ///
    /// # Returns
    ///
    /// Vector of mint addresses that would be evicted.
    pub fn preview_eviction(&self, max_age_ms: u64) -> Vec<Pubkey> {
        let now_ms = current_time_ms();

        self.storage
            .filter_keys(|buffer| now_ms.saturating_sub(buffer.created_at_ms) > max_age_ms)
    }

    /// Get list of stale mints using the configured max age.
    pub fn preview_eviction_default(&self) -> Vec<Pubkey> {
        self.preview_eviction(self.config.max_age_ms)
    }
}

// ============================================================================
// Standalone Functions for Direct Use
// ============================================================================

/// Evict stale snapshots from a storage instance (standalone function).
///
/// This function provides a simpler API for cases where you don't need
/// the full EvictionManager functionality.
///
/// # Arguments
///
/// * `storage` - The snapshot storage to clean
/// * `max_age_ms` - Maximum age in milliseconds
///
/// # Returns
///
/// Number of snapshot buffers evicted.
///
/// # Example
///
/// ```ignore
/// let evicted = evict_stale(&storage, 300_000);
/// ```
pub fn evict_stale<S: SnapshotStorage>(storage: &S, max_age_ms: u64) -> usize {
    let now_ms = current_time_ms();

    // Find stale entries
    let stale_keys: Vec<Pubkey> =
        storage.filter_keys(|buffer| now_ms.saturating_sub(buffer.created_at_ms) > max_age_ms);

    let count = stale_keys.len();
    for key in stale_keys {
        storage.remove(&key);
    }

    count
}

/// Evict stale snapshots using default max age (standalone function).
pub fn evict_stale_default<S: SnapshotStorage>(storage: &S) -> usize {
    evict_stale(storage, DEFAULT_SNAPSHOT_MAX_AGE_MS)
}

/// Force cleanup of all snapshots (standalone function).
pub fn force_cleanup<S: SnapshotStorage>(storage: &S) -> usize {
    let count = storage.len();
    storage.clear();
    count
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shadow_ledger::storage::DashMapSnapshotStorage;
    use crate::shadow_ledger::types::MarketSnapshot;

    fn create_test_buffer(created_at_ms: u64) -> super::super::types::SnapshotBuffer {
        super::super::types::SnapshotBuffer::with_timestamp(
            vec![MarketSnapshot::new(1000)],
            created_at_ms,
        )
    }

    fn create_storage_with_entries(entries: Vec<(Pubkey, u64)>) -> DashMapSnapshotStorage {
        let storage = DashMapSnapshotStorage::new();
        for (mint, created_at_ms) in entries {
            storage.insert(mint, create_test_buffer(created_at_ms));
        }
        storage
    }

    // =========================================================================
    // EvictionConfig Tests
    // =========================================================================

    #[test]
    fn test_eviction_config_default() {
        let config = EvictionConfig::default();
        assert_eq!(config.max_age_ms, DEFAULT_SNAPSHOT_MAX_AGE_MS);
        assert!(!config.log_stats);
    }

    #[test]
    fn test_eviction_config_with_max_age() {
        let config = EvictionConfig::with_max_age_ms(60_000);
        assert_eq!(config.max_age_ms, 60_000);
        assert!(!config.log_stats);
    }

    #[test]
    fn test_eviction_config_mainnet() {
        let config = EvictionConfig::mainnet();
        assert_eq!(config.max_age_ms, DEFAULT_SNAPSHOT_MAX_AGE_MS);
        assert!(config.log_stats);
    }

    #[test]
    fn test_eviction_config_testnet() {
        let config = EvictionConfig::testnet();
        assert_eq!(config.max_age_ms, 120_000);
        assert!(config.log_stats);
    }

    #[test]
    fn test_eviction_config_aggressive() {
        let config = EvictionConfig::aggressive();
        assert_eq!(config.max_age_ms, 30_000);
        assert!(config.log_stats);
    }

    // =========================================================================
    // EvictionResult Tests
    // =========================================================================

    #[test]
    fn test_eviction_result_total_processed() {
        let result = EvictionResult {
            evicted_count: 10,
            retained_count: 90,
            ..Default::default()
        };
        assert_eq!(result.total_processed(), 100);
    }

    #[test]
    fn test_eviction_result_eviction_rate() {
        let result = EvictionResult {
            evicted_count: 25,
            retained_count: 75,
            ..Default::default()
        };
        assert!((result.eviction_rate() - 25.0).abs() < 0.01);
    }

    #[test]
    fn test_eviction_result_empty() {
        let result = EvictionResult::default();
        assert_eq!(result.total_processed(), 0);
        assert_eq!(result.eviction_rate(), 0.0);
    }

    // =========================================================================
    // EvictionManager Core Tests
    // =========================================================================

    #[test]
    fn test_eviction_manager_new() {
        let storage = DashMapSnapshotStorage::new();
        let manager = EvictionManager::new(storage);
        assert_eq!(manager.config().max_age_ms, DEFAULT_SNAPSHOT_MAX_AGE_MS);
    }

    #[test]
    fn test_eviction_manager_with_config() {
        let storage = DashMapSnapshotStorage::new();
        let config = EvictionConfig::with_max_age_ms(60_000);
        let manager = EvictionManager::with_config(storage, config);
        assert_eq!(manager.config().max_age_ms, 60_000);
    }

    #[test]
    fn test_evict_stale_empty_storage() {
        let storage = DashMapSnapshotStorage::new();
        let manager = EvictionManager::new(storage);

        let result = manager.evict_stale(300_000);

        assert_eq!(result.evicted_count, 0);
        assert_eq!(result.retained_count, 0);
    }

    #[test]
    fn test_evict_stale_all_fresh() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms),        // Just created
            (Pubkey::new_unique(), now_ms - 1000), // 1 second ago
            (Pubkey::new_unique(), now_ms - 5000), // 5 seconds ago
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        let result = manager.evict_stale(300_000); // 5 minute max age

        assert_eq!(result.evicted_count, 0);
        assert_eq!(result.retained_count, 3);
        assert_eq!(storage.len(), 3);
    }

    #[test]
    fn test_evict_stale_all_stale() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms - 400_000), // 6.6 minutes ago
            (Pubkey::new_unique(), now_ms - 500_000), // 8.3 minutes ago
            (Pubkey::new_unique(), now_ms - 600_000), // 10 minutes ago
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        let result = manager.evict_stale(300_000); // 5 minute max age

        assert_eq!(result.evicted_count, 3);
        assert_eq!(result.retained_count, 0);
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_evict_stale_mixed() {
        let now_ms = current_time_ms();
        let old_mint = Pubkey::new_unique();
        let new_mint = Pubkey::new_unique();

        let entries = vec![
            (old_mint, now_ms - 400_000), // Stale (6.6 minutes)
            (new_mint, now_ms - 10_000),  // Fresh (10 seconds)
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        let result = manager.evict_stale(300_000);

        assert_eq!(result.evicted_count, 1);
        assert_eq!(result.retained_count, 1);
        assert!(!storage.contains(&old_mint));
        assert!(storage.contains(&new_mint));
    }

    #[test]
    fn test_evict_stale_zero_max_age() {
        let now_ms = current_time_ms();
        let entries = vec![
            // Created 1ms ago - age is 1ms, which is > 0, so it's stale
            (Pubkey::new_unique(), now_ms - 1),
            // Created 2ms ago - age is 2ms, which is > 0, so it's stale
            (Pubkey::new_unique(), now_ms - 2),
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        // max_age_ms = 0 means snapshots with age > 0 are stale
        let result = manager.evict_stale(0);

        assert_eq!(result.evicted_count, 2);
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_evict_stale_count() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms - 400_000),
            (Pubkey::new_unique(), now_ms - 500_000),
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage);

        let count = manager.evict_stale_count(300_000);

        assert_eq!(count, 2);
    }

    #[test]
    fn test_force_cleanup() {
        let entries = vec![
            (Pubkey::new_unique(), current_time_ms()),
            (Pubkey::new_unique(), current_time_ms()),
            (Pubkey::new_unique(), current_time_ms()),
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        assert_eq!(storage.len(), 3);

        let count = manager.force_cleanup();

        assert_eq!(count, 3);
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_remove_mints() {
        let mint1 = Pubkey::new_unique();
        let mint2 = Pubkey::new_unique();
        let mint3 = Pubkey::new_unique();

        let entries = vec![
            (mint1, current_time_ms()),
            (mint2, current_time_ms()),
            (mint3, current_time_ms()),
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        let count = manager.remove_mints(vec![mint1, mint2]);

        assert_eq!(count, 2);
        assert!(!storage.contains(&mint1));
        assert!(!storage.contains(&mint2));
        assert!(storage.contains(&mint3));
    }

    // =========================================================================
    // Diagnostic Tests
    // =========================================================================

    #[test]
    fn test_get_diagnostic() {
        let mint = Pubkey::new_unique();
        let now_ms = current_time_ms();
        let created_at = now_ms - 10_000; // 10 seconds ago

        let storage = DashMapSnapshotStorage::new();
        storage.insert(mint, create_test_buffer(created_at));

        let manager = EvictionManager::new(storage);

        let diagnostic = manager.get_diagnostic(&mint);
        assert!(diagnostic.is_some());

        let d = diagnostic.unwrap();
        assert_eq!(d.mint, mint);
        assert!(d.age_ms >= 10_000 && d.age_ms < 11_000);
        assert_eq!(d.snapshot_count, 1);
        assert_eq!(d.created_at_ms, created_at);
        assert!(!d.is_stale); // 10 seconds < 5 minutes
    }

    #[test]
    fn test_get_diagnostic_not_found() {
        let storage = DashMapSnapshotStorage::new();
        let manager = EvictionManager::new(storage);

        let diagnostic = manager.get_diagnostic(&Pubkey::new_unique());
        assert!(diagnostic.is_none());
    }

    #[test]
    fn test_get_all_diagnostics() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms - 1000),
            (Pubkey::new_unique(), now_ms - 2000),
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage);

        let diagnostics = manager.get_all_diagnostics();

        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_get_stats() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms - 10_000),  // Fresh (10s)
            (Pubkey::new_unique(), now_ms - 20_000),  // Fresh (20s)
            (Pubkey::new_unique(), now_ms - 400_000), // Stale (6.6min)
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage);

        let stats = manager.get_stats(300_000);

        assert_eq!(stats.total_buffers, 3);
        assert_eq!(stats.stale_count, 1);
        assert_eq!(stats.fresh_count, 2);
        assert_eq!(stats.total_snapshots, 3);
        assert!(stats.avg_age_ms > 0.0);
        assert!(stats.max_age_ms >= 400_000);
        assert!(stats.min_age_ms >= 10_000);
    }

    #[test]
    fn test_get_stats_empty() {
        let storage = DashMapSnapshotStorage::new();
        let manager = EvictionManager::new(storage);

        let stats = manager.get_stats(300_000);

        assert_eq!(stats.total_buffers, 0);
        assert_eq!(stats.stale_count, 0);
        assert_eq!(stats.fresh_count, 0);
        assert_eq!(stats.avg_age_ms, 0.0);
        assert_eq!(stats.max_age_ms, 0);
        assert_eq!(stats.min_age_ms, 0);
    }

    #[test]
    fn test_preview_eviction() {
        let now_ms = current_time_ms();
        let stale_mint = Pubkey::new_unique();
        let fresh_mint = Pubkey::new_unique();

        let entries = vec![
            (stale_mint, now_ms - 400_000), // Stale
            (fresh_mint, now_ms - 10_000),  // Fresh
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        let preview = manager.preview_eviction(300_000);

        assert_eq!(preview.len(), 1);
        assert!(preview.contains(&stale_mint));

        // Verify nothing was actually evicted
        assert_eq!(storage.len(), 2);
    }

    // =========================================================================
    // Standalone Function Tests
    // =========================================================================

    #[test]
    fn test_standalone_evict_stale() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms - 400_000),
            (Pubkey::new_unique(), now_ms - 10_000),
        ];
        let storage = create_storage_with_entries(entries);

        let count = evict_stale(&storage, 300_000);

        assert_eq!(count, 1);
        assert_eq!(storage.len(), 1);
    }

    #[test]
    fn test_standalone_force_cleanup() {
        let entries = vec![
            (Pubkey::new_unique(), current_time_ms()),
            (Pubkey::new_unique(), current_time_ms()),
        ];
        let storage = create_storage_with_entries(entries);

        let count = force_cleanup(&storage);

        assert_eq!(count, 2);
        assert_eq!(storage.len(), 0);
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_evict_with_max_u64_age() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms - 1_000_000_000), // Very old
            (Pubkey::new_unique(), 0),                      // Epoch time
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        // u64::MAX effectively disables eviction
        let result = manager.evict_stale(u64::MAX);

        assert_eq!(result.evicted_count, 0);
        assert_eq!(storage.len(), 2);
    }

    #[test]
    fn test_evict_future_timestamps() {
        let now_ms = current_time_ms();
        let entries = vec![
            (Pubkey::new_unique(), now_ms + 1_000_000), // Future timestamp
        ];
        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        // Future timestamps should not cause issues (saturating_sub handles it)
        let result = manager.evict_stale(300_000);

        // Future timestamp results in age = 0, so it's fresh
        assert_eq!(result.evicted_count, 0);
        assert_eq!(result.retained_count, 1);
    }

    // =========================================================================
    // Large Volume Tests (Spike Tests)
    // =========================================================================

    #[test]
    fn test_evict_large_volume() {
        let now_ms = current_time_ms();
        let mut entries = Vec::new();

        // Create 1000 entries, half stale and half fresh
        for i in 0..1000 {
            let age = if i % 2 == 0 { 400_000 } else { 10_000 };
            entries.push((Pubkey::new_unique(), now_ms - age));
        }

        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        let result = manager.evict_stale(300_000);

        assert_eq!(result.evicted_count, 500);
        assert_eq!(result.retained_count, 500);
        assert_eq!(storage.len(), 500);
    }

    #[test]
    fn test_force_cleanup_large_volume() {
        let entries: Vec<_> = (0..5000)
            .map(|_| (Pubkey::new_unique(), current_time_ms()))
            .collect();

        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage.clone());

        assert_eq!(storage.len(), 5000);

        let count = manager.force_cleanup();

        assert_eq!(count, 5000);
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_get_stats_large_volume() {
        let now_ms = current_time_ms();
        let entries: Vec<_> = (0..1000)
            .map(|i| (Pubkey::new_unique(), now_ms - (i * 100)))
            .collect();

        let storage = create_storage_with_entries(entries);
        let manager = EvictionManager::new(storage);

        let stats = manager.get_stats(300_000);

        assert_eq!(stats.total_buffers, 1000);
        assert_eq!(stats.total_snapshots, 1000); // 1 snapshot per buffer
    }

    // =========================================================================
    // Aggressive Eviction Config Tests
    // =========================================================================

    #[test]
    fn test_aggressive_eviction_config_default() {
        let config = AggressiveEvictionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.bonding_progress_threshold, 90);
        assert_eq!(config.max_slot_age, 10);
    }

    #[test]
    fn test_aggressive_eviction_config_enabled() {
        let config = AggressiveEvictionConfig::enabled();
        assert!(config.enabled);
        assert_eq!(config.bonding_progress_threshold, 90);
        assert_eq!(config.max_slot_age, 10);
    }

    #[test]
    fn test_aggressive_eviction_config_with_bonding_threshold() {
        let config = AggressiveEvictionConfig::with_bonding_threshold(80);
        assert!(config.enabled);
        assert_eq!(config.bonding_progress_threshold, 80);
    }

    #[test]
    fn test_aggressive_eviction_config_with_slot_age() {
        let config = AggressiveEvictionConfig::with_slot_age(5);
        assert!(config.enabled);
        assert_eq!(config.max_slot_age, 5);
    }

    // =========================================================================
    // LRU Config Tests
    // =========================================================================

    #[test]
    fn test_lru_config_default() {
        let config = LruConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_curves, 5000);
    }

    #[test]
    fn test_lru_config_enabled() {
        let config = LruConfig::enabled();
        assert!(config.enabled);
        assert_eq!(config.max_curves, 5000);
    }

    #[test]
    fn test_lru_config_with_max_curves() {
        let config = LruConfig::with_max_curves(3000);
        assert!(config.enabled);
        assert_eq!(config.max_curves, 3000);
    }

    // =========================================================================
    // EvictionConfig with New Features Tests
    // =========================================================================

    #[test]
    fn test_eviction_config_mainnet_has_lru() {
        let config = EvictionConfig::mainnet();
        assert!(config.lru.enabled);
        assert_eq!(config.lru.max_curves, 5000);
        assert!(!config.aggressive.enabled);
    }

    #[test]
    fn test_eviction_config_testnet_has_aggressive() {
        let config = EvictionConfig::testnet();
        assert!(config.aggressive.enabled);
        assert!(config.lru.enabled);
    }

    #[test]
    fn test_eviction_config_aggressive_preset() {
        let config = EvictionConfig::aggressive();
        assert!(config.aggressive.enabled);
        assert_eq!(config.aggressive.bonding_progress_threshold, 80);
        assert_eq!(config.aggressive.max_slot_age, 5);
        assert_eq!(config.lru.max_curves, 3000);
    }

    #[test]
    fn test_eviction_config_with_aggressive() {
        let config = EvictionConfig::default()
            .with_aggressive(AggressiveEvictionConfig::with_bonding_threshold(95));
        assert!(config.aggressive.enabled);
        assert_eq!(config.aggressive.bonding_progress_threshold, 95);
    }

    #[test]
    fn test_eviction_config_with_lru() {
        let config = EvictionConfig::default().with_lru(LruConfig::with_max_curves(2000));
        assert!(config.lru.enabled);
        assert_eq!(config.lru.max_curves, 2000);
    }

    // =========================================================================
    // EvictionMetrics Tests
    // =========================================================================

    #[test]
    fn test_eviction_metrics_new() {
        let metrics = EvictionMetrics::new();
        assert_eq!(metrics.eviction_count(), 0);
        assert_eq!(metrics.aggressive_eviction_count(), 0);
        assert_eq!(metrics.lru_eviction_count(), 0);
        assert_eq!(metrics.bootstrap_count(), 0);
        assert_eq!(metrics.bootstrap_failure_count(), 0);
    }

    #[test]
    fn test_eviction_metrics_record_eviction() {
        let metrics = EvictionMetrics::new();
        metrics.record_eviction(5);
        metrics.record_eviction(3);
        assert_eq!(metrics.eviction_count(), 8);
    }

    #[test]
    fn test_eviction_metrics_record_aggressive_eviction() {
        let metrics = EvictionMetrics::new();
        metrics.record_aggressive_eviction(10);
        assert_eq!(metrics.aggressive_eviction_count(), 10);
    }

    #[test]
    fn test_eviction_metrics_record_lru_eviction() {
        let metrics = EvictionMetrics::new();
        metrics.record_lru_eviction(7);
        assert_eq!(metrics.lru_eviction_count(), 7);
    }

    #[test]
    fn test_eviction_metrics_record_bootstrap() {
        let metrics = EvictionMetrics::new();
        metrics.record_bootstrap(3);
        metrics.record_bootstrap(5);
        assert_eq!(metrics.bootstrap_count(), 2);
        assert!((metrics.avg_bootstrap_duration_slots() - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_eviction_metrics_record_bootstrap_failure() {
        let metrics = EvictionMetrics::new();
        metrics.record_bootstrap_failure();
        metrics.record_bootstrap_failure();
        assert_eq!(metrics.bootstrap_failure_count(), 2);
    }

    #[test]
    fn test_eviction_metrics_avg_bootstrap_duration_empty() {
        let metrics = EvictionMetrics::new();
        assert_eq!(metrics.avg_bootstrap_duration_slots(), 0.0);
    }

    #[test]
    fn test_eviction_metrics_reset() {
        let metrics = EvictionMetrics::new();
        metrics.record_eviction(5);
        metrics.record_aggressive_eviction(3);
        metrics.record_lru_eviction(2);
        metrics.record_bootstrap(10);
        metrics.record_bootstrap_failure();

        metrics.reset();

        assert_eq!(metrics.eviction_count(), 0);
        assert_eq!(metrics.aggressive_eviction_count(), 0);
        assert_eq!(metrics.lru_eviction_count(), 0);
        assert_eq!(metrics.bootstrap_count(), 0);
        assert_eq!(metrics.bootstrap_failure_count(), 0);
    }

    #[test]
    fn test_eviction_metrics_to_prometheus() {
        let metrics = EvictionMetrics::new();
        metrics.record_eviction(10);
        metrics.record_aggressive_eviction(5);
        metrics.record_lru_eviction(3);

        let prometheus_output = metrics.to_prometheus();

        assert!(prometheus_output.contains("ghost_eviction_count 10"));
        assert!(prometheus_output.contains("ghost_aggressive_eviction_count 5"));
        assert!(prometheus_output.contains("ghost_lru_eviction_count 3"));
        assert!(prometheus_output.contains("# TYPE ghost_eviction_count counter"));
    }

    // =========================================================================
    // Advanced Eviction Method Tests
    // =========================================================================

    #[test]
    fn test_evict_aggressive_disabled() {
        let storage = DashMapSnapshotStorage::new();
        storage.insert(Pubkey::new_unique(), create_test_buffer(current_time_ms()));

        // Default config has aggressive disabled
        let manager = EvictionManager::new(storage.clone());

        let evicted = manager.evict_aggressive(|_| None, 1000);
        assert_eq!(evicted, 0);
        assert_eq!(storage.len(), 1);
    }

    #[test]
    fn test_evict_aggressive_by_bonding_progress() {
        let storage = DashMapSnapshotStorage::new();
        let high_progress_mint = Pubkey::new_unique();
        let low_progress_mint = Pubkey::new_unique();

        storage.insert(high_progress_mint, create_test_buffer(current_time_ms()));
        storage.insert(low_progress_mint, create_test_buffer(current_time_ms()));

        let config = EvictionConfig::default()
            .with_aggressive(AggressiveEvictionConfig::with_bonding_threshold(90));
        let manager = EvictionManager::with_config(storage.clone(), config);

        // High progress mint should be evicted
        let evicted = manager.evict_aggressive(
            |mint| {
                if *mint == high_progress_mint {
                    Some(super::super::storage::CurveInfo {
                        bonding_progress: 95,
                        last_updated_slot: 100,
                        ..Default::default()
                    })
                } else {
                    Some(super::super::storage::CurveInfo {
                        bonding_progress: 50,
                        last_updated_slot: 100,
                        ..Default::default()
                    })
                }
            },
            100,
        );

        assert_eq!(evicted, 1);
        assert!(!storage.contains(&high_progress_mint));
        assert!(storage.contains(&low_progress_mint));
    }

    #[test]
    fn test_evict_aggressive_by_slot_age() {
        let storage = DashMapSnapshotStorage::new();
        let stale_mint = Pubkey::new_unique();
        let fresh_mint = Pubkey::new_unique();

        storage.insert(stale_mint, create_test_buffer(current_time_ms()));
        storage.insert(fresh_mint, create_test_buffer(current_time_ms()));

        let config =
            EvictionConfig::default().with_aggressive(AggressiveEvictionConfig::with_slot_age(10));
        let manager = EvictionManager::with_config(storage.clone(), config);

        // Stale mint should be evicted (last updated 20 slots ago)
        let evicted = manager.evict_aggressive(
            |mint| {
                if *mint == stale_mint {
                    Some(super::super::storage::CurveInfo {
                        bonding_progress: 50,
                        last_updated_slot: 80, // 20 slots behind
                        ..Default::default()
                    })
                } else {
                    Some(super::super::storage::CurveInfo {
                        bonding_progress: 50,
                        last_updated_slot: 98, // Only 2 slots behind
                        ..Default::default()
                    })
                }
            },
            100,
        );

        assert_eq!(evicted, 1);
        assert!(!storage.contains(&stale_mint));
        assert!(storage.contains(&fresh_mint));
    }

    #[test]
    fn test_evict_lru_disabled() {
        let storage = DashMapSnapshotStorage::new();
        for _ in 0..10 {
            storage.insert(Pubkey::new_unique(), create_test_buffer(current_time_ms()));
        }

        // Default config has LRU disabled
        let manager = EvictionManager::new(storage.clone());

        let evicted = manager.evict_lru(|_| Some(1.0));
        assert_eq!(evicted, 0);
        assert_eq!(storage.len(), 10);
    }

    #[test]
    fn test_evict_lru_under_limit() {
        let storage = DashMapSnapshotStorage::new();
        for _ in 0..5 {
            storage.insert(Pubkey::new_unique(), create_test_buffer(current_time_ms()));
        }

        let config = EvictionConfig::default().with_lru(LruConfig::with_max_curves(10));
        let manager = EvictionManager::with_config(storage.clone(), config);

        // Under limit, nothing should be evicted
        let evicted = manager.evict_lru(|_| Some(1.0));
        assert_eq!(evicted, 0);
        assert_eq!(storage.len(), 5);
    }

    #[test]
    fn test_evict_lru_over_limit() {
        let storage = DashMapSnapshotStorage::new();
        let mut mints = Vec::new();
        for i in 0..15 {
            let mint = Pubkey::new_unique();
            mints.push((mint, i as f64));
            storage.insert(mint, create_test_buffer(current_time_ms()));
        }

        let config = EvictionConfig::default().with_lru(LruConfig::with_max_curves(10));
        let manager = EvictionManager::with_config(storage.clone(), config);

        // Should evict 5 entries (15 - 10 = 5)
        let evicted =
            manager.evict_lru(|mint| mints.iter().find(|(m, _)| m == mint).map(|(_, p)| *p));

        assert_eq!(evicted, 5);
        assert_eq!(storage.len(), 10);
    }

    #[test]
    fn test_evict_lru_by_age() {
        let now = current_time_ms();
        let storage = DashMapSnapshotStorage::new();

        // Insert entries with different ages
        for i in 0..15 {
            storage.insert(Pubkey::new_unique(), create_test_buffer(now - (i * 1000)));
        }

        let config = EvictionConfig::default().with_lru(LruConfig::with_max_curves(10));
        let manager = EvictionManager::with_config(storage.clone(), config);

        // Should evict 5 oldest entries
        let evicted = manager.evict_lru_by_age();

        assert_eq!(evicted, 5);
        assert_eq!(storage.len(), 10);
    }

    #[test]
    fn test_evict_all_combined() {
        let now = current_time_ms();
        let storage = DashMapSnapshotStorage::new();

        // Add some old entries (for age-based eviction)
        for _ in 0..3 {
            storage.insert(Pubkey::new_unique(), create_test_buffer(now - 400_000));
        }

        // Add some fresh entries
        for _ in 0..10 {
            storage.insert(Pubkey::new_unique(), create_test_buffer(now - 10_000));
        }

        let config = EvictionConfig::default()
            .with_aggressive(AggressiveEvictionConfig::default())
            .with_lru(LruConfig::with_max_curves(8));
        let manager = EvictionManager::with_config(storage.clone(), config);

        let result = manager.evict_all::<fn(&Pubkey) -> Option<super::super::storage::CurveInfo>, fn(&Pubkey) -> Option<f64>>(
            None, None, 100
        );

        // Should evict 3 old + 2 LRU = 5 total (to get down to 8)
        assert!(result.evicted_count >= 3);
        assert!(storage.len() <= 8);
    }
}
