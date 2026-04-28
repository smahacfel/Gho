//! A/B Boundary-Equalization: Window specification and runtime state.
//!
//! Defines the deterministic observation window for A/B comparison.
//! Every pool record is measured from `t0` to `t_end = t0 + window_ms`
//! on the epoch-like event axis selected through time-provenance helpers
//! (`chain_event` first, then explicit ingress wall time).

use serde::{Deserialize, Serialize};

// ─── Config-only types ──────────────────────────────────────────────────────

/// How the window start (`t0`) is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartKind {
    /// `t0` comes from the provenance-aware NewPoolDetected event clock.
    NewPoolDetectedEventTs,
    /// Fallback: `t0` comes from the first transaction's decision-eligible
    /// event clock when no pool-detected event arrived before it.
    FirstTxEventTs,
}

impl StartKind {
    /// Stable tag for JSONL serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NewPoolDetectedEventTs => "NewPoolDetected",
            Self::FirstTxEventTs => "FirstTxFallback",
        }
    }
}

impl Default for StartKind {
    fn default() -> Self {
        Self::NewPoolDetectedEventTs
    }
}

/// How the window end (`t_end`) is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndKind {
    /// `t_end = t0 + window_ms`.
    T0PlusWindowMs,
}

impl Default for EndKind {
    fn default() -> Self {
        Self::T0PlusWindowMs
    }
}

/// Immutable specification of the observation window (config-only).
///
/// Does **not** replace `max_wait_time_ms` — this is a *data* definition
/// used to establish deterministic A/B boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSpec {
    /// Window length in milliseconds (e.g. 10_000).
    pub window_ms: u64,
    /// How `t0` is chosen.
    pub start_kind: StartKind,
    /// How `t_end` is derived from `t0`.
    pub end_kind: EndKind,
    /// Minimum transactions inside the window for the record to be
    /// considered complete in downstream A/B analysis.
    pub min_tx_in_window: u32,
}

impl Default for WindowSpec {
    fn default() -> Self {
        Self {
            window_ms: 10_000,
            start_kind: StartKind::default(),
            end_kind: EndKind::default(),
            min_tx_in_window: 10,
        }
    }
}

// ─── Epoch-ms guard ─────────────────────────────────────────────────────────

/// Detect epoch-seconds passed in a `*_ms` field and auto-correct to epoch-ms.
///
/// Epoch-ms values (2020–2050) fall in `1.6e12..2.5e12` (13 digits).
/// Epoch-seconds fall in `1.6e9..2.5e9` (10 digits).
/// If the value falls in the epoch-seconds range (`1_000_000_000..10_000_000_000`)
/// it is treated as seconds and multiplied by 1000 with an `ERROR`-level log.
/// Values below `1_000_000_000` (e.g. synthetic test values) pass through unchanged.
pub fn ensure_epoch_ms(ts: u64, field: &'static str, pool: &str) -> u64 {
    if ts >= 1_000_000_000 && ts < 10_000_000_000 {
        let fixed = ts.saturating_mul(1000);
        tracing::error!(
            field = %field,
            pool = %pool,
            ts = %ts,
            fixed = %fixed,
            "SECONDS_IN_MS_FIELD: auto-multiplying by 1000"
        );
        return fixed;
    }
    ts
}

// ─── Runtime-only state (per pool) ──────────────────────────────────────────

/// Reason why the window was closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowCloseReason {
    /// A transaction with `ts > t_end` arrived.
    EndReached,
    /// The periodic sweep detected `now >= t_end`.
    EndReachedBySweep,
    /// Pool was rejected before the window completed.
    PoolRejectedEarly,
    /// Pool received a BUY verdict before the window completed.
    PoolBoughtEarly,
    /// Pool completed compare-only shadow buy without real network send.
    PoolShadowedEarly,
    /// Gatekeeper timed out before the window completed.
    GatekeeperTimeout,
}

impl WindowCloseReason {
    /// Stable tag for JSONL serialization.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::EndReached => "END_REACHED",
            Self::EndReachedBySweep => "END_REACHED_BY_SWEEP",
            Self::PoolRejectedEarly => "POOL_REJECTED_EARLY",
            Self::PoolBoughtEarly => "POOL_BOUGHT_EARLY",
            Self::PoolShadowedEarly => "POOL_SHADOWED_EARLY",
            Self::GatekeeperTimeout => "GATEKEEPER_TIMEOUT",
        }
    }
}

/// Lightweight per-pool window state.
///
/// Does **not** influence Gatekeeper verdicts — it is a "measuring tape"
/// for logging, JSONL enrichment, and downstream A/B filtering.
#[derive(Debug, Clone)]
pub struct WindowState {
    pub t0_event_ts_ms: u64,
    pub t_end_event_ts_ms: u64,
    pub window_ms: u64,
    pub started_from: StartKind,
    pub window_complete: bool,
    pub window_close_reason: Option<WindowCloseReason>,
    pub tx_count_window: u32,
    pub unique_signers_window: u32,
    pub fail_count_window: u32,
    pub last_seen_event_ts_ms: u64,
    /// Set of unique signer addresses within the window (kept for counting).
    signers: std::collections::HashSet<String>,
}

impl WindowState {
    /// Create from a `NewPoolDetected` event timestamp.
    pub fn from_pool_detected(pool_ts_ms: u64, window_ms: u64) -> Self {
        let t0 = ensure_epoch_ms(pool_ts_ms, "t0_event_ts_ms", "from_pool_detected");
        Self {
            t0_event_ts_ms: t0,
            t_end_event_ts_ms: t0.saturating_add(window_ms),
            window_ms,
            started_from: StartKind::NewPoolDetectedEventTs,
            window_complete: false,
            window_close_reason: None,
            tx_count_window: 0,
            unique_signers_window: 0,
            fail_count_window: 0,
            last_seen_event_ts_ms: t0,
            signers: std::collections::HashSet::new(),
        }
    }

    /// Fallback: create from the first transaction timestamp.
    pub fn from_first_tx(tx_ts_ms: u64, window_ms: u64) -> Self {
        let t0 = ensure_epoch_ms(tx_ts_ms, "t0_event_ts_ms", "from_first_tx");
        Self {
            t0_event_ts_ms: t0,
            t_end_event_ts_ms: t0.saturating_add(window_ms),
            window_ms,
            started_from: StartKind::FirstTxEventTs,
            window_complete: false,
            window_close_reason: None,
            tx_count_window: 0,
            unique_signers_window: 0,
            fail_count_window: 0,
            last_seen_event_ts_ms: t0,
            signers: std::collections::HashSet::new(),
        }
    }

    /// Try to ingest a transaction event. Returns `true` if the tx falls
    /// inside `[t0, t_end]` and was counted.
    ///
    /// Side-effects:
    /// - tx before t0 → ignored (returns false)
    /// - tx after t_end → marks window complete, returns false
    /// - tx in window → updates counters, returns true
    pub fn try_ingest(&mut self, tx_ts_ms: u64, signer: &str, success: bool) -> bool {
        self.last_seen_event_ts_ms = self.last_seen_event_ts_ms.max(tx_ts_ms);

        if tx_ts_ms < self.t0_event_ts_ms {
            return false;
        }

        if tx_ts_ms > self.t_end_event_ts_ms {
            if !self.window_complete {
                self.window_complete = true;
                self.window_close_reason = Some(WindowCloseReason::EndReached);
            }
            return false;
        }

        // Inside window
        self.tx_count_window += 1;
        if self.signers.insert(signer.to_string()) {
            self.unique_signers_window += 1;
        }
        if !success {
            self.fail_count_window += 1;
        }
        true
    }

    /// Called by the periodic sweep when wall-clock `now_ms >= t_end`.
    pub fn try_sweep_complete(&mut self, now_ms: u64) {
        if !self.window_complete && now_ms >= self.t_end_event_ts_ms {
            self.window_complete = true;
            self.window_close_reason = Some(WindowCloseReason::EndReachedBySweep);
        }
    }

    /// Mark as prematurely closed due to a Gatekeeper verdict.
    pub fn mark_verdict_early(&mut self, reason: WindowCloseReason) {
        if !self.window_complete {
            self.window_close_reason = Some(reason);
            // window_complete stays false — record is NOT usable for A/B
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_cutoff() {
        let mut ws = WindowState::from_pool_detected(1_000, 10_000);

        // tx before t0 → ignored
        assert!(!ws.try_ingest(900, "signerA", true));
        assert_eq!(ws.tx_count_window, 0);

        // tx inside window
        assert!(ws.try_ingest(1_500, "signerB", true));
        assert_eq!(ws.tx_count_window, 1);

        // tx after t_end → forces completion
        assert!(!ws.try_ingest(11_500, "signerC", true));
        assert_eq!(ws.tx_count_window, 1);
        assert!(ws.window_complete);
        assert_eq!(
            ws.window_close_reason.as_ref().unwrap().tag(),
            "END_REACHED"
        );
    }

    #[test]
    fn test_verdict_before_completion() {
        let mut ws = WindowState::from_pool_detected(1_000, 10_000);

        // tx inside window
        assert!(ws.try_ingest(2_000, "signerA", true));
        assert_eq!(ws.tx_count_window, 1);
        assert!(!ws.window_complete);

        // Gatekeeper rejects before window closes
        ws.mark_verdict_early(WindowCloseReason::PoolRejectedEarly);

        assert!(!ws.window_complete);
        assert_eq!(
            ws.window_close_reason.as_ref().unwrap().tag(),
            "POOL_REJECTED_EARLY"
        );
    }

    #[test]
    fn test_sweep_completes_window() {
        let mut ws = WindowState::from_pool_detected(1_000, 10_000);

        // No tx arrived past t_end, but sweep runs
        ws.try_sweep_complete(11_001);

        assert!(ws.window_complete);
        assert_eq!(
            ws.window_close_reason.as_ref().unwrap().tag(),
            "END_REACHED_BY_SWEEP"
        );
    }

    #[test]
    fn test_first_tx_fallback() {
        let ws = WindowState::from_first_tx(5_000, 10_000);
        assert_eq!(ws.t0_event_ts_ms, 5_000);
        assert_eq!(ws.t_end_event_ts_ms, 15_000);
        assert_eq!(ws.started_from, StartKind::FirstTxEventTs);
    }

    #[test]
    fn test_unique_signers_counted() {
        let mut ws = WindowState::from_pool_detected(1_000, 10_000);
        assert!(ws.try_ingest(2_000, "signerA", true));
        assert!(ws.try_ingest(3_000, "signerA", true)); // duplicate
        assert!(ws.try_ingest(4_000, "signerB", false));

        assert_eq!(ws.tx_count_window, 3);
        assert_eq!(ws.unique_signers_window, 2);
        assert_eq!(ws.fail_count_window, 1);
    }

    #[test]
    fn test_default_window_spec() {
        let spec = WindowSpec::default();
        assert_eq!(spec.window_ms, 10_000);
        assert_eq!(spec.start_kind, StartKind::NewPoolDetectedEventTs);
        assert_eq!(spec.end_kind, EndKind::T0PlusWindowMs);
        assert_eq!(spec.min_tx_in_window, 10);
    }

    #[test]
    fn test_start_kind_as_str() {
        assert_eq!(
            StartKind::NewPoolDetectedEventTs.as_str(),
            "NewPoolDetected"
        );
        assert_eq!(StartKind::FirstTxEventTs.as_str(), "FirstTxFallback");
    }

    #[test]
    fn test_window_close_reason_tags() {
        assert_eq!(WindowCloseReason::EndReached.tag(), "END_REACHED");
        assert_eq!(
            WindowCloseReason::EndReachedBySweep.tag(),
            "END_REACHED_BY_SWEEP"
        );
        assert_eq!(
            WindowCloseReason::PoolRejectedEarly.tag(),
            "POOL_REJECTED_EARLY"
        );
        assert_eq!(
            WindowCloseReason::PoolBoughtEarly.tag(),
            "POOL_BOUGHT_EARLY"
        );
        assert_eq!(
            WindowCloseReason::PoolShadowedEarly.tag(),
            "POOL_SHADOWED_EARLY"
        );
        assert_eq!(
            WindowCloseReason::GatekeeperTimeout.tag(),
            "GATEKEEPER_TIMEOUT"
        );
    }

    #[test]
    fn test_ensure_epoch_ms_seconds_auto_fix() {
        // Input in epoch-seconds (10 digits) → should be multiplied by 1000
        let t0 = 1_772_315_421_u64; // seconds
        let window_ms = 2_000_u64;
        let fixed_t0 = ensure_epoch_ms(t0, "ab_t0_event_ts_ms", "test_pool");
        assert_eq!(fixed_t0, 1_772_315_421_000);
        let t_end = fixed_t0.saturating_add(window_ms);
        assert_eq!(t_end, 1_772_315_423_000);
    }

    #[test]
    fn test_ensure_epoch_ms_passthrough() {
        // Input already in epoch-ms (13 digits) → should pass through unchanged
        let t0 = 1_772_315_421_123_u64;
        let result = ensure_epoch_ms(t0, "ab_t0_event_ts_ms", "test_pool");
        assert_eq!(result, t0);
    }

    #[test]
    fn test_window_invariant_t_end_minus_t0_equals_window_ms() {
        let window_ms = 2_000_u64;

        // Ms input → passthrough (producer now emits epoch-ms)
        let ws = WindowState::from_pool_detected(1_772_315_421_000, window_ms);
        assert_eq!(ws.t0_event_ts_ms, 1_772_315_421_000);
        assert_eq!(ws.t_end_event_ts_ms, 1_772_315_423_000);
        assert_eq!(ws.t_end_event_ts_ms - ws.t0_event_ts_ms, window_ms);

        // Ms input → passthrough
        let ws2 = WindowState::from_first_tx(1_772_315_421_123, window_ms);
        assert_eq!(ws2.t0_event_ts_ms, 1_772_315_421_123);
        assert_eq!(ws2.t_end_event_ts_ms, 1_772_315_423_123);
        assert_eq!(ws2.t_end_event_ts_ms - ws2.t0_event_ts_ms, window_ms);
    }
}
