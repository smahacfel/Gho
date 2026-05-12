//! Oracle Decision Logger
//!
//! This module implements comprehensive logging of Oracle Brain scoring decisions
//! including initialScore, followupScores (1s, 5s, 30s, 60s), and all corrections
//! with explicit reasons.
//!
//! # Design Principles
//! 1. **Comprehensive Coverage**: Log all decision points and corrections
//! 2. **Minimal Latency**: Async writes with fire-and-forget pattern
//! 3. **JSONL Format**: One decision record per line for easy processing
//! 4. **Full Traceability**: Capture reasons for all score adjustments
//!
//! # Log Schema
//! ```json
//! {
//!   "candidate_id": "pool_amm_id",
//!   "timestamp": 1234567890,
//!   "initialScore": 62,
//!   "initial_decision": "BUY",
//!   "initial_components": {
//!     "base_shadow": 60,
//!     "qass_score": 78,
//!     "qedd_survival_30s": 0.71,
//!     "mci": 0.74,
//!     "chaos_loss_prob": 0.12,
//!     "gene_match_score": 0.03
//!   },
//!   "followupScores": [
//!     { "t_ms": 1000, "score": 58, "reason": "small drop in QASS", "corrections": [...] },
//!     { "t_ms": 5000, "score": 45, "reason": "mci drop & chaos risk", "corrections": [...] },
//!     { "t_ms": 30000, "score": 40, "reason": "QEDD λ spike", "corrections": [...] },
//!     { "t_ms": 60000, "score": 35, "reason": "sustained MCI decline", "corrections": [...] }
//!   ],
//!   "veto": null,
//!   "final_decision": "HOLD"
//! }
//! ```

use anyhow::{Context, Result};
use ghost_core::health::RuntimeHealth;
use ghost_core::tx_intelligence::types::FundingSourceDiagnostics;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs::{create_dir_all, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Maximum number of ab_record_id entries held in the dedup cache.
const DEDUP_MAX_CAPACITY: usize = 100_000;
/// TTL for dedup cache entries (20 minutes). After this period the entry is
/// eligible for eviction, so a legitimately-retried write (very unlikely but
/// theoretically possible for pool IDs that reappear after a restart) would
/// pass through rather than be silently dropped forever.
const DEDUP_TTL: Duration = Duration::from_secs(20 * 60);

/// Default directory for decision logs
pub const DEFAULT_DECISION_LOG_DIR: &str = "datasets/decisions";
pub const CYCLIC_LOG_SCHEMA_VERSION: u32 = 1;
/// v3 adds vectors_* fields (window sequences for DTW/Hill/MI/TDA analysis).
/// v4 adds explicit `curve_finality` tier fields for ShadowLedger-backed Phase 6 telemetry.
/// v7 adds explicit `min_sol_threshold` so historical coverage tooling can stop
/// inferring dust semantics from startup logs.
/// v8 adds explicit clock-source tags for observation identity and curve-t0 telemetry.
/// v10 adds explicit legacy/sybil policy bucket fields to GatekeeperBuyLog.
/// v11 adds explicit alpha-gate diagnostics and thresholds.
/// v12 adds additive FSC miss taxonomy diagnostics.
/// v13 adds opposite gatekeeper threshold bounds (max/min counterparts) to
/// BUY/decision JSONL telemetry.
/// v14 adds strict prosperity-overlay diagnostics and thresholds.
/// v15 adds 3-second early top-3 buy concentration telemetry and thresholding.
/// v16 adds V2.5 shadow decision fields (early/normal window shadow verdicts,
/// observation stage, confidence proxy).
/// v17 separates legacy_live and v25_shadow decision planes and routes records by
/// rollout_profile / gatekeeper_version / decision_plane / config_hash.
/// v18 adds V2.5 confidence breakdown fields so calibration can separate raw
/// model quality from terminal PDD/TAS veto zeroing.
/// v19 adds typed `reason_code` (GatekeeperReasonCode enum, version 2)
/// for all verdict types including TIMEOUT subtypes.
pub const GATEKEEPER_BUY_LOG_SCHEMA_VERSION: u32 = 19;
/// Gatekeeper version string embedded in every V2.5 shadow BUY log for traceability.
pub const GATEKEEPER_VERSION: &str = "v2.5";
/// Legacy Gatekeeper version string for pre-V2.5 live-plane semantics.
pub const LEGACY_GATEKEEPER_VERSION: &str = "v2.2";
const DECISION_PLANE_LEGACY_LIVE: &str = "legacy_live";
const DECISION_PLANE_V25_SHADOW: &str = "v25_shadow";

fn is_no_space_error<'a>(
    errors: impl IntoIterator<Item = &'a (dyn std::error::Error + 'static)>,
) -> bool {
    for cause in errors {
        if cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_err| io_err.raw_os_error() == Some(28))
        {
            return true;
        }
    }
    false
}

fn mark_gatekeeper_log_progress(health: Option<&Arc<RuntimeHealth>>, buy_eligible: bool) {
    if let Some(h) = health {
        h.mark_decisions_write();
        if buy_eligible {
            h.mark_buys_write();
        }
    }
}

/// File name for ALL gatekeeper decisions (PASS + REJECT + TIMEOUT).
pub const GATEKEEPER_DECISIONS_JSONL: &str = "gatekeeper_v2_decisions.jsonl";
/// File name for PASSED/BUY-eligible decisions only (kept as buys.jsonl for pipeline compat).
pub const GATEKEEPER_PASSED_JSONL: &str = "gatekeeper_v2_buys.jsonl";

/// Default value for GatekeeperBuyLog.mode (for backward-compatible deserialization)
fn default_mode_standard() -> String {
    "standard".to_string()
}

fn default_zero_f64() -> f64 {
    0.0
}

fn default_one_f64() -> f64 {
    1.0
}

fn default_large_f64() -> f64 {
    9999.0
}

// =============================================================================
// Cyclic Engine Support (Task: OPERACJA GHOST MODE)
// =============================================================================

/// Metrics snapshot for a single cycle in the S1-S13 heartbeat loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleMetricsSnapshot {
    /// Log schema version for parser compatibility.
    pub log_schema_version: u32,
    /// SOBP (Speed of Buying Pressure) in SOL/sec
    pub sobp: f64,
    /// MPCF (Organic ratio: 0.0=Bot, 1.0=Organic)
    pub mpcf_organic: f64,
    /// Volume change in this cycle (SOL)
    pub volume_delta: f64,
    /// Safety multiplier applied (1.0=safe, <1.0=risky)
    pub safety_multiplier: f64,
    /// Canonical cycle event-time.
    pub cycle_now_event_ts_ms: u64,
    /// Event-time cycle window start.
    pub start_event_ts_ms: u64,
    /// Event-time cycle window end.
    pub end_event_ts_ms: u64,
    /// SOBP input event-time range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sobp_input_event_ts_min: Option<u64>,
    /// SOBP input event-time range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sobp_input_event_ts_max: Option<u64>,
    /// CIR input event-time range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cir_input_event_ts_min: Option<u64>,
    /// CIR input event-time range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cir_input_event_ts_max: Option<u64>,
}

/// Cyclic engine decision log entry (for S1-S13 heartbeat system)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CyclicEngineLog {
    /// Log schema version for parser compatibility.
    pub log_schema_version: u32,
    /// Pool AMM ID
    pub pool_id: String,
    /// Timestamp (RFC3339 format)
    pub timestamp: String,
    /// Event type: "GUNSHOT", "VERDICT", "KILLED"
    pub event_type: String,
    /// Final score (0.0-100.0)
    pub score: f64,
    /// Cycle reached (1-13)
    pub cycle_reached: u8,
    /// Whether this is a dry-run (no real transaction)
    pub is_dry_run: bool,
    /// Metrics snapshot (optional - only for buy/verdict events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_snapshot: Option<CycleMetricsSnapshot>,
    /// Kill reason (only for KILLED events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kill_reason: Option<String>,
    /// Versioned reason-code enum string (if present for no-emit/skipped semantics).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// Reason code schema version.
    pub reason_code_version: u32,
}

// =============================================================================
// Gatekeeper V2 Buy Decision Logging
// =============================================================================

/// Gatekeeper V2 Buy decision log with full phase breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatekeeperBuyLog {
    pub log_schema_version: u32,
    pub timestamp: String, // RFC3339
    pub pool_id: String,

    // ═══════════════════════════════════════════
    // Observation identity (for outcome labeling)
    // ═══════════════════════════════════════════
    /// Stable join key: "{pool_id}:{base_mint}:{first_seen_ts_ms}"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_key: Option<String>,

    /// Base token mint address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_mint: Option<String>,

    /// Unix timestamp (ms) when the pool was first detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_ts_ms: Option<u64>,

    /// Provenance tag for `first_seen_ts_ms` (`registered_wall`, `detected_wall`, `tx_ingress_wall`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_clock_source: Option<String>,

    /// Canonical start of the full Gatekeeper observation window used by
    /// `total_tx_evaluated` / `observation_duration_ms`.
    ///
    /// For current runtime semantics this is identical to `first_seen_ts_ms`,
    /// but it is emitted explicitly so downstream coverage tooling does not
    /// need to infer denominator bounds from legacy aliases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_start_ts_ms: Option<u64>,

    /// Canonical end of the full Gatekeeper observation window used by
    /// `total_tx_evaluated`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_end_ts_ms: Option<u64>,

    /// Canonical full-observation window length used by `total_tx_evaluated`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_window_ms: Option<u64>,

    /// Legacy compatibility alias preserved for older downstream consumers.
    ///
    /// Despite the historical `10s` name, current runtime code may populate it
    /// from a shorter identity/window alias. New tooling must use the explicit
    /// `observation_*` fields above when constructing coverage denominators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_10s_ts_ms: Option<u64>,

    /// Whether all core gates (core1 + core2 + core3) passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_pass: Option<bool>,

    /// Gatekeeper version string (e.g. "v2.2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_version: Option<String>,

    /// Rollout profile that produced this record (e.g. "shadow-burnin").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_profile: Option<String>,

    /// Explicit semantic plane carried by this record (`legacy_live`, `v25_shadow`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_plane: Option<String>,

    /// Blake3 hash of key config thresholds — reproducible across restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_hash: Option<String>,

    /// Developer wallet pubkey, if identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_pubkey: Option<String>,

    /// Whether metadata required for shadow-run is complete at BUY time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_ready: Option<bool>,

    /// Missing metadata fields that prevented a clean shadow-run handoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_missing_fields: Option<Vec<String>>,

    /// Source used to hydrate BUY metadata (`local_task_state`, `runtime_registry`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_metadata_source: Option<String>,

    /// Whether Trigger was present in runtime when BUY fired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_trigger_present: Option<bool>,

    /// Trigger entry mode seen by BUY path (`shadow_only`, `live_and_shadow`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_entry_mode: Option<String>,

    /// Whether runtime config would actually attempt shadow execution for this BUY.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_trigger_eligible: Option<bool>,

    /// Final BUY-path outcome from the shadow/live handoff point of view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_execution_outcome: Option<String>,

    /// Explicit execution correlation id shared by decision/shadow/paper traces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_candidate_id: Option<String>,

    // Mode
    /// Gatekeeper operating mode: "standard" or "long"
    #[serde(default = "default_mode_standard")]
    pub mode: String,

    // Summary
    pub phases_passed: u8,
    pub min_phases_to_pass: u8,
    pub observation_duration_ms: u64,
    pub finalize_lag_ms: u64,
    pub max_wait_time_ms: u64,
    pub eval_count: usize,
    pub dust_filtered_count: u64,
    /// Dust threshold that the Gatekeeper session actually used while deciding
    /// whether a tx contributes to `dust_filtered_count` vs `total_tx_evaluated`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_sol_threshold: Option<f64>,

    // Phase 1: Quantity Gate
    /// Transactions evaluated across the full Gatekeeper observation window,
    /// not just the A/B subwindow.
    pub total_tx_evaluated: usize,
    /// Unique transaction identities evaluated across the full observation window.
    ///
    /// This is the tx-level denominator-compatible count keyed by transaction identity
    /// (signature + event_ordinal fallback), unlike `total_tx_evaluated` which may still
    /// reflect event-level observations for backward compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_tx_evaluated: Option<usize>,
    pub min_tx_count: usize,
    pub unique_signers_evaluated: usize,
    pub min_unique_signers: usize,
    pub buy_count: usize,
    pub min_buy_count: usize,

    // Phase 2: Velocity Profile (measured + thresholds)
    pub phase2_passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_cv: Option<f64>,
    pub min_interval_cv: f64,
    #[serde(default = "default_large_f64")]
    pub max_interval_cv: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burst_ratio: Option<f64>,
    pub max_burst_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_interval_ms: Option<f64>,
    pub min_avg_interval_ms: f64,
    pub max_avg_interval_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing_entropy: Option<f64>,
    pub min_timing_entropy: f64,
    #[serde(default = "default_large_f64")]
    pub max_timing_entropy: f64,
    pub min_dust_filtered_count: u64,

    // Phase 3: Signer Diversity (measured + thresholds)
    pub phase3_passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_ratio: Option<f64>,
    pub min_unique_ratio: f64,
    pub max_unique_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hhi: Option<f64>,
    pub max_hhi: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tx_per_signer_observed: Option<usize>,
    pub max_tx_per_signer: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_gini: Option<f64>,
    #[serde(default)]
    pub min_volume_gini: f64,
    pub max_volume_gini: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top3_volume_pct: Option<f64>,
    pub max_top3_volume_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub same_ms_tx_ratio: Option<f64>,
    pub max_same_ms_tx_ratio: f64,

    // Phase 4: Volume Sanity (measured + thresholds)
    pub phase4_passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_ratio: Option<f64>,
    pub min_buy_ratio: f64,
    pub max_buy_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_tx_sol: Option<f64>,
    pub min_avg_tx_sol: f64,
    pub max_avg_tx_sol: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_cv: Option<f64>,
    pub min_volume_cv: f64,
    #[serde(default = "default_large_f64")]
    pub max_volume_cv: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_volume_sol: Option<f64>,
    pub min_total_volume_sol: f64,
    #[serde(default = "default_large_f64")]
    pub max_total_volume_sol: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sol_buy_ratio: Option<f64>,
    pub min_sol_buy_ratio: f64,
    #[serde(default = "default_one_f64")]
    pub max_sol_buy_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_consecutive_buys_observed: Option<usize>,
    pub min_consecutive_buys: usize,

    // Phase 5: Dev Behavior (measured + thresholds)
    pub phase5_passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_wallet_known: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_buy_total_sol: Option<f64>,
    pub max_dev_buy_sol: f64,
    pub min_dev_buy_sol: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_tx_ratio: Option<f64>,
    pub max_dev_tx_ratio: f64,
    #[serde(default = "default_zero_f64")]
    pub min_dev_tx_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_volume_ratio: Option<f64>,
    pub max_dev_volume_ratio: f64,
    pub min_dev_volume_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_has_sold: Option<bool>,
    pub reject_on_dev_sell: bool,

    // Phase 6: Bonding Curve Dynamics (measured + thresholds)
    pub phase6_passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_change_ratio: Option<f64>,
    #[serde(default = "default_zero_f64")]
    pub min_price_change_ratio: f64,
    pub max_price_change_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_single_tx_price_impact_pct_observed: Option<f64>,
    pub max_single_tx_price_impact_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_single_sell_impact_pct_observed: Option<f64>,
    #[serde(default = "default_zero_f64")]
    pub min_single_sell_impact_pct: f64,
    pub max_single_sell_impact_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonding_progress_pct: Option<f64>,
    pub min_bonding_progress_pct: f64,
    pub max_bonding_progress_pct: f64,
    /// Whether bonding curve data was successfully parsed (explicit parser flag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_data_known: Option<bool>,
    /// Finality tier of the curve state used by Phase 6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_finality: Option<String>,
    /// Convenience flag for downstream parsers that only care about finalized/non-finalized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_finality_is_finalized: Option<bool>,
    /// Whether the bonding progress range-check was skipped (curve_data_known == false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonding_progress_check_skipped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_market_cap_sol: Option<f64>,
    pub min_market_cap_sol: f64,

    // ── Curve Readiness Latch telemetry ────────────────────────────────
    /// Configured curve_wait_ms for this pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_wait_ms: Option<u64>,
    /// Event-time t0 (ms) at which the curve latch started — selected from the
    /// provenance-aware detection clock and only falling back to registered wall time
    /// when no explicit epoch-like event clock is available.
    /// Essential for post-mortem: `curve_wait_elapsed_ms = highest_seen_ts - curve_t0_event_ts_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_t0_event_ts_ms: Option<u64>,
    /// Provenance tag for `curve_t0_event_ts_ms` (`chain_event`, `ingress_wall`, `detected_wall`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_t0_clock_source: Option<String>,
    /// Actual elapsed ms since buffer creation when verdict was issued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_wait_elapsed_ms: Option<u64>,
    /// Whether curve data was required for BUY (always true in current config).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_required_for_buy: Option<bool>,

    // ═══════════════════════════════════════════
    // Three-Layer Decision (optional — populated when use_three_layer_decision=true)
    // ═══════════════════════════════════════════
    /// Whether the three-layer decision system was used
    #[serde(default)]
    pub three_layer_enabled: bool,

    /// Hard fail reason (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hard_fail_reason: Option<String>,

    /// Core-1 (Quantity Gate) passed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core1_passed: Option<bool>,

    /// Core-2 (Capital Dominance) passed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core2_passed: Option<bool>,

    /// Core-3 (Dev + Curve Safety) passed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core3_passed: Option<bool>,

    /// Whether dev wallet was unknown (triggers stricter requirements)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_unknown: Option<bool>,

    /// Soft signal score (0..11) — unweighted flag count (compat)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_score: Option<u8>,

    /// Weighted soft points (group-based scoring)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_points: Option<u8>,

    /// Maximum allowed soft points for this pool (may be reduced for dev_unknown)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_soft_points: Option<u8>,

    /// Effective max soft points used (accounts for dev_unknown override)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_max_soft_points: Option<u8>,

    /// Maximum allowed soft score (deprecated, compat)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_soft_score: Option<u8>,

    /// Comma-separated soft signal flag names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_flags: Option<String>,

    /// Explicit legacy soft points exported alongside the compatibility alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_soft_points: Option<u8>,

    /// Effective legacy soft threshold used for the decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_soft_threshold: Option<u8>,

    /// Explicit legacy soft flags exported alongside the compatibility alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_soft_flags: Option<String>,

    /// Dedicated Sybil Interference bucket score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sybil_soft_points: Option<u16>,

    /// Effective Sybil Interference threshold used for the decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sybil_soft_threshold: Option<u8>,

    /// Legacy + sybil points exported for telemetry only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_soft_points: Option<u16>,

    /// Comma-separated sybil soft signal flag names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sybil_soft_flags: Option<String>,

    /// Lead signal chosen inside the Sybil Interference bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sybil_lead_signal: Option<String>,

    /// Recognized sybil interference patterns for this decision.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sybil_interference_patterns: Vec<String>,

    /// Optional aggregated sybil meta-score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sybil_meta_score: Option<u16>,

    /// Whether verdict-level sybil bucket enforcement was enabled in config.
    #[serde(default)]
    pub sybil_interference_layer_enabled: bool,

    /// Whether combo-veto was enabled in config.
    #[serde(default)]
    pub sybil_combo_veto_enabled: bool,

    /// Final reason chain from three-layer decision
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,

    /// Typed reason code (GatekeeperReasonCode taxonomy, version 2).
    /// Always populated for every verdict type (BUY, REJECT, TIMEOUT).
    /// Replaces the legacy free-form `decision_reason` for machine auditability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,

    /// Reason code taxonomy version (2 = all verdict types, 1 = NoEmit-only).
    #[serde(default)]
    pub reason_code_version: u32,

    /// Final verdict from three-layer system (true=BUY, false=REJECT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_verdict_buy: Option<bool>,

    /// Explicit verdict type: BUY, REJECT_HARD_FAIL, REJECT_CORE_FAIL,
    /// REJECT_SOFT_EXCESS, REJECT_SYBIL_SOFT_EXCESS, REJECT_SYBIL_INTERFERENCE,
    /// REJECT_LOW_ALPHA, REJECT_LOW_PROSPERITY, TIMEOUT_PHASE1, TIMEOUT_NO_DATA
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict_type: Option<String>,

    /// Explicit legacy live-plane reason chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_live_reason_chain: Option<String>,

    /// Explicit legacy live-plane verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_live_verdict_buy: Option<bool>,

    /// Explicit legacy live-plane verdict type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_live_verdict_type: Option<String>,

    /// Explicit V2.5 shadow-plane verdict type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_shadow_verdict_type: Option<String>,

    /// Explicit V2.5 shadow-plane reason chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_shadow_reason_chain: Option<String>,

    /// Explicit V2.5 shadow-plane confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_shadow_confidence: Option<f64>,

    /// Provenance of `v25_shadow_confidence` (`shadow_window_terminal` or `assessment_cached`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_shadow_confidence_source: Option<String>,

    /// Explicit V2.5 shadow-plane terminal observation stage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_shadow_observation_stage: Option<String>,

    /// Promotion state for the V2.5 plane (`shadow_only`, `live_enabled`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_promotion_state: Option<String>,

    /// Whether alpha gate was enabled in config for this decision.
    #[serde(default)]
    pub alpha_gate_enabled: bool,

    /// Whether alpha gate passed. None means the gate did not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_pass: Option<bool>,

    /// Whether alpha gate had enough data to evaluate real scalars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_actionable: Option<bool>,

    /// Alpha momentum scalar in [0,1].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub momentum: Option<f64>,

    /// Alpha demand scalar in [0,1].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand: Option<f64>,

    /// Joint alpha scalar (`momentum * demand`) in [0,1].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_joint: Option<f64>,

    /// Configured alpha momentum threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_momentum: Option<f64>,

    /// Configured alpha demand threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_demand: Option<f64>,

    /// Configured alpha joint threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_alpha_joint: Option<f64>,

    /// Configured minimum sample size for actionable alpha gating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_alpha_sample: Option<usize>,

    /// Exact alpha reject trigger if the gate rejected the pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_reject_trigger: Option<String>,

    /// Skip reason when alpha failed open (e.g. insufficient sample or missing inputs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_skip_reason: Option<String>,

    /// Whether the final prosperity selector was enabled for this decision.
    #[serde(default)]
    pub prosperity_filter_enabled: bool,

    /// Whether the prosperity selector passed. None means it did not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_pass: Option<bool>,

    /// Whether the prosperity selector had enough required inputs to evaluate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_actionable: Option<bool>,

    /// Exact prosperity reject trigger if the filter rejected the pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_reject_trigger: Option<String>,

    /// Light-veto market-cap floor status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_market_cap_floor_pass: Option<bool>,

    /// Light-veto CPV status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_cpv_pass: Option<bool>,

    /// Balanced branch B1 (conviction + clean sells) status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch1_pass: Option<bool>,

    /// Balanced branch B2 (large cap + buy dominance) status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch2_pass: Option<bool>,

    /// Balanced branch B3 (organic structure) status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch3_pass: Option<bool>,

    /// Whether the strict prosperity overlay was enabled for this decision.
    #[serde(default)]
    pub prosperity_overlay_enabled: bool,

    /// Whether the overlay passed once a Balanced branch matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_pass: Option<bool>,

    /// Global overlay price-extension status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_price_change_pass: Option<bool>,

    /// Global overlay bonding-progress status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_bonding_progress_pass: Option<bool>,

    /// Global overlay FTDI quality status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_fee_topology_diversity_pass: Option<bool>,

    /// Overlay sell/buy status for B2/B3 matched branches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_branch23_sell_buy_pass: Option<bool>,

    /// Additional B2-only price-extension status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_branch2_price_change_pass: Option<bool>,

    /// Branch labels matched by the prosperity selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prosperity_matched_branches: Vec<String>,

    /// Configured prosperity light-veto market-cap floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_min_market_cap_sol: Option<f64>,

    /// Configured prosperity light-veto CPV ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_max_signer_cross_pool_velocity: Option<f64>,

    /// Configured B1 minimum block-0 sniped supply fraction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch1_min_block0_sniped_supply_pct: Option<f64>,

    /// Configured B1 maximum sell/buy ratio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch1_max_sell_buy_ratio: Option<f64>,

    /// Configured B2 elevated market-cap floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch2_min_market_cap_sol: Option<f64>,

    /// Configured B2 minimum early-slot buy dominance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch2_min_early_slot_volume_dominance_buy: Option<f64>,

    /// Configured B3 maximum HHI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch3_max_hhi: Option<f64>,

    /// Configured B3 minimum FTDI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_branch3_min_fee_topology_diversity_index: Option<f64>,

    /// Configured global prosperity overlay max price-change ratio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_max_price_change_ratio: Option<f64>,

    /// Configured global prosperity overlay max bonding-progress percentage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_max_bonding_progress_pct: Option<f64>,

    /// Configured global prosperity overlay minimum FTDI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_min_fee_topology_diversity_index: Option<f64>,

    /// Configured overlay max sell/buy ratio for B2/B3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_branch23_max_sell_buy_ratio: Option<f64>,

    /// Configured stricter overlay max price-change ratio for B2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosperity_overlay_branch2_max_price_change_ratio: Option<f64>,

    // ═══════════════════════════════════════════
    // IWIM Veto Gate Telemetry
    // ═══════════════════════════════════════════
    /// Whether IWIM veto gate was enabled for this decision.
    #[serde(default)]
    pub iwim_enabled: bool,

    /// IWIM feed mode: "PP" or "GRPC".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_mode: Option<String>,

    /// IWIM RPC fetch status: "OK", "TIMEOUT", "ERROR".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_fetch_status: Option<String>,

    /// IWIM data quality: "HIGH", "LOW", "NONE".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_quality: Option<String>,

    /// IWIM analysis confidence (0.0-1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_confidence: Option<f32>,

    /// Number of TX analyzed by IWIM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_n_tx: Option<usize>,

    /// Number of TX requested from RPC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_n_tx_requested: Option<usize>,

    /// Total IWIM latency (fetch + analyze) in ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_latency_ms: Option<u64>,

    /// Which RPC was used: "primary" or "fallback".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_rpc_used: Option<String>,

    /// IWIM veto status: "OK", "VETO", "UNKNOWN".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_status: Option<String>,

    /// IWIM veto reason code (if VETO).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_veto_reason: Option<String>,

    /// Gatekeeper strength classification: "STRONG" or "BORDERLINE".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_gatekeeper_strength: Option<String>,

    /// Raw IWIM rug_threat_score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_rug_threat_score: Option<f32>,

    /// Raw IWIM sybil_score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_sybil_score: Option<f32>,

    /// Raw IWIM organic_score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iwim_organic_score: Option<f32>,

    // ═══════════════════════════════════════════
    // Shadow Ledger Curve Snapshot at IWIM Verdict
    // Captured immediately after IWIM returns (≈ max_wait_time_ms + 300 ms).
    // All values derived from the bonding curve x·y = k
    // (x = virtual_sol_reserves, y = virtual_token_reserves).
    // ═══════════════════════════════════════════
    /// Virtual SOL reserves (Shadow Ledger) at IWIM verdict time, in SOL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iwim_snap_virtual_sol_sol: Option<f64>,

    /// Virtual token reserves (Shadow Ledger) at IWIM verdict time, in display token units (6 decimals).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iwim_snap_virtual_tokens: Option<f64>,

    /// Market cap derived from the bonding curve at IWIM verdict time, in SOL.
    /// Formula: (virtual_sol_reserves × token_total_supply) / virtual_token_reserves  →  SOL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iwim_snap_market_cap_sol: Option<f64>,

    /// Bonding curve progress at IWIM verdict time (0.0–100.0 %).
    /// Formula: (real_sol_reserves / MAX_REAL_SOL_RESERVES) × 100
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iwim_snap_bonding_progress_pct: Option<f64>,

    /// Spot price at IWIM verdict time, in SOL per display token.
    /// Formula: (virtual_sol_reserves / 1e9) / (virtual_token_reserves / 1e6)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iwim_snap_price_sol_per_token: Option<f64>,

    // ═══════════════════════════════════════════
    // Early Fingerprint Metrics (gRPC / Yellowstone)
    // ═══════════════════════════════════════════
    /// Fraction of supply bought in the creation slot (0..1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block0_sniped_supply_pct: Option<f64>,

    /// Fraction of buyers that dumped ≥50 % within the 10 s window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flip_ratio_10s: Option<f64>,

    /// CU price P90 in the first second (micro-lamports / CU).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cu_price_p90_1s: Option<f64>,

    /// CU price P90 over the full 10 s window (micro-lamports / CU).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cu_price_p90_10s: Option<f64>,

    /// Priority fee surge slope (micro-lamports / CU / second).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority_fee_surge_slope: Option<f64>,

    /// Coefficient of variation of buyer SOL pre-balances (sybil signal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_pre_balance_cv: Option<f64>,

    /// Average inner-instruction count over the first 50 tx.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_inner_ix_count_50tx: Option<f64>,
    #[serde(default)]
    pub min_avg_inner_ix_count_50tx: f64,
    #[serde(default = "default_large_f64")]
    pub max_avg_inner_ix_count_50tx: f64,

    /// Average CPI depth proxy over the first 50 tx.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_cpi_depth_50tx: Option<f64>,

    /// SELL count / BUY count in the observation window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell_buy_ratio: Option<f64>,
    #[serde(default = "default_zero_f64")]
    pub min_sell_buy_ratio: f64,
    #[serde(default = "default_large_f64")]
    pub max_sell_buy_ratio: f64,

    /// Dominance of clustered compute-unit consumption values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_unit_cluster_dominance: Option<f64>,
    #[serde(default = "default_zero_f64")]
    pub min_compute_unit_cluster_dominance: f64,
    #[serde(default = "default_one_f64")]
    pub max_compute_unit_cluster_dominance: f64,

    /// Ratio of the most common exact `(cu_limit, cu_price)` buy profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_fee_profile_ratio: Option<f64>,
    #[serde(default = "default_zero_f64")]
    pub min_static_fee_profile_ratio: f64,
    #[serde(default = "default_one_f64")]
    pub max_static_fee_profile_ratio: f64,

    /// Ratio of the dominant 0.001-SOL buy-size bucket.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_size_buy_ratio: Option<f64>,
    #[serde(default)]
    pub min_fixed_size_buy_ratio: f64,

    /// Ratio of the dominant 0.0001-SOL buy-size bucket.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_size_buy_ratio_1e4: Option<f64>,

    /// Fraction of owner wallets that both bought and sold in-window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flipper_presence_ratio: Option<f64>,

    /// Fraction of tx with deterministically detected Jito tips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jito_tip_intensity: Option<f64>,
    #[serde(default = "default_zero_f64")]
    pub min_jito_tip_intensity: f64,
    #[serde(default = "default_one_f64")]
    pub max_jito_tip_intensity: f64,

    /// Share of buy volume concentrated in the first configured slots after creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub early_slot_volume_dominance_buy: Option<f64>,
    #[serde(default)]
    pub max_early_slot_volume_dominance_buy: f64,

    /// Share of buy volume captured by the top-3 buyers during the first 3 seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub early_top3_buy_volume_pct_3s: Option<f64>,
    #[serde(default = "default_one_f64")]
    pub max_early_top3_buy_volume_pct_3s: f64,

    /// Sell/buy ratio for the top-3 early buyers by owner-resolved bought tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whale_reversal_ratio_top3: Option<f64>,

    /// Sell/buy ratio for the top-1 early buyer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whale_reversal_ratio_top1: Option<f64>,

    /// Latency from pool birth to the first meaningful developer sell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_paperhand_latency_ms: Option<u64>,

    /// Whether the developer sold meaningfully within 3 seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_sold_within_3s: Option<bool>,

    /// Whether the developer sold meaningfully within 5 seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_sold_within_5s: Option<bool>,

    /// Whether any fingerprint metric was computed in degraded mode.
    #[serde(default)]
    pub fingerprint_degraded: bool,

    /// Reason(s) for degraded fingerprint computation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint_reason: Option<String>,

    // ═══════════════════════════════════════════
    // Sybil Resistance Metrics (canonical feature bundle)
    // ═══════════════════════════════════════════
    /// Fee Topology Diversity Index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_topology_diversity_index: Option<f64>,
    #[serde(default)]
    pub min_fee_topology_diversity_index: f64,

    /// Dev-Buyer Infrastructure Affinity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_buyer_infrastructure_affinity: Option<f64>,
    #[serde(default)]
    pub max_dev_buyer_infrastructure_affinity: f64,

    /// Spend Fraction Divergence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_fraction_divergence: Option<f64>,
    #[serde(default)]
    pub min_spend_fraction_divergence: f64,

    /// Demand Elasticity Score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demand_elasticity_score: Option<f64>,
    #[serde(default)]
    pub min_demand_elasticity_score: f64,

    /// Signer Cross-Pool Velocity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_cross_pool_velocity: Option<f64>,
    #[serde(default)]
    pub max_signer_cross_pool_velocity: f64,

    /// Funding Source Concentration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_concentration: Option<f64>,
    #[serde(default)]
    pub max_funding_source_concentration: f64,
    /// Additive FSC diagnostics that separate structural vs operational miss causes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_diagnostics: Option<FundingSourceDiagnostics>,

    /// Sybil metric degraded reasons from canonical feature materialization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sybil_metric_degraded_reasons: Vec<String>,

    // ═══════════════════════════════════════════
    // A/B Window Boundary Fields
    // ═══════════════════════════════════════════
    /// Observation window length in ms (from WindowSpec).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_window_ms: Option<u64>,

    /// Window start `t0` in `event_ts_ms` (SSOT).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_t0_event_ts_ms: Option<u64>,

    /// Window end `t_end = t0 + window_ms` in `event_ts_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_t_end_event_ts_ms: Option<u64>,

    /// Whether the observation window ran to completion.
    #[serde(default)]
    pub ab_window_complete: bool,

    /// Why the window was closed (e.g. "END_REACHED", "POOL_REJECTED_EARLY").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_window_close_reason: Option<String>,

    /// Transactions counted strictly within `[t0, t_end]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_tx_count_window: Option<u32>,

    /// Unique signers within the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_unique_signers_window: Option<u32>,

    /// Failed transactions within the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_fail_count_window: Option<u32>,

    /// How `t0` was determined: "NewPoolDetected" or "FirstTxFallback".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_window_origin: Option<String>,

    /// Deterministic record key for downstream dedup: "{pool_id}:{t0}:{t_end}:{verdict}".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ab_record_id: Option<String>,

    // ═══════════════════════════════════════════
    // Window Vectors (v3: DTW/Hill/MI/TDA analysis)
    // Deterministic sequences from [t0, t_end], length-bounded.
    // All vectors aligned to the same tx-event axis.
    // ═══════════════════════════════════════════
    /// Max vector length used for deterministic downsampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors_max_len: Option<u32>,

    /// Transaction timestamp offsets from t0 (ms), sorted, integer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors_ts_offsets_ms: Option<Vec<i64>>,

    /// SOL amounts per transaction in the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors_sol_amounts: Option<Vec<f64>>,

    /// Price (SOL/token) at each tx in the window (looked up from price_history).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors_prices: Option<Vec<f64>>,

    /// Inter-event intervals in ms (diff of ts_offsets); length = N−1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors_interval_ms: Option<Vec<f64>>,

    /// Price change between consecutive tx (diff of prices); length = N−1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors_d_price: Option<Vec<f64>>,

    // ═══════════════════════════════════════════
    // V2.5 Shadow Decision Fields (v16)
    // ═══════════════════════════════════════════
    /// Shadow verdict at early window (2-5s): "BUY" / "REJECT" / "INSUFFICIENT_DATA"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_early_verdict: Option<String>,
    /// Shadow verdict at normal window (5-7s)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_normal_verdict: Option<String>,
    /// Elapsed wall-clock ms at early shadow evaluation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_early_elapsed_ms: Option<u64>,
    /// Shadow verdict at extended window (7-10s): "BUY" / "REJECT" — the terminal shadow decision
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_extended_verdict: Option<String>,
    /// Elapsed wall-clock ms at extended shadow evaluation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_extended_elapsed_ms: Option<u64>,
    /// Elapsed wall-clock ms at normal shadow evaluation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_normal_elapsed_ms: Option<u64>,
    /// Phases passed at early shadow evaluation (0-6)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_early_phases_passed: Option<u8>,
    /// Phases passed at normal shadow evaluation (0-6)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_normal_phases_passed: Option<u8>,
    /// Terminal observation stage (always "Extended" for current live path)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_stage: Option<String>,
    /// V2.5 confidence score proxy at terminal decision
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence: Option<f64>,
    /// V2.5 confidence before hard PDD/TAS zeroing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_pre_veto: Option<f64>,
    /// Base-quality component (`phases_passed / 6`) of the V2.5 confidence model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_base_quality: Option<f64>,
    /// Alpha-quality component of the V2.5 confidence model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_alpha_quality: Option<f64>,
    /// PDD cleanliness modulator of the V2.5 confidence model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_pdd_modulator: Option<f64>,
    /// TAS modulator of the V2.5 confidence model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_tas_modulator: Option<f64>,
    /// Sybil modulator of the V2.5 confidence model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_sybil_modulator: Option<f64>,
    /// Whether the final V2.5 confidence was zeroed by a PDD hard fail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_zeroed_by_pdd_hard_fail: Option<bool>,
    /// Whether the final V2.5 confidence was zeroed by a TAS hard reject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_zeroed_by_tas_hard_reject: Option<bool>,
    /// Whether TAS inputs were fully available for this assessment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_available: Option<bool>,
    /// Why TAS could not be materialized when disabled/partial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_unavailable_reason: Option<String>,
    /// Whether sequence-dependent PDD checks were available from the underlying source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_sequence_signals_available: Option<bool>,
    /// P1: Specific reason when PDD sequence signals are unavailable.
    /// Mirrors the taxonomy from `pdd_sequence_signals_availability()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_sequence_signals_unavailable_reason: Option<String>,
    /// Whether a valid price anchor was available for entry-drift evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_price_anchor_available: Option<bool>,
    /// Whether V2.5 confidence is fully interpretable for this record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_available: Option<bool>,
    /// Why V2.5 confidence is unavailable when partial inputs prevented computation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_confidence_unavailable_reason: Option<String>,

    /// Reason for TAS-based shadow reject (populated when RejectLowTrajectory)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_tas_reject_reason: Option<String>,

    // ═══════════════════════════════════════════
    // V2.5 Trajectory Aware Scoring (TAS) fields
    // ═══════════════════════════════════════════
    /// Overall TAS trajectory score (weighted 5-dimension)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_overall_score: Option<f64>,
    /// Momentum dimension score: T2/T0 tx count ratio
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_momentum_score: Option<f64>,
    /// HHI trajectory score: T2/T0 HHI ratio
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_hhi_score: Option<f64>,
    /// Volume consistency score: CV across segments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_volume_score: Option<f64>,
    /// Interval trajectory score: T2/T0 interval ratio
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_interval_score: Option<f64>,
    /// Buy ratio stability score
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tas_buy_ratio_score: Option<f64>,

    // ═══════════════════════════════════════════
    // V2.5 Pump & Dump Detector (PDD) fields
    // ═══════════════════════════════════════════
    /// PDD hard fail reason tag (ENTRY_DRIFT/SPIKE/RAMPING/WHALE/RESERVE/FLASH_CRASH)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_hard_fail: Option<String>,
    /// Entry drift percentage from initial price
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_pct: Option<f64>,
    /// Entry drift anchor source provenance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_anchor_source: Option<String>,
    /// Entry drift anchor quality: "strong" / "weak"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_anchor_quality: Option<String>,
    /// Volume spike pattern detected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_spike_detected: Option<bool>,
    /// Consecutive same-size buy ramping detected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_ramping_detected: Option<bool>,
    /// Top-3 whale volume concentration percentage
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_whale_top3_pct: Option<f64>,
    /// Flash crash sell cluster risk detected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_flash_crash_risk: Option<bool>,
    /// Overall PDD cleanliness score (1.0 = clean, 0.0 = hard fail)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_score: Option<f64>,

    // ═══════════════════════════════════════════
    // V2.5 Adaptive Prosperity (APS) fields
    // ═══════════════════════════════════════════
    /// Detected market regime: "Normal" / "HighVolatility"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aps_regime: Option<String>,
    /// Shadow-suggested entry drift max for this regime
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aps_shadow_entry_drift_max: Option<f64>,
    /// Shadow-suggested confidence minimum for this regime
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aps_shadow_confidence_min: Option<f64>,
    /// Shadow-suggested prosperity market cap floor
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aps_shadow_prosperity_mcap: Option<f64>,
    /// Shadow-suggested branch1 sniped supply threshold
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aps_shadow_branch1_sniped: Option<f64>,
    /// Shadow-suggested branch3 HHI max threshold
    /// Contrafactual: would prosperity filter have passed with APS shadow thresholds?
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aps_shadow_prosperity_would_pass: Option<bool>,
    pub aps_shadow_branch3_hhi: Option<f64>,
    /// Serialized APS shadow thresholds as JSON string for ablation tooling
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aps_shadow_thresholds: Option<String>,

    // ═══════════════════════════════════════════
    // V2.5 top-level telemetry (Plan Section 6.3 — cross-module fields)
    // ═══════════════════════════════════════════
    /// Entry drift percentage (from PDD, exposed as top-level for ablation)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_drift_pct: Option<f64>,
    /// Entry drift anchor source provenance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_drift_anchor_source: Option<String>,
    /// Entry drift anchor quality: "strong" / "weak"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_drift_anchor_quality: Option<String>,
    /// Market regime detected by APS (top-level alias for aps_regime)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_regime: Option<String>,
    /// PDD soft flags fired (comma-separated)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_soft_flags: Option<String>,
    /// PDD-based shadow reject reason chain
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_pdd_reject_reason: Option<String>,
}

/// Decision made by Oracle Brain (initial or follow-up)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum DecisionType {
    /// Initial buy decision (T <= 2s)
    Buy,
    /// Skip this candidate
    Skip,
    /// Hold current position
    Hold,
    /// Sell/exit position
    Sell,
    /// Scale out (partial sell)
    ScaleOut,
}

/// Reason for score correction or adjustment
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CorrectionReason {
    /// MCI (Market Coherence Index) dropped below threshold
    MciDrop {
        old_value: f32,
        new_value: f32,
        threshold: f32,
        impact: i16,
    },
    /// QEDD λ (decay rate) spiked indicating instability
    QeddLambdaSpike {
        old_lambda: f32,
        new_lambda: f32,
        threshold: f32,
        impact: i16,
    },
    /// QEDD survival probability dropped
    QeddSurvivalDrop {
        old_survival: f32,
        new_survival: f32,
        horizon_s: u32,
        impact: i16,
    },
    /// GeneMapper detected known scam pattern
    GeneMapperHit {
        match_score: f32,
        pattern_id: String,
        impact: i16,
    },
    /// Guardian watchdog triggered abort
    GuardianAbort {
        reason: String,
        signal_name: String,
        impact: i16,
    },
    /// Chaos Engine simulation showed high loss probability
    ChaosHighRisk {
        loss_prob: f32,
        threshold: f32,
        impact: i16,
    },
    /// QASS score dropped significantly
    QassScoreDrop {
        old_score: f32,
        new_score: f32,
        drop_pct: f32,
        impact: i16,
    },
    /// Resonance detector found bot activity
    ResonanceDetected {
        ratio: f32,
        threshold: f32,
        impact: i16,
    },
    /// Shadow Ledger detected migration imminent
    ShadowMigrationRisk { bonding_progress: f32, impact: i16 },
    /// Custom/other reason
    Other { description: String, impact: i16 },
}

impl CorrectionReason {
    /// Get the score impact of this correction
    pub fn impact(&self) -> i16 {
        match self {
            Self::MciDrop { impact, .. } => *impact,
            Self::QeddLambdaSpike { impact, .. } => *impact,
            Self::QeddSurvivalDrop { impact, .. } => *impact,
            Self::GeneMapperHit { impact, .. } => *impact,
            Self::GuardianAbort { impact, .. } => *impact,
            Self::ChaosHighRisk { impact, .. } => *impact,
            Self::QassScoreDrop { impact, .. } => *impact,
            Self::ResonanceDetected { impact, .. } => *impact,
            Self::ShadowMigrationRisk { impact, .. } => *impact,
            Self::Other { impact, .. } => *impact,
        }
    }
}

/// Components that contribute to the initial score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialComponents {
    /// Base score from Shadow Ledger heuristics
    pub base_shadow: u8,
    /// QASS (Quantum Amplitude Superposition Scoring) result
    pub qass_score: f32,
    /// QEDD survival probability at 30s horizon
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qedd_survival_30s: Option<f32>,
    /// MCI (Market Coherence Index)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mci: Option<f32>,
    /// Chaos Engine loss probability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chaos_loss_prob: Option<f32>,
    /// GeneMapper match score (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gene_match_score: Option<f32>,
    /// Confidence score (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Additional component scores
    #[serde(flatten)]
    pub extras: HashMap<String, f32>,
}

/// Follow-up score at a specific time interval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowupScore {
    /// Time since initial decision (milliseconds)
    pub t_ms: u64,
    /// Score at this time point (0-100)
    pub score: u8,
    /// Primary reason for score change
    pub reason: String,
    /// All corrections applied at this interval
    pub corrections: Vec<CorrectionReason>,
    /// Decision made at this point
    pub decision: DecisionType,
    /// Updated component values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<InitialComponents>,
    /// Confidence score at this time point (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

/// Veto type that overrides scoring
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VetoType {
    /// GeneMapper detected scam pattern
    Gene,
    /// Guardian watchdog abort
    Guardian,
    /// MCI below critical threshold
    Mci,
    /// QEDD decay rate too high
    Qedd,
    /// Shadow Ledger migration risk
    Shadow,
}

/// Complete decision log for a candidate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleDecisionLog {
    /// Candidate identifier (pool AMM ID)
    pub candidate_id: String,
    /// Unix timestamp (seconds)
    pub timestamp: u64,
    /// Initial score (0-100)
    #[serde(rename = "initialScore")]
    pub initial_score: u8,
    /// Initial decision made
    pub initial_decision: DecisionType,
    /// Component scores that contributed to initial score
    pub initial_components: InitialComponents,
    /// Initial confidence score (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_confidence: Option<f32>,
    /// Follow-up scores at 1s, 5s, 30s, 60s intervals
    #[serde(rename = "followupScores")]
    pub followup_scores: Vec<FollowupScore>,
    /// Veto type if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub veto: Option<VetoType>,
    /// Final decision after all follow-ups
    pub final_decision: DecisionType,
    /// Final confidence score (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_confidence: Option<f32>,
    /// Total number of corrections applied
    pub total_corrections: usize,
    /// Processing completed timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
}

impl OracleDecisionLog {
    /// Create a new decision log with initial score
    pub fn new(
        candidate_id: String,
        initial_score: u8,
        initial_decision: DecisionType,
        initial_components: InitialComponents,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time should be after Unix epoch")
            .as_secs();

        // Extract initial confidence from components
        let initial_confidence = initial_components.confidence;

        Self {
            candidate_id,
            timestamp,
            initial_score,
            initial_decision: initial_decision.clone(),
            initial_components,
            initial_confidence,
            followup_scores: Vec::new(),
            veto: None,
            final_decision: initial_decision,
            final_confidence: initial_confidence,
            total_corrections: 0,
            completed_at: None,
        }
    }

    /// Add a follow-up score
    pub fn add_followup(&mut self, followup: FollowupScore) {
        self.total_corrections += followup.corrections.len();
        self.final_decision = followup.decision.clone();
        // Update final confidence if present in followup
        if let Some(conf) = followup.confidence {
            self.final_confidence = Some(conf);
        }
        self.followup_scores.push(followup);
    }

    /// Set veto and final decision
    pub fn set_veto(&mut self, veto: VetoType, final_decision: DecisionType) {
        self.veto = Some(veto);
        self.final_decision = final_decision;
        // On veto, confidence should drop significantly
        self.final_confidence = Some(0.0);
    }

    /// Mark log as completed
    pub fn complete(&mut self) {
        self.completed_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System time should be after Unix epoch")
                .as_secs(),
        );
    }
}

/// Configuration for decision logger
#[derive(Debug, Clone)]
pub struct DecisionLoggerConfig {
    /// Base directory for decision logs
    pub log_dir: PathBuf,
    /// Base directory for gatekeeper-specific decision logs.
    /// Gatekeeper V2 verdicts (decisions + buys) are written here. By default
    /// this stays aligned with `log_dir` so runtime config remains the SSOT
    /// for artifact rooting and library defaults cannot inject rollout-specific
    /// paths.
    pub gatekeeper_log_dir: PathBuf,
    /// Stable rollout-profile tag stamped onto routed gatekeeper records.
    pub gatekeeper_rollout_profile: String,
    /// Deterministic gatekeeper config hash stamped onto routed gatekeeper records.
    pub gatekeeper_config_hash: String,
    /// Channel buffer size
    pub channel_buffer_size: usize,
    /// Enable logging
    pub enabled: bool,
}

impl Default for DecisionLoggerConfig {
    fn default() -> Self {
        Self {
            log_dir: PathBuf::from(DEFAULT_DECISION_LOG_DIR),
            gatekeeper_log_dir: PathBuf::from(DEFAULT_DECISION_LOG_DIR),
            gatekeeper_rollout_profile: "unknown_rollout".to_string(),
            gatekeeper_config_hash: "unknown_config_hash".to_string(),
            channel_buffer_size: 1000,
            enabled: true,
        }
    }
}

/// Async decision logger for Oracle Brain
pub struct DecisionLogger {
    tx: mpsc::Sender<LogCommand>,
}

enum LogCommand {
    Write(OracleDecisionLog),
    WriteCyclic(CyclicEngineLog),
    WriteGatekeeperBuy(GatekeeperBuyLog),
    Shutdown,
}

impl DecisionLogger {
    /// Create a new decision logger
    pub fn new(config: DecisionLoggerConfig) -> Self {
        Self::new_with_health(config, None)
    }

    /// Create a new decision logger with optional RuntimeHealth for watchdog heartbeats.
    ///
    /// When `health` is `Some`, the writer task calls `mark_decisions_write()` after
    /// every gatekeeper decision JSONL flush and `mark_buys_write()` after every
    /// buy-eligible JSONL flush.
    pub fn new_with_health(
        config: DecisionLoggerConfig,
        health: Option<Arc<RuntimeHealth>>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<LogCommand>(config.channel_buffer_size);

        if !config.enabled {
            info!("DecisionLogger: disabled by configuration");
            return Self { tx };
        }

        // Spawn async writer task
        tokio::spawn(async move {
            let mut logging_disabled_due_to_enospc = false;
            if let Err(e) = create_dir_all(&config.log_dir).await {
                if is_no_space_error(std::iter::once(&e as &(dyn std::error::Error + 'static))) {
                    logging_disabled_due_to_enospc = true;
                    error!(
                        "DecisionLogger: disabling file writes after ENOSPC while creating {:?}",
                        config.log_dir
                    );
                } else {
                    error!("Failed to create decision log directory: {}", e);
                    return;
                }
            }
            // Also create the dedicated gatekeeper log directory.
            if let Err(e) = create_dir_all(&config.gatekeeper_log_dir).await {
                if is_no_space_error(std::iter::once(&e as &(dyn std::error::Error + 'static))) {
                    logging_disabled_due_to_enospc = true;
                    error!(
                        "DecisionLogger: disabling file writes after ENOSPC while creating GK dir {:?}",
                        config.gatekeeper_log_dir
                    );
                } else {
                    error!("Failed to create gatekeeper log directory: {}", e);
                    return;
                }
            }

            info!(
                "DecisionLogger: started writing decisions → {:?}, gatekeeper → {:?}",
                config.log_dir, config.gatekeeper_log_dir
            );

            // ── A/B dedup guard: TTL+size-bounded cache ──────────────────────
            let mut dedup_map: HashMap<String, ()> = HashMap::new();
            let mut dedup_queue: VecDeque<(Instant, String)> = VecDeque::new();

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    LogCommand::Write(log) => {
                        if logging_disabled_due_to_enospc {
                            continue;
                        }
                        if let Err(e) = write_log(&config.log_dir, &log).await {
                            if is_no_space_error(e.chain()) {
                                logging_disabled_due_to_enospc = true;
                                error!(
                                    "DecisionLogger: disabling file writes after ENOSPC on decision log for {}",
                                    log.candidate_id
                                );
                                continue;
                            }
                            error!(
                                "Failed to write decision log for {}: {}",
                                log.candidate_id, e
                            );
                        }
                    }
                    LogCommand::WriteCyclic(log) => {
                        if logging_disabled_due_to_enospc {
                            continue;
                        }
                        if let Err(e) = write_cyclic_log(&config.log_dir, &log).await {
                            if is_no_space_error(e.chain()) {
                                logging_disabled_due_to_enospc = true;
                                error!(
                                    "DecisionLogger: disabling file writes after ENOSPC on cyclic log for {}",
                                    log.pool_id
                                );
                                continue;
                            }
                            error!(
                                "Failed to write cyclic engine log for {}: {}",
                                log.pool_id, e
                            );
                        }
                    }
                    LogCommand::WriteGatekeeperBuy(log) => {
                        if logging_disabled_due_to_enospc {
                            continue;
                        }
                        for mut plane_log in expand_gatekeeper_plane_logs(log) {
                            hydrate_gatekeeper_routing_fields(&mut plane_log, &config);

                            // ── A/B dedup: skip duplicate ab_record_id writes per plane ──
                            if let Some(ref record_id) = plane_log.ab_record_id {
                                let plane =
                                    plane_log.decision_plane.as_deref().unwrap_or("unscoped");
                                let dedup_key = format!("{record_id}:{plane}");
                                let now = Instant::now();

                                while let Some((ts, _)) = dedup_queue.front() {
                                    if now.duration_since(*ts) >= DEDUP_TTL {
                                        if let Some((_, key)) = dedup_queue.pop_front() {
                                            dedup_map.remove(&key);
                                        }
                                    } else {
                                        break;
                                    }
                                }

                                if dedup_map.len() >= DEDUP_MAX_CAPACITY {
                                    if let Some((_, key)) = dedup_queue.pop_front() {
                                        dedup_map.remove(&key);
                                    }
                                }

                                if dedup_map.contains_key(&dedup_key) {
                                    warn!(
                                        "DEDUP_GUARD: skipping duplicate JSONL write for ab_record_id={} plane={} pool={}",
                                        record_id, plane, plane_log.pool_id
                                    );
                                    continue;
                                }
                                dedup_map.insert(dedup_key.clone(), ());
                                dedup_queue.push_back((now, dedup_key));
                            }

                            let buy_eligible = plane_log.decision_verdict_buy == Some(true);
                            if let Err(e) =
                                write_gatekeeper_buy_log(&config.gatekeeper_log_dir, &plane_log)
                                    .await
                            {
                                if is_no_space_error(e.chain()) {
                                    logging_disabled_due_to_enospc = true;
                                    error!(
                                        "DecisionLogger: disabling file writes after ENOSPC on gatekeeper log for {} plane={}",
                                        plane_log.pool_id,
                                        plane_log
                                            .decision_plane
                                            .as_deref()
                                            .unwrap_or("unknown")
                                    );
                                    mark_gatekeeper_log_progress(health.as_ref(), buy_eligible);
                                    continue;
                                }
                                error!(
                                    "Failed to write gatekeeper buy log for {} plane={}: {}",
                                    plane_log.pool_id,
                                    plane_log.decision_plane.as_deref().unwrap_or("unknown"),
                                    e
                                );
                            } else {
                                mark_gatekeeper_log_progress(health.as_ref(), buy_eligible);
                            }
                        }
                    }
                    LogCommand::Shutdown => {
                        info!("DecisionLogger: shutting down");
                        break;
                    }
                }
            }
        });

        Self { tx }
    }

    /// Log a decision (fire-and-forget)
    pub async fn log(&self, decision: OracleDecisionLog) {
        if let Err(e) = self.tx.send(LogCommand::Write(decision)).await {
            warn!("Failed to send log command: {}", e);
        }
    }

    /// Log a cyclic engine decision (for S1-S13 heartbeat system)
    ///
    /// This logs decisions from the new cyclic engine including:
    /// - GUNSHOT events (early buy triggers in S1-S12)
    /// - VERDICT events (final decision after S13)
    /// - KILLED events (rejection with reason)
    ///
    /// # Arguments
    ///
    /// * `pool_id` - Pool AMM ID as Pubkey
    /// * `event_type` - Type of event ("GUNSHOT", "VERDICT", "KILLED")
    /// * `score` - Final score (0.0-100.0)
    /// * `cycle` - Cycle number reached (1-12)
    /// * `metrics` - Optional metrics snapshot (SOBP, MPCF, etc.)
    /// * `is_dry_run` - Whether this is a dry-run simulation
    /// * `kill_reason` - Optional reason for kill events
    pub async fn log_cyclic_engine(
        &self,
        pool_id: &solana_sdk::pubkey::Pubkey,
        event_type: &str,
        score: f64,
        cycle: u8,
        metrics: Option<CycleMetricsSnapshot>,
        is_dry_run: bool,
        kill_reason: Option<String>,
    ) {
        let log_entry = CyclicEngineLog {
            log_schema_version: CYCLIC_LOG_SCHEMA_VERSION,
            pool_id: pool_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            event_type: event_type.to_string(),
            score,
            cycle_reached: cycle,
            is_dry_run,
            metrics_snapshot: metrics,
            kill_reason,
            reason_code: None,
            reason_code_version: 1,
        };

        // Write directly using the async writer
        // We'll reuse the Write command but with a new variant
        if let Err(e) = self.tx.send(LogCommand::WriteCyclic(log_entry)).await {
            warn!("Failed to send cyclic engine log command: {}", e);
        }
    }

    /// Log a Gatekeeper V2 Buy decision
    pub async fn log_gatekeeper_buy_decision(&self, log: GatekeeperBuyLog) {
        if let Err(e) = self.tx.send(LogCommand::WriteGatekeeperBuy(log)).await {
            warn!("Failed to send gatekeeper buy log command: {}", e);
        }
    }

    /// Shutdown the logger gracefully
    pub async fn shutdown(&self) {
        let _ = self.tx.send(LogCommand::Shutdown).await;
    }
}

fn hydrate_gatekeeper_routing_fields(log: &mut GatekeeperBuyLog, config: &DecisionLoggerConfig) {
    if log.rollout_profile.is_none() {
        log.rollout_profile = Some(config.gatekeeper_rollout_profile.clone());
    }
    if log.config_hash.is_none() {
        log.config_hash = Some(config.gatekeeper_config_hash.clone());
    }
}

fn gatekeeper_buy_alias_from_verdict(verdict_type: Option<&str>) -> Option<bool> {
    verdict_type.map(|verdict| matches!(verdict, "BUY" | "EARLY_BUY"))
}

fn expand_gatekeeper_plane_logs(log: GatekeeperBuyLog) -> Vec<GatekeeperBuyLog> {
    if log.decision_plane.is_some() {
        return vec![log];
    }

    let legacy_reason = log
        .legacy_live_reason_chain
        .clone()
        .or_else(|| log.decision_reason.clone());
    let legacy_verdict_buy = log.legacy_live_verdict_buy.or(log.decision_verdict_buy);
    let legacy_verdict_type = log
        .legacy_live_verdict_type
        .clone()
        .or_else(|| log.verdict_type.clone());
    let has_legacy_plane =
        legacy_reason.is_some() || legacy_verdict_buy.is_some() || legacy_verdict_type.is_some();

    let has_shadow_plane = log.v25_shadow_verdict_type.is_some()
        || log.v25_shadow_reason_chain.is_some()
        || log.v25_shadow_confidence.is_some()
        || log.v25_shadow_observation_stage.is_some();

    let mut expanded = Vec::with_capacity(if has_shadow_plane && has_legacy_plane {
        2
    } else {
        1
    });

    if has_legacy_plane {
        let mut legacy = log.clone();
        legacy.gatekeeper_version = Some(LEGACY_GATEKEEPER_VERSION.to_string());
        legacy.decision_plane = Some(DECISION_PLANE_LEGACY_LIVE.to_string());
        legacy.legacy_live_reason_chain = legacy_reason.clone();
        legacy.legacy_live_verdict_buy = legacy_verdict_buy;
        legacy.legacy_live_verdict_type = legacy_verdict_type.clone();
        legacy.decision_reason = legacy_reason;
        legacy.decision_verdict_buy = legacy_verdict_buy;
        // P4: per-plane reason_code — recompute from legacy verdict type
        // when a 1:1 mapping exists. For generic/aggregate tags, keep the
        // unified reason_code from the primary assessment (100% completeness).
        let legacy_rc = legacy_verdict_type.as_ref().and_then(|vt| {
            crate::oracle::reason_code::GatekeeperReasonCode::derive_from_verdict_type_str(vt)
        });
        legacy.verdict_type = legacy_verdict_type;
        legacy.reason_code = legacy_rc.or_else(|| log.reason_code.clone());
        expanded.push(legacy);
    }

    if has_shadow_plane {
        let mut shadow = log.clone();
        let shadow_verdict_type = shadow.v25_shadow_verdict_type.clone();
        shadow.gatekeeper_version = Some(GATEKEEPER_VERSION.to_string());
        shadow.decision_plane = Some(DECISION_PLANE_V25_SHADOW.to_string());
        shadow.decision_reason = shadow.v25_shadow_reason_chain.clone();
        shadow.decision_verdict_buy =
            gatekeeper_buy_alias_from_verdict(shadow_verdict_type.as_deref());
        // P4: per-plane reason_code — recompute from shadow verdict type
        // when a 1:1 mapping exists. For generic/aggregate tags, keep the
        // unified reason_code from the primary assessment (100% completeness).
        let shadow_rc = shadow_verdict_type.as_ref().and_then(|vt| {
            crate::oracle::reason_code::GatekeeperReasonCode::derive_from_verdict_type_str(vt)
        });
        shadow.verdict_type = shadow_verdict_type;
        shadow.reason_code = shadow_rc.or_else(|| log.reason_code.clone());
        expanded.push(shadow);
    }

    if expanded.is_empty() {
        let mut fallback = log;
        fallback.gatekeeper_version = Some(LEGACY_GATEKEEPER_VERSION.to_string());
        fallback.decision_plane = Some(DECISION_PLANE_LEGACY_LIVE.to_string());
        expanded.push(fallback);
    }

    expanded
}

fn gatekeeper_route_dir(base_dir: &Path, log: &GatekeeperBuyLog) -> PathBuf {
    let rollout_profile = log.rollout_profile.as_deref().unwrap_or("unknown_rollout");
    let gatekeeper_version = log
        .gatekeeper_version
        .as_deref()
        .unwrap_or("unknown_gatekeeper_version");
    let decision_plane = log
        .decision_plane
        .as_deref()
        .unwrap_or("unknown_decision_plane");
    let config_hash = log.config_hash.as_deref().unwrap_or("unknown_config_hash");

    base_dir
        .join(rollout_profile)
        .join(gatekeeper_version)
        .join(decision_plane)
        .join(config_hash)
}

/// Write a decision log to JSONL file
async fn write_log(base_dir: &Path, log: &OracleDecisionLog) -> Result<()> {
    // Create candidate-specific directory
    let candidate_dir = base_dir.join(&log.candidate_id);
    create_dir_all(&candidate_dir)
        .await
        .context("Failed to create candidate directory")?;

    // Append to decision.jsonl
    let log_path = candidate_dir.join("decision.jsonl");

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await
        .context("Failed to open decision log file")?;

    // Serialize to JSON
    let json = serde_json::to_string(log).context("Failed to serialize decision log")?;

    // Write JSONL line (JSON + newline)
    file.write_all(json.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;

    debug!(
        "Decision log written for {} to {:?}",
        log.candidate_id, log_path
    );

    Ok(())
}

/// Write a cyclic engine decision log to JSONL file
///
/// Logs are written to: `{base_dir}/cyclic_engine.jsonl`
/// Each line is a complete JSON object representing one decision event.
async fn write_cyclic_log(base_dir: &Path, log: &CyclicEngineLog) -> Result<()> {
    // Create log directory if it doesn't exist
    create_dir_all(base_dir)
        .await
        .context("Failed to create cyclic engine log directory")?;

    // Append to cyclic_engine.jsonl
    let log_path = base_dir.join("cyclic_engine.jsonl");

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await
        .context("Failed to open cyclic engine log file")?;

    // Serialize to JSON
    let json = serde_json::to_string(log).context("Failed to serialize cyclic engine log")?;

    // Write JSONL line (JSON + newline)
    file.write_all(json.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;

    debug!(
        "Cyclic engine log written for {} ({}) to {:?}",
        log.pool_id, log.event_type, log_path
    );

    Ok(())
}

/// Write a gatekeeper V2 decision log with routing.
///
/// Every plane-specific decision is appended to:
/// `{base_dir}/{rollout_profile}/{gatekeeper_version}/{decision_plane}/{config_hash}/`.
/// Inside that routed directory, every decision is ALWAYS appended to
/// `gatekeeper_v2_decisions.jsonl`. Decisions where `decision_verdict_buy ==
/// Some(true)` are ADDITIONALLY appended to `gatekeeper_v2_buys.jsonl`.
async fn write_gatekeeper_buy_log(base_dir: &Path, log: &GatekeeperBuyLog) -> Result<()> {
    let routed_dir = gatekeeper_route_dir(base_dir, log);

    // Create log directory if it doesn't exist
    create_dir_all(&routed_dir)
        .await
        .context("Failed to create gatekeeper buy log directory")?;

    // INVARIANT: verdict_type must always be present in every JSONL record.
    // Runtime terminal assessments now carry decision metadata for BUY/REJECT/TIMEOUT,
    // so a missing verdict_type should only happen on malformed legacy rows or
    // partially populated test fixtures.
    //
    // If verdict_type is somehow None for a REJECT record (should not happen in
    // practice because decision.is_some() → verdict_type.is_some()), we emit
    // "REJECT_UNKNOWN" rather than a bare "REJECT" (which is not a valid tag in
    // GatekeeperVerdictType) to make the data-loss explicit.
    let patched;
    let log = if log.verdict_type.is_none() {
        let fallback = match log.decision_verdict_buy {
            Some(true) => "BUY".to_string(),
            Some(false) => {
                warn!(
                    "VERDICT_TYPE_MISSING: verdict_type absent for REJECT record pool={} — emitting REJECT_UNKNOWN",
                    log.pool_id
                );
                "REJECT_UNKNOWN".to_string()
            }
            None => "TIMEOUT".to_string(),
        };
        patched = GatekeeperBuyLog {
            verdict_type: Some(fallback),
            ..log.clone()
        };
        &patched
    } else {
        log
    };

    // Serialize to JSON once (reused for both files)
    let json = serde_json::to_string(log).context("Failed to serialize gatekeeper buy log")?;

    // 1. ALWAYS write to decisions file (all verdicts)
    let decisions_path = routed_dir.join(GATEKEEPER_DECISIONS_JSONL);
    {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&decisions_path)
            .await
            .context("Failed to open gatekeeper decisions log file")?;
        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
    }

    // 2. ADDITIONALLY write to passed/buys file only if verdict is BUY
    let is_passed = log.decision_verdict_buy == Some(true);
    if is_passed {
        let passed_path = routed_dir.join(GATEKEEPER_PASSED_JSONL);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&passed_path)
            .await
            .context("Failed to open gatekeeper passed log file")?;
        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
    }

    debug!(
        "Gatekeeper decision log written for {} (passed={}) to {:?}",
        log.pool_id, is_passed, decisions_path
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use tokio::fs;

    fn create_test_components() -> InitialComponents {
        InitialComponents {
            base_shadow: 60,
            qass_score: 78.5,
            qedd_survival_30s: Some(0.71),
            mci: Some(0.74),
            chaos_loss_prob: Some(0.12),
            gene_match_score: Some(0.03),
            confidence: Some(0.85),
            extras: HashMap::new(),
        }
    }

    #[test]
    fn test_decision_log_creation() {
        let components = create_test_components();
        let log = OracleDecisionLog::new(
            "test_pool_123".to_string(),
            62,
            DecisionType::Buy,
            components,
        );

        assert_eq!(log.candidate_id, "test_pool_123");
        assert_eq!(log.initial_score, 62);
        assert_eq!(log.initial_decision, DecisionType::Buy);
        assert!(log.followup_scores.is_empty());
        assert!(log.veto.is_none());
        assert_eq!(log.total_corrections, 0);
    }

    #[test]
    fn test_followup_score_addition() {
        let components = create_test_components();
        let mut log = OracleDecisionLog::new(
            "test_pool_123".to_string(),
            62,
            DecisionType::Buy,
            components,
        );

        let correction = CorrectionReason::MciDrop {
            old_value: 0.74,
            new_value: 0.45,
            threshold: 0.50,
            impact: -15,
        };

        let followup = FollowupScore {
            t_ms: 5000,
            score: 47,
            reason: "MCI drop below threshold".to_string(),
            corrections: vec![correction],
            decision: DecisionType::Sell,
            components: None,
            confidence: Some(0.70),
        };

        log.add_followup(followup);

        assert_eq!(log.followup_scores.len(), 1);
        assert_eq!(log.total_corrections, 1);
        assert_eq!(log.final_decision, DecisionType::Sell);
    }

    #[test]
    fn test_veto_setting() {
        let components = create_test_components();
        let mut log = OracleDecisionLog::new(
            "test_pool_123".to_string(),
            62,
            DecisionType::Buy,
            components,
        );

        log.set_veto(VetoType::Gene, DecisionType::Skip);

        assert_eq!(log.veto, Some(VetoType::Gene));
        assert_eq!(log.final_decision, DecisionType::Skip);
    }

    #[test]
    fn test_correction_reason_impact() {
        let correction = CorrectionReason::QeddLambdaSpike {
            old_lambda: 0.5,
            new_lambda: 3.2,
            threshold: 2.0,
            impact: -25,
        };

        assert_eq!(correction.impact(), -25);
    }

    #[tokio::test]
    async fn test_decision_logger_write() {
        let temp_dir = TempDir::new().unwrap();
        let config = DecisionLoggerConfig {
            log_dir: temp_dir.path().to_path_buf(),
            gatekeeper_log_dir: temp_dir.path().to_path_buf(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 10,
            enabled: true,
        };

        let logger = DecisionLogger::new(config);

        let components = create_test_components();
        let log = OracleDecisionLog::new(
            "test_pool_456".to_string(),
            75,
            DecisionType::Buy,
            components,
        );

        logger.log(log).await;

        // Give time for async write
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify file exists
        let log_path = temp_dir.path().join("test_pool_456").join("decision.jsonl");
        assert!(log_path.exists());

        // Read and verify content
        let content = fs::read_to_string(log_path).await.unwrap();
        assert!(content.contains("test_pool_456"));
        assert!(content.contains("\"initialScore\":75"));
    }

    #[test]
    fn test_default_config_keeps_gatekeeper_root_aligned_with_log_root() {
        let config = DecisionLoggerConfig::default();

        assert_eq!(config.gatekeeper_log_dir, config.log_dir);
        assert_eq!(config.gatekeeper_rollout_profile, "unknown_rollout");
    }

    #[tokio::test]
    async fn test_multiple_followups() {
        let components = create_test_components();
        let mut log = OracleDecisionLog::new(
            "test_pool_789".to_string(),
            62,
            DecisionType::Buy,
            components,
        );

        // Add follow-ups at different intervals
        let intervals = vec![1000, 5000, 30000, 60000];
        let scores = vec![58, 45, 40, 35];

        for (t_ms, score) in intervals.iter().zip(scores.iter()) {
            let followup = FollowupScore {
                t_ms: *t_ms,
                score: *score,
                reason: format!("Update at {}ms", t_ms),
                corrections: vec![],
                decision: DecisionType::Hold,
                components: None,
                confidence: Some(0.75),
            };
            log.add_followup(followup);
        }

        assert_eq!(log.followup_scores.len(), 4);
        assert_eq!(log.followup_scores[0].t_ms, 1000);
        assert_eq!(log.followup_scores[3].t_ms, 60000);
        assert_eq!(log.followup_scores[3].score, 35);
    }

    #[test]
    fn test_serialization_to_json() {
        let components = create_test_components();
        let mut log = OracleDecisionLog::new(
            "test_pool_json".to_string(),
            62,
            DecisionType::Buy,
            components,
        );

        let correction = CorrectionReason::ChaosHighRisk {
            loss_prob: 0.85,
            threshold: 0.60,
            impact: -20,
        };

        let followup = FollowupScore {
            t_ms: 5000,
            score: 42,
            reason: "High chaos risk".to_string(),
            corrections: vec![correction],
            decision: DecisionType::Sell,
            components: None,
            confidence: Some(0.65),
        };

        log.add_followup(followup);
        log.complete();

        let json = serde_json::to_string_pretty(&log).unwrap();
        assert!(json.contains("initialScore"));
        assert!(json.contains("followupScores"));
        assert!(json.contains("chaos_high_risk"));
        assert!(json.contains("completed_at"));
    }

    #[tokio::test]
    async fn test_gatekeeper_buy_log_file_write() {
        use std::path::PathBuf;
        use tempfile::TempDir;

        // Create a temporary directory for the test
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();

        // Create a mock buy log
        let buy_log = crate::oracle::GatekeeperBuyLog {
            log_schema_version: crate::oracle::GATEKEEPER_BUY_LOG_SCHEMA_VERSION,
            timestamp: chrono::Utc::now().to_rfc3339(),
            pool_id: "test_pool_123".to_string(),
            join_key: None,
            base_mint: None,
            first_seen_ts_ms: None,
            first_seen_clock_source: None,
            observation_start_ts_ms: None,
            observation_end_ts_ms: None,
            observation_window_ms: None,
            end_10s_ts_ms: None,
            core_pass: None,
            gatekeeper_version: None,
            rollout_profile: Some("test-rollout".to_string()),
            decision_plane: None,
            config_hash: Some("test-config-hash".to_string()),
            dev_pubkey: None,
            shadow_ready: None,
            shadow_missing_fields: None,
            shadow_metadata_source: None,
            shadow_trigger_present: None,
            // Populated downstream by launcher-side shadow handoff enrichment before persistence.
            shadow_entry_mode: None,
            shadow_trigger_eligible: None,
            shadow_execution_outcome: None,
            execution_candidate_id: None,
            mode: "standard".to_string(),
            phases_passed: 6,
            min_phases_to_pass: 5,
            observation_duration_ms: 1500,
            finalize_lag_ms: 0,
            max_wait_time_ms: 2000,
            eval_count: 2,
            dust_filtered_count: 1,
            min_sol_threshold: Some(0.01),
            total_tx_evaluated: 30,
            unique_tx_evaluated: Some(28),
            min_tx_count: 8,
            unique_signers_evaluated: 25,
            min_unique_signers: 5,
            buy_count: 22,
            min_buy_count: 20,
            phase2_passed: true,
            interval_cv: Some(0.35),
            min_interval_cv: 0.30,
            max_interval_cv: 9999.0,
            burst_ratio: Some(0.65),
            max_burst_ratio: 0.70,
            avg_interval_ms: Some(120.0),
            min_avg_interval_ms: 60.0,
            max_avg_interval_ms: 6000.0,
            timing_entropy: Some(2.5),
            min_timing_entropy: 1.2,
            max_timing_entropy: 9999.0,
            min_dust_filtered_count: 0,
            phase3_passed: true,
            unique_ratio: Some(0.75),
            min_unique_ratio: 0.40,
            max_unique_ratio: 1.0,
            hhi: Some(0.10),
            max_hhi: 0.25,
            max_tx_per_signer_observed: Some(3),
            max_tx_per_signer: 4,
            volume_gini: Some(0.45),
            min_volume_gini: 0.0,
            max_volume_gini: 0.70,
            top3_volume_pct: Some(0.60),
            max_top3_volume_pct: 0.75,
            same_ms_tx_ratio: Some(0.10),
            max_same_ms_tx_ratio: 0.30,
            phase4_passed: true,
            buy_ratio: Some(0.70),
            min_buy_ratio: 0.50,
            max_buy_ratio: 1.0,
            avg_tx_sol: Some(1.2),
            min_avg_tx_sol: 0.02,
            max_avg_tx_sol: 25.0,
            volume_cv: Some(0.55),
            min_volume_cv: 0.15,
            max_volume_cv: 9999.0,
            total_volume_sol: Some(20.0),
            min_total_volume_sol: 0.5,
            max_total_volume_sol: 9999.0,
            sol_buy_ratio: Some(0.65),
            min_sol_buy_ratio: 0.50,
            max_sol_buy_ratio: 1.0,
            max_consecutive_buys_observed: Some(3),
            min_consecutive_buys: 0,
            phase5_passed: true,
            dev_wallet_known: Some(true),
            dev_buy_total_sol: Some(2.0),
            max_dev_buy_sol: 8.0,
            min_dev_buy_sol: 0.0,
            dev_tx_ratio: Some(0.10),
            max_dev_tx_ratio: 0.20,
            min_dev_tx_ratio: 0.0,
            dev_volume_ratio: Some(0.15),
            max_dev_volume_ratio: 0.40,
            min_dev_volume_ratio: 0.0,
            dev_has_sold: Some(false),
            reject_on_dev_sell: true,
            phase6_passed: true,
            price_change_ratio: Some(2.0),
            min_price_change_ratio: 0.0,
            max_price_change_ratio: 4.0,
            max_single_tx_price_impact_pct_observed: Some(5.0),
            max_single_tx_price_impact_pct: 25.0,
            bonding_progress_pct: Some(8.0),
            min_bonding_progress_pct: 0.0,
            max_bonding_progress_pct: 15.0,
            curve_data_known: Some(true),
            curve_finality: Some("provisional".to_string()),
            curve_finality_is_finalized: Some(false),
            bonding_progress_check_skipped: Some(false),
            max_single_sell_impact_pct_observed: Some(10.0),
            min_single_sell_impact_pct: 0.0,
            max_single_sell_impact_pct: 30.0,
            current_market_cap_sol: Some(30.0),
            min_market_cap_sol: 20.0,

            // Curve Readiness Latch (not used in this test)
            curve_wait_ms: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            curve_required_for_buy: None,

            // Three-Layer Decision (not used in this test)
            three_layer_enabled: false,
            hard_fail_reason: None,
            core1_passed: None,
            core2_passed: None,
            core3_passed: None,
            dev_unknown: None,
            soft_score: None,
            soft_points: None,
            max_soft_points: None,
            effective_max_soft_points: None,
            max_soft_score: None,
            soft_flags: None,
            legacy_soft_points: None,
            legacy_soft_threshold: None,
            legacy_soft_flags: None,
            sybil_soft_points: None,
            sybil_soft_threshold: None,
            total_soft_points: None,
            sybil_soft_flags: None,
            sybil_lead_signal: None,
            sybil_interference_patterns: vec![],
            sybil_meta_score: None,
            sybil_interference_layer_enabled: false,
            sybil_combo_veto_enabled: false,
            decision_reason: None,
            decision_verdict_buy: None,
            verdict_type: None,
            legacy_live_reason_chain: None,
            legacy_live_verdict_buy: None,
            legacy_live_verdict_type: None,
            v25_shadow_verdict_type: None,
            v25_shadow_reason_chain: None,
            v25_shadow_confidence: None,
            v25_shadow_confidence_source: None,
            v25_shadow_observation_stage: None,
            v25_promotion_state: Some("shadow_only".to_string()),
            alpha_gate_enabled: false,
            alpha_pass: None,
            alpha_actionable: None,
            momentum: None,
            demand: None,
            alpha_joint: None,
            min_momentum: None,
            min_demand: None,
            min_alpha_joint: None,
            min_alpha_sample: None,
            alpha_reject_trigger: None,
            alpha_skip_reason: None,
            prosperity_filter_enabled: false,
            prosperity_pass: None,
            prosperity_actionable: None,
            prosperity_reject_trigger: None,
            prosperity_market_cap_floor_pass: None,
            prosperity_cpv_pass: None,
            prosperity_branch1_pass: None,
            prosperity_branch2_pass: None,
            prosperity_branch3_pass: None,
            prosperity_overlay_enabled: false,
            prosperity_overlay_pass: None,
            prosperity_overlay_price_change_pass: None,
            prosperity_overlay_bonding_progress_pass: None,
            prosperity_overlay_fee_topology_diversity_pass: None,
            prosperity_overlay_branch23_sell_buy_pass: None,
            prosperity_overlay_branch2_price_change_pass: None,
            prosperity_matched_branches: vec![],
            prosperity_min_market_cap_sol: None,
            prosperity_max_signer_cross_pool_velocity: None,
            prosperity_branch1_min_block0_sniped_supply_pct: None,
            prosperity_branch1_max_sell_buy_ratio: None,
            prosperity_branch2_min_market_cap_sol: None,
            prosperity_branch2_min_early_slot_volume_dominance_buy: None,
            prosperity_branch3_max_hhi: None,
            prosperity_branch3_min_fee_topology_diversity_index: None,
            prosperity_overlay_max_price_change_ratio: None,
            prosperity_overlay_max_bonding_progress_pct: None,
            prosperity_overlay_min_fee_topology_diversity_index: None,
            prosperity_overlay_branch23_max_sell_buy_ratio: None,
            prosperity_overlay_branch2_max_price_change_ratio: None,
            // IWIM veto gate fields (not used in this test)
            iwim_enabled: false,
            iwim_mode: None,
            iwim_fetch_status: None,
            iwim_quality: None,
            iwim_confidence: None,
            iwim_n_tx: None,
            iwim_n_tx_requested: None,
            iwim_latency_ms: None,
            iwim_rpc_used: None,
            iwim_status: None,
            iwim_veto_reason: None,
            iwim_gatekeeper_strength: None,
            iwim_rug_threat_score: None,
            iwim_sybil_score: None,
            iwim_organic_score: None,
            iwim_snap_virtual_sol_sol: None,
            iwim_snap_virtual_tokens: None,
            iwim_snap_market_cap_sol: None,
            iwim_snap_bonding_progress_pct: None,
            iwim_snap_price_sol_per_token: None,
            // Early Fingerprint Metrics (not used in this test)
            block0_sniped_supply_pct: None,
            flip_ratio_10s: None,
            cu_price_p90_1s: None,
            cu_price_p90_10s: None,
            priority_fee_surge_slope: None,
            buyer_pre_balance_cv: None,
            avg_inner_ix_count_50tx: None,
            min_avg_inner_ix_count_50tx: 0.0,
            max_avg_inner_ix_count_50tx: 9999.0,
            avg_cpi_depth_50tx: None,
            sell_buy_ratio: Some(0.40),
            min_sell_buy_ratio: 0.0,
            max_sell_buy_ratio: 9999.0,
            compute_unit_cluster_dominance: Some(0.78),
            min_compute_unit_cluster_dominance: 0.0,
            max_compute_unit_cluster_dominance: 1.0,
            static_fee_profile_ratio: Some(0.85),
            min_static_fee_profile_ratio: 0.0,
            max_static_fee_profile_ratio: 1.0,
            fixed_size_buy_ratio: Some(0.80),
            min_fixed_size_buy_ratio: 0.0,
            fixed_size_buy_ratio_1e4: Some(0.60),
            flipper_presence_ratio: Some(0.25),
            jito_tip_intensity: Some(0.33),
            min_jito_tip_intensity: 0.0,
            max_jito_tip_intensity: 1.0,
            early_slot_volume_dominance_buy: Some(0.72),
            max_early_slot_volume_dominance_buy: 1.0,
            early_top3_buy_volume_pct_3s: Some(0.71),
            max_early_top3_buy_volume_pct_3s: 1.0,
            whale_reversal_ratio_top3: Some(0.18),
            whale_reversal_ratio_top1: Some(0.12),
            dev_paperhand_latency_ms: Some(4200),
            dev_sold_within_3s: Some(false),
            dev_sold_within_5s: Some(true),
            fingerprint_degraded: false,
            fingerprint_reason: None,
            fee_topology_diversity_index: Some(0.42),
            min_fee_topology_diversity_index: 0.0,
            dev_buyer_infrastructure_affinity: Some(0.31),
            max_dev_buyer_infrastructure_affinity: 1.0,
            spend_fraction_divergence: Some(0.27),
            min_spend_fraction_divergence: 0.0,
            demand_elasticity_score: Some(-0.25),
            min_demand_elasticity_score: -1.0,
            signer_cross_pool_velocity: Some(0.45),
            max_signer_cross_pool_velocity: 1.0,
            funding_source_concentration: Some(0.52),
            max_funding_source_concentration: 1.0,
            funding_source_diagnostics: None,
            sybil_metric_degraded_reasons: vec!["DBIA_NO_DEV_BUY".to_string()],
            // A/B Window fields (not used in this test)
            ab_window_ms: None,
            ab_t0_event_ts_ms: None,
            ab_t_end_event_ts_ms: None,
            ab_window_complete: false,
            ab_window_close_reason: None,
            ab_tx_count_window: None,
            ab_unique_signers_window: None,
            ab_fail_count_window: None,
            ab_window_origin: None,
            ab_record_id: None,
            // Window vectors (not used in this test)
            vectors_max_len: None,
            vectors_ts_offsets_ms: None,
            vectors_sol_amounts: None,
            vectors_prices: None,
            vectors_interval_ms: None,
            vectors_d_price: None,
            // V2.5 Shadow fields (not used in this test)
            shadow_extended_verdict: None,
            shadow_extended_elapsed_ms: None,
            shadow_early_verdict: None,
            shadow_normal_verdict: None,
            shadow_early_elapsed_ms: None,
            shadow_normal_elapsed_ms: None,
            shadow_early_phases_passed: None,
            shadow_normal_phases_passed: None,
            observation_stage: None,
            v25_confidence: None,
            v25_confidence_pre_veto: None,
            v25_confidence_base_quality: None,
            v25_confidence_alpha_quality: None,
            v25_confidence_pdd_modulator: None,
            v25_confidence_tas_modulator: None,
            v25_confidence_sybil_modulator: None,
            v25_confidence_zeroed_by_pdd_hard_fail: None,
            v25_confidence_zeroed_by_tas_hard_reject: None,
            tas_available: None,
            tas_unavailable_reason: None,
            pdd_sequence_signals_available: None,
            pdd_price_anchor_available: None,
            v25_confidence_available: None,
            v25_confidence_unavailable_reason: None,
            shadow_tas_reject_reason: None,
            tas_overall_score: None,
            tas_momentum_score: None,
            tas_hhi_score: None,
            tas_volume_score: None,
            tas_interval_score: None,
            tas_buy_ratio_score: None,
            pdd_hard_fail: None,
            pdd_entry_drift_pct: None,
            pdd_entry_drift_anchor_source: None,
            pdd_entry_drift_anchor_quality: None,
            pdd_spike_detected: None,
            pdd_ramping_detected: None,
            pdd_whale_top3_pct: None,
            pdd_flash_crash_risk: None,
            pdd_score: None,
            aps_regime: None,
            aps_shadow_entry_drift_max: None,
            aps_shadow_confidence_min: None,
            aps_shadow_prosperity_mcap: None,
            aps_shadow_branch1_sniped: None,
            aps_shadow_branch3_hhi: None,
            aps_shadow_prosperity_would_pass: None,
            aps_shadow_thresholds: None,
            entry_drift_pct: None,
            entry_drift_anchor_source: None,
            entry_drift_anchor_quality: None,
            market_regime: None,
            pdd_soft_flags: None,
            shadow_pdd_reject_reason: None,
            reason_code: None,
            reason_code_version: 0,
            pdd_sequence_signals_unavailable_reason: None,
        };

        // Initialize the logger
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        // Log the buy decision
        logger.log_gatekeeper_buy_decision(buy_log.clone()).await;

        // Give the async writer time to flush
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify the decisions file was created (all verdicts go here)
        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        assert!(
            decisions_file.exists(),
            "Decisions log file should be created"
        );

        // Read the file and verify content
        let content = fs::read_to_string(&decisions_file).await.unwrap();
        assert!(!content.is_empty(), "Log file should not be empty");

        // Verify it's valid JSONL (one JSON per line)
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 1, "Should have exactly one log entry");

        // Parse the JSON and verify key fields
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(
            parsed["log_schema_version"],
            crate::oracle::GATEKEEPER_BUY_LOG_SCHEMA_VERSION
        );
        assert_eq!(parsed["curve_finality"], "provisional");
        assert_eq!(parsed["pool_id"], "test_pool_123");
        assert_eq!(parsed["phases_passed"], 6);
        assert_eq!(parsed["total_tx_evaluated"], 30);
        assert_eq!(parsed["buy_count"], 22);

        // Verify phase data is present
        assert!(parsed.get("interval_cv").is_some());
        assert!(parsed.get("unique_ratio").is_some());
        assert!(parsed.get("buy_ratio").is_some());
        assert!(parsed.get("dev_wallet_known").is_some());
        assert!(parsed.get("price_change_ratio").is_some());
        assert_eq!(parsed["fee_topology_diversity_index"], 0.42);
        assert_eq!(parsed["dev_buyer_infrastructure_affinity"], 0.31);
        assert_eq!(
            parsed["sybil_metric_degraded_reasons"][0],
            "DBIA_NO_DEV_BUY"
        );

        // Shutdown the logger
        logger.shutdown().await;
    }

    /// Create a mock GatekeeperBuyLog for testing.
    fn create_test_buy_log() -> GatekeeperBuyLog {
        crate::oracle::GatekeeperBuyLog {
            log_schema_version: crate::oracle::GATEKEEPER_BUY_LOG_SCHEMA_VERSION,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            pool_id: "test_pool_dedup".to_string(),
            join_key: None,
            base_mint: None,
            first_seen_ts_ms: None,
            first_seen_clock_source: None,
            observation_start_ts_ms: None,
            observation_end_ts_ms: None,
            observation_window_ms: None,
            end_10s_ts_ms: None,
            core_pass: None,
            gatekeeper_version: None,
            rollout_profile: Some("test-rollout".to_string()),
            decision_plane: None,
            config_hash: Some("test-config-hash".to_string()),
            dev_pubkey: None,
            shadow_ready: None,
            shadow_missing_fields: None,
            shadow_metadata_source: None,
            shadow_trigger_present: None,
            // Populated downstream by launcher-side shadow handoff enrichment before persistence.
            shadow_entry_mode: None,
            shadow_trigger_eligible: None,
            shadow_execution_outcome: None,
            execution_candidate_id: None,
            mode: "standard".to_string(),
            phases_passed: 6,
            min_phases_to_pass: 5,
            observation_duration_ms: 1500,
            finalize_lag_ms: 0,
            max_wait_time_ms: 2000,
            eval_count: 2,
            dust_filtered_count: 1,
            min_sol_threshold: Some(0.01),
            total_tx_evaluated: 30,
            unique_tx_evaluated: Some(28),
            min_tx_count: 8,
            unique_signers_evaluated: 25,
            min_unique_signers: 5,
            buy_count: 22,
            min_buy_count: 20,
            phase2_passed: true,
            interval_cv: Some(0.35),
            min_interval_cv: 0.30,
            max_interval_cv: 9999.0,
            burst_ratio: Some(0.65),
            max_burst_ratio: 0.70,
            avg_interval_ms: Some(120.0),
            min_avg_interval_ms: 60.0,
            max_avg_interval_ms: 6000.0,
            timing_entropy: Some(2.5),
            min_timing_entropy: 1.2,
            max_timing_entropy: 9999.0,
            min_dust_filtered_count: 0,
            phase3_passed: true,
            unique_ratio: Some(0.75),
            min_unique_ratio: 0.40,
            max_unique_ratio: 1.0,
            hhi: Some(0.10),
            max_hhi: 0.25,
            max_tx_per_signer_observed: Some(3),
            max_tx_per_signer: 4,
            volume_gini: Some(0.45),
            min_volume_gini: 0.0,
            max_volume_gini: 0.70,
            top3_volume_pct: Some(0.60),
            max_top3_volume_pct: 0.75,
            same_ms_tx_ratio: Some(0.10),
            max_same_ms_tx_ratio: 0.30,
            phase4_passed: true,
            buy_ratio: Some(0.70),
            min_buy_ratio: 0.50,
            max_buy_ratio: 1.0,
            avg_tx_sol: Some(1.2),
            min_avg_tx_sol: 0.02,
            max_avg_tx_sol: 25.0,
            volume_cv: Some(0.55),
            min_volume_cv: 0.15,
            max_volume_cv: 9999.0,
            total_volume_sol: Some(20.0),
            min_total_volume_sol: 0.5,
            max_total_volume_sol: 9999.0,
            sol_buy_ratio: Some(0.65),
            min_sol_buy_ratio: 0.50,
            max_sol_buy_ratio: 1.0,
            max_consecutive_buys_observed: Some(3),
            min_consecutive_buys: 0,
            phase5_passed: true,
            dev_wallet_known: Some(true),
            dev_buy_total_sol: Some(2.0),
            max_dev_buy_sol: 8.0,
            min_dev_buy_sol: 0.0,
            dev_tx_ratio: Some(0.10),
            max_dev_tx_ratio: 0.20,
            min_dev_tx_ratio: 0.0,
            dev_volume_ratio: Some(0.15),
            max_dev_volume_ratio: 0.40,
            min_dev_volume_ratio: 0.0,
            dev_has_sold: Some(false),
            reject_on_dev_sell: true,
            phase6_passed: true,
            price_change_ratio: Some(2.0),
            min_price_change_ratio: 0.0,
            max_price_change_ratio: 4.0,
            max_single_tx_price_impact_pct_observed: Some(5.0),
            max_single_tx_price_impact_pct: 25.0,
            bonding_progress_pct: Some(8.0),
            min_bonding_progress_pct: 0.0,
            max_bonding_progress_pct: 15.0,
            curve_data_known: Some(true),
            curve_finality: Some("provisional".to_string()),
            curve_finality_is_finalized: Some(false),
            bonding_progress_check_skipped: Some(false),
            max_single_sell_impact_pct_observed: Some(10.0),
            min_single_sell_impact_pct: 0.0,
            max_single_sell_impact_pct: 30.0,
            current_market_cap_sol: Some(30.0),
            min_market_cap_sol: 20.0,
            curve_wait_ms: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            curve_required_for_buy: None,
            three_layer_enabled: false,
            hard_fail_reason: None,
            core1_passed: None,
            core2_passed: None,
            core3_passed: None,
            dev_unknown: None,
            soft_score: None,
            soft_points: None,
            max_soft_points: None,
            effective_max_soft_points: None,
            max_soft_score: None,
            soft_flags: None,
            legacy_soft_points: None,
            legacy_soft_threshold: None,
            legacy_soft_flags: None,
            sybil_soft_points: None,
            sybil_soft_threshold: None,
            total_soft_points: None,
            sybil_soft_flags: None,
            sybil_lead_signal: None,
            sybil_interference_patterns: vec![],
            sybil_meta_score: None,
            sybil_interference_layer_enabled: false,
            sybil_combo_veto_enabled: false,
            decision_reason: None,
            decision_verdict_buy: None,
            verdict_type: None,
            legacy_live_reason_chain: None,
            legacy_live_verdict_buy: None,
            legacy_live_verdict_type: None,
            v25_shadow_verdict_type: None,
            v25_shadow_reason_chain: None,
            v25_shadow_confidence: None,
            v25_shadow_confidence_source: None,
            v25_shadow_observation_stage: None,
            v25_promotion_state: Some("shadow_only".to_string()),
            alpha_gate_enabled: false,
            alpha_pass: None,
            alpha_actionable: None,
            momentum: None,
            demand: None,
            alpha_joint: None,
            min_momentum: None,
            min_demand: None,
            min_alpha_joint: None,
            min_alpha_sample: None,
            alpha_reject_trigger: None,
            alpha_skip_reason: None,
            prosperity_filter_enabled: false,
            prosperity_pass: None,
            prosperity_actionable: None,
            prosperity_reject_trigger: None,
            prosperity_market_cap_floor_pass: None,
            prosperity_cpv_pass: None,
            prosperity_branch1_pass: None,
            prosperity_branch2_pass: None,
            prosperity_branch3_pass: None,
            prosperity_overlay_enabled: false,
            prosperity_overlay_pass: None,
            prosperity_overlay_price_change_pass: None,
            prosperity_overlay_bonding_progress_pass: None,
            prosperity_overlay_fee_topology_diversity_pass: None,
            prosperity_overlay_branch23_sell_buy_pass: None,
            prosperity_overlay_branch2_price_change_pass: None,
            prosperity_matched_branches: vec![],
            prosperity_min_market_cap_sol: None,
            prosperity_max_signer_cross_pool_velocity: None,
            prosperity_branch1_min_block0_sniped_supply_pct: None,
            prosperity_branch1_max_sell_buy_ratio: None,
            prosperity_branch2_min_market_cap_sol: None,
            prosperity_branch2_min_early_slot_volume_dominance_buy: None,
            prosperity_branch3_max_hhi: None,
            prosperity_branch3_min_fee_topology_diversity_index: None,
            prosperity_overlay_max_price_change_ratio: None,
            prosperity_overlay_max_bonding_progress_pct: None,
            prosperity_overlay_min_fee_topology_diversity_index: None,
            prosperity_overlay_branch23_max_sell_buy_ratio: None,
            prosperity_overlay_branch2_max_price_change_ratio: None,
            iwim_enabled: false,
            iwim_mode: None,
            iwim_fetch_status: None,
            iwim_quality: None,
            iwim_confidence: None,
            iwim_n_tx: None,
            iwim_n_tx_requested: None,
            iwim_latency_ms: None,
            iwim_rpc_used: None,
            iwim_status: None,
            iwim_veto_reason: None,
            iwim_gatekeeper_strength: None,
            iwim_rug_threat_score: None,
            iwim_sybil_score: None,
            iwim_organic_score: None,
            iwim_snap_virtual_sol_sol: None,
            iwim_snap_virtual_tokens: None,
            iwim_snap_market_cap_sol: None,
            iwim_snap_bonding_progress_pct: None,
            iwim_snap_price_sol_per_token: None,
            block0_sniped_supply_pct: None,
            flip_ratio_10s: None,
            cu_price_p90_1s: None,
            cu_price_p90_10s: None,
            priority_fee_surge_slope: None,
            buyer_pre_balance_cv: None,
            avg_inner_ix_count_50tx: None,
            min_avg_inner_ix_count_50tx: 0.0,
            max_avg_inner_ix_count_50tx: 9999.0,
            avg_cpi_depth_50tx: None,
            sell_buy_ratio: None,
            min_sell_buy_ratio: 0.0,
            max_sell_buy_ratio: 9999.0,
            compute_unit_cluster_dominance: None,
            min_compute_unit_cluster_dominance: 0.0,
            max_compute_unit_cluster_dominance: 1.0,
            static_fee_profile_ratio: None,
            min_static_fee_profile_ratio: 0.0,
            max_static_fee_profile_ratio: 1.0,
            fixed_size_buy_ratio: None,
            min_fixed_size_buy_ratio: 0.0,
            fixed_size_buy_ratio_1e4: None,
            flipper_presence_ratio: None,
            jito_tip_intensity: None,
            min_jito_tip_intensity: 0.0,
            max_jito_tip_intensity: 1.0,
            early_slot_volume_dominance_buy: None,
            max_early_slot_volume_dominance_buy: 1.0,
            early_top3_buy_volume_pct_3s: None,
            max_early_top3_buy_volume_pct_3s: 1.0,
            whale_reversal_ratio_top3: None,
            whale_reversal_ratio_top1: None,
            dev_paperhand_latency_ms: None,
            dev_sold_within_3s: None,
            dev_sold_within_5s: None,
            fingerprint_degraded: false,
            fingerprint_reason: None,
            fee_topology_diversity_index: Some(0.42),
            min_fee_topology_diversity_index: 0.0,
            dev_buyer_infrastructure_affinity: Some(0.31),
            max_dev_buyer_infrastructure_affinity: 1.0,
            spend_fraction_divergence: Some(0.27),
            min_spend_fraction_divergence: 0.0,
            demand_elasticity_score: Some(-0.25),
            min_demand_elasticity_score: -1.0,
            signer_cross_pool_velocity: Some(0.45),
            max_signer_cross_pool_velocity: 1.0,
            funding_source_concentration: Some(0.52),
            max_funding_source_concentration: 1.0,
            funding_source_diagnostics: None,
            sybil_metric_degraded_reasons: vec!["DBIA_NO_DEV_BUY".to_string()],
            ab_window_ms: Some(10_000),
            ab_t0_event_ts_ms: Some(1000),
            ab_t_end_event_ts_ms: Some(11_000),
            ab_window_complete: true,
            ab_window_close_reason: Some("END_REACHED".to_string()),
            ab_tx_count_window: Some(10),
            ab_unique_signers_window: Some(5),
            ab_fail_count_window: Some(0),
            ab_window_origin: Some("NewPoolDetected".to_string()),
            ab_record_id: Some("test_pool_dedup:1000:11000:REJECT".to_string()),
            vectors_max_len: None,
            vectors_ts_offsets_ms: None,
            vectors_sol_amounts: None,
            vectors_prices: None,
            vectors_interval_ms: None,
            vectors_d_price: None,
            // V2.5 Shadow fields (not used in this test)
            shadow_extended_verdict: None,
            shadow_extended_elapsed_ms: None,
            shadow_early_verdict: None,
            shadow_normal_verdict: None,
            shadow_early_elapsed_ms: None,
            shadow_normal_elapsed_ms: None,
            shadow_early_phases_passed: None,
            shadow_normal_phases_passed: None,
            observation_stage: None,
            v25_confidence: None,
            v25_confidence_pre_veto: None,
            v25_confidence_base_quality: None,
            v25_confidence_alpha_quality: None,
            v25_confidence_pdd_modulator: None,
            v25_confidence_tas_modulator: None,
            v25_confidence_sybil_modulator: None,
            v25_confidence_zeroed_by_pdd_hard_fail: None,
            v25_confidence_zeroed_by_tas_hard_reject: None,
            tas_available: None,
            tas_unavailable_reason: None,
            pdd_sequence_signals_available: None,
            pdd_price_anchor_available: None,
            v25_confidence_available: None,
            v25_confidence_unavailable_reason: None,
            shadow_tas_reject_reason: None,
            tas_overall_score: None,
            tas_momentum_score: None,
            tas_hhi_score: None,
            tas_volume_score: None,
            tas_interval_score: None,
            tas_buy_ratio_score: None,
            pdd_hard_fail: None,
            pdd_entry_drift_pct: None,
            pdd_entry_drift_anchor_source: None,
            pdd_entry_drift_anchor_quality: None,
            pdd_spike_detected: None,
            pdd_ramping_detected: None,
            pdd_whale_top3_pct: None,
            pdd_flash_crash_risk: None,
            pdd_score: None,
            aps_regime: None,
            aps_shadow_entry_drift_max: None,
            aps_shadow_confidence_min: None,
            aps_shadow_prosperity_mcap: None,
            aps_shadow_branch1_sniped: None,
            aps_shadow_branch3_hhi: None,
            aps_shadow_prosperity_would_pass: None,
            aps_shadow_thresholds: None,
            entry_drift_pct: None,
            entry_drift_anchor_source: None,
            entry_drift_anchor_quality: None,
            market_regime: None,
            pdd_soft_flags: None,
            shadow_pdd_reject_reason: None,
            reason_code: None,
            reason_code_version: 0,
            pdd_sequence_signals_unavailable_reason: None,
        }
    }

    fn test_gatekeeper_route_dir(
        base: &Path,
        gatekeeper_version: &str,
        decision_plane: &str,
    ) -> PathBuf {
        base.join("test-rollout")
            .join(gatekeeper_version)
            .join(decision_plane)
            .join("test-config-hash")
    }

    #[tokio::test]
    async fn test_ab_record_id_dedup_guard() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();

        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        let buy_log = create_test_buy_log();

        // Send the same record 3 times (simulating double-write bug)
        logger.log_gatekeeper_buy_decision(buy_log.clone()).await;
        logger.log_gatekeeper_buy_decision(buy_log.clone()).await;
        logger.log_gatekeeper_buy_decision(buy_log.clone()).await;

        // Give the async writer time to process all 3 messages
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Read the JSONL file — should contain exactly 1 line, not 3
        let log_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        assert!(log_file.exists(), "Log file should be created");
        let content = fs::read_to_string(&log_file).await.unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "Dedup guard must write exactly 1 record, got {}",
            lines.len()
        );

        // Verify the written record has the correct ab_record_id
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(
            parsed["ab_record_id"], "test_pool_dedup:1000:11000:REJECT",
            "Written record should have correct ab_record_id"
        );

        // A DIFFERENT ab_record_id should still be written
        let mut buy_log2 = create_test_buy_log();
        buy_log2.ab_record_id = Some("test_pool_dedup:1000:11000:BUY".to_string());
        logger.log_gatekeeper_buy_decision(buy_log2).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let content2 = fs::read_to_string(&log_file).await.unwrap();
        let lines2: Vec<&str> = content2.trim().lines().collect();
        assert_eq!(
            lines2.len(),
            2,
            "Different ab_record_id should produce 2 records total, got {}",
            lines2.len()
        );

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_decisions_vs_passed_routing() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();

        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        // 1× PASS (decision_verdict_buy = Some(true))
        let mut pass_log = create_test_buy_log();
        pass_log.pool_id = "pool_pass".to_string();
        pass_log.decision_verdict_buy = Some(true);
        pass_log.verdict_type = Some("BUY".to_string());
        pass_log.ab_record_id = Some("pool_pass:1000:11000:BUY".to_string());
        pass_log.sell_buy_ratio = Some(0.40);
        pass_log.compute_unit_cluster_dominance = Some(0.78);
        pass_log.static_fee_profile_ratio = Some(0.85);
        pass_log.fixed_size_buy_ratio = Some(0.80);
        pass_log.fixed_size_buy_ratio_1e4 = Some(0.60);
        pass_log.flipper_presence_ratio = Some(0.25);
        pass_log.jito_tip_intensity = Some(0.33);
        pass_log.early_slot_volume_dominance_buy = Some(0.72);
        pass_log.early_top3_buy_volume_pct_3s = Some(0.71);
        pass_log.whale_reversal_ratio_top3 = Some(0.18);
        pass_log.whale_reversal_ratio_top1 = Some(0.12);
        pass_log.dev_paperhand_latency_ms = Some(4200);
        pass_log.dev_sold_within_3s = Some(false);
        pass_log.dev_sold_within_5s = Some(true);

        // 1× REJECT (decision_verdict_buy = Some(false))
        let mut reject_log = create_test_buy_log();
        reject_log.pool_id = "pool_reject".to_string();
        reject_log.decision_verdict_buy = Some(false);
        reject_log.verdict_type = Some("REJECT_CORE_FAIL".to_string());
        reject_log.ab_record_id = Some("pool_reject:1000:11000:REJECT".to_string());

        // 1× TIMEOUT (decision_verdict_buy = None)
        let mut timeout_log = create_test_buy_log();
        timeout_log.pool_id = "pool_timeout".to_string();
        timeout_log.decision_verdict_buy = None;
        timeout_log.verdict_type = Some("TIMEOUT_PHASE1".to_string());
        timeout_log.ab_record_id = Some("pool_timeout:1000:11000:TIMEOUT".to_string());

        logger.log_gatekeeper_buy_decision(pass_log).await;
        logger.log_gatekeeper_buy_decision(reject_log).await;
        logger.log_gatekeeper_buy_decision(timeout_log).await;

        // Give the async writer time to flush
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // decisions file should have ALL 3 records
        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        assert!(decisions_file.exists(), "Decisions file should exist");
        let decisions_content = fs::read_to_string(&decisions_file).await.unwrap();
        let decisions_lines: Vec<&str> = decisions_content.trim().lines().collect();
        assert_eq!(
            decisions_lines.len(),
            3,
            "Decisions file must have 3 lines (PASS+REJECT+TIMEOUT), got {}",
            decisions_lines.len()
        );

        // passed/buys file should have only 1 record (the PASS)
        let passed_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_PASSED_JSONL);
        assert!(passed_file.exists(), "Passed/buys file should exist");
        let passed_content = fs::read_to_string(&passed_file).await.unwrap();
        let passed_lines: Vec<&str> = passed_content.trim().lines().collect();
        assert_eq!(
            passed_lines.len(),
            1,
            "Passed/buys file must have exactly 1 line (only PASS), got {}",
            passed_lines.len()
        );

        // Verify the passed record is the correct one
        let parsed: serde_json::Value = serde_json::from_str(passed_lines[0]).unwrap();
        assert_eq!(parsed["pool_id"], "pool_pass");
        assert_eq!(parsed["decision_verdict_buy"], true);
        assert_eq!(parsed["sell_buy_ratio"], 0.40);
        assert_eq!(parsed["compute_unit_cluster_dominance"], 0.78);
        assert_eq!(parsed["static_fee_profile_ratio"], 0.85);
        assert_eq!(parsed["fixed_size_buy_ratio"], 0.80);
        assert_eq!(parsed["fixed_size_buy_ratio_1e4"], 0.60);
        assert_eq!(parsed["flipper_presence_ratio"], 0.25);
        assert_eq!(parsed["jito_tip_intensity"], 0.33);
        assert_eq!(parsed["early_slot_volume_dominance_buy"], 0.72);
        assert_eq!(parsed["early_top3_buy_volume_pct_3s"], 0.71);
        assert_eq!(parsed["fee_topology_diversity_index"], 0.42);
        assert_eq!(parsed["dev_buyer_infrastructure_affinity"], 0.31);
        assert_eq!(
            parsed["sybil_metric_degraded_reasons"][0],
            "DBIA_NO_DEV_BUY"
        );
        assert_eq!(parsed["whale_reversal_ratio_top3"], 0.18);
        assert_eq!(parsed["whale_reversal_ratio_top1"], 0.12);
        assert_eq!(parsed["dev_paperhand_latency_ms"], 4200);
        assert_eq!(parsed["dev_sold_within_3s"], false);
        assert_eq!(parsed["dev_sold_within_5s"], true);

        let decision_records: Vec<serde_json::Value> = decisions_lines
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let pass_in_decisions = decision_records
            .iter()
            .find(|record| record["pool_id"] == "pool_pass")
            .expect("PASS record should also be present in decisions file");
        assert_eq!(pass_in_decisions["sell_buy_ratio"], 0.40);
        assert_eq!(pass_in_decisions["compute_unit_cluster_dominance"], 0.78);
        assert_eq!(pass_in_decisions["static_fee_profile_ratio"], 0.85);
        assert_eq!(pass_in_decisions["fixed_size_buy_ratio"], 0.80);
        assert_eq!(pass_in_decisions["fixed_size_buy_ratio_1e4"], 0.60);
        assert_eq!(pass_in_decisions["flipper_presence_ratio"], 0.25);
        assert_eq!(pass_in_decisions["jito_tip_intensity"], 0.33);
        assert_eq!(pass_in_decisions["early_slot_volume_dominance_buy"], 0.72);
        assert_eq!(pass_in_decisions["early_top3_buy_volume_pct_3s"], 0.71);
        assert_eq!(pass_in_decisions["whale_reversal_ratio_top3"], 0.18);
        assert_eq!(pass_in_decisions["whale_reversal_ratio_top1"], 0.12);
        assert_eq!(pass_in_decisions["dev_paperhand_latency_ms"], 4200);
        assert_eq!(pass_in_decisions["dev_sold_within_3s"], false);
        assert_eq!(pass_in_decisions["dev_sold_within_5s"], true);

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_logger_splits_legacy_and_shadow_planes() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        let mut mixed_log = create_test_buy_log();
        mixed_log.pool_id = "pool_mixed".to_string();
        mixed_log.decision_reason = Some("legacy_buy".to_string());
        mixed_log.decision_verdict_buy = Some(true);
        mixed_log.verdict_type = Some("BUY".to_string());
        mixed_log.legacy_live_reason_chain = Some("legacy_buy".to_string());
        mixed_log.legacy_live_verdict_buy = Some(true);
        mixed_log.legacy_live_verdict_type = Some("BUY".to_string());
        mixed_log.v25_shadow_reason_chain = Some("shadow reject due to PDD".to_string());
        mixed_log.v25_shadow_verdict_type = Some("REJECT_PUMP_AND_DUMP".to_string());
        mixed_log.v25_shadow_confidence = Some(0.0);
        mixed_log.v25_shadow_observation_stage = Some("Normal".to_string());
        mixed_log.ab_record_id = Some("pool_mixed:1000:11000:MIXED".to_string());

        logger.log_gatekeeper_buy_decision(mixed_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let legacy_dir = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        );
        let shadow_dir =
            test_gatekeeper_route_dir(&log_dir, GATEKEEPER_VERSION, DECISION_PLANE_V25_SHADOW);

        let legacy_decisions = legacy_dir.join(GATEKEEPER_DECISIONS_JSONL);
        let shadow_decisions = shadow_dir.join(GATEKEEPER_DECISIONS_JSONL);
        let legacy_buys = legacy_dir.join(GATEKEEPER_PASSED_JSONL);
        let shadow_buys = shadow_dir.join(GATEKEEPER_PASSED_JSONL);

        assert!(
            legacy_decisions.exists(),
            "legacy plane decisions file should exist"
        );
        assert!(
            shadow_decisions.exists(),
            "shadow plane decisions file should exist"
        );
        assert!(legacy_buys.exists(), "legacy BUY file should exist");
        assert!(
            !shadow_buys.exists(),
            "shadow BUY file should not exist for a shadow reject"
        );

        let legacy_lines = fs::read_to_string(&legacy_decisions).await.unwrap();
        let shadow_lines = fs::read_to_string(&shadow_decisions).await.unwrap();
        assert_eq!(legacy_lines.trim().lines().count(), 1);
        assert_eq!(shadow_lines.trim().lines().count(), 1);

        let legacy_record: serde_json::Value =
            serde_json::from_str(legacy_lines.trim().lines().next().unwrap()).unwrap();
        let shadow_record: serde_json::Value =
            serde_json::from_str(shadow_lines.trim().lines().next().unwrap()).unwrap();

        assert_eq!(legacy_record["decision_plane"], DECISION_PLANE_LEGACY_LIVE);
        assert_eq!(
            legacy_record["gatekeeper_version"],
            LEGACY_GATEKEEPER_VERSION
        );
        assert_eq!(legacy_record["decision_verdict_buy"], true);
        assert_eq!(legacy_record["verdict_type"], "BUY");
        assert_eq!(
            legacy_record["v25_shadow_verdict_type"],
            "REJECT_PUMP_AND_DUMP"
        );

        assert_eq!(shadow_record["decision_plane"], DECISION_PLANE_V25_SHADOW);
        assert_eq!(shadow_record["gatekeeper_version"], GATEKEEPER_VERSION);
        assert_eq!(shadow_record["decision_verdict_buy"], false);
        assert_eq!(shadow_record["verdict_type"], "REJECT_PUMP_AND_DUMP");
        assert_eq!(shadow_record["decision_reason"], "shadow reject due to PDD");
        assert_eq!(shadow_record["legacy_live_verdict_type"], "BUY");

        logger.shutdown().await;
    }

    /// Every JSONL record must include verdict_type — even when upstream
    /// fixtures omit it. Runtime BUY/REJECT/TIMEOUT assessments now attach
    /// decision metadata, so `verdict_type = None` should only appear on
    /// malformed legacy rows or partially populated test fixtures.
    #[tokio::test]
    async fn test_verdict_type_always_present() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        // Case 1: legacy/malformed fallback: verdict_type = None, decision_verdict_buy = None → TIMEOUT
        let mut timeout_log = create_test_buy_log();
        timeout_log.pool_id = "pool_vt_timeout".to_string();
        timeout_log.decision_verdict_buy = None;
        timeout_log.verdict_type = None;
        timeout_log.ab_record_id = Some("pool_vt_timeout:1:2:A".to_string());

        // Case 2: verdict_type = None, decision_verdict_buy = Some(false) → REJECT_UNKNOWN
        let mut reject_log = create_test_buy_log();
        reject_log.pool_id = "pool_vt_reject".to_string();
        reject_log.decision_verdict_buy = Some(false);
        reject_log.verdict_type = None;
        reject_log.ab_record_id = Some("pool_vt_reject:1:2:B".to_string());

        // Case 3: verdict_type = None, decision_verdict_buy = Some(true) → BUY
        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_vt_buy".to_string();
        buy_log.decision_verdict_buy = Some(true);
        buy_log.verdict_type = None;
        buy_log.ab_record_id = Some("pool_vt_buy:1:2:C".to_string());

        logger.log_gatekeeper_buy_decision(timeout_log).await;
        logger.log_gatekeeper_buy_decision(reject_log).await;
        logger.log_gatekeeper_buy_decision(buy_log).await;

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        let content = fs::read_to_string(&decisions_file).await.unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 3, "Expected 3 records in decisions file");

        // Find each record by pool_id and verify verdict_type was filled in.
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            let pool_id = parsed["pool_id"].as_str().unwrap_or("");
            let vt = parsed["verdict_type"].as_str();
            match pool_id {
                "pool_vt_timeout" => assert_eq!(
                    vt,
                    Some("TIMEOUT"),
                    "TIMEOUT path must emit verdict_type=TIMEOUT"
                ),
                "pool_vt_reject" => assert_eq!(
                    vt,
                    Some("REJECT_UNKNOWN"),
                    "REJECT path with missing verdict_type must emit verdict_type=REJECT_UNKNOWN"
                ),
                "pool_vt_buy" => assert_eq!(vt, Some("BUY"), "BUY path must emit verdict_type=BUY"),
                _ => panic!("Unexpected pool_id: {}", pool_id),
            }
        }

        logger.shutdown().await;
    }
}
