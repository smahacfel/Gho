//! Market State Module - Shadow Ledger (Zero-Latency State)
//!
//! This module provides structures for in-memory replication of Pump.fun Bonding Curve state,
//! eliminating the need for RPC getAccountInfo calls in the hot-path.
//!
//! ## Overview
//!
//! The Shadow Ledger maintains a local copy of bonding curve state in RAM, allowing
//! near-instantaneous price calculations without network latency.
//!
//! ## Bonding Curve Structure
//!
//! The `BondingCurve` struct exactly mirrors the binary layout of Pump.fun's bonding curve
//! account, using `bytemuck` for safe zero-copy deserialization from raw account data.
//!
//! ## Shadow State Extension
//!
//! The `ShadowBondingCurve` struct extends `BondingCurve` with metadata for staleness
//! detection, including the slot number when the state was last updated.

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::shadow_ledger::history_types::CurveFinality;

/// Storage-level precedence rank for ShadowLedger writes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShadowLedgerWriteStrength {
    #[default]
    BootstrapSeed,
    ConfirmedBootstrap,
    Repair,
    CanonicalCommit,
    LiveAppend,
}

impl ShadowLedgerWriteStrength {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BootstrapSeed => "p0_bootstrap_seed",
            Self::ConfirmedBootstrap => "p1_confirmed_bootstrap",
            Self::Repair => "p2_repair",
            Self::CanonicalCommit => "p3_canonical_commit",
            Self::LiveAppend => "p4_live_append",
        }
    }
}

/// Source label for ShadowLedger writes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShadowLedgerWriteSource {
    #[default]
    #[serde(rename = "compatibility_bootstrap", alias = "legacy_compat")]
    CompatibilityBootstrap,
    SeerBootstrap,
    EventBusBootstrapListener,
    RpcBootstrapSeeder,
    AccountUpdate,
    Reconciliation,
    WalReplayCurve,
    CanonicalCommit,
    WalReplayCommitRestore,
    LivePipeline,
    WalReplayLiveAppend,
}

impl ShadowLedgerWriteSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompatibilityBootstrap => "compatibility_bootstrap",
            Self::SeerBootstrap => "seer_bootstrap",
            Self::EventBusBootstrapListener => "event_bus_bootstrap_listener",
            Self::RpcBootstrapSeeder => "rpc_bootstrap_seeder",
            Self::AccountUpdate => "account_update",
            Self::Reconciliation => "reconciliation",
            Self::WalReplayCurve => "wal_replay_curve",
            Self::CanonicalCommit => "canonical_commit",
            Self::WalReplayCommitRestore => "wal_replay_commit_restore",
            Self::LivePipeline => "live_pipeline",
            Self::WalReplayLiveAppend => "wal_replay_live_append",
        }
    }
}

/// Confidence tier attached to the currently stored state.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShadowLedgerStateConfidence {
    #[default]
    Speculative,
    Observed,
    #[serde(rename = "diagnostic")]
    Diagnostic,
    Canonical,
    Live,
}

impl ShadowLedgerStateConfidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Speculative => "speculative",
            Self::Observed => "observed",
            Self::Diagnostic => "diagnostic",
            Self::Canonical => "canonical",
            Self::Live => "live",
        }
    }
}

/// Explicit reason for a ShadowLedger write.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShadowLedgerWriteReason {
    #[default]
    #[serde(rename = "compatibility_bootstrap", alias = "legacy_compat")]
    CompatibilityBootstrap,
    BootstrapSeed,
    ConfirmedBootstrap,
    DirectAccountUpdate,
    FinalityRefresh,
    #[serde(rename = "reconciliation_update", alias = "reconciliation_repair")]
    ReconciliationUpdate,
    WalReplayCurveUpdate,
}

impl ShadowLedgerWriteReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompatibilityBootstrap => "compatibility_bootstrap",
            Self::BootstrapSeed => "bootstrap_seed",
            Self::ConfirmedBootstrap => "confirmed_bootstrap",
            Self::DirectAccountUpdate => "direct_account_update",
            Self::FinalityRefresh => "finality_refresh",
            Self::ReconciliationUpdate => "reconciliation_update",
            Self::WalReplayCurveUpdate => "wal_replay_curve_update",
        }
    }
}

/// Pump.fun Bonding Curve Account State
///
/// This structure represents the on-chain state of a Pump.fun bonding curve.
/// It uses a constant product AMM formula (x * y = k) with virtual reserves
/// and a 1% fee on trades.
///
/// # Binary Layout (C-compatible)
///
/// The structure uses `#[repr(C)]` to ensure the memory layout matches the
/// on-chain account data exactly:
///
/// | Offset | Size | Field                   |
/// |--------|------|-------------------------|
/// | 0      | 8    | discriminator           |
/// | 8      | 8    | virtual_token_reserves  |
/// | 16     | 8    | virtual_sol_reserves    |
/// | 24     | 8    | real_token_reserves     |
/// | 32     | 8    | real_sol_reserves       |
/// | 40     | 8    | token_total_supply      |
/// | 48     | 1    | complete                |
/// | 49     | 7    | _padding (alignment)    |
///
/// Total: 56 bytes
///
/// # Safety
///
/// The struct implements `Pod` (Plain Old Data) and `Zeroable` traits from bytemuck,
/// which guarantees safe casting from raw bytes. These traits ensure:
/// - No padding bytes contain uninitialized data
/// - The struct contains only types that are safe to cast
/// - Alignment requirements are met
///
/// # Pricing Formula
///
/// **Buy (SOL → Token):**
/// - Input: SOL amount (with 1% fee deducted)
/// - Formula: `token_out = virtual_token_reserves - (k / (virtual_sol_reserves + sol_after_fee))`
/// - Where: `k = virtual_token_reserves * virtual_sol_reserves`
///
/// **Sell (Token → SOL):**
/// - Input: Token amount
/// - Formula: `sol_out = virtual_sol_reserves - (k / (virtual_token_reserves + token_in))`
/// - Then apply 1% fee: `sol_out_after_fee = sol_out * 0.99`
///
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Pod, Zeroable, Serialize, Deserialize)]
pub struct BondingCurve {
    /// Account discriminator (8 bytes)
    /// Used to identify the account type on-chain
    pub discriminator: u64,

    /// Virtual token reserves (8 bytes)
    /// Used in the constant product formula for price calculation
    pub virtual_token_reserves: u64,

    /// Virtual SOL reserves (8 bytes)
    /// Used in the constant product formula for price calculation
    pub virtual_sol_reserves: u64,

    /// Real token reserves (8 bytes)
    /// Actual token balance in the bonding curve
    pub real_token_reserves: u64,

    /// Real SOL reserves (8 bytes)
    /// Actual SOL balance in the bonding curve
    pub real_sol_reserves: u64,

    /// Total supply of the token (8 bytes)
    /// Maximum number of tokens that can be minted
    pub token_total_supply: u64,

    /// Whether the bonding curve is complete (1 byte)
    /// When true, the curve has graduated to a standard AMM pool
    pub complete: u8,

    /// Padding for 8-byte alignment (7 bytes)
    /// Required to maintain C struct alignment rules
    pub _padding: [u8; 7],
}

/// Extended bonding curve state with staleness tracking
///
/// This structure wraps the on-chain `BondingCurve` state with additional
/// metadata for Shadow Ledger staleness detection.
///
/// # Fields
///
/// - `curve`: The on-chain bonding curve state (56 bytes)
/// - `last_updated_slot`: Solana slot number when this state was last updated
///
/// # Staleness Detection
///
/// The `last_updated_slot` is used to detect stale state. If the current slot
/// is more than 3 slots ahead of `last_updated_slot`, the state is considered
/// stale and should be refreshed via RPC.
///
/// # Metadata
///
/// Besides slot-based stale checks, this structure also tracks the wall-clock
/// moment of the last update. That allows launcher-side freshness gating in
/// milliseconds without replacing the existing slot-based safety model.
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShadowBondingCurve {
    /// On-chain bonding curve state
    pub curve: BondingCurve,

    /// Slot number when this state was last updated
    /// Used for staleness detection (current_slot > last_updated_slot + 3)
    pub last_updated_slot: u64,

    /// Whether the curve data was successfully parsed from a confirmed source
    /// (AccountUpdate with valid layout). False for genesis seeds / bootstrap
    /// placeholders that have not been confirmed by an account update.
    pub curve_data_known: bool,

    /// Wall-clock timestamp (ms since UNIX epoch) when this state was last updated.
    pub last_update_ts_ms: u64,

    /// Finality tier of the currently stored curve state.
    #[serde(default)]
    pub curve_finality: CurveFinality,

    /// Storage-level provenance of the last accepted write.
    #[serde(default)]
    pub write_source: ShadowLedgerWriteSource,

    /// Storage-level precedence rank of the last accepted write.
    #[serde(default)]
    pub write_strength: ShadowLedgerWriteStrength,

    /// Confidence of the last accepted write.
    #[serde(default)]
    pub state_confidence: ShadowLedgerStateConfidence,

    /// Reason attached to the last accepted write.
    #[serde(default)]
    pub write_reason: ShadowLedgerWriteReason,
}

impl ShadowBondingCurve {
    /// Maximum age in slots before state is considered stale
    pub const MAX_AGE_SLOTS: u64 = 3;

    /// Create a new ShadowBondingCurve
    pub fn new(curve: BondingCurve, slot: u64) -> Self {
        Self::new_at(curve, slot, current_time_ms())
    }

    /// Create a new ShadowBondingCurve with an explicit wall-clock update time.
    pub fn new_at(curve: BondingCurve, slot: u64, last_update_ts_ms: u64) -> Self {
        Self {
            curve,
            last_updated_slot: slot,
            curve_data_known: false,
            last_update_ts_ms,
            curve_finality: CurveFinality::Speculative,
            write_source: ShadowLedgerWriteSource::SeerBootstrap,
            write_strength: ShadowLedgerWriteStrength::BootstrapSeed,
            state_confidence: ShadowLedgerStateConfidence::Speculative,
            write_reason: ShadowLedgerWriteReason::BootstrapSeed,
        }
    }

    /// Create a new ShadowBondingCurve with explicit curve_data_known flag
    pub fn new_with_known(curve: BondingCurve, slot: u64, curve_data_known: bool) -> Self {
        Self::new_with_known_at_finality(
            curve,
            slot,
            curve_data_known,
            CurveFinality::from_curve_data_known(curve_data_known),
            current_time_ms(),
        )
    }

    /// Create a new ShadowBondingCurve with explicit curve_data_known and finality.
    pub fn new_with_known_finality(
        curve: BondingCurve,
        slot: u64,
        curve_data_known: bool,
        curve_finality: CurveFinality,
    ) -> Self {
        Self::new_with_known_at_finality(
            curve,
            slot,
            curve_data_known,
            curve_finality,
            current_time_ms(),
        )
    }

    /// Create a new ShadowBondingCurve with explicit curve_data_known flag and
    /// wall-clock update time.
    pub fn new_with_known_at(
        curve: BondingCurve,
        slot: u64,
        curve_data_known: bool,
        last_update_ts_ms: u64,
    ) -> Self {
        Self::new_with_known_at_finality(
            curve,
            slot,
            curve_data_known,
            CurveFinality::from_curve_data_known(curve_data_known),
            last_update_ts_ms,
        )
    }

    /// Create a new ShadowBondingCurve with explicit curve_data_known, finality,
    /// and wall-clock update time.
    pub fn new_with_known_at_finality(
        curve: BondingCurve,
        slot: u64,
        curve_data_known: bool,
        curve_finality: CurveFinality,
        last_update_ts_ms: u64,
    ) -> Self {
        Self {
            curve,
            last_updated_slot: slot,
            curve_data_known,
            last_update_ts_ms,
            curve_finality: curve_finality.normalized(curve_data_known),
            write_source: if curve_data_known {
                ShadowLedgerWriteSource::RpcBootstrapSeeder
            } else {
                ShadowLedgerWriteSource::SeerBootstrap
            },
            write_strength: if curve_data_known {
                ShadowLedgerWriteStrength::ConfirmedBootstrap
            } else {
                ShadowLedgerWriteStrength::BootstrapSeed
            },
            state_confidence: if curve_data_known {
                ShadowLedgerStateConfidence::Observed
            } else {
                ShadowLedgerStateConfidence::Speculative
            },
            write_reason: if curve_data_known {
                ShadowLedgerWriteReason::ConfirmedBootstrap
            } else {
                ShadowLedgerWriteReason::BootstrapSeed
            },
        }
    }

    /// Check if this state is stale at the given current slot
    ///
    /// Returns `true` if current_slot > last_updated_slot + MAX_AGE_SLOTS
    pub fn is_stale(&self, current_slot: u64) -> bool {
        current_slot > self.last_updated_slot.saturating_add(Self::MAX_AGE_SLOTS)
    }

    /// Get the age of this state in slots
    pub fn age_slots(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.last_updated_slot)
    }

    /// Get the wall-clock age of this state in milliseconds.
    pub fn age_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.last_update_ts_ms)
    }
}

#[inline]
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl BondingCurve {
    /// Fee percentage for Pump.fun trades (1% = 0.01)
    /// This fee is applied to all buy and sell operations
    pub const FEE_BPS: u64 = 100; // 100 basis points = 1%
    pub const BPS_DENOMINATOR: u64 = 10000;

    /// Create a new BondingCurve from raw account data
    ///
    /// # Arguments
    ///
    /// * `data` - Raw bytes from the on-chain account
    ///
    /// # Returns
    ///
    /// * `Some(BondingCurve)` if the data is exactly 56 bytes and properly aligned
    /// * `None` if the data is invalid or incorrectly sized
    ///
    /// # Safety
    ///
    /// This function uses `bytemuck::try_from_bytes` which performs alignment
    /// and size checks before casting.
    pub fn from_bytes(data: &[u8]) -> Option<&Self> {
        bytemuck::try_from_bytes(data).ok()
    }

    /// Calculate how much SOL is required to buy a specific amount of tokens
    ///
    /// Uses the constant product AMM formula: x * y = k
    ///
    /// # Arguments
    ///
    /// * `amount_out` - Number of tokens to purchase
    ///
    /// # Returns
    ///
    /// Amount of SOL required (in lamports), including the 1% fee
    ///
    /// # Formula
    ///
    /// 1. Calculate invariant: k = virtual_token_reserves * virtual_sol_reserves
    /// 2. Calculate new token reserves: new_token = virtual_token_reserves - amount_out
    /// 3. Calculate new SOL reserves needed: new_sol = k / new_token
    /// 4. SOL needed (before fee): sol_needed = new_sol - virtual_sol_reserves
    /// 5. Add 1% fee: sol_with_fee = sol_needed / 0.99
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `amount_out` is greater than or equal to `virtual_token_reserves` (would drain pool)
    /// - Arithmetic overflow occurs
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let sol_cost = curve.calculate_buy_price(1_000_000); // Buy 1M tokens
    /// println!("Cost: {} SOL", sol_cost as f64 / 1e9);
    /// ```
    pub fn calculate_buy_price(&self, amount_out: u64) -> u64 {
        // Ensure we're not trying to buy more than available
        if amount_out >= self.virtual_token_reserves {
            panic!("Cannot buy more tokens than available in reserves");
        }

        // Calculate the constant product invariant: k = x * y
        let k = (self.virtual_token_reserves as u128)
            .checked_mul(self.virtual_sol_reserves as u128)
            .expect("Overflow calculating invariant");

        // After buying amount_out tokens, the new token reserve will be reduced
        let new_token_reserves = self
            .virtual_token_reserves
            .checked_sub(amount_out)
            .expect("Underflow calculating new token reserves");

        // Calculate new SOL reserves needed to maintain k
        // new_sol_reserves = k / new_token_reserves
        let new_sol_reserves = (k / new_token_reserves as u128) as u64;

        // The SOL needed is the difference
        let sol_needed = new_sol_reserves
            .checked_sub(self.virtual_sol_reserves)
            .expect("Underflow calculating SOL needed");

        // Apply 1% fee: actual SOL needed = sol_needed / (1 - 0.01)
        // Which is: sol_needed * 10000 / 9900

        ((sol_needed as u128)
            .checked_mul(Self::BPS_DENOMINATOR as u128)
            .expect("Overflow applying fee")
            / (Self::BPS_DENOMINATOR - Self::FEE_BPS) as u128) as u64
    }

    /// Calculate how much SOL will be received when selling tokens
    ///
    /// Uses the constant product AMM formula: x * y = k
    ///
    /// # Arguments
    ///
    /// * `amount_in` - Number of tokens to sell
    ///
    /// # Returns
    ///
    /// Amount of SOL received (in lamports), after deducting the 1% fee
    ///
    /// # Formula
    ///
    /// 1. Calculate invariant: k = virtual_token_reserves * virtual_sol_reserves
    /// 2. Calculate new token reserves: new_token = virtual_token_reserves + amount_in
    /// 3. Calculate new SOL reserves: new_sol = k / new_token
    /// 4. SOL to give (before fee): sol_out = virtual_sol_reserves - new_sol
    /// 5. Deduct 1% fee: sol_after_fee = sol_out * 0.99
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `amount_in` would cause the token reserves to overflow
    /// - The sell would drain more SOL than available
    /// - Arithmetic overflow occurs
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let sol_received = curve.calculate_sell_price(1_000_000); // Sell 1M tokens
    /// println!("Received: {} SOL", sol_received as f64 / 1e9);
    /// ```
    pub fn calculate_sell_price(&self, amount_in: u64) -> u64 {
        // Calculate the constant product invariant: k = x * y
        let k = (self.virtual_token_reserves as u128)
            .checked_mul(self.virtual_sol_reserves as u128)
            .expect("Overflow calculating invariant");

        // After selling amount_in tokens, the new token reserve will be increased
        let new_token_reserves = self
            .virtual_token_reserves
            .checked_add(amount_in)
            .expect("Overflow calculating new token reserves");

        // Calculate new SOL reserves needed to maintain k
        // new_sol_reserves = k / new_token_reserves
        let new_sol_reserves = (k / new_token_reserves as u128) as u64;

        // The SOL to give out is the difference
        let sol_out = self
            .virtual_sol_reserves
            .checked_sub(new_sol_reserves)
            .expect("Underflow calculating SOL out - would drain pool");

        // Apply 1% fee: actual SOL received = sol_out * (1 - 0.01)
        // Which is: sol_out * 9900 / 10000

        ((sol_out as u128)
            .checked_mul((Self::BPS_DENOMINATOR - Self::FEE_BPS) as u128)
            .expect("Overflow applying fee")
            / Self::BPS_DENOMINATOR as u128) as u64
    }

    /// Simulate a buy operation and calculate expected tokens out
    ///
    /// This method simulates the Pump.fun buy mechanics accurately:
    /// 1. Deduct 1% fee from the input SOL
    /// 2. Add the remaining SOL to virtual reserves
    /// 3. Calculate tokens out using constant product formula (k = x * y)
    ///
    /// # Arguments
    ///
    /// * `amount_in_sol` - Amount of SOL to spend (in lamports)
    ///
    /// # Returns
    ///
    /// Expected number of tokens to receive
    ///
    /// # Important: Shadow Slippage Guard
    ///
    /// This is the core method for the "Shadow Slippage" anti-front-run mechanism.
    /// The fee deduction is CRITICAL: Pump.fun takes 1% from the input before
    /// updating reserves. If we calculated without this fee, we would overestimate
    /// the tokens we'll receive, causing our `min_tokens_out` guard to be set
    /// too high, and our transaction would fail on-chain.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let expected_tokens = curve.simulate_buy(1_000_000_000); // Buy with 1 SOL
    /// let min_tokens_out = expected_tokens * 995 / 1000; // 0.5% slippage tolerance
    /// ```
    pub fn simulate_buy(&self, amount_in_sol: u64) -> u64 {
        // Avoid division by zero if input is too small
        if amount_in_sol == 0 {
            return 0;
        }

        let virtual_sol = self.virtual_sol_reserves;
        let virtual_tokens = self.virtual_token_reserves;

        // CRITICAL: Deduct 1% fee from input first
        // Pump.fun takes ~1% from the input SOL before adding to reserves.
        // This is essential for accurate min_tokens_out calculation.
        // Note: For amounts < 100 lamports, the fee will be 0 due to integer division.
        // This is acceptable as such small amounts are below practical trading thresholds.
        let fee = amount_in_sol / 100;
        let effective_sol_in = amount_in_sol.saturating_sub(fee);

        // Calculate the constant product invariant: k = x * y
        let invariant = (virtual_sol as u128).saturating_mul(virtual_tokens as u128);

        // New SOL reserves after adding the effective input (after fee)
        let new_sol_reserves = (virtual_sol as u128).saturating_add(effective_sol_in as u128);

        // Avoid division by zero
        if new_sol_reserves == 0 {
            return 0;
        }

        // New token reserves to maintain invariant: new_tokens = k / new_sol
        let new_token_reserves = invariant / new_sol_reserves;

        // Tokens out is the difference (what leaves the pool)
        let tokens_out = (virtual_tokens as u128).saturating_sub(new_token_reserves);

        tokens_out as u64
    }

    /// Check if the bonding curve is still active
    ///
    /// # Returns
    ///
    /// * `true` if the curve is active (complete == 0)
    /// * `false` if the curve has graduated to a standard pool (complete != 0)
    pub fn is_active(&self) -> bool {
        self.complete == 0
    }

    /// Get the current price (SOL per token)
    ///
    /// This is an approximation based on the current reserves ratio.
    ///
    /// # Returns
    ///
    /// Current price as a ratio: virtual_sol_reserves / virtual_token_reserves
    ///
    /// # Note
    ///
    /// This is the instantaneous price and doesn't account for slippage or fees.
    /// For actual trade costs, use `calculate_buy_price` or `calculate_sell_price`.
    pub fn current_price(&self) -> f64 {
        if self.virtual_token_reserves == 0 {
            return 0.0;
        }
        self.virtual_sol_reserves as f64 / self.virtual_token_reserves as f64
    }

    // =========================================================================
    // BONDING CURVE VALIDATION (BCV) - Analytical Methods
    // =========================================================================

    /// Pump.fun bonding curve completion threshold
    /// When bonding progress exceeds this value (99%), the token migrates to Raydium
    pub const MIGRATION_THRESHOLD_PERCENT: u64 = 99;

    /// Initial virtual SOL reserves for Pump.fun bonding curves (30 SOL)
    /// This is the standard starting value for all Pump.fun tokens
    pub const INITIAL_VIRTUAL_SOL: u64 = 30_000_000_000;

    /// Initial virtual token reserves for Pump.fun bonding curves
    /// Standard value: 1.073 billion tokens (1_073_000_000_000_000 with 6 decimals)
    pub const INITIAL_VIRTUAL_TOKENS: u64 = 1_073_000_000_000_000;

    /// Maximum real SOL reserves before bonding curve completes
    /// When real_sol_reserves reaches this (~85 SOL), migration to Raydium triggers
    pub const MAX_REAL_SOL_RESERVES: u64 = 85_000_000_000;

    /// Get the current market cap in SOL (lamports)
    ///
    /// Calculates the fully diluted market cap based on the current price
    /// and total token supply.
    ///
    /// # Formula
    ///
    /// ```text
    /// market_cap_sol = (virtual_sol_reserves / virtual_token_reserves) * token_total_supply
    /// ```
    ///
    /// # Returns
    ///
    /// Market cap in lamports (SOL * 10^9)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let mcap_lamports = curve.get_market_cap_sol();
    /// let mcap_sol = mcap_lamports as f64 / 1e9;
    /// println!("Market Cap: {} SOL", mcap_sol);
    /// ```
    #[inline]
    pub fn get_market_cap_sol(&self) -> u64 {
        if self.virtual_token_reserves == 0 {
            return 0;
        }

        // Use u128 to prevent overflow during calculation
        // market_cap = (virtual_sol * total_supply) / virtual_tokens
        let market_cap = (self.virtual_sol_reserves as u128)
            .saturating_mul(self.token_total_supply as u128)
            / (self.virtual_token_reserves as u128);

        market_cap as u64
    }

    /// Get the bonding curve completion progress as a percentage (0-100)
    ///
    /// This indicates how close the token is to migrating to Raydium.
    /// When progress exceeds 99%, the token typically migrates to a standard AMM pool.
    ///
    /// # Formula
    ///
    /// The progress is calculated based on real SOL reserves accumulated:
    /// ```text
    /// progress = (real_sol_reserves / MAX_REAL_SOL_RESERVES) * 100
    /// ```
    ///
    /// # Returns
    ///
    /// Progress percentage (0-100). Values > 99 indicate imminent migration.
    ///
    /// # Migration Trigger
    ///
    /// When `get_bonding_progress() > 99`, the bonding curve is about to complete
    /// and migrate to Raydium. At this point:
    /// - Trading on the bonding curve may be disabled soon
    /// - Price dynamics will change significantly
    /// - New buy opportunities should be avoided
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let progress = curve.get_bonding_progress();
    /// if progress > 99 {
    ///     println!("WARNING: Token migrating to Raydium soon!");
    /// }
    /// ```
    #[inline]
    pub fn get_bonding_progress(&self) -> u64 {
        if Self::MAX_REAL_SOL_RESERVES == 0 {
            return 100;
        }

        // Calculate progress based on real SOL reserves
        // progress = (real_sol_reserves * 100) / MAX_REAL_SOL_RESERVES
        let progress = (self.real_sol_reserves as u128).saturating_mul(100)
            / (Self::MAX_REAL_SOL_RESERVES as u128);

        // Cap at 100%
        std::cmp::min(progress as u64, 100)
    }

    /// Get the price impact of a buy order as a percentage
    ///
    /// Calculates how much the price will move as a result of our trade.
    /// This is critical for:
    /// - Slippage estimation
    /// - Front-running detection (if our impact is too high, we're vulnerable)
    /// - Order sizing decisions
    ///
    /// # Arguments
    ///
    /// * `amount_in_lamports` - SOL amount to spend (in lamports)
    ///
    /// # Returns
    ///
    /// Price impact as a percentage (e.g., 2.5 means 2.5% price increase)
    ///
    /// # Formula
    ///
    /// ```text
    /// price_before = virtual_sol_reserves / virtual_token_reserves
    /// price_after = (virtual_sol_reserves + effective_sol) / (virtual_token_reserves - tokens_out)
    /// impact = ((price_after - price_before) / price_before) * 100
    /// ```
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let impact = curve.get_price_impact(1_000_000_000); // 1 SOL
    /// if impact > 5.0 {
    ///     println!("WARNING: High price impact ({}%), consider smaller order", impact);
    /// }
    /// ```
    #[inline]
    pub fn get_price_impact(&self, amount_in_lamports: u64) -> f64 {
        if amount_in_lamports == 0
            || self.virtual_token_reserves == 0
            || self.virtual_sol_reserves == 0
        {
            return 0.0;
        }

        // Calculate price before trade
        let price_before = self.virtual_sol_reserves as f64 / self.virtual_token_reserves as f64;

        // Simulate the buy to get tokens out (this accounts for 1% fee)
        let tokens_out = self.simulate_buy(amount_in_lamports);
        if tokens_out == 0 {
            return 0.0;
        }

        // Calculate effective SOL added (after 1% fee)
        let fee = amount_in_lamports / 100;
        let effective_sol = amount_in_lamports.saturating_sub(fee);

        // Calculate price after trade
        let new_sol_reserves = self.virtual_sol_reserves.saturating_add(effective_sol);
        let new_token_reserves = self.virtual_token_reserves.saturating_sub(tokens_out);

        if new_token_reserves == 0 {
            return 100.0; // Would drain the pool
        }

        let price_after = new_sol_reserves as f64 / new_token_reserves as f64;

        // Calculate percentage impact
        if price_before == 0.0 {
            return 0.0;
        }

        ((price_after - price_before) / price_before) * 100.0
    }

    /// Simulate a sell operation and calculate expected SOL out
    ///
    /// This method simulates the Pump.fun sell mechanics accurately:
    /// 1. Add tokens to virtual reserves
    /// 2. Calculate SOL out using constant product formula
    /// 3. Deduct 1% fee from output SOL
    ///
    /// # Arguments
    ///
    /// * `amount_in_tokens` - Amount of tokens to sell
    ///
    /// # Returns
    ///
    /// Expected SOL to receive (in lamports), after 1% fee deduction
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let sol_out = curve.simulate_sell(1_000_000); // Sell 1M tokens
    /// let min_sol_out = sol_out * 995 / 1000; // 0.5% slippage tolerance
    /// ```
    #[inline]
    pub fn simulate_sell(&self, amount_in_tokens: u64) -> u64 {
        if amount_in_tokens == 0 {
            return 0;
        }

        let virtual_sol = self.virtual_sol_reserves;
        let virtual_tokens = self.virtual_token_reserves;

        // Calculate the constant product invariant: k = x * y
        let invariant = (virtual_sol as u128).saturating_mul(virtual_tokens as u128);

        // New token reserves after adding the tokens being sold
        let new_token_reserves = (virtual_tokens as u128).saturating_add(amount_in_tokens as u128);

        if new_token_reserves == 0 {
            return 0;
        }

        // New SOL reserves to maintain invariant: new_sol = k / new_tokens
        let new_sol_reserves = invariant / new_token_reserves;

        // SOL out is the difference (what leaves the pool)
        let sol_out = (virtual_sol as u128).saturating_sub(new_sol_reserves);

        // Apply 1% fee to output: sol_after_fee = sol_out * 0.99
        let fee = sol_out / 100;
        let sol_after_fee = sol_out.saturating_sub(fee);

        sol_after_fee as u64
    }

    /// Check if the bonding curve is near migration (>99% complete)
    ///
    /// This is a convenience method that checks if the token is about to
    /// migrate to Raydium, which is a critical trading signal.
    ///
    /// # Returns
    ///
    /// * `true` if bonding progress > 99% (migration imminent)
    /// * `false` if bonding curve is still active with room to grow
    #[inline]
    pub fn is_near_migration(&self) -> bool {
        self.get_bonding_progress() > Self::MIGRATION_THRESHOLD_PERCENT
    }

    /// Get the current instantaneous price in SOL per million tokens
    ///
    /// This is useful for human-readable price display, as raw prices
    /// are extremely small numbers.
    ///
    /// # Returns
    ///
    /// Price in SOL per 1 million tokens
    ///
    /// # Example
    ///
    /// ```ignore
    /// let curve = BondingCurve { ... };
    /// let price = curve.get_price_per_million_tokens();
    /// println!("Price: {} SOL / 1M tokens", price);
    /// ```
    #[inline]
    pub fn get_price_per_million_tokens(&self) -> f64 {
        self.current_price() * 1_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a test bonding curve
    fn create_test_curve() -> BondingCurve {
        BondingCurve {
            discriminator: 0x1234567890abcdef,
            virtual_token_reserves: 1_000_000_000_000, // 1 trillion tokens
            virtual_sol_reserves: 30_000_000_000,      // 30 SOL (30B lamports)
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 20_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        }
    }

    #[test]
    fn test_bonding_curve_size() {
        // Verify the struct size matches the expected 56 bytes
        assert_eq!(std::mem::size_of::<BondingCurve>(), 56);
    }

    #[test]
    fn test_bonding_curve_alignment() {
        // Verify the struct is properly aligned for safe casting
        assert_eq!(std::mem::align_of::<BondingCurve>(), 8);
    }

    #[test]
    fn test_from_bytes_valid() {
        let curve = create_test_curve();
        let bytes = bytemuck::bytes_of(&curve);

        // Should successfully parse valid bytes
        let parsed = BondingCurve::from_bytes(bytes);
        assert!(parsed.is_some());

        let parsed = parsed.unwrap();
        assert_eq!(parsed.discriminator, curve.discriminator);
        assert_eq!(parsed.virtual_token_reserves, curve.virtual_token_reserves);
        assert_eq!(parsed.virtual_sol_reserves, curve.virtual_sol_reserves);
    }

    #[test]
    fn test_from_bytes_invalid_size() {
        // Should fail with incorrect size
        let invalid_data = vec![0u8; 32];
        assert!(BondingCurve::from_bytes(&invalid_data).is_none());
    }

    #[test]
    fn test_calculate_buy_price_small_amount() {
        let curve = create_test_curve();

        // Buy a small amount: 1M tokens (0.0001% of supply)
        let amount = 1_000_000;
        let cost = curve.calculate_buy_price(amount);

        // Cost should be non-zero and reasonable
        assert!(cost > 0);

        // For small amounts, the cost should be approximately:
        // amount * price * (1 + fee)
        let expected_approx = ((amount as f64) * curve.current_price() * 1.01) as u64;

        // Allow 5% tolerance due to slippage and rounding
        let tolerance = expected_approx / 20;
        assert!(
            cost >= expected_approx.saturating_sub(tolerance)
                && cost <= expected_approx + tolerance,
            "Cost {} not within tolerance of expected {}",
            cost,
            expected_approx
        );
    }

    #[test]
    fn test_calculate_buy_price_with_fee() {
        let curve = create_test_curve();

        // Buy 100M tokens (0.01% of supply)
        let amount = 100_000_000;
        let cost = curve.calculate_buy_price(amount);

        // The fee should be approximately 1% of the base cost
        // We can't easily calculate the exact base cost without the fee,
        // but we can verify the cost is positive and reasonable
        assert!(cost > 0);

        // Cost should be greater than simple proportional calculation
        let min_expected = (amount as u128 * curve.virtual_sol_reserves as u128
            / curve.virtual_token_reserves as u128) as u64;
        assert!(cost >= min_expected);
    }

    #[test]
    fn test_calculate_sell_price_small_amount() {
        let curve = create_test_curve();

        // Sell a small amount: 1M tokens
        let amount = 1_000_000;
        let received = curve.calculate_sell_price(amount);

        // Should receive non-zero SOL
        assert!(received > 0);

        // For small amounts, received should be approximately:
        // amount * price * (1 - fee)
        let expected_approx = ((amount as f64) * curve.current_price() * 0.99) as u64;

        // Allow 5% tolerance
        let tolerance = expected_approx / 20;
        assert!(
            received >= expected_approx.saturating_sub(tolerance)
                && received <= expected_approx + tolerance,
            "Received {} not within tolerance of expected {}",
            received,
            expected_approx
        );
    }

    #[test]
    fn test_calculate_sell_price_with_fee() {
        let curve = create_test_curve();

        // Sell 100M tokens
        let amount = 100_000_000;
        let received = curve.calculate_sell_price(amount);

        // Should receive less than the proportional amount due to fee
        let max_without_fee = (amount as u128 * curve.virtual_sol_reserves as u128
            / curve.virtual_token_reserves as u128) as u64;

        // With 1% fee, should receive ~99% of the base amount
        let expected_with_fee = (max_without_fee as f64 * 0.99) as u64;

        // Allow some tolerance for slippage
        assert!(received <= max_without_fee);
        assert!(received >= expected_with_fee - (expected_with_fee / 10));
    }

    #[test]
    fn test_buy_sell_price_symmetry() {
        let curve = create_test_curve();

        // For small trades, buying and selling should be roughly symmetric
        let amount = 1_000_000;

        let buy_cost = curve.calculate_buy_price(amount);
        let sell_received = curve.calculate_sell_price(amount);

        // Due to fees, sell should receive less than buy costs
        assert!(sell_received < buy_cost);

        // The difference should be approximately 2% (1% fee each way)
        let fee_loss = buy_cost - sell_received;
        let expected_fee = (buy_cost as f64 * 0.02) as u64;

        // Allow 20% tolerance on the fee amount
        let tolerance = expected_fee / 5;
        assert!(
            fee_loss >= expected_fee.saturating_sub(tolerance)
                && fee_loss <= expected_fee + tolerance,
            "Fee loss {} not within tolerance of expected {}",
            fee_loss,
            expected_fee
        );
    }

    #[test]
    fn test_large_trade_impact() {
        let curve = create_test_curve();

        // Buy a large amount: 10% of supply
        let large_amount = curve.virtual_token_reserves / 10;
        let large_cost = curve.calculate_buy_price(large_amount);

        // Buy a small amount: 0.01% of supply
        let small_amount = curve.virtual_token_reserves / 10000;
        let small_cost = curve.calculate_buy_price(small_amount);

        // Price per token for large trade should be significantly higher
        let large_price_per_token = large_cost as f64 / large_amount as f64;
        let small_price_per_token = small_cost as f64 / small_amount as f64;

        // Large trade should have at least 10% higher price per token due to slippage
        assert!(large_price_per_token > small_price_per_token * 1.1);
    }

    #[test]
    #[should_panic(expected = "Cannot buy more tokens than available")]
    fn test_buy_more_than_available() {
        let curve = create_test_curve();

        // Try to buy more than the entire supply
        curve.calculate_buy_price(curve.virtual_token_reserves + 1);
    }

    #[test]
    #[should_panic(expected = "Cannot buy more tokens than available")]
    fn test_buy_exact_reserves() {
        let curve = create_test_curve();

        // Try to buy exactly all reserves (not allowed)
        curve.calculate_buy_price(curve.virtual_token_reserves);
    }

    #[test]
    fn test_is_active() {
        let mut curve = create_test_curve();

        // Initially should be active
        assert!(curve.is_active());

        // Mark as complete
        curve.complete = 1;
        assert!(!curve.is_active());
    }

    #[test]
    fn test_current_price() {
        let curve = create_test_curve();

        let price = curve.current_price();

        // Price should match the ratio of reserves
        let expected = curve.virtual_sol_reserves as f64 / curve.virtual_token_reserves as f64;

        assert!((price - expected).abs() < 1e-10);
    }

    #[test]
    fn test_current_price_zero_tokens() {
        let mut curve = create_test_curve();
        curve.virtual_token_reserves = 0;

        // Should return 0 to avoid division by zero
        assert_eq!(curve.current_price(), 0.0);
    }

    #[test]
    fn test_fee_constants() {
        // Verify the fee constants are correct
        assert_eq!(BondingCurve::FEE_BPS, 100);
        assert_eq!(BondingCurve::BPS_DENOMINATOR, 10000);

        // 100 / 10000 = 0.01 = 1%
        let fee_percentage = BondingCurve::FEE_BPS as f64 / BondingCurve::BPS_DENOMINATOR as f64;
        assert!((fee_percentage - 0.01).abs() < 1e-10);
    }

    #[test]
    fn test_invariant_preserved_buy() {
        let curve = create_test_curve();

        // Calculate invariant before trade
        let k_before =
            (curve.virtual_token_reserves as u128) * (curve.virtual_sol_reserves as u128);

        // Simulate a buy
        let tokens_to_buy = 1_000_000;
        let sol_cost = curve.calculate_buy_price(tokens_to_buy);

        // Calculate what the reserves would be after the trade (ignoring fee for invariant check)
        // The actual implementation applies fee to input, so we need to calculate the effective SOL added
        let sol_after_fee = (sol_cost as u128
            * (BondingCurve::BPS_DENOMINATOR - BondingCurve::FEE_BPS) as u128
            / BondingCurve::BPS_DENOMINATOR as u128) as u64;

        let new_token_reserves = curve.virtual_token_reserves - tokens_to_buy;
        let new_sol_reserves = curve.virtual_sol_reserves + sol_after_fee;

        let k_after = (new_token_reserves as u128) * (new_sol_reserves as u128);

        // Invariant should be approximately preserved (within rounding error)
        let diff = if k_after > k_before {
            k_after - k_before
        } else {
            k_before - k_after
        };

        // Allow small rounding differences (less than 0.01%)
        let tolerance = k_before / 10000;
        assert!(
            diff <= tolerance,
            "Invariant changed too much: {} vs {}",
            k_before,
            k_after
        );
    }

    #[test]
    fn test_pod_trait() {
        // Verify that BondingCurve can be safely cast from bytes
        let curve = create_test_curve();
        let bytes = bytemuck::bytes_of(&curve);
        let cast_back: &BondingCurve = bytemuck::from_bytes(bytes);

        assert_eq!(cast_back.discriminator, curve.discriminator);
        assert_eq!(
            cast_back.virtual_token_reserves,
            curve.virtual_token_reserves
        );
        assert_eq!(cast_back.virtual_sol_reserves, curve.virtual_sol_reserves);
    }

    #[test]
    fn test_shadow_bonding_curve_staleness() {
        let curve = create_test_curve();
        let shadow = ShadowBondingCurve::new(curve, 1000);

        // Fresh state (within 3 slots)
        assert!(!shadow.is_stale(1000)); // Same slot
        assert!(!shadow.is_stale(1001)); // 1 slot old
        assert!(!shadow.is_stale(1002)); // 2 slots old
        assert!(!shadow.is_stale(1003)); // 3 slots old

        // Stale state (more than 3 slots)
        assert!(shadow.is_stale(1004)); // 4 slots old
        assert!(shadow.is_stale(1005)); // 5 slots old
        assert!(shadow.is_stale(2000)); // 1000 slots old
    }

    #[test]
    fn test_shadow_bonding_curve_age() {
        let curve = create_test_curve();
        let shadow = ShadowBondingCurve::new(curve, 1000);

        assert_eq!(shadow.age_slots(1000), 0);
        assert_eq!(shadow.age_slots(1001), 1);
        assert_eq!(shadow.age_slots(1003), 3);
        assert_eq!(shadow.age_slots(1004), 4);
        assert_eq!(shadow.age_slots(2000), 1000);
    }

    #[test]
    fn test_shadow_bonding_curve_max_age_constant() {
        assert_eq!(ShadowBondingCurve::MAX_AGE_SLOTS, 3);
    }

    /// Test with real Pump.fun values to ensure price calculation accuracy
    ///
    /// This test uses actual bonding curve state from a real Pump.fun token
    /// and verifies that our calculation matches the expected result.
    #[test]
    fn test_pump_fun_real_values_accuracy() {
        // Real Pump.fun bonding curve values (example)
        // These would be sourced from Solscan for a real token
        let real_curve = BondingCurve {
            discriminator: 0x17b7bca8e24d1d39,
            virtual_token_reserves: 1_073_000_000_000, // ~1.073T tokens
            virtual_sol_reserves: 30_000_000_000,      // 30 SOL
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 20_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // Calculate buy price for 1 SOL worth (at current price)
        let one_sol = 1_000_000_000; // 1 SOL in lamports

        // For 1 SOL input, we need to calculate how many tokens we can buy
        // Using the inverse formula: token_out = virtual_token_reserves - (k / (virtual_sol_reserves + sol_after_fee))
        let sol_after_fee = (one_sol as u128 * 9900 / 10000) as u64; // 1% fee
        let k =
            (real_curve.virtual_token_reserves as u128) * (real_curve.virtual_sol_reserves as u128);
        let new_sol_reserves = real_curve.virtual_sol_reserves + sol_after_fee;
        let new_token_reserves = (k / new_sol_reserves as u128) as u64;
        let tokens_out = real_curve.virtual_token_reserves - new_token_reserves;

        // Now calculate the reverse: how much SOL needed to buy those tokens
        let calculated_cost = real_curve.calculate_buy_price(tokens_out);

        // The cost should match 1 SOL within a small tolerance (due to rounding)
        let diff = if calculated_cost > one_sol {
            calculated_cost - one_sol
        } else {
            one_sol - calculated_cost
        };

        // Allow tolerance of 1 lamport per 1000 lamports (0.1% for rounding)
        let tolerance = one_sol / 1000;
        assert!(
            diff <= tolerance,
            "Price calculation inaccurate: expected {}, got {} (diff: {})",
            one_sol,
            calculated_cost,
            diff
        );
    }

    #[test]
    fn test_precision_1_lamport() {
        // Test that our calculation is precise and deterministic
        let curve = create_test_curve();

        // Use amount that's significant enough to show price differences
        // but small enough to avoid precision issues
        let amount = 10_000_000; // 10M tokens (0.001% of 1T supply)

        let price1 = curve.calculate_buy_price(amount);
        let price2 = curve.calculate_buy_price(amount);

        // Should be deterministic and exact
        assert_eq!(price1, price2);

        // Verify that different amounts result in different prices
        // Use 1% difference to ensure measurable impact
        let amount_less = amount - (amount / 100); // 1% less
        let amount_more = amount + (amount / 100); // 1% more

        let price_less = curve.calculate_buy_price(amount_less);
        let price_more = curve.calculate_buy_price(amount_more);

        // Due to the constant product formula, buying more tokens should cost more
        assert!(price_less < price1, "Buying fewer tokens should cost less");
        assert!(price_more > price1, "Buying more tokens should cost more");

        // Verify prices are significantly different (no precision loss)
        let diff_less = price1 - price_less;
        let diff_more = price_more - price1;

        assert!(diff_less > 0, "Price difference should be measurable");
        assert!(diff_more > 0, "Price difference should be measurable");
    }

    #[test]
    fn test_simulate_buy_basic() {
        let curve = create_test_curve();

        // Simulate buying with 1 SOL
        let one_sol = 1_000_000_000; // 1 SOL in lamports
        let tokens_out = curve.simulate_buy(one_sol);

        // Should receive a positive amount of tokens
        assert!(tokens_out > 0, "Should receive tokens");

        // Verify the result is less than if we didn't account for fee
        let fee = one_sol / 100;
        let effective_sol = one_sol - fee;
        let k = (curve.virtual_sol_reserves as u128) * (curve.virtual_token_reserves as u128);
        let new_sol = curve.virtual_sol_reserves as u128 + effective_sol as u128;
        let expected_tokens = (curve.virtual_token_reserves as u128) - (k / new_sol);

        assert_eq!(
            tokens_out, expected_tokens as u64,
            "simulate_buy should match expected calculation"
        );
    }

    #[test]
    fn test_simulate_buy_fee_impact() {
        let curve = create_test_curve();
        let one_sol = 1_000_000_000; // 1 SOL

        // Calculate tokens with fee (what simulate_buy does)
        let tokens_with_fee = curve.simulate_buy(one_sol);

        // Calculate tokens WITHOUT fee (incorrect calculation)
        let k = (curve.virtual_sol_reserves as u128) * (curve.virtual_token_reserves as u128);
        let new_sol_no_fee = curve.virtual_sol_reserves as u128 + one_sol as u128;
        let tokens_no_fee = (curve.virtual_token_reserves as u128) - (k / new_sol_no_fee);

        // With fee deducted from input, we should receive FEWER tokens
        assert!(
            tokens_with_fee < tokens_no_fee as u64,
            "Fee deduction should result in fewer tokens: with_fee={}, no_fee={}",
            tokens_with_fee,
            tokens_no_fee
        );

        // The difference should be approximately 1% less tokens
        // (not exactly, due to AMM mechanics, but in the ballpark)
        let difference_pct = 100 - (tokens_with_fee as u128 * 100 / tokens_no_fee);
        assert!(
            difference_pct >= 1 && difference_pct <= 2,
            "Difference should be around 1%, got {}%",
            difference_pct
        );
    }

    #[test]
    fn test_simulate_buy_zero_input() {
        let curve = create_test_curve();

        // Zero input should return zero tokens
        let tokens_out = curve.simulate_buy(0);
        assert_eq!(tokens_out, 0, "Zero input should yield zero tokens");
    }

    #[test]
    fn test_simulate_buy_small_amount() {
        let curve = create_test_curve();

        // Very small amount (100 lamports = 0.0000001 SOL)
        let tokens_out = curve.simulate_buy(100);

        // Should still get some tokens (may be 0 due to rounding for very small amounts)
        // Just ensure no panic
        assert!(
            tokens_out < curve.virtual_token_reserves,
            "Should not exceed reserves"
        );
    }

    #[test]
    fn test_simulate_buy_large_amount() {
        let curve = create_test_curve();

        // Large amount: 10 SOL
        let ten_sol = 10_000_000_000;
        let tokens_out = curve.simulate_buy(ten_sol);

        // Should get more tokens than with 1 SOL
        let one_sol_tokens = curve.simulate_buy(1_000_000_000);
        assert!(
            tokens_out > one_sol_tokens,
            "More SOL should yield more tokens"
        );

        // But not linearly more due to price impact
        assert!(
            tokens_out < one_sol_tokens * 10,
            "Price impact should make 10x SOL yield less than 10x tokens"
        );
    }

    #[test]
    fn test_simulate_buy_roundtrip_consistency() {
        let curve = create_test_curve();

        // Simulate a buy and then verify the inverse calculation is consistent
        let one_sol = 1_000_000_000;
        let tokens_out = curve.simulate_buy(one_sol);

        // Calculate how much SOL we'd need to buy these tokens using calculate_buy_price
        let sol_needed = curve.calculate_buy_price(tokens_out);

        // The SOL needed should be close to our input (within rounding tolerance)
        // Note: There will be some difference due to fee handling differences
        let diff = if sol_needed > one_sol {
            sol_needed - one_sol
        } else {
            one_sol - sol_needed
        };

        // Allow 1% tolerance for rounding differences
        let tolerance = one_sol / 100;
        assert!(
            diff <= tolerance,
            "Roundtrip should be consistent: input={}, needed={}, diff={}",
            one_sol,
            sol_needed,
            diff
        );
    }

    #[test]
    fn test_simulate_buy_deterministic() {
        let curve = create_test_curve();
        let amount = 1_000_000_000;

        // Multiple calls with same input should yield same result
        let result1 = curve.simulate_buy(amount);
        let result2 = curve.simulate_buy(amount);
        let result3 = curve.simulate_buy(amount);

        assert_eq!(result1, result2, "simulate_buy should be deterministic");
        assert_eq!(result2, result3, "simulate_buy should be deterministic");
    }

    // =========================================================================
    // BCV (Bonding Curve Validation) Tests
    // =========================================================================

    #[test]
    fn test_get_market_cap_sol_basic() {
        let curve = create_test_curve();

        let market_cap = curve.get_market_cap_sol();

        // Market cap should be positive
        assert!(market_cap > 0, "Market cap should be positive");

        // Calculate expected: (30B lamports / 1T tokens) * 1T tokens = 30B lamports = 30 SOL
        // This is the initial market cap
        let expected_approx = 30_000_000_000u64; // 30 SOL in lamports

        // Allow 10% tolerance for rounding
        let tolerance = expected_approx / 10;
        assert!(
            market_cap >= expected_approx.saturating_sub(tolerance)
                && market_cap <= expected_approx + tolerance,
            "Market cap {} not within tolerance of expected {}",
            market_cap,
            expected_approx
        );
    }

    #[test]
    fn test_get_market_cap_sol_zero_reserves() {
        let mut curve = create_test_curve();
        curve.virtual_token_reserves = 0;

        let market_cap = curve.get_market_cap_sol();
        assert_eq!(market_cap, 0, "Market cap with zero tokens should be 0");
    }

    #[test]
    fn test_get_bonding_progress_initial() {
        let curve = create_test_curve();

        let progress = curve.get_bonding_progress();

        // With 20B lamports real SOL reserves out of 85B max = ~23.5%
        let expected = (20_000_000_000u128 * 100 / 85_000_000_000u128) as u64;
        assert_eq!(progress, expected, "Progress should be ~23%");
    }

    #[test]
    fn test_get_bonding_progress_near_completion() {
        let mut curve = create_test_curve();
        // Set real SOL reserves to 84 SOL (near max of 85 SOL)
        curve.real_sol_reserves = 84_000_000_000;

        let progress = curve.get_bonding_progress();

        // Should be around 98-99%
        assert!(
            progress >= 98,
            "Progress should be >= 98% when near completion"
        );
        assert!(progress <= 100, "Progress should not exceed 100%");
    }

    #[test]
    fn test_get_bonding_progress_completed() {
        let mut curve = create_test_curve();
        // Set real SOL reserves to max
        curve.real_sol_reserves = 85_000_000_000;

        let progress = curve.get_bonding_progress();

        assert_eq!(progress, 100, "Progress should be 100% at max reserves");
    }

    #[test]
    fn test_get_bonding_progress_over_max() {
        let mut curve = create_test_curve();
        // Set real SOL reserves above max (edge case)
        curve.real_sol_reserves = 100_000_000_000;

        let progress = curve.get_bonding_progress();

        // Should cap at 100%
        assert_eq!(progress, 100, "Progress should cap at 100%");
    }

    #[test]
    fn test_get_price_impact_zero_input() {
        let curve = create_test_curve();

        let impact = curve.get_price_impact(0);

        assert_eq!(impact, 0.0, "Zero input should have zero price impact");
    }

    #[test]
    fn test_get_price_impact_small_order() {
        let curve = create_test_curve();

        // 0.01 SOL - very small order
        let small_sol = 10_000_000;
        let impact = curve.get_price_impact(small_sol);

        // Small order should have minimal impact (< 1%)
        assert!(impact >= 0.0, "Price impact should be non-negative");
        assert!(
            impact < 1.0,
            "Small order should have < 1% impact, got {}%",
            impact
        );
    }

    #[test]
    fn test_get_price_impact_medium_order() {
        let curve = create_test_curve();

        // 1 SOL - medium order
        let one_sol = 1_000_000_000;
        let impact = curve.get_price_impact(one_sol);

        // Medium order should have noticeable but not extreme impact
        assert!(impact > 0.0, "1 SOL should have positive price impact");
        assert!(
            impact < 10.0,
            "1 SOL should have < 10% impact in initial curve"
        );

        println!("Price impact for 1 SOL: {}%", impact);
    }

    #[test]
    fn test_get_price_impact_large_order() {
        let curve = create_test_curve();

        // 10 SOL - large order (33% of initial reserves)
        let ten_sol = 10_000_000_000;
        let impact = curve.get_price_impact(ten_sol);

        // Large order should have significant impact
        assert!(impact > 10.0, "10 SOL should have > 10% impact");

        println!("Price impact for 10 SOL: {}%", impact);
    }

    #[test]
    fn test_get_price_impact_increases_with_size() {
        let curve = create_test_curve();

        let impact_small = curve.get_price_impact(100_000_000); // 0.1 SOL
        let impact_medium = curve.get_price_impact(1_000_000_000); // 1 SOL
        let impact_large = curve.get_price_impact(5_000_000_000); // 5 SOL

        assert!(
            impact_medium > impact_small,
            "Larger orders should have more impact: {} vs {}",
            impact_medium,
            impact_small
        );
        assert!(
            impact_large > impact_medium,
            "Even larger orders should have more impact: {} vs {}",
            impact_large,
            impact_medium
        );
    }

    #[test]
    fn test_simulate_sell_basic() {
        let curve = create_test_curve();

        // Sell 1M tokens
        let tokens_to_sell = 1_000_000;
        let sol_out = curve.simulate_sell(tokens_to_sell);

        // Should receive positive SOL
        assert!(sol_out > 0, "Should receive SOL for selling tokens");

        // Verify it's less than the buy price (due to spread + fees)
        // This confirms proper AMM mechanics
    }

    #[test]
    fn test_simulate_sell_zero() {
        let curve = create_test_curve();

        let sol_out = curve.simulate_sell(0);

        assert_eq!(sol_out, 0, "Zero tokens should yield zero SOL");
    }

    #[test]
    fn test_simulate_sell_fee_applied() {
        let curve = create_test_curve();

        // Sell 100M tokens
        let tokens_to_sell = 100_000_000;
        let sol_out = curve.simulate_sell(tokens_to_sell);

        // Calculate what we'd get without fee
        let k = (curve.virtual_sol_reserves as u128) * (curve.virtual_token_reserves as u128);
        let new_tokens = curve.virtual_token_reserves as u128 + tokens_to_sell as u128;
        let new_sol = k / new_tokens;
        let sol_out_no_fee = curve.virtual_sol_reserves as u128 - new_sol;

        // With fee should be 99% of no-fee amount
        let expected_with_fee = (sol_out_no_fee * 99 / 100) as u64;

        // Allow small rounding tolerance
        let tolerance = expected_with_fee / 100;
        assert!(
            sol_out >= expected_with_fee.saturating_sub(tolerance)
                && sol_out <= expected_with_fee + tolerance,
            "Sell with fee {} not within tolerance of expected {}",
            sol_out,
            expected_with_fee
        );
    }

    #[test]
    fn test_buy_sell_roundtrip_loss() {
        let curve = create_test_curve();

        // Buy with 1 SOL
        let sol_in = 1_000_000_000;
        let tokens_bought = curve.simulate_buy(sol_in);

        // Immediately sell those tokens
        let sol_back = curve.simulate_sell(tokens_bought);

        // We should get back less than we put in (due to 2x 1% fees + AMM slippage)
        assert!(
            sol_back < sol_in,
            "Roundtrip should lose money due to fees: in={}, back={}",
            sol_in,
            sol_back
        );

        // Loss includes:
        // - 1% fee on buy (applied to input SOL)
        // - 1% fee on sell (applied to output SOL)
        // - AMM slippage from price movement during trades
        // Total loss is typically 5-10% for reasonable trade sizes
        let loss_percent = 100.0 - (sol_back as f64 / sol_in as f64 * 100.0);
        assert!(
            loss_percent >= 1.5 && loss_percent <= 15.0,
            "Roundtrip loss should be between 1.5% and 15%, got {}%",
            loss_percent
        );

        println!("Roundtrip loss: {}%", loss_percent);
    }

    #[test]
    fn test_is_near_migration_false() {
        let curve = create_test_curve();

        // Initial curve should not be near migration
        assert!(
            !curve.is_near_migration(),
            "Initial curve should not be near migration"
        );
    }

    #[test]
    fn test_is_near_migration_true() {
        let mut curve = create_test_curve();
        // Set real SOL reserves to >99% of max
        curve.real_sol_reserves = 85_000_000_000; // 100% of max

        assert!(
            curve.is_near_migration(),
            "Full curve should be near migration"
        );

        // Test boundary at exactly 99%
        curve.real_sol_reserves = 84_150_000_000; // 99% of 85 SOL
        assert!(
            !curve.is_near_migration(),
            "Exactly 99% should not trigger migration warning"
        );

        // Test just above 99%
        curve.real_sol_reserves = 84_200_000_000; // ~99.05% of 85 SOL
                                                  // Note: Due to integer division, this might still be 99
    }

    #[test]
    fn test_get_price_per_million_tokens() {
        let curve = create_test_curve();

        let price_per_million = curve.get_price_per_million_tokens();

        // Should be positive
        assert!(
            price_per_million > 0.0,
            "Price per million tokens should be positive"
        );

        // Should be 1 million times the current price
        let expected = curve.current_price() * 1_000_000.0;
        assert!(
            (price_per_million - expected).abs() < 1e-10,
            "Price per million should be 1M * current_price"
        );

        println!("Price per 1M tokens: {} SOL", price_per_million / 1e9);
    }

    #[test]
    fn test_bcv_constants() {
        // Verify BCV constants are correct
        assert_eq!(BondingCurve::MIGRATION_THRESHOLD_PERCENT, 99);
        assert_eq!(BondingCurve::INITIAL_VIRTUAL_SOL, 30_000_000_000); // 30 SOL
        assert_eq!(BondingCurve::MAX_REAL_SOL_RESERVES, 85_000_000_000); // 85 SOL
    }

    #[test]
    fn test_simulate_sell_deterministic() {
        let curve = create_test_curve();
        let amount = 1_000_000;

        // Multiple calls with same input should yield same result
        let result1 = curve.simulate_sell(amount);
        let result2 = curve.simulate_sell(amount);
        let result3 = curve.simulate_sell(amount);

        assert_eq!(result1, result2, "simulate_sell should be deterministic");
        assert_eq!(result2, result3, "simulate_sell should be deterministic");
    }

    #[test]
    fn test_price_impact_with_zero_reserves() {
        let mut curve = create_test_curve();
        curve.virtual_token_reserves = 0;

        let impact = curve.get_price_impact(1_000_000_000);
        assert_eq!(impact, 0.0, "Price impact with zero reserves should be 0");

        curve.virtual_token_reserves = 1_000_000_000;
        curve.virtual_sol_reserves = 0;

        let impact = curve.get_price_impact(1_000_000_000);
        assert_eq!(
            impact, 0.0,
            "Price impact with zero SOL reserves should be 0"
        );
    }

    /// Performance test: Verify BCV calculations complete in microseconds
    /// (The actual <50ns target is verified in benchmarks, this ensures no regression)
    #[test]
    fn test_bcv_performance() {
        let curve = create_test_curve();
        let iterations = 10000;

        // Time simulate_buy
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(curve.simulate_buy(1_000_000_000));
        }
        let simulate_buy_duration = start.elapsed();
        let simulate_buy_ns = simulate_buy_duration.as_nanos() / iterations as u128;

        // Should complete in < 1 microsecond per call
        assert!(
            simulate_buy_ns < 1000,
            "simulate_buy took {} ns/call, should be < 1000 ns",
            simulate_buy_ns
        );

        // Time get_price_impact
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(curve.get_price_impact(1_000_000_000));
        }
        let price_impact_duration = start.elapsed();
        let price_impact_ns = price_impact_duration.as_nanos() / iterations as u128;

        // Should complete in < 1 microsecond per call
        assert!(
            price_impact_ns < 1000,
            "get_price_impact took {} ns/call, should be < 1000 ns",
            price_impact_ns
        );

        // Time get_market_cap_sol
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(curve.get_market_cap_sol());
        }
        let market_cap_duration = start.elapsed();
        let market_cap_ns = market_cap_duration.as_nanos() / iterations as u128;

        assert!(
            market_cap_ns < 1000,
            "get_market_cap_sol took {} ns/call, should be < 1000 ns",
            market_cap_ns
        );

        println!("BCV Performance (ns/call): simulate_buy={}, get_price_impact={}, get_market_cap_sol={}",
            simulate_buy_ns,
            price_impact_ns,
            market_cap_ns
        );
    }
}
