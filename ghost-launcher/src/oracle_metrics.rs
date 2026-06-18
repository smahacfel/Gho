//! Oracle Runtime Metrics
//!
//! Prometheus metrics for tracking oracle event gathering, validation, and
//! Shadow Ledger pipeline health.

use ghost_core::pipeline_coverage;
use ghost_core::tx_intelligence::types::FscMissClass;
use once_cell::sync::Lazy;
use prometheus::{
    Gauge, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Registry,
};
use std::sync::atomic::{AtomicU64, Ordering};

/// Counter for real events (PoolTransaction) gathered per pool
/// Labels: pool (pool AMM ID)
pub static ORACLE_GATHER_EVENTS_REAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "oracle_gather_events_real_total",
            "Total real events (PoolTransaction) gathered per pool"
        ),
        &["pool"],
    )
    .expect("Failed to create oracle_gather_events_real_total metric")
});

/// Counter for synthetic events (DetectedPool) gathered per pool
/// Labels: pool (pool AMM ID)
pub static ORACLE_GATHER_EVENTS_SYNTHETIC: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "oracle_gather_events_synthetic_total",
            "Total synthetic events (DetectedPool) gathered per pool"
        ),
        &["pool"],
    )
    .expect("Failed to create oracle_gather_events_synthetic_total metric")
});

/// Gauge for tracking the last gather statistics per pool
/// Labels: pool (pool AMM ID)
pub static ORACLE_GATHER_LAST_REAL_COUNT: Lazy<IntGaugeVec> = Lazy::new(|| {
    IntGaugeVec::new(
        prometheus::opts!(
            "oracle_gather_last_real_count",
            "Count of real events in the most recent gather operation"
        ),
        &["pool"],
    )
    .expect("Failed to create oracle_gather_last_real_count metric")
});

/// Counter for identity promotion attempts.
/// Labels: result ("success" | "failure")
pub static POOL_IDENTITY_PROMOTION_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "pool_identity_promotion_attempts_total",
            "Total identity promotion attempts from tx stream"
        ),
        &["result"],
    )
    .expect("Failed to create pool_identity_promotion_attempts_total metric")
});

/// Counter for pools that exhausted all identity promotion retries.
pub static POOL_IDENTITY_EXHAUSTED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "pool_identity_exhausted_total",
        "Total pools where identity promotion budget was exhausted",
    )
    .expect("Failed to create pool_identity_exhausted_total metric")
});

// =============================================================================
// Shadow Ledger pipeline health metrics
// =============================================================================

/// Gauge: fraction of gRPC-received txs that reached Shadow Ledger (percent, 0–100).
///
/// Computed as `shadow_ledger_total / grpc_received * 100` from PipelineCoverage.
/// Returns 0.0 when no gRPC txs have been received yet (denominator == 0).
/// Call `record_shadow_ledger_health()` periodically (e.g. every 5 s) to update.
pub static SHADOW_LEDGER_COVERAGE_RATIO: Lazy<Gauge> = Lazy::new(|| {
    Gauge::new(
        "shadow_ledger_coverage_ratio_pct",
        "Fraction of gRPC-received txs that reached Shadow Ledger (shadow_ledger_total/grpc_received*100)",
    )
    .expect("Failed to create shadow_ledger_coverage_ratio_pct metric")
});

/// Gauge: number of pools with committed state in Shadow Ledger.
pub static SHADOW_LEDGER_COMMITTED_POOLS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "shadow_ledger_committed_pools",
        "Number of pools with committed state in Shadow Ledger",
    )
    .expect("Failed to create shadow_ledger_committed_pools metric")
});

/// Gauge: total number of snapshots stored across all pools.
pub static SHADOW_LEDGER_TOTAL_SNAPSHOTS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "shadow_ledger_total_snapshots",
        "Total MarketSnapshots stored across all committed pools",
    )
    .expect("Failed to create shadow_ledger_total_snapshots metric")
});

/// Gauge: fraction of Gatekeeper-buffered txs that were committed (percent, 0–100).
///
/// A value below 100 % means txs are being dropped by the Gatekeeper
/// (observation-window timeout or veto).
pub static GATEKEEPER_COMMIT_RATIO: Lazy<Gauge> = Lazy::new(|| {
    Gauge::new(
        "gatekeeper_commit_ratio_pct",
        "Fraction of Gatekeeper-buffered txs that were committed to Shadow Ledger (percent)",
    )
    .expect("Failed to create gatekeeper_commit_ratio_pct metric")
});

/// Gauge: fraction of Gatekeeper-buffered txs that were dropped (percent, 0–100).
pub static GATEKEEPER_DROP_RATIO: Lazy<Gauge> = Lazy::new(|| {
    Gauge::new(
        "gatekeeper_drop_ratio_pct",
        "Fraction of Gatekeeper-buffered txs dropped (observation-window timeout, percent)",
    )
    .expect("Failed to create gatekeeper_drop_ratio_pct metric")
});

/// Counter: total number of lagged events skipped by EventBus consumers.
/// Labels: consumer
pub static EVENTBUS_LAG_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "eventbus_lag_total",
            "Total broadcast event bus messages skipped due to RecvError::Lagged"
        ),
        &["consumer"],
    )
    .expect("Failed to create eventbus_lag_total metric")
});

/// Gauge: current number of active EventBus receivers.
pub static EVENTBUS_ACTIVE_RECEIVERS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "eventbus_active_receivers",
        "Current number of active broadcast EventBus receivers",
    )
    .expect("Failed to create eventbus_active_receivers metric")
});

/// Gauge: current number of signer entries retained by the CPV rolling index.
pub static CPV_INDEX_ENTRIES: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "cpv_index_entries",
        "Current number of signer entries retained by the cross-pool velocity index",
    )
    .expect("Failed to create cpv_index_entries metric")
});

/// Counter: total number of signer entries evicted from the CPV rolling index.
pub static CPV_INDEX_EVICTIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "cpv_index_evictions_total",
        "Total signer entries evicted from the cross-pool velocity index",
    )
    .expect("Failed to create cpv_index_evictions_total metric")
});

/// Counter: total number of signer history lookups that hit the CPV rolling index.
pub static CPV_LOOKUP_HITS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "cpv_lookup_hits_total",
        "Total signer history lookups that hit the cross-pool velocity index",
    )
    .expect("Failed to create cpv_lookup_hits_total metric")
});

/// Counter: total number of signer history lookups that missed the CPV rolling index.
pub static CPV_LOOKUP_MISSES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "cpv_lookup_misses_total",
        "Total signer history lookups that missed the cross-pool velocity index",
    )
    .expect("Failed to create cpv_lookup_misses_total metric")
});

/// Gauge: current number of recipient entries retained by the FSC rolling index.
pub static FSC_INDEX_ENTRIES: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_index_entries",
        "Current number of recipient entries retained by the funding-source index",
    )
    .expect("Failed to create fsc_index_entries metric")
});

/// Counter: total number of per-recipient history overflows in the FSC rolling index.
pub static FSC_INDEX_PER_RECIPIENT_OVERFLOWS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_index_per_recipient_overflows_total",
        "Total funding-source records evicted because a recipient exceeded its bounded history cap",
    )
    .expect("Failed to create fsc_index_per_recipient_overflows_total metric")
});

/// Counter: total number of recipient entries evicted from the FSC rolling index.
pub static FSC_INDEX_GLOBAL_EVICTIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_index_global_evictions_total",
        "Total recipient entries evicted from the funding-source index",
    )
    .expect("Failed to create fsc_index_global_evictions_total metric")
});

/// Counter: recipient entries removed because the global FSC recipient cap was exceeded.
pub static FSC_INDEX_GLOBAL_CAP_EVICTIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_index_global_cap_evictions_total",
        "Total funding-source recipient entries evicted specifically by the global recipient cap",
    )
    .expect("Failed to create fsc_index_global_cap_evictions_total metric")
});

/// Counter: recipient entries pruned because their retained history aged out of the FSC window.
pub static FSC_INDEX_WINDOW_PRUNES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_index_window_prunes_total",
        "Total funding-source recipient entries pruned because they aged out of the retention window",
    )
    .expect("Failed to create fsc_index_window_prunes_total metric")
});

/// Counter: recipient entries removed after lookup pruned their last retained transfer.
pub static FSC_INDEX_LOOKUP_EMPTY_PRUNES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_index_lookup_empty_prunes_total",
        "Total funding-source recipient entries removed after lookup emptied their retained history",
    )
    .expect("Failed to create fsc_index_lookup_empty_prunes_total metric")
});

/// Gauge: current configured FSC global recipient cap.
pub static FSC_INDEX_GLOBAL_RECIPIENT_CAP: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_index_global_recipient_cap",
        "Current configured global recipient cap for the funding-source index",
    )
    .expect("Failed to create fsc_index_global_recipient_cap metric")
});

/// Gauge: current configured FSC per-recipient transfer history cap.
pub static FSC_INDEX_PER_RECIPIENT_CAP: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_index_per_recipient_cap",
        "Current configured per-recipient transfer history cap for the funding-source index",
    )
    .expect("Failed to create fsc_index_per_recipient_cap metric")
});

/// Gauge: current configured FSC retention window in milliseconds.
pub static FSC_INDEX_LOOKBACK_WINDOW_MS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_index_lookback_window_ms",
        "Current configured lookback/retention window for the funding-source index in milliseconds",
    )
    .expect("Failed to create fsc_index_lookback_window_ms metric")
});

/// Gauge: configured maximum retained transfer records implied by current FSC caps.
pub static FSC_INDEX_CONFIGURED_TRANSFER_CAPACITY: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_index_configured_transfer_capacity",
        "Configured maximum retained transfer records implied by FSC global and per-recipient caps",
    )
    .expect("Failed to create fsc_index_configured_transfer_capacity metric")
});

/// Gauge: retained recipient tombstones used to classify cap-evicted lookup misses.
pub static FSC_INDEX_EVICTED_RECIPIENT_ENTRIES: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_index_evicted_recipient_entries",
        "Current number of cap-evicted recipient tombstones retained for FSC miss attribution",
    )
    .expect("Failed to create fsc_index_evicted_recipient_entries metric")
});

/// Gauge: rough retained-memory estimate for FSC rolling state under current occupancy/caps.
pub static FSC_INDEX_ESTIMATED_MEMORY_BYTES: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_index_estimated_memory_bytes",
        "Approximate FSC rolling-index memory footprint in bytes based on current recipients and configured history cap",
    )
    .expect("Failed to create fsc_index_estimated_memory_bytes metric")
});

/// Counter: FSC evidence status emitted at decision-time.
pub static FSC_EVIDENCE_STATUS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "fsc_evidence_status_total",
            "Decision-time FSC evidence status counts"
        ),
        &["status"],
    )
    .expect("Failed to create fsc_evidence_status_total metric")
});

/// Counter: total number of recipient funding lookups that hit the FSC rolling index.
pub static FSC_LOOKUP_HITS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_lookup_hits_total",
        "Total recipient funding lookups that hit the funding-source index",
    )
    .expect("Failed to create fsc_lookup_hits_total metric")
});

/// Counter: total number of recipient funding lookups that missed the FSC rolling index.
pub static FSC_LOOKUP_MISSES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_lookup_misses_total",
        "Total recipient funding lookups that missed the funding-source index",
    )
    .expect("Failed to create fsc_lookup_misses_total metric")
});

/// Counter: FSC lookup misses split by taxonomy reason/class.
pub static FSC_LOOKUP_MISS_REASON_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "fsc_lookup_miss_reason_total",
            "Total FSC lookup misses partitioned by miss taxonomy reason and class"
        ),
        &["reason", "class"],
    )
    .expect("Failed to create fsc_lookup_miss_reason_total metric")
});

/// Gauge: whether the authoritative FSC funding stream is currently available (0/1).
pub static FSC_AUTHORITATIVE_FUNDING_STREAM_AVAILABLE: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_authoritative_funding_stream_available",
        "Whether the authoritative funding-source lane is currently available to runtime (0 or 1)",
    )
    .expect("Failed to create fsc_authoritative_funding_stream_available metric")
});

/// Gauge: whether the FSC stream/index path is warm enough to serve lookups (0/1).
pub static FSC_WARMUP_READY: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_warmup_ready",
        "Whether the authoritative funding-source stream has warmed the rolling index (0 or 1)",
    )
    .expect("Failed to create fsc_warmup_ready metric")
});

/// Gauge: whether the authoritative funding lane has covered the full FSC lookback window (0/1).
pub static FSC_COVERAGE_WINDOW_READY: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_coverage_window_ready",
        "Whether the authoritative funding lane has continuously covered the full FSC lookback window (0 or 1)",
    )
    .expect("Failed to create fsc_coverage_window_ready metric")
});

/// Gauge: milliseconds remaining until the authoritative funding lane covers the FSC lookback window.
pub static FSC_COVERAGE_WINDOW_REMAINING_MS: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_coverage_window_remaining_ms",
        "Milliseconds remaining until authoritative funding coverage spans the full FSC lookback window",
    )
    .expect("Failed to create fsc_coverage_window_remaining_ms metric")
});

/// Gauge: whether authoritative/live BUY is currently permitted by the FSC coverage gate (0/1).
pub static FSC_AUTHORITATIVE_BUY_GATE_OPEN: Lazy<IntGauge> = Lazy::new(|| {
    IntGauge::new(
        "fsc_authoritative_buy_gate_open",
        "Whether authoritative live BUY is currently open under the FSC coverage gate (0 or 1)",
    )
    .expect("Failed to create fsc_authoritative_buy_gate_open metric")
});

/// Gauge: cumulative FSC lookup hit-rate derived from total hits / (hits + misses).
pub static FSC_LOOKUP_HIT_RATE: Lazy<Gauge> = Lazy::new(|| {
    Gauge::new(
        "fsc_lookup_hit_rate",
        "Cumulative hit-rate for funding-source lookups after warmup",
    )
    .expect("Failed to create fsc_lookup_hit_rate metric")
});

/// Histogram: prune duration for FSC rolling-index maintenance.
pub static FSC_PRUNE_DURATION_MS: Lazy<Histogram> = Lazy::new(|| {
    Histogram::with_opts(HistogramOpts::new(
        "fsc_prune_duration_ms",
        "Duration of funding-source index prune passes in milliseconds",
    ))
    .expect("Failed to create fsc_prune_duration_ms metric")
});

/// Counter: durable rows written to system_transfers_raw_v1.jsonl.
pub static FSC_SYSTEM_TRANSFERS_RAW_WRITTEN_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_system_transfers_raw_written_total",
        "Durable raw native SOL transfer rows queued for system_transfers_raw_v1.jsonl",
    )
    .expect("Failed to create fsc_system_transfers_raw_written_total metric")
});

/// Counter: durable rows written to funding_events_v1.jsonl.
pub static FSC_FUNDING_EVENTS_WRITTEN_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::new(
        "fsc_funding_events_written_total",
        "Durable normalized funding-event rows queued for funding_events_v1.jsonl",
    )
    .expect("Failed to create fsc_funding_events_written_total metric")
});

/// Gauge: available transfer evidence sources for FSC capture (0/1).
pub static FSC_TRANSFER_SOURCE_AVAILABLE: Lazy<IntGaugeVec> = Lazy::new(|| {
    IntGaugeVec::new(
        prometheus::opts!(
            "fsc_transfer_source_available",
            "Whether an FSC transfer evidence source is available to capture (0 or 1)"
        ),
        &["source"],
    )
    .expect("Failed to create fsc_transfer_source_available metric")
});

/// Counter for APS regime distribution (provisional until post-V2.5 outcome tracker).
/// Labels: regime ("Normal", "HighVolatility", "LowVolatility")
pub static GATEKEEPER_APS_REGIME_DISTRIBUTION: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "gatekeeper_aps_regime_distribution_total",
            "Distribution of detected market regimes (provisional, pool-local heuristic)"
        ),
        &["regime"],
    )
    .expect("Failed to create gatekeeper_aps_regime_distribution_total metric")
});

/// Counter for DOW timer-fired shadow checkpoint evaluations.
/// Labels: stage ("Early", "Normal", "Extended")
pub static GATEKEEPER_DOW_TIMER_FIRED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "gatekeeper_dow_timer_fired_total",
            "Total DOW timer-fired shadow checkpoints by stage"
        ),
        &["stage"],
    )
    .expect("Failed to create gatekeeper_dow_timer_fired_total metric")
});

static FSC_LOOKUP_HITS_ACCUM: AtomicU64 = AtomicU64::new(0);
static FSC_LOOKUP_MISSES_ACCUM: AtomicU64 = AtomicU64::new(0);

/// Initialize oracle metrics with the Prometheus registry
pub fn register_oracle_metrics(registry: &Registry) -> Result<(), Box<dyn std::error::Error>> {
    registry.register(Box::new(ORACLE_GATHER_EVENTS_REAL.clone()))?;
    registry.register(Box::new(ORACLE_GATHER_EVENTS_SYNTHETIC.clone()))?;
    registry.register(Box::new(ORACLE_GATHER_LAST_REAL_COUNT.clone()))?;
    registry.register(Box::new(POOL_IDENTITY_PROMOTION_TOTAL.clone()))?;
    registry.register(Box::new(POOL_IDENTITY_EXHAUSTED_TOTAL.clone()))?;
    // Shadow Ledger health gauges
    registry.register(Box::new(SHADOW_LEDGER_COVERAGE_RATIO.clone()))?;
    registry.register(Box::new(SHADOW_LEDGER_COMMITTED_POOLS.clone()))?;
    registry.register(Box::new(SHADOW_LEDGER_TOTAL_SNAPSHOTS.clone()))?;
    registry.register(Box::new(GATEKEEPER_COMMIT_RATIO.clone()))?;
    registry.register(Box::new(GATEKEEPER_DROP_RATIO.clone()))?;
    registry.register(Box::new(EVENTBUS_LAG_TOTAL.clone()))?;
    registry.register(Box::new(EVENTBUS_ACTIVE_RECEIVERS.clone()))?;
    registry.register(Box::new(GATEKEEPER_DOW_TIMER_FIRED_TOTAL.clone()))?;
    registry.register(Box::new(GATEKEEPER_APS_REGIME_DISTRIBUTION.clone()))?;
    registry.register(Box::new(GATEKEEPER_SHADOW_LIFECYCLE_STATUS_TOTAL.clone()))?;
    registry.register(Box::new(CPV_INDEX_ENTRIES.clone()))?;
    registry.register(Box::new(CPV_INDEX_EVICTIONS_TOTAL.clone()))?;
    registry.register(Box::new(CPV_LOOKUP_HITS_TOTAL.clone()))?;
    registry.register(Box::new(CPV_LOOKUP_MISSES_TOTAL.clone()))?;
    registry.register(Box::new(FSC_INDEX_ENTRIES.clone()))?;
    registry.register(Box::new(FSC_INDEX_PER_RECIPIENT_OVERFLOWS_TOTAL.clone()))?;
    registry.register(Box::new(FSC_INDEX_GLOBAL_EVICTIONS_TOTAL.clone()))?;
    registry.register(Box::new(FSC_INDEX_GLOBAL_CAP_EVICTIONS_TOTAL.clone()))?;
    registry.register(Box::new(FSC_INDEX_WINDOW_PRUNES_TOTAL.clone()))?;
    registry.register(Box::new(FSC_INDEX_LOOKUP_EMPTY_PRUNES_TOTAL.clone()))?;
    registry.register(Box::new(FSC_INDEX_GLOBAL_RECIPIENT_CAP.clone()))?;
    registry.register(Box::new(FSC_INDEX_PER_RECIPIENT_CAP.clone()))?;
    registry.register(Box::new(FSC_INDEX_LOOKBACK_WINDOW_MS.clone()))?;
    registry.register(Box::new(FSC_INDEX_CONFIGURED_TRANSFER_CAPACITY.clone()))?;
    registry.register(Box::new(FSC_INDEX_EVICTED_RECIPIENT_ENTRIES.clone()))?;
    registry.register(Box::new(FSC_INDEX_ESTIMATED_MEMORY_BYTES.clone()))?;
    registry.register(Box::new(FSC_EVIDENCE_STATUS_TOTAL.clone()))?;
    registry.register(Box::new(FSC_LOOKUP_HITS_TOTAL.clone()))?;
    registry.register(Box::new(FSC_LOOKUP_MISSES_TOTAL.clone()))?;
    registry.register(Box::new(FSC_LOOKUP_MISS_REASON_TOTAL.clone()))?;
    registry.register(Box::new(FSC_AUTHORITATIVE_FUNDING_STREAM_AVAILABLE.clone()))?;
    registry.register(Box::new(FSC_WARMUP_READY.clone()))?;
    registry.register(Box::new(FSC_COVERAGE_WINDOW_READY.clone()))?;
    registry.register(Box::new(FSC_COVERAGE_WINDOW_REMAINING_MS.clone()))?;
    registry.register(Box::new(FSC_AUTHORITATIVE_BUY_GATE_OPEN.clone()))?;
    registry.register(Box::new(FSC_LOOKUP_HIT_RATE.clone()))?;
    registry.register(Box::new(FSC_PRUNE_DURATION_MS.clone()))?;
    registry.register(Box::new(FSC_SYSTEM_TRANSFERS_RAW_WRITTEN_TOTAL.clone()))?;
    registry.register(Box::new(FSC_FUNDING_EVENTS_WRITTEN_TOTAL.clone()))?;
    registry.register(Box::new(FSC_TRANSFER_SOURCE_AVAILABLE.clone()))?;
    Ok(())
}

/// Update Shadow Ledger health gauges.
///
/// Call this periodically (e.g. every 5 s from the oracle heartbeat task) to keep
/// the Prometheus gauges up-to-date.
///
/// # Arguments
/// - `committed_pools`  — from `ShadowLedger::committed_pool_count()`
/// - `total_snapshots`  — from `ShadowLedger::total_snapshot_count()`
pub fn record_shadow_ledger_health(committed_pools: usize, total_snapshots: usize) {
    let coverage = pipeline_coverage().snapshot();

    // Coverage ratio: shadow_ledger_total / grpc_received * 100.
    // Using grpc_received (not chain_truth) as denominator so the gauge stays
    // meaningful in tx-only mode where chain_truth may be 0.
    let coverage_pct = if coverage.grpc_received == 0 {
        0.0
    } else {
        (coverage.shadow_ledger_total() as f64 / coverage.grpc_received as f64) * 100.0
    };
    SHADOW_LEDGER_COVERAGE_RATIO.set(coverage_pct);

    // Gatekeeper pass/drop ratios
    GATEKEEPER_COMMIT_RATIO.set(coverage.gatekeeper_commit_ratio());
    GATEKEEPER_DROP_RATIO.set(coverage.gatekeeper_drop_ratio());

    // Shadow Ledger state sizes
    SHADOW_LEDGER_COMMITTED_POOLS.set(committed_pools as i64);
    SHADOW_LEDGER_TOTAL_SNAPSHOTS.set(total_snapshots as i64);
}

/// Record oracle event gathering metrics
/// This uses separate counters for real and synthetic events to avoid high cardinality.
pub fn record_gather_events(pool_amm_id: &str, real_count: usize, synthetic_count: usize) {
    // Increment real events counter
    if real_count > 0 {
        ORACLE_GATHER_EVENTS_REAL
            .with_label_values(&[pool_amm_id])
            .inc_by(real_count as u64);
    }

    // Increment synthetic events counter
    if synthetic_count > 0 {
        ORACLE_GATHER_EVENTS_SYNTHETIC
            .with_label_values(&[pool_amm_id])
            .inc_by(synthetic_count as u64);
    }

    // Set gauge for last real count
    ORACLE_GATHER_LAST_REAL_COUNT
        .with_label_values(&[pool_amm_id])
        .set(real_count as i64);
}

/// P5: Counter for shadow lifecycle dispatch outcomes.
/// Labels: status ("no_dispatch_eligible", "no_dispatch_rejected", "dispatched", "failed_reconciliation")
pub static GATEKEEPER_SHADOW_LIFECYCLE_STATUS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        prometheus::opts!(
            "gatekeeper_shadow_lifecycle_status_total",
            "Shadow lifecycle dispatch outcomes — distinguishes no_dispatch from failed_reconciliation"
        ),
        &["status"],
    )
    .expect("Failed to create gatekeeper_shadow_lifecycle_status_total metric")
});

/// Record a shadow lifecycle dispatch outcome.
pub fn record_shadow_lifecycle_status(status: &str) {
    GATEKEEPER_SHADOW_LIFECYCLE_STATUS_TOTAL
        .with_label_values(&[status])
        .inc();
}

/// Record an APS regime observation (provisional, pool-local heuristic).
pub fn record_aps_regime(regime: &str) {
    GATEKEEPER_APS_REGIME_DISTRIBUTION
        .with_label_values(&[regime])
        .inc();
}

/// Record a DOW timer-fired shadow checkpoint evaluation for the given stage.
pub fn record_dow_timer_fired(stage: &str) {
    GATEKEEPER_DOW_TIMER_FIRED_TOTAL
        .with_label_values(&[stage])
        .inc();
}

pub fn record_eventbus_lag(consumer: &str, skipped: u64) {
    if skipped > 0 {
        EVENTBUS_LAG_TOTAL
            .with_label_values(&[consumer])
            .inc_by(skipped);
    }
}

pub fn record_eventbus_active_receivers(count: usize) {
    EVENTBUS_ACTIVE_RECEIVERS.set(count as i64);
}

pub fn record_cpv_index_entries(entries: usize) {
    CPV_INDEX_ENTRIES.set(entries as i64);
}

pub fn record_cpv_index_evictions(evictions: u64) {
    if evictions > 0 {
        CPV_INDEX_EVICTIONS_TOTAL.inc_by(evictions);
    }
}

pub fn record_cpv_lookup_hits(hits: u64) {
    if hits > 0 {
        CPV_LOOKUP_HITS_TOTAL.inc_by(hits);
    }
}

pub fn record_cpv_lookup_misses(misses: u64) {
    if misses > 0 {
        CPV_LOOKUP_MISSES_TOTAL.inc_by(misses);
    }
}

pub fn record_fsc_index_entries(entries: usize) {
    FSC_INDEX_ENTRIES.set(entries as i64);
}

pub fn record_fsc_index_per_recipient_overflows(overflows: u64) {
    if overflows > 0 {
        FSC_INDEX_PER_RECIPIENT_OVERFLOWS_TOTAL.inc_by(overflows);
    }
}

pub fn record_fsc_index_global_evictions(evictions: u64) {
    if evictions > 0 {
        FSC_INDEX_GLOBAL_EVICTIONS_TOTAL.inc_by(evictions);
    }
}

pub fn record_fsc_index_global_cap_evictions(evictions: u64) {
    if evictions > 0 {
        FSC_INDEX_GLOBAL_CAP_EVICTIONS_TOTAL.inc_by(evictions);
    }
}

pub fn record_fsc_index_window_prunes(prunes: u64) {
    if prunes > 0 {
        FSC_INDEX_WINDOW_PRUNES_TOTAL.inc_by(prunes);
    }
}

pub fn record_fsc_index_lookup_empty_prunes(prunes: u64) {
    if prunes > 0 {
        FSC_INDEX_LOOKUP_EMPTY_PRUNES_TOTAL.inc_by(prunes);
    }
}

pub fn record_fsc_retention_config(
    global_recipient_cap: usize,
    per_recipient_cap: usize,
    lookback_window_ms: u64,
) {
    FSC_INDEX_GLOBAL_RECIPIENT_CAP.set(usize_to_i64(global_recipient_cap));
    FSC_INDEX_PER_RECIPIENT_CAP.set(usize_to_i64(per_recipient_cap));
    FSC_INDEX_LOOKBACK_WINDOW_MS.set(u64_to_i64(lookback_window_ms));
    FSC_INDEX_CONFIGURED_TRANSFER_CAPACITY.set(u128_to_i64(
        (global_recipient_cap as u128).saturating_mul(per_recipient_cap as u128),
    ));
}

pub fn record_fsc_index_evicted_recipient_entries(entries: usize) {
    FSC_INDEX_EVICTED_RECIPIENT_ENTRIES.set(usize_to_i64(entries));
}

pub fn record_fsc_index_estimated_memory_bytes(
    recipient_entries: usize,
    evicted_recipient_entries: usize,
    per_recipient_cap: usize,
) {
    const RECIPIENT_HISTORY_BYTES: u128 = 256;
    const TRANSFER_RECORD_WITH_HEAP_BYTES: u128 = 384;
    const EVICTED_RECIPIENT_BYTES: u128 = 96;

    let recipient_entries = recipient_entries as u128;
    let evicted_recipient_entries = evicted_recipient_entries as u128;
    let per_recipient_cap = per_recipient_cap as u128;
    let transfer_capacity = recipient_entries.saturating_mul(per_recipient_cap);
    let estimated_bytes = recipient_entries
        .saturating_mul(RECIPIENT_HISTORY_BYTES)
        .saturating_add(transfer_capacity.saturating_mul(TRANSFER_RECORD_WITH_HEAP_BYTES))
        .saturating_add(evicted_recipient_entries.saturating_mul(EVICTED_RECIPIENT_BYTES));
    FSC_INDEX_ESTIMATED_MEMORY_BYTES.set(u128_to_i64(estimated_bytes));
}

pub fn record_fsc_evidence_status(status: &str) {
    FSC_EVIDENCE_STATUS_TOTAL.with_label_values(&[status]).inc();
}

pub fn record_fsc_lookup_hits(hits: u64) {
    if hits > 0 {
        FSC_LOOKUP_HITS_TOTAL.inc_by(hits);
        FSC_LOOKUP_HITS_ACCUM.fetch_add(hits, Ordering::Relaxed);
    }
    refresh_fsc_lookup_hit_rate();
}

pub fn record_fsc_lookup_misses(misses: u64) {
    if misses > 0 {
        FSC_LOOKUP_MISSES_TOTAL.inc_by(misses);
        FSC_LOOKUP_MISSES_ACCUM.fetch_add(misses, Ordering::Relaxed);
    }
    refresh_fsc_lookup_hit_rate();
}

pub fn record_fsc_lookup_miss_reason(reason: &str, class: FscMissClass, misses: u64) {
    if misses > 0 {
        FSC_LOOKUP_MISS_REASON_TOTAL
            .with_label_values(&[reason, class.as_str()])
            .inc_by(misses);
    }
}

pub fn record_fsc_authoritative_funding_stream_available(available: bool) {
    FSC_AUTHORITATIVE_FUNDING_STREAM_AVAILABLE.set(if available { 1 } else { 0 });
}

pub fn record_fsc_warmup_ready(ready: bool) {
    FSC_WARMUP_READY.set(if ready { 1 } else { 0 });
}

pub fn record_fsc_coverage_window_ready(ready: bool) {
    FSC_COVERAGE_WINDOW_READY.set(if ready { 1 } else { 0 });
}

pub fn record_fsc_coverage_window_remaining_ms(remaining_ms: u64) {
    FSC_COVERAGE_WINDOW_REMAINING_MS.set(remaining_ms.min(i64::MAX as u64) as i64);
}

pub fn record_fsc_authoritative_buy_gate_open(open: bool) {
    FSC_AUTHORITATIVE_BUY_GATE_OPEN.set(if open { 1 } else { 0 });
}

pub fn record_fsc_prune_duration_ms(duration_ms: f64) {
    FSC_PRUNE_DURATION_MS.observe(duration_ms.max(0.0));
}

pub fn record_fsc_system_transfers_raw_written(count: u64) {
    if count > 0 {
        FSC_SYSTEM_TRANSFERS_RAW_WRITTEN_TOTAL.inc_by(count);
    }
}

pub fn record_fsc_funding_events_written(count: u64) {
    if count > 0 {
        FSC_FUNDING_EVENTS_WRITTEN_TOTAL.inc_by(count);
    }
}

pub fn record_fsc_transfer_source_available(source: &str, available: bool) {
    FSC_TRANSFER_SOURCE_AVAILABLE
        .with_label_values(&[source])
        .set(if available { 1 } else { 0 });
}

fn refresh_fsc_lookup_hit_rate() {
    let hits = FSC_LOOKUP_HITS_ACCUM.load(Ordering::Relaxed);
    let misses = FSC_LOOKUP_MISSES_ACCUM.load(Ordering::Relaxed);
    let total = hits.saturating_add(misses);
    let rate = if total == 0 {
        0.0
    } else {
        hits as f64 / total as f64
    };
    FSC_LOOKUP_HIT_RATE.set(rate);
}

fn usize_to_i64(value: usize) -> i64 {
    value.min(i64::MAX as usize) as i64
}

fn u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn u128_to_i64(value: u128) -> i64 {
    value.min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eventbus_metrics_register_and_update() {
        let registry = Registry::new();
        register_oracle_metrics(&registry).expect("register oracle metrics");

        record_eventbus_active_receivers(4);
        record_eventbus_lag("oracle_runtime", 3);
        record_cpv_index_entries(7);
        record_cpv_index_evictions(2);
        record_cpv_lookup_hits(4);
        record_cpv_lookup_misses(1);
        record_fsc_index_entries(5);
        record_fsc_index_per_recipient_overflows(3);
        record_fsc_index_global_evictions(2);
        record_fsc_index_global_cap_evictions(1);
        record_fsc_index_window_prunes(1);
        record_fsc_index_lookup_empty_prunes(1);
        record_fsc_retention_config(50_000, 256, 300_000);
        record_fsc_index_evicted_recipient_entries(2);
        record_fsc_index_estimated_memory_bytes(5, 2, 256);
        record_fsc_evidence_status("clean");
        record_fsc_lookup_hits(6);
        record_fsc_lookup_misses(1);
        record_fsc_lookup_miss_reason(
            "FSC_NO_RETAINED_RECIPIENT_HISTORY",
            FscMissClass::Indeterminate,
            1,
        );
        record_fsc_authoritative_funding_stream_available(true);
        record_fsc_warmup_ready(true);
        record_fsc_coverage_window_ready(true);
        record_fsc_coverage_window_remaining_ms(0);
        record_fsc_authoritative_buy_gate_open(true);
        record_fsc_prune_duration_ms(1.5);
        record_fsc_system_transfers_raw_written(3);
        record_fsc_funding_events_written(3);
        record_fsc_transfer_source_available("grpc_funding_lane_full_chain", true);
        record_fsc_transfer_source_available("program_stream_system_transfers", false);

        let metric_families = registry.gather();
        let names: Vec<_> = metric_families
            .iter()
            .map(|family| family.get_name().to_string())
            .collect();

        assert!(names.iter().any(|name| name == "eventbus_active_receivers"));
        assert!(names.iter().any(|name| name == "eventbus_lag_total"));
        assert!(names.iter().any(|name| name == "cpv_index_entries"));
        assert!(names.iter().any(|name| name == "cpv_index_evictions_total"));
        assert!(names.iter().any(|name| name == "cpv_lookup_hits_total"));
        assert!(names.iter().any(|name| name == "cpv_lookup_misses_total"));
        assert!(names.iter().any(|name| name == "fsc_index_entries"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_per_recipient_overflows_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_global_evictions_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_global_cap_evictions_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_window_prunes_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_lookup_empty_prunes_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_global_recipient_cap"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_per_recipient_cap"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_lookback_window_ms"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_configured_transfer_capacity"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_evicted_recipient_entries"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_index_estimated_memory_bytes"));
        assert!(names.iter().any(|name| name == "fsc_evidence_status_total"));
        assert!(names.iter().any(|name| name == "fsc_lookup_hits_total"));
        assert!(names.iter().any(|name| name == "fsc_lookup_misses_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_lookup_miss_reason_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_authoritative_funding_stream_available"));
        assert!(names.iter().any(|name| name == "fsc_warmup_ready"));
        assert!(names.iter().any(|name| name == "fsc_coverage_window_ready"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_coverage_window_remaining_ms"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_authoritative_buy_gate_open"));
        assert!(names.iter().any(|name| name == "fsc_lookup_hit_rate"));
        assert!(names.iter().any(|name| name == "fsc_prune_duration_ms"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_system_transfers_raw_written_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_funding_events_written_total"));
        assert!(names
            .iter()
            .any(|name| name == "fsc_transfer_source_available"));

        let lag_family = metric_families
            .iter()
            .find(|family| family.get_name() == "eventbus_lag_total")
            .expect("eventbus_lag_total family");
        assert!(
            lag_family
                .get_metric()
                .iter()
                .any(|metric| metric.get_counter().get_value() >= 3.0),
            "expected eventbus_lag_total counter to be incremented"
        );
        assert_eq!(CPV_INDEX_ENTRIES.get(), 7);
        assert_eq!(CPV_INDEX_EVICTIONS_TOTAL.get(), 2);
        assert_eq!(CPV_LOOKUP_HITS_TOTAL.get(), 4);
        assert_eq!(CPV_LOOKUP_MISSES_TOTAL.get(), 1);
        assert_eq!(FSC_INDEX_ENTRIES.get(), 5);
        assert_eq!(FSC_INDEX_PER_RECIPIENT_OVERFLOWS_TOTAL.get(), 3);
        assert_eq!(FSC_INDEX_GLOBAL_EVICTIONS_TOTAL.get(), 2);
        assert_eq!(FSC_INDEX_GLOBAL_CAP_EVICTIONS_TOTAL.get(), 1);
        assert_eq!(FSC_INDEX_WINDOW_PRUNES_TOTAL.get(), 1);
        assert_eq!(FSC_INDEX_LOOKUP_EMPTY_PRUNES_TOTAL.get(), 1);
        assert_eq!(FSC_INDEX_GLOBAL_RECIPIENT_CAP.get(), 50_000);
        assert_eq!(FSC_INDEX_PER_RECIPIENT_CAP.get(), 256);
        assert_eq!(FSC_INDEX_LOOKBACK_WINDOW_MS.get(), 300_000);
        assert_eq!(FSC_INDEX_CONFIGURED_TRANSFER_CAPACITY.get(), 50_000 * 256);
        assert_eq!(FSC_INDEX_EVICTED_RECIPIENT_ENTRIES.get(), 2);
        assert!(FSC_INDEX_ESTIMATED_MEMORY_BYTES.get() > 0);
        assert_eq!(FSC_LOOKUP_HITS_TOTAL.get(), 6);
        assert_eq!(FSC_LOOKUP_MISSES_TOTAL.get(), 1);
        assert_eq!(FSC_AUTHORITATIVE_FUNDING_STREAM_AVAILABLE.get(), 1);
        assert_eq!(FSC_WARMUP_READY.get(), 1);
        assert_eq!(FSC_SYSTEM_TRANSFERS_RAW_WRITTEN_TOTAL.get(), 3);
        assert_eq!(FSC_FUNDING_EVENTS_WRITTEN_TOTAL.get(), 3);
        assert_eq!(
            FSC_TRANSFER_SOURCE_AVAILABLE
                .with_label_values(&["grpc_funding_lane_full_chain"])
                .get(),
            1
        );
        assert_eq!(
            FSC_TRANSFER_SOURCE_AVAILABLE
                .with_label_values(&["program_stream_system_transfers"])
                .get(),
            0
        );
        assert!((FSC_LOOKUP_HIT_RATE.get() - (6.0 / 7.0)).abs() < f64::EPSILON);
    }
}
