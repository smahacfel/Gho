//! Type definitions for Seer module
//!
//! This module defines the data structures used for event processing and candidate creation.

use ghost_core::{
    CurveFinality, EventSemanticEnvelope, EventTimeMetadata, ObservationProvenanceV1,
    RawProviderRoleV1, SourceKind,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub use crate::early_fingerprint::TokenDelta;

static ARRIVAL_START: Lazy<Instant> = Lazy::new(Instant::now);

/// Monotonic arrival timestamp (ms) from process start.
pub fn arrival_time_ms() -> u64 {
    ARRIVAL_START.elapsed().as_millis() as u64
}

/// Monotonic arrival timestamp (ns) from process start.
pub fn arrival_time_ns() -> u64 {
    ARRIVAL_START.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

/// Epoch timestamp in milliseconds captured at event ingress.
pub fn ingress_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Normalize slot metadata: Some(0) is treated as unknown => None.
pub fn normalize_slot(slot: Option<u64>) -> Option<u64> {
    match slot {
        Some(0) => None,
        other => other,
    }
}

/// Convert block_time (seconds) to event timestamp in ms, if available.
pub fn event_ts_from_block_time(block_time: Option<i64>) -> Option<u64> {
    block_time.and_then(|t| {
        if t >= 0 {
            Some((t as u64).saturating_mul(1000))
        } else {
            None
        }
    })
}

fn is_grpc_like_source(source: &str) -> bool {
    matches!(
        source,
        "grpc" | "grpc_backfill" | "grpc_global_stream" | "grpc_pool_stream"
    )
}

/// Derive explicit time provenance from the legacy raw transaction contract.
///
/// `GeyserEvent::Transaction.event_ts_ms` remains a compatibility field whose
/// historical meaning is intentionally left untouched. New code should use this
/// helper and carry the returned metadata forward instead of inferring
/// provenance from the presence of a single timestamp.
pub fn transaction_event_time(event: &GeyserEvent) -> EventTimeMetadata {
    match event {
        GeyserEvent::Transaction {
            event_time,
            source,
            event_ts_ms,
            block_time,
            arrival_ts_ms,
            ..
        } => {
            if !event_time.is_empty() {
                return *event_time;
            }
            let chain_event_ts_ms =
                if ghost_core::source_kind_from_label(source) == SourceKind::PumpPortal {
                    None
                } else {
                    event_ts_from_block_time(*block_time)
                };
            let ingress_wall_ts_ms = if chain_event_ts_ms.is_none() && is_grpc_like_source(source) {
                *event_ts_ms
            } else {
                None
            };
            EventTimeMetadata::new(chain_event_ts_ms, ingress_wall_ts_ms, *arrival_ts_ms)
        }
        _ => EventTimeMetadata::default(),
    }
}

pub fn transaction_timestamp_quality_from_event(
    event: &GeyserEvent,
) -> Option<ghost_core::TimestampQuality> {
    let event_time = transaction_event_time(event);
    if event_time.chain_event_ts_ms.is_some() {
        Some(ghost_core::TimestampQuality::Chain)
    } else if event_time.ingress_wall_ts_ms.is_some() {
        Some(ghost_core::TimestampQuality::WallClock)
    } else {
        None
    }
}

impl GeyserEvent {
    pub fn effective_event_ts_ms(&self) -> Option<u64> {
        transaction_event_time(self).effective_event_ts_ms()
    }

    pub fn compat_event_ts_ms(&self) -> Option<u64> {
        match self {
            Self::Transaction { event_ts_ms, .. } => {
                transaction_event_time(self).compat_event_ts_ms(*event_ts_ms)
            }
            _ => None,
        }
    }
}

/// Canonical final outcome categories for trade processing telemetry.
///
/// The same labels are reused in logs, metrics and tests so buffering/replay
/// semantics stay explicit instead of being inferred from ad-hoc strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeOutcome {
    ForwardedLive,
    ForwardedReplay,
    BufferedMissingPool,
    BufferedMissingMapping,
    FilteredInvalidPool,
    FilteredWsolPool,
    FilteredMappingConflictUnrecoverable,
    ExpiredWaitingForMapping,
    DedupDropped,
    IpcSendFailed,
}

impl TradeOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            TradeOutcome::ForwardedLive => "forwarded_live",
            TradeOutcome::ForwardedReplay => "forwarded_replay",
            TradeOutcome::BufferedMissingPool => "buffered_missing_pool",
            TradeOutcome::BufferedMissingMapping => "buffered_missing_mapping",
            TradeOutcome::FilteredInvalidPool => "filtered_invalid_pool",
            TradeOutcome::FilteredWsolPool => "filtered_wsol_pool",
            TradeOutcome::FilteredMappingConflictUnrecoverable => {
                "filtered_mapping_conflict_unrecoverable"
            }
            TradeOutcome::ExpiredWaitingForMapping => "expired_waiting_for_mapping",
            TradeOutcome::DedupDropped => "dedup_dropped",
            TradeOutcome::IpcSendFailed => "ipc_send_failed",
        }
    }

    pub const fn is_buffered(self) -> bool {
        matches!(
            self,
            TradeOutcome::BufferedMissingPool | TradeOutcome::BufferedMissingMapping
        )
    }
}

pub fn record_trade_outcome_metric(outcome: TradeOutcome) {
    ::metrics::increment_counter!("seer_trade_outcome_total", "outcome" => outcome.as_str());
}

/// Represents a raw event from the Geyser/WebSocket stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GeyserEvent {
    /// Typed loss of local post-provider coverage. This is not a Yellowstone
    /// slot gap and does not authorize reconnect/backfill.
    LocalCoverageGap { gap: ghost_core::LocalCoverageGapV1 },

    /// Transaction event with slot, signature, and transaction data
    Transaction {
        /// Stable identifier of the raw provider that supplied this observation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        /// Configured role of the raw provider. This is metadata-only in PR 1A.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_role: Option<RawProviderRoleV1>,
        /// Captured-provider payload provenance used by the PR1D observation
        /// boundary. For Yellowstone this hashes the prost encoding of the
        /// already-decoded `SubscribeUpdateTransaction`, not an original gRPC
        /// wire frame.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observation_provenance: Option<ObservationProvenanceV1>,
        /// Slot number when transaction was processed (if known)
        slot: Option<u64>,
        /// Authoritative transaction index within the slot from the source stream.
        ///
        /// `Some(0)` is the first transaction in a slot and is valid. `None`
        /// means the source did not provide a canonical transaction index.
        #[serde(default)]
        tx_index: Option<u32>,
        /// Compatibility event timestamp in milliseconds (if known).
        #[serde(default)]
        event_ts_ms: Option<u64>,
        /// Monotonic arrival timestamp captured at ingest time.
        #[serde(default)]
        arrival_ts_ms: Option<u64>,
        /// Explicit provenance for event/ingest time axes.
        #[serde(default)]
        event_time: EventTimeMetadata,
        /// Transaction signature
        signature: Signature,
        /// List of account keys involved
        accounts: Vec<Pubkey>,
        /// Raw instruction data
        instructions: Vec<RawInstruction>,
        /// Transaction logs
        logs: Vec<String>,
        /// Block time (Unix timestamp)
        block_time: Option<i64>,
        /// Account data for key accounts (e.g., bonding curve)
        /// Map of Pubkey to raw account data bytes
        account_data: HashMap<Pubkey, Vec<u8>>,
        /// Pre-transaction lamport balances for all accounts
        #[serde(default)]
        pre_balances: Vec<u64>,
        /// Post-transaction lamport balances for all accounts
        #[serde(default)]
        post_balances: Vec<u64>,
        /// True when transaction succeeded (meta.err is None)
        success: bool,
        /// Parsed error code if transaction failed
        error_code: Option<String>,
        /// Compute units consumed (if available)
        compute_units_consumed: Option<u64>,
        /// Whether this transaction is synthetic (generated for bootstrap/seed)
        #[serde(default)]
        synthetic: bool,
        /// Source of the transaction (e.g., "shadow_ledger_bootstrap", "geyser")
        #[serde(default)]
        source: String,
        /// MPCF payload bytes for actor classification (entropy analysis)
        ///
        /// Contains aggregated instruction data from the transaction.
        /// This is NOT the complete serialized transaction - it is instruction
        /// payload data sufficient for MPCF entropy analysis and bot detection.
        /// When None, the system falls back to heuristic-based classification.
        #[serde(default)]
        mpcf_payload_bytes: Option<Vec<u8>>,
        /// Reason why MPCF payload bytes are missing (if mpcf_payload_bytes is None)
        ///
        /// This field provides epistemic clarity about WHY bytes are not available,
        /// enabling proper telemetry distinction between provider limitations vs bugs.
        #[serde(default)]
        mpcf_payload_missing_reason: RawBytesMissingReason,
        /// Inner instructions from CPI calls (meta.inner_instructions).
        /// Each group has an `index` (top-level instruction index) and a list of
        /// inner instructions with `program_id_index`, `accounts`, `data`, and
        /// optional `stack_height`.
        #[serde(default)]
        inner_instructions: Vec<InnerInstructionGroup>,
        /// Pre-transaction token balances
        #[serde(default)]
        pre_token_balances: Vec<RawTokenBalance>,
        /// Post-transaction token balances
        #[serde(default)]
        post_token_balances: Vec<RawTokenBalance>,
    },

    /// Account update event
    AccountUpdate {
        /// Stable identifier of the raw provider that supplied this observation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        /// Configured role of the raw provider. This is metadata-only in PR 1A.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_role: Option<RawProviderRoleV1>,
        /// Slot number
        slot: u64,
        /// Explicit provenance for event/ingest time axes.
        #[serde(default)]
        event_time: EventTimeMetadata,
        /// Optional Solana account write-version used for same-slot ordering.
        #[serde(default)]
        write_version: Option<u64>,
        /// Transaction whose write produced this account update, when supplied
        /// by Yellowstone. Absence remains explicit and is never inferred.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        txn_signature: Option<Signature>,
        /// Updated account pubkey
        pubkey: Pubkey,
        /// Account data
        data: Vec<u8>,
        /// Owner program
        owner: Pubkey,
    },

    /// Slot update event
    SlotUpdate {
        /// Slot number
        slot: u64,
        /// Parent slot
        parent: u64,
        /// Root slot
        root: u64,
    },

    /// Entry anchor event — provides coverage denominator per slot.
    ///
    /// Emitted by Yellowstone for every slot with `executed_transaction_count`.
    /// Used by lib.rs to compute true coverage % (parsed trades / total txs).
    /// `raw` carries the original SubscribeUpdateEntry proto bytes so that
    /// lib.rs can scan inner CPI data embedded in Entry events (RC-1.2 fix).
    EntryAnchor {
        /// Stable identifier of the raw provider that supplied this observation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        /// Configured role of the raw provider. This is metadata-only in PR 1A.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_role: Option<RawProviderRoleV1>,
        /// Slot number
        slot: u64,
        /// Number of executed transactions in this slot (coverage denominator).
        executed_transaction_count: u64,
        /// Raw SubscribeUpdateEntry proto bytes for CPI scanning (may be empty).
        raw: Vec<u8>,
    },
}

/// Raw instruction data from a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawInstruction {
    /// Program ID that processes this instruction
    pub program_id: Pubkey,
    /// Indices of accounts in the transaction's account keys
    pub account_indices: Vec<u8>,
    /// Instruction data (discriminator + parameters)
    pub data: Vec<u8>,
}

/// Token balance snapshot for an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTokenBalance {
    /// Index into the transaction's account keys
    pub account_index: u32,
    /// Token mint pubkey string
    pub mint: String,
    /// Final owner wallet from transaction meta when provided by Yellowstone.
    pub owner: Option<String>,
    /// Token amount (in raw units)
    pub amount: u64,
}

/// Execution-tree provenance for a parsed instruction/event.
///
/// This captures where in the outer↔inner instruction tree a semantic parser
/// event originated, without changing downstream business semantics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstructionProvenance {
    /// Index of the top-level instruction that owns this execution subtree.
    #[serde(default)]
    pub outer_instruction_index: Option<u32>,
    /// Inner group index as reported by Solana transaction metadata.
    #[serde(default)]
    pub inner_group_index: Option<u32>,
    /// Program id of the owning top-level instruction, when available.
    #[serde(default)]
    pub outer_program_id: Option<String>,
    /// Program id of the instruction or CPI that produced the event.
    pub invoked_program_id: String,
    /// CPI stack height when provided by the source adapter.
    #[serde(default)]
    pub stack_height: Option<u32>,
    /// Structural path inside the owning outer instruction's CPI execution
    /// tree. Top-level instructions use `Some(vec![])`. Inner instructions use
    /// sibling ordinals derived deterministically from the chain-provided
    /// stack-height sequence. Missing or invalid stack-height structure
    /// remains `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_instruction_path: Option<Vec<u16>>,
    /// Backward-compatible mirror of the legacy `from_cpi` flag.
    pub from_cpi: bool,
}

/// Provenance for a route-specific account meta observed in the source
/// transaction instruction.
///
/// `instruction_account_position` is the position inside the instruction's
/// account list. `message_account_index` is the resolved index into the
/// transaction account-key set. Keeping both prevents confusing a route account
/// position with a global message account index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedAccountMetaProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tx_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_slot: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_slot_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_instruction_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_program_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_discriminator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_buy_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_account_position: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_account_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_address_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_err: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_status: Option<String>,
}

/// A group of inner instructions belonging to one top-level instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InnerInstructionGroup {
    /// Index of the top-level instruction this group belongs to.
    pub index: u32,
    /// Inner instructions within this group.
    pub instructions: Vec<InnerIx>,
}

/// A single inner instruction (CPI call).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InnerIx {
    /// Index into the transaction's account keys for the program ID.
    pub program_id_index: u8,
    /// Account key indices used by this instruction.
    pub accounts: Vec<u8>,
    /// Instruction data bytes.
    pub data: Vec<u8>,
    /// CPI stack height (available since Solana v1.14.6).
    pub stack_height: Option<u32>,
}

/// Raw infrastructure fingerprint extracted from the source transaction.
///
/// This payload is additive and transport-only. It carries parser-side counts
/// needed by FTDI today and DBIA in later phases, without making any policy
/// decisions on the parser side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolchainFingerprintInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_keys_len: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_instruction_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_instruction_group_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_set_compute_unit_limit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_set_compute_unit_price: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_fee_transfer_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_fee_transfer_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filtered_wsol_self_transfer_count: Option<u32>,
}

impl ToolchainFingerprintInput {
    pub fn is_empty(&self) -> bool {
        self.account_keys_len.is_none()
            && self.outer_instruction_count.is_none()
            && self.inner_instruction_group_count.is_none()
            && self.has_set_compute_unit_limit.is_none()
            && self.has_set_compute_unit_price.is_none()
            && self.internal_fee_transfer_count.is_none()
            && self.external_fee_transfer_count.is_none()
            && self.filtered_wsol_self_transfer_count.is_none()
    }

    pub fn fee_topology(&self) -> Option<(u32, u32)> {
        Some((
            self.external_fee_transfer_count?,
            self.internal_fee_transfer_count?,
        ))
    }
}

/// Parsed InitializePool event from Pump.fun or Bonk.fun
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializePoolEvent {
    /// Stable raw-provider identifier that delivered the source transaction.
    ///
    /// This is additive observation provenance. It does not select canonical
    /// state authority or alter pool-detection behavior in PR 1A.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// Configured role of the raw provider that delivered the source transaction.
    ///
    /// This remains metadata-only until a later explicit arbitration phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_role: Option<RawProviderRoleV1>,

    /// Slot when the pool was initialized (if known)
    pub slot: Option<u64>,

    /// Stable semantic ordinal within the source transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_ordinal: Option<u32>,

    /// Canonical Yellowstone transaction index within the slot. `Some(0)` is
    /// valid and must not be treated as missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u32>,

    /// Execution-tree location of the initialize mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<InstructionProvenance>,

    /// Effective event timestamp in milliseconds, derived via provenance helpers.
    pub event_ts_ms: Option<u64>,

    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,

    /// Transaction signature
    pub signature: Signature,

    /// AMM program ID (Pump.fun or Bonk.fun)
    pub amm_program_id: Pubkey,

    /// Pool AMM account ID (the main pool account)
    pub pool_amm_id: Pubkey,

    /// Base token mint (the new token being launched)
    pub base_mint: Pubkey,

    /// Quote token mint (SOL, USDC, or BONK)
    pub quote_mint: Pubkey,

    /// Bonding curve account (PDA derived from mint)
    pub bonding_curve: Pubkey,

    /// Creator wallet (transaction signer/payer)
    pub creator: Pubkey,

    /// Optional: Initial virtual token reserves
    pub initial_virtual_token_reserves: Option<u64>,

    /// Optional: Initial virtual SOL reserves
    pub initial_virtual_sol_reserves: Option<u64>,

    /// Optional: Initial real token reserves
    pub initial_real_token_reserves: Option<u64>,

    /// Optional: Initial real SOL reserves
    pub initial_real_sol_reserves: Option<u64>,

    /// Optional: Token total supply
    pub token_total_supply: Option<u64>,

    /// Block time when initialized
    pub block_time: Option<i64>,

    /// Raw instruction data for debugging
    pub raw_data: Vec<u8>,
}

/// Parsed trade (Buy/Sell) event from Pump.fun or Bonk.fun
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,

    /// Stable raw-provider identifier that delivered the source transaction.
    ///
    /// This is observation provenance only; no policy, state authority, or
    /// canonical-emission branch may derive behavior from it in PR 1A.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// Configured role of the raw provider that delivered the source transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_role: Option<RawProviderRoleV1>,

    /// Slot when the trade occurred (if known)
    pub slot: Option<u64>,

    /// Transaction signature
    pub signature: Signature,

    /// Stable event ordinal within a single transaction.
    ///
    /// A single transaction signature may legally emit multiple semantic trade
    /// events; this ordinal distinguishes them without overloading `signature`
    /// as the sole identity key.
    #[serde(default)]
    pub event_ordinal: Option<u32>,

    /// Transaction index within the Solana slot when the upstream feed exposes it.
    ///
    /// This is additive ordering metadata for FSC v2 anti-leakage. It must not
    /// be confused with `event_ordinal`, which is local to semantic events inside
    /// one transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u32>,

    /// Execution-tree provenance copied from the parser event source.
    #[serde(default)]
    pub provenance: Option<InstructionProvenance>,

    /// Timestamp in milliseconds
    pub timestamp_ms: u64,

    /// Monotonic arrival timestamp (ms) assigned at WS frame reception
    pub arrival_ts_ms: u64,

    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,

    /// Pool/bonding curve being traded on
    pub pool_amm_id: Pubkey,

    /// Token mint being traded
    pub mint: Pubkey,

    /// User/signer executing the trade
    pub signer: Pubkey,

    /// True if Buy, false if Sell
    pub is_buy: bool,

    /// True if this is a developer buy (txType:"create")
    #[serde(default)]
    pub is_dev_buy: bool,

    /// Amount of tokens/lamports being traded
    pub amount: u64,

    /// For Buy: maximum SOL cost (in lamports)
    /// For Sell: 0
    pub max_sol_cost: u64,

    /// For Sell: minimum SOL output (in lamports)
    /// For Buy: 0
    pub min_sol_output: u64,

    /// True when transaction succeeded (meta.err is None)
    pub success: bool,

    /// Parsed error code if transaction failed
    pub error_code: Option<String>,

    /// Compute units consumed (if available)
    pub compute_units_consumed: Option<u64>,

    /// Owner-resolved token deltas derived from pre/post token balances and ATA ownership.
    #[serde(default)]
    pub owner_token_deltas: Vec<TokenDelta>,

    /// MPCF payload bytes (aggregated instruction data for actor classification)
    pub mpcf_payload: Vec<u8>,

    /// Reason why MPCF payload is missing/empty (if mpcf_payload is empty)
    pub mpcf_payload_missing_reason: RawBytesMissingReason,

    /// Virtual tokens remaining in bonding curve (f64, token units).
    /// PumpPortal: `vTokensInBondingCurve`
    #[serde(default)]
    pub v_tokens_in_bonding_curve: Option<f64>,

    /// Virtual SOL remaining in bonding curve (f64, SOL units).
    /// PumpPortal: `vSolInBondingCurve`
    #[serde(default)]
    pub v_sol_in_bonding_curve: Option<f64>,

    /// Raw post-trade virtual SOL reserves from the canonical Pump trade event.
    #[serde(default)]
    pub virtual_sol_reserves: Option<u64>,
    /// Raw post-trade virtual token reserves from the canonical Pump trade event.
    #[serde(default)]
    pub virtual_token_reserves: Option<u64>,
    /// Raw post-trade real SOL reserves from the canonical Pump trade event.
    #[serde(default)]
    pub real_sol_reserves: Option<u64>,
    /// Raw post-trade real token reserves from the canonical Pump trade event.
    #[serde(default)]
    pub real_token_reserves: Option<u64>,
    /// Raw post-trade Pump completion flag from the canonical Pump trade event.
    #[serde(default)]
    pub complete: Option<bool>,

    /// Market cap in SOL as reported by PumpPortal.
    /// PumpPortal: `marketCapSol`
    #[serde(default)]
    pub market_cap_sol: Option<f64>,

    /// Optional pool-specific global config account observed in the source instruction.
    #[serde(default)]
    pub global_config: Option<Pubkey>,

    /// Optional pool-specific fee recipient observed in the source instruction.
    #[serde(default)]
    pub fee_recipient: Option<Pubkey>,

    /// Optional token program observed in the source instruction.
    #[serde(default)]
    pub token_program: Option<Pubkey>,

    /// Observed on-chain buy variant name for Pump.fun (`legacy_buy` or `routed_exact_sol_in`).
    #[serde(default)]
    pub buy_variant: Option<String>,

    /// Observed associated bonding curve account from the source instruction.
    #[serde(default)]
    pub associated_bonding_curve: Option<Pubkey>,

    /// Observed creator vault account from the source instruction fixed account layout.
    ///
    /// This is execution account evidence. It is not a creator identity and
    /// must only be used as authoritative when copied from observed pump.fun
    /// instruction accounts.
    #[serde(default)]
    pub creator_vault: Option<Pubkey>,

    /// Observed route-specific bonding_curve_v2 account from the source instruction.
    ///
    /// This is an execution-load account for routed Pump.fun buy builders, not the
    /// canonical bonding_curve account carried by `pool_amm_id`.
    #[serde(default)]
    pub bonding_curve_v2: Option<Pubkey>,

    /// Parser-side provenance for `bonding_curve_v2` when it was populated from
    /// a source transaction account meta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_curve_v2_provenance: Option<ObservedAccountMetaProvenance>,

    /// Remaining accounts passed after the fixed legacy Pump.fun buy account list.
    ///
    /// Current on-chain legacy buys may require the observed buyback recipient
    /// and its companion quote account. These are not BCV2 accounts.
    #[serde(default)]
    pub buy_remaining_accounts: Vec<Pubkey>,

    /// PumpPortal internal flag indicating unusual market conditions.
    /// Passed through for future analysis.
    #[serde(default)]
    pub is_mayhem_mode: Option<bool>,

    /// CU price in micro-lamports/CU from SetComputeUnitPrice instruction.
    /// Extracted from ComputeBudgetProgram instruction data in gRPC transactions.
    #[serde(default)]
    pub cu_price_micro_lamports: Option<u64>,

    /// CU limit from SetComputeUnitLimit instruction.
    #[serde(default)]
    pub compute_unit_limit: Option<u32>,

    /// Total number of inner instructions across all top-level instructions.
    /// Extracted from meta.inner_instructions in gRPC transactions.
    #[serde(default)]
    pub inner_ix_count: Option<u32>,

    /// Maximum CPI stack depth (max stack_height from inner instructions).
    /// Extracted from meta.inner_instructions in gRPC transactions.
    #[serde(default)]
    pub cpi_depth: Option<u32>,

    /// Number of ATA-creation inner instructions (heuristic: 4-instruction groups).
    #[serde(default)]
    pub ata_create_count: Option<u32>,

    /// SOL pre-balance (lamports) of the signer before the transaction.
    /// Extracted from meta.pre_balances at the signer's account index.
    #[serde(default)]
    pub signer_pre_balance_lamports: Option<u64>,

    /// SOL post-balance (lamports) of the signer after the transaction.
    /// Extracted from meta.post_balances at the signer's account index.
    #[serde(default)]
    pub signer_post_balance_lamports: Option<u64>,

    /// Deterministic Jito-tip detection.
    /// Some(true/false) when tx instructions were available to inspect, None otherwise.
    #[serde(default)]
    pub jito_tip_detected: Option<bool>,

    /// Parser-side raw infrastructure fingerprint used by FTDI/DBIA.
    #[serde(default, skip_serializing_if = "ToolchainFingerprintInput::is_empty")]
    pub toolchain_fingerprint: ToolchainFingerprintInput,

    /// Whether bonding curve data was successfully parsed from a trusted source.
    /// True for PumpPortal reserves and confirmed AccountUpdate parses.
    /// False for genesis_seed bootstrap or parse failures.
    #[serde(default)]
    pub curve_data_known: bool,

    /// Finality tier of the curve state attached to this trade.
    /// Defaults to `speculative` for backward-compatible deserialization.
    #[serde(default)]
    pub curve_finality: CurveFinality,

    /// True when this trade originated from the PumpSwap AMM program (pAMMBay6...).
    /// False (default) for pump.fun bonding-curve trades.
    ///
    /// Used to skip bonding-curve-specific enrichment (buy_variant, associated_bonding_curve,
    /// fee_recipient in the pump.fun sense) which is not applicable to AMM trades.
    #[serde(default)]
    pub is_pumpswap: bool,
}

/// Synthetic payload encoded into PumpPortal events.
///
/// This allows PumpPortal to carry fully parsed data without binary parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyntheticPayload {
    InitializePool(InitializePoolEvent),
    Trade(TradeEvent),
}

/// Reason why MPCF payload bytes are missing (or indication that they are present)
///
/// This enum provides epistemic clarity about the state of MPCF payload bytes,
/// allowing the system to distinguish between:
/// - Payload present (not missing)
/// - Provider limitations (not a bug)
/// - Pipeline bugs (should be investigated)
/// - Intentional filtering (expected)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RawBytesMissingReason {
    /// MPCF payload bytes are present (not missing)
    /// This is the "happy path" - bytes are available for analysis
    NotMissing,

    /// Provider/upstream does not supply raw transaction bytes
    /// (e.g., WebSocket/Helius provides parsed JSON, not raw bytes)
    ProviderDoesNotSupport,

    /// Raw bytes were available upstream but were lost in the pipeline
    /// This is a regression/bug and should be investigated
    DroppedUpstream,

    /// Raw bytes were intentionally filtered out by configuration
    /// (e.g., to save bandwidth or storage)
    FilteredByConfig,

    /// Cannot determine why raw bytes are missing
    /// Use sparingly - prefer specific reasons when possible
    /// NOTE: This means "missing for unknown reason", NOT "payload present"
    Unknown,
}

impl Default for RawBytesMissingReason {
    fn default() -> Self {
        RawBytesMissingReason::Unknown
    }
}

impl std::fmt::Display for RawBytesMissingReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RawBytesMissingReason::NotMissing => write!(f, "NotMissing"),
            RawBytesMissingReason::ProviderDoesNotSupport => write!(f, "ProviderDoesNotSupport"),
            RawBytesMissingReason::DroppedUpstream => write!(f, "DroppedUpstream"),
            RawBytesMissingReason::FilteredByConfig => write!(f, "FilteredByConfig"),
            RawBytesMissingReason::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Candidate pool to be forwarded to Oracle for scoring
///
/// This is the output of Seer and input to Oracle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidatePool {
    /// Cross-source semantic envelope carried through canonical ingest.
    #[serde(default)]
    pub semantic: EventSemanticEnvelope,

    /// Stable raw-provider identifier that delivered the pool-init observation.
    ///
    /// The field keeps `InitializePoolEvent` provenance intact at the Seer IPC
    /// boundary. It is metadata-only and not an authority selector in PR 1A.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// Configured role of the raw provider that delivered the pool-init observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_role: Option<RawProviderRoleV1>,

    /// Slot when detected
    pub slot: Option<u64>,

    /// Transaction index within the Solana slot when the upstream feed exposes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_index: Option<u32>,

    /// Effective event timestamp in milliseconds, derived via provenance helpers.
    pub event_ts_ms: Option<u64>,

    /// Explicit provenance for event/ingest time axes.
    #[serde(default)]
    pub event_time: EventTimeMetadata,

    /// Transaction signature
    pub signature: String,

    /// AMM program ID ("pumpfun" or "pumpswap")
    pub amm_program_id: Pubkey,

    /// Pool AMM account ID
    pub pool_amm_id: Pubkey,

    /// Base token mint
    pub base_mint: Pubkey,

    /// Quote token mint
    pub quote_mint: Pubkey,

    /// Bonding curve account
    pub bonding_curve: Pubkey,

    /// Creator wallet (transaction signer/payer)
    pub creator: Pubkey,

    /// Timestamp when detected (Unix timestamp)
    pub timestamp: u64,

    /// Optional: Bonding curve progress (0.0 - 1.0)
    /// For Pump.fun: 100 - ((base_balance - initial_threshold) * 100 / max_supply)
    /// For Bonk.fun: 100 - ((base_balance - 206900000) * 100 / 793100000)
    pub bonding_curve_progress: Option<f64>,

    /// Optional: Initial liquidity in SOL
    pub initial_liquidity_sol: Option<f64>,

    /// Optional: Token total supply
    pub token_total_supply: Option<u64>,

    /// Block time when pool was initialized
    pub block_time: Option<i64>,
}

/// Candidate with optional enhanced analysis
///
/// This enum allows Seer to optionally provide enhanced contextual analysis
/// when available, while maintaining backward compatibility with basic candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DetectedCandidate {
    /// Basic candidate without enhanced analysis
    Basic(CandidatePool),

    /// Enhanced candidate with transaction-level contextual analysis
    Enhanced(ghost_core::EnhancedCandidate),
}

impl From<InitializePoolEvent> for CandidatePool {
    fn from(event: InitializePoolEvent) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = event
            .block_time
            .and_then(|bt| if bt >= 0 { Some(bt as u64) } else { None })
            .unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            });

        // Convert initial reserves to SOL (lamports to SOL)
        let initial_liquidity_sol = event
            .initial_virtual_sol_reserves
            .or(event.initial_real_sol_reserves)
            .map(|lamports| lamports as f64 / 1_000_000_000.0);

        Self {
            semantic: EventSemanticEnvelope::default(),
            provider_id: event.provider_id,
            provider_role: event.provider_role,
            slot: event.slot,
            tx_index: event.tx_index,
            event_ts_ms: event
                .event_ts_ms
                .or_else(|| event_ts_from_block_time(event.block_time)),
            event_time: event.event_time,
            signature: event.signature.to_string(),
            amm_program_id: event.amm_program_id,
            pool_amm_id: event.pool_amm_id,
            base_mint: event.base_mint,
            quote_mint: event.quote_mint,
            bonding_curve: event.bonding_curve,
            creator: event.creator,
            timestamp,
            bonding_curve_progress: None, // Will be calculated from on-chain data if needed
            initial_liquidity_sol,
            token_total_supply: event.token_total_supply,
            block_time: event.block_time,
        }
    }
}

impl InitializePoolEvent {
    pub fn compat_event_ts_ms(&self) -> Option<u64> {
        self.event_time.compat_event_ts_ms(self.event_ts_ms)
    }
}

impl TradeEvent {
    pub fn effective_event_ts_ms(&self) -> Option<u64> {
        self.event_time.effective_event_ts_ms()
    }

    pub fn compat_event_ts_ms(&self) -> Option<u64> {
        self.event_time
            .compat_event_ts_ms((self.timestamp_ms > 0).then_some(self.timestamp_ms))
    }
}

impl CandidatePool {
    pub fn effective_event_ts_ms(&self) -> Option<u64> {
        self.event_time.effective_event_ts_ms()
    }

    pub fn compat_event_ts_ms(&self) -> Option<u64> {
        self.event_time.compat_event_ts_ms(self.event_ts_ms)
    }

    pub fn source_kind(&self) -> SourceKind {
        self.semantic.source_kind
    }
}

/// AMM program identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmProgram {
    /// Pump.fun AMM (Bonding Curve phase)
    PumpFun,
    /// Pump.fun AMM (post-migration, pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA)
    PumpSwap,
}

impl AmmProgram {
    /// Get the program ID as a Pubkey
    pub fn program_id(&self) -> Pubkey {
        match self {
            AmmProgram::PumpFun => "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            AmmProgram::PumpSwap => "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
                .parse()
                .unwrap(),
        }
    }

    /// Get the program name as a string
    pub fn name(&self) -> &'static str {
        match self {
            AmmProgram::PumpFun => "pumpfun",
            AmmProgram::PumpSwap => "pumpswap",
        }
    }

    /// Try to identify AMM program from a Pubkey
    pub fn from_pubkey(pubkey: &Pubkey) -> Option<Self> {
        if pubkey == &AmmProgram::PumpFun.program_id() {
            Some(AmmProgram::PumpFun)
        } else if pubkey == &AmmProgram::PumpSwap.program_id() {
            Some(AmmProgram::PumpSwap)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grpc_transaction_event(event_ts_ms: Option<u64>, block_time: Option<i64>) -> GeyserEvent {
        GeyserEvent::Transaction {
            provider_id: None,
            provider_role: None,
            observation_provenance: None,
            slot: Some(42),
            tx_index: None,
            event_ts_ms,
            arrival_ts_ms: Some(77),
            event_time: EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![],
            logs: vec![],
            block_time,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    #[test]
    fn old_geyser_transaction_json_remains_readable_and_baseline_shape_is_unchanged() {
        let event = grpc_transaction_event(Some(1_000), Some(1));
        let value = serde_json::to_value(&event).expect("serialize transaction");
        let payload = value
            .get("Transaction")
            .and_then(serde_json::Value::as_object)
            .expect("externally tagged transaction payload");

        assert!(!payload.contains_key("provider_id"));
        assert!(!payload.contains_key("provider_role"));

        let decoded: GeyserEvent =
            serde_json::from_value(value).expect("deserialize old transaction shape");
        match decoded {
            GeyserEvent::Transaction {
                provider_id,
                provider_role,
                tx_index,
                ..
            } => {
                assert_eq!(provider_id, None);
                assert_eq!(provider_role, None);
                assert_eq!(tx_index, None);
            }
            other => panic!("expected transaction, got {other:?}"),
        }
    }

    #[test]
    fn old_geyser_entry_anchor_json_defaults_provider_metadata() {
        let value = serde_json::json!({
            "EntryAnchor": {
                "slot": 7,
                "executed_transaction_count": 3,
                "raw": []
            }
        });

        let decoded: GeyserEvent =
            serde_json::from_value(value).expect("deserialize old entry anchor shape");
        match decoded {
            GeyserEvent::EntryAnchor {
                provider_id,
                provider_role,
                slot,
                executed_transaction_count,
                raw,
            } => {
                assert_eq!(provider_id, None);
                assert_eq!(provider_role, None);
                assert_eq!(slot, 7);
                assert_eq!(executed_transaction_count, 3);
                assert!(raw.is_empty());
            }
            other => panic!("expected entry anchor, got {other:?}"),
        }

        let event = GeyserEvent::EntryAnchor {
            provider_id: None,
            provider_role: None,
            slot: 7,
            executed_transaction_count: 3,
            raw: vec![],
        };
        let value = serde_json::to_value(&event).expect("serialize entry anchor");
        let payload = value
            .get("EntryAnchor")
            .and_then(serde_json::Value::as_object)
            .expect("externally tagged entry anchor payload");
        assert!(!payload.contains_key("provider_id"));
        assert!(!payload.contains_key("provider_role"));
    }

    #[test]
    fn test_amm_program_identification() {
        let pump_id = AmmProgram::PumpFun.program_id();
        assert_eq!(AmmProgram::from_pubkey(&pump_id), Some(AmmProgram::PumpFun));

        let random_id = Pubkey::new_unique();
        assert_eq!(AmmProgram::from_pubkey(&random_id), None);
    }

    #[test]
    fn test_amm_program_names() {
        assert_eq!(AmmProgram::PumpFun.name(), "pumpfun");
    }

    #[test]
    fn slot_zero_normalized_to_none() {
        assert_eq!(normalize_slot(Some(0)), None);
        assert_eq!(normalize_slot(Some(42)), Some(42));
        assert_eq!(normalize_slot(None), None);
    }

    #[test]
    fn grpc_compat_timestamp_without_block_time_is_wallclock_not_chain() {
        let event = grpc_transaction_event(Some(1_234), None);

        let event_time = transaction_event_time(&event);
        assert_eq!(event_time.chain_event_ts_ms, None);
        assert_eq!(event_time.ingress_wall_ts_ms, Some(1_234));
        assert_eq!(event.compat_event_ts_ms(), Some(1_234));
        assert_eq!(
            transaction_timestamp_quality_from_event(&event),
            Some(ghost_core::TimestampQuality::WallClock)
        );
    }

    #[test]
    fn grpc_chain_time_stays_authoritative_even_with_legacy_compat_field() {
        let event = grpc_transaction_event(Some(1_234), Some(2));

        let event_time = transaction_event_time(&event);
        assert_eq!(event_time.chain_event_ts_ms, Some(2_000));
        assert_eq!(event_time.ingress_wall_ts_ms, None);
        assert_eq!(event.compat_event_ts_ms(), Some(2_000));
        assert_eq!(event.effective_event_ts_ms(), Some(2_000));
        assert_eq!(
            transaction_timestamp_quality_from_event(&event),
            Some(ghost_core::TimestampQuality::Chain)
        );
    }
}
