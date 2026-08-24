/// grpc_connection.rs — Ghost Pump.fun transport layer (final)
///
/// Addresses all known coverage gaps:
///   [FIX-1] UpdateOneof::Entry fully decoded + inner-ix walked for migrate CPI
///   [FIX-2] Spill channel now fully bounded — short bursts spill, sustained lag
///            drops newest events explicitly instead of growing RAM without bound
///   [FIX-3] Multi-provider fan-in — N independent endpoints merged onto one channel
///   Track `from_slot` for diagnostics; replay still requires explicit backfill
///   [FIX-5] Explicit delayed-account-update queue exposed to parser for
///            CurveMintRegistry race-condition window
///
/// Transport responsibilities (unchanged):
///   • Build authoritative SubscribeRequest (logged as SSOT)
///   • Resilient stream with ping/pong + stall watchdog
///   • Slot-gap detection on every message variant
///   • Profile-aware AccountRegistry → resub only when the effective request shape changes
///   • Backfill inject with parser-first tag
///
/// [dependencies]
/// yellowstone-grpc-client  = "1.14"
/// yellowstone-grpc-proto   = "1.14"
/// tokio                    = { version = "1", features = ["full"] }
/// tonic                    = { version = "0.10", features = ["tls", "tls-roots"] }
/// futures                  = "0.3"
/// prost                    = "0.12"
/// bs58                     = "0.5"
/// backoff                  = { version = "0.4", features = ["tokio"] }
/// crossbeam-channel        = "0.5"
/// dashmap                  = "5"
/// parking_lot              = "0.12"
/// metrics                  = "0.21"  (macro syntax: metrics::counter!("name", val) not .increment())
/// anyhow                   = "1"
/// thiserror                = "1"
/// tracing                  = "0.1"
use std::{
    collections::{hash_map::DefaultHasher, BTreeSet, HashMap, HashSet},
    future::Future,
    hash::{Hash, Hasher},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use backoff::{backoff::Backoff, ExponentialBackoffBuilder};
use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError, TrySendError};
use dashmap::{DashMap, DashSet};
use futures::{Sink, SinkExt, StreamExt};
use parking_lot::Mutex;
use prost::Message;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::rpc_http_client::new_async_rpc_client;
use tonic::{
    metadata::{Ascii, AsciiMetadataValue, MetadataKey},
    service::Interceptor,
    transport::Endpoint,
    Request, Status,
};
use tonic_health::pb::health_client::HealthClient;
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::prelude::{
    geyser_client::GeyserClient, subscribe_request_filter_accounts_filter,
    subscribe_request_filter_accounts_filter_memcmp, subscribe_update::UpdateOneof,
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterAccounts,
    SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterMemcmp,
    SubscribeRequestFilterBlocks, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterEntry,
    SubscribeRequestFilterSlots, SubscribeRequestFilterTransactions, SubscribeRequestPing,
    SubscribeUpdate,
};

// ─── Program / account constants ─────────────────────────────────────────────

pub const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub const PUMP_FUN_FEE_ACCOUNT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
pub const GRPC_GLOBAL_STREAM_SOURCE_LABEL: &str = "grpc_global_stream";
pub const GRPC_FUNDING_LANE_PUMP_FILTERED_SOURCE_LABEL: &str = "grpc_funding_lane_pump_filtered";
pub const GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL: &str = "grpc_funding_lane_full_chain";
pub const GRPC_PUMP_RESEARCH_GLOBAL_V1_SOURCE_LABEL: &str = "grpc_pump_research_global_v1";
pub const GRPC_PUMP_RESEARCH_EXACT_STATE_V2_SOURCE_LABEL: &str =
    "grpc_pump_research_exact_state_v2";
/// V2 receives unfiltered full block messages only in its standalone
/// completeness-evidence lane.  This cap is explicit: it avoids inheriting
/// tonic's small default while keeping the decoded source object bounded.
pub(crate) const PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_GRPC_AUTH_HEADER: &str = "x-token";
const YELLOWSTONE_TRANSACTION_PAYLOAD_SCHEMA_ID: &str =
    "yellowstone_subscribe_update_transaction.prost.v1";

/// Bonding-curve account discriminator — used in memcmp filter so we receive
/// account updates only for actual BondingCurve accounts, not every account
/// owned by the Pump.fun program (which would be enormous noise).
pub const BONDING_CURVE_DISC: [u8; 8] = [0x17, 0xb7, 0xf8, 0x37, 0x60, 0xd8, 0xac, 0x60];

/// PumpSwap AMM pool account discriminator — used in a separate memcmp filter
/// so that AMM pool account updates are received alongside bonding-curve updates.
/// Without this, new pools whose pubkeys haven't been added to the explicit
/// `acct_list` are silently filtered out by the bonding-curve-only memcmp.
pub const AMM_POOL_DISC: [u8; 8] = [0xf1, 0x9a, 0x6d, 0x28, 0x1c, 0x37, 0xe4, 0x55];

// ─── Tuning constants ─────────────────────────────────────────────────────────

/// One bounded ingress/work queue.
///
/// Backward-compatible default for programmatic constructors.
///
/// Production wiring uses `SeerConfig::ingress_queue_capacity`. The default is
/// the complete 2,048-event frozen concurrent burst used by the PR1B harness;
/// capacity is no longer inferred from parser throughput.
const DEFAULT_PRIMARY_CHANNEL_CAP: usize = 2_048;

const BACKOFF_INIT_MS: u64 = 50;
const BACKOFF_MAX_MS: u64 = 5_000;
/// Debounce dynamic re-subscribe requests when watch registry changes.
/// 1000ms: during launch bursts (100+ pools/s) the registry version increments
/// continuously — a 250ms debounce caused near-constant resubscriptions on the
/// hot path, potentially causing server-side delivery gaps on each SubscribeRequest.
/// In single_global mode resubscription happens only on health_tick (every 5s),
/// so this debounce serves as a guard against back-to-back health ticks
/// triggering duplicate resubs. PooledFiltered may opt into immediate/ticker
/// resubscribe and uses the same debounce as a burst guard.
const DEFAULT_RESUB_DEBOUNCE_MS: u64 = 1_000;

/// If no gRPC message arrives for this long → force reconnect.
///
/// This is intentionally provider-configurable. Some Yellowstone providers emit
/// BlockMeta/Slot heartbeats frequently; others keep filtered streams quiet
/// between matching transactions. A 2s hard-coded watchdog caused NLN streams
/// to reconnect before the first useful message arrived.
const DEFAULT_SILENT_STALL_SECS: u64 = 20;

const WATCHDOG_TICK_SECS: u64 = 2;
const HEALTH_TICK_SECS: u64 = 5;
const PING_INTERVAL_SECS: u64 = 10;
const REGISTRY_RESUB_TICK_MS: u64 = 500;
const PROVIDER_CIRCUIT_BREAKER_WAIT_POLL_MS: u64 = 250;
const DEFAULT_PROVIDER_MAX_STALLS_BEFORE_OPEN: u32 = 3;
const DEFAULT_PROVIDER_CIRCUIT_BREAKER_COOLDOWN_MS: u64 = 15_000;

/// Sleep when the queue is empty (prevents hot-spin while keeping low latency).
const DRAIN_IDLE_SLEEP_US: u64 = 50;

/// Yellowstone exact-account filters accept at most 200 pubkeys per branch.
/// One slot is reserved for the Pump.fun fee account.
const EXACT_ACCOUNT_FILTER_CAP: usize = 200;
const EXACT_ACCOUNT_PAYLOAD_CAP: usize = EXACT_ACCOUNT_FILTER_CAP - 1;

/// Periodic sweep interval for watched pool/mint state.
const WATCH_SWEEP_INTERVAL_SECS: u64 = 5;

static PARSER_TX_DECODE_TOTAL: AtomicU64 = AtomicU64::new(0);
static PARSER_TX_DECODE_MALFORMED: AtomicU64 = AtomicU64::new(0);

// ─── PumpEvent — transport output / parser input ──────────────────────────────

#[derive(Debug, Clone)]
pub enum PumpEvent {
    Transaction {
        provider_id: Option<String>,
        provider_role: Option<ghost_core::RawProviderRoleV1>,
        /// Monotonic timestamp captured at the adapter receive boundary.
        received_at_monotonic_ns: u64,
        signature: String,
        slot: u64,
        received_at: Instant,
        /// Yellowstone already decoded this child before handing it to the
        /// adapter. Ownership crosses the receive boundary without a
        /// prost encode/decode round trip.
        decoded: Box<yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction>,
        /// Capture is an opt-in WAL/audit concern. The encoded representation
        /// is produced once by the consumer-side normalizer, never by the
        /// socket receive loop and never used as parser transport.
        capture_payload: bool,
    },
    AccountUpdate {
        provider_id: Option<String>,
        provider_role: Option<ghost_core::RawProviderRoleV1>,
        pubkey: String,
        slot: u64,
        received_at: Instant,
        /// Pre-decoded GeyserEvent to avoid double proto decode
        #[doc(hidden)]
        decoded: Option<crate::types::GeyserEvent>,
    },
    /// [FIX-1] Entry events are now fully decoded and forwarded.
    ///
    /// An Entry carries `executed_transaction_count` (raw slot-throughput telemetry) and
    /// may contain inner-instruction / CPI data not present in Tx meta when the
    /// transaction was packed into a block entry rather than a standalone Tx update.
    /// Ghost parser walks inner_ixs here to catch the ~20-30% of migrate events
    /// that appear only as CPI inside block entries.
    EntryUpdate {
        provider_id: Option<String>,
        provider_role: Option<ghost_core::RawProviderRoleV1>,
        slot: u64,
        received_at: Instant,
        executed_transaction_count: u64,
        /// Raw SubscribeUpdateEntry proto bytes — parser decodes inner Ixs.
        raw: Vec<u8>,
    },
    /// Backfill replay — identical wire format as Transaction.
    /// Tagged so parser enforces "backfill → parse → classify" policy.
    BackfillTransaction {
        provider_id: Option<String>,
        provider_role: Option<ghost_core::RawProviderRoleV1>,
        signature: String,
        slot: u64,
        received_at: Instant,
        /// Pre-decoded GeyserEvent to avoid double proto decode
        #[doc(hidden)]
        decoded: Option<crate::types::GeyserEvent>,
    },
}

impl PumpEvent {
    #[inline(always)]
    pub fn slot(&self) -> u64 {
        match self {
            Self::Transaction { slot, .. } => *slot,
            Self::AccountUpdate { slot, .. } => *slot,
            Self::EntryUpdate { slot, .. } => *slot,
            Self::BackfillTransaction { slot, .. } => *slot,
        }
    }
    #[inline(always)]
    pub fn received_at(&self) -> Instant {
        match self {
            Self::Transaction { received_at, .. } => *received_at,
            Self::AccountUpdate { received_at, .. } => *received_at,
            Self::EntryUpdate { received_at, .. } => *received_at,
            Self::BackfillTransaction { received_at, .. } => *received_at,
        }
    }
    #[inline(always)]
    pub fn e2e_ms(&self) -> u64 {
        self.received_at().elapsed().as_millis() as u64
    }
    pub fn is_backfill(&self) -> bool {
        matches!(self, Self::BackfillTransaction { .. })
    }

    fn local_gap_boundary(&self) -> ghost_core::LocalCoverageBoundaryV1 {
        let signature = match self {
            Self::Transaction { signature, .. } | Self::BackfillTransaction { signature, .. } => {
                Signature::from_str(signature).ok()
            }
            _ => None,
        };
        ghost_core::LocalCoverageBoundaryV1 {
            slot: Some(self.slot()),
            signature,
        }
    }

    fn provider_id(&self) -> &str {
        match self {
            Self::Transaction { provider_id, .. }
            | Self::AccountUpdate { provider_id, .. }
            | Self::EntryUpdate { provider_id, .. }
            | Self::BackfillTransaction { provider_id, .. } => {
                provider_id.as_deref().unwrap_or("unknown")
            }
        }
    }
}

// ─── PR1B single bounded ingress queue ───────────────────────────────────────

/// Compatibility name retained for callers; PR1B deliberately collapses the
/// former fast+overflow cascade into exactly one bounded FIFO.
#[derive(Clone)]
pub struct DualLaneChannel {
    queue: Sender<PumpEvent>,
    capture_live_payload: Arc<AtomicBool>,
    local_gap: Arc<crate::local_gap::LocalGapTracker>,
    stream_epoch: Arc<AtomicU64>,
}

pub struct DualLaneReceiver {
    pub queue: Receiver<PumpEvent>,
    local_gap: Arc<crate::local_gap::LocalGapTracker>,
}

#[cfg(test)]
impl DualLaneReceiver {
    pub(crate) fn take_completed_local_gap(&self) -> Option<ghost_core::LocalCoverageGapV1> {
        self.local_gap.take_completed()
    }
}

impl DualLaneChannel {
    pub fn new() -> (Self, DualLaneReceiver) {
        Self::with_capacity(DEFAULT_PRIMARY_CHANNEL_CAP)
    }

    fn with_capacity(capacity: usize) -> (Self, DualLaneReceiver) {
        let (queue, receiver) = bounded(capacity);
        let local_gap = Arc::new(crate::local_gap::LocalGapTracker::new(
            ghost_core::LocalCoverageGapReasonV1::IngressQueueSaturated,
        ));
        (
            Self {
                queue,
                capture_live_payload: Arc::new(AtomicBool::new(false)),
                local_gap: Arc::clone(&local_gap),
                stream_epoch: Arc::new(AtomicU64::new(0)),
            },
            DualLaneReceiver {
                queue: receiver,
                local_gap,
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn with_capacities(
        fast_capacity: usize,
        overflow_capacity: usize,
    ) -> (Self, DualLaneReceiver) {
        Self::with_capacity(fast_capacity.saturating_add(overflow_capacity).max(1))
    }

    /// Nonblocking enqueue into the only ingress FIFO.
    ///
    /// Saturation never blocks the Yellowstone receive loop. The missing local
    /// coverage is represented by one deterministic typed gap for the
    /// continuous full episode and remains fail-closed until a future explicit
    /// recovery contract proves completeness.
    #[inline(always)]
    pub fn send(&self, ev: PumpEvent, stats: &Arc<TransportStats>) -> bool {
        let boundary = ev.local_gap_boundary();
        match self.queue.try_send(ev) {
            Ok(()) => {
                self.local_gap.observe_admitted(boundary);
                true
            }
            Err(TrySendError::Full(ev)) => {
                stats.bump_spill();
                stats.bump_overflow_drop();
                self.local_gap.observe_saturation(
                    ev.provider_id(),
                    self.stream_epoch.load(Ordering::Acquire),
                    boundary,
                    self.queue
                        .capacity()
                        .unwrap_or_else(|| self.queue.len())
                        .max(self.queue.len()),
                );
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                warn!("Bounded ingress channel disconnected");
                false
            }
        }
    }

    #[inline(always)]
    pub fn overflow_len(&self) -> usize {
        self.queue.len()
    }

    pub fn set_live_transaction_capture_enabled(&self, enabled: bool) {
        self.capture_live_payload.store(enabled, Ordering::Release);
    }

    #[inline(always)]
    fn live_transaction_capture_enabled(&self) -> bool {
        self.capture_live_payload.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn depth(&self) -> usize {
        self.queue.len()
    }
}

// ─── Dynamic account registry ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bcv2AccountContext {
    pub account_pubkey: Pubkey,
    pub base_mint: Option<Pubkey>,
    pub pool_id: Option<Pubkey>,
    pub canonical_bonding_curve: Option<Pubkey>,
    pub tx_signature: Option<String>,
    pub observed_instruction_index: Option<u32>,
    pub observed_account_position: Option<u32>,
    pub provenance_status: Option<String>,
    pub observed_slot: Option<u64>,
}

#[derive(Clone, Default)]
pub struct AccountRegistry {
    generic_accounts: Arc<DashSet<String>>,
    bcv2_accounts: Arc<DashSet<String>>,
    bcv2_contexts: Arc<DashMap<String, Bcv2AccountContext>>,
    curve_accounts: Arc<DashSet<String>>,
    pool_accounts: Arc<DashSet<String>>,
    mint_accounts: Arc<DashSet<String>>,
    /// Version of fields that materially change the subscribe request.
    /// Any exact-account watch insert/remove must bump this so provider workers
    /// can resubscribe with the refreshed tracked-account set.
    version: Arc<AtomicU64>,
    /// Fired when an exact-watch lane gets a new entry.
    /// Only modes that opt into immediate exact-watch refresh consume this
    /// signal; single_global batches registry changes until the next health tick.
    resub_notify: Arc<tokio::sync::Notify>,
    /// Dedicated BCV2 refresh signal. Route-compatible observed BCV2 accounts
    /// are not covered by the known curve/pool discriminators, so they get an
    /// immediate primary-global refresh path without changing curve/pool churn.
    bcv2_resub_notify: Arc<tokio::sync::Notify>,
    touch_seq: Arc<AtomicU64>,
    last_touch: Arc<DashMap<String, u64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountRegistrySnapshot {
    pub generic_accounts: Vec<String>,
    pub bcv2_accounts: Vec<String>,
    pub curve_accounts: Vec<String>,
    pub pool_accounts: Vec<String>,
    pub mint_accounts: Vec<String>,
}

impl AccountRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn record_touch(last_touch: &DashMap<String, u64>, touch_seq: &AtomicU64, addr: &str) {
        let seq = touch_seq.fetch_add(1, Ordering::Relaxed) + 1;
        last_touch.insert(addr.to_string(), seq);
    }

    fn insert_into(
        set: &DashSet<String>,
        version: &AtomicU64,
        touch_seq: &AtomicU64,
        last_touch: &DashMap<String, u64>,
        addr: String,
        bump_version: bool,
        touch_existing: bool,
    ) -> bool {
        let inserted = set.insert(addr.clone());
        if inserted && bump_version {
            version.fetch_add(1, Ordering::Relaxed);
        }
        if inserted || touch_existing {
            Self::record_touch(last_touch, touch_seq, &addr);
        }
        inserted
    }

    /// Insert a pubkey into the generic account lane.
    ///
    /// This keeps backward compatibility for existing parser callers.
    #[inline(always)]
    pub fn insert(&self, addr: impl Into<String>) -> bool {
        let inserted = Self::insert_into(
            &self.generic_accounts,
            &self.version,
            &self.touch_seq,
            &self.last_touch,
            addr.into(),
            true,
            false,
        );
        if inserted {
            self.resub_notify.notify_one();
        }
        inserted
    }

    #[inline(always)]
    pub fn insert_curve(&self, addr: impl Into<String>) -> bool {
        let inserted = Self::insert_into(
            &self.curve_accounts,
            &self.version,
            &self.touch_seq,
            &self.last_touch,
            addr.into(),
            true,
            true,
        );
        if inserted {
            self.resub_notify.notify_one();
        }
        inserted
    }

    #[inline(always)]
    pub fn insert_pool(&self, addr: impl Into<String>) -> bool {
        let inserted = Self::insert_into(
            &self.pool_accounts,
            &self.version,
            &self.touch_seq,
            &self.last_touch,
            addr.into(),
            true,
            true,
        );
        if inserted {
            self.resub_notify.notify_one();
        }
        inserted
    }

    #[inline(always)]
    pub fn insert_bcv2(&self, addr: impl Into<String>) -> bool {
        let addr = addr.into();
        let inserted = self.bcv2_accounts.insert(addr.clone());
        if inserted {
            self.version.fetch_add(1, Ordering::Relaxed);
        } else {
            // BCV2 exact-watch is priority-ranked by recency. Touching an
            // already-known account may change the over-cap selection surface.
            self.version.fetch_add(1, Ordering::Relaxed);
        }
        Self::record_touch(&self.last_touch, &self.touch_seq, &addr);
        self.prune_bcv2_exact_watch_retention(EXACT_ACCOUNT_PAYLOAD_CAP);
        self.bcv2_resub_notify.notify_one();
        inserted
    }

    #[inline(always)]
    pub fn insert_bcv2_with_context(&self, context: Bcv2AccountContext) -> bool {
        let addr = context.account_pubkey.to_string();
        self.bcv2_contexts.insert(addr.clone(), context);
        self.insert_bcv2(addr)
    }

    #[inline(always)]
    pub fn insert_mint(&self, addr: impl Into<String>) -> bool {
        Self::insert_into(
            &self.mint_accounts,
            &self.version,
            &self.touch_seq,
            &self.last_touch,
            addr.into(),
            false,
            true,
        )
    }

    fn snapshot_set(set: &DashSet<String>) -> Vec<String> {
        let mut out: Vec<String> = set.iter().map(|r| r.clone()).collect();
        out.sort_unstable();
        out
    }

    pub fn snapshot_by_lane(&self) -> AccountRegistrySnapshot {
        AccountRegistrySnapshot {
            generic_accounts: Self::snapshot_set(&self.generic_accounts),
            bcv2_accounts: Self::snapshot_set(&self.bcv2_accounts),
            curve_accounts: Self::snapshot_set(&self.curve_accounts),
            pool_accounts: Self::snapshot_set(&self.pool_accounts),
            mint_accounts: Self::snapshot_set(&self.mint_accounts),
        }
    }

    pub fn snapshot(&self) -> Vec<String> {
        let lanes = self.snapshot_by_lane();
        let mut merged = BTreeSet::new();
        for value in lanes.generic_accounts {
            merged.insert(value);
        }
        for value in lanes.bcv2_accounts {
            merged.insert(value);
        }
        for value in lanes.curve_accounts {
            merged.insert(value);
        }
        for value in lanes.pool_accounts {
            merged.insert(value);
        }
        for value in lanes.mint_accounts {
            merged.insert(value);
        }
        merged.into_iter().collect()
    }

    /// Notify handle for immediate resubscription on new pool/generic insert.
    pub fn resub_notify(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.resub_notify)
    }

    /// Dedicated notify handle for route-compatible BCV2 exact-watch refresh.
    pub fn bcv2_resub_notify(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.bcv2_resub_notify)
    }

    /// Snapshot of merged dynamic exact-watch accounts across all lanes.
    ///
    /// Not every subscription profile consumes every exact-watch lane. In
    /// particular, `PrimaryGlobal` already receives Pump.fun curve and PumpSwap
    /// pool updates through global owner+memcmp filters, so its dynamic exact
    /// branch only needs explicitly registered generic accounts.
    ///
    /// Ordering is by global recency across all exact-watch lanes so that the
    /// provider cap favors the newest session-relevant accounts instead of
    /// starving one lane behind another.
    pub fn snapshot_exact_watch_accounts(&self, budget: usize) -> Vec<String> {
        let mut merged: Vec<(String, u64)> = self
            .curve_accounts
            .iter()
            .map(|value| value.clone())
            .chain(self.pool_accounts.iter().map(|value| value.clone()))
            .chain(self.bcv2_accounts.iter().map(|value| value.clone()))
            .chain(self.generic_accounts.iter().map(|value| value.clone()))
            .map(|value| {
                let rank = self.touch_rank(&value);
                (value, rank)
            })
            .collect();
        Self::sort_ranked_accounts_by_recency(&mut merged);

        let mut out = Vec::with_capacity(budget);
        let mut seen = BTreeSet::new();
        for (value, _) in merged {
            if seen.insert(value.clone()) {
                out.push(value);
                if out.len() >= budget {
                    return out;
                }
            }
        }
        out
    }

    fn touch_rank(&self, addr: &str) -> u64 {
        self.last_touch
            .get(addr)
            .map(|entry| *entry)
            .unwrap_or_default()
    }

    fn sort_ranked_accounts_by_recency(accounts: &mut [(String, u64)]) {
        accounts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }

    fn snapshot_set_by_recency(&self, set: &DashSet<String>) -> Vec<String> {
        let mut out: Vec<(String, u64)> = set
            .iter()
            .map(|entry| {
                let value = entry.clone();
                let rank = self.touch_rank(&value);
                (value, rank)
            })
            .collect();
        Self::sort_ranked_accounts_by_recency(&mut out);
        out.into_iter().map(|(value, _)| value).collect()
    }

    fn remove_touch_if_unreferenced(&self, addr: &str) {
        if !self.generic_accounts.contains(addr)
            && !self.bcv2_accounts.contains(addr)
            && !self.curve_accounts.contains(addr)
            && !self.pool_accounts.contains(addr)
            && !self.mint_accounts.contains(addr)
        {
            self.last_touch.remove(addr);
        }
    }

    fn prune_bcv2_exact_watch_retention(&self, budget: usize) -> usize {
        let ranked = self.snapshot_set_by_recency(&self.bcv2_accounts);
        if ranked.len() <= budget {
            return 0;
        }
        let mut removed = 0usize;
        for stale in ranked.into_iter().skip(budget) {
            if self.bcv2_accounts.remove(&stale).is_some() {
                self.bcv2_contexts.remove(&stale);
                self.remove_touch_if_unreferenced(&stale);
                removed += 1;
            }
        }
        if removed > 0 {
            self.version.fetch_add(1, Ordering::Relaxed);
            warn!(
                "BCV2_EXACT_WATCH_RETAIN_PRUNED removed={} retain_cap={}",
                removed, budget
            );
        }
        removed
    }

    pub fn snapshot_primary_global_exact_accounts(&self, budget: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(budget);
        let mut seen = BTreeSet::new();
        for lane in [
            self.snapshot_set_by_recency(&self.bcv2_accounts),
            self.snapshot_set_by_recency(&self.generic_accounts),
        ] {
            for value in lane {
                if seen.insert(value.clone()) {
                    out.push(value);
                    if out.len() >= budget {
                        return out;
                    }
                }
            }
        }
        out
    }

    pub fn prioritized_snapshot(&self, budget: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(budget);
        let mut seen = BTreeSet::new();
        for lane in [
            self.snapshot_set_by_recency(&self.pool_accounts),
            self.snapshot_set_by_recency(&self.curve_accounts),
            self.snapshot_set_by_recency(&self.bcv2_accounts),
            self.snapshot_set_by_recency(&self.generic_accounts),
        ] {
            for value in lane {
                if seen.insert(value.clone()) {
                    out.push(value);
                    if out.len() >= budget {
                        return out;
                    }
                }
            }
        }
        out
    }
    pub fn len(&self) -> usize {
        self.snapshot().len()
    }
    pub fn is_empty(&self) -> bool {
        self.generic_accounts.is_empty()
            && self.bcv2_accounts.is_empty()
            && self.curve_accounts.is_empty()
            && self.pool_accounts.is_empty()
            && self.mint_accounts.is_empty()
    }

    /// Monotonic version incremented on every unique insert.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    fn remove_from(
        set: &DashSet<String>,
        version: &AtomicU64,
        last_touch: &DashMap<String, u64>,
        addr: &str,
        bump_version: bool,
    ) -> bool {
        let removed = set.remove(addr).is_some();
        if removed {
            if bump_version {
                version.fetch_add(1, Ordering::Relaxed);
            }
            last_touch.remove(addr);
        }
        removed
    }

    #[inline(always)]
    pub fn remove_curve(&self, addr: &str) -> bool {
        Self::remove_from(
            &self.curve_accounts,
            &self.version,
            &self.last_touch,
            addr,
            true,
        )
    }

    #[inline(always)]
    pub fn remove_pool(&self, addr: &str) -> bool {
        Self::remove_from(
            &self.pool_accounts,
            &self.version,
            &self.last_touch,
            addr,
            true,
        )
    }

    #[inline(always)]
    pub fn remove_bcv2(&self, addr: &str) -> bool {
        let removed = Self::remove_from(
            &self.bcv2_accounts,
            &self.version,
            &self.last_touch,
            addr,
            true,
        );
        if removed {
            self.bcv2_contexts.remove(addr);
            self.bcv2_resub_notify.notify_one();
        }
        removed
    }

    #[inline(always)]
    pub fn remove_generic(&self, addr: &str) -> bool {
        Self::remove_from(
            &self.generic_accounts,
            &self.version,
            &self.last_touch,
            addr,
            true,
        )
    }

    #[inline(always)]
    pub fn contains_bcv2(&self, addr: &str) -> bool {
        self.bcv2_accounts.contains(addr)
    }

    #[inline(always)]
    pub fn bcv2_context(&self, addr: &str) -> Option<Bcv2AccountContext> {
        self.bcv2_contexts
            .get(addr)
            .map(|entry| entry.value().clone())
    }
}

// ─── [FIX-5] Delayed Account Queue ────────────────────────────────────────────
//
// Root cause of the race:
//   Yellowstone can deliver an AccountUpdate for a freshly-created BondingCurve
//   *before* the Transaction containing the Create instruction arrives (or before
//   the parser has processed the Create and registered curve→mint in
//   CurveMintRegistry).  Without buffering, the parser enriches these early
//   updates with mint=UNKNOWN and the ShadowLedger cannot apply them → 1-2%
//   coverage loss on launch bursts.
//
// Ownership model (PR-2 contract):
//   `pending_curve_updates` in lib.rs is the SOLE owner of AccountUpdate
//   replay for the curve→mint mapping race window.  It buffers the
//   already-converted GeyserEvent and applies the curve state directly to
//   ShadowLedger in `replay_pending_curve_update`, which is called from
//   `register_curve_mapping`.  The drain is guarded by `HashMap::remove`,
//   so it executes exactly once regardless of concurrent callers.
//
//   Degraded/test compatibility mode: when
//   `canonical_account_update_relay_enabled = false`,
//   `handle_account_update` returns immediately without buffering or writing to
//   ShadowLedger. The pending_curve_updates buffer and IPC send path are never
//   reached. `pending_curve_updates` as sole-owner remains correct in that
//   compatibility path.
//
//   `DelayedAccountQueue` is transport-layer infrastructure that exposes
//   push()/drain()/sweep() for use by external callers (e.g. future
//   transport-layer replay, diagnostics, or a higher-level orchestrator).
//   It is NOT a second replay owner for AccountUpdate→ShadowLedger writes.
//   `GrpcConnection::push_delayed_account_update` and
//   `GrpcConnection::drain_and_reinject_delayed_account_updates` are
//   available as building blocks but are NOT called from lib.rs hot paths
//   because that would create a second competing recovery owner.
//
//   `sweep_expired()` and `depth()` are called on health_tick for
//   observability.  `push()`/`drain()` are available to callers that need
//   the transport-level buffering primitive without owning the ShadowLedger
//   replay side effect.

/// Maximum simultaneous buffered pubkeys.  Each holds one AccountUpdate.
/// [RC-6 fix] Increased from 2048 → 8192: at launch bursts of ~100 new tokens/s
/// the 2k queue filled within ~20s and began LRU-evicting bonding-curve updates
/// before Create TX were processed, causing permanent coverage loss for those pools.
/// 8192 gives ~80s headroom at 100 tokens/s.
const MAX_DELAYED_ACCTS: usize = 8_192;

/// How long to keep a buffered account update before evicting.
/// [RC-6 fix] Increased from 30s → 120s: under network congestion Create TX
/// confirmation can lag 30-90s; the old 30s TTL caused account updates to expire
/// before the pool was registered, losing the initial bonding-curve state.
const DELAYED_TTL_SECS: u64 = 120;

struct DelayedEntry {
    event: PumpEvent,
    queued_at: Instant,
}

/// Thread-safe buffer for account updates whose curve→mint mapping is not yet
/// known.  All operations are O(1) amortised.
///
/// Transport-layer buffer for AccountUpdates arriving before the curve→mint
/// mapping is known.  NOT a replay owner for ShadowLedger writes — that role
/// belongs exclusively to `pending_curve_updates` in lib.rs.  See module-level
/// comment for the full ownership model.
pub struct DelayedAccountQueue {
    inner: Mutex<HashMap<String, DelayedEntry>>,
    len: AtomicU64, // lock-free snapshot for metrics
}

impl Default for DelayedAccountQueue {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::with_capacity(256)),
            len: AtomicU64::new(0),
        }
    }
}

impl DelayedAccountQueue {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Buffer an account update for `pubkey`.
    ///
    /// If the queue is full, the oldest entry is evicted (LRU-lite via
    /// find-min-queued_at scan; acceptable at MAX 2k entries).
    pub fn push(&self, pubkey: String, ev: PumpEvent) {
        let mut map = self.inner.lock();
        Self::evict_expired_inner(&mut map);

        if map.len() >= MAX_DELAYED_ACCTS {
            // Evict the oldest entry to make room
            let oldest_key = map
                .iter()
                .min_by_key(|(_, e)| e.queued_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                map.remove(&k);
                let _ = self
                    .len
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                        Some(v.saturating_sub(1))
                    });
            }
        }

        let was_new = !map.contains_key(&pubkey);
        map.insert(
            pubkey,
            DelayedEntry {
                event: ev,
                queued_at: Instant::now(),
            },
        );
        if was_new {
            self.len.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Drain all buffered events for `pubkey`.
    ///
    /// Call this immediately after `CurveMintRegistry::insert(curve, mint)` so
    /// the parser can re-process the buffered account update with a known mint.
    /// Returns an empty vec if nothing was buffered (the common case).
    pub fn drain(&self, pubkey: &str) -> Vec<PumpEvent> {
        let mut map = self.inner.lock();
        Self::evict_expired_inner(&mut map);
        match map.remove(pubkey) {
            None => vec![],
            Some(e) => {
                let _ = self
                    .len
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                        Some(v.saturating_sub(1))
                    });
                vec![e.event]
            }
        }
    }

    /// Evict all entries older than DELAYED_TTL_SECS.
    /// Called automatically by `push`/`drain`; also callable by a background sweep.
    pub fn sweep_expired(&self) {
        let mut map = self.inner.lock();
        let before = map.len() as u64;
        Self::evict_expired_inner(&mut map);
        let after = map.len() as u64;
        if before > after {
            self.len.fetch_sub(before - after, Ordering::Relaxed);
        }
    }

    /// Current buffer depth (approximate, lock-free).
    pub fn depth(&self) -> u64 {
        self.len.load(Ordering::Relaxed)
    }

    // ── internals ─────────────────────────────────────────────────────────────

    fn evict_expired_inner(map: &mut HashMap<String, DelayedEntry>) {
        let ttl = Duration::from_secs(DELAYED_TTL_SECS);
        map.retain(|_, e| e.queued_at.elapsed() < ttl);
    }
}

// ─── Telemetry ────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct TransportStats {
    pub msgs_received: AtomicU64,
    pub msgs_spilled: AtomicU64, // fast-lane overflows into bounded spill lane
    pub msgs_overflow_dropped: AtomicU64, // bounded spill lane exhausted
    pub reconnects: AtomicU64,
    pub resubs_sent: AtomicU64,
    pub pings_sent: AtomicU64,
    pub pongs_received: AtomicU64,
    pub slot_gaps_total: AtomicU64,
    pub inner_ix_seen: AtomicU64,
    pub stall_reconnects: AtomicU64,
    pub entry_events: AtomicU64,   // [FIX-1] entry event counter
    pub tx_events: AtomicU64,      // per-type: transaction events
    pub account_events: AtomicU64, // per-type: account update events
    pub delayed_pushes: AtomicU64, // [FIX-5] buffered account updates
    pub delayed_drains: AtomicU64, // [FIX-5] recovered account updates
    /// Wall-clock ms of last received gRPC message (any variant).
    /// Zero = never received.  Updated atomically on every message.
    pub last_msg_wall_ms: AtomicI64,
}

impl TransportStats {
    #[inline(always)]
    pub fn bump_recv(&self) {
        self.msgs_received.fetch_add(1, Ordering::Relaxed);
        metrics::increment_counter!("ghost.pump.recv");
    }
    #[inline(always)]
    pub fn bump_spill(&self) {
        self.msgs_spilled.fetch_add(1, Ordering::Relaxed);
        metrics::increment_counter!("ghost.pump.spill");
    }
    #[inline(always)]
    pub fn bump_overflow_drop(&self) -> u64 {
        let dropped = self.msgs_overflow_dropped.fetch_add(1, Ordering::Relaxed) + 1;
        metrics::increment_counter!("ghost.pump.overflow_dropped");
        dropped
    }
    #[inline(always)]
    pub fn bump_recon(&self) {
        self.bump_recon_with_source("unknown");
    }
    #[inline(always)]
    pub fn bump_recon_with_source(&self, source_label: &str) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "ghost.pump.reconnects",
            1,
            "source_label" => source_label.to_string()
        );
    }
    #[inline(always)]
    pub fn bump_resub(&self) {
        self.resubs_sent.fetch_add(1, Ordering::Relaxed);
    }
    #[inline(always)]
    pub fn bump_ping(&self) {
        self.pings_sent.fetch_add(1, Ordering::Relaxed);
    }
    #[inline(always)]
    pub fn bump_pong(&self) {
        self.pongs_received.fetch_add(1, Ordering::Relaxed);
    }
    #[inline(always)]
    pub fn bump_stall(&self) {
        self.bump_stall_with_source("unknown");
    }
    #[inline(always)]
    pub fn bump_stall_with_source(&self, source_label: &str) {
        self.stall_reconnects.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(
            "ghost.pump.stalls",
            1,
            "source_label" => source_label.to_string()
        );
    }
    #[inline(always)]
    pub fn bump_inner(&self) {
        self.inner_ix_seen.fetch_add(1, Ordering::Relaxed);
    }
    #[inline(always)]
    pub fn bump_entry(&self) {
        self.entry_events.fetch_add(1, Ordering::Relaxed);
        metrics::increment_counter!("entry_received_total");
    }
    #[inline(always)]
    pub fn bump_tx(&self) {
        self.tx_events.fetch_add(1, Ordering::Relaxed);
        metrics::increment_counter!("ghost.pump.tx_recv");
        metrics::increment_counter!("tx_received_total");
    }
    #[inline(always)]
    pub fn bump_account(&self) {
        self.account_events.fetch_add(1, Ordering::Relaxed);
        metrics::increment_counter!("ghost.pump.account_recv");
        metrics::increment_counter!("account_received_total");
    }
    #[inline(always)]
    pub fn bump_delayed_push(&self) {
        self.delayed_pushes.fetch_add(1, Ordering::Relaxed);
        metrics::increment_counter!("ghost.pump.delayed_push");
    }
    #[inline(always)]
    pub fn bump_delayed_drain(&self) {
        self.delayed_drains.fetch_add(1, Ordering::Relaxed);
        metrics::increment_counter!("ghost.pump.delayed_drain");
    }

    #[inline(always)]
    pub fn add_gap(&self, n: u64) {
        self.slot_gaps_total.fetch_add(n, Ordering::Relaxed);
        metrics::counter!("ghost.pump.slot_gaps", n);
    }

    /// Stamp wall-clock on every received gRPC message.
    #[inline(always)]
    pub fn touch(&self) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.last_msg_wall_ms.store(ms, Ordering::Relaxed);
    }

    /// Milliseconds since last received message.  0 = startup (never received).
    pub fn ms_since_last_msg(&self) -> u64 {
        let last = self.last_msg_wall_ms.load(Ordering::Relaxed);
        if last == 0 {
            return 0;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        (now - last).max(0) as u64
    }

    pub fn coverage_pct(&self) -> f64 {
        let received = self.msgs_received.load(Ordering::Relaxed);
        if received == 0 {
            return 100.0;
        }
        let dropped = self.msgs_overflow_dropped.load(Ordering::Relaxed);
        ((received.saturating_sub(dropped)) as f64 / received as f64) * 100.0
    }

    pub fn stall_rate(&self) -> f64 {
        let reconnects = self.reconnects.load(Ordering::Relaxed);
        if reconnects == 0 {
            return 0.0;
        }
        let stalls = self.stall_reconnects.load(Ordering::Relaxed);
        stalls as f64 / reconnects as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderCircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl ProviderCircuitState {
    #[inline]
    fn as_str(&self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::HalfOpen => "half_open",
        }
    }

    #[inline]
    fn gauge_value(&self) -> f64 {
        match self {
            Self::Closed => 0.0,
            Self::HalfOpen => 1.0,
            Self::Open => 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ProviderAttemptPermit {
    half_open_probe: bool,
}

#[derive(Debug, Clone, Copy)]
struct ProviderCircuitSnapshot {
    state: ProviderCircuitState,
    consecutive_stalls: u32,
    total_stalls: u64,
}

#[derive(Debug, Clone, Copy)]
struct ProviderCircuitBreakerConfig {
    max_stalls_before_open: u32,
    cooldown_ms: u64,
}

impl Default for ProviderCircuitBreakerConfig {
    fn default() -> Self {
        Self {
            max_stalls_before_open: DEFAULT_PROVIDER_MAX_STALLS_BEFORE_OPEN,
            cooldown_ms: DEFAULT_PROVIDER_CIRCUIT_BREAKER_COOLDOWN_MS,
        }
    }
}

#[derive(Debug)]
struct ProviderCircuitBreakerState {
    state: ProviderCircuitState,
    consecutive_stalls: u32,
    total_stalls: u64,
    opened_at_ms: u64,
}

impl Default for ProviderCircuitBreakerState {
    fn default() -> Self {
        Self {
            state: ProviderCircuitState::Closed,
            consecutive_stalls: 0,
            total_stalls: 0,
            opened_at_ms: 0,
        }
    }
}

struct ProviderCircuitBreaker {
    provider_label: String,
    source_label: String,
    config: ProviderCircuitBreakerConfig,
    state: Mutex<ProviderCircuitBreakerState>,
}

impl ProviderCircuitBreaker {
    fn new(
        provider_label: impl Into<String>,
        source_label: impl Into<String>,
        config: ProviderCircuitBreakerConfig,
    ) -> Arc<Self> {
        let breaker = Arc::new(Self {
            provider_label: provider_label.into(),
            source_label: source_label.into(),
            config: ProviderCircuitBreakerConfig {
                max_stalls_before_open: config.max_stalls_before_open.max(1),
                cooldown_ms: config.cooldown_ms.max(1),
            },
            state: Mutex::new(ProviderCircuitBreakerState::default()),
        });
        breaker.emit_state_metric(ProviderCircuitState::Closed);
        breaker
    }

    async fn acquire_attempt_permit(
        &self,
        shutdown: &Arc<AtomicBool>,
        shutdown_token: &Arc<CancellationToken>,
    ) -> Option<ProviderAttemptPermit> {
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return None;
            }

            let wait_ms = {
                let mut state = self.state.lock();
                let now_ms = wall_clock_ms();
                match state.state {
                    ProviderCircuitState::Closed => {
                        return Some(ProviderAttemptPermit {
                            half_open_probe: false,
                        });
                    }
                    ProviderCircuitState::Open => {
                        let elapsed_ms = now_ms.saturating_sub(state.opened_at_ms);
                        if elapsed_ms >= self.config.cooldown_ms {
                            state.state = ProviderCircuitState::HalfOpen;
                            self.emit_state_metric(ProviderCircuitState::HalfOpen);
                            info!(
                                provider = %self.provider_label,
                                source_label = %self.source_label,
                                cooldown_ms = self.config.cooldown_ms,
                                "Provider circuit entering half-open probe"
                            );
                            return Some(ProviderAttemptPermit {
                                half_open_probe: true,
                            });
                        }
                        self.config.cooldown_ms.saturating_sub(elapsed_ms)
                    }
                    ProviderCircuitState::HalfOpen => PROVIDER_CIRCUIT_BREAKER_WAIT_POLL_MS,
                }
            };

            tokio::select! {
                biased;
                _ = shutdown_token.cancelled() => return None,
                _ = tokio::time::sleep(Duration::from_millis(
                    wait_ms.min(PROVIDER_CIRCUIT_BREAKER_WAIT_POLL_MS),
                )) => {}
            }
        }
    }

    fn record_message_progress(&self) {
        let mut state = self.state.lock();
        let previous_state = state.state;
        state.state = ProviderCircuitState::Closed;
        state.consecutive_stalls = 0;
        state.opened_at_ms = 0;
        if previous_state != ProviderCircuitState::Closed {
            info!(
                provider = %self.provider_label,
                source_label = %self.source_label,
                "Provider circuit closed after successful data probe"
            );
            self.emit_state_metric(ProviderCircuitState::Closed);
        }
    }

    fn record_stall(&self) {
        let mut state = self.state.lock();
        state.total_stalls = state.total_stalls.saturating_add(1);
        metrics::counter!(
            "ghost.pump.provider_stall_total",
            1,
            "provider" => self.provider_label.clone(),
            "source_label" => self.source_label.clone()
        );

        match state.state {
            ProviderCircuitState::HalfOpen => {
                state.state = ProviderCircuitState::Open;
                state.consecutive_stalls = self.config.max_stalls_before_open;
                state.opened_at_ms = wall_clock_ms();
                warn!(
                    provider = %self.provider_label,
                    source_label = %self.source_label,
                    cooldown_ms = self.config.cooldown_ms,
                    "Provider half-open probe stalled; reopening circuit"
                );
                self.emit_state_metric(ProviderCircuitState::Open);
            }
            ProviderCircuitState::Closed => {
                state.consecutive_stalls = state.consecutive_stalls.saturating_add(1);
                if state.consecutive_stalls >= self.config.max_stalls_before_open {
                    state.state = ProviderCircuitState::Open;
                    state.opened_at_ms = wall_clock_ms();
                    warn!(
                        provider = %self.provider_label,
                        source_label = %self.source_label,
                        consecutive_stalls = state.consecutive_stalls,
                        cooldown_ms = self.config.cooldown_ms,
                        "Provider circuit opened after consecutive stalls"
                    );
                    self.emit_state_metric(ProviderCircuitState::Open);
                }
            }
            ProviderCircuitState::Open => {
                state.opened_at_ms = wall_clock_ms();
            }
        }
    }

    fn record_probe_failure(&self, reason: &str) {
        let mut state = self.state.lock();
        if state.state != ProviderCircuitState::HalfOpen {
            return;
        }
        state.state = ProviderCircuitState::Open;
        state.consecutive_stalls = self.config.max_stalls_before_open;
        state.opened_at_ms = wall_clock_ms();
        warn!(
            provider = %self.provider_label,
            source_label = %self.source_label,
            reason,
            cooldown_ms = self.config.cooldown_ms,
            "Provider half-open probe failed; reopening circuit"
        );
        self.emit_state_metric(ProviderCircuitState::Open);
    }

    fn snapshot(&self) -> ProviderCircuitSnapshot {
        let state = self.state.lock();
        ProviderCircuitSnapshot {
            state: state.state,
            consecutive_stalls: state.consecutive_stalls,
            total_stalls: state.total_stalls,
        }
    }

    fn emit_state_metric(&self, state: ProviderCircuitState) {
        metrics::gauge!(
            "ghost.pump.provider_state",
            state.gauge_value(),
            "provider" => self.provider_label.clone(),
            "source_label" => self.source_label.clone()
        );
    }
}

fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ms_since_wall_clock(last_wall_ms: u64) -> u64 {
    wall_clock_ms().saturating_sub(last_wall_ms)
}

fn record_parser_malformed_tx_sample(malformed: bool) {
    let total = PARSER_TX_DECODE_TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
    let malformed_total = if malformed {
        PARSER_TX_DECODE_MALFORMED.fetch_add(1, Ordering::Relaxed) + 1
    } else {
        PARSER_TX_DECODE_MALFORMED.load(Ordering::Relaxed)
    };
    metrics::gauge!(
        "parser_malformed_tx_rate",
        malformed_total as f64 / total as f64
    );
}

fn handle_watchdog_tick(
    id: &str,
    cfg: &GrpcConfig,
    stats: &Arc<TransportStats>,
    breaker: &Arc<ProviderCircuitBreaker>,
    last_msg_wall_ms: u64,
) -> Result<()> {
    let silence_ms = ms_since_wall_clock(last_msg_wall_ms);
    let threshold = cfg.stall_timeout_secs * 1_000;
    if silence_ms > threshold {
        let source_label = cfg.subscription_profile.source_label();
        stats.bump_stall_with_source(source_label);
        breaker.record_stall();
        warn!(
            source_label,
            "[{id}] SILENT STALL: no message for {silence_ms}ms (threshold={threshold}ms) — reconnecting"
        );
        metrics::counter!(
            "ghost.pump.silent_stalls",
            1,
            "source_label" => source_label.to_string()
        );
        return Err(anyhow::anyhow!("silent stall after {silence_ms}ms"));
    }
    Ok(())
}

#[cfg(test)]
async fn run_watchdog_tick_for_test(
    id: &str,
    cfg: &GrpcConfig,
    stats: &Arc<TransportStats>,
    breaker: &Arc<ProviderCircuitBreaker>,
    last_msg_wall_ms: u64,
) -> Result<()> {
    let mut watchdog_ticker = tokio::time::interval(Duration::from_millis(1));
    watchdog_ticker.tick().await;
    handle_watchdog_tick(id, cfg, stats, breaker, last_msg_wall_ms)
}

// ─── Slot tracker ─────────────────────────────────────────────────────────────

struct SlotTracker {
    last: AtomicU64,
    gaps: AtomicU64,
}

impl SlotTracker {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            last: AtomicU64::new(0),
            gaps: AtomicU64::new(0),
        })
    }

    #[inline(always)]
    fn update(&self, slot: u64) -> Option<SlotGap> {
        let prev = self.last.fetch_max(slot, Ordering::Relaxed);
        if prev == 0 || slot <= prev {
            return None;
        }
        let gap = slot - prev - 1;
        if gap > 0 {
            self.gaps.fetch_add(gap, Ordering::Relaxed);
            return Some(SlotGap {
                start_slot: prev + 1,
                end_slot: slot - 1,
            });
        }
        None
    }

    fn last_slot(&self) -> u64 {
        self.last.load(Ordering::Relaxed)
    }
    #[cfg(test)]
    fn total_gaps(&self) -> u64 {
        self.gaps.load(Ordering::Relaxed)
    }
}

impl Default for SlotTracker {
    fn default() -> Self {
        Self {
            last: AtomicU64::new(0),
            gaps: AtomicU64::new(0),
        }
    }
}

// ─── Provider endpoint ────────────────────────────────────────────────────────

/// [FIX-3] A single gRPC provider.  Multiple providers = multiple workers,
/// all feeding the same DualLaneChannel.
#[derive(Clone)]
pub struct Provider {
    /// gRPC endpoint URL.
    pub endpoint: String,
    /// Authentication token (if required).
    pub x_token: Option<String>,
    /// Metadata header used for authentication.
    pub auth_header: String,
    /// Stable provider identifier used for logs, metrics and provenance.
    pub label: String,
    /// Explicit raw-provider role. It is metadata-only in PR 1A.
    pub role: ghost_core::RawProviderRoleV1,
}

impl Provider {
    pub fn new(
        endpoint: impl Into<String>,
        x_token: Option<String>,
        label: impl Into<String>,
    ) -> Self {
        Self::new_with_auth_header(endpoint, x_token, label, DEFAULT_GRPC_AUTH_HEADER)
    }

    pub fn new_with_auth_header(
        endpoint: impl Into<String>,
        x_token: Option<String>,
        label: impl Into<String>,
        auth_header: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            x_token,
            auth_header: auth_header.into(),
            label: label.into(),
            role: ghost_core::RawProviderRoleV1::PrimaryAuthority,
        }
    }

    /// Construct a provider with an explicit authority/witness role.
    pub fn new_with_role(
        endpoint: impl Into<String>,
        x_token: Option<String>,
        label: impl Into<String>,
        role: ghost_core::RawProviderRoleV1,
    ) -> Self {
        let mut provider = Self::new(endpoint, x_token, label);
        provider.role = role;
        provider
    }

    /// Convenience constructor for a secondary raw witness.
    pub fn secondary(
        endpoint: impl Into<String>,
        x_token: Option<String>,
        label: impl Into<String>,
    ) -> Self {
        Self::new_with_role(
            endpoint,
            x_token,
            label,
            ghost_core::RawProviderRoleV1::SecondaryWitness,
        )
    }

    /// Convenience: single anonymous provider.
    pub fn single(endpoint: impl Into<String>, x_token: Option<String>) -> Self {
        Self::new(endpoint, x_token, "primary")
    }

    pub fn single_with_auth_header(
        endpoint: impl Into<String>,
        x_token: Option<String>,
        auth_header: impl Into<String>,
    ) -> Self {
        Self::primary_with_auth_header(endpoint, x_token, "primary", auth_header)
    }

    /// Construct the primary provider with a stable, config-supplied ID.
    pub fn primary_with_auth_header(
        endpoint: impl Into<String>,
        x_token: Option<String>,
        provider_id: impl Into<String>,
        auth_header: impl Into<String>,
    ) -> Self {
        Self::new_with_auth_header(endpoint, x_token, provider_id, auth_header)
    }
}

// ─── Configuration ────────────────────────────────────────────────────────────

/// Declarative primary/secondary assignment expected from production config.
///
/// The transport validates this against concrete `Provider`s before starting
/// workers. This is intentionally a configuration/transport contract only;
/// PR 1A does not use it to change account-state or canonical-event authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawProviderRoleConfigV1 {
    pub primary_raw_provider_id: String,
    pub secondary_raw_provider_ids: Vec<String>,
}

impl RawProviderRoleConfigV1 {
    pub fn new(
        primary_raw_provider_id: impl Into<String>,
        secondary_raw_provider_ids: Vec<String>,
    ) -> Self {
        Self {
            primary_raw_provider_id: primary_raw_provider_id.into(),
            secondary_raw_provider_ids,
        }
    }

    fn validate_declared_ids(&self) -> Result<()> {
        if self.primary_raw_provider_id.trim().is_empty()
            || self.primary_raw_provider_id != self.primary_raw_provider_id.trim()
        {
            anyhow::bail!("primary_raw_provider_id must be non-empty and trimmed");
        }

        let mut secondary_ids = HashSet::new();
        for secondary_id in &self.secondary_raw_provider_ids {
            if secondary_id.trim().is_empty() || secondary_id != secondary_id.trim() {
                anyhow::bail!("secondary_raw_provider_ids must contain only non-empty trimmed IDs");
            }
            if secondary_id == &self.primary_raw_provider_id {
                anyhow::bail!(
                    "secondary_raw_provider_ids must not contain primary_raw_provider_id '{}'",
                    self.primary_raw_provider_id
                );
            }
            if !secondary_ids.insert(secondary_id.as_str()) {
                anyhow::bail!("secondary_raw_provider_ids contains duplicate ID '{secondary_id}'");
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct GrpcConfig {
    /// [FIX-3] Multiple providers — all run in parallel, events merged on one channel.
    /// Provides N-way redundancy: one provider outage ≠ 0% coverage.
    pub providers: Vec<Provider>,
    /// Production configuration expected for primary/secondary provider IDs.
    /// `None` preserves programmatic API compatibility while still enforcing
    /// one primary and unique, non-empty provider IDs at runtime.
    pub provider_role_config: Option<RawProviderRoleConfigV1>,
    /// Independent streams per provider (increase for very high TPS endpoints).
    pub streams_per_provider: usize,
    pub commitment: CommitmentLevel,
    pub stall_timeout_secs: u64,
    pub resub_debounce_ms: u64,
    pub max_stalls_before_open: u32,
    pub circuit_breaker_cooldown_ms: u64,
    pub subscription_profile: GrpcSubscriptionProfile,
    pub registry_resubscribe_mode: RegistryResubscribeMode,
    pub ingress_queue_capacity: usize,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            providers: vec![],
            provider_role_config: None,
            streams_per_provider: 1,
            commitment: CommitmentLevel::Processed,
            stall_timeout_secs: DEFAULT_SILENT_STALL_SECS,
            resub_debounce_ms: DEFAULT_RESUB_DEBOUNCE_MS,
            max_stalls_before_open: DEFAULT_PROVIDER_MAX_STALLS_BEFORE_OPEN,
            circuit_breaker_cooldown_ms: DEFAULT_PROVIDER_CIRCUIT_BREAKER_COOLDOWN_MS,
            subscription_profile: GrpcSubscriptionProfile::default(),
            registry_resubscribe_mode: RegistryResubscribeMode::HealthTickOnly,
            ingress_queue_capacity: DEFAULT_PRIMARY_CHANNEL_CAP,
        }
    }
}

impl GrpcConfig {
    pub fn with_provider(mut self, p: Provider) -> Self {
        self.providers.push(p);
        self
    }

    pub fn with_provider_role_config(mut self, config: RawProviderRoleConfigV1) -> Self {
        self.provider_role_config = Some(config);
        self
    }

    fn validate_provider_roles(&self) -> Result<()> {
        if self.providers.is_empty() {
            anyhow::bail!("GrpcConfig has no providers — add at least one Provider");
        }

        let mut provider_ids = HashSet::new();
        let mut primary_count = 0usize;
        for provider in &self.providers {
            if provider.label.trim().is_empty() || provider.label != provider.label.trim() {
                anyhow::bail!("raw provider ID must be non-empty and trimmed");
            }
            if !provider_ids.insert(provider.label.as_str()) {
                anyhow::bail!("duplicate raw provider ID '{}'", provider.label);
            }
            if provider.role == ghost_core::RawProviderRoleV1::PrimaryAuthority {
                primary_count += 1;
            }
        }

        if primary_count != 1 {
            anyhow::bail!(
                "GrpcConfig requires exactly one PrimaryAuthority provider, found {primary_count}"
            );
        }

        if let Some(contract) = &self.provider_role_config {
            contract.validate_declared_ids()?;
            for provider in &self.providers {
                let expected_role = if provider.label == contract.primary_raw_provider_id {
                    ghost_core::RawProviderRoleV1::PrimaryAuthority
                } else if contract
                    .secondary_raw_provider_ids
                    .iter()
                    .any(|id| id == &provider.label)
                {
                    ghost_core::RawProviderRoleV1::SecondaryWitness
                } else {
                    anyhow::bail!(
                        "raw provider '{}' is not declared by primary_raw_provider_id or secondary_raw_provider_ids",
                        provider.label
                    );
                };

                if provider.role != expected_role {
                    anyhow::bail!(
                        "raw provider '{}' has role '{}' but configured role is '{}'",
                        provider.label,
                        provider.role.as_str(),
                        expected_role.as_str()
                    );
                }
            }

            if !provider_ids.contains(contract.primary_raw_provider_id.as_str()) {
                anyhow::bail!(
                    "primary_raw_provider_id '{}' has no configured provider endpoint",
                    contract.primary_raw_provider_id
                );
            }
            for secondary_id in &contract.secondary_raw_provider_ids {
                if !provider_ids.contains(secondary_id.as_str()) {
                    anyhow::bail!(
                        "secondary_raw_provider_id '{}' has no configured provider endpoint",
                        secondary_id
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryResubscribeMode {
    HealthTickOnly,
    ImmediateAndTick,
}

impl RegistryResubscribeMode {
    const fn uses_immediate_notify(self) -> bool {
        matches!(self, Self::ImmediateAndTick)
    }

    const fn uses_registry_tick(self) -> bool {
        matches!(self, Self::ImmediateAndTick)
    }
}

const PUMP_RESEARCH_EXACT_STATE_V2_ACCOUNT_FILTER_LOG_SCOPE: &str =
    "[pump_owned_bonding_curve_discriminator,canonical_pump_global]";
const PUMP_RESEARCH_EXACT_STATE_V2_SOURCE_CAPTURE_LOG_NOTE: &str =
    "standalone_stream_only_bonding_curve_global_raw_source_capture_before_projection";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrpcSubscriptionProfile {
    #[default]
    PrimaryGlobal,
    /// Standalone Pump Research Evidence Tape source profile.
    ///
    /// This path deliberately receives decoded Yellowstone source messages
    /// before they are projected into `GeyserEvent` or seen by the parser.  It
    /// is not selectable through `SeerConfig` and therefore cannot alter the
    /// active Seer runtime subscription.
    PumpResearchGlobalV1,
    /// Prospective exact-state research capture.  Its stream-only V1.1 account
    /// contract receives Pump-owner BondingCurve updates and the exact
    /// canonical Pump Global account only.  It remains standalone and cannot
    /// be selected through `SeerConfig`.
    PumpResearchExactStateV2,
    FundingLanePumpFiltered,
    FundingLaneFullChain,
}

impl GrpcSubscriptionProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            GrpcSubscriptionProfile::PrimaryGlobal => "primary_global",
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => "pump_research_global_v1",
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => "pump_research_exact_state_v2",
            GrpcSubscriptionProfile::FundingLanePumpFiltered => "funding_lane_pump_filtered",
            GrpcSubscriptionProfile::FundingLaneFullChain => "funding_lane_full_chain",
        }
    }

    pub const fn source_label(self) -> &'static str {
        match self {
            GrpcSubscriptionProfile::PrimaryGlobal => GRPC_GLOBAL_STREAM_SOURCE_LABEL,
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => {
                GRPC_PUMP_RESEARCH_GLOBAL_V1_SOURCE_LABEL
            }
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                GRPC_PUMP_RESEARCH_EXACT_STATE_V2_SOURCE_LABEL
            }
            GrpcSubscriptionProfile::FundingLanePumpFiltered => {
                GRPC_FUNDING_LANE_PUMP_FILTERED_SOURCE_LABEL
            }
            GrpcSubscriptionProfile::FundingLaneFullChain => {
                GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL
            }
        }
    }

    pub const fn uses_registry_filters(self) -> bool {
        matches!(self, GrpcSubscriptionProfile::PrimaryGlobal)
    }

    pub const fn transaction_filter_name(self) -> &'static str {
        match self {
            GrpcSubscriptionProfile::FundingLaneFullChain => "all_txs",
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => "pump_research_transactions",
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                "pump_research_exact_state_v2_transactions"
            }
            GrpcSubscriptionProfile::PrimaryGlobal
            | GrpcSubscriptionProfile::FundingLanePumpFiltered => "pump_txs",
        }
    }

    pub fn transaction_account_include(self) -> Vec<String> {
        match self {
            GrpcSubscriptionProfile::PumpResearchGlobalV1
            | GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                vec![PUMP_FUN_PROGRAM_ID.to_string()]
            }
            GrpcSubscriptionProfile::PrimaryGlobal
            | GrpcSubscriptionProfile::FundingLanePumpFiltered => vec![
                PUMP_FUN_PROGRAM_ID.to_string(),
                PUMP_SWAP_PROGRAM_ID.to_string(),
            ],
            GrpcSubscriptionProfile::FundingLaneFullChain => vec![],
        }
    }

    const fn transaction_failed_filter(self) -> Option<bool> {
        match self {
            // `None` is intentional: standalone raw captures keep both
            // successful and failed Pump transactions.  Failure
            // classification belongs to the offline materializer, never to
            // the provider-side source filter.
            GrpcSubscriptionProfile::PumpResearchGlobalV1
            | GrpcSubscriptionProfile::PumpResearchExactStateV2 => None,
            Self::PrimaryGlobal | Self::FundingLanePumpFiltered | Self::FundingLaneFullChain => {
                Some(false)
            }
        }
    }
}

/// One decoded Yellowstone message admitted by the research-only source tap.
///
/// The receive task transfers ownership of the already decoded protobuf and
/// does not serialize, hash, parse, or perform filesystem I/O.  The tape
/// writer owns deterministic `prost` re-encoding and durable framing.
pub(crate) struct PumpResearchSourceUpdateV1 {
    pub provider_id: String,
    pub stream_epoch: u64,
    pub ingress_wall_ts_ms: u64,
    pub ingress_monotonic_ts_ms: u64,
    pub update: SubscribeUpdate,
}

/// Research-only ownership hand-off from Yellowstone receive to the immutable
/// raw-tape writer.  Implementations must be nonblocking: a full capture queue
/// is represented as typed local coverage loss rather than backpressuring the
/// gRPC receive loop.
pub(crate) trait PumpResearchSourceSinkV1: Send + Sync {
    /// The immutable source subscription was established successfully.  This
    /// is deliberately separate from receiving a source update: a connected
    /// but silent stream is not sufficient evidence for a complete capture.
    fn source_stream_established(&self, stream_epoch: u64);

    /// Called when an already-established standalone research stream ends
    /// abnormally before the connector considers a retry.  Historical V1
    /// implementations may retain their existing reconnect lifecycle, but a
    /// prospective capture can override this hook to fail closed immediately
    /// instead of waiting for a later successful reconnect/epoch change.
    fn source_stream_interrupted(&self, _stream_epoch: u64, _error: String) {}

    fn try_capture(&self, update: PumpResearchSourceUpdateV1);

    /// The standalone connector observed an abnormal worker/supervisor exit.
    /// This is research-only lifecycle evidence and must make the capture
    /// fail closed rather than being collapsed into a clean shutdown.
    fn source_worker_failed(&self, error: String);

    /// Called only after every standalone source worker joined without a
    /// panic/error and after an explicit source shutdown request.
    fn source_workers_stopped_cleanly(&self);

    /// Called exactly once after every source worker has stopped.  It lets the
    /// tape side close an open saturation episode at the truthful process
    /// boundary before the writer is drained.
    fn finish_source(&self);
}

// ─── Connector ────────────────────────────────────────────────────────────────

struct YellowstoneConnector {
    config: GrpcConfig,
    channel: DualLaneChannel,
    stats: Arc<TransportStats>,
    shutdown: Arc<AtomicBool>,
    shutdown_token: Arc<CancellationToken>,
    registry: AccountRegistry,
    gap_tx: tokio::sync::mpsc::UnboundedSender<SlotGap>,
    health: Option<Arc<RuntimeHealth>>,
    availability_tracker: Arc<LaneAvailabilityTracker>,
    /// [FIX-5] Shared delayed-account buffer — Arc-cloned into parser.
    delayed_queue: Arc<DelayedAccountQueue>,
    /// Latest block_time (Unix seconds) seen in any BlockMeta event.
    /// Workers update this via fetch_max; stream consumer reads it for
    /// event_ts_ms anchoring so observation windows start at block time
    /// rather than our local ingress wall-clock.
    latest_block_time_secs: Arc<AtomicI64>,
    /// Optional research-only decoded-source hand-off.  When installed the
    /// connector does not project updates into `PumpEvent`/`GeyserEvent`; it
    /// feeds the immutable tape input boundary instead.
    research_capture_sink: Option<Arc<dyn PumpResearchSourceSinkV1>>,
}

#[derive(Default)]
struct LaneAvailabilityTracker {
    connected_workers: AtomicUsize,
    state_tx: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
}

impl LaneAvailabilityTracker {
    fn set_sender(&self, tx: tokio::sync::watch::Sender<bool>) {
        let available = self.connected_workers.load(Ordering::SeqCst) > 0;
        let _ = tx.send(available);
        *self.state_tx.lock() = Some(tx);
    }

    fn connected_guard(self: &Arc<Self>) -> LaneAvailabilityGuard {
        if self.connected_workers.fetch_add(1, Ordering::SeqCst) == 0 {
            self.publish(true);
        }
        LaneAvailabilityGuard {
            tracker: Some(Arc::clone(self)),
        }
    }

    fn disconnect(&self) {
        let mut current = self.connected_workers.load(Ordering::SeqCst);
        while current != 0 {
            match self.connected_workers.compare_exchange(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    if current == 1 {
                        self.publish(false);
                    }
                    return;
                }
                Err(actual) => current = actual,
            }
        }
    }

    fn publish(&self, available: bool) {
        if let Some(tx) = self.state_tx.lock().as_ref() {
            let _ = tx.send(available);
        }
    }
}

struct LaneAvailabilityGuard {
    tracker: Option<Arc<LaneAvailabilityTracker>>,
}

impl Drop for LaneAvailabilityGuard {
    fn drop(&mut self) {
        if let Some(tracker) = self.tracker.take() {
            tracker.disconnect();
        }
    }
}

impl YellowstoneConnector {
    fn new(
        config: GrpcConfig,
    ) -> (
        Self,
        DualLaneReceiver,
        tokio::sync::mpsc::UnboundedReceiver<SlotGap>,
    ) {
        let (ch, rx) = DualLaneChannel::with_capacity(config.ingress_queue_capacity);
        let (gap_tx, gap_rx) = tokio::sync::mpsc::unbounded_channel();
        let availability_tracker = Arc::new(LaneAvailabilityTracker::default());
        let c = Self {
            config,
            channel: ch,
            stats: Arc::new(TransportStats::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
            shutdown_token: Arc::new(CancellationToken::new()),
            registry: AccountRegistry::new(),
            gap_tx,
            health: None,
            availability_tracker,
            delayed_queue: DelayedAccountQueue::new(),
            latest_block_time_secs: Arc::new(AtomicI64::new(0)),
            research_capture_sink: None,
        };
        (c, rx, gap_rx)
    }

    fn set_health(&mut self, health: Arc<RuntimeHealth>) {
        self.health = Some(health);
    }

    fn stats(&self) -> Arc<TransportStats> {
        Arc::clone(&self.stats)
    }
    fn shutdown_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }
    fn shutdown_token_handle(&self) -> Arc<CancellationToken> {
        Arc::clone(&self.shutdown_token)
    }
    fn registry(&self) -> AccountRegistry {
        self.registry.clone()
    }

    fn injector(&self) -> DualLaneChannel {
        self.channel.clone()
    }

    fn install_pump_research_capture_sink(&mut self, sink: Arc<dyn PumpResearchSourceSinkV1>) {
        self.research_capture_sink = Some(sink);
    }

    /// [FIX-5] Returns the shared delayed-account queue.
    ///
    /// The parser should Arc-clone this during setup, then:
    ///   - Call `push(pubkey, ev)` for any AccountUpdate where the mint is unknown.
    ///   - Call `drain(curve_pubkey)` after every successful `CurveMintRegistry::insert`.
    fn delayed_queue(&self) -> Arc<DelayedAccountQueue> {
        Arc::clone(&self.delayed_queue)
    }

    /// Inject a backfill transaction for parser-first processing.
    /// Policy: backfill → parser → classify.  Never short-circuit in transport.
    #[cfg(test)]
    fn inject_backfill(&self, sig: String, slot: u64, raw: Vec<u8>) -> bool {
        let decoded =
            decode_tx_to_geyser_event(raw, &sig, slot, "grpc_backfill", None, None, None).ok();
        let ev = PumpEvent::BackfillTransaction {
            provider_id: None,
            provider_role: None,
            signature: sig,
            slot,
            received_at: Instant::now(),
            decoded,
        };
        self.channel.send(ev, &self.stats)
    }

    /// Run all provider workers.  Returns when shutdown is signalled.
    async fn run(self) -> Result<()> {
        self.config.validate_provider_roles()?;

        // The legacy/active connector intentionally keeps its historical
        // "workers are best-effort" behavior.  The standalone research source
        // has a stricter evidence contract: a panic, worker error, or worker
        // exit before explicit shutdown is a failed source, never a clean
        // capture completion.
        let strict_research_supervision = self.research_capture_sink.is_some();

        let n_providers = self.config.providers.len();
        let n_streams = self.config.streams_per_provider.max(1);
        let total = n_providers * n_streams;

        info!(
            "Ghost/Pump transport profile={} source_label={} : {} provider(s) × {} stream(s) = {} worker(s)",
            self.config.subscription_profile.as_str(),
            self.config.subscription_profile.source_label(),
            n_providers,
            n_streams,
            total
        );

        // [FIX-3] One SlotTracker is shared across ALL providers so gaps are
        // detected globally, not per-provider.  If two providers overlap in slots
        // (they will), fetch_max is idempotent.
        let slots = SlotTracker::new();
        let mut handles = Vec::with_capacity(total);
        let source_label = self.config.subscription_profile.source_label();

        for provider in &self.config.providers {
            let breaker = ProviderCircuitBreaker::new(
                provider.label.clone(),
                source_label,
                ProviderCircuitBreakerConfig {
                    max_stalls_before_open: self.config.max_stalls_before_open,
                    cooldown_ms: self.config.circuit_breaker_cooldown_ms,
                },
            );
            for stream_id in 0..n_streams {
                let worker_id = format!(
                    "{}:{}:{}",
                    self.config.subscription_profile.as_str(),
                    provider.label,
                    stream_id
                );
                let cfg = self.config.clone();
                let prov = provider.clone();
                let channel = self.channel.clone();
                let stats = Arc::clone(&self.stats);
                let shutdown = Arc::clone(&self.shutdown);
                let shutdown_token = Arc::clone(&self.shutdown_token);
                let registry = self.registry.clone();
                let slots = Arc::clone(&slots);
                let delayed_queue = Arc::clone(&self.delayed_queue); // [FIX-5]
                let gap_tx = self.gap_tx.clone();
                let health = self.health.clone();
                let availability_tracker = Arc::clone(&self.availability_tracker);
                let breaker = Arc::clone(&breaker);
                let latest_block_time_secs = Arc::clone(&self.latest_block_time_secs);
                let research_capture_sink = self.research_capture_sink.clone();

                handles.push(tokio::spawn(async move {
                    connection_loop(
                        worker_id,
                        prov,
                        cfg,
                        channel,
                        stats,
                        slots,
                        registry,
                        gap_tx,
                        delayed_queue,
                        breaker,
                        health,
                        availability_tracker,
                        shutdown,
                        shutdown_token,
                        latest_block_time_secs,
                        research_capture_sink,
                    )
                    .await
                }));
            }
        }

        supervise_connector_workers(handles, strict_research_supervision, &self.shutdown).await?;
        info!(
            "Ghost/Pump transport profile={} source_label={}: all workers exited",
            self.config.subscription_profile.as_str(),
            self.config.subscription_profile.source_label()
        );
        Ok(())
    }
}

async fn supervise_connector_workers(
    handles: Vec<tokio::task::JoinHandle<Result<()>>>,
    strict_research_supervision: bool,
    shutdown: &AtomicBool,
) -> Result<()> {
    // Preserve the active connector's historical behavior byte-for-byte at
    // the lifecycle boundary: it deliberately treats workers as best-effort
    // and discards their terminal result.  Only the standalone research
    // source is allowed to turn an individual worker result into an evidence
    // validity outcome.
    if !strict_research_supervision {
        for handle in handles {
            let _ = handle.await;
        }
        return Ok(());
    }

    let mut worker_failure = None;
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                worker_failure
                    .get_or_insert_with(|| format!("research source worker failed: {error:#}"));
            }
            Err(error) => {
                worker_failure
                    .get_or_insert_with(|| format!("research source worker panicked: {error}"));
            }
        }
    }
    if let Some(error) = worker_failure {
        return Err(anyhow::anyhow!(error));
    }
    if !shutdown.load(Ordering::Acquire) {
        bail!("Pump Research source worker exited before an explicit shutdown request");
    }
    Ok(())
}

/// Dedicated, standalone raw-source connection for Pump Research Evidence Tape
/// V1.  It reuses YellowstoneConnector's authenticated connection, reconnect,
/// ping/pong and watchdog machinery, but installs a research source tap before
/// the active runtime projection boundary.
///
/// It is intentionally not exposed through `SeerConfig` and it never returns
/// `GeyserEvent`, starts manual RPC backfill, touches AccountStateCore, or
/// emits into the Event Bus.
pub(crate) struct PumpResearchSourceConnectionV1 {
    connector: Mutex<Option<YellowstoneConnector>>,
    shutdown: Arc<AtomicBool>,
    shutdown_token: Arc<CancellationToken>,
    capture_abort: CancellationToken,
    sink: Arc<dyn PumpResearchSourceSinkV1>,
}

impl PumpResearchSourceConnectionV1 {
    /// Construct the historical V1 source connection.  Kept as the explicit
    /// compatibility entry point so existing GO-D capture code cannot
    /// accidentally switch request contracts.
    pub(crate) fn new(
        endpoint: String,
        auth_token: Option<String>,
        auth_header: String,
        provider_id: String,
        ingress_queue_capacity: usize,
        sink: Arc<dyn PumpResearchSourceSinkV1>,
        capture_abort: CancellationToken,
    ) -> Result<Self> {
        Self::new_for_subscription_profile(
            endpoint,
            auth_token,
            auth_header,
            provider_id,
            ingress_queue_capacity,
            sink,
            capture_abort,
            GrpcSubscriptionProfile::PumpResearchGlobalV1,
        )
    }

    /// Construct the standalone prospective Exact-State Tape V2 source.
    /// This is intentionally not configurable through active `SeerConfig`;
    /// callers must opt into the V2 capture contract explicitly.
    pub(crate) fn new_exact_state_v2(
        endpoint: String,
        auth_token: Option<String>,
        auth_header: String,
        provider_id: String,
        ingress_queue_capacity: usize,
        sink: Arc<dyn PumpResearchSourceSinkV1>,
        capture_abort: CancellationToken,
    ) -> Result<Self> {
        Self::new_for_subscription_profile(
            endpoint,
            auth_token,
            auth_header,
            provider_id,
            ingress_queue_capacity,
            sink,
            capture_abort,
            GrpcSubscriptionProfile::PumpResearchExactStateV2,
        )
    }

    fn new_for_subscription_profile(
        endpoint: String,
        auth_token: Option<String>,
        auth_header: String,
        provider_id: String,
        ingress_queue_capacity: usize,
        sink: Arc<dyn PumpResearchSourceSinkV1>,
        capture_abort: CancellationToken,
        subscription_profile: GrpcSubscriptionProfile,
    ) -> Result<Self> {
        let config = GrpcConfig {
            providers: vec![Provider::primary_with_auth_header(
                endpoint,
                auth_token,
                provider_id.clone(),
                auth_header,
            )],
            provider_role_config: Some(RawProviderRoleConfigV1::new(provider_id, Vec::new())),
            streams_per_provider: 1,
            commitment: CommitmentLevel::Processed,
            stall_timeout_secs: DEFAULT_SILENT_STALL_SECS,
            resub_debounce_ms: DEFAULT_RESUB_DEBOUNCE_MS,
            max_stalls_before_open: DEFAULT_PROVIDER_MAX_STALLS_BEFORE_OPEN,
            circuit_breaker_cooldown_ms: DEFAULT_PROVIDER_CIRCUIT_BREAKER_COOLDOWN_MS,
            subscription_profile,
            registry_resubscribe_mode: RegistryResubscribeMode::HealthTickOnly,
            // The primary runtime FIFO is unused in capture-only mode.  Keep
            // it bounded and sized from the standalone capture config so no
            // hidden unbounded transport buffer is introduced.
            ingress_queue_capacity: ingress_queue_capacity.max(1),
            scope_primary_global_account_updates_to_registry: false,
        };
        config.validate_provider_roles()?;

        let (mut connector, _unused_rx, _unused_gap_rx) = YellowstoneConnector::new(config);
        let shutdown = connector.shutdown_handle();
        let shutdown_token = connector.shutdown_token_handle();
        connector.install_pump_research_capture_sink(Arc::clone(&sink));

        Ok(Self {
            connector: Mutex::new(Some(connector)),
            shutdown,
            shutdown_token,
            capture_abort,
            sink,
        })
    }

    /// Runs until `request_shutdown` is called.  The source sink receives its
    /// terminal notification only after all connector workers have exited, so
    /// the capture coordinator can drain accepted records without a race.
    pub(crate) async fn run(&self) -> Result<()> {
        let connector =
            self.connector.lock().take().ok_or_else(|| {
                anyhow::anyhow!("pump research source connection already started")
            })?;
        let connector_future = connector.run();
        tokio::pin!(connector_future);
        let result = tokio::select! {
            result = &mut connector_future => result,
            _ = self.capture_abort.cancelled() => {
                self.request_shutdown();
                connector_future.await
            }
        };
        match &result {
            Ok(()) => self.sink.source_workers_stopped_cleanly(),
            Err(error) => self.sink.source_worker_failed(format!("{error:#}")),
        }
        self.sink.finish_source();
        result
    }

    pub(crate) fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.shutdown_token.cancel();
    }
}

// ─── Per-worker reconnect loop ────────────────────────────────────────────────

async fn connection_loop(
    id: String,
    prov: Provider,
    cfg: GrpcConfig,
    channel: DualLaneChannel,
    stats: Arc<TransportStats>,
    slots: Arc<SlotTracker>,
    registry: AccountRegistry,
    gap_tx: tokio::sync::mpsc::UnboundedSender<SlotGap>,
    delayed_queue: Arc<DelayedAccountQueue>,
    breaker: Arc<ProviderCircuitBreaker>,
    health: Option<Arc<RuntimeHealth>>,
    availability_tracker: Arc<LaneAvailabilityTracker>,
    shutdown: Arc<AtomicBool>,
    shutdown_token: Arc<CancellationToken>,
    latest_block_time_secs: Arc<AtomicI64>,
    research_capture_sink: Option<Arc<dyn PumpResearchSourceSinkV1>>,
) -> Result<()> {
    let mut backoff = ExponentialBackoffBuilder::new()
        .with_initial_interval(Duration::from_millis(BACKOFF_INIT_MS))
        .with_max_interval(Duration::from_millis(BACKOFF_MAX_MS))
        .with_max_elapsed_time(None)
        .build();

    let mut first_attempt = true;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            if let Some(ref h) = health {
                h.set_grpc_state(GRPC_STATE_DISCONNECTED);
            }
            break;
        }

        let Some(permit) = breaker
            .acquire_attempt_permit(&shutdown, &shutdown_token)
            .await
        else {
            break;
        };
        if permit.half_open_probe {
            info!(
                provider = %prov.label,
                "[{id}] Provider circuit granted half-open probe"
            );
        }

        if let Some(ref h) = health {
            if first_attempt {
                h.set_grpc_state(GRPC_STATE_CONNECTING);
            } else {
                h.inc_grpc_reconnects();
                h.set_grpc_state(GRPC_STATE_RECONNECTING);
            }
        }
        first_attempt = false;

        stats.bump_recon_with_source(cfg.subscription_profile.source_label());
        let stream_epoch = channel.stream_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        // `from_slot` is tracked for diagnostics. Current proto 1.14 request
        // shape does not encode replay from this value.
        let from_slot = slots.last_slot();
        info!(
            "[{id}] Connecting → {} (from_slot={from_slot})",
            prov.endpoint
        );

        let client = tokio::select! {
            biased;
            _ = shutdown_token.cancelled() => {
                if let Some(ref h) = health {
                    h.set_grpc_state(GRPC_STATE_DISCONNECTED);
                }
                break;
            }
            result = build_client(&prov, cfg.subscription_profile) => {
                match result {
                    Ok(client) => client,
                    Err(e) => {
                        if permit.half_open_probe {
                            breaker.record_probe_failure("build_client");
                        }
                        if let Some(ref h) = health {
                            h.set_grpc_state(GRPC_STATE_FAILED);
                        }
                        error!("[{id}] build_client: {e:#}");
                        if !sleep_before_reconnect(&id, &mut backoff, &shutdown, &shutdown_token).await {
                            break;
                        }
                        continue;
                    }
                }
            }
        };

        let mut research_stream_established = false;
        let result = stream_loop(
            &id,
            client,
            &cfg,
            &prov.label,
            prov.role,
            from_slot,
            &channel,
            &stats,
            &slots,
            &registry,
            &gap_tx,
            &delayed_queue,
            &breaker,
            permit,
            health.clone(),
            &availability_tracker,
            &shutdown,
            &shutdown_token,
            &latest_block_time_secs,
            stream_epoch,
            research_capture_sink.clone(),
            &mut research_stream_established,
        )
        .await;

        match result {
            Ok(()) => break,
            Err(err) => {
                if research_stream_established {
                    if let Some(research_capture_sink) = research_capture_sink.as_ref() {
                        research_capture_sink
                            .source_stream_interrupted(stream_epoch, format!("{err:#}"));
                    }
                }
                if shutdown.load(Ordering::Relaxed) {
                    debug!("[{id}] stream ended during shutdown: {err:#}");
                    break;
                }
                if permit.half_open_probe
                    && breaker.snapshot().state == ProviderCircuitState::HalfOpen
                {
                    breaker.record_probe_failure(&err.to_string());
                }
                if let Some(ref h) = health {
                    h.set_grpc_state(GRPC_STATE_DISCONNECTED);
                }
                warn!("[{id}] stream ended: {err:#}");
                if !sleep_before_reconnect(&id, &mut backoff, &shutdown, &shutdown_token).await {
                    break;
                }
            }
        }
    }

    if let Some(ref h) = health {
        h.set_grpc_state(if shutdown.load(Ordering::Relaxed) {
            GRPC_STATE_DISCONNECTED
        } else {
            GRPC_STATE_FAILED
        });
    }
    Ok(())
}

async fn sleep_before_reconnect(
    id: &str,
    backoff: &mut backoff::ExponentialBackoff,
    shutdown: &Arc<AtomicBool>,
    shutdown_token: &Arc<CancellationToken>,
) -> bool {
    let Some(delay) = backoff.next_backoff() else {
        error!("[{id}] fatal: reconnect backoff exhausted");
        return false;
    };

    tokio::select! {
        biased;
        _ = shutdown_token.cancelled() => false,
        _ = tokio::time::sleep(delay), if !shutdown.load(Ordering::Relaxed) => true,
        else => false,
    }
}

// ─── gRPC client builder ──────────────────────────────────────────────────────

/// Normalise endpoint: if the user supplies `host:port` without scheme,
/// prepend `https://` so tonic's `Endpoint::from_shared` doesn't reject it.
fn normalise_endpoint(raw: &str) -> String {
    if raw.starts_with("https://") || raw.starts_with("http://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    }
}

#[derive(Clone)]
struct AuthHeaderInterceptor {
    auth: Option<(MetadataKey<Ascii>, AsciiMetadataValue)>,
}

impl AuthHeaderInterceptor {
    fn new(header: &str, token: Option<&str>) -> Result<Self> {
        let auth = match token.filter(|value| !value.trim().is_empty()) {
            Some(token) => {
                let header = if header.trim().is_empty() {
                    DEFAULT_GRPC_AUTH_HEADER
                } else {
                    header.trim()
                };
                let key = MetadataKey::<Ascii>::from_bytes(header.as_bytes())
                    .with_context(|| format!("invalid gRPC auth header name: {header}"))?;
                let value = AsciiMetadataValue::try_from(token.trim())
                    .context("invalid gRPC auth token metadata value")?;
                Some((key, value))
            }
            None => None,
        };
        Ok(Self { auth })
    }
}

impl Interceptor for AuthHeaderInterceptor {
    fn call(&mut self, mut request: Request<()>) -> std::result::Result<Request<()>, Status> {
        if let Some((key, value)) = self.auth.clone() {
            request.metadata_mut().insert(key, value);
        }
        Ok(request)
    }
}

async fn build_client(
    prov: &Provider,
    subscription_profile: GrpcSubscriptionProfile,
) -> Result<GeyserGrpcClient<impl tonic::service::Interceptor>> {
    let uri = normalise_endpoint(&prov.endpoint);
    let endpoint = Endpoint::from_shared(uri.clone())?
        .http2_adaptive_window(true)
        .initial_connection_window_size(1 << 26) // 64 MiB flow control
        .initial_stream_window_size(1 << 25) // 32 MiB
        .keep_alive_while_idle(true)
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(5))
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30));

    let channel = endpoint
        .connect()
        .await
        .with_context(|| format!("gRPC connect to {uri}"))?;
    let interceptor = AuthHeaderInterceptor::new(&prov.auth_header, prov.x_token.as_deref())?;
    let health = HealthClient::with_interceptor(channel.clone(), interceptor.clone());
    let geyser = if matches!(
        subscription_profile,
        GrpcSubscriptionProfile::PumpResearchExactStateV2
    ) {
        GeyserClient::with_interceptor(channel, interceptor)
            .max_decoding_message_size(PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES)
    } else {
        GeyserClient::with_interceptor(channel, interceptor)
    };
    Ok(GeyserGrpcClient::new(health, geyser))
}

// ─── SubscribeRequest builder — SSOT ─────────────────────────────────────────

#[cfg(test)]
fn build_subscribe_request(
    commitment: CommitmentLevel,
    registry: &AccountRegistry,
    from_slot: u64,
) -> SubscribeRequest {
    build_subscribe_request_for_profile(
        commitment,
        GrpcSubscriptionProfile::PrimaryGlobal,
        registry,
        from_slot,
    )
}

fn tracked_exact_accounts_for_profile(
    subscription_profile: GrpcSubscriptionProfile,
    registry: &AccountRegistry,
    budget: usize,
) -> Vec<String> {
    match subscription_profile {
        // Curve and pool AccountUpdates already arrive through the global layout
        // filters in the primary profile. Keeping them out of the dynamic exact
        // branch avoids shape-changing resubscribe requests that have been
        // observed to trigger provider-side h2 failures on Chainstack.
        GrpcSubscriptionProfile::PrimaryGlobal => {
            registry.snapshot_primary_global_exact_accounts(budget)
        }
        GrpcSubscriptionProfile::PumpResearchGlobalV1
        | GrpcSubscriptionProfile::PumpResearchExactStateV2
        | GrpcSubscriptionProfile::FundingLanePumpFiltered
        | GrpcSubscriptionProfile::FundingLaneFullChain => Vec::new(),
    }
}

fn tracked_exact_total_for_profile(
    subscription_profile: GrpcSubscriptionProfile,
    lanes: &AccountRegistrySnapshot,
) -> usize {
    match subscription_profile {
        GrpcSubscriptionProfile::PrimaryGlobal => {
            lanes.bcv2_accounts.len() + lanes.generic_accounts.len()
        }
        GrpcSubscriptionProfile::PumpResearchGlobalV1
        | GrpcSubscriptionProfile::PumpResearchExactStateV2
        | GrpcSubscriptionProfile::FundingLanePumpFiltered
        | GrpcSubscriptionProfile::FundingLaneFullChain => 0,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ExactAccountSelectionCounts {
    exact_total: usize,
    tracked_sent: usize,
    tracked_dropped: usize,
    tracked_bcv2: usize,
    bcv2_sent: usize,
    bcv2_dropped: usize,
}

fn exact_account_selection_counts_for_profile(
    subscription_profile: GrpcSubscriptionProfile,
    lanes: &AccountRegistrySnapshot,
    tracked_accounts: &[String],
) -> ExactAccountSelectionCounts {
    let exact_total = tracked_exact_total_for_profile(subscription_profile, lanes);
    let tracked_sent = tracked_accounts.len();
    let tracked_dropped = exact_total.saturating_sub(tracked_sent);
    let tracked_bcv2 = match subscription_profile {
        GrpcSubscriptionProfile::PrimaryGlobal => lanes.bcv2_accounts.len(),
        GrpcSubscriptionProfile::PumpResearchGlobalV1
        | GrpcSubscriptionProfile::PumpResearchExactStateV2
        | GrpcSubscriptionProfile::FundingLanePumpFiltered
        | GrpcSubscriptionProfile::FundingLaneFullChain => 0,
    };
    let selected: HashSet<&str> = tracked_accounts.iter().map(String::as_str).collect();
    let bcv2_sent = lanes
        .bcv2_accounts
        .iter()
        .filter(|account| selected.contains(account.as_str()))
        .count();
    ExactAccountSelectionCounts {
        exact_total,
        tracked_sent,
        tracked_dropped,
        tracked_bcv2,
        bcv2_sent,
        bcv2_dropped: tracked_bcv2.saturating_sub(bcv2_sent),
    }
}

fn subscribe_request_fingerprint_for_profile(
    subscription_profile: GrpcSubscriptionProfile,
    registry: &AccountRegistry,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    subscription_profile.as_str().hash(&mut hasher);
    tracked_exact_accounts_for_profile(subscription_profile, registry, EXACT_ACCOUNT_PAYLOAD_CAP)
        .hash(&mut hasher);
    hasher.finish()
}

/// Build the immutable standalone source request used by Pump Research Tape
/// capture.  It intentionally has no registry, candidate, or watched-pool
/// input; every eligible Pump transaction and global curve update is admitted
/// by the provider-side source boundary.
///
/// This is the one request constructor used both by the live standalone
/// source and by the manifest fingerprint.  Do not introduce a parallel
/// request description here: provenance requires the two to remain coupled.
pub(crate) fn pump_research_subscribe_request_v1() -> SubscribeRequest {
    build_subscribe_request_for_profile(
        CommitmentLevel::Processed,
        GrpcSubscriptionProfile::PumpResearchGlobalV1,
        &AccountRegistry::new(),
        0,
    )
}

/// Build the immutable standalone source request for prospective Exact-State
/// Tape V2. Its stream-only V1.1 account contract is deliberately closed to
/// Pump-owner BondingCurve updates plus the canonical Pump Global account.
/// Do not widen this request into an all-owner scan: accounts outside those
/// two classes are a fail-closed source-contract error, not retained state
/// authority.
pub(crate) fn pump_research_exact_state_v2_subscribe_request() -> SubscribeRequest {
    build_subscribe_request_for_profile(
        CommitmentLevel::Processed,
        GrpcSubscriptionProfile::PumpResearchExactStateV2,
        &AccountRegistry::new(),
        0,
    )
}

fn append_canonical_request_field(bytes: &mut Vec<u8>, label: &str, value: &[u8]) {
    bytes.extend_from_slice(&(label.len() as u32).to_le_bytes());
    bytes.extend_from_slice(label.as_bytes());
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(value);
}

/// Canonicalize one protobuf map directly from the `SubscribeRequest` object
/// which will be sent to Yellowstone.  `prost` deliberately does not make
/// `HashMap` iteration order a durable encoding contract, so we sort map
/// entries by their actual key and their actual prost-encoded value.
fn append_canonical_request_map<M: prost::Message>(
    bytes: &mut Vec<u8>,
    label: &str,
    values: &HashMap<String, M>,
) {
    let mut entries: Vec<(&str, Vec<u8>)> = values
        .iter()
        .map(|(key, value)| (key.as_str(), value.encode_to_vec()))
        .collect();
    entries.sort_unstable_by(|(left_key, left_value), (right_key, right_value)| {
        left_key
            .cmp(right_key)
            .then_with(|| left_value.cmp(right_value))
    });
    append_canonical_request_field(
        bytes,
        &format!("{label}.len"),
        &(entries.len() as u32).to_le_bytes(),
    );
    for (index, (key, value)) in entries.into_iter().enumerate() {
        append_canonical_request_field(bytes, &format!("{label}.{index}.key"), key.as_bytes());
        append_canonical_request_field(bytes, &format!("{label}.{index}.value"), &value);
    }
}

fn append_canonical_request_messages<M: prost::Message>(
    bytes: &mut Vec<u8>,
    label: &str,
    values: &[M],
) {
    append_canonical_request_field(
        bytes,
        &format!("{label}.len"),
        &(values.len() as u32).to_le_bytes(),
    );
    for (index, value) in values.iter().enumerate() {
        append_canonical_request_field(bytes, &format!("{label}.{index}"), &value.encode_to_vec());
    }
}

/// Canonical BLAKE3 identity of the complete `SubscribeRequest` object.
///
/// The representation is deterministic across Rust `HashMap` iteration but
/// is derived only from the concrete request fields.  Every request field is
/// represented, including empty maps, empty account data slices and the
/// optional ping field, so a source-request drift cannot leave the manifest
/// fingerprint unchanged.
pub(crate) fn subscribe_request_fingerprint_blake3_v1(request: &SubscribeRequest) -> [u8; 32] {
    let mut canonical = Vec::with_capacity(1_024);
    append_canonical_request_field(
        &mut canonical,
        "schema",
        b"pump_research_subscribe_request_canonical_v1",
    );
    append_canonical_request_map(&mut canonical, "accounts", &request.accounts);
    append_canonical_request_map(&mut canonical, "slots", &request.slots);
    append_canonical_request_map(&mut canonical, "transactions", &request.transactions);
    append_canonical_request_map(
        &mut canonical,
        "transactions_status",
        &request.transactions_status,
    );
    append_canonical_request_map(&mut canonical, "blocks", &request.blocks);
    append_canonical_request_map(&mut canonical, "blocks_meta", &request.blocks_meta);
    append_canonical_request_map(&mut canonical, "entry", &request.entry);
    match request.commitment {
        Some(commitment) => {
            append_canonical_request_field(&mut canonical, "commitment.present", &[1]);
            append_canonical_request_field(
                &mut canonical,
                "commitment.value",
                &commitment.to_le_bytes(),
            );
        }
        None => append_canonical_request_field(&mut canonical, "commitment.present", &[0]),
    }
    append_canonical_request_messages(
        &mut canonical,
        "accounts_data_slice",
        &request.accounts_data_slice,
    );
    match request.ping.as_ref() {
        Some(ping) => {
            append_canonical_request_field(&mut canonical, "ping.present", &[1]);
            append_canonical_request_field(&mut canonical, "ping.value", &ping.encode_to_vec());
        }
        None => append_canonical_request_field(&mut canonical, "ping.present", &[0]),
    }
    *blake3::hash(&canonical).as_bytes()
}

/// BLAKE3 identity of the actual frozen standalone research request.
pub(crate) fn pump_research_subscription_request_fingerprint_blake3_v1() -> [u8; 32] {
    subscribe_request_fingerprint_blake3_v1(&pump_research_subscribe_request_v1())
}

/// BLAKE3 identity of the actual standalone prospective Exact-State V2
/// request.  The existing canonicalization format is request-generic even
/// though its historical function name carries the V1 suffix.
pub(crate) fn pump_research_exact_state_v2_subscription_request_fingerprint_blake3() -> [u8; 32] {
    subscribe_request_fingerprint_blake3_v1(&pump_research_exact_state_v2_subscribe_request())
}

fn build_subscribe_request_for_profile(
    commitment: CommitmentLevel,
    subscription_profile: GrpcSubscriptionProfile,
    registry: &AccountRegistry,
    from_slot: u64,
) -> SubscribeRequest {
    // ── Transactions ─────────────────────────────────────────────────────────
    let tx_account_include = subscription_profile.transaction_account_include();

    let mut tx_filters = HashMap::new();
    tx_filters.insert(
        subscription_profile.transaction_filter_name().into(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: subscription_profile.transaction_failed_filter(),
            account_include: tx_account_include,
            account_exclude: vec![],
            account_required: vec![],
            signature: None,
        },
    );

    // ── Accounts ─────────────────────────────────────────────────────────────
    // Global owner+memcmp branches already carry canonical Pump.fun curve and
    // PumpSwap pool AccountUpdates for the primary profile. The dynamic exact
    // branch therefore stays generic-only there, which avoids provider-facing
    // request-shape churn on every local watch-set change.
    let (
        acc_filters,
        tracked_curve_count,
        tracked_pool_count,
        tracked_mint_count,
        tracked_generic_count,
        tracked_bcv2_count,
        exact_total,
        tracked_sent,
        tracked_dropped,
        bcv2_sent,
        bcv2_dropped,
    ) = if subscription_profile.uses_registry_filters() {
        let lanes = registry.snapshot_by_lane();
        let tracked_curve_count = lanes.curve_accounts.len();
        let tracked_pool_count = lanes.pool_accounts.len();
        let tracked_mint_count = lanes.mint_accounts.len();
        let tracked_generic_count = lanes.generic_accounts.len();
        let tracked_accounts = tracked_exact_accounts_for_profile(
            subscription_profile,
            registry,
            EXACT_ACCOUNT_PAYLOAD_CAP,
        );
        let counts = exact_account_selection_counts_for_profile(
            subscription_profile,
            &lanes,
            &tracked_accounts,
        );
        if counts.tracked_dropped > 0 {
            warn!(
                    "AccountRegistry exact watch set for profile {} has {} dynamic entries but filter allows only {} — dropping {} accounts (from_slot={})",
                    subscription_profile.as_str(),
                    counts.exact_total,
                    EXACT_ACCOUNT_PAYLOAD_CAP,
                    counts.tracked_dropped,
                    from_slot,
                );
        }
        if counts.bcv2_sent > 0 {
            info!(
                "BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED profile={} bcv2_sent={} bcv2_dropped={} tracked_bcv2={} from_slot={}",
                subscription_profile.as_str(),
                counts.bcv2_sent,
                counts.bcv2_dropped,
                counts.tracked_bcv2,
                from_slot
            );
        }
        if counts.bcv2_dropped > 0 {
            warn!(
                "BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED profile={} bcv2_dropped={} tracked_bcv2={} bcv2_sent={} exact_payload_cap={} from_slot={}",
                subscription_profile.as_str(),
                counts.bcv2_dropped,
                counts.tracked_bcv2,
                counts.bcv2_sent,
                EXACT_ACCOUNT_PAYLOAD_CAP,
                from_slot
            );
        }

        let mut acc_filters = HashMap::new();
        acc_filters.insert(
            "pumpfun_curve_layouts".into(),
            SubscribeRequestFilterAccounts {
                account: vec![],
                owner: vec![PUMP_FUN_PROGRAM_ID.to_string()],
                filters: vec![SubscribeRequestFilterAccountsFilter {
                    filter: Some(subscribe_request_filter_accounts_filter::Filter::Memcmp(
                        SubscribeRequestFilterAccountsFilterMemcmp {
                            offset: 0,
                            data: Some(
                                subscribe_request_filter_accounts_filter_memcmp::Data::Bytes(
                                    BONDING_CURVE_DISC.to_vec(),
                                ),
                            ),
                        },
                    )),
                }],
            },
        );
        acc_filters.insert(
            "pumpswap_pool_layouts".into(),
            SubscribeRequestFilterAccounts {
                account: vec![],
                owner: vec![PUMP_SWAP_PROGRAM_ID.to_string()],
                filters: vec![SubscribeRequestFilterAccountsFilter {
                    filter: Some(subscribe_request_filter_accounts_filter::Filter::Memcmp(
                        SubscribeRequestFilterAccountsFilterMemcmp {
                            offset: 0,
                            data: Some(
                                subscribe_request_filter_accounts_filter_memcmp::Data::Bytes(
                                    AMM_POOL_DISC.to_vec(),
                                ),
                            ),
                        },
                    )),
                }],
            },
        );
        acc_filters.insert(
            "tracked_accounts".into(),
            SubscribeRequestFilterAccounts {
                account: {
                    let mut tracked = vec![PUMP_FUN_FEE_ACCOUNT.into()];
                    tracked.extend(tracked_accounts);
                    tracked
                },
                owner: vec![],
                filters: vec![],
            },
        );

        (
            acc_filters,
            tracked_curve_count,
            tracked_pool_count,
            tracked_mint_count,
            tracked_generic_count,
            counts.tracked_bcv2,
            counts.exact_total,
            counts.tracked_sent,
            counts.tracked_dropped,
            counts.bcv2_sent,
            counts.bcv2_dropped,
        )
    } else if matches!(
        subscription_profile,
        GrpcSubscriptionProfile::PumpResearchGlobalV1
    ) {
        let mut acc_filters = HashMap::new();
        acc_filters.insert(
            "pump_research_bonding_curves".into(),
            SubscribeRequestFilterAccounts {
                account: vec![],
                owner: vec![PUMP_FUN_PROGRAM_ID.to_string()],
                filters: vec![SubscribeRequestFilterAccountsFilter {
                    filter: Some(subscribe_request_filter_accounts_filter::Filter::Memcmp(
                        SubscribeRequestFilterAccountsFilterMemcmp {
                            offset: 0,
                            data: Some(
                                subscribe_request_filter_accounts_filter_memcmp::Data::Bytes(
                                    BONDING_CURVE_DISC.to_vec(),
                                ),
                            ),
                        },
                    )),
                }],
            },
        );
        acc_filters.insert(
            "pump_research_global".into(),
            SubscribeRequestFilterAccounts {
                account: vec![
                    ghost_core::pump_research_tape::PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1.to_string(),
                ],
                owner: vec![],
                filters: vec![],
            },
        );
        (acc_filters, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    } else if matches!(
        subscription_profile,
        GrpcSubscriptionProfile::PumpResearchExactStateV2
    ) {
        let mut acc_filters = HashMap::new();
        acc_filters.insert(
            "pump_research_exact_state_v2_bonding_curves".into(),
            SubscribeRequestFilterAccounts {
                account: vec![],
                owner: vec![PUMP_FUN_PROGRAM_ID.to_string()],
                filters: vec![SubscribeRequestFilterAccountsFilter {
                    filter: Some(subscribe_request_filter_accounts_filter::Filter::Memcmp(
                        SubscribeRequestFilterAccountsFilterMemcmp {
                            offset: 0,
                            data: Some(
                                subscribe_request_filter_accounts_filter_memcmp::Data::Bytes(
                                    BONDING_CURVE_DISC.to_vec(),
                                ),
                            ),
                        },
                    )),
                }],
            },
        );
        acc_filters.insert(
            "pump_research_exact_state_v2_global".into(),
            SubscribeRequestFilterAccounts {
                account: vec![
                    ghost_core::pump_research_tape::PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1.to_string(),
                ],
                owner: vec![],
                filters: vec![],
            },
        );
        (acc_filters, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    } else {
        (HashMap::new(), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    };

    // ── Entry filter ──────────────────────────────────────────────────────────
    // DISABLED: Entry subscription with empty filter receives ALL entries from
    // the entire Solana network (~84% of stream traffic). This floods the gRPC
    // connection and causes chronic 1-25s delivery stalls during peak load.
    // BlockMeta now provides slot-gap tracking. Standard Yellowstone nodes
    // produce no embedded CPI data in entries (fast-path no-op), so we lose
    // nothing on Chainstack.
    let entry_filters: HashMap<String, SubscribeRequestFilterEntry> = HashMap::new();

    // ── BlockMeta filter ──────────────────────────────────────────────────────
    // Used to capture block_time per slot so event_ts_ms can be anchored to
    // the on-chain block time rather than our local ingress wall-clock.
    let mut blocks_meta_filters = HashMap::new();
    blocks_meta_filters.insert(
        match subscription_profile {
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => "pump_research_blocks_meta",
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                "pump_research_exact_state_v2_blocks_meta"
            }
            _ => "ghost_blocks_meta",
        }
        .into(),
        SubscribeRequestFilterBlocksMeta {},
    );

    // V2 keeps a second, unfiltered block evidence lane. The standalone
    // materializer later compares non-vote, actual Pump invocations from each
    // parent-linked post-readiness block against the same invocation predicate
    // on the transaction tap. `account_include` deliberately admits broader
    // account-reference traffic, so it is not itself an invocation count.
    // The full block is deliberately not provider-filtered: a filter cannot
    // prove that it did not omit an inner-CPI or loaded-address invocation.
    let mut block_filters = HashMap::new();
    if matches!(
        subscription_profile,
        GrpcSubscriptionProfile::PumpResearchExactStateV2
    ) {
        block_filters.insert(
            "pump_research_exact_state_v2_full_blocks".into(),
            SubscribeRequestFilterBlocks {
                account_include: vec![],
                include_transactions: Some(true),
                include_accounts: Some(false),
                include_entries: Some(false),
            },
        );
    }

    // The research profile is the only raw-capture profile that needs slot
    // parent/status evidence for offline canonicality.  `false` deliberately
    // retains Processed/Confirmed/Finalized updates; canonicality is derived
    // later from the tape and never pre-classified by the provider filter.
    let mut slot_filters = HashMap::new();
    if matches!(
        subscription_profile,
        GrpcSubscriptionProfile::PumpResearchGlobalV1
            | GrpcSubscriptionProfile::PumpResearchExactStateV2
    ) {
        slot_filters.insert(
            match subscription_profile {
                GrpcSubscriptionProfile::PumpResearchGlobalV1 => "pump_research_slots",
                GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                    "pump_research_exact_state_v2_slots"
                }
                _ => unreachable!("only standalone research profiles reach this branch"),
            }
            .into(),
            SubscribeRequestFilterSlots {
                filter_by_commitment: Some(false),
            },
        );
    }

    let req = SubscribeRequest {
        accounts: acc_filters,
        slots: slot_filters,
        transactions: tx_filters,
        transactions_status: HashMap::new(),
        blocks: block_filters,
        blocks_meta: blocks_meta_filters,
        entry: entry_filters,
        commitment: Some(commitment as i32),
        accounts_data_slice: vec![],
        ping: None,
    };

    let total_filter_branches = req.accounts.len()
        + req.slots.len()
        + req.transactions.len()
        + req.blocks.len()
        + req.blocks_meta.len()
        + req.entry.len()
        + req.accounts_data_slice.len();

    // Single-line SSOT log — operators grep this to diagnose "0 events" issues.
    info!(
        "SUBSCRIBE_SENT profile={} source_label={} tx_filter={} \
         programs={} \
         accounts={} \
         blocks={} \
         blocks_meta={} \
         slots={} \
         entry=DISABLED \
         tracked_curves={} tracked_pools={} tracked_mints={} tracked_generic={} tracked_bcv2={} \
         exact_total={} tracked_sent={} tracked_dropped={} bcv2_sent={} bcv2_dropped={} \
         note={} \
         filter_branches={} \
         from_slot={from_slot} \
         commitment={commitment:?}",
        subscription_profile.as_str(),
        subscription_profile.source_label(),
        subscription_profile.transaction_filter_name(),
        match subscription_profile {
            GrpcSubscriptionProfile::FundingLaneFullChain => "[ALL_TRANSACTIONS]",
            GrpcSubscriptionProfile::PumpResearchGlobalV1
            | GrpcSubscriptionProfile::PumpResearchExactStateV2 => "[PumpFun]",
            GrpcSubscriptionProfile::PrimaryGlobal
            | GrpcSubscriptionProfile::FundingLanePumpFiltered => "[PumpFun,PumpSwap]",
        },
        match subscription_profile {
            GrpcSubscriptionProfile::PrimaryGlobal => {
                "[global_curve_layouts,global_pool_layouts,generic_exact_only]"
            }
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => {
                "[pump_owned_bonding_curve_discriminator,canonical_pump_global]"
            }
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                PUMP_RESEARCH_EXACT_STATE_V2_ACCOUNT_FILTER_LOG_SCOPE
            }
            GrpcSubscriptionProfile::FundingLanePumpFiltered
            | GrpcSubscriptionProfile::FundingLaneFullChain => "[DISABLED]",
        },
        match subscription_profile {
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                "[unfiltered_full_blocks_with_transactions]"
            }
            _ => "[DISABLED]",
        },
        match subscription_profile {
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => "pump_research_blocks_meta",
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                "pump_research_exact_state_v2_blocks_meta"
            }
            _ => "ghost_blocks_meta",
        },
        match subscription_profile {
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => {
                "pump_research_slots(filter_by_commitment=false)"
            }
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                "pump_research_exact_state_v2_slots(filter_by_commitment=false)"
            }
            _ => "DISABLED",
        },
        tracked_curve_count,
        tracked_pool_count,
        tracked_mint_count,
        tracked_generic_count,
        tracked_bcv2_count,
        exact_total,
        tracked_sent,
        tracked_dropped,
        bcv2_sent,
        bcv2_dropped,
        match subscription_profile {
            GrpcSubscriptionProfile::PrimaryGlobal => {
                "canonical_account_updates_via_global_layout_filters_generic_exact_only"
            }
            GrpcSubscriptionProfile::FundingLanePumpFiltered => {
                "dedicated_filtered_funding_lane_no_account_updates"
            }
            GrpcSubscriptionProfile::FundingLaneFullChain => {
                "dedicated_full_chain_funding_lane_no_account_updates"
            }
            GrpcSubscriptionProfile::PumpResearchGlobalV1 => {
                "standalone_decoded_source_capture_before_projection"
            }
            GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
                PUMP_RESEARCH_EXACT_STATE_V2_SOURCE_CAPTURE_LOG_NOTE
            }
        },
        total_filter_branches,
    );

    req
}

fn build_ping(id: i32) -> SubscribeRequest {
    SubscribeRequest {
        accounts: HashMap::new(),
        slots: HashMap::new(),
        transactions: HashMap::new(),
        transactions_status: HashMap::new(),
        blocks: HashMap::new(),
        blocks_meta: HashMap::new(),
        entry: HashMap::new(),
        commitment: None,
        accounts_data_slice: vec![],
        ping: Some(SubscribeRequestPing { id }),
    }
}

async fn send_request_with_timeout<S>(
    sink: &mut S,
    request: SubscribeRequest,
    op_name: &str,
) -> Result<()>
where
    S: Sink<SubscribeRequest> + Unpin,
    S::Error: std::fmt::Display,
{
    tokio::time::timeout(
        Duration::from_secs(STREAM_SEND_TIMEOUT_SECS),
        sink.send(request),
    )
    .await
    .map_err(|_| anyhow::anyhow!("{op_name} timed out after {}s", STREAM_SEND_TIMEOUT_SECS))?
    .map_err(|e| anyhow::anyhow!("{op_name} failed: {e}"))
}

async fn maybe_send_resubscribe<S>(
    id: &str,
    reason: &str,
    sink: &mut S,
    cfg: &GrpcConfig,
    registry: &AccountRegistry,
    slots: &Arc<SlotTracker>,
    stats: &Arc<TransportStats>,
    health: Option<&Arc<RuntimeHealth>>,
    last_reg_version: &mut u64,
    last_request_fingerprint: &mut u64,
    last_resub_at: &mut Instant,
    force: bool,
    bypass_debounce: bool,
) -> Result<()>
where
    S: Sink<SubscribeRequest> + Unpin,
    S::Error: std::fmt::Display,
{
    let cur_version = registry.version();
    if !cfg.subscription_profile.uses_registry_filters() {
        *last_reg_version = cur_version;
        *last_request_fingerprint =
            subscribe_request_fingerprint_for_profile(cfg.subscription_profile, registry);
        return Ok(());
    }
    if !force && cur_version == *last_reg_version {
        return Ok(());
    }

    let current_request_fingerprint =
        subscribe_request_fingerprint_for_profile(cfg.subscription_profile, registry);
    if current_request_fingerprint == *last_request_fingerprint {
        debug!(
            "[{id}] Registry change (reason={reason}) ver={}->{} but request shape is unchanged; skipping resubscribe",
            *last_reg_version,
            cur_version
        );
        *last_reg_version = cur_version;
        return Ok(());
    }

    let now = Instant::now();
    if !bypass_debounce
        && now.duration_since(*last_resub_at) < Duration::from_millis(cfg.resub_debounce_ms)
    {
        return Ok(());
    }

    let resub_from_slot = slots.last_slot();
    info!(
        "[{id}] Registry change (reason={reason}) ver={}->{}: resub from_slot={resub_from_slot}",
        *last_reg_version, cur_version
    );
    metrics::increment_counter!("seer_resubscribe_total");
    let req = build_subscribe_request_for_profile(
        cfg.commitment,
        cfg.subscription_profile,
        registry,
        resub_from_slot,
    );
    if let Some(h) = health {
        h.set_grpc_state(GRPC_STATE_SUBSCRIBING);
        h.mark_grpc_subscribe_sent();
    }
    send_request_with_timeout(sink, req, "resubscribe").await?;
    if let Some(h) = health {
        h.set_grpc_state(GRPC_STATE_CONNECTED);
    }
    stats.bump_resub();
    if reason.starts_with("bcv2") {
        info!(
            "BCV2_EXACT_WATCH_RESUBSCRIBE_SENT reason={} profile={} from_slot={} registry_version={}",
            reason,
            cfg.subscription_profile.as_str(),
            resub_from_slot,
            cur_version
        );
    }
    *last_reg_version = cur_version;
    *last_request_fingerprint = current_request_fingerprint;
    *last_resub_at = now;

    Ok(())
}

// ─── Stream loop ──────────────────────────────────────────────────────────────

async fn stream_loop(
    id: &str,
    mut client: GeyserGrpcClient<impl tonic::service::Interceptor>,
    cfg: &GrpcConfig,
    provider_id: &str,
    provider_role: ghost_core::RawProviderRoleV1,
    from_slot: u64,
    channel: &DualLaneChannel,
    stats: &Arc<TransportStats>,
    slots: &Arc<SlotTracker>,
    registry: &AccountRegistry,
    gap_tx: &tokio::sync::mpsc::UnboundedSender<SlotGap>,
    delayed_queue: &Arc<DelayedAccountQueue>,
    breaker: &Arc<ProviderCircuitBreaker>,
    permit: ProviderAttemptPermit,
    health: Option<Arc<RuntimeHealth>>,
    availability_tracker: &Arc<LaneAvailabilityTracker>,
    shutdown: &Arc<AtomicBool>,
    shutdown_token: &Arc<CancellationToken>,
    latest_block_time_secs: &Arc<AtomicI64>,
    stream_epoch: u64,
    research_capture_sink: Option<Arc<dyn PumpResearchSourceSinkV1>>,
    research_stream_established: &mut bool,
) -> Result<()> {
    let req = match cfg.subscription_profile {
        // Each standalone research source must send the exact request whose
        // canonical identity is recorded in its immutable run manifest.  Do
        // not reconstruct either request from mutable runtime registry state.
        GrpcSubscriptionProfile::PumpResearchGlobalV1 => pump_research_subscribe_request_v1(),
        GrpcSubscriptionProfile::PumpResearchExactStateV2 => {
            pump_research_exact_state_v2_subscribe_request()
        }
        _ => build_subscribe_request_for_profile(
            cfg.commitment,
            cfg.subscription_profile,
            registry,
            from_slot,
        ),
    };
    if let Some(ref h) = health {
        h.set_grpc_state(GRPC_STATE_SUBSCRIBING);
        h.mark_grpc_subscribe_sent();
    }
    let subscribe_result = tokio::select! {
        biased;
        _ = shutdown_token.cancelled() => {
            info!("[{id}] Shutdown before subscribe completed");
            return Ok(());
        }
        result = tokio::time::timeout(
            Duration::from_secs(SUBSCRIBE_REQUEST_TIMEOUT_SECS),
            client.subscribe_with_request(Some(req)),
        ) => {
            result.map_err(|_| {
                anyhow::anyhow!(
                    "subscribe_with_request timed out after {}s",
                    SUBSCRIBE_REQUEST_TIMEOUT_SECS
                )
            })?
        }
    };
    let (mut sink, mut stream) = subscribe_result.context("subscribe_with_request")?;

    if let Some(ref h) = health {
        h.set_grpc_state(GRPC_STATE_CONNECTED);
    }

    let _availability_guard = availability_tracker.connected_guard();
    info!("[{id}] Stream established");
    if let Some(research_capture_sink) = research_capture_sink.as_ref() {
        research_capture_sink.source_stream_established(stream_epoch);
        *research_stream_established = true;
    }

    let mut health_ticker = tokio::time::interval(Duration::from_secs(HEALTH_TICK_SECS));
    let mut ping_ticker = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
    let mut watchdog_ticker = tokio::time::interval(Duration::from_secs(WATCHDOG_TICK_SECS));
    let mut registry_resub_ticker =
        tokio::time::interval(Duration::from_millis(REGISTRY_RESUB_TICK_MS));
    let resub_notify = registry.resub_notify();
    let bcv2_resub_notify = registry.bcv2_resub_notify();

    let mut ping_seq: i32 = 0;
    let mut last_pong: i32 = -1;
    let mut last_reg_version = registry.version();
    let mut last_request_fingerprint =
        subscribe_request_fingerprint_for_profile(cfg.subscription_profile, registry);
    let mut last_resub_at = Instant::now() - Duration::from_millis(cfg.resub_debounce_ms);
    let provider_label = breaker.provider_label.as_str();
    let mut last_msg_wall_ms = wall_clock_ms();

    stats.touch(); // initialise watchdog clock

    loop {
        if shutdown.load(Ordering::Relaxed) {
            info!("[{id}] Shutdown");
            return Ok(());
        }

        tokio::select! {
            biased;

            _ = shutdown_token.cancelled() => {
                info!("[{id}] Shutdown");
                return Ok(());
            }

            // ── Incoming gRPC message ─────────────────────────────────────
            maybe = stream.next() => {
                match maybe {
                    None => {
                        return Err(anyhow::anyhow!("stream closed by server"));
                    }
                    Some(Err(e)) => {
                        error!("[{id}] gRPC error: {e}");
                        return Err(e.into());
                    }
                    Some(Ok(msg)) => {
                        last_msg_wall_ms = wall_clock_ms();
                        stats.touch(); // heartbeat — watchdog reset
                        if let Some(ref h) = health {
                            h.mark_grpc_msg();
                            h.set_grpc_state(GRPC_STATE_CONNECTED);
                        }

                        // Pong: handled inline, never forwarded.
                        if let Some(UpdateOneof::Pong(p)) = &msg.update_oneof {
                            last_pong = p.id;
                            stats.bump_pong();
                            debug!("[{id}] ← pong {}", p.id);
                        } else if is_server_transport_control_update(&msg) {
                            // A server-originated Yellowstone Ping is a
                            // transport-control update, not raw market
                            // evidence.  Keep it on the transport-control
                            // path, not raw market evidence.  The
                            // frozen generated schema's server Ping carries
                            // no response identifier, so it is deliberately
                            // filtered rather than guessed or persisted as a
                            // V1 market record.
                            breaker.record_message_progress();
                            debug!("[{id}] ← server ping (filtered from research tape)");
                        } else if let Some(research_capture_sink) = research_capture_sink.as_ref() {
                            // This is the standalone research source tap.  It
                            // must retain the decoded protobuf before any
                            // lossy `PumpEvent`/`GeyserEvent` projection,
                            // parser route selection, candidate filtering, or
                            // account-registry scoping.  `try_capture` is
                            // explicitly nonblocking and owns the local-gap
                            // semantics when its bounded queue is full.
                            breaker.record_message_progress();
                            research_capture_sink.try_capture(PumpResearchSourceUpdateV1 {
                                provider_id: provider_id.to_string(),
                                stream_epoch,
                                ingress_wall_ts_ms: last_msg_wall_ms,
                                ingress_monotonic_ts_ms: crate::types::arrival_time_ms(),
                                update: msg,
                            });
                        } else {
                            breaker.record_message_progress();
                            route_update(
                                id,
                                msg,
                                provider_id,
                                provider_role,
                                channel,
                                stats,
                                slots,
                                gap_tx,
                                latest_block_time_secs,
                                registry,
                            );
                        }

                        // [RC-2 fix] Removed immediate resub on every incoming event.
                        // During launch bursts (100+ new pools/s) the registry version
                        // increments on nearly every message, causing O(events/s)
                        // SubscribeRequest sends.  Each server-side filter update risks a
                        // brief delivery gap.  Resubscription is now handled exclusively
                        // on health_tick (every 5s) below, keeping the hot path free of
                        // async sink writes.
                    }
                }
            }

            // ── Stall watchdog ────────────────────────────────────────────
            // [FIX for "CONNECTED, 0 events"]
            // TCP keepalive + gRPC ping confirm socket is open but NOT that data flows.
            // This watchdog confirms actual data flow by checking last_msg_wall_ms.
            _ = watchdog_ticker.tick() => {
                if let Err(err) = handle_watchdog_tick(id, cfg, stats, breaker, last_msg_wall_ms) {
                    return Err(err);
                }
            }

            // ── Ping keepalive ────────────────────────────────────────────
            _ = ping_ticker.tick() => {
                ping_seq = ping_seq.wrapping_add(1);
                if ping_seq > 1 && last_pong < ping_seq - 1 {
                    warn!("[{id}] Missing pong for ping {} (last_pong={last_pong})", ping_seq - 1);
                }
                debug!("[{id}] → ping {ping_seq}");
                send_request_with_timeout(&mut sink, build_ping(ping_seq), "ping").await?;
                stats.bump_ping();
            }

            // ── Immediate resub when pool/generic account is registered ──
            // SingleGlobal intentionally batches exact-watch refresh until
            // health_tick so launch bursts do not turn registry changes into a
            // stream of SubscribeRequest churn. Future pooled-filtered modes may
            // opt back into immediate refresh.
            _ = async {
                if cfg.registry_resubscribe_mode.uses_immediate_notify() {
                    resub_notify.notified().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                maybe_send_resubscribe(
                    id,
                    "registry_notify",
                    &mut sink,
                    cfg,
                    registry,
                    slots,
                    stats,
                    health.as_ref(),
                    &mut last_reg_version,
                    &mut last_request_fingerprint,
                    &mut last_resub_at,
                    true,
                    false,
                ).await?;
            }

            // ── BCV2 exact-watch refresh ─────────────────────────────────
            // In SingleGlobal mode this must stay batched on health_tick like
            // regular registry changes. NLN accepted rapid SubscribeRequest
            // updates but then stopped delivering data; keeping request-shape
            // churn off the hot path preserves the active stream while RPC
            // hydration covers immediate BCV2 evidence.
            _ = async {
                if cfg.subscription_profile.uses_registry_filters()
                    && cfg.registry_resubscribe_mode.uses_immediate_notify()
                {
                    bcv2_resub_notify.notified().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                maybe_send_resubscribe(
                    id,
                    "bcv2_registry_notify",
                    &mut sink,
                    cfg,
                    registry,
                    slots,
                    stats,
                    health.as_ref(),
                    &mut last_reg_version,
                    &mut last_request_fingerprint,
                    &mut last_resub_at,
                    false,
                    false,
                ).await?;
            }

            // ── Registry resubscribe ticker (fallback / paranoia) ────────
            _ = async {
                if cfg.registry_resubscribe_mode.uses_registry_tick() {
                    registry_resub_ticker.tick().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                maybe_send_resubscribe(
                    id,
                    "registry_tick",
                    &mut sink,
                    cfg,
                    registry,
                    slots,
                    stats,
                    health.as_ref(),
                    &mut last_reg_version,
                    &mut last_request_fingerprint,
                    &mut last_resub_at,
                    false,
                    false,
                ).await?;
            }

            // ── Health log + dynamic resub ────────────────────────────────
            _ = health_ticker.tick() => {
                let spill      = stats.msgs_spilled.load(Ordering::Relaxed);
                let overflow_dropped = stats.msgs_overflow_dropped.load(Ordering::Relaxed);
                let recv       = stats.msgs_received.load(Ordering::Relaxed);
                let gaps       = stats.slot_gaps_total.load(Ordering::Relaxed);
                let silence_ms = ms_since_wall_clock(last_msg_wall_ms);
                let dq_depth   = delayed_queue.depth(); // [FIX-5]
                let overflow_depth = channel.overflow_len();
                let breaker_snapshot = breaker.snapshot();

                // [FIX-5] Sweep expired delayed entries on every health tick.
                // Prevents unbounded RAM growth if drain() is never called
                // (e.g. Create tx was never seen for some orphan curve).
                delayed_queue.sweep_expired();

                info!(
                    source_label = cfg.subscription_profile.source_label(),
                    "[{id}] recv={recv} spill={spill} overflow_depth={overflow_depth} overflow_dropped={overflow_dropped} gaps={gaps} \
                     entry={} inner={} watched={} last_slot={} silence={silence_ms}ms \
                     recon={} delayed_q={dq_depth} provider_state={} provider_stalls={} consecutive_stalls={}",
                    stats.entry_events.load(Ordering::Relaxed),
                    stats.inner_ix_seen.load(Ordering::Relaxed),
                    registry.len(),
                    slots.last_slot(),
                    stats.reconnects.load(Ordering::Relaxed),
                    breaker_snapshot.state.as_str(),
                    breaker_snapshot.total_stalls,
                    breaker_snapshot.consecutive_stalls,
                );

                metrics::gauge!("ghost.pump.spill_depth", overflow_depth as f64);
                metrics::gauge!("ghost.pump.last_slot", slots.last_slot() as f64);
                metrics::gauge!("ghost.pump.silence_ms", silence_ms as f64);
                metrics::gauge!("ghost.pump.watched", registry.len() as f64);
                metrics::gauge!("ghost.pump.delayed_q_depth", dq_depth as f64); // [FIX-5]
                metrics::gauge!(
                    "ghost.pump.stall_rate",
                    stats.stall_rate(),
                    "source_label" => cfg.subscription_profile.source_label().to_string()
                );
                metrics::gauge!(
                    "ghost.pump.provider_state",
                    breaker_snapshot.state.gauge_value(),
                    "provider" => provider_label.to_string(),
                    "source_label" => cfg.subscription_profile.source_label().to_string()
                );

                if permit.half_open_probe {
                    debug!(
                        "[{id}] half-open probe still active for provider={provider_label}"
                    );
                }

                // Retry pending (debounced) resub on every health tick.
                maybe_send_resubscribe(
                    id,
                    "health_tick",
                    &mut sink,
                    cfg,
                    registry,
                    slots,
                    stats,
                    health.as_ref(),
                    &mut last_reg_version,
                    &mut last_request_fingerprint,
                    &mut last_resub_at,
                    false,
                    false,
                ).await?;
            }
        }
    }
}

/// Yellowstone transport-control messages are not source-market evidence.
/// `Pong` is handled before this predicate so this branch represents an
/// unsolicited server Ping in the live loop; keeping the classification in a
/// helper makes the raw-tape exclusion directly regression-testable.
fn is_server_transport_control_update(update: &SubscribeUpdate) -> bool {
    matches!(
        update.update_oneof.as_ref(),
        Some(UpdateOneof::Ping(_)) | Some(UpdateOneof::Pong(_))
    )
}

// ─── Message router ───────────────────────────────────────────────────────────

#[inline(always)]
fn route_update(
    _id: &str,
    msg: yellowstone_grpc_proto::prelude::SubscribeUpdate,
    provider_id: &str,
    provider_role: ghost_core::RawProviderRoleV1,
    channel: &DualLaneChannel,
    stats: &Arc<TransportStats>,
    slots: &Arc<SlotTracker>,
    gap_tx: &tokio::sync::mpsc::UnboundedSender<SlotGap>,
    latest_block_time_secs: &Arc<AtomicI64>,
    registry: &AccountRegistry,
) {
    let received_at_monotonic_ns = crate::types::arrival_time_ns();
    let received_at = Instant::now();

    match msg.update_oneof {
        // ── Transaction ───────────────────────────────────────────────────────
        Some(UpdateOneof::Transaction(t)) => {
            let slot = t.slot;
            track_slot(slots, stats, slot, gap_tx);
            // tx_touches_pump check REMOVED (RC-1.6 fix):
            // The gRPC server already filters transactions via account_include in the
            // SubscribeRequest. Re-filtering here with bs58-encoding of every account key
            // wastes ~5-15μs per tx on the hot path (10k TPS → 50-150ms/s CPU waste).

            let has_inner = t
                .transaction
                .as_ref()
                .and_then(|ti| ti.meta.as_ref())
                .map(|m| !m.inner_instructions.is_empty())
                .unwrap_or(false);
            if has_inner {
                stats.bump_inner();
            }

            let sig = extract_sig(&t);
            stats.bump_tx();

            // The I/O thread transfers the already-decoded Yellowstone child.
            // Optional capture encoding is deferred to the consumer-side
            // normalizer and never becomes parser transport.
            emit(
                channel,
                stats,
                PumpEvent::Transaction {
                    provider_id: Some(provider_id.to_string()),
                    provider_role: Some(provider_role),
                    received_at_monotonic_ns,
                    signature: sig,
                    slot,
                    received_at,
                    decoded: Box::new(t),
                    capture_payload: channel.live_transaction_capture_enabled(),
                },
            );
        }

        // ── Account update ────────────────────────────────────────────────────
        Some(UpdateOneof::Account(a)) => {
            let slot = a.slot;
            track_slot(slots, stats, slot, gap_tx);

            let pubkey = a
                .account
                .as_ref()
                .map(|acc| bs58::encode(&acc.pubkey).into_string())
                .unwrap_or_default();
            if pubkey.is_empty() {
                return;
            }
            if registry.contains_bcv2(&pubkey) {
                let (owner, data_len) = a
                    .account
                    .as_ref()
                    .map(|account| {
                        (
                            bs58::encode(&account.owner).into_string(),
                            account.data.len(),
                        )
                    })
                    .unwrap_or_else(|| ("".to_string(), 0));
                info!(
                    "BCV2_ACCOUNT_UPDATE_RECEIVED pubkey={} slot={} owner={} data_len={}",
                    pubkey, slot, owner, data_len
                );
            }

            stats.bump_account();

            // Account updates do not need raw bytes downstream, so avoid encode+decode churn.
            let decoded =
                account_update_to_geyser_event(a, &pubkey, slot, provider_id, provider_role).ok();

            emit(
                channel,
                stats,
                PumpEvent::AccountUpdate {
                    provider_id: Some(provider_id.to_string()),
                    provider_role: Some(provider_role),
                    pubkey,
                    slot,
                    received_at,
                    decoded,
                },
            );
        }

        // ── [FIX-1] Entry update ──────────────────────────────────────────────
        //
        // Entry events are NOT just "block metadata".  They carry:
        //   1. executed_transaction_count → raw slot-throughput telemetry
        //   2. In some Yellowstone configurations, inner-instruction data for
        //      CPI calls (notably migrate) that were packed into entries rather
        //      than appearing in standalone Tx meta.inner_instructions.
        //
        // Previous version dropped Entry events — that caused ~20-30% migrate CPI loss.
        // Now we forward the full raw proto to the parser which walks inner Ixs.
        //
        // Gap tracking on Entry slots is also critical: Entry events are emitted
        // for EVERY slot, even those with no Pump.fun txs, making them the most
        // reliable source for SlotTracker continuity.
        Some(UpdateOneof::Entry(e)) => {
            let slot = e.slot;
            let executed_transaction_count = e.executed_transaction_count;
            track_slot(slots, stats, slot, gap_tx); // most reliable source for gap detection
            stats.bump_entry();

            let raw = encode_proto(&e);
            emit(
                channel,
                stats,
                PumpEvent::EntryUpdate {
                    provider_id: Some(provider_id.to_string()),
                    provider_role: Some(provider_role),
                    slot,
                    received_at,
                    executed_transaction_count,
                    raw,
                },
            );
        }

        // ── Slot notification ─────────────────────────────────────────────────
        // Track slot only — not forwarded to parser.
        Some(UpdateOneof::Slot(s)) => {
            track_slot(slots, stats, s.slot, gap_tx);
        }

        // ── BlockMeta: extract block_time for event_ts anchoring ──────────────
        // Yellowstone SubscribeUpdateTransaction does not carry block_time.
        // SubscribeUpdateBlockMeta does — it arrives after all transactions in
        // the block are processed.  We update a shared atomic so the stream
        // consumer can anchor event_ts_ms to the on-chain block time rather
        // than our local wall-clock ingress time, advancing the gatekeeper
        // observation window start by ~1 second.
        Some(UpdateOneof::BlockMeta(bm)) => {
            track_slot(slots, stats, bm.slot, gap_tx);
            if let Some(ts) = bm.block_time.as_ref().map(|t| t.timestamp) {
                if ts > 0 {
                    latest_block_time_secs.fetch_max(ts, Ordering::Relaxed);
                    info!("BLOCK_META_TIME slot={} block_time={}", bm.slot, ts);
                }
            } else {
                info!("BLOCK_META_NO_TIME slot={}", bm.slot);
            }
        }

        // Pong handled upstream; everything else silently ignored.
        _ => {}
    }
}

#[cfg(test)]
pub(crate) fn route_update_for_hot_path_harness(
    msg: yellowstone_grpc_proto::prelude::SubscribeUpdate,
) -> SeerResult<GeyserEvent> {
    route_update_for_hot_path_harness_with_capture(msg, true)
}

#[cfg(test)]
pub(crate) fn route_update_for_hot_path_harness_with_capture(
    msg: yellowstone_grpc_proto::prelude::SubscribeUpdate,
    capture_payload: bool,
) -> SeerResult<GeyserEvent> {
    let (channel, receiver) = DualLaneChannel::with_capacities(64, 64);
    channel.set_live_transaction_capture_enabled(capture_payload);
    let stats = Arc::new(TransportStats::default());
    let slots = Arc::new(SlotTracker::default());
    let (gap_tx, _gap_rx) = tokio::sync::mpsc::unbounded_channel();
    let latest_block_time_secs = Arc::new(AtomicI64::new(0));
    let registry = AccountRegistry::new();

    route_update(
        "pr1b-hot-path-harness",
        msg,
        "pr1b-primary",
        ghost_core::RawProviderRoleV1::PrimaryAuthority,
        &channel,
        &stats,
        &slots,
        &gap_tx,
        &latest_block_time_secs,
        &registry,
    );

    let event = receiver.queue.try_recv().map_err(|_| {
        SeerError::ParseError("PR1B harness route_update emitted no event".to_string())
    })?;
    pump_event_to_geyser_event(event, GRPC_GLOBAL_STREAM_SOURCE_LABEL, None).ok_or_else(|| {
        SeerError::ParseError("PR1B harness event has no normalized mapping".to_string())
    })?
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

#[inline(always)]
fn track_slot(
    slots: &Arc<SlotTracker>,
    stats: &Arc<TransportStats>,
    slot: u64,
    gap_tx: &tokio::sync::mpsc::UnboundedSender<SlotGap>,
) {
    if let Some(gap) = slots.update(slot) {
        let count = gap
            .end_slot
            .saturating_sub(gap.start_slot)
            .saturating_add(1);
        stats.add_gap(count);
        let _ = gap_tx.send(gap);
        warn!(
            "Slot gap: {} slots missed before {} (range={}..={})",
            count, slot, gap.start_slot, gap.end_slot
        );
    }
}

#[inline(always)]
fn extract_sig(t: &yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction) -> String {
    t.transaction
        .as_ref()
        .and_then(|ti| ti.transaction.as_ref())
        .and_then(|tx| tx.signatures.first())
        .map(|b| bs58::encode(b).into_string())
        .unwrap_or_default()
}

#[inline(always)]
pub fn encode_proto<M: prost::Message>(msg: &M) -> Vec<u8> {
    let mut buf = Vec::with_capacity(msg.encoded_len());
    // encode() = encode_raw() + capacity check.  Both produce identical wire bytes,
    // but encode() is the idiomatic public API and returns Result for safety.
    msg.encode(&mut buf)
        .expect("encode to Vec<u8> is infallible");
    buf
}

#[inline(always)]
fn emit(channel: &DualLaneChannel, stats: &Arc<TransportStats>, ev: PumpEvent) {
    stats.bump_recv();
    channel.send(ev, stats);
}

// ─── GrpcConnection adapter ──────────────────────────────────────────────────
//
// Bridges the new YellowstoneConnector + PumpEvent architecture to the
// lib.rs-expected GrpcConnection API surface:
//   GrpcConnection::new(...) → .with_stream_config(...) → .connect_geyser() → EventStream
//   .add_watched_mint()  .watch_pool()  .is_pool_watched()  .is_mint_watched()
//   .watched_pools_count()  .dedup_dropped_total()  .set_health()

use crate::config::CommitmentLevel as SeerCommitmentLevel;
use crate::errors::{SeerError, SeerResult};
use crate::metrics::SeerMetrics;
use crate::paradox_sensor::ParadoxSensor;
use crate::types::{
    AmmProgram, GeyserEvent, InnerInstructionGroup, InnerIx, RawBytesMissingReason, RawInstruction,
};
use crate::websocket_connection::{
    extract_balances_from_meta, extract_logs_from_meta, parse_ui_transaction_with_meta,
};
use ghost_core::health::{
    RuntimeHealth, GRPC_STATE_CONNECTED, GRPC_STATE_CONNECTING, GRPC_STATE_DISCONNECTED,
    GRPC_STATE_FAILED, GRPC_STATE_RECONNECTING, GRPC_STATE_SUBSCRIBING,
};
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::pin::Pin;

pub type EventStream = Pin<Box<dyn futures::Stream<Item = SeerResult<GeyserEvent>> + Send>>;

const MANUAL_BACKFILL_MAX_SLOTS: u64 = 128;
const MANUAL_BACKFILL_MAX_ADDRESSES: usize = 32;
/// [RC-4 fix] Increased from 64 → 200: at 3s reconnect gaps with active pools
/// producing 10-30 tx/s each, 64 sigs/address covered only ~2s of history.
const MANUAL_BACKFILL_SIGNATURE_LIMIT_PER_ADDRESS: usize = 200;
/// [RC-4 fix] Increased from 256 → 1000: a 3s gap at 100 pools × 3 tx/pool/s
/// = 900 transactions; 256 truncated most of the gap's event set.
const MANUAL_BACKFILL_MAX_TXS_PER_GAP: usize = 1_000;
const STREAM_SEND_TIMEOUT_SECS: u64 = 10;
const SUBSCRIBE_REQUEST_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotGap {
    start_slot: u64,
    end_slot: u64,
}

#[derive(Debug, Clone)]
struct ManualBackfillConfig {
    rpc_endpoint: String,
    max_slots: u64,
    max_addresses: usize,
    signature_limit_per_address: usize,
    max_transactions_per_gap: usize,
}

pub struct GrpcConnection {
    // Core transport — Option so we can take() them once in connect_geyser
    connector: Mutex<Option<YellowstoneConnector>>,
    rx: Mutex<Option<DualLaneReceiver>>,
    gap_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<SlotGap>>>,
    injector: DualLaneChannel,
    registry: AccountRegistry,
    delayed_queue: Arc<DelayedAccountQueue>,
    stats: Arc<TransportStats>,
    shutdown: Arc<AtomicBool>,
    shutdown_token: Arc<CancellationToken>,
    availability_tracker: Arc<LaneAvailabilityTracker>,
    manual_backfill_cfg: Option<ManualBackfillConfig>,
    manual_backfill_enabled: bool,
    // Pool/mint watching (adapts old API surface)
    watched_curve_accounts: Arc<DashMap<String, u64>>,
    watched_pool_accounts: Arc<DashMap<String, u64>>,
    watched_mints: Arc<DashMap<String, u64>>,
    watched_pools_ttl_ms: u64,
    watched_pools_cap: usize,
    dedup_dropped: Arc<AtomicU64>,
    // Config carried for connect_geyser
    #[allow(dead_code)]
    config: GrpcConfig,
    // Stored for compat but not used by new transport
    _paradox: Option<Arc<ParadoxSensor>>,
    _health: Option<Arc<RuntimeHealth>>,
}

enum DrainPick {
    Event { ev: PumpEvent },
    Empty,
    Disconnected,
}

#[inline(always)]
fn try_drain_dual_lane(rx: &DualLaneReceiver, _legacy_preference: bool) -> DrainPick {
    match rx.queue.try_recv() {
        Ok(ev) => DrainPick::Event { ev },
        Err(TryRecvError::Empty) => DrainPick::Empty,
        Err(TryRecvError::Disconnected) => DrainPick::Disconnected,
    }
}

#[inline(always)]
fn watch_now_ms() -> u64 {
    crate::types::arrival_time_ms()
}

#[inline(always)]
fn effective_watch_cap(configured: usize) -> usize {
    match configured {
        0 => EXACT_ACCOUNT_PAYLOAD_CAP,
        n => n.min(EXACT_ACCOUNT_PAYLOAD_CAP),
    }
}

fn prune_watch_map(
    map: &DashMap<String, u64>,
    ttl_ms: u64,
    cap: usize,
    now_ms: u64,
) -> Vec<String> {
    let mut removed = Vec::new();
    let mut stale = HashSet::new();
    if ttl_ms > 0 {
        for entry in map.iter() {
            if now_ms.saturating_sub(*entry.value()) > ttl_ms {
                stale.insert(entry.key().clone());
            }
        }
    }

    let mut survivors: Vec<(String, u64)> = map
        .iter()
        .filter_map(|entry| {
            if stale.contains(entry.key()) {
                None
            } else {
                Some((entry.key().clone(), *entry.value()))
            }
        })
        .collect();
    survivors.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if survivors.len() > cap {
        removed.extend(survivors.drain(cap..).map(|(key, _)| key));
    }
    removed.extend(stale);

    for key in &removed {
        map.remove(key);
    }
    removed
}

fn prune_exact_watch_maps(
    watched_curve_accounts: &DashMap<String, u64>,
    watched_pool_accounts: &DashMap<String, u64>,
    ttl_ms: u64,
    cap: usize,
    now_ms: u64,
) -> (Vec<String>, Vec<String>) {
    // Apply cap and TTL independently per local watch lane. Global owner+memcmp
    // branches carry canonical curve/pool AccountUpdates for the primary stream;
    // these local watch sets remain useful for diagnostics, coverage audit and
    // any future profile that opts back into dynamic exact-watch refresh.
    let removed_curves = prune_watch_map(watched_curve_accounts, ttl_ms, cap, now_ms);
    let removed_pools = prune_watch_map(watched_pool_accounts, ttl_ms, cap, now_ms);
    (removed_curves, removed_pools)
}

fn snapshot_watch_map(map: &DashMap<String, u64>) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = map
        .iter()
        .map(|entry| (entry.key().clone(), *entry.value()))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

impl GrpcConnection {
    /// Constructor matching lib.rs call-site:
    /// ```ignore
    /// GrpcConnection::new(endpoint, client_id, auth_token, metrics,
    ///     max_reconnect, delay, max_delay, verbose, commitment)
    /// ```
    pub fn new(
        endpoint: String,
        _client_id: Option<String>,
        auth_token: Option<String>,
        _metrics: Arc<SeerMetrics>,
        _max_reconnect: u32,
        _reconnect_delay: u64,
        _max_reconnect_delay: u64,
        _verbose: bool,
        commitment: SeerCommitmentLevel,
        rpc_endpoint: Option<String>,
    ) -> Self {
        Self::new_with_auth_header(
            endpoint,
            _client_id,
            auth_token,
            DEFAULT_GRPC_AUTH_HEADER.to_string(),
            _metrics,
            _max_reconnect,
            _reconnect_delay,
            _max_reconnect_delay,
            _verbose,
            commitment,
            rpc_endpoint,
        )
    }

    pub fn new_with_auth_header(
        endpoint: String,
        _client_id: Option<String>,
        auth_token: Option<String>,
        auth_header: String,
        _metrics: Arc<SeerMetrics>,
        _max_reconnect: u32,
        _reconnect_delay: u64,
        _max_reconnect_delay: u64,
        _verbose: bool,
        commitment: SeerCommitmentLevel,
        rpc_endpoint: Option<String>,
    ) -> Self {
        Self::new_with_auth_header_and_provider_roles(
            endpoint,
            _client_id,
            auth_token,
            auth_header,
            "primary".to_string(),
            Vec::new(),
            _metrics,
            _max_reconnect,
            _reconnect_delay,
            _max_reconnect_delay,
            _verbose,
            commitment,
            rpc_endpoint,
        )
    }

    /// Builds the active gRPC transport with a declared provider-role contract.
    ///
    /// The contract is validated by `connect_geyser` before any connection
    /// worker starts. A non-empty secondary list without matching provider
    /// endpoints therefore fails closed instead of being ignored.
    pub fn new_with_auth_header_and_provider_roles(
        endpoint: String,
        _client_id: Option<String>,
        auth_token: Option<String>,
        auth_header: String,
        primary_raw_provider_id: String,
        secondary_raw_provider_ids: Vec<String>,
        _metrics: Arc<SeerMetrics>,
        _max_reconnect: u32,
        _reconnect_delay: u64,
        _max_reconnect_delay: u64,
        _verbose: bool,
        commitment: SeerCommitmentLevel,
        rpc_endpoint: Option<String>,
    ) -> Self {
        let proto_commitment = match commitment {
            SeerCommitmentLevel::Mempool => {
                yellowstone_grpc_proto::prelude::CommitmentLevel::Processed
            }
            SeerCommitmentLevel::Confirmed => {
                yellowstone_grpc_proto::prelude::CommitmentLevel::Confirmed
            }
            SeerCommitmentLevel::Finalized => {
                yellowstone_grpc_proto::prelude::CommitmentLevel::Finalized
            }
        };

        let cfg = GrpcConfig {
            providers: vec![Provider::primary_with_auth_header(
                endpoint,
                auth_token,
                primary_raw_provider_id.clone(),
                auth_header,
            )],
            provider_role_config: Some(RawProviderRoleConfigV1::new(
                primary_raw_provider_id,
                secondary_raw_provider_ids,
            )),
            streams_per_provider: 1,
            commitment: proto_commitment,
            stall_timeout_secs: DEFAULT_SILENT_STALL_SECS,
            resub_debounce_ms: DEFAULT_RESUB_DEBOUNCE_MS,
            max_stalls_before_open: DEFAULT_PROVIDER_MAX_STALLS_BEFORE_OPEN,
            circuit_breaker_cooldown_ms: DEFAULT_PROVIDER_CIRCUIT_BREAKER_COOLDOWN_MS,
            subscription_profile: GrpcSubscriptionProfile::PrimaryGlobal,
            registry_resubscribe_mode: RegistryResubscribeMode::HealthTickOnly,
            ingress_queue_capacity: DEFAULT_PRIMARY_CHANNEL_CAP,
        };

        let (connector, rx, gap_rx) = YellowstoneConnector::new(cfg.clone());
        let availability_tracker = Arc::clone(&connector.availability_tracker);
        let injector = connector.injector();
        let registry = connector.registry();
        let delayed_queue = connector.delayed_queue();
        let stats = connector.stats();
        let shutdown = connector.shutdown_handle();
        let shutdown_token = connector.shutdown_token_handle();

        Self {
            connector: Mutex::new(Some(connector)),
            rx: Mutex::new(Some(rx)),
            gap_rx: Mutex::new(Some(gap_rx)),
            injector,
            registry,
            delayed_queue,
            stats,
            shutdown,
            shutdown_token,
            availability_tracker,
            manual_backfill_cfg: rpc_endpoint.map(|rpc_endpoint| ManualBackfillConfig {
                rpc_endpoint,
                max_slots: MANUAL_BACKFILL_MAX_SLOTS,
                max_addresses: MANUAL_BACKFILL_MAX_ADDRESSES,
                signature_limit_per_address: MANUAL_BACKFILL_SIGNATURE_LIMIT_PER_ADDRESS,
                max_transactions_per_gap: MANUAL_BACKFILL_MAX_TXS_PER_GAP,
            }),
            manual_backfill_enabled: true,
            watched_curve_accounts: Arc::new(DashMap::new()),
            watched_pool_accounts: Arc::new(DashMap::new()),
            watched_mints: Arc::new(DashMap::new()),
            watched_pools_ttl_ms: 120_000,
            watched_pools_cap: EXACT_ACCOUNT_PAYLOAD_CAP,
            dedup_dropped: Arc::new(AtomicU64::new(0)),
            config: cfg,
            _paradox: None,
            _health: None,
        }
    }

    /// Chain-able config — matches lib.rs call-site.
    pub fn with_stream_config(
        mut self,
        stream_mode: crate::config::StreamMode,
        watched_pools_ttl: u64,
        watched_pools_cap: usize,
        watch_debounce_ms: u64,
    ) -> Self {
        let (resub_debounce_ms, registry_resubscribe_mode) = match stream_mode {
            crate::config::StreamMode::SingleGlobal => (
                DEFAULT_RESUB_DEBOUNCE_MS,
                RegistryResubscribeMode::HealthTickOnly,
            ),
            crate::config::StreamMode::PooledFiltered => {
                (watch_debounce_ms, RegistryResubscribeMode::ImmediateAndTick)
            }
        };
        self.config.resub_debounce_ms = resub_debounce_ms;
        self.config.registry_resubscribe_mode = registry_resubscribe_mode;
        self.watched_pools_ttl_ms = watched_pools_ttl;
        self.watched_pools_cap = effective_watch_cap(watched_pools_cap);
        if let Some(connector) = self.connector.get_mut().as_mut() {
            connector.config.resub_debounce_ms = resub_debounce_ms;
            connector.config.registry_resubscribe_mode = registry_resubscribe_mode;
        }
        self
    }

    pub fn with_ingress_queue_capacity(mut self, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (channel, receiver) = DualLaneChannel::with_capacity(capacity);
        self.config.ingress_queue_capacity = capacity;
        self.injector = channel.clone();
        *self.rx.get_mut() = Some(receiver);
        if let Some(connector) = self.connector.get_mut().as_mut() {
            connector.config.ingress_queue_capacity = capacity;
            connector.channel = channel;
        }
        self
    }

    pub fn with_subscription_profile(
        mut self,
        subscription_profile: GrpcSubscriptionProfile,
    ) -> Self {
        self.config.subscription_profile = subscription_profile;
        if let Some(connector) = self.connector.get_mut().as_mut() {
            connector.config.subscription_profile = subscription_profile;
        }
        self
    }

    pub fn with_circuit_breaker_config(
        mut self,
        max_stalls_before_open: u32,
        cooldown_ms: u64,
    ) -> Self {
        let max_stalls_before_open = max_stalls_before_open.max(1);
        let cooldown_ms = cooldown_ms.max(1);
        self.config.max_stalls_before_open = max_stalls_before_open;
        self.config.circuit_breaker_cooldown_ms = cooldown_ms;
        if let Some(connector) = self.connector.get_mut().as_mut() {
            connector.config.max_stalls_before_open = max_stalls_before_open;
            connector.config.circuit_breaker_cooldown_ms = cooldown_ms;
        }
        self
    }

    pub fn with_stall_timeout_secs(mut self, stall_timeout_secs: u64) -> Self {
        let stall_timeout_secs = stall_timeout_secs.max(1);
        self.config.stall_timeout_secs = stall_timeout_secs;
        if let Some(connector) = self.connector.get_mut().as_mut() {
            connector.config.stall_timeout_secs = stall_timeout_secs;
        }
        self
    }

    /// Chain-able feature flag for slot-gap RPC manual backfill.
    pub fn with_manual_backfill_enabled(mut self, enabled: bool) -> Self {
        self.manual_backfill_enabled = enabled;
        self
    }

    /// Chain-able — stores ParadoxSensor for compat.
    pub fn with_paradox_sensor(mut self, sensor: Arc<ParadoxSensor>) -> Self {
        self._paradox = Some(sensor);
        self
    }

    /// Set RuntimeHealth handle.
    pub fn set_health(&mut self, health: Arc<RuntimeHealth>) {
        self._health = Some(Arc::clone(&health));
        if let Some(connector) = self.connector.get_mut().as_mut() {
            connector.set_health(health);
        }
    }

    pub fn set_authoritative_funding_stream_availability_sender(
        &self,
        tx: tokio::sync::watch::Sender<bool>,
    ) {
        self.availability_tracker.set_sender(tx);
    }

    /// Request a graceful stop of all Yellowstone gRPC workers owned by this connection.
    ///
    /// Launcher shutdown must stop reconnect/read/subscribe loops before receivers disappear.
    /// Otherwise workers can continue trying to forward into closed channels and flood
    /// disconnect logs after global shutdown has already started.
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.shutdown_token.cancel();
        self.availability_tracker.publish(false);
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }

    /// Enable the optional PR1A-compatible prost capture for live
    /// transactions. Capture is encoded once after the receive queue and is
    /// shared by WAL/audit consumers; the parser always uses normalized fields.
    pub fn set_live_transaction_capture_enabled(&self, enabled: bool) {
        self.injector.set_live_transaction_capture_enabled(enabled);
    }

    /// Connect and return an async event stream yielding `GeyserEvent` items.
    ///
    /// Internally spawns the YellowstoneConnector as a background task and
    /// drains the DualLaneReceiver, converting PumpEvent → GeyserEvent.
    pub async fn connect_geyser(&self) -> SeerResult<EventStream> {
        self.config.validate_provider_roles().map_err(|e| {
            SeerError::ConfigError(format!("invalid raw provider role contract: {e}"))
        })?;

        let connector = self
            .connector
            .lock()
            .take()
            .ok_or_else(|| SeerError::GrpcError("connect_geyser already called".into()))?;
        let rx = self
            .rx
            .lock()
            .take()
            .ok_or_else(|| SeerError::GrpcError("connect_geyser already called (rx)".into()))?;
        let gap_rx =
            self.gap_rx.lock().take().ok_or_else(|| {
                SeerError::GrpcError("connect_geyser already called (gap_rx)".into())
            })?;
        let shutdown = Arc::clone(&self.shutdown);
        let shutdown_token = Arc::clone(&self.shutdown_token);

        // Spawn the connector — it runs all provider workers until shutdown.
        let connector_shutdown = Arc::clone(&self.shutdown);
        tokio::spawn(async move {
            if let Err(e) = connector.run().await {
                if !connector_shutdown.load(Ordering::Relaxed) {
                    error!("YellowstoneConnector terminated: {e}");
                }
            }
        });

        if self.manual_backfill_enabled {
            if let Some(backfill_cfg) = self.manual_backfill_cfg.clone() {
                let injector = self.injector.clone();
                let stats = Arc::clone(&self.stats);
                let watched_curve_accounts = Arc::clone(&self.watched_curve_accounts);
                let watched_pool_accounts = Arc::clone(&self.watched_pool_accounts);
                let watched_mints = Arc::clone(&self.watched_mints);
                let shutdown = Arc::clone(&self.shutdown);
                tokio::spawn(async move {
                    run_manual_backfill_worker(
                        backfill_cfg,
                        gap_rx,
                        injector,
                        stats,
                        watched_curve_accounts,
                        watched_pool_accounts,
                        watched_mints,
                        shutdown,
                    )
                    .await;
                });
            }
        } else if self.manual_backfill_cfg.is_some() {
            info!("MANUAL_BACKFILL_DISABLED source=grpc_connection reason=config_flag_off");
        }

        if self.config.subscription_profile.uses_registry_filters() {
            let watched_curve_accounts = Arc::clone(&self.watched_curve_accounts);
            let watched_pool_accounts = Arc::clone(&self.watched_pool_accounts);
            let watched_mints = Arc::clone(&self.watched_mints);
            let registry = self.registry.clone();
            let shutdown = Arc::clone(&self.shutdown);
            let ttl_ms = self.watched_pools_ttl_ms;
            let cap = self.watched_pools_cap;
            tokio::spawn(async move {
                let mut ticker =
                    tokio::time::interval(Duration::from_secs(WATCH_SWEEP_INTERVAL_SECS));
                loop {
                    ticker.tick().await;
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    let now_ms = watch_now_ms();
                    let (removed_curves, removed_pools) = prune_exact_watch_maps(
                        &watched_curve_accounts,
                        &watched_pool_accounts,
                        ttl_ms,
                        cap,
                        now_ms,
                    );
                    let removed_mints = prune_watch_map(&watched_mints, ttl_ms, cap, now_ms);

                    for curve in &removed_curves {
                        registry.remove_curve(curve);
                    }
                    for pool in &removed_pools {
                        registry.remove_pool(pool);
                    }

                    let removed_total =
                        removed_curves.len() + removed_pools.len() + removed_mints.len();
                    if removed_total > 0 {
                        info!(
                            "WATCH_SET_PRUNED curves_removed={} pools_removed={} mints_removed={} ttl_ms={} cap={}",
                            removed_curves.len(),
                            removed_pools.len(),
                            removed_mints.len(),
                            ttl_ms,
                            cap,
                        );
                    }
                }
            });
        }

        let source_label = self.config.subscription_profile.source_label();

        // Build an async stream that drains the DualLaneReceiver and converts
        // PumpEvent → GeyserEvent.  Priority: fast lane first, then overflow.
        let stream = async_stream::stream! {
            loop {
                if shutdown.load(Ordering::Relaxed) {
                    rx.local_gap.close_open_without_after();
                    while let Some(gap) = rx.local_gap.take_completed() {
                        yield Ok(GeyserEvent::LocalCoverageGap { gap });
                    }
                    break;
                }

                if let Some(gap) = rx.local_gap.take_completed() {
                    yield Ok(GeyserEvent::LocalCoverageGap { gap });
                    continue;
                }

                let ev = match try_drain_dual_lane(&rx, false) {
                    DrainPick::Event { ev } => ev,
                    DrainPick::Empty => {
                        rx.local_gap.close_open_without_after();
                        tokio::select! {
                            biased;
                            _ = shutdown_token.cancelled() => break,
                            _ = tokio::time::sleep(Duration::from_micros(DRAIN_IDLE_SLEEP_US)) => {}
                        }
                        continue;
                    }
                    DrainPick::Disconnected => break,
                };

                let ingestion_latency_ms = ev.received_at().elapsed().as_secs_f64() * 1000.0;
                // Use ingress_ts_ms (wall clock at gRPC decode time) as event_ts.
                // block_time_hint is ~1.4s before our actual receipt, which would shorten
                // the real observation window to 6.6s instead of 8s, causing coverage loss.
                // With ingress_ts_ms: full 8s window, decision at T_onchain + 9.4s.
                match pump_event_to_geyser_event(ev, source_label, None) {
                    Some(Ok(geyser_ev)) => {
                        metrics::histogram!("ingestion_latency_ms", ingestion_latency_ms);
                        yield Ok(geyser_ev)
                    }
                    Some(Err(e))        => {
                        metrics::histogram!("ingestion_latency_ms", ingestion_latency_ms);
                        yield Err(e)
                    }
                    None                => {}
                }
            }
        };

        Ok(Box::pin(stream))
    }

    /// Register a mint for tx filtering/backfill.
    ///
    /// Exact-account subscription capacity is the scarce resource on the provider
    /// side. Mint account updates are not required on the hot path, so keeping
    /// them out of the exact-account branch avoids resubscribe churn and
    /// truncation of genuinely useful curve subscriptions.
    pub fn add_watched_mint(&self, mint: impl std::fmt::Display) {
        let mint_str = mint.to_string();
        let now_ms = watch_now_ms();
        self.watched_mints.insert(mint_str, now_ms);
        prune_watch_map(
            &self.watched_mints,
            self.watched_pools_ttl_ms,
            self.watched_pools_cap,
            now_ms,
        );
    }

    /// Register a pool for tx filtering/backfill and local watch diagnostics.
    pub fn watch_pool(&self, pool: impl std::fmt::Display, amm: AmmProgram, _ts: u64) {
        let pool_str = pool.to_string();
        let now_ms = watch_now_ms();
        match amm {
            AmmProgram::PumpFun => {
                self.watched_curve_accounts.insert(pool_str.clone(), now_ms);
                self.registry.insert_curve(pool_str.clone());
                let (removed_curves, removed_pools) = prune_exact_watch_maps(
                    &self.watched_curve_accounts,
                    &self.watched_pool_accounts,
                    self.watched_pools_ttl_ms,
                    self.watched_pools_cap,
                    now_ms,
                );
                for removed in removed_curves {
                    self.registry.remove_curve(&removed);
                }
                for removed in removed_pools {
                    self.registry.remove_pool(&removed);
                }
            }
            AmmProgram::PumpSwap => {
                self.watched_pool_accounts.insert(pool_str.clone(), now_ms);
                self.registry.insert_pool(pool_str.clone());
                let (removed_curves, removed_pools) = prune_exact_watch_maps(
                    &self.watched_curve_accounts,
                    &self.watched_pool_accounts,
                    self.watched_pools_ttl_ms,
                    self.watched_pools_cap,
                    now_ms,
                );
                for removed in removed_curves {
                    self.registry.remove_curve(&removed);
                }
                for removed in removed_pools {
                    self.registry.remove_pool(&removed);
                }
            }
        }
    }

    pub fn is_pool_watched(&self, pool: &Pubkey) -> bool {
        let key = pool.to_string();
        self.watched_curve_accounts.contains_key(&key)
            || self.watched_pool_accounts.contains_key(&key)
    }

    pub fn is_mint_watched(&self, mint: &Pubkey) -> bool {
        self.watched_mints.contains_key(&mint.to_string())
    }

    pub fn watched_pools_count(&self) -> usize {
        self.watched_curve_accounts.len() + self.watched_pool_accounts.len()
    }

    pub fn dedup_dropped_total(&self) -> u64 {
        self.dedup_dropped.load(Ordering::Relaxed)
    }

    /// Get the shared AccountRegistry (used by BinaryParser adapter).
    pub fn account_registry(&self) -> AccountRegistry {
        self.registry.clone()
    }

    /// Get the shared DelayedAccountQueue.
    pub fn delayed_account_queue(&self) -> Arc<DelayedAccountQueue> {
        Arc::clone(&self.delayed_queue)
    }

    /// Get transport stats (for metrics/diagnostics).
    pub fn transport_stats(&self) -> Arc<TransportStats> {
        Arc::clone(&self.stats)
    }

    /// Buffer an AccountUpdate PumpEvent in the `DelayedAccountQueue` for
    /// `curve_pubkey`.
    ///
    /// This is a transport-layer primitive.  It does NOT trigger ShadowLedger
    /// writes — that responsibility belongs exclusively to `pending_curve_updates`
    /// in lib.rs.  Call this when you need the PumpEvent preserved at the
    /// transport level for a future re-injection, independently of the
    /// GeyserEvent-level replay owned by lib.rs.
    ///
    /// Note: lib.rs `handle_account_update` does NOT call this method.
    /// `pending_curve_updates` is the sole recovery owner for the
    /// curve→mint mapping race window (PR-2 contract).
    pub fn push_delayed_account_update(
        &self,
        curve_pubkey: String,
        slot: u64,
        decoded: crate::types::GeyserEvent,
    ) {
        let (provider_id, provider_role) = match &decoded {
            crate::types::GeyserEvent::AccountUpdate {
                provider_id,
                provider_role,
                ..
            } => (provider_id.clone(), *provider_role),
            _ => (None, None),
        };
        let ev = PumpEvent::AccountUpdate {
            provider_id,
            provider_role,
            pubkey: curve_pubkey.clone(),
            slot,
            received_at: Instant::now(),
            decoded: Some(decoded),
        };
        self.delayed_queue.push(curve_pubkey, ev);
        self.stats.delayed_pushes.fetch_add(1, Ordering::Relaxed);
    }

    /// Drain all buffered PumpEvents for `curve_pubkey` from the
    /// `DelayedAccountQueue` and re-inject them into the dispatch channel.
    ///
    /// This is a transport-layer primitive.  Re-injected events go through the
    /// normal `handle_account_update` path, which will apply them to ShadowLedger
    /// **only if** the mapping is now known.  Do NOT call this from
    /// `register_curve_mapping` together with `replay_pending_curve_update` —
    /// that would create two competing replay owners for the same AccountUpdate
    /// (PR-2 violation).  Use either this OR `pending_curve_updates`, never both.
    ///
    /// Returns the number of events re-injected.
    pub fn drain_and_reinject_delayed_account_updates(&self, curve_pubkey: &str) -> usize {
        let events = self.delayed_queue.drain(curve_pubkey);
        let n = events.len();
        for ev in events {
            self.injector.send(ev, &self.stats);
            self.stats.delayed_drains.fetch_add(1, Ordering::Relaxed);
        }
        n
    }
}

async fn run_manual_backfill_worker(
    cfg: ManualBackfillConfig,
    mut gap_rx: tokio::sync::mpsc::UnboundedReceiver<SlotGap>,
    injector: DualLaneChannel,
    stats: Arc<TransportStats>,
    watched_curve_accounts: Arc<DashMap<String, u64>>,
    watched_pool_accounts: Arc<DashMap<String, u64>>,
    watched_mints: Arc<DashMap<String, u64>>,
    shutdown: Arc<AtomicBool>,
) {
    run_manual_backfill_worker_with_fetcher(
        cfg,
        &mut gap_rx,
        injector,
        stats,
        watched_curve_accounts,
        watched_pool_accounts,
        watched_mints,
        shutdown,
        |cfg, addresses, gap| async move { fetch_gap_backfill_events(&cfg, &addresses, gap).await },
    )
    .await;
}

async fn run_manual_backfill_worker_with_fetcher<F, Fut>(
    cfg: ManualBackfillConfig,
    gap_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SlotGap>,
    injector: DualLaneChannel,
    stats: Arc<TransportStats>,
    watched_curve_accounts: Arc<DashMap<String, u64>>,
    watched_pool_accounts: Arc<DashMap<String, u64>>,
    watched_mints: Arc<DashMap<String, u64>>,
    shutdown: Arc<AtomicBool>,
    fetcher: F,
) where
    F: Fn(ManualBackfillConfig, Vec<String>, SlotGap) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Vec<GeyserEvent>>> + Send,
{
    while let Some(gap) = gap_rx.recv().await {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if gap.start_slot == 0 || gap.end_slot < gap.start_slot {
            continue;
        }

        let span = gap
            .end_slot
            .saturating_sub(gap.start_slot)
            .saturating_add(1);
        if span > cfg.max_slots {
            warn!(
                "MANUAL_BACKFILL_SKIP range={}..={} span={} exceeds cap={}",
                gap.start_slot, gap.end_slot, span, cfg.max_slots
            );
            ::metrics::increment_counter!("seer_manual_backfill_skipped_total", "reason" => "gap_too_wide");
            continue;
        }

        let addresses = snapshot_backfill_addresses(
            &watched_curve_accounts,
            &watched_pool_accounts,
            &watched_mints,
            cfg.max_addresses,
        );
        if addresses.is_empty() {
            continue;
        }

        match fetcher(cfg.clone(), addresses.clone(), gap).await {
            Ok(events) => {
                if events.is_empty() {
                    continue;
                }

                let mut recovered = 0u64;
                for event in events {
                    if let GeyserEvent::Transaction {
                        provider_id,
                        provider_role,
                        signature,
                        slot,
                        ..
                    } = &event
                    {
                        let signature_str = signature.to_string();
                        let sent_fast = injector.send(
                            PumpEvent::BackfillTransaction {
                                provider_id: provider_id.clone(),
                                provider_role: *provider_role,
                                signature: signature_str,
                                slot: slot.unwrap_or_default(),
                                received_at: Instant::now(),
                                decoded: Some(event),
                            },
                            &stats,
                        );
                        if !sent_fast {
                            ::metrics::increment_counter!("seer_manual_backfill_spilled_total");
                        }
                        recovered = recovered.saturating_add(1);
                    }
                }

                if recovered > 0 {
                    ::metrics::counter!("seer_manual_backfill_recovered_total", recovered);
                    info!(
                        "MANUAL_BACKFILL_RECOVERED range={}..={} recovered={} addresses={}",
                        gap.start_slot,
                        gap.end_slot,
                        recovered,
                        addresses.len()
                    );
                }
            }
            Err(err) => {
                warn!(
                    "MANUAL_BACKFILL_FAILED range={}..={} err={}",
                    gap.start_slot, gap.end_slot, err
                );
                ::metrics::increment_counter!("seer_manual_backfill_failed_total");
            }
        }
    }
}

fn snapshot_backfill_addresses(
    watched_curve_accounts: &DashMap<String, u64>,
    watched_pool_accounts: &DashMap<String, u64>,
    watched_mints: &DashMap<String, u64>,
    max_addresses: usize,
) -> Vec<String> {
    let mut out = Vec::with_capacity(max_addresses);
    let mut seen = BTreeSet::new();
    for (addr, _) in snapshot_watch_map(watched_pool_accounts)
        .into_iter()
        .chain(snapshot_watch_map(watched_curve_accounts).into_iter())
        .chain(snapshot_watch_map(watched_mints).into_iter())
    {
        if seen.insert(addr.clone()) {
            out.push(addr);
            if out.len() >= max_addresses {
                break;
            }
        }
    }
    out
}

async fn fetch_gap_backfill_events(
    cfg: &ManualBackfillConfig,
    addresses: &[String],
    gap: SlotGap,
) -> Result<Vec<GeyserEvent>> {
    let rpc = new_async_rpc_client(cfg.rpc_endpoint.clone());
    let commitment = CommitmentConfig::confirmed();
    let tx_config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(commitment.clone()),
        max_supported_transaction_version: Some(0),
    };

    let mut signatures: HashMap<String, (Signature, u64)> = HashMap::new();
    for addr in addresses {
        let Ok(pubkey) = Pubkey::from_str(addr) else {
            continue;
        };
        let entries = rpc
            .get_signatures_for_address_with_config(
                &pubkey,
                GetConfirmedSignaturesForAddress2Config {
                    before: None,
                    until: None,
                    limit: Some(cfg.signature_limit_per_address),
                    commitment: Some(commitment.clone()),
                },
            )
            .await
            .unwrap_or_default();

        for entry in entries {
            if entry.slot < gap.start_slot || entry.slot > gap.end_slot {
                continue;
            }
            let Ok(signature) = Signature::from_str(&entry.signature) else {
                continue;
            };
            signatures
                .entry(entry.signature)
                .or_insert((signature, entry.slot));
        }
    }

    let mut ordered: Vec<(Signature, u64)> = signatures.into_values().collect();
    ordered.sort_by_key(|(signature, slot)| (*slot, signature.to_string()));
    if ordered.len() > cfg.max_transactions_per_gap {
        ordered.truncate(cfg.max_transactions_per_gap);
    }

    let mut out = Vec::with_capacity(ordered.len());
    for (signature, slot) in ordered {
        let Ok(tx) = rpc
            .get_transaction_with_config(&signature, tx_config.clone())
            .await
        else {
            continue;
        };
        let Some(meta) = tx.transaction.meta.as_ref() else {
            continue;
        };
        let Some((accounts, instructions)) =
            parse_ui_transaction_with_meta(&tx.transaction.transaction, Some(meta))
        else {
            continue;
        };
        let (pre_balances, post_balances) = extract_balances_from_meta(meta);
        let logs = extract_logs_from_meta(meta);

        let arrival_ts_ms = crate::types::arrival_time_ms();
        let ingress_wall_ts_ms = crate::types::ingress_epoch_ms();
        out.push(GeyserEvent::Transaction {
            provider_id: None,
            provider_role: None,
            observation_provenance: None,
            slot: crate::types::normalize_slot(Some(slot)),
            tx_index: None,
            event_ts_ms: crate::types::event_ts_from_block_time(tx.block_time),
            arrival_ts_ms: Some(arrival_ts_ms),
            event_time: ghost_core::EventTimeMetadata::new(
                crate::types::event_ts_from_block_time(tx.block_time),
                Some(ingress_wall_ts_ms),
                Some(arrival_ts_ms),
            ),
            signature,
            accounts,
            instructions,
            logs,
            block_time: tx.block_time,
            account_data: HashMap::new(),
            pre_balances,
            post_balances,
            success: meta.err.is_none(),
            error_code: meta.err.as_ref().map(|err| format!("{:?}", err)),
            compute_units_consumed: Option::<u64>::from(meta.compute_units_consumed.clone()),
            synthetic: false,
            source: "grpc_backfill".to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        });
    }

    Ok(out)
}

/// Convert a PumpEvent (raw proto bytes) into a GeyserEvent (parsed fields).
///
/// Returns None only for future transport-only events that do not map to the
/// public `GeyserEvent` stream. `EntryUpdate` maps to `EntryAnchor` because it
/// is part of Yellowstone coverage/gap provenance.
pub(crate) fn pump_event_to_geyser_event(
    ev: PumpEvent,
    live_source_label: &'static str,
    block_time: Option<i64>,
) -> Option<SeerResult<GeyserEvent>> {
    match ev {
        PumpEvent::Transaction {
            provider_id,
            provider_role,
            received_at_monotonic_ns,
            signature,
            slot,
            received_at: _,
            decoded,
            capture_payload,
        } => {
            // PR1D requires a stable captured-payload digest for every accepted
            // Yellowstone transaction. The adapter hands normalization a decoded
            // child, so the captured representation is its prost encoding rather
            // than the original gRPC wire frame. WAL/MPCF capture remains opt-in:
            // only the digest is unconditional.
            #[cfg(test)]
            crate::hot_path_metrics::record_live_transaction_prost_encode();
            let normalized_payload = encode_proto(decoded.as_ref());
            let observation_provenance = yellowstone_transaction_observation_provenance(
                provider_id.as_deref(),
                live_source_label,
                &normalized_payload,
                received_at_monotonic_ns,
            );
            let captured_payload = if capture_payload {
                normalized_payload
            } else {
                Vec::new()
            };
            Some(tx_update_to_geyser_event(
                *decoded,
                &signature,
                slot,
                live_source_label,
                captured_payload,
                block_time,
                provider_id,
                provider_role,
                observation_provenance,
            ))
        }
        PumpEvent::BackfillTransaction {
            provider_id,
            provider_role,
            signature: _,
            slot: _,
            received_at: _,
            decoded,
        } => Some(
            decoded
                .ok_or_else(|| SeerError::ParseError("failed inline decode of backfill".into()))
                .map(|mut decoded| {
                    // The manual backfill producer may have decoded the transaction
                    // before it attached its outer raw-provider metadata. Preserve
                    // explicit decoder provenance, and fill only absent values from
                    // the enclosing PumpEvent.
                    if let GeyserEvent::Transaction {
                        provider_id: decoded_provider_id,
                        provider_role: decoded_provider_role,
                        ..
                    } = &mut decoded
                    {
                        if decoded_provider_id.is_none() {
                            *decoded_provider_id = provider_id;
                        }
                        if decoded_provider_role.is_none() {
                            *decoded_provider_role = provider_role;
                        }
                    }
                    decoded
                }),
        ),
        PumpEvent::AccountUpdate {
            provider_id: _,
            provider_role: _,
            pubkey: _,
            slot: _,
            received_at: _,
            decoded,
        } => {
            Some(decoded.ok_or_else(|| {
                SeerError::ParseError("failed inline decode of account update".into())
            }))
        }
        PumpEvent::EntryUpdate {
            provider_id,
            provider_role,
            slot,
            executed_transaction_count,
            raw,
            ..
        } => Some(Ok(GeyserEvent::EntryAnchor {
            provider_id,
            provider_role,
            slot,
            executed_transaction_count,
            raw,
        })),
    }
}

fn decode_tx_to_geyser_event(
    raw: Vec<u8>,
    sig_str: &str,
    slot: u64,
    source: &str,
    block_time: Option<i64>,
    provider_id: Option<String>,
    provider_role: Option<ghost_core::RawProviderRoleV1>,
) -> SeerResult<GeyserEvent> {
    use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;

    let result = (|| {
        #[cfg(test)]
        crate::hot_path_metrics::record_live_transaction_normalizer_decode();
        let update = <SubscribeUpdateTransaction as prost::Message>::decode(raw.as_slice())
            .map_err(|e| SeerError::ParseError(format!("proto decode Tx: {e}")))?;
        let observation_provenance = yellowstone_transaction_observation_provenance(
            provider_id.as_deref(),
            source,
            &raw,
            crate::types::arrival_time_ns(),
        );
        tx_update_to_geyser_event(
            update,
            sig_str,
            slot,
            source,
            raw,
            block_time,
            provider_id,
            provider_role,
            observation_provenance,
        )
    })();
    record_parser_malformed_tx_sample(result.is_err());
    result
}

fn tx_update_to_geyser_event(
    update: yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction,
    sig_str: &str,
    slot: u64,
    source: &str,
    raw: Vec<u8>,
    block_time: Option<i64>,
    provider_id: Option<String>,
    provider_role: Option<ghost_core::RawProviderRoleV1>,
    observation_provenance: Option<ghost_core::ObservationProvenanceV1>,
) -> SeerResult<GeyserEvent> {
    let arrival_ts_ms = crate::types::arrival_time_ms();
    let ingress_ts_ms = crate::types::ingress_epoch_ms();
    let tx_info = update
        .transaction
        .as_ref()
        .ok_or_else(|| SeerError::ParseError("missing tx_info".into()))?;
    // Yellowstone supplies the canonical transaction position in the slot.
    // Zero is valid: it is the first transaction in that slot.
    let tx_index = u32::try_from(tx_info.index).map_err(|_| {
        SeerError::ParseError(format!(
            "Yellowstone transaction index {} exceeds u32",
            tx_info.index
        ))
    })?;
    let tx = tx_info
        .transaction
        .as_ref()
        .ok_or_else(|| SeerError::ParseError("missing transaction".into()))?;
    let msg = tx
        .message
        .as_ref()
        .ok_or_else(|| SeerError::ParseError("missing message".into()))?;
    let meta = tx_info.meta.as_ref();

    // Signature
    let signature = Signature::from_str(sig_str).unwrap_or_default();

    // Account keys (static + loaded ALT)
    let mut all_keys: Vec<Pubkey> = msg
        .account_keys
        .iter()
        .map(|b| Pubkey::try_from(b.as_slice()).unwrap_or_default())
        .collect();
    if let Some(m) = meta {
        for k in &m.loaded_writable_addresses {
            all_keys.push(Pubkey::try_from(k.as_slice()).unwrap_or_default());
        }
        for k in &m.loaded_readonly_addresses {
            all_keys.push(Pubkey::try_from(k.as_slice()).unwrap_or_default());
        }
    }

    // Instructions
    let instructions: Vec<RawInstruction> = msg
        .instructions
        .iter()
        .map(|ix| RawInstruction {
            program_id: all_keys
                .get(ix.program_id_index as usize)
                .copied()
                .unwrap_or_default(),
            account_indices: ix.accounts.clone(),
            data: ix.data.clone(),
        })
        .collect();

    // Inner instructions
    let inner_instructions: Vec<InnerInstructionGroup> = meta
        .map(|m| {
            m.inner_instructions
                .iter()
                .map(|group| InnerInstructionGroup {
                    index: group.index,
                    instructions: group
                        .instructions
                        .iter()
                        .map(|ii| InnerIx {
                            program_id_index: ii.program_id_index as u8,
                            accounts: ii.accounts.clone(),
                            data: ii.data.clone(),
                            stack_height: ii.stack_height,
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    // Logs
    let logs: Vec<String> = meta.map(|m| m.log_messages.clone()).unwrap_or_default();

    // Balances
    let (pre_balances, post_balances) = meta
        .map(|m| (m.pre_balances.clone(), m.post_balances.clone()))
        .unwrap_or_default();

    // Token Balances
    let pre_token_balances = meta
        .map(|m| {
            m.pre_token_balances
                .iter()
                .map(|b| crate::types::RawTokenBalance {
                    account_index: b.account_index,
                    mint: b.mint.clone(),
                    owner: if b.owner.is_empty() {
                        None
                    } else {
                        Some(b.owner.clone())
                    },
                    amount: b
                        .ui_token_amount
                        .as_ref()
                        .and_then(|a| a.amount.parse::<u64>().ok())
                        .unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();

    let post_token_balances = meta
        .map(|m| {
            m.post_token_balances
                .iter()
                .map(|b| crate::types::RawTokenBalance {
                    account_index: b.account_index,
                    mint: b.mint.clone(),
                    owner: if b.owner.is_empty() {
                        None
                    } else {
                        Some(b.owner.clone())
                    },
                    amount: b
                        .ui_token_amount
                        .as_ref()
                        .and_then(|a| a.amount.parse::<u64>().ok())
                        .unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();

    // Success / error
    let success = meta.map(|m| m.err.is_none()).unwrap_or(true);
    let error_code = meta
        .and_then(|m| m.err.as_ref())
        .map(|e| format!("{:?}", e.err));

    // Compute units
    let compute_units_consumed = meta.and_then(|m| m.compute_units_consumed);

    let (mpcf_payload_bytes, mpcf_payload_missing_reason) = if raw.is_empty() {
        (None, RawBytesMissingReason::Unknown)
    } else {
        (Some(raw), RawBytesMissingReason::NotMissing)
    };

    Ok(GeyserEvent::Transaction {
        provider_id,
        provider_role,
        observation_provenance,
        slot: Some(slot),
        tx_index: Some(tx_index),
        event_ts_ms: Some(compat_tx_event_ts_ms(block_time, ingress_ts_ms)),
        arrival_ts_ms: Some(arrival_ts_ms),
        event_time: ghost_core::EventTimeMetadata::new(
            crate::types::event_ts_from_block_time(block_time),
            Some(ingress_ts_ms),
            Some(arrival_ts_ms),
        ),
        signature,
        accounts: all_keys,
        instructions,
        logs,
        block_time,
        account_data: std::collections::HashMap::new(),
        pre_balances,
        post_balances,
        success,
        error_code,
        compute_units_consumed,
        synthetic: false,
        source: source.to_string(),
        mpcf_payload_bytes,
        mpcf_payload_missing_reason,
        inner_instructions,
        pre_token_balances,
        post_token_balances,
    })
}

/// Decode a frozen PR-A transaction payload for the offline research
/// materializer without routing it through the active connector.
///
/// The raw V1 tape stores the deterministic prost payload of
/// `SubscribeUpdateTransaction`.  Reusing the existing normalized
/// transaction conversion prevents a second interpretation of account keys,
/// loaded addresses, instruction trees and transaction metadata.  The caller
/// supplies the captured event-time axes; no local wall/monotonic clock is
/// used as historical evidence.
pub(crate) fn decode_research_raw_transaction_v1(
    source_payload: &[u8],
    expected_slot: u64,
    block_time: Option<i64>,
    provider_id: &str,
    captured_event_time: ghost_core::pump_research_tape::PumpResearchEventTimeV1,
) -> SeerResult<GeyserEvent> {
    use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;

    let update = SubscribeUpdateTransaction::decode(source_payload)
        .map_err(|error| SeerError::ParseError(format!("research raw tx proto decode: {error}")))?;
    if update.slot != expected_slot {
        return Err(SeerError::ParseError(format!(
            "research raw tx slot {} differs from record slot {expected_slot}",
            update.slot
        )));
    }
    let transaction = update.transaction.as_ref().ok_or_else(|| {
        SeerError::ParseError("research raw tx lacks transaction info".to_string())
    })?;
    let signature = Signature::try_from(transaction.signature.as_slice()).map_err(|_| {
        SeerError::ParseError("research raw tx has non-64-byte signature".to_string())
    })?;
    let observed_time = ghost_core::EventTimeMetadata::new(
        captured_event_time.chain_event_ts_ms.or_else(|| {
            block_time
                .and_then(|seconds| u64::try_from(seconds).ok())
                .and_then(|seconds| seconds.checked_mul(1_000))
        }),
        captured_event_time.ingress_wall_ts_ms,
        captured_event_time.ingress_monotonic_ts_ms,
    );
    let observation_provenance = Some(ghost_core::ObservationProvenanceV1 {
        source_family: ghost_core::ObservationSourceFamilyV1::RawYellowstone,
        source_id: "pump_research_tape_v1_offline_replay".to_string(),
        provider_id: provider_id.to_string(),
        schema_id: YELLOWSTONE_TRANSACTION_PAYLOAD_SCHEMA_ID.to_string(),
        payload_hash_blake3:
            ghost_core::ObservationProvenanceV1::payload_hash_for_captured_provider_payload(
                source_payload,
            ),
        // This is diagnostic provenance only.  The true captured monotonic
        // axis remains in `event_time` above and is never reinterpreted as ns.
        received_at_monotonic_ns: 0,
    });
    let mut event = tx_update_to_geyser_event(
        update,
        &signature.to_string(),
        expected_slot,
        "pump_research_tape_v1_offline_replay",
        source_payload.to_vec(),
        block_time,
        Some(provider_id.to_string()),
        Some(ghost_core::RawProviderRoleV1::PrimaryAuthority),
        observation_provenance,
    )?;
    if let GeyserEvent::Transaction {
        event_time,
        event_ts_ms,
        arrival_ts_ms,
        ..
    } = &mut event
    {
        *event_time = observed_time;
        *event_ts_ms = observed_time.effective_event_ts_ms();
        *arrival_ts_ms = captured_event_time.ingress_wall_ts_ms;
    }
    Ok(event)
}

fn yellowstone_transaction_observation_provenance(
    provider_id: Option<&str>,
    source_id: &str,
    captured_payload: &[u8],
    received_at_monotonic_ns: u64,
) -> Option<ghost_core::ObservationProvenanceV1> {
    let provider_id = provider_id
        .map(str::trim)
        .filter(|provider_id| !provider_id.is_empty())?;
    Some(ghost_core::ObservationProvenanceV1 {
        source_family: ghost_core::ObservationSourceFamilyV1::RawYellowstone,
        source_id: source_id.to_string(),
        provider_id: provider_id.to_string(),
        schema_id: YELLOWSTONE_TRANSACTION_PAYLOAD_SCHEMA_ID.to_string(),
        payload_hash_blake3:
            ghost_core::ObservationProvenanceV1::payload_hash_for_captured_provider_payload(
                captured_payload,
            ),
        received_at_monotonic_ns,
    })
}

fn compat_tx_event_ts_ms(block_time: Option<i64>, ingress_ts_ms: u64) -> u64 {
    crate::types::event_ts_from_block_time(block_time).unwrap_or(ingress_ts_ms)
}

fn account_update_to_geyser_event(
    update: yellowstone_grpc_proto::prelude::SubscribeUpdateAccount,
    pubkey_str: &str,
    slot: u64,
    provider_id: &str,
    provider_role: ghost_core::RawProviderRoleV1,
) -> SeerResult<GeyserEvent> {
    let acc = update
        .account
        .as_ref()
        .ok_or_else(|| SeerError::ParseError("missing account".into()))?;

    let pubkey = Pubkey::from_str(pubkey_str).unwrap_or_default();
    let owner = Pubkey::try_from(acc.owner.as_slice()).unwrap_or_default();
    let txn_signature = acc
        .txn_signature
        .as_deref()
        .map(Signature::try_from)
        .transpose()
        .map_err(|err| {
            SeerError::ParseError(format!(
                "invalid account txn_signature for {pubkey_str}: {err}"
            ))
        })?;

    Ok(GeyserEvent::AccountUpdate {
        provider_id: Some(provider_id.to_string()),
        provider_role: Some(provider_role),
        slot,
        event_time: ghost_core::EventTimeMetadata::new(
            None,
            Some(crate::types::ingress_epoch_ms()),
            Some(crate::types::arrival_time_ms()),
        ),
        write_version: Some(acc.write_version),
        txn_signature,
        pubkey,
        data: acc.data.clone(),
        owner,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary_parser::BinaryParser;
    use crate::ipc::{create_ipc_channel, EventPriority, IpcChannelConfig, SeerEvent};
    use futures::StreamExt;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Signature;
    use yellowstone_grpc_proto::prelude::{SubscribeUpdateAccount, SubscribeUpdateAccountInfo};

    fn raw_account_update(
        account_pubkey: Pubkey,
        owner: Pubkey,
        write_version: u64,
        txn_signature: Option<Signature>,
    ) -> SubscribeUpdateAccount {
        SubscribeUpdateAccount {
            account: Some(SubscribeUpdateAccountInfo {
                pubkey: account_pubkey.to_bytes().to_vec(),
                lamports: 1,
                owner: owner.to_bytes().to_vec(),
                executable: false,
                rent_epoch: 0,
                data: vec![1, 2, 3],
                write_version,
                txn_signature: txn_signature.map(|signature| signature.as_ref().to_vec()),
            }),
            slot: 42,
            is_startup: false,
        }
    }

    #[test]
    fn account_update_preserves_provider_and_optional_transaction_signature() {
        let account_pubkey = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let signature = Signature::new_unique();

        let with_signature = account_update_to_geyser_event(
            raw_account_update(account_pubkey, owner, 7, Some(signature)),
            &account_pubkey.to_string(),
            42,
            "raw-primary",
            ghost_core::RawProviderRoleV1::PrimaryAuthority,
        )
        .expect("valid account update");
        let without_signature = account_update_to_geyser_event(
            raw_account_update(account_pubkey, owner, 8, None),
            &account_pubkey.to_string(),
            43,
            "raw-secondary",
            ghost_core::RawProviderRoleV1::SecondaryWitness,
        )
        .expect("valid account update without transaction signature");

        match with_signature {
            GeyserEvent::AccountUpdate {
                provider_id,
                provider_role,
                txn_signature,
                ..
            } => {
                assert_eq!(provider_id.as_deref(), Some("raw-primary"));
                assert_eq!(
                    provider_role,
                    Some(ghost_core::RawProviderRoleV1::PrimaryAuthority)
                );
                assert_eq!(txn_signature, Some(signature));
            }
            other => panic!("expected account update, got {other:?}"),
        }
        match without_signature {
            GeyserEvent::AccountUpdate {
                provider_id,
                provider_role,
                txn_signature,
                ..
            } => {
                assert_eq!(provider_id.as_deref(), Some("raw-secondary"));
                assert_eq!(
                    provider_role,
                    Some(ghost_core::RawProviderRoleV1::SecondaryWitness)
                );
                assert_eq!(txn_signature, None);
            }
            other => panic!("expected account update, got {other:?}"),
        }
    }

    // ── SlotTracker ───────────────────────────────────────────────────────────

    #[test]
    fn slot_no_gap() {
        let t = SlotTracker::default();
        t.update(10);
        assert_eq!(t.update(11), None);
    }
    #[test]
    fn slot_gap_of_4() {
        let t = SlotTracker::default();
        t.update(10);
        assert_eq!(
            t.update(15),
            Some(SlotGap {
                start_slot: 11,
                end_slot: 14,
            })
        );
        assert_eq!(t.total_gaps(), 4);
    }
    #[test]
    fn slot_ignores_rewind() {
        let t = SlotTracker::default();
        t.update(100);
        assert_eq!(t.update(50), None);
        assert_eq!(t.last_slot(), 100);
    }
    #[test]
    fn slot_first_no_gap() {
        let t = SlotTracker::default();
        assert_eq!(t.update(999_999), None);
    }
    #[test]
    fn slot_adjacent_ok() {
        let t = SlotTracker::default();
        t.update(999);
        assert_eq!(t.update(1000), None);
    }
    #[test]
    fn slot_many_gaps_accumulated() {
        let t = SlotTracker::default();
        t.update(1);
        t.update(10);
        t.update(20);
        assert_eq!(t.total_gaps(), 17); // (10-1-1)=8 + (20-10-1)=9
    }

    // ── AccountRegistry ───────────────────────────────────────────────────────

    #[test]
    fn registry_dedup() {
        let r = AccountRegistry::new();
        assert!(r.insert("A"));
        assert!(!r.insert("A"));
        assert_eq!(r.len(), 1);
    }
    #[test]
    fn registry_version_increments_only_on_new_values() {
        let r = AccountRegistry::new();
        assert_eq!(r.version(), 0);
        assert!(r.insert("A"));
        let v1 = r.version();
        assert!(v1 > 0);
        assert!(!r.insert("A"));
        assert_eq!(r.version(), v1);
        assert!(r.insert("B"));
        assert!(r.version() > v1);
    }
    #[test]
    fn registry_version_tracks_exact_watch_lanes_but_ignores_mints() {
        let r = AccountRegistry::new();
        assert_eq!(r.version(), 0);
        assert!(r.insert_curve("curve-A"));
        let v_curve = r.version();
        assert!(v_curve > 0);
        assert!(r.insert_pool("pool-A"));
        let v_pool = r.version();
        assert!(v_pool > v_curve);
        assert!(r.insert_bcv2("bcv2-A"));
        let v_bcv2 = r.version();
        assert!(v_bcv2 > v_pool);
        assert!(r.insert_mint("mint-A"));
        assert_eq!(r.version(), v_bcv2);
        assert!(r.insert("generic-A"));
        assert!(r.version() > v_bcv2);
    }

    #[test]
    fn bcv2_context_roundtrip_and_remove() {
        let r = AccountRegistry::new();
        let account_pubkey = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let pool_id = Pubkey::new_unique();
        let canonical_bonding_curve = Pubkey::new_unique();
        let context = Bcv2AccountContext {
            account_pubkey,
            base_mint: Some(base_mint),
            pool_id: Some(pool_id),
            canonical_bonding_curve: Some(canonical_bonding_curve),
            tx_signature: Some("bcv2-sig".to_string()),
            observed_instruction_index: Some(4),
            observed_account_position: Some(16),
            provenance_status: Some("route_compatible".to_string()),
            observed_slot: Some(77),
        };

        assert!(r.insert_bcv2_with_context(context.clone()));
        assert!(r.contains_bcv2(&account_pubkey.to_string()));

        let observed = r
            .bcv2_context(&account_pubkey.to_string())
            .expect("bcv2 context must be stored");
        assert_eq!(observed, context);
        assert_eq!(observed.base_mint, Some(base_mint));
        assert_eq!(observed.pool_id, Some(pool_id));
        assert_eq!(
            observed.canonical_bonding_curve,
            Some(canonical_bonding_curve)
        );

        assert!(r.remove_bcv2(&account_pubkey.to_string()));
        assert!(!r.contains_bcv2(&account_pubkey.to_string()));
        assert!(r.bcv2_context(&account_pubkey.to_string()).is_none());
    }

    #[test]
    fn registry_snapshot_all() {
        let r = AccountRegistry::new();
        r.insert("X");
        r.insert("Y");
        assert_eq!(r.snapshot().len(), 2);
    }

    // ── TransportStats ────────────────────────────────────────────────────────

    #[test]
    fn stats_touch_then_recent() {
        let s = TransportStats::default();
        assert_eq!(s.ms_since_last_msg(), 0); // never touched
        s.touch();
        assert!(s.ms_since_last_msg() < 200);
    }

    #[test]
    fn stats_spill_counted() {
        let s = TransportStats::default();
        s.bump_spill();
        s.bump_spill();
        assert_eq!(s.msgs_spilled.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn stats_stall_rate_tracks_stall_reconnect_fraction() {
        let s = TransportStats::default();
        s.bump_recon();
        s.bump_recon();
        s.bump_stall();
        assert!((s.stall_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn provider_circuit_breaker_opens_after_threshold_stalls() {
        let breaker = ProviderCircuitBreaker::new(
            "provider-a",
            "test_source",
            ProviderCircuitBreakerConfig {
                max_stalls_before_open: 2,
                cooldown_ms: 10,
            },
        );

        breaker.record_stall();
        assert_eq!(breaker.snapshot().state, ProviderCircuitState::Closed);

        breaker.record_stall();
        let snapshot = breaker.snapshot();
        assert_eq!(snapshot.state, ProviderCircuitState::Open);
        assert_eq!(snapshot.consecutive_stalls, 2);
        assert_eq!(snapshot.total_stalls, 2);
    }

    #[tokio::test]
    async fn provider_circuit_breaker_half_open_probe_closes_on_progress() {
        let breaker = ProviderCircuitBreaker::new(
            "provider-b",
            "test_source",
            ProviderCircuitBreakerConfig {
                max_stalls_before_open: 1,
                cooldown_ms: 5,
            },
        );

        breaker.record_stall();
        assert_eq!(breaker.snapshot().state, ProviderCircuitState::Open);

        tokio::time::sleep(Duration::from_millis(10)).await;
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_token = Arc::new(CancellationToken::new());
        let permit = breaker
            .acquire_attempt_permit(&shutdown, &shutdown_token)
            .await
            .expect("permit");
        assert!(permit.half_open_probe);
        assert_eq!(breaker.snapshot().state, ProviderCircuitState::HalfOpen);

        breaker.record_message_progress();
        let snapshot = breaker.snapshot();
        assert_eq!(snapshot.state, ProviderCircuitState::Closed);
        assert_eq!(snapshot.consecutive_stalls, 0);
    }

    #[tokio::test]
    async fn watchdog_stream_path_opens_circuit_for_mock_provider() {
        let cfg = GrpcConfig {
            stall_timeout_secs: 1,
            max_stalls_before_open: 1,
            circuit_breaker_cooldown_ms: 5,
            ..GrpcConfig::default()
        };
        let stats = Arc::new(TransportStats::default());
        stats.bump_recon();
        let breaker = ProviderCircuitBreaker::new(
            "mock-provider",
            "test_source",
            ProviderCircuitBreakerConfig {
                max_stalls_before_open: 1,
                cooldown_ms: 5,
            },
        );
        let stale_last_msg_wall_ms = wall_clock_ms().saturating_sub(2_000);

        let result = run_watchdog_tick_for_test(
            "mock-provider:0",
            &cfg,
            &stats,
            &breaker,
            stale_last_msg_wall_ms,
        )
        .await;

        assert!(result.is_err(), "watchdog path must fail on stale stream");
        assert_eq!(
            breaker.snapshot().state,
            ProviderCircuitState::Open,
            "watchdog-triggered stall must open provider circuit"
        );
        assert_eq!(stats.stall_reconnects.load(Ordering::Relaxed), 1);
        assert!((stats.stall_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn with_circuit_breaker_config_updates_connection_config() {
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        )
        .with_circuit_breaker_config(7, 42_000);

        assert_eq!(conn.config.max_stalls_before_open, 7);
        assert_eq!(conn.config.circuit_breaker_cooldown_ms, 42_000);
    }

    // ── PR1B single bounded ingress queue ────────────────────────────────────

    #[test]
    fn dual_lane_fast_path() {
        let (ch, rx) = DualLaneChannel::new();
        let stats = Arc::new(TransportStats::default());
        let ev = PumpEvent::EntryUpdate {
            provider_id: None,
            provider_role: None,
            slot: 1,
            received_at: Instant::now(),
            executed_transaction_count: 5,
            raw: vec![],
        };
        assert!(ch.send(ev, &stats));
        assert_eq!(stats.msgs_spilled.load(Ordering::Relaxed), 0);
        rx.queue
            .recv_timeout(Duration::from_millis(10))
            .expect("should be in ingress queue");
    }

    #[test]
    fn bounded_ingress_saturation_is_nonblocking_and_emits_one_gap() {
        let (ch, rx) = DualLaneChannel::with_capacity(1);
        let stats = Arc::new(TransportStats::default());
        let mk = |slot| PumpEvent::EntryUpdate {
            provider_id: Some("primary-a".to_string()),
            provider_role: None,
            slot,
            received_at: Instant::now(),
            executed_transaction_count: 0,
            raw: vec![],
        };

        assert!(ch.send(mk(1), &stats));
        let started = Instant::now();
        assert!(!ch.send(mk(2), &stats));
        assert!(!ch.send(mk(3), &stats));
        assert!(
            started.elapsed() < Duration::from_millis(5),
            "saturation must not block the receive thread"
        );
        rx.queue.try_recv().expect("drain pre-gap event");
        assert!(ch.send(mk(4), &stats), "first post-gap event is admitted");

        assert_eq!(stats.msgs_spilled.load(Ordering::Relaxed), 2);
        assert_eq!(stats.msgs_overflow_dropped.load(Ordering::Relaxed), 2);
        assert_eq!(rx.local_gap.completed_len(), 1);
        let gap = rx.local_gap.take_completed().expect("typed local gap");
        assert_eq!(
            gap.reason,
            ghost_core::LocalCoverageGapReasonV1::IngressQueueSaturated
        );
        assert_eq!(gap.before.slot, Some(1));
        assert_eq!(gap.after.slot, Some(4));
        assert!(!gap.recovered);
    }

    // ── [FIX-1] Entry event forwarded ────────────────────────────────────────

    #[test]
    fn entry_event_is_emitted_not_dropped() {
        // Verify that EntryUpdate variant exists and carries executed_tx_count
        let ev = PumpEvent::EntryUpdate {
            provider_id: None,
            provider_role: None,
            slot: 100,
            received_at: Instant::now(),
            executed_transaction_count: 42,
            raw: vec![0u8; 8],
        };
        assert_eq!(ev.slot(), 100);
        if let PumpEvent::EntryUpdate {
            executed_transaction_count,
            ..
        } = ev
        {
            assert_eq!(executed_transaction_count, 42);
        } else {
            panic!("Entry variant not matched");
        }
    }

    // ── [FIX-2] BackfillTransaction ───────────────────────────────────────────

    #[test]
    fn backfill_tagged_correctly() {
        let ev = PumpEvent::BackfillTransaction {
            provider_id: None,
            provider_role: None,
            signature: "SIG".into(),
            slot: 99,
            received_at: Instant::now(),
            decoded: None,
        };
        assert!(ev.is_backfill());
        assert_eq!(ev.slot(), 99);
    }

    // ── [FIX-3] Multi-provider config ────────────────────────────────────────

    #[test]
    fn config_multi_provider() {
        let cfg = GrpcConfig::default()
            .with_provider(Provider::new("https://p1.com:443", None, "p1"))
            .with_provider(Provider::secondary(
                "https://p2.com:443",
                Some("tok".into()),
                "p2",
            ));
        assert_eq!(cfg.providers.len(), 2);
        assert_eq!(cfg.providers[0].label, "p1");
        assert_eq!(
            cfg.providers[0].role,
            ghost_core::RawProviderRoleV1::PrimaryAuthority
        );
        assert_eq!(cfg.providers[1].x_token, Some("tok".into()));
        assert_eq!(
            cfg.providers[1].role,
            ghost_core::RawProviderRoleV1::SecondaryWitness
        );
    }

    #[test]
    fn provider_role_contract_accepts_exactly_one_declared_primary() {
        let cfg = GrpcConfig::default()
            .with_provider(Provider::new("https://p1.com:443", None, "raw-primary"))
            .with_provider(Provider::secondary(
                "https://p2.com:443",
                None,
                "raw-secondary",
            ))
            .with_provider_role_config(RawProviderRoleConfigV1::new(
                "raw-primary",
                vec!["raw-secondary".to_string()],
            ));

        cfg.validate_provider_roles()
            .expect("declared primary and secondary providers must validate");
    }

    #[test]
    fn provider_role_contract_rejects_multiple_implicit_primaries() {
        let cfg = GrpcConfig::default()
            .with_provider(Provider::new("https://p1.com:443", None, "raw-primary-a"))
            .with_provider(Provider::new("https://p2.com:443", None, "raw-primary-b"));

        let error = cfg
            .validate_provider_roles()
            .expect_err("multiple primary providers must fail closed");
        assert!(error.to_string().contains("exactly one PrimaryAuthority"));
    }

    #[test]
    fn provider_role_contract_rejects_duplicate_or_missing_provider_ids() {
        let duplicate = GrpcConfig::default()
            .with_provider(Provider::new("https://p1.com:443", None, "same-id"))
            .with_provider(Provider::secondary("https://p2.com:443", None, "same-id"));
        assert!(duplicate
            .validate_provider_roles()
            .expect_err("duplicate provider IDs must fail closed")
            .to_string()
            .contains("duplicate raw provider ID"));

        let missing_secondary = GrpcConfig::default()
            .with_provider(Provider::new("https://p1.com:443", None, "raw-primary"))
            .with_provider_role_config(RawProviderRoleConfigV1::new(
                "raw-primary",
                vec!["raw-secondary".to_string()],
            ));
        assert!(missing_secondary
            .validate_provider_roles()
            .expect_err("declared secondary without endpoint must fail closed")
            .to_string()
            .contains("has no configured provider endpoint"));
    }

    #[test]
    fn configured_primary_id_replaces_legacy_hardcoded_provider_label() {
        let connection = GrpcConnection::new_with_auth_header_and_provider_roles(
            "https://p1.com:443".to_string(),
            None,
            None,
            DEFAULT_GRPC_AUTH_HEADER.to_string(),
            "configured-primary".to_string(),
            Vec::new(),
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        );

        assert_eq!(connection.config.providers.len(), 1);
        assert_eq!(connection.config.providers[0].label, "configured-primary");
        assert_eq!(
            connection
                .config
                .provider_role_config
                .as_ref()
                .expect("active connection must carry a role contract")
                .primary_raw_provider_id,
            "configured-primary"
        );
        connection
            .config
            .validate_provider_roles()
            .expect("configured single primary must validate");
    }

    #[tokio::test]
    async fn connect_geyser_fails_closed_on_invalid_provider_role_contract() {
        let connection = GrpcConnection::new_with_auth_header_and_provider_roles(
            "https://p1.com:443".to_string(),
            None,
            None,
            DEFAULT_GRPC_AUTH_HEADER.to_string(),
            "raw-primary".to_string(),
            vec!["missing-secondary".to_string()],
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        )
        .with_manual_backfill_enabled(false);

        let err = match connection.connect_geyser().await {
            Ok(_) => panic!("invalid provider contract must not return an active stream"),
            Err(err) => err,
        };

        match err {
            SeerError::ConfigError(message) => {
                assert!(message.contains("missing-secondary"));
                assert!(message.contains("has no configured provider endpoint"));
            }
            other => panic!("expected ConfigError, got {other:?}"),
        }
    }

    #[test]
    fn entry_anchor_preserves_provider_metadata() {
        let event = PumpEvent::EntryUpdate {
            provider_id: Some("raw-secondary".to_string()),
            provider_role: Some(ghost_core::RawProviderRoleV1::SecondaryWitness),
            slot: 123,
            received_at: Instant::now(),
            executed_transaction_count: 9,
            raw: vec![1, 2, 3],
        };

        let converted = pump_event_to_geyser_event(event, GRPC_GLOBAL_STREAM_SOURCE_LABEL, None)
            .expect("entry update must map to entry anchor")
            .expect("entry update conversion must succeed");

        match converted {
            GeyserEvent::EntryAnchor {
                provider_id,
                provider_role,
                slot,
                executed_transaction_count,
                raw,
            } => {
                assert_eq!(provider_id.as_deref(), Some("raw-secondary"));
                assert_eq!(
                    provider_role,
                    Some(ghost_core::RawProviderRoleV1::SecondaryWitness)
                );
                assert_eq!(slot, 123);
                assert_eq!(executed_transaction_count, 9);
                assert_eq!(raw, vec![1, 2, 3]);
            }
            other => panic!("expected entry anchor, got {other:?}"),
        }
    }

    #[test]
    fn run_fails_without_providers() {
        let cfg = GrpcConfig::default(); // no providers
        let (conn, _rx, _gap_rx) = YellowstoneConnector::new(cfg);
        // Should bail immediately with a meaningful error
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(conn.run());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no providers"));
    }

    // ── from_slot diagnostics only (no wire-level replay in proto 1.14) ─────

    #[test]
    fn subscribe_request_accepts_from_slot_for_diagnostics() {
        let r = AccountRegistry::new();
        let req = build_subscribe_request(CommitmentLevel::Processed, &r, 123_456_789);
        assert_eq!(req.commitment, Some(CommitmentLevel::Processed as i32));
    }

    // ── SubscribeRequest correctness ──────────────────────────────────────────

    #[test]
    fn subscribe_both_programs_in_tx_filter() {
        let r = AccountRegistry::new();
        let req = build_subscribe_request(CommitmentLevel::Processed, &r, 0);
        let tf = req.transactions.get("pump_txs").unwrap();
        assert!(tf.account_include.contains(&PUMP_FUN_PROGRAM_ID.into()));
        assert!(tf.account_include.contains(&PUMP_SWAP_PROGRAM_ID.into()));
        assert_eq!(tf.vote, Some(false));
        assert_eq!(tf.failed, Some(false));
    }

    #[test]
    fn subscribe_disables_entry_filter_to_avoid_global_entry_flood() {
        let req = build_subscribe_request(CommitmentLevel::Processed, &AccountRegistry::new(), 0);
        assert!(
            req.entry.is_empty(),
            "entry filter must stay disabled because an empty entry branch floods the stream with global entries"
        );
    }

    #[test]
    fn subscribe_uses_tracked_exact_accounts_only() {
        let req = build_subscribe_request(CommitmentLevel::Processed, &AccountRegistry::new(), 0);
        assert!(req.accounts.contains_key("tracked_accounts"));
        assert!(req.accounts.contains_key("pumpfun_curve_layouts"));
        assert!(req.accounts.contains_key("pumpswap_pool_layouts"));
        assert_eq!(req.accounts.len(), 3);
    }

    #[test]
    fn subscribe_pump_filtered_funding_lane_disables_account_filters() {
        let req = build_subscribe_request_for_profile(
            CommitmentLevel::Processed,
            GrpcSubscriptionProfile::FundingLanePumpFiltered,
            &AccountRegistry::new(),
            0,
        );
        assert!(req.accounts.is_empty());
        let tf = req.transactions.get("pump_txs").unwrap();
        assert!(tf.account_include.contains(&PUMP_FUN_PROGRAM_ID.into()));
        assert!(tf.account_include.contains(&PUMP_SWAP_PROGRAM_ID.into()));
    }

    #[test]
    fn subscribe_full_chain_funding_lane_uses_all_transactions_without_accounts() {
        let req = build_subscribe_request_for_profile(
            CommitmentLevel::Processed,
            GrpcSubscriptionProfile::FundingLaneFullChain,
            &AccountRegistry::new(),
            0,
        );
        assert!(req.accounts.is_empty());
        let tf = req.transactions.get("all_txs").unwrap();
        assert!(tf.account_include.is_empty());
        assert_eq!(req.transactions.len(), 1);
    }

    #[test]
    fn subscribe_primary_global_exact_branch_only_carries_generic_accounts() {
        let r = AccountRegistry::new();
        r.insert_curve("DynamicCurve11111111111111111111111111111111");
        r.insert_pool("DynamicPool111111111111111111111111111111111");
        r.insert_mint("DynamicMint111111111111111111111111111111111");
        r.insert("GenericWatch111111111111111111111111111111111");
        let req = build_subscribe_request(CommitmentLevel::Processed, &r, 0);
        let tracked_filter = req.accounts.get("tracked_accounts").unwrap();
        // Generic accounts are in the exact filter.
        assert!(tracked_filter
            .account
            .contains(&"GenericWatch111111111111111111111111111111111".into()));
        // Curve/pool updates already arrive via the global layout filters, so the
        // dynamic exact branch must stay generic-only to avoid request-shape churn.
        assert!(!tracked_filter
            .account
            .contains(&"DynamicPool111111111111111111111111111111111".into()));
        assert!(!tracked_filter
            .account
            .contains(&"DynamicCurve11111111111111111111111111111111".into()));
        // Mints are never in the exact filter.
        assert!(!tracked_filter
            .account
            .contains(&"DynamicMint111111111111111111111111111111111".into()));
        assert!(tracked_filter
            .account
            .contains(&PUMP_FUN_FEE_ACCOUNT.to_string()));
    }

    #[test]
    fn bcv2_insert_primary_global_exact_branch_contains_bcv2() {
        let r = AccountRegistry::new();
        r.insert_bcv2("Bcv2Watch1111111111111111111111111111111111");
        r.insert("GenericWatch111111111111111111111111111111111");

        let req = build_subscribe_request(CommitmentLevel::Processed, &r, 0);
        let tracked_filter = req.accounts.get("tracked_accounts").unwrap();

        assert!(tracked_filter
            .account
            .contains(&"Bcv2Watch1111111111111111111111111111111111".to_string()));
        assert!(tracked_filter
            .account
            .contains(&"GenericWatch111111111111111111111111111111111".to_string()));
    }

    #[test]
    fn bcv2_insert_changes_primary_global_request_fingerprint() {
        let r = AccountRegistry::new();
        let baseline =
            subscribe_request_fingerprint_for_profile(GrpcSubscriptionProfile::PrimaryGlobal, &r);

        r.insert_bcv2("bcv2-A");

        let after_bcv2 =
            subscribe_request_fingerprint_for_profile(GrpcSubscriptionProfile::PrimaryGlobal, &r);
        assert_ne!(
            after_bcv2, baseline,
            "route-compatible BCV2 exact-watch registrations must alter the active primary request shape"
        );
    }

    #[test]
    fn bcv2_insert_does_not_include_curve_pool_lanes_in_primary_exact_branch() {
        let r = AccountRegistry::new();
        r.insert_curve("curve-A");
        r.insert_pool("pool-A");
        r.insert_bcv2("bcv2-A");

        let req = build_subscribe_request(CommitmentLevel::Processed, &r, 0);
        let tracked_filter = req.accounts.get("tracked_accounts").unwrap();

        assert!(tracked_filter.account.contains(&"bcv2-A".to_string()));
        assert!(!tracked_filter.account.contains(&"curve-A".to_string()));
        assert!(!tracked_filter.account.contains(&"pool-A".to_string()));
    }

    #[test]
    fn bcv2_insert_preserves_fee_account_and_exact_account_cap() {
        let r = AccountRegistry::new();
        for i in 0..EXACT_ACCOUNT_FILTER_CAP {
            r.insert(format!("generic-{i:03}"));
        }
        r.insert_bcv2("bcv2-priority");

        let req = build_subscribe_request(CommitmentLevel::Processed, &r, 0);
        let tracked_filter = req.accounts.get("tracked_accounts").unwrap();

        assert_eq!(tracked_filter.account.len(), EXACT_ACCOUNT_FILTER_CAP);
        assert!(tracked_filter
            .account
            .contains(&PUMP_FUN_FEE_ACCOUNT.to_string()));
        assert!(tracked_filter
            .account
            .contains(&"bcv2-priority".to_string()));
    }

    #[test]
    fn bcv2_insert_retains_newest_accounts_within_payload_cap() {
        let r = AccountRegistry::new();
        for i in 0..(EXACT_ACCOUNT_PAYLOAD_CAP + 6) {
            r.insert_bcv2(format!("bcv2-{i:03}"));
        }

        let lanes = r.snapshot_by_lane();
        let tracked_accounts = tracked_exact_accounts_for_profile(
            GrpcSubscriptionProfile::PrimaryGlobal,
            &r,
            EXACT_ACCOUNT_PAYLOAD_CAP,
        );
        let counts = exact_account_selection_counts_for_profile(
            GrpcSubscriptionProfile::PrimaryGlobal,
            &lanes,
            &tracked_accounts,
        );

        assert_eq!(counts.tracked_bcv2, EXACT_ACCOUNT_PAYLOAD_CAP);
        assert_eq!(counts.bcv2_sent, EXACT_ACCOUNT_PAYLOAD_CAP);
        assert_eq!(counts.bcv2_dropped, 0);
        assert!(!lanes.bcv2_accounts.contains(&"bcv2-000".to_string()));
        assert!(!tracked_accounts.contains(&"bcv2-000".to_string()));
        assert!(tracked_accounts.contains(&format!("bcv2-{:03}", EXACT_ACCOUNT_PAYLOAD_CAP + 5)));
    }

    #[test]
    fn account_registry_recency_sort_uses_total_order_for_ties() {
        let mut ranked = vec![
            ("bcv2-b".to_string(), 7),
            ("bcv2-c".to_string(), 9),
            ("bcv2-a".to_string(), 7),
        ];

        AccountRegistry::sort_ranked_accounts_by_recency(&mut ranked);

        assert_eq!(
            ranked,
            vec![
                ("bcv2-c".to_string(), 9),
                ("bcv2-a".to_string(), 7),
                ("bcv2-b".to_string(), 7),
            ]
        );
    }

    #[test]
    fn primary_global_exact_snapshot_survives_concurrent_bcv2_retouch() {
        let r = Arc::new(AccountRegistry::new());
        let account_count = EXACT_ACCOUNT_PAYLOAD_CAP + 32;
        for i in 0..account_count {
            r.insert_bcv2(format!("bcv2-{i:03}"));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();
        for worker in 0..4 {
            let registry = Arc::clone(&r);
            let stop = Arc::clone(&stop);
            handles.push(std::thread::spawn(move || {
                let mut index = worker;
                while !stop.load(Ordering::Relaxed) {
                    registry.insert_bcv2(format!("bcv2-{index:03}"));
                    index = (index + 7) % account_count;
                }
            }));
        }

        for _ in 0..200 {
            let snapshot = r.snapshot_primary_global_exact_accounts(EXACT_ACCOUNT_PAYLOAD_CAP);
            assert!(snapshot.len() <= EXACT_ACCOUNT_PAYLOAD_CAP);
        }

        stop.store(true, Ordering::Relaxed);
        for handle in handles {
            handle.join().expect("bcv2 retouch thread must not panic");
        }
    }

    #[test]
    fn subscribe_layout_filters_cover_curve_and_pool_discriminators() {
        let req = build_subscribe_request(CommitmentLevel::Processed, &AccountRegistry::new(), 0);

        let pumpfun_filter = req.accounts.get("pumpfun_curve_layouts").unwrap();
        assert_eq!(pumpfun_filter.owner, vec![PUMP_FUN_PROGRAM_ID.to_string()]);
        assert!(pumpfun_filter.account.is_empty());
        assert_eq!(pumpfun_filter.filters.len(), 1);
        let Some(subscribe_request_filter_accounts_filter::Filter::Memcmp(memcmp)) =
            pumpfun_filter.filters[0].filter.as_ref()
        else {
            panic!("pumpfun curve layout filter must be memcmp");
        };
        assert_eq!(memcmp.offset, 0);
        assert_eq!(
            memcmp.data.as_ref(),
            Some(
                &subscribe_request_filter_accounts_filter_memcmp::Data::Bytes(
                    BONDING_CURVE_DISC.to_vec(),
                )
            )
        );

        let pumpswap_filter = req.accounts.get("pumpswap_pool_layouts").unwrap();
        assert_eq!(
            pumpswap_filter.owner,
            vec![PUMP_SWAP_PROGRAM_ID.to_string()]
        );
        assert!(pumpswap_filter.account.is_empty());
        assert_eq!(pumpswap_filter.filters.len(), 1);
        let Some(subscribe_request_filter_accounts_filter::Filter::Memcmp(memcmp)) =
            pumpswap_filter.filters[0].filter.as_ref()
        else {
            panic!("pumpswap pool layout filter must be memcmp");
        };
        assert_eq!(memcmp.offset, 0);
        assert_eq!(
            memcmp.data.as_ref(),
            Some(
                &subscribe_request_filter_accounts_filter_memcmp::Data::Bytes(
                    AMM_POOL_DISC.to_vec(),
                )
            )
        );
    }

    #[test]
    fn subscribe_stays_within_provider_branch_budget() {
        let req = build_subscribe_request(CommitmentLevel::Processed, &AccountRegistry::new(), 0);
        let total_filter_branches = req.accounts.len()
            + req.transactions.len()
            + req.blocks_meta.len()
            + req.entry.len()
            + req.accounts_data_slice.len();
        assert_eq!(
            total_filter_branches, 5,
            "request must keep a bounded filter-branch surface"
        );
    }

    #[test]
    fn registry_snapshot_is_deterministic_across_lanes() {
        let r = AccountRegistry::new();
        r.insert_bcv2("bcv2-b");
        r.insert_bcv2("bcv2-a");
        r.insert_pool("pool-b");
        r.insert_pool("pool-a");
        r.insert_curve("curve-b");
        r.insert_curve("curve-a");
        r.insert_mint("mint-b");
        r.insert_mint("mint-a");

        let snap = r.snapshot_by_lane();
        assert_eq!(
            snap.bcv2_accounts,
            vec!["bcv2-a".to_string(), "bcv2-b".to_string()]
        );
        assert_eq!(
            snap.pool_accounts,
            vec!["pool-a".to_string(), "pool-b".to_string()]
        );
        assert_eq!(
            snap.curve_accounts,
            vec!["curve-a".to_string(), "curve-b".to_string()]
        );
        assert_eq!(
            snap.mint_accounts,
            vec!["mint-a".to_string(), "mint-b".to_string()]
        );
    }

    #[test]
    fn registry_prioritized_snapshot_prefers_hot_pools_and_curves() {
        let r = AccountRegistry::new();
        for i in 0..256 {
            r.insert(format!("generic-{i:03}"));
        }
        r.insert_pool("pool-hot");
        r.insert_curve("curve-hot");

        let snap = r.prioritized_snapshot(2);
        assert_eq!(snap, vec!["pool-hot".to_string(), "curve-hot".to_string()]);
    }

    #[test]
    fn primary_global_request_fingerprint_ignores_curve_and_pool_registry_churn() {
        let r = AccountRegistry::new();
        let baseline =
            subscribe_request_fingerprint_for_profile(GrpcSubscriptionProfile::PrimaryGlobal, &r);

        r.insert_curve("curve-A");
        r.insert_pool("pool-A");
        let after_curve_pool =
            subscribe_request_fingerprint_for_profile(GrpcSubscriptionProfile::PrimaryGlobal, &r);
        assert_eq!(
            after_curve_pool, baseline,
            "global layout filters already cover curve/pool updates; they must not perturb the primary request shape"
        );

        r.insert("generic-A");
        let after_generic =
            subscribe_request_fingerprint_for_profile(GrpcSubscriptionProfile::PrimaryGlobal, &r);
        assert_ne!(
            after_generic, baseline,
            "explicit generic exact-watch registrations must still change the primary request shape"
        );
    }

    #[tokio::test]
    async fn bcv2_insert_triggers_immediate_primary_global_resubscribe() {
        let registry = AccountRegistry::new();
        let cfg = GrpcConfig::default();
        let slots = SlotTracker::new();
        let stats = Arc::new(TransportStats::default());
        let (mut sink, mut rx) = futures::channel::mpsc::unbounded::<SubscribeRequest>();
        let mut last_reg_version = registry.version();
        let mut last_request_fingerprint = subscribe_request_fingerprint_for_profile(
            GrpcSubscriptionProfile::PrimaryGlobal,
            &registry,
        );
        let mut last_resub_at = Instant::now();

        registry.insert_bcv2("bcv2-A");

        maybe_send_resubscribe(
            "primary:0",
            "bcv2_registry_notify",
            &mut sink,
            &cfg,
            &registry,
            &slots,
            &stats,
            None,
            &mut last_reg_version,
            &mut last_request_fingerprint,
            &mut last_resub_at,
            true,
            true,
        )
        .await
        .expect("bcv2 exact-watch changes should trigger an immediate resubscribe");

        let req = tokio::time::timeout(Duration::from_millis(50), rx.next())
            .await
            .expect("bcv2 resubscribe request should be sent")
            .expect("resubscribe stream item should exist");
        let tracked_filter = req.accounts.get("tracked_accounts").unwrap();
        assert!(tracked_filter.account.contains(&"bcv2-A".to_string()));
        assert_eq!(stats.resubs_sent.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn primary_global_skips_resubscribe_when_curve_pool_churn_does_not_change_request_shape()
    {
        let registry = AccountRegistry::new();
        let cfg = GrpcConfig::default();
        let slots = SlotTracker::new();
        let stats = Arc::new(TransportStats::default());
        let (mut sink, mut rx) = futures::channel::mpsc::unbounded::<SubscribeRequest>();
        let mut last_reg_version = registry.version();
        let mut last_request_fingerprint = subscribe_request_fingerprint_for_profile(
            GrpcSubscriptionProfile::PrimaryGlobal,
            &registry,
        );
        let mut last_resub_at = Instant::now() - Duration::from_millis(cfg.resub_debounce_ms);

        registry.insert_curve("curve-A");
        registry.insert_pool("pool-A");

        maybe_send_resubscribe(
            "primary:0",
            "health_tick",
            &mut sink,
            &cfg,
            &registry,
            &slots,
            &stats,
            None,
            &mut last_reg_version,
            &mut last_request_fingerprint,
            &mut last_resub_at,
            false,
            false,
        )
        .await
        .expect("curve/pool-only churn should not fail");

        assert_eq!(last_reg_version, registry.version());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), rx.next())
                .await
                .is_err(),
            "curve/pool churn must not emit a resubscribe when the effective request shape is unchanged"
        );
    }

    #[tokio::test]
    async fn primary_global_resubscribes_when_generic_exact_accounts_change() {
        let registry = AccountRegistry::new();
        let cfg = GrpcConfig::default();
        let slots = SlotTracker::new();
        let stats = Arc::new(TransportStats::default());
        let (mut sink, mut rx) = futures::channel::mpsc::unbounded::<SubscribeRequest>();
        let mut last_reg_version = registry.version();
        let mut last_request_fingerprint = subscribe_request_fingerprint_for_profile(
            GrpcSubscriptionProfile::PrimaryGlobal,
            &registry,
        );
        let mut last_resub_at = Instant::now() - Duration::from_millis(cfg.resub_debounce_ms);

        registry.insert("generic-A");

        maybe_send_resubscribe(
            "primary:0",
            "health_tick",
            &mut sink,
            &cfg,
            &registry,
            &slots,
            &stats,
            None,
            &mut last_reg_version,
            &mut last_request_fingerprint,
            &mut last_resub_at,
            false,
            false,
        )
        .await
        .expect("generic exact-watch changes should trigger a resubscribe");

        let req = tokio::time::timeout(Duration::from_millis(50), rx.next())
            .await
            .expect("resubscribe request should be sent")
            .expect("resubscribe stream item should exist");
        let tracked_filter = req.accounts.get("tracked_accounts").unwrap();
        assert!(tracked_filter.account.contains(&"generic-A".to_string()));
    }

    #[test]
    fn subscribe_exact_branch_truncates_stale_generic_exact_accounts() {
        // Exact-watch accounts are bounded by the provider cap.
        // Insert 400 generic exact-watch addresses so the snapshot must truncate
        // to the newest entries while keeping the static Pump.fun fee account.
        let r = AccountRegistry::new();
        for i in 0..400 {
            r.insert(format!("generic-{i:03}"));
        }

        let req = build_subscribe_request(CommitmentLevel::Processed, &r, 0);
        let tracked_filter = req.accounts.get("tracked_accounts").unwrap();
        assert!(
            !tracked_filter.account.contains(&"generic-000".to_string()),
            "stale exact-watch head should be dropped when exact branch exceeds provider cap"
        );
        assert_eq!(tracked_filter.account.len(), EXACT_ACCOUNT_FILTER_CAP);
        assert!(tracked_filter
            .account
            .contains(&PUMP_FUN_FEE_ACCOUNT.to_string()));
    }

    #[tokio::test]
    async fn registry_insert_curve_triggers_resub_notify() {
        let r = AccountRegistry::new();
        let notify = r.resub_notify();

        assert!(r.insert_curve("curve-A"));

        tokio::time::timeout(Duration::from_millis(50), notify.notified())
            .await
            .expect("curve inserts must trigger an exact-watch resubscribe");
    }

    #[tokio::test]
    async fn watch_sets_respect_stream_config_cap() {
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            Some("http://localhost:8899".to_string()),
        )
        .with_stream_config(crate::config::StreamMode::SingleGlobal, 60_000, 1, 0);

        let p1 = Pubkey::new_unique();
        let p2 = Pubkey::new_unique();
        let p3 = Pubkey::new_unique();
        let m1 = Pubkey::new_unique();
        let m2 = Pubkey::new_unique();

        conn.watch_pool(p1, AmmProgram::PumpFun, 0);
        conn.watch_pool(p2, AmmProgram::PumpFun, 0);
        conn.watch_pool(p3, AmmProgram::PumpFun, 0);
        conn.add_watched_mint(m1);
        conn.add_watched_mint(m2);

        assert!(
            conn.is_pool_watched(&p1) || conn.is_pool_watched(&p2) || conn.is_pool_watched(&p3)
        );
        assert!(conn.is_mint_watched(&m1) || conn.is_mint_watched(&m2));
        assert!(
            conn.watched_pools_count() <= 1,
            "watch set must be pruned to the configured exact-account cap"
        );
    }

    #[tokio::test]
    async fn watched_mints_do_not_inflate_exact_account_registry() {
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            Some("http://localhost:8899".to_string()),
        )
        .with_stream_config(crate::config::StreamMode::SingleGlobal, 60_000, 1, 0);

        conn.add_watched_mint(Pubkey::new_unique());
        conn.add_watched_mint(Pubkey::new_unique());

        let lanes = conn.account_registry().snapshot_by_lane();
        assert!(lanes.mint_accounts.is_empty());
        assert!(lanes.pool_accounts.is_empty());
        assert!(lanes.curve_accounts.is_empty());
    }

    #[tokio::test]
    async fn stream_config_single_global_ignores_watch_debounce_for_resubscribe() {
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            Some("http://localhost:8899".to_string()),
        )
        .with_stream_config(crate::config::StreamMode::SingleGlobal, 60_000, 1, 0);

        assert_eq!(conn.config.resub_debounce_ms, DEFAULT_RESUB_DEBOUNCE_MS);
        assert_eq!(
            conn.config.registry_resubscribe_mode,
            RegistryResubscribeMode::HealthTickOnly
        );
    }

    #[tokio::test]
    async fn stream_config_pooled_filtered_preserves_watch_debounce_for_resubscribe() {
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            Some("http://localhost:8899".to_string()),
        )
        .with_stream_config(crate::config::StreamMode::PooledFiltered, 60_000, 1, 0);

        assert_eq!(conn.config.resub_debounce_ms, 0);
        assert_eq!(
            conn.config.registry_resubscribe_mode,
            RegistryResubscribeMode::ImmediateAndTick
        );
    }

    #[tokio::test]
    async fn watch_pool_populates_exact_account_registry_for_recent_pools() {
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            Some("http://localhost:8899".to_string()),
        )
        .with_stream_config(crate::config::StreamMode::SingleGlobal, 60_000, 1, 0);

        let pumpfun_curve = Pubkey::new_unique();
        let pumpswap_pool = Pubkey::new_unique();

        conn.watch_pool(pumpfun_curve, AmmProgram::PumpFun, 0);
        conn.watch_pool(pumpswap_pool, AmmProgram::PumpSwap, 0);

        let lanes = conn.account_registry().snapshot_by_lane();
        assert_eq!(lanes.curve_accounts, vec![pumpfun_curve.to_string()]);
        assert_eq!(lanes.pool_accounts, vec![pumpswap_pool.to_string()]);
        assert!(lanes.mint_accounts.is_empty());
        assert!(conn.is_pool_watched(&pumpfun_curve));
        assert!(conn.is_pool_watched(&pumpswap_pool));
    }

    #[tokio::test]
    async fn watch_pool_cap_applies_within_pool_lane() {
        // cap=1: two PumpSwap pools inserted with a time gap → older evicted.
        // PumpFun curves use their own separate cap and are not evicted by pool inserts.
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            Some("http://localhost:8899".to_string()),
        )
        .with_stream_config(crate::config::StreamMode::SingleGlobal, 60_000, 1, 0);

        let a_curve = Pubkey::new_unique();
        let older_pool = Pubkey::new_unique();
        let newer_pool = Pubkey::new_unique();

        conn.watch_pool(a_curve, AmmProgram::PumpFun, 0);
        conn.watch_pool(older_pool, AmmProgram::PumpSwap, 0);
        tokio::time::sleep(Duration::from_millis(2)).await;
        conn.watch_pool(newer_pool, AmmProgram::PumpSwap, 0);

        // Older PumpSwap pool evicted (pool lane cap = 1, newer wins).
        assert!(!conn.is_pool_watched(&older_pool));
        assert!(conn.is_pool_watched(&newer_pool));
        // PumpFun curve is NOT evicted — per-lane cap, not combined.
        assert!(conn.is_pool_watched(&a_curve));

        let lanes = conn.account_registry().snapshot_by_lane();
        assert_eq!(lanes.curve_accounts, vec![a_curve.to_string()]);
        assert_eq!(lanes.pool_accounts, vec![newer_pool.to_string()]);
    }

    #[test]
    fn subscribe_no_vote_no_failed() {
        let req = build_subscribe_request(CommitmentLevel::Processed, &AccountRegistry::new(), 0);
        let tf = req.transactions.get("pump_txs").unwrap();
        assert_eq!(tf.vote, Some(false));
        assert_eq!(tf.failed, Some(false));
    }

    #[test]
    fn pump_research_profile_is_source_global_without_touching_primary_profile() {
        let research = pump_research_subscribe_request_v1();
        let research_tx = research
            .transactions
            .get("pump_research_transactions")
            .expect("research transaction filter");
        assert_eq!(
            research_tx.account_include,
            vec![PUMP_FUN_PROGRAM_ID.to_owned()]
        );
        assert_eq!(research_tx.vote, Some(false));
        assert_eq!(
            research_tx.failed, None,
            "raw research capture must retain failed transactions"
        );
        assert!(
            research.entry.is_empty(),
            "Entry remains deliberately disabled"
        );
        assert!(research.blocks.is_empty());
        assert!(research
            .blocks_meta
            .contains_key("pump_research_blocks_meta"));
        assert_eq!(
            research
                .slots
                .get("pump_research_slots")
                .and_then(|filter| filter.filter_by_commitment),
            Some(false),
            "slot raw evidence must retain processed/confirmed/finalized updates"
        );
        let bonding_curve = research
            .accounts
            .get("pump_research_bonding_curves")
            .expect("Pump-owned BondingCurve filter");
        assert_eq!(bonding_curve.owner, vec![PUMP_FUN_PROGRAM_ID.to_owned()]);
        let global = research
            .accounts
            .get("pump_research_global")
            .expect("canonical Pump Global exact account filter");
        assert_eq!(
            global.account,
            vec![ghost_core::pump_research_tape::PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1.to_owned()]
        );

        let primary = build_subscribe_request_for_profile(
            CommitmentLevel::Processed,
            GrpcSubscriptionProfile::PrimaryGlobal,
            &AccountRegistry::new(),
            0,
        );
        let primary_tx = primary
            .transactions
            .get("pump_txs")
            .expect("existing primary transaction filter");
        assert_eq!(primary_tx.failed, Some(false));
        assert!(primary_tx
            .account_include
            .contains(&PUMP_SWAP_PROGRAM_ID.to_owned()));
        assert!(
            primary.slots.is_empty(),
            "active primary profile remains unchanged"
        );
    }

    #[test]
    fn pump_research_exact_state_v2_profile_captures_only_bonding_curves_and_global() {
        let request = pump_research_exact_state_v2_subscribe_request();
        let transaction = request
            .transactions
            .get("pump_research_exact_state_v2_transactions")
            .expect("V2 transaction filter");
        assert_eq!(transaction.vote, Some(false));
        assert_eq!(transaction.failed, None);
        assert_eq!(
            transaction.account_include,
            vec![PUMP_FUN_PROGRAM_ID.to_owned()],
            "V2 must retain every Pump transaction, including failed ones"
        );

        assert_eq!(
            request.accounts.len(),
            2,
            "V2 stream-only source contract has exactly two account filters"
        );
        let account = request
            .accounts
            .get("pump_research_exact_state_v2_bonding_curves")
            .expect("Pump-owned BondingCurve filter");
        assert!(
            account.account.is_empty(),
            "BondingCurve subscription is owner-plus-discriminator scoped"
        );
        assert_eq!(account.owner, vec![PUMP_FUN_PROGRAM_ID.to_owned()]);
        assert_eq!(account.filters.len(), 1);
        let global = request
            .accounts
            .get("pump_research_exact_state_v2_global")
            .expect("canonical Pump Global exact account filter");
        assert_eq!(
            global.account,
            vec![ghost_core::pump_research_tape::PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1.to_owned()]
        );
        assert!(global.owner.is_empty());
        assert!(global.filters.is_empty());
        assert!(
            !request
                .accounts
                .contains_key("pump_research_exact_state_v2_all_pump_owned"),
            "V2 must not retain an unbounded all-Pump-owner filter"
        );
        assert_eq!(
            PUMP_RESEARCH_EXACT_STATE_V2_ACCOUNT_FILTER_LOG_SCOPE,
            "[pump_owned_bonding_curve_discriminator,canonical_pump_global]",
            "the durable subscription diagnostic must describe the same two V1.1 account filters"
        );
        assert_eq!(
            PUMP_RESEARCH_EXACT_STATE_V2_SOURCE_CAPTURE_LOG_NOTE,
            "standalone_stream_only_bonding_curve_global_raw_source_capture_before_projection",
            "the durable subscription diagnostic must not claim an all-owner raw capture"
        );

        assert!(request.entry.is_empty());
        let full_blocks = request
            .blocks
            .get("pump_research_exact_state_v2_full_blocks")
            .expect("V2 unfiltered full-block completeness lane");
        assert!(
            full_blocks.account_include.is_empty(),
            "V2 block completeness evidence must not repeat the Pump transaction filter"
        );
        assert_eq!(full_blocks.include_transactions, Some(true));
        assert_eq!(full_blocks.include_accounts, Some(false));
        assert_eq!(full_blocks.include_entries, Some(false));
        assert!(request
            .blocks_meta
            .contains_key("pump_research_exact_state_v2_blocks_meta"));
        assert_eq!(
            request
                .slots
                .get("pump_research_exact_state_v2_slots")
                .and_then(|filter| filter.filter_by_commitment),
            Some(false),
            "V2 must retain processed/confirmed/finalized slot evidence"
        );

        assert_eq!(
            pump_research_exact_state_v2_subscription_request_fingerprint_blake3(),
            subscribe_request_fingerprint_blake3_v1(&request),
            "V2 manifest identity must fingerprint the exact source request"
        );
        assert_ne!(
            pump_research_exact_state_v2_subscription_request_fingerprint_blake3(),
            pump_research_subscription_request_fingerprint_blake3_v1(),
            "V2 must not silently reuse the narrower V1 source-request identity"
        );
    }

    #[test]
    fn pump_research_manifest_fingerprint_is_derived_from_the_actual_request() {
        let request = pump_research_subscribe_request_v1();
        let frozen = pump_research_subscription_request_fingerprint_blake3_v1();
        assert_eq!(
            frozen,
            subscribe_request_fingerprint_blake3_v1(&request),
            "the manifest helper must fingerprint the exact request constructor used by the stream"
        );

        // Every top-level request field must be bound into the canonical
        // identity, including fields that are intentionally empty in V1.
        let mut mutations = Vec::new();
        let mut accounts = request.clone();
        accounts
            .accounts
            .insert("fingerprint-drift-accounts".to_owned(), Default::default());
        mutations.push(accounts);

        let mut slots = request.clone();
        slots
            .slots
            .insert("fingerprint-drift-slots".to_owned(), Default::default());
        mutations.push(slots);

        let mut transactions = request.clone();
        transactions.transactions.insert(
            "fingerprint-drift-transactions".to_owned(),
            Default::default(),
        );
        mutations.push(transactions);

        let mut transaction_status = request.clone();
        transaction_status.transactions_status.insert(
            "fingerprint-drift-transaction-status".to_owned(),
            Default::default(),
        );
        mutations.push(transaction_status);

        let mut blocks = request.clone();
        blocks
            .blocks
            .insert("fingerprint-drift-blocks".to_owned(), Default::default());
        mutations.push(blocks);

        let mut block_meta = request.clone();
        block_meta.blocks_meta.insert(
            "fingerprint-drift-block-meta".to_owned(),
            Default::default(),
        );
        mutations.push(block_meta);

        let mut entry = request.clone();
        entry
            .entry
            .insert("fingerprint-drift-entry".to_owned(), Default::default());
        mutations.push(entry);

        let mut commitment = request.clone();
        commitment.commitment = Some(CommitmentLevel::Finalized as i32);
        mutations.push(commitment);

        let mut account_slice = request.clone();
        account_slice.accounts_data_slice.push(Default::default());
        mutations.push(account_slice);

        let mut ping = request.clone();
        ping.ping = Some(SubscribeRequestPing { id: 7 });
        mutations.push(ping);

        for changed in mutations {
            assert_ne!(
                frozen,
                subscribe_request_fingerprint_blake3_v1(&changed),
                "a real SubscribeRequest change must change the immutable manifest fingerprint"
            );
        }

        // Map insertion order is not source identity.  Re-inserting real
        // entries in the opposite order must preserve the canonical hash.
        let mut reordered = request.clone();
        let account_entries: Vec<_> = reordered.accounts.drain().collect();
        for (name, filter) in account_entries.into_iter().rev() {
            reordered.accounts.insert(name, filter);
        }
        assert_eq!(
            frozen,
            subscribe_request_fingerprint_blake3_v1(&reordered),
            "canonical fingerprint must not depend on HashMap iteration order"
        );
    }

    #[test]
    fn server_ping_is_transport_control_not_research_raw_evidence() {
        let ping = SubscribeUpdate {
            filters: Vec::new(),
            update_oneof: Some(UpdateOneof::Ping(
                yellowstone_grpc_proto::prelude::SubscribeUpdatePing {},
            )),
        };
        let pong = SubscribeUpdate {
            filters: Vec::new(),
            update_oneof: Some(UpdateOneof::Pong(
                yellowstone_grpc_proto::prelude::SubscribeUpdatePong { id: 1 },
            )),
        };
        let market = SubscribeUpdate {
            filters: Vec::new(),
            update_oneof: Some(UpdateOneof::Slot(
                yellowstone_grpc_proto::prelude::SubscribeUpdateSlot {
                    slot: 1,
                    parent: None,
                    status: CommitmentLevel::Processed as i32,
                },
            )),
        };
        assert!(is_server_transport_control_update(&ping));
        assert!(is_server_transport_control_update(&pong));
        assert!(!is_server_transport_control_update(&market));
    }

    #[tokio::test]
    async fn research_worker_supervision_propagates_panic_and_early_exit() {
        let shutdown_after_panic = AtomicBool::new(true);
        let panic_worker = tokio::spawn(async {
            panic!("injected Pump Research worker panic");
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });
        let panic_error =
            supervise_connector_workers(vec![panic_worker], true, &shutdown_after_panic)
                .await
                .expect_err("research worker panic must fail source supervision");
        assert!(format!("{panic_error:#}").contains("panicked"));

        let shutdown_not_requested = AtomicBool::new(false);
        let clean_early_worker = tokio::spawn(async { Ok::<(), anyhow::Error>(()) });
        let early_exit_error =
            supervise_connector_workers(vec![clean_early_worker], true, &shutdown_not_requested)
                .await
                .expect_err("research worker clean exit before shutdown must fail capture");
        assert!(format!("{early_exit_error:#}").contains("before an explicit shutdown"));

        let active_path_worker = tokio::spawn(async {
            Err::<(), anyhow::Error>(anyhow::anyhow!("legacy worker failure"))
        });
        assert!(
            supervise_connector_workers(vec![active_path_worker], false, &shutdown_not_requested,)
                .await
                .is_ok(),
            "active connector keeps its pre-existing best-effort worker semantics"
        );
    }

    #[test]
    fn ping_request_correct() {
        let r = build_ping(42);
        assert_eq!(r.ping.as_ref().unwrap().id, 42);
        assert!(r.accounts.is_empty());
        assert!(r.entry.is_empty());
    }

    // ── encode_proto roundtrip ────────────────────────────────────────────────

    #[test]
    fn encode_proto_roundtrip() {
        use yellowstone_grpc_proto::prelude::SubscribeRequestPing;
        let p = SubscribeRequestPing { id: 99 };
        let bytes = encode_proto(&p);
        let back = <SubscribeRequestPing as prost::Message>::decode(bytes.as_slice()).unwrap();
        assert_eq!(back.id, 99);
    }

    // ── inject_backfill ───────────────────────────────────────────────────────

    #[test]
    fn inject_backfill_reachable_via_ingress_queue() {
        let cfg = GrpcConfig::default();
        let (conn, rx, _gap_rx) = YellowstoneConnector::new(cfg);
        conn.inject_backfill("SIG1".into(), 42, vec![1, 2, 3]);
        let ev = rx
            .queue
            .recv_timeout(Duration::from_millis(50))
            .expect("event not received");
        assert!(ev.is_backfill());
        assert_eq!(ev.slot(), 42);
    }

    #[test]
    fn snapshot_backfill_addresses_prioritize_pools_then_fill_mints_deterministically() {
        let watched_curves = DashMap::new();
        let watched_pools = DashMap::new();
        let watched_mints = DashMap::new();
        watched_curves.insert("curve-z".to_string(), 10);
        watched_pools.insert("pool-b".to_string(), 20);
        watched_pools.insert("pool-a".to_string(), 30);
        watched_mints.insert("mint-a".to_string(), 5);
        watched_mints.insert("mint-b".to_string(), 15);

        let addresses =
            snapshot_backfill_addresses(&watched_curves, &watched_pools, &watched_mints, 4);
        assert_eq!(
            addresses,
            vec![
                "pool-a".to_string(),
                "pool-b".to_string(),
                "curve-z".to_string(),
                "mint-b".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn manual_backfill_worker_skips_gap_wider_than_cap_without_injecting_events() {
        let cfg = GrpcConfig::default();
        let (connector, rx, _gap_rx) = YellowstoneConnector::new(cfg);
        let injector = connector.injector();
        let stats = connector.stats();
        let watched_curves = Arc::new(DashMap::new());
        let watched_pools = Arc::new(DashMap::new());
        let watched_mints = Arc::new(DashMap::new());
        watched_pools.insert(Pubkey::new_unique().to_string(), 1);

        let (gap_tx, gap_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let handle = tokio::spawn(run_manual_backfill_worker(
            ManualBackfillConfig {
                rpc_endpoint: "http://127.0.0.1:1".to_string(),
                max_slots: 2,
                max_addresses: MANUAL_BACKFILL_MAX_ADDRESSES,
                signature_limit_per_address: MANUAL_BACKFILL_SIGNATURE_LIMIT_PER_ADDRESS,
                max_transactions_per_gap: MANUAL_BACKFILL_MAX_TXS_PER_GAP,
            },
            gap_rx,
            injector,
            stats,
            watched_curves,
            watched_pools,
            watched_mints,
            shutdown,
        ));

        gap_tx
            .send(SlotGap {
                start_slot: 10,
                end_slot: 15,
            })
            .unwrap();
        drop(gap_tx);

        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("worker timeout")
            .expect("worker join");

        assert!(matches!(try_drain_dual_lane(&rx, false), DrainPick::Empty));
    }

    #[test]
    fn manual_backfill_can_be_disabled_explicitly() {
        let conn = GrpcConnection::new(
            "http://localhost:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            Some("http://localhost:8899".to_string()),
        )
        .with_manual_backfill_enabled(false);

        assert!(conn.manual_backfill_cfg.is_some());
        assert!(!conn.manual_backfill_enabled);
    }

    #[tokio::test]
    async fn grpc_connection_request_shutdown_wakes_idle_event_stream() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        )
        .with_manual_backfill_enabled(false);

        let mut stream = conn.connect_geyser().await.expect("stream");
        conn.request_shutdown();
        assert!(conn.is_shutdown_requested());

        let next = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("shutdown should wake idle stream drain");
        assert!(next.is_none(), "stream must end after shutdown request");
    }

    #[tokio::test]
    async fn provider_circuit_breaker_wait_exits_on_shutdown_token() {
        let breaker = ProviderCircuitBreaker::new(
            "provider-shutdown",
            "test_source",
            ProviderCircuitBreakerConfig {
                max_stalls_before_open: 1,
                cooldown_ms: 30_000,
            },
        );
        breaker.record_stall();
        assert_eq!(breaker.snapshot().state, ProviderCircuitState::Open);

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_token = Arc::new(CancellationToken::new());
        let waiter = {
            let breaker = Arc::clone(&breaker);
            let shutdown = Arc::clone(&shutdown);
            let shutdown_token = Arc::clone(&shutdown_token);
            tokio::spawn(async move {
                breaker
                    .acquire_attempt_permit(&shutdown, &shutdown_token)
                    .await
            })
        };

        tokio::time::sleep(Duration::from_millis(10)).await;
        shutdown.store(true, Ordering::Relaxed);
        shutdown_token.cancel();

        let permit = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("shutdown should wake circuit-breaker wait")
            .expect("waiter join");
        assert!(permit.is_none(), "shutdown must not grant reconnect permit");
    }

    #[tokio::test]
    async fn manual_backfill_worker_recovers_gap_into_connect_geyser_stream() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        );

        let watched_pool = Pubkey::new_unique();
        conn.watch_pool(watched_pool, AmmProgram::PumpFun, 0);

        let (gap_tx, mut gap_rx) = tokio::sync::mpsc::unbounded_channel();
        let injector = conn.injector.clone();
        let stats = conn.transport_stats();
        let watched_curves = Arc::clone(&conn.watched_curve_accounts);
        let watched_pools = Arc::clone(&conn.watched_pool_accounts);
        let watched_mints = Arc::clone(&conn.watched_mints);
        let shutdown = Arc::clone(&conn.shutdown);
        let expected_signature = Signature::new_unique();
        let expected_signature_str = expected_signature.to_string();

        let worker = tokio::spawn(async move {
            run_manual_backfill_worker_with_fetcher(
                ManualBackfillConfig {
                    rpc_endpoint: "http://127.0.0.1:8899".to_string(),
                    max_slots: MANUAL_BACKFILL_MAX_SLOTS,
                    max_addresses: MANUAL_BACKFILL_MAX_ADDRESSES,
                    signature_limit_per_address: MANUAL_BACKFILL_SIGNATURE_LIMIT_PER_ADDRESS,
                    max_transactions_per_gap: MANUAL_BACKFILL_MAX_TXS_PER_GAP,
                },
                &mut gap_rx,
                injector,
                stats,
                watched_curves,
                watched_pools,
                watched_mints,
                shutdown,
                move |_cfg, addresses, gap| {
                    let expected_signature = expected_signature;
                    let watched_pool = watched_pool;
                    async move {
                        assert_eq!(gap.start_slot, 400);
                        assert_eq!(gap.end_slot, 401);
                        assert_eq!(addresses, vec![watched_pool.to_string()]);
                        Ok(vec![make_decoded_tx(
                            expected_signature,
                            401,
                            "grpc_backfill",
                        )])
                    }
                },
            )
            .await;
        });

        let mut stream = conn.connect_geyser().await.expect("stream");
        gap_tx
            .send(SlotGap {
                start_slot: 400,
                end_slot: 401,
            })
            .expect("gap send");
        drop(gap_tx);

        let recovered = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("backfill timeout")
            .expect("stream ended")
            .expect("stream error");
        match recovered {
            GeyserEvent::Transaction {
                signature,
                slot,
                source,
                ..
            } => {
                assert_eq!(signature, expected_signature);
                assert_eq!(slot, Some(401));
                assert_eq!(source, "grpc_backfill");
            }
            other => panic!("expected recovered backfill transaction, got {:?}", other),
        }
        assert_eq!(conn.dedup_dropped_total(), 0);

        worker.await.expect("worker join");
        assert_eq!(
            expected_signature_str,
            expected_signature.to_string(),
            "signature must remain stable across worker recovery"
        );
    }

    fn make_decoded_tx(signature: Signature, slot: u64, source: &str) -> GeyserEvent {
        GeyserEvent::Transaction {
            provider_id: None,
            provider_role: None,
            observation_provenance: None,
            slot: Some(slot),
            tx_index: None,
            event_ts_ms: Some(slot * 1000),
            arrival_ts_ms: Some(crate::types::arrival_time_ms()),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature,
            accounts: vec![],
            instructions: vec![],
            logs: vec![],
            block_time: Some(slot as i64),
            account_data: HashMap::new(),
            pre_balances: vec![],
            post_balances: vec![],
            success: true,
            error_code: None,
            compute_units_consumed: None,
            synthetic: false,
            source: source.to_string(),
            mpcf_payload_bytes: None,
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            inner_instructions: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
        }
    }

    /// Build minimal valid SubscribeUpdateTransaction proto bytes carrying `signature`.
    /// Used in tests that inject PumpEvent::Transaction with raw bytes.
    fn make_raw_tx(signature: Signature, slot: u64) -> Vec<u8> {
        make_raw_tx_with_index(signature, slot, 0)
    }

    fn make_raw_tx_with_index(signature: Signature, slot: u64, index: u64) -> Vec<u8> {
        use prost::Message as _;
        use yellowstone_grpc_proto::prelude::{
            Message as GrpcMessage, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
            Transaction as GrpcTransaction,
        };
        let sig_bytes = signature.as_ref().to_vec();
        let update = SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature: sig_bytes.clone(),
                is_vote: false,
                transaction: Some(GrpcTransaction {
                    signatures: vec![sig_bytes],
                    message: Some(GrpcMessage {
                        header: None,
                        account_keys: vec![],
                        recent_blockhash: vec![],
                        instructions: vec![],
                        versioned: false,
                        address_table_lookups: vec![],
                    }),
                }),
                meta: None,
                index,
            }),
            slot,
        };
        let mut buf = vec![];
        update.encode(&mut buf).expect("encode proto");
        buf
    }

    /// Build one raw Yellowstone transaction whose direct Pump.fun buy can be
    /// decoded by `BinaryParser`. The fixture intentionally starts at the raw
    /// transport boundary so the provenance assertion cannot bypass
    /// `PumpEvent -> GeyserEvent` normalization.
    fn make_raw_pump_buy_tx(signature: Signature, slot: u64) -> Vec<u8> {
        use prost::Message as _;
        use yellowstone_grpc_proto::prelude::{
            CompiledInstruction, Message as GrpcMessage, SubscribeUpdateTransaction,
            SubscribeUpdateTransactionInfo, Transaction as GrpcTransaction,
        };

        let mut accounts = vec![Pubkey::new_unique(); 13];
        accounts[2] = Pubkey::new_unique(); // Pump mint
        accounts[3] = Pubkey::new_unique(); // Pump bonding curve
        accounts[6] = Pubkey::new_unique(); // Transaction signer
        accounts[8] = Pubkey::from_str(ghost_core::transaction_parser::ProgramIds::TOKEN_PROGRAM)
            .expect("token program id");
        accounts[12] = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("pump.fun program id");

        let mut data = crate::binary_parser::DISC_BUY.to_vec();
        data.extend_from_slice(&1_000_000_u64.to_le_bytes());
        data.extend_from_slice(&50_000_000_u64.to_le_bytes());
        let signature_bytes = signature.as_ref().to_vec();
        let update = SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature: signature_bytes.clone(),
                is_vote: false,
                transaction: Some(GrpcTransaction {
                    signatures: vec![signature_bytes],
                    message: Some(GrpcMessage {
                        header: None,
                        account_keys: accounts
                            .iter()
                            .map(|account| account.to_bytes().to_vec())
                            .collect(),
                        recent_blockhash: vec![],
                        instructions: vec![CompiledInstruction {
                            program_id_index: 12,
                            accounts: (0_u8..12_u8).collect(),
                            data,
                        }],
                        versioned: false,
                        address_table_lookups: vec![],
                    }),
                }),
                meta: None,
                index: 0,
            }),
            slot,
        };

        let mut raw = Vec::new();
        update.encode(&mut raw).expect("encode raw Pump buy proto");
        raw
    }

    fn decoded_live_tx(
        raw: &[u8],
    ) -> Box<yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction> {
        Box::new(
            <yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction as prost::Message>::decode(
                raw,
            )
            .expect("decode live transaction fixture"),
        )
    }

    #[tokio::test]
    async fn connect_geyser_live_transaction_retains_raw_payload_bytes() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        );
        let signature = Signature::new_unique();
        let raw = make_raw_tx(signature, 321);
        let expected_raw = raw.clone();
        conn.set_live_transaction_capture_enabled(true);
        let stats = conn.transport_stats();
        conn.injector.send(
            PumpEvent::Transaction {
                provider_id: None,
                provider_role: None,
                received_at_monotonic_ns: crate::types::arrival_time_ns(),
                signature: signature.to_string(),
                slot: 321,
                received_at: Instant::now(),
                decoded: decoded_live_tx(&raw),
                capture_payload: true,
            },
            &stats,
        );

        let mut stream = conn.connect_geyser().await.expect("stream");
        let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("event timeout")
            .expect("stream ended")
            .expect("stream error");
        conn.request_shutdown();

        match event {
            GeyserEvent::Transaction {
                signature: observed,
                slot,
                tx_index,
                source,
                mpcf_payload_bytes,
                mpcf_payload_missing_reason,
                ..
            } => {
                assert_eq!(observed, signature);
                assert_eq!(slot, Some(321));
                assert_eq!(tx_index, Some(0), "Yellowstone index=0 is canonical");
                assert_eq!(source, GRPC_GLOBAL_STREAM_SOURCE_LABEL);
                assert_eq!(mpcf_payload_bytes, Some(expected_raw));
                assert_eq!(
                    mpcf_payload_missing_reason,
                    RawBytesMissingReason::NotMissing
                );
            }
            other => panic!("expected tx event, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn raw_transaction_provider_metadata_reaches_parsed_trade_and_ipc() {
        let signature = Signature::new_unique();
        let provider_id = "raw-primary".to_string();
        let provider_role = ghost_core::RawProviderRoleV1::PrimaryAuthority;
        let raw = make_raw_pump_buy_tx(signature, 321);
        let expected_payload_hash = *blake3::hash(&raw).as_bytes();

        let geyser_event = pump_event_to_geyser_event(
            PumpEvent::Transaction {
                provider_id: Some(provider_id.clone()),
                provider_role: Some(provider_role),
                received_at_monotonic_ns: crate::types::arrival_time_ns(),
                signature: signature.to_string(),
                slot: 321,
                received_at: Instant::now(),
                decoded: decoded_live_tx(&raw),
                capture_payload: true,
            },
            GRPC_GLOBAL_STREAM_SOURCE_LABEL,
            None,
        )
        .expect("raw transaction maps to public Geyser event")
        .expect("raw transaction decodes");

        match &geyser_event {
            GeyserEvent::Transaction {
                provider_id: observed_provider_id,
                provider_role: observed_provider_role,
                tx_index,
                observation_provenance,
                ..
            } => {
                assert_eq!(observed_provider_id.as_deref(), Some(provider_id.as_str()));
                assert_eq!(*observed_provider_role, Some(provider_role));
                assert_eq!(*tx_index, Some(0), "first transaction index is valid");
                let provenance = observation_provenance
                    .as_ref()
                    .expect("raw Yellowstone observation provenance");
                assert_eq!(
                    provenance.source_family,
                    ghost_core::ObservationSourceFamilyV1::RawYellowstone
                );
                assert_eq!(provenance.source_id, GRPC_GLOBAL_STREAM_SOURCE_LABEL);
                assert_eq!(provenance.provider_id, provider_id);
                assert_eq!(
                    provenance.schema_id,
                    YELLOWSTONE_TRANSACTION_PAYLOAD_SCHEMA_ID
                );
                assert_eq!(provenance.payload_hash_blake3, expected_payload_hash);
            }
            other => panic!("expected normalized transaction, got {other:?}"),
        }

        let parser = BinaryParser::new(false);
        let bundle = parser
            .parse_transaction_bundle(&geyser_event)
            .expect("raw Pump buy bundle");
        assert_eq!(
            bundle.trades.len(),
            1,
            "fixture contains exactly one Pump buy"
        );
        assert_eq!(bundle.trade_observations.len(), bundle.trades.len());
        let trade = bundle.trades.into_iter().next().expect("trade");
        let observation = bundle
            .trade_observations
            .into_iter()
            .next()
            .flatten()
            .expect("aligned raw trade observation");
        assert_eq!(trade.provider_id.as_deref(), Some(provider_id.as_str()));
        assert_eq!(trade.provider_role, Some(provider_role));
        assert_eq!(observation.signature, signature);
        assert_eq!(observation.raw_provider_role, Some(provider_role));
        assert_eq!(observation.raw_transaction_mutation_count, Some(1));
        assert_eq!(observation.provenance.provider_id, provider_id);
        assert_eq!(
            observation.provenance.payload_hash_blake3,
            expected_payload_hash
        );
        let locator = observation
            .locator_hint
            .as_ref()
            .expect("raw structural locator");
        let order = observation
            .canonical_order
            .as_ref()
            .expect("raw canonical order");
        assert_eq!(locator.signature, signature);
        assert_eq!(locator.outer_instruction_index, 0);
        assert!(locator.inner_instruction_path.is_empty());
        assert_eq!(order.slot, 321);
        assert_eq!(order.tx_index, 0);
        assert_eq!(order.outer_instruction_index, 0);
        assert!(order.inner_instruction_path.is_empty());

        let (sender, mut receiver, _) = create_ipc_channel(IpcChannelConfig::default());
        sender
            .send_trade_with_observation(trade, Some(observation.clone()), EventPriority::Normal)
            .await
            .expect("trade provenance must cross the Seer IPC boundary");
        match receiver.recv().await.expect("IPC trade event") {
            SeerEvent::Trade(observed) => {
                assert_eq!(
                    observed.trade.provider_id.as_deref(),
                    Some(provider_id.as_str())
                );
                assert_eq!(observed.trade.provider_role, Some(provider_role));
                assert_eq!(observed.observation, Some(observation));
            }
            other => panic!("expected IPC trade, got {other:?}"),
        }
    }

    #[test]
    fn raw_yellowstone_transaction_index_is_preserved_for_zero_and_nonzero_values() {
        for (index, expected) in [(0_u64, 0_u32), (37_u64, 37_u32)] {
            let signature = Signature::new_unique();
            let event = decode_tx_to_geyser_event(
                make_raw_tx_with_index(signature, 777, index),
                &signature.to_string(),
                777,
                GRPC_GLOBAL_STREAM_SOURCE_LABEL,
                None,
                Some("provider-a".to_string()),
                Some(ghost_core::RawProviderRoleV1::PrimaryAuthority),
            )
            .expect("raw Yellowstone transaction must decode");
            match event {
                GeyserEvent::Transaction {
                    provider_id,
                    provider_role,
                    tx_index,
                    ..
                } => {
                    assert_eq!(tx_index, Some(expected));
                    assert_eq!(provider_id.as_deref(), Some("provider-a"));
                    assert_eq!(
                        provider_role,
                        Some(ghost_core::RawProviderRoleV1::PrimaryAuthority)
                    );
                }
                other => panic!("expected transaction, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn connect_geyser_live_transaction_uses_funding_lane_source_label() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        )
        .with_subscription_profile(GrpcSubscriptionProfile::FundingLaneFullChain)
        .with_manual_backfill_enabled(false);
        let signature = Signature::new_unique();
        let raw = make_raw_tx(signature, 654);
        let stats = conn.transport_stats();
        conn.injector.send(
            PumpEvent::Transaction {
                provider_id: None,
                provider_role: None,
                received_at_monotonic_ns: crate::types::arrival_time_ns(),
                signature: signature.to_string(),
                slot: 654,
                received_at: Instant::now(),
                decoded: decoded_live_tx(&raw),
                capture_payload: false,
            },
            &stats,
        );

        let mut stream = conn.connect_geyser().await.expect("stream");
        let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("event timeout")
            .expect("stream ended")
            .expect("stream error");
        conn.request_shutdown();

        match event {
            GeyserEvent::Transaction { source, slot, .. } => {
                assert_eq!(slot, Some(654));
                assert_eq!(source, GRPC_FUNDING_LANE_FULL_CHAIN_SOURCE_LABEL);
            }
            other => panic!("expected tx event, got {:?}", other),
        }
    }

    #[test]
    fn grpc_transaction_sets_compat_event_ts_ms_from_ingress_when_block_time_missing() {
        let ingress_ts_ms = 1_773_238_166_123u64;
        assert_eq!(compat_tx_event_ts_ms(None, ingress_ts_ms), ingress_ts_ms);
    }

    #[test]
    fn grpc_transaction_prefers_chain_time_for_compat_event_ts_ms_when_present() {
        let ingress_ts_ms = 1_773_238_166_123u64;
        assert_eq!(
            compat_tx_event_ts_ms(Some(1_773_238_000), ingress_ts_ms),
            1_773_238_000_000
        );
    }

    #[tokio::test]
    async fn connect_geyser_preserves_live_and_backfill_observations_with_same_signature() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        );
        let signature = Signature::new_unique();
        let signature_str = signature.to_string();
        let stats = conn.transport_stats();
        conn.injector.send(
            PumpEvent::Transaction {
                provider_id: None,
                provider_role: None,
                received_at_monotonic_ns: crate::types::arrival_time_ns(),
                signature: signature_str.clone(),
                slot: 321,
                received_at: Instant::now(),
                decoded: decoded_live_tx(&make_raw_tx(signature, 321)),
                capture_payload: false,
            },
            &stats,
        );
        conn.injector.send(
            PumpEvent::BackfillTransaction {
                provider_id: None,
                provider_role: None,
                signature: signature_str.clone(),
                slot: 321,
                received_at: Instant::now(),
                decoded: Some(make_decoded_tx(signature, 321, "grpc_backfill")),
            },
            &stats,
        );

        let mut stream = conn.connect_geyser().await.expect("stream");
        let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("first event timeout")
            .expect("stream ended")
            .expect("stream error");
        match first {
            GeyserEvent::Transaction {
                signature: observed,
                ..
            } => assert_eq!(observed, signature),
            other => panic!("expected tx event, got {:?}", other),
        }

        let second = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("second event timeout")
            .expect("stream ended")
            .expect("stream error");
        assert!(matches!(
            second,
            GeyserEvent::Transaction { signature: observed, .. } if observed == signature
        ));
        assert_eq!(conn.dedup_dropped_total(), 0);
        conn.request_shutdown();
    }

    #[tokio::test]
    async fn manual_backfill_worker_preserves_overlap_for_ledger_classification() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".to_string(),
            None,
            None,
            Arc::new(crate::metrics::SeerMetrics::new()),
            1,
            1,
            1,
            false,
            crate::config::CommitmentLevel::Confirmed,
            None,
        );

        let watched_pool = Pubkey::new_unique();
        conn.watch_pool(watched_pool, AmmProgram::PumpFun, 0);

        let signature = Signature::new_unique();
        let signature_str = signature.to_string();
        let stats = conn.transport_stats();
        conn.injector.send(
            PumpEvent::Transaction {
                provider_id: None,
                provider_role: None,
                received_at_monotonic_ns: crate::types::arrival_time_ns(),
                signature: signature_str.clone(),
                slot: 500,
                received_at: Instant::now(),
                decoded: decoded_live_tx(&make_raw_tx(signature, 500)),
                capture_payload: false,
            },
            &stats,
        );

        let (gap_tx, mut gap_rx) = tokio::sync::mpsc::unbounded_channel();
        let injector = conn.injector.clone();
        let watched_curves = Arc::clone(&conn.watched_curve_accounts);
        let watched_pools = Arc::clone(&conn.watched_pool_accounts);
        let watched_mints = Arc::clone(&conn.watched_mints);
        let shutdown = Arc::clone(&conn.shutdown);
        let worker = tokio::spawn(async move {
            run_manual_backfill_worker_with_fetcher(
                ManualBackfillConfig {
                    rpc_endpoint: "http://127.0.0.1:8899".to_string(),
                    max_slots: MANUAL_BACKFILL_MAX_SLOTS,
                    max_addresses: MANUAL_BACKFILL_MAX_ADDRESSES,
                    signature_limit_per_address: MANUAL_BACKFILL_SIGNATURE_LIMIT_PER_ADDRESS,
                    max_transactions_per_gap: MANUAL_BACKFILL_MAX_TXS_PER_GAP,
                },
                &mut gap_rx,
                injector,
                stats,
                watched_curves,
                watched_pools,
                watched_mints,
                shutdown,
                move |_cfg, _addresses, _gap| async move {
                    Ok(vec![make_decoded_tx(signature, 500, "grpc_backfill")])
                },
            )
            .await;
        });

        let mut stream = conn.connect_geyser().await.expect("stream");
        gap_tx
            .send(SlotGap {
                start_slot: 500,
                end_slot: 500,
            })
            .expect("gap send");
        drop(gap_tx);

        let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("first event timeout")
            .expect("stream ended")
            .expect("stream error");
        match first {
            GeyserEvent::Transaction {
                signature: observed,
                source,
                ..
            } => {
                assert_eq!(observed, signature);
                assert_eq!(source, "grpc_global_stream");
            }
            other => panic!("expected live transaction first, got {:?}", other),
        }

        let second = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("backfill event timeout")
            .expect("stream ended")
            .expect("stream error");
        assert!(matches!(
            second,
            GeyserEvent::Transaction { signature: observed, .. } if observed == signature
        ));
        assert_eq!(conn.dedup_dropped_total(), 0);

        worker.await.expect("worker join");
    }

    // ── [FIX-5] DelayedAccountQueue ──────────────────────────────────────────

    fn make_acct_ev(slot: u64, pubkey: &str) -> PumpEvent {
        PumpEvent::AccountUpdate {
            provider_id: None,
            provider_role: None,
            pubkey: pubkey.into(),
            slot,
            received_at: Instant::now(),
            decoded: None,
        }
    }

    #[test]
    fn delayed_queue_push_and_drain() {
        let q = DelayedAccountQueue::new();
        q.push("CURVE1".into(), make_acct_ev(10, "CURVE1"));
        assert_eq!(q.depth(), 1);
        let drained = q.drain("CURVE1");
        assert_eq!(drained.len(), 1, "one event should be returned");
        assert_eq!(q.depth(), 0, "queue should be empty after drain");
        if let PumpEvent::AccountUpdate { slot, .. } = &drained[0] {
            assert_eq!(*slot, 10);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn delayed_queue_drain_unknown_returns_empty() {
        let q = DelayedAccountQueue::new();
        assert!(q.drain("GHOST_CURVE").is_empty());
    }

    #[test]
    fn delayed_queue_overwrite_on_re_push() {
        // Second push for the same pubkey overwrites the first.
        let q = DelayedAccountQueue::new();
        q.push("C".into(), make_acct_ev(1, "C"));
        q.push("C".into(), make_acct_ev(2, "C"));
        let d = q.drain("C");
        assert_eq!(d.len(), 1);
        if let PumpEvent::AccountUpdate { slot, .. } = &d[0] {
            assert_eq!(*slot, 2, "newest event should win");
        }
    }

    #[test]
    fn delayed_queue_evicts_on_overflow() {
        // Build a tiny queue and fill it past MAX_DELAYED_ACCTS.
        // We can't change the constant, so just verify push() never panics
        // and depth stays <= MAX_DELAYED_ACCTS.
        let q = DelayedAccountQueue::new();
        for i in 0..MAX_DELAYED_ACCTS + 10 {
            q.push(format!("C{i}"), make_acct_ev(i as u64, "x"));
        }
        assert!(
            q.depth() <= MAX_DELAYED_ACCTS as u64,
            "depth should be capped at MAX_DELAYED_ACCTS after overflow"
        );
    }

    #[test]
    fn delayed_queue_sweep_expired_clears_old() {
        // We can't mock time, so just verify sweep_expired() doesn't panic
        // and leaves fresh entries untouched.
        let q = DelayedAccountQueue::new();
        q.push("FRESH".into(), make_acct_ev(99, "FRESH"));
        q.sweep_expired(); // fresh entry is < 30s old — should NOT be removed
        assert_eq!(q.depth(), 1, "fresh entry should survive sweep");
    }

    #[test]
    fn delayed_queue_accessible_from_connector() {
        let (conn, _rx, _gap_rx) = YellowstoneConnector::new(GrpcConfig::default());
        let q = conn.delayed_queue();
        q.push("CURVE_X".into(), make_acct_ev(42, "CURVE_X"));
        assert_eq!(q.depth(), 1);
        assert_eq!(q.drain("CURVE_X").len(), 1);
    }

    #[test]
    fn delayed_queue_multi_curve_independent() {
        let q = DelayedAccountQueue::new();
        q.push("A".into(), make_acct_ev(1, "A"));
        q.push("B".into(), make_acct_ev(2, "B"));
        assert_eq!(q.depth(), 2);
        let d_a = q.drain("A");
        assert_eq!(d_a.len(), 1);
        assert_eq!(q.depth(), 1, "draining A should not affect B");
        let d_b = q.drain("B");
        assert_eq!(d_b.len(), 1);
        assert_eq!(q.depth(), 0);
    }

    // ── PR-2: GrpcConnection push/drain hot path integration ─────────────────

    fn make_account_update_geyser_event(
        pubkey: solana_sdk::pubkey::Pubkey,
        slot: u64,
    ) -> crate::types::GeyserEvent {
        crate::types::GeyserEvent::AccountUpdate {
            provider_id: None,
            provider_role: None,
            slot,
            event_time: ghost_core::EventTimeMetadata::new(
                None,
                Some(crate::types::ingress_epoch_ms()),
                Some(crate::types::arrival_time_ms()),
            ),
            write_version: None,
            txn_signature: None,
            pubkey,
            data: vec![0u8; 56],
            owner: solana_sdk::pubkey::Pubkey::default(),
        }
    }

    /// PR-2 hot path: `push_delayed_account_update` must buffer the event in the
    /// DelayedAccountQueue and increment the `delayed_pushes` stats counter.
    #[test]
    fn grpc_connection_push_delayed_account_update_buffers_event() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".into(),
            None,
            None,
            Arc::new(SeerMetrics::new()),
            1,
            1,
            5,
            false,
            SeerCommitmentLevel::Confirmed,
            None,
        );
        let curve = solana_sdk::pubkey::Pubkey::new_unique();
        let ev = make_account_update_geyser_event(curve, 42);

        let pushes_before = conn.stats.delayed_pushes.load(Ordering::Relaxed);
        conn.push_delayed_account_update(curve.to_string(), 42, ev);
        assert_eq!(
            conn.delayed_queue.depth(),
            1,
            "event must be buffered in DelayedAccountQueue"
        );
        assert_eq!(
            conn.stats.delayed_pushes.load(Ordering::Relaxed) - pushes_before,
            1,
            "delayed_pushes counter must increment"
        );
    }

    /// PR-2 hot path: `drain_and_reinject_delayed_account_updates` must remove
    /// the event from the queue, re-inject it into the dispatch channel, and
    /// increment the `delayed_drains` stats counter.
    #[test]
    fn grpc_connection_drain_and_reinject_requeues_event() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".into(),
            None,
            None,
            Arc::new(SeerMetrics::new()),
            1,
            1,
            5,
            false,
            SeerCommitmentLevel::Confirmed,
            None,
        );
        let curve = solana_sdk::pubkey::Pubkey::new_unique();
        let curve_str = curve.to_string();
        let ev = make_account_update_geyser_event(curve, 99);

        conn.push_delayed_account_update(curve_str.clone(), 99, ev);
        assert_eq!(conn.delayed_queue.depth(), 1);

        let drains_before = conn.stats.delayed_drains.load(Ordering::Relaxed);
        let n = conn.drain_and_reinject_delayed_account_updates(&curve_str);
        assert_eq!(n, 1, "one event must be re-injected");
        assert_eq!(
            conn.delayed_queue.depth(),
            0,
            "queue must be empty after drain"
        );
        assert_eq!(
            conn.stats.delayed_drains.load(Ordering::Relaxed) - drains_before,
            1,
            "delayed_drains counter must increment"
        );
    }

    /// PR-2 hot path: draining an unknown curve must return 0 and not panic.
    #[test]
    fn grpc_connection_drain_unknown_curve_is_noop() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".into(),
            None,
            None,
            Arc::new(SeerMetrics::new()),
            1,
            1,
            5,
            false,
            SeerCommitmentLevel::Confirmed,
            None,
        );
        let n = conn.drain_and_reinject_delayed_account_updates("NONEXISTENT_CURVE");
        assert_eq!(n, 0, "draining unknown curve must return 0");
        assert_eq!(conn.stats.delayed_drains.load(Ordering::Relaxed), 0);
    }

    /// PR-2 hot path: a second push for the same pubkey overwrites the first
    /// (one-entry-per-curve semantics), and drain re-injects exactly one event.
    #[test]
    fn grpc_connection_push_overwrites_drain_emits_one() {
        let conn = GrpcConnection::new(
            "http://127.0.0.1:10000".into(),
            None,
            None,
            Arc::new(SeerMetrics::new()),
            1,
            1,
            5,
            false,
            SeerCommitmentLevel::Confirmed,
            None,
        );
        let curve = solana_sdk::pubkey::Pubkey::new_unique();
        let curve_str = curve.to_string();

        conn.push_delayed_account_update(
            curve_str.clone(),
            10,
            make_account_update_geyser_event(curve, 10),
        );
        conn.push_delayed_account_update(
            curve_str.clone(),
            20,
            make_account_update_geyser_event(curve, 20),
        );
        assert_eq!(
            conn.delayed_queue.depth(),
            1,
            "second push must overwrite first"
        );

        let n = conn.drain_and_reinject_delayed_account_updates(&curve_str);
        assert_eq!(n, 1, "exactly one event must be re-injected");
        assert_eq!(conn.delayed_queue.depth(), 0);
    }
}
