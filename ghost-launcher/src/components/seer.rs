//! Seer component wrapper

use crate::config::{redact_endpoint_for_logs, SeerCommitment, SeerComponentConfig};
use crate::events::{
    AccountUpdateEvent, DetectedPool, EventBusSender, FundingTransferObserved, GhostEvent,
};
use anyhow::Result;
use ghost_brain::oracle::{InitPoolEvent, SnapshotEngine};
use ghost_core::health::RuntimeHealth;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::{TimestampQuality, Wal};
use metrics::increment_counter;
use seer::{
    config::{
        ConnectionMode, FilterConfig, FundingLaneMode, PumpPortalConfig, SeerConfig,
        SeerSourceMode, StreamMode, TxFilterStrategy,
    },
    ipc::{create_ipc_channel, BackpressurePolicy, IpcChannelConfig},
    Seer,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
// SystemTime is used transitively via event.detected_at.elapsed()
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};

const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
const PUMPSWAP_PROGRAM_ID_STR: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const SESSION_POOL_TRADE_BUFFER_TTL: Duration = Duration::from_millis(10);
const SESSION_POOL_TRADE_BUFFER_PER_POOL_CAP: usize = 64;
const SESSION_POOL_TRADE_BUFFER_GLOBAL_CAP: usize = 2_048;
const SESSION_ACCOUNT_UPDATE_BUFFER_TTL: Duration = Duration::from_secs(2);
const SESSION_ACCOUNT_UPDATE_BUFFER_PER_KEY_CAP: usize = 8;
const SESSION_ACCOUNT_UPDATE_BUFFER_GLOBAL_CAP: usize = 4_096;
const SESSION_POOL_REGISTRY_FALLBACK_TTL: Duration = Duration::from_secs(30 * 60);
const SESSION_POOL_REGISTRY_FALLBACK_CAP: usize = 16_384;
const SESSION_POOL_BRIDGE_PRUNE_INTERVAL: Duration = Duration::from_millis(250);

fn map_launcher_commitment(commitment: SeerCommitment) -> seer::config::CommitmentLevel {
    match commitment {
        SeerCommitment::Processed => seer::config::CommitmentLevel::Mempool,
        SeerCommitment::Confirmed => seer::config::CommitmentLevel::Confirmed,
        SeerCommitment::Finalized => seer::config::CommitmentLevel::Finalized,
    }
}

fn sanitize_detected_creator(creator: Pubkey) -> String {
    let creator_str = creator.to_string();
    if !creator.is_on_curve()
        || creator == Pubkey::default()
        || creator_str == SYSTEM_PROGRAM_ID
        || creator_str == TOKEN_PROGRAM_ID
        || creator_str == TOKEN_2022_PROGRAM_ID
        || creator_str == COMPUTE_BUDGET_PROGRAM_ID
        || creator_str == ASSOCIATED_TOKEN_PROGRAM_ID
        || creator_str.starts_with("Sysvar")
    {
        "unknown".to_string()
    } else {
        creator_str
    }
}

fn trade_has_forwardable_identity(trade: &seer::types::TradeEvent) -> bool {
    trade.pool_amm_id != Pubkey::default() && trade.mint != Pubkey::default()
}

#[derive(Clone)]
struct BufferedSessionTrade {
    trade: seer::types::TradeEvent,
    buffered_at: Instant,
    dedupe_key: BufferedSessionTradeKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BufferedSessionTradeKey {
    pool: Pubkey,
    signature: String,
    event_ordinal: Option<u32>,
}

impl BufferedSessionTradeKey {
    fn from_trade(trade: &seer::types::TradeEvent) -> Self {
        Self {
            pool: trade.pool_amm_id,
            signature: trade.signature.to_string(),
            event_ordinal: trade.event_ordinal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTradeDecision {
    /// Pool is registered in this session — trade forwarded immediately.
    ForwardNow,
    /// Pool not in this session registry — trade silently discarded.
    SilentDrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionTradeIngressResult {
    pub decision: SessionTradeDecision,
    pub expired_count: usize,
    pub evicted_per_pool: usize,
    pub evicted_global: usize,
}

pub struct SessionTradeFlushResult {
    pub replay_ready: Vec<seer::types::TradeEvent>,
    pub expired_count: usize,
    pub expired_detected_pools: usize,
    pub evicted_detected_pools: usize,
}

#[derive(Clone)]
struct BufferedSessionAccountUpdate {
    update: seer::ipc::DetectedAccountUpdateEvent,
    buffered_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAccountUpdateDecision {
    ForwardNow,
    BufferedUntilPoolDetected,
    SilentDrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionAccountUpdateIngressResult {
    pub decision: SessionAccountUpdateDecision,
    pub expired_count: usize,
    pub expired_detected_keys: usize,
    pub evicted_per_key: usize,
    pub evicted_global: usize,
}

pub struct SessionAccountUpdateFlushResult {
    pub replay_ready: Vec<seer::ipc::DetectedAccountUpdateEvent>,
    pub expired_count: usize,
    pub expired_detected_keys: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionAccountUpdateLivenessResult {
    expired_count: usize,
    expired_detected_keys: usize,
}

pub struct SessionAccountUpdateBridge {
    detected_keys: HashMap<Pubkey, Instant>,
    detected_key_order: VecDeque<Pubkey>,
    pending_updates: HashMap<Pubkey, VecDeque<BufferedSessionAccountUpdate>>,
    pending_total: usize,
    ttl: Duration,
    per_key_cap: usize,
    global_cap: usize,
    detected_key_ttl: Duration,
    detected_key_cap: usize,
}

impl Default for SessionAccountUpdateBridge {
    fn default() -> Self {
        Self::new(
            SESSION_ACCOUNT_UPDATE_BUFFER_TTL,
            SESSION_ACCOUNT_UPDATE_BUFFER_PER_KEY_CAP,
            SESSION_ACCOUNT_UPDATE_BUFFER_GLOBAL_CAP,
            SESSION_POOL_REGISTRY_FALLBACK_TTL,
            SESSION_POOL_REGISTRY_FALLBACK_CAP,
        )
    }
}

impl SessionAccountUpdateBridge {
    pub fn new(
        ttl: Duration,
        per_key_cap: usize,
        global_cap: usize,
        detected_key_ttl: Duration,
        detected_key_cap: usize,
    ) -> Self {
        Self {
            detected_keys: HashMap::new(),
            detected_key_order: VecDeque::new(),
            pending_updates: HashMap::new(),
            pending_total: 0,
            ttl,
            per_key_cap: per_key_cap.max(1),
            global_cap: global_cap.max(1),
            detected_key_ttl,
            detected_key_cap: detected_key_cap.max(1),
        }
    }

    fn from_runtime_config(detected_key_ttl: Duration, detected_key_cap: usize) -> Self {
        Self::new(
            SESSION_ACCOUNT_UPDATE_BUFFER_TTL,
            SESSION_ACCOUNT_UPDATE_BUFFER_PER_KEY_CAP,
            SESSION_ACCOUNT_UPDATE_BUFFER_GLOBAL_CAP,
            detected_key_ttl,
            detected_key_cap,
        )
    }

    pub fn register_detected_pool(
        &mut self,
        candidate: &seer::types::CandidatePool,
        now: Instant,
    ) -> SessionAccountUpdateFlushResult {
        let liveness = self.refresh_detected_keys(
            [
                candidate.pool_amm_id,
                candidate.bonding_curve,
                candidate.base_mint,
            ],
            now,
        );

        let mut replay_ready = Vec::new();
        let mut flush_keys = Vec::new();
        for key in [
            candidate.pool_amm_id,
            candidate.bonding_curve,
            candidate.base_mint,
        ] {
            if key != Pubkey::default() && !flush_keys.contains(&key) {
                flush_keys.push(key);
            }
        }

        for key in flush_keys {
            if let Some(mut queue) = self.pending_updates.remove(&key) {
                while let Some(buffered) = queue.pop_front() {
                    self.pending_total = self.pending_total.saturating_sub(1);
                    if now.duration_since(buffered.buffered_at) <= self.ttl {
                        replay_ready.push(buffered.update);
                    }
                }
            }
        }

        replay_ready.sort_by_key(|update| {
            (
                update.slot,
                update.write_version.unwrap_or(u64::MAX),
                update.sequence_number,
            )
        });

        SessionAccountUpdateFlushResult {
            replay_ready,
            expired_count: liveness.expired_count,
            expired_detected_keys: liveness.expired_detected_keys,
        }
    }

    fn refresh_from_trade(
        &mut self,
        trade: &seer::types::TradeEvent,
        now: Instant,
    ) -> SessionAccountUpdateLivenessResult {
        self.refresh_detected_keys([trade.pool_amm_id, trade.mint], now)
    }

    pub fn ingest_account_update(
        &mut self,
        update: &seer::ipc::DetectedAccountUpdateEvent,
        now: Instant,
    ) -> SessionAccountUpdateIngressResult {
        let (expired_count, expired_detected_keys) = self.prune_expired(now);

        if self.detected_keys.contains_key(&update.bonding_curve)
            || self.detected_keys.contains_key(&update.base_mint)
        {
            self.mark_detected_keys([update.bonding_curve, update.base_mint], now);
            return SessionAccountUpdateIngressResult {
                decision: SessionAccountUpdateDecision::ForwardNow,
                expired_count,
                expired_detected_keys,
                evicted_per_key: 0,
                evicted_global: 0,
            };
        }

        let key = if update.bonding_curve != Pubkey::default() {
            update.bonding_curve
        } else if update.base_mint != Pubkey::default() {
            update.base_mint
        } else {
            return SessionAccountUpdateIngressResult {
                decision: SessionAccountUpdateDecision::SilentDrop,
                expired_count,
                expired_detected_keys,
                evicted_per_key: 0,
                evicted_global: 0,
            };
        };

        let mut evicted_per_key = 0;
        let mut evicted_global = 0;

        while self.pending_total >= self.global_cap {
            if self.evict_oldest_pending_update().is_some() {
                evicted_global += 1;
            } else {
                break;
            }
        }

        let queue = self.pending_updates.entry(key).or_default();
        while queue.len() >= self.per_key_cap {
            if queue.pop_front().is_some() {
                self.pending_total = self.pending_total.saturating_sub(1);
                evicted_per_key += 1;
            } else {
                break;
            }
        }

        queue.push_back(BufferedSessionAccountUpdate {
            update: update.clone(),
            buffered_at: now,
        });
        self.pending_total += 1;

        SessionAccountUpdateIngressResult {
            decision: SessionAccountUpdateDecision::BufferedUntilPoolDetected,
            expired_count,
            expired_detected_keys,
            evicted_per_key,
            evicted_global,
        }
    }

    fn prune_expired(&mut self, now: Instant) -> (usize, usize) {
        let mut expired = 0;
        let mut empty_keys = Vec::new();

        for (key, queue) in self.pending_updates.iter_mut() {
            while matches!(queue.front(), Some(front) if now.duration_since(front.buffered_at) > self.ttl)
            {
                if queue.pop_front().is_some() {
                    self.pending_total = self.pending_total.saturating_sub(1);
                    expired += 1;
                }
            }

            if queue.is_empty() {
                empty_keys.push(*key);
            }
        }

        for key in empty_keys {
            self.pending_updates.remove(&key);
        }

        let mut expired_detected_keys = 0;
        while let Some(key) = self.detected_key_order.front().copied() {
            let is_expired = self
                .detected_keys
                .get(&key)
                .map(|last_seen| now.duration_since(*last_seen) > self.detected_key_ttl)
                .unwrap_or(true);
            if !is_expired {
                break;
            }

            self.detected_key_order.pop_front();
            if self.detected_keys.remove(&key).is_some() {
                expired_detected_keys += 1;
            }
        }

        (expired, expired_detected_keys)
    }

    fn mark_detected_key(&mut self, key: Pubkey, now: Instant) {
        if !self.detected_keys.contains_key(&key) {
            self.detected_key_order.push_back(key);
        }
        self.detected_keys.insert(key, now);
    }

    fn mark_detected_keys<I>(&mut self, keys: I, now: Instant)
    where
        I: IntoIterator<Item = Pubkey>,
    {
        let mut protected_keys = HashSet::new();
        for key in keys {
            if key != Pubkey::default() {
                self.mark_detected_key(key, now);
                protected_keys.insert(key);
            }
        }

        while self.detected_keys.len() > self.detected_key_cap {
            if self.evict_oldest_detected_key(&protected_keys).is_none() {
                break;
            }
        }
    }

    fn refresh_detected_keys<I>(
        &mut self,
        keys: I,
        now: Instant,
    ) -> SessionAccountUpdateLivenessResult
    where
        I: IntoIterator<Item = Pubkey>,
    {
        let (expired_count, expired_detected_keys) = self.prune_expired(now);
        self.mark_detected_keys(keys, now);
        SessionAccountUpdateLivenessResult {
            expired_count,
            expired_detected_keys,
        }
    }

    fn evict_oldest_pending_update(&mut self) -> Option<seer::ipc::DetectedAccountUpdateEvent> {
        let oldest_key = self
            .pending_updates
            .iter()
            .filter_map(|(key, queue)| queue.front().map(|front| (*key, front.buffered_at)))
            .min_by_key(|(_, buffered_at)| *buffered_at)
            .map(|(key, _)| key)?;

        let removed = {
            let queue = self.pending_updates.get_mut(&oldest_key)?;
            let removed = queue.pop_front();
            let emptied = queue.is_empty();
            (removed, emptied)
        };

        if removed.0.is_some() {
            self.pending_total = self.pending_total.saturating_sub(1);
        }
        if removed.1 {
            self.pending_updates.remove(&oldest_key);
        }

        removed.0.map(|buffered| buffered.update)
    }

    fn evict_oldest_detected_key(&mut self, protected_keys: &HashSet<Pubkey>) -> Option<Pubkey> {
        let mut deferred = VecDeque::new();
        let mut evicted = None;

        while let Some(key) = self.detected_key_order.pop_front() {
            if !self.detected_keys.contains_key(&key) {
                continue;
            }
            if protected_keys.contains(&key) {
                deferred.push_back(key);
                continue;
            }

            self.detected_keys.remove(&key);
            evicted = Some(key);
            break;
        }

        while let Some(key) = deferred.pop_front() {
            self.detected_key_order.push_back(key);
        }

        evicted
    }

    #[cfg(test)]
    fn pending_total(&self) -> usize {
        self.pending_total
    }
}

pub struct SessionPoolTradeBridge {
    detected_pools: HashMap<Pubkey, Instant>,
    detected_pool_order: VecDeque<Pubkey>,
    pending_trades: HashMap<Pubkey, VecDeque<BufferedSessionTrade>>,
    pending_trade_keys: HashSet<BufferedSessionTradeKey>,
    pending_total: usize,
    ttl: Duration,
    per_pool_cap: usize,
    global_cap: usize,
    detected_pool_ttl: Duration,
    detected_pool_cap: usize,
}

impl Default for SessionPoolTradeBridge {
    fn default() -> Self {
        Self::new(
            SESSION_POOL_TRADE_BUFFER_TTL,
            SESSION_POOL_TRADE_BUFFER_PER_POOL_CAP,
            SESSION_POOL_TRADE_BUFFER_GLOBAL_CAP,
            SESSION_POOL_REGISTRY_FALLBACK_TTL,
            SESSION_POOL_REGISTRY_FALLBACK_CAP,
        )
    }
}

impl SessionPoolTradeBridge {
    pub fn new(
        ttl: Duration,
        per_pool_cap: usize,
        global_cap: usize,
        detected_pool_ttl: Duration,
        detected_pool_cap: usize,
    ) -> Self {
        Self {
            detected_pools: HashMap::new(),
            detected_pool_order: VecDeque::new(),
            pending_trades: HashMap::new(),
            pending_trade_keys: HashSet::new(),
            pending_total: 0,
            ttl,
            per_pool_cap: per_pool_cap.max(1),
            global_cap: global_cap.max(1),
            detected_pool_ttl,
            detected_pool_cap: detected_pool_cap.max(1),
        }
    }

    fn from_runtime_config(
        pending_ttl: Duration,
        detected_pool_ttl: Duration,
        detected_pool_cap: usize,
    ) -> Self {
        Self::new(
            pending_ttl,
            SESSION_POOL_TRADE_BUFFER_PER_POOL_CAP,
            SESSION_POOL_TRADE_BUFFER_GLOBAL_CAP,
            detected_pool_ttl,
            detected_pool_cap,
        )
    }

    pub fn register_detected_pool(
        &mut self,
        pool: Pubkey,
        now: Instant,
    ) -> SessionTradeFlushResult {
        let (expired_count, expired_detected_pools) = self.prune_expired(now);
        self.mark_detected_pool(pool, now);

        let mut evicted_detected_pools = 0;
        while self.detected_pools.len() > self.detected_pool_cap {
            if self.evict_oldest_detected_pool(pool).is_some() {
                evicted_detected_pools += 1;
            } else {
                break;
            }
        }

        let mut replay_ready = Vec::new();
        if let Some(mut queue) = self.pending_trades.remove(&pool) {
            while let Some(buffered) = queue.pop_front() {
                self.pending_total = self.pending_total.saturating_sub(1);
                self.pending_trade_keys.remove(&buffered.dedupe_key);
                if now.duration_since(buffered.buffered_at) <= self.ttl {
                    replay_ready.push(buffered.trade);
                }
            }
        }

        SessionTradeFlushResult {
            replay_ready,
            expired_count,
            expired_detected_pools,
            evicted_detected_pools,
        }
    }

    pub fn ingest_trade(
        &mut self,
        trade: &seer::types::TradeEvent,
        now: Instant,
    ) -> SessionTradeIngressResult {
        let (expired_count, _) = self.prune_expired(now);
        if self.detected_pools.contains_key(&trade.pool_amm_id) {
            self.mark_detected_pool(trade.pool_amm_id, now);
            return SessionTradeIngressResult {
                decision: SessionTradeDecision::ForwardNow,
                expired_count,
                evicted_per_pool: 0,
                evicted_global: 0,
            };
        }

        if trade.pool_amm_id == Pubkey::default() || trade.mint == Pubkey::default() {
            return SessionTradeIngressResult {
                decision: SessionTradeDecision::SilentDrop,
                expired_count,
                evicted_per_pool: 0,
                evicted_global: 0,
            };
        }
        SessionTradeIngressResult {
            decision: SessionTradeDecision::SilentDrop,
            expired_count,
            evicted_per_pool: 0,
            evicted_global: 0,
        }
    }

    fn prune_expired(&mut self, now: Instant) -> (usize, usize) {
        let mut expired = 0;
        let mut expired_detected_pools = 0;
        let mut empty_pools = Vec::new();

        for (pool, queue) in self.pending_trades.iter_mut() {
            while matches!(queue.front(), Some(front) if now.duration_since(front.buffered_at) > self.ttl)
            {
                if let Some(removed) = queue.pop_front() {
                    self.pending_total = self.pending_total.saturating_sub(1);
                    self.pending_trade_keys.remove(&removed.dedupe_key);
                    expired += 1;
                }
            }

            if queue.is_empty() {
                empty_pools.push(*pool);
            }
        }

        for pool in empty_pools {
            self.pending_trades.remove(&pool);
        }

        while let Some(pool) = self.detected_pool_order.front().copied() {
            let is_expired = self
                .detected_pools
                .get(&pool)
                .map(|last_seen| now.duration_since(*last_seen) > self.detected_pool_ttl)
                .unwrap_or(true);

            if !is_expired {
                break;
            }

            self.detected_pool_order.pop_front();
            if self.detected_pools.remove(&pool).is_some() {
                expired_detected_pools += 1;
            }
        }

        (expired, expired_detected_pools)
    }

    fn evict_oldest_pending_trade(&mut self) -> Option<seer::types::TradeEvent> {
        let oldest_pool = self
            .pending_trades
            .iter()
            .filter_map(|(pool, queue)| queue.front().map(|front| (*pool, front.buffered_at)))
            .min_by_key(|(_, buffered_at)| *buffered_at)
            .map(|(pool, _)| pool)?;

        let removed = {
            let queue = self.pending_trades.get_mut(&oldest_pool)?;
            let removed = queue.pop_front();
            let emptied = queue.is_empty();
            (removed, emptied)
        };

        if let Some(ref removed_trade) = removed.0 {
            self.pending_total = self.pending_total.saturating_sub(1);
            self.pending_trade_keys.remove(&removed_trade.dedupe_key);
        }
        if removed.1 {
            self.pending_trades.remove(&oldest_pool);
        }

        removed.0.map(|buffered| buffered.trade)
    }

    fn mark_detected_pool(&mut self, pool: Pubkey, now: Instant) {
        if !self.detected_pools.contains_key(&pool) {
            self.detected_pool_order.push_back(pool);
        }
        self.detected_pools.insert(pool, now);
    }

    fn evict_oldest_detected_pool(&mut self, protected_pool: Pubkey) -> Option<Pubkey> {
        let mut deferred = VecDeque::new();
        let mut evicted = None;

        while let Some(pool) = self.detected_pool_order.pop_front() {
            if !self.detected_pools.contains_key(&pool) {
                continue;
            }
            if pool == protected_pool {
                deferred.push_back(pool);
                continue;
            }

            self.detected_pools.remove(&pool);
            evicted = Some(pool);
            break;
        }

        while let Some(pool) = deferred.pop_front() {
            self.detected_pool_order.push_back(pool);
        }

        evicted
    }

    #[cfg(test)]
    fn pending_total(&self) -> usize {
        self.pending_total
    }

    #[cfg(test)]
    fn detected_total(&self) -> usize {
        self.detected_pools.len()
    }
}

fn record_session_buffer_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!("seer_bridge_session_pool_expired_total", count as u64);
}

fn record_session_detected_pool_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_pool_registry_expired_total",
        count as u64
    );
}

fn record_session_detected_pool_evicted(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_pool_registry_evicted_total",
        count as u64
    );
}

fn record_session_account_update_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_account_update_expired_total",
        count as u64
    );
}

fn record_session_account_update_detected_key_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!(
        "seer_bridge_session_account_update_registry_expired_total",
        count as u64
    );
}

fn record_session_account_update_evictions(per_key: usize, global: usize) {
    if per_key > 0 {
        ::metrics::counter!(
            "seer_bridge_session_account_update_rejected_total",
            per_key as u64,
            "reason" => "per_key_cap"
        );
    }
    if global > 0 {
        ::metrics::counter!(
            "seer_bridge_session_account_update_rejected_total",
            global as u64,
            "reason" => "global_cap"
        );
    }
}

fn record_session_buffer_evictions(per_pool: usize, global: usize) {
    if per_pool > 0 {
        ::metrics::counter!(
            "seer_bridge_session_pool_rejected_total",
            per_pool as u64,
            "reason" => "per_pool_cap"
        );
    }
    if global > 0 {
        ::metrics::counter!(
            "seer_bridge_session_pool_rejected_total",
            global as u64,
            "reason" => "global_cap"
        );
    }
}

fn session_bridge_prune_interval(ttl: Duration, detected_pool_ttl: Duration) -> Duration {
    let min_window = ttl.min(detected_pool_ttl);
    min_window
        .min(SESSION_POOL_BRIDGE_PRUNE_INTERVAL)
        .max(Duration::from_millis(50))
}

fn detected_pool_from_candidate(
    candidate: &seer::types::CandidatePool,
    detected_ms: u64,
) -> DetectedPool {
    DetectedPool {
        semantic: if candidate.effective_event_ts_ms().is_some() {
            candidate.semantic
        } else {
            candidate
                .semantic
                .with_timestamp_quality(TimestampQuality::WallClock)
        },
        pool_amm_id: candidate.pool_amm_id.to_string(),
        base_mint: candidate.base_mint.to_string(),
        quote_mint: candidate.quote_mint.to_string(),
        amm_program: candidate.amm_program_id.to_string(),
        bonding_curve: candidate.bonding_curve.to_string(),
        creator: sanitize_detected_creator(candidate.creator),
        slot: candidate.slot,
        timestamp_ms: candidate.compat_event_ts_ms().unwrap_or(detected_ms),
        event_time: candidate.event_time,
        detected_wall_ts_ms: Some(detected_ms),
        initial_liquidity_sol: candidate.initial_liquidity_sol,
        signature: candidate.signature.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectionClockSummary {
    compat_event_ts_ms: u64,
    effective_event_ts_ms: Option<u64>,
    chain_event_ts_ms: Option<u64>,
    has_explicit_event_time: bool,
    ingest_latency_ms: u64,
}

fn detection_clock_summary(
    candidate: &seer::types::CandidatePool,
    detected_ms: u64,
) -> DetectionClockSummary {
    let effective_event_ts_ms = candidate.effective_event_ts_ms();
    let compat_event_ts_ms = candidate.compat_event_ts_ms().unwrap_or(detected_ms);
    DetectionClockSummary {
        compat_event_ts_ms,
        effective_event_ts_ms,
        chain_event_ts_ms: candidate.event_time.chain_event_ts_ms,
        has_explicit_event_time: effective_event_ts_ms.is_some(),
        ingest_latency_ms: detected_ms.saturating_sub(effective_event_ts_ms.unwrap_or(detected_ms)),
    }
}

fn process_trade_event_for_session_gate(
    tx: &EventBusSender,
    session_trade_bridge: &mut SessionPoolTradeBridge,
    trade: &seer::types::TradeEvent,
    health: Option<&Arc<RuntimeHealth>>,
    now: Instant,
) -> SessionTradeIngressResult {
    let gating_result = session_trade_bridge.ingest_trade(trade, now);
    record_session_buffer_expired(gating_result.expired_count);
    record_session_buffer_evictions(gating_result.evicted_per_pool, gating_result.evicted_global);

    match gating_result.decision {
        SessionTradeDecision::ForwardNow => {
            emit_pool_transaction_to_event_bus(tx, trade, health, false);
        }
        SessionTradeDecision::SilentDrop => {
            // Pool not yet registered in this session — discarded without event bus emission.
            increment_counter!("seer_bridge_session_pool_silent_drop_total");
        }
    }

    gating_result
}

fn process_pool_detected_event_for_session_gate(
    tx: &EventBusSender,
    session_trade_bridge: &mut SessionPoolTradeBridge,
    candidate: &seer::types::CandidatePool,
    health: Option<&Arc<RuntimeHealth>>,
    now: Instant,
    detected_ms: u64,
) -> SessionTradeFlushResult {
    let detected_pool = detected_pool_from_candidate(candidate, detected_ms);

    info!(
        "Seer: 🚀 Emitting NewPoolDetected: pool_amm_id={}, base_mint={}, slot={:?}, amm_program={}",
        detected_pool.pool_amm_id,
        detected_pool.base_mint,
        detected_pool.slot,
        detected_pool.amm_program
    );

    if let Err(e) = tx.send(GhostEvent::new_pool_detected(detected_pool.clone())) {
        error!("Seer: ❌ Failed to emit NewPoolDetected event: {}", e);
    } else {
        if let Some(health) = health {
            health.mark_bus_event();
        }
        info!(
            "Seer: ✅ Event emitted to Event Bus for new pool: pool={}, receivers={}",
            detected_pool.pool_amm_id,
            tx.receiver_count()
        );
    }

    let flush_result = session_trade_bridge.register_detected_pool(candidate.pool_amm_id, now);
    record_session_buffer_expired(flush_result.expired_count);
    record_session_detected_pool_expired(flush_result.expired_detected_pools);
    record_session_detected_pool_evicted(flush_result.evicted_detected_pools);

    if !flush_result.replay_ready.is_empty() {
        ::metrics::counter!(
            "seer_bridge_session_pool_replayed_total",
            flush_result.replay_ready.len() as u64
        );
        info!(
            "Seer: ♻️ Replaying {} session-buffered trades after PoolDetected for pool={}",
            flush_result.replay_ready.len(),
            candidate.pool_amm_id
        );
        for trade in &flush_result.replay_ready {
            emit_pool_transaction_to_event_bus(tx, trade, health, true);
        }
    }

    flush_result
}

fn emit_pool_transaction_to_event_bus(
    tx: &EventBusSender,
    trade: &seer::types::TradeEvent,
    health: Option<&Arc<RuntimeHealth>>,
    replayed_from_session_buffer: bool,
) {
    let pool_tx = trade_event_to_pool_transaction(trade);

    info!(
        "Seer: 🚀 Emitting PoolTransaction: {} pool={} volume={:.4} SOL replayed_from_session_buffer={}",
        if trade.is_buy { "BUY" } else { "SELL" },
        pool_tx.pool_amm_id,
        pool_tx.volume_sol,
        replayed_from_session_buffer
    );

    if let Err(e) = tx.send(GhostEvent::pool_transaction(pool_tx)) {
        error!("Seer: ❌ Failed to emit PoolTransaction event: {}", e);
    } else {
        if let Some(health) = health {
            health.mark_bus_event();
        }
        info!(
            "Seer: ✅ PoolTransaction ZOSTAŁA PRZEKAZANA DO MAGISTRALI ZDARZEŃ: receivers={} replayed_from_session_buffer={}",
            tx.receiver_count(),
            replayed_from_session_buffer
        );
    }
}

fn emit_account_update_to_event_bus(
    tx: &EventBusSender,
    update: &seer::ipc::DetectedAccountUpdateEvent,
    health: Option<&Arc<RuntimeHealth>>,
    replayed_from_session_buffer: bool,
) {
    tracing::info!(
        base_mint = %update.base_mint,
        bonding_curve = %update.bonding_curve,
        slot = update.slot,
        sol_reserves = update.sol_reserves,
        token_reserves = update.token_reserves,
        complete = update.complete,
        curve_finality = %update.curve_finality.as_str(),
        replayed_from_session_buffer,
        "DIAG_ACCOUNT_UPDATE_RELAY"
    );
    let ghost_event = GhostEvent::AccountUpdate(AccountUpdateEvent {
        semantic: update.semantic,
        base_mint: update.base_mint,
        bonding_curve: update.bonding_curve,
        curve_finality: update.curve_finality,
        sol_reserves: update.sol_reserves,
        token_reserves: update.token_reserves,
        complete: update.complete,
        slot: update.slot,
        write_version: update.write_version,
        replay_origin: update.replay_origin,
        replay_buffer_dwell_ms: update.replay_buffer_dwell_ms,
        detected_at: update.detected_at,
        sequence_number: update.sequence_number,
    });
    if let Err(e) = tx.send(ghost_event) {
        tracing::debug!(
            "Seer: AccountUpdate event not delivered (no receivers or lag): {}",
            e
        );
        return;
    }
    if let Some(health) = health {
        health.mark_bus_event();
    }
}

fn emit_funding_transfer_to_event_bus(
    tx: &EventBusSender,
    funding_event: &seer::ipc::DetectedFundingTransferEvent,
    health: Option<&Arc<RuntimeHealth>>,
) {
    let ghost_event = GhostEvent::funding_transfer_observed(FundingTransferObserved {
        semantic: funding_event.transfer.semantic.clone(),
        slot: funding_event.transfer.slot,
        event_ordinal: funding_event.transfer.event_ordinal,
        outer_instruction_index: funding_event.transfer.outer_instruction_index,
        inner_group_index: funding_event.transfer.inner_group_index,
        cpi_stack_height: funding_event.transfer.cpi_stack_height,
        event_time: funding_event.transfer.event_time.clone(),
        arrival_ts_ms: funding_event.transfer.arrival_ts_ms,
        signature: funding_event.transfer.signature.clone(),
        source_wallet: funding_event.transfer.source_wallet.clone(),
        recipient_wallet: funding_event.transfer.recipient_wallet.clone(),
        lamports: funding_event.transfer.lamports,
        full_chain_coverage: funding_event.transfer.full_chain_coverage,
        provenance: funding_event.transfer.provenance,
        detected_at: funding_event.detected_at.clone(),
        sequence_number: funding_event.sequence_number,
    });
    if let Err(e) = tx.send(ghost_event) {
        tracing::debug!(
            "Seer: FundingTransfer event not delivered (no receivers or lag): {}",
            e
        );
        return;
    }
    if let Some(health) = health {
        health.mark_bus_event();
    }
}

/// Run the Seer component
pub async fn run(
    config: SeerComponentConfig,
    mut shutdown_rx: broadcast::Receiver<()>,
    event_bus_tx: Option<EventBusSender>,
    snapshot_engine: Option<Arc<SnapshotEngine>>,
    shadow_ledger: Option<Arc<ShadowLedger>>,
    wal: Option<Arc<Wal>>,
    paradox_tx: Option<
        tokio::sync::oneshot::Sender<
            tokio::sync::watch::Receiver<seer::paradox_sensor::ParadoxState>,
        >,
    >,
    health: Option<Arc<RuntimeHealth>>,
    authoritative_funding_stream_tx: Option<watch::Sender<bool>>,
    canonical_account_update_relay_enabled: bool,
) -> Result<()> {
    info!("Seer: Initializing component");

    if snapshot_engine.is_some() {
        info!("Seer: 📸 SnapshotEngine integration enabled");
    }

    // Convert launcher config to Seer config

    // Determine source mode first, checking both specific source_mode and legacy connection_mode
    let derived_source_mode = if let Some(mode) = &config.source_mode {
        match mode.to_lowercase().as_str() {
            "grpc" => Some(SeerSourceMode::GeyserGrpc),
            "geyser_grpc" => Some(SeerSourceMode::GeyserGrpc),
            "websocket" | "ws" => Some(SeerSourceMode::GeyserWebSocket),
            "geyser_websocket" => Some(SeerSourceMode::GeyserWebSocket),
            "helius_websocket" => Some(SeerSourceMode::HeliusWebSocket),
            "pump_portal_ws" => Some(SeerSourceMode::PumpPortalWs),
            _ => {
                warn!(
                    "Unknown source_mode '{}', will derive from connection_mode",
                    mode
                );
                None
            }
        }
    } else {
        // Fallback to inferring from connection_mode for backward compatibility
        match config.connection_mode.to_lowercase().as_str() {
            "helius_websocket" => Some(SeerSourceMode::HeliusWebSocket),
            _ => None,
        }
    };
    let funding_lane_mode = match config.funding_lane_mode.to_lowercase().as_str() {
        "disabled" => FundingLaneMode::Disabled,
        "pump_filtered" => FundingLaneMode::PumpFiltered,
        "full_chain" => FundingLaneMode::FullChain,
        other => {
            warn!(
                "Unknown seer funding_lane_mode='{}' — defaulting to disabled",
                other
            );
            FundingLaneMode::Disabled
        }
    };

    let seer_config = SeerConfig {
        connection_mode: match config.connection_mode.to_lowercase().as_str() {
            "websocket" | "ws" | "helius_websocket" => ConnectionMode::WebSocket,
            "grpc" | "g" => ConnectionMode::Grpc,
            _ => ConnectionMode::Grpc,
        },
        source_mode: derived_source_mode,
        geyser_endpoint: config.geyser_endpoint,
        grpc_endpoint: config.grpc_endpoint,
        helius_endpoint: config.helius_endpoint,
        rpc_endpoint: config.rpc_endpoint,
        grpc_manual_backfill_enabled: config.grpc_manual_backfill_enabled,
        grpc_client_id: config.grpc_client_id,
        // Use grpc_x_token if provided, otherwise fall back to grpc_auth_token
        // grpc_x_token is the preferred way to authenticate with Chainstack/Yellowstone
        grpc_auth_token: config.grpc_x_token.or(config.grpc_auth_token),
        max_reconnect_attempts: 10,
        reconnect_delay_secs: 5,
        max_reconnect_delay_secs: 300,
        grpc_max_stalls_before_open: SeerConfig::default_grpc_max_stalls_before_open(),
        grpc_circuit_breaker_cooldown_ms: SeerConfig::default_grpc_circuit_breaker_cooldown_ms(),
        verbose: false,
        filter: FilterConfig {
            enable_pumpfun: config.enable_pumpfun,
            enable_bonkfun: config.enable_bonkfun,
            allowed_quote_mints: Vec::new(),
            min_initial_liquidity_sol: None,
        },
        channel_buffer_size: config.ipc_buffer_size,
        ipc_config: IpcChannelConfig {
            buffer_size: config.ipc_buffer_size,
            backpressure_policy: match config.ipc_backpressure_policy.to_lowercase().as_str() {
                "block" => BackpressurePolicy::Block,
                "dropoldest" | "drop_oldest" => BackpressurePolicy::DropOldest,
                "dropnew" | "drop_new" => BackpressurePolicy::DropNew,
                "dropbypriority" | "drop_by_priority" => BackpressurePolicy::DropByPriority,
                _ => BackpressurePolicy::Block,
            },
            log_drops: true,
            log_overflows: true,
            warning_threshold_percent: 80.0,
        },
        metrics_port: config.metrics_port,
        ultrafast_enter_threshold: 80.0,
        ultrafast_exit_threshold: 50.0,
        commitment: map_launcher_commitment(config.commitment),
        grpc_commitment_fallback_to_websocket: config.grpc_commitment_fallback_to_websocket,
        stream_mode: match config.stream_mode.to_lowercase().as_str() {
            "pooled_filtered" => StreamMode::PooledFiltered,
            _ => StreamMode::SingleGlobal,
        },
        tx_filter_strategy: match config.tx_filter_strategy.to_lowercase().as_str() {
            "all" => TxFilterStrategy::All,
            _ => TxFilterStrategy::PerPool,
        },
        funding_lane_mode,
        watched_pools_ttl_ms: config.watched_pools_ttl_ms,
        watched_pools_cap: config.watched_pools_cap,
        watch_debounce_ms: config.watch_debounce_ms,
        canonical_account_update_relay_enabled,
        pumpportal: PumpPortalConfig {
            ws_url: config.pumpportal.ws_url.clone(),
            max_active_mints: config.pumpportal.max_active_mints,
            subscription_batch_size: config.pumpportal.subscription_batch_size,
            reconnect_base_delay_secs: config.pumpportal.reconnect_base_delay_secs,
            reconnect_max_delay_secs: config.pumpportal.reconnect_max_delay_secs,
            stats_window_secs: config.pumpportal.stats_window_secs,
        },
    };

    info!("Seer: Configuration loaded");
    info!(
        "  Effective source mode: {:?}",
        seer_config.effective_source_mode()
    );
    info!(
        "  gRPC endpoint: {}",
        redact_endpoint_for_logs(&seer_config.grpc_endpoint)
    );
    info!(
        "  RPC endpoint: {}",
        redact_endpoint_for_logs(&seer_config.rpc_endpoint)
    );
    info!(
        "  grpc_manual_backfill_enabled: {}",
        seer_config.grpc_manual_backfill_enabled
    );
    info!(
        "  grpc_commitment_fallback_to_websocket: {}",
        seer_config.grpc_commitment_fallback_to_websocket
    );
    info!("  stream_mode: {:?}", seer_config.stream_mode);
    info!("  tx_filter_strategy: {:?}", seer_config.tx_filter_strategy);
    info!(
        "  funding_lane_mode: {}",
        seer_config.funding_lane_mode.as_str()
    );
    info!("  commitment: {}", seer_config.commitment.as_str());
    info!(
        "  watched_pools: ttl_ms={} cap={} debounce_ms={}",
        seer_config.watched_pools_ttl_ms,
        seer_config.watched_pools_cap,
        seer_config.watch_debounce_ms
    );

    // Log PumpPortal config when in PumpPortal mode
    if matches!(
        seer_config.effective_source_mode(),
        seer::config::SeerSourceMode::PumpPortalWs
    ) {
        info!("  PumpPortal WS URL: {}", seer_config.pumpportal.ws_url);
        info!(
            "  PumpPortal max active mints: {}",
            seer_config.pumpportal.max_active_mints
        );
        info!(
            "  PumpPortal subscription batch size: {}",
            seer_config.pumpportal.subscription_batch_size
        );
    }
    info!(
        "  x-token auth: {}",
        if seer_config.grpc_auth_token.is_some() {
            "ENABLED (will be sent with every streaming message)"
        } else {
            "DISABLED"
        }
    );

    // Create IPC channel for candidate forwarding
    let (ipc_sender, mut ipc_receiver, ipc_metrics) =
        create_ipc_channel(seer_config.ipc_config.clone());

    // Create Seer instance (optionally with ShadowLedger for live curve updates)
    let mut seer_instance = match shadow_ledger {
        Some(ledger) => {
            Seer::new_with_ipc_and_shadow_ledger(seer_config.clone(), ipc_sender, ledger)
        }
        None => Seer::new_with_ipc(seer_config.clone(), ipc_sender),
    };

    // Wire RuntimeHealth into Seer → GrpcConnection for gRPC heartbeats
    if let Some(ref h) = health {
        seer_instance.set_health(Arc::clone(h));
    }

    if let Some(wal) = wal {
        seer_instance = seer_instance.with_wal(wal);
    }

    if let Some(tx) = authoritative_funding_stream_tx {
        if seer_instance.set_authoritative_funding_stream_availability_sender(tx) {
            info!(
                "Seer: FSC authoritative funding availability signal wired (funding_lane_mode={})",
                seer_config.funding_lane_mode.as_str()
            );
        } else {
            info!(
                "Seer: FSC authoritative funding availability remains fail-closed (funding_lane_mode={})",
                seer_config.funding_lane_mode.as_str()
            );
        }
    }

    let seer = Arc::new(seer_instance);

    // Get Paradox Sensor state receiver and send it back to caller if requested
    if let Some(paradox_rx) = seer.paradox_state_receiver() {
        info!("Seer: 🔮 Paradox Sensor state receiver available for HFT detection");
        if let Some(tx) = paradox_tx {
            let _ = tx.send(paradox_rx);
            info!("Seer: 🔮 Paradox Sensor state sent to OracleRuntime");
        }
    } else {
        warn!("Seer: 🔮 Paradox Sensor state receiver is None - HFT detection disabled");
    }

    // Start Seer event loop
    let seer_handle = {
        let seer = Arc::clone(&seer);
        tokio::spawn(async move {
            loop {
                info!("Seer: Starting event processing loop");
                match Arc::clone(&seer).run().await {
                    Ok(()) => {
                        info!("Seer: Event loop ended normally");
                        break;
                    }
                    Err(e) => {
                        error!(
                            "Seer: Error in event loop: {}. Restarting in 10 seconds...",
                            e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    }
                }
            }
        })
    };

    // Process IPC events and emit to event bus
    let health_ipc = health.clone();
    let ipc_handle = tokio::spawn(async move {
        let detected_pool_ttl = Duration::from_millis(seer_config.watched_pools_ttl_ms.max(1));
        let detected_pool_cap = seer_config.watched_pools_cap.max(1);
        let mut session_trade_bridge = SessionPoolTradeBridge::from_runtime_config(
            SESSION_POOL_TRADE_BUFFER_TTL,
            detected_pool_ttl,
            detected_pool_cap,
        );
        let mut session_account_update_bridge =
            SessionAccountUpdateBridge::from_runtime_config(detected_pool_ttl, detected_pool_cap);
        let mut prune_interval = tokio::time::interval(session_bridge_prune_interval(
            SESSION_POOL_TRADE_BUFFER_TTL,
            detected_pool_ttl,
        ));
        info!("Seer: Starting IPC event processing");
        info!("Seer: IPC receiver task is now listening for pool detection events from Seer core");

        loop {
            let seer_event = tokio::select! {
                _ = prune_interval.tick() => {
                    let (expired_pending, expired_detected) =
                        session_trade_bridge.prune_expired(Instant::now());
                    record_session_buffer_expired(expired_pending);
                    record_session_detected_pool_expired(expired_detected);
                    let (expired_updates, expired_update_keys) =
                        session_account_update_bridge.prune_expired(Instant::now());
                    record_session_account_update_expired(expired_updates);
                    record_session_account_update_detected_key_expired(expired_update_keys);
                    continue;
                }
                maybe_event = ipc_receiver.recv() => match maybe_event {
                    Some(event) => event,
                    None => break,
                }
            };

            // Mark IPC heartbeat on every received event
            if let Some(ref h) = health_ipc {
                h.mark_ipc_event();
            }

            match seer_event {
                seer::ipc::SeerEvent::PoolDetected(event) => {
                    let candidate = &event.candidate;

                    info!(
                        "Seer: Pool detected via IPC - pool={}, amm={}, priority={:?}",
                        candidate.pool_amm_id, candidate.amm_program_id, event.priority
                    );

                    // Use event.detected_at for true IPC latency
                    // (time from IPC event creation to consumption), not
                    // candidate.timestamp which is in seconds and gives
                    // false ~500-600ms readings.
                    let ipc_latency_ms = event
                        .detected_at
                        .elapsed()
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let detected_ms = event
                        .detected_at
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let clock_summary = detection_clock_summary(&candidate, detected_ms);

                    info!(
                        "Seer: 🕒 Pool detection latency={}ms pool={} base_mint={} slot={:?} compat_event_ts_ms={} effective_event_ts_ms={:?} chain_event_ts_ms={:?} detected_wall_ts_ms={} has_explicit_event_time={} ingest_latency_ms={} source=ipc",
                        ipc_latency_ms,
                        candidate.pool_amm_id,
                        candidate.base_mint,
                        candidate.slot,
                        clock_summary.compat_event_ts_ms,
                        clock_summary.effective_event_ts_ms,
                        clock_summary.chain_event_ts_ms,
                        detected_ms,
                        clock_summary.has_explicit_event_time,
                        clock_summary.ingest_latency_ms
                    );

                    // [LAST-GATE VALIDATION] Final invariant checks before emission/bootstrap
                    // This is the last line of defense against invalid data propagating downstream
                    let pumpfun_global_state_str = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM";

                    let base_mint_str = candidate.base_mint.to_string();
                    let bonding_curve_str = candidate.bonding_curve.to_string();
                    let amm_program_str = candidate.amm_program_id.to_string();
                    let pool_amm_id_str = candidate.pool_amm_id.to_string();

                    // Invariant 1: base_mint must NEVER be the program ID
                    if is_known_pump_fun_program_id(&base_mint_str) {
                        error!(
                            "🚨 LAST-GATE REJECT: base_mint equals Pump.fun program ID | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue; // Skip this pool - do not emit, do not bootstrap
                    }

                    // Invariant 2: base_mint must NEVER be the global state address
                    if base_mint_str == pumpfun_global_state_str {
                        error!(
                            "🚨 LAST-GATE REJECT: base_mint equals Pump.fun global state | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 3: bonding_curve must NEVER be the program ID
                    if is_known_pump_fun_program_id(&bonding_curve_str) {
                        error!(
                            "🚨 LAST-GATE REJECT: bonding_curve equals Pump.fun program ID | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 4: bonding_curve must NEVER be the global state address
                    if bonding_curve_str == pumpfun_global_state_str {
                        error!(
                            "🚨 LAST-GATE REJECT: bonding_curve equals Pump.fun global state | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 5: base_mint must NEVER equal amm_program (field swap detection)
                    if base_mint_str == amm_program_str {
                        error!(
                            "🚨 LAST-GATE REJECT: base_mint equals amm_program (field swap) | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | DROPPED",
                            candidate.signature,
                            pool_amm_id_str,
                            base_mint_str,
                            bonding_curve_str,
                            amm_program_str
                        );
                        continue;
                    }

                    // Invariant 6: base_mint should not equal bonding_curve (fallback bug detection)
                    // This is usually a sign of incorrect fallback logic
                    if base_mint_str == bonding_curve_str {
                        warn!(
                            "⚠️  LAST-GATE WARNING: base_mint equals bonding_curve (possible fallback) | \
                             source=seer_ipc | signature={} | pool={} | base_mint={} | \
                             bonding_curve={} | amm_program={} | ALLOWED",
                            candidate.signature, pool_amm_id_str, base_mint_str,
                            bonding_curve_str, amm_program_str
                        );
                        // Allow but log - this can be legitimate in some edge cases
                    }

                    // Bootstrap SnapshotEngine only after final invariants pass.
                    if let Some(ref engine) = snapshot_engine {
                        engine.track_pool(candidate.pool_amm_id);
                        let init_event = InitPoolEvent {
                            pool_amm_id: candidate.pool_amm_id,
                            base_mint: candidate.base_mint,
                            quote_mint: candidate.quote_mint,
                            slot: candidate.slot,
                            timestamp_ms: clock_summary.compat_event_ts_ms,
                            initial_liquidity_sol: candidate.initial_liquidity_sol.unwrap_or(0.0),
                            initial_reserve_base: 0.0,
                            initial_reserve_quote: candidate.initial_liquidity_sol.unwrap_or(0.0),
                            initial_price_quote: 0.0,
                        };

                        engine.handle_initialize_pool_event(&init_event);
                        info!(
                            "Seer: 📸 Bootstrapped SnapshotEngine for pool={}",
                            candidate.pool_amm_id
                        );
                    }

                    // Emit to unified event bus if available
                    if let Some(ref tx) = event_bus_tx {
                        process_pool_detected_event_for_session_gate(
                            tx,
                            &mut session_trade_bridge,
                            candidate,
                            health_ipc.as_ref(),
                            Instant::now(),
                            detected_ms,
                        );
                        let flush = session_account_update_bridge
                            .register_detected_pool(candidate, Instant::now());
                        record_session_account_update_expired(flush.expired_count);
                        record_session_account_update_detected_key_expired(
                            flush.expired_detected_keys,
                        );
                        if !flush.replay_ready.is_empty() {
                            ::metrics::counter!(
                                "seer_bridge_session_account_update_replayed_total",
                                flush.replay_ready.len() as u64
                            );
                            for update in &flush.replay_ready {
                                emit_account_update_to_event_bus(
                                    tx,
                                    update,
                                    health_ipc.as_ref(),
                                    true,
                                );
                            }
                        }
                    } else {
                        let flush_result = session_trade_bridge
                            .register_detected_pool(candidate.pool_amm_id, Instant::now());
                        record_session_buffer_expired(flush_result.expired_count);
                        record_session_detected_pool_expired(flush_result.expired_detected_pools);
                        record_session_detected_pool_evicted(flush_result.evicted_detected_pools);
                        let flush = session_account_update_bridge
                            .register_detected_pool(candidate, Instant::now());
                        record_session_account_update_expired(flush.expired_count);
                        record_session_account_update_detected_key_expired(
                            flush.expired_detected_keys,
                        );
                    }

                    // Log metrics periodically
                    if event.sequence_number % 100 == 0 {
                        let drop_rate = ipc_metrics.calculate_drop_rate();
                        let queue_util = ipc_metrics.calculate_queue_utilization(10000);
                        info!(
                            "Seer: IPC metrics - queue_util={:.1}%, drop_rate={:.2}%",
                            queue_util, drop_rate
                        );
                    }
                }

                seer::ipc::SeerEvent::Trade(trade_event) => {
                    let trade = &trade_event.trade;

                    if !trade_has_forwardable_identity(trade) {
                        warn!(
                            "Seer: dropping unresolved trade before Event Bus bridge sig={} pool={} mint={} event_ordinal={:?}",
                            trade.signature,
                            trade.pool_amm_id,
                            trade.mint,
                            trade.event_ordinal
                        );
                        continue;
                    }

                    // --- Canonical Bridge: Seer TradeEvent → Shadow Ledger PoolTransaction ---
                    // This is the single, explicit adapter that maps Seer parsed trade semantics
                    // into the canonical PoolTransaction input model consumed by the Shadow Ledger
                    // runtime flow (Gatekeeper pre-commit / LivePipeline post-commit).
                    //
                    // Seer is the canonical transaction PRODUCER.
                    // Shadow Ledger (via Gatekeeper + LivePipeline) is the authoritative curve-state CONSUMER.
                    //
                    // NOTE: Log only forwarded trades (ForwardNow). Pools born before session
                    // startup are SilentDrop — logging before the gate check would spam hundreds
                    // of thousands of INFO lines per minute for pools we will never observe.
                    if let Some(ref tx) = event_bus_tx {
                        let now = Instant::now();
                        let gate = process_trade_event_for_session_gate(
                            tx,
                            &mut session_trade_bridge,
                            trade,
                            health_ipc.as_ref(),
                            now,
                        );
                        if gate.decision == SessionTradeDecision::ForwardNow {
                            let liveness =
                                session_account_update_bridge.refresh_from_trade(trade, now);
                            record_session_account_update_expired(liveness.expired_count);
                            record_session_account_update_detected_key_expired(
                                liveness.expired_detected_keys,
                            );
                            let ipc_volume_sol = if trade.is_buy {
                                trade.max_sol_cost as f64 / 1_000_000_000.0
                            } else {
                                trade.min_sol_output as f64 / 1_000_000_000.0
                            };
                            info!(
                                "Seer: 🔄 Trade detected via IPC - {} on pool={}, mint={}, sol_volume={:.6} SOL, token_amount={:.6}, signer={}",
                                if trade.is_buy { "BUY" } else { "SELL" },
                                trade.pool_amm_id,
                                trade.mint,
                                ipc_volume_sol,
                                trade.amount as f64 / 1_000_000.0,
                                trade.signer
                            );
                        }
                    }
                }

                seer::ipc::SeerEvent::FundingTransfer(funding_event) => {
                    if let Some(ref tx) = event_bus_tx {
                        emit_funding_transfer_to_event_bus(tx, &funding_event, health_ipc.as_ref());
                    }
                }

                // ── Live AccountUpdate canonical ingest wiring ────────────────
                // This boolean is the launcher-derived effective runtime state,
                // not a primary production config selector. When true, Seer
                // forwards canonical reserve snapshots to OracleRuntime so
                // AccountStateCore remains hydrated in real time.
                //
                // When false, we are in explicit degraded/test compatibility
                // startup and the canonical AccountUpdate relay is intentionally
                // suppressed end-to-end.
                seer::ipc::SeerEvent::AccountUpdate(au) => {
                    if canonical_account_update_relay_enabled {
                        if let Some(ref tx) = event_bus_tx {
                            let ingress = session_account_update_bridge
                                .ingest_account_update(&au, Instant::now());
                            record_session_account_update_expired(ingress.expired_count);
                            record_session_account_update_detected_key_expired(
                                ingress.expired_detected_keys,
                            );
                            record_session_account_update_evictions(
                                ingress.evicted_per_key,
                                ingress.evicted_global,
                            );
                            match ingress.decision {
                                SessionAccountUpdateDecision::ForwardNow => {
                                    emit_account_update_to_event_bus(
                                        tx,
                                        &au,
                                        health_ipc.as_ref(),
                                        false,
                                    );
                                }
                                SessionAccountUpdateDecision::BufferedUntilPoolDetected => {
                                    ::metrics::counter!(
                                        "seer_bridge_session_account_update_buffered_total",
                                        1u64
                                    );
                                }
                                SessionAccountUpdateDecision::SilentDrop => {
                                    increment_counter!(
                                        "seer_bridge_session_account_update_silent_drop_total"
                                    );
                                }
                            }
                        }
                    }
                    // degraded/test compatibility: silently drop — no
                    // ShadowLedger writes happen in Seer, so there is no local
                    // reconciliation side effect.
                }
            }
        }

        warn!("Seer: IPC receiver task has exited - no more pool events will be processed!");
        warn!("Seer: This usually means the Seer core component has stopped or the IPC channel closed");
    });

    // Wait for shutdown signal
    let _ = shutdown_rx.recv().await;
    info!("Seer: Shutdown signal received");

    // Cancel tasks
    seer_handle.abort();
    ipc_handle.abort();

    info!("Seer: Component stopped");
    Ok(())
}

/// Canonical bridge: maps a Seer-parsed `TradeEvent` into the `PoolTransaction` input
/// model consumed by the Shadow Ledger runtime flow.
///
/// This is the **single, explicit adapter** between Seer's ingress/parser role and the
/// Shadow Ledger's authoritative curve-state evolution path (Gatekeeper pre-commit +
/// LivePipeline post-commit).
///
/// ## Semantic contract
/// - Seer is the canonical **transaction producer**: parse, dedup, ordering metadata,
///   mint/pool/curve mapping, and event production.
/// - Shadow Ledger is the authoritative **curve-state machine**: it consumes the
///   `PoolTransaction` produced here via `forward_approved_tx_to_commit_or_live_pipeline`.
///
/// ## Fields preserved
/// | `TradeEvent` source        | `PoolTransaction` field          |
/// |----------------------------|----------------------------------|
/// | `mint`                     | `token_mint`                     |
/// | `event_ordinal`            | `event_ordinal`                  |
/// | `provenance.*`             | execution provenance optionals   |
/// | `timestamp_ms`, `slot`     | `timestamp_ms`, `slot`           |
/// | `arrival_ts_ms`            | `arrival_ts_ms`                  |
/// | `is_buy`                   | `is_buy`                         |
/// | `max_sol_cost`/`min_sol_output` | `sol_amount_lamports`       |
/// | `amount`                   | `token_amount_units`             |
/// | `signer`                   | `signer`                         |
/// | `is_dev_buy`               | `is_dev_buy`, `dev_buy_lamports` |
/// | `v_tokens_*`, `v_sol_*`    | `reserve_base`, `reserve_quote`  |
/// | `signature`                | `signature`                      |
pub fn trade_event_to_pool_transaction(
    trade: &seer::types::TradeEvent,
) -> crate::events::PoolTransaction {
    let sol_amount_lamports = if trade.is_buy {
        trade.max_sol_cost
    } else {
        trade.min_sol_output
    };
    let volume_sol = sol_amount_lamports as f64 / 1_000_000_000.0;

    crate::events::PoolTransaction {
        semantic: trade.semantic,
        pool_amm_id: trade.pool_amm_id.to_string(),
        slot: trade.slot,
        event_ordinal: trade.event_ordinal,
        outer_instruction_index: trade
            .provenance
            .as_ref()
            .and_then(|value| value.outer_instruction_index),
        inner_group_index: trade
            .provenance
            .as_ref()
            .and_then(|value| value.inner_group_index),
        outer_program_id: trade
            .provenance
            .as_ref()
            .and_then(|value| value.outer_program_id.clone()),
        cpi_stack_height: trade
            .provenance
            .as_ref()
            .and_then(|value| value.stack_height),
        timestamp_ms: trade.timestamp_ms,
        event_time: trade.event_time,
        arrival_ts_ms: trade.arrival_ts_ms,
        signer: trade.signer.to_string(),
        is_buy: trade.is_buy,
        volume_sol,
        sol_amount_lamports: Some(sol_amount_lamports),
        token_amount_units: Some(trade.amount),
        reserve_base: trade.v_tokens_in_bonding_curve,
        reserve_quote: trade.v_sol_in_bonding_curve,
        price_quote: match (
            trade.v_tokens_in_bonding_curve,
            trade.v_sol_in_bonding_curve,
        ) {
            (Some(tokens), Some(sol)) if tokens > 0.0 => Some(sol / tokens),
            _ => None,
        },
        is_dev_buy: trade.is_dev_buy,
        dev_buy_lamports: if trade.is_dev_buy {
            sol_amount_lamports
        } else {
            0
        },
        signature: trade.signature.to_string(),
        success: trade.success,
        error_code: trade.error_code.clone(),
        compute_units_consumed: trade.compute_units_consumed,
        owner_token_deltas: trade.owner_token_deltas.clone(),
        mpcf_payload: trade.mpcf_payload.clone(),
        mpcf_payload_missing_reason: trade.mpcf_payload_missing_reason,
        token_mint: (trade.mint != Pubkey::default()).then(|| trade.mint.to_string()),
        v_tokens_in_bonding_curve: trade.v_tokens_in_bonding_curve,
        v_sol_in_bonding_curve: trade.v_sol_in_bonding_curve,
        market_cap_sol: trade.market_cap_sol,
        global_config: trade.global_config.map(|value| value.to_string()),
        fee_recipient: trade.fee_recipient.map(|value| value.to_string()),
        token_program: trade.token_program.map(|value| value.to_string()),
        buy_variant: trade.buy_variant.clone(),
        associated_bonding_curve: trade
            .associated_bonding_curve
            .map(|value| value.to_string()),
        is_mayhem_mode: trade.is_mayhem_mode,
        cu_price_micro_lamports: trade.cu_price_micro_lamports,
        compute_unit_limit: trade.compute_unit_limit,
        inner_ix_count: trade.inner_ix_count,
        cpi_depth: trade.cpi_depth,
        ata_create_count: trade.ata_create_count,
        signer_pre_balance_lamports: trade.signer_pre_balance_lamports,
        signer_post_balance_lamports: trade.signer_post_balance_lamports,
        jito_tip_detected: trade.jito_tip_detected,
        toolchain_fingerprint: trade.toolchain_fingerprint.clone(),
        curve_data_known: trade.curve_data_known,
        curve_finality: trade.curve_finality,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        detected_pool_from_candidate, detection_clock_summary, emit_funding_transfer_to_event_bus,
        process_pool_detected_event_for_session_gate, process_trade_event_for_session_gate,
        pumpswap_program_id, trade_event_to_pool_transaction, trade_has_forwardable_identity,
        SessionAccountUpdateBridge, SessionAccountUpdateDecision, SessionPoolTradeBridge,
        SessionTradeDecision,
    };
    use crate::events::{create_event_bus, GhostEvent};
    use ghost_core::CurveFinality;
    use seer::ipc::{
        AccountUpdateReplayOrigin, DetectedAccountUpdateEvent, DetectedFundingTransferEvent,
        DetectedPoolEvent, DetectedTradeEvent, EventPriority, FundingTransferEvent, SeerEvent,
    };
    use seer::types::{CandidatePool, InstructionProvenance, RawBytesMissingReason, TradeEvent};
    use solana_sdk::{pubkey::Pubkey, signature::Signature};
    use std::str::FromStr;
    use std::time::{Duration, Instant, SystemTime};

    fn make_candidate(pool: Pubkey, mint: Pubkey) -> CandidatePool {
        CandidatePool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(11),
            event_ts_ms: Some(11_000),
            event_time: ghost_core::EventTimeMetadata::default(),
            signature: Signature::new_unique().to_string(),
            amm_program_id: Pubkey::new_unique(),
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            timestamp: 11,
            bonding_curve_progress: Some(1.0),
            initial_liquidity_sol: Some(1.5),
            token_total_supply: Some(1_000_000),
            block_time: Some(11),
        }
    }

    async fn recv_only_event(rx: &mut tokio::sync::broadcast::Receiver<GhostEvent>) -> GhostEvent {
        tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("event bus closed")
    }

    fn make_trade(pool: Pubkey, mint: Pubkey) -> TradeEvent {
        TradeEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(1),
            signature: Signature::new_unique(),
            event_ordinal: Some(7),
            provenance: None,
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1_010,
            pool_amm_id: pool,
            mint,
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: false,
            amount: 42,
            max_sol_cost: 1_000_000_000,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
            v_tokens_in_bonding_curve: Some(10.0),
            v_sol_in_bonding_curve: Some(1.0),
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: true,
            curve_finality: ghost_core::CurveFinality::Provisional,
            is_pumpswap: false,
        }
    }

    fn make_funding_transfer() -> FundingTransferEvent {
        FundingTransferEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            slot: Some(22),
            event_ordinal: Some(4),
            outer_instruction_index: Some(1),
            inner_group_index: Some(1),
            cpi_stack_height: Some(2),
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 22_010,
            signature: "funding-sig".to_string(),
            source_wallet: Pubkey::new_unique().to_string(),
            recipient_wallet: Pubkey::new_unique().to_string(),
            lamports: 50_000_000,
            full_chain_coverage: false,
            provenance: seer::ipc::FundingTransferProvenance::filtered_grpc_global_stream_live(),
        }
    }

    fn make_account_update(base_mint: Pubkey, bonding_curve: Pubkey) -> DetectedAccountUpdateEvent {
        DetectedAccountUpdateEvent {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            base_mint,
            bonding_curve,
            curve_finality: CurveFinality::Provisional,
            sol_reserves: 10,
            token_reserves: 20,
            complete: 0,
            slot: 42,
            write_version: Some(7),
            replay_origin: AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: SystemTime::now(),
            sequence_number: 1,
        }
    }

    #[test]
    fn canonical_account_update_decode_supports_pumpswap_pool_layout() {
        let base_mint = Pubkey::new_unique();
        let pool_state = seer::binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 9,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: Pubkey::from_str("So11111111111111111111111111111111111111112")
                .expect("valid wsol mint")
                .to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 777,
            quote_amount: 888,
        };
        let mut data = seer::binary_parser::DISC_AMM_POOL.to_vec();
        data.push(pool_state.pool_bump);
        data.extend_from_slice(&pool_state.index.to_le_bytes());
        data.extend_from_slice(&pool_state.creator);
        data.extend_from_slice(&pool_state.base_mint);
        data.extend_from_slice(&pool_state.quote_mint);
        data.extend_from_slice(&pool_state.lp_mint);
        data.extend_from_slice(&pool_state.pool_base_token_account);
        data.extend_from_slice(&pool_state.pool_quote_token_account);
        data.extend_from_slice(&pool_state.base_amount.to_le_bytes());
        data.extend_from_slice(&pool_state.quote_amount.to_le_bytes());

        let payload = seer::decode_canonical_account_update(*pumpswap_program_id(), &data)
            .expect("pumpswap AMM pool must decode as canonical account update");
        assert_eq!(payload.sol_reserves(), 888);
        assert_eq!(payload.token_reserves(), 777);
        assert_eq!(payload.complete(), 1);
    }

    #[test]
    fn canonical_account_update_decode_is_data_driven_even_for_unknown_owner() {
        let base_mint = Pubkey::new_unique();
        let pool_state = seer::binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 5,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: Pubkey::from_str("So11111111111111111111111111111111111111112")
                .expect("valid wsol mint")
                .to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 10,
            quote_amount: 20,
        };
        let mut data = seer::binary_parser::DISC_AMM_POOL.to_vec();
        data.push(pool_state.pool_bump);
        data.extend_from_slice(&pool_state.index.to_le_bytes());
        data.extend_from_slice(&pool_state.creator);
        data.extend_from_slice(&pool_state.base_mint);
        data.extend_from_slice(&pool_state.quote_mint);
        data.extend_from_slice(&pool_state.lp_mint);
        data.extend_from_slice(&pool_state.pool_base_token_account);
        data.extend_from_slice(&pool_state.pool_quote_token_account);
        data.extend_from_slice(&pool_state.base_amount.to_le_bytes());
        data.extend_from_slice(&pool_state.quote_amount.to_le_bytes());

        let unknown_owner = Pubkey::new_unique();
        assert_ne!(unknown_owner, *pumpswap_program_id());
        let payload = seer::decode_canonical_account_update(unknown_owner, &data)
            .expect("layout decoding remains data-driven");
        assert_eq!(payload.sol_reserves(), 20);
        assert_eq!(payload.token_reserves(), 10);
    }

    #[test]
    fn trade_event_to_pool_transaction_preserves_failed_status() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut trade = make_trade(pool, mint);
        trade.success = false;
        trade.error_code = Some("InstructionError(Custom(1))".to_string());

        let pool_tx = trade_event_to_pool_transaction(&trade);

        assert!(!pool_tx.success);
        assert_eq!(
            pool_tx.error_code.as_deref(),
            Some("InstructionError(Custom(1))")
        );
    }

    #[test]
    fn trade_with_default_mint_is_not_forwardable() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::default());
        assert!(!trade_has_forwardable_identity(&trade));
    }

    #[test]
    fn trade_with_resolved_identity_is_forwardable() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        assert!(trade_has_forwardable_identity(&trade));
    }

    #[test]
    fn bridge_preserves_event_ordinal() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        let tx = trade_event_to_pool_transaction(&trade);
        assert_eq!(tx.event_ordinal, trade.event_ordinal);
    }

    #[test]
    fn bridge_preserves_toolchain_fingerprint() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.toolchain_fingerprint = seer::types::ToolchainFingerprintInput {
            account_keys_len: Some(18),
            outer_instruction_count: Some(3),
            inner_instruction_group_count: Some(2),
            has_set_compute_unit_limit: Some(true),
            has_set_compute_unit_price: Some(true),
            internal_fee_transfer_count: Some(0),
            external_fee_transfer_count: Some(2),
            filtered_wsol_self_transfer_count: Some(1),
        };

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(tx.toolchain_fingerprint, trade.toolchain_fingerprint);
    }

    #[test]
    fn bridge_preserves_signer_post_balance() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.signer_pre_balance_lamports = Some(5_000_000_000);
        trade.signer_post_balance_lamports = Some(4_100_000_000);

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(
            tx.signer_pre_balance_lamports,
            trade.signer_pre_balance_lamports
        );
        assert_eq!(
            tx.signer_post_balance_lamports,
            trade.signer_post_balance_lamports
        );
    }

    #[test]
    fn bridge_preserves_provenance_when_enabled() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.provenance = Some(InstructionProvenance {
            outer_instruction_index: Some(4),
            inner_group_index: Some(2),
            outer_program_id: Some("outer-program".to_string()),
            invoked_program_id: "invoked-program".to_string(),
            stack_height: Some(3),
            from_cpi: true,
        });

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(tx.outer_instruction_index, Some(4));
        assert_eq!(tx.inner_group_index, Some(2));
        assert_eq!(tx.outer_program_id.as_deref(), Some("outer-program"));
        assert_eq!(tx.cpi_stack_height, Some(3));
    }

    #[test]
    fn bridge_omits_default_mint_identity() {
        let trade = make_trade(Pubkey::new_unique(), Pubkey::default());
        let tx = trade_event_to_pool_transaction(&trade);
        assert!(tx.token_mint.is_none());
    }

    #[test]
    fn bridge_preserves_trade_semantics_and_mpcf_payload() {
        let mut trade = make_trade(Pubkey::new_unique(), Pubkey::new_unique());
        trade.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::PumpPortal,
            ghost_core::EventTruthKind::Synthetic,
            ghost_core::SlotQuality::Absent,
            ghost_core::TimestampQuality::Adapter,
            ghost_core::EventCompleteness::Partial,
        );
        trade.mpcf_payload = vec![9, 8, 7];
        trade.mpcf_payload_missing_reason = RawBytesMissingReason::NotMissing;

        let tx = trade_event_to_pool_transaction(&trade);

        assert_eq!(tx.semantic, trade.semantic);
        assert_eq!(tx.mpcf_payload, trade.mpcf_payload);
        assert_eq!(
            tx.mpcf_payload_missing_reason,
            RawBytesMissingReason::NotMissing
        );
    }

    #[test]
    fn detected_pool_fallback_downgrades_timestamp_quality_to_wall_clock() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_ts_ms = None;
        candidate.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::Grpc,
            ghost_core::EventTruthKind::RawChain,
            ghost_core::SlotQuality::Present,
            ghost_core::TimestampQuality::Chain,
            ghost_core::EventCompleteness::Full,
        );

        let detected = detected_pool_from_candidate(&candidate, 77_000);

        assert_eq!(detected.timestamp_ms, 77_000);
        assert_eq!(
            detected.semantic.timestamp_quality,
            ghost_core::TimestampQuality::WallClock
        );
        assert_eq!(
            detected.semantic.completeness,
            ghost_core::EventCompleteness::Partial
        );
    }

    #[test]
    fn detection_clock_summary_prefers_explicit_ingress_time_for_latency() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_time = ghost_core::EventTimeMetadata::new(None, Some(66_000), None);
        candidate.event_ts_ms = Some(55_000);

        let summary = detection_clock_summary(&candidate, 77_000);

        assert_eq!(summary.compat_event_ts_ms, 66_000);
        assert_eq!(summary.effective_event_ts_ms, Some(66_000));
        assert_eq!(summary.chain_event_ts_ms, None);
        assert!(summary.has_explicit_event_time);
        assert_eq!(summary.ingest_latency_ms, 11_000);
    }

    #[test]
    fn detection_clock_summary_ignores_legacy_only_timestamp_for_latency() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_time = ghost_core::EventTimeMetadata::default();
        candidate.event_ts_ms = Some(66_000);

        let summary = detection_clock_summary(&candidate, 77_000);

        assert_eq!(summary.compat_event_ts_ms, 66_000);
        assert_eq!(summary.effective_event_ts_ms, None);
        assert_eq!(summary.chain_event_ts_ms, None);
        assert!(!summary.has_explicit_event_time);
        assert_eq!(summary.ingest_latency_ms, 0);
    }

    #[test]
    fn detected_pool_legacy_timestamp_downgrades_timestamp_quality_to_wall_clock() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_time = ghost_core::EventTimeMetadata::default();
        candidate.event_ts_ms = Some(66_000);
        candidate.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::Grpc,
            ghost_core::EventTruthKind::RawChain,
            ghost_core::SlotQuality::Present,
            ghost_core::TimestampQuality::Chain,
            ghost_core::EventCompleteness::Full,
        );

        let detected = detected_pool_from_candidate(&candidate, 77_000);

        assert_eq!(detected.timestamp_ms, 66_000);
        assert_eq!(
            detected.semantic.timestamp_quality,
            ghost_core::TimestampQuality::WallClock
        );
        assert_eq!(
            detected.semantic.completeness,
            ghost_core::EventCompleteness::Partial
        );
    }

    #[test]
    fn detected_pool_explicit_chain_event_time_preserves_chain_quality() {
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.event_ts_ms = None;
        candidate.event_time = ghost_core::EventTimeMetadata::new(Some(66_000), None, None);
        candidate.semantic = ghost_core::EventSemanticEnvelope::new(
            ghost_core::SourceKind::Grpc,
            ghost_core::EventTruthKind::RawChain,
            ghost_core::SlotQuality::Present,
            ghost_core::TimestampQuality::Chain,
            ghost_core::EventCompleteness::Full,
        );

        let detected = detected_pool_from_candidate(&candidate, 77_000);

        assert_eq!(detected.timestamp_ms, 66_000);
        assert_eq!(
            detected.semantic.timestamp_quality,
            ghost_core::TimestampQuality::Chain
        );
        assert_eq!(
            detected.semantic.completeness,
            ghost_core::EventCompleteness::Full
        );
    }

    #[test]
    fn session_bridge_silently_drops_unknown_pool_trade() {
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        // Unknown pool → SilentDrop.
        let ingress = bridge.ingest_trade(&trade, now);
        assert_eq!(ingress.decision, SessionTradeDecision::SilentDrop);
        assert_eq!(bridge.pending_total(), 0);

        // Subsequent trade for now-registered pool → ForwardNow.
        let flush = bridge.register_detected_pool(pool, now + Duration::from_millis(50));
        assert_eq!(flush.expired_count, 0);
        assert!(flush.replay_ready.is_empty());
        let ingress2 = bridge.ingest_trade(&trade, now + Duration::from_millis(51));
        assert_eq!(ingress2.decision, SessionTradeDecision::ForwardNow);
    }

    #[test]
    fn session_bridge_silently_drops_unknown_pool_regardless_of_timing() {
        let ttl = Duration::from_millis(1); // extremely short TTL
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        let ingress = bridge.ingest_trade(&trade, now);
        assert_eq!(ingress.decision, SessionTradeDecision::SilentDrop);

        // TTL elapsed before PoolDetected → still no replay because unknown pool was never buffered.
        let flush = bridge.register_detected_pool(pool, now + Duration::from_millis(500));
        assert_eq!(flush.expired_count, 0);
        assert!(flush.replay_ready.is_empty());
    }

    #[test]
    fn session_bridge_forwards_immediately_after_pool_detected() {
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        let flush = bridge.register_detected_pool(pool, now);
        assert!(flush.replay_ready.is_empty());

        let ingress = bridge.ingest_trade(&trade, now + Duration::from_millis(1));
        assert_eq!(ingress.decision, SessionTradeDecision::ForwardNow);
    }

    #[test]
    fn session_bridge_forwards_trades_after_pool_detected_registration() {
        // Production path: PoolDetected arrives first, then trades.
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let now = Instant::now();

        // Trade before registration → SilentDrop.
        let pre = bridge.ingest_trade(&trade, now);
        assert_eq!(pre.decision, SessionTradeDecision::SilentDrop);

        // PoolDetected registers the pool, but no prior trade is replayed.
        let flush = bridge.register_detected_pool(pool, now + Duration::from_millis(1));
        assert!(flush.replay_ready.is_empty());

        // All subsequent trades → ForwardNow.
        let first = bridge.ingest_trade(&trade, now + Duration::from_millis(2));
        assert_eq!(first.decision, SessionTradeDecision::ForwardNow);

        let second = bridge.ingest_trade(&trade, now + Duration::from_millis(3));
        assert_eq!(second.decision, SessionTradeDecision::ForwardNow);
        assert_eq!(bridge.pending_total(), 0);
    }

    #[test]
    fn session_bridge_prunes_detected_pool_registry_after_ttl() {
        let ttl = Duration::from_millis(100);
        let registry_ttl = Duration::from_millis(20);
        let mut bridge = SessionPoolTradeBridge::new(ttl, 4, 16, registry_ttl, 32);
        let pool = Pubkey::new_unique();
        let now = Instant::now();

        let flush = bridge.register_detected_pool(pool, now);
        assert_eq!(flush.expired_detected_pools, 0);
        assert_eq!(bridge.detected_total(), 1);

        let (_expired_pending, expired_detected) =
            bridge.prune_expired(now + Duration::from_millis(25));
        assert_eq!(expired_detected, 1);
        assert_eq!(bridge.detected_total(), 0);
    }

    #[test]
    fn session_account_update_bridge_buffers_until_pool_detected() {
        let ttl = Duration::from_millis(100);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let ingress = bridge.ingest_account_update(&update, now);
        assert_eq!(
            ingress.decision,
            SessionAccountUpdateDecision::BufferedUntilPoolDetected
        );
        assert_eq!(bridge.pending_total(), 1);

        let flush = bridge.register_detected_pool(&candidate, now + Duration::from_millis(1));
        assert_eq!(flush.expired_count, 0);
        assert_eq!(flush.replay_ready.len(), 1);
        assert_eq!(flush.replay_ready[0].base_mint, mint);
        assert_eq!(bridge.pending_total(), 0);
    }

    #[test]
    fn session_account_update_bridge_expires_unknown_updates() {
        let ttl = Duration::from_millis(5);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, Duration::from_secs(60), 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let ingress = bridge.ingest_account_update(&update, now);
        assert_eq!(
            ingress.decision,
            SessionAccountUpdateDecision::BufferedUntilPoolDetected
        );

        let flush = bridge.register_detected_pool(&candidate, now + Duration::from_millis(20));
        assert_eq!(flush.replay_ready.len(), 0);
        assert_eq!(flush.expired_count, 1);
    }

    #[test]
    fn session_account_update_bridge_refreshes_detected_keys_on_forward_now() {
        let ttl = Duration::from_millis(100);
        let detected_key_ttl = Duration::from_millis(10);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, detected_key_ttl, 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let flush = bridge.register_detected_pool(&candidate, now);
        assert_eq!(flush.expired_detected_keys, 0);

        let first = bridge.ingest_account_update(&update, now + Duration::from_millis(8));
        assert_eq!(first.decision, SessionAccountUpdateDecision::ForwardNow);

        let second = bridge.ingest_account_update(&update, now + Duration::from_millis(15));
        assert_eq!(second.decision, SessionAccountUpdateDecision::ForwardNow);
    }

    #[test]
    fn session_account_update_bridge_refreshes_detected_keys_from_trade_activity() {
        let ttl = Duration::from_millis(100);
        let detected_key_ttl = Duration::from_millis(10);
        let mut bridge = SessionAccountUpdateBridge::new(ttl, 4, 16, detected_key_ttl, 32);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let curve = Pubkey::new_unique();
        let mut candidate = make_candidate(pool, mint);
        candidate.bonding_curve = curve;
        let trade = make_trade(pool, mint);
        let update = make_account_update(mint, curve);
        let now = Instant::now();

        let flush = bridge.register_detected_pool(&candidate, now);
        assert_eq!(flush.expired_detected_keys, 0);

        let keepalive = bridge.refresh_from_trade(&trade, now + Duration::from_millis(8));
        assert_eq!(keepalive.expired_detected_keys, 0);

        let ingress = bridge.ingest_account_update(&update, now + Duration::from_millis(15));
        assert_eq!(ingress.decision, SessionAccountUpdateDecision::ForwardNow);
    }

    #[tokio::test]
    async fn seer_trade_without_pool_detected_is_silently_dropped() {
        // Trade for an unknown pool must not hit the event bus immediately; it is
        // discarded by the launcher-side session gate until PoolDetected arrives.
        let (tx, mut rx) = create_event_bus();
        let pool = Pubkey::new_unique();
        let trade = make_trade(pool, Pubkey::new_unique());
        let trade_event = SeerEvent::Trade(DetectedTradeEvent {
            trade: trade.clone(),
            detected_at: SystemTime::now(),
            sequence_number: 1,
            priority: EventPriority::Normal,
        });
        let mut bridge = SessionPoolTradeBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );

        match trade_event {
            SeerEvent::Trade(event) => {
                let gating = process_trade_event_for_session_gate(
                    &tx,
                    &mut bridge,
                    &event.trade,
                    None,
                    Instant::now(),
                );
                assert_eq!(gating.decision, SessionTradeDecision::SilentDrop);
            }
            _ => unreachable!(),
        }

        // Nothing emitted — dropped trade must not produce any event bus message.
        let timeout_result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
        assert!(
            timeout_result.is_err(),
            "No event should be emitted for an unknown-pool trade"
        );
    }

    #[tokio::test]
    async fn seer_trade_before_pool_detected_does_not_replay_after_registration() {
        let (tx, mut rx) = create_event_bus();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = make_trade(pool, mint);
        let candidate = make_candidate(pool, mint);
        let mut bridge = SessionPoolTradeBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );

        let gating =
            process_trade_event_for_session_gate(&tx, &mut bridge, &trade, None, Instant::now());
        assert_eq!(gating.decision, SessionTradeDecision::SilentDrop);

        let detected_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let flush = process_pool_detected_event_for_session_gate(
            &tx,
            &mut bridge,
            &candidate,
            None,
            Instant::now() + Duration::from_millis(1),
            detected_ms,
        );
        assert!(flush.replay_ready.is_empty());

        let first = recv_only_event(&mut rx).await;
        match first {
            GhostEvent::NewPoolDetected(pool_event) => {
                assert_eq!(pool_event.pool_amm_id, pool.to_string());
                assert_eq!(pool_event.base_mint, mint.to_string());
            }
            other => panic!("expected NewPoolDetected, got {}", other.event_type()),
        }

        assert!(tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn seer_pool_detected_then_trade_emits_new_pool_detected_then_pool_transaction() {
        // Production path (seer FIFO guarantee): PoolDetected always precedes Trade on
        // the IPC channel for newly created pools. This test verifies the canonical flow:
        // 1. PoolDetected → bridge registers pool + event bus receives NewPoolDetected
        // 2. Trade → bridge returns ForwardNow + event bus receives PoolTransaction
        let (tx, mut rx) = create_event_bus();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let trade = make_trade(pool, mint);
        let candidate = make_candidate(pool, mint);
        let pool_event = SeerEvent::PoolDetected(DetectedPoolEvent {
            candidate: candidate.clone(),
            detected_at: SystemTime::now(),
            sequence_number: 1,
            priority: EventPriority::Normal,
        });
        let trade_event = SeerEvent::Trade(DetectedTradeEvent {
            trade: trade.clone(),
            detected_at: SystemTime::now(),
            sequence_number: 2,
            priority: EventPriority::Normal,
        });
        let mut bridge = SessionPoolTradeBridge::new(
            Duration::from_millis(100),
            4,
            16,
            Duration::from_secs(60),
            32,
        );

        // Step 1: PoolDetected → registers pool, emits NewPoolDetected.
        match pool_event {
            SeerEvent::PoolDetected(event) => {
                let detected_ms = event
                    .detected_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let flush = process_pool_detected_event_for_session_gate(
                    &tx,
                    &mut bridge,
                    &event.candidate,
                    None,
                    Instant::now(),
                    detected_ms,
                );
                assert!(flush.replay_ready.is_empty());
            }
            _ => unreachable!(),
        }

        // Step 2: Trade for now-registered pool → ForwardNow.
        match trade_event {
            SeerEvent::Trade(event) => {
                let gating = process_trade_event_for_session_gate(
                    &tx,
                    &mut bridge,
                    &event.trade,
                    None,
                    Instant::now(),
                );
                assert_eq!(gating.decision, SessionTradeDecision::ForwardNow);
            }
            _ => unreachable!(),
        }

        // First event: NewPoolDetected.
        let first = recv_only_event(&mut rx).await;
        match first {
            GhostEvent::NewPoolDetected(pool_event) => {
                let expected = detected_pool_from_candidate(
                    &candidate,
                    pool_event
                        .detected_wall_ts_ms
                        .expect("missing detected wall ts"),
                );
                assert_eq!(pool_event.pool_amm_id, expected.pool_amm_id);
                assert_eq!(pool_event.base_mint, expected.base_mint);
            }
            other => panic!("expected NewPoolDetected, got {}", other.event_type()),
        }

        // Second event: PoolTransaction.
        let second = recv_only_event(&mut rx).await;
        match second {
            GhostEvent::PoolTransaction(pool_tx) => {
                assert_eq!(pool_tx.pool_amm_id, pool.to_string());
                assert_eq!(pool_tx.signature, trade.signature.to_string());
            }
            other => panic!("expected PoolTransaction, got {}", other.event_type()),
        }

        assert!(tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn seer_funding_transfer_emits_funding_transfer_observed() {
        let (tx, mut rx) = create_event_bus();
        let funding = SeerEvent::FundingTransfer(DetectedFundingTransferEvent {
            transfer: make_funding_transfer(),
            detected_at: SystemTime::now(),
            sequence_number: 7,
            priority: EventPriority::High,
        });

        match funding {
            SeerEvent::FundingTransfer(event) => {
                emit_funding_transfer_to_event_bus(&tx, &event, None);
            }
            _ => unreachable!(),
        }

        let received = recv_only_event(&mut rx).await;
        match received {
            GhostEvent::FundingTransferObserved(observed) => {
                assert_eq!(observed.signature, "funding-sig");
                assert_eq!(observed.lamports, 50_000_000);
                assert_eq!(observed.event_ordinal, Some(4));
                assert_eq!(observed.outer_instruction_index, Some(1));
                assert_eq!(observed.inner_group_index, Some(1));
                assert_eq!(observed.cpi_stack_height, Some(2));
                assert_eq!(observed.arrival_ts_ms, 22_010);
                assert_eq!(observed.sequence_number, 7);
                assert!(!observed.full_chain_coverage);
                assert_eq!(
                    observed.provenance,
                    seer::ipc::FundingTransferProvenance::filtered_grpc_global_stream_live()
                );
            }
            other => panic!(
                "expected FundingTransferObserved, got {}",
                other.event_type()
            ),
        }
    }
}

const PUMP_FUN_PROGRAM_ID_STR: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

fn is_known_pump_fun_program_id(value: &str) -> bool {
    value == PUMP_FUN_PROGRAM_ID_STR
}

fn pumpswap_program_id() -> &'static Pubkey {
    use std::sync::OnceLock;
    static PK: OnceLock<Pubkey> = OnceLock::new();
    PK.get_or_init(|| Pubkey::from_str(PUMPSWAP_PROGRAM_ID_STR).expect("valid pumpswap program ID"))
}
