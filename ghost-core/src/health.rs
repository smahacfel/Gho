//! Runtime Health Monitor
//!
//! Provides `RuntimeHealth` — an `Arc`-wrapped collection of atomic counters
//! that track the last-seen timestamps for every data-plane channel
//! (gRPC, IPC, bus, gatekeeper, JSONL writers) and the current gRPC
//! connection state.
//!
//! All timestamps are **epoch-milliseconds** (13-digit).

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Monotonic epoch-millisecond helper.
#[inline]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Numeric representation of gRPC connection states.
///
/// Mirrors `ConnectionState` in `grpc_connection.rs`:
///
/// | value | state           |
/// |-------|-----------------|
/// | 0     | Disconnected    |
/// | 1     | Connecting      |
/// | 2     | Authenticating  |
/// | 3     | Subscribing     |
/// | 4     | Connected       |
/// | 5     | Reconnecting    |
/// | 6     | Failed          |
pub const GRPC_STATE_DISCONNECTED: u8 = 0;
pub const GRPC_STATE_CONNECTING: u8 = 1;
pub const GRPC_STATE_AUTHENTICATING: u8 = 2;
pub const GRPC_STATE_SUBSCRIBING: u8 = 3;
pub const GRPC_STATE_CONNECTED: u8 = 4;
pub const GRPC_STATE_RECONNECTING: u8 = 5;
pub const GRPC_STATE_FAILED: u8 = 6;

/// Shared, lock-free health snapshot updated by every data-plane component.
///
/// Wrap in `Arc` and pass to gRPC connection, IPC listener, bus consumer,
/// gatekeeper, and JSONL writers so each can call the corresponding `mark_*`
/// helper.
#[derive(Debug)]
pub struct RuntimeHealth {
    // ── data-plane timestamps (epoch-ms) ────────────────────────────
    pub last_grpc_msg_ts_ms: AtomicU64,
    pub last_grpc_progress_ts_ms: AtomicU64,
    pub last_ipc_event_ts_ms: AtomicU64,
    pub last_bus_event_ts_ms: AtomicU64,
    pub last_gatekeeper_decision_ts_ms: AtomicU64,

    // ── writer timestamps (epoch-ms) ────────────────────────────────
    pub last_decisions_write_ts_ms: AtomicU64,
    pub last_buys_write_ts_ms: AtomicU64,
    pub last_events_write_ts_ms: AtomicU64,

    // ── gRPC subscribe proof (epoch-ms) ────────────────────────────
    pub subscribe_sent_ts_ms: AtomicU64,

    // ── gRPC connection meta ────────────────────────────────────────
    pub grpc_state_u8: AtomicU8,
    pub grpc_reconnects: AtomicU32,
}

impl RuntimeHealth {
    /// Create a new `RuntimeHealth` wrapped in `Arc`, ready to be shared.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last_grpc_msg_ts_ms: AtomicU64::new(0),
            last_grpc_progress_ts_ms: AtomicU64::new(0),
            last_ipc_event_ts_ms: AtomicU64::new(0),
            last_bus_event_ts_ms: AtomicU64::new(0),
            last_gatekeeper_decision_ts_ms: AtomicU64::new(0),
            last_decisions_write_ts_ms: AtomicU64::new(0),
            last_buys_write_ts_ms: AtomicU64::new(0),
            last_events_write_ts_ms: AtomicU64::new(0),
            subscribe_sent_ts_ms: AtomicU64::new(0),
            grpc_state_u8: AtomicU8::new(GRPC_STATE_DISCONNECTED),
            grpc_reconnects: AtomicU32::new(0),
        })
    }

    // ── mark helpers ────────────────────────────────────────────────

    #[inline]
    fn mark_grpc_progress(&self) {
        self.last_grpc_progress_ts_ms
            .store(now_ms(), Ordering::Relaxed);
    }

    /// Record a gRPC message arrival (sets `last_grpc_msg_ts_ms` to now).
    #[inline]
    pub fn mark_grpc_msg(&self) {
        let now = now_ms();
        self.last_grpc_msg_ts_ms.store(now, Ordering::Relaxed);
        self.last_grpc_progress_ts_ms.store(now, Ordering::Relaxed);
    }

    /// Record an IPC event arrival.
    #[inline]
    pub fn mark_ipc_event(&self) {
        self.last_ipc_event_ts_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// Record a bus event arrival.
    #[inline]
    pub fn mark_bus_event(&self) {
        self.last_bus_event_ts_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// Record a gatekeeper decision.
    #[inline]
    pub fn mark_gatekeeper_decision(&self) {
        self.last_gatekeeper_decision_ts_ms
            .store(now_ms(), Ordering::Relaxed);
    }

    /// Record a decisions JSONL write.
    #[inline]
    pub fn mark_decisions_write(&self) {
        self.last_decisions_write_ts_ms
            .store(now_ms(), Ordering::Relaxed);
    }

    /// Record a buys JSONL write.
    #[inline]
    pub fn mark_buys_write(&self) {
        self.last_buys_write_ts_ms
            .store(now_ms(), Ordering::Relaxed);
    }

    /// Record an events JSONL write.
    #[inline]
    pub fn mark_events_write(&self) {
        self.last_events_write_ts_ms
            .store(now_ms(), Ordering::Relaxed);
    }

    // ── gRPC state helpers ──────────────────────────────────────────

    /// Update the gRPC connection state (0..6).
    #[inline]
    pub fn set_grpc_state(&self, state_u8: u8) {
        self.grpc_state_u8.store(state_u8, Ordering::Relaxed);
        self.mark_grpc_progress();
    }

    /// Increment the gRPC reconnect counter.
    ///
    /// NOTE: does NOT call `mark_grpc_progress()`. A reconnect is a failure
    /// event — it must not refresh the transport-progress timestamp or the
    /// watchdog zombie guard (`GRPC_ZOMBIE_EXIT_MS`) will never fire.
    #[inline]
    pub fn inc_grpc_reconnects(&self) {
        self.grpc_reconnects.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a gRPC subscribe request was sent.
    #[inline]
    pub fn mark_grpc_subscribe_sent(&self) {
        self.subscribe_sent_ts_ms.store(now_ms(), Ordering::Relaxed);
        self.mark_grpc_progress();
    }
}

impl Default for RuntimeHealth {
    fn default() -> Self {
        // Used internally by `new()`. Prefer `RuntimeHealth::new()` which returns Arc.
        Self {
            last_grpc_msg_ts_ms: AtomicU64::new(0),
            last_grpc_progress_ts_ms: AtomicU64::new(0),
            last_ipc_event_ts_ms: AtomicU64::new(0),
            last_bus_event_ts_ms: AtomicU64::new(0),
            last_gatekeeper_decision_ts_ms: AtomicU64::new(0),
            last_decisions_write_ts_ms: AtomicU64::new(0),
            last_buys_write_ts_ms: AtomicU64::new(0),
            last_events_write_ts_ms: AtomicU64::new(0),
            subscribe_sent_ts_ms: AtomicU64::new(0),
            grpc_state_u8: AtomicU8::new(GRPC_STATE_DISCONNECTED),
            grpc_reconnects: AtomicU32::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_ms_returns_epoch_milliseconds() {
        let ts = now_ms();
        // Should be a 13-digit epoch-ms value (year ~2020+)
        assert!(
            ts > 1_600_000_000_000,
            "Expected 13-digit epoch-ms, got {}",
            ts
        );
    }

    #[test]
    fn test_runtime_health_initial_state() {
        let h = RuntimeHealth::new();
        assert_eq!(h.last_grpc_msg_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.last_grpc_progress_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.last_ipc_event_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.last_bus_event_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.last_gatekeeper_decision_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.last_decisions_write_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.last_buys_write_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.last_events_write_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(h.subscribe_sent_ts_ms.load(Ordering::Relaxed), 0);
        assert_eq!(
            h.grpc_state_u8.load(Ordering::Relaxed),
            GRPC_STATE_DISCONNECTED
        );
        assert_eq!(h.grpc_reconnects.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_mark_grpc_msg() {
        let h = RuntimeHealth::new();
        let before = now_ms();
        h.mark_grpc_msg();
        let after = now_ms();
        let ts = h.last_grpc_msg_ts_ms.load(Ordering::Relaxed);
        let progress_ts = h.last_grpc_progress_ts_ms.load(Ordering::Relaxed);
        assert!(ts >= before && ts <= after);
        assert!(progress_ts >= before && progress_ts <= after);
    }

    #[test]
    fn test_mark_ipc_event() {
        let h = RuntimeHealth::new();
        h.mark_ipc_event();
        assert!(h.last_ipc_event_ts_ms.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_mark_bus_event() {
        let h = RuntimeHealth::new();
        h.mark_bus_event();
        assert!(h.last_bus_event_ts_ms.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_mark_gatekeeper_decision() {
        let h = RuntimeHealth::new();
        h.mark_gatekeeper_decision();
        assert!(h.last_gatekeeper_decision_ts_ms.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_mark_writers() {
        let h = RuntimeHealth::new();
        h.mark_decisions_write();
        h.mark_buys_write();
        h.mark_events_write();
        assert!(h.last_decisions_write_ts_ms.load(Ordering::Relaxed) > 0);
        assert!(h.last_buys_write_ts_ms.load(Ordering::Relaxed) > 0);
        assert!(h.last_events_write_ts_ms.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_set_grpc_state() {
        let h = RuntimeHealth::new();
        let before = now_ms();
        h.set_grpc_state(GRPC_STATE_CONNECTED);
        let progress_ts = h.last_grpc_progress_ts_ms.load(Ordering::Relaxed);
        assert_eq!(
            h.grpc_state_u8.load(Ordering::Relaxed),
            GRPC_STATE_CONNECTED
        );
        assert!(progress_ts >= before && progress_ts <= now_ms());
        h.set_grpc_state(GRPC_STATE_FAILED);
        assert_eq!(h.grpc_state_u8.load(Ordering::Relaxed), GRPC_STATE_FAILED);
    }

    #[test]
    fn test_inc_grpc_reconnects() {
        let h = RuntimeHealth::new();
        h.inc_grpc_reconnects();
        h.inc_grpc_reconnects();
        h.inc_grpc_reconnects();
        assert_eq!(h.grpc_reconnects.load(Ordering::Relaxed), 3);
        // Reconnects must NOT update last_grpc_progress_ts_ms — a reconnect is a
        // failure event, not forward progress.  The zombie guard in the watchdog
        // relies on this invariant: if only reconnects occur (no actual data or
        // subscribe), progress_ts stays at 0 so the hard exit fires correctly.
        assert_eq!(h.last_grpc_progress_ts_ms.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_runtime_health_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RuntimeHealth>();
    }

    #[test]
    fn test_mark_grpc_subscribe_sent() {
        let h = RuntimeHealth::new();
        assert_eq!(h.subscribe_sent_ts_ms.load(Ordering::Relaxed), 0);
        h.mark_grpc_subscribe_sent();
        assert!(h.subscribe_sent_ts_ms.load(Ordering::Relaxed) > 0);
        assert!(h.last_grpc_progress_ts_ms.load(Ordering::Relaxed) > 0);
    }
}
