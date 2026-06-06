//! Seer - Real-time InitializePool Detection Module
//!
//! This module detects `InitializePool` events from Pump.fun and Bonk.fun AMMs
//! in real-time from the Solana blockchain using Geyser/WebSocket streaming.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐
//! │   Geyser    │
//! │  WebSocket  │
//! └──────┬──────┘
//!        │ Stream
//!        ▼
//! ┌─────────────┐
//! │   Binary    │
//! │   Parser    │
//! └──────┬──────┘
//!        │ InitializePoolEvent
//!        ▼
//! ┌─────────────┐
//! │  Candidate  │
//! │   Builder   │
//! └──────┬──────┘
//!        │ CandidatePool
//!        ▼
//! ┌─────────────┐
//! │   Oracle    │
//! │   Scorer    │
//! └─────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! use seer::Seer;
//! use seer::config::SeerConfig;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = SeerConfig::default();
//!     let (candidate_tx, mut candidate_rx) = tokio::sync::mpsc::channel(100);
//!     
//!     let seer = std::sync::Arc::new(Seer::new(config, candidate_tx));
//!     
//!     // Run Seer in background
//!     tokio::spawn(async move {
//!         seer.run().await.expect("Seer failed");
//!     });
//!     
//!     // Process candidates
//!     while let Some(candidate) = candidate_rx.recv().await {
//!         println!("New pool: {:?}", candidate);
//!     }
//! }
//! ```

pub mod binary_parser;
pub mod config;
pub mod curve_parser;
pub mod early_fingerprint;
pub mod enhanced_builder;
pub mod errors;
pub mod grpc_connection;
pub mod helius_websocket_adapter;
pub mod ipc;
pub mod metrics;
pub mod nln_program_streams;
pub mod paradox_sensor;
pub mod pumpportal_connection;
pub mod rpc_http_client;
pub mod types;
pub mod websocket_connection;

pub use crate::curve_parser::{parse_curve_from_account, ParseCurveError};
pub use crate::rpc_http_client::{
    configure_rpc_http_auth, new_async_rpc_client, new_async_rpc_client_with_timeout,
    new_blocking_rpc_client, new_blocking_rpc_client_with_timeout, rpc_http_auth_applies_to_url,
    DEFAULT_RPC_AUTH_HEADER, LEGACY_PROVIDER_AUTH_HEADER_ENV, LEGACY_PROVIDER_AUTH_TOKEN_ENV,
    RPC_HTTP_AUTH_HEADER_ENV, RPC_HTTP_AUTH_TOKEN_ENV, RPC_HTTP_USER_AGENT,
};

use binary_parser::{BinaryParser, PumpAccountState};

use config::{FundingLaneMode, SeerConfig, SeerSourceMode};
use errors::{SeerError, SeerResult};
use futures_util::StreamExt;
use grpc_connection::{
    Bcv2AccountContext, EventStream, GrpcConnection, GrpcSubscriptionProfile,
    GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL, GRPC_FUNDING_LANE_PUMP_FILTERED_SOURCE_LABEL,
    GRPC_GLOBAL_STREAM_SOURCE_LABEL,
};
use helius_websocket_adapter::HeliusWebSocketAdapter;
use ipc::{AccountUpdateReplayOrigin, EventPriority, IpcSender};
use metrics::SeerMetrics;
use paradox_sensor::ParadoxSensor;
use parking_lot::{Mutex, RwLock};
use pumpportal_connection::PumpPortalConnection;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::{HashMap, HashSet, VecDeque};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tracing::{debug, error, info, trace, warn};
use types::{
    record_trade_outcome_metric, AmmProgram, CandidatePool, SyntheticPayload, TradeOutcome,
};
use websocket_connection::{
    extract_balances_from_meta, extract_inner_instructions_from_meta, extract_logs_from_meta,
    extract_token_balances_from_meta, parse_ui_transaction_with_meta, WebSocketConnection,
};

fn is_no_space_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_err| io_err.raw_os_error() == Some(28))
    })
}

use ghost_core::coverage_audit;
use ghost_core::health::RuntimeHealth;
use ghost_core::market_state::BondingCurve;
use ghost_core::CoverageAuditWatchRegistration;
use ghost_core::{
    normalize_account_update_semantics, normalize_transaction_semantics,
    record_event_semantic_metric, EventSemanticEnvelope, ExecutionAccountEvidence,
    ExecutionAccountEvidenceSource, ExecutionAccountEvidenceStatus, ExecutionAccountRole,
    TimestampQuality,
};
use ghost_core::{pipeline_coverage, PipelineCoverageStage};
use ghost_core::{ParsedEventKind as WalParsedEventKind, Wal, WalRecord, WalRecordClock};
// EPIC 2: bootstrap_snapshots and BootstrapConfig are no longer used here
// since we no longer inject synthetic snapshots to ShadowLedger from Seer
use ghost_core::shadow_ledger::{
    protocol_genesis_curve, CurveWriteMetadata, ShadowLedger, ShadowLedgerStateConfidence,
    ShadowLedgerWriteReason, ShadowLedgerWriteSource, ShadowLedgerWriteStrength,
};
use humantime::format_rfc3339_seconds;

const LATE_DETECTION_THRESHOLD_MS: f64 = 300.0;
const DEV_BUY_SOL_SANITY_LIMIT: f64 = 5_000.0;
const COVERAGE_LOG_INTERVAL: Duration = Duration::from_secs(10);
const PARSE_MISS_LOG_EVERY: u64 = 200;
const PENDING_TRADE_TTL: Duration = Duration::from_millis(30);
const PENDING_TRADES_PER_CURVE_MAX: usize = 1_024;
const EVENT_WORKERS_PER_CORE: usize = 2;
const MIN_EVENT_WORKERS: usize = 4;
const MAX_EVENT_WORKERS: usize = 32;
const ENTRY_CPI_SCAN_QUEUE_CAP: usize = 64;
/// Number of lamports used for seed-curve bootstrap reserve defaults.
/// 1 SOL = 1_000_000_000 lamports; 10_000_000 lamports (≈0.01 SOL) mirrors
/// the historical genesis bootstrap size.
pub const GENESIS_BOOTSTRAP_LAMPORTS: u64 = 10_000_000;
const WSOL_MINT_STR: &str = "So11111111111111111111111111111111111111112";

fn wsol_mint_pubkey() -> &'static Pubkey {
    use std::sync::OnceLock;
    static PK: OnceLock<Pubkey> = OnceLock::new();
    PK.get_or_init(|| Pubkey::from_str(WSOL_MINT_STR).expect("valid WSOL mint"))
}

fn transaction_timestamp_quality(
    event: &types::GeyserEvent,
    source_label: &str,
    synthetic: bool,
) -> TimestampQuality {
    if synthetic {
        return TimestampQuality::Adapter;
    }

    match source_label {
        "websocket" | "helius" | "pumpportal" => TimestampQuality::Adapter,
        _ => types::transaction_timestamp_quality_from_event(event)
            .unwrap_or(TimestampQuality::WallClock),
    }
}

fn transaction_semantic_from_event(
    event: &types::GeyserEvent,
    source_label: &str,
    synthetic: bool,
) -> EventSemanticEnvelope {
    let slot_present = match event {
        types::GeyserEvent::Transaction { slot, .. } => slot.is_some(),
        _ => false,
    };
    normalize_transaction_semantics(
        source_label,
        synthetic,
        slot_present,
        transaction_timestamp_quality(event, source_label, synthetic),
    )
}

fn infer_trade_timestamp_quality(
    trade: &types::TradeEvent,
    source_label: &str,
) -> TimestampQuality {
    match source_label {
        "websocket" | "helius" | "pumpportal" => TimestampQuality::Adapter,
        _ if trade.event_time.chain_event_ts_ms.is_some() => TimestampQuality::Chain,
        _ if trade.event_time.ingress_wall_ts_ms.is_some() => TimestampQuality::WallClock,
        _ => TimestampQuality::WallClock,
    }
}

const SYSTEM_PROGRAM_TRANSFER_DISCRIMINATOR: [u8; 4] = [2, 0, 0, 0];
const TOKEN_SYNC_NATIVE_DISCRIMINATOR: u8 = 17;
const TOKEN_LEGACY_PROGRAM_ID_STR: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID_STR: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

fn token_legacy_program_pubkey() -> &'static Pubkey {
    use std::sync::OnceLock;
    static PK: OnceLock<Pubkey> = OnceLock::new();
    PK.get_or_init(|| Pubkey::from_str(TOKEN_LEGACY_PROGRAM_ID_STR).expect("valid tokenkeg id"))
}

fn token_2022_program_pubkey() -> &'static Pubkey {
    use std::sync::OnceLock;
    static PK: OnceLock<Pubkey> = OnceLock::new();
    PK.get_or_init(|| Pubkey::from_str(TOKEN_2022_PROGRAM_ID_STR).expect("valid token2022 id"))
}

fn is_supported_token_program(program_id: &Pubkey) -> bool {
    *program_id == *token_legacy_program_pubkey() || *program_id == *token_2022_program_pubkey()
}

fn is_sync_native_instruction(program_id: &Pubkey, data: &[u8]) -> bool {
    is_supported_token_program(program_id)
        && data.first().copied() == Some(TOKEN_SYNC_NATIVE_DISCRIMINATOR)
}

fn collect_sync_native_accounts(event: &types::GeyserEvent) -> HashSet<Pubkey> {
    let types::GeyserEvent::Transaction {
        accounts,
        instructions,
        inner_instructions,
        ..
    } = event
    else {
        return HashSet::new();
    };

    let mut sync_native_accounts = HashSet::new();

    for instruction in instructions {
        if !is_sync_native_instruction(&instruction.program_id, &instruction.data) {
            continue;
        }
        let Some(&account_index) = instruction.account_indices.first() else {
            continue;
        };
        let Some(account) = accounts.get(account_index as usize) else {
            continue;
        };
        sync_native_accounts.insert(*account);
    }

    for group in inner_instructions {
        for instruction in &group.instructions {
            let Some(program_id) = accounts.get(instruction.program_id_index as usize) else {
                continue;
            };
            if !is_sync_native_instruction(program_id, &instruction.data) {
                continue;
            }
            let Some(&account_index) = instruction.accounts.first() else {
                continue;
            };
            let Some(account) = accounts.get(account_index as usize) else {
                continue;
            };
            sync_native_accounts.insert(*account);
        }
    }

    sync_native_accounts
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingTransferObservation {
    pub source_wallet: String,
    pub recipient_wallet: String,
    pub lamports: u64,
    pub event_ordinal: Option<u32>,
    pub outer_instruction_index: Option<u32>,
    pub inner_group_index: Option<u32>,
    pub cpi_stack_height: Option<u32>,
}

fn parse_system_transfer_lamports(data: &[u8]) -> Option<u64> {
    if data.get(..4) != Some(&SYSTEM_PROGRAM_TRANSFER_DISCRIMINATOR) || data.len() < 12 {
        return None;
    }

    Some(u64::from_le_bytes(data[4..12].try_into().ok()?))
}

fn token_account_owner_overrides(
    accounts: &[Pubkey],
    pre_token_balances: &[types::RawTokenBalance],
    post_token_balances: &[types::RawTokenBalance],
) -> HashMap<Pubkey, String> {
    let mut owners = HashMap::new();
    for balance in pre_token_balances.iter().chain(post_token_balances.iter()) {
        let Some(owner) = balance.owner.as_ref().map(|owner| owner.trim()) else {
            continue;
        };
        if owner.is_empty() {
            continue;
        }
        let Some(account) = accounts.get(balance.account_index as usize) else {
            continue;
        };
        owners.entry(*account).or_insert_with(|| owner.to_string());
    }
    owners
}

fn canonicalize_funding_wallet(
    accounts: &[Pubkey],
    account_index: u8,
    token_account_owner_overrides: &HashMap<Pubkey, String>,
) -> Option<String> {
    let account = accounts.get(account_index as usize)?;
    Some(
        token_account_owner_overrides
            .get(account)
            .cloned()
            .unwrap_or_else(|| account.to_string()),
    )
}

fn extracted_funding_transfer_from_accounts(
    accounts: &[Pubkey],
    token_account_owner_overrides: &HashMap<Pubkey, String>,
    source_index: u8,
    recipient_index: u8,
    lamports: u64,
    event_ordinal: Option<u32>,
    outer_instruction_index: Option<u32>,
    inner_group_index: Option<u32>,
    cpi_stack_height: Option<u32>,
) -> Option<FundingTransferObservation> {
    if lamports == 0 {
        return None;
    }

    let source_wallet =
        canonicalize_funding_wallet(accounts, source_index, token_account_owner_overrides)?;
    let recipient_wallet =
        canonicalize_funding_wallet(accounts, recipient_index, token_account_owner_overrides)?;
    if source_wallet.is_empty() || recipient_wallet.is_empty() || source_wallet == recipient_wallet
    {
        return None;
    }

    Some(FundingTransferObservation {
        source_wallet,
        recipient_wallet,
        lamports,
        event_ordinal,
        outer_instruction_index,
        inner_group_index,
        cpi_stack_height,
    })
}

pub fn extract_funding_transfer_observations(
    event: &types::GeyserEvent,
) -> Vec<FundingTransferObservation> {
    let types::GeyserEvent::Transaction {
        accounts,
        instructions,
        inner_instructions,
        pre_token_balances,
        post_token_balances,
        success,
        ..
    } = event
    else {
        return Vec::new();
    };

    if !success {
        return Vec::new();
    }

    let mut transfers = Vec::new();
    let sync_native_accounts = collect_sync_native_accounts(event);
    let token_account_owner_overrides =
        token_account_owner_overrides(accounts, pre_token_balances, post_token_balances);
    let mut instruction_ordinal: u32 = 0;

    for (outer_instruction_index, instruction) in instructions.iter().enumerate() {
        if instruction.program_id != solana_sdk::system_program::ID {
            instruction_ordinal = instruction_ordinal.saturating_add(1);
            continue;
        }
        let Some(lamports) = parse_system_transfer_lamports(&instruction.data) else {
            instruction_ordinal = instruction_ordinal.saturating_add(1);
            continue;
        };
        let Some(&source_index) = instruction.account_indices.first() else {
            instruction_ordinal = instruction_ordinal.saturating_add(1);
            continue;
        };
        let Some(&recipient_index) = instruction.account_indices.get(1) else {
            instruction_ordinal = instruction_ordinal.saturating_add(1);
            continue;
        };
        let Some(recipient_pubkey) = accounts.get(recipient_index as usize) else {
            instruction_ordinal = instruction_ordinal.saturating_add(1);
            continue;
        };
        if sync_native_accounts.contains(recipient_pubkey) {
            instruction_ordinal = instruction_ordinal.saturating_add(1);
            continue;
        }
        if let Some(transfer) = extracted_funding_transfer_from_accounts(
            accounts,
            &token_account_owner_overrides,
            source_index,
            recipient_index,
            lamports,
            Some(instruction_ordinal),
            Some(outer_instruction_index as u32),
            None,
            None,
        ) {
            transfers.push(transfer);
        }
        instruction_ordinal = instruction_ordinal.saturating_add(1);
    }

    for group in inner_instructions {
        let insts = &group.instructions;
        for (inner_ix_index, instruction) in insts.iter().enumerate() {
            let curr_sh = instruction.stack_height;
            let next_sh = insts.get(inner_ix_index + 1).and_then(|ix| ix.stack_height);
            let has_deeper_next = matches!((curr_sh, next_sh), (Some(c), Some(n)) if n > c);

            let Some(program_id) = accounts.get(instruction.program_id_index as usize) else {
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
                continue;
            };
            if *program_id != solana_sdk::system_program::ID {
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
                continue;
            }
            let Some(lamports) = parse_system_transfer_lamports(&instruction.data) else {
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
                continue;
            };
            let Some(&source_index) = instruction.accounts.first() else {
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
                continue;
            };
            let Some(&recipient_index) = instruction.accounts.get(1) else {
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
                continue;
            };
            let Some(recipient_pubkey) = accounts.get(recipient_index as usize) else {
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
                continue;
            };
            if sync_native_accounts.contains(recipient_pubkey) {
                if !has_deeper_next {
                    instruction_ordinal = instruction_ordinal.saturating_add(1);
                }
                continue;
            }
            if let Some(transfer) = extracted_funding_transfer_from_accounts(
                accounts,
                &token_account_owner_overrides,
                source_index,
                recipient_index,
                lamports,
                Some(instruction_ordinal),
                Some(group.index),
                Some(group.index),
                instruction.stack_height,
            ) {
                transfers.push(transfer);
            }
            if !has_deeper_next {
                instruction_ordinal = instruction_ordinal.saturating_add(1);
            }
        }
    }

    transfers
}

#[derive(Default)]
struct CoverageCounters {
    rx_tx_pool_stream_total: AtomicU64,
    rx_tx_global_stream_total: AtomicU64,
    // ── Signature-level counters (one per unique on-chain tx signature) ──────
    /// Unique tx signatures seen that contained a trade candidate instruction.
    trade_candidate_total: AtomicU64,
    /// Unique tx signatures whose parse produced at least one TradeEvent.
    trade_parsed_total: AtomicU64,
    /// Unique tx signatures that failed to parse despite being a candidate.
    parse_miss_total: AtomicU64,
    /// Unique tx signatures successfully forwarded live (first seen, not replay).
    trade_signatures_forwarded_live_total: AtomicU64,
    /// Unique tx signatures successfully forwarded via replay path.
    trade_signatures_forwarded_replay_total: AtomicU64,
    // ── Event-level counters (one per TradeEvent object emitted) ─────────────
    /// Number of TradeEvent objects forwarded live to IPC.
    trade_events_forwarded_live_total: AtomicU64,
    /// Number of TradeEvent objects forwarded via the replay/recover path.
    /// Incremented once per successfully IPC-sent replayed TradeEvent (same condition as
    /// `trade_signatures_forwarded_replay_total` but before the signature dedup check).
    trade_events_forwarded_replay_total: AtomicU64,
    /// Number of TradeEvent objects placed into the pending-mapping buffer.
    pending_mapping_buffered_total: AtomicU64,
    /// Number of TradeEvent objects that expired in the pending buffer (TTL).
    trade_events_expired_total: AtomicU64,
    trade_filtered_total: AtomicU64,
    /// Unique forwarded tx signatures whose source was grpc_backfill.
    rpc_fallback_signatures_forwarded_total: AtomicU64,
    /// Forwarded TradeEvent objects whose source was grpc_backfill.
    rpc_fallback_events_forwarded_total: AtomicU64,
    /// Raw sum of executed_transaction_count from EntryAnchor events for slot-throughput diagnostics.
    entry_anchor_tx_total: AtomicU64,
}

/// Structured coverage summary split by level (event vs signature) and path (live vs replay).
#[derive(Debug, Clone, Copy, PartialEq)]
struct CoverageSummary {
    // Signature-level
    trade_candidates: u64,
    sigs_parsed: u64,
    sigs_forwarded_live: u64,
    sigs_forwarded_replay: u64,
    // Event-level
    events_forwarded_live: u64,
    events_forwarded_replay: u64,
    events_buffered: u64,
    events_expired: u64,
    rpc_fallback_sigs: u64,
    rpc_fallback_events: u64,
    // Derived percentages – all from signature-level to avoid mixing levels
    /// Fraction of candidate signatures that were successfully parsed (%).
    parser_coverage_pct: f64,
    /// Fraction of parsed signatures forwarded live (instant, before replay) (%).
    live_coverage_pct: f64,
    /// Fraction of parsed signatures forwarded at all (live + replay) (%).
    eventual_coverage_pct: f64,
    /// Fraction of forwarded events that were live (event-level) (%).
    event_live_pct: f64,
    /// Fraction of forwarded events that were replayed (event-level) (%).
    event_replay_pct: f64,
    /// Fraction of forwarded signatures that came from grpc_backfill (%).
    rpc_fallback_sig_share_pct: f64,
    /// Fraction of forwarded events that came from grpc_backfill (%).
    rpc_fallback_event_share_pct: f64,
}

fn summarize_coverage(
    trade_candidates: u64,
    sigs_parsed: u64,
    sigs_forwarded_live: u64,
    sigs_forwarded_replay: u64,
    events_forwarded_live: u64,
    events_forwarded_replay: u64,
    events_buffered: u64,
    events_expired: u64,
    rpc_fallback_sigs: u64,
    rpc_fallback_events: u64,
) -> CoverageSummary {
    let parser_coverage_pct = if trade_candidates == 0 {
        100.0
    } else {
        (sigs_parsed as f64 / trade_candidates as f64) * 100.0
    };
    let live_coverage_pct = if sigs_parsed == 0 {
        if trade_candidates == 0 {
            100.0
        } else {
            0.0
        }
    } else {
        (sigs_forwarded_live as f64 / sigs_parsed as f64) * 100.0
    };
    let sigs_forwarded_total = sigs_forwarded_live + sigs_forwarded_replay;
    let eventual_coverage_pct = if sigs_parsed == 0 {
        if trade_candidates == 0 {
            100.0
        } else {
            0.0
        }
    } else {
        (sigs_forwarded_total as f64 / sigs_parsed as f64) * 100.0
    };
    let events_forwarded_total = events_forwarded_live + events_forwarded_replay;
    let event_live_pct = if events_forwarded_total == 0 {
        0.0
    } else {
        (events_forwarded_live as f64 / events_forwarded_total as f64) * 100.0
    };
    let event_replay_pct = if events_forwarded_total == 0 {
        0.0
    } else {
        (events_forwarded_replay as f64 / events_forwarded_total as f64) * 100.0
    };
    let rpc_fallback_sig_share_pct = if sigs_forwarded_total == 0 {
        0.0
    } else {
        (rpc_fallback_sigs as f64 / sigs_forwarded_total as f64) * 100.0
    };
    let rpc_fallback_event_share_pct = if events_forwarded_total == 0 {
        0.0
    } else {
        (rpc_fallback_events as f64 / events_forwarded_total as f64) * 100.0
    };
    CoverageSummary {
        trade_candidates,
        sigs_parsed,
        sigs_forwarded_live,
        sigs_forwarded_replay,
        events_forwarded_live,
        events_forwarded_replay,
        events_buffered,
        events_expired,
        rpc_fallback_sigs,
        rpc_fallback_events,
        parser_coverage_pct,
        live_coverage_pct,
        eventual_coverage_pct,
        event_live_pct,
        event_replay_pct,
        rpc_fallback_sig_share_pct,
        rpc_fallback_event_share_pct,
    }
}

#[derive(Clone)]
struct PendingCurveUpdateSnapshot {
    slot: u64,
    event_time: ghost_core::EventTimeMetadata,
    write_version: Option<u64>,
    owner: Pubkey,
    data: Vec<u8>,
    queued_at: Instant,
}

impl PendingCurveUpdateSnapshot {
    fn new(
        slot: u64,
        event_time: ghost_core::EventTimeMetadata,
        write_version: Option<u64>,
        owner: Pubkey,
        data: &[u8],
        queued_at: Instant,
    ) -> Self {
        Self {
            slot,
            event_time,
            write_version,
            owner,
            data: data.to_vec(),
            queued_at,
        }
    }

    fn ordering_key(&self) -> (u64, u64) {
        (
            self.slot,
            account_update_write_version_key(self.write_version),
        )
    }
}

#[derive(Clone)]
struct PendingCurveUpdate {
    earliest: PendingCurveUpdateSnapshot,
    latest: PendingCurveUpdateSnapshot,
}

impl PendingCurveUpdate {
    fn new(snapshot: PendingCurveUpdateSnapshot) -> Self {
        Self {
            earliest: snapshot.clone(),
            latest: snapshot,
        }
    }

    fn store(&mut self, snapshot: PendingCurveUpdateSnapshot) -> PendingCurveUpdateStoreOutcome {
        let snapshot_key = snapshot.ordering_key();
        if snapshot_key < self.earliest.ordering_key() {
            return PendingCurveUpdateStoreOutcome::IgnoredOlder;
        }
        if snapshot_key < self.latest.ordering_key() {
            return PendingCurveUpdateStoreOutcome::IgnoredOlder;
        }
        self.latest = snapshot;
        PendingCurveUpdateStoreOutcome::ReplacedNewer
    }

    fn replay_snapshots(self) -> Vec<PendingCurveUpdateSnapshot> {
        if self.earliest.ordering_key() == self.latest.ordering_key() {
            vec![self.latest]
        } else {
            vec![self.earliest, self.latest]
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingCurveUpdateStoreOutcome {
    Inserted,
    ReplacedNewer,
    IgnoredOlder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalAccountUpdatePayload {
    sol_reserves: u64,
    token_reserves: u64,
    complete: u8,
    token_mint: Option<Pubkey>,
}

impl CanonicalAccountUpdatePayload {
    #[inline]
    pub fn sol_reserves(&self) -> u64 {
        self.sol_reserves
    }

    #[inline]
    pub fn token_reserves(&self) -> u64 {
        self.token_reserves
    }

    #[inline]
    pub fn complete(&self) -> u8 {
        self.complete
    }

    #[inline]
    pub fn token_mint(&self) -> Option<Pubkey> {
        self.token_mint
    }
}

pub fn decode_canonical_account_update(
    owner: Pubkey,
    data: &[u8],
) -> Result<CanonicalAccountUpdatePayload, String> {
    match binary_parser::decode_account_data(data) {
        PumpAccountState::BondingCurve(curve) => Ok(CanonicalAccountUpdatePayload {
            sol_reserves: curve.virtual_sol_reserves,
            token_reserves: curve.virtual_token_reserves,
            complete: u8::from(curve.complete),
            token_mint: None,
        }),
        PumpAccountState::AmmPool(pool) => {
            let base_mint = Pubkey::new_from_array(pool.base_mint);
            let quote_mint = Pubkey::new_from_array(pool.quote_mint);
            if base_mint == *wsol_mint_pubkey() {
                Ok(CanonicalAccountUpdatePayload {
                    sol_reserves: pool.base_amount,
                    token_reserves: pool.quote_amount,
                    complete: 1,
                    token_mint: Some(quote_mint),
                })
            } else if quote_mint == *wsol_mint_pubkey() {
                Ok(CanonicalAccountUpdatePayload {
                    sol_reserves: pool.quote_amount,
                    token_reserves: pool.base_amount,
                    complete: 1,
                    token_mint: Some(base_mint),
                })
            } else {
                Err(format!(
                    "unsupported amm pair base_mint={base_mint} quote_mint={quote_mint}"
                ))
            }
        }
        PumpAccountState::Unknown { .. } => parse_curve_from_account(data)
            .map(|curve| CanonicalAccountUpdatePayload {
                sol_reserves: curve.virtual_sol_reserves,
                token_reserves: curve.virtual_token_reserves,
                complete: curve.complete,
                token_mint: None,
            })
            .map_err(|err| err.to_string()),
        PumpAccountState::Global(_) => Err(format!(
            "unsupported account layout for owner={owner}: global_state"
        )),
    }
}

#[inline]
fn account_update_write_version_key(write_version: Option<u64>) -> u64 {
    write_version.unwrap_or(u64::MAX)
}

/// Explicit reason why a trade was placed into the pending-trades buffer.
/// Stored on each `PendingTrade` so that metrics and replay logic can
/// distinguish the different unresolved cases without relying on string tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingTradeReason {
    /// `pool_amm_id` is known but the curve→mint mapping is not yet registered.
    CurveMappingMissing,
    /// `pool_amm_id` is default (unknown), but `mint` is known.
    MissingPoolFromMint,
    /// Both `pool_amm_id` and `mint` are default — weakest unresolved case.
    MissingMintAndPool,
    /// A curve→mint mapping exists but conflicts with the trade's mint.
    MappingConflict,
}

impl PendingTradeReason {
    /// Returns a stable snake_case label suitable for use as a metrics tag.
    fn as_str(&self) -> &'static str {
        match self {
            PendingTradeReason::CurveMappingMissing => "curve_mapping_missing",
            PendingTradeReason::MissingPoolFromMint => "missing_pool_from_mint",
            PendingTradeReason::MissingMintAndPool => "missing_mint_and_pool",
            PendingTradeReason::MappingConflict => "mapping_conflict",
        }
    }

    fn buffered_outcome(&self) -> TradeOutcome {
        match self {
            PendingTradeReason::MissingPoolFromMint | PendingTradeReason::MissingMintAndPool => {
                TradeOutcome::BufferedMissingPool
            }
            PendingTradeReason::CurveMappingMissing | PendingTradeReason::MappingConflict => {
                TradeOutcome::BufferedMissingMapping
            }
        }
    }
}

/// Typed buffer key for pending trades.
///
/// Using a plain `[u8; 32]` for every case caused all "unresolved" trades
/// (where both `pool_amm_id` and `mint` are `Pubkey::default()`) to collide
/// under the all-zeros key.  This enum makes the three cases explicit and
/// collision-free.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PendingTradeKey {
    /// Trade keyed by the bonding-curve / pool pubkey (most common case).
    ByCurve([u8; 32]),
    /// Trade keyed by mint — used when `pool_amm_id` is not yet known.
    ByMint([u8; 32]),
    /// Fallback: keyed by the first 32 bytes of the transaction signature.
    /// Used only when both `pool_amm_id` and `mint` are unknown; these
    /// trades have no active resolve path and will expire after TTL.
    BySignature(String),
}

#[derive(Clone)]
struct PendingTrade {
    trade: types::TradeEvent,
    source_label: String,
    is_coverage_source: bool,
    queued_at: Instant,
    reason: PendingTradeReason,
}

#[derive(Debug)]
struct EntryCpiScanJob {
    slot: u64,
    raw: Vec<u8>,
}

enum TradeForwardDecision {
    /// Forward live; no side effects needed after emit.
    Forward,
    /// Forward live and trigger replay of any pending trades for this pool→mint pair.
    /// Carries (pool, mint) so `handle_trade_event` can call `replay_pending_trades`.
    ForwardWithReplay(Pubkey, Pubkey),
    BufferedPendingMapping,
    Filtered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PoolInitCandidateMode {
    Observe,
    ContinuityOnly { observation_pool: Pubkey },
    Suppressed,
}

/// Genesis bonding curve template representing the default Pump.fun starting state.
/// Used only as a temporary seed until the first real AccountUpdate arrives.
pub fn genesis_curve() -> BondingCurve {
    protocol_genesis_curve()
}

// parse_curve_from_account is now provided by curve_parser module (re-exported above).

/// Store a bonding curve in ShadowLedger (curve only, no synthetic snapshots).
///
/// # EPIC 2 Compliance (Single-Writer Architecture)
///
/// **IMPORTANT**: This function NO LONGER injects bootstrap snapshots (G0/G1/G2)
/// into ShadowLedger.
///
/// Canonical snapshot flow:
/// - Gatekeeper collects TX from mint creation
/// - After Gatekeeper pass, atomically commits sorted history to ShadowLedger
/// - Live TX appends continue after commit
///
/// This ensures:
/// - No slot=0 snapshots pollute canonical history
/// - Single deterministic writer (Gatekeeper commit + live append)
/// - Scoring has immediate access to real TX-based data
///
/// Note on `slot=0`:
/// - `slot=0` is reserved only for seed curve states (curve math bootstrap),
///   never for canonical TX snapshots.
fn store_curve_with_snapshots(
    ledger: &ShadowLedger,
    curve_key: Pubkey,
    base_mint: Pubkey,
    curve: BondingCurve,
    slot: Option<u64>,
    curve_data_known: bool,
) {
    let metadata = if let Some(slot) = slot.filter(|slot| *slot > 0) {
        CurveWriteMetadata::new(
            if curve_data_known {
                ShadowLedgerWriteSource::RpcBootstrapSeeder
            } else {
                ShadowLedgerWriteSource::AccountUpdate
            },
            if curve_data_known {
                ShadowLedgerWriteStrength::ConfirmedBootstrap
            } else {
                ShadowLedgerWriteStrength::Repair
            },
            if curve_data_known {
                ShadowLedgerStateConfidence::Observed
            } else {
                ShadowLedgerStateConfidence::Speculative
            },
            if curve_data_known {
                ShadowLedgerWriteReason::ConfirmedBootstrap
            } else {
                ShadowLedgerWriteReason::DirectAccountUpdate
            },
            Some(slot),
            ghost_core::CurveFinality::from_curve_data_known(curve_data_known),
        )
    } else {
        CurveWriteMetadata::new(
            ShadowLedgerWriteSource::SeerBootstrap,
            ShadowLedgerWriteStrength::BootstrapSeed,
            ShadowLedgerStateConfidence::Speculative,
            ShadowLedgerWriteReason::BootstrapSeed,
            None,
            ghost_core::CurveFinality::Speculative,
        )
    };
    let _ = ledger.apply_curve_write(Some(base_mint), curve_key, curve, metadata);
    // EPIC 2: Do NOT inject bootstrap snapshots to ShadowLedger.
    // Bootstrap snapshots (G0/G1/G2) have slot=0 which is prohibited.
    // Canonical snapshots come from Gatekeeper commit + live TX append only.
    //
    // Previously: ledger.set_snapshots(base_mint, snapshots);
    // Now: No snapshot injection from Seer bootstrap path.
}

pub fn store_bootstrap_seed(
    ledger: &ShadowLedger,
    curve_key: Pubkey,
    base_mint: Pubkey,
    curve: BondingCurve,
) {
    let _ = ledger.apply_curve_write(
        Some(base_mint),
        curve_key,
        curve,
        CurveWriteMetadata::new(
            ShadowLedgerWriteSource::SeerBootstrap,
            ShadowLedgerWriteStrength::BootstrapSeed,
            ShadowLedgerStateConfidence::Speculative,
            ShadowLedgerWriteReason::BootstrapSeed,
            None,
            ghost_core::CurveFinality::Speculative,
        ),
    );
}

pub fn store_confirmed_bootstrap(
    ledger: &ShadowLedger,
    curve_key: Pubkey,
    base_mint: Pubkey,
    curve: BondingCurve,
    slot: u64,
) {
    let _ = ledger.apply_curve_write(
        Some(base_mint),
        curve_key,
        curve,
        CurveWriteMetadata::new(
            ShadowLedgerWriteSource::RpcBootstrapSeeder,
            ShadowLedgerWriteStrength::ConfirmedBootstrap,
            ShadowLedgerStateConfidence::Observed,
            ShadowLedgerWriteReason::ConfirmedBootstrap,
            Some(slot),
            ghost_core::CurveFinality::Provisional,
        ),
    );
}

#[doc(hidden)]
pub fn store_repair_curve(
    ledger: &ShadowLedger,
    curve_key: Pubkey,
    base_mint: Pubkey,
    curve: BondingCurve,
    slot: u64,
    curve_finality: ghost_core::CurveFinality,
) {
    let _ = ledger.apply_curve_write(
        Some(base_mint),
        curve_key,
        curve,
        CurveWriteMetadata::new(
            ShadowLedgerWriteSource::AccountUpdate,
            ShadowLedgerWriteStrength::Repair,
            ShadowLedgerStateConfidence::Observed,
            ShadowLedgerWriteReason::DirectAccountUpdate,
            Some(slot),
            curve_finality,
        ),
    );
}

/// Main Seer component for real-time pool detection
pub struct Seer {
    /// Configuration
    config: SeerConfig,

    /// WebSocket connection manager (optional, used when mode is WebSocket)
    ws_connection: Option<WebSocketConnection>,

    /// gRPC connection manager (optional, used when mode is gRPC)
    grpc_connection: Option<GrpcConnection>,

    /// Optional dedicated funding-transfer lane via Yellowstone gRPC.
    funding_grpc_connection: Option<GrpcConnection>,

    /// Optional WebSocket fallback when gRPC commitment is unsupported
    ws_fallback: Option<WebSocketConnection>,

    /// Helius WebSocket adapter (optional, used for HeliusWebSocket mode)
    helius_adapter: Option<HeliusWebSocketAdapter>,

    /// PumpPortal WebSocket connection (optional, used for PumpPortalWs mode)
    pumpportal_connection: Option<PumpPortalConnection>,

    /// Binary parser for instruction detection (optional, None in PumpPortal mode)
    parser: Option<BinaryParser>,

    /// Best-effort Entry CPI scan queue. The worker is started lazily from `run()`
    /// and must never be allowed to stall the shared Seer event workers.
    entry_cpi_scan_tx: Option<mpsc::Sender<EntryCpiScanJob>>,
    entry_cpi_scan_rx: Mutex<Option<mpsc::Receiver<EntryCpiScanJob>>>,

    /// Metrics collector
    metrics: Arc<SeerMetrics>,

    /// Channel to send detected candidates (for backward compatibility)
    candidate_sender: Option<mpsc::Sender<CandidatePool>>,

    /// IPC sender for Seer→Trigger communication
    ipc_sender: Option<IpcSender>,

    /// Paradox Sensor for network telemetry analysis
    paradox_sensor: Option<Arc<ParadoxSensor>>,

    /// Paradox Sensor state receiver
    paradox_rx: Option<tokio::sync::watch::Receiver<paradox_sensor::ParadoxState>>,

    /// Backpressure guard flag for ultrafast degraded mode
    ultrafast_mode: AtomicBool,

    /// Optional ShadowLedger for direct curve updates
    shadow_ledger: Option<Arc<ShadowLedger>>,

    /// Tracked bonding_curve -> base_mint mapping for AccountUpdate routing
    tracked_curves: Arc<RwLock<HashMap<Pubkey, Pubkey>>>,

    /// Curve -> mint registry used by mint-first pipelines (bytes-level, lock-light).
    curve_to_mint: Arc<RwLock<HashMap<[u8; 32], [u8; 32]>>>,

    /// Optional reverse mapping for fast mint -> curve lookups.
    mint_to_curve: Arc<RwLock<HashMap<[u8; 32], [u8; 32]>>>,

    /// Bounded earliest+latest parsable account updates kept until mapping is resolved.
    pending_curve_updates: Arc<RwLock<HashMap<[u8; 32], PendingCurveUpdate>>>,

    /// Trades buffered until curve->mint mapping becomes known.
    /// Keyed by `PendingTradeKey` (ByCurve / ByMint / BySignature) so that
    /// unresolved trades never collide under the all-zeros default pubkey.
    pending_trades: Arc<RwLock<HashMap<PendingTradeKey, VecDeque<PendingTrade>>>>,

    /// Rate-limit map for UNSUPPORTED_LAYOUT debug logs (pubkey -> last log Instant).
    /// Max 1 log per pubkey per 60 seconds to avoid log spam.
    unsupported_layout_last_log: Arc<RwLock<HashMap<Pubkey, Instant>>>,

    /// Coverage counters for tx_stream_received -> trade_parsed -> forwarded (event- and signature-level).
    coverage: Arc<CoverageCounters>,

    /// Last timestamp when coverage line was logged.
    coverage_last_log: Arc<Mutex<Instant>>,

    /// Runtime health monitor for watchdog heartbeats
    health: Option<Arc<RuntimeHealth>>,

    /// Optional write-ahead log for ingest durability and replay diagnostics.
    wal: Option<Arc<Wal>>,
    wal_disabled_due_to_enospc: AtomicBool,

    /// [FIX-3] Slot when the session started to prevent backfilled pools from being forwarded
    session_start_slot: AtomicU64,

    /// Degraded/test compatibility gate for the downstream AccountUpdate relay.
    ///
    /// When `false`, canonical AccountUpdate forwarding and pending-update replay
    /// are skipped for explicit degraded/test harnesses. Production launcher
    /// startup derives the effective setting from `AccountStateCore` enablement.
    canonical_account_update_relay_enabled: bool,
}

impl Seer {
    fn event_worker_concurrency() -> usize {
        std::thread::available_parallelism()
            .map(|parallelism| parallelism.get().saturating_mul(EVENT_WORKERS_PER_CORE))
            .unwrap_or(MIN_EVENT_WORKERS)
            .clamp(MIN_EVENT_WORKERS, MAX_EVENT_WORKERS)
    }

    fn funding_lane_profile(mode: FundingLaneMode) -> Option<GrpcSubscriptionProfile> {
        match mode {
            FundingLaneMode::Disabled => None,
            FundingLaneMode::PumpFiltered => Some(GrpcSubscriptionProfile::FundingLanePumpFiltered),
            FundingLaneMode::FullChain => Some(GrpcSubscriptionProfile::FundingLaneFullChain),
        }
    }

    fn funding_lane_source_label(mode: FundingLaneMode) -> Option<&'static str> {
        Self::funding_lane_profile(mode).map(GrpcSubscriptionProfile::source_label)
    }

    fn ensure_entry_cpi_scan_worker(self: &Arc<Self>) {
        let Some(parser) = self.parser.clone() else {
            return;
        };
        let Some(rx) = self.entry_cpi_scan_rx.lock().take() else {
            return;
        };

        let seer = Arc::clone(self);
        tokio::spawn(async move {
            seer.run_entry_cpi_scan_worker(parser, rx).await;
        });
    }

    async fn run_entry_cpi_scan_worker(
        self: Arc<Self>,
        parser: BinaryParser,
        mut rx: mpsc::Receiver<EntryCpiScanJob>,
    ) {
        info!(
            "Seer: entry CPI scan worker started queue_cap={}",
            ENTRY_CPI_SCAN_QUEUE_CAP
        );

        while let Some(job) = rx.recv().await {
            let slot = job.slot;
            let raw_len = job.raw.len();
            let parser = parser.clone();
            let raw = job.raw;
            let scan_result =
                tokio::task::spawn_blocking(move || parser.scan_entry_cpi_creates(&raw, slot))
                    .await;
            let hits = match scan_result {
                Ok(hits) => hits,
                Err(err) => {
                    ::metrics::increment_counter!(
                        "seer_entry_cpi_scan_skipped_total",
                        "reason" => "worker_join_error"
                    );
                    warn!(
                        slot,
                        raw_len,
                        error = %err,
                        "Seer: entry CPI scan worker failed"
                    );
                    continue;
                }
            };

            if hits.is_empty() {
                continue;
            }

            ::metrics::counter!("seer_entry_cpi_create_hits_total", hits.len() as u64);
            for (curve_b58, mint_b58, _creator) in hits {
                let (Ok(curve_pk), Ok(mint_pk)) = (
                    solana_sdk::pubkey::Pubkey::from_str(&curve_b58),
                    solana_sdk::pubkey::Pubkey::from_str(&mint_b58),
                ) else {
                    continue;
                };
                info!("ENTRY_CPI_CREATE curve={curve_b58} mint={mint_b58} slot={slot}");
                self.register_curve_mapping(curve_pk, mint_pk, "entry_cpi", true)
                    .await;
            }
        }
    }

    fn enqueue_entry_cpi_scan(&self, raw: Vec<u8>, slot: u64) {
        let Some(tx) = &self.entry_cpi_scan_tx else {
            return;
        };

        let raw_len = raw.len();
        match tx.try_send(EntryCpiScanJob { slot, raw }) {
            Ok(()) => {
                ::metrics::increment_counter!("seer_entry_cpi_scan_enqueued_total");
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                ::metrics::increment_counter!(
                    "seer_entry_cpi_scan_skipped_total",
                    "reason" => "queue_full"
                );
                debug!(
                    slot,
                    raw_len,
                    queue_cap = ENTRY_CPI_SCAN_QUEUE_CAP,
                    "Seer: skipping best-effort entry CPI scan because the worker queue is full"
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                ::metrics::increment_counter!(
                    "seer_entry_cpi_scan_skipped_total",
                    "reason" => "worker_closed"
                );
                debug!(
                    slot,
                    raw_len,
                    "Seer: skipping best-effort entry CPI scan because the worker is closed"
                );
            }
        }
    }

    fn is_dedicated_funding_lane_source(source_label: &str) -> bool {
        matches!(
            source_label,
            GRPC_FUNDING_LANE_PUMP_FILTERED_SOURCE_LABEL
                | GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL
        )
    }

    /// Create a new Seer instance with traditional mpsc channel (backward compatible)
    pub fn new(config: SeerConfig, candidate_sender: mpsc::Sender<CandidatePool>) -> Self {
        Self::new_internal(config, Some(candidate_sender), None, None)
    }

    /// Create a new Seer instance with IPC channel for Seer→Trigger communication
    pub fn new_with_ipc(config: SeerConfig, ipc_sender: IpcSender) -> Self {
        Self::new_internal(config, None, Some(ipc_sender), None)
    }

    /// Create a new Seer instance with IPC channel and ShadowLedger integration
    pub fn new_with_ipc_and_shadow_ledger(
        config: SeerConfig,
        ipc_sender: IpcSender,
        shadow_ledger: Arc<ShadowLedger>,
    ) -> Self {
        Self::new_internal(config, None, Some(ipc_sender), Some(shadow_ledger))
    }

    /// Create a new Seer instance with Shadow Ledger support
    pub fn new_with_shadow_ledger(
        config: SeerConfig,
        candidate_sender: mpsc::Sender<CandidatePool>,
        shadow_ledger: Arc<ShadowLedger>,
    ) -> Self {
        Self::new_internal(config, Some(candidate_sender), None, Some(shadow_ledger))
    }

    /// Internal constructor with all options
    fn new_internal(
        config: SeerConfig,
        candidate_sender: Option<mpsc::Sender<CandidatePool>>,
        ipc_sender: Option<IpcSender>,
        shadow_ledger: Option<Arc<ShadowLedger>>,
    ) -> Self {
        let metrics = Arc::new(SeerMetrics::new());

        // Initialize Paradox Sensor
        let (paradox_sensor, paradox_rx) = ParadoxSensor::new();
        let paradox_sensor = Arc::new(paradox_sensor);

        // Determine effective source mode
        let effective_mode = config.effective_source_mode();
        if !matches!(effective_mode, SeerSourceMode::GeyserGrpc)
            && !matches!(config.funding_lane_mode, FundingLaneMode::Disabled)
        {
            warn!(
                "Ignoring funding_lane_mode={} because effective_source_mode={:?} is not geyser_grpc",
                config.funding_lane_mode.as_str(),
                effective_mode
            );
        }

        // Initialize connection based on effective source mode
        let (
            ws_connection,
            grpc_connection,
            funding_grpc_connection,
            helius_adapter,
            pumpportal_connection,
            ws_fallback,
        ) = match effective_mode {
            SeerSourceMode::GeyserWebSocket => {
                let ws_conn = if let Some(ref ledger) = shadow_ledger {
                    WebSocketConnection::new_with_shadow_ledger(
                        config.geyser_endpoint.clone(),
                        config.rpc_endpoint.clone(),
                        Arc::clone(&metrics),
                        config.max_reconnect_attempts,
                        config.reconnect_delay_secs,
                        config.verbose,
                        Arc::clone(ledger),
                        config.commitment.clone(),
                    )
                    .with_paradox_sensor(Arc::clone(&paradox_sensor))
                } else {
                    WebSocketConnection::new(
                        config.geyser_endpoint.clone(),
                        config.rpc_endpoint.clone(),
                        Arc::clone(&metrics),
                        config.max_reconnect_attempts,
                        config.reconnect_delay_secs,
                        config.verbose,
                        config.commitment.clone(),
                    )
                    .with_paradox_sensor(Arc::clone(&paradox_sensor))
                };
                (Some(ws_conn), None, None, None, None, None)
            }
            SeerSourceMode::GeyserGrpc => {
                let build_grpc_connection =
                    |subscription_profile: GrpcSubscriptionProfile, manual_backfill_enabled| {
                        GrpcConnection::new_with_auth_header(
                            config.grpc_endpoint.clone(),
                            config.grpc_client_id.clone(),
                            config.grpc_auth_token.clone(),
                            config.grpc_auth_header.clone(),
                            Arc::clone(&metrics),
                            config.max_reconnect_attempts,
                            config.reconnect_delay_secs,
                            config.max_reconnect_delay_secs,
                            config.verbose,
                            config.commitment.clone(),
                            Some(config.rpc_endpoint.clone()),
                        )
                        .with_subscription_profile(subscription_profile)
                        .with_stall_timeout_secs(config.grpc_stall_timeout_secs)
                        .with_circuit_breaker_config(
                            config.grpc_max_stalls_before_open,
                            config.grpc_circuit_breaker_cooldown_ms,
                        )
                        .with_manual_backfill_enabled(manual_backfill_enabled)
                        .with_stream_config(
                            config.stream_mode.clone(),
                            config.watched_pools_ttl_ms,
                            config.watched_pools_cap,
                            config.watch_debounce_ms,
                        )
                        .with_paradox_sensor(Arc::clone(&paradox_sensor))
                    };
                let grpc_conn = build_grpc_connection(
                    GrpcSubscriptionProfile::PrimaryGlobal,
                    config.grpc_manual_backfill_enabled,
                );
                let funding_grpc_connection = Self::funding_lane_profile(config.funding_lane_mode)
                    .map(|profile| build_grpc_connection(profile, false));
                let ws_fallback = if config.grpc_commitment_fallback_to_websocket {
                    Some(
                        WebSocketConnection::new(
                            config.geyser_endpoint.clone(),
                            config.rpc_endpoint.clone(),
                            Arc::clone(&metrics),
                            config.max_reconnect_attempts,
                            config.reconnect_delay_secs,
                            config.verbose,
                            config.commitment.clone(),
                        )
                        .with_paradox_sensor(Arc::clone(&paradox_sensor)),
                    )
                } else {
                    None
                };
                (
                    None,
                    Some(grpc_conn),
                    funding_grpc_connection,
                    None,
                    None,
                    ws_fallback,
                )
            }
            SeerSourceMode::HeliusWebSocket => {
                // Use helius_endpoint if provided, otherwise fallback to geyser_endpoint
                let endpoint = config
                    .helius_endpoint
                    .clone()
                    .unwrap_or_else(|| config.geyser_endpoint.clone());

                let helius_adp = HeliusWebSocketAdapter::new(
                    endpoint,
                    config.rpc_endpoint.clone(),
                    Arc::clone(&metrics),
                    config.max_reconnect_attempts,
                    config.reconnect_delay_secs,
                );
                (None, None, None, Some(helius_adp), None, None)
            }
            SeerSourceMode::PumpPortalWs => {
                info!("Initializing PumpPortal WebSocket mode");
                let pumpportal_conn =
                    PumpPortalConnection::new(config.pumpportal.clone(), Arc::clone(&metrics))
                        .expect("Failed to initialize PumpPortal connection");
                (None, None, None, None, Some(pumpportal_conn), None)
            }
        };

        // Only create binary parser for Geyser modes (not for PumpPortal)
        // PumpPortal provides pre-parsed synthetic events, no binary parsing needed
        let parser = match effective_mode {
            SeerSourceMode::PumpPortalWs => {
                info!("🔀 Source Router: PumpPortal mode - binary parser DISABLED (synthetic events only)");
                None
            }
            _ => {
                let mode_name = match effective_mode {
                    SeerSourceMode::GeyserGrpc => "GeyserGrpc",
                    SeerSourceMode::GeyserWebSocket => "GeyserWebSocket",
                    SeerSourceMode::HeliusWebSocket => "HeliusWebSocket",
                    _ => "Unknown",
                };
                info!(
                    "🔀 Source Router: {} mode - binary parser ENABLED",
                    mode_name
                );
                if matches!(effective_mode, SeerSourceMode::GeyserGrpc) {
                    if let Some(grpc_connection) = grpc_connection.as_ref() {
                        Some(BinaryParser::with_account_registry_bcv2_hydration_and_ipc(
                            config.verbose,
                            grpc_connection.account_registry(),
                            Some(config.rpc_endpoint.clone()),
                            ipc_sender.clone(),
                        ))
                    } else {
                        Some(BinaryParser::new(config.verbose))
                    }
                } else {
                    Some(BinaryParser::new(config.verbose))
                }
            }
        };

        // Extract before config is moved into Self.
        let canonical_account_update_relay_enabled = config.canonical_account_update_relay_enabled;
        let (entry_cpi_scan_tx, entry_cpi_scan_rx) = if parser.is_some() {
            let (tx, rx) = mpsc::channel(ENTRY_CPI_SCAN_QUEUE_CAP);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        Self {
            config,
            ws_connection,
            grpc_connection,
            funding_grpc_connection,
            helius_adapter,
            pumpportal_connection,
            ws_fallback,
            parser,
            entry_cpi_scan_tx,
            entry_cpi_scan_rx: Mutex::new(entry_cpi_scan_rx),
            metrics,
            candidate_sender,
            ipc_sender,
            paradox_sensor: Some(paradox_sensor),
            paradox_rx: Some(paradox_rx),
            ultrafast_mode: AtomicBool::new(false),
            shadow_ledger,
            tracked_curves: Arc::new(RwLock::new(HashMap::new())),
            curve_to_mint: Arc::new(RwLock::new(HashMap::new())),
            mint_to_curve: Arc::new(RwLock::new(HashMap::new())),
            pending_curve_updates: Arc::new(RwLock::new(HashMap::new())),
            pending_trades: Arc::new(RwLock::new(HashMap::new())),
            unsupported_layout_last_log: Arc::new(RwLock::new(HashMap::new())),
            coverage: Arc::new(CoverageCounters::default()),
            coverage_last_log: Arc::new(Mutex::new(Instant::now())),
            health: None,
            wal: None,
            wal_disabled_due_to_enospc: AtomicBool::new(false),
            session_start_slot: AtomicU64::new(0),
            canonical_account_update_relay_enabled,
        }
    }

    /// Set the RuntimeHealth monitor for watchdog heartbeats.
    ///
    /// When set, the health will be forwarded to the gRPC connection
    /// (if present) for `mark_grpc_msg()` calls on every received message.
    pub fn set_health(&mut self, health: Arc<RuntimeHealth>) {
        if let Some(ref mut grpc) = self.grpc_connection {
            grpc.set_health(Arc::clone(&health));
        }
        if let Some(ref mut grpc) = self.funding_grpc_connection {
            grpc.set_health(Arc::clone(&health));
        }
        self.health = Some(health);
    }

    /// Attach a shared WAL handle for raw/parsed ingest durability.
    pub fn with_wal(mut self, wal: Arc<Wal>) -> Self {
        self.wal = Some(wal);
        self
    }

    /// Get the Paradox Sensor state receiver
    ///
    /// This allows external components (e.g., Trigger) to subscribe to
    /// real-time network telemetry analysis.
    pub fn paradox_state_receiver(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<paradox_sensor::ParadoxState>> {
        self.paradox_rx.clone()
    }

    pub fn set_authoritative_funding_stream_availability_sender(
        &self,
        tx: watch::Sender<bool>,
    ) -> bool {
        if !matches!(self.config.funding_lane_mode, FundingLaneMode::FullChain) {
            return false;
        }

        let Some(connection) = self.funding_grpc_connection.as_ref() else {
            return false;
        };

        connection.set_authoritative_funding_stream_availability_sender(tx);
        true
    }

    fn append_wal_record(&self, record: WalRecord, record_kind: &'static str) {
        self.append_wal_record_with_clock(record, record_kind, WalRecordClock::default());
    }

    fn append_wal_record_with_clock(
        &self,
        record: WalRecord,
        record_kind: &'static str,
        clock: WalRecordClock,
    ) {
        let Some(wal) = self.wal.as_ref() else {
            return;
        };
        if self.wal_disabled_due_to_enospc.load(Relaxed) {
            return;
        }

        if let Err(err) = wal.append_with_clock(&record, clock) {
            if is_no_space_error(&err) {
                if !self.wal_disabled_due_to_enospc.swap(true, Relaxed) {
                    error!(
                        record_kind,
                        error = %err,
                        "Seer: disabling WAL after ENOSPC; runtime will continue without further WAL appends"
                    );
                }
                return;
            }
            warn!(
                record_kind,
                error = %err,
                "Seer: failed to append WAL record"
            );
        }
    }

    fn append_raw_tx_to_wal(&self, event: &types::GeyserEvent) {
        let event_time = types::transaction_event_time(event);
        let compat_event_ts_ms = event.compat_event_ts_ms();
        let types::GeyserEvent::Transaction {
            slot,
            signature,
            mpcf_payload_bytes,
            ..
        } = event
        else {
            return;
        };

        let Some(raw_tx) = mpcf_payload_bytes
            .as_ref()
            .filter(|bytes| !bytes.is_empty())
        else {
            return;
        };

        self.append_wal_record_with_clock(
            WalRecord::RawTx {
                ts_ms: compat_event_ts_ms.unwrap_or_else(types::ingress_epoch_ms),
                slot: slot.unwrap_or_default(),
                signature: Some(signature.as_ref().to_vec()),
                raw_tx: raw_tx.clone(),
            },
            "raw_tx",
            WalRecordClock::new(event_time, compat_event_ts_ms),
        );
    }

    fn append_parsed_event_to_wal(
        &self,
        ts_ms: u64,
        event_time: ghost_core::EventTimeMetadata,
        slot: Option<u64>,
        pool_id: Option<Pubkey>,
        kind: WalParsedEventKind,
    ) {
        self.append_wal_record_with_clock(
            WalRecord::ParsedEvent {
                ts_ms,
                slot: slot.unwrap_or_default(),
                pool_id: pool_id.map(|pubkey| pubkey.to_bytes().to_vec()),
                kind,
            },
            "parsed_event",
            WalRecordClock::new(event_time, Some(ts_ms)),
        );
    }

    /// Run the Seer main loop
    ///
    /// This will connect to event source (via WebSocket, gRPC, or Helius) and process events until an error occurs or shutdown.
    pub async fn run(self: Arc<Self>) -> SeerResult<()> {
        info!("Starting Seer module");
        let effective_mode = self.config.effective_source_mode();
        let event_worker_concurrency = Self::event_worker_concurrency();
        let mut funding_lane_task: Option<tokio::task::JoinHandle<()>> = None;
        let mut primary_grpc_active = false;
        self.ensure_entry_cpi_scan_worker();
        info!("Effective source mode: {:?}", effective_mode);
        info!("Pump.fun enabled: {}", self.config.filter.enable_pumpfun);
        info!(
            "Stream mode: {:?} | tx_filter_strategy: {:?} | funding_lane_mode={} | grpc_stall_timeout_secs={} | watched_pools_ttl_ms={} | watched_pools_cap={} | watch_debounce_ms={} | event_worker_concurrency={}",
            self.config.stream_mode,
            self.config.tx_filter_strategy,
            self.config.funding_lane_mode.as_str(),
            self.config.grpc_stall_timeout_secs,
            self.config.watched_pools_ttl_ms,
            self.config.watched_pools_cap,
            self.config.watch_debounce_ms,
            event_worker_concurrency,
        );

        // Start Paradox Sensor background analysis loop
        if let Some(ref sensor) = self.paradox_sensor {
            let sensor_clone = Arc::clone(sensor);
            tokio::spawn(async move {
                info!("🔮 Paradox Sensor analysis loop started");
                ParadoxSensor::run_analysis_loop(sensor_clone).await;
            });
            info!("🔮 Paradox Sensor initialized and running");
        }

        // Connect using the appropriate adapter based on effective source mode
        let mut event_stream: std::pin::Pin<
            Box<dyn futures_util::Stream<Item = SeerResult<types::GeyserEvent>> + Send>,
        > = match effective_mode {
            SeerSourceMode::HeliusWebSocket => {
                info!("Connecting via Helius WebSocket (standard RPC)...");
                self.helius_adapter
                    .as_ref()
                    .ok_or_else(|| {
                        SeerError::ConfigError("Helius adapter not initialized".to_string())
                    })?
                    .connect()
                    .await?
            }
            SeerSourceMode::GeyserWebSocket => {
                info!("Connecting via Geyser WebSocket...");
                self.ws_connection
                    .as_ref()
                    .ok_or_else(|| {
                        SeerError::ConfigError("WebSocket connection not initialized".to_string())
                    })?
                    .connect_geyser()
                    .await?
            }
            SeerSourceMode::GeyserGrpc => {
                info!("Connecting via Yellowstone gRPC...");
                match self
                    .grpc_connection
                    .as_ref()
                    .ok_or_else(|| {
                        SeerError::ConfigError("gRPC connection not initialized".to_string())
                    })?
                    .connect_geyser()
                    .await
                {
                    Ok(stream) => {
                        primary_grpc_active = true;
                        stream
                    }
                    Err(e) => {
                        warn!(
                            "gRPC connection failed (commitment={}): {}",
                            self.config.commitment.as_str(),
                            e
                        );

                        if let Some(ws) = &self.ws_fallback {
                            warn!(
                                "Falling back to WebSocket/Geyser stream for early mempool signals"
                            );
                            ws.connect_geyser().await?
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
            SeerSourceMode::PumpPortalWs => {
                info!("Connecting via PumpPortal WebSocket...");
                self.pumpportal_connection
                    .as_ref()
                    .ok_or_else(|| {
                        SeerError::ConfigError("PumpPortal connection not initialized".to_string())
                    })?
                    .connect()
                    .await?
            }
        };

        if matches!(effective_mode, SeerSourceMode::GeyserGrpc) && primary_grpc_active {
            if let Some(funding_connection) = self.funding_grpc_connection.as_ref() {
                let funding_lane_mode = self.config.funding_lane_mode;
                let funding_source_label = Self::funding_lane_source_label(funding_lane_mode)
                    .unwrap_or(GRPC_GLOBAL_STREAM_SOURCE_LABEL);
                info!(
                    "Connecting dedicated funding lane via Yellowstone gRPC... mode={} source_label={}",
                    funding_lane_mode.as_str(),
                    funding_source_label
                );
                match funding_connection.connect_geyser().await {
                    Ok(funding_stream) => {
                        let seer = Arc::clone(&self);
                        funding_lane_task = Some(tokio::spawn(async move {
                            seer.run_funding_lane_loop(funding_lane_mode, funding_stream)
                                .await;
                        }));
                    }
                    Err(error) => {
                        error!(
                            "Dedicated funding lane connect failed mode={} source_label={}: {}",
                            funding_lane_mode.as_str(),
                            funding_source_label,
                            error
                        );
                    }
                }
            }
        } else if matches!(effective_mode, SeerSourceMode::GeyserGrpc)
            && self.funding_grpc_connection.is_some()
        {
            warn!(
                "Skipping dedicated funding lane because the primary ingest path is not on Yellowstone gRPC"
            );
        }

        info!("Seer is now listening for InitializePool events...");

        let mut workers = JoinSet::new();
        while let Some(event_result) = event_stream.next().await {
            while workers.len() >= event_worker_concurrency {
                if let Some(joined) = workers.join_next().await {
                    if let Err(join_err) = joined {
                        error!("Seer worker task failed: {}", join_err);
                    }
                }
            }

            match event_result {
                Ok(event) => {
                    let seer = Arc::clone(&self);
                    workers.spawn(async move {
                        if let Err(e) = seer.process_event(event).await {
                            error!("Error processing event: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error receiving event: {}", e);
                    // Stream-level decode/provider failures must not kill Seer.
                    // The underlying connection handles reconnects independently.
                    continue;
                }
            }
        }

        while let Some(joined) = workers.join_next().await {
            if let Err(join_err) = joined {
                error!("Seer worker task failed after stream end: {}", join_err);
            }
        }

        if let Some(funding_lane_task) = funding_lane_task {
            funding_lane_task.abort();
            if let Err(join_err) = funding_lane_task.await {
                if !join_err.is_cancelled() {
                    error!("Funding lane task join failed: {}", join_err);
                }
            }
        }

        warn!("Event stream ended");
        Ok(())
    }

    async fn run_funding_lane_loop(
        self: Arc<Self>,
        mode: FundingLaneMode,
        mut event_stream: EventStream,
    ) {
        let worker_concurrency = Self::event_worker_concurrency();
        let source_label = Self::funding_lane_source_label(mode).unwrap_or("unknown");
        info!(
            "Dedicated funding lane started mode={} source_label={} worker_concurrency={}",
            mode.as_str(),
            source_label,
            worker_concurrency
        );

        let mut workers = JoinSet::new();
        while let Some(event_result) = event_stream.next().await {
            while workers.len() >= worker_concurrency {
                if let Some(joined) = workers.join_next().await {
                    if let Err(join_err) = joined {
                        error!("Funding lane worker task failed: {}", join_err);
                    }
                }
            }

            match event_result {
                Ok(event) => {
                    let seer = Arc::clone(&self);
                    workers.spawn(async move {
                        if let Err(error) = seer.process_funding_lane_event(event).await {
                            error!("Error processing funding-lane event: {}", error);
                        }
                    });
                }
                Err(error) => {
                    error!("Error receiving funding-lane event: {}", error);
                }
            }
        }

        while let Some(joined) = workers.join_next().await {
            if let Err(join_err) = joined {
                error!(
                    "Funding lane worker task failed after stream end: {}",
                    join_err
                );
            }
        }

        warn!(
            "Dedicated funding lane stream ended mode={} source_label={}",
            mode.as_str(),
            source_label
        );
    }

    /// Rate-limit interval for UNSUPPORTED_LAYOUT debug logs (60 seconds per pubkey).
    const UNSUPPORTED_LAYOUT_LOG_INTERVAL_SECS: u64 = 60;

    fn set_curve_mapping(
        &self,
        curve: Pubkey,
        mint: Pubkey,
        source: &'static str,
        authoritative: bool,
    ) {
        Self::set_curve_mapping_in_state(
            &self.curve_to_mint,
            &self.mint_to_curve,
            &self.tracked_curves,
            curve,
            mint,
            source,
            authoritative,
        );
    }

    fn set_curve_mapping_in_state(
        curve_to_mint: &Arc<RwLock<HashMap<[u8; 32], [u8; 32]>>>,
        mint_to_curve: &Arc<RwLock<HashMap<[u8; 32], [u8; 32]>>>,
        tracked_curves: &Arc<RwLock<HashMap<Pubkey, Pubkey>>>,
        curve: Pubkey,
        mint: Pubkey,
        source: &'static str,
        authoritative: bool,
    ) {
        let curve_bytes = curve.to_bytes();
        let mint_bytes = mint.to_bytes();

        let mut changed = false;
        let mut conflict_existing: Option<[u8; 32]> = None;
        {
            let mut map = curve_to_mint.write();
            match map.get(&curve_bytes).copied() {
                Some(existing) if existing != mint_bytes => {
                    conflict_existing = Some(existing);
                    if authoritative {
                        map.insert(curve_bytes, mint_bytes);
                        changed = true;
                    }
                }
                Some(_) => {}
                None => {
                    map.insert(curve_bytes, mint_bytes);
                    changed = true;
                }
            }
        }

        {
            let mut reverse = mint_to_curve.write();
            if authoritative {
                reverse.insert(mint_bytes, curve_bytes);
                if let Some(existing_mint) = conflict_existing {
                    if reverse.get(&existing_mint) == Some(&curve_bytes) {
                        reverse.remove(&existing_mint);
                    }
                }
            } else {
                reverse.entry(mint_bytes).or_insert(curve_bytes);
            }
        }

        if authoritative || changed {
            tracked_curves.write().insert(curve, mint);
        }

        if changed {
            info!(
                "CURVE_MAP_SET source={} curve={} mint={}",
                source, curve, mint
            );
        } else if let Some(existing) = conflict_existing {
            if authoritative {
                debug!(
                    "CURVE_MAP_CONFLICT source={} curve={} existing_mint={} new_mint={} action=NOOP",
                    source,
                    curve,
                    Pubkey::new_from_array(existing),
                    mint
                );
            }
        }
    }

    fn sync_curve_mapping_to_parser(&self, curve: Pubkey, mint: Pubkey) {
        Self::sync_curve_mapping_to_parser_from_state(self.parser.as_ref(), curve, mint);
    }

    fn observation_alias_pool_for_mint(&self, mint: Pubkey) -> Option<Pubkey> {
        if mint == Pubkey::default() || mint == *wsol_mint_pubkey() {
            return None;
        }

        self.mint_to_curve
            .read()
            .get(&mint.to_bytes())
            .copied()
            .map(Pubkey::new_from_array)
    }

    fn seed_pumpswap_continuity(
        &self,
        candidate: &CandidatePool,
        observation_pool: Pubkey,
        detection_received_at: std::time::SystemTime,
    ) {
        self.set_curve_mapping(
            candidate.bonding_curve,
            candidate.base_mint,
            "pumpswap_continuity",
            false,
        );
        self.sync_curve_mapping_to_parser(candidate.bonding_curve, candidate.base_mint);
        ::metrics::increment_counter!("seer_pumpswap_continuity_seed_total");

        info!(
            pumpswap_pool = %candidate.pool_amm_id,
            base_mint = %candidate.base_mint,
            observation_pool = %observation_pool,
            "Suppressing PumpSwap create as a new candidate and preserving continuity on the existing observation alias"
        );

        let watch_started_at_ms = detection_received_at
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.register_watch_from_mapping(
            candidate.bonding_curve,
            candidate.base_mint,
            AmmProgram::PumpSwap,
            watch_started_at_ms,
            "pumpswap_continuity",
        );
    }

    fn sync_curve_mapping_to_parser_from_state(
        parser: Option<&BinaryParser>,
        curve: Pubkey,
        mint: Pubkey,
    ) {
        if let Some(parser) = parser {
            parser.set_curve_mapping(&curve.to_string(), &mint.to_string());
        }
    }

    fn register_watch_from_mapping(
        &self,
        curve: Pubkey,
        mint: Pubkey,
        amm_program: AmmProgram,
        watch_started_at_ms: u64,
        source: &'static str,
    ) {
        if let Some(grpc_connection) = &self.grpc_connection {
            grpc_connection.add_watched_mint(mint);
            grpc_connection.watch_pool(curve, amm_program, watch_started_at_ms);
            let registry = grpc_connection.account_registry();
            let lanes = registry.snapshot_by_lane();
            let transport = grpc_connection.transport_stats();
            coverage_audit().record_watch_registration(
                &curve.to_string(),
                CoverageAuditWatchRegistration {
                    wall_ms: watch_started_at_ms,
                    source: source.to_string(),
                    registry_version: registry.version(),
                    exact_curve_accounts: lanes.curve_accounts.len() as u64,
                    exact_pool_accounts: lanes.pool_accounts.len() as u64,
                    watched_mints: lanes.mint_accounts.len() as u64,
                    transport_resubs_sent: transport.resubs_sent.load(Relaxed),
                    transport_msgs_spilled: transport.msgs_spilled.load(Relaxed),
                    transport_overflow_dropped: transport.msgs_overflow_dropped.load(Relaxed),
                    transport_slot_gaps: transport.slot_gaps_total.load(Relaxed),
                    transport_last_msg_gap_ms: transport.ms_since_last_msg(),
                },
            );
        }
    }

    fn authoritative_mapping_watch_program(source: &str) -> Option<AmmProgram> {
        match source {
            "create" | "entry_cpi" => Some(AmmProgram::PumpFun),
            "pumpswap_continuity" => Some(AmmProgram::PumpSwap),
            _ => None,
        }
    }

    async fn register_curve_mapping_from_state(
        curve_to_mint: &Arc<RwLock<HashMap<[u8; 32], [u8; 32]>>>,
        mint_to_curve: &Arc<RwLock<HashMap<[u8; 32], [u8; 32]>>>,
        tracked_curves: &Arc<RwLock<HashMap<Pubkey, Pubkey>>>,
        parser: Option<&BinaryParser>,
        pending_curve_updates: &Arc<RwLock<HashMap<[u8; 32], PendingCurveUpdate>>>,
        pending_trades: &Arc<RwLock<HashMap<PendingTradeKey, VecDeque<PendingTrade>>>>,
        ipc_sender: Option<IpcSender>,
        canonical_account_update_relay_enabled: bool,
        metrics: &Arc<SeerMetrics>,
        coverage: &Arc<CoverageCounters>,
        curve: Pubkey,
        mint: Pubkey,
        source: &'static str,
        authoritative: bool,
    ) {
        Self::set_curve_mapping_in_state(
            curve_to_mint,
            mint_to_curve,
            tracked_curves,
            curve,
            mint,
            source,
            authoritative,
        );
        Self::sync_curve_mapping_to_parser_from_state(parser, curve, mint);
        Self::replay_pending_curve_update_from_state(
            pending_curve_updates,
            ipc_sender.clone(),
            canonical_account_update_relay_enabled,
            curve,
            mint,
        )
        .await;
        Self::replay_pending_trades_from_state(
            pending_trades,
            ipc_sender,
            metrics,
            coverage,
            curve,
            mint,
        )
        .await;
    }

    async fn register_curve_mapping(
        &self,
        curve: Pubkey,
        mint: Pubkey,
        source: &'static str,
        authoritative: bool,
    ) {
        self.set_curve_mapping(curve, mint, source, authoritative);
        self.sync_curve_mapping_to_parser(curve, mint);
        if authoritative {
            if let Some(amm_program) = Self::authoritative_mapping_watch_program(source) {
                self.register_watch_from_mapping(
                    curve,
                    mint,
                    amm_program,
                    types::arrival_time_ms(),
                    source,
                );
            }
        }
        // [RC-6] Drain any AccountUpdate buffered before this mapping was known.
        // `pending_curve_updates` is the sole owner of AccountUpdate replay.
        // `replay_pending_curve_update` drains it exactly once — HashMap::remove
        // guarantees that concurrent or repeated calls are no-ops.
        self.replay_pending_curve_update(curve, mint).await;
        self.replay_pending_trades(curve, mint).await;
    }

    fn buffer_pending_trade(
        &self,
        trade: types::TradeEvent,
        source_label: &str,
        is_coverage_source: bool,
    ) {
        let now = Instant::now();
        // Choose the buffer key and reason based on what is known about this trade:
        // - Known curve  → ByCurve; reason depends on whether mint is also known.
        //   When mint is also known (but mapping is missing/conflicting), the only
        //   call sites that reach here are `should_forward_trade` line 1769 (mapping
        //   conflict where can_forward_with_trade_mint is false) — so MappingConflict.
        //   When mint is unknown, reason is CurveMappingMissing.
        // - Default curve but known mint → ByMint / MissingPoolFromMint so that
        //   replay fires correctly when register_curve_mapping(curve, mint) is called.
        // - Both unknown → BySignature / MissingMintAndPool; such trades have no
        //   active resolve path and will expire after TTL.
        let (key, reason) = if trade.pool_amm_id != Pubkey::default() {
            let reason = if trade.mint == Pubkey::default() {
                PendingTradeReason::CurveMappingMissing
            } else {
                PendingTradeReason::MappingConflict
            };
            (
                PendingTradeKey::ByCurve(trade.pool_amm_id.to_bytes()),
                reason,
            )
        } else if trade.mint != Pubkey::default() && trade.mint != *wsol_mint_pubkey() {
            (
                PendingTradeKey::ByMint(trade.mint.to_bytes()),
                PendingTradeReason::MissingPoolFromMint,
            )
        } else {
            (
                PendingTradeKey::BySignature(trade.signature.to_string()),
                PendingTradeReason::MissingMintAndPool,
            )
        };
        let mut pending = self.pending_trades.write();
        let alternate_key = if trade.pool_amm_id != Pubkey::default()
            && trade.mint != Pubkey::default()
            && trade.mint != *wsol_mint_pubkey()
        {
            let by_mint = PendingTradeKey::ByMint(trade.mint.to_bytes());
            (by_mint != key).then_some(by_mint)
        } else {
            None
        };
        let already_buffered = pending
            .get(&key)
            .into_iter()
            .chain(alternate_key.as_ref().and_then(|alt| pending.get(alt)))
            .flat_map(|queue| queue.iter())
            .any(|queued| Self::same_trade_identity(&queued.trade, &trade));
        if already_buffered {
            self.record_trade_outcome(TradeOutcome::DedupDropped);
            debug!(
                "TX_DEDUP sig={} pool={} mint={} source={} outcome={}",
                trade.signature,
                trade.pool_amm_id,
                trade.mint,
                source_label,
                TradeOutcome::DedupDropped.as_str()
            );
            return;
        }

        let queue = pending.entry(key).or_default();

        while matches!(queue.front(), Some(oldest) if now.duration_since(oldest.queued_at) > PENDING_TRADE_TTL)
        {
            if let Some(expired) = queue.pop_front() {
                Self::record_pending_trade_expired(
                    &self.metrics,
                    &self.coverage,
                    &expired,
                    "buffer_enqueue",
                );
            }
        }

        if queue.len() >= PENDING_TRADES_PER_CURVE_MAX {
            queue.pop_front();
            ::metrics::increment_counter!("seer_pending_trade_evicted_total");
        }

        let outcome = reason.buffered_outcome();
        info!(
            "TX_BUFFERED_INACTIVE sig={} pool={} mint={} source={} outcome={} reason={}",
            trade.signature,
            trade.pool_amm_id,
            trade.mint,
            source_label,
            outcome.as_str(),
            reason.as_str()
        );
        if is_coverage_source {
            let pool_id = trade.pool_amm_id.to_string();
            let signature = trade.signature.to_string();
            coverage_audit().record_mapping_missing_with_reason(
                &pool_id,
                &signature,
                Some(reason.as_str()),
            );
        }
        queue.push_back(PendingTrade {
            trade,
            source_label: source_label.to_string(),
            is_coverage_source,
            queued_at: now,
            reason,
        });
        Self::record_trade_outcome_with_metrics(&self.metrics, outcome);
        self.coverage
            .pending_mapping_buffered_total
            .fetch_add(1, Relaxed);
        pipeline_coverage().increment(PipelineCoverageStage::PendingMappingBuffered, 1);
        ::metrics::increment_counter!("seer_pending_trade_buffered_total", "reason" => reason.as_str());
    }

    fn take_pending_trades_from_store(
        pending_trades: &Arc<RwLock<HashMap<PendingTradeKey, VecDeque<PendingTrade>>>>,
        metrics: &Arc<SeerMetrics>,
        coverage: &Arc<CoverageCounters>,
        curve: Pubkey,
        mint: Pubkey,
    ) -> Vec<PendingTrade> {
        let now = Instant::now();
        let curve_key = PendingTradeKey::ByCurve(curve.to_bytes());
        let mint_key = PendingTradeKey::ByMint(mint.to_bytes());

        // Drain both the ByCurve bucket and the ByMint bucket (the latter
        // collects trades that arrived before the curve pubkey was known).
        let mut queues: Vec<VecDeque<PendingTrade>> = {
            let mut pending = pending_trades.write();
            let mut qs = Vec::with_capacity(2);
            if let Some(q) = pending.remove(&curve_key) {
                qs.push(q);
            }
            // Only drain the mint bucket when it is a distinct key (avoids double-drain
            // when curve == mint, which should not happen in practice but is safe to guard).
            // Also skip WSOL mints — they are never used as buffer keys.
            if mint != Pubkey::default() && mint != *wsol_mint_pubkey() && mint_key != curve_key {
                if let Some(q) = pending.remove(&mint_key) {
                    qs.push(q);
                }
            }
            qs
        };

        let mut ready = Vec::new();
        for queue in queues.iter_mut() {
            while let Some(mut pending) = queue.pop_front() {
                if now.duration_since(pending.queued_at) > PENDING_TRADE_TTL {
                    Self::record_pending_trade_expired(metrics, coverage, &pending, "replay_drain");
                    continue;
                }
                if mint != Pubkey::default() && pending.trade.mint != mint {
                    pending.trade.mint = mint;
                }
                if pending.trade.pool_amm_id == Pubkey::default() {
                    pending.trade.pool_amm_id = curve;
                }
                ready.push(pending);
            }
        }
        ready
    }

    async fn replay_pending_trades_from_state(
        pending_trades: &Arc<RwLock<HashMap<PendingTradeKey, VecDeque<PendingTrade>>>>,
        ipc_sender: Option<IpcSender>,
        metrics: &Arc<SeerMetrics>,
        coverage: &Arc<CoverageCounters>,
        curve: Pubkey,
        mint: Pubkey,
    ) {
        let pending =
            Self::take_pending_trades_from_store(pending_trades, metrics, coverage, curve, mint);
        if pending.is_empty() {
            return;
        }

        // Fire all replay IPC sends concurrently (was: sequential for+await).
        // Sequential replay held the worker for N*IPC_latency before freeing the
        // slot, starving subsequent CreatePool events of worker capacity.
        let Some(ipc) = ipc_sender else {
            return;
        };

        struct ReplayJob {
            sig: Signature,
            pool_id_str: String,
            source_label: String,
            is_coverage: bool,
            is_rpc_fallback: bool,
            reason_label: String,
        }

        let futs: Vec<_> = pending
            .into_iter()
            .map(|pt| {
                let side = if pt.trade.is_buy { "buy" } else { "sell" };
                let lamports = if pt.trade.is_buy {
                    pt.trade.max_sol_cost
                } else {
                    pt.trade.min_sol_output
                };
                let volume_sol = lamports as f64 / 1_000_000_000.0;
                info!(
                    "Emitting PoolTransaction sig={} pool={} side={} volume={} source={} replayed=true outcome={} reason={}",
                    pt.trade.signature,
                    pt.trade.pool_amm_id,
                    side,
                    volume_sol,
                    pt.source_label,
                    TradeOutcome::ForwardedReplay.as_str(),
                    pt.reason.as_str()
                );
                let job = ReplayJob {
                    sig: pt.trade.signature,
                    pool_id_str: pt.trade.pool_amm_id.to_string(),
                    source_label: pt.source_label.clone(),
                    is_coverage: pt.is_coverage_source,
                    is_rpc_fallback: pt.source_label == "grpc_backfill",
                    reason_label: pt.reason.as_str().to_string(),
                };
                let ipc = ipc.clone();
                async move {
                    let result = ipc
                        .send_trade(pt.trade, ipc::EventPriority::Normal)
                        .await;
                    (result, job)
                }
            })
            .collect();

        let send_results = futures_util::future::join_all(futs).await;

        let mut emitted_coverage_signatures = HashSet::new();
        let mut emitted_fallback_signatures = HashSet::new();
        for (result, job) in send_results {
            match result {
                Ok(()) => {
                    if job.is_coverage {
                        let signature = job.sig.to_string();
                        coverage_audit().record_seer_emitted(
                            &job.pool_id_str,
                            &signature,
                            &job.source_label,
                        );
                    }
                    Self::record_trade_outcome_with_metrics(metrics, TradeOutcome::ForwardedReplay);
                    if job.is_rpc_fallback {
                        coverage
                            .rpc_fallback_events_forwarded_total
                            .fetch_add(1, Relaxed);
                    }
                    let is_new_coverage_sig = if job.is_coverage {
                        emitted_coverage_signatures.insert(job.sig)
                    } else {
                        false
                    };
                    let is_new_fallback_sig = if job.is_rpc_fallback {
                        emitted_fallback_signatures.insert(job.sig)
                    } else {
                        false
                    };
                    if job.is_coverage {
                        coverage
                            .trade_events_forwarded_replay_total
                            .fetch_add(1, Relaxed);
                        if is_new_coverage_sig {
                            coverage
                                .trade_signatures_forwarded_replay_total
                                .fetch_add(1, Relaxed);
                        }
                        pipeline_coverage()
                            .increment(PipelineCoverageStage::PendingMappingReplayed, 1);
                    }
                    if job.is_rpc_fallback && is_new_fallback_sig {
                        coverage
                            .rpc_fallback_signatures_forwarded_total
                            .fetch_add(1, Relaxed);
                    }
                    ::metrics::increment_counter!("seer_pending_trade_replayed_total", "reason" => job.reason_label);
                }
                Err(e) => {
                    Self::record_trade_outcome_with_metrics(metrics, TradeOutcome::IpcSendFailed);
                    warn!(
                        "Failed to send replayed trade via IPC: {} outcome={} reason={}",
                        e,
                        TradeOutcome::IpcSendFailed.as_str(),
                        job.reason_label
                    );
                }
            }
        }

        if !emitted_coverage_signatures.is_empty() {
            pipeline_coverage().increment(
                PipelineCoverageStage::SeerForwarded,
                emitted_coverage_signatures.len() as u64,
            );
        }
    }

    async fn replay_pending_trades(&self, curve: Pubkey, mint: Pubkey) {
        Self::replay_pending_trades_from_state(
            &self.pending_trades,
            self.ipc_sender.clone(),
            &self.metrics,
            &self.coverage,
            curve,
            mint,
        )
        .await;
    }

    /// Drain and replay any buffered AccountUpdate for `curve` now that `mint` is known.
    ///
    /// Called from `register_curve_mapping()` so buffered AccountUpdate payloads can be
    /// forwarded immediately into the canonical account-update path keyed by `mint`
    /// instead of waiting for the slower async mapping resolution loop.
    ///
    /// [RC-6] This is safe to call concurrently with the RPC-resolve drain because both
    /// paths use `HashMap::remove()` — only one will obtain the value, the other gets None.
    async fn replay_pending_curve_update(&self, curve: Pubkey, mint: Pubkey) {
        Self::replay_pending_curve_update_from_state(
            &self.pending_curve_updates,
            self.ipc_sender.clone(),
            self.canonical_account_update_relay_enabled,
            curve,
            mint,
        )
        .await;
    }

    async fn replay_pending_curve_update_from_state(
        pending_curve_updates: &Arc<RwLock<HashMap<[u8; 32], PendingCurveUpdate>>>,
        ipc_sender: Option<IpcSender>,
        canonical_account_update_relay_enabled: bool,
        curve: Pubkey,
        mint: Pubkey,
    ) {
        let pending = pending_curve_updates.write().remove(&curve.to_bytes());
        ::metrics::gauge!(
            "seer.account_updates.pending_curve_depth",
            pending_curve_updates.read().len() as f64
        );

        let Some(pending) = pending else { return };

        if !canonical_account_update_relay_enabled {
            ::metrics::increment_counter!(
                "seer.account_updates.pending_curve_replay_skipped_total",
                "reason" => "account_updates_disabled"
            );
            return;
        }

        let Some(ipc) = ipc_sender else { return };

        for replay in pending.replay_snapshots() {
            ::metrics::increment_counter!("seer_pending_curve_replayed_on_mapping_total");
            ::metrics::increment_counter!("seer.account_updates.pending_curve_replay_total");
            let dwell_ms = replay.queued_at.elapsed().as_secs_f64() * 1000.0;
            ::metrics::histogram!(
                "seer.account_updates.pending_curve_replay_dwell_ms",
                dwell_ms
            );
            coverage_audit().record_seer_account_update_pending_replay(
                &curve.to_string(),
                Some(dwell_ms.max(0.0).round() as u64),
                false,
                false,
            );

            if let Ok(update_payload) = decode_canonical_account_update(replay.owner, &replay.data)
            {
                let semantic = normalize_account_update_semantics(
                    "grpc_global_stream",
                    replay.event_time,
                    true,
                );
                if ipc
                    .send_account_update(
                        semantic,
                        replay.event_time,
                        mint,
                        curve,
                        ghost_core::CurveFinality::Provisional,
                        update_payload.sol_reserves,
                        update_payload.token_reserves,
                        update_payload.complete,
                        replay.slot,
                        replay.write_version,
                        AccountUpdateReplayOrigin::PendingReplay,
                        Some(dwell_ms.max(0.0).round() as u64),
                    )
                    .await
                    .is_ok()
                {
                    record_event_semantic_metric(semantic);
                    ::metrics::increment_counter!("seer.account_updates.emitted");
                    ::metrics::increment_counter!(
                        "seer.account_updates.emitted_total",
                        "origin" => "pending_replay"
                    );
                    ::metrics::histogram!(
                        "seer.account_updates.latency_us",
                        replay.queued_at.elapsed().as_micros() as f64
                    );
                    ::metrics::histogram!(
                        "seer.account_updates.end_to_end_latency_ms",
                        dwell_ms,
                        "origin" => "pending_replay"
                    );
                    ::metrics::gauge!(
                        "seer.account_updates.queue_depth",
                        ipc.current_queue_length() as f64
                    );
                } else {
                    ::metrics::increment_counter!(
                        "seer.account_updates.pending_curve_replay_send_failed_total"
                    );
                    coverage_audit().record_seer_account_update_pending_replay(
                        &curve.to_string(),
                        Some(dwell_ms.max(0.0).round() as u64),
                        true,
                        false,
                    );
                }
            } else {
                ::metrics::increment_counter!(
                    "seer.account_updates.pending_curve_parse_failed_total"
                );
                coverage_audit().record_seer_account_update_pending_replay(
                    &curve.to_string(),
                    Some(dwell_ms.max(0.0).round() as u64),
                    false,
                    true,
                );
                warn!(
                    "CURVE_REPLAY_PARSE_FAIL curve={} slot={} source=on_mapping",
                    curve, replay.slot
                );
            }
        }
    }

    fn lookup_curve_mint(&self, curve: Pubkey) -> Option<Pubkey> {
        if let Some(bytes) = self.curve_to_mint.read().get(&curve.to_bytes()).copied() {
            return Some(Pubkey::new_from_array(bytes));
        }

        self.tracked_curves.read().get(&curve).copied()
    }

    fn queue_pending_curve_update(
        &self,
        curve: Pubkey,
        slot: u64,
        event_time: ghost_core::EventTimeMetadata,
        write_version: Option<u64>,
        owner: Pubkey,
        data: &[u8],
    ) -> PendingCurveUpdateStoreOutcome {
        let key = curve.to_bytes();
        let mut pending = self.pending_curve_updates.write();
        let queued_at = Instant::now();
        let snapshot = PendingCurveUpdateSnapshot::new(
            slot,
            event_time,
            write_version,
            owner,
            data,
            queued_at,
        );
        let outcome = match pending.get_mut(&key) {
            Some(existing) => existing.store(snapshot),
            None => {
                pending.insert(key, PendingCurveUpdate::new(snapshot));
                PendingCurveUpdateStoreOutcome::Inserted
            }
        };
        ::metrics::gauge!(
            "seer.account_updates.pending_curve_depth",
            pending.len() as f64
        );
        ::metrics::increment_counter!("seer.account_updates.pending_curve_buffered_total");
        ::metrics::increment_counter!(
            "seer.account_updates.pending_curve_store_total",
            "outcome" => match outcome {
                PendingCurveUpdateStoreOutcome::Inserted => "inserted",
                PendingCurveUpdateStoreOutcome::ReplacedNewer => "replaced_newer",
                PendingCurveUpdateStoreOutcome::IgnoredOlder => "ignored_older",
            }
        );
        if matches!(outcome, PendingCurveUpdateStoreOutcome::ReplacedNewer) {
            ::metrics::increment_counter!("seer.account_updates.pending_curve_overwrite_total");
        }
        outcome
    }

    /// Look up route-aware BCV2 context without mutating canonical AccountUpdate state.
    fn bcv2_context_for_account_update(&self, pubkey: Pubkey) -> Option<Bcv2AccountContext> {
        self.grpc_connection
            .as_ref()
            .and_then(|grpc| grpc.account_registry().bcv2_context(&pubkey.to_string()))
    }

    async fn emit_bcv2_account_update_evidence(
        &self,
        context: Bcv2AccountContext,
        slot: u64,
        write_version: Option<u64>,
        owner: Pubkey,
        data_len: usize,
    ) {
        let Some(ipc) = &self.ipc_sender else {
            return;
        };
        let now_ms = types::ingress_epoch_ms();
        let evidence = ExecutionAccountEvidence {
            role: ExecutionAccountRole::BondingCurveV2,
            account_pubkey: context.account_pubkey,
            base_mint: context.base_mint,
            pool_id: context.pool_id,
            canonical_bonding_curve: context.canonical_bonding_curve,
            source: ExecutionAccountEvidenceSource::YellowstoneAccountUpdate,
            status: ExecutionAccountEvidenceStatus::AccountUpdateReceived,
            slot: Some(slot),
            context_slot: None,
            write_version,
            owner: Some(owner),
            data_len: Some(data_len as u64),
            tx_signature: context.tx_signature,
            observed_instruction_index: context.observed_instruction_index,
            observed_account_position: context.observed_account_position,
            provenance_status: context.provenance_status,
            detected_at_ms: now_ms,
            received_at_ms: now_ms,
            evidence_ready: true,
            reason: None,
        };

        match ipc
            .send_execution_account_evidence(evidence, EventPriority::High)
            .await
        {
            Ok(()) => {
                ::metrics::increment_counter!(
                    "seer.execution_account_evidence.emitted_total",
                    "source" => "yellowstone_account_update",
                    "status" => "account_update_received"
                );
            }
            Err(err) => {
                warn!(
                    account_pubkey = %context.account_pubkey,
                    error = %err,
                    "BCV2_ACCOUNT_UPDATE_EVIDENCE_SEND_FAILED"
                );
            }
        }
    }

    /// Handle AccountUpdate events and forward canonical state if the pubkey is tracked.
    ///
    /// Curve account data does not include mint, so mint resolution is delegated
    /// to curve->mint registry populated from create/trade hooks or RPC resolve.
    ///
    /// In explicit degraded/test mode (`canonical_account_update_relay_enabled == false`) this
    /// method returns immediately without forwarding the canonical update.
    async fn handle_account_update(&self, event: &types::GeyserEvent) -> SeerResult<bool> {
        // Legacy degraded/test compatibility: downstream AccountUpdate path is
        // disabled, so skip without side effects.
        // WAŻNE: zwracamy Ok(true) TYLKO dla eventów AccountUpdate, żeby inne typy
        // (Transaction, EntryAnchor) nie były pomijane przez wywołującego w process_event.
        if !self.canonical_account_update_relay_enabled {
            return Ok(matches!(event, types::GeyserEvent::AccountUpdate { .. }));
        }

        let recv_started_at = Instant::now();
        let (slot, event_time, write_version, pubkey, data, owner) = match event {
            types::GeyserEvent::AccountUpdate {
                slot,
                event_time,
                write_version,
                pubkey,
                data,
                owner,
            } => (*slot, *event_time, *write_version, *pubkey, data, *owner),
            _ => return Ok(false),
        };
        ::metrics::increment_counter!("seer.account_updates.received_total");

        if let Some(context) = self.bcv2_context_for_account_update(pubkey) {
            self.emit_bcv2_account_update_evidence(context, slot, write_version, owner, data.len())
                .await;
        }

        let update_payload = match decode_canonical_account_update(owner, data) {
            Ok(payload) => payload,
            Err(err) => {
                ::metrics::increment_counter!("seer.account_updates.parse_failed_total");
                self.log_unsupported_layout_rate_limited(pubkey, data, &err);
                return Ok(true);
            }
        };

        let wal_ingress_wall_ts_ms = types::ingress_epoch_ms();
        self.append_parsed_event_to_wal(
            wal_ingress_wall_ts_ms,
            event_time.with_missing_from(ghost_core::EventTimeMetadata::new(
                None,
                Some(wal_ingress_wall_ts_ms),
                None,
            )),
            Some(slot),
            Some(pubkey),
            WalParsedEventKind::AccountUpdate,
        );

        let base_mint = if let Some(token_mint) = update_payload.token_mint() {
            if self.lookup_curve_mint(pubkey) != Some(token_mint) {
                let preserve_observation_alias = owner == AmmProgram::PumpSwap.program_id()
                    && self
                        .observation_alias_pool_for_mint(token_mint)
                        .is_some_and(|observation_pool| observation_pool != pubkey);
                self.register_curve_mapping(
                    pubkey,
                    token_mint,
                    if preserve_observation_alias {
                        "account_update_pumpswap_continuity"
                    } else {
                        "account_update"
                    },
                    !preserve_observation_alias,
                )
                .await;
            }
            token_mint
        } else {
            match self.lookup_curve_mint(pubkey) {
                Some(mint) => {
                    self.register_curve_mapping(pubkey, mint, "account_update", false)
                        .await;
                    mint
                }
                None => {
                    // `pending_curve_updates` is the sole owner of AccountUpdate recovery.
                    // It buffers the parsed event and is drained deterministically in
                    // `register_curve_mapping` → `replay_pending_curve_update`.
                    let store_outcome = self.queue_pending_curve_update(
                        pubkey,
                        slot,
                        event_time,
                        write_version,
                        owner,
                        data,
                    );
                    ::metrics::increment_counter!(
                        "seer.account_updates.before_mapping_total",
                        "store_outcome" => match store_outcome {
                            PendingCurveUpdateStoreOutcome::Inserted => "inserted",
                            PendingCurveUpdateStoreOutcome::ReplacedNewer => "replaced_newer",
                            PendingCurveUpdateStoreOutcome::IgnoredOlder => "ignored_older",
                        }
                    );
                    coverage_audit().record_seer_account_update_before_mapping(
                        &pubkey.to_string(),
                        matches!(store_outcome, PendingCurveUpdateStoreOutcome::ReplacedNewer),
                    );
                    return Ok(true);
                }
            }
        };

        if let Some(amm_program) = AmmProgram::from_pubkey(&owner) {
            self.register_watch_from_mapping(
                pubkey,
                base_mint,
                amm_program,
                types::arrival_time_ms(),
                "account_update",
            );
        }

        let _ = &self.shadow_ledger;

        // ── Primary canonical-state wiring ────────────────────────────────────
        // AccountUpdate is now the primary AccountStateCore feed. Reserves parsed
        // from tx remain bootstrap hints only; canonical live truth flows solely
        // through this IPC event.
        if let Some(ipc) = &self.ipc_sender {
            let semantic =
                normalize_account_update_semantics("grpc_global_stream", event_time, true);
            match ipc
                .send_account_update(
                    semantic,
                    event_time,
                    base_mint,
                    pubkey,
                    self.config.commitment.curve_finality(),
                    update_payload.sol_reserves,
                    update_payload.token_reserves,
                    update_payload.complete,
                    slot,
                    write_version,
                    AccountUpdateReplayOrigin::Live,
                    None,
                )
                .await
            {
                Ok(()) => {
                    record_event_semantic_metric(semantic);
                    ::metrics::increment_counter!(
                        "seer_account_updates_reconciliation_forwarded_total"
                    );
                    ::metrics::increment_counter!("seer.account_updates.emitted");
                    ::metrics::increment_counter!(
                        "seer.account_updates.emitted_total",
                        "origin" => "live"
                    );
                    ::metrics::histogram!(
                        "seer.account_updates.latency_us",
                        recv_started_at.elapsed().as_micros() as f64
                    );
                    ::metrics::histogram!(
                        "seer.account_updates.end_to_end_latency_ms",
                        recv_started_at.elapsed().as_secs_f64() * 1000.0,
                        "origin" => "live"
                    );
                    ::metrics::gauge!(
                        "seer.account_updates.queue_depth",
                        ipc.current_queue_length() as f64
                    );
                }
                Err(err) => {
                    ::metrics::increment_counter!("seer.account_updates.send_failed_total");
                    warn!(
                        bonding_curve = %pubkey,
                        base_mint = %base_mint,
                        slot,
                        write_version = ?write_version,
                        error = %err,
                        "Failed to forward live AccountUpdate via IPC"
                    );
                }
            }
        }

        Ok(true)
    }

    /// Log UNSUPPORTED_LAYOUT at DEBUG level, rate-limited to max 1x per 60s per pubkey.
    fn log_unsupported_layout_rate_limited(&self, pubkey: Pubkey, data: &[u8], reason: &str) {
        let now = Instant::now();
        let mut map = self.unsupported_layout_last_log.write();
        let should_log = match map.get(&pubkey) {
            Some(last) => {
                now.duration_since(*last).as_secs() >= Self::UNSUPPORTED_LAYOUT_LOG_INTERVAL_SECS
            }
            None => true,
        };

        if should_log {
            map.insert(pubkey, now);
            let first_bytes: Vec<String> =
                data.iter().take(16).map(|b| format!("{:02x}", b)).collect();
            debug!(
                "AccountUpdate rejected for bonding_curve {}: {}, len={}, first_bytes=[{}]",
                pubkey,
                reason,
                data.len(),
                first_bytes.join(" ")
            );
        }
    }

    /// Sanitize developer buy amount extracted from enhanced analysis.
    /// Returns an anomaly label when the value is clamped or reset; otherwise returns `None`.
    fn sanitize_dev_buy(candidate: &mut ghost_core::EnhancedCandidate) -> Option<&'static str> {
        if !candidate.dev_buy_sol.is_finite() || candidate.dev_buy_sol < 0.0 {
            candidate.dev_buy_sol = 0.0;
            candidate.has_dev_buy = false;
            return Some("non_finite");
        }

        if candidate.dev_buy_sol > DEV_BUY_SOL_SANITY_LIMIT {
            candidate.dev_buy_sol = DEV_BUY_SOL_SANITY_LIMIT;
            return Some("unrealistic_value");
        }

        None
    }

    fn track_tx_stream_coverage(&self, source_label: &str) {
        match source_label {
            "grpc_pool_stream" => {
                self.coverage.rx_tx_pool_stream_total.fetch_add(1, Relaxed);
                pipeline_coverage().increment(PipelineCoverageStage::GrpcReceived, 1);
            }
            "grpc_global_stream" => {
                self.coverage
                    .rx_tx_global_stream_total
                    .fetch_add(1, Relaxed);
                pipeline_coverage().increment(PipelineCoverageStage::GrpcReceived, 1);
            }
            _ => {}
        }
    }

    fn tx_contains_supported_trade_instruction(event: &types::GeyserEvent) -> bool {
        let (accounts, instructions, inner_instructions): (
            &Vec<Pubkey>,
            &Vec<types::RawInstruction>,
            &Vec<types::InnerInstructionGroup>,
        ) = match event {
            types::GeyserEvent::Transaction {
                accounts,
                instructions,
                inner_instructions,
                ..
            } => (accounts, instructions, inner_instructions),
            _ => return false,
        };

        let is_trade_ix = |program_id: &Pubkey, data: &[u8]| -> bool {
            if data.len() < 8 {
                return false;
            }

            const JUPITER_V6_PROGRAM_ID: Pubkey =
                solana_sdk::pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
            const DFLOW_V4_PROGRAM_ID: Pubkey =
                solana_sdk::pubkey!("DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH");

            let disc = &data[..8];
            if *program_id == JUPITER_V6_PROGRAM_ID {
                return disc == binary_parser::DISC_JUPITER_ROUTE_V2.as_slice()
                    || disc == binary_parser::DISC_JUPITER_ROUTE.as_slice();
            }
            if *program_id == DFLOW_V4_PROGRAM_ID {
                return disc == binary_parser::DISC_DFLOW_SWAP2.as_slice()
                    || disc == binary_parser::DISC_DFLOW_SWAP.as_slice()
                    || disc == binary_parser::DISC_DFLOW_SWAP2_WITH_DESTINATION.as_slice()
                    || disc == binary_parser::DISC_DFLOW_SWAP2_WITH_DESTINATION_NATIVE.as_slice();
            }

            let amm = match AmmProgram::from_pubkey(program_id) {
                Some(amm) => amm,
                None => return false,
            };
            if matches!(amm, AmmProgram::PumpFun) {
                if disc == binary_parser::DISC_PUMP_BUY_ROUTED.as_slice() {
                    return true;
                }
                if disc == binary_parser::DISC_SWAP_OUTER_WRAPPER.as_slice() && data.len() >= 16 {
                    let inner_disc = &data[8..16];
                    if inner_disc == binary_parser::DISC_EVENT_TRADE.as_slice() {
                        return true;
                    }
                }
            }
            let amm_type = match amm {
                AmmProgram::PumpFun => ghost_core::AmmType::PumpFun,
                AmmProgram::PumpSwap => ghost_core::AmmType::PumpSwap,
            };

            ghost_core::is_trade_instruction(data, amm_type).is_some()
        };

        if instructions
            .iter()
            .any(|ix| is_trade_ix(&ix.program_id, &ix.data))
        {
            return true;
        }

        inner_instructions.iter().any(|group| {
            group.instructions.iter().any(|ix| {
                accounts
                    .get(ix.program_id_index as usize)
                    .map(|program_id| is_trade_ix(program_id, &ix.data))
                    .unwrap_or(false)
            })
        })
    }

    fn tx_contains_supported_initialize_pool_instruction(event: &types::GeyserEvent) -> bool {
        let (accounts, instructions, inner_instructions): (
            &Vec<Pubkey>,
            &Vec<types::RawInstruction>,
            &Vec<types::InnerInstructionGroup>,
        ) = match event {
            types::GeyserEvent::Transaction {
                accounts,
                instructions,
                inner_instructions,
                ..
            } => (accounts, instructions, inner_instructions),
            _ => return false,
        };

        let is_initialize_pool_ix = |program_id: &Pubkey, data: &[u8]| -> bool {
            if data.len() < 8 || AmmProgram::from_pubkey(program_id).is_none() {
                return false;
            }

            let disc = &data[..8];
            if disc == binary_parser::DISC_CREATE.as_slice()
                || disc == binary_parser::DISC_CREATE_ANCHOR.as_slice()
                || disc == binary_parser::DISC_SWAP_CREATE_POOL.as_slice()
                || disc == binary_parser::DISC_EVENT_CREATE.as_slice()
            {
                return true;
            }

            disc == binary_parser::DISC_SWAP_OUTER_WRAPPER.as_slice()
                && data.len() >= 16
                && data[8..16] == binary_parser::DISC_EVENT_CREATE
        };

        if instructions
            .iter()
            .any(|ix| is_initialize_pool_ix(&ix.program_id, &ix.data))
        {
            return true;
        }

        inner_instructions.iter().any(|group| {
            group.instructions.iter().any(|ix| {
                accounts
                    .get(ix.program_id_index as usize)
                    .map(|program_id| is_initialize_pool_ix(program_id, &ix.data))
                    .unwrap_or(false)
            })
        })
    }

    fn tx_should_log_initialize_pool_miss(event: &types::GeyserEvent) -> bool {
        let (instructions, logs): (&Vec<types::RawInstruction>, &Vec<String>) = match event {
            types::GeyserEvent::Transaction {
                instructions, logs, ..
            } => (instructions, logs),
            _ => return false,
        };

        if Self::tx_contains_supported_initialize_pool_instruction(event) {
            return true;
        }

        let has_amm_instruction = instructions
            .iter()
            .any(|ix| AmmProgram::from_pubkey(&ix.program_id).is_some());
        if !has_amm_instruction || Self::tx_contains_supported_trade_instruction(event) {
            return false;
        }

        logs.iter().any(|log| {
            log.contains("Instruction: Create")
                || log.contains("Instruction: CreatePool")
                || log.contains("Instruction: Initialize")
                || log.contains("InitializeMint")
        })
    }

    fn maybe_log_coverage(&self) {
        let now = Instant::now();
        let mut last_tick = self.coverage_last_log.lock();
        if now.duration_since(*last_tick) < COVERAGE_LOG_INTERVAL {
            return;
        }
        *last_tick = now;

        let pools_watched = self
            .grpc_connection
            .as_ref()
            .map(|conn| conn.watched_pools_count())
            .unwrap_or(0);
        let dedup_dropped = self
            .grpc_connection
            .as_ref()
            .map(|conn| conn.dedup_dropped_total())
            .unwrap_or(0);

        let trade_candidates = self.coverage.trade_candidate_total.load(Relaxed);
        let sigs_parsed = self.coverage.trade_parsed_total.load(Relaxed);
        let sigs_fwd_live = self
            .coverage
            .trade_signatures_forwarded_live_total
            .load(Relaxed);
        let sigs_fwd_replay = self
            .coverage
            .trade_signatures_forwarded_replay_total
            .load(Relaxed);
        let events_fwd_live = self
            .coverage
            .trade_events_forwarded_live_total
            .load(Relaxed);
        let events_fwd_replay = self
            .coverage
            .trade_events_forwarded_replay_total
            .load(Relaxed);
        let rpc_fallback_sigs = self
            .coverage
            .rpc_fallback_signatures_forwarded_total
            .load(Relaxed);
        let rpc_fallback_events = self
            .coverage
            .rpc_fallback_events_forwarded_total
            .load(Relaxed);
        let pending_buffered = self.coverage.pending_mapping_buffered_total.load(Relaxed);
        let events_expired = self.coverage.trade_events_expired_total.load(Relaxed);
        let entry_exec_total = self.coverage.entry_anchor_tx_total.load(Relaxed);
        let grpc_received = self.coverage.rx_tx_pool_stream_total.load(Relaxed)
            + self.coverage.rx_tx_global_stream_total.load(Relaxed);
        let parse_miss = self.coverage.parse_miss_total.load(Relaxed);
        let filtered = self.coverage.trade_filtered_total.load(Relaxed);

        let cov = summarize_coverage(
            trade_candidates,
            sigs_parsed,
            sigs_fwd_live,
            sigs_fwd_replay,
            events_fwd_live,
            events_fwd_replay,
            pending_buffered,
            events_expired,
            rpc_fallback_sigs,
            rpc_fallback_events,
        );
        // Expose signature-level parser coverage in the Prometheus gauge.
        self.metrics
            .seer_coverage_ratio
            .set(cov.parser_coverage_pct);
        self.metrics
            .rpc_fallback_forwarded_signatures_total
            .set(cov.rpc_fallback_sigs as i64);
        self.metrics
            .rpc_fallback_forwarded_events_total
            .set(cov.rpc_fallback_events as i64);
        self.metrics
            .rpc_fallback_signature_share_pct
            .set(cov.rpc_fallback_sig_share_pct);
        self.metrics
            .rpc_fallback_event_share_pct
            .set(cov.rpc_fallback_event_share_pct);

        let pipeline_snapshot = pipeline_coverage().snapshot();
        self.metrics
            .seer_ledger_coverage_ratio
            .set(pipeline_snapshot.final_ledger_ratio());

        for stage in PipelineCoverageStage::ALL {
            let value = pipeline_snapshot.total_for_stage(stage);
            let ratio = if matches!(stage, PipelineCoverageStage::ChainTruth) {
                if pipeline_snapshot.chain_truth == 0 {
                    0.0
                } else {
                    100.0
                }
            } else {
                pipeline_snapshot.ratio_vs_chain(value)
            };
            self.metrics
                .pipeline_stage_total
                .with_label_values(&[stage.as_str()])
                .set(value as i64);
            self.metrics
                .pipeline_stage_ratio
                .with_label_values(&[stage.as_str()])
                .set(ratio);
        }
        self.metrics
            .pipeline_stage_total
            .with_label_values(&["shadow_ledger_total"])
            .set(pipeline_snapshot.shadow_ledger_total() as i64);
        self.metrics
            .pipeline_stage_ratio
            .with_label_values(&["shadow_ledger_total"])
            .set(pipeline_snapshot.final_ledger_ratio());

        if cov.parser_coverage_pct < 95.0 {
            warn!(
                "⚠️ Parser (signature) coverage dropped below 95%: {:.2}% (sigs_parsed={} candidates={} parse_miss={})",
                cov.parser_coverage_pct,
                sigs_parsed,
                cov.trade_candidates,
                parse_miss,
            );
        }

        info!(
            "SEER_COVERAGE tick={} pools_watched={} rx_pool={} rx_global={} grpc_rx={} \
candidate_sigs={} sigs_parsed={} sigs_fwd_live={} sigs_fwd_replay={} \
events_fwd_live={} events_fwd_replay={} rpc_fallback_sigs={} rpc_fallback_events={} \
dedup={} parse_miss={} buffered={} expired={} filtered={} \
entry_exec_total={} \
sig_parser_cov={:.2}% sig_live_cov={:.2}% sig_eventual_cov={:.2}% \
event_live_pct={:.2}% event_replay_pct={:.2}% rpc_fallback_sig_share={:.2}% rpc_fallback_event_share={:.2}% \
listener_fwd={} snapshot_accept={} ledger_commit={} ledger_live={} ledger_total={} ledger_vs_chain={:.2}%",
            types::arrival_time_ms(),
            pools_watched,
            self.coverage.rx_tx_pool_stream_total.load(Relaxed),
            self.coverage.rx_tx_global_stream_total.load(Relaxed),
            grpc_received,
            cov.trade_candidates,
            cov.sigs_parsed,
            cov.sigs_forwarded_live,
            cov.sigs_forwarded_replay,
            cov.events_forwarded_live,
            cov.events_forwarded_replay,
            cov.rpc_fallback_sigs,
            cov.rpc_fallback_events,
            dedup_dropped,
            parse_miss,
            cov.events_buffered,
            cov.events_expired,
            filtered,
            entry_exec_total,
            cov.parser_coverage_pct,
            cov.live_coverage_pct,
            cov.eventual_coverage_pct,
            cov.event_live_pct,
            cov.event_replay_pct,
            cov.rpc_fallback_sig_share_pct,
            cov.rpc_fallback_event_share_pct,
            pipeline_snapshot.listener_forwarded,
            pipeline_snapshot.snapshot_engine_accepted,
            pipeline_snapshot.shadow_ledger_committed,
            pipeline_snapshot.shadow_ledger_live_appended,
            pipeline_snapshot.shadow_ledger_total(),
            pipeline_snapshot.final_ledger_ratio()
        );
    }

    fn hydrate_trade_mapping(&self, trade: &mut types::TradeEvent) {
        if trade.pool_amm_id == Pubkey::default() && trade.mint != Pubkey::default() {
            if let Some(curve_bytes) = self
                .mint_to_curve
                .read()
                .get(&trade.mint.to_bytes())
                .copied()
            {
                trade.pool_amm_id = Pubkey::new_from_array(curve_bytes);
            }
        }

        if trade.mint == Pubkey::default() && !Self::is_invalid_trade_pool(&trade.pool_amm_id) {
            if let Some(mint) = self.lookup_curve_mint(trade.pool_amm_id) {
                trade.mint = mint;
            }
        }

        if trade.is_pumpswap && trade.mint != Pubkey::default() && trade.mint != *wsol_mint_pubkey()
        {
            if let Some(observation_pool) = self.observation_alias_pool_for_mint(trade.mint) {
                if observation_pool != trade.pool_amm_id {
                    debug!(
                        observed_pool = %observation_pool,
                        pumpswap_pool = %trade.pool_amm_id,
                        mint = %trade.mint,
                        "Remapping PumpSwap trade onto the existing observation alias"
                    );
                    trade.pool_amm_id = observation_pool;
                }
            }
        }
    }

    fn record_trade_outcome(&self, outcome: TradeOutcome) {
        Self::record_trade_outcome_with_metrics(&self.metrics, outcome);
    }

    fn record_trade_outcome_with_metrics(metrics: &Arc<SeerMetrics>, outcome: TradeOutcome) {
        if outcome.is_buffered() {
            metrics
                .events_buffered
                .with_label_values(&[outcome.as_str()])
                .inc();
        } else {
            metrics
                .events_filtered
                .with_label_values(&[outcome.as_str()])
                .inc();
        }
        record_trade_outcome_metric(outcome);
    }

    fn invalid_trade_outcome(trade: &types::TradeEvent) -> TradeOutcome {
        if trade.pool_amm_id == *wsol_mint_pubkey() || trade.mint == *wsol_mint_pubkey() {
            TradeOutcome::FilteredWsolPool
        } else {
            TradeOutcome::FilteredInvalidPool
        }
    }

    #[inline]
    fn trade_quote_amount(trade: &types::TradeEvent) -> u64 {
        if trade.is_buy {
            trade.max_sol_cost
        } else {
            trade.min_sol_output
        }
    }

    #[inline]
    fn same_trade_identity(a: &types::TradeEvent, b: &types::TradeEvent) -> bool {
        if a.signature != b.signature {
            return false;
        }

        if let (Some(a_ord), Some(b_ord)) = (a.event_ordinal, b.event_ordinal) {
            if a_ord != b_ord {
                return false;
            }
        }

        a.is_buy == b.is_buy
            && a.amount == b.amount
            && Self::trade_quote_amount(a) == Self::trade_quote_amount(b)
            && a.signer == b.signer
    }

    fn record_pending_trade_expired(
        metrics: &Arc<SeerMetrics>,
        coverage: &Arc<CoverageCounters>,
        pending_trade: &PendingTrade,
        context: &'static str,
    ) {
        Self::record_trade_outcome_with_metrics(metrics, TradeOutcome::ExpiredWaitingForMapping);
        ::metrics::increment_counter!(
            "seer_pending_trade_expired_total",
            "reason" => pending_trade.reason.as_str()
        );
        metrics
            .pending_trade_expired_while_buffered_total
            .with_label_values(&[pending_trade.reason.as_str()])
            .inc();
        if pending_trade.is_coverage_source {
            coverage.trade_events_expired_total.fetch_add(1, Relaxed);
        }
        info!(
            "TX_PENDING_EXPIRED sig={} pool={} mint={} source={} outcome={} reason={} context={}",
            pending_trade.trade.signature,
            pending_trade.trade.pool_amm_id,
            pending_trade.trade.mint,
            pending_trade.source_label,
            TradeOutcome::ExpiredWaitingForMapping.as_str(),
            pending_trade.reason.as_str(),
            context
        );
    }

    fn should_forward_trade(
        &self,
        trade: &types::TradeEvent,
        source_label: &str,
        is_coverage_source: bool,
    ) -> TradeForwardDecision {
        let pool_id = &trade.pool_amm_id;
        let mint = &trade.mint;

        if Self::is_invalid_trade_pool(pool_id) || *mint == *wsol_mint_pubkey() || pool_id == mint {
            return TradeForwardDecision::Filtered;
        }

        if *mint == Pubkey::default() {
            self.buffer_pending_trade(trade.clone(), source_label, is_coverage_source);
            return TradeForwardDecision::BufferedPendingMapping;
        }

        let pool_bytes = pool_id.to_bytes();
        let mint_bytes = mint.to_bytes();
        let mapping_ok = {
            let fwd = self.curve_to_mint.read();
            if let Some(mapped_mint) = fwd.get(&pool_bytes) {
                *mapped_mint == mint_bytes
            } else {
                // Fallback: accept if reverse map points to THIS exact pool.
                let rev = self.mint_to_curve.read();
                matches!(rev.get(&mint_bytes), Some(mapped_pool) if *mapped_pool == pool_bytes)
            }
        };

        if !mapping_ok {
            let can_forward_with_trade_mint =
                *mint != Pubkey::default() && *mint != *wsol_mint_pubkey() && pool_id != mint;

            if can_forward_with_trade_mint {
                // Optimistic self-registration: the trade itself is the fastest possible
                // source of truth for its own pool↔mint relationship. Pump.fun pool
                // addresses are unique and never reused for a different mint, so using the
                // trade-supplied pair here is safe.
                //
                // authoritative=false guarantees that a subsequent CREATE event from the
                // gRPC stream (which carries `authoritative=true`) will override this entry
                // if needed. Until then, the correct mint is already in the registry and
                // downstream consumers receive the trade without any buffering delay.
                //
                // Old behaviour: buffer + wait for CREATE to arrive within 30ms TTL.
                // Every buffer miss (CREATE arrives >30ms late or out of order)
                // = permanently lost trade.
                self.set_curve_mapping(*pool_id, *mint, "trade_optimistic", false);
                let amm_program = if trade.is_pumpswap {
                    AmmProgram::PumpSwap
                } else {
                    AmmProgram::PumpFun
                };
                self.register_watch_from_mapping(
                    *pool_id,
                    *mint,
                    amm_program,
                    types::arrival_time_ms(),
                    "trade_optimistic",
                );
                ::metrics::increment_counter!("seer_trade_optimistic_mapping_total");
                info!(
                    "TRADE_OPTIMISTIC_FORWARD pool={} mint={} source={} action=self_register_and_forward",
                    pool_id, mint, source_label
                );
                return TradeForwardDecision::ForwardWithReplay(*pool_id, *mint);
            }

            self.buffer_pending_trade(trade.clone(), source_label, is_coverage_source);
            debug!(
                "TRADE_MAP_MISS pool={} mint={} source={} action=awaiting_authoritative_mapping",
                pool_id, mint, source_label
            );
            return TradeForwardDecision::BufferedPendingMapping;
        }

        TradeForwardDecision::Forward
    }

    #[inline(always)]
    fn is_invalid_trade_pool(pool_id: &Pubkey) -> bool {
        *pool_id == Pubkey::default() || *pool_id == *wsol_mint_pubkey()
    }

    async fn emit_trade_only(
        &self,
        trade: types::TradeEvent,
        source_label: &str,
        replayed: bool,
        is_coverage_source: bool,
    ) -> bool {
        let side = if trade.is_buy { "buy" } else { "sell" };
        let lamports = if trade.is_buy {
            trade.max_sol_cost
        } else {
            trade.min_sol_output
        };
        let volume_sol = lamports as f64 / 1_000_000_000.0;
        let success_outcome = if replayed {
            TradeOutcome::ForwardedReplay
        } else {
            TradeOutcome::ForwardedLive
        };
        info!(
            "Emitting PoolTransaction sig={} pool={} side={} volume={} source={} replayed={} outcome={} has_token_program={} has_global_config={} has_fee_recipient={}",
            trade.signature,
            trade.pool_amm_id,
            side,
            volume_sol,
            source_label,
            replayed,
            success_outcome.as_str(),
            trade.token_program.is_some(),
            trade.global_config.is_some(),
            trade.fee_recipient.is_some()
        );

        let pool_id = trade.pool_amm_id.to_string();
        let signature = trade.signature.to_string();

        if let Some(ipc_sender) = &self.ipc_sender {
            let semantic = trade.semantic;
            match ipc_sender
                .send_trade(trade, ipc::EventPriority::Normal)
                .await
            {
                Ok(()) => {
                    record_event_semantic_metric(semantic);
                    if is_coverage_source {
                        coverage_audit().record_seer_emitted(&pool_id, &signature, source_label);
                    }
                    self.record_trade_outcome(success_outcome);
                    if source_label == "grpc_backfill" {
                        self.coverage
                            .rpc_fallback_events_forwarded_total
                            .fetch_add(1, Relaxed);
                    }
                    if is_coverage_source {
                        // Event-level: one increment per TradeEvent successfully forwarded.
                        if replayed {
                            self.coverage
                                .trade_events_forwarded_replay_total
                                .fetch_add(1, Relaxed);
                        } else {
                            self.coverage
                                .trade_events_forwarded_live_total
                                .fetch_add(1, Relaxed);
                        }
                    }
                    true
                }
                Err(e) => {
                    self.record_trade_outcome(TradeOutcome::IpcSendFailed);
                    warn!(
                        "Failed to send trade via IPC: {} outcome={}",
                        e,
                        TradeOutcome::IpcSendFailed.as_str()
                    );
                    false
                }
            }
        } else {
            self.record_trade_outcome(TradeOutcome::IpcSendFailed);
            false
        }
    }

    async fn handle_trade_event(
        &self,
        mut trade: types::TradeEvent,
        source_label: &str,
        is_coverage_source: bool,
    ) -> bool {
        let inferred_timestamp_quality = infer_trade_timestamp_quality(&trade, source_label);
        trade.semantic = normalize_transaction_semantics(
            source_label,
            source_label == "pumpportal" || trade.semantic.is_synthetic(),
            trade.slot.is_some(),
            inferred_timestamp_quality,
        );
        self.hydrate_trade_mapping(&mut trade);
        let trade_ts_ms = trade
            .compat_event_ts_ms()
            .unwrap_or_else(types::ingress_epoch_ms);
        let parsed_kind = if trade.is_buy {
            WalParsedEventKind::Buy {
                lamports: trade.max_sol_cost,
                token_amount: trade.amount as u128,
            }
        } else {
            WalParsedEventKind::Sell {
                lamports: trade.min_sol_output,
                token_amount: trade.amount as u128,
            }
        };
        self.append_parsed_event_to_wal(
            trade_ts_ms,
            trade.event_time,
            trade.slot,
            Some(trade.pool_amm_id).filter(|pool_id| *pool_id != Pubkey::default()),
            parsed_kind,
        );
        if is_coverage_source {
            let pool_id = trade.pool_amm_id.to_string();
            let signature = trade.signature.to_string();
            coverage_audit().record_seer_rx(&pool_id, &signature, source_label);
        }

        if Self::is_invalid_trade_pool(&trade.pool_amm_id) {
            // pool_amm_id == Pubkey::default() means the CPI parser could not yet resolve
            // the pool for this trade (race: CREATE hasn't arrived yet).  Buffer the trade
            // so it can be replayed once register_curve_mapping() fires, instead of
            // dropping it permanently as ROLE_MISMATCH.
            if trade.pool_amm_id == Pubkey::default() {
                self.buffer_pending_trade(trade, source_label, is_coverage_source);
                return false;
            }

            // Only genuinely invalid pools (e.g. WSOL as pool_amm_id) are hard-dropped.
            if is_coverage_source {
                self.coverage.trade_filtered_total.fetch_add(1, Relaxed);
                pipeline_coverage().increment(PipelineCoverageStage::SeerFiltered, 1);
            }
            let outcome = Self::invalid_trade_outcome(&trade);
            self.record_trade_outcome(outcome);
            warn!(
                "TX_DROPPED sig={} slot={:?} source={} outcome={} detail=pool_invalid_or_wsol pool={} mint={}",
                trade.signature,
                trade.slot,
                source_label,
                outcome.as_str(),
                trade.pool_amm_id,
                trade.mint
            );
            return false;
        }

        // Race hardening: CREATE may register curve→mint after the first hydrate,
        // but before we decide to buffer a mint=default trade. Re-check once here
        // to avoid buffering a trade when the mapping is already known (which could
        // otherwise miss the replay window if CREATE already drained an empty buffer).
        self.hydrate_trade_mapping(&mut trade);

        match self.should_forward_trade(&trade, source_label, is_coverage_source) {
            TradeForwardDecision::Forward => {
                if let Some(grpc_connection) = &self.grpc_connection {
                    grpc_connection.add_watched_mint(trade.mint);
                }
                self.emit_trade_only(trade, source_label, false, is_coverage_source)
                    .await
            }
            TradeForwardDecision::ForwardWithReplay(pool, mint) => {
                // Optimistic self-registration path: mapping was just set in
                // should_forward_trade. Replay any buffered pending trades for
                // this pool→mint pair first, then emit the live trade.
                //
                // Dedup guard: if the exact same trade event (same signature + ordinal)
                // was already buffered, it will be replayed — suppress the live duplicate.
                // This handles the case where the same tx arrives twice (e.g. grpc retry).
                // Sibling events (same signature, different ordinals) are NOT suppressed.
                // Only suppress when event_ordinal is explicitly equal (both Some and equal);
                // when ordinals are None (legacy path) we allow both emissions since we
                // cannot distinguish siblings from duplicates without ordinals.
                let was_buffered = {
                    let pending = self.pending_trades.read();
                    let curve_key = PendingTradeKey::ByCurve(pool.to_bytes());
                    let mint_key = PendingTradeKey::ByMint(mint.to_bytes());
                    pending
                        .get(&curve_key)
                        .into_iter()
                        .chain(pending.get(&mint_key))
                        .flat_map(|queue| queue.iter())
                        .any(|queued| {
                            Self::same_trade_identity(&queued.trade, &trade)
                                && matches!(
                                    (queued.trade.event_ordinal, trade.event_ordinal),
                                    (Some(a), Some(b)) if a == b
                                )
                        })
                };

                if let Some(grpc_connection) = &self.grpc_connection {
                    grpc_connection.add_watched_mint(trade.mint);
                }
                self.sync_curve_mapping_to_parser(pool, mint);
                self.replay_pending_curve_update(pool, mint).await;
                self.replay_pending_trades(pool, mint).await;

                if was_buffered {
                    // Replay already emitted this exact trade; suppress the live copy.
                    self.record_trade_outcome(TradeOutcome::DedupDropped);
                    debug!(
                        "TX_DEDUP_REPLAY sig={} pool={} mint={} source={} outcome=dedup_replay_suppressed",
                        trade.signature,
                        trade.pool_amm_id,
                        trade.mint,
                        source_label
                    );
                    false
                } else {
                    self.emit_trade_only(trade, source_label, false, is_coverage_source)
                        .await
                }
            }
            TradeForwardDecision::BufferedPendingMapping => false,
            TradeForwardDecision::Filtered => {
                if is_coverage_source {
                    self.coverage.trade_filtered_total.fetch_add(1, Relaxed);
                    pipeline_coverage().increment(PipelineCoverageStage::SeerFiltered, 1);
                }
                let outcome = Self::invalid_trade_outcome(&trade);
                self.record_trade_outcome(outcome);
                debug!(
                    "TX_FILTERED sig={} pool={} mint={} source={} outcome={}",
                    trade.signature,
                    trade.pool_amm_id,
                    trade.mint,
                    source_label,
                    outcome.as_str()
                );
                false
            }
        }
    }

    async fn parse_and_forward_binary_trades(
        &self,
        parser: &BinaryParser,
        event: &types::GeyserEvent,
        source_label: &str,
        is_coverage_source: bool,
    ) -> (usize, bool) {
        self.metrics
            .binary_parser_invocations
            .with_label_values(&["trade"])
            .inc();

        let has_trade_candidate = Self::tx_contains_supported_trade_instruction(event);
        if is_coverage_source && has_trade_candidate {
            if let types::GeyserEvent::Transaction { signature, .. } = event {
                let signature = signature.to_string();
                coverage_audit().record_parse_candidate(&signature);
            }
        }

        match parser.parse_trades(event) {
            Ok(trades) => {
                let parsed_trade_count = trades.len();
                if is_coverage_source && (has_trade_candidate || parsed_trade_count > 0) {
                    self.coverage.trade_candidate_total.fetch_add(1, Relaxed);
                    pipeline_coverage().increment(PipelineCoverageStage::ChainTruth, 1);
                }
                if is_coverage_source && parsed_trade_count > 0 {
                    self.coverage.trade_parsed_total.fetch_add(1, Relaxed);
                    pipeline_coverage().increment(PipelineCoverageStage::ParsedOk, 1);
                } else if is_coverage_source && has_trade_candidate {
                    let miss_count = self.coverage.parse_miss_total.fetch_add(1, Relaxed) + 1;
                    pipeline_coverage().increment(PipelineCoverageStage::ParseMiss, 1);
                    if let types::GeyserEvent::Transaction { signature, .. } = event {
                        let signature = signature.to_string();
                        coverage_audit().record_parse_miss(&signature);
                    }
                    if miss_count % PARSE_MISS_LOG_EVERY == 0 {
                        if let types::GeyserEvent::Transaction { signature, .. } = event {
                            warn!(
                                "SEER_PARSE_MISS count={} sig={} source={}",
                                miss_count, signature, source_label
                            );
                        }
                    }
                }

                // Fire all trade IPC sends concurrently instead of sequentially.
                // With sequential for+await, one worker was blocked for N*IPC_latency
                // before freeing its slot, starving CreatePool events of worker capacity.
                let emitted_any =
                    futures_util::future::join_all(trades.into_iter().map(|trade| {
                        self.handle_trade_event(trade, source_label, is_coverage_source)
                    }))
                    .await
                    .into_iter()
                    .any(|r| r);

                if is_coverage_source && emitted_any {
                    self.coverage
                        .trade_signatures_forwarded_live_total
                        .fetch_add(1, Relaxed);
                    pipeline_coverage().increment(PipelineCoverageStage::SeerForwarded, 1);
                }
                if source_label == "grpc_backfill" && emitted_any {
                    self.coverage
                        .rpc_fallback_signatures_forwarded_total
                        .fetch_add(1, Relaxed);
                }

                (parsed_trade_count, emitted_any)
            }
            Err(e) => {
                trace!("Error parsing trades: {}", e);
                (0, false)
            }
        }
    }

    // spawn_watch_mint_backfill REMOVED.
    // We never fetch historical transactions. Only live gRPC stream events are processed.
    // A pool is only registered when its CREATE instruction is seen in the live stream.

    #[cfg_attr(test, allow(dead_code))]
    pub async fn process_event(&self, event: types::GeyserEvent) -> SeerResult<()> {
        // [EntryAnchor] — keep raw slot throughput stats and scan for embedded CPI.
        if let types::GeyserEvent::EntryAnchor {
            executed_transaction_count,
            raw,
            slot,
        } = event
        {
            self.coverage
                .entry_anchor_tx_total
                .fetch_add(executed_transaction_count, Relaxed);

            // Best-effort entry CPI scan must never block the shared Seer event
            // workers. Queue it to a dedicated bounded worker instead of parsing
            // inline on the hot path.
            if !raw.is_empty() {
                self.enqueue_entry_cpi_scan(raw, slot);
            }

            return Ok(());
        }

        if self.handle_account_update(&event).await? {
            return Ok(());
        }

        let source_label = match &event {
            types::GeyserEvent::Transaction { source, .. } => source.as_str(),
            _ => "unknown",
        };

        // SOURCE ROUTING: Check if this is a synthetic event (e.g., from PumpPortal)
        // Synthetic events are pre-parsed and should NEVER go through binary parsing
        let is_synthetic = match &event {
            types::GeyserEvent::Transaction { synthetic, .. } => *synthetic,
            _ => false,
        };
        let is_dedicated_funding_lane_source = Self::is_dedicated_funding_lane_source(source_label);

        // Track event source in metrics
        self.metrics
            .events_received
            .with_label_values(&[source_label, if is_synthetic { "synthetic" } else { "raw" }])
            .inc();

        if is_dedicated_funding_lane_source {
            self.maybe_emit_funding_transfer_observations(&event, source_label, is_synthetic)
                .await?;
            return Ok(());
        }

        let start = Instant::now();
        let detection_received_at = std::time::SystemTime::now();
        let ultrafast_mode = self.update_ultrafast_mode();
        let is_coverage_source = matches!(
            source_label,
            "grpc_pool_stream" | GRPC_GLOBAL_STREAM_SOURCE_LABEL
        );
        if is_coverage_source {
            if let types::GeyserEvent::Transaction { signature, .. } = &event {
                let signature = signature.to_string();
                coverage_audit().record_raw_received(&signature, source_label);
            }
        }
        if is_coverage_source {
            self.track_tx_stream_coverage(source_label);
            self.maybe_log_coverage();
        }

        self.append_raw_tx_to_wal(&event);

        // Extract synthetic payload for PumpPortal events (pre-parsed data)
        let mut synthetic_pool: Option<types::InitializePoolEvent> = None;
        let mut synthetic_trade: Option<types::TradeEvent> = None;
        if is_synthetic && source_label == "pumpportal" {
            if let types::GeyserEvent::Transaction { instructions, .. } = &event {
                for ix in instructions {
                    if ix.data.is_empty() {
                        continue;
                    }
                    match bincode::deserialize::<SyntheticPayload>(&ix.data) {
                        Ok(SyntheticPayload::InitializePool(pool)) => {
                            synthetic_pool = Some(pool);
                            break;
                        }
                        Ok(SyntheticPayload::Trade(trade)) => {
                            synthetic_trade = Some(trade);
                            break;
                        }
                        Err(_) => {
                            // Ignore non-synthetic payloads
                        }
                    }
                }
            }
        }

        // SOURCE ROUTING: Determine if binary parsing is needed
        let should_use_binary_parser = match self.config.effective_source_mode() {
            SeerSourceMode::PumpPortalWs => {
                // PumpPortal mode: NEVER use binary parser, events are pre-parsed
                if !is_synthetic {
                    warn!(
                        "⚠️ Unexpected non-synthetic event in PumpPortal mode: source={}",
                        source_label
                    );
                }
                false
            }
            _ => {
                // Geyser modes: Use binary parser for non-synthetic events
                if is_synthetic {
                    debug!(
                        "Skipping binary parsing for synthetic event: source={}",
                        source_label
                    );
                    false
                } else {
                    true
                }
            }
        };

        self.maybe_emit_funding_transfer_observations(&event, source_label, is_synthetic)
            .await?;

        // Parse event for InitializePool instruction (only if parser is available and should be used)
        let mut parse_result = if should_use_binary_parser {
            if let Some(ref parser) = self.parser {
                // Track binary parser invocation
                self.metrics
                    .binary_parser_invocations
                    .with_label_values(&["initialize_pool"])
                    .inc();
                parser.parse_initialize_pool(&event)?
            } else {
                // This represents an invariant violation: we should never need parsing when parser is None
                error!("❌ Binary parser not available but was requested - this is a logic bug in source routing");
                return Err(SeerError::ConfigError(
                    "Binary parser required but not initialized".to_string(),
                ));
            }
        } else {
            // Skip binary parsing for synthetic events or PumpPortal mode
            trace!(
                "Source routing: skipping binary parser for {} event",
                if is_synthetic {
                    "synthetic"
                } else {
                    "PumpPortal"
                }
            );
            None
        };

        if parse_result.is_none() {
            if let Some(pool_event) = synthetic_pool.take() {
                parse_result = Some(pool_event);
            }
        }

        match parse_result {
            Some(pool_event) => {
                let pool_slot = pool_event.slot;
                let pool_block_time = pool_event.block_time;
                let amm_program =
                    AmmProgram::from_pubkey(&pool_event.amm_program_id).ok_or_else(|| {
                        SeerError::InvalidProgramId {
                            expected: "Pump.fun or Bonk.fun".to_string(),
                            actual: pool_event.amm_program_id.to_string(),
                        }
                    })?;

                // Record detection
                self.metrics
                    .initialize_pool_detected
                    .with_label_values(&[amm_program.name()])
                    .inc();

                // [FIX-3] Session-Start Slot Guard
                if let Some(slot) = pool_slot {
                    let current_start = self
                        .session_start_slot
                        .load(std::sync::atomic::Ordering::Relaxed);
                    if current_start == 0 {
                        // First valid slot becomes session start
                        self.session_start_slot
                            .store(slot, std::sync::atomic::Ordering::SeqCst);
                        info!("Session start slot initialized to {}", slot);
                    } else if slot < current_start {
                        warn!(
                            "Rejecting CandidatePool from old slot {} (session started at {})",
                            slot, current_start
                        );
                        return Ok(());
                    }
                }

                // [FIX-3] Reject BackfillTransaction for InitializePool
                let is_backfill = if let types::GeyserEvent::Transaction { source, .. } = &event {
                    source == "grpc_backfill"
                } else {
                    false
                };
                if is_backfill {
                    warn!(
                        "Rejecting CandidatePool from backfill queue (slot {:?})",
                        pool_slot
                    );
                    return Ok(());
                }

                // Apply filters
                if !self.should_process_pool(&pool_event, amm_program) {
                    self.metrics
                        .pool_events_filtered
                        .with_label_values(&["filtered_by_config"])
                        .inc();
                    return Ok(());
                }

                // Convert to CandidatePool
                let mut candidate: CandidatePool = pool_event.into();
                candidate.semantic =
                    transaction_semantic_from_event(&event, source_label, is_synthetic);
                let candidate_mode = self.pool_init_candidate_mode(amm_program, &candidate);
                let observe_candidate = matches!(candidate_mode, PoolInitCandidateMode::Observe);

                if matches!(candidate_mode, PoolInitCandidateMode::Suppressed) {
                    self.metrics
                        .pool_events_filtered
                        .with_label_values(&["pumpswap_candidate_suppressed"])
                        .inc();
                    ::metrics::increment_counter!(
                        "seer_pumpswap_candidate_suppressed_total",
                        "reason" => "unknown_mint"
                    );
                    info!(
                        pumpswap_pool = %candidate.pool_amm_id,
                        base_mint = %candidate.base_mint,
                        "Suppressing PumpSwap create because the mint has no existing PumpFun observation to continue"
                    );
                    return Ok(());
                }
                self.append_parsed_event_to_wal(
                    candidate
                        .compat_event_ts_ms()
                        .unwrap_or_else(types::ingress_epoch_ms),
                    candidate.event_time,
                    candidate.slot,
                    Some(candidate.pool_amm_id),
                    WalParsedEventKind::Create,
                );

                // Send PoolDetected via IPC *before* register_curve_mapping.
                //
                // register_curve_mapping ends with replay_pending_trades(), which emits
                // SeerEvent::Trade on this same IPC channel for any trades buffered during
                // the create/trade race window. The IPC channel is a FIFO queue, so sending
                // PoolDetected here guarantees that ghost-launcher always observes:
                //
                //   PoolDetected → Trade   (correct order)
                //
                // instead of the inverted Trade → PoolDetected that the previous placement
                // produced. Without this ordering, the launcher's SessionPoolTradeBridge
                // receives Trade before the pool is registered and silently drops it,
                // causing the first dev-buy to be lost every time.
                match candidate_mode {
                    PoolInitCandidateMode::Observe => {
                        if let Some(ipc_sender) = &self.ipc_sender {
                            record_event_semantic_metric(candidate.semantic);
                            ipc_sender
                                .send(candidate.clone(), EventPriority::Normal)
                                .await
                                .map_err(|e| SeerError::ChannelSendError(e.to_string()))?;
                        } else if let Some(candidate_sender) = &self.candidate_sender {
                            // candidate_sender is the legacy non-IPC path; it does not participate
                            // in trade replay, so this move is semantic cleanup only for that branch.
                            candidate_sender.send(candidate.clone()).await.map_err(
                                |e: tokio::sync::mpsc::error::SendError<CandidatePool>| {
                                    SeerError::ChannelSendError(e.to_string())
                                },
                            )?;
                        } else {
                            return Err(SeerError::ChannelSendError(
                                "No sender configured".to_string(),
                            ));
                        }

                        self.register_curve_mapping(
                            candidate.bonding_curve,
                            candidate.base_mint,
                            "create",
                            true,
                        )
                        .await;
                    }
                    PoolInitCandidateMode::ContinuityOnly { observation_pool } => {
                        ::metrics::increment_counter!(
                            "seer_pumpswap_candidate_suppressed_total",
                            "reason" => "continuity_only"
                        );
                        self.seed_pumpswap_continuity(
                            &candidate,
                            observation_pool,
                            detection_received_at,
                        );
                    }
                    PoolInitCandidateMode::Suppressed => unreachable!(),
                }

                // Try to build EnhancedCandidate in a background task so it doesn't block the hot path
                if observe_candidate && !ultrafast_mode {
                    let candidate_clone = candidate.clone();
                    // Just take what we need to avoid cloning the large event if possible,
                    // but we need event for analysis. GeyserEvent clone is moderately expensive but
                    // doing it off the hot path is fine, or we can just send it raw.
                    let event_clone = event.clone();
                    let metrics_clone = Arc::clone(&self.metrics);
                    tokio::spawn(async move {
                        if let Some(mut enhanced) =
                            crate::enhanced_builder::build_enhanced_candidate(
                                &candidate_clone,
                                &event_clone,
                                amm_program,
                            )
                        {
                            if let Some(reason) = Self::sanitize_dev_buy(&mut enhanced) {
                                metrics_clone
                                    .dev_buy_anomaly_total
                                    .with_label_values(&[reason])
                                    .inc();
                            }
                            debug!(
                                "Built EnhancedCandidate (spawned): vanity={}, dev_buy={}, mint_auth_disabled={}",
                                enhanced.vanity_score,
                                enhanced.has_dev_buy,
                                enhanced.mint_auth_disabled
                            );
                        }
                    });
                }

                // Record successful parse
                self.metrics
                    .initialize_pool_parsed_success
                    .with_label_values(&[amm_program.name()])
                    .inc();

                // Track bonding_curve for ShadowLedger updates and bootstrap with genesis until first AccountUpdate
                if observe_candidate {
                    if let Some(ledger) = &self.shadow_ledger {
                        self.tracked_curves
                            .write()
                            .insert(candidate.bonding_curve, candidate.base_mint);

                        info!(
                            "CURVE_SUBSCRIBED bonding_curve={} base_mint={}",
                            candidate.bonding_curve, candidate.base_mint
                        );

                        let genesis_curve = genesis_curve();

                        store_bootstrap_seed(
                            ledger.as_ref(),
                            candidate.bonding_curve,
                            candidate.base_mint,
                            genesis_curve,
                        );
                        info!(
                            "CURVE_BOOTSTRAP source=genesis_seed bonding_curve={} base_mint={}",
                            candidate.bonding_curve, candidate.base_mint
                        );
                    }
                }

                // Record latency
                let latency_ms = start.elapsed().as_millis() as f64;
                self.metrics
                    .processing_latency
                    .with_label_values(&[amm_program.name()])
                    .observe(latency_ms);

                // Mint-to-detection latency if block_time is available
                if let Some(block_time) = pool_block_time {
                    if let Some(block_ts) = std::time::UNIX_EPOCH
                        .checked_add(std::time::Duration::from_secs(block_time as u64))
                    {
                        if let Ok(delta) = detection_received_at.duration_since(block_ts) {
                            let delta_ms = delta.as_millis() as f64;
                            self.metrics
                                .mint_to_detection_latency
                                .with_label_values(&[amm_program.name(), source_label])
                                .observe(delta_ms);

                            if delta_ms > LATE_DETECTION_THRESHOLD_MS {
                                self.metrics
                                    .late_detection_total
                                    .with_label_values(&[amm_program.name(), source_label])
                                    .inc();
                                warn!(
                                    "⚠️ Late pool detection: {:.2}ms after mint (slot={:?}, source={})",
                                    delta_ms,
                                    pool_slot,
                                    source_label
                                );
                            }
                        }
                    }
                }

                let parser_finished_at = std::time::SystemTime::now();

                if observe_candidate {
                    info!(
                        "Detected new pool: {} on {} (latency: {:.2}ms) [enhanced: false] | detection_ts={} parser_ts={} block_time={:?} slot={:?} source={}",
                        candidate.pool_amm_id,
                        amm_program.name(),
                        latency_ms,
                        format_rfc3339_seconds(detection_received_at),
                        format_rfc3339_seconds(parser_finished_at),
                        pool_block_time,
                        pool_slot,
                        source_label
                    );

                    self.metrics
                        .candidate_forwarded_to_oracle
                        .with_label_values(&[amm_program.name()])
                        .inc();

                    // Log Land Rate
                    let land_rate = self.metrics.calculate_land_rate(amm_program.name());
                    if land_rate < 95.0 {
                        warn!(
                            "Land Rate for {} is {:.2}% (below 95% SLA)",
                            amm_program.name(),
                            land_rate
                        );
                    }
                } else if let PoolInitCandidateMode::ContinuityOnly { observation_pool } =
                    candidate_mode
                {
                    info!(
                        "PumpSwap continuity seeded: pool={} observation_pool={} mint={} latency={:.2}ms detection_ts={} parser_ts={} slot={:?} source={}",
                        candidate.pool_amm_id,
                        observation_pool,
                        candidate.base_mint,
                        latency_ms,
                        format_rfc3339_seconds(detection_received_at),
                        format_rfc3339_seconds(parser_finished_at),
                        pool_slot,
                        source_label
                    );
                }

                // ── Atomic create+buy: parse trades from the same TX ──────────
                //
                // On pump.fun the creator's initial buy is a separate instruction
                // inside the same transaction that carries the Create instruction.
                // On PumpSwap the genesis capital may live only in `CreatePool`
                // params with no explicit swap event at all. `parse_trades`
                // covers both cases: explicit co-located trades and create-only
                // synthetic creator-entry flow derived from observed CreatePool
                // amounts. Without this block the genesis signature is
                // permanently lost from the trade stream, causing undercounted
                // tx/volume and missing creator exposure.
                //
                // For normal candidates PoolDetected was already sent above.
                // For PumpSwap continuity-only creates we skip PoolDetected and rely
                // on mint-alias remapping so the trade stays attached to the existing
                // observation session.
                if should_use_binary_parser {
                    if let Some(ref parser) = self.parser {
                        let (parsed_trade_count, _) = self
                            .parse_and_forward_binary_trades(
                                parser,
                                &event,
                                source_label,
                                is_coverage_source,
                            )
                            .await;
                        if parsed_trade_count > 0 {
                            debug!(
                                "CREATE_TX_TRADES: {} co-located trade(s) in create TX for pool={}",
                                parsed_trade_count, candidate.pool_amm_id
                            );
                        }
                    }
                }
            }
            None => {
                // PumpPortal synthetic trade path (pre-parsed)
                if let Some(trade) = synthetic_trade.take() {
                    let emitted = self
                        .handle_trade_event(trade, source_label, is_coverage_source)
                        .await;
                    if emitted && is_coverage_source {
                        // Signature-level: one live-forwarded signature for the entire tx.
                        self.coverage
                            .trade_signatures_forwarded_live_total
                            .fetch_add(1, Relaxed);
                        pipeline_coverage().increment(PipelineCoverageStage::SeerForwarded, 1);
                    }
                    if emitted && source_label == "grpc_backfill" {
                        self.coverage
                            .rpc_fallback_signatures_forwarded_total
                            .fetch_add(1, Relaxed);
                    }

                    return Ok(());
                }

                // Event was not recognized as InitializePool
                // Try parsing as trade transactions (Buy/Sell) - only if parser is available AND should be used
                // This ensures synthetic events and PumpPortal mode events skip trade parsing too
                if should_use_binary_parser {
                    if let Some(ref parser) = self.parser {
                        let _ = self
                            .parse_and_forward_binary_trades(
                                parser,
                                &event,
                                source_label,
                                is_coverage_source,
                            )
                            .await;
                    } else {
                        // This should never happen if should_use_binary_parser is true
                        error!("❌ Trade parsing requested but parser not available - logic bug");
                    }
                }

                // Event was not recognized as InitializePool - log diagnostic info for debugging
                // This helps identify why events might be dropped from the pipeline
                // NOTE: Only log these warnings in Geyser modes, NOT in PumpPortal mode
                if !matches!(
                    self.config.effective_source_mode(),
                    SeerSourceMode::PumpPortalWs
                ) {
                    if let types::GeyserEvent::Transaction {
                        signature,
                        accounts,
                        instructions,
                        logs,
                        ..
                    } = &event
                    {
                        if Self::tx_should_log_initialize_pool_miss(&event) {
                            let empty_data_count = instructions
                                .iter()
                                .filter(|ix| {
                                    AmmProgram::from_pubkey(&ix.program_id).is_some()
                                        && ix.data.is_empty()
                                })
                                .count();

                            debug!(
                                "DROPPED_CANDIDATE: sig={} accounts={} instructions={} logs={} empty_ix={}",
                                signature,
                                accounts.len(),
                                instructions.len(),
                                logs.len(),
                                empty_data_count
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_funding_lane_event(&self, event: types::GeyserEvent) -> SeerResult<()> {
        let source_label = match &event {
            types::GeyserEvent::Transaction { source, .. } => source.as_str(),
            _ => return Ok(()),
        };
        let is_synthetic = match &event {
            types::GeyserEvent::Transaction { synthetic, .. } => *synthetic,
            _ => false,
        };

        self.metrics
            .events_received
            .with_label_values(&[source_label, if is_synthetic { "synthetic" } else { "raw" }])
            .inc();

        self.maybe_emit_funding_transfer_observations(&event, source_label, is_synthetic)
            .await
    }

    fn funding_transfer_contract_for_source(
        &self,
        source_label: &str,
    ) -> Option<(bool, ipc::FundingTransferProvenance)> {
        match source_label {
            GRPC_GLOBAL_STREAM_SOURCE_LABEL => Some((
                false,
                ipc::FundingTransferProvenance::filtered_grpc_global_stream_live(),
            )),
            GRPC_FUNDING_LANE_PUMP_FILTERED_SOURCE_LABEL
                if matches!(self.config.funding_lane_mode, FundingLaneMode::PumpFiltered) =>
            {
                Some((
                    false,
                    ipc::FundingTransferProvenance::funding_lane_pump_filtered_live(),
                ))
            }
            GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL
                if matches!(self.config.funding_lane_mode, FundingLaneMode::FullChain) =>
            {
                Some((
                    true,
                    ipc::FundingTransferProvenance::authoritative_full_feed_live(),
                ))
            }
            _ => None,
        }
    }

    /// PR-2 lane split: `grpc_global_stream` stays filtered forever; dedicated
    /// funding lanes are opt-in and may emit authoritative `full_chain_coverage`
    /// only when the explicit full-chain mode is enabled.
    async fn maybe_emit_funding_transfer_observations(
        &self,
        event: &types::GeyserEvent,
        source_label: &str,
        is_synthetic: bool,
    ) -> SeerResult<()> {
        if is_synthetic
            || !matches!(
                self.config.effective_source_mode(),
                SeerSourceMode::GeyserGrpc
            )
        {
            return Ok(());
        }

        let Some(ipc_sender) = &self.ipc_sender else {
            return Ok(());
        };
        let Some((full_chain_coverage, provenance)) =
            self.funding_transfer_contract_for_source(source_label)
        else {
            return Ok(());
        };

        let types::GeyserEvent::Transaction {
            slot,
            event_ts_ms,
            arrival_ts_ms,
            event_time,
            signature,
            ..
        } = event
        else {
            return Ok(());
        };

        let transfers = extract_funding_transfer_observations(event);
        if transfers.is_empty() {
            return Ok(());
        }

        let semantic = transaction_semantic_from_event(event, source_label, is_synthetic);
        let resolved_arrival_ts_ms = arrival_ts_ms
            .or(event_time.ingress_wall_ts_ms)
            .or(*event_ts_ms)
            .unwrap_or_default();
        ::metrics::counter!(
            "seer_funding_transfer_observations_total",
            transfers.len() as u64,
            "lane" => provenance.lane_kind.as_str(),
            "coverage" => provenance.coverage_class.as_str()
        );

        for transfer in transfers {
            ipc_sender
                .send_funding_transfer(
                    ipc::FundingTransferEvent {
                        semantic: semantic.clone(),
                        slot: *slot,
                        event_ordinal: transfer.event_ordinal,
                        tx_index: None,
                        outer_instruction_index: transfer.outer_instruction_index,
                        inner_group_index: transfer.inner_group_index,
                        cpi_stack_height: transfer.cpi_stack_height,
                        event_time: event_time.clone(),
                        arrival_ts_ms: resolved_arrival_ts_ms,
                        signature: signature.to_string(),
                        source_wallet: transfer.source_wallet,
                        recipient_wallet: transfer.recipient_wallet,
                        lamports: transfer.lamports,
                        full_chain_coverage,
                        provenance,
                    },
                    EventPriority::High,
                )
                .await
                .map_err(|e| SeerError::ChannelSendError(e.to_string()))?;
        }

        Ok(())
    }

    /// Update ultrafast degraded mode flag based on queue utilization

    fn update_ultrafast_mode(&self) -> bool {
        // Prefer IPC queue metrics when available
        let utilization = if let Some(ipc_sender) = &self.ipc_sender {
            ipc_sender.queue_utilization()
        } else if let Some(sender) = &self.candidate_sender {
            let capacity: usize = self.config.channel_buffer_size;
            if capacity == 0 {
                100.0
            } else {
                let remaining: usize = sender.capacity();
                let used = capacity.saturating_sub(remaining);
                (used as f64 / capacity as f64) * 100.0
            }
        } else {
            0.0
        };

        let currently_ultrafast = self.ultrafast_mode.load(Relaxed);

        if utilization >= self.config.ultrafast_enter_threshold && !currently_ultrafast {
            warn!(
                "Seer backpressure detected (queue_util={:.1}%) - entering ultrafast degraded mode",
                utilization
            );
            self.ultrafast_mode.store(true, Relaxed);
            true
        } else if utilization <= self.config.ultrafast_exit_threshold && currently_ultrafast {
            info!(
                "Seer queue recovered (queue_util={:.1}%) - exiting ultrafast degraded mode",
                utilization
            );
            self.ultrafast_mode.store(false, Relaxed);
            false
        } else {
            currently_ultrafast
        }
    }

    /// Check if a pool should be processed based on configuration filters
    fn should_process_pool(
        &self,
        pool_event: &types::InitializePoolEvent,
        amm_program: AmmProgram,
    ) -> bool {
        // Check if AMM is enabled
        match amm_program {
            AmmProgram::PumpFun if !self.config.filter.enable_pumpfun => return false,
            _ => {}
        }

        // Check quote mint filter
        if !self
            .config
            .filter
            .is_quote_mint_allowed(&pool_event.quote_mint)
        {
            return false;
        }

        // Check liquidity requirement
        let liquidity_sol = pool_event
            .initial_virtual_sol_reserves
            .or(pool_event.initial_real_sol_reserves)
            .map(|lamports| lamports as f64 / 1_000_000_000.0);

        if !self
            .config
            .filter
            .meets_liquidity_requirement(liquidity_sol)
        {
            return false;
        }

        true
    }

    fn pool_init_candidate_mode(
        &self,
        amm_program: AmmProgram,
        candidate: &CandidatePool,
    ) -> PoolInitCandidateMode {
        match amm_program {
            AmmProgram::PumpFun => PoolInitCandidateMode::Observe,
            AmmProgram::PumpSwap => match self.observation_alias_pool_for_mint(candidate.base_mint)
            {
                Some(observation_pool) if observation_pool != candidate.pool_amm_id => {
                    PoolInitCandidateMode::ContinuityOnly { observation_pool }
                }
                _ => PoolInitCandidateMode::Suppressed,
            },
        }
    }

    /// Get current metrics
    pub fn metrics(&self) -> &SeerMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{StreamMode, TxFilterStrategy};
    use crate::ipc::{
        create_ipc_channel, BackpressurePolicy, IpcChannelConfig, IpcReceiver, SeerEvent,
    };
    use ghost_core::{ParsedEventKind as WalParsedEventKind, Wal, WalRecord};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, Signature, Signer};
    use std::collections::{HashMap, HashSet};
    use tempfile::tempdir;

    /// Mutex that serializes tests which snapshot and assert on the global
    /// `pipeline_coverage()` counters.  Those counters are process-wide atomics,
    /// so any two tests that (a) take a "before" snapshot, (b) do work that
    /// increments the counters, and (c) check the delta must not run concurrently
    /// with each other or any other test that also increments those counters.
    fn coverage_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    const TRADE_FORWARD_TIMEOUT_MS: u64 = 200;

    fn test_trade(pool_amm_id: Pubkey, mint: Pubkey) -> types::TradeEvent {
        types::TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: Signature::new_unique(),
            event_ordinal: Some(0),
            tx_index: None,
            provenance: None,
            timestamp_ms: 1_000,
            arrival_ts_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            pool_amm_id,
            mint,
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: false,
            amount: 42,
            max_sol_cost: 1_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: types::RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: Some(10.0),
            v_sol_in_bonding_curve: Some(1.0),
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            creator_vault: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: vec![],
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: types::ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        }
    }

    fn synthetic_initialize_pool_event(pool: types::InitializePoolEvent) -> types::GeyserEvent {
        let payload = bincode::serialize(&types::SyntheticPayload::InitializePool(pool.clone()))
            .expect("serialize synthetic pool");
        types::GeyserEvent::Transaction {
            slot: pool.slot,
            event_ts_ms: pool.event_ts_ms,
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: pool.event_time,
            signature: pool.signature,
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: Pubkey::default(),
                account_indices: vec![],
                data: payload,
            }],
            logs: vec![],
            block_time: pool.block_time,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: true,
            source: "pumpportal".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    fn push_borsh_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn cpi_create_payload(mint: Pubkey, curve: Pubkey, creator: Pubkey) -> Vec<u8> {
        let mut payload = binary_parser::DISC_EVENT_CREATE.to_vec();
        push_borsh_string(&mut payload, "TestToken");
        push_borsh_string(&mut payload, "TEST");
        push_borsh_string(&mut payload, "https://example.invalid/token.json");
        payload.extend_from_slice(mint.as_ref());
        payload.extend_from_slice(curve.as_ref());
        payload.extend_from_slice(creator.as_ref());
        payload
    }

    fn cpi_buy_payload(
        mint: Pubkey,
        creator: Pubkey,
        sol_amount: u64,
        token_amount: u64,
    ) -> Vec<u8> {
        let mut payload = binary_parser::DISC_EVENT_TRADE.to_vec();
        payload.extend_from_slice(mint.as_ref());
        payload.extend_from_slice(&sol_amount.to_le_bytes());
        payload.extend_from_slice(&token_amount.to_le_bytes());
        payload.push(1u8);
        payload.extend_from_slice(creator.as_ref());
        payload.extend_from_slice(&1_700_000_000i64.to_le_bytes());
        payload.extend_from_slice(&30_000_000_000u64.to_le_bytes());
        payload.extend_from_slice(&1_000_000_000_000u64.to_le_bytes());
        payload
    }

    fn create_tx_with_cpi_create_and_trade(
        signature: Signature,
        mint: Pubkey,
        curve: Pubkey,
        creator: Pubkey,
        source_label: &str,
    ) -> types::GeyserEvent {
        let pump_program = Pubkey::from_str(crate::grpc_connection::PUMP_FUN_PROGRAM_ID)
            .expect("valid pumpfun id");

        types::GeyserEvent::Transaction {
            slot: Some(42),
            event_ts_ms: Some(1_777_777_777_000),
            arrival_ts_ms: Some(1_777_777_777_123),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature,
            accounts: vec![pump_program],
            instructions: vec![],
            logs: vec![],
            block_time: Some(1_777_777_777),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: source_label.to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![types::InnerInstructionGroup {
                index: 0,
                instructions: vec![
                    types::InnerIx {
                        program_id_index: 0,
                        accounts: vec![],
                        data: cpi_create_payload(mint, curve, creator),
                        stack_height: Some(1),
                    },
                    types::InnerIx {
                        program_id_index: 0,
                        accounts: vec![],
                        data: cpi_buy_payload(mint, creator, 100_000_000, 1_000_000),
                        stack_height: Some(1),
                    },
                ],
            }],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    fn system_transfer_payload(lamports: u64) -> Vec<u8> {
        let mut payload = SYSTEM_PROGRAM_TRANSFER_DISCRIMINATOR.to_vec();
        payload.extend_from_slice(&lamports.to_le_bytes());
        payload
    }

    fn create_tx_with_funding_transfer_observations(
        source_label: &str,
    ) -> (types::GeyserEvent, Vec<(Pubkey, Pubkey, u64)>) {
        let top_level_source = Pubkey::new_unique();
        let top_level_recipient = Pubkey::new_unique();
        let inner_source = Pubkey::new_unique();
        let inner_recipient = Pubkey::new_unique();
        let top_level_lamports = 5_000_000_000;
        let inner_lamports = 7_000_000_000;

        (
            types::GeyserEvent::Transaction {
                slot: Some(77),
                event_ts_ms: Some(1_666_666_666_000),
                arrival_ts_ms: Some(1_666_666_666_123),
                event_time: ghost_core::EventTimeMetadata::default(),
                signature: Signature::new_unique(),
                accounts: vec![
                    top_level_source,
                    top_level_recipient,
                    solana_sdk::system_program::ID,
                    inner_source,
                    inner_recipient,
                ],
                instructions: vec![types::RawInstruction {
                    program_id: solana_sdk::system_program::ID,
                    account_indices: vec![0, 1],
                    data: system_transfer_payload(top_level_lamports),
                }],
                logs: vec![],
                block_time: Some(1_666_666_666),
                account_data: HashMap::new(),
                pre_balances: vec![],
                post_balances: vec![],
                success: true,
                error_code: None,
                compute_units_consumed: None,
                synthetic: false,
                source: source_label.to_string(),
                mpcf_payload_bytes: None,
                mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
                inner_instructions: vec![types::InnerInstructionGroup {
                    index: 0,
                    instructions: vec![types::InnerIx {
                        program_id_index: 2,
                        accounts: vec![3, 4],
                        data: system_transfer_payload(inner_lamports),
                        stack_height: Some(1),
                    }],
                }],
                pre_token_balances: vec![],
                post_token_balances: vec![],
            },
            vec![
                (top_level_source, top_level_recipient, top_level_lamports),
                (inner_source, inner_recipient, inner_lamports),
            ],
        )
    }

    #[derive(Clone, Copy)]
    enum WorkerOrder {
        CreateThenTrade,
        TradeThenCreate,
    }

    async fn assert_single_forwarded_trade(
        ipc_receiver: &mut IpcReceiver,
        pool: Pubkey,
        mint: Pubkey,
    ) {
        let forwarded = tokio::time::timeout(
            std::time::Duration::from_millis(TRADE_FORWARD_TIMEOUT_MS),
            ipc_receiver.recv(),
        )
        .await
        .expect("trade must be forwarded within timeout")
        .expect("IPC channel must not be closed");

        match forwarded {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(
                    trade_event.trade.pool_amm_id, pool,
                    "forwarded trade must carry the correct pool"
                );
                assert_eq!(
                    trade_event.trade.mint, mint,
                    "forwarded trade must carry the resolved mint"
                );
            }
            other => panic!("expected a Trade event, got {:?}", other),
        }

        assert!(
            ipc_receiver.try_recv().is_err(),
            "trade must be forwarded exactly once (no duplicates)"
        );
    }

    async fn assert_forwarded_funding_transfer(
        ipc_receiver: &mut IpcReceiver,
        expected_source: Pubkey,
        expected_recipient: Pubkey,
        expected_lamports: u64,
        expected_full_chain_coverage: bool,
        expected_provenance: ipc::FundingTransferProvenance,
        expected_event_ordinal: Option<u32>,
        expected_outer_instruction_index: Option<u32>,
        expected_inner_group_index: Option<u32>,
        expected_cpi_stack_height: Option<u32>,
    ) {
        let forwarded = tokio::time::timeout(
            std::time::Duration::from_millis(TRADE_FORWARD_TIMEOUT_MS),
            ipc_receiver.recv(),
        )
        .await
        .expect("funding transfer must be forwarded within timeout")
        .expect("IPC channel must not be closed");

        match forwarded {
            SeerEvent::FundingTransfer(event) => {
                assert_eq!(event.transfer.source_wallet, expected_source.to_string());
                assert_eq!(
                    event.transfer.recipient_wallet,
                    expected_recipient.to_string()
                );
                assert_eq!(event.transfer.lamports, expected_lamports);
                assert_eq!(
                    event.transfer.full_chain_coverage,
                    expected_full_chain_coverage
                );
                assert_eq!(event.transfer.provenance, expected_provenance);
                assert_eq!(event.transfer.event_ordinal, expected_event_ordinal);
                assert_eq!(
                    event.transfer.outer_instruction_index,
                    expected_outer_instruction_index
                );
                assert_eq!(event.transfer.inner_group_index, expected_inner_group_index);
                assert_eq!(event.transfer.cpi_stack_height, expected_cpi_stack_height);
            }
            other => panic!("expected FundingTransfer event, got {:?}", other),
        }
    }

    async fn run_create_trade_worker_order_test(order: WorkerOrder) {
        let _guard = coverage_test_lock().lock().await;

        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Arc::new(Seer::new_with_ipc(SeerConfig::default(), ipc_sender));

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = test_trade(pool, Pubkey::default());

        let ordering_gate = Arc::new(tokio::sync::Notify::new());

        let create_task = {
            let seer = Arc::clone(&seer);
            let ordering_gate = Arc::clone(&ordering_gate);
            tokio::spawn(async move {
                if matches!(order, WorkerOrder::TradeThenCreate) {
                    ordering_gate.notified().await;
                }
                seer.register_curve_mapping(pool, mint, "create", true)
                    .await;
                if matches!(order, WorkerOrder::CreateThenTrade) {
                    ordering_gate.notify_one();
                }
            })
        };

        let trade_task = {
            let seer = Arc::clone(&seer);
            let ordering_gate = Arc::clone(&ordering_gate);
            tokio::spawn(async move {
                if matches!(order, WorkerOrder::CreateThenTrade) {
                    ordering_gate.notified().await;
                }
                let forwarded = seer
                    .handle_trade_event(trade, "grpc_global_stream", true)
                    .await;
                if matches!(order, WorkerOrder::TradeThenCreate) {
                    ordering_gate.notify_one();
                }
                forwarded
            })
        };

        let forwarded = trade_task.await.expect("trade task must not panic");
        create_task.await.expect("create task must not panic");

        match order {
            WorkerOrder::CreateThenTrade => {
                assert!(
                    forwarded,
                    "trade should forward live when CREATE worker wins"
                );
            }
            WorkerOrder::TradeThenCreate => {
                assert!(
                    !forwarded,
                    "trade should buffer first and replay when TRADE worker wins"
                );
            }
        }

        assert_single_forwarded_trade(&mut ipc_receiver, pool, mint).await;
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(pool.to_bytes())),
            "pending queue must drain after CREATE/TRADE worker ordering test"
        );
    }

    fn read_outcome_count(counter: &prometheus::CounterVec, outcome: TradeOutcome) -> f64 {
        counter.with_label_values(&[outcome.as_str()]).get()
    }

    fn read_reason_count(counter: &prometheus::CounterVec, reason: &str) -> f64 {
        counter.with_label_values(&[reason]).get()
    }

    #[test]
    fn test_summarize_coverage_uses_trade_candidates_as_denominator() {
        // 100 candidate sigs, 97 parsed, 25 forwarded live, 5 forwarded replay.
        // (candidates, sigs_parsed, sigs_live, sigs_replay, events_live, events_replay, buffered, expired)
        let summary = summarize_coverage(100, 97, 25, 5, 30, 6, 10, 2, 4, 5);
        assert_eq!(summary.trade_candidates, 100);
        // parser_coverage_pct = 97 / 100 * 100 = 97.0
        assert!((summary.parser_coverage_pct - 97.0).abs() < f64::EPSILON);
        // live_coverage_pct = 25 / 97 * 100
        assert!((summary.live_coverage_pct - (25.0 / 97.0 * 100.0)).abs() < 1e-9);
        // eventual_coverage_pct = (25 + 5) / 97 * 100
        assert!((summary.eventual_coverage_pct - (30.0 / 97.0 * 100.0)).abs() < 1e-9);
        // event_live_pct = 30 / (30 + 6) * 100
        assert!((summary.event_live_pct - (30.0 / 36.0 * 100.0)).abs() < 1e-9);
        assert!((summary.rpc_fallback_sig_share_pct - (4.0 / 30.0 * 100.0)).abs() < 1e-9);
        assert!((summary.rpc_fallback_event_share_pct - (5.0 / 36.0 * 100.0)).abs() < 1e-9);
        assert_eq!(summary.events_buffered, 10);
        assert_eq!(summary.events_expired, 2);
    }

    #[test]
    fn test_summarize_coverage_handles_empty_input_without_false_alarm() {
        let summary = summarize_coverage(0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(summary.trade_candidates, 0);
        assert_eq!(summary.parser_coverage_pct, 100.0);
        assert_eq!(summary.live_coverage_pct, 100.0);
        assert_eq!(summary.eventual_coverage_pct, 100.0);
        assert_eq!(summary.rpc_fallback_sig_share_pct, 0.0);
        assert_eq!(summary.rpc_fallback_event_share_pct, 0.0);

        let no_parse = summarize_coverage(4, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(no_parse.parser_coverage_pct, 0.0);
        assert_eq!(no_parse.live_coverage_pct, 0.0);
        assert_eq!(no_parse.eventual_coverage_pct, 0.0);
    }

    #[tokio::test]
    async fn test_grpc_backfill_trade_updates_fallback_event_counter() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        seer.register_curve_mapping(pool, mint, "create", true)
            .await;

        let trade = test_trade(pool, mint);
        let forwarded = seer.handle_trade_event(trade, "grpc_backfill", false).await;
        assert!(forwarded);
        let _ = ipc_receiver.recv().await.expect("forwarded fallback trade");

        assert_eq!(
            seer.coverage
                .rpc_fallback_events_forwarded_total
                .load(Relaxed),
            1,
            "event-level fallback counter must track grpc_backfill trade emissions"
        );
    }

    async fn assert_authoritative_mapping_source_starts_grpc_watch(source: &'static str) {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, _ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        seer.register_curve_mapping(pool, mint, source, true).await;

        let connection = seer
            .grpc_connection
            .as_ref()
            .expect("grpc connection should exist for watch registration");
        assert!(
            connection.is_pool_watched(&pool),
            "authoritative {source} mapping must start pool watch immediately"
        );
        assert!(
            connection.is_mint_watched(&mint),
            "authoritative {source} mapping must also register mint watch"
        );
    }

    #[tokio::test]
    async fn test_register_curve_mapping_create_starts_grpc_watch() {
        assert_authoritative_mapping_source_starts_grpc_watch("create").await;
    }

    #[tokio::test]
    async fn test_register_curve_mapping_entry_cpi_starts_grpc_watch() {
        assert_authoritative_mapping_source_starts_grpc_watch("entry_cpi").await;
    }

    #[tokio::test]
    async fn test_seed_pumpswap_continuity_starts_grpc_watch() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, _ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let candidate = CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            tx_index: None,
            event_ts_ms: Some(1_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique().to_string(),
            amm_program_id: AmmProgram::PumpSwap.program_id(),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 1_000,
            bonding_curve_progress: Some(0.0),
            initial_liquidity_sol: Some(30.0),
            token_total_supply: Some(1_000_000_000_000_000),
            block_time: Some(1),
        };

        seer.seed_pumpswap_continuity(
            &candidate,
            Pubkey::new_unique(),
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_000),
        );

        let connection = seer
            .grpc_connection
            .as_ref()
            .expect("grpc connection should exist for watch registration");
        assert!(
            connection.is_pool_watched(&candidate.bonding_curve),
            "PumpSwap continuity seed must start pool watch immediately"
        );
        assert!(
            connection.is_mint_watched(&candidate.base_mint),
            "PumpSwap continuity seed must also register mint watch"
        );
    }

    #[tokio::test]
    async fn test_process_event_emits_co_located_create_and_trade_with_coverage() {
        let _guard = coverage_test_lock().lock().await;

        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let signature = Signature::new_unique();
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let creator = Keypair::new().pubkey();

        let candidate_before = seer.coverage.trade_candidate_total.load(Relaxed);
        let parsed_before = seer.coverage.trade_parsed_total.load(Relaxed);
        let forwarded_before = seer
            .coverage
            .trade_signatures_forwarded_live_total
            .load(Relaxed);

        let event = create_tx_with_cpi_create_and_trade(
            signature,
            mint,
            curve,
            creator,
            GRPC_GLOBAL_STREAM_SOURCE_LABEL,
        );
        seer.process_event(event)
            .await
            .expect("create+trade tx must process");

        let first =
            tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
                .await
                .expect("first IPC event must arrive")
                .expect("IPC channel must stay open");
        let second =
            tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
                .await
                .expect("second IPC event must arrive")
                .expect("IPC channel must stay open");

        match first {
            SeerEvent::PoolDetected(ev) => {
                assert_eq!(ev.candidate.pool_amm_id, curve);
                assert_eq!(ev.candidate.base_mint, mint);
                assert_eq!(ev.candidate.creator, creator);
            }
            other => panic!("expected PoolDetected first, got {:?}", other),
        }

        match second {
            SeerEvent::Trade(ev) => {
                assert_eq!(ev.trade.pool_amm_id, curve);
                assert_eq!(ev.trade.mint, mint);
                assert_eq!(ev.trade.signer, creator);
                assert_eq!(ev.trade.signature, signature);
                assert!(ev.trade.is_buy, "co-located tx must emit creator BUY");
            }
            other => panic!("expected Trade second, got {:?}", other),
        }

        assert!(
            ipc_receiver.try_recv().is_err(),
            "co-located create+trade tx must emit exactly two IPC events"
        );

        assert_eq!(
            seer.coverage.trade_candidate_total.load(Relaxed) - candidate_before,
            1,
            "create+trade tx must count as one trade candidate signature"
        );
        assert_eq!(
            seer.coverage.trade_parsed_total.load(Relaxed) - parsed_before,
            1,
            "create+trade tx must count as one parsed trade signature"
        );
        assert_eq!(
            seer.coverage
                .trade_signatures_forwarded_live_total
                .load(Relaxed)
                - forwarded_before,
            1,
            "create+trade tx must count as one live-forwarded trade signature"
        );
    }

    #[tokio::test]
    async fn test_pumpswap_initialize_pool_without_known_mint_is_suppressed() {
        let (ipc_sender, mut ipc_receiver, _metrics) =
            create_ipc_channel(IpcChannelConfig::default());
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let filtered_before = read_reason_count(
            &seer.metrics.pool_events_filtered,
            "pumpswap_candidate_suppressed",
        );

        seer.process_event(synthetic_initialize_pool_event(
            types::InitializePoolEvent {
                slot: Some(11),
                event_ts_ms: Some(11_000),
                event_time: ghost_core::EventTimeMetadata::default(),
                signature: Signature::new_unique(),
                amm_program_id: AmmProgram::PumpSwap.program_id(),
                pool_amm_id: pool,
                base_mint: mint,
                quote_mint: *wsol_mint_pubkey(),
                bonding_curve: pool,
                creator: Pubkey::new_unique(),
                initial_virtual_token_reserves: None,
                initial_virtual_sol_reserves: None,
                initial_real_token_reserves: None,
                initial_real_sol_reserves: None,
                token_total_supply: None,
                block_time: Some(11),
                raw_data: vec![],
            },
        ))
        .await
        .expect("pumpswap create must process");

        assert!(
            ipc_receiver.try_recv().is_err(),
            "unknown PumpSwap pool must not emit PoolDetected or Trade"
        );
        assert_eq!(
            read_reason_count(
                &seer.metrics.pool_events_filtered,
                "pumpswap_candidate_suppressed"
            ) - filtered_before,
            1.0
        );
        assert_eq!(
            seer.lookup_curve_mint(pool),
            None,
            "suppressed unknown PumpSwap pool must not seed a new observation mapping"
        );
    }

    #[tokio::test]
    async fn test_pumpswap_initialize_pool_known_mint_seeds_continuity_without_pool_detected() {
        let (ipc_sender, mut ipc_receiver, _metrics) =
            create_ipc_channel(IpcChannelConfig::default());
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);
        let observation_pool = Pubkey::new_unique();
        let pumpswap_pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        seer.set_curve_mapping(observation_pool, mint, "test", true);

        seer.process_event(synthetic_initialize_pool_event(
            types::InitializePoolEvent {
                slot: Some(22),
                event_ts_ms: Some(22_000),
                event_time: ghost_core::EventTimeMetadata::default(),
                signature: Signature::new_unique(),
                amm_program_id: AmmProgram::PumpSwap.program_id(),
                pool_amm_id: pumpswap_pool,
                base_mint: mint,
                quote_mint: *wsol_mint_pubkey(),
                bonding_curve: pumpswap_pool,
                creator: Pubkey::new_unique(),
                initial_virtual_token_reserves: None,
                initial_virtual_sol_reserves: None,
                initial_real_token_reserves: None,
                initial_real_sol_reserves: None,
                token_total_supply: None,
                block_time: Some(22),
                raw_data: vec![],
            },
        ))
        .await
        .expect("known-mint pumpswap create must process");

        assert!(
            ipc_receiver.try_recv().is_err(),
            "migration-only PumpSwap create must not emit a fresh PoolDetected"
        );
        assert_eq!(seer.lookup_curve_mint(pumpswap_pool), Some(mint));
        assert_eq!(
            seer.mint_to_curve.read().get(&mint.to_bytes()).copied(),
            Some(observation_pool.to_bytes()),
            "migration continuity must preserve the existing observation alias"
        );
    }

    #[tokio::test]
    async fn test_pumpswap_trade_remaps_to_existing_observation_alias() {
        let (ipc_sender, mut ipc_receiver, _metrics) =
            create_ipc_channel(IpcChannelConfig::default());
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);
        let observation_pool = Pubkey::new_unique();
        let pumpswap_pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        seer.set_curve_mapping(observation_pool, mint, "test", true);

        let mut trade = test_trade(pumpswap_pool, mint);
        trade.is_pumpswap = true;

        assert!(
            seer.handle_trade_event(trade, "grpc_global_stream", false)
                .await,
            "pumpswap trade for known mint must be forwarded"
        );

        let Some(SeerEvent::Trade(event)) = ipc_receiver.recv().await else {
            panic!("expected remapped PumpSwap trade over IPC");
        };
        assert_eq!(event.trade.pool_amm_id, observation_pool);
        assert_eq!(event.trade.mint, mint);
    }

    #[test]
    fn test_extract_funding_transfer_observations_includes_top_level_and_inner() {
        let (event, expected) = create_tx_with_funding_transfer_observations("grpc_global_stream");

        let transfers = extract_funding_transfer_observations(&event);

        assert_eq!(transfers.len(), 2);
        assert_eq!(
            transfers[0],
            FundingTransferObservation {
                source_wallet: expected[0].0.to_string(),
                recipient_wallet: expected[0].1.to_string(),
                lamports: expected[0].2,
                event_ordinal: Some(0),
                outer_instruction_index: Some(0),
                inner_group_index: None,
                cpi_stack_height: None,
            }
        );
        assert_eq!(
            transfers[1],
            FundingTransferObservation {
                source_wallet: expected[1].0.to_string(),
                recipient_wallet: expected[1].1.to_string(),
                lamports: expected[1].2,
                event_ordinal: Some(1),
                outer_instruction_index: Some(0),
                inner_group_index: Some(0),
                cpi_stack_height: Some(1),
            }
        );
    }

    #[test]
    fn test_extract_funding_transfer_observations_skips_sync_native_wrapped_sol_transfer() {
        let source = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let event = types::GeyserEvent::Transaction {
            slot: Some(88),
            event_ts_ms: Some(1_777_777_777_000),
            arrival_ts_ms: Some(1_777_777_777_123),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![source, recipient],
            instructions: vec![
                types::RawInstruction {
                    program_id: solana_sdk::system_program::ID,
                    account_indices: vec![0, 1],
                    data: system_transfer_payload(500_000_000),
                },
                types::RawInstruction {
                    program_id: *token_legacy_program_pubkey(),
                    account_indices: vec![1],
                    data: vec![TOKEN_SYNC_NATIVE_DISCRIMINATOR],
                },
            ],
            logs: vec![],
            block_time: Some(1_777_777_777),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        let transfers = extract_funding_transfer_observations(&event);

        assert!(
            transfers.is_empty(),
            "syncNative self-wrap transfer must not be treated as wallet funding provenance"
        );
    }

    #[test]
    fn test_extract_funding_transfer_observations_canonicalizes_token_account_owners() {
        let source_token_account = Pubkey::new_unique();
        let recipient_token_account = Pubkey::new_unique();
        let source_owner = Pubkey::new_unique();
        let recipient_owner = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let event = types::GeyserEvent::Transaction {
            slot: Some(89),
            event_ts_ms: Some(1_777_777_778_000),
            arrival_ts_ms: Some(1_777_777_778_123),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![source_token_account, recipient_token_account],
            instructions: vec![types::RawInstruction {
                program_id: solana_sdk::system_program::ID,
                account_indices: vec![0, 1],
                data: system_transfer_payload(500_000_000),
            }],
            logs: vec![],
            block_time: Some(1_777_777_778),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_funding_lane_full_chain".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![
                types::RawTokenBalance {
                    account_index: 0,
                    mint: mint.to_string(),
                    owner: Some(source_owner.to_string()),
                    amount: 1,
                },
                types::RawTokenBalance {
                    account_index: 1,
                    mint: mint.to_string(),
                    owner: Some(recipient_owner.to_string()),
                    amount: 0,
                },
            ],
            post_token_balances: vec![],
        };

        let transfers = extract_funding_transfer_observations(&event);

        assert_eq!(
            transfers,
            vec![FundingTransferObservation {
                source_wallet: source_owner.to_string(),
                recipient_wallet: recipient_owner.to_string(),
                lamports: 500_000_000,
                event_ordinal: Some(0),
                outer_instruction_index: Some(0),
                inner_group_index: None,
                cpi_stack_height: None,
            }]
        );
    }

    #[tokio::test]
    async fn test_process_event_preserves_funding_lane_boundaries() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let mut full_chain_config = SeerConfig::default();
        full_chain_config.funding_lane_mode = FundingLaneMode::FullChain;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config.clone());
        let seer = Seer::new_with_ipc(full_chain_config, ipc_sender);

        let (global_event, expected_global) =
            create_tx_with_funding_transfer_observations(GRPC_GLOBAL_STREAM_SOURCE_LABEL);
        seer.process_event(global_event)
            .await
            .expect("global-stream funding tx must process");
        assert_forwarded_funding_transfer(
            &mut ipc_receiver,
            expected_global[0].0,
            expected_global[0].1,
            expected_global[0].2,
            false,
            ipc::FundingTransferProvenance::filtered_grpc_global_stream_live(),
            Some(0),
            Some(0),
            None,
            None,
        )
        .await;
        assert_forwarded_funding_transfer(
            &mut ipc_receiver,
            expected_global[1].0,
            expected_global[1].1,
            expected_global[1].2,
            false,
            ipc::FundingTransferProvenance::filtered_grpc_global_stream_live(),
            Some(1),
            Some(0),
            Some(0),
            Some(1),
        )
        .await;
        assert!(
            ipc_receiver.try_recv().is_err(),
            "global stream transfer-only tx should emit only filtered funding events"
        );

        let (full_chain_event, expected_full_chain) =
            create_tx_with_funding_transfer_observations(GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL);
        seer.process_event(full_chain_event)
            .await
            .expect("full-chain funding tx must process");
        assert_forwarded_funding_transfer(
            &mut ipc_receiver,
            expected_full_chain[0].0,
            expected_full_chain[0].1,
            expected_full_chain[0].2,
            true,
            ipc::FundingTransferProvenance::authoritative_full_feed_live(),
            Some(0),
            Some(0),
            None,
            None,
        )
        .await;
        assert_forwarded_funding_transfer(
            &mut ipc_receiver,
            expected_full_chain[1].0,
            expected_full_chain[1].1,
            expected_full_chain[1].2,
            true,
            ipc::FundingTransferProvenance::authoritative_full_feed_live(),
            Some(1),
            Some(0),
            Some(0),
            Some(1),
        )
        .await;
        assert!(
            ipc_receiver.try_recv().is_err(),
            "full-chain funding tx should emit only authoritative funding events"
        );

        let (pump_filtered_mismatch_event, _) = create_tx_with_funding_transfer_observations(
            GRPC_FUNDING_LANE_PUMP_FILTERED_SOURCE_LABEL,
        );
        seer.process_event(pump_filtered_mismatch_event)
            .await
            .expect("mismatched dedicated filtered funding tx must process");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), ipc_receiver.recv())
                .await
                .is_err(),
            "dedicated filtered lane must stay inactive when funding_lane_mode=full_chain"
        );

        let (pool_stream_event, _) =
            create_tx_with_funding_transfer_observations("grpc_pool_stream");
        seer.process_event(pool_stream_event)
            .await
            .expect("pool-stream funding tx must process");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), ipc_receiver.recv())
                .await
                .is_err(),
            "pool-local stream must not emit funding observations"
        );

        let (backfill_event, _) = create_tx_with_funding_transfer_observations("grpc_backfill");
        seer.process_event(backfill_event)
            .await
            .expect("backfill funding tx must process");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), ipc_receiver.recv())
                .await
                .is_err(),
            "grpc_backfill must not emit funding observations"
        );

        let mut pump_filtered_config = SeerConfig::default();
        pump_filtered_config.funding_lane_mode = FundingLaneMode::PumpFiltered;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config.clone());
        let seer = Seer::new_with_ipc(pump_filtered_config, ipc_sender);

        let (pump_filtered_event, expected_filtered) = create_tx_with_funding_transfer_observations(
            GRPC_FUNDING_LANE_PUMP_FILTERED_SOURCE_LABEL,
        );
        seer.process_event(pump_filtered_event)
            .await
            .expect("pump-filtered funding tx must process");
        assert_forwarded_funding_transfer(
            &mut ipc_receiver,
            expected_filtered[0].0,
            expected_filtered[0].1,
            expected_filtered[0].2,
            false,
            ipc::FundingTransferProvenance::funding_lane_pump_filtered_live(),
            Some(0),
            Some(0),
            None,
            None,
        )
        .await;
        assert_forwarded_funding_transfer(
            &mut ipc_receiver,
            expected_filtered[1].0,
            expected_filtered[1].1,
            expected_filtered[1].2,
            false,
            ipc::FundingTransferProvenance::funding_lane_pump_filtered_live(),
            Some(1),
            Some(0),
            Some(0),
            Some(1),
        )
        .await;
        assert!(
            ipc_receiver.try_recv().is_err(),
            "pump-filtered funding lane should emit only filtered dedicated funding events"
        );

        let (full_chain_mismatch_event, _) =
            create_tx_with_funding_transfer_observations(GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL);
        seer.process_event(full_chain_mismatch_event)
            .await
            .expect("mismatched authoritative funding tx must process");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), ipc_receiver.recv())
                .await
                .is_err(),
            "authoritative lane must stay inactive when funding_lane_mode=pump_filtered"
        );

        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);
        let (disabled_full_chain_event, _) =
            create_tx_with_funding_transfer_observations(GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL);
        seer.process_event(disabled_full_chain_event)
            .await
            .expect("disabled authoritative funding tx must process");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), ipc_receiver.recv())
                .await
                .is_err(),
            "disabled funding_lane_mode must not emit dedicated funding observations"
        );
    }

    #[tokio::test]
    async fn test_dedicated_funding_lane_skips_trade_detection_and_buffering() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let mut config = SeerConfig::default();
        config.funding_lane_mode = FundingLaneMode::FullChain;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(config, ipc_sender);

        let signature = Signature::new_unique();
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let creator = Keypair::new().pubkey();
        let event = create_tx_with_cpi_create_and_trade(
            signature,
            mint,
            curve,
            creator,
            GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL,
        );
        seer.process_event(event)
            .await
            .expect("dedicated funding-lane trade tx must process");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), ipc_receiver.recv())
                .await
                .is_err(),
            "dedicated funding lane must not forward PoolDetected or Trade events"
        );
    }

    /// Coverage semantics: replaying two TradeEvents from the same tx signature must
    /// increment the event-level replay counter twice but the signature-level counter only once.
    #[tokio::test]
    async fn test_coverage_replay_two_events_one_signature() {
        let _guard = coverage_test_lock().lock().await;
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        // Two events that share the same signature (simulating a tx that produces two TradeEvents).
        let shared_sig = Signature::new_unique();
        let mut trade1 = test_trade(pool, Pubkey::default());
        trade1.signature = shared_sig;
        let mut trade2 = test_trade(pool, Pubkey::default());
        trade2.signature = shared_sig;

        let ev_replay_before = seer
            .coverage
            .trade_events_forwarded_replay_total
            .load(Relaxed);
        let sig_replay_before = seer
            .coverage
            .trade_signatures_forwarded_replay_total
            .load(Relaxed);

        // Buffer both trades.
        seer.handle_trade_event(trade1, "test", true).await;
        seer.handle_trade_event(trade2, "test", true).await;

        // Replay triggered by mapping registration.
        seer.register_curve_mapping(pool, mint, "create", true)
            .await;
        // Drain the two replayed events.
        let _ = ipc_receiver.recv().await;
        let _ = ipc_receiver.recv().await;

        let ev_replay_after = seer
            .coverage
            .trade_events_forwarded_replay_total
            .load(Relaxed);
        let sig_replay_after = seer
            .coverage
            .trade_signatures_forwarded_replay_total
            .load(Relaxed);

        // Event counter must increase by 2.
        assert_eq!(
            ev_replay_after - ev_replay_before,
            2,
            "event-level replay counter must be 2"
        );
        // Signature counter must increase by 1 (same signature for both events).
        assert_eq!(
            sig_replay_after - sig_replay_before,
            1,
            "signature-level replay counter must be 1"
        );
    }

    /// Coverage semantics: a buffered trade must NOT be counted as forwarded until it is actually
    /// replayed.  Before the mapping arrives the event counters must stay at zero.
    #[tokio::test]
    async fn test_coverage_buffered_not_counted_as_forwarded() {
        let pool = Pubkey::new_unique();
        let trade = test_trade(pool, Pubkey::default());

        let ipc_config = IpcChannelConfig::default();
        let (ipc_sender, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let ev_live_before = seer
            .coverage
            .trade_events_forwarded_live_total
            .load(Relaxed);
        let ev_replay_before = seer
            .coverage
            .trade_events_forwarded_replay_total
            .load(Relaxed);
        let buffered_before = seer.coverage.pending_mapping_buffered_total.load(Relaxed);

        // Trade arrives before mapping — should be buffered.
        seer.handle_trade_event(trade, "test", true).await;

        assert_eq!(
            seer.coverage.pending_mapping_buffered_total.load(Relaxed) - buffered_before,
            1,
            "buffered counter must increase"
        );
        assert_eq!(
            seer.coverage
                .trade_events_forwarded_live_total
                .load(Relaxed)
                - ev_live_before,
            0,
            "buffered trade must not increment live-forwarded counter"
        );
        assert_eq!(
            seer.coverage
                .trade_events_forwarded_replay_total
                .load(Relaxed)
                - ev_replay_before,
            0,
            "buffered trade must not increment replay-forwarded counter"
        );
    }

    /// Coverage semantics: an expired pending trade must increment the expired counter and must
    /// NOT increment any forwarded counter.
    #[tokio::test]
    async fn test_coverage_expired_trade_increments_expired_not_forwarded() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let ipc_config = IpcChannelConfig::default();
        let (ipc_sender, _ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        // Manually insert an already-expired pending trade.
        // The extra 1s ensures `queued_at + PENDING_TRADE_TTL` is strictly in the past even with
        // Instant granularity differences between platforms.
        let expired_trade = PendingTrade {
            trade: test_trade(pool, Pubkey::default()),
            source_label: "test".to_string(),
            is_coverage_source: true,
            queued_at: Instant::now() - PENDING_TRADE_TTL - Duration::from_secs(1),
            reason: PendingTradeReason::CurveMappingMissing,
        };
        seer.pending_trades
            .write()
            .entry(PendingTradeKey::ByCurve(pool.to_bytes()))
            .or_default()
            .push_back(expired_trade);

        let expired_before = seer.coverage.trade_events_expired_total.load(Relaxed);
        let ev_replay_before = seer
            .coverage
            .trade_events_forwarded_replay_total
            .load(Relaxed);

        // Trigger replay — the trade is expired so it should be dropped, not forwarded.
        seer.register_curve_mapping(pool, mint, "create", true)
            .await;

        assert_eq!(
            seer.coverage.trade_events_expired_total.load(Relaxed) - expired_before,
            1,
            "expired counter must increase by 1"
        );
        assert_eq!(
            seer.coverage
                .trade_events_forwarded_replay_total
                .load(Relaxed)
                - ev_replay_before,
            0,
            "expired trade must not increment replay-forwarded counter"
        );
    }

    /// Coverage semantics: live forwarded events and replay forwarded events are tracked separately.
    #[tokio::test]
    async fn test_coverage_live_and_replay_counters_are_separate() {
        let _guard = coverage_test_lock().lock().await;
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool_live = Pubkey::new_unique();
        let mint_live = Pubkey::new_unique();
        let pool_replay = Pubkey::new_unique();
        let mint_replay = Pubkey::new_unique();

        // Pre-register the live pool so the trade can be forwarded immediately.
        seer.register_curve_mapping(pool_live, mint_live, "create", true)
            .await;
        // Drain any events that may have been produced by the replay triggered by the
        // registration above.  Because no pending trades exist yet the channel is likely
        // empty — the timeout is expected to expire without a message and is intentionally
        // ignored (we only care that the counter baseline is captured after the flush).
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(10), ipc_receiver.recv()).await;

        let ev_live_before = seer
            .coverage
            .trade_events_forwarded_live_total
            .load(Relaxed);
        let ev_replay_before = seer
            .coverage
            .trade_events_forwarded_replay_total
            .load(Relaxed);
        let sig_replay_before = seer
            .coverage
            .trade_signatures_forwarded_replay_total
            .load(Relaxed);

        // Live trade (pool already known): emitted immediately via emit_trade_only.
        let live_trade = test_trade(pool_live, mint_live);
        seer.handle_trade_event(live_trade, "test", true).await;
        let _ = ipc_receiver.recv().await;

        // Replay trade (pool registered after the trade is buffered).
        let replay_trade = test_trade(pool_replay, Pubkey::default());
        seer.handle_trade_event(replay_trade, "test", true).await;
        seer.register_curve_mapping(pool_replay, mint_replay, "create", true)
            .await;
        let _ = ipc_receiver.recv().await;

        // Event-level counters are tracked in emit_trade_only (live) and
        // replay_pending_trades_from_state (replay) — both reachable from handle_trade_event.
        assert_eq!(
            seer.coverage
                .trade_events_forwarded_live_total
                .load(Relaxed)
                - ev_live_before,
            1,
            "exactly one live event must be counted"
        );
        assert_eq!(
            seer.coverage
                .trade_events_forwarded_replay_total
                .load(Relaxed)
                - ev_replay_before,
            1,
            "exactly one replay event must be counted"
        );
        // Signature-level replay counter is tracked inside replay_pending_trades_from_state.
        assert_eq!(
            seer.coverage
                .trade_signatures_forwarded_replay_total
                .load(Relaxed)
                - sig_replay_before,
            1,
            "exactly one replay signature must be counted"
        );
        // Signature-level live counter is incremented at the process_event level (per-tx outer
        // scope, not per-event), so it is not asserted here where handle_trade_event is called
        // directly. Its correctness is covered by the binary-parser and PumpPortal paths.
    }

    #[tokio::test]
    async fn test_seer_creation() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(100);
        let seer = Seer::new(config, tx);

        assert!(seer.config.filter.enable_pumpfun);
    }

    #[tokio::test]
    async fn entry_anchor_processing_returns_when_scan_queue_is_full() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(8);
        let seer = Seer::new(config, tx);
        let scan_tx = seer
            .entry_cpi_scan_tx
            .as_ref()
            .expect("geyser modes should provision entry scan queue");

        for slot in 0..ENTRY_CPI_SCAN_QUEUE_CAP {
            scan_tx
                .try_send(EntryCpiScanJob {
                    slot: slot as u64,
                    raw: vec![0u8; 8],
                })
                .expect("scan queue fill should succeed");
        }

        let before = seer.coverage.entry_anchor_tx_total.load(Relaxed);
        tokio::time::timeout(
            Duration::from_millis(50),
            seer.process_event(types::GeyserEvent::EntryAnchor {
                slot: 77,
                executed_transaction_count: 17,
                raw: vec![1u8; 4_096],
            }),
        )
        .await
        .expect("entry processing must not block on best-effort scan queue")
        .expect("entry processing should succeed");

        assert_eq!(
            seer.coverage.entry_anchor_tx_total.load(Relaxed),
            before + 17
        );
    }

    #[tokio::test]
    async fn test_ultrafast_mode_toggles_with_backpressure() {
        let mut config = SeerConfig::default();
        config.channel_buffer_size = 2;

        let (tx, mut rx) = mpsc::channel(config.channel_buffer_size);
        let tx_clone = tx.clone();
        let seer = Seer::new(config, tx);

        let candidate = CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            tx_index: None,
            event_ts_ms: None,
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: "sig1".to_string(),
            amm_program_id: Pubkey::new_unique(),
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 0,
            bonding_curve_progress: None,
            initial_liquidity_sol: None,
            token_total_supply: None,
            block_time: None,
        };

        tx_clone.try_send(candidate.clone()).unwrap();
        tx_clone.try_send(candidate).unwrap();

        assert!(seer.update_ultrafast_mode());

        // Drain one message to reduce utilization below hysteresis threshold
        rx.recv().await.unwrap();
        assert!(!seer.update_ultrafast_mode());
    }

    #[tokio::test]
    async fn test_create_sets_curve_mapping() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(10);
        let seer = Seer::new(config, tx);

        let signature = Signature::new_unique();
        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let pumpfun_program_id = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("valid pumpfun id");
        let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112")
            .expect("valid sol mint");

        let pool = types::InitializePoolEvent {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature,
            amm_program_id: pumpfun_program_id,
            pool_amm_id: curve,
            base_mint: mint,
            quote_mint: sol_mint,
            bonding_curve: curve,
            creator: Pubkey::new_unique(),
            initial_virtual_token_reserves: None,
            initial_virtual_sol_reserves: None,
            initial_real_token_reserves: None,
            initial_real_sol_reserves: None,
            token_total_supply: None,
            block_time: Some(1),
            raw_data: vec![],
        };

        let payload = bincode::serialize(&types::SyntheticPayload::InitializePool(pool))
            .expect("serialize synthetic pool");
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature,
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: Pubkey::new_unique(),
                account_indices: vec![],
                data: payload,
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: true,
            source: "pumpportal".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        seer.process_event(event).await.unwrap();

        assert_eq!(
            seer.curve_to_mint.read().get(&curve.to_bytes()).copied(),
            Some(mint.to_bytes())
        );
    }

    #[tokio::test]
    async fn test_wal_records_raw_and_parsed_synthetic_trade() {
        let mut config = SeerConfig::default();
        config.source_mode = Some(crate::config::SeerSourceMode::PumpPortalWs);

        let (ipc_sender, _ipc_receiver, _metrics) = create_ipc_channel(IpcChannelConfig::default());
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Arc::new(Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init"));
        let seer = Seer::new_with_ipc(config, ipc_sender).with_wal(Arc::clone(&wal));

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        seer.set_curve_mapping(curve, mint, "test", true);

        let trade = test_trade(curve, mint);
        let payload = bincode::serialize(&types::SyntheticPayload::Trade(trade.clone()))
            .expect("serialize synthetic trade");

        seer.process_event(types::GeyserEvent::Transaction {
            slot: Some(7),
            event_ts_ms: Some(11_111),
            arrival_ts_ms: Some(11_222),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: trade.signature,
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: Pubkey::new_unique(),
                account_indices: vec![],
                data: payload,
            }],
            logs: vec![],
            block_time: None,
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: true,
            source: "pumpportal".to_string(),
            mpcf_payload_bytes: Some(vec![1, 2, 3, 4]),
            mpcf_payload_missing_reason: types::RawBytesMissingReason::NotMissing,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        })
        .await
        .expect("synthetic trade should process");

        wal.flush().expect("wal flush");
        let mut records = Vec::new();
        wal.replay_all(|record| records.push(record))
            .expect("wal replay");

        assert_eq!(records.len(), 2, "expected raw+parsed WAL records");
        assert!(matches!(
            &records[0],
            WalRecord::RawTx {
                slot,
                signature,
                raw_tx,
                ..
            } if *slot == 7
                && signature.as_ref() == Some(&trade.signature.as_ref().to_vec())
                && raw_tx == &vec![1, 2, 3, 4]
        ));
        assert!(matches!(
            &records[1],
            WalRecord::ParsedEvent {
                slot,
                pool_id: Some(pool_id),
                kind: WalParsedEventKind::Buy { lamports, token_amount },
                ..
            } if *slot == trade.slot.unwrap_or_default()
                && *pool_id == curve.to_bytes().to_vec()
                && *lamports == trade.max_sol_cost
                && *token_amount == trade.amount as u128
        ));
    }

    #[tokio::test]
    async fn test_account_update_uses_curve_mapping() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        seer.curve_to_mint
            .write()
            .insert(bonding_curve.to_bytes(), base_mint.to_bytes());

        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&500u64.to_le_bytes());
        data[8..16].copy_from_slice(&1_000u64.to_le_bytes());

        let owner = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("valid pumpfun id");
        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 42,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: bonding_curve,
            data,
            owner,
        })
        .await
        .unwrap();

        assert!(
            ledger.get_curve_info(&bonding_curve).is_none(),
            "AccountUpdate resolved through curve_to_mint must not write ShadowLedger"
        );

        let Some(SeerEvent::AccountUpdate(event)) = rx.recv().await else {
            panic!("expected AccountUpdate to be forwarded over IPC");
        };
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.bonding_curve, bonding_curve);
        assert_eq!(event.slot, 42);
        assert_eq!(event.token_reserves, 500);
        assert_eq!(event.sol_reserves, 1_000);
    }

    #[tokio::test]
    async fn test_account_update_without_mapping_queues_resolve() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(10);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_shadow_ledger(config, tx, Arc::clone(&ledger));

        let curve = Pubkey::new_unique();
        let owner = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("valid pumpfun id");
        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&700u64.to_le_bytes());
        data[8..16].copy_from_slice(&900u64.to_le_bytes());

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 55,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(1_055), Some(55)),
            write_version: None,
            pubkey: curve,
            data,
            owner,
        })
        .await
        .unwrap();

        let mut newer = vec![0u8; 56];
        newer[0..8].copy_from_slice(&701u64.to_le_bytes());
        newer[8..16].copy_from_slice(&901u64.to_le_bytes());
        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 56,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(1_056), Some(56)),
            write_version: None,
            pubkey: curve,
            data: newer,
            owner,
        })
        .await
        .unwrap();

        assert!(seer
            .pending_curve_updates
            .read()
            .contains_key(&curve.to_bytes()));
        assert_eq!(
            seer.pending_curve_updates
                .read()
                .get(&curve.to_bytes())
                .map(|p| (p.earliest.slot, p.latest.slot)),
            Some((55, 56))
        );
        assert!(ledger.get_curve_info(&curve).is_none());
    }

    #[tokio::test]
    async fn test_account_update_replay_preserves_earliest_and_latest_snapshot() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let owner = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("valid pumpfun id");

        let mut first = vec![0u8; 56];
        first[0..8].copy_from_slice(&700u64.to_le_bytes());
        first[8..16].copy_from_slice(&900u64.to_le_bytes());
        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 55,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(2_055), Some(55)),
            write_version: Some(1),
            pubkey: curve,
            data: first,
            owner,
        })
        .await
        .unwrap();

        let mut second = vec![0u8; 56];
        second[0..8].copy_from_slice(&701u64.to_le_bytes());
        second[8..16].copy_from_slice(&901u64.to_le_bytes());
        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 56,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(2_056), Some(56)),
            write_version: Some(2),
            pubkey: curve,
            data: second,
            owner,
        })
        .await
        .unwrap();

        seer.register_curve_mapping(curve, mint, "test", true).await;

        let Some(SeerEvent::AccountUpdate(first_replay)) = rx.recv().await else {
            panic!("expected first replayed AccountUpdate over IPC");
        };
        let Some(SeerEvent::AccountUpdate(second_replay)) = rx.recv().await else {
            panic!("expected second replayed AccountUpdate over IPC");
        };

        assert_eq!(
            (
                first_replay.slot,
                first_replay.write_version,
                first_replay.event_time.ingress_wall_ts_ms,
                first_replay.token_reserves,
                first_replay.sol_reserves,
                first_replay.replay_origin
            ),
            (
                55,
                Some(1),
                Some(2_055),
                700,
                900,
                AccountUpdateReplayOrigin::PendingReplay
            )
        );
        assert_eq!(
            (
                second_replay.slot,
                second_replay.write_version,
                second_replay.event_time.ingress_wall_ts_ms,
                second_replay.token_reserves,
                second_replay.sol_reserves,
                second_replay.replay_origin
            ),
            (
                56,
                Some(2),
                Some(2_056),
                701,
                901,
                AccountUpdateReplayOrigin::PendingReplay
            )
        );
        assert!(ledger.get_curve_info(&curve).is_none());
    }

    #[tokio::test]
    async fn test_pumpswap_account_update_without_mapping_self_resolves_token_mint() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let pool = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let state = binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 9,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: Pubkey::from_str("So11111111111111111111111111111111111111112")
                .expect("valid wrapped SOL mint")
                .to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 900,
            quote_amount: 2_000,
        };
        let mut data = binary_parser::DISC_AMM_POOL.to_vec();
        data.push(state.pool_bump);
        data.extend_from_slice(&state.index.to_le_bytes());
        data.extend_from_slice(&state.creator);
        data.extend_from_slice(&state.base_mint);
        data.extend_from_slice(&state.quote_mint);
        data.extend_from_slice(&state.lp_mint);
        data.extend_from_slice(&state.pool_base_token_account);
        data.extend_from_slice(&state.pool_quote_token_account);
        data.extend_from_slice(&state.base_amount.to_le_bytes());
        data.extend_from_slice(&state.quote_amount.to_le_bytes());
        let owner = Pubkey::from_str("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA")
            .expect("valid pumpswap program");

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 55,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: Some(2),
            pubkey: pool,
            data,
            owner,
        })
        .await
        .unwrap();

        let Some(SeerEvent::AccountUpdate(event)) = rx.recv().await else {
            panic!("expected PumpSwap AccountUpdate over IPC");
        };
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.bonding_curve, pool);
        assert_eq!(event.sol_reserves, state.quote_amount);
        assert_eq!(event.token_reserves, state.base_amount);
        assert_eq!(event.complete, 1);
        assert_eq!(seer.lookup_curve_mint(pool), Some(base_mint));
        assert!(
            !seer
                .pending_curve_updates
                .read()
                .contains_key(&pool.to_bytes()),
            "self-resolved PumpSwap AccountUpdate must not remain buffered"
        );
    }

    #[tokio::test]
    async fn test_pumpswap_account_update_preserves_existing_observation_alias() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let observation_pool = Pubkey::new_unique();
        let pool = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        seer.set_curve_mapping(observation_pool, base_mint, "test", true);

        let state = binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 9,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: Pubkey::from_str("So11111111111111111111111111111111111111112")
                .expect("valid wrapped SOL mint")
                .to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 900,
            quote_amount: 2_000,
        };
        let mut data = binary_parser::DISC_AMM_POOL.to_vec();
        data.push(state.pool_bump);
        data.extend_from_slice(&state.index.to_le_bytes());
        data.extend_from_slice(&state.creator);
        data.extend_from_slice(&state.base_mint);
        data.extend_from_slice(&state.quote_mint);
        data.extend_from_slice(&state.lp_mint);
        data.extend_from_slice(&state.pool_base_token_account);
        data.extend_from_slice(&state.pool_quote_token_account);
        data.extend_from_slice(&state.base_amount.to_le_bytes());
        data.extend_from_slice(&state.quote_amount.to_le_bytes());
        let owner = Pubkey::from_str("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA")
            .expect("valid pumpswap program");

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 55,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: Some(2),
            pubkey: pool,
            data,
            owner,
        })
        .await
        .unwrap();

        let Some(SeerEvent::AccountUpdate(_)) = rx.recv().await else {
            panic!("expected PumpSwap AccountUpdate over IPC");
        };
        assert_eq!(seer.lookup_curve_mint(pool), Some(base_mint));
        assert_eq!(
            seer.mint_to_curve
                .read()
                .get(&base_mint.to_bytes())
                .copied(),
            Some(observation_pool.to_bytes()),
            "PumpSwap account update must keep the existing observation alias authoritative"
        );
    }

    #[tokio::test]
    async fn test_account_update_forwards_primary_account_state_event() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        seer.tracked_curves.write().insert(bonding_curve, base_mint);

        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 500,
            virtual_sol_reserves: 1_000,
            real_token_reserves: 400,
            real_sol_reserves: 900,
            token_total_supply: 1_500,
            complete: 1,
            _padding: [0; 7],
        };

        // Budujemy dane ręcznie zgodnie z nowym parserem (56-bajtowy layout, off=0):
        // virtual_token_reserves zaczyna się od offsetu 0 (brak Anchor discriminatora).
        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&curve.virtual_token_reserves.to_le_bytes());
        data[8..16].copy_from_slice(&curve.virtual_sol_reserves.to_le_bytes());
        data[16..24].copy_from_slice(&curve.real_token_reserves.to_le_bytes());
        data[24..32].copy_from_slice(&curve.real_sol_reserves.to_le_bytes());
        data[32..40].copy_from_slice(&curve.token_total_supply.to_le_bytes());
        data[40] = curve.complete;

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 42,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: bonding_curve,
            data,
            owner: Pubkey::new_unique(),
        })
        .await
        .unwrap();

        assert!(
            ledger.get_curve_info(&bonding_curve).is_none(),
            "AccountUpdate must not write ShadowLedger on the primary canonical ingest path"
        );

        let Some(SeerEvent::AccountUpdate(event)) = rx.recv().await else {
            panic!("expected AccountUpdate to be forwarded over IPC");
        };
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.bonding_curve, bonding_curve);
        assert_eq!(event.sol_reserves, curve.virtual_sol_reserves);
        assert_eq!(event.token_reserves, curve.virtual_token_reserves);
        assert_eq!(event.complete, curve.complete);
        assert_eq!(event.slot, 42);
    }

    #[tokio::test]
    async fn test_pumpswap_account_update_forwards_primary_account_state_event() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let pool = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        seer.tracked_curves.write().insert(pool, base_mint);

        let state = binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 7,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: wsol_mint_pubkey().to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 500,
            quote_amount: 1_000,
        };
        let mut data = binary_parser::DISC_AMM_POOL.to_vec();
        data.push(state.pool_bump);
        data.extend_from_slice(&state.index.to_le_bytes());
        data.extend_from_slice(&state.creator);
        data.extend_from_slice(&state.base_mint);
        data.extend_from_slice(&state.quote_mint);
        data.extend_from_slice(&state.lp_mint);
        data.extend_from_slice(&state.pool_base_token_account);
        data.extend_from_slice(&state.pool_quote_token_account);
        data.extend_from_slice(&state.base_amount.to_le_bytes());
        data.extend_from_slice(&state.quote_amount.to_le_bytes());

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 43,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: Some(1),
            pubkey: pool,
            data,
            owner: Pubkey::from_str("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA")
                .expect("valid pumpswap program"),
        })
        .await
        .unwrap();

        assert!(
            ledger.get_curve_info(&pool).is_none(),
            "PumpSwap AccountUpdate must not write ShadowLedger on the primary canonical ingest path"
        );

        let Some(SeerEvent::AccountUpdate(event)) = rx.recv().await else {
            panic!("expected PumpSwap AccountUpdate to be forwarded over IPC");
        };
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.bonding_curve, pool);
        assert_eq!(event.sol_reserves, state.quote_amount);
        assert_eq!(event.token_reserves, state.base_amount);
        assert_eq!(event.complete, 1);
        assert_eq!(event.slot, 43);
        assert_eq!(event.write_version, Some(1));
    }

    #[tokio::test]
    async fn pumpswap_account_update_before_mapping_replays() {
        // Tests that a pump.fun BondingCurve AccountUpdate arriving before the
        // curve→mint mapping is known is buffered in `pending_curve_updates` and
        // replayed once `register_curve_mapping` fires. PumpSwap AmmPool events
        // self-resolve their own mint from account data; this test exercises the
        // buffering path for BondingCurve accounts that carry no embedded mint.
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let curve_pubkey = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        // BondingCurve data (56-byte layout, no embedded mint → requires external mapping).
        let virtual_token: u64 = 500_000_000_000_000;
        let virtual_sol: u64 = 30_000_000_000;
        let real_token: u64 = 206_900_000_000_000;
        let real_sol: u64 = 0;
        let total_supply: u64 = 1_000_000_000_000_000;
        let complete: u8 = 0;

        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&virtual_token.to_le_bytes());
        data[8..16].copy_from_slice(&virtual_sol.to_le_bytes());
        data[16..24].copy_from_slice(&real_token.to_le_bytes());
        data[24..32].copy_from_slice(&real_sol.to_le_bytes());
        data[32..40].copy_from_slice(&total_supply.to_le_bytes());
        data[40] = complete;

        let pump_program = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("valid pump.fun program");

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 55,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: Some(2),
            pubkey: curve_pubkey,
            data,
            owner: pump_program,
        })
        .await
        .unwrap();

        assert!(
            seer.pending_curve_updates
                .read()
                .contains_key(&curve_pubkey.to_bytes()),
            "BondingCurve AccountUpdate must be buffered before mapping is known"
        );

        seer.register_curve_mapping(curve_pubkey, base_mint, "create", true)
            .await;

        let Some(SeerEvent::AccountUpdate(event)) = rx.recv().await else {
            panic!("expected replayed BondingCurve AccountUpdate over IPC");
        };
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.bonding_curve, curve_pubkey);
        assert_eq!(event.sol_reserves, virtual_sol);
        assert_eq!(event.token_reserves, virtual_token);
        assert_eq!(event.complete, complete);
        assert_eq!(
            event.replay_origin,
            AccountUpdateReplayOrigin::PendingReplay
        );
        assert!(event.replay_buffer_dwell_ms.is_some());
        assert!(
            !seer
                .pending_curve_updates
                .read()
                .contains_key(&curve_pubkey.to_bytes()),
            "pending_curve_updates must drain after replay"
        );
    }

    /// Regression: in tx-only mode (`canonical_account_update_relay_enabled = false`) a
    /// `GeyserEvent::AccountUpdate` must be dropped before touching ShadowLedger.
    ///
    /// Verifies that even with a valid curve→mint mapping, no write reaches the
    /// ShadowLedger and the IPC channel receives no AccountUpdate message.
    #[tokio::test]
    async fn test_handle_account_update_noop_in_tx_only_mode() {
        let mut config = SeerConfig::default();
        config.canonical_account_update_relay_enabled = false; // tx-only

        let (candidate_tx, _rx) = mpsc::channel(10);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_shadow_ledger(config, candidate_tx, Arc::clone(&ledger));

        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        // Seed the curve→mint mapping so handle_account_update would normally proceed
        seer.tracked_curves.write().insert(bonding_curve, base_mint);

        // Build a valid 56-byte bonding curve payload
        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&500u64.to_le_bytes()); // virtual_token_reserves
        data[8..16].copy_from_slice(&1_000u64.to_le_bytes()); // virtual_sol_reserves
        data[16..24].copy_from_slice(&400u64.to_le_bytes()); // real_token_reserves
        data[24..32].copy_from_slice(&900u64.to_le_bytes()); // real_sol_reserves
        data[32..40].copy_from_slice(&1_500u64.to_le_bytes()); // token_total_supply
        data[40] = 1; // complete

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 99,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: bonding_curve,
            data,
            owner: Pubkey::new_unique(),
        })
        .await
        .expect("process_event must not error");

        // ShadowLedger must NOT be written in tx-only mode
        assert!(
            ledger.get_curve_info(&bonding_curve).is_none(),
            "ShadowLedger must not be written in tx-only mode (canonical_account_update_relay_enabled=false)"
        );
    }

    #[tokio::test]
    async fn registered_bcv2_account_update_emits_evidence_before_canonical_decode() {
        let (ipc_sender, mut ipc_receiver, _metrics) =
            create_ipc_channel(IpcChannelConfig::default());
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let account_pubkey = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        seer.grpc_connection
            .as_ref()
            .expect("default seer config should create grpc connection")
            .account_registry()
            .insert_bcv2_with_context(Bcv2AccountContext {
                account_pubkey,
                base_mint: Some(base_mint),
                pool_id: Some(pool_id),
                canonical_bonding_curve: Some(pool_id),
                tx_signature: Some("bcv2-sig".to_string()),
                observed_instruction_index: Some(4),
                observed_account_position: Some(16),
                provenance_status: Some("route_compatible".to_string()),
                observed_slot: Some(41),
            });

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 99,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: Some(7),
            pubkey: account_pubkey,
            data: vec![1, 2, 3],
            owner,
        })
        .await
        .expect("account update processing must not fail");

        let event = tokio::time::timeout(Duration::from_millis(250), ipc_receiver.recv())
            .await
            .expect("bcv2 evidence should be emitted before canonical decode")
            .expect("IPC channel should remain open");

        match event {
            SeerEvent::ExecutionAccountEvidence(event) => {
                assert_eq!(event.evidence.role, ExecutionAccountRole::BondingCurveV2);
                assert_eq!(event.evidence.account_pubkey, account_pubkey);
                assert_eq!(event.evidence.base_mint, Some(base_mint));
                assert_eq!(event.evidence.pool_id, Some(pool_id));
                assert_eq!(event.evidence.canonical_bonding_curve, Some(pool_id));
                assert_eq!(
                    event.evidence.source,
                    ExecutionAccountEvidenceSource::YellowstoneAccountUpdate
                );
                assert_eq!(
                    event.evidence.status,
                    ExecutionAccountEvidenceStatus::AccountUpdateReceived
                );
                assert!(event.evidence.evidence_ready);
                assert_eq!(event.evidence.slot, Some(99));
                assert_eq!(event.evidence.write_version, Some(7));
                assert_eq!(event.evidence.owner, Some(owner));
                assert_eq!(event.evidence.data_len, Some(3));
            }
            SeerEvent::AccountUpdate(_) => {
                panic!("BCV2 evidence must not be routed through AccountUpdate")
            }
            other => panic!("expected ExecutionAccountEvidence, got {:?}", other),
        }

        assert!(
            ipc_receiver.try_recv().is_err(),
            "canonical decode failure should not emit AccountUpdate"
        );
    }

    #[test]
    fn test_store_curve_with_snapshots_seed_curve_uses_reserved_slot_only() {
        let ledger = ShadowLedger::new();
        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let curve = genesis_curve();

        store_curve_with_snapshots(
            &ledger,
            bonding_curve,
            base_mint,
            curve,
            None,
            false, // seed curve, not account-update confirmed
        );

        let info = ledger
            .get_curve_info(&bonding_curve)
            .expect("curve info should be stored");
        assert_eq!(info.last_updated_slot, 0);

        let (_, curve_data_known) = ledger
            .get_curve_with_known(&bonding_curve)
            .expect("curve should exist");
        assert!(!curve_data_known);
        assert!(
            ledger.get_snapshots(&base_mint).is_none(),
            "seed curve path must not create canonical snapshots"
        );
    }

    #[test]
    fn test_genesis_curve_uses_shadow_ledger_protocol_genesis() {
        let seer_curve = genesis_curve();
        let protocol_curve = protocol_genesis_curve();

        assert_eq!(
            seer_curve.virtual_token_reserves,
            protocol_curve.virtual_token_reserves
        );
        assert_eq!(
            seer_curve.virtual_sol_reserves,
            protocol_curve.virtual_sol_reserves
        );
        assert_eq!(
            seer_curve.real_token_reserves,
            protocol_curve.real_token_reserves
        );
        assert_eq!(
            seer_curve.real_sol_reserves,
            protocol_curve.real_sol_reserves
        );
        assert_eq!(
            seer_curve.token_total_supply,
            protocol_curve.token_total_supply
        );
        assert_eq!(seer_curve.complete, protocol_curve.complete);
    }

    #[tokio::test]
    async fn test_account_update_rejects_short_data() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(10);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_shadow_ledger(config, tx, Arc::clone(&ledger));

        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        seer.tracked_curves.write().insert(bonding_curve, base_mint);

        // Send data that is too short (20 bytes instead of 56)
        let short_data = vec![0u8; 20];
        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 42,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: bonding_curve,
            data: short_data,
            owner: Pubkey::new_unique(),
        })
        .await
        .unwrap();

        // Curve info should NOT be stored because data was too short
        assert!(ledger.get_curve_info(&bonding_curve).is_none());
    }

    #[tokio::test]
    async fn test_account_update_accepts_longer_data() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        seer.tracked_curves.write().insert(bonding_curve, base_mint);

        // Build 151-byte data (real pump.fun layout) with valid curve fields at offsets
        let mut data = vec![0u8; 151];
        // virtual_token_reserves at offset 8
        data[8..16].copy_from_slice(&500u64.to_le_bytes());
        // virtual_sol_reserves at offset 16
        data[16..24].copy_from_slice(&1_000u64.to_le_bytes());
        // token_total_supply at offset 40
        data[40..48].copy_from_slice(&1_500u64.to_le_bytes());

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 42,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: bonding_curve,
            data,
            owner: Pubkey::new_unique(),
        })
        .await
        .unwrap();

        assert!(
            ledger.get_curve_info(&bonding_curve).is_none(),
            "long-form AccountUpdate must stay on canonical IPC path only"
        );

        let Some(SeerEvent::AccountUpdate(event)) = rx.recv().await else {
            panic!("expected AccountUpdate to be forwarded over IPC");
        };
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.bonding_curve, bonding_curve);
        assert_eq!(event.slot, 42);
        assert_eq!(event.token_reserves, 500);
        assert_eq!(event.sol_reserves, 1_000);
    }

    #[tokio::test]
    async fn test_account_update_accepts_nonzero_discriminator() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (tx, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, tx, Arc::clone(&ledger));

        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        seer.tracked_curves.write().insert(bonding_curve, base_mint);

        // Anchor-style account with non-zero discriminator at offset 0 must be accepted.
        let mut data = vec![0u8; 151];
        data[0..8].copy_from_slice(&99u64.to_le_bytes());
        data[8..16].copy_from_slice(&500u64.to_le_bytes());
        data[16..24].copy_from_slice(&1_000u64.to_le_bytes());
        data[24..32].copy_from_slice(&400u64.to_le_bytes());
        data[32..40].copy_from_slice(&900u64.to_le_bytes());
        data[40..48].copy_from_slice(&1_500u64.to_le_bytes());
        data[48] = 0;

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 42,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: bonding_curve,
            data,
            owner: Pubkey::new_unique(),
        })
        .await
        .unwrap();

        assert!(
            ledger.get_curve_info(&bonding_curve).is_none(),
            "non-zero discriminator AccountUpdate must stay on canonical IPC path only"
        );

        let Some(SeerEvent::AccountUpdate(event)) = rx.recv().await else {
            panic!("expected AccountUpdate to be forwarded over IPC");
        };
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.bonding_curve, bonding_curve);
        assert_eq!(event.slot, 42);
        assert_eq!(event.token_reserves, 500);
        assert_eq!(event.sol_reserves, 1_000);
        assert_eq!(event.complete, 0);
    }

    #[tokio::test]
    async fn test_account_update_untracked_pubkey_is_queued_for_resolve() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(10);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_shadow_ledger(config, tx, Arc::clone(&ledger));

        let unknown_pubkey = Pubkey::new_unique();
        let owner = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("valid pumpfun id");
        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&500u64.to_le_bytes());
        data[8..16].copy_from_slice(&1_000u64.to_le_bytes());
        data[16..24].copy_from_slice(&400u64.to_le_bytes());
        data[24..32].copy_from_slice(&900u64.to_le_bytes());
        data[32..40].copy_from_slice(&1_500u64.to_le_bytes());
        data[40] = 0;

        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: 42,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: unknown_pubkey,
            data,
            owner,
        })
        .await
        .unwrap();

        assert!(seer
            .pending_curve_updates
            .read()
            .contains_key(&unknown_pubkey.to_bytes()));
        assert!(ledger.get_curve_info(&unknown_pubkey).is_none());
    }

    #[test]
    fn test_should_forward_trade_requires_exact_pool_mint_mapping() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(config, tx);

        let good_pool = Pubkey::new_unique();
        let wrong_pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        seer.set_curve_mapping(good_pool, mint, "create", true);
        let good_trade = test_trade(good_pool, mint);
        let wrong_trade = test_trade(wrong_pool, mint);

        assert!(
            matches!(
                seer.should_forward_trade(&good_trade, "pumpportal", false),
                TradeForwardDecision::Forward
            ),
            "exact mapped pool/mint must be forwarded"
        );
        // With optimistic self-registration: a trade carrying a non-default pool + mint
        // is forwarded immediately (pool→mint registered non-authoritatively).
        // There is no longer a "wait for authoritative mapping" buffer path for trades
        // that carry both pool and mint — pump.fun pools are 1:1 with mints by protocol.
        assert!(
            matches!(
                seer.should_forward_trade(&wrong_trade, "pumpportal", false),
                TradeForwardDecision::ForwardWithReplay(..)
            ),
            "trade with unknown pool but valid mint: optimistic self-registration forwards immediately"
        );
        // Optimistic mapping for wrong_pool must be registered.
        assert_eq!(
            seer.lookup_curve_mint(wrong_pool),
            Some(mint),
            "optimistic mapping for previously-unknown pool must be registered from trade mint"
        );
    }

    #[test]
    fn test_should_forward_grpc_trade_even_when_pool_not_watched() {
        let mut config = SeerConfig::default();
        config.stream_mode = StreamMode::SingleGlobal;
        config.tx_filter_strategy = TxFilterStrategy::PerPool;
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(config, tx);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        seer.set_curve_mapping(pool, mint, "create", true);
        let trade = test_trade(pool, mint);

        assert!(
            matches!(
                seer.should_forward_trade(&trade, "grpc_global_stream", true),
                TradeForwardDecision::Forward
            ),
            "grpc global trades must not be filtered just because pool is not watched yet"
        );
    }

    #[test]
    fn test_should_forward_grpc_trade_on_mapping_conflict_with_known_mint() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(config, tx);

        // Scenario A: no prior mapping — trade registers optimistic mapping and forwards.
        let pool_a = Pubkey::new_unique();
        let mint_a = Pubkey::new_unique();
        let trade_a = test_trade(pool_a, mint_a);

        assert!(
            matches!(
                seer.should_forward_trade(&trade_a, "grpc_global_stream", true),
                TradeForwardDecision::ForwardWithReplay(..)
            ),
            "trade with no prior mapping: optimistic self-registration must forward immediately"
        );
        // Optimistic mapping must be registered from trade-supplied mint.
        assert_eq!(
            seer.lookup_curve_mint(pool_a),
            Some(mint_a),
            "optimistic mapping must be registered from trade-supplied mint"
        );

        // Scenario B: authoritative mapping already present with a different mint.
        // The trade still forwards (we don't suppress it) but does NOT override the
        // authoritative entry (authoritative=false never wins over authoritative=true).
        let pool_b = Pubkey::new_unique();
        let stale_mint = Pubkey::new_unique();
        let trade_mint = Pubkey::new_unique();
        seer.set_curve_mapping(pool_b, stale_mint, "create", true);
        let conflicting_trade = test_trade(pool_b, trade_mint);

        assert!(
            matches!(
                seer.should_forward_trade(&conflicting_trade, "grpc_global_stream", true),
                TradeForwardDecision::ForwardWithReplay(..)
            ),
            "trade conflicting with authoritative mapping must still forward immediately"
        );
        // Authoritative entry is preserved; non-authoritative optimistic must not win.
        assert_eq!(
            seer.lookup_curve_mint(pool_b),
            Some(stale_mint),
            "existing authoritative mapping must NOT be overridden by trade-optimistic (non-authoritative)"
        );
    }

    #[test]
    fn test_is_invalid_trade_pool_rejects_default_pubkey() {
        assert!(Seer::is_invalid_trade_pool(&Pubkey::default()));
    }

    #[test]
    fn test_parse_curve_offset_56_bytes() {
        // Stary 56-bajtowy layout bez Anchor discriminatora: virtual_token zaczyna się od offset=0.
        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&1_073_000_000_000_000u64.to_le_bytes()); // virtual_token @ off=0
        data[8..16].copy_from_slice(&30_000_000_000u64.to_le_bytes()); // virtual_sol
        data[16..24].copy_from_slice(&793_100_000_000_000u64.to_le_bytes()); // real_token
        data[24..32].copy_from_slice(&30_000_000_000u64.to_le_bytes()); // real_sol
        data[32..40].copy_from_slice(&1_000_000_000_000_000u64.to_le_bytes()); // supply
                                                                               // complete at [40] defaults to 0
        let parsed = parse_curve_from_account(&data).unwrap();
        assert_eq!(parsed.virtual_token_reserves, 1_073_000_000_000_000);
        assert_eq!(parsed.virtual_sol_reserves, 30_000_000_000);
        assert_eq!(parsed.complete, 0);
    }

    #[test]
    fn test_parse_curve_offset_151_bytes() {
        let mut data = vec![0u8; 151];
        // discriminator at offset 0 (Anchor-style, non-zero)
        data[0..8].copy_from_slice(&12345u64.to_le_bytes());
        data[8..16].copy_from_slice(&500u64.to_le_bytes());
        data[16..24].copy_from_slice(&1_000u64.to_le_bytes());
        data[24..32].copy_from_slice(&400u64.to_le_bytes());
        data[32..40].copy_from_slice(&900u64.to_le_bytes());
        data[40..48].copy_from_slice(&1_500u64.to_le_bytes());
        data[48] = 1; // complete

        let parsed = parse_curve_from_account(&data).unwrap();
        // parser zawsze zeruje discriminator — pole nie jest eksponowane downstream
        assert_eq!(parsed.discriminator, 0);
        assert_eq!(parsed.virtual_token_reserves, 500);
        assert_eq!(parsed.virtual_sol_reserves, 1_000);
        assert_eq!(parsed.real_token_reserves, 400);
        assert_eq!(parsed.real_sol_reserves, 900);
        assert_eq!(parsed.token_total_supply, 1_500);
        assert_eq!(parsed.complete, 1);
    }

    #[test]
    fn test_parse_curve_offset_83_bytes() {
        let mut data = vec![0u8; 83];
        data[8..16].copy_from_slice(&200u64.to_le_bytes());
        data[16..24].copy_from_slice(&300u64.to_le_bytes());
        let parsed = parse_curve_from_account(&data).unwrap();
        assert_eq!(parsed.virtual_token_reserves, 200);
        assert_eq!(parsed.virtual_sol_reserves, 300);
    }

    #[test]
    fn test_parse_curve_rejects_48_bytes() {
        let data = vec![0u8; 48];
        assert!(parse_curve_from_account(&data).is_err());
    }

    #[test]
    fn test_tx_contains_supported_trade_instruction_detects_pumpswap_exact_quote_in() {
        let pumpswap_program =
            Pubkey::from_str("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA").unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: pumpswap_program,
                account_indices: vec![],
                data: ghost_core::PUMPSWAP_BUY_EXACT_QUOTE_IN_DISCRIMINATOR.to_vec(),
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        assert!(Seer::tx_contains_supported_trade_instruction(&event));
    }

    #[test]
    fn test_tx_contains_supported_trade_instruction_detects_inner_routed_buy_under_jupiter_v6() {
        let jupiter_program =
            Pubkey::from_str("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4").unwrap();
        let pumpfun_program =
            Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![jupiter_program, pumpfun_program],
            instructions: vec![types::RawInstruction {
                program_id: jupiter_program,
                account_indices: vec![0],
                data: vec![0xAA, 0xBB, 0xCC, 0xDD],
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![types::InnerInstructionGroup {
                index: 0,
                instructions: vec![types::InnerIx {
                    program_id_index: 1,
                    accounts: vec![],
                    data: binary_parser::DISC_PUMP_BUY_ROUTED.to_vec(),
                    stack_height: Some(2),
                }],
            }],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        assert!(Seer::tx_contains_supported_trade_instruction(&event));
    }

    #[test]
    fn test_tx_contains_supported_trade_instruction_detects_top_level_jupiter_route_v2() {
        let jupiter_program =
            Pubkey::from_str("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4").unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: jupiter_program,
                account_indices: vec![],
                data: binary_parser::DISC_JUPITER_ROUTE_V2.to_vec(),
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        assert!(Seer::tx_contains_supported_trade_instruction(&event));
    }

    #[test]
    fn test_tx_contains_supported_trade_instruction_detects_top_level_dflow_swap2() {
        let dflow_program =
            Pubkey::from_str("DF1ow4tspfHX9JwWJsAb9epbkA8hmpSEAtxXy1V27QBH").unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: dflow_program,
                account_indices: vec![],
                data: binary_parser::DISC_DFLOW_SWAP2.to_vec(),
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        assert!(Seer::tx_contains_supported_trade_instruction(&event));
    }

    #[test]
    fn test_tx_contains_supported_initialize_pool_instruction_detects_pumpfun_create() {
        let pumpfun_program =
            Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: pumpfun_program,
                account_indices: vec![],
                data: binary_parser::DISC_CREATE.to_vec(),
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        assert!(Seer::tx_contains_supported_initialize_pool_instruction(
            &event
        ));
        assert!(Seer::tx_should_log_initialize_pool_miss(&event));
    }

    #[test]
    fn test_tx_should_log_initialize_pool_miss_ignores_trade_with_ata_create_logs() {
        let pumpswap_program =
            Pubkey::from_str("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA").unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: pumpswap_program,
                account_indices: vec![],
                data: binary_parser::DISC_SELL.to_vec(),
            }],
            logs: vec![
                "Program log: Create".to_string(),
                "Program log: Instruction: InitializeAccount3".to_string(),
                "Program log: Instruction: Sell".to_string(),
            ],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: "grpc_global_stream".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        assert!(Seer::tx_contains_supported_trade_instruction(&event));
        assert!(!Seer::tx_contains_supported_initialize_pool_instruction(
            &event
        ));
        assert!(!Seer::tx_should_log_initialize_pool_miss(&event));
    }

    #[tokio::test]
    async fn test_trade_before_create_recovers_mapping() {
        // Serialize with the race test to avoid interference on the shared global
        // pipeline_coverage() counters (process-wide AtomicU64 incremented by both).
        let _guard = coverage_test_lock().lock().await;
        let coverage_before = pipeline_coverage().snapshot();
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = test_trade(pool, Pubkey::default());
        let forwarded_replay_before =
            read_outcome_count(&seer.metrics.events_filtered, TradeOutcome::ForwardedReplay);

        assert!(matches!(
            seer.should_forward_trade(&trade, "grpc_global_stream", true),
            TradeForwardDecision::BufferedPendingMapping
        ));
        assert_eq!(
            seer.pending_trades
                .read()
                .get(&PendingTradeKey::ByCurve(pool.to_bytes()))
                .map(|queue| queue.len()),
            Some(1)
        );

        seer.register_curve_mapping(pool, mint, "create", true)
            .await;

        let replayed = ipc_receiver.recv().await.expect("replayed trade");
        match replayed {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.pool_amm_id, pool);
                assert_eq!(trade_event.trade.mint, mint);
            }
            other => panic!("expected replayed trade, got {:?}", other),
        }
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(pool.to_bytes())),
            "pending queue should drain after mapping replay"
        );
        assert_eq!(
            read_outcome_count(&seer.metrics.events_filtered, TradeOutcome::ForwardedReplay)
                - forwarded_replay_before,
            1.0
        );

        let delta = pipeline_coverage()
            .snapshot()
            .saturating_delta_from(&coverage_before);
        // These counters are process-global and other tests may run concurrently.
        // Assert lower bounds to avoid flakes while still validating that this
        // path records coverage.
        assert!(delta.pending_mapping_buffered >= 1);
        assert!(delta.pending_mapping_replayed >= 1);
        assert!(delta.seer_forwarded >= 1);
    }

    /// P0 regression: a CpiTrade with pool_amm_id == Pubkey::default() but a known mint
    /// must be buffered (keyed by mint) and replayed once register_curve_mapping fires —
    /// never dropped immediately as ROLE_MISMATCH.
    #[tokio::test]
    async fn test_trade_with_default_pool_and_known_mint_is_buffered_not_dropped() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Trade arrives before the pool CREATE — pool_amm_id is unknown (default).
        let trade = test_trade(Pubkey::default(), mint);
        let buffered_before = read_outcome_count(
            &seer.metrics.events_buffered,
            TradeOutcome::BufferedMissingPool,
        );

        // Should NOT be dropped; should be buffered under the mint key.
        let forwarded = seer
            .handle_trade_event(trade.clone(), "grpc_global_stream", true)
            .await;
        assert!(!forwarded, "trade must not be forwarded immediately");
        assert_eq!(
            read_outcome_count(
                &seer.metrics.events_buffered,
                TradeOutcome::BufferedMissingPool
            ) - buffered_before,
            1.0
        );

        // Buffered under ByMint key, not under ByCurve(default) key.
        let pending = seer.pending_trades.read();
        assert!(
            pending.contains_key(&PendingTradeKey::ByMint(mint.to_bytes())),
            "trade must be keyed by ByMint when pool_amm_id is default"
        );
        assert!(
            !pending.contains_key(&PendingTradeKey::ByCurve(Pubkey::default().to_bytes())),
            "trade must NOT be stored under the ByCurve(default) key"
        );
        drop(pending);

        // Once the CREATE arrives, the mapping is registered and the trade replays.
        seer.register_curve_mapping(curve, mint, "create", true)
            .await;

        let replayed = ipc_receiver.recv().await.expect("replayed trade");
        match replayed {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(
                    trade_event.trade.pool_amm_id, curve,
                    "pool_amm_id patched to curve on replay"
                );
                assert_eq!(trade_event.trade.mint, mint, "mint preserved on replay");
            }
            other => panic!("expected replayed trade, got {:?}", other),
        }

        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByMint(mint.to_bytes())),
            "ByMint bucket must drain after replay"
        );
    }

    #[tokio::test]
    async fn test_register_curve_mapping_replays_pending_trades_by_curve_and_by_mint() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let trade_by_curve = test_trade(curve, Pubkey::default());
        let trade_by_mint = test_trade(Pubkey::default(), mint);

        assert!(
            !seer
                .handle_trade_event(trade_by_curve.clone(), "grpc_global_stream", true)
                .await
        );
        assert!(
            !seer
                .handle_trade_event(trade_by_mint.clone(), "grpc_global_stream", true)
                .await
        );

        seer.register_curve_mapping(curve, mint, "create", true)
            .await;

        let mut seen_signatures = HashSet::new();
        for _ in 0..2 {
            let replayed = ipc_receiver.recv().await.expect("replayed trade");
            match replayed {
                SeerEvent::Trade(trade_event) => {
                    assert_eq!(trade_event.trade.pool_amm_id, curve);
                    assert_eq!(trade_event.trade.mint, mint);
                    seen_signatures.insert(trade_event.trade.signature);
                }
                other => panic!("expected replayed trade, got {:?}", other),
            }
        }

        assert!(seen_signatures.contains(&trade_by_curve.signature));
        assert!(seen_signatures.contains(&trade_by_mint.signature));
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(curve.to_bytes())),
            "ByCurve bucket must drain after replay"
        );
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByMint(mint.to_bytes())),
            "ByMint bucket must drain after replay"
        );
        assert!(
            ipc_receiver.try_recv().is_err(),
            "mapping replay should emit exactly the pending trades once"
        );
    }

    #[test]
    fn test_pending_trade_expiry_records_expired_outcome_not_filtered_invalid_pool() {
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(SeerConfig::default(), tx);

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = test_trade(Pubkey::default(), mint);
        let expired = PendingTrade {
            trade,
            source_label: "test".to_string(),
            is_coverage_source: false,
            queued_at: Instant::now() - PENDING_TRADE_TTL - Duration::from_secs(1),
            reason: PendingTradeReason::MissingPoolFromMint,
        };
        seer.pending_trades.write().insert(
            PendingTradeKey::ByMint(mint.to_bytes()),
            VecDeque::from([expired]),
        );

        let expired_before = read_outcome_count(
            &seer.metrics.events_filtered,
            TradeOutcome::ExpiredWaitingForMapping,
        );
        let invalid_before = read_outcome_count(
            &seer.metrics.events_filtered,
            TradeOutcome::FilteredInvalidPool,
        );

        let replay_ready = Seer::take_pending_trades_from_store(
            &seer.pending_trades,
            &seer.metrics,
            &seer.coverage,
            curve,
            mint,
        );

        assert!(
            replay_ready.is_empty(),
            "expired trades must not be replayed"
        );
        assert_eq!(
            read_outcome_count(
                &seer.metrics.events_filtered,
                TradeOutcome::ExpiredWaitingForMapping
            ) - expired_before,
            1.0
        );
        assert_eq!(
            read_outcome_count(
                &seer.metrics.events_filtered,
                TradeOutcome::FilteredInvalidPool
            ) - invalid_before,
            0.0
        );
    }

    /// P0 regression: a CpiTrade with both pool_amm_id and mint == Pubkey::default()
    /// must still be buffered (keyed by signature prefix), not dropped immediately.
    #[tokio::test]
    async fn test_trade_with_both_default_fields_is_buffered_not_dropped() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, _ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        // Both pool_amm_id and mint are default — the weakest unresolved case.
        let trade = test_trade(Pubkey::default(), Pubkey::default());

        let forwarded = seer
            .handle_trade_event(trade.clone(), "grpc_global_stream", true)
            .await;
        assert!(
            !forwarded,
            "trade with both-default fields must not be forwarded"
        );

        // Should be buffered under a BySignature key, not under any ByCurve(default) key.
        let pending = seer.pending_trades.read();
        assert!(
            !pending.contains_key(&PendingTradeKey::ByCurve(Pubkey::default().to_bytes())),
            "trade must NOT be stored under the ByCurve(default) key"
        );
        // At least one entry should exist somewhere in the map.
        assert!(
            !pending.is_empty(),
            "trade must be buffered (not simply dropped)"
        );
    }

    #[tokio::test]
    async fn test_duplicate_buffered_trade_records_dedup_outcome() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, _ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let mint = Pubkey::new_unique();
        let trade = test_trade(Pubkey::default(), mint);
        let dedup_before =
            read_outcome_count(&seer.metrics.events_filtered, TradeOutcome::DedupDropped);

        assert!(
            !seer
                .handle_trade_event(trade.clone(), "grpc_global_stream", true)
                .await
        );
        assert!(
            !seer
                .handle_trade_event(trade, "grpc_global_stream", true)
                .await
        );
        assert_eq!(
            read_outcome_count(&seer.metrics.events_filtered, TradeOutcome::DedupDropped)
                - dedup_before,
            1.0
        );
        assert_eq!(
            seer.pending_trades
                .read()
                .get(&PendingTradeKey::ByMint(mint.to_bytes()))
                .map(|queue| queue.len()),
            Some(1),
            "duplicate buffered trade must not be enqueued twice"
        );
    }

    #[tokio::test]
    async fn test_register_curve_mapping_does_not_duplicate_live_and_replayed_trade() {
        // With optimistic self-registration, a live trade (pool+mint both known)
        // registers the mapping and triggers replay of any buffered trades.
        //
        // When the buffered and live trade share the same (signature, event_ordinal)
        // the live copy is a duplicate — suppressed so exactly ONE event reaches IPC.
        // The replayed copy (with pool_amm_id filled in) is the canonical emission.
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Step 1: buffer a trade that has pool=default (CPI race, mint known).
        //         test_trade() sets event_ordinal = Some(0).
        let buffered_trade = test_trade(Pubkey::default(), mint);
        assert!(
            !seer
                .handle_trade_event(buffered_trade.clone(), "grpc_global_stream", true)
                .await,
            "pool=default trade must be buffered, not forwarded"
        );

        // Step 2: live trade with full pool+mint triggers optimistic self-registration + replay.
        //         Same signature and event_ordinal=0 → live is a duplicate of buffered → suppressed.
        let mut live_trade = buffered_trade.clone();
        live_trade.pool_amm_id = curve;

        let live_forwarded = seer
            .handle_trade_event(live_trade, "grpc_global_stream", true)
            .await;
        assert!(
            !live_forwarded,
            "live trade is a dup of buffered (same sig + ordinal) — replay already emitted it, live must be suppressed"
        );

        // Step 3: exactly one event must arrive — the replayed copy with pool filled in.
        let replayed = ipc_receiver
            .recv()
            .await
            .expect("replayed trade must arrive");
        match replayed {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.signature, buffered_trade.signature);
                assert_eq!(trade_event.trade.pool_amm_id, curve);
                assert_eq!(trade_event.trade.mint, mint);
            }
            other => panic!("expected a replayed trade event, got {:?}", other),
        }

        // Step 4: no second copy (live was suppressed by dedup).
        assert!(
            ipc_receiver.try_recv().is_err(),
            "trade must be delivered exactly once; replay suppresses the duplicate live copy"
        );

        // Step 5: pending_trades must drain after optimistic mapping + replay.
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByMint(mint.to_bytes())),
            "replayed bucket should drain after optimistic mapping registration"
        );
    }

    #[tokio::test]
    async fn test_trade_with_known_mint_registers_optimistic_mapping_and_forwards() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        let forwarded = seer
            .handle_trade_event(test_trade(curve, mint), "grpc_global_stream", true)
            .await;
        assert!(
            forwarded,
            "trade with both pool+mint known must be forwarded immediately via optimistic self-registration"
        );
        assert_eq!(
            seer.lookup_curve_mint(curve),
            Some(mint),
            "optimistic mapping must be registered from trade-supplied pool+mint"
        );
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(curve.to_bytes())),
            "trade must NOT be buffered — was forwarded live"
        );
        // Trade must have actually reached IPC.
        assert!(
            ipc_receiver.try_recv().is_ok(),
            "trade must be delivered to IPC without buffering"
        );
    }

    #[tokio::test]
    async fn test_mapping_conflict_trade_forwards_immediately_without_replay() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let curve = Pubkey::new_unique();
        let stale_mint = Pubkey::new_unique();
        let actual_mint = Pubkey::new_unique();
        seer.register_curve_mapping(curve, stale_mint, "create", true)
            .await;

        // Trade arrives with a different mint than the registered stale_mint.
        // Optimistic self-registration: forward immediately without buffering.
        let forwarded = seer
            .handle_trade_event(test_trade(curve, actual_mint), "grpc_global_stream", true)
            .await;
        assert!(
            forwarded,
            "conflicting trade must be forwarded immediately via optimistic self-registration"
        );

        let emitted = ipc_receiver
            .try_recv()
            .expect("trade must reach IPC without delay");
        match emitted {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.pool_amm_id, curve);
                assert_eq!(
                    trade_event.trade.mint, actual_mint,
                    "forwarded trade must carry the trade-supplied mint"
                );
            }
            other => panic!("expected SeerEvent::Trade, got {:?}", other),
        }
        assert!(
            seer.pending_trades.read().is_empty(),
            "no trade should remain buffered after optimistic forward"
        );
    }

    #[test]
    fn test_pool_filter_uses_pool_metric_not_trade_outcome_metric() {
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(SeerConfig::default(), tx);
        let trade_metric_before = read_outcome_count(
            &seer.metrics.events_filtered,
            TradeOutcome::FilteredInvalidPool,
        );
        let pool_metric_before =
            read_reason_count(&seer.metrics.pool_events_filtered, "filtered_by_config");

        seer.metrics
            .pool_events_filtered
            .with_label_values(&["filtered_by_config"])
            .inc();

        assert_eq!(
            read_reason_count(&seer.metrics.pool_events_filtered, "filtered_by_config")
                - pool_metric_before,
            1.0
        );
        assert_eq!(
            read_outcome_count(
                &seer.metrics.events_filtered,
                TradeOutcome::FilteredInvalidPool
            ) - trade_metric_before,
            0.0,
            "pool filtering must not reuse trade outcome metrics"
        );
    }

    /// Verify that `PendingTradeReason::CurveMappingMissing` is assigned when the pool
    /// is known but mint is default (curve mapping not yet registered).
    #[test]
    fn test_pending_trade_reason_curve_mapping_missing() {
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(SeerConfig::default(), tx);

        let pool = Pubkey::new_unique();
        // Trade with known pool but unknown mint — curve mapping is missing.
        let trade = test_trade(pool, Pubkey::default());
        seer.buffer_pending_trade(trade.clone(), "test", false);

        let pending = seer.pending_trades.read();
        let queue = pending
            .get(&PendingTradeKey::ByCurve(pool.to_bytes()))
            .expect("trade must be keyed by ByCurve");
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue[0].reason,
            PendingTradeReason::CurveMappingMissing,
            "known pool + default mint → CurveMappingMissing"
        );
    }

    /// Verify that `PendingTradeReason::MissingPoolFromMint` is assigned when the mint
    /// is known but pool_amm_id is default.
    #[test]
    fn test_pending_trade_reason_missing_pool_from_mint() {
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(SeerConfig::default(), tx);

        let mint = Pubkey::new_unique();
        // Trade with unknown pool but known mint.
        let trade = test_trade(Pubkey::default(), mint);
        seer.buffer_pending_trade(trade.clone(), "test", false);

        let pending = seer.pending_trades.read();
        let queue = pending
            .get(&PendingTradeKey::ByMint(mint.to_bytes()))
            .expect("trade must be keyed by ByMint");
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue[0].reason,
            PendingTradeReason::MissingPoolFromMint,
            "default pool + known mint → MissingPoolFromMint"
        );
    }

    /// Verify that `PendingTradeReason::MissingMintAndPool` is assigned when both
    /// pool_amm_id and mint are default.
    #[test]
    fn test_pending_trade_reason_missing_mint_and_pool() {
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(SeerConfig::default(), tx);

        // Trade with both fields unknown.
        let trade = test_trade(Pubkey::default(), Pubkey::default());
        let expected_key = PendingTradeKey::BySignature(trade.signature.to_string());
        seer.buffer_pending_trade(trade.clone(), "test", false);

        let pending = seer.pending_trades.read();
        let queue = pending
            .get(&expected_key)
            .expect("trade must be keyed by BySignature");
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue[0].reason,
            PendingTradeReason::MissingMintAndPool,
            "default pool + default mint → MissingMintAndPool"
        );
    }

    /// Verify that `PendingTradeReason::MappingConflict` is assigned when the pool is
    /// known, the mint is known, but the existing mapping conflicts.
    #[test]
    fn test_pending_trade_reason_mapping_conflict() {
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(SeerConfig::default(), tx);

        let pool = Pubkey::new_unique();
        let stale_mint = Pubkey::new_unique();
        let actual_mint = Pubkey::new_unique();

        // Establish a mapping for pool → stale_mint
        seer.set_curve_mapping(pool, stale_mint, "create", true);

        // Trade arrives with pool + actual_mint (conflicts with stale_mint mapping);
        // `should_forward_trade` buffers it because can_forward_with_trade_mint is false
        // (the reverse map doesn't point to this pool either).
        let trade = test_trade(pool, actual_mint);
        seer.buffer_pending_trade(trade.clone(), "test", false);

        let pending = seer.pending_trades.read();
        let queue = pending
            .get(&PendingTradeKey::ByCurve(pool.to_bytes()))
            .expect("trade must be keyed by ByCurve");
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue[0].reason,
            PendingTradeReason::MappingConflict,
            "known pool + known mint (mismatched mapping) → MappingConflict"
        );
    }

    #[test]
    fn test_buffer_pending_trade_respects_event_ordinal_for_same_signature() {
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(SeerConfig::default(), tx);

        let pool = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let signature = Signature::new_unique();

        let mut trade_a = test_trade(pool, Pubkey::default());
        trade_a.signature = signature;
        trade_a.signer = signer;
        trade_a.event_ordinal = Some(0);

        let mut trade_b = trade_a.clone();
        trade_b.event_ordinal = Some(1);

        seer.buffer_pending_trade(trade_a, "test", false);
        seer.buffer_pending_trade(trade_b, "test", false);

        let pending = seer.pending_trades.read();
        let queue = pending
            .get(&PendingTradeKey::ByCurve(pool.to_bytes()))
            .expect("both sibling events must share the ByCurve bucket");
        assert_eq!(
            queue.len(),
            2,
            "same signature trades with different event ordinals must both stay buffered"
        );
    }

    #[tokio::test]
    async fn test_handle_trade_event_does_not_suppress_sibling_event_with_same_signature() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Arc::new(Seer::new_with_ipc(SeerConfig::default(), ipc_sender));

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let signature = Signature::new_unique();

        let mut buffered_trade = test_trade(pool, Pubkey::default());
        buffered_trade.signature = signature;
        buffered_trade.signer = signer;
        buffered_trade.event_ordinal = Some(0);
        seer.buffer_pending_trade(buffered_trade, "test", false);

        let mut live_trade = test_trade(pool, mint);
        live_trade.signature = signature;
        live_trade.signer = signer;
        live_trade.event_ordinal = Some(1);

        assert!(
            seer.handle_trade_event(live_trade, "test", false).await,
            "resolved sibling trade should still forward live"
        );

        let mut seen_ordinals = HashSet::new();
        for _ in 0..2 {
            let forwarded = tokio::time::timeout(
                std::time::Duration::from_millis(TRADE_FORWARD_TIMEOUT_MS),
                ipc_receiver.recv(),
            )
            .await
            .expect("trade must be forwarded within timeout")
            .expect("IPC channel must remain open");
            match forwarded {
                SeerEvent::Trade(trade_event) => {
                    seen_ordinals.insert(trade_event.trade.event_ordinal);
                }
                other => panic!("expected Trade event, got {:?}", other),
            }
        }

        assert!(
            seen_ordinals.contains(&Some(0)),
            "buffered sibling must replay"
        );
        assert!(
            seen_ordinals.contains(&Some(1)),
            "live sibling must not be suppressed"
        );
    }

    /// Race condition test: CREATE and TRADE events processed concurrently.
    ///
    /// Explicitly covers the worker ordering where CREATE wins and the TRADE
    /// should forward live without ever being dropped.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_race_create_vs_trade_create_worker_first() {
        run_create_trade_worker_order_test(WorkerOrder::CreateThenTrade).await;
    }

    /// Explicitly covers the worker ordering where TRADE wins and must be
    /// buffered+replayed once CREATE registers the mapping.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_race_create_vs_trade_trade_worker_first() {
        run_create_trade_worker_order_test(WorkerOrder::TradeThenCreate).await;
    }

    /// Race condition test: CREATE and TRADE events processed concurrently.
    ///
    /// Spawns the TRADE handler and the CREATE (register_curve_mapping) on separate
    /// async tasks so that the tokio scheduler can interleave them freely.  The test
    /// asserts that, regardless of which task wins the race, the trade is eventually
    /// forwarded exactly once and the pending-trades buffer drains cleanly.
    ///
    /// Uses `coverage_test_lock()` to prevent interference with other tests that
    /// snapshot the global `pipeline_coverage()` counters.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_race_create_vs_trade_concurrent_processing() {
        // Serialize with test_trade_before_create_recovers_mapping so that our
        // buffer/replay increments don't contaminate that test's coverage delta.
        let _guard = coverage_test_lock().lock().await;

        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Arc::new(Seer::new_with_ipc(SeerConfig::default(), ipc_sender));

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Build a trade whose pool is known but whose mint hasn't been mapped yet.
        // This represents the typical race window: TRADE tx lands before CREATE.
        let trade = test_trade(pool, Pubkey::default());

        let seer_trade = Arc::clone(&seer);
        let seer_create = Arc::clone(&seer);

        // Spawn both operations concurrently and let the runtime schedule them.
        let trade_task = tokio::spawn(async move {
            seer_trade
                .handle_trade_event(trade, "grpc_global_stream", true)
                .await
        });
        let create_task = tokio::spawn(async move {
            seer_create
                .register_curve_mapping(pool, mint, "create", true)
                .await
        });

        // Wait for both tasks to finish.
        let _ = trade_task.await.expect("trade task must not panic");
        let _ = create_task.await.expect("create task must not panic");

        // The trade must be forwarded exactly once (either directly or via replay).
        // Give the IPC channel a brief window to deliver the message.
        let forwarded =
            tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
                .await
                .expect("trade must be forwarded within timeout")
                .expect("IPC channel must not be closed");

        match forwarded {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(
                    trade_event.trade.pool_amm_id, pool,
                    "forwarded trade must carry the correct pool"
                );
                assert_eq!(
                    trade_event.trade.mint, mint,
                    "forwarded trade must carry the resolved mint"
                );
            }
            other => panic!("expected a Trade event, got {:?}", other),
        }

        // After replay the pending-trades bucket for this pool must be empty.
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(pool.to_bytes())),
            "pending queue must drain after concurrent CREATE+TRADE processing"
        );

        // No second trade should arrive (no duplicates).
        assert!(
            ipc_receiver.try_recv().is_err(),
            "trade must be forwarded exactly once (no duplicates)"
        );
    }

    #[tokio::test]
    async fn test_ultrafast_mode_keeps_forwarding_trades() {
        let mut config = SeerConfig::default();
        config.source_mode = Some(SeerSourceMode::PumpPortalWs);

        let ipc_config = IpcChannelConfig {
            buffer_size: 2,
            backpressure_policy: BackpressurePolicy::Block,
            log_drops: false,
            log_overflows: false,
            warning_threshold_percent: 50.0,
        };
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(config, ipc_sender);
        seer.ultrafast_mode.store(true, Relaxed);

        let trade = test_trade(Pubkey::new_unique(), Pubkey::new_unique());
        let payload = bincode::serialize(&types::SyntheticPayload::Trade(trade.clone())).unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(1),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: trade.signature,
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: Pubkey::new_unique(),
                account_indices: vec![],
                data: payload,
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: true,
            source: "pumpportal".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        seer.process_event(event).await.unwrap();

        let maybe_trade = ipc_receiver
            .recv()
            .await
            .expect("trade should still forward");
        match maybe_trade {
            SeerEvent::Trade(trade_event) => {
                assert_eq!(trade_event.trade.signature, trade.signature);
            }
            other => panic!("expected trade event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_session_start_slot_rejects_old_pools() {
        let config = SeerConfig::default();
        let (tx, mut rx) = mpsc::channel(10);
        let seer = Seer::new(config, tx);

        // Force session_start_slot to 100
        seer.session_start_slot.store(100, Relaxed);

        let pumpfun_program_id =
            Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap();
        let sol_mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();

        let pool = types::InitializePoolEvent {
            slot: Some(99), // Old slot
            event_ts_ms: Some(1_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            amm_program_id: pumpfun_program_id,
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: sol_mint,
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            initial_virtual_token_reserves: None,
            initial_virtual_sol_reserves: None,
            initial_real_token_reserves: None,
            initial_real_sol_reserves: None,
            token_total_supply: None,
            block_time: Some(1),
            raw_data: vec![],
        };

        let payload = bincode::serialize(&types::SyntheticPayload::InitializePool(pool)).unwrap();
        let event = types::GeyserEvent::Transaction {
            slot: Some(99),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: Pubkey::default(),
                account_indices: vec![],
                data: payload,
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: true,
            source: "pumpportal".to_string(), // Uses synthetic payload parsing
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        seer.process_event(event).await.unwrap();

        // Should be rejected since slot 99 < 100
        assert!(
            rx.try_recv().is_err(),
            "Older pool should have been rejected"
        );

        let pool_new = types::InitializePoolEvent {
            slot: Some(101), // New slot
            event_ts_ms: Some(1_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            amm_program_id: pumpfun_program_id,
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: sol_mint,
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            initial_virtual_token_reserves: None,
            initial_virtual_sol_reserves: None,
            initial_real_token_reserves: None,
            initial_real_sol_reserves: None,
            token_total_supply: None,
            block_time: Some(1),
            raw_data: vec![],
        };

        let payload_new =
            bincode::serialize(&types::SyntheticPayload::InitializePool(pool_new)).unwrap();
        let event_new = types::GeyserEvent::Transaction {
            slot: Some(101),
            event_ts_ms: Some(1_000),
            arrival_ts_ms: Some(types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique(),
            accounts: vec![],
            instructions: vec![types::RawInstruction {
                program_id: Pubkey::default(),
                account_indices: vec![],
                data: payload_new,
            }],
            logs: vec![],
            block_time: Some(1),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: true,
            source: "pumpportal".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: types::RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        };

        seer.process_event(event_new).await.unwrap();
        // Should be accepted since slot 101 >= 100
        assert!(rx.try_recv().is_ok(), "Newer pool should be accepted");
    }

    // ── resolve throughput: config + stress tests ─────────────────────────

    /// Burst of 32 trades for 32 distinct unknown curves — all must be replayed
    /// (none expired) when mappings are registered quickly. This guards the
    /// post-resolve-removal path: there is no semaphore-gated resolver anymore,
    /// so burst replay depends only on the pending-trade registry and mapping replay.
    #[tokio::test]
    async fn test_burst_trades_no_premature_expiry() {
        let _guard = coverage_test_lock().lock().await;
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 512;
        let (ipc_sender, _ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        const BURST: usize = 32;

        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        // Each pair: (curve / pool_amm_id, mint).  Trade arrives before CREATE, so
        // pool_amm_id is known but mint is Pubkey::default() → buffered as ByCurve.
        let pairs: Vec<(Pubkey, Pubkey)> = (0..BURST)
            .map(|_| (Pubkey::new_unique(), Pubkey::new_unique()))
            .collect();

        for (curve, _mint) in &pairs {
            let trade = test_trade(*curve, Pubkey::default());
            let _ = seer
                .handle_trade_event(trade, "grpc_global_stream", true)
                .await;
        }

        // Register all mappings immediately — simulates a fast bulk resolve.
        for (curve, mint) in &pairs {
            seer.register_curve_mapping(*curve, *mint, "create", false)
                .await;
        }

        let replayed =
            read_outcome_count(&seer.metrics.events_filtered, TradeOutcome::ForwardedReplay)
                as usize;
        assert_eq!(
            replayed, BURST,
            "expected exactly {BURST} replayed trades, got {replayed}"
        );

        // No pending trade should have expired — the mapping arrived before TTL.
        let expired_count = seer
            .metrics
            .pending_trade_expired_while_buffered_total
            .with_label_values(&["curve_mapping_missing"])
            .get();
        assert_eq!(
            expired_count, 0,
            "no trade should expire before resolve when mappings arrive quickly"
        );
    }

    // ──────────────────────────────────────────────────────────────────────────────
    // Boundary regression tests: Seer must not act as a curve-state authority.
    // Shadow Ledger is the sole authoritative curve-state machine.
    // ──────────────────────────────────────────────────────────────────────────────

    /// Verify that Seer's trade emission path does NOT update ShadowLedger curve state.
    ///
    /// Seer is the canonical transaction *producer* only.  Shadow Ledger (via Gatekeeper
    /// + LivePipeline) is the authoritative state-evolution *consumer*.  This test acts
    /// as a regression guard: if someone re-introduces a direct `store_curve_with_snapshots`
    /// call inside the trade-emission path, the ShadowLedger slot will advance and this
    /// test will fail.
    #[tokio::test]
    async fn test_seer_emit_does_not_write_shadow_ledger_from_trade() {
        let config = SeerConfig::default();
        let ledger = Arc::new(ShadowLedger::new());
        let ipc_config = IpcChannelConfig::default();
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, ipc_sender, Arc::clone(&ledger));

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Register the mapping so the trade is forwarded live (not buffered).
        seer.tracked_curves.write().insert(curve, mint);
        seer.set_curve_mapping(curve, mint, "test", true);

        let mut trade = test_trade(curve, mint);
        // Populate virtual reserves so that, if the old code path were present, it would
        // write a non-zero BondingCurve to the ShadowLedger.
        trade.v_tokens_in_bonding_curve = Some(500_000.0);
        trade.v_sol_in_bonding_curve = Some(30.0);
        trade.curve_data_known = true;

        // Emit the trade.
        seer.emit_trade_only(trade, "test", false, false).await;

        // Drain the IPC event to confirm the trade was forwarded.
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv()).await;
        assert!(result.is_ok(), "trade should have been forwarded via IPC");

        // BOUNDARY ASSERTION: Seer must NOT have written any curve state to ShadowLedger.
        // The only way curve state enters ShadowLedger is via account updates (Geyser)
        // or the Gatekeeper/LivePipeline tx-driven path — never from Seer's trade emission.
        assert!(
            ledger.get_curve_info(&curve).is_none(),
            "Seer trade emission must NOT write curve state to ShadowLedger; \
             Shadow Ledger is the sole curve-state authority"
        );
    }

    /// Verify that Seer's pending-trade replay path also does NOT update ShadowLedger.
    ///
    /// When a buffered trade is replayed after the curve→mint mapping arrives, it must
    /// be emitted via IPC only — no direct ShadowLedger mutation should occur.
    #[tokio::test]
    async fn test_seer_replay_does_not_write_shadow_ledger_from_trade() {
        let config = SeerConfig::default();
        let ledger = Arc::new(ShadowLedger::new());
        let ipc_config = IpcChannelConfig::default();
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, ipc_sender, Arc::clone(&ledger));

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Construct a trade that arrives BEFORE the mapping is known → gets buffered.
        let mut trade = test_trade(curve, Pubkey::default());
        trade.v_tokens_in_bonding_curve = Some(500_000.0);
        trade.v_sol_in_bonding_curve = Some(30.0);
        trade.curve_data_known = true;

        // Buffer it (mapping unknown → pool_amm_id resolved but mint is default).
        seer.handle_trade_event(trade, "test", false).await;

        // Now register the mapping — this triggers replay.
        seer.register_curve_mapping(curve, mint, "create", true)
            .await;

        // Drain the replayed event.
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv()).await;
        assert!(
            result.is_ok(),
            "replayed trade should have been forwarded via IPC"
        );

        // BOUNDARY ASSERTION: the replay path must NOT write curve state to ShadowLedger.
        assert!(
            ledger.get_curve_info(&curve).is_none(),
            "Seer trade replay must NOT write curve state to ShadowLedger; \
             Shadow Ledger is the sole curve-state authority"
        );
    }

    // ─── PR-2 contract tests ──────────────────────────────────────────────────

    /// PR-2 §1 — A trade arriving before the pool CREATE must be buffered and
    /// replayed **exactly once** after `register_curve_mapping` fires.
    /// No duplicate must appear in IPC and the pending queue must drain fully.
    #[tokio::test]
    async fn trade_before_create_replays_once() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = test_trade(pool, Pubkey::default());
        let trade_sig = trade.signature;

        // Trade arrives before mapping is known — must be buffered, not forwarded.
        let forwarded_live = seer.handle_trade_event(trade, "test", false).await;
        assert!(
            !forwarded_live,
            "trade must be buffered, not forwarded live"
        );
        assert!(
            seer.pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(pool.to_bytes())),
            "trade must be in pending_trades keyed by ByCurve"
        );

        // Mapping arrives (pool CREATE processed) — triggers replay.
        seer.register_curve_mapping(pool, mint, "create", true)
            .await;

        // Exactly one replayed trade must arrive via IPC.
        let ev = tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
            .await
            .expect("replayed trade must arrive within 200 ms")
            .expect("IPC channel must not be closed");
        match ev {
            SeerEvent::Trade(te) => {
                assert_eq!(te.trade.signature, trade_sig, "signature must match");
                assert_eq!(te.trade.pool_amm_id, pool, "pool must match");
                assert_eq!(
                    te.trade.mint, mint,
                    "mint must be resolved mint, not default"
                );
            }
            other => panic!("expected Trade, got {:?}", other),
        }

        // No second event must appear (no duplicate).
        assert!(
            ipc_receiver.try_recv().is_err(),
            "trade must be replayed exactly once — no duplicate"
        );

        // Pending queue must be empty.
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(pool.to_bytes())),
            "pending queue must drain after replay"
        );
    }

    /// PR-2 §2 — An AccountUpdate arriving before curve→mint mapping is known
    /// must be buffered in `pending_curve_updates` (sole owner) and replayed to
    /// ShadowLedger **exactly once** when `register_curve_mapping` fires.
    ///
    /// "Exactly once" is proven by:
    ///   1. `last_updated_slot` after the first mapping = slot of the AccountUpdate.
    ///   2. `pending_curve_updates` is empty immediately after the first drain
    ///      (HashMap::remove guarantees drain-once).
    ///   3. A second `register_curve_mapping` call is a no-op — the buffer is
    ///      already empty, so ShadowLedger does not receive a second write and
    ///      `last_updated_slot` remains unchanged.
    #[tokio::test]
    async fn account_update_before_mapping_replays() {
        let config = SeerConfig::default();
        let ipc_config = IpcChannelConfig::default();
        let (ipc_sender, mut rx, _metrics) = create_ipc_channel(ipc_config);
        let ledger = Arc::new(ShadowLedger::new());
        let seer = Seer::new_with_ipc_and_shadow_ledger(config, ipc_sender, Arc::clone(&ledger));

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        const UPDATE_SLOT: u64 = 42;

        // Build a valid 56-byte bonding-curve account payload.
        let owner = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("valid pump program id");
        let mut data = vec![0u8; 56];
        data[0..8].copy_from_slice(&1_000_000u64.to_le_bytes()); // virtual_token_reserves
        data[8..16].copy_from_slice(&500_000u64.to_le_bytes()); // virtual_sol_reserves

        // AccountUpdate arrives before mapping is known — must buffer, not apply.
        seer.process_event(types::GeyserEvent::AccountUpdate {
            slot: UPDATE_SLOT,
            event_time: ghost_core::EventTimeMetadata::default(),
            write_version: None,
            pubkey: curve,
            data: data.clone(),
            owner,
        })
        .await
        .unwrap();

        // ── Pre-mapping assertions ────────────────────────────────────────────
        assert!(
            seer.pending_curve_updates
                .read()
                .contains_key(&curve.to_bytes()),
            "AccountUpdate must be buffered in pending_curve_updates when mapping unknown"
        );
        assert!(
            ledger.get_curve_info(&curve).is_none(),
            "ShadowLedger must NOT be updated before mapping is known"
        );

        // ── First mapping: triggers replay ────────────────────────────────────
        seer.register_curve_mapping(curve, mint, "create", true)
            .await;

        // Buffer must be drained (drain-once guarantee via HashMap::remove).
        assert!(
            !seer
                .pending_curve_updates
                .read()
                .contains_key(&curve.to_bytes()),
            "pending_curve_updates must be empty after first register_curve_mapping"
        );

        let event_after_first = rx
            .recv()
            .await
            .expect("pending AccountUpdate must replay into IPC after mapping");
        let SeerEvent::AccountUpdate(event_after_first) = event_after_first else {
            panic!("expected replayed AccountUpdate event");
        };
        assert_eq!(event_after_first.base_mint, mint);
        assert_eq!(event_after_first.bonding_curve, curve);
        assert_eq!(event_after_first.slot, UPDATE_SLOT);
        assert_eq!(
            event_after_first.replay_origin,
            AccountUpdateReplayOrigin::PendingReplay
        );
        assert!(
            event_after_first.replay_buffer_dwell_ms.is_some(),
            "replayed AccountUpdate must carry replay dwell telemetry"
        );
        assert!(
            ledger.get_curve_info(&curve).is_none(),
            "pending AccountUpdate replay must not write ShadowLedger"
        );

        // ── Second mapping call: must be a no-op for ShadowLedger ─────────────
        // pending_curve_updates is empty → replay_pending_curve_update returns
        // immediately without writing — proving exactly-once semantics.
        seer.register_curve_mapping(curve, mint, "create", true)
            .await;

        // Buffer still empty after second call.
        assert!(
            !seer
                .pending_curve_updates
                .read()
                .contains_key(&curve.to_bytes()),
            "pending_curve_updates must remain empty after second register_curve_mapping"
        );

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), rx.recv())
                .await
                .is_err(),
            "second mapping must not replay a duplicate AccountUpdate"
        );
    }

    #[test]
    fn queue_pending_curve_update_overwrites_only_newer_versions() {
        let config = SeerConfig::default();
        let (tx, _rx) = mpsc::channel(4);
        let seer = Seer::new(config, tx);
        let curve = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let older = vec![1u8; 56];
        let newer = vec![2u8; 56];

        let first = seer.queue_pending_curve_update(
            curve,
            10,
            ghost_core::EventTimeMetadata::default(),
            Some(1),
            owner,
            &older,
        );
        let second = seer.queue_pending_curve_update(
            curve,
            11,
            ghost_core::EventTimeMetadata::default(),
            Some(2),
            owner,
            &newer,
        );
        let third = seer.queue_pending_curve_update(
            curve,
            9,
            ghost_core::EventTimeMetadata::default(),
            Some(0),
            owner,
            &older,
        );

        assert_eq!(first, PendingCurveUpdateStoreOutcome::Inserted);
        assert_eq!(second, PendingCurveUpdateStoreOutcome::ReplacedNewer);
        assert_eq!(third, PendingCurveUpdateStoreOutcome::IgnoredOlder);

        let pending = seer
            .pending_curve_updates
            .read()
            .get(&curve.to_bytes())
            .cloned()
            .expect("pending curve update must remain stored");
        assert_eq!(pending.earliest.slot, 10);
        assert_eq!(pending.earliest.write_version, Some(1));
        assert_eq!(pending.latest.slot, 11);
        assert_eq!(pending.latest.write_version, Some(2));
        assert_eq!(pending.latest.data, newer);
    }

    /// PR-2 §3 — `mint == Pubkey::default()` (the 111…1 sentinel) must NEVER
    /// appear as the mint in a trade forwarded via IPC.  Only the unresolved
    /// buffer path may carry the default mint; once resolved the real mint must
    /// be substituted before IPC delivery.
    #[tokio::test]
    async fn mint_111_never_reaches_ipc() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool = Pubkey::new_unique();
        let real_mint = Pubkey::new_unique();

        // Scenario A: trade with default mint arrives before mapping (buffered path).
        let trade_a = test_trade(pool, Pubkey::default());
        seer.handle_trade_event(trade_a, "test", false).await;

        // Register mapping — replays the buffered trade.
        seer.register_curve_mapping(pool, real_mint, "create", true)
            .await;

        // Drain replayed event and verify mint is real.
        let ev = tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
            .await
            .expect("replayed trade must arrive")
            .expect("IPC channel open");
        if let SeerEvent::Trade(te) = ev {
            assert_ne!(
                te.trade.mint,
                Pubkey::default(),
                "Pubkey::default() must NOT reach IPC as resolved mint (replayed path)"
            );
            assert_eq!(te.trade.mint, real_mint, "real mint must be substituted");
        } else {
            panic!("expected Trade event");
        }

        // Scenario B: trade with default mint arrives AFTER mapping is known.
        // The live path must also resolve the mint before IPC delivery.
        let pool2 = Pubkey::new_unique();
        let real_mint2 = Pubkey::new_unique();
        seer.register_curve_mapping(pool2, real_mint2, "create", true)
            .await;
        // Drain any spurious event from empty-pending replay.
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(10), ipc_receiver.recv()).await;

        // Trade with known pool but default mint (parser couldn't resolve) — live path.
        let trade_b = test_trade(pool2, Pubkey::default());
        seer.handle_trade_event(trade_b, "test", false).await;

        // The live path either forwards with real mint or buffers; either way, if
        // anything reaches IPC it must not carry default mint.
        if let Ok(Some(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(50), ipc_receiver.recv()).await
        {
            if let SeerEvent::Trade(te) = ev {
                assert_ne!(
                    te.trade.mint,
                    Pubkey::default(),
                    "Pubkey::default() must NOT reach IPC as resolved mint (live path)"
                );
            }
        }
    }

    /// PR-2 §4 — When the same trade is buffered (mapping unknown) and the live
    /// variant arrives before the mapping fires, only one copy must be delivered
    /// to IPC.  The dedup guard checks pending_trades before allowing the live
    /// event to forward, suppressing it when a buffered copy exists.
    #[tokio::test]
    async fn replay_no_dup_on_overlapping_arrivals() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Arc::new(Seer::new_with_ipc(SeerConfig::default(), ipc_sender));

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let shared_sig = Signature::new_unique();
        let signer = Pubkey::new_unique();

        // Step 1: trade arrives without mapping — buffered under ByCurve.
        let mut buffered_trade = test_trade(pool, Pubkey::default());
        buffered_trade.signature = shared_sig;
        buffered_trade.signer = signer;
        buffered_trade.event_ordinal = Some(0);
        seer.handle_trade_event(buffered_trade.clone(), "test", false)
            .await;
        assert!(
            seer.pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(pool.to_bytes())),
            "trade must be buffered in pending_trades"
        );

        // Step 2: the same trade arrives a second time (live path), now with the
        // real mint and pool resolved.  The dedup guard must detect the buffered
        // copy and suppress this live forward.
        let mut live_trade = test_trade(pool, mint);
        live_trade.signature = shared_sig;
        live_trade.signer = signer;
        live_trade.event_ordinal = Some(0);
        // register_curve_mapping is called inside handle_trade_event when mint is known.
        // This drains the buffer and replays; the live forward must be suppressed.
        seer.handle_trade_event(live_trade, "test", false).await;

        // Collect all IPC events.  Exactly one must arrive.
        let mut received = vec![];
        loop {
            match tokio::time::timeout(std::time::Duration::from_millis(150), ipc_receiver.recv())
                .await
            {
                Ok(Some(ev)) => received.push(ev),
                _ => break,
            }
        }

        assert_eq!(
            received.len(),
            1,
            "trade must be delivered exactly once; buffered+live must not produce duplicates (got {})",
            received.len()
        );
    }

    /// PR-2 §5 — `event_ordinal` must be preserved end-to-end through the replay
    /// path.  The ordinal on the replayed IPC event must match the ordinal that
    /// was set when the trade was originally buffered.
    #[tokio::test]
    async fn event_ordinal_preserved_after_replay() {
        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 8;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let shared_sig = Signature::new_unique();

        // Two events from the same transaction — distinct ordinals.
        let mut trade0 = test_trade(pool, Pubkey::default());
        trade0.signature = shared_sig;
        trade0.event_ordinal = Some(0);

        let mut trade1 = test_trade(pool, Pubkey::default());
        trade1.signature = shared_sig;
        trade1.event_ordinal = Some(1);

        // Buffer both before mapping is known.
        seer.handle_trade_event(trade0, "test", false).await;
        seer.handle_trade_event(trade1, "test", false).await;

        // Trigger replay.
        seer.register_curve_mapping(pool, mint, "create", true)
            .await;

        // Collect the two replayed events.
        let mut seen_ordinals: HashSet<Option<u32>> = HashSet::new();
        for _ in 0..2 {
            let ev =
                tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
                    .await
                    .expect("replayed event must arrive")
                    .expect("IPC channel open");
            if let SeerEvent::Trade(te) = ev {
                seen_ordinals.insert(te.trade.event_ordinal);
            } else {
                panic!("expected Trade event");
            }
        }

        assert!(
            seen_ordinals.contains(&Some(0)),
            "event_ordinal=0 must be preserved through replay"
        );
        assert!(
            seen_ordinals.contains(&Some(1)),
            "event_ordinal=1 must be preserved through replay"
        );
        assert_eq!(seen_ordinals.len(), 2, "both ordinals must be distinct");
    }

    /// Regression test for IPC event ordering on the pool create path.
    ///
    /// `register_curve_mapping` ends with `replay_pending_trades`, which emits
    /// `SeerEvent::Trade` on the IPC channel for any trades buffered during the
    /// create/trade race window.  If `PoolDetected` is sent *after*
    /// `register_curve_mapping` (the original bug), ghost-launcher's
    /// `SessionPoolTradeBridge` receives Trade before the pool is registered and
    /// silently drops it — the first dev-buy is lost.
    ///
    /// The fixed create path sends PoolDetected *before* calling
    /// `register_curve_mapping`.  This test encodes that contract: it directly
    /// replicates the fixed sequence and asserts that the IPC receiver observes
    ///
    ///   PoolDetected → Trade
    ///
    /// and never the inverted order.
    #[tokio::test]
    async fn test_ipc_ordering_pool_detected_before_replayed_trade() {
        let _guard = coverage_test_lock().lock().await;

        let mut ipc_config = IpcChannelConfig::default();
        ipc_config.buffer_size = 16;
        let (ipc_sender, mut ipc_receiver, _metrics) = create_ipc_channel(ipc_config);

        // Keep a clone of the sender so the test can inject PoolDetected directly,
        // mirroring what process_event does after the fix.
        let test_sender = ipc_sender.clone();
        let seer = Seer::new_with_ipc(SeerConfig::default(), ipc_sender);

        let curve = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Buffer a pending trade: arrives before pool CREATE, mint unknown.
        // trade.pool_amm_id = curve (bonding curve), trade.mint = default.
        let trade = test_trade(curve, Pubkey::default());
        let buffered = seer
            .handle_trade_event(trade.clone(), "grpc_global_stream", false)
            .await;
        assert!(
            !buffered,
            "trade must be buffered, not forwarded immediately"
        );
        assert!(
            seer.pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(curve.to_bytes())),
            "trade must be in pending-trades buffer under ByCurve key"
        );

        // Simulate the FIXED create path:
        //   Step 1 — send PoolDetected on IPC FIRST.
        let candidate = types::CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            tx_index: None,
            event_ts_ms: Some(1_000_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique().to_string(),
            amm_program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
                .parse()
                .unwrap(),
            pool_amm_id: curve,
            base_mint: mint,
            quote_mint: "So11111111111111111111111111111111111111112"
                .parse()
                .unwrap(),
            bonding_curve: curve,
            creator: Pubkey::new_unique(),
            timestamp: 1_000_000,
            bonding_curve_progress: Some(0.0),
            initial_liquidity_sol: Some(30.0),
            token_total_supply: Some(1_000_000_000_000_000),
            block_time: Some(1),
        };
        test_sender
            .send(candidate.clone(), EventPriority::Normal)
            .await
            .expect("PoolDetected send must succeed");

        //   Step 2 — register_curve_mapping → replay_pending_trades → Trade on IPC SECOND.
        seer.register_curve_mapping(curve, mint, "create", true)
            .await;

        // Collect the first two IPC events in arrival order.
        let first =
            tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
                .await
                .expect("first IPC event must arrive within timeout")
                .expect("IPC channel must not be closed");

        let second =
            tokio::time::timeout(std::time::Duration::from_millis(200), ipc_receiver.recv())
                .await
                .expect("second IPC event must arrive within timeout")
                .expect("IPC channel must not be closed");

        // Assert ordering: PoolDetected → Trade.
        match &first {
            SeerEvent::PoolDetected(ev) => {
                assert_eq!(
                    ev.candidate.bonding_curve, curve,
                    "first IPC event must be PoolDetected for the new pool"
                );
            }
            other => panic!("expected PoolDetected as first IPC event, got {:?}", other),
        }
        match &second {
            SeerEvent::Trade(ev) => {
                assert_eq!(
                    ev.trade.pool_amm_id, curve,
                    "second IPC event must be replayed Trade for the new pool"
                );
                assert_eq!(
                    ev.trade.mint, mint,
                    "replayed trade must carry the resolved mint"
                );
            }
            other => panic!("expected Trade as second IPC event, got {:?}", other),
        }

        // No further events.
        assert!(
            ipc_receiver.try_recv().is_err(),
            "no further IPC events expected after PoolDetected + replayed Trade"
        );

        // Pending-trades buffer must be fully drained after replay.
        assert!(
            !seer
                .pending_trades
                .read()
                .contains_key(&PendingTradeKey::ByCurve(curve.to_bytes())),
            "pending-trades buffer must drain after register_curve_mapping"
        );
    }
}
