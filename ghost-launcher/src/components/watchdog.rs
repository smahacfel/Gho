//! Watchdog task — periodic health-status logging and controlled exit on stall.
//!
//! Every 60 seconds the watchdog reads the shared `RuntimeHealth` atomics and
//! emits a single INFO line with channel ages. If any data-plane channel is
//! stalled beyond configured thresholds the watchdog escalates:
//!
//! | condition                                  | action         |
//! |--------------------------------------------|----------------|
//! | `age_grpc_ms > 60 000`                     | ERROR log      |
//! | `age_grpc_ms > 300 000` and no gRPC progress | `exit(2)`    |
//! | gRPC fresh + recent gatekeeper activity + `age_decisions_ms > 300 000` | `exit(3)` |
//! | gRPC fresh + `age_events_ms > 300 000`     | `exit(4)`      |
//!
//! A timestamp of **0** means "not yet reported" (startup) and is treated as
//! unknown — never triggering an exit.

use ghost_core::health::{
    now_ms, RuntimeHealth, GRPC_STATE_AUTHENTICATING, GRPC_STATE_CONNECTED, GRPC_STATE_CONNECTING,
    GRPC_STATE_DISCONNECTED, GRPC_STATE_FAILED, GRPC_STATE_RECONNECTING, GRPC_STATE_SUBSCRIBING,
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Threshold (ms) after which a gRPC stall is logged as ERROR.
const GRPC_STALL_WARN_MS: u64 = 60_000;
/// Threshold (ms) after which a gRPC stall triggers `process::exit(2)`.
const GRPC_STALL_EXIT_MS: u64 = 300_000;
/// Absolute cap (ms) — zombie-connection guard.
///
/// Even when `transport_progress_is_recent` is true (reconnects keep the
/// progress timestamp fresh), if no actual gRPC message has arrived for this
/// long the connection is a zombie: TCP/HTTP2 handshakes happen, state
/// transitions fire, reconnect counter rises, but subscription data never
/// flows.  After this hard deadline we exit regardless.
const GRPC_ZOMBIE_EXIT_MS: u64 = 10 * 60 * 1_000; // 10 minutes
/// Threshold (ms) after which a pipeline stall (decisions) triggers `exit(3)`.
const PIPELINE_DECISIONS_STALL_EXIT_MS: u64 = 300_000;
/// Threshold (ms) after which a pipeline stall (events) triggers `exit(4)`.
const PIPELINE_EVENTS_STALL_EXIT_MS: u64 = 300_000;
/// gRPC is considered "fresh" when last message is within this window.
const GRPC_FRESH_MS: u64 = 30_000;

/// Human-readable label for `grpc_state_u8`.
fn grpc_state_label(state: u8) -> &'static str {
    match state {
        GRPC_STATE_DISCONNECTED => "DISCONNECTED",
        GRPC_STATE_CONNECTING => "CONNECTING",
        GRPC_STATE_AUTHENTICATING => "AUTHENTICATING",
        GRPC_STATE_SUBSCRIBING => "SUBSCRIBING",
        GRPC_STATE_CONNECTED => "CONNECTED",
        GRPC_STATE_RECONNECTING => "RECONNECTING",
        GRPC_STATE_FAILED => "FAILED",
        _ => "UNKNOWN",
    }
}

/// Compute age in ms, returning `None` when timestamp is 0 (startup / unknown).
#[inline]
fn age_or_unknown(ts: u64) -> Option<u64> {
    if ts == 0 {
        None
    } else {
        Some(now_ms().saturating_sub(ts))
    }
}

/// Format age for the log line. `None` → `"unknown"`.
fn fmt_age(age: Option<u64>) -> String {
    match age {
        Some(ms) => format!("{}ms", ms),
        None => "unknown".to_string(),
    }
}

#[inline]
fn grpc_subscribe_has_stalled_without_messages(
    grpc_age: Option<u64>,
    subscribe_age: Option<u64>,
    threshold_ms: u64,
) -> bool {
    grpc_age.is_none() && subscribe_age.is_some_and(|age| age > threshold_ms)
}

#[inline]
fn grpc_transport_has_recent_progress(progress_age: Option<u64>, threshold_ms: u64) -> bool {
    progress_age.is_some_and(|age| age <= threshold_ms)
}

/// Spawn the watchdog background task.
///
/// * `health` — shared `RuntimeHealth` instance (SSOT).
/// * `is_grpc_mode` — `true` when `source_mode == geyser_grpc`; controls
///   gRPC-specific stall detection.
///
/// The task runs until the process exits. It never panics.
///
/// # Env vars
///
/// | Variable | Default | Effect |
/// |---|---|---|
/// | `GHOST_WATCHDOG_FATAL_EXIT` | `1` | Set to `0` / `false` to log stalls as ERROR without calling `process::exit()`. Useful when connecting via SSH/phone where a brief disconnect should not terminate the launcher. |
pub async fn run(health: Arc<RuntimeHealth>, is_grpc_mode: bool) {
    // Allow disabling fatal exits via environment variable so that SSH
    // disconnects or brief stalls do not terminate the launcher.
    let fatal_exit_enabled = std::env::var("GHOST_WATCHDOG_FATAL_EXIT")
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(true);

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    info!(
        "🐕 Watchdog started (interval=60s, grpc_mode={}, grpc_stall_warn={}s, grpc_stall_exit={}s, fatal_exit={})",
        is_grpc_mode,
        GRPC_STALL_WARN_MS / 1000,
        GRPC_STALL_EXIT_MS / 1000,
        fatal_exit_enabled,
    );

    loop {
        interval.tick().await;

        let grpc_ts = health.last_grpc_msg_ts_ms.load(Ordering::Relaxed);
        let grpc_progress_ts = health.last_grpc_progress_ts_ms.load(Ordering::Relaxed);
        let ipc_ts = health.last_ipc_event_ts_ms.load(Ordering::Relaxed);
        let bus_ts = health.last_bus_event_ts_ms.load(Ordering::Relaxed);
        let gk_ts = health
            .last_gatekeeper_decision_ts_ms
            .load(Ordering::Relaxed);
        let dec_ts = health.last_decisions_write_ts_ms.load(Ordering::Relaxed);
        let buys_ts = health.last_buys_write_ts_ms.load(Ordering::Relaxed);
        let events_ts = health.last_events_write_ts_ms.load(Ordering::Relaxed);
        let subscribe_ts = health.subscribe_sent_ts_ms.load(Ordering::Relaxed);
        let grpc_state = health.grpc_state_u8.load(Ordering::Relaxed);
        let reconnects = health.grpc_reconnects.load(Ordering::Relaxed);

        let age_grpc = age_or_unknown(grpc_ts);
        let age_grpc_progress = age_or_unknown(grpc_progress_ts);
        let age_ipc = age_or_unknown(ipc_ts);
        let age_bus = age_or_unknown(bus_ts);
        let age_gk = age_or_unknown(gk_ts);
        let age_dec = age_or_unknown(dec_ts);
        let age_buys = age_or_unknown(buys_ts);
        let age_events = age_or_unknown(events_ts);
        let age_subscribe = age_or_unknown(subscribe_ts);

        // ── status log line ─────────────────────────────────────────
        info!(
            "WATCHDOG | grpc_state={} reconnects={} | age_grpc={} age_ipc={} age_bus={} age_gk={} age_dec={} age_buys={} age_events={}",
            grpc_state_label(grpc_state),
            reconnects,
            fmt_age(age_grpc),
            fmt_age(age_ipc),
            fmt_age(age_bus),
            fmt_age(age_gk),
            fmt_age(age_dec),
            fmt_age(age_buys),
            fmt_age(age_events),
        );

        // ── gRPC stall detection (only in grpc mode) ────────────────
        if is_grpc_mode {
            let transport_progress_is_recent =
                grpc_transport_has_recent_progress(age_grpc_progress, GRPC_STALL_EXIT_MS);
            if let Some(grpc_age) = age_grpc {
                if grpc_age > GRPC_STALL_EXIT_MS {
                    if transport_progress_is_recent {
                        if grpc_age > GRPC_ZOMBIE_EXIT_MS {
                            // Zombie guard: transport keeps reconnecting (refreshing
                            // progress_ts) but no subscription data has arrived for
                            // GRPC_ZOMBIE_EXIT_MS.  This breaks the infinite warn loop.
                            error!(
                                "WATCHDOG FATAL: gRPC zombie — silent for {}ms despite transport progress {} ago (reconnects={}, state={}) — absolute cap {}ms exceeded — exiting with code 2",
                                grpc_age,
                                fmt_age(age_grpc_progress),
                                reconnects,
                                grpc_state_label(grpc_state),
                                GRPC_ZOMBIE_EXIT_MS,
                            );
                            if fatal_exit_enabled {
                                std::process::exit(2);
                            }
                        } else {
                            error!(
                                "WATCHDOG WARN: gRPC silent for {}ms but transport progress was observed {} ago (state={}, reconnects={})",
                                grpc_age,
                                fmt_age(age_grpc_progress),
                                grpc_state_label(grpc_state),
                                reconnects,
                            );
                        }
                    } else {
                        error!(
                            "WATCHDOG FATAL: gRPC stalled for {}ms and transport progress is {} (>{} ms) — exiting with code 2",
                            grpc_age,
                            fmt_age(age_grpc_progress),
                            GRPC_STALL_EXIT_MS,
                        );
                        if fatal_exit_enabled {
                            std::process::exit(2);
                        }
                    }
                } else if grpc_age > GRPC_STALL_WARN_MS {
                    error!(
                        "WATCHDOG WARN: gRPC stalled for {}ms (>{} ms)",
                        grpc_age, GRPC_STALL_WARN_MS,
                    );
                }
            } else if grpc_subscribe_has_stalled_without_messages(
                age_grpc,
                age_subscribe,
                GRPC_STALL_EXIT_MS,
            ) {
                if transport_progress_is_recent {
                    error!(
                        "WATCHDOG WARN: gRPC has not delivered any message for {} after subscribe attempt while state={} but transport progress was observed {} ago",
                        fmt_age(age_subscribe),
                        grpc_state_label(grpc_state),
                        fmt_age(age_grpc_progress),
                    );
                } else {
                    error!(
                        "WATCHDOG FATAL: gRPC has not delivered any message for {} after subscribe attempt while state={} and transport progress is {} — exiting with code 2",
                        fmt_age(age_subscribe),
                        grpc_state_label(grpc_state),
                        fmt_age(age_grpc_progress),
                    );
                    if fatal_exit_enabled {
                        std::process::exit(2);
                    }
                }
            } else if grpc_subscribe_has_stalled_without_messages(
                age_grpc,
                age_subscribe,
                GRPC_STALL_WARN_MS,
            ) {
                error!(
                    "WATCHDOG WARN: gRPC has not delivered any message for {} after subscribe attempt while state={}",
                    fmt_age(age_subscribe),
                    grpc_state_label(grpc_state),
                );
            }
            // age_grpc == None (ts==0) → startup, do not trigger exit
        }

        // ── pipeline stall detection (gRPC fresh, pipeline stuck) ───
        let grpc_is_fresh = age_grpc.map_or(false, |a| a < GRPC_FRESH_MS);
        let gatekeeper_recent = age_gk.map_or(false, |a| a < GRPC_FRESH_MS);

        if grpc_is_fresh {
            if let Some(dec_age) = age_dec {
                if gatekeeper_recent && dec_age > PIPELINE_DECISIONS_STALL_EXIT_MS {
                    error!(
                        "WATCHDOG FATAL: decisions writer stalled for {}ms while gatekeeper decisions are flowing — exiting with code 3",
                        dec_age
                    );
                    if fatal_exit_enabled {
                        std::process::exit(3);
                    }
                }
            }

            if let Some(events_age) = age_events {
                if events_age > PIPELINE_EVENTS_STALL_EXIT_MS {
                    // Only exit if decisions are still flowing (pipeline partially alive)
                    let decisions_recent = age_dec.map_or(false, |a| a < GRPC_FRESH_MS);
                    if decisions_recent {
                        error!(
                            "WATCHDOG FATAL: events writer stalled for {}ms while decisions are flowing — exiting with code 4",
                            events_age,
                        );
                        if fatal_exit_enabled {
                            std::process::exit(4);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_age_or_unknown_zero_returns_none() {
        assert_eq!(age_or_unknown(0), None);
    }

    #[test]
    fn test_age_or_unknown_nonzero() {
        let ts = now_ms() - 5000;
        let age = age_or_unknown(ts);
        assert!(age.is_some());
        // Age should be approximately 5000ms (allow some slack)
        let a = age.unwrap();
        assert!(a >= 4900 && a <= 6000, "age was {}", a);
    }

    #[test]
    fn test_fmt_age_none() {
        assert_eq!(fmt_age(None), "unknown");
    }

    #[test]
    fn test_fmt_age_some() {
        assert_eq!(fmt_age(Some(1234)), "1234ms");
    }

    #[test]
    fn test_grpc_state_labels() {
        assert_eq!(grpc_state_label(0), "DISCONNECTED");
        assert_eq!(grpc_state_label(4), "CONNECTED");
        assert_eq!(grpc_state_label(6), "FAILED");
        assert_eq!(grpc_state_label(99), "UNKNOWN");
    }

    #[test]
    fn test_subscribe_stall_without_messages() {
        assert!(grpc_subscribe_has_stalled_without_messages(
            None,
            Some(GRPC_STALL_EXIT_MS + 1),
            GRPC_STALL_EXIT_MS,
        ));
        assert!(!grpc_subscribe_has_stalled_without_messages(
            Some(1),
            Some(GRPC_STALL_EXIT_MS + 1),
            GRPC_STALL_EXIT_MS,
        ));
        assert!(!grpc_subscribe_has_stalled_without_messages(
            None,
            Some(GRPC_STALL_WARN_MS - 1),
            GRPC_STALL_WARN_MS,
        ));
    }

    #[test]
    fn test_grpc_transport_recent_progress() {
        assert!(grpc_transport_has_recent_progress(
            Some(GRPC_STALL_EXIT_MS),
            GRPC_STALL_EXIT_MS,
        ));
        assert!(!grpc_transport_has_recent_progress(
            Some(GRPC_STALL_EXIT_MS + 1),
            GRPC_STALL_EXIT_MS,
        ));
        assert!(!grpc_transport_has_recent_progress(
            None,
            GRPC_STALL_EXIT_MS
        ));
    }
}
