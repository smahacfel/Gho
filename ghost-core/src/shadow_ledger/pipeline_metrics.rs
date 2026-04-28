//! Shadow Ledger Pipeline Metrics
//!
//! Thin recording helpers for latency, coverage, and throughput measurements
//! at every critical stage of the gRPC → Gatekeeper → ShadowLedger pipeline.
//!
//! ## Metrics emitted
//!
//! ### Gatekeeper stage
//! - `gatekeeper_pool_tx_buffered_total{pool}`        — txs accepted into a buffer
//! - `gatekeeper_pool_tx_committed_total{pool}`       — txs committed to Shadow Ledger
//! - `gatekeeper_pool_snapshots_committed_total{pool}`— snapshots written per pool
//! - `gatekeeper_pool_tx_dropped_total{pool}`         — txs lost when buffer expires
//! - `gatekeeper_buffer_age_at_commit_ms{pool}`       — histogram: buffer age at commit
//! - `gatekeeper_tx_count_at_commit`                  — histogram: tx count at commit
//! - `gatekeeper_snapshot_build_duration_us`          — histogram: Phase-2 build time
//! - `gatekeeper_dropped_buffer_age_ms`               — histogram: age of dropped buffers
//! - `gatekeeper_dropped_buffer_tx_count`             — histogram: tx count of dropped buffers
//!
//! ### Shadow Ledger commit stage
//! - `shadow_ledger_pool_snapshots_written_total{pool}` — snapshots written via commit_history
//! - `shadow_ledger_commit_history_lag_ms`              — histogram: last-snap-ts → now
//!
//! ### Shadow Ledger live-append stage
//! - `shadow_ledger_live_appended_total{pool}`  — successful append_live calls
//! - `shadow_ledger_live_append_lag_ms`         — histogram: snap-ts → now for live appends
//!
//! ### Live Pipeline flush stage
//! - `live_pipeline_buffer_size_at_flush{pool}` — histogram: pending events before flush
//! - `live_pipeline_flush_duration_us`          — histogram: flush wall-clock time
//! - `live_pipeline_snapshots_appended_total{pool}` — snapshots written per flush
//!
//! All pool labels use the base-mint pubkey as a string.  High cardinality is
//! expected and intentional — the purpose is per-pool coverage diagnostics.

// ─── Gatekeeper ──────────────────────────────────────────────────────────────

/// Record one transaction accepted into a Gatekeeper buffer.
///
/// Call from `GatekeeperRegistry::add_tx` on `Ok(())`.
#[inline]
pub fn on_tx_gatekeeper_buffered(pool: &str) {
    metrics::counter!("gatekeeper_pool_tx_buffered_total", 1, "pool" => pool.to_string());
}

/// Record a Gatekeeper buffer successfully committed to Shadow Ledger.
///
/// # Arguments
/// - `pool`             — base-mint pubkey string
/// - `buffer_age_ms`    — wall-clock age of the buffer at commit time (ms)
/// - `tx_count`         — transactions buffered (from `GatekeeperMintBuffer::tx_count()`)
/// - `snapshot_count`   — MarketSnapshots persisted (`CommitResult::committed_count`)
/// - `build_duration_us`— Phase-2 snapshot-build wall time (μs); 0 when not measured
#[inline]
pub fn on_gatekeeper_committed(
    pool: &str,
    buffer_age_ms: u64,
    tx_count: usize,
    snapshot_count: usize,
    build_duration_us: u64,
) {
    metrics::counter!(
        "gatekeeper_pool_tx_committed_total",
        tx_count as u64,
        "pool" => pool.to_string()
    );
    metrics::counter!(
        "gatekeeper_pool_snapshots_committed_total",
        snapshot_count as u64,
        "pool" => pool.to_string()
    );
    metrics::histogram!(
        "gatekeeper_buffer_age_at_commit_ms",
        buffer_age_ms as f64,
        "pool" => pool.to_string()
    );
    metrics::histogram!("gatekeeper_tx_count_at_commit", tx_count as f64);
    if build_duration_us > 0 {
        metrics::histogram!(
            "gatekeeper_snapshot_build_duration_us",
            build_duration_us as f64
        );
    }
}

/// Record a Gatekeeper buffer dropped without committing (observation-window timeout).
///
/// # Arguments
/// - `pool`          — base-mint pubkey string
/// - `buffer_age_ms` — age at drop time (ms)
/// - `tx_count`      — buffered transactions lost
#[inline]
pub fn on_gatekeeper_dropped(pool: &str, buffer_age_ms: u64, tx_count: usize) {
    metrics::counter!(
        "gatekeeper_pool_tx_dropped_total",
        tx_count as u64,
        "pool" => pool.to_string()
    );
    metrics::histogram!("gatekeeper_dropped_buffer_age_ms", buffer_age_ms as f64);
    metrics::histogram!("gatekeeper_dropped_buffer_tx_count", tx_count as f64);
}

// ─── Shadow Ledger commit_history ────────────────────────────────────────────

/// Record a successful `ShadowLedger::commit_history` call.
///
/// # Arguments
/// - `pool`            — base-mint pubkey string
/// - `snap_count`      — snapshots written
/// - `last_snap_ts_ms` — `timestamp_ms` of the last committed snapshot (on-chain tx time);
///                       pass 0 if unavailable
/// - `now_ms`          — current wall-clock (ms since UNIX epoch)
#[inline]
pub fn on_commit_history(pool: &str, snap_count: usize, last_snap_ts_ms: u64, now_ms: u64) {
    metrics::counter!(
        "shadow_ledger_pool_snapshots_written_total",
        snap_count as u64,
        "pool" => pool.to_string()
    );
    if last_snap_ts_ms > 0 && now_ms >= last_snap_ts_ms {
        let lag_ms = (now_ms - last_snap_ts_ms) as f64;
        metrics::histogram!("shadow_ledger_commit_history_lag_ms", lag_ms);
    }
}

// ─── Shadow Ledger append_live ───────────────────────────────────────────────

/// Record a successful `ShadowLedger::append_live` call.
///
/// # Arguments
/// - `pool`        — base-mint pubkey string
/// - `snap_ts_ms`  — `timestamp_ms` of the appended snapshot (on-chain tx time)
/// - `now_ms`      — current wall-clock (ms since UNIX epoch)
#[inline]
pub fn on_live_append(pool: &str, snap_ts_ms: u64, now_ms: u64) {
    metrics::counter!(
        "shadow_ledger_live_appended_total",
        1,
        "pool" => pool.to_string()
    );
    if snap_ts_ms > 0 && now_ms >= snap_ts_ms {
        let lag_ms = (now_ms - snap_ts_ms) as f64;
        metrics::histogram!("shadow_ledger_live_append_lag_ms", lag_ms);
    }
}

// ─── Live Pipeline flush ─────────────────────────────────────────────────────

/// Record one `LivePipeline::flush_mint` call.
///
/// # Arguments
/// - `pool`         — base-mint pubkey string
/// - `buffer_size`  — pending events in the buffer before flush
/// - `appended`     — snapshots successfully appended to ShadowLedger
/// - `duration_us`  — flush wall-clock time (μs)
#[inline]
pub fn on_live_pipeline_flush(pool: &str, buffer_size: usize, appended: usize, duration_us: u64) {
    metrics::histogram!(
        "live_pipeline_buffer_size_at_flush",
        buffer_size as f64,
        "pool" => pool.to_string()
    );
    metrics::histogram!("live_pipeline_flush_duration_us", duration_us as f64);
    if appended > 0 {
        metrics::counter!(
            "live_pipeline_snapshots_appended_total",
            appended as u64,
            "pool" => pool.to_string()
        );
    }
}
