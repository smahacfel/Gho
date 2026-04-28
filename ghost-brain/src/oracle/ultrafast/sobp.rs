//! SOBP (Bucket-Over-Bucket Buying Pressure) Module
//!
//! Ultra-fast buying pressure analysis using event-time transaction bucketing.
//! Detects pump onset within the critical 0-2 second window after token launch
//! by measuring the rate of change in weighted buy intensity across consecutive time buckets.
//!
//! ## Core Concept
//!
//! SOBP analyzes how quickly buying pressure changes between consecutive time buckets:
//!
//! ```text
//! SOBP(bucket) = BuyIntensity(bucket) / BuyIntensity(bucket-1)
//! ```
//!
//! Where BuyIntensity is the sum of weighted transactions in that bucket.
//!
//! ## Why SOBP Works
//!
//! Traditional metrics (volume, price, holder count) lag by several seconds and require
//! historical context. SOBP operates on ~400ms buckets derived from event time
//! and detects **changes in dynamics** rather than absolute values, making it effective
//! even with zero macro-level volume.
//!
//! ## Signal Interpretation
//!
//! | SOBP Value | Interpretation                                    |
//! |------------|---------------------------------------------------|
//! | > 3.0      | Hyper-aggressive buy influx → early pump detected |
//! | 1.5-3.0    | Stable organic growth → bullish                   |
//! | 1.0-1.5    | Moderate upward trend                             |
//! | 0.8-1.0    | Stagnation / no clear direction                   |
//! | 0.4-0.8    | Pressure decline → early rug behavior             |
//! | < 0.4      | Demand implosion → panic/dump setup               |
//!
//! ## Performance Target
//!
//! - **Bucket Update**: <10 microseconds per transaction insertion
//! - **SOBP Calculation**: <5 microseconds per bucket query
//! - **Zero Heap Allocation**: Stack-only circular buffer with fixed capacity
//! - **O(1) Access**: Constant-time bucket lookups and updates
//!
//! ## Thread Safety
//!
//! All types implement `Send + Sync` for concurrent usage across threads.
//!
//! ## Integration with MPCF
//!
//! SOBP leverages MPCF (Micro-Payload Cognitive Fingerprint) actor classification
//! to apply intelligent weighting:
//!
//! - **Human actors** (HumanMobile, HumanDesktop): 2.0x weight multiplier
//! - **Sniper bots** (SniperScript): 0.5x weight multiplier (noise reduction)
//! - **Other actors**: 1.0x default weight
//!
//! This ensures organic human buying generates stronger signals while filtering
//! bot-driven artificial activity.
//!
//! # Example
//!
//! ```rust,ignore
//! use ghost_brain::oracle::ultrafast::sobp::{SobpCore, TransactionRecord};
//! use ghost_brain::oracle::ultrafast::mpcf::ActorType;
//!
//! // Initialize SOBP tracker with 64-bucket history
//! let mut sobp = SobpCore::<64>::new();
//!
//! // Process incoming transactions
//! let tx_record = TransactionRecord {
//!     slot: None,
//!     actor_type: ActorType::HumanMobile,
//!     amount_sol: 1.0,
//!     cir_effective: None,
//!     tx_size_bytes: 256,
//!     is_buy: true,
//!     timestamp_ms: 1_700_000_000_000,
//!     price: None,
//! };
//!
//! sobp.record_transaction(&tx_record);
//!
//! // Calculate SOBP for current bucket
//! if let Some(sobp_value) = sobp.calculate_sobp(1_700_000_000_000 / SOBP_BUCKET_MS) {
//!     if sobp_value > 3.0 {
//!         println!("🚀 ULTRA PUMP DETECTED: SOBP = {}", sobp_value);
//!     }
//! }
//! ```

// =============================================================================
// Imports
// =============================================================================

use super::mpcf::ActorType;
use metrics::increment_counter;

// =============================================================================
// Constants
// =============================================================================

/// Default bucket history capacity (must be power of 2 for efficient modulo)
pub const DEFAULT_SLOT_CAPACITY: usize = 64;

/// Minimum bucket history for reliable SOBP calculation
pub const MIN_SLOT_HISTORY: usize = 2;

/// Weight multiplier for human actors (bullish signal amplification)
const HUMAN_WEIGHT_MULTIPLIER: f32 = 2.0;

/// Weight multiplier for sniper bots (noise reduction)
const SNIPER_WEIGHT_MULTIPLIER: f32 = 0.5;

/// Default weight multiplier for other actors
const DEFAULT_WEIGHT_MULTIPLIER: f32 = 1.0;

/// Minimum buy intensity to avoid division by zero
const MIN_BUY_INTENSITY: f32 = 0.0001;

/// Base weight for a transaction (before actor multiplier)
const BASE_TRANSACTION_WEIGHT: f32 = 1.0;
/// Event-time bucket size in milliseconds for SOBP bucketing.
const SOBP_BUCKET_MS: u64 = 400;

/// Baseline intensity for missing previous slot (treated as neutral pressure)
const BASELINE_INTENSITY: f32 = 1.0;

/// Maximum allowed SOBP ratio to prevent extreme spikes when previous slot had near-zero intensity.
/// Tuned (~50x relative jump) to preserve hyper pump detection (see SOBP_THRESHOLD_HYPER=3.0) while
/// bounding outliers that would overflow downstream metrics.
const SOBP_RATIO_MAX: f32 = 50.0;
/// Minimum allowed SOBP ratio; negative ratios are non-physical and treated as zero pressure.
const SOBP_RATIO_MIN: f32 = 0.0;
/// Baseline ratio used when input is invalid/NaN; keeps signal neutral instead of propagating NaN.
const SOBP_RATIO_BASELINE: f32 = BASELINE_INTENSITY;

/// SOBP threshold for Hyper-aggressive buying (ultra pump detection)
const SOBP_THRESHOLD_HYPER: f32 = 3.0;

/// SOBP threshold for Growth phase (bullish)
const SOBP_THRESHOLD_GROWTH: f32 = 1.5;

/// SOBP threshold for Stagnation (neutral)
const SOBP_THRESHOLD_STAGNATION: f32 = 0.8;

/// SOBP threshold for Implosion (demand collapse)
const SOBP_THRESHOLD_IMPLOSION: f32 = 0.4;

/// Confidence calculation: Max slots for full history factor
const CONFIDENCE_MAX_HISTORY_SLOTS: f32 = 10.0;

/// Confidence calculation: Min transaction count for high confidence
const CONFIDENCE_MIN_TX_COUNT: f32 = 5.0;

/// Confidence calculation: Intensity normalization factor
const CONFIDENCE_INTENSITY_NORMALIZER: f32 = 10.0;

/// Confidence calculation: Weight for history factor
/// Note: All weights must sum to 1.0 for proper normalization
/// CONFIDENCE_WEIGHT_HISTORY + CONFIDENCE_WEIGHT_TX_COUNT + CONFIDENCE_WEIGHT_INTENSITY = 1.0
const CONFIDENCE_WEIGHT_HISTORY: f32 = 0.4;

/// Confidence calculation: Weight for transaction count factor
const CONFIDENCE_WEIGHT_TX_COUNT: f32 = 0.3;

/// Confidence calculation: Weight for intensity factor
const CONFIDENCE_WEIGHT_INTENSITY: f32 = 0.3;

// =============================================================================
// Helpers
// =============================================================================

#[inline]
fn clamp_sobp_ratio(raw: f32) -> f32 {
    if !raw.is_finite() {
        increment_counter!("sobp_ratio_invalid_total");
        return SOBP_RATIO_BASELINE;
    }

    if raw < SOBP_RATIO_MIN {
        increment_counter!("sobp_ratio_clamped_total", "direction" => "lower");
        return SOBP_RATIO_MIN;
    }

    if raw > SOBP_RATIO_MAX {
        increment_counter!("sobp_ratio_clamped_total", "direction" => "upper");
        return SOBP_RATIO_MAX;
    }

    raw
}

// =============================================================================
// Pressure State Classification
// =============================================================================

/// Buying pressure state classification based on SOBP value.
///
/// Maps SOBP ratios to discrete market pressure states for
/// decision-making in trading strategies.
///
/// # State Transitions
///
/// ```text
/// Implosion (<0.4) → Decline (0.4-0.8) → Stagnation (0.8-1.5) → Growth (1.5-3.0) → Hyper (≥3.0)
/// ```
///
/// # Thread Safety
///
/// Implements `Copy + Clone + Send + Sync` for concurrent usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureState {
    /// Demand implosion: SOBP < 0.4
    ///
    /// Buying pressure has collapsed. Indicates panic selling,
    /// rug-pull execution, or total loss of interest.
    /// Action: Exit immediately or avoid entry.
    Implosion,

    /// Decline: 0.4 ≤ SOBP < 0.8
    ///
    /// Declining pressure but not catastrophic. May indicate
    /// early dump setup or natural cooldown after pump.
    /// Action: Reduce position or wait for confirmation.
    Decline,

    /// Stagnation: 0.8 ≤ SOBP < 1.5
    ///
    /// Stable buying pressure with no clear direction.
    /// May indicate consolidation or sideways movement.
    /// Action: Monitor for breakout signals.
    Stagnation,

    /// Growth: 1.5 ≤ SOBP < 3.0
    ///
    /// Stable organic growth with increasing buying pressure.
    /// Bullish signal indicating sustained demand.
    /// Action: Hold or add to position.
    Growth,

    /// Hyper-aggressive: SOBP ≥ 3.0
    ///
    /// Ultra-pump detected. Explosive buying influx indicating
    /// viral launch or coordinated pump onset.
    /// Action: Immediate entry or take profits if already in.
    Hyper,
}

impl PressureState {
    /// Creates a PressureState from an SOBP value.
    ///
    /// # Arguments
    ///
    /// * `sobp` - SOBP ratio value
    ///
    /// # Returns
    ///
    /// Classified pressure state based on threshold boundaries.
    ///
    /// # Performance
    ///
    /// O(1) operation with simple threshold comparisons.
    #[inline]
    pub fn from_sobp(sobp: f32) -> Self {
        if sobp >= SOBP_THRESHOLD_HYPER {
            PressureState::Hyper
        } else if sobp >= SOBP_THRESHOLD_GROWTH {
            PressureState::Growth
        } else if sobp >= SOBP_THRESHOLD_STAGNATION {
            PressureState::Stagnation
        } else if sobp >= SOBP_THRESHOLD_IMPLOSION {
            PressureState::Decline
        } else {
            PressureState::Implosion
        }
    }

    /// Returns true if the pressure state is bullish (Growth or Hyper).
    #[inline]
    pub fn is_bullish(&self) -> bool {
        matches!(self, PressureState::Growth | PressureState::Hyper)
    }

    /// Returns true if the pressure state is bearish (Decline or Implosion).
    #[inline]
    pub fn is_bearish(&self) -> bool {
        matches!(self, PressureState::Decline | PressureState::Implosion)
    }

    /// Returns a human-readable description of the pressure state.
    pub fn description(&self) -> &'static str {
        match self {
            PressureState::Implosion => "Demand implosion - panic/dump setup",
            PressureState::Decline => "Pressure decline - early rug behavior",
            PressureState::Stagnation => "Stagnation - no clear direction",
            PressureState::Growth => "Stable organic growth - bullish",
            PressureState::Hyper => "Ultra pump detected - viral influx",
        }
    }
}

// =============================================================================
// SOBP Result
// =============================================================================

/// Result structure for SOBP analysis, designed for QOFSV integration.
///
/// Contains SOBP calculation result, pressure state classification,
/// and metadata for decision-making in the Oracle pipeline.
///
/// # Thread Safety
///
/// Implements `Clone + Copy + Send + Sync` for concurrent usage.
///
/// # Integration
///
/// This structure is designed to be consumed by QOFSV (Quantum Oracle
/// Final Signal Verification) and other Oracle components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SobpResult {
    /// SOBP ratio value (Current / Previous)
    pub sobp_value: f32,

    /// Classified pressure state
    pub pressure_state: PressureState,

    /// Current slot number
    pub current_slot: u64,

    /// Current slot buy intensity
    pub current_intensity: f32,

    /// Previous slot buy intensity
    pub previous_intensity: f32,

    /// Number of buy transactions in current slot
    pub buy_count: u32,

    /// Confidence score (0.0-1.0)
    ///
    /// Based on data quality and slot history depth.
    /// Higher values indicate more reliable signal.
    pub confidence: f32,
}

impl SobpResult {
    /// Creates a new SobpResult with calculated confidence.
    ///
    /// # Arguments
    ///
    /// * `sobp_value` - SOBP ratio
    /// * `current_slot` - Current slot number
    /// * `current_intensity` - Current slot buy intensity
    /// * `previous_intensity` - Previous slot buy intensity
    /// * `buy_count` - Number of buy transactions in current slot
    /// * `slot_history_depth` - Number of slots in history buffer
    ///
    /// # Returns
    ///
    /// SobpResult with calculated confidence based on data quality.
    pub fn new(
        sobp_value: f32,
        current_slot: u64,
        current_intensity: f32,
        previous_intensity: f32,
        buy_count: u32,
        slot_history_depth: usize,
    ) -> Self {
        let pressure_state = PressureState::from_sobp(sobp_value);

        // Confidence calculation based on:
        // 1. Slot history depth (more history = higher confidence)
        // 2. Transaction count (more txs = higher confidence)
        // 3. Intensity magnitude (higher values = more significant signal)

        let history_factor = (slot_history_depth as f32 / CONFIDENCE_MAX_HISTORY_SLOTS).min(1.0);
        let tx_factor = (buy_count as f32 / CONFIDENCE_MIN_TX_COUNT).min(1.0);
        let intensity_factor =
            ((current_intensity + previous_intensity) / CONFIDENCE_INTENSITY_NORMALIZER).min(1.0);

        let confidence = (history_factor * CONFIDENCE_WEIGHT_HISTORY
            + tx_factor * CONFIDENCE_WEIGHT_TX_COUNT
            + intensity_factor * CONFIDENCE_WEIGHT_INTENSITY)
            .max(0.1)
            .min(1.0);

        Self {
            sobp_value,
            pressure_state,
            current_slot,
            current_intensity,
            previous_intensity,
            buy_count,
            confidence,
        }
    }
}

impl Default for SobpResult {
    fn default() -> Self {
        Self {
            sobp_value: 1.0,
            pressure_state: PressureState::Stagnation,
            current_slot: 0,
            current_intensity: 0.0,
            previous_intensity: 0.0,
            buy_count: 0,
            confidence: 0.0,
        }
    }
}

// =============================================================================
// Slot Metrics
// =============================================================================

/// Stores weighted buy intensity metrics for a single slot.
///
/// This structure accumulates weighted transaction data within a slot window.
/// All operations are stack-allocated with zero heap usage.
///
/// # Thread Safety
///
/// Implements `Copy + Clone + Send + Sync` for concurrent usage.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotMetrics {
    /// Slot number (Solana slot)
    pub slot: u64,

    /// Accumulated weighted buy intensity
    ///
    /// Sum of all buy transaction weights in this slot.
    /// Formula: Σ(base_weight * actor_multiplier) for all buys
    pub buy_intensity: f32,

    /// Number of buy transactions recorded in this slot
    pub buy_count: u32,

    /// Number of sell transactions recorded in this slot (for future extensions)
    pub sell_count: u32,

    /// Total number of transactions (buy + sell)
    pub total_count: u32,
}

impl SlotMetrics {
    /// Creates a new empty SlotMetrics for the given slot.
    ///
    /// # Arguments
    ///
    /// * `slot` - Solana slot number
    ///
    /// # Returns
    ///
    /// A new SlotMetrics with zero intensity and counts.
    #[inline]
    pub fn new(slot: u64) -> Self {
        Self {
            slot,
            buy_intensity: 0.0,
            buy_count: 0,
            sell_count: 0,
            total_count: 0,
        }
    }

    /// Adds a weighted buy transaction to this slot.
    ///
    /// # Arguments
    ///
    /// * `weight` - Pre-calculated transaction weight
    ///
    /// # Performance
    ///
    /// O(1) operation with no heap allocation.
    #[inline]
    pub fn add_buy(&mut self, weight: f32) {
        self.buy_intensity += weight;
        self.buy_count += 1;
        self.total_count += 1;
    }

    /// Adds a sell transaction to this slot (for future SOBP extensions).
    ///
    /// Currently only tracks count, not weighted intensity.
    #[inline]
    pub fn add_sell(&mut self) {
        self.sell_count += 1;
        self.total_count += 1;
    }

    /// Returns true if this slot has any buy activity.
    #[inline]
    pub fn has_buys(&self) -> bool {
        self.buy_count > 0
    }

    /// Returns the buy intensity clamped to minimum threshold.
    ///
    /// Prevents division by zero in SOBP calculations.
    #[inline]
    pub fn safe_buy_intensity(&self) -> f32 {
        self.buy_intensity.max(MIN_BUY_INTENSITY)
    }
}

// =============================================================================
// Circular Buffer
// =============================================================================

/// Zero-copy circular buffer for slot history with fixed capacity.
///
/// Stores the last N slot metrics using a ring buffer architecture.
/// All data is stack-allocated with no heap usage, enabling ultra-fast
/// slot transitions and O(1) random access.
///
/// # Generic Parameters
///
/// * `N` - Buffer capacity (number of slots to track). Must be power of 2
///         for efficient modulo operations. Default is 64 slots (~25-30 seconds).
///
/// # Performance
///
/// - **Insertion**: O(1)
/// - **Lookup**: O(1)
/// - **Memory**: Stack-only, N * sizeof(SlotMetrics) bytes
///
/// # Thread Safety
///
/// Implements `Send + Sync` for concurrent usage.
#[derive(Debug, Clone)]
pub struct CircularBuffer<const N: usize> {
    /// Ring buffer of slot metrics (stack-allocated)
    buffer: [SlotMetrics; N],

    /// Current write position in the buffer
    head: usize,

    /// Number of valid entries (0 to N)
    count: usize,

    /// Highest slot number seen (for monotonicity validation)
    max_slot: u64,
}

impl<const N: usize> CircularBuffer<N> {
    /// Creates a new empty circular buffer.
    ///
    /// # Panics
    ///
    /// Panics if N is not a power of 2 or is zero.
    pub fn new() -> Self {
        assert!(N > 0, "CircularBuffer capacity must be greater than 0");
        assert!(
            N.is_power_of_two(),
            "CircularBuffer capacity must be power of 2"
        );

        Self {
            buffer: [SlotMetrics::default(); N],
            head: 0,
            count: 0,
            max_slot: 0,
        }
    }

    /// Gets or creates a mutable reference to the metrics for the given slot.
    ///
    /// If the slot is new and later than max_slot, advances the buffer.
    /// If the slot already exists in the buffer, returns existing metrics.
    ///
    /// # Arguments
    ///
    /// * `slot` - Solana slot number
    ///
    /// # Returns
    ///
    /// Mutable reference to SlotMetrics for the given slot.
    ///
    /// # Performance
    ///
    /// O(1) operation using modulo arithmetic on power-of-2 capacity.
    #[inline]
    pub fn get_or_create_mut(&mut self, slot: u64) -> &mut SlotMetrics {
        // Handle first slot
        if self.count == 0 {
            self.max_slot = slot;
            self.buffer[0] = SlotMetrics::new(slot);
            self.count = 1;
            return &mut self.buffer[0];
        }

        // Check if slot already exists in buffer
        if slot <= self.max_slot && slot > self.max_slot.saturating_sub(self.count as u64) {
            // Slot is within current window), find it
            let offset = (self.max_slot - slot) as usize;
            if offset < self.count {
                let idx = (self.head + N - offset) % N;
                if self.buffer[idx].slot == slot {
                    return &mut self.buffer[idx];
                }
            }
        }

        // New slot or out of range - advance buffer
        if slot > self.max_slot {
            // Advance head position
            self.head = (self.head + 1) % N;
            self.buffer[self.head] = SlotMetrics::new(slot);
            self.max_slot = slot;

            if self.count < N {
                self.count += 1;
            }

            &mut self.buffer[self.head]
        } else {
            // Old slot - in production this shouldn't happen as slots are monotonically increasing
            // In debug mode, we catch this with an assertion to prevent silent data corruption
            #[cfg(debug_assertions)]
            {
                panic!(
                    "Attempted to insert old slot {} when max_slot is {}",
                    slot, self.max_slot
                );
            }

            // In release mode, we try to find and return the existing slot if it's still in buffer
            // This prevents buffer corruption while handling edge cases gracefully
            #[cfg(not(debug_assertions))]
            {
                // Check if the old slot is still in the buffer window
                if slot > self.max_slot.saturating_sub(self.count as u64) {
                    let offset = (self.max_slot - slot) as usize;
                    let idx = (self.head + N - offset) % N;
                    if self.buffer[idx].slot == slot {
                        // Slot exists, return it
                        return &mut self.buffer[idx];
                    }
                }

                // Slot too old or not found - return a dummy slot at head
                // This is degraded behavior but prevents corruption
                // The data will be ignored as it's not properly tracked
                self.buffer[self.head] = SlotMetrics::new(slot);
                &mut self.buffer[self.head]
            }
        }
    }

    /// Gets immutable reference to metrics for a specific slot.
    ///
    /// # Arguments
    ///
    /// * `slot` - Solana slot number to query
    ///
    /// # Returns
    ///
    /// Some(&SlotMetrics) if slot exists in buffer, None otherwise.
    ///
    /// # Performance
    ///
    /// O(1) lookup with single modulo operation.
    #[inline]
    pub fn get(&self, slot: u64) -> Option<&SlotMetrics> {
        if self.count == 0 || slot > self.max_slot {
            return None;
        }

        // Check if slot is within valid window
        if slot <= self.max_slot.saturating_sub(self.count as u64) {
            return None;
        }

        let offset = (self.max_slot - slot) as usize;
        if offset >= self.count {
            return None;
        }

        let idx = (self.head + N - offset) % N;
        let metrics = &self.buffer[idx];

        if metrics.slot == slot {
            Some(metrics)
        } else {
            None
        }
    }

    /// Returns the number of slots currently stored in the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns true if the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns the highest slot number seen.
    #[inline]
    pub fn max_slot(&self) -> u64 {
        self.max_slot
    }
}

impl<const N: usize> Default for CircularBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Transaction Record
// =============================================================================

/// Minimal transaction record for SOBP processing.
///
/// Contains only the essential fields needed for event-time bucketing and weighting.
/// Designed to be lightweight and zero-copy compatible.
///
/// # Thread Safety
///
/// Implements `Copy + Clone + Send + Sync`.
#[derive(Debug, Clone, Copy)]
pub struct TransactionRecord {
    /// Solana slot number when transaction was processed (metadata only)
    pub slot: Option<u64>,

    /// Actor type classification from MPCF
    pub actor_type: ActorType,

    /// Transaction amount in SOL (used when CIR weighting is provided)
    pub amount_sol: f32,

    /// Optional CIR effective weight (0.0-1.0). If provided, overrides actor weighting.
    pub cir_effective: Option<f32>,

    /// Transaction size in bytes (for future weight adjustments)
    pub tx_size_bytes: usize,

    /// True if this is a buy transaction, false if sell
    pub is_buy: bool,

    /// Timestamp in milliseconds (event time)
    pub timestamp_ms: u64,

    /// Raw transaction price (FALLBACK ONLY for early window)
    pub price: Option<f32>,
}

// =============================================================================
// SOBP Core
// =============================================================================

/// Main SOBP (Bucket-Over-Bucket Buying Pressure) calculation engine.
///
/// Orchestrates transaction bucketing, weight calculation, and SOBP ratio computation.
/// Maintains a rolling window of slot metrics with zero heap allocation.
///
/// # Generic Parameters
///
/// * `N` - Slot history capacity. Default is 64 slots (~25-30 seconds of history).
///         Must be power of 2.
///
/// # Performance
///
/// - **Transaction Recording**: <10μs
/// - **SOBP Calculation**: <5μs
/// - **Memory**: Stack-only, O(N) space
///
/// # Thread Safety
///
/// Not thread-safe by default. Wrap in `Mutex` or use message passing for
/// concurrent access.
///
/// # Example
///
/// ```rust,ignore
/// let mut sobp = SobpCore::<64>::new();
///
/// // Record transactions as they arrive
/// sobp.record_transaction(&TransactionRecord {
///     slot: Some(100),
///     actor_type: ActorType::HumanMobile,
///     amount_sol: 1.0,
///     cir_effective: None,
///     tx_size_bytes: 256,
///     is_buy: true,
///     timestamp_ms: 0,
///     price: None,
/// });
///
/// // Calculate SOBP
/// if let Some(ratio) = sobp.calculate_sobp(100) {
///     println!("SOBP = {}", ratio);
/// }
/// ```
pub struct SobpCore<const N: usize> {
    /// Circular buffer of slot metrics
    buffer: CircularBuffer<N>,

    // =========================================================================
    // Time-Based Early Window State (Event Time)
    // =========================================================================
    /// Timestamp of the first observed event (ms)
    pub first_event_ts_ms: u64,

    /// Total number of events processed
    pub event_count: usize,

    /// Minimum window duration for early phase (ms)
    /// Default: 10,000ms (10s)
    pub min_window_ms: u64,

    /// Minimum event count for early phase
    /// Default: 3
    pub min_events: usize,

    /// Last known raw transaction price (FALLBACK CACHE ONLY)
    ///
    /// CRITICAL: This is NOT a calculated price. It is strictly a cache of the
    /// last raw transaction price saw, used ONLY for fallback during the
    /// early window phase when SnapshotEngine aggregation is not yet reliable.
    pub last_known_price: Option<f32>,
}

impl<const N: usize> SobpCore<N> {
    /// Creates a new SOBP core with empty slot history.
    ///
    /// # Panics
    ///
    /// Panics if N is not a power of 2 or is zero.
    pub fn new() -> Self {
        Self {
            buffer: CircularBuffer::new(),
            first_event_ts_ms: 0,
            event_count: 0,
            min_window_ms: 2_000,
            min_events: 3,
            last_known_price: None,
        }
    }

    /// Records a transaction and updates the appropriate slot metrics.
    ///
    /// Calculates transaction weight based on actor type and updates the
    /// buy intensity for the transaction's slot.
    ///
    /// # Arguments
    ///
    /// * `tx` - Transaction record to process
    ///
    /// # Performance
    ///
    /// O(1) operation with zero heap allocation.
    #[inline]
    pub fn record_transaction(&mut self, tx: &TransactionRecord) {
        // Update Early Window state
        if self.first_event_ts_ms == 0 {
            self.first_event_ts_ms = tx.timestamp_ms;
        }
        self.event_count += 1;

        // Event-time bucketing (slot metadata is ignored for windowing)
        let bucket_id = tx.timestamp_ms / SOBP_BUCKET_MS.max(1);

        // Cache fallback price if available (CACHE ONLY, NO CALCULATION)
        if let Some(price) = tx.price {
            self.last_known_price = Some(price);
        }

        if !tx.is_buy {
            // For now, only track buys (sells can be added later)
            let metrics = self.buffer.get_or_create_mut(bucket_id);
            metrics.add_sell();
            return;
        }

        let weight = Self::calculate_weight(tx);
        let metrics = self.buffer.get_or_create_mut(bucket_id);
        metrics.add_buy(weight);
    }

    /// Calculates the weighted value for a transaction based on actor type.
    ///
    /// # Weight Multipliers
    ///
    /// - **HumanMobile, HumanDesktop**: 2.0x (organic activity amplification)
    /// - **SniperScript**: 0.5x (bot noise reduction)
    /// - **Other actors**: 1.0x (neutral weight)
    ///
    /// # Arguments
    ///
    /// * `tx` - Transaction record
    ///
    /// # Returns
    ///
    /// Weighted value for the transaction.
    ///
    /// # Performance
    ///
    /// Pure computation, <1μs execution time.
    #[inline]
    pub fn calculate_weight(tx: &TransactionRecord) -> f32 {
        if let Some(cir_effective) = tx.cir_effective {
            let cir = cir_effective.max(0.0);
            let volume_weight = tx.amount_sol * cir;
            let baseline_weight = BASE_TRANSACTION_WEIGHT * cir;
            return (volume_weight + baseline_weight).max(0.0);
        }

        let multiplier = match tx.actor_type {
            ActorType::HumanMobile | ActorType::HumanDesktop => HUMAN_WEIGHT_MULTIPLIER,
            ActorType::SniperScript => SNIPER_WEIGHT_MULTIPLIER,
            _ => DEFAULT_WEIGHT_MULTIPLIER,
        };

        BASE_TRANSACTION_WEIGHT * multiplier
    }

    /// Calculates SOBP ratio for the given slot.
    ///
    /// Formula: SOBP(slot) = BuyIntensity(slot) / BuyIntensity(slot-1)
    ///
    /// # Arguments
    ///
    /// * `slot` - Current slot to calculate SOBP for
    ///
    /// # Returns
    ///
    /// - `Some(ratio)` if current slot exists
    /// - `None` if current slot doesn't exist or slot is 0
    ///
    /// # Special Cases
    ///
    /// - If previous slot missing, uses BASELINE_INTENSITY (1.0) for neutral reference
    /// - If previous slot has zero recorded intensity, uses MIN_BUY_INTENSITY to avoid division by zero
    /// - If current slot has zero recorded intensity, returns MIN_BUY_INTENSITY / previous_intensity
    ///   (effectively near-zero ratio indicating buying pressure collapse)
    ///
    /// # Performance
    ///
    /// O(1) with two buffer lookups and one division.
    pub fn calculate_sobp(&self, slot: u64) -> Option<f32> {
        if slot == 0 {
            return None;
        }

        let current = self.buffer.get(slot)?;
        let previous = self.buffer.get(slot - 1);

        let current_intensity = current.safe_buy_intensity();

        // If no previous slot, treat as baseline neutral pressure
        let previous_intensity = previous
            .map(|m| m.safe_buy_intensity())
            .unwrap_or(BASELINE_INTENSITY);

        let sobp_ratio = current_intensity / previous_intensity;

        Some(clamp_sobp_ratio(sobp_ratio))
    }

    /// Gets metrics for a specific slot.
    ///
    /// # Arguments
    ///
    /// * `slot` - Slot number to query
    ///
    /// # Returns
    ///
    /// Some(&SlotMetrics) if slot exists, None otherwise.
    #[inline]
    pub fn get_slot_metrics(&self, slot: u64) -> Option<&SlotMetrics> {
        self.buffer.get(slot)
    }

    /// Returns the highest slot number currently tracked.
    #[inline]
    pub fn current_slot(&self) -> u64 {
        self.buffer.max_slot()
    }

    /// Returns the number of slots currently in history.
    #[inline]
    pub fn slot_count(&self) -> usize {
        self.buffer.len()
    }

    /// Calculates SOBP for the most recent slot.
    ///
    /// Convenience method equivalent to `calculate_sobp(current_slot())`.
    ///
    /// # Returns
    ///
    /// SOBP ratio for the latest slot, or None if insufficient history.
    pub fn current_sobp(&self) -> Option<f32> {
        let current = self.current_slot();
        if current == 0 {
            return None;
        }
        self.calculate_sobp(current)
    }

    /// Generates a complete SobpResult for the given slot.
    ///
    /// This is the main output method for integration with QOFSV and other
    /// Oracle components. Includes SOBP calculation, pressure state classification,
    /// and confidence scoring.
    ///
    /// # Arguments
    ///
    /// * `slot` - Slot number to analyze
    ///
    /// # Returns
    ///
    /// `Some(SobpResult)` if slot exists and can be analyzed, `None` otherwise.
    ///
    /// # Performance
    ///
    /// O(1) operation with minimal overhead beyond SOBP calculation.
    pub fn analyze_slot(&self, slot: u64) -> Option<SobpResult> {
        if slot == 0 {
            return None;
        }

        let current = self.buffer.get(slot)?;
        let previous = self.buffer.get(slot - 1);

        let current_intensity = current.safe_buy_intensity();
        let previous_intensity = previous
            .map(|m| m.safe_buy_intensity())
            .unwrap_or(BASELINE_INTENSITY);

        let sobp_value = clamp_sobp_ratio(current_intensity / previous_intensity);

        Some(SobpResult::new(
            sobp_value,
            slot,
            current_intensity,
            previous_intensity,
            current.buy_count,
            self.buffer.len(),
        ))
    }

    /// Generates a complete SobpResult for the current (most recent) slot.
    ///
    /// Convenience method for analyzing the latest slot.
    ///
    /// # Returns
    ///
    /// `Some(SobpResult)` if current slot can be analyzed, `None` otherwise.
    pub fn analyze_current(&self) -> Option<SobpResult> {
        let current = self.current_slot();
        if current == 0 {
            return None;
        }
        self.analyze_slot(current)
    }

    /// Calculates Simple Moving Average (SMA) of SOBP values over a window.
    ///
    /// This optional smoothing reduces noise in SOBP signals, useful for
    /// filtering out transient spikes or drops in buying pressure.
    ///
    /// # Arguments
    ///
    /// * `slot` - Current slot to calculate SMA for
    /// * `window_size` - Number of slots to average (must be ≥ 1)
    ///
    /// # Returns
    ///
    /// `Some(f32)` with smoothed SOBP value, `None` if insufficient data.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // 3-slot SMA smoothing
    /// if let Some(smoothed_sobp) = sobp.calculate_sobp_sma(105, 3) {
    ///     println!("Smoothed SOBP: {}", smoothed_sobp);
    /// }
    /// ```
    ///
    /// # Performance
    ///
    /// O(window_size) with stack-allocated accumulation.
    pub fn calculate_sobp_sma(&self, slot: u64, window_size: usize) -> Option<f32> {
        if window_size == 0 || slot == 0 {
            return None;
        }

        let start_slot = slot.saturating_sub(window_size as u64 - 1);
        let mut sum = 0.0f32;
        let mut count = 0usize;

        for s in start_slot..=slot {
            if let Some(sobp) = self.calculate_sobp(s) {
                sum += sobp;
                count += 1;
            }
        }

        if count == 0 {
            None
        } else {
            Some(sum / count as f32)
        }
    }

    /// Generates SobpResult with optional SMA smoothing.
    ///
    /// Combines SOBP calculation with optional SMA smoothing for noise reduction.
    ///
    /// # Arguments
    ///
    /// * `slot` - Slot number to analyze
    /// * `sma_window` - Optional window size for SMA smoothing. If `None`, no smoothing is applied.
    ///
    /// # Returns
    ///
    /// `Some(SobpResult)` with potentially smoothed SOBP value, `None` if insufficient data.
    ///
    /// # Performance
    ///
    /// O(sma_window) when smoothing is enabled, O(1) otherwise.
    pub fn analyze_slot_smoothed(
        &self,
        slot: u64,
        sma_window: Option<usize>,
    ) -> Option<SobpResult> {
        if slot == 0 {
            return None;
        }

        let current = self.buffer.get(slot)?;
        let previous = self.buffer.get(slot - 1);

        let current_intensity = current.safe_buy_intensity();
        let previous_intensity = previous
            .map(|m| m.safe_buy_intensity())
            .unwrap_or(BASELINE_INTENSITY);

        // Calculate raw SOBP with early clamp to keep SMA inputs bounded
        let raw_sobp = clamp_sobp_ratio(current_intensity / previous_intensity);

        // Apply smoothing if requested
        let sobp_value = if let Some(window) = sma_window {
            self.calculate_sobp_sma(slot, window).unwrap_or(raw_sobp)
        } else {
            raw_sobp
        };

        Some(SobpResult::new(
            sobp_value,
            slot,
            current_intensity,
            previous_intensity,
            current.buy_count,
            self.buffer.len(),
        ))
    }

    // =========================================================================
    // Event-Time Early Window API (FAZA 5: Event-Time Only)
    // =========================================================================

    /// Determines if SOBP is in the early window phase.
    ///
    /// Early window = insufficient data for reliable SOBP calculation.
    /// During early window, fallback to raw transaction prices should be used.
    ///
    /// # Arguments
    /// * `now_ts_ms` - Current timestamp in milliseconds
    ///
    /// # Returns
    /// `true` if in early window (too little time OR too few events)
    ///
    /// # Logic
    /// Early window when EITHER:
    /// - elapsed_ms < min_window_ms (default 10s)
    /// - event_count < min_events (default 3)
    ///
    /// # Note
    /// This method uses ONLY event-time (`event_ts_ms`) for decisions.
    /// Slot is NOT used for early window detection (per FAZA 5 requirements).
    #[inline]
    pub fn is_early_window(&self, now_ts_ms: u64) -> bool {
        // If no events yet, always early window
        if self.first_event_ts_ms == 0 {
            return true;
        }

        let elapsed_ms = now_ts_ms.saturating_sub(self.first_event_ts_ms);
        let too_little_time = elapsed_ms < self.min_window_ms;
        let too_few_events = self.event_count < self.min_events;

        too_little_time || too_few_events
    }

    /// Gets the fallback price for early window scenarios.
    ///
    /// Returns the last known raw transaction price, which should be used
    /// when `is_early_window()` returns `true`.
    ///
    /// # Returns
    /// `Some(price)` if any transaction with price has been recorded
    /// `None` if no price data is available
    ///
    /// # Usage
    /// ```ignore
    /// if sobp.is_early_window(now_ts_ms) {
    ///     // Use fallback price from raw transactions
    ///     if let Some(price) = sobp.get_fallback_price() {
    ///         return price;
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn get_fallback_price(&self) -> Option<f32> {
        self.last_known_price
    }
}

impl<const N: usize> Default for SobpCore<N> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_metrics_creation() {
        let metrics = SlotMetrics::new(100);
        assert_eq!(metrics.slot, 100);
        assert_eq!(metrics.buy_intensity, 0.0);
        assert_eq!(metrics.buy_count, 0);
        assert_eq!(metrics.sell_count, 0);
        assert_eq!(metrics.total_count, 0);
    }

    #[test]
    fn test_slot_metrics_add_buy() {
        let mut metrics = SlotMetrics::new(100);
        metrics.add_buy(1.5);

        assert_eq!(metrics.buy_intensity, 1.5);
        assert_eq!(metrics.buy_count, 1);
        assert_eq!(metrics.total_count, 1);

        metrics.add_buy(2.0);
        assert_eq!(metrics.buy_intensity, 3.5);
        assert_eq!(metrics.buy_count, 2);
        assert_eq!(metrics.total_count, 2);
    }

    #[test]
    fn test_slot_metrics_safe_buy_intensity() {
        let mut metrics = SlotMetrics::new(100);
        assert_eq!(metrics.safe_buy_intensity(), MIN_BUY_INTENSITY);

        metrics.add_buy(5.0);
        assert_eq!(metrics.safe_buy_intensity(), 5.0);
    }

    #[test]
    fn test_circular_buffer_creation() {
        let buffer = CircularBuffer::<64>::new();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
        assert_eq!(buffer.max_slot(), 0);
    }

    #[test]
    fn test_circular_buffer_single_slot() {
        let mut buffer = CircularBuffer::<64>::new();
        let metrics = buffer.get_or_create_mut(100);
        metrics.add_buy(1.0);

        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.max_slot(), 100);

        let retrieved = buffer.get(100).unwrap();
        assert_eq!(retrieved.slot, 100);
        assert_eq!(retrieved.buy_intensity, 1.0);
    }

    #[test]
    fn test_circular_buffer_sequential_slots() {
        let mut buffer = CircularBuffer::<64>::new();

        for slot in 100..110 {
            let metrics = buffer.get_or_create_mut(slot);
            metrics.add_buy(slot as f32);
        }

        assert_eq!(buffer.len(), 10);
        assert_eq!(buffer.max_slot(), 109);

        // Verify all slots exist
        for slot in 100..110 {
            let metrics = buffer.get(slot).unwrap();
            assert_eq!(metrics.slot, slot);
            assert_eq!(metrics.buy_intensity, slot as f32);
        }
    }

    #[test]
    fn test_circular_buffer_wraparound() {
        let mut buffer = CircularBuffer::<8>::new();

        // Fill buffer beyond capacity
        for slot in 100..120 {
            let metrics = buffer.get_or_create_mut(slot);
            metrics.add_buy(slot as f32);
        }

        // Buffer should only hold last 8 slots
        assert_eq!(buffer.len(), 8);
        assert_eq!(buffer.max_slot(), 119);

        // Old slots should be gone
        assert!(buffer.get(100).is_none());
        assert!(buffer.get(110).is_none());

        // Recent slots should exist
        for slot in 112..120 {
            let metrics = buffer.get(slot).unwrap();
            assert_eq!(metrics.slot, slot);
        }
    }

    #[test]
    fn test_circular_buffer_same_slot_update() {
        let mut buffer = CircularBuffer::<64>::new();

        let metrics = buffer.get_or_create_mut(100);
        metrics.add_buy(1.0);

        let metrics = buffer.get_or_create_mut(100);
        metrics.add_buy(2.0);

        // Should have updated the same slot
        assert_eq!(buffer.len(), 1);
        let retrieved = buffer.get(100).unwrap();
        assert_eq!(retrieved.buy_intensity, 3.0);
        assert_eq!(retrieved.buy_count, 2);
    }

    #[test]
    fn test_weight_calculation_human() {
        let tx = TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            timestamp_ms: 0,
            price: None,
        };

        let weight = SobpCore::<64>::calculate_weight(&tx);
        assert_eq!(weight, BASE_TRANSACTION_WEIGHT * HUMAN_WEIGHT_MULTIPLIER);
    }

    #[test]
    fn test_weight_calculation_sniper() {
        let tx = TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::SniperScript,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            timestamp_ms: 0,
            price: None,
        };

        let weight = SobpCore::<64>::calculate_weight(&tx);
        assert_eq!(weight, BASE_TRANSACTION_WEIGHT * SNIPER_WEIGHT_MULTIPLIER);
    }

    #[test]
    fn test_weight_calculation_default() {
        let tx = TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::MEVArb,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            timestamp_ms: 0,
            price: None,
        };

        let weight = SobpCore::<64>::calculate_weight(&tx);
        assert_eq!(weight, BASE_TRANSACTION_WEIGHT * DEFAULT_WEIGHT_MULTIPLIER);
    }

    #[test]
    fn test_sobp_core_record_transaction() {
        let mut sobp = SobpCore::<64>::new();

        let tx = TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            timestamp_ms: 0,
            price: None,
        };

        sobp.record_transaction(&tx);

        assert_eq!(sobp.slot_count(), 1);
        assert_eq!(sobp.current_slot(), 100);

        let metrics = sobp.get_slot_metrics(100).unwrap();
        assert_eq!(metrics.buy_count, 1);
        assert_eq!(metrics.buy_intensity, 2.0); // HUMAN_WEIGHT_MULTIPLIER
    }

    #[test]
    fn test_sobp_calculation_basic() {
        let mut sobp = SobpCore::<64>::new();

        // Slot 100: 3 buys
        for _ in 0..3 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 101: 11 buys
        for _ in 0..11 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        let sobp_value = sobp.calculate_sobp(101).unwrap();

        // Expected: (11 * 2.0) / (3 * 2.0) = 22.0 / 6.0 = 3.666...
        assert!((sobp_value - 3.666).abs() < 0.01);
    }

    #[test]
    fn test_sobp_ratio_clamped_on_extreme_growth() {
        let mut sobp = SobpCore::<64>::new();

        // Create previous slot with minimal buy intensity (sell-only)
        sobp.record_transaction(&TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: false,
            timestamp_ms: 0,
            price: None,
        });

        // Current slot with strong buy burst
        for _ in 0..50 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        let sobp_value = sobp.calculate_sobp(101).unwrap();
        assert_eq!(sobp_value, SOBP_RATIO_MAX);

        let analyzed = sobp.analyze_slot(101).unwrap();
        assert_eq!(analyzed.sobp_value, SOBP_RATIO_MAX);
        assert_eq!(analyzed.pressure_state, PressureState::Hyper);
    }

    #[test]
    fn test_sobp_ratio_clamped_on_negative_input() {
        // Directly validate helper for malformed negative ratios
        let clamped = clamp_sobp_ratio(-5.0);
        assert_eq!(clamped, SOBP_RATIO_MIN);
    }

    #[test]
    fn test_sobp_ratio_invalid_returns_baseline() {
        let clamped_nan = clamp_sobp_ratio(f32::NAN);
        let clamped_inf = clamp_sobp_ratio(f32::INFINITY);
        assert_eq!(clamped_nan, SOBP_RATIO_BASELINE);
        assert_eq!(clamped_inf, SOBP_RATIO_BASELINE);
    }

    #[test]
    fn test_sobp_sma_respects_clamp_bounds() {
        let mut sobp = SobpCore::<64>::new();

        // Slot 100: sell-only to create tiny baseline
        sobp.record_transaction(&TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: false,
            timestamp_ms: 0,
            price: None,
        });

        // Slot 101: huge buy burst -> raw ratio would be enormous, should clamp to MAX
        for _ in 0..120 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 102: another burst to keep SMA window populated
        for _ in 0..120 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(102),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // SMA over last 2 slots should stay within clamp bounds
        let smoothed = sobp.analyze_slot_smoothed(102, Some(2)).unwrap();
        assert!(smoothed.sobp_value <= SOBP_RATIO_MAX);
        assert!(smoothed.sobp_value.is_finite());
    }

    #[test]
    fn test_sobp_calculation_first_slot() {
        let mut sobp = SobpCore::<64>::new();

        sobp.record_transaction(&TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            timestamp_ms: 0,
            price: None,
        });

        // No previous slot, should use baseline of 1.0
        let sobp_value = sobp.calculate_sobp(100).unwrap();
        assert_eq!(sobp_value, 2.0); // 2.0 / 1.0 baseline
    }

    #[test]
    fn test_sobp_pump_scenario() {
        let mut sobp = SobpCore::<64>::new();

        // Simulate pump.fun scenario from docs
        // Slot 101: 2 BUY
        for _ in 0..2 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::HumanDesktop,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 102: 15 BUY
        for _ in 0..15 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(102),
                actor_type: ActorType::HumanDesktop,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 103: 19 BUY
        for _ in 0..19 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(103),
                actor_type: ActorType::HumanDesktop,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 104: 6 BUY
        for _ in 0..6 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(104),
                actor_type: ActorType::HumanDesktop,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // SOBP(102) = 15 / 2 = 7.5 → ultra pump trigger
        let sobp_102 = sobp.calculate_sobp(102).unwrap();
        assert!((sobp_102 - 7.5).abs() < 0.01);
        assert!(sobp_102 > 3.0, "Should trigger ultra pump");

        // SOBP(103) = 19 / 15 = 1.27 → still pumping
        let sobp_103 = sobp.calculate_sobp(103).unwrap();
        assert!((sobp_103 - 1.266).abs() < 0.01);
        assert!(sobp_103 > 1.0 && sobp_103 < 1.5);

        // SOBP(104) = 6 / 19 = 0.31 → early dump
        let sobp_104 = sobp.calculate_sobp(104).unwrap();
        assert!((sobp_104 - 0.315).abs() < 0.01);
        assert!(sobp_104 < 0.4, "Should indicate dump");
    }

    #[test]
    fn test_sobp_mixed_actors() {
        let mut sobp = SobpCore::<64>::new();

        // Slot 100: 2 humans (2 * 2.0 = 4.0)
        for _ in 0..2 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 101: 1 human (2.0) + 4 bots (4 * 0.5 = 2.0) = 4.0 total
        sobp.record_transaction(&TransactionRecord {
            slot: Some(101),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            timestamp_ms: 0,
            price: None,
        });
        for _ in 0..4 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::SniperScript,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // SOBP should be 1.0 (same weighted intensity)
        let sobp_value = sobp.calculate_sobp(101).unwrap();
        assert!((sobp_value - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_sobp_zero_intensity() {
        let mut sobp = SobpCore::<64>::new();

        sobp.record_transaction(&TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            timestamp_ms: 0,
            price: None,
        });

        // Slot 101 has no transactions
        // Should still calculate based on MIN_BUY_INTENSITY
        let sobp_value = sobp.calculate_sobp(101);
        assert!(sobp_value.is_none()); // Slot 101 doesn't exist
    }

    #[test]
    fn test_sobp_current_sobp() {
        let mut sobp = SobpCore::<64>::new();

        for slot in 100..105 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(slot),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        let current = sobp.current_sobp().unwrap();
        let explicit = sobp.calculate_sobp(104).unwrap();
        assert_eq!(current, explicit);
    }

    #[test]
    fn test_sell_tracking() {
        let mut sobp = SobpCore::<64>::new();

        sobp.record_transaction(&TransactionRecord {
            slot: Some(100),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: false, // sell
            timestamp_ms: 0,
            price: None,
        });

        let metrics = sobp.get_slot_metrics(100).unwrap();
        assert_eq!(metrics.sell_count, 1);
        assert_eq!(metrics.buy_count, 0);
        assert_eq!(metrics.buy_intensity, 0.0);
    }

    // =============================================================================
    // Differential Analysis & Signal Classification Tests
    // =============================================================================

    #[test]
    fn test_pressure_state_classification() {
        // Test threshold boundaries
        assert_eq!(PressureState::from_sobp(4.0), PressureState::Hyper);
        assert_eq!(PressureState::from_sobp(3.0), PressureState::Hyper);
        assert_eq!(PressureState::from_sobp(2.5), PressureState::Growth);
        assert_eq!(PressureState::from_sobp(1.5), PressureState::Growth);
        assert_eq!(PressureState::from_sobp(1.2), PressureState::Stagnation);
        assert_eq!(PressureState::from_sobp(0.8), PressureState::Stagnation);
        assert_eq!(PressureState::from_sobp(0.6), PressureState::Decline);
        assert_eq!(PressureState::from_sobp(0.4), PressureState::Decline);
        assert_eq!(PressureState::from_sobp(0.3), PressureState::Implosion);
        assert_eq!(PressureState::from_sobp(0.1), PressureState::Implosion);
    }

    #[test]
    fn test_pressure_state_helpers() {
        assert!(PressureState::Hyper.is_bullish());
        assert!(PressureState::Growth.is_bullish());
        assert!(!PressureState::Stagnation.is_bullish());
        assert!(!PressureState::Decline.is_bullish());
        assert!(!PressureState::Implosion.is_bullish());

        assert!(PressureState::Implosion.is_bearish());
        assert!(PressureState::Decline.is_bearish());
        assert!(!PressureState::Stagnation.is_bearish());
        assert!(!PressureState::Growth.is_bearish());
        assert!(!PressureState::Hyper.is_bearish());
    }

    #[test]
    fn test_sobp_result_creation() {
        let result = SobpResult::new(
            3.5,  // SOBP value
            105,  // current slot
            14.0, // current intensity
            4.0,  // previous intensity
            7,    // buy count
            10,   // slot history depth
        );

        assert_eq!(result.sobp_value, 3.5);
        assert_eq!(result.pressure_state, PressureState::Hyper);
        assert_eq!(result.current_slot, 105);
        assert_eq!(result.current_intensity, 14.0);
        assert_eq!(result.previous_intensity, 4.0);
        assert_eq!(result.buy_count, 7);
        assert!(result.confidence > 0.0 && result.confidence <= 1.0);
    }

    #[test]
    fn test_analyze_slot() {
        let mut sobp = SobpCore::<64>::new();

        // Slot 100: 3 buys
        for _ in 0..3 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 101: 12 buys (4x increase)
        for _ in 0..12 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        let result = sobp.analyze_slot(101).unwrap();

        // Expected SOBP: (12 * 2.0) / (3 * 2.0) = 24.0 / 6.0 = 4.0
        assert!((result.sobp_value - 4.0).abs() < 0.01);
        assert_eq!(result.pressure_state, PressureState::Hyper);
        assert_eq!(result.buy_count, 12);
    }

    #[test]
    fn test_analyze_current() {
        let mut sobp = SobpCore::<64>::new();

        for slot in 100..103 {
            for _ in 0..5 {
                sobp.record_transaction(&TransactionRecord {
                    slot: Some(slot),
                    actor_type: ActorType::HumanMobile,
                    amount_sol: 1.0,
                    cir_effective: None,
                    tx_size_bytes: 256,
                    is_buy: true,
                    timestamp_ms: 0,
                    price: None,
                });
            }
        }

        let result = sobp.analyze_current().unwrap();
        assert_eq!(result.current_slot, 102);
        assert_eq!(result.sobp_value, 1.0); // Same intensity across slots
        assert_eq!(result.pressure_state, PressureState::Stagnation);
    }

    #[test]
    fn test_differential_pump_scenario() {
        // Issue requirement: "Pump" scenario with increasing intensities (3 → 11 → 18)
        let mut sobp = SobpCore::<64>::new();

        // Slot 100: 3 buys (initial)
        for _ in 0..3 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 101: 11 buys (strong growth)
        for _ in 0..11 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 102: 18 buys (hyper pump)
        for _ in 0..18 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(102),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Analyze slot 101: (11 txs * 2.0 weight) / (3 txs * 2.0 weight) = 22.0 / 6.0 = 3.666... → Hyper
        let result_101 = sobp.analyze_slot(101).unwrap();
        assert!(
            (result_101.sobp_value - 3.666).abs() < 0.01,
            "Slot 101 SOBP should be ~3.67, got {}",
            result_101.sobp_value
        );
        assert_eq!(
            result_101.pressure_state,
            PressureState::Hyper,
            "Slot 101 should be Hyper pressure"
        );

        // Analyze slot 102: (18 txs * 2.0 weight) / (11 txs * 2.0 weight) = 36.0 / 22.0 = 1.636... → Growth
        let result_102 = sobp.analyze_slot(102).unwrap();
        assert!(
            (result_102.sobp_value - 1.636).abs() < 0.01,
            "Slot 102 SOBP should be ~1.64, got {}",
            result_102.sobp_value
        );
        assert_eq!(
            result_102.pressure_state,
            PressureState::Growth,
            "Slot 102 should be Growth pressure"
        );

        // Verify intensities
        assert_eq!(result_101.buy_count, 11);
        assert_eq!(result_102.buy_count, 18);
    }

    #[test]
    fn test_differential_dump_rug_scenario() {
        // Issue requirement: "Dump/Rug" scenario with sharp decline (19 → 5 → 2)
        let mut sobp = SobpCore::<64>::new();

        // Slot 100: 19 buys (peak activity)
        for _ in 0..19 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 101: 5 buys (sharp decline)
        for _ in 0..5 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(101),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 102: 2 buys (implosion)
        for _ in 0..2 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(102),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Analyze slot 101: (5 txs * 2.0 weight) / (19 txs * 2.0 weight) = 10.0 / 38.0 = 0.263... → Implosion
        let result_101 = sobp.analyze_slot(101).unwrap();
        assert!(
            (result_101.sobp_value - 0.263).abs() < 0.01,
            "Slot 101 SOBP should be ~0.26, got {}",
            result_101.sobp_value
        );
        assert_eq!(
            result_101.pressure_state,
            PressureState::Implosion,
            "Slot 101 should be Implosion (rug detected)"
        );

        // Analyze slot 102: (2 txs * 2.0 weight) / (5 txs * 2.0 weight) = 4.0 / 10.0 = 0.4 → Decline (at boundary)
        let result_102 = sobp.analyze_slot(102).unwrap();
        assert!(
            (result_102.sobp_value - 0.4).abs() < 0.01,
            "Slot 102 SOBP should be 0.4, got {}",
            result_102.sobp_value
        );
        assert_eq!(
            result_102.pressure_state,
            PressureState::Decline,
            "Slot 102 should be Decline pressure"
        );

        // Verify both are bearish
        assert!(result_101.pressure_state.is_bearish());
        assert!(result_102.pressure_state.is_bearish());
    }

    #[test]
    fn test_sobp_sma_smoothing() {
        let mut sobp = SobpCore::<64>::new();

        // Create slots with varying intensities
        let intensities = vec![3, 11, 18, 15, 12, 10];

        for (i, &intensity) in intensities.iter().enumerate() {
            for _ in 0..intensity {
                sobp.record_transaction(&TransactionRecord {
                    slot: Some(100 + i as u64),
                    actor_type: ActorType::HumanMobile,
                    amount_sol: 1.0,
                    cir_effective: None,
                    tx_size_bytes: 256,
                    is_buy: true,
                    timestamp_ms: 0,
                    price: None,
                });
            }
        }

        // Calculate 3-slot SMA for slot 105
        let sma = sobp.calculate_sobp_sma(105, 3).unwrap();

        // Manual calculation with HumanMobile weight (2.0):
        // Slot 103: (15 * 2.0) / (18 * 2.0) = 30.0 / 36.0 = 0.833
        // Slot 104: (12 * 2.0) / (15 * 2.0) = 24.0 / 30.0 = 0.8
        // Slot 105: (10 * 2.0) / (12 * 2.0) = 20.0 / 24.0 = 0.833
        // SMA = (0.833 + 0.8 + 0.833) / 3 = 0.822

        assert!(
            (sma - 0.822).abs() < 0.01,
            "3-slot SMA should be ~0.82, got {}",
            sma
        );
    }

    #[test]
    fn test_analyze_slot_with_smoothing() {
        let mut sobp = SobpCore::<64>::new();

        // Create noisy signal with spike
        let intensities = vec![5, 5, 5, 20, 6, 6]; // Spike at slot 103

        for (i, &intensity) in intensities.iter().enumerate() {
            for _ in 0..intensity {
                sobp.record_transaction(&TransactionRecord {
                    slot: Some(100 + i as u64),
                    actor_type: ActorType::HumanMobile,
                    amount_sol: 1.0,
                    cir_effective: None,
                    tx_size_bytes: 256,
                    is_buy: true,
                    timestamp_ms: 0,
                    price: None,
                });
            }
        }

        // Raw SOBP at slot 103 (spike): (20 * 2.0) / (5 * 2.0) = 40.0 / 10.0 = 4.0 → Hyper
        let raw_result = sobp.analyze_slot(103).unwrap();
        assert_eq!(raw_result.pressure_state, PressureState::Hyper);

        // Smoothed SOBP at slot 104 using 3-slot window reduces spike effect
        let smoothed_result = sobp.analyze_slot_smoothed(104, Some(3)).unwrap();

        // The smoothed value should be less extreme than raw spike
        assert!(
            smoothed_result.sobp_value < 4.0,
            "Smoothed SOBP should be less than raw spike"
        );
    }

    #[test]
    fn test_zero_division_handling() {
        let mut sobp = SobpCore::<64>::new();

        // Slot 100: 5 buys
        for _ in 0..5 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Slot 101: 0 buys (sell-only slot)
        sobp.record_transaction(&TransactionRecord {
            slot: Some(101),
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: false,
            timestamp_ms: 0,
            price: None,
        });

        // Should handle zero intensity gracefully using MIN_BUY_INTENSITY
        let result = sobp.analyze_slot(101);
        assert!(result.is_some(), "Should handle zero-buy slot");

        let result = result.unwrap();
        assert!(
            result.sobp_value < 0.01,
            "Zero buys should produce very low SOBP"
        );
        assert_eq!(result.pressure_state, PressureState::Implosion);
    }

    #[test]
    fn test_baseline_intensity_first_slot() {
        let mut sobp = SobpCore::<64>::new();

        // First slot with 10 buys
        for _ in 0..10 {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }

        // Should use BASELINE_INTENSITY (1.0) for missing previous slot
        let result = sobp.analyze_slot(100).unwrap();

        // Expected: (10 * 2.0) / 1.0 = 20.0
        assert_eq!(result.sobp_value, 20.0);
        assert_eq!(result.previous_intensity, BASELINE_INTENSITY);
        assert_eq!(result.pressure_state, PressureState::Hyper);
    }

    // =============================================================================
    // Performance Benchmarks
    // =============================================================================

    #[test]
    fn test_transaction_recording_performance() {
        use std::time::Instant;

        let mut sobp = SobpCore::<64>::new();
        let iterations = 10_000;

        let start = Instant::now();
        for i in 0..iterations {
            sobp.record_transaction(&TransactionRecord {
                slot: Some(100 + (i / 100)),
                actor_type: ActorType::HumanMobile,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 256,
                is_buy: true,
                timestamp_ms: 0,
                price: None,
            });
        }
        let elapsed = start.elapsed();

        let avg_time_us = (elapsed.as_micros() as f64) / (iterations as f64);

        println!("Average transaction recording time: {:.3} μs", avg_time_us);
        println!("Total time for {} transactions: {:?}", iterations, elapsed);

        // Performance target: <10 μs per transaction
        #[cfg(not(debug_assertions))]
        assert!(
            avg_time_us < 10.0,
            "Transaction recording too slow: {:.3} μs (target: <10 μs)",
            avg_time_us
        );

        // Debug mode allows more time
        #[cfg(debug_assertions)]
        assert!(
            avg_time_us < 100.0,
            "Transaction recording too slow even for debug: {:.3} μs",
            avg_time_us
        );
    }

    #[test]
    fn test_sobp_calculation_performance() {
        use std::time::Instant;

        let mut sobp = SobpCore::<64>::new();

        // Setup: populate 50 slots
        for slot in 100..150 {
            for _ in 0..10 {
                sobp.record_transaction(&TransactionRecord {
                    slot: Some(slot),
                    actor_type: ActorType::HumanMobile,
                    amount_sol: 1.0,
                    cir_effective: None,
                    tx_size_bytes: 256,
                    is_buy: true,
                    timestamp_ms: 0,
                    price: None,
                });
            }
        }

        let iterations = 10_000;
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = sobp.calculate_sobp(125);
        }
        let elapsed = start.elapsed();

        let avg_time_us = (elapsed.as_micros() as f64) / (iterations as f64);

        println!("Average SOBP calculation time: {:.3} μs", avg_time_us);

        // Performance target: <5 μs per calculation
        #[cfg(not(debug_assertions))]
        assert!(
            avg_time_us < 5.0,
            "SOBP calculation too slow: {:.3} μs (target: <5 μs)",
            avg_time_us
        );

        // Debug mode allows more time
        #[cfg(debug_assertions)]
        assert!(
            avg_time_us < 50.0,
            "SOBP calculation too slow even for debug: {:.3} μs",
            avg_time_us
        );
    }

    #[test]
    fn test_circular_buffer_performance() {
        use std::time::Instant;

        let mut buffer = CircularBuffer::<64>::new();
        let iterations = 10_000;

        let start = Instant::now();
        for i in 0..iterations {
            let metrics = buffer.get_or_create_mut(100 + i);
            metrics.add_buy(1.0);
        }
        let elapsed = start.elapsed();

        let avg_time_us = (elapsed.as_micros() as f64) / (iterations as f64);

        println!("Average buffer operation time: {:.3} μs", avg_time_us);

        // Should be very fast (O(1) operations)
        #[cfg(not(debug_assertions))]
        assert!(
            avg_time_us < 2.0,
            "Buffer operation too slow: {:.3} μs (target: <2 μs)",
            avg_time_us
        );

        #[cfg(debug_assertions)]
        assert!(
            avg_time_us < 20.0,
            "Buffer operation too slow even for debug: {:.3} μs",
            avg_time_us
        );
    }

    #[test]
    #[should_panic(expected = "CircularBuffer capacity must be power of 2")]
    fn test_buffer_non_power_of_two() {
        let _buffer = CircularBuffer::<63>::new();
    }

    #[test]
    #[should_panic(expected = "CircularBuffer capacity must be greater than 0")]
    fn test_buffer_zero_capacity() {
        let _buffer = CircularBuffer::<0>::new();
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Attempted to insert old slot")]
    fn test_buffer_rejects_old_slots_in_debug() {
        let mut buffer = CircularBuffer::<64>::new();

        // Insert slot 100
        buffer.get_or_create_mut(100);

        // Insert slot 200 (advances max_slot to 200)
        buffer.get_or_create_mut(200);

        // Try to insert slot 150 (old slot - should panic in debug)
        buffer.get_or_create_mut(150);
    }

    // =============================================================================
    // Property-Based Tests (using proptest)
    // =============================================================================

    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        /// Generate valid ActorType for property tests
        fn actor_type_strategy() -> impl Strategy<Value = ActorType> {
            prop_oneof![
                Just(ActorType::HumanMobile),
                Just(ActorType::HumanDesktop),
                Just(ActorType::SniperScript),
                Just(ActorType::MEVArb),
                Just(ActorType::LiquidityBot),
                Just(ActorType::RPCFiller),
                Just(ActorType::SybilBot),
                Just(ActorType::Unknown),
            ]
        }

        proptest! {
            /// Property: All SOBP values must be non-negative
            #[test]
            fn prop_sobp_always_non_negative(
                slot_count in 2u64..20,
                tx_per_slot in 1usize..50,
            ) {
                let mut sobp = SobpCore::<64>::new();

                for slot in 100..(100 + slot_count) {
                    for _ in 0..tx_per_slot {
                        sobp.record_transaction(&TransactionRecord {
                            slot: Some(slot),
                            actor_type: ActorType::HumanMobile,
                            amount_sol: 1.0,
                            cir_effective: None,
                            tx_size_bytes: 256,
                            is_buy: true,
            timestamp_ms: 0,
            price: None,
                        });
                    }
                }

                // Check all calculated SOBP values
                for slot in 101..(100 + slot_count) {
                    if let Some(value) = sobp.calculate_sobp(slot) {
                        prop_assert!(value >= 0.0, "SOBP must be non-negative, got {}", value);
                    }
                }
            }

            /// Property: Weight calculation must be deterministic
            #[test]
            fn prop_weight_deterministic(
                slot in 100u64..200,
                actor in actor_type_strategy(),
                tx_size in 32usize..1024,
            ) {
                let tx1 = TransactionRecord {
                    slot: Some(slot),
                    actor_type: actor,
                    amount_sol: 1.0,
                    cir_effective: None,
                    tx_size_bytes: tx_size,
                    is_buy: true,
            timestamp_ms: 0,
            price: None,
                };

                let tx2 = TransactionRecord {
                    slot: Some(slot),
                    actor_type: actor,
                    amount_sol: 1.0,
                    cir_effective: None,
                    tx_size_bytes: tx_size,
                    is_buy: true,
            timestamp_ms: 0,
            price: None,
                };

                let w1 = SobpCore::<64>::calculate_weight(&tx1);
                let w2 = SobpCore::<64>::calculate_weight(&tx2);

                prop_assert_eq!(w1, w2, "Weight calculation must be deterministic");
            }

            /// Property: SOBP core must handle monotonically increasing slot sequences
            #[test]
            fn prop_handles_arbitrary_slots(
                slots in prop::collection::vec(100u64..1000, 1..20),
            ) {
                let mut sobp = SobpCore::<64>::new();

                // Sort slots to ensure monotonically increasing (as happens in production)
                let mut sorted_slots = slots.clone();
                sorted_slots.sort_unstable();

                for &slot in &sorted_slots {
                    sobp.record_transaction(&TransactionRecord {
                        slot: Some(slot),
                        actor_type: ActorType::HumanMobile,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: true,
            timestamp_ms: 0,
            price: None,
                    });
                }

                // Should not panic or produce invalid state
                prop_assert!(sobp.slot_count() > 0);
                prop_assert!(sobp.current_slot() >= 100);
            }

            /// Property: Circular buffer maintains size constraint
            #[test]
            fn prop_buffer_size_bounded(
                slot_count in 1usize..200,
            ) {
                let mut buffer = CircularBuffer::<64>::new();

                for slot in 100..(100 + slot_count as u64) {
                    let metrics = buffer.get_or_create_mut(slot);
                    metrics.add_buy(1.0);
                }

                // Buffer should never exceed capacity
                prop_assert!(buffer.len() <= 64, "Buffer exceeded capacity: {}", buffer.len());
            }

            /// Property: Buy intensity is sum of weights
            #[test]
            fn prop_buy_intensity_is_sum_of_weights(
                buy_count in 1usize..20,
            ) {
                let mut sobp = SobpCore::<64>::new();
                let slot = 100;

                let mut expected_total = 0.0f32;
                for _ in 0..buy_count {
                    let tx = TransactionRecord {
                        slot: Some(slot),
                        actor_type: ActorType::HumanMobile,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: true,
            timestamp_ms: 0,
            price: None,
                    };
                    expected_total += SobpCore::<64>::calculate_weight(&tx);
                    sobp.record_transaction(&tx);
                }

                let metrics = sobp.get_slot_metrics(slot).unwrap();
                let diff = (metrics.buy_intensity - expected_total).abs();
                prop_assert!(diff < 0.001, "Buy intensity mismatch: {} vs {}",
                    metrics.buy_intensity, expected_total);
            }

            /// Property: SOBP ratio matches manual calculation
            #[test]
            fn prop_sobp_matches_manual_calculation(
                prev_count in 1usize..30,
                curr_count in 1usize..30,
            ) {
                let mut sobp = SobpCore::<64>::new();

                // Previous slot
                for _ in 0..prev_count {
                    sobp.record_transaction(&TransactionRecord {
                        slot: Some(100),
                        actor_type: ActorType::HumanDesktop,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: true,
            timestamp_ms: 0,
            price: None,
                    });
                }

                // Current slot
                for _ in 0..curr_count {
                    sobp.record_transaction(&TransactionRecord {
                        slot: Some(101),
                        actor_type: ActorType::HumanDesktop,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: true,
            timestamp_ms: 0,
            price: None,
                    });
                }

                let calculated_sobp = sobp.calculate_sobp(101).unwrap();
                let expected_sobp = curr_count as f32 / prev_count as f32;

                let diff = (calculated_sobp - expected_sobp).abs();
                prop_assert!(diff < 0.001, "SOBP mismatch: {} vs {}",
                    calculated_sobp, expected_sobp);
            }

            /// Property: Mixed actor types maintain correct weighting
            #[test]
            fn prop_mixed_actors_correct_weighting(
                human_count in 0usize..20,
                bot_count in 0usize..20,
            ) {
                // Skip if both are zero
                if human_count == 0 && bot_count == 0 {
                    return Ok(());
                }

                let mut sobp = SobpCore::<64>::new();
                let slot = 100;

                let mut expected_intensity = 0.0f32;

                for _ in 0..human_count {
                    sobp.record_transaction(&TransactionRecord {
                        slot: Some(slot),
                        actor_type: ActorType::HumanMobile,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: true,
            timestamp_ms: 0,
            price: None,
                    });
                    expected_intensity += 2.0; // HUMAN_WEIGHT_MULTIPLIER
                }

                for _ in 0..bot_count {
                    sobp.record_transaction(&TransactionRecord {
                        slot: Some(slot),
                        actor_type: ActorType::SniperScript,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: true,
            timestamp_ms: 0,
            price: None,
                    });
                    expected_intensity += 0.5; // SNIPER_WEIGHT_MULTIPLIER
                }

                let metrics = sobp.get_slot_metrics(slot).unwrap();
                let diff = (metrics.buy_intensity - expected_intensity).abs();
                prop_assert!(diff < 0.001, "Intensity mismatch: {} vs {}",
                    metrics.buy_intensity, expected_intensity);
            }

            /// Property: Slot metrics count matches transaction count
            #[test]
            fn prop_counts_match_transactions(
                buy_count in 0usize..50,
                sell_count in 0usize..50,
            ) {
                let mut sobp = SobpCore::<64>::new();
                let slot = 100;

                for _ in 0..buy_count {
                    sobp.record_transaction(&TransactionRecord {
                        slot: Some(slot),
                        actor_type: ActorType::HumanMobile,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: true,
            timestamp_ms: 0,
            price: None,
                    });
                }

                for _ in 0..sell_count {
                    sobp.record_transaction(&TransactionRecord {
                        slot: Some(slot),
                        actor_type: ActorType::HumanMobile,
                        amount_sol: 1.0,
                        cir_effective: None,
                        tx_size_bytes: 256,
                        is_buy: false,
            timestamp_ms: 0,
            price: None,
                    });
                }

                if buy_count > 0 || sell_count > 0 {
                    let metrics = sobp.get_slot_metrics(slot).unwrap();
                    prop_assert_eq!(metrics.buy_count, buy_count as u32);
                    prop_assert_eq!(metrics.sell_count, sell_count as u32);
                    prop_assert_eq!(metrics.total_count, (buy_count + sell_count) as u32);
                }
            }

            /// Property: Buffer correctly handles slot lookups
            #[test]
            fn prop_buffer_lookup_consistency(
                slots in prop::collection::vec(100u64..200, 1..30),
            ) {
                let mut buffer = CircularBuffer::<64>::new();
                let mut unique_slots = slots.clone();
                unique_slots.sort_unstable();
                unique_slots.dedup();

                for &slot in &unique_slots {
                    let metrics = buffer.get_or_create_mut(slot);
                    metrics.add_buy(slot as f32);
                }

                // Only verify the last N slots where N = min(buffer.len(), 64)
                let buffer_len = buffer.len();
                let max_slot = buffer.max_slot();

                // For non-sequential slots, only recent slots are guaranteed to be in buffer
                // We'll verify that the max_slot is retrievable
                if buffer_len > 0 {
                    let retrieved = buffer.get(max_slot);
                    prop_assert!(retrieved.is_some(),
                        "Max slot {} should always be in buffer", max_slot);

                    // Verify buffer doesn't exceed capacity
                    prop_assert!(buffer_len <= 64, "Buffer exceeded capacity");
                }
            }
        }
    }

    // =========================================================================
    // Event-Time Early Window Tests (FAZA 5)
    // =========================================================================

    #[test]
    fn test_is_early_window_no_events() {
        let sobp = SobpCore::<64>::new();
        // first_event_ts_ms = 0 (default), no events recorded
        // Should always be early window
        assert!(sobp.is_early_window(5_000));
        assert!(sobp.is_early_window(100_000));
    }

    #[test]
    fn test_is_early_window_too_little_time() {
        let mut sobp = SobpCore::<64>::new();
        sobp.min_window_ms = 10_000; // 10s
        sobp.min_events = 3;
        sobp.first_event_ts_ms = 1_000;
        sobp.event_count = 10; // Plenty of events

        // 5s elapsed (6000 - 1000 = 5000ms < 10000ms) - too little time
        assert!(
            sobp.is_early_window(6_000),
            "Should be early window: 5s elapsed < 10s min_window_ms"
        );
    }

    #[test]
    fn test_is_early_window_too_few_events() {
        let mut sobp = SobpCore::<64>::new();
        sobp.min_window_ms = 10_000; // 10s
        sobp.min_events = 3;
        sobp.first_event_ts_ms = 1_000;
        sobp.event_count = 2; // Too few events

        // 15s elapsed (16000 - 1000 = 15000ms > 10000ms) - enough time
        // But only 2 events < 3 min_events
        assert!(
            sobp.is_early_window(16_000),
            "Should be early window: 2 events < 3 min_events"
        );
    }

    #[test]
    fn test_is_early_window_normal_window() {
        let mut sobp = SobpCore::<64>::new();
        sobp.min_window_ms = 10_000; // 10s
        sobp.min_events = 3;
        sobp.first_event_ts_ms = 1_000;
        sobp.event_count = 5; // Enough events

        // 15s elapsed - normal window (not early)
        assert!(
            !sobp.is_early_window(16_000),
            "Should NOT be early window: 15s elapsed >= 10s AND 5 events >= 3"
        );
    }

    #[test]
    fn test_is_early_window_boundary_conditions() {
        let mut sobp = SobpCore::<64>::new();
        sobp.min_window_ms = 10_000;
        sobp.min_events = 3;
        sobp.first_event_ts_ms = 1_000;
        sobp.event_count = 3; // Exactly min_events

        // Exactly 10s elapsed (11000 - 1000 = 10000ms)
        // Should NOT be early window (boundary is < not <=)
        assert!(
            !sobp.is_early_window(11_000),
            "Boundary: 10s elapsed == 10s min_window_ms should NOT be early"
        );

        // Just under 10s elapsed
        assert!(
            sobp.is_early_window(10_999),
            "Boundary: 9.999s elapsed < 10s should be early"
        );
    }

    #[test]
    fn test_get_fallback_price_none() {
        let sobp = SobpCore::<64>::new();
        assert_eq!(sobp.get_fallback_price(), None);
    }

    #[test]
    fn test_get_fallback_price_cached() {
        let mut sobp = SobpCore::<64>::new();
        sobp.last_known_price = Some(0.00001);
        assert_eq!(sobp.get_fallback_price(), Some(0.00001));
    }

    #[test]
    fn test_early_window_integration_with_record_transaction() {
        let mut sobp = SobpCore::<64>::new();
        sobp.min_window_ms = 5_000; // 5s
        sobp.min_events = 2;

        // Initially early window
        assert!(sobp.is_early_window(1_000));

        // Record first transaction at t=1000ms
        sobp.record_transaction(&TransactionRecord {
            slot: Some(100),
            timestamp_ms: 1_000,
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            price: Some(0.00005),
        });

        // Still early: only 1 event, need 2
        assert!(sobp.is_early_window(3_000));
        assert_eq!(sobp.get_fallback_price(), Some(0.00005));

        // Record second transaction at t=2000ms
        sobp.record_transaction(&TransactionRecord {
            slot: Some(101),
            timestamp_ms: 2_000,
            actor_type: ActorType::HumanMobile,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 256,
            is_buy: true,
            price: Some(0.00006),
        });

        // Still early: 2 events >= 2, but only 3s elapsed < 5s
        assert!(sobp.is_early_window(4_000));
        assert_eq!(sobp.get_fallback_price(), Some(0.00006));

        // Now at t=6s: 5s elapsed, 2 events - NOT early window
        assert!(!sobp.is_early_window(6_000));
    }

    #[test]
    fn test_early_window_is_identical_for_none_and_some_slot() {
        let mut sobp_none = SobpCore::<64>::new();
        let mut sobp_some = SobpCore::<64>::new();
        sobp_none.min_window_ms = 5_000;
        sobp_none.min_events = 3;
        sobp_some.min_window_ms = 5_000;
        sobp_some.min_events = 3;

        let timestamps = [1_000u64, 2_000u64, 3_000u64];
        for (idx, ts) in timestamps.iter().enumerate() {
            sobp_none.record_transaction(&TransactionRecord {
                slot: None,
                timestamp_ms: *ts,
                actor_type: ActorType::Unknown,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 0,
                is_buy: true,
                price: None,
            });
            sobp_some.record_transaction(&TransactionRecord {
                slot: Some(100 + idx as u64),
                timestamp_ms: *ts,
                actor_type: ActorType::Unknown,
                amount_sol: 1.0,
                cir_effective: None,
                tx_size_bytes: 0,
                is_buy: true,
                price: None,
            });
        }

        for now_ts_ms in [3_500u64, 6_000u64, 7_000u64] {
            assert_eq!(
                sobp_none.is_early_window(now_ts_ms),
                sobp_some.is_early_window(now_ts_ms),
                "early_window mismatch at now_ts_ms={}",
                now_ts_ms
            );
        }
    }

    #[test]
    fn test_early_to_stable_transition_depends_only_on_event_time_and_count() {
        let mut sobp = SobpCore::<64>::new();
        sobp.min_window_ms = 4_000;
        sobp.min_events = 3;

        sobp.record_transaction(&TransactionRecord {
            slot: None,
            timestamp_ms: 1_000,
            actor_type: ActorType::Unknown,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 0,
            is_buy: true,
            price: None,
        });
        assert!(
            sobp.is_early_window(2_000),
            "must remain early with 1 event regardless of slot metadata"
        );

        sobp.record_transaction(&TransactionRecord {
            slot: Some(42),
            timestamp_ms: 2_000,
            actor_type: ActorType::Unknown,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 0,
            is_buy: true,
            price: None,
        });
        assert!(
            sobp.is_early_window(4_500),
            "must remain early when event_count < min_events even after slot appears"
        );

        sobp.record_transaction(&TransactionRecord {
            slot: Some(0),
            timestamp_ms: 5_500,
            actor_type: ActorType::Unknown,
            amount_sol: 1.0,
            cir_effective: None,
            tx_size_bytes: 0,
            is_buy: true,
            price: None,
        });
        assert!(
            !sobp.is_early_window(5_500),
            "must exit early only after elapsed_ms>=min_window_ms and event_count>=min_events"
        );
    }
}
