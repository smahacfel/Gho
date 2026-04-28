//! Shadow Ledger Types - Pure Data Types for Market State Representation
//!
//! This module contains all type definitions and public structures for the Shadow Ledger:
//! - `MarketSnapshot` - Geometric market state for derivative analysis
//! - `SnapshotBuffer` - Wrapper for snapshot vector with lifecycle metadata
//! - `BuySimulationResult` - Pre-transaction buy simulation output
//! - `SellSimulationResult` - Pre-transaction sell simulation output
//!
//! ## Design Principles
//!
//! - **No logic**: Only structs, enums, trivial methods, `impl Default`, `Copy`, `Clone`
//! - **No dependencies on simulation/API logic**: This module is the dependency, not the dependent
//! - **Stack allocation**: All types are designed for zero-heap-allocation hot paths

use super::trade_types::TxKey;
use crate::market_state::BondingCurve;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

/// Number of lamports per SOL (1 SOL = 10^9 lamports)
pub const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// Default snapshot age limit in milliseconds (5 minutes = 300,000 ms)
/// Snapshots older than this are eligible for eviction during cleanup cycles.
pub const DEFAULT_SNAPSHOT_MAX_AGE_MS: u64 = 300_000;

/// Epsilon value for derivative calculations to avoid division by zero.
/// Used when computing price sensitivity gradients in micro-simulations.
pub const DERIVATIVE_EPSILON: f64 = 1e-12;

/// Sentinel for snapshots where the Solana slot is unknown.
/// Reserved value: real snapshots should provide the observed slot; slot=0 means "unknown/legacy".
const UNKNOWN_SLOT: u64 = 0;

// ============================================================================
// Utility Functions
// ============================================================================

/// Get the current time in milliseconds since UNIX epoch.
///
/// This is a shared utility function used by eviction and staleness checks.
/// Uses saturating arithmetic to handle edge cases.
#[inline]
pub fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// Price State - Epistemic Quality of Price Data
// ============================================================================

/// Epistemic state of price data
///
/// This enum distinguishes between different reasons why a price might not be usable:
/// - **Valid**: Price is derived from reliable data and can be used for calculations
/// - **Unknown**: No data available yet (e.g., waiting for first update), but no logical contradiction
/// - **Invalid**: Price data is logically impossible (NaN, Inf, negative, contradictory reserves)
///
/// # Decision Making
///
/// - `Unknown` should NOT kill a session - it just means data hasn't arrived yet
/// - `Invalid` indicates a critical problem and MAY trigger session termination
/// - Only `Valid` prices should be used for Chaos/QEDD price-dependent calculations
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PriceState {
    /// Price is valid and can be used
    Valid,
    /// No price data available yet (not an error, just waiting)
    Unknown,
    /// Price data is logically impossible or contradictory (critical error)
    Invalid,
}

/// Reason describing why price is Unknown or Invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PriceReason {
    MissingReserves,
    MissingPriceData,
    ZeroOrNearZeroReserves,
    NonFinite,
    NonPositive,
    Contradiction,
    FallbackUsed,
    MissingCurve,
}

impl Default for PriceState {
    fn default() -> Self {
        PriceState::Unknown
    }
}

impl PriceState {
    /// Check if price is valid for use in calculations
    pub fn is_valid(&self) -> bool {
        matches!(self, PriceState::Valid)
    }

    /// Check if price is unknown (not yet available)
    pub fn is_unknown(&self) -> bool {
        matches!(self, PriceState::Unknown)
    }

    /// Check if price is invalid (critical error)
    pub fn is_invalid(&self) -> bool {
        matches!(self, PriceState::Invalid)
    }

    /// Get string representation for telemetry
    pub fn as_str(&self) -> &'static str {
        match self {
            PriceState::Valid => "Valid",
            PriceState::Unknown => "Unknown",
            PriceState::Invalid => "Invalid",
        }
    }

    /// Classify a raw price into PriceState with an explicit reason.
    ///
    /// - Valid  : finite and > 0
    /// - Invalid: NaN/Inf or negative
    /// - Unknown: zero or missing (treated as absent price data)
    #[inline]
    pub fn from_price(price: f64) -> (PriceState, Option<PriceReason>) {
        if price.is_finite() && price > 0.0 {
            return (PriceState::Valid, None);
        }

        if price.is_nan() || price.is_infinite() {
            return (PriceState::Invalid, Some(PriceReason::NonFinite));
        }

        if price < 0.0 {
            return (PriceState::Invalid, Some(PriceReason::NonPositive));
        }

        (PriceState::Unknown, Some(PriceReason::MissingPriceData))
    }
}

// ============================================================================
// Market Snapshot - Geometric Market State Representation
// ============================================================================

/// A single "frame" of market geometry for a pool.
///
/// Synthetic snapshots represent different projections of the same state plus
/// local micro-dynamics. Used by scoring modules (SCR, ULVF, POVC, HOSD, QOFSV)
/// for derivative calculations, curvature analysis, and trajectory prediction.
///
/// # Bootstrap Snapshots
///
/// At T=0 (InitializePool), three synthetic snapshots are generated:
/// - **G0** (Genesis): Pure InitializePool state
/// - **G1** (Projected Liquidity): Simulated minimal trade impact
/// - **G2** (First Tick): Reconstructed 2nd-order derivatives (price sensitivity)
///
/// # Fields
///
/// All fields are designed for sub-50ns access and stack allocation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarketSnapshot {
    /// Optional Solana slot when this snapshot was observed.
    /// - Some(slot) = slot known from blockchain
    /// - None = synthetic snapshot or slot unknown (PumpPortal events)
    /// Slot MUST NOT be derived from timestamp_ms.
    /// Some(0) is INVALID and will be rejected (legacy sentinel abuse).
    pub slot: Option<u64>,

    /// Deterministic TX identifier when available (canonical dedup key).
    pub tx_key: Option<TxKey>,

    /// Timestamp in milliseconds since UNIX_EPOCH.
    /// Requirement: non-decreasing within a single snapshot vector (G0 < G1 < G2).
    pub timestamp_ms: u64,

    /// Cumulative volume transferred in SOL since InitializePool.
    /// - G0 = 0.0
    /// - G1/G2 = synthetic volume based on simulated transactions
    pub cum_volume_sol: f64,

    /// Number of transactions (synthetic) included up to this snapshot.
    /// - G0 = 0
    /// - G1 >= 1
    /// - G2 >= G1
    pub tx_count: u64,

    /// Number of unique addresses (synthetic): owner + hypothetical early traders.
    /// - G0 = 1 (only owner)
    /// - G1/G2 >= 1
    pub unique_addrs: u64,

    /// Instantaneous price in SOL per token (lamports/token as f64).
    /// Calculated from `BondingCurve::current_price()` or simulation.
    pub price_sol_per_token: f64,
    /// Epistemic state of price data (Valid, Unknown, or Invalid)
    pub price_state: PriceState,
    /// Reason for Unknown/Invalid price state (None when Valid)
    pub price_reason: Option<PriceReason>,

    /// Current (synthetic) market cap in SOL (lamports cast to f64).
    /// Based on `BondingCurve::get_market_cap_sol()`.
    pub market_cap_sol: f64,

    /// Current base token reserves (e.g., token amount in the pool).
    pub reserve_base: f64,

    /// Current quote reserves (e.g., SOL in the pool).
    pub reserve_quote: f64,

    /// Bonding progress percentage (0.0-100.0).
    /// Calculated from `BondingCurve::get_bonding_progress()`.
    pub bonding_progress_pct: f64,

    /// Local derivatives used by SCR/ULVF/POVC/HOSD.
    /// May be 0.0 in G0, populated in G1/G2:
    ///
    /// ∂p/∂volume — price sensitivity to volume (delta price / delta volume).
    pub d_price_d_volume: f64,

    /// ∂p/∂liquidity — slope with respect to reserve changes
    /// (e.g., delta price / delta virtual_sol).
    pub d_price_d_liquidity: f64,

    /// ∂p/∂slippage — curvature of slippage
    /// (how fast slippage grows with order size).
    pub d_price_d_slippage: f64,
}

impl Default for MarketSnapshot {
    fn default() -> Self {
        Self {
            slot: None, // Synthetic/unknown slot
            tx_key: None,
            timestamp_ms: 0,
            cum_volume_sol: 0.0,
            tx_count: 0,
            unique_addrs: 1,
            price_sol_per_token: 0.0,
            price_state: PriceState::Unknown,
            price_reason: None,
            market_cap_sol: 0.0,
            reserve_base: 0.0,
            reserve_quote: 0.0,
            bonding_progress_pct: 0.0,
            d_price_d_volume: 0.0,
            d_price_d_liquidity: 0.0,
            d_price_d_slippage: 0.0,
        }
    }
}

// ============================================================================
// BVA (Behavioral Vacuum Analysis) Archive Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BvaClassification {
    Organic,
    Steered,
    Chaotic,
    Dormant,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BvaMetrics {
    pub tds: f64,
    pub dc: f64,
    pub se: f64,
    pub cer: f64,
    pub erp: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BvaArchive {
    pub birth_slot: Option<u64>,
    pub birth_ts_ms: u64,
    pub last_update_slot: Option<u64>,
    pub last_update_ts_ms: u64,
    pub tx_count_total: u64,
    pub unique_signers: u64,
    pub score: f64,
    pub confidence: f64,
    pub classification: BvaClassification,
    pub metrics: BvaMetrics,
}

impl MarketSnapshot {
    /// Create a new MarketSnapshot with the given timestamp.
    ///
    /// Backward compatibility helper used when slot information is not available.
    /// New code should prefer `new_with_slot` to preserve canonical slot semantics.
    #[inline]
    pub fn new(timestamp_ms: u64) -> Self {
        // Backward compatibility helper: use None when real slot is unavailable.
        // Prefer `new_with_slot` whenever the originating Solana slot is known.
        Self::new_with_slot(timestamp_ms, None)
    }

    /// Create a new MarketSnapshot with explicit slot and timestamp.
    /// slot: Some(n) for known blockchain slot, None for synthetic/unknown.
    /// Some(0) is INVALID and should not be used.
    #[inline]
    pub fn new_with_slot(timestamp_ms: u64, slot: Option<u64>) -> Self {
        Self {
            slot,
            timestamp_ms,
            ..Default::default()
        }
    }

    /// Backward compatibility helper: Check if price is valid for use
    ///
    /// This provides compatibility with old code that checked `price_valid: bool`.
    /// New code should use `price_state.is_valid()` directly.
    #[inline]
    pub fn price_valid(&self) -> bool {
        self.price_state.is_valid()
    }

    /// Helper for scoring: only Valid prices are usable.
    #[inline]
    pub fn is_price_usable_for_scoring(&self) -> bool {
        self.price_state.is_valid()
    }

    /// Create a genesis snapshot (G0) from a bonding curve.
    ///
    /// This represents the pure InitializePool state with no trades.
    #[inline]
    pub fn from_curve_genesis(curve: &BondingCurve, timestamp_ms: u64) -> Self {
        let price = curve.current_price();
        let (price_state, price_reason) = PriceState::from_price(price);

        Self {
            slot: None, // Synthetic genesis snapshot - no real slot
            tx_key: None,
            timestamp_ms,
            cum_volume_sol: 0.0,
            tx_count: 0,
            unique_addrs: 1, // Only the owner
            price_sol_per_token: price,
            price_state,
            price_reason,
            market_cap_sol: curve.get_market_cap_sol() as f64,
            reserve_base: curve.virtual_token_reserves as f64,
            reserve_quote: curve.virtual_sol_reserves as f64 / LAMPORTS_PER_SOL,
            bonding_progress_pct: curve.get_bonding_progress() as f64,
            d_price_d_volume: 0.0,
            d_price_d_liquidity: 0.0,
            d_price_d_slippage: 0.0,
        }
    }
}

// ============================================================================
// Snapshot Buffer - Wrapper for Snapshot Vector with Metadata
// ============================================================================

/// Buffer storing snapshots for a single mint with lifecycle metadata.
///
/// This wrapper enables lazy eviction by tracking when the snapshots were created.
/// The `created_at_ms` field is used during cleanup cycles to identify stale entries.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SnapshotBuffer {
    /// The actual market snapshots (typically G0, G1, G2).
    pub snapshots: Vec<MarketSnapshot>,

    /// Timestamp when this buffer was created (in milliseconds since UNIX_EPOCH).
    /// Used for age-based eviction during cleanup cycles.
    pub created_at_ms: u64,
}

impl SnapshotBuffer {
    /// Create a new SnapshotBuffer with the current timestamp.
    pub fn new(snapshots: Vec<MarketSnapshot>) -> Self {
        let created_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            snapshots,
            created_at_ms,
        }
    }

    /// Create a new SnapshotBuffer with a specific timestamp.
    pub fn with_timestamp(snapshots: Vec<MarketSnapshot>, created_at_ms: u64) -> Self {
        Self {
            snapshots,
            created_at_ms,
        }
    }

    /// Check if this buffer is older than the given max age.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Maximum age in milliseconds
    ///
    /// # Returns
    ///
    /// `true` if the buffer is older than max_age_ms
    #[inline]
    pub fn is_stale(&self, max_age_ms: u64) -> bool {
        current_time_ms().saturating_sub(self.created_at_ms) > max_age_ms
    }

    /// Get the age of this buffer in milliseconds.
    #[inline]
    pub fn age_ms(&self) -> u64 {
        current_time_ms().saturating_sub(self.created_at_ms)
    }
}

impl Default for SnapshotBuffer {
    fn default() -> Self {
        Self {
            snapshots: Vec::new(),
            created_at_ms: 0,
        }
    }
}

// ============================================================================
// Simulation Result Types
// ============================================================================

/// Result of a buy simulation
///
/// Contains all information needed for pre-transaction analysis:
/// - Expected tokens out
/// - Minimum tokens out (with slippage)
/// - Price impact percentage
/// - Effective price per token
///
/// # Performance
///
/// This struct is designed for <50ns simulation time:
/// - All fields are stack-allocated primitives
/// - No heap allocations
/// - Copy semantics for zero-cost passing
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuySimulationResult {
    /// Expected tokens to receive (before slippage buffer)
    pub tokens_out: u64,

    /// Minimum tokens to accept (after slippage buffer)
    /// Use this as `min_amount_out` in the buy instruction
    pub min_tokens_out: u64,

    /// SOL amount being spent (in lamports)
    pub sol_in: u64,

    /// Effective SOL after 1% fee deduction
    pub effective_sol_in: u64,

    /// Price impact as a percentage (e.g., 2.5 = 2.5% price increase)
    pub price_impact_percent: f64,

    /// Effective price per token in lamports
    /// (sol_in / tokens_out) - useful for tracking entry price
    pub effective_price_per_token: f64,

    /// Current market cap in SOL (lamports)
    pub market_cap_sol: u64,

    /// Bonding curve progress percentage (0-100)
    /// >99% indicates imminent migration to Raydium
    pub bonding_progress: u64,
}

impl Default for BuySimulationResult {
    fn default() -> Self {
        Self {
            tokens_out: 0,
            min_tokens_out: 0,
            sol_in: 0,
            effective_sol_in: 0,
            price_impact_percent: 0.0,
            effective_price_per_token: 0.0,
            market_cap_sol: 0,
            bonding_progress: 0,
        }
    }
}

/// Result of a sell simulation
///
/// Contains all information needed for sell decision analysis:
/// - Expected SOL out
/// - Minimum SOL out (with slippage)
/// - Price impact percentage
/// - Effective price per token
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SellSimulationResult {
    /// Expected SOL to receive (before slippage buffer)
    pub sol_out: u64,

    /// Minimum SOL to accept (after slippage buffer)
    /// Use this as `min_sol_out` in the sell instruction
    pub min_sol_out: u64,

    /// Tokens being sold
    pub tokens_in: u64,

    /// Price impact as a percentage (negative = price decrease)
    pub price_impact_percent: f64,

    /// Effective price per token in lamports
    /// (sol_out / tokens_in)
    pub effective_price_per_token: f64,
}

impl Default for SellSimulationResult {
    fn default() -> Self {
        Self {
            sol_out: 0,
            min_sol_out: 0,
            tokens_in: 0,
            price_impact_percent: 0.0,
            effective_price_per_token: 0.0,
        }
    }
}

// ============================================================================
// Unit Tests for Types
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // MarketSnapshot Tests
    // =========================================================================

    #[test]
    fn test_market_snapshot_default() {
        let snap = MarketSnapshot::default();
        // Default slot is None (synthetic/unknown)
        assert_eq!(snap.slot, None);
        assert_eq!(snap.timestamp_ms, 0);
        assert_eq!(snap.cum_volume_sol, 0.0);
        assert_eq!(snap.tx_count, 0);
        assert_eq!(snap.unique_addrs, 1); // Default is 1 (owner)
        assert_eq!(snap.price_sol_per_token, 0.0);
        assert_eq!(snap.price_state, PriceState::Unknown);
        assert_eq!(snap.market_cap_sol, 0.0);
        assert_eq!(snap.reserve_base, 0.0);
        assert_eq!(snap.reserve_quote, 0.0);
        assert_eq!(snap.bonding_progress_pct, 0.0);
        assert_eq!(snap.d_price_d_volume, 0.0);
        assert_eq!(snap.d_price_d_liquidity, 0.0);
        assert_eq!(snap.d_price_d_slippage, 0.0);
    }

    #[test]
    fn test_market_snapshot_new() {
        let snap = MarketSnapshot::new(12345678);
        // new() uses None for slot (synthetic/unknown)
        assert_eq!(snap.slot, None);
        assert_eq!(snap.timestamp_ms, 12345678);
        assert_eq!(snap.cum_volume_sol, 0.0);
        assert_eq!(snap.unique_addrs, 1);
    }

    #[test]
    fn test_market_snapshot_clone() {
        let snap1 = MarketSnapshot {
            slot: Some(42),
            tx_key: None,
            timestamp_ms: 1000,
            cum_volume_sol: 1.5,
            tx_count: 5,
            unique_addrs: 10,
            price_sol_per_token: 0.00001,
            price_state: PriceState::Valid,
            price_reason: None,
            market_cap_sol: 500_000_000.0,
            reserve_base: 1_000_000.0,
            reserve_quote: 12.5,
            bonding_progress_pct: 25.0,
            d_price_d_volume: 0.1,
            d_price_d_liquidity: 0.2,
            d_price_d_slippage: 0.3,
        };

        let snap3 = snap1.clone();
        assert_eq!(snap1, snap3);
    }

    #[test]
    fn test_market_snapshot_partial_eq() {
        let snap1 = MarketSnapshot::new(1000);
        let snap2 = MarketSnapshot::new(1000);
        let snap3 = MarketSnapshot::new(2000);

        assert_eq!(snap1, snap2);
        assert_ne!(snap1, snap3);
    }

    #[test]
    fn test_market_snapshot_from_curve_genesis() {
        // Create a test bonding curve
        let curve = BondingCurve {
            discriminator: 0x1234567890abcdef,
            virtual_token_reserves: 1_000_000_000_000, // 1 trillion tokens
            virtual_sol_reserves: 30_000_000_000,      // 30 SOL (30B lamports)
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 24_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        let timestamp_ms = 12345678u64;
        let snapshot = MarketSnapshot::from_curve_genesis(&curve, timestamp_ms);

        // Verify timestamp
        assert_eq!(snapshot.timestamp_ms, timestamp_ms);

        // Verify genesis state
        assert_eq!(snapshot.cum_volume_sol, 0.0);
        assert_eq!(snapshot.tx_count, 0);
        assert_eq!(snapshot.unique_addrs, 1); // Only the owner

        // Verify reserves are populated correctly
        assert_eq!(snapshot.reserve_base, curve.virtual_token_reserves as f64);
        assert_eq!(
            snapshot.reserve_quote,
            curve.virtual_sol_reserves as f64 / LAMPORTS_PER_SOL
        );

        // Verify reserves are non-zero
        assert!(
            snapshot.reserve_base > 0.0,
            "reserve_base should be non-zero"
        );
        assert!(
            snapshot.reserve_quote > 0.0,
            "reserve_quote should be non-zero"
        );

        // Verify specific expected values
        assert_eq!(snapshot.reserve_base, 1_000_000_000_000.0);
        assert_eq!(snapshot.reserve_quote, 30.0); // 30 SOL

        // Verify price and market cap are calculated
        assert!(snapshot.price_sol_per_token > 0.0);
        assert!(snapshot.market_cap_sol > 0.0);

        // Verify bonding progress is set
        assert!(snapshot.bonding_progress_pct >= 0.0 && snapshot.bonding_progress_pct <= 100.0);

        // Verify derivatives are zero for genesis
        assert_eq!(snapshot.d_price_d_volume, 0.0);
        assert_eq!(snapshot.d_price_d_liquidity, 0.0);
        assert_eq!(snapshot.d_price_d_slippage, 0.0);
    }

    // =========================================================================
    // SnapshotBuffer Tests
    // =========================================================================

    #[test]
    fn test_snapshot_buffer_default() {
        let buffer = SnapshotBuffer::default();
        assert!(buffer.snapshots.is_empty());
        assert_eq!(buffer.created_at_ms, 0);
    }

    #[test]
    fn test_snapshot_buffer_new() {
        let snaps = vec![MarketSnapshot::new(1000)];
        let buffer = SnapshotBuffer::new(snaps.clone());

        assert_eq!(buffer.snapshots.len(), 1);
        assert_eq!(buffer.snapshots[0].timestamp_ms, 1000);
        assert!(buffer.created_at_ms > 0, "created_at_ms should be set");
    }

    #[test]
    fn test_snapshot_buffer_with_timestamp() {
        let snaps = vec![MarketSnapshot::new(1000)];
        let custom_ts = 12345678u64;
        let buffer = SnapshotBuffer::with_timestamp(snaps, custom_ts);

        assert_eq!(buffer.created_at_ms, custom_ts);
    }

    #[test]
    fn test_snapshot_buffer_staleness() {
        let snaps = vec![MarketSnapshot::new(1000)];
        // Create buffer with timestamp from 1 second ago
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 1000;
        let buffer = SnapshotBuffer::with_timestamp(snaps, old_ts);

        // Should be stale if max_age is 500ms
        assert!(buffer.is_stale(500), "Buffer should be stale after 500ms");

        // Should not be stale if max_age is 2000ms
        assert!(
            !buffer.is_stale(2000),
            "Buffer should not be stale within 2000ms"
        );
    }

    #[test]
    fn test_snapshot_buffer_age() {
        let snaps = vec![MarketSnapshot::new(1000)];
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 100;
        let buffer = SnapshotBuffer::with_timestamp(snaps, old_ts);

        let age = buffer.age_ms();
        assert!(
            age >= 100 && age < 200,
            "Age should be approximately 100ms, got {}",
            age
        );
    }

    #[test]
    fn test_snapshot_buffer_clone() {
        let snaps = vec![MarketSnapshot::new(1000), MarketSnapshot::new(2000)];
        let buffer = SnapshotBuffer::new(snaps);
        let cloned = buffer.clone();

        assert_eq!(buffer.snapshots.len(), cloned.snapshots.len());
        assert_eq!(buffer.created_at_ms, cloned.created_at_ms);
    }

    // =========================================================================
    // BuySimulationResult Tests
    // =========================================================================

    #[test]
    fn test_buy_simulation_result_default() {
        let result = BuySimulationResult::default();
        assert_eq!(result.tokens_out, 0);
        assert_eq!(result.min_tokens_out, 0);
        assert_eq!(result.sol_in, 0);
        assert_eq!(result.effective_sol_in, 0);
        assert_eq!(result.price_impact_percent, 0.0);
        assert_eq!(result.effective_price_per_token, 0.0);
        assert_eq!(result.market_cap_sol, 0);
        assert_eq!(result.bonding_progress, 0);
    }

    #[test]
    fn test_buy_simulation_result_copy_clone() {
        let result = BuySimulationResult {
            tokens_out: 1000000,
            min_tokens_out: 995000,
            sol_in: 1_000_000_000,
            effective_sol_in: 990_000_000,
            price_impact_percent: 0.5,
            effective_price_per_token: 0.001,
            market_cap_sol: 30_000_000_000,
            bonding_progress: 25,
        };

        // Test Copy
        let result_copy = result;
        assert_eq!(result.tokens_out, result_copy.tokens_out);

        // Test Clone
        let result_clone = result.clone();
        assert_eq!(result.tokens_out, result_clone.tokens_out);

        // Test PartialEq
        assert_eq!(result, result_copy);
    }

    // =========================================================================
    // SellSimulationResult Tests
    // =========================================================================

    #[test]
    fn test_sell_simulation_result_default() {
        let result = SellSimulationResult::default();
        assert_eq!(result.sol_out, 0);
        assert_eq!(result.min_sol_out, 0);
        assert_eq!(result.tokens_in, 0);
        assert_eq!(result.price_impact_percent, 0.0);
        assert_eq!(result.effective_price_per_token, 0.0);
    }

    #[test]
    fn test_sell_simulation_result_copy_clone() {
        let result = SellSimulationResult {
            sol_out: 500_000_000,
            min_sol_out: 495_000_000,
            tokens_in: 1_000_000,
            price_impact_percent: -1.5,
            effective_price_per_token: 0.0005,
        };

        // Test Copy
        let result_copy = result;
        assert_eq!(result.sol_out, result_copy.sol_out);

        // Test Clone
        let result_clone = result.clone();
        assert_eq!(result.sol_out, result_clone.sol_out);

        // Test PartialEq
        assert_eq!(result, result_copy);
    }

    // =========================================================================
    // PriceState Tests
    // =========================================================================

    #[test]
    fn test_price_state_default() {
        let state = PriceState::default();
        assert_eq!(state, PriceState::Unknown);
    }

    #[test]
    fn test_price_state_is_valid() {
        assert!(PriceState::Valid.is_valid());
        assert!(!PriceState::Unknown.is_valid());
        assert!(!PriceState::Invalid.is_valid());
    }

    #[test]
    fn test_price_state_is_unknown() {
        assert!(!PriceState::Valid.is_unknown());
        assert!(PriceState::Unknown.is_unknown());
        assert!(!PriceState::Invalid.is_unknown());
    }

    #[test]
    fn test_price_state_is_invalid() {
        assert!(!PriceState::Valid.is_invalid());
        assert!(!PriceState::Unknown.is_invalid());
        assert!(PriceState::Invalid.is_invalid());
    }

    #[test]
    fn test_price_state_as_str() {
        assert_eq!(PriceState::Valid.as_str(), "Valid");
        assert_eq!(PriceState::Unknown.as_str(), "Unknown");
        assert_eq!(PriceState::Invalid.as_str(), "Invalid");
    }

    #[test]
    fn test_market_snapshot_price_valid_helper() {
        let mut snap = MarketSnapshot::default();
        snap.price_state = PriceState::Unknown;
        assert!(!snap.price_valid());

        snap.price_state = PriceState::Valid;
        assert!(snap.price_valid());

        snap.price_state = PriceState::Invalid;
        assert!(!snap.price_valid());
    }

    // =========================================================================
    // Constants Tests
    // =========================================================================

    #[test]
    fn test_lamports_per_sol_constant() {
        assert_eq!(LAMPORTS_PER_SOL, 1_000_000_000.0);
    }

    #[test]
    fn test_default_snapshot_max_age_constant() {
        // Verify the default constant is 5 minutes (300,000 ms)
        assert_eq!(DEFAULT_SNAPSHOT_MAX_AGE_MS, 300_000);
    }

    #[test]
    fn test_derivative_epsilon_constant() {
        // Verify epsilon is a small positive value
        assert!(DERIVATIVE_EPSILON > 0.0);
        assert!(DERIVATIVE_EPSILON < 1e-10);
    }
}
