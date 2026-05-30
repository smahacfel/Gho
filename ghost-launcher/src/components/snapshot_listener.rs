//! Snapshot Listener Component
//!
//! This component listens to PoolTransaction events from the event bus
//! and feeds them to SnapshotEngine for real-time signals.
//!
//! Forward mode controls which TX are forwarded:
//! - `None`: drop all TX
//! - `TrackedBuffered`: forward TX for known pools as SoftTruth during observation
//! - `ApprovedOnly`: forward only for pools approved by Gatekeeper
//! - `Provisional`: forward all TX with SoftTruth data source

use crate::config::SnapshotListenerForwardMode;
use crate::events::{
    DetectedPool, EventBusReceiver, GhostEvent, PoolTransaction, RawBytesMissingReason,
};
use anyhow::Result;
use ghost_brain::oracle::snapshot_engine::{DataSource, PoolLifecycle, PoolMetrics, TxEvent};
use ghost_brain::oracle::{ApprovedPools, SnapshotEngine};
use ghost_core::EventTruthKind;
use ghost_core::{pipeline_coverage, PipelineCoverageStage, PoolIdentityRegistry};
use metrics::increment_counter;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

const SNAPSHOT_LISTENER_PRUNE_INTERVAL_FLOOR: Duration = Duration::from_millis(50);
const SNAPSHOT_LISTENER_PRUNE_INTERVAL_CEILING: Duration = Duration::from_millis(250);

#[derive(Clone)]
struct StagedPoolTransaction {
    tx: Arc<PoolTransaction>,
    signer: Pubkey,
    staged_at: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct SnapshotStagingConfig {
    pub per_pool_capacity: usize,
    pub global_capacity: usize,
    pub ttl: Duration,
    pub prune_interval: Duration,
}

impl SnapshotStagingConfig {
    pub fn new(per_pool_capacity: usize, ttl_ms: u64, max_pools: usize) -> Self {
        let per_pool_capacity = per_pool_capacity.max(1);
        let ttl = Duration::from_millis(ttl_ms.max(1));
        let global_capacity = per_pool_capacity.saturating_mul(max_pools.max(1));
        let prune_interval = ttl
            .min(SNAPSHOT_LISTENER_PRUNE_INTERVAL_CEILING)
            .max(SNAPSHOT_LISTENER_PRUNE_INTERVAL_FLOOR);

        Self {
            per_pool_capacity,
            global_capacity,
            ttl,
            prune_interval,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StagedTransactionInsertResult {
    evicted_per_pool: usize,
    evicted_global: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StagedTransactionPruneResult {
    expired_transactions: usize,
    expired_pools: usize,
}

fn record_staged_transaction_expired(count: usize) {
    if count == 0 {
        return;
    }
    ::metrics::counter!("snapshot_listener_staged_expired_total", count as u64);
    ::metrics::counter!("ghost_pipeline_stage_total", count as u64, "stage" => "listener_buffer_expired");
}

fn record_staged_transaction_evictions(evicted_per_pool: usize, evicted_global: usize) {
    if evicted_per_pool > 0 {
        ::metrics::counter!("snapshot_listener_staged_evicted_total", evicted_per_pool as u64, "reason" => "per_pool_cap");
        ::metrics::counter!("ghost_pipeline_stage_total", evicted_per_pool as u64, "stage" => "listener_buffer_rejected", "reason" => "per_pool_cap");
    }
    if evicted_global > 0 {
        ::metrics::counter!("snapshot_listener_staged_evicted_total", evicted_global as u64, "reason" => "global_cap");
        ::metrics::counter!("ghost_pipeline_stage_total", evicted_global as u64, "stage" => "listener_buffer_rejected", "reason" => "global_cap");
    }
}

fn evict_oldest_staged_transaction(
    staged_transactions: &mut HashMap<Pubkey, VecDeque<StagedPoolTransaction>>,
    staged_total: &mut usize,
) -> Option<StagedPoolTransaction> {
    let oldest_pool = staged_transactions
        .iter()
        .filter_map(|(pool, queue)| queue.front().map(|tx| (*pool, tx.staged_at)))
        .min_by_key(|(_, staged_at)| *staged_at)
        .map(|(pool, _)| pool)?;

    let (removed, queue_empty) = {
        let queue = staged_transactions.get_mut(&oldest_pool)?;
        let removed = queue.pop_front();
        let queue_empty = queue.is_empty();
        (removed, queue_empty)
    };

    if queue_empty {
        staged_transactions.remove(&oldest_pool);
    }
    if removed.is_some() {
        *staged_total = staged_total.saturating_sub(1);
    }

    removed
}

fn prune_staged_transactions(
    staged_transactions: &mut HashMap<Pubkey, VecDeque<StagedPoolTransaction>>,
    staged_total: &mut usize,
    ttl: Duration,
    now: Instant,
) -> StagedTransactionPruneResult {
    let mut expired_transactions = 0usize;
    let mut expired_pools = 0usize;
    let mut empty_pools = Vec::new();

    for (pool, queue) in staged_transactions.iter_mut() {
        while matches!(queue.front(), Some(front) if now.duration_since(front.staged_at) > ttl) {
            queue.pop_front();
            *staged_total = staged_total.saturating_sub(1);
            expired_transactions += 1;
        }

        if queue.is_empty() {
            empty_pools.push(*pool);
        }
    }

    for pool in empty_pools {
        staged_transactions.remove(&pool);
        expired_pools += 1;
    }

    StagedTransactionPruneResult {
        expired_transactions,
        expired_pools,
    }
}

/// Build a TxEvent from a PoolTransaction for SnapshotEngine consumption.
///
/// Extracted as a standalone function so it can be unit-tested without
/// the async event bus plumbing.
fn build_tx_event(
    pool_tx: &PoolTransaction,
    pool_pubkey: Pubkey,
    base_mint: Pubkey,
    signer: Pubkey,
    data_source: DataSource,
) -> TxEvent {
    TxEvent {
        semantic: pool_tx.semantic,
        pool_amm_id: pool_pubkey,
        base_mint,
        pool_state: PoolLifecycle::Active,
        metrics: PoolMetrics {
            tx_count: 1,
            unique_addrs: 1,
            volume_sol: pool_tx.volume_sol,
            buy_volume_sol: if pool_tx.is_buy {
                pool_tx.volume_sol
            } else {
                0.0
            },
            sell_volume_sol: if pool_tx.is_buy {
                0.0
            } else {
                pool_tx.volume_sol
            },
            dev_buy_lamports: pool_tx.dev_buy_lamports,
            ..Default::default()
        },
        slot: pool_tx.slot,
        timestamp_ms: pool_tx.timestamp_ms,
        event_time: pool_tx.event_time,
        signer,
        is_buy: pool_tx.is_buy,
        volume_sol: pool_tx.volume_sol,
        reserve_base: pool_tx.reserve_base,
        reserve_quote: pool_tx.reserve_quote,
        price_quote: pool_tx.price_quote,
        is_dev_buy: pool_tx.is_dev_buy,
        dev_buy_lamports: pool_tx.dev_buy_lamports,
        signature: Some(pool_tx.signature.clone()),
        // Carry event_ordinal through so SnapshotEngine can build distinct TxKeys
        // for multiple semantic trades within the same transaction signature.
        event_ordinal: pool_tx.event_ordinal,
        block_time: None,
        arrival_time_ms: Some(pool_tx.arrival_ts_ms),
        data_source,
        intra_slot_offset_ms: None,
        raw_data: (!pool_tx.mpcf_payload.is_empty()).then(|| pool_tx.mpcf_payload.clone()),
        raw_data_missing_reason: pool_tx.mpcf_payload_missing_reason,
    }
}

fn is_raw_chain_semantic(pool_tx: &PoolTransaction) -> bool {
    matches!(pool_tx.semantic.event_truth_kind, EventTruthKind::RawChain)
}

fn resolve_base_mint(
    pool_tx: &PoolTransaction,
    pool_pubkey: &Pubkey,
    pool_identities: &PoolIdentityRegistry,
    resolved_pool_base_mints: &HashMap<Pubkey, Pubkey>,
) -> Option<Pubkey> {
    let parsed_tx_mint = pool_tx
        .token_mint
        .as_ref()
        .and_then(|mint_str| Pubkey::from_str(mint_str).ok());

    if let Some(identity) = pool_identities.get_by_pool(pool_pubkey) {
        if let Some(tx_mint) = parsed_tx_mint {
            if tx_mint != identity.base_mint {
                warn!(
                    pool = %pool_pubkey,
                    tx_base_mint = %tx_mint,
                    registry_base_mint = %identity.base_mint,
                    "SnapshotListener: token_mint mismatch; using registry identity"
                );
            }
        }
        return Some(identity.base_mint.into());
    }

    if let Some(base_mint) = resolved_pool_base_mints.get(pool_pubkey) {
        if let Some(tx_mint) = parsed_tx_mint {
            if tx_mint != *base_mint {
                warn!(
                    pool = %pool_pubkey,
                    tx_base_mint = %tx_mint,
                    detected_base_mint = %base_mint,
                    "SnapshotListener: token_mint mismatch; using detected identity"
                );
            }
        }
        return Some(*base_mint);
    }

    parsed_tx_mint.filter(|mint| *mint != Pubkey::default())
}

fn has_authoritative_pool_identity(
    pool_pubkey: &Pubkey,
    pool_identities: &PoolIdentityRegistry,
    resolved_pool_base_mints: &HashMap<Pubkey, Pubkey>,
) -> bool {
    pool_identities.get_by_pool(pool_pubkey).is_some()
        || resolved_pool_base_mints.contains_key(pool_pubkey)
}

fn stage_transaction(
    staged_transactions: &mut HashMap<Pubkey, VecDeque<StagedPoolTransaction>>,
    staged_total: &mut usize,
    staging_config: SnapshotStagingConfig,
    pool_pubkey: Pubkey,
    signer: Pubkey,
    pool_tx: Arc<PoolTransaction>,
    reason: &'static str,
) -> StagedTransactionInsertResult {
    let mut insert_result = StagedTransactionInsertResult::default();

    while *staged_total >= staging_config.global_capacity {
        if evict_oldest_staged_transaction(staged_transactions, staged_total).is_some() {
            insert_result.evicted_global += 1;
        } else {
            break;
        }
    }

    let queue = staged_transactions.entry(pool_pubkey).or_default();
    while queue.len() >= staging_config.per_pool_capacity {
        if queue.pop_front().is_some() {
            *staged_total = staged_total.saturating_sub(1);
            insert_result.evicted_per_pool += 1;
        }
    }

    queue.push_back(StagedPoolTransaction {
        tx: pool_tx.clone(),
        signer,
        staged_at: Instant::now(),
    });
    *staged_total += 1;

    pipeline_coverage().increment(PipelineCoverageStage::PendingMappingBuffered, 1);
    increment_counter!(
        "ghost_pipeline_stage_total",
        "stage" => "listener_buffered",
        "reason" => reason
    );
    debug!(
        pool = %pool_pubkey,
        signature = %pool_tx.signature,
        reason,
        "SnapshotListener: staged tx for later identity resolution"
    );

    insert_result
}

fn maybe_cache_detected_pool_identity(
    detected_pool: &DetectedPool,
    resolved_pool_base_mints: &mut HashMap<Pubkey, Pubkey>,
) -> Option<Pubkey> {
    let pool_pubkey = Pubkey::from_str(&detected_pool.pool_amm_id).ok()?;
    let base_mint = Pubkey::from_str(&detected_pool.base_mint).ok()?;
    resolved_pool_base_mints.insert(pool_pubkey, base_mint);
    Some(pool_pubkey)
}

fn forward_transaction(
    snapshot_engine: &SnapshotEngine,
    forward_mode: SnapshotListenerForwardMode,
    approved_pools: &ApprovedPools,
    ack_tx: Option<&tokio::sync::mpsc::Sender<String>>,
    pool_tx: &PoolTransaction,
    pool_pubkey: Pubkey,
    base_mint: Pubkey,
    signer: Pubkey,
) {
    if !pool_tx.success {
        debug!(
            "SnapshotListener: Skipping failed tx - base_mint={}, pool={}, signature={}, error_code={:?}",
            base_mint,
            pool_tx.pool_amm_id,
            pool_tx.signature,
            pool_tx.error_code
        );
        increment_counter!("ghost_pipeline_stage_total", "stage" => "listener_skipped_failed");
        pipeline_coverage().increment(PipelineCoverageStage::ListenerReceived, 1);
        return;
    }

    // Pool lifecycle is owned by Seer/OracleRuntime. Re-activating here on every TX
    // recreates removed pools after terminal verdicts and restarts their counters.
    increment_counter!("ghost_pipeline_stage_total", "stage" => "listener_forwarded");
    pipeline_coverage().increment(PipelineCoverageStage::ListenerForwarded, 1);
    let is_approved = approved_pools.is_approved(&pool_pubkey);
    let data_source = match forward_mode {
        SnapshotListenerForwardMode::ApprovedOnly if is_raw_chain_semantic(pool_tx) => {
            DataSource::HardTruth
        }
        SnapshotListenerForwardMode::TrackedBuffered
            if is_approved && is_raw_chain_semantic(pool_tx) =>
        {
            DataSource::HardTruth
        }
        _ => DataSource::SoftTruth,
    };
    let tx_event = build_tx_event(pool_tx, pool_pubkey, base_mint, signer, data_source);
    snapshot_engine.handle_tx_event(&tx_event);

    if let Some(tx) = ack_tx {
        let _ = tx.try_send(pool_tx.signature.clone());
    }

    debug!(
        "SnapshotListener: Forwarded tx - base_mint={}, pool={}, is_buy={}, volume={:.4} SOL, mode={:?}",
        base_mint,
        pool_tx.pool_amm_id,
        pool_tx.is_buy,
        pool_tx.volume_sol,
        forward_mode
    );
}

fn replay_staged_transactions_for_pool(
    snapshot_engine: &SnapshotEngine,
    approved_pools: &ApprovedPools,
    pool_identities: &PoolIdentityRegistry,
    forward_mode: SnapshotListenerForwardMode,
    max_pools: usize,
    ack_tx: Option<&tokio::sync::mpsc::Sender<String>>,
    resolved_pool_base_mints: &HashMap<Pubkey, Pubkey>,
    staged_transactions: &mut HashMap<Pubkey, VecDeque<StagedPoolTransaction>>,
    staged_total: &mut usize,
    pool_pubkey: Pubkey,
) -> u64 {
    let Some(mut pending) = staged_transactions.remove(&pool_pubkey) else {
        return 0;
    };
    *staged_total = staged_total.saturating_sub(pending.len());

    let mut replayed = 0u64;
    let mut still_pending = VecDeque::new();

    while let Some(staged) = pending.pop_front() {
        let base_mint = resolve_base_mint(
            staged.tx.as_ref(),
            &pool_pubkey,
            pool_identities,
            resolved_pool_base_mints,
        );
        let authoritative_identity = has_authoritative_pool_identity(
            &pool_pubkey,
            pool_identities,
            resolved_pool_base_mints,
        );

        let stage_reason = match forward_mode {
            SnapshotListenerForwardMode::TrackedBuffered if !authoritative_identity => {
                Some("unknown_pool")
            }
            _ if base_mint.is_none() => Some("unresolved_identity"),
            _ => None,
        };

        if let Some(reason) = stage_reason {
            still_pending.push_back(staged);
            debug!(
                pool = %pool_pubkey,
                reason,
                "SnapshotListener: staged tx remains pending after replay attempt"
            );
            continue;
        }

        let Some(base_mint) = base_mint else {
            still_pending.push_back(staged);
            continue;
        };

        let is_approved = approved_pools.is_approved(&pool_pubkey);
        let should_forward = match forward_mode {
            SnapshotListenerForwardMode::None => false,
            SnapshotListenerForwardMode::TrackedBuffered => true,
            SnapshotListenerForwardMode::ApprovedOnly => is_approved,
            SnapshotListenerForwardMode::Provisional => {
                snapshot_engine.active_pool_count() <= max_pools
            }
        };

        if should_forward {
            pipeline_coverage().increment(PipelineCoverageStage::PendingMappingReplayed, 1);
            increment_counter!("ghost_pipeline_stage_total", "stage" => "listener_replayed");
            forward_transaction(
                snapshot_engine,
                forward_mode,
                approved_pools,
                ack_tx,
                staged.tx.as_ref(),
                pool_pubkey,
                base_mint,
                staged.signer,
            );
            replayed += 1;
        } else {
            still_pending.push_back(staged);
        }
    }

    if !still_pending.is_empty() {
        *staged_total += still_pending.len();
        staged_transactions.insert(pool_pubkey, still_pending);
    }

    replayed
}

/// Run the Snapshot Listener component
///
/// This component subscribes to the event bus and processes PoolTransaction events,
/// converting them to TxEvents and feeding them to SnapshotEngine only.
///
/// `ack_tx` is an optional test-only channel: after each forwarded TX the
/// signature is sent through it so tests can get hard confirmation of event
/// ingestion without relying on sleep/poll.
pub async fn run(
    snapshot_engine: Arc<SnapshotEngine>,
    approved_pools: Arc<ApprovedPools>,
    pool_identities: Arc<PoolIdentityRegistry>,
    mut shutdown_rx: broadcast::Receiver<()>,
    mut event_bus_rx: EventBusReceiver,
    forward_mode: SnapshotListenerForwardMode,
    max_pools: usize,
    staging_config: SnapshotStagingConfig,
    ack_tx: Option<tokio::sync::mpsc::Sender<String>>,
) -> Result<()> {
    info!("SnapshotListener: Initializing component");
    info!("  Forward mode: {:?}", forward_mode);
    info!("  Max pools guard: {}", max_pools);

    let mut events_processed = 0u64;
    let mut events_forwarded = 0u64;
    let mut events_filtered = 0u64;
    let mut events_lagged = 0u64;
    let mut resolved_pool_base_mints = HashMap::new();
    let mut staged_transactions: HashMap<Pubkey, VecDeque<StagedPoolTransaction>> = HashMap::new();
    let mut staged_total = 0usize;
    let mut last_log = std::time::Instant::now();
    let mut prune_interval = tokio::time::interval(staging_config.prune_interval);

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                let prune_result = prune_staged_transactions(
                    &mut staged_transactions,
                    &mut staged_total,
                    staging_config.ttl,
                    Instant::now(),
                );
                record_staged_transaction_expired(prune_result.expired_transactions);
                info!("SnapshotListener: Shutdown signal received");
                info!("  Total events processed: {}, forwarded: {}, staged_remaining={}", events_processed, events_forwarded, staged_total);
                break;
            }
            _ = prune_interval.tick() => {
                let prune_result = prune_staged_transactions(
                    &mut staged_transactions,
                    &mut staged_total,
                    staging_config.ttl,
                    Instant::now(),
                );
                record_staged_transaction_expired(prune_result.expired_transactions);
                if prune_result.expired_pools > 0 {
                    debug!(
                        expired_pools = prune_result.expired_pools,
                        expired_transactions = prune_result.expired_transactions,
                        staged_total,
                        "SnapshotListener: pruned expired staged transactions"
                    );
                }
            }
            event = event_bus_rx.recv() => {
                match event {
                    Ok(ghost_event) => {
                        match ghost_event {
                            GhostEvent::PoolTransaction(pool_tx) => {
                                increment_counter!("ghost_pipeline_stage_total", "stage" => "listener_received");
                                pipeline_coverage().increment(PipelineCoverageStage::ListenerReceived, 1);
                                let pool_pubkey = Pubkey::from_str(&pool_tx.pool_amm_id);
                                let signer = Pubkey::from_str(&pool_tx.signer);

                                match (pool_pubkey, signer) {
                                    (Ok(pool_pubkey), Ok(signer)) => {
                                        let base_mint = resolve_base_mint(
                                            &pool_tx,
                                            &pool_pubkey,
                                            &pool_identities,
                                            &resolved_pool_base_mints,
                                        );
                                        let known_pool = has_authoritative_pool_identity(
                                            &pool_pubkey,
                                            &pool_identities,
                                            &resolved_pool_base_mints,
                                        );

                                        let is_approved = approved_pools.is_approved(&pool_pubkey);
                                        let stage_reason = match forward_mode {
                                            SnapshotListenerForwardMode::TrackedBuffered if !known_pool => {
                                                Some("unknown_pool")
                                            }
                                            _ if base_mint.is_none() => Some("unresolved_identity"),
                                            _ => None,
                                        };
                                        let reject_reason = match forward_mode {
                                            SnapshotListenerForwardMode::None => Some("mode_none"),
                                            SnapshotListenerForwardMode::ApprovedOnly if !is_approved => Some("not_approved"),
                                            SnapshotListenerForwardMode::Provisional
                                                if snapshot_engine.active_pool_count() > max_pools =>
                                            {
                                                Some("max_pools_guard")
                                            }
                                            _ => None,
                                        };

                                        let should_forward = match forward_mode {
                                            SnapshotListenerForwardMode::None => false,
                                            SnapshotListenerForwardMode::TrackedBuffered => known_pool,
                                            SnapshotListenerForwardMode::ApprovedOnly => is_approved,
                                            SnapshotListenerForwardMode::Provisional => {
                                                // Memory guard: cap active pools
                                                if snapshot_engine.active_pool_count() > max_pools {
                                                    warn!(
                                                        "SnapshotListener: Pool count {} exceeds max {}, dropping provisional tx for pool={}",
                                                        snapshot_engine.active_pool_count(),
                                                        max_pools,
                                                        pool_pubkey
                                                    );
                                                    false
                                                } else {
                                                    true
                                                }
                                            }
                                        };

                                        events_processed += 1;

                                        if let Some(reason) = stage_reason {
                                            let insert_result = stage_transaction(
                                                &mut staged_transactions,
                                                &mut staged_total,
                                                staging_config,
                                                pool_pubkey,
                                                signer,
                                                Arc::clone(&pool_tx),
                                                reason,
                                            );
                                            record_staged_transaction_evictions(
                                                insert_result.evicted_per_pool,
                                                insert_result.evicted_global,
                                            );
                                        } else if should_forward {
                                            let Some(base_mint) = base_mint else {
                                                let insert_result = stage_transaction(
                                                    &mut staged_transactions,
                                                    &mut staged_total,
                                                    staging_config,
                                                    pool_pubkey,
                                                    signer,
                                                    Arc::clone(&pool_tx),
                                                    "unresolved_identity",
                                                );
                                                record_staged_transaction_evictions(
                                                    insert_result.evicted_per_pool,
                                                    insert_result.evicted_global,
                                                );
                                                continue;
                                            };
                                            forward_transaction(
                                                snapshot_engine.as_ref(),
                                                forward_mode,
                                                approved_pools.as_ref(),
                                                ack_tx.as_ref(),
                                                pool_tx.as_ref(),
                                                pool_pubkey,
                                                base_mint,
                                                signer,
                                            );
                                            events_forwarded += 1;
                                        } else {
                                            let base_mint = base_mint
                                                .map(|mint| mint.to_string())
                                                .unwrap_or_else(|| "<unresolved>".to_string());
                                            events_filtered += 1;
                                            pipeline_coverage().increment(PipelineCoverageStage::ListenerFiltered, 1);
                                            increment_counter!(
                                                "ghost_pipeline_stage_total",
                                                "stage" => "listener_filtered",
                                                "reason" => reject_reason.unwrap_or("filtered")
                                            );
                                            debug!(
                                                "SnapshotListener: Ignored tx (mode={:?}) - base_mint={}, pool={}, is_buy={}, volume={:.4} SOL",
                                                forward_mode,
                                                base_mint,
                                                pool_tx.pool_amm_id,
                                                pool_tx.is_buy,
                                                pool_tx.volume_sol
                                            );
                                        }

                                        if last_log.elapsed().as_secs() >= 60 {
                                            let elapsed_secs = last_log.elapsed().as_secs().max(1);
                                            info!(
                                                "SnapshotListener: Stats - events={}, forwarded={}, filtered={}, lagged={}, staged_total={}, staged_pools={}, rate={:.1}/min, mode={:?}",
                                                events_processed,
                                                events_forwarded,
                                                events_filtered,
                                                events_lagged,
                                                staged_total,
                                                staged_transactions.len(),
                                                events_processed as f64 / (elapsed_secs as f64 / 60.0),
                                                forward_mode
                                            );
                                            last_log = std::time::Instant::now();
                                        }
                                    }
                                    _ => {
                                        events_filtered += 1;
                                        pipeline_coverage().increment(PipelineCoverageStage::ListenerFiltered, 1);
                                        increment_counter!(
                                            "ghost_pipeline_stage_total",
                                            "stage" => "listener_filtered",
                                            "reason" => "pubkey_parse"
                                        );
                                        error!(
                                            "SnapshotListener: Failed to parse pubkeys - pool={}, signer={}, token_mint={:?}",
                                            pool_tx.pool_amm_id,
                                            pool_tx.signer,
                                            pool_tx.token_mint
                                        );
                                    }
                                }
                            }
                            GhostEvent::NewPoolDetected(detected_pool) => {
                                if let Some(pool_pubkey) = maybe_cache_detected_pool_identity(
                                    detected_pool.as_ref(),
                                    &mut resolved_pool_base_mints,
                                ) {
                                    // Pool lifecycle is activated on authoritative detection events,
                                    // not on every tx, so buffered replays can ingest without
                                    // re-creating removed pools from stray traffic.
                                    snapshot_engine.mark_pool_active(pool_pubkey);
                                    events_forwarded += replay_staged_transactions_for_pool(
                                        snapshot_engine.as_ref(),
                                        approved_pools.as_ref(),
                                        pool_identities.as_ref(),
                                        forward_mode,
                                        max_pools,
                                        ack_tx.as_ref(),
                                        &resolved_pool_base_mints,
                                        &mut staged_transactions,
                                        &mut staged_total,
                                        pool_pubkey,
                                    );
                                }
                            }
                            GhostEvent::GatekeeperCommitted {
                                pool_amm_id,
                                base_mint,
                                ..
                            } => {
                                let resolved_pool = Pubkey::from_str(&pool_amm_id)
                                    .ok()
                                    .or_else(|| {
                                        Pubkey::from_str(&base_mint)
                                            .ok()
                                            .and_then(|mint| pool_identities.get_by_base_mint(&mint))
                                            .map(|identity| identity.pool_id.into())
                                    });

                                if let Some(pool_id) = resolved_pool {
                                    if let Ok(base_mint_pubkey) = Pubkey::from_str(&base_mint) {
                                        resolved_pool_base_mints.insert(pool_id, base_mint_pubkey);
                                    }
                                    approved_pools.insert(pool_id);
                                    snapshot_engine.mark_pool_committed(pool_id);
                                    events_forwarded += replay_staged_transactions_for_pool(
                                        snapshot_engine.as_ref(),
                                        approved_pools.as_ref(),
                                        pool_identities.as_ref(),
                                        forward_mode,
                                        max_pools,
                                        ack_tx.as_ref(),
                                        &resolved_pool_base_mints,
                                        &mut staged_transactions,
                                        &mut staged_total,
                                        pool_id,
                                    );
                                    debug!(
                                        pool = %pool_id,
                                        base_mint,
                                        "SnapshotListener: processed GatekeeperCommitted"
                                    );
                                } else {
                                    warn!(
                                        "SnapshotListener: could not resolve committed pool for base_mint={} pool_amm_id={}",
                                        base_mint,
                                        pool_amm_id
                                    );
                                }
                            }
                            _ => {
                                // Ignore other event types
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        events_lagged = events_lagged.saturating_add(skipped as u64);
                        pipeline_coverage().increment(PipelineCoverageStage::ListenerLagged, skipped as u64);
                        ::metrics::counter!("ghost_pipeline_stage_total", skipped as u64, "stage" => "listener_lagged");
                        ::metrics::counter!("ghost_pipeline_listener_lagged_skipped_total", skipped as u64);
                        warn!("SnapshotListener: Lagged on event bus, skipped {} events", skipped);
                    }
                    Err(e) => {
                        error!("SnapshotListener: Error receiving event: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::gatekeeper_commit_loop::{self, GatekeeperCommitLoopConfig};
    use crate::events::create_event_bus;
    use crate::events::RawBytesMissingReason;
    use ghost_brain::oracle::snapshot_engine::InitPoolEvent;
    use ghost_core::shadow_ledger::{BufferedTx, LivePipeline, TradeSide, TxKey};
    use ghost_core::{
        pipeline_coverage, BaseMint, BondingCurveKey, PoolId, PoolIdentity, PoolIdentityRegistry,
    };
    use solana_sdk::pubkey::Pubkey;
    use std::time::Duration;

    fn test_staging_config() -> SnapshotStagingConfig {
        SnapshotStagingConfig::new(1024, 5_000, 500)
    }

    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
    }

    fn coverage_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn make_identity_registry(pool_id: Pubkey, base_mint: Pubkey) -> Arc<PoolIdentityRegistry> {
        let registry = Arc::new(PoolIdentityRegistry::new());
        registry.register(PoolIdentity {
            pool_id: PoolId::from(pool_id),
            base_mint: BaseMint::from(base_mint),
            bonding_curve: BondingCurveKey::from(pool_id),
        });
        registry
    }

    /// Helper: build a PoolTransaction with real Pubkeys.
    fn make_pool_tx(
        pool_id: Pubkey,
        base_mint: Pubkey,
        signer: Pubkey,
        sig: &str,
    ) -> PoolTransaction {
        PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_id.to_string(),
            slot: Some(12345),
            event_ordinal: Some(0),
            tx_index: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_700_000_000_000,
            event_time: ghost_core::EventTimeMetadata::new(
                Some(1_700_000_000_000),
                Some(1_700_000_000_025),
                Some(50),
            ),
            arrival_ts_ms: 1_700_000_000_050,
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: 1.5,
            sol_amount_lamports: Some(1_500_000_000),
            token_amount_units: Some(15_000_000),
            reserve_base: Some(1000.0),
            reserve_quote: Some(100.0),
            price_quote: Some(0.1),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: sig.to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![1, 2, 3],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: Some(base_mint.to_string()),
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
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
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: ghost_core::CurveFinality::Speculative,
        }
    }

    fn make_detected_pool(pool_id: Pubkey, base_mint: Pubkey) -> DetectedPool {
        DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: base_mint.to_string(),
            quote_mint: Pubkey::new_unique().to_string(),
            amm_program: Pubkey::new_unique().to_string(),
            bonding_curve: pool_id.to_string(),
            creator: Pubkey::new_unique().to_string(),
            slot: Some(12345),
            tx_index: None,
            timestamp_ms: 1_700_000_000_100,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_700_000_000_120),
            initial_liquidity_sol: Some(42.0),
            signature: format!("detected-{pool_id}"),
        }
    }

    // ── Unit test: build_tx_event produces correct TxEvent ──────────

    #[test]
    fn test_build_tx_event_mapping() {
        init_tracing();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let ptx = make_pool_tx(pool, mint, signer, "sig_unit");

        let ev = build_tx_event(&ptx, pool, mint, signer, DataSource::SoftTruth);

        assert_eq!(ev.pool_amm_id, pool);
        assert_eq!(ev.base_mint, mint);
        assert_eq!(ev.signer, signer);
        assert!(ev.is_buy);
        assert!((ev.volume_sol - 1.5).abs() < f64::EPSILON);
        assert_eq!(ev.pool_state, PoolLifecycle::Active);
        assert_eq!(ev.signature, Some("sig_unit".to_string()));
        assert!(ev.raw_data.is_some()); // mpcf_payload was non-empty
    }

    #[test]
    fn test_build_tx_event_empty_mpcf_yields_none() {
        init_tracing();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let mut ptx = make_pool_tx(pool, mint, signer, "sig_empty");
        ptx.mpcf_payload = vec![];
        ptx.mpcf_payload_missing_reason = RawBytesMissingReason::ProviderDoesNotSupport;

        let ev = build_tx_event(&ptx, pool, mint, signer, DataSource::SoftTruth);
        assert!(ev.raw_data.is_none());
    }

    #[test]
    fn snapshot_listener_keeps_failed_trade_status_from_trade_event() {
        init_tracing();
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let mut ptx = make_pool_tx(pool, mint, signer, "sig_failed");
        ptx.success = false;
        ptx.error_code = Some("InstructionError(Custom(1))".to_string());

        let snapshot_engine = SnapshotEngine::new(128, 200);
        let approved_pools = ApprovedPools::new();
        snapshot_engine.mark_pool_active(pool);
        snapshot_engine.handle_initialize_pool_event(&InitPoolEvent {
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint: Pubkey::new_unique(),
            slot: Some(12345),
            timestamp_ms: 1_700_000_000_000,
            initial_liquidity_sol: 80.0,
            initial_reserve_base: 1_000_000.0,
            initial_reserve_quote: 100.0,
            initial_price_quote: 0.0001,
        });

        forward_transaction(
            &snapshot_engine,
            SnapshotListenerForwardMode::Provisional,
            &approved_pools,
            None,
            &ptx,
            pool,
            mint,
            signer,
        );

        assert!(
            snapshot_engine.get_transactions(&pool).is_empty(),
            "failed trade must not be forwarded to SnapshotEngine as landed tx"
        );
    }

    // ── Integration: event bus → listener → ACK (hard proof) ────────

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_snapshot_listener_provisional_ack() {
        init_tracing();

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = Arc::new(PoolIdentityRegistry::new());

        // 1. Bootstrap: mark active + init pool so engine accepts TxEvents
        snapshot_engine.mark_pool_active(pool_id);
        snapshot_engine.handle_initialize_pool_event(&InitPoolEvent {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint: Pubkey::new_unique(),
            slot: Some(100),
            timestamp_ms: 1_700_000_000_000,
            initial_liquidity_sol: 80.0,
            initial_reserve_base: 1000.0,
            initial_reserve_quote: 80.0,
            initial_price_quote: 0.00008,
        });

        // 2. Create ACK channel
        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);

        // 3. Start listener
        let (event_tx, event_rx) = create_event_bus();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let engine_clone = Arc::clone(&snapshot_engine);
        let ap_clone = Arc::clone(&approved_pools);
        let _handle = tokio::spawn(async move {
            run(
                engine_clone,
                ap_clone,
                pool_identities,
                shutdown_rx,
                event_rx,
                SnapshotListenerForwardMode::Provisional,
                500,
                test_staging_config(),
                Some(ack_tx),
            )
            .await
        });

        // 4. Send PoolTransaction
        let ptx = make_pool_tx(pool_id, base_mint, signer, "sig_ack_test");
        let receivers = event_tx
            .send(GhostEvent::pool_transaction(ptx))
            .expect("send must succeed");
        assert!(receivers >= 1, "at least 1 receiver");

        // 5. Hard ACK: wait for listener to confirm processing
        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv()).await;
        assert!(
            ack.is_ok(),
            "SnapshotListener did not ACK within 2 s – event never reached listener"
        );
        assert_eq!(ack.unwrap().unwrap(), "sig_ack_test");

        // 6. Hard assert: SnapshotEngine ingested the tx
        let txs = snapshot_engine.get_transactions(&pool_id);
        assert!(
            !txs.is_empty(),
            "SnapshotEngine must have ingested at least 1 tx (got {})",
            txs.len()
        );

        _handle.abort();
    }

    // ── Integration: approved_only mode only forwards approved pools ───

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_snapshot_listener_approved_only_ignores_unknown() {
        init_tracing();

        let approved_pool = Pubkey::new_unique();
        let unapproved_pool = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = make_identity_registry(approved_pool, base_mint);

        // Only bootstrap + approve one pool
        snapshot_engine.mark_pool_active(approved_pool);
        snapshot_engine.handle_initialize_pool_event(&InitPoolEvent {
            pool_amm_id: approved_pool,
            base_mint,
            quote_mint: Pubkey::new_unique(),
            slot: Some(100),
            timestamp_ms: 1_700_000_000_000,
            initial_liquidity_sol: 80.0,
            initial_reserve_base: 1000.0,
            initial_reserve_quote: 80.0,
            initial_price_quote: 0.00008,
        });
        approved_pools.insert(approved_pool); // <-- canonical approval

        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let engine_clone = Arc::clone(&snapshot_engine);
        let ap_clone = Arc::clone(&approved_pools);
        let _handle = tokio::spawn(async move {
            run(
                engine_clone,
                ap_clone,
                pool_identities,
                shutdown_rx,
                event_rx,
                SnapshotListenerForwardMode::ApprovedOnly,
                500,
                test_staging_config(),
                Some(ack_tx),
            )
            .await
        });

        // Send TX for unapproved pool – should NOT be forwarded
        let ptx_unknown = make_pool_tx(unapproved_pool, base_mint, signer, "sig_unknown");
        event_tx
            .send(GhostEvent::pool_transaction(ptx_unknown))
            .unwrap();

        // Send TX for approved pool – SHOULD be forwarded
        let ptx_approved = make_pool_tx(approved_pool, base_mint, signer, "sig_approved");
        event_tx
            .send(GhostEvent::pool_transaction(ptx_approved))
            .unwrap();

        // We should only get the ACK for the approved pool
        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv()).await;
        assert!(ack.is_ok(), "Expected ACK for approved pool");
        assert_eq!(ack.unwrap().unwrap(), "sig_approved");

        // Engine should have transactions for the approved pool
        let txs = snapshot_engine.get_transactions(&approved_pool);
        assert!(
            !txs.is_empty(),
            "approved pool must have ingested tx (got {})",
            txs.len()
        );

        _handle.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_snapshot_listener_tracked_buffered_resolves_registry_base_mint() {
        init_tracing();

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = make_identity_registry(pool_id, base_mint);

        snapshot_engine.mark_pool_active(pool_id);
        snapshot_engine.handle_initialize_pool_event(&InitPoolEvent {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint: Pubkey::new_unique(),
            slot: Some(100),
            timestamp_ms: 1_700_000_000_000,
            initial_liquidity_sol: 80.0,
            initial_reserve_base: 1000.0,
            initial_reserve_quote: 80.0,
            initial_price_quote: 0.00008,
        });

        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let engine_clone = Arc::clone(&snapshot_engine);
        let ap_clone = Arc::clone(&approved_pools);
        let ids_clone = Arc::clone(&pool_identities);
        let _handle = tokio::spawn(async move {
            run(
                engine_clone,
                ap_clone,
                ids_clone,
                shutdown_rx,
                event_rx,
                SnapshotListenerForwardMode::TrackedBuffered,
                500,
                test_staging_config(),
                Some(ack_tx),
            )
            .await
        });

        let mut ptx = make_pool_tx(pool_id, base_mint, signer, "sig_tracked_buffered");
        ptx.token_mint = None;
        event_tx.send(GhostEvent::pool_transaction(ptx)).unwrap();

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv()).await;
        assert!(ack.is_ok(), "Expected ACK for tracked buffered pool");
        assert_eq!(ack.unwrap().unwrap(), "sig_tracked_buffered");

        let buffered = snapshot_engine.get_transactions(&pool_id);
        assert!(
            !buffered.is_empty(),
            "known tracked pool should ingest when lifecycle state already exists"
        );

        _handle.abort();
    }

    #[test]
    fn forward_transaction_does_not_reactivate_removed_pool() {
        init_tracing();

        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();
        let snapshot_engine = SnapshotEngine::new(128, 200);
        let approved_pools = ApprovedPools::new();

        snapshot_engine.mark_pool_active(pool);
        snapshot_engine.handle_initialize_pool_event(&InitPoolEvent {
            pool_amm_id: pool,
            base_mint: mint,
            quote_mint: Pubkey::new_unique(),
            slot: Some(123),
            timestamp_ms: 1_700_000_000_000,
            initial_liquidity_sol: 42.0,
            initial_reserve_base: 1_000_000.0,
            initial_reserve_quote: 42.0,
            initial_price_quote: 0.000042,
        });

        let first = make_pool_tx(pool, mint, signer, "sig_before_remove");
        forward_transaction(
            &snapshot_engine,
            SnapshotListenerForwardMode::Provisional,
            &approved_pools,
            None,
            &first,
            pool,
            mint,
            signer,
        );
        assert!(
            !snapshot_engine.get_transactions(&pool).is_empty(),
            "pre-remove tx must reach SnapshotEngine"
        );

        snapshot_engine.remove_pool(pool);
        assert!(
            !snapshot_engine.has_pool(&pool),
            "pool must be removed before replay regression check"
        );

        let second = make_pool_tx(pool, mint, signer, "sig_after_remove");
        forward_transaction(
            &snapshot_engine,
            SnapshotListenerForwardMode::Provisional,
            &approved_pools,
            None,
            &second,
            pool,
            mint,
            signer,
        );

        assert!(
            !snapshot_engine.has_pool(&pool),
            "listener must not recreate a pool that runtime removed"
        );
        assert!(
            snapshot_engine.get_transactions(&pool).is_empty(),
            "removed pool must not expose a fresh snapshot/tx history after forward"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracked_buffered_unknown_pool_is_buffered() {
        init_tracing();
        let _coverage_guard = coverage_test_lock().lock().await;

        let coverage_before = pipeline_coverage().snapshot();
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = Arc::new(PoolIdentityRegistry::new());

        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let listener_handle = tokio::spawn({
            let engine_clone = Arc::clone(&snapshot_engine);
            let approved_clone = Arc::clone(&approved_pools);
            let identities_clone = Arc::clone(&pool_identities);
            async move {
                run(
                    engine_clone,
                    approved_clone,
                    identities_clone,
                    shutdown_rx,
                    event_rx,
                    SnapshotListenerForwardMode::TrackedBuffered,
                    500,
                    test_staging_config(),
                    Some(ack_tx),
                )
                .await
            }
        });

        event_tx
            .send(GhostEvent::pool_transaction(make_pool_tx(
                pool_id,
                base_mint,
                signer,
                "sig_unknown_buffered",
            )))
            .expect("tx send");

        let no_ack = tokio::time::timeout(Duration::from_millis(200), ack_rx.recv()).await;
        assert!(
            no_ack.is_err(),
            "unknown pool must stage first instead of forwarding immediately"
        );
        assert!(
            snapshot_engine.get_transactions(&pool_id).is_empty(),
            "staged unknown pool must not reach SnapshotEngine before mapping"
        );

        event_tx
            .send(GhostEvent::new_pool_detected(make_detected_pool(
                pool_id, base_mint,
            )))
            .expect("detected pool send");

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv())
            .await
            .expect("staged tx replay ack timeout")
            .expect("ack channel closed");
        assert_eq!(ack, "sig_unknown_buffered");
        assert!(
            !snapshot_engine.get_transactions(&pool_id).is_empty(),
            "staged unknown pool should replay into SnapshotEngine after mapping"
        );

        let delta = pipeline_coverage()
            .snapshot()
            .saturating_delta_from(&coverage_before);
        assert!(
            delta.pending_mapping_buffered >= 1,
            "default-mint path should buffer at least one pending mapping replay"
        );
        assert!(
            delta.pending_mapping_replayed >= 1,
            "tracked-buffered unknown pool should replay at least once after mapping"
        );

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), listener_handle)
            .await
            .expect("listener join timeout")
            .expect("listener join failed")
            .expect("listener returned error");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracked_buffered_unknown_pool_expires_before_replay() {
        init_tracing();

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = Arc::new(PoolIdentityRegistry::new());

        let staging_config = SnapshotStagingConfig::new(8, 25, 4);
        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let listener_handle = tokio::spawn({
            let engine_clone = Arc::clone(&snapshot_engine);
            let approved_clone = Arc::clone(&approved_pools);
            let identities_clone = Arc::clone(&pool_identities);
            async move {
                run(
                    engine_clone,
                    approved_clone,
                    identities_clone,
                    shutdown_rx,
                    event_rx,
                    SnapshotListenerForwardMode::TrackedBuffered,
                    4,
                    staging_config,
                    Some(ack_tx),
                )
                .await
            }
        });

        event_tx
            .send(GhostEvent::pool_transaction(make_pool_tx(
                pool_id,
                base_mint,
                signer,
                "sig_expired_before_replay",
            )))
            .expect("tx send");

        tokio::time::sleep(Duration::from_millis(80)).await;

        event_tx
            .send(GhostEvent::new_pool_detected(make_detected_pool(
                pool_id, base_mint,
            )))
            .expect("detected pool send");

        let no_ack = tokio::time::timeout(Duration::from_millis(200), ack_rx.recv()).await;
        assert!(
            no_ack.is_err(),
            "expired staged tx must not replay after TTL"
        );
        assert!(
            snapshot_engine.get_transactions(&pool_id).is_empty(),
            "expired staged tx must never reach SnapshotEngine"
        );

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), listener_handle)
            .await
            .expect("listener join timeout")
            .expect("listener join failed")
            .expect("listener returned error");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracked_buffered_unknown_pool_respects_per_pool_cap() {
        init_tracing();

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = Arc::new(PoolIdentityRegistry::new());

        let staging_config = SnapshotStagingConfig::new(1, 5_000, 4);
        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let listener_handle = tokio::spawn({
            let engine_clone = Arc::clone(&snapshot_engine);
            let approved_clone = Arc::clone(&approved_pools);
            let identities_clone = Arc::clone(&pool_identities);
            async move {
                run(
                    engine_clone,
                    approved_clone,
                    identities_clone,
                    shutdown_rx,
                    event_rx,
                    SnapshotListenerForwardMode::TrackedBuffered,
                    4,
                    staging_config,
                    Some(ack_tx),
                )
                .await
            }
        });

        event_tx
            .send(GhostEvent::pool_transaction(make_pool_tx(
                pool_id,
                base_mint,
                signer,
                "sig_cap_old",
            )))
            .expect("tx1 send");
        event_tx
            .send(GhostEvent::pool_transaction(make_pool_tx(
                pool_id,
                base_mint,
                signer,
                "sig_cap_new",
            )))
            .expect("tx2 send");

        event_tx
            .send(GhostEvent::new_pool_detected(make_detected_pool(
                pool_id, base_mint,
            )))
            .expect("detected pool send");

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv())
            .await
            .expect("cap replay ack timeout")
            .expect("ack channel closed");
        assert_eq!(
            ack, "sig_cap_new",
            "newest staged tx should survive per-pool cap eviction"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), ack_rx.recv())
                .await
                .is_err()
        );

        let ingested = snapshot_engine.get_transactions(&pool_id);
        assert_eq!(ingested.len(), 1, "only one tx should survive per-pool cap");
        assert_eq!(ingested[0].signature, "sig_cap_new");

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), listener_handle)
            .await
            .expect("listener join timeout")
            .expect("listener join failed")
            .expect("listener returned error");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn default_mint_never_forwarded() {
        init_tracing();
        let _coverage_guard = coverage_test_lock().lock().await;

        let coverage_before = pipeline_coverage().snapshot();
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = Arc::new(PoolIdentityRegistry::new());

        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let listener_handle = tokio::spawn({
            let engine_clone = Arc::clone(&snapshot_engine);
            let approved_clone = Arc::clone(&approved_pools);
            let identities_clone = Arc::clone(&pool_identities);
            async move {
                run(
                    engine_clone,
                    approved_clone,
                    identities_clone,
                    shutdown_rx,
                    event_rx,
                    SnapshotListenerForwardMode::Provisional,
                    500,
                    test_staging_config(),
                    Some(ack_tx),
                )
                .await
            }
        });

        let mut tx = make_pool_tx(pool_id, base_mint, signer, "sig_default_mint");
        tx.token_mint = Some(Pubkey::default().to_string());
        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("tx send");

        let no_ack = tokio::time::timeout(Duration::from_millis(200), ack_rx.recv()).await;
        assert!(
            no_ack.is_err(),
            "default mint must never be forwarded as final identity"
        );
        assert!(
            snapshot_engine.get_transactions(&pool_id).is_empty(),
            "default mint tx must stay out of SnapshotEngine until resolved"
        );

        event_tx
            .send(GhostEvent::new_pool_detected(make_detected_pool(
                pool_id, base_mint,
            )))
            .expect("detected pool send");

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv())
            .await
            .expect("default mint replay ack timeout")
            .expect("ack channel closed");
        assert_eq!(ack, "sig_default_mint");

        let delta = pipeline_coverage()
            .snapshot()
            .saturating_delta_from(&coverage_before);
        assert!(
            delta.pending_mapping_buffered >= 1,
            "default mint replay path may stage more than once before resolution"
        );
        assert!(
            delta.pending_mapping_replayed >= 1,
            "default mint resolution must replay at least once after identity is known"
        );

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), listener_handle)
            .await
            .expect("listener join timeout")
            .expect("listener join failed")
            .expect("listener returned error");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tx_before_pool_identity_is_staged_not_dropped() {
        init_tracing();
        let _coverage_guard = coverage_test_lock().lock().await;

        let coverage_before = pipeline_coverage().snapshot();
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        let pool_identities = Arc::new(PoolIdentityRegistry::new());

        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let listener_handle = tokio::spawn({
            let engine_clone = Arc::clone(&snapshot_engine);
            let approved_clone = Arc::clone(&approved_pools);
            let identities_clone = Arc::clone(&pool_identities);
            async move {
                run(
                    engine_clone,
                    approved_clone,
                    identities_clone,
                    shutdown_rx,
                    event_rx,
                    SnapshotListenerForwardMode::Provisional,
                    500,
                    test_staging_config(),
                    Some(ack_tx),
                )
                .await
            }
        });

        let mut tx = make_pool_tx(pool_id, base_mint, signer, "sig_identity_pending");
        tx.token_mint = None;
        event_tx
            .send(GhostEvent::pool_transaction(tx))
            .expect("tx send");

        let no_ack = tokio::time::timeout(Duration::from_millis(200), ack_rx.recv()).await;
        assert!(
            no_ack.is_err(),
            "tx without resolved identity must stage instead of disappearing"
        );
        assert!(
            snapshot_engine.get_transactions(&pool_id).is_empty(),
            "identity-pending tx must not reach SnapshotEngine before replay"
        );

        event_tx
            .send(GhostEvent::new_pool_detected(make_detected_pool(
                pool_id, base_mint,
            )))
            .expect("detected pool send");

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv())
            .await
            .expect("identity replay ack timeout")
            .expect("ack channel closed");
        assert_eq!(ack, "sig_identity_pending");
        assert!(
            !snapshot_engine.get_transactions(&pool_id).is_empty(),
            "identity-pending tx should replay into SnapshotEngine after mapping"
        );

        let delta = pipeline_coverage()
            .snapshot()
            .saturating_delta_from(&coverage_before);
        assert_eq!(delta.pending_mapping_buffered, 1);
        assert_eq!(delta.pending_mapping_replayed, 1);

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), listener_handle)
            .await
            .expect("listener join timeout")
            .expect("listener join failed")
            .expect("listener returned error");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_snapshot_listener_records_lagged_events_before_recovering() {
        init_tracing();
        let _coverage_guard = coverage_test_lock().lock().await;

        let coverage_before = pipeline_coverage().snapshot();
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        let approved_pools = Arc::new(ApprovedPools::new());
        approved_pools.insert(pool_id);
        let pool_identities = make_identity_registry(pool_id, base_mint);

        snapshot_engine.mark_pool_active(pool_id);
        snapshot_engine.handle_initialize_pool_event(&InitPoolEvent {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint: Pubkey::new_unique(),
            slot: Some(100),
            timestamp_ms: 1_700_000_000_000,
            initial_liquidity_sol: 80.0,
            initial_reserve_base: 1000.0,
            initial_reserve_quote: 80.0,
            initial_price_quote: 0.00008,
        });

        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = tokio::sync::broadcast::channel::<GhostEvent>(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

        event_tx
            .send(GhostEvent::pool_transaction(make_pool_tx(
                pool_id,
                base_mint,
                signer,
                "sig_lagged_1",
            )))
            .unwrap();
        event_tx
            .send(GhostEvent::pool_transaction(make_pool_tx(
                pool_id,
                base_mint,
                signer,
                "sig_lagged_2",
            )))
            .unwrap();
        event_tx
            .send(GhostEvent::pool_transaction(make_pool_tx(
                pool_id,
                base_mint,
                signer,
                "sig_lagged_3",
            )))
            .unwrap();

        let handle = tokio::spawn({
            let engine_clone = Arc::clone(&snapshot_engine);
            let approved_clone = Arc::clone(&approved_pools);
            let ids_clone = Arc::clone(&pool_identities);
            async move {
                run(
                    engine_clone,
                    approved_clone,
                    ids_clone,
                    shutdown_rx,
                    event_rx,
                    SnapshotListenerForwardMode::TrackedBuffered,
                    500,
                    test_staging_config(),
                    Some(ack_tx),
                )
                .await
            }
        });

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv())
            .await
            .expect("lagged listener ack timeout")
            .expect("ack channel closed");
        assert_eq!(
            ack, "sig_lagged_3",
            "listener should recover after lag and process the retained latest event"
        );

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("listener join timeout")
            .expect("listener join failed")
            .expect("listener returned error");

        let delta = pipeline_coverage()
            .snapshot()
            .saturating_delta_from(&coverage_before);
        assert_eq!(delta.listener_lagged, 2);
        assert_eq!(delta.listener_forwarded, 1);
        assert_eq!(delta.listener_received, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_tracked_buffered_commit_loop_and_live_pipeline_reach_full_ledger_coverage() {
        init_tracing();
        let _coverage_guard = coverage_test_lock().lock().await;

        let coverage_before = pipeline_coverage().snapshot();
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let signer = Pubkey::new_unique();

        let shadow_ledger = Arc::new(ghost_core::ShadowLedger::new());
        let live_pipeline = Arc::new(LivePipeline::new());
        #[allow(deprecated)]
        let runtime = Arc::new(crate::oracle_runtime::OracleRuntime::new_with_config(
            Arc::new(ghost_brain::oracle::hyper_prediction::HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::clone(&shadow_ledger),
            None,
            None,
            Arc::clone(&live_pipeline),
            crate::oracle_runtime::OracleRuntimeConfig {
                runtime_shadowledger_snapshots_enabled: false,
                ..crate::oracle_runtime::OracleRuntimeConfig::default()
            },
        ));
        assert!(runtime.register_new_pool(
            pool_id,
            base_mint,
            ghost_brain::fast_pipeline::EnhancedCandidate {
                pool_amm_id: pool_id,
                base_mint,
                bonding_curve: Pubkey::new_unique(),
                slot: Some(200),
                ..Default::default()
            },
            None,
        ));
        runtime.mark_pool_approved(pool_id);
        runtime.approved_pools().insert(pool_id);

        let mut engine = SnapshotEngine::new(128, 50);
        engine.set_shadow_ledger(Arc::clone(&shadow_ledger));
        let snapshot_engine = Arc::new(engine);
        let approved_pools = Arc::new(ApprovedPools::new());
        approved_pools.insert(pool_id);
        snapshot_engine.set_approved_pools(Arc::clone(&approved_pools));
        snapshot_engine.mark_pool_active(pool_id);
        let pool_identities = make_identity_registry(pool_id, base_mint);

        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<String>(16);
        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, _) = broadcast::channel::<()>(4);

        let listener_handle = tokio::spawn({
            let engine_clone = Arc::clone(&snapshot_engine);
            let approved_clone = Arc::clone(&approved_pools);
            let ids_clone = Arc::clone(&pool_identities);
            let shutdown_rx = shutdown_tx.subscribe();
            async move {
                run(
                    engine_clone,
                    approved_clone,
                    ids_clone,
                    shutdown_rx,
                    event_rx,
                    SnapshotListenerForwardMode::TrackedBuffered,
                    500,
                    test_staging_config(),
                    Some(ack_tx),
                )
                .await
            }
        });

        let mut tx1 = make_pool_tx(pool_id, base_mint, signer, "sig_precommit_buffered");
        tx1.slot = Some(200);
        tx1.timestamp_ms = 1_700_000_000_100;
        tx1.arrival_ts_ms = 1_700_000_000_150;
        pipeline_coverage().increment(PipelineCoverageStage::ChainTruth, 1);
        pipeline_coverage().increment(PipelineCoverageStage::GrpcReceived, 1);
        pipeline_coverage().increment(PipelineCoverageStage::ParsedOk, 1);
        pipeline_coverage().increment(PipelineCoverageStage::SeerForwarded, 1);
        event_tx
            .send(GhostEvent::pool_transaction(tx1.clone()))
            .expect("tx1 send");

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv())
            .await
            .expect("tx1 ack timeout")
            .expect("tx1 ack channel closed");
        assert_eq!(ack, "sig_precommit_buffered");
        assert!(
            !snapshot_engine.get_transactions(&pool_id).is_empty(),
            "tracked pool tx should ingest immediately during observation"
        );

        let tx1_key = TxKey::new(tx1.timestamp_ms, tx1.slot, Some(0), None, 0).unwrap();
        runtime.commit_coordinator().stage_history(
            pool_id,
            base_mint,
            100_000_000_000,
            1_000_000_000,
            vec![BufferedTx::new(
                tx1_key,
                TradeSide::Buy,
                tx1.sol_amount_lamports.unwrap(),
                tx1.token_amount_units.unwrap(),
                false,
                Some(signer),
            )
            .unwrap()],
        );

        snapshot_engine.handle_initialize_pool_event(&InitPoolEvent {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint: Pubkey::new_unique(),
            slot: tx1.slot,
            timestamp_ms: tx1.timestamp_ms,
            initial_liquidity_sol: 100.0,
            initial_reserve_base: 1_000.0,
            initial_reserve_quote: 100.0,
            initial_price_quote: 0.1,
        });
        assert!(
            !snapshot_engine.get_transactions(&pool_id).is_empty(),
            "tracked pool should retain ingested tx after init bootstrap"
        );

        let commit_handle = tokio::spawn({
            let shutdown_rx = shutdown_tx.subscribe();
            let runtime = Arc::clone(&runtime);
            let live_pipeline = Arc::clone(&live_pipeline);
            let shadow_ledger = Arc::clone(&shadow_ledger);
            let event_tx = event_tx.clone();
            async move {
                gatekeeper_commit_loop::run(
                    runtime,
                    live_pipeline,
                    shadow_ledger,
                    Some(event_tx),
                    shutdown_rx,
                    GatekeeperCommitLoopConfig {
                        check_interval_ms: 10,
                    },
                )
                .await;
            }
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if shadow_ledger.is_committed(&base_mint)
                    && live_pipeline.is_initialized(&base_mint)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("commit loop did not commit and initialize live pipeline");

        let mut tx2 = make_pool_tx(pool_id, base_mint, signer, "sig_postcommit_live");
        tx2.slot = Some(201);
        tx2.timestamp_ms = tx1.timestamp_ms + 1_000;
        tx2.arrival_ts_ms = tx1.arrival_ts_ms + 1_000;
        tx2.volume_sol = 2.0;
        tx2.sol_amount_lamports = Some(2_000_000_000);
        tx2.token_amount_units = Some(20_000_000);
        tx2.reserve_base = Some(980.0);
        tx2.reserve_quote = Some(102.0);
        pipeline_coverage().increment(PipelineCoverageStage::ChainTruth, 1);
        pipeline_coverage().increment(PipelineCoverageStage::GrpcReceived, 1);
        pipeline_coverage().increment(PipelineCoverageStage::ParsedOk, 1);
        pipeline_coverage().increment(PipelineCoverageStage::SeerForwarded, 1);
        event_tx
            .send(GhostEvent::pool_transaction(tx2.clone()))
            .expect("tx2 send");

        let ack = tokio::time::timeout(Duration::from_secs(2), ack_rx.recv())
            .await
            .expect("tx2 ack timeout")
            .expect("tx2 ack channel closed");
        assert_eq!(ack, "sig_postcommit_live");

        live_pipeline
            .process_event(
                ghost_core::LiveTxEvent::new(
                    base_mint,
                    tx2.slot,
                    Some(1),
                    None,
                    tx2.timestamp_ms,
                    TradeSide::Buy,
                    tx2.sol_amount_lamports.unwrap(),
                    tx2.token_amount_units.unwrap(),
                    false,
                    Some(signer),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            live_pipeline
                .flush_mint(&base_mint, &shadow_ledger)
                .unwrap(),
            1,
            "post-commit live pipeline should append exactly one canonical snapshot"
        );

        let ledger_snaps = shadow_ledger
            .get_snapshots(&base_mint)
            .expect("ledger snapshots must exist after commit");
        assert_eq!(
            ledger_snaps.len(),
            2,
            "ledger should contain one committed snapshot and one live append"
        );

        let coverage_after = pipeline_coverage().snapshot();
        let delta = coverage_after.saturating_delta_from(&coverage_before);
        assert_eq!(delta.chain_truth, 2);
        assert!(
            delta.listener_forwarded >= 2,
            "listener must forward the pre-commit tracked tx and the post-commit live tx; replay may add an extra forward, got {}",
            delta.listener_forwarded
        );
        assert_eq!(delta.shadow_ledger_committed, 1);
        assert_eq!(delta.shadow_ledger_live_appended, 1);
        assert_eq!(delta.shadow_ledger_total(), 2);
        assert!(
            (delta.final_ledger_ratio() - 100.0).abs() < f64::EPSILON,
            "expected ledger_vs_chain=100%, got {:.2}%",
            delta.final_ledger_ratio()
        );

        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(2), listener_handle)
            .await
            .expect("listener join timeout")
            .expect("listener join failed")
            .expect("listener returned error");
        tokio::time::timeout(Duration::from_secs(2), commit_handle)
            .await
            .expect("commit loop join timeout")
            .expect("commit loop join failed");
    }
}
