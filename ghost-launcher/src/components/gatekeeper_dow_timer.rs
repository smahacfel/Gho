//! Gatekeeper V2.5 — DOW (Dynamic Observation Window) timer helper.
//!
//! Provides helpers to create a per-pool interval timer that drives shadow
//! checkpoints independently of TX traffic. The interval is consumed as a
//! branch in the existing `pool_observation_task` tokio::select! loop,
//! guaranteeing natural serialization with the TX ingestion path — no
//! separate lock contention, no duplicate checkpoints.
//!
//! Contract:
//! - Creates `tokio::time::interval` with tick every `dow.tick_interval_ms`.
//! - On each tick in the select! loop, calls `maybe_fire_shadow_checkpoint`.
//! - Terminates when pool reaches terminal state or deadline expires.
//!
//! Integration point: `pool_observation_task` in `oracle_runtime.rs`.
//! The interval is added as a `tokio::select!` branch alongside `rx.recv()`
//! and the main `deadline` timer.

use std::time::Duration;
use tokio::time::{Interval, MissedTickBehavior};

/// Create a DOW timer interval with the configured tick period.
///
/// Uses `MissedTickBehavior::Skip` to avoid burst ticks after stalls,
/// matching the pattern used in `oracle_runtime`, `main`, and `gatekeeper_commit_loop`.
///
/// The first tick is skipped (0ms burst avoidance) by the caller
/// via `interval.tick().await` before entering the select loop.
pub fn dow_timer_interval(tick_interval_ms: u64) -> Interval {
    assert!(
        tick_interval_ms > 0,
        "P0 invariant violated: dow.tick_interval_ms must be > 0 when creating DOW timer interval"
    );
    let mut interval = tokio::time::interval(Duration::from_millis(tick_interval_ms));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_brain::config::gatekeeper_v25_config::DynamicObservationWindowConfig;

    /// Verify that the DOW timer interval is created with the correct period.
    #[tokio::test]
    async fn test_dow_timer_interval_creation() {
        let interval = dow_timer_interval(250);
        assert_eq!(interval.period(), Duration::from_millis(250));
    }

    /// Verify that the DOW config defaults include `tick_interval_ms = 250`.
    #[test]
    fn test_dow_timer_default_config_has_tick_interval() {
        let dow = DynamicObservationWindowConfig::default();
        assert_eq!(dow.tick_interval_ms, 250);
    }

    /// Verify DOW config deserializes tick_interval_ms correctly.
    #[test]
    fn test_dow_timer_config_partial_override() {
        let toml_str = r#"
enabled = true
tick_interval_ms = 100
"#;
        let dow: DynamicObservationWindowConfig = toml::from_str(toml_str).unwrap();
        assert!(dow.enabled);
        assert_eq!(dow.tick_interval_ms, 100);
        // Unchanged fields use defaults
        assert_eq!(dow.normal_window_ms, 7000);
    }

    #[test]
    #[should_panic(expected = "dow.tick_interval_ms must be > 0")]
    fn test_dow_timer_interval_rejects_zero_tick() {
        let _ = dow_timer_interval(0);
    }
}
