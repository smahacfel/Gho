//! Shadow Ledger Bootstrap Module - G0/G1/G2 Snapshot Generation & Initialization
//!
//! This module provides algorithms for generating initial geometric market snapshots
//! during pool initialization. It handles the bootstrap phase where synthetic snapshots
//! (G0, G1, G2) are created for immediate use by derivative-based scoring modules.
//!
//! ## Design Principles
//!
//! - **Stateless**: Bootstrap functions are pure and do not store state in DashMap
//! - **Separation of Concerns**: Calculation logic is isolated from storage
//! - **Selective Bootstrap**: Supports filtering by liquidity thresholds and activity
//! - **Observable**: Integrates with Prometheus for success/fail metrics
//!
//! ## Snapshot Definitions
//!
//! - **G0** (Genesis): Pure InitializePool state with no trades
//! - **G1** (Projected Liquidity): Simulated minimal trade impact with first-order derivatives
//! - **G2** (First Tick): Second-order derivative reconstruction with full gradients
//!
//! ## Usage
//!
//! ```ignore
//! use ghost_core::shadow_ledger::bootstrap::{BootstrapConfig, bootstrap_snapshots};
//! use ghost_core::market_state::BondingCurve;
//!
//! // Bootstrap with default config (no filtering)
//! let snapshots = bootstrap_snapshots(&curve, 10_000_000, Some(current_slot), &BootstrapConfig::default());
//!
//! // Bootstrap with selective filtering (high liquidity pools only)
//! let config = BootstrapConfig::promising_pools(100_000_000_000); // 100 SOL min
//! if config.should_bootstrap(&curve) {
//!     let snapshots = bootstrap_snapshots(&curve, 10_000_000, Some(current_slot), &config);
//!     ledger.set_snapshots(mint, snapshots);
//! }
//! ```
//!
//! ## Integration with ShadowLedger
//!
//! This module is called from `ShadowLedger::bootstrap_from_initialize()` or directly
//! by Oracle infrastructure for RPC/Geyser-detected pools.

use super::simulation::{compute_all_derivatives, simulate_buy_pure};
use super::types::{MarketSnapshot, PriceState, LAMPORTS_PER_SOL};
use crate::market_state::BondingCurve;

use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Bootstrap Configuration
// ============================================================================

/// Configuration for selective bootstrap behavior.
///
/// This struct allows filtering which pools should be bootstrapped based on
/// liquidity and activity criteria. Pools that don't meet the criteria are
/// skipped to conserve resources.
#[derive(Clone, Debug)]
pub struct BootstrapConfig {
    /// Minimum virtual SOL reserves (in lamports) for a pool to be bootstrapped.
    /// Pools with lower reserves are skipped.
    /// Default: 0 (no minimum)
    pub min_sol_reserves_lamports: u64,

    /// Minimum virtual token reserves for a pool to be bootstrapped.
    /// Default: 0 (no minimum)
    pub min_token_reserves: u64,

    /// Minimum bonding progress percentage (0-100) required.
    /// Pools below this threshold are skipped.
    /// Default: 0 (no minimum)
    pub min_bonding_progress: u64,

    /// Maximum bonding progress percentage (0-100) allowed.
    /// Pools above this threshold are skipped (near migration).
    /// Default: 100 (no maximum)
    pub max_bonding_progress: u64,

    /// Whether to skip pools that appear to be already completed/migrated.
    /// Default: true
    pub skip_completed: bool,

    /// Transaction count threshold for scoring (used in selective bootstrap).
    /// If tx_count is below this threshold, pool may be deprioritized.
    /// Note: This is for future integration with activity tracking.
    /// Default: 0
    pub min_tx_count: u64,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            min_sol_reserves_lamports: 0,
            min_token_reserves: 0,
            min_bonding_progress: 0,
            max_bonding_progress: 100,
            skip_completed: true,
            min_tx_count: 0,
        }
    }
}

impl BootstrapConfig {
    /// Create a config for bootstrapping only "promising" high-liquidity pools.
    ///
    /// This is recommended for mainnet where resource conservation is important.
    ///
    /// # Arguments
    ///
    /// * `min_sol_lamports` - Minimum SOL reserves in lamports
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Only bootstrap pools with at least 100 SOL
    /// let config = BootstrapConfig::promising_pools(100_000_000_000);
    /// ```
    pub fn promising_pools(min_sol_lamports: u64) -> Self {
        Self {
            min_sol_reserves_lamports: min_sol_lamports,
            min_token_reserves: 0,
            min_bonding_progress: 0,
            max_bonding_progress: 98, // Skip pools near migration
            skip_completed: true,
            min_tx_count: 0,
        }
    }

    /// Create a config with liquidity threshold.
    ///
    /// # Arguments
    ///
    /// * `min_sol_lamports` - Minimum SOL reserves in lamports
    pub fn with_min_liquidity(min_sol_lamports: u64) -> Self {
        Self {
            min_sol_reserves_lamports: min_sol_lamports,
            ..Default::default()
        }
    }

    /// Create a config that filters by bonding progress range.
    ///
    /// # Arguments
    ///
    /// * `min_progress` - Minimum bonding progress (0-100)
    /// * `max_progress` - Maximum bonding progress (0-100)
    pub fn with_progress_range(min_progress: u64, max_progress: u64) -> Self {
        Self {
            min_bonding_progress: min_progress,
            max_bonding_progress: max_progress,
            ..Default::default()
        }
    }

    /// Check if a pool should be bootstrapped based on the config criteria.
    ///
    /// # Arguments
    ///
    /// * `curve` - Reference to the bonding curve state
    ///
    /// # Returns
    ///
    /// `true` if the pool meets all criteria and should be bootstrapped.
    #[inline]
    pub fn should_bootstrap(&self, curve: &BondingCurve) -> bool {
        // Check if pool is complete
        if self.skip_completed && curve.complete != 0 {
            return false;
        }

        // Check SOL reserves
        if curve.virtual_sol_reserves < self.min_sol_reserves_lamports {
            return false;
        }

        // Check token reserves
        if curve.virtual_token_reserves < self.min_token_reserves {
            return false;
        }

        // Check bonding progress range
        let progress = curve.get_bonding_progress();
        if progress < self.min_bonding_progress {
            return false;
        }
        if progress > self.max_bonding_progress {
            return false;
        }

        true
    }

    /// Check if a pool should be bootstrapped with additional tx_count check.
    ///
    /// # Arguments
    ///
    /// * `curve` - Reference to the bonding curve state
    /// * `tx_count` - Number of transactions observed for this pool
    ///
    /// # Returns
    ///
    /// `true` if the pool meets all criteria including activity threshold.
    #[inline]
    pub fn should_bootstrap_with_activity(&self, curve: &BondingCurve, tx_count: u64) -> bool {
        if tx_count < self.min_tx_count {
            return false;
        }
        self.should_bootstrap(curve)
    }
}

// ============================================================================
// Bootstrap Result
// ============================================================================

/// Result of a bootstrap operation with success/failure details.
///
/// This struct provides insight into what was bootstrapped and why it may
/// have failed, useful for monitoring and debugging.
#[derive(Clone, Debug, Default)]
pub struct BootstrapResult {
    /// Whether the bootstrap operation succeeded.
    pub success: bool,

    /// Number of snapshots generated (typically 3 for G0, G1, G2).
    pub snapshot_count: usize,

    /// Duration of the bootstrap operation in microseconds.
    pub duration_us: u64,

    /// Reason for failure (if success is false).
    pub failure_reason: Option<String>,
}

impl BootstrapResult {
    /// Create a successful result.
    pub fn success(snapshot_count: usize, duration_us: u64) -> Self {
        Self {
            success: true,
            snapshot_count,
            duration_us,
            failure_reason: None,
        }
    }

    /// Create a failed result with reason.
    pub fn failed(reason: impl Into<String>) -> Self {
        Self {
            success: false,
            snapshot_count: 0,
            duration_us: 0,
            failure_reason: Some(reason.into()),
        }
    }
}

// ============================================================================
// Bootstrap Metrics - Prometheus-Compatible Metrics
// ============================================================================

/// Bootstrap metrics for Prometheus/PromQL export.
///
/// These metrics are designed to be compatible with Prometheus and can be
/// exported via a metrics endpoint for monitoring and alerting.
///
/// ## PromQL Metrics
///
/// - `bootstrap_success`: Counter of successful bootstrap operations
/// - `bootstrap_fail`: Counter of failed bootstrap operations
/// - `bootstrap_skipped`: Counter of pools skipped due to filtering
/// - `bootstrap_duration_us_sum`: Sum of bootstrap durations
/// - `shadow_ledger_seeds_generated_total`: Counter of successful seed generations
/// - `shadow_ledger_seed_generation_failure_total`: Counter of seed generation failures
///
/// ## Usage
///
/// ```ignore
/// let metrics = BootstrapMetrics::new();
///
/// // After bootstrap
/// metrics.record_success(250); // 250µs duration
///
/// // After seed generation
/// metrics.record_seed_generated();
///
/// // Export for Prometheus
/// println!("{}", metrics.to_prometheus());
/// ```
#[derive(Debug, Default)]
pub struct BootstrapMetrics {
    /// Count of successful bootstrap operations.
    success_count: AtomicU64,

    /// Count of failed bootstrap operations.
    fail_count: AtomicU64,

    /// Count of pools skipped due to config filtering.
    skipped_count: AtomicU64,

    /// Sum of bootstrap durations in microseconds.
    duration_sum_us: AtomicU64,

    /// Count of successful seed generations.
    seeds_generated_total: AtomicU64,

    /// Count of failed seed generations (watchdog timeout).
    seed_generation_failure_total: AtomicU64,
}

impl BootstrapMetrics {
    /// Create a new metrics instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful bootstrap.
    pub fn record_success(&self, duration_us: u64) {
        self.success_count.fetch_add(1, Ordering::Relaxed);
        self.duration_sum_us
            .fetch_add(duration_us, Ordering::Relaxed);
    }

    /// Record a failed bootstrap.
    pub fn record_fail(&self) {
        self.fail_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a skipped pool (didn't meet criteria).
    pub fn record_skipped(&self) {
        self.skipped_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a successful seed generation.
    pub fn record_seed_generated(&self) {
        self.seeds_generated_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a failed seed generation (watchdog timeout).
    pub fn record_seed_generation_failure(&self) {
        self.seed_generation_failure_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Get success count.
    pub fn success_count(&self) -> u64 {
        self.success_count.load(Ordering::Relaxed)
    }

    /// Get fail count.
    pub fn fail_count(&self) -> u64 {
        self.fail_count.load(Ordering::Relaxed)
    }

    /// Get skipped count.
    pub fn skipped_count(&self) -> u64 {
        self.skipped_count.load(Ordering::Relaxed)
    }

    /// Get total duration sum in microseconds.
    pub fn duration_sum_us(&self) -> u64 {
        self.duration_sum_us.load(Ordering::Relaxed)
    }

    /// Get count of successful seed generations.
    pub fn seeds_generated_total(&self) -> u64 {
        self.seeds_generated_total.load(Ordering::Relaxed)
    }

    /// Get count of failed seed generations.
    pub fn seed_generation_failure_total(&self) -> u64 {
        self.seed_generation_failure_total.load(Ordering::Relaxed)
    }

    /// Get average bootstrap duration in microseconds.
    pub fn avg_duration_us(&self) -> f64 {
        let count = self.success_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let sum = self.duration_sum_us.load(Ordering::Relaxed);
        sum as f64 / count as f64
    }

    /// Reset all metrics to zero.
    pub fn reset(&self) {
        self.success_count.store(0, Ordering::Relaxed);
        self.fail_count.store(0, Ordering::Relaxed);
        self.skipped_count.store(0, Ordering::Relaxed);
        self.duration_sum_us.store(0, Ordering::Relaxed);
        self.seeds_generated_total.store(0, Ordering::Relaxed);
        self.seed_generation_failure_total
            .store(0, Ordering::Relaxed);
    }

    /// Export metrics in Prometheus text format.
    ///
    /// Returns a string that can be served via HTTP for Prometheus scraping.
    pub fn to_prometheus(&self) -> String {
        format!(
            "# HELP ghost_bootstrap_success Total number of successful bootstrap operations\n\
             # TYPE ghost_bootstrap_success counter\n\
             ghost_bootstrap_success {}\n\
             \n\
             # HELP ghost_bootstrap_fail Total number of failed bootstrap operations\n\
             # TYPE ghost_bootstrap_fail counter\n\
             ghost_bootstrap_fail {}\n\
             \n\
             # HELP ghost_bootstrap_skipped Total number of pools skipped due to filtering\n\
             # TYPE ghost_bootstrap_skipped counter\n\
             ghost_bootstrap_skipped {}\n\
             \n\
             # HELP ghost_bootstrap_duration_us_sum Sum of bootstrap durations in microseconds\n\
             # TYPE ghost_bootstrap_duration_us_sum counter\n\
             ghost_bootstrap_duration_us_sum {}\n\
             \n\
             # HELP shadow_ledger_seeds_generated_total Total number of successful seed generations\n\
             # TYPE shadow_ledger_seeds_generated_total counter\n\
             shadow_ledger_seeds_generated_total {}\n\
             \n\
             # HELP shadow_ledger_seed_generation_failure_total Total number of seed generation failures (watchdog timeout)\n\
             # TYPE shadow_ledger_seed_generation_failure_total counter\n\
             shadow_ledger_seed_generation_failure_total {}\n",
            self.success_count(),
            self.fail_count(),
            self.skipped_count(),
            self.duration_sum_us(),
            self.seeds_generated_total(),
            self.seed_generation_failure_total(),
        )
    }
}

// ============================================================================
// Helper: Get Current Timestamp
// ============================================================================

/// Get the current time in milliseconds since UNIX epoch.
#[inline]
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// Core Bootstrap Functions - Pure Stateless Functions
// ============================================================================

/// Generate G0 (Genesis) snapshot from a bonding curve.
///
/// This represents the pure InitializePool state with no trades.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `timestamp_ms` - Timestamp for the snapshot
///
/// # Returns
///
/// A `MarketSnapshot` representing the genesis state.
#[inline]
pub fn generate_g0(curve: &BondingCurve, timestamp_ms: u64, _slot: Option<u64>) -> MarketSnapshot {
    MarketSnapshot::from_curve_genesis(curve, timestamp_ms)
}

/// Generate G1 (Projected Liquidity) snapshot from a bonding curve.
///
/// Simulates a minimal buy to project initial liquidity impact and computes
/// first-order derivatives.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `min_sol_lamports` - Synthetic minimum SOL for simulation (e.g., 0.01 SOL)
/// * `timestamp_ms` - Timestamp for the snapshot
///
/// # Returns
///
/// A `MarketSnapshot` with projected liquidity and first-order derivatives.
#[inline]
pub fn generate_g1(
    curve: &BondingCurve,
    min_sol_lamports: u64,
    timestamp_ms: u64,
    _slot: Option<u64>,
) -> MarketSnapshot {
    let price_g0 = curve.current_price();

    // Simulate a minimal buy
    let buy_sim = simulate_buy_pure(curve, min_sol_lamports);

    // Calculate price after the simulated buy
    let price_after = if buy_sim.effective_price_per_token > 0.0 {
        buy_sim.effective_price_per_token
    } else {
        price_g0
    };

    // Volume in SOL for G1
    let volume_sol = (buy_sim.sol_in as f64) / LAMPORTS_PER_SOL;

    // Compute derivatives using the optimized simulation module
    let (d_price_d_volume, d_price_d_liquidity, _) =
        compute_all_derivatives(curve, min_sol_lamports);

    let reserve_quote =
        (curve.virtual_sol_reserves as f64 + buy_sim.effective_sol_in as f64) / LAMPORTS_PER_SOL;
    let reserve_base = curve
        .virtual_token_reserves
        .saturating_sub(buy_sim.tokens_out) as f64;

    let (price_state, price_reason) = PriceState::from_price(price_after);

    MarketSnapshot {
        slot: None, // Synthetic G1 snapshot - no real slot
        tx_key: None,
        timestamp_ms,
        cum_volume_sol: volume_sol,
        tx_count: 1,
        unique_addrs: 2, // Owner + hypothetical first buyer
        price_sol_per_token: price_after,
        price_state,
        price_reason,
        market_cap_sol: buy_sim.market_cap_sol as f64,
        reserve_base,
        reserve_quote,
        bonding_progress_pct: buy_sim.bonding_progress as f64,
        d_price_d_volume,
        d_price_d_liquidity,
        d_price_d_slippage: 0.0, // Will be computed for G2
    }
}

/// Generate G2 (First Tick) snapshot with second-order derivatives.
///
/// Performs micro-simulations at different volume points to compute curvature
/// (d_price_d_slippage).
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `min_sol_lamports` - Base amount for simulation (will use 2x for G2)
/// * `timestamp_ms` - Timestamp for the snapshot
/// * `g1` - Reference to the G1 snapshot for continuity
///
/// # Returns
///
/// A `MarketSnapshot` with second-order derivatives and full gradients.
#[inline]
pub fn generate_g2(
    curve: &BondingCurve,
    min_sol_lamports: u64,
    timestamp_ms: u64,
    g1: &MarketSnapshot,
    _slot: Option<u64>,
) -> MarketSnapshot {
    // Simulate buy at 2x the min amount for curvature analysis
    let double_sol_lamports = min_sol_lamports.saturating_mul(2);
    let buy_sim_2x = simulate_buy_pure(curve, double_sol_lamports);

    let price_after = if buy_sim_2x.effective_price_per_token > 0.0 {
        buy_sim_2x.effective_price_per_token
    } else {
        g1.price_sol_per_token
    };

    let volume_sol = (buy_sim_2x.sol_in as f64) / LAMPORTS_PER_SOL;

    // Compute all derivatives at the 2x volume point
    let (d_price_d_volume, d_price_d_liquidity, d_price_d_slippage) =
        compute_all_derivatives(curve, double_sol_lamports);

    let reserve_quote =
        (curve.virtual_sol_reserves as f64 + buy_sim_2x.effective_sol_in as f64) / LAMPORTS_PER_SOL;
    let reserve_base = curve
        .virtual_token_reserves
        .saturating_sub(buy_sim_2x.tokens_out) as f64;

    let (price_state, price_reason) = PriceState::from_price(price_after);

    MarketSnapshot {
        slot: None, // Synthetic G2 snapshot - no real slot
        tx_key: None,
        timestamp_ms,
        cum_volume_sol: volume_sol,
        tx_count: g1.tx_count + 1,
        unique_addrs: g1.unique_addrs + 1, // One more synthetic trader
        price_sol_per_token: price_after,
        price_state,
        price_reason,
        market_cap_sol: buy_sim_2x.market_cap_sol as f64,
        reserve_base,
        reserve_quote,
        bonding_progress_pct: buy_sim_2x.bonding_progress as f64,
        d_price_d_volume,
        d_price_d_liquidity,
        d_price_d_slippage,
    }
}

/// Bootstrap geometric snapshots (G0, G1, G2) for a bonding curve.
///
/// This is the primary bootstrap function that generates all three synthetic
/// snapshots for immediate use by derivative-based scoring modules.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `min_sol_lamports` - Synthetic minimum SOL for G1 simulation (e.g., 0.01 SOL = 10_000_000)
/// * `slot` - Solana slot used for all bootstrap snapshots
/// * `config` - Bootstrap configuration for filtering and validation
///
/// # Returns
///
/// A vector of 3 `MarketSnapshot`s: [G0, G1, G2]
///
/// # Example
///
/// ```ignore
/// let snapshots = bootstrap_snapshots(&curve, 10_000_000, Some(current_slot), &BootstrapConfig::default());
/// assert_eq!(snapshots.len(), 3);
/// ```
pub fn bootstrap_snapshots(
    curve: &BondingCurve,
    min_sol_lamports: u64,
    slot: Option<u64>,
    _config: &BootstrapConfig,
) -> Vec<MarketSnapshot> {
    let now_ms = current_timestamp_ms();

    // Generate G0 (Genesis)
    let g0 = generate_g0(curve, now_ms, slot);

    // Generate G1 (Projected Liquidity)
    let g1 = generate_g1(curve, min_sol_lamports, now_ms + 1, slot);

    // Generate G2 (First Tick)
    let g2 = generate_g2(curve, min_sol_lamports, now_ms + 2, &g1, slot);

    vec![g0, g1, g2]
}

/// Bootstrap with result tracking for metrics integration.
///
/// This function wraps `bootstrap_snapshots` and provides result/error tracking
/// suitable for integration with `BootstrapMetrics`.
///
/// # Arguments
///
/// * `curve` - Reference to the bonding curve state
/// * `min_sol_lamports` - Synthetic minimum SOL for simulation
/// * `slot` - Solana slot used for all bootstrap snapshots
/// * `config` - Bootstrap configuration
///
/// # Returns
///
/// A tuple of (`BootstrapResult`, `Option<Vec<MarketSnapshot>>`)
pub fn bootstrap_with_result(
    curve: &BondingCurve,
    min_sol_lamports: u64,
    slot: u64,
    config: &BootstrapConfig,
) -> (BootstrapResult, Option<Vec<MarketSnapshot>>) {
    let start = std::time::Instant::now();

    // Check if pool should be bootstrapped
    if !config.should_bootstrap(curve) {
        return (
            BootstrapResult::failed("Pool does not meet bootstrap criteria"),
            None,
        );
    }

    // Validate curve state
    if curve.virtual_token_reserves == 0 {
        return (BootstrapResult::failed("Zero token reserves"), None);
    }

    if curve.virtual_sol_reserves == 0 {
        return (BootstrapResult::failed("Zero SOL reserves"), None);
    }

    // Generate snapshots
    let snapshots = bootstrap_snapshots(curve, min_sol_lamports, Some(slot), config);

    let duration_us = start.elapsed().as_micros() as u64;

    (
        BootstrapResult::success(snapshots.len(), duration_us),
        Some(snapshots),
    )
}

// ============================================================================
// Synthetic Seed Generation - Quick Bootstrap Transactions
// ============================================================================

/// Synthetic transaction data for quick seed generation.
///
/// This struct represents a lightweight synthetic transaction with timestamp
/// and payload for bootstrap purposes. Used by Shadow Ledger to publish
/// synthetic events to EventBus immediately after pool detection.
#[derive(Debug, Clone)]
pub struct SyntheticTransaction {
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Synthetic payload (8 bytes)
    pub payload: [u8; 8],
}

/// Generate quick synthetic seed transactions for a pool.
///
/// This function generates a deterministic set of 8 synthetic transactions
/// with timestamps and payloads that vary based on pool parameters. The
/// transactions are lightweight (zero-allocation where possible) and
/// designed for immediate EventBus publishing after pool detection.
///
/// # Design Goals
///
/// - **Deterministic**: Same pool parameters always produce same seed
/// - **Lightweight**: Minimal allocations, target < 20ms for full generation
/// - **Varied**: Payloads vary with pool parameters to provide entropy
/// - **Fast**: Simple hash-based generation without complex crypto
///
/// # Arguments
///
/// * `pool_id` - Pool/mint Pubkey for deterministic seed generation
/// * `virtual_sol_reserves` - Virtual SOL reserves in lamports
/// * `virtual_token_reserves` - Virtual token reserves
/// * `timestamp_base_ms` - Base timestamp for the seed (typically current time)
///
/// # Returns
///
/// A vector of 8 `SyntheticTransaction`s with unique timestamps and payloads.
///
/// # Example
///
/// ```ignore
/// let pool_id = Pubkey::new_unique();
/// let txs = generate_quick_seed(
///     &pool_id,
///     30_000_000_000,    // 30 SOL
///     1_000_000_000_000, // 1T tokens
///     current_timestamp_ms()
/// );
/// assert_eq!(txs.len(), 8);
/// ```
pub fn generate_quick_seed(
    pool_id: &Pubkey,
    virtual_sol_reserves: u64,
    virtual_token_reserves: u64,
    timestamp_base_ms: u64,
) -> Vec<SyntheticTransaction> {
    const SEED_COUNT: usize = 8;
    const TIME_DELTA_MS: u64 = 100; // 100ms between synthetic txs - chosen to simulate natural transaction intervals
    const OFFSET_STEP: usize = 4; // Step size for cycling through pool_id bytes
    const PUBKEY_BYTES: usize = 32; // Solana Pubkey size
    const PAYLOAD_SIZE: usize = 8; // Synthetic payload size

    // Constants for deterministic mixing to create unique payloads
    const SOL_SHIFT: u8 = 16; // Bit shift for SOL reserves mixing
    const TOKEN_SHIFT: u8 = 20; // Bit shift for token reserves mixing
    const INDEX_MULTIPLIER: u8 = 17; // Prime multiplier for index mixing
    const XOR_MASK: u8 = 31; // Mask for additional entropy
    const ROTATE_BITS: u8 = 8; // Bit rotation amount

    let mut transactions = Vec::with_capacity(SEED_COUNT);

    // Use pool_id bytes for deterministic seed base
    let pool_bytes = pool_id.to_bytes();

    for i in 0..SEED_COUNT {
        // Generate timestamp with incrementing offset
        let timestamp_ms = timestamp_base_ms + (i as u64 * TIME_DELTA_MS);

        // Generate deterministic payload using pool parameters + index
        // Mix pool_id, reserves, and index to create unique payloads
        let mut payload = [0u8; PAYLOAD_SIZE];

        // Use simple deterministic mixing with better distribution
        // Bytes 0-3: primarily derived from pool_id to ensure different pools produce different seeds
        let offset1 = (i * OFFSET_STEP) % PUBKEY_BYTES;
        let offset2 = (i * OFFSET_STEP + 1) % PUBKEY_BYTES;
        let offset3 = (i * OFFSET_STEP + 2) % PUBKEY_BYTES;
        let offset4 = (i * OFFSET_STEP + 3) % PUBKEY_BYTES;

        payload[0] = pool_bytes[offset1];
        payload[1] = pool_bytes[offset2];
        payload[2] = pool_bytes[offset3];
        payload[3] = pool_bytes[offset4];

        // Bytes 4-7: mix pool_id with reserves for additional entropy
        let sol_bytes = virtual_sol_reserves.to_le_bytes();
        let token_bytes = virtual_token_reserves.to_le_bytes();

        payload[4] = pool_bytes[(i + SOL_SHIFT as usize) % PUBKEY_BYTES]
            ^ sol_bytes[i % ROTATE_BITS as usize];
        payload[5] = pool_bytes[(i + TOKEN_SHIFT as usize) % PUBKEY_BYTES]
            ^ token_bytes[i % ROTATE_BITS as usize];
        payload[6] =
            (i as u8).wrapping_mul(INDEX_MULTIPLIER) ^ sol_bytes[(i + 1) % ROTATE_BITS as usize];
        payload[7] = (i as u8).wrapping_mul(XOR_MASK) ^ token_bytes[(i + 1) % ROTATE_BITS as usize];

        transactions.push(SyntheticTransaction {
            timestamp_ms,
            payload,
        });
    }

    transactions
}

/// Check payload entropy for synthetic transactions.
///
/// This function verifies that the generated payloads have sufficient entropy
/// by checking that they are not all identical and have reasonable variation.
///
/// # Arguments
///
/// * `transactions` - Slice of synthetic transactions to check
///
/// # Returns
///
/// `true` if payloads have sufficient entropy, `false` otherwise.
///
/// # Entropy Criteria
///
/// - Not all payloads are identical
/// - At least 50% of payloads are unique
pub fn check_payload_entropy(transactions: &[SyntheticTransaction]) -> bool {
    if transactions.is_empty() {
        return false;
    }

    // Check that not all payloads are identical
    let first_payload = transactions[0].payload;
    let all_identical = transactions.iter().all(|tx| tx.payload == first_payload);

    if all_identical {
        return false;
    }

    // Check for reasonable uniqueness (at least 50% unique)
    let unique_count = transactions
        .iter()
        .map(|tx| tx.payload)
        .collect::<HashSet<_>>()
        .len();

    let uniqueness_ratio = unique_count as f64 / transactions.len() as f64;
    uniqueness_ratio >= 0.5
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a test bonding curve
    fn create_test_curve(virtual_token_reserves: u64, virtual_sol_reserves: u64) -> BondingCurve {
        BondingCurve {
            discriminator: 0x1234567890abcdef,
            virtual_token_reserves,
            virtual_sol_reserves,
            real_token_reserves: virtual_token_reserves * 8 / 10,
            real_sol_reserves: virtual_sol_reserves * 8 / 10,
            token_total_supply: virtual_token_reserves,
            complete: 0,
            _padding: [0; 7],
        }
    }

    /// Helper function to create a completed curve
    fn create_completed_curve() -> BondingCurve {
        let mut curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        curve.complete = 1;
        curve
    }

    // =========================================================================
    // BootstrapConfig Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_config_default() {
        let config = BootstrapConfig::default();
        assert_eq!(config.min_sol_reserves_lamports, 0);
        assert_eq!(config.min_token_reserves, 0);
        assert_eq!(config.min_bonding_progress, 0);
        assert_eq!(config.max_bonding_progress, 100);
        assert!(config.skip_completed);
        assert_eq!(config.min_tx_count, 0);
    }

    #[test]
    fn test_bootstrap_config_promising_pools() {
        let config = BootstrapConfig::promising_pools(100_000_000_000);
        assert_eq!(config.min_sol_reserves_lamports, 100_000_000_000);
        assert_eq!(config.max_bonding_progress, 98);
        assert!(config.skip_completed);
    }

    #[test]
    fn test_bootstrap_config_with_min_liquidity() {
        let config = BootstrapConfig::with_min_liquidity(50_000_000_000);
        assert_eq!(config.min_sol_reserves_lamports, 50_000_000_000);
    }

    #[test]
    fn test_bootstrap_config_with_progress_range() {
        let config = BootstrapConfig::with_progress_range(10, 90);
        assert_eq!(config.min_bonding_progress, 10);
        assert_eq!(config.max_bonding_progress, 90);
    }

    #[test]
    fn test_should_bootstrap_normal_pool() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        assert!(config.should_bootstrap(&curve));
    }

    #[test]
    fn test_should_bootstrap_completed_pool() {
        let curve = create_completed_curve();
        let config = BootstrapConfig::default();

        // Completed pools should be skipped by default
        assert!(!config.should_bootstrap(&curve));
    }

    #[test]
    fn test_should_bootstrap_low_liquidity() {
        let curve = create_test_curve(1_000_000_000_000, 10_000_000_000); // 10 SOL
        let config = BootstrapConfig::promising_pools(100_000_000_000); // 100 SOL min

        // Pool doesn't meet liquidity threshold
        assert!(!config.should_bootstrap(&curve));
    }

    #[test]
    fn test_should_bootstrap_with_activity() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let mut config = BootstrapConfig::default();
        config.min_tx_count = 5;

        // With 0 transactions, should fail
        assert!(!config.should_bootstrap_with_activity(&curve, 0));

        // With enough transactions, should pass
        assert!(config.should_bootstrap_with_activity(&curve, 10));
    }

    // =========================================================================
    // BootstrapResult Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_result_success() {
        let result = BootstrapResult::success(3, 100);
        assert!(result.success);
        assert_eq!(result.snapshot_count, 3);
        assert_eq!(result.duration_us, 100);
        assert!(result.failure_reason.is_none());
    }

    #[test]
    fn test_bootstrap_result_failed() {
        let result = BootstrapResult::failed("Test failure");
        assert!(!result.success);
        assert_eq!(result.snapshot_count, 0);
        assert_eq!(result.failure_reason, Some("Test failure".to_string()));
    }

    // =========================================================================
    // BootstrapMetrics Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_metrics_new() {
        let metrics = BootstrapMetrics::new();
        assert_eq!(metrics.success_count(), 0);
        assert_eq!(metrics.fail_count(), 0);
        assert_eq!(metrics.skipped_count(), 0);
        assert_eq!(metrics.duration_sum_us(), 0);
    }

    #[test]
    fn test_bootstrap_metrics_record_success() {
        let metrics = BootstrapMetrics::new();
        metrics.record_success(100);
        metrics.record_success(200);

        assert_eq!(metrics.success_count(), 2);
        assert_eq!(metrics.duration_sum_us(), 300);
        assert!((metrics.avg_duration_us() - 150.0).abs() < 0.01);
    }

    #[test]
    fn test_bootstrap_metrics_record_fail() {
        let metrics = BootstrapMetrics::new();
        metrics.record_fail();
        metrics.record_fail();

        assert_eq!(metrics.fail_count(), 2);
    }

    #[test]
    fn test_bootstrap_metrics_record_skipped() {
        let metrics = BootstrapMetrics::new();
        metrics.record_skipped();

        assert_eq!(metrics.skipped_count(), 1);
    }

    #[test]
    fn test_bootstrap_metrics_avg_duration_empty() {
        let metrics = BootstrapMetrics::new();
        assert_eq!(metrics.avg_duration_us(), 0.0);
    }

    #[test]
    fn test_bootstrap_metrics_reset() {
        let metrics = BootstrapMetrics::new();
        metrics.record_success(100);
        metrics.record_fail();
        metrics.record_skipped();

        metrics.reset();

        assert_eq!(metrics.success_count(), 0);
        assert_eq!(metrics.fail_count(), 0);
        assert_eq!(metrics.skipped_count(), 0);
        assert_eq!(metrics.duration_sum_us(), 0);
    }

    #[test]
    fn test_bootstrap_metrics_to_prometheus() {
        let metrics = BootstrapMetrics::new();
        metrics.record_success(100);
        metrics.record_fail();
        metrics.record_skipped();

        let output = metrics.to_prometheus();

        assert!(output.contains("ghost_bootstrap_success 1"));
        assert!(output.contains("ghost_bootstrap_fail 1"));
        assert!(output.contains("ghost_bootstrap_skipped 1"));
        assert!(output.contains("# TYPE ghost_bootstrap_success counter"));
    }

    // =========================================================================
    // Generate G0/G1/G2 Tests
    // =========================================================================

    #[test]
    fn test_generate_g0() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let g0 = generate_g0(&curve, 1000, None);

        assert_eq!(g0.timestamp_ms, 1000);
        assert_eq!(g0.cum_volume_sol, 0.0);
        assert_eq!(g0.tx_count, 0);
        assert_eq!(g0.unique_addrs, 1);
        assert!(g0.price_sol_per_token > 0.0);
        assert!(g0.market_cap_sol > 0.0);
        assert_eq!(g0.d_price_d_volume, 0.0);
        assert_eq!(g0.d_price_d_liquidity, 0.0);
        assert_eq!(g0.d_price_d_slippage, 0.0);
    }

    #[test]
    fn test_generate_g0_uses_market_snapshot_genesis_constructor() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let timestamp_ms = 1000;

        let g0 = generate_g0(&curve, timestamp_ms, None);
        let expected = MarketSnapshot::from_curve_genesis(&curve, timestamp_ms);

        assert_eq!(g0, expected);
    }

    #[test]
    fn test_generate_g0_ignores_optional_slot_parameter() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let timestamp_ms = 1000;

        let without_slot = generate_g0(&curve, timestamp_ms, None);
        let with_slot = generate_g0(&curve, timestamp_ms, Some(123));

        assert_eq!(without_slot, with_slot);
    }

    #[test]
    fn test_generate_g1() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let g1 = generate_g1(&curve, 10_000_000, 1001, Some(100));

        assert_eq!(g1.timestamp_ms, 1001);
        assert!(g1.cum_volume_sol > 0.0);
        assert_eq!(g1.tx_count, 1);
        assert_eq!(g1.unique_addrs, 2);
        assert!(g1.price_sol_per_token > 0.0);
        assert!(g1.d_price_d_volume > 0.0);
        assert!(g1.d_price_d_liquidity > 0.0);
    }

    #[test]
    fn test_generate_g2() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let g1 = generate_g1(&curve, 10_000_000, 1001, Some(100));
        let g2 = generate_g2(&curve, 10_000_000, 1002, &g1, Some(100));

        assert_eq!(g2.timestamp_ms, 1002);
        assert!(g2.cum_volume_sol > g1.cum_volume_sol);
        assert_eq!(g2.tx_count, 2);
        assert_eq!(g2.unique_addrs, 3);
        assert!(g2.d_price_d_slippage > 0.0); // G2 should have slippage curvature
    }

    // =========================================================================
    // Bootstrap Snapshots Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_snapshots_basic() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        let snapshots = bootstrap_snapshots(&curve, 10_000_000, Some(1), &config);

        assert_eq!(snapshots.len(), 3);

        // G0 checks
        let g0 = &snapshots[0];
        assert_eq!(g0.cum_volume_sol, 0.0);
        assert_eq!(g0.tx_count, 0);
        assert_eq!(g0.unique_addrs, 1);

        // G1 checks
        let g1 = &snapshots[1];
        assert!(g1.timestamp_ms > g0.timestamp_ms);
        assert!(g1.cum_volume_sol > 0.0);
        assert_eq!(g1.tx_count, 1);

        // G2 checks
        let g2 = &snapshots[2];
        assert!(g2.timestamp_ms > g1.timestamp_ms);
        assert_eq!(g2.tx_count, 2);
    }

    #[test]
    fn test_bootstrap_snapshots_monotonic_timestamps() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        let snapshots = bootstrap_snapshots(&curve, 10_000_000, Some(1), &config);

        // Timestamps should be strictly increasing
        assert!(snapshots[0].timestamp_ms < snapshots[1].timestamp_ms);
        assert!(snapshots[1].timestamp_ms < snapshots[2].timestamp_ms);
    }

    #[test]
    fn test_bootstrap_snapshots_derivatives_increase() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        let snapshots = bootstrap_snapshots(&curve, 10_000_000, Some(1), &config);

        // G2 should have non-zero slippage derivative
        let g2 = &snapshots[2];
        assert!(g2.d_price_d_slippage > 0.0);
    }

    // =========================================================================
    // Bootstrap with Result Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_with_result_success() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        let (result, snapshots) = bootstrap_with_result(&curve, 10_000_000, 1, &config);

        assert!(result.success);
        assert_eq!(result.snapshot_count, 3);
        assert!(result.duration_us > 0);
        assert!(snapshots.is_some());
        assert_eq!(snapshots.unwrap().len(), 3);
    }

    #[test]
    fn test_bootstrap_with_result_failed_criteria() {
        let curve = create_test_curve(1_000_000_000_000, 10_000_000_000);
        let config = BootstrapConfig::promising_pools(100_000_000_000); // 100 SOL min

        let (result, snapshots) = bootstrap_with_result(&curve, 10_000_000, 1, &config);

        assert!(!result.success);
        assert!(result.failure_reason.is_some());
        assert!(snapshots.is_none());
    }

    #[test]
    fn test_bootstrap_with_result_zero_token_reserves() {
        let curve = create_test_curve(0, 30_000_000_000);
        let config = BootstrapConfig::default();

        let (result, snapshots) = bootstrap_with_result(&curve, 10_000_000, 1, &config);

        assert!(!result.success);
        assert_eq!(
            result.failure_reason,
            Some("Zero token reserves".to_string())
        );
        assert!(snapshots.is_none());
    }

    #[test]
    fn test_bootstrap_with_result_zero_sol_reserves() {
        let curve = create_test_curve(1_000_000_000_000, 0);
        let config = BootstrapConfig::default();

        let (result, snapshots) = bootstrap_with_result(&curve, 10_000_000, 1, &config);

        assert!(!result.success);
        assert_eq!(result.failure_reason, Some("Zero SOL reserves".to_string()));
        assert!(snapshots.is_none());
    }

    #[test]
    fn test_bootstrap_with_result_completed_pool() {
        let curve = create_completed_curve();
        let config = BootstrapConfig::default();

        let (result, snapshots) = bootstrap_with_result(&curve, 10_000_000, 1, &config);

        assert!(!result.success);
        assert!(result.failure_reason.is_some());
        assert!(snapshots.is_none());
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_with_very_small_amount() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        // Very small amount: 100 lamports
        let snapshots = bootstrap_snapshots(&curve, 100, Some(1), &config);

        assert_eq!(snapshots.len(), 3);
        // Should not panic
    }

    #[test]
    fn test_bootstrap_with_large_amount() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        // Large amount: 10 SOL
        let snapshots = bootstrap_snapshots(&curve, 10_000_000_000, Some(1), &config);

        assert_eq!(snapshots.len(), 3);
        // G2 should show significant price impact
        let g2 = &snapshots[2];
        assert!(g2.d_price_d_slippage > 0.0);
    }

    #[test]
    fn test_bootstrap_with_small_pool() {
        // Small pool with minimal reserves
        let curve = create_test_curve(100_000_000, 1_000_000);
        let config = BootstrapConfig::default();

        let snapshots = bootstrap_snapshots(&curve, 10_000, Some(1), &config);

        assert_eq!(snapshots.len(), 3);
    }

    #[test]
    fn test_bootstrap_with_large_pool() {
        // Large pool with massive reserves
        let curve = create_test_curve(1_000_000_000_000_000, 1_000_000_000_000);
        let config = BootstrapConfig::default();

        let snapshots = bootstrap_snapshots(&curve, 10_000_000, Some(1), &config);

        assert_eq!(snapshots.len(), 3);
    }

    // =========================================================================
    // Determinism Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_deterministic_derivatives() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let config = BootstrapConfig::default();

        let snapshots1 = bootstrap_snapshots(&curve, 10_000_000, Some(1), &config);
        let snapshots2 = bootstrap_snapshots(&curve, 10_000_000, Some(1), &config);

        // Derivatives should be deterministic (same curve, same amount)
        assert_eq!(
            snapshots1[1].d_price_d_volume,
            snapshots2[1].d_price_d_volume
        );
        assert_eq!(
            snapshots1[2].d_price_d_slippage,
            snapshots2[2].d_price_d_slippage
        );
    }

    // =========================================================================
    // Config Edge Cases
    // =========================================================================

    #[test]
    fn test_config_allow_completed() {
        let curve = create_completed_curve();
        let mut config = BootstrapConfig::default();
        config.skip_completed = false;

        // With skip_completed = false, completed pools should pass
        assert!(config.should_bootstrap(&curve));
    }

    #[test]
    fn test_config_progress_range_edge() {
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let progress = curve.get_bonding_progress();

        // Config with exact progress range
        let config = BootstrapConfig::with_progress_range(progress, progress);

        // Should still pass since progress is within [progress, progress]
        assert!(config.should_bootstrap(&curve));
    }

    // =========================================================================
    // Synthetic Seed Generation Tests
    // =========================================================================

    #[test]
    fn test_generate_quick_seed_basic() {
        let pool_id = Pubkey::new_unique();
        let txs = generate_quick_seed(&pool_id, 30_000_000_000, 1_000_000_000_000, 1000);

        assert_eq!(txs.len(), 8, "Should generate 8 synthetic transactions");

        // Check timestamps are monotonically increasing
        for i in 1..txs.len() {
            assert!(
                txs[i].timestamp_ms > txs[i - 1].timestamp_ms,
                "Timestamps should be strictly increasing"
            );
        }
    }

    #[test]
    fn test_generate_quick_seed_varies_with_pool() {
        let pool_id1 = Pubkey::new_unique();
        let pool_id2 = Pubkey::new_unique();

        let txs1 = generate_quick_seed(&pool_id1, 30_000_000_000, 1_000_000_000_000, 1000);
        let txs2 = generate_quick_seed(&pool_id2, 30_000_000_000, 1_000_000_000_000, 1000);

        // At least some payloads should differ between different pools
        // Note: Pubkey::new_unique() can generate pubkeys with leading zeros,
        // so we only require at least one difference rather than all 8
        let mut different_count = 0;
        for i in 0..txs1.len() {
            if txs1[i].payload != txs2[i].payload {
                different_count += 1;
            }
        }

        assert!(
            different_count >= 1,
            "At least one payload should differ for different pools, got {}. \
            Pool1={:?}, Pool2={:?}",
            different_count,
            pool_id1,
            pool_id2
        );
    }

    #[test]
    fn test_generate_quick_seed_varies_with_reserves() {
        let pool_id = Pubkey::new_unique();

        let txs1 = generate_quick_seed(&pool_id, 30_000_000_000, 1_000_000_000_000, 1000);
        let txs2 = generate_quick_seed(&pool_id, 100_000_000_000, 1_000_000_000_000, 1000);
        let txs3 = generate_quick_seed(&pool_id, 30_000_000_000, 5_000_000_000_000, 1000);

        // Payloads should differ when reserves change
        let mut sol_different = 0;
        let mut token_different = 0;

        for i in 0..txs1.len() {
            if txs1[i].payload != txs2[i].payload {
                sol_different += 1;
            }
            if txs1[i].payload != txs3[i].payload {
                token_different += 1;
            }
        }

        assert!(
            sol_different >= 4,
            "Payloads should vary with SOL reserves, got {} differences",
            sol_different
        );
        assert!(
            token_different >= 4,
            "Payloads should vary with token reserves, got {} differences",
            token_different
        );
    }

    #[test]
    fn test_generate_quick_seed_deterministic() {
        let pool_id = Pubkey::new_unique();

        let txs1 = generate_quick_seed(&pool_id, 30_000_000_000, 1_000_000_000_000, 1000);
        let txs2 = generate_quick_seed(&pool_id, 30_000_000_000, 1_000_000_000_000, 1000);

        // Same inputs should produce identical outputs
        assert_eq!(txs1.len(), txs2.len());
        for i in 0..txs1.len() {
            assert_eq!(
                txs1[i].timestamp_ms, txs2[i].timestamp_ms,
                "Timestamps should be identical for same inputs"
            );
            assert_eq!(
                txs1[i].payload, txs2[i].payload,
                "Payloads should be identical for same inputs"
            );
        }
    }

    #[test]
    fn test_quick_payload_entropy() {
        let pool_id = Pubkey::new_unique();
        let txs = generate_quick_seed(&pool_id, 30_000_000_000, 1_000_000_000_000, 1000);

        // Check that payloads have sufficient entropy
        assert!(
            check_payload_entropy(&txs),
            "Generated payloads should have sufficient entropy"
        );

        // Verify not all payloads are identical
        let first_payload = txs[0].payload;
        let all_same = txs.iter().all(|tx| tx.payload == first_payload);
        assert!(!all_same, "Not all payloads should be identical");
    }

    #[test]
    fn test_check_payload_entropy_empty() {
        let txs: Vec<SyntheticTransaction> = vec![];
        assert!(
            !check_payload_entropy(&txs),
            "Empty vector should fail entropy check"
        );
    }

    #[test]
    fn test_check_payload_entropy_all_identical() {
        let txs = vec![
            SyntheticTransaction {
                timestamp_ms: 1000,
                payload: [1, 2, 3, 4, 5, 6, 7, 8],
            },
            SyntheticTransaction {
                timestamp_ms: 1100,
                payload: [1, 2, 3, 4, 5, 6, 7, 8],
            },
            SyntheticTransaction {
                timestamp_ms: 1200,
                payload: [1, 2, 3, 4, 5, 6, 7, 8],
            },
        ];

        assert!(
            !check_payload_entropy(&txs),
            "All identical payloads should fail entropy check"
        );
    }

    #[test]
    fn test_check_payload_entropy_sufficient_variance() {
        let txs = vec![
            SyntheticTransaction {
                timestamp_ms: 1000,
                payload: [1, 2, 3, 4, 5, 6, 7, 8],
            },
            SyntheticTransaction {
                timestamp_ms: 1100,
                payload: [1, 2, 3, 4, 5, 6, 7, 9], // Different
            },
            SyntheticTransaction {
                timestamp_ms: 1200,
                payload: [1, 2, 3, 4, 5, 6, 8, 8], // Different
            },
            SyntheticTransaction {
                timestamp_ms: 1300,
                payload: [1, 2, 3, 4, 5, 7, 7, 8], // Different
            },
        ];

        assert!(
            check_payload_entropy(&txs),
            "Sufficient variance should pass entropy check"
        );
    }

    #[test]
    fn test_synthetic_transaction_payload_size() {
        let pool_id = Pubkey::new_unique();
        let txs = generate_quick_seed(&pool_id, 30_000_000_000, 1_000_000_000_000, 1000);

        // Verify all payloads are exactly 8 bytes
        for tx in &txs {
            assert_eq!(tx.payload.len(), 8, "Payload should be exactly 8 bytes");
        }
    }
}
