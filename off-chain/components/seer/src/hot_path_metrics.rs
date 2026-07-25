//! Test-only counters used by the deterministic PR1B ingest harness.
//!
//! The module is compiled only for `seer` unit tests. Production builds do not
//! contain these atomics or the optional synthetic sink delays.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HotPathCounterSnapshot {
    pub live_transaction_prost_encodes: u64,
    pub live_transaction_normalizer_decodes: u64,
    pub live_transaction_parser_decodes: u64,
    pub full_instruction_tree_scans: u64,
    pub wal_append_calls: u64,
    pub wal_blocking_waits: u64,
    pub ipc_blocking_waits: u64,
}

static LIVE_TRANSACTION_PROST_ENCODES: AtomicU64 = AtomicU64::new(0);
static LIVE_TRANSACTION_NORMALIZER_DECODES: AtomicU64 = AtomicU64::new(0);
static LIVE_TRANSACTION_PARSER_DECODES: AtomicU64 = AtomicU64::new(0);
static FULL_INSTRUCTION_TREE_SCANS: AtomicU64 = AtomicU64::new(0);
static WAL_APPEND_CALLS: AtomicU64 = AtomicU64::new(0);
static WAL_BLOCKING_WAITS: AtomicU64 = AtomicU64::new(0);
static IPC_BLOCKING_WAITS: AtomicU64 = AtomicU64::new(0);
static SYNTHETIC_WAL_DELAY_MS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn reset() {
    LIVE_TRANSACTION_PROST_ENCODES.store(0, Ordering::Relaxed);
    LIVE_TRANSACTION_NORMALIZER_DECODES.store(0, Ordering::Relaxed);
    LIVE_TRANSACTION_PARSER_DECODES.store(0, Ordering::Relaxed);
    FULL_INSTRUCTION_TREE_SCANS.store(0, Ordering::Relaxed);
    WAL_APPEND_CALLS.store(0, Ordering::Relaxed);
    WAL_BLOCKING_WAITS.store(0, Ordering::Relaxed);
    IPC_BLOCKING_WAITS.store(0, Ordering::Relaxed);
    SYNTHETIC_WAL_DELAY_MS.store(0, Ordering::Relaxed);
}

pub(crate) fn snapshot() -> HotPathCounterSnapshot {
    HotPathCounterSnapshot {
        live_transaction_prost_encodes: LIVE_TRANSACTION_PROST_ENCODES.load(Ordering::Relaxed),
        live_transaction_normalizer_decodes: LIVE_TRANSACTION_NORMALIZER_DECODES
            .load(Ordering::Relaxed),
        live_transaction_parser_decodes: LIVE_TRANSACTION_PARSER_DECODES.load(Ordering::Relaxed),
        full_instruction_tree_scans: FULL_INSTRUCTION_TREE_SCANS.load(Ordering::Relaxed),
        wal_append_calls: WAL_APPEND_CALLS.load(Ordering::Relaxed),
        wal_blocking_waits: WAL_BLOCKING_WAITS.load(Ordering::Relaxed),
        ipc_blocking_waits: IPC_BLOCKING_WAITS.load(Ordering::Relaxed),
    }
}

#[inline]
pub(crate) fn record_live_transaction_prost_encode() {
    LIVE_TRANSACTION_PROST_ENCODES.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_live_transaction_normalizer_decode() {
    LIVE_TRANSACTION_NORMALIZER_DECODES.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_live_transaction_parser_decode() {
    LIVE_TRANSACTION_PARSER_DECODES.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_full_instruction_tree_scan() {
    FULL_INSTRUCTION_TREE_SCANS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_wal_append() {
    WAL_APPEND_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_ipc_blocking_wait() {
    IPC_BLOCKING_WAITS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn set_synthetic_wal_delay(delay: Duration) {
    SYNTHETIC_WAL_DELAY_MS.store(delay.as_millis() as u64, Ordering::Relaxed);
}

pub(crate) fn apply_synthetic_wal_delay() {
    let delay_ms = SYNTHETIC_WAL_DELAY_MS.load(Ordering::Relaxed);
    if delay_ms == 0 {
        return;
    }
    WAL_BLOCKING_WAITS.fetch_add(1, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(delay_ms));
}
