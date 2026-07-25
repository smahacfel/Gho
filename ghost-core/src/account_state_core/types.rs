use crate::metric_contracts::ReserveVelocityStatusV1;
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
    /// Read-only processed-RPC point query performed only for an already
    /// managed shadow position after its stream state became stale.
    RpcRefresh,
    WalReplay,
    /// Historical compatibility marker for a reserve tuple inferred from a
    /// parsed transaction.  PR1C deliberately rejects this source at the raw
    /// account arbiter boundary: a parsed transaction is not a provider
    /// AccountUpdate and cannot become live account-state authority.
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
    /// Retained only for deserializing historical result records.  PR1C emits
    /// [`Self::StaleObservation`] instead of comparing slots in a legacy
    /// boolean guard.
    OlderSlot,
    /// Retained only for deserializing historical result records.  Local
    /// receive sequence is never a canonical ordering field in PR1C.
    OlderOrDuplicateReceiveSeq,
    DuplicateObservation,
    StaleObservation,
    ProviderConflict,
    UnorderableWithoutWriteVersion,
    SecondaryWitness,
    MissingProviderProvenance,
    InvalidAccountDataHash,
    UnsupportedAccountUpdateSource,
    /// The per-account arbiter state is poisoned or otherwise unavailable.
    /// This is fail-closed: a new observation must not recreate unknown
    /// canonical ordering state.
    ArbiterStateUnavailable,
    /// The arbiter cannot retain a further unique observation or conflict
    /// witness within its explicit in-process evidence bounds. This is
    /// fail-closed; the update must not mutate canonical account state.
    AccountObservationEvidenceCapacityExceeded,
    RpcRefreshInvalidSource,
    RpcRefreshMissingAccountDataHash,
    RpcRefreshWithoutCanonicalState,
    RpcRefreshIdentityMismatch,
    RpcRefreshPhaseRegression,
}

impl AccountUpdateRejectReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OlderSlot => "older_slot",
            Self::OlderOrDuplicateReceiveSeq => "older_or_duplicate_recv_seq",
            Self::DuplicateObservation => "duplicate_observation",
            Self::StaleObservation => "stale_observation",
            Self::ProviderConflict => "provider_conflict",
            Self::UnorderableWithoutWriteVersion => "unorderable_without_write_version",
            Self::SecondaryWitness => "secondary_witness",
            Self::MissingProviderProvenance => "missing_provider_provenance",
            Self::InvalidAccountDataHash => "invalid_account_data_hash",
            Self::UnsupportedAccountUpdateSource => "unsupported_account_update_source",
            Self::ArbiterStateUnavailable => "arbiter_state_unavailable",
            Self::AccountObservationEvidenceCapacityExceeded => {
                "account_observation_evidence_capacity_exceeded"
            }
            Self::RpcRefreshInvalidSource => "rpc_refresh_invalid_source",
            Self::RpcRefreshMissingAccountDataHash => "rpc_refresh_missing_account_data_hash",
            Self::RpcRefreshWithoutCanonicalState => "rpc_refresh_without_canonical_state",
            Self::RpcRefreshIdentityMismatch => "rpc_refresh_identity_mismatch",
            Self::RpcRefreshPhaseRegression => "rpc_refresh_phase_regression",
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

/// Result of the observation-only RPC refresh path.
///
/// It is deliberately separate from [`AccountUpdateResult`]: an RPC context
/// slot is neither a canonical account-write ordering key nor an authority to
/// mutate reserves, state phase, counters, velocity, or account provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcRefreshResult {
    /// The captured RPC payload is identical to the last raw-primary
    /// canonical payload.
    ObservationMatchesCanonical,
    /// The captured RPC payload differs from the last raw-primary canonical
    /// payload. The difference is surfaced to the caller but is not applied.
    ObservationDivergesFromCanonical,
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
    /// Latest canonical raw-primary observation boundary. Processed RPC
    /// refreshes are deliberately not written here: they are diagnostic
    /// observations and never obtain authority over canonical account state.
    #[serde(default)]
    pub last_observed_slot: u64,
    #[serde(default)]
    pub last_observed_ts_ms: u64,
    #[serde(default)]
    pub last_observation_source: UpdateSource,
    #[serde(default)]
    pub observation_count: u64,
    /// Last time the decoded account contents actually changed.  This is the
    /// only AccountStateCore timestamp/counter used for activity and velocity.
    #[serde(default)]
    pub last_data_change_ts_ms: u64,
    #[serde(default)]
    pub last_data_change_source: UpdateSource,
    #[serde(default)]
    pub data_change_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_write_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_pubkey: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_owner_or_program: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// BLAKE3 hexadecimal digest of the captured raw account payload handed
    /// from the provider adapter to normalization.  A Geyser/WAL observation
    /// without a valid 32-byte digest is rejected by `AccountObservationArbiter`.
    pub account_data_hash: Option<String>,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountStateReserveVelocitySnapshotV1 {
    pub legacy_velocity_sol_per_sec: f64,
    pub previous_real_sol_reserves_lamports: Option<u64>,
    pub current_real_sol_reserves_lamports: Option<u64>,
    pub interval_ms: Option<u64>,
    pub accepted_update_count: u64,
    pub status: ReserveVelocityStatusV1,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_pubkey: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_account_owner_or_program: Option<Pubkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_data_hash: Option<String>,
    pub receive_ts_ms: u64,
    pub receive_seq: u64,
    pub curve_finality: CurveFinality,
    pub source: UpdateSource,
    // Appended, rather than inserted in the legacy positional layout, to
    // retain the historical bincode failure behaviour of the pre-existing
    // fixture while keeping JSON/JSONL additive and omission-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_role: Option<crate::RawProviderRoleV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txn_signature: Option<solana_sdk::signature::Signature>,
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
