use crate::CurveFinality;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// Authoritative state phase for a pool inside AccountStateCore.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatePhase {
    #[default]
    Bootstrap,
    PendingConfirmation,
    Canonical,
    Migrated,
}

impl StatePhase {
    /// Explicit transition matrix used by PR1 tests and future reducers.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        match (self, next) {
            (Self::Bootstrap, Self::Bootstrap)
            | (Self::Bootstrap, Self::PendingConfirmation)
            | (Self::Bootstrap, Self::Canonical)
            | (Self::PendingConfirmation, Self::PendingConfirmation)
            | (Self::PendingConfirmation, Self::Canonical)
            | (Self::Canonical, Self::Canonical)
            | (Self::Canonical, Self::Migrated)
            | (Self::Migrated, Self::Migrated) => true,
            _ => false,
        }
    }

    #[must_use]
    pub const fn is_canonical(self) -> bool {
        matches!(self, Self::Canonical)
    }

    #[must_use]
    pub const fn is_bootstrap_like(self) -> bool {
        matches!(self, Self::Bootstrap | Self::PendingConfirmation)
    }
}

/// Source tag for account-state updates entering the canonical reducer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSource {
    #[default]
    GeyserAccountUpdate,
    WalReplay,
    TxObservedBootstrap,
}

/// Optional bootstrap hints captured before the first canonical account update.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BootstrapHints {
    pub speculative_reserves: Option<(u64, u64)>,
    pub token_total_supply: Option<u64>,
    pub bonding_curve_progress: Option<f64>,
    pub initial_liquidity_sol: Option<f64>,
}

/// Non-canonical bootstrap state registered from CREATE / detected-pool flow.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BootstrapPoolState {
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub speculative_reserves: Option<(u64, u64)>,
    pub token_total_supply: Option<u64>,
    pub bonding_curve_progress: Option<f64>,
    pub initial_liquidity_sol: Option<f64>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountUpdateRejectReason {
    OlderSlot,
    OlderOrDuplicateReceiveSeq,
}

impl AccountUpdateRejectReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OlderSlot => "older_slot",
            Self::OlderOrDuplicateReceiveSeq => "older_or_duplicate_recv_seq",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountUpdateResult {
    Applied,
    PromotedFromBootstrap,
    Rejected(AccountUpdateRejectReason),
}

/// Canonical per-pool state materialized by AccountStateCore.
///
/// Unit contract:
/// - reserve fields remain in raw on-chain units
///   - `*_sol_reserves`: lamports
///   - `*_token_reserves`: base token units (Pump.fun: 10^6 per token)
/// - `price_sol`: normalized human `SOL/token`
/// - `market_cap_sol`: normalized human `SOL`
/// - `reserve_velocity_sol_per_sec`: normalized human `SOL/sec`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CanonicalPoolState {
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub virtual_sol_reserves: u64,
    pub virtual_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub bonding_curve_progress: f64,
    pub price_sol: f64,
    pub market_cap_sol: f64,
    pub token_total_supply: u64,
    pub is_complete: bool,
    pub last_update_slot: u64,
    pub last_update_ts_ms: u64,
    pub curve_finality: CurveFinality,
    pub state_phase: StatePhase,
    pub update_count: u64,
    #[serde(default)]
    pub initial_price_sol: f64,
    #[serde(default)]
    pub price_change_since_t0_pct: f64,
    #[serde(default)]
    pub reserve_velocity_sol_per_sec: f64,
}

/// Input event accepted by AccountStateCore.
///
/// Unit contract:
/// - `sol_reserves`: raw lamports from the bonding-curve account
/// - `token_reserves`: raw token base units from the bonding-curve account
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountStateUpdate {
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub sol_reserves: u64,
    pub token_reserves: u64,
    pub is_complete: u8,
    pub slot: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_version: Option<u64>,
    pub receive_ts_ms: u64,
    pub receive_seq: u64,
    pub curve_finality: CurveFinality,
    pub source: UpdateSource,
}

/// Canonical feature bundle derived from account state and passed onward.
///
/// `current_reserves` preserves raw reserve units, while price/market-cap/velocity
/// are emitted in normalized human units for downstream policy/runtime consumers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountStateFeatures {
    pub current_reserves: (u64, u64),
    pub price_sol: f64,
    pub market_cap_sol: f64,
    pub bonding_progress: f64,
    pub price_change_since_t0_pct: f64,
    pub reserve_velocity_sol_per_sec: f64,
    pub is_bootstrap: bool,
    pub curve_finality: CurveFinality,
    pub state_phase: StatePhase,
    pub update_count: u64,
}
