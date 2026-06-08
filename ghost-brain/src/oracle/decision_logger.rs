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
use ghost_core::features::coordination::CoordinationRiskEvidenceUnit;
use ghost_core::health::RuntimeHealth;
use ghost_core::tx_intelligence::types::{FscV2Evidence, FundingSourceDiagnostics};
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
/// v20 adds additive V3 P0 shadow/evidence sidecar fields on existing decision rows.
/// v21 adds ordered Gatekeeper gate trace diagnostics.
/// v22 adds Gatekeeper V2 replay-input contract fields for manifest-locked
/// offline axis replay.
/// v23 adds additive replay payload hash fields for V3 shadow-sidecar parity.
/// v24 adds additive FSC v2 evidence capture fields without policy activation.
/// v25 adds additive FSC v2 shadow-policy counterfactual fields without verdict impact.
pub const GATEKEEPER_BUY_LOG_SCHEMA_VERSION: u32 = 25;
/// Gatekeeper version string embedded in every V2.5 shadow BUY log for traceability.
pub const GATEKEEPER_VERSION: &str = "v2.5";
/// Legacy Gatekeeper version string for pre-V2.5 live-plane semantics.
pub const LEGACY_GATEKEEPER_VERSION: &str = "v2.2";
const COORDINATION_RISK_EVIDENCE_JSONL: &str = "coordination_risk_evidence.jsonl";
/// Additive selector score sidecar. This file is diagnostic-only and must never
/// be consumed as a Gatekeeper verdict or execution signal.
pub const SELECTOR_SHADOW_SCORE_JSONL: &str = "selector_shadow_score_v1.jsonl";
const SELECTOR_SHADOW_SCORE_SCHEMA_VERSION: &str = "selector_shadow_score_v1";
const SELECTOR_SHADOW_SCORE_VERSION: &str = "selector_shadow_score_combined_simple_v1";
const SELECTOR_SHADOW_SCORE_CANDIDATE_ID: &str = "combined:simple_feature_score_v1";
const SELECTOR_SHADOW_SCORE_VALID: &str = "score_valid";
const SELECTOR_SHADOW_SCORE_DEGRADED_MISSING_CONCENTRATION: &str =
    "score_degraded_missing_concentration";
const SELECTOR_SHADOW_SCORE_INVALID_MISSING_CORE: &str = "score_invalid_missing_core_curve_market";
const SELECTOR_SHADOW_SCORE_INVALID_CUTOFF_UNVERIFIED: &str = "score_invalid_cutoff_unverified";
const SELECTOR_SHADOW_TOP10_EQUIV_THRESHOLD: f64 = 0.69986805365771;
const SELECTOR_SHADOW_TOP25_EQUIV_THRESHOLD: f64 = 0.6925460600540485;
const SELECTOR_SHADOW_Q99_THRESHOLD: f64 = 0.6944972661163069;
const SELECTOR_SHADOW_Q98_THRESHOLD: f64 = 0.6910207260694384;
const SELECTOR_SHADOW_Q975_THRESHOLD: f64 = 0.6897754365786619;
const SELECTOR_SHADOW_TARGET_PRECISION_0_70_THRESHOLD: f64 = 0.6851504774409787;
const DECISION_PLANE_LEGACY_LIVE: &str = "legacy_live";
const DECISION_PLANE_V25_SHADOW: &str = "v25_shadow";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorShadowRuntimeFeatureSource {
    Mapped,
    MissingRuntimeMapping,
}

#[derive(Debug, Clone, Copy)]
struct SelectorShadowFeatureSpec {
    name: &'static str,
    min: f64,
    max: f64,
    direction: f64,
    source: SelectorShadowRuntimeFeatureSource,
}

// Frozen from r19 P3J/P3K:
// selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final
// candidate_id=combined:simple_feature_score_v1.
const SELECTOR_SHADOW_FEATURE_SPECS: &[SelectorShadowFeatureSpec] = &[
    SelectorShadowFeatureSpec {
        name: "net_quote_in_15s",
        min: -0.93354436800000196,
        max: 267.14919743400009,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping,
    },
    SelectorShadowFeatureSpec {
        name: "net_quote_in_30s",
        min: -0.93354436800000196,
        max: 267.14919743400009,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping,
    },
    SelectorShadowFeatureSpec {
        name: "trade_rate",
        min: 0.0,
        max: 17.998200179982003,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping,
    },
    SelectorShadowFeatureSpec {
        name: "unique_buyers",
        min: 0.0,
        max: 107.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping,
    },
    SelectorShadowFeatureSpec {
        name: "sell_share",
        min: 0.0,
        max: 0.90909090909090906,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping,
    },
    SelectorShadowFeatureSpec {
        name: "top1_wallet_share",
        min: 0.061039535295271542,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping,
    },
    SelectorShadowFeatureSpec {
        name: "buyer_hhi",
        min: 0.018038408746430926,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping,
    },
    SelectorShadowFeatureSpec {
        name: "gk_curve_wait_elapsed_ms",
        min: 10002.0,
        max: 10033.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_bonding_progress_pct",
        min: 0.0,
        max: 100.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_current_market_cap_sol",
        min: 0.0,
        max: 215.158814914,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_price_change_ratio",
        min: 0.0,
        max: 7.2846422337758385,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_max_single_tx_price_impact_pct_observed",
        min: 0.0,
        max: 1277.0596242129784,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_max_single_sell_impact_pct_observed",
        min: 0.0,
        max: 85.206741389537655,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_total_tx_evaluated",
        min: 0.0,
        max: 174.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_unique_tx_evaluated",
        min: 0.0,
        max: 174.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_unique_signers_evaluated",
        min: 0.0,
        max: 103.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_buy_count",
        min: 0.0,
        max: 109.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_buy_ratio",
        min: 0.090909090909090912,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_sell_buy_ratio",
        min: 0.0,
        max: 10.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_sol_buy_ratio",
        min: 0.48712774238836481,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_total_volume_sol",
        min: 0.001,
        max: 267.42312335400004,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_avg_tx_sol",
        min: 0.001,
        max: 39.506172837999998,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_volume_cv",
        min: 0.0,
        max: 5.0730213797575932,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_volume_gini",
        min: 0.0,
        max: 0.93775422557405719,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_hhi",
        min: 0.011687363038714402,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_top3_volume_pct",
        min: 0.16526499168337733,
        max: 1.0000000000000013,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_same_ms_tx_ratio",
        min: 0.0,
        max: 0.72727272727272729,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_max_consecutive_buys_observed",
        min: 1.0,
        max: 57.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_dev_buy_total_sol",
        min: 0.0,
        max: 85.005359057000007,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_dev_tx_ratio",
        min: 0.0,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_dev_volume_ratio",
        min: 0.0,
        max: 1.0000000000000002,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_dev_has_sold",
        min: 0.0,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_dev_sold_within_3s",
        min: 0.0,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_dev_sold_within_5s",
        min: 0.0,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_block0_sniped_supply_pct",
        min: 0.0,
        max: 1.0435198862184567,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_flip_ratio_10s",
        min: 0.0,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_buyer_pre_balance_cv",
        min: 0.43297197375371305,
        max: 7.1006613119179018,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_cu_price_p90_1s",
        min: 1.0,
        max: 15000000000.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_cu_price_p90_10s",
        min: 1.0,
        max: 2444444444.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_priority_fee_surge_slope",
        min: -1664965880.7777777,
        max: 265113805.1111111,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_fee_topology_diversity_index",
        min: 0.042857142857142858,
        max: 1.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_dev_buyer_infrastructure_affinity",
        min: 0.0,
        max: 0.55000000000000004,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_spend_fraction_divergence",
        min: 0.0,
        max: 0.44452863472766163,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_demand_elasticity_score",
        min: -1.0,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_signer_cross_pool_velocity",
        min: 0.0,
        max: 1.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_compute_unit_cluster_dominance",
        min: 0.16037735849056603,
        max: 0.92307692307692313,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_static_fee_profile_ratio",
        min: 0.058823529411764705,
        max: 1.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_fixed_size_buy_ratio",
        min: 0.024691358024691357,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_flipper_presence_ratio",
        min: 0.0,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_jito_tip_intensity",
        min: 0.0,
        max: 1.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_early_slot_volume_dominance_buy",
        min: 0.0,
        max: 1.0000000000000002,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_early_top3_buy_volume_pct_3s",
        min: 0.16313553352604218,
        max: 1.0000000000000002,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_whale_reversal_ratio_top3",
        min: 0.0,
        max: 1.4811318070421395,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_whale_reversal_ratio_top1",
        min: 0.0,
        max: 3.3959483927726843,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_iwim_confidence",
        min: 0.0,
        max: 1.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_iwim_sybil_score",
        min: 0.20000000000000001,
        max: 0.40000000000000002,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_iwim_organic_score",
        min: 0.90000000000000002,
        max: 1.0,
        direction: -1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_fsc_buyer_sample_count",
        min: 0.0,
        max: 88.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_event_count",
        min: 1.0,
        max: 48.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_price_first",
        min: 2.7960857440198816e-08,
        max: 4.1088016812075742e-07,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_price_last",
        min: 6.5405163524447076e-09,
        max: 1.7881629156092599e-06,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_price_return",
        min: -0.76761931836160091,
        max: 58.944681273478011,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_price_max",
        min: 2.7960857440198816e-08,
        max: 1.7881629156092599e-06,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_price_min",
        min: 6.5405163524447076e-09,
        max: 4.1088016812075742e-07,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_price_drawdown",
        min: 0.0,
        max: 0.76761931836160091,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_sol_sum",
        min: 0.001,
        max: 262.35652092100003,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_sol_max",
        min: 0.001,
        max: 89.686098654000006,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_interval_median",
        min: 0.0,
        max: 1998.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_interval_min",
        min: 0.0,
        max: 1998.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
    SelectorShadowFeatureSpec {
        name: "gk_vector_interval_max",
        min: 0.0,
        max: 1998.0,
        direction: 1.0,
        source: SelectorShadowRuntimeFeatureSource::Mapped,
    },
];

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

/// Ordered policy-gate trace entry emitted by Gatekeeper evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatekeeperGateTraceEntry {
    pub order_idx: u32,
    pub gate: String,
    pub status: String,
    pub hard_or_soft: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
}

/// Shadow-only selector score sidecar row.
///
/// P3L-A intentionally emits a schema and feature availability inventory before
/// enabling any runtime score calculation. This keeps the offline P3K score
/// contract auditable without mutating Gatekeeper decisions, execution
/// eligibility, or live/send paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorShadowScoreSidecarLog {
    pub schema_version: String,
    pub score_version: String,
    pub score_candidate_id: String,
    pub scope: String,
    pub decision_plane: String,
    pub candidate_id: String,
    pub pool_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_mint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_verdict_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_ts_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_cutoff_ts_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_shadow_score: Option<f64>,
    pub score_validity_status: String,
    pub score_valid: bool,
    pub score_degraded: bool,
    pub feature_missing_count: usize,
    pub required_feature_missing_count: usize,
    pub thresholds: SelectorShadowScoreThresholds,
    pub reason_vector: SelectorShadowScoreReasonVector,
    pub feature_availability: SelectorShadowScoreFeatureAvailability,
    pub claim_boundaries: SelectorShadowScoreClaimBoundaries,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorShadowScoreThresholds {
    pub top10_equiv_pass: bool,
    pub top25_equiv_pass: bool,
    pub q99_pass: bool,
    pub q98_pass: bool,
    pub q975_pass: bool,
    pub target_precision_0_70_pass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorShadowScoreReasonVector {
    pub positive: Vec<String>,
    pub negative: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorShadowScoreFeatureAvailability {
    pub core_curve_market_available: bool,
    pub concentration_available: bool,
    pub gk_context_available: bool,
    pub flow_available: bool,
    pub runtime_score_adapter_available: bool,
    pub feature_mapping_status: String,
    pub mapped_feature_count: usize,
    pub missing_runtime_mapping_count: usize,
    pub cutoff_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorShadowScoreClaimBoundaries {
    pub diagnostic_only: bool,
    pub shadow_only: bool,
    pub production_promotion_allowed: bool,
    pub gatekeeper_tuning_started: bool,
    pub changes_gatekeeper_decision: bool,
    pub changes_execution: bool,
    pub send_path_changed: bool,
}

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

    /// Rollout/runtime run identifier. R16 uses this to prove that decision,
    /// probe, and lifecycle artifacts come from one namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,

    /// Rollout/runtime session identifier. Additive for legacy row parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Ghost Brain config path used by the launcher for this decision row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_config_path: Option<String>,

    /// Blake3 hash of the Ghost Brain config file bytes used by this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_config_hash: Option<String>,

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

    /// Typed reason code (GatekeeperReasonCode taxonomy).
    /// Always populated for every verdict type (BUY, REJECT, TIMEOUT).
    /// Replaces the legacy free-form `decision_reason` for machine auditability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,

    /// Active reason code taxonomy version (2 = V2.5 active verdict contract).
    /// V3 P0 sidecar codes are versioned by `v3_shadow_schema_version`.
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

    // ═══════════════════════════════════════════
    // V3 P0 shadow/evidence sidecar fields (schema v20)
    // These are additive fields on the existing decision rows, not a separate
    // routed decision plane.
    // ═══════════════════════════════════════════
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_reason_chain: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_secondary_reason_codes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_risk_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_risk_primary_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_risk_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_opportunity_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_opportunity_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_confidence_raw: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_confidence_after_risk: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_confidence_after_stage: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_confidence_cap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_confidence_cap_reasons: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_confidence_final: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_evidence_status: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_organic_broadening: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_manipulation_contradictions: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_evidence_status: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_organic_broadening: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_manipulation_contradictions: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_policy_config_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_feature_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_replay_payload_schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_materialized_feature_snapshot: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_policy_config_payload: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_materialization_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_policy_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_stage_thresholds: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_component_scores: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_actionability: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v3_shadow_notes: Option<serde_json::Value>,

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
    /// Additive FSC v2 evidence payload. Export-only until FSC v2 policy is explicitly enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_source_v2: Option<FscV2Evidence>,
    /// Shadow-only indication whether FSC v2 would be a policy signal if enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_fsc_v2_policy_signal: Option<bool>,
    /// Shadow-only soft points FSC v2 would add if the FSC policy branch were enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_fsc_v2_soft_points_if_enabled: Option<u16>,
    /// Shadow-only reason for the FSC v2 counterfactual.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_fsc_v2_reason_if_enabled: Option<String>,

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
    /// Elapsed ms between the exact PDD drift anchor and current price point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_elapsed_ms: Option<u64>,
    /// Exact anchor price used by PDD drift detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_anchor_price: Option<f64>,
    /// Exact current price used by PDD drift detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_current_price: Option<f64>,
    /// Exact anchor timestamp used by PDD drift detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_anchor_ts_ms: Option<u64>,
    /// Exact current timestamp used by PDD drift detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_current_ts_ms: Option<u64>,
    /// Static configured PDD drift max.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_static_max_pct: Option<f64>,
    /// Elapsed-scaled PDD drift max when enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_elapsed_max_pct: Option<f64>,
    /// Effective drift threshold used for this PDD decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_effective_max_pct: Option<f64>,
    /// Threshold source: static, regime_static, elapsed_scaled, fallback_no_anchor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_threshold_source: Option<String>,
    /// Entry drift anchor source provenance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_anchor_source: Option<String>,
    /// Entry drift anchor quality: "strong" / "weak"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_entry_drift_anchor_quality: Option<String>,
    /// Volume spike pattern detected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_spike_detected: Option<bool>,
    /// Recent-vs-earlier volume-rate ratio used for spike detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_spike_ratio: Option<f64>,
    /// Quality/status for pdd_spike_ratio: ok, earlier_rate_zero,
    /// insufficient_earlier_window, insufficient_recent_window, unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_spike_ratio_quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_spike_recent_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_spike_earlier_rate: Option<f64>,
    /// Consecutive same-size buy ramping detected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_ramping_detected: Option<bool>,
    /// Top-3 whale volume concentration percentage
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_whale_top3_pct: Option<f64>,
    /// Single largest signer volume concentration percentage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_whale_single_max_pct: Option<f64>,
    /// Flash crash sell cluster risk detected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_flash_crash_risk: Option<bool>,
    /// Overall PDD cleanliness score (1.0 = clean, 0.0 = hard fail)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_score: Option<f64>,

    // ═══════════════════════════════════════════
    // Gate trace diagnostics (v21)
    // ═══════════════════════════════════════════
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_first_kill_gate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_first_kill_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_terminal_gate: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gatekeeper_gate_trace: Vec<GatekeeperGateTraceEntry>,

    // ═══════════════════════════════════════════
    // Gatekeeper V2 replay input contract (v22)
    // ═══════════════════════════════════════════
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_v2_replay_input_schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_v2_replay_ready_non_temporal: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_v2_replay_ready_temporal: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gatekeeper_v2_replay_missing_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_v2_phase_pass_vector: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_reject_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_soft_penalty_points: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdd_hard_fail_evaluated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_fail_hhi_threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_window_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_eval_snapshots: Option<serde_json::Value>,

    // ═══════════════════════════════════════════
    // Full replay/calibration evidence payloads (v23)
    // ═══════════════════════════════════════════
    /// Complete Gatekeeper V2/V2.5 config that was active for this decision row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_v2_config_payload: Option<serde_json::Value>,
    /// Complete Gatekeeper V3 config payload carried even when V3 replay payloads
    /// are disabled, so every row records the sidecar policy context for the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_v3_config_payload: Option<serde_json::Value>,
    /// Canonical decision-time feature snapshot used by Gatekeeper evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_feature_snapshot: Option<serde_json::Value>,
    /// Structured terminal decision object, preserving fields that are otherwise
    /// spread across flat compatibility columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gatekeeper_decision_payload: Option<serde_json::Value>,
    /// Complete list of V2.5 shadow checkpoint decisions, not only the terminal
    /// convenience aliases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v25_shadow_decisions_payload: Option<serde_json::Value>,

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
    /// Optional run identifier stamped onto gatekeeper decision records.
    pub gatekeeper_run_id: Option<String>,
    /// Optional session identifier stamped onto gatekeeper decision records.
    pub gatekeeper_session_id: Option<String>,
    /// Optional Ghost Brain config path stamped onto gatekeeper decision records.
    pub brain_config_path: Option<String>,
    /// Optional Ghost Brain config file hash stamped onto gatekeeper decision records.
    pub brain_config_hash: Option<String>,
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
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
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
    WriteCoordinationRiskEvidence(CoordinationRiskEvidenceUnit),
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

                            if plane_log.log_schema_version >= 19 && plane_log.reason_code.is_none()
                            {
                                warn!(
                                    "GK_REASON_CODE_MISSING: dropping plane row pool={} plane={} verdict_type={}",
                                    plane_log.pool_id,
                                    plane_log
                                        .decision_plane
                                        .as_deref()
                                        .unwrap_or("unknown"),
                                    plane_log.verdict_type.as_deref().unwrap_or("unknown"),
                                );
                                continue;
                            }

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
                                if let Err(e) = write_selector_shadow_score_sidecar(
                                    &config.gatekeeper_log_dir,
                                    &plane_log,
                                )
                                .await
                                {
                                    if is_no_space_error(e.chain()) {
                                        logging_disabled_due_to_enospc = true;
                                        error!(
                                            "DecisionLogger: disabling file writes after ENOSPC on selector shadow-score sidecar for {} plane={}",
                                            plane_log.pool_id,
                                            plane_log
                                                .decision_plane
                                                .as_deref()
                                                .unwrap_or("unknown")
                                        );
                                        continue;
                                    }
                                    error!(
                                        "Failed to write selector shadow-score sidecar for {} plane={}: {}",
                                        plane_log.pool_id,
                                        plane_log
                                            .decision_plane
                                            .as_deref()
                                            .unwrap_or("unknown"),
                                        e
                                    );
                                }
                            }
                        }
                    }
                    LogCommand::WriteCoordinationRiskEvidence(mut unit) => {
                        if logging_disabled_due_to_enospc {
                            continue;
                        }
                        hydrate_coordination_risk_routing_fields(&mut unit, &config);
                        if let Err(e) =
                            write_coordination_risk_evidence_unit(&config.gatekeeper_log_dir, &unit)
                                .await
                        {
                            if is_no_space_error(e.chain()) {
                                logging_disabled_due_to_enospc = true;
                                error!(
                                    "DecisionLogger: disabling file writes after ENOSPC on coordination-risk sidecar for {}",
                                    unit.pool_id
                                );
                                continue;
                            }
                            error!(
                                "Failed to write coordination-risk evidence sidecar for {}: {}",
                                unit.pool_id, e
                            );
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

    /// Log additive Phase 0.6 coordination-risk evidence.
    ///
    /// This sidecar is export-only. It is routed independently from the
    /// decision JSONL and must not mutate Gatekeeper verdict payloads.
    pub async fn log_coordination_risk_evidence(&self, unit: CoordinationRiskEvidenceUnit) {
        if let Err(e) = self
            .tx
            .send(LogCommand::WriteCoordinationRiskEvidence(unit))
            .await
        {
            warn!(
                "Failed to send coordination-risk evidence log command: {}",
                e
            );
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
    if log.run_id.is_none() {
        log.run_id = config.gatekeeper_run_id.clone();
    }
    if log.session_id.is_none() {
        log.session_id = config.gatekeeper_session_id.clone();
    }
    if log.brain_config_path.is_none() {
        log.brain_config_path = config.brain_config_path.clone();
    }
    if log.brain_config_hash.is_none() {
        log.brain_config_hash = config.brain_config_hash.clone();
    }
}

fn hydrate_coordination_risk_routing_fields(
    unit: &mut CoordinationRiskEvidenceUnit,
    config: &DecisionLoggerConfig,
) {
    if unit.scope_id.trim().is_empty() {
        unit.scope_id = config.gatekeeper_rollout_profile.clone();
    }
    if unit.run_id.is_none() {
        unit.run_id = config.gatekeeper_run_id.clone();
    }
    if unit.gatekeeper_version.is_none() {
        unit.gatekeeper_version = Some(GATEKEEPER_VERSION.to_string());
    }
}

fn gatekeeper_buy_alias_from_verdict(verdict_type: Option<&str>) -> Option<bool> {
    let verdict = verdict_type?.trim();
    if matches!(
        verdict,
        "BUY" | "EARLY_BUY" | "BUY_NORMAL" | "BUY_EARLY" | "BUY_EXTENDED"
    ) {
        Some(true)
    } else if verdict.starts_with("TIMEOUT") {
        None
    } else if verdict.starts_with("REJECT") {
        Some(false)
    } else {
        None
    }
}

fn has_v25_shadow_cached_assessment_evidence(log: &GatekeeperBuyLog) -> bool {
    log.v25_shadow_confidence.is_some() || log.v25_shadow_observation_stage.is_some()
}

fn v25_shadow_verdict_type_or_terminal_fallback(log: &GatekeeperBuyLog) -> Option<String> {
    log.v25_shadow_verdict_type.clone().or_else(|| {
        if !has_v25_shadow_cached_assessment_evidence(log) {
            return None;
        }
        log.verdict_type.as_deref().and_then(|verdict| {
            let verdict = verdict.trim();
            verdict.starts_with("TIMEOUT").then(|| verdict.to_string())
        })
    })
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
        legacy.verdict_type = legacy_verdict_type;
        legacy.reason_code = log.reason_code.clone();
        expanded.push(legacy);
    }

    if has_shadow_plane {
        let mut shadow = log.clone();
        let used_terminal_timeout_fallback = shadow.v25_shadow_verdict_type.is_none()
            && has_v25_shadow_cached_assessment_evidence(&shadow)
            && shadow
                .verdict_type
                .as_deref()
                .map(|verdict| verdict.trim().starts_with("TIMEOUT"))
                .unwrap_or(false);
        let shadow_verdict_type = v25_shadow_verdict_type_or_terminal_fallback(&shadow);
        shadow.gatekeeper_version = Some(GATEKEEPER_VERSION.to_string());
        shadow.decision_plane = Some(DECISION_PLANE_V25_SHADOW.to_string());
        shadow.decision_reason = shadow
            .v25_shadow_reason_chain
            .clone()
            .or_else(|| shadow.decision_reason.clone());
        shadow.decision_verdict_buy =
            gatekeeper_buy_alias_from_verdict(shadow_verdict_type.as_deref());
        shadow.verdict_type = shadow_verdict_type;
        let shadow_reason_code = shadow
            .verdict_type
            .as_deref()
            .and_then(crate::oracle::reason_code::GatekeeperReasonCode::from_log_str)
            .map(crate::oracle::reason_code::GatekeeperReasonCode::as_log_str);
        shadow.reason_code = if shadow_reason_code.is_none() && used_terminal_timeout_fallback {
            shadow.reason_code.clone()
        } else {
            shadow_reason_code
        };
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

fn selector_shadow_bool_value(value: Option<bool>) -> Option<f64> {
    value.map(|flag| if flag { 1.0 } else { 0.0 })
}

fn selector_shadow_vector_event_count(log: &GatekeeperBuyLog) -> Option<f64> {
    let count = [
        log.vectors_prices.as_ref().map(Vec::len),
        log.vectors_sol_amounts.as_ref().map(Vec::len),
        log.vectors_ts_offsets_ms.as_ref().map(Vec::len),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);
    if count > 0 {
        Some(count as f64)
    } else {
        None
    }
}

fn selector_shadow_price_return(prices: &[f64]) -> Option<f64> {
    let first = *prices.first()?;
    if first == 0.0 {
        return None;
    }
    let last = *prices.last()?;
    Some((last / first) - 1.0)
}

fn selector_shadow_price_drawdown(prices: &[f64]) -> Option<f64> {
    let mut iter = prices.iter().copied();
    let mut peak = iter.next()?;
    let mut max_drawdown = 0.0;
    for price in std::iter::once(peak).chain(iter) {
        if price > peak {
            peak = price;
        }
        if peak > 0.0 {
            let drawdown = (peak - price) / peak;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }
    Some(max_drawdown)
}

fn selector_shadow_intervals_from_offsets(offsets: &[i64]) -> Vec<f64> {
    if offsets.len() < 2 {
        return Vec::new();
    }
    offsets
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).max(0) as f64)
        .collect()
}

fn selector_shadow_median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        Some((sorted[mid - 1] + sorted[mid]) / 2.0)
    } else {
        Some(sorted[mid])
    }
}

fn selector_shadow_runtime_feature_value(log: &GatekeeperBuyLog, feature: &str) -> Option<f64> {
    let value = match feature {
        "gk_curve_wait_elapsed_ms" => log.curve_wait_elapsed_ms.map(|value| value as f64),
        "gk_bonding_progress_pct" => log.bonding_progress_pct,
        "gk_current_market_cap_sol" => log.current_market_cap_sol,
        "gk_price_change_ratio" => log.price_change_ratio,
        "gk_max_single_tx_price_impact_pct_observed" => log.max_single_tx_price_impact_pct_observed,
        "gk_max_single_sell_impact_pct_observed" => log.max_single_sell_impact_pct_observed,
        "gk_total_tx_evaluated" => Some(log.total_tx_evaluated as f64),
        "gk_unique_tx_evaluated" => log.unique_tx_evaluated.map(|value| value as f64),
        "gk_unique_signers_evaluated" => Some(log.unique_signers_evaluated as f64),
        "gk_buy_count" => Some(log.buy_count as f64),
        "gk_buy_ratio" => log.buy_ratio,
        "gk_sell_buy_ratio" => log.sell_buy_ratio,
        "gk_sol_buy_ratio" => log.sol_buy_ratio,
        "gk_total_volume_sol" => log.total_volume_sol,
        "gk_avg_tx_sol" => log.avg_tx_sol,
        "gk_volume_cv" => log.volume_cv,
        "gk_volume_gini" => log.volume_gini,
        "gk_hhi" => log.hhi,
        "gk_top3_volume_pct" => log.top3_volume_pct,
        "gk_same_ms_tx_ratio" => log.same_ms_tx_ratio,
        "gk_max_consecutive_buys_observed" => {
            log.max_consecutive_buys_observed.map(|value| value as f64)
        }
        "gk_dev_buy_total_sol" => log.dev_buy_total_sol,
        "gk_dev_tx_ratio" => log.dev_tx_ratio,
        "gk_dev_volume_ratio" => log.dev_volume_ratio,
        "gk_dev_has_sold" => selector_shadow_bool_value(log.dev_has_sold),
        "gk_dev_sold_within_3s" => selector_shadow_bool_value(log.dev_sold_within_3s),
        "gk_dev_sold_within_5s" => selector_shadow_bool_value(log.dev_sold_within_5s),
        "gk_block0_sniped_supply_pct" => log.block0_sniped_supply_pct,
        "gk_flip_ratio_10s" => log.flip_ratio_10s,
        "gk_buyer_pre_balance_cv" => log.buyer_pre_balance_cv,
        "gk_cu_price_p90_1s" => log.cu_price_p90_1s,
        "gk_cu_price_p90_10s" => log.cu_price_p90_10s,
        "gk_priority_fee_surge_slope" => log.priority_fee_surge_slope,
        "gk_fee_topology_diversity_index" => log.fee_topology_diversity_index,
        "gk_dev_buyer_infrastructure_affinity" => log.dev_buyer_infrastructure_affinity,
        "gk_spend_fraction_divergence" => log.spend_fraction_divergence,
        "gk_demand_elasticity_score" => log.demand_elasticity_score,
        "gk_signer_cross_pool_velocity" => log.signer_cross_pool_velocity,
        "gk_compute_unit_cluster_dominance" => log.compute_unit_cluster_dominance,
        "gk_static_fee_profile_ratio" => log.static_fee_profile_ratio,
        "gk_fixed_size_buy_ratio" => log.fixed_size_buy_ratio,
        "gk_flipper_presence_ratio" => log.flipper_presence_ratio,
        "gk_jito_tip_intensity" => log.jito_tip_intensity,
        "gk_early_slot_volume_dominance_buy" => log.early_slot_volume_dominance_buy,
        "gk_early_top3_buy_volume_pct_3s" => log.early_top3_buy_volume_pct_3s,
        "gk_whale_reversal_ratio_top3" => log.whale_reversal_ratio_top3,
        "gk_whale_reversal_ratio_top1" => log.whale_reversal_ratio_top1,
        "gk_iwim_confidence" => log.iwim_confidence.map(f64::from),
        "gk_iwim_sybil_score" => log.iwim_sybil_score.map(f64::from),
        "gk_iwim_organic_score" => log.iwim_organic_score.map(f64::from),
        "gk_fsc_buyer_sample_count" => log
            .funding_source_v2
            .as_ref()
            .map(|evidence| f64::from(evidence.total_buyers)),
        "gk_vector_event_count" => selector_shadow_vector_event_count(log),
        "gk_vector_price_first" => log
            .vectors_prices
            .as_ref()
            .and_then(|prices| prices.first().copied()),
        "gk_vector_price_last" => log
            .vectors_prices
            .as_ref()
            .and_then(|prices| prices.last().copied()),
        "gk_vector_price_return" => log
            .vectors_prices
            .as_ref()
            .and_then(|prices| selector_shadow_price_return(prices)),
        "gk_vector_price_max" => log
            .vectors_prices
            .as_ref()
            .and_then(|prices| prices.iter().copied().reduce(f64::max)),
        "gk_vector_price_min" => log
            .vectors_prices
            .as_ref()
            .and_then(|prices| prices.iter().copied().reduce(f64::min)),
        "gk_vector_price_drawdown" => log
            .vectors_prices
            .as_ref()
            .and_then(|prices| selector_shadow_price_drawdown(prices)),
        "gk_vector_sol_sum" => log
            .vectors_sol_amounts
            .as_ref()
            .map(|amounts| amounts.iter().sum()),
        "gk_vector_sol_max" => log
            .vectors_sol_amounts
            .as_ref()
            .and_then(|amounts| amounts.iter().copied().reduce(f64::max)),
        "gk_vector_interval_median" => log.vectors_ts_offsets_ms.as_ref().and_then(|offsets| {
            selector_shadow_median(&selector_shadow_intervals_from_offsets(offsets))
        }),
        "gk_vector_interval_min" => log.vectors_ts_offsets_ms.as_ref().and_then(|offsets| {
            selector_shadow_intervals_from_offsets(offsets)
                .iter()
                .copied()
                .reduce(f64::min)
        }),
        "gk_vector_interval_max" => log.vectors_ts_offsets_ms.as_ref().and_then(|offsets| {
            selector_shadow_intervals_from_offsets(offsets)
                .iter()
                .copied()
                .reduce(f64::max)
        }),
        _ => None,
    };
    value.filter(|value| value.is_finite())
}

fn selector_shadow_normalized_feature(value: Option<f64>, spec: &SelectorShadowFeatureSpec) -> f64 {
    let Some(value) = value else {
        return 0.0;
    };
    if !value.is_finite() {
        return 0.0;
    }
    let denom = spec.max - spec.min;
    if denom.abs() <= f64::EPSILON {
        return 0.0;
    }
    let mut normalized = (value - spec.min) / denom;
    if spec.direction < 0.0 {
        normalized = 1.0 - normalized;
    }
    normalized.clamp(0.0, 1.0)
}

fn build_selector_shadow_score_sidecar(log: &GatekeeperBuyLog) -> SelectorShadowScoreSidecarLog {
    let cutoff_verified = log.observation_end_ts_ms.is_some();
    let core_curve_market_available = log.bonding_progress_pct.is_some()
        && log.current_market_cap_sol.is_some()
        && log.price_change_ratio.is_some()
        && log.curve_data_known.unwrap_or(false);
    let concentration_available = log.hhi.is_some() && log.top3_volume_pct.is_some();

    let mut missing = Vec::new();
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let mut missing_runtime_mapping_count = 0usize;
    let mut mapped_feature_count = 0usize;
    let mut normalized_sum = 0.0;

    for spec in SELECTOR_SHADOW_FEATURE_SPECS {
        let value = match spec.source {
            SelectorShadowRuntimeFeatureSource::Mapped => {
                let value = selector_shadow_runtime_feature_value(log, spec.name);
                if value.is_some() {
                    mapped_feature_count += 1;
                }
                value
            }
            SelectorShadowRuntimeFeatureSource::MissingRuntimeMapping => {
                missing_runtime_mapping_count += 1;
                None
            }
        };
        if value.is_none() {
            missing.push(spec.name.to_string());
        }
        let normalized = selector_shadow_normalized_feature(value, spec);
        normalized_sum += normalized;
        if value.is_some() {
            if normalized >= 0.75 {
                if spec.direction >= 0.0 {
                    positive.push(format!("high_{}", spec.name));
                } else {
                    positive.push(format!("low_{}", spec.name));
                }
            } else if normalized <= 0.25 {
                if spec.direction >= 0.0 {
                    negative.push(format!("low_{}", spec.name));
                } else {
                    negative.push(format!("high_{}", spec.name));
                }
            }
        }
    }

    let selector_shadow_score = Some(normalized_sum / SELECTOR_SHADOW_FEATURE_SPECS.len() as f64);
    let required_missing = [
        "gk_bonding_progress_pct",
        "gk_current_market_cap_sol",
        "gk_price_change_ratio",
    ]
    .into_iter()
    .filter(|feature| selector_shadow_runtime_feature_value(log, feature).is_none())
    .count()
        + usize::from(!log.curve_data_known.unwrap_or(false));
    let gk_context_available = mapped_feature_count > 0 && cutoff_verified;
    let flow_available = SELECTOR_SHADOW_FEATURE_SPECS.iter().any(|spec| {
        !spec.name.starts_with("gk_")
            && spec.source == SelectorShadowRuntimeFeatureSource::Mapped
            && selector_shadow_runtime_feature_value(log, spec.name).is_some()
    });
    let score_validity_status = if !cutoff_verified {
        SELECTOR_SHADOW_SCORE_INVALID_CUTOFF_UNVERIFIED
    } else if required_missing > 0 || !core_curve_market_available {
        SELECTOR_SHADOW_SCORE_INVALID_MISSING_CORE
    } else if !concentration_available {
        SELECTOR_SHADOW_SCORE_DEGRADED_MISSING_CONCENTRATION
    } else {
        SELECTOR_SHADOW_SCORE_VALID
    }
    .to_string();
    let score_valid = score_validity_status == SELECTOR_SHADOW_SCORE_VALID;
    let score_degraded = score_validity_status.starts_with("score_degraded");

    if !concentration_available {
        if log.hhi.is_none() {
            negative.insert(0, "gk_concentration_missing".to_string());
        }
        if log.top3_volume_pct.is_none() {
            negative.insert(0, "gk_top3_volume_pct_missing".to_string());
        }
    }
    if !gk_context_available {
        negative.insert(0, "gk_context_missing_or_unverified".to_string());
    }

    positive.truncate(12);
    negative.truncate(12);
    missing.truncate(40);

    let threshold_score = selector_shadow_score.unwrap_or(0.0);

    SelectorShadowScoreSidecarLog {
        schema_version: SELECTOR_SHADOW_SCORE_SCHEMA_VERSION.to_string(),
        score_version: SELECTOR_SHADOW_SCORE_VERSION.to_string(),
        score_candidate_id: SELECTOR_SHADOW_SCORE_CANDIDATE_ID.to_string(),
        scope: log
            .rollout_profile
            .clone()
            .unwrap_or_else(|| "unknown_rollout".to_string()),
        decision_plane: log
            .decision_plane
            .clone()
            .unwrap_or_else(|| "unknown_decision_plane".to_string()),
        candidate_id: log
            .execution_candidate_id
            .clone()
            .or_else(|| log.join_key.clone())
            .unwrap_or_else(|| log.pool_id.clone()),
        pool_id: log.pool_id.clone(),
        base_mint: log.base_mint.clone(),
        gatekeeper_verdict_type: log.verdict_type.clone(),
        decision_ts_ms: log
            .observation_end_ts_ms
            .or(log.end_10s_ts_ms)
            .or(log.first_seen_ts_ms),
        feature_cutoff_ts_ms: log.observation_end_ts_ms,
        selector_shadow_score,
        score_validity_status,
        score_valid,
        score_degraded,
        feature_missing_count: SELECTOR_SHADOW_FEATURE_SPECS.len() - mapped_feature_count,
        required_feature_missing_count: required_missing,
        thresholds: SelectorShadowScoreThresholds {
            top10_equiv_pass: threshold_score >= SELECTOR_SHADOW_TOP10_EQUIV_THRESHOLD,
            top25_equiv_pass: threshold_score >= SELECTOR_SHADOW_TOP25_EQUIV_THRESHOLD,
            q99_pass: threshold_score >= SELECTOR_SHADOW_Q99_THRESHOLD,
            q98_pass: threshold_score >= SELECTOR_SHADOW_Q98_THRESHOLD,
            q975_pass: threshold_score >= SELECTOR_SHADOW_Q975_THRESHOLD,
            target_precision_0_70_pass: threshold_score
                >= SELECTOR_SHADOW_TARGET_PRECISION_0_70_THRESHOLD,
        },
        reason_vector: SelectorShadowScoreReasonVector {
            positive,
            negative,
            missing,
        },
        feature_availability: SelectorShadowScoreFeatureAvailability {
            core_curve_market_available,
            concentration_available,
            gk_context_available,
            flow_available,
            runtime_score_adapter_available: true,
            feature_mapping_status: if missing_runtime_mapping_count == 0 {
                "complete_runtime_mapping".to_string()
            } else {
                "partial_runtime_mapping_missing_flow_features".to_string()
            },
            mapped_feature_count,
            missing_runtime_mapping_count,
            cutoff_verified,
        },
        claim_boundaries: SelectorShadowScoreClaimBoundaries {
            diagnostic_only: true,
            shadow_only: true,
            production_promotion_allowed: false,
            gatekeeper_tuning_started: false,
            changes_gatekeeper_decision: false,
            changes_execution: false,
            send_path_changed: false,
        },
    }
}

fn coordination_risk_route_dir(base_dir: &Path, unit: &CoordinationRiskEvidenceUnit) -> PathBuf {
    let scope_id = safe_log_path_component(unit.scope_id.as_str(), "unknown_scope");
    let gatekeeper_version = safe_log_path_component(
        unit.gatekeeper_version
            .as_deref()
            .unwrap_or("unknown_gatekeeper_version"),
        "unknown_gatekeeper_version",
    );

    base_dir
        .join(scope_id)
        .join(gatekeeper_version)
        .join("coordination_risk")
}

fn safe_log_path_component(value: &str, fallback: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim_matches('_');
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized.to_string()
    }
}

fn v3_serialized_replay_payload_hash(
    log_value: &serde_json::Value,
    materialization_version: u32,
) -> Option<String> {
    let mut canonical_snapshot = log_value.get("v3_materialized_feature_snapshot")?.clone();
    if let Some(session_metadata) = canonical_snapshot
        .get_mut("session_metadata")
        .and_then(serde_json::Value::as_object_mut)
    {
        // Session id is run-local identity, not replay evidence.
        session_metadata.remove("session_id");
    }

    let payload = serde_json::json!({
        "materialization_version": materialization_version,
        "v3_materialized_feature_snapshot": canonical_snapshot
    });
    let bytes = serde_json::to_vec(&payload).ok()?;
    Some(blake3::hash(&bytes).to_hex().to_string())
}

fn v3_serialized_replay_payload_hash_mismatch(log: &GatekeeperBuyLog) -> Option<(String, String)> {
    let logged_hash = log.v3_feature_snapshot_hash.as_ref()?;
    let materialization_version = log.v3_materialization_version?;
    let log_value = serde_json::to_value(log).ok()?;
    let serialized_payload_hash =
        v3_serialized_replay_payload_hash(&log_value, materialization_version)?;

    if logged_hash == &serialized_payload_hash {
        None
    } else {
        Some((logged_hash.clone(), serialized_payload_hash))
    }
}

fn v3_post_serialize_replay_payload_hash_mismatch(
    log: &GatekeeperBuyLog,
    json: &str,
) -> Option<(String, String)> {
    let logged_hash = log.v3_feature_snapshot_hash.as_ref()?;
    let materialization_version = log.v3_materialization_version?;
    let json_value = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let post_serialize_hash =
        v3_serialized_replay_payload_hash(&json_value, materialization_version)?;

    if logged_hash == &post_serialize_hash {
        None
    } else {
        Some((logged_hash.clone(), post_serialize_hash))
    }
}

fn v3_replay_stable_gatekeeper_buy_log_json(log: &GatekeeperBuyLog) -> Result<String> {
    let json = serde_json::to_string(log).context("Failed to serialize gatekeeper buy log")?;
    let Some((logged_hash, post_serialize_hash)) =
        v3_post_serialize_replay_payload_hash_mismatch(log, &json)
    else {
        return Ok(json);
    };

    let mut patched = log.clone();
    patched.v3_feature_snapshot_hash = Some(post_serialize_hash.clone());
    let patched_json =
        serde_json::to_string(&patched).context("Failed to serialize gatekeeper buy log")?;

    if let Some((patched_hash, patched_post_serialize_hash)) =
        v3_post_serialize_replay_payload_hash_mismatch(&patched, &patched_json)
    {
        warn!(
            logged_hash = %patched_hash,
            post_serialize_hash = %patched_post_serialize_hash,
            materialization_version = ?patched.v3_materialization_version,
            replay_payload_schema_version = ?patched.v3_replay_payload_schema_version,
            pool = %patched.pool_id,
            plane = ?patched.decision_plane,
            "V3 replay payload hash post-serialize mismatch"
        );
    } else {
        debug!(
            previous_hash = %logged_hash,
            canonical_hash = %post_serialize_hash,
            materialization_version = ?patched.v3_materialization_version,
            replay_payload_schema_version = ?patched.v3_replay_payload_schema_version,
            pool = %patched.pool_id,
            plane = ?patched.decision_plane,
            "V3 replay payload hash canonicalized at post-serialize boundary"
        );
    }

    Ok(patched_json)
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

/// Write an additive coordination-risk evidence sidecar.
///
/// The sidecar is intentionally separate from Gatekeeper decision rows so Phase
/// 0.6 cannot change `GatekeeperBuyLog`, `MaterializedFeatureSet`, verdicts, or
/// replay payload hashes. Routing mirrors the rollout/gatekeeper-version prefix
/// and then writes a dedicated coordination-risk JSONL file.
async fn write_coordination_risk_evidence_unit(
    base_dir: &Path,
    unit: &CoordinationRiskEvidenceUnit,
) -> Result<()> {
    let routed_dir = coordination_risk_route_dir(base_dir, unit);
    create_dir_all(&routed_dir)
        .await
        .context("Failed to create coordination-risk evidence log directory")?;

    let path = routed_dir.join(COORDINATION_RISK_EVIDENCE_JSONL);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .context("Failed to open coordination-risk evidence log file")?;

    let json =
        serde_json::to_string(unit).context("Failed to serialize coordination-risk evidence")?;
    file.write_all(json.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;

    debug!(
        "Coordination-risk evidence sidecar written for {} to {:?}",
        unit.pool_id, path
    );

    Ok(())
}

/// Write an additive selector shadow-score sidecar.
///
/// P3L-A keeps this as a dedicated JSONL file next to routed Gatekeeper
/// decisions. The sidecar is diagnostic-only and deliberately omitted from
/// `GatekeeperBuyLog` so replay payloads, verdicts, and execution eligibility
/// remain unchanged.
async fn write_selector_shadow_score_sidecar(
    base_dir: &Path,
    log: &GatekeeperBuyLog,
) -> Result<()> {
    let routed_dir = gatekeeper_route_dir(base_dir, log);
    create_dir_all(&routed_dir)
        .await
        .context("Failed to create selector shadow-score sidecar directory")?;

    let path = routed_dir.join(SELECTOR_SHADOW_SCORE_JSONL);
    let sidecar = build_selector_shadow_score_sidecar(log);
    let json =
        serde_json::to_string(&sidecar).context("Failed to serialize selector shadow-score")?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .context("Failed to open selector shadow-score sidecar file")?;
    file.write_all(json.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;

    debug!(
        "Selector shadow-score sidecar written for {} to {:?}",
        log.pool_id, path
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

    if let Some((logged_hash, serialized_payload_hash)) =
        v3_serialized_replay_payload_hash_mismatch(log)
    {
        warn!(
            logged_hash = %logged_hash,
            serialized_payload_hash = %serialized_payload_hash,
            materialization_version = ?log.v3_materialization_version,
            replay_payload_schema_version = ?log.v3_replay_payload_schema_version,
            pool = %log.pool_id,
            plane = ?log.decision_plane,
            "V3 replay payload hash logger-boundary mismatch"
        );
    }

    // Serialize to JSON once (reused for both files).
    let json = v3_replay_stable_gatekeeper_buy_log_json(log)?;

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
    use crate::oracle::reason_code::GatekeeperReasonCode;
    use ghost_core::features::coordination::{
        CoordinationMetricBreakdowns, CoordinationRiskFeatures, CoordinationSnapshotMode,
        FundingVisibility,
    };
    use solana_sdk::pubkey::Pubkey;
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
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
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
            run_id: None,
            session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
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
            decision_reason: Some("test buy".to_string()),
            decision_verdict_buy: Some(true),
            verdict_type: Some("BUY".to_string()),
            legacy_live_reason_chain: Some("test buy".to_string()),
            legacy_live_verdict_buy: Some(true),
            legacy_live_verdict_type: Some("BUY".to_string()),
            v25_shadow_verdict_type: None,
            v25_shadow_reason_chain: None,
            v25_shadow_confidence: None,
            v25_shadow_confidence_source: None,
            v25_shadow_observation_stage: None,
            v25_promotion_state: Some("shadow_only".to_string()),
            v3_shadow_schema_version: None,
            v3_shadow_verdict: None,
            v3_shadow_stage: None,
            v3_shadow_reason_code: None,
            v3_shadow_reason_chain: None,
            v3_shadow_secondary_reason_codes: None,
            v3_shadow_risk_status: None,
            v3_shadow_risk_primary_reason: None,
            v3_shadow_risk_penalty: None,
            v3_shadow_opportunity_status: None,
            v3_shadow_opportunity_score: None,
            v3_shadow_confidence_raw: None,
            v3_shadow_confidence_after_risk: None,
            v3_shadow_confidence_after_stage: None,
            v3_shadow_confidence_cap: None,
            v3_shadow_confidence_cap_reasons: None,
            v3_shadow_confidence_final: None,
            v3_shadow_confidence: None,
            v3_shadow_evidence_status: None,
            v3_shadow_organic_broadening: None,
            v3_shadow_manipulation_contradictions: None,
            v3_evidence_status: None,
            v3_organic_broadening: None,
            v3_manipulation_contradictions: None,
            v3_policy_config_hash: None,
            v3_feature_snapshot_hash: None,
            v3_replay_payload_schema_version: None,
            v3_materialized_feature_snapshot: None,
            v3_policy_config_payload: None,
            v3_materialization_version: None,
            v3_policy_version: None,
            v3_stage_thresholds: None,
            v3_component_scores: None,
            v3_actionability: None,
            v3_shadow_notes: None,
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
            funding_source_v2: None,
            shadow_fsc_v2_policy_signal: None,
            shadow_fsc_v2_soft_points_if_enabled: None,
            shadow_fsc_v2_reason_if_enabled: None,
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
            pdd_entry_drift_elapsed_ms: None,
            pdd_entry_drift_anchor_price: None,
            pdd_entry_drift_current_price: None,
            pdd_entry_drift_anchor_ts_ms: None,
            pdd_entry_drift_current_ts_ms: None,
            pdd_entry_drift_static_max_pct: None,
            pdd_entry_drift_elapsed_max_pct: None,
            pdd_entry_drift_effective_max_pct: None,
            pdd_entry_drift_threshold_source: None,
            pdd_entry_drift_anchor_source: None,
            pdd_entry_drift_anchor_quality: None,
            pdd_spike_detected: None,
            pdd_spike_ratio: None,
            pdd_spike_ratio_quality: None,
            pdd_spike_recent_rate: None,
            pdd_spike_earlier_rate: None,
            pdd_ramping_detected: None,
            pdd_whale_top3_pct: None,
            pdd_whale_single_max_pct: None,
            pdd_flash_crash_risk: None,
            pdd_score: None,
            gatekeeper_first_kill_gate: None,
            gatekeeper_first_kill_reason: None,
            gatekeeper_terminal_gate: None,
            gatekeeper_gate_trace: vec![],
            gatekeeper_v2_replay_input_schema_version: None,
            gatekeeper_v2_replay_ready_non_temporal: None,
            gatekeeper_v2_replay_ready_temporal: None,
            gatekeeper_v2_replay_missing_fields: vec![],
            gatekeeper_v2_phase_pass_vector: None,
            hard_reject_reason: None,
            pdd_soft_penalty_points: None,
            pdd_hard_fail_evaluated: None,
            hard_fail_hhi_threshold: None,
            observed_mode: None,
            observed_window_ms: None,
            observed_stage: None,
            decision_eval_snapshots: None,
            gatekeeper_v2_config_payload: None,
            gatekeeper_v3_config_payload: None,
            materialized_feature_snapshot: None,
            gatekeeper_decision_payload: None,
            v25_shadow_decisions_payload: None,
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
            reason_code: Some(GatekeeperReasonCode::BuyNormal.as_log_str()),
            reason_code_version: GatekeeperReasonCode::version(),
            pdd_sequence_signals_unavailable_reason: None,
        };

        // Initialize the logger
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
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
            run_id: None,
            session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
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
            decision_reason: Some("test reject".to_string()),
            decision_verdict_buy: Some(false),
            verdict_type: Some("REJECT_CORE_FAIL".to_string()),
            legacy_live_reason_chain: None,
            legacy_live_verdict_buy: None,
            legacy_live_verdict_type: None,
            v25_shadow_verdict_type: None,
            v25_shadow_reason_chain: None,
            v25_shadow_confidence: None,
            v25_shadow_confidence_source: None,
            v25_shadow_observation_stage: None,
            v25_promotion_state: Some("shadow_only".to_string()),
            v3_shadow_schema_version: None,
            v3_shadow_verdict: None,
            v3_shadow_stage: None,
            v3_shadow_reason_code: None,
            v3_shadow_reason_chain: None,
            v3_shadow_secondary_reason_codes: None,
            v3_shadow_risk_status: None,
            v3_shadow_risk_primary_reason: None,
            v3_shadow_risk_penalty: None,
            v3_shadow_opportunity_status: None,
            v3_shadow_opportunity_score: None,
            v3_shadow_confidence_raw: None,
            v3_shadow_confidence_after_risk: None,
            v3_shadow_confidence_after_stage: None,
            v3_shadow_confidence_cap: None,
            v3_shadow_confidence_cap_reasons: None,
            v3_shadow_confidence_final: None,
            v3_shadow_confidence: None,
            v3_shadow_evidence_status: None,
            v3_shadow_organic_broadening: None,
            v3_shadow_manipulation_contradictions: None,
            v3_evidence_status: None,
            v3_organic_broadening: None,
            v3_manipulation_contradictions: None,
            v3_policy_config_hash: None,
            v3_feature_snapshot_hash: None,
            v3_replay_payload_schema_version: None,
            v3_materialized_feature_snapshot: None,
            v3_policy_config_payload: None,
            v3_materialization_version: None,
            v3_policy_version: None,
            v3_stage_thresholds: None,
            v3_component_scores: None,
            v3_actionability: None,
            v3_shadow_notes: None,
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
            funding_source_v2: None,
            shadow_fsc_v2_policy_signal: None,
            shadow_fsc_v2_soft_points_if_enabled: None,
            shadow_fsc_v2_reason_if_enabled: None,
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
            pdd_entry_drift_elapsed_ms: None,
            pdd_entry_drift_anchor_price: None,
            pdd_entry_drift_current_price: None,
            pdd_entry_drift_anchor_ts_ms: None,
            pdd_entry_drift_current_ts_ms: None,
            pdd_entry_drift_static_max_pct: None,
            pdd_entry_drift_elapsed_max_pct: None,
            pdd_entry_drift_effective_max_pct: None,
            pdd_entry_drift_threshold_source: None,
            pdd_entry_drift_anchor_source: None,
            pdd_entry_drift_anchor_quality: None,
            pdd_spike_detected: None,
            pdd_spike_ratio: None,
            pdd_spike_ratio_quality: None,
            pdd_spike_recent_rate: None,
            pdd_spike_earlier_rate: None,
            pdd_ramping_detected: None,
            pdd_whale_top3_pct: None,
            pdd_whale_single_max_pct: None,
            pdd_flash_crash_risk: None,
            pdd_score: None,
            gatekeeper_first_kill_gate: None,
            gatekeeper_first_kill_reason: None,
            gatekeeper_terminal_gate: None,
            gatekeeper_gate_trace: vec![],
            gatekeeper_v2_replay_input_schema_version: None,
            gatekeeper_v2_replay_ready_non_temporal: None,
            gatekeeper_v2_replay_ready_temporal: None,
            gatekeeper_v2_replay_missing_fields: vec![],
            gatekeeper_v2_phase_pass_vector: None,
            hard_reject_reason: None,
            pdd_soft_penalty_points: None,
            pdd_hard_fail_evaluated: None,
            hard_fail_hhi_threshold: None,
            observed_mode: None,
            observed_window_ms: None,
            observed_stage: None,
            decision_eval_snapshots: None,
            gatekeeper_v2_config_payload: None,
            gatekeeper_v3_config_payload: None,
            materialized_feature_snapshot: None,
            gatekeeper_decision_payload: None,
            v25_shadow_decisions_payload: None,
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
            reason_code: Some(GatekeeperReasonCode::RejectCoreFail.as_log_str()),
            reason_code_version: GatekeeperReasonCode::version(),
            pdd_sequence_signals_unavailable_reason: None,
        }
    }

    #[test]
    fn test_v3_replay_payload_fields_serialize_only_when_some() {
        let mut log = create_test_buy_log();
        let record = serde_json::to_value(&log).unwrap();
        assert!(record.get("v3_replay_payload_schema_version").is_none());
        assert!(record.get("v3_materialized_feature_snapshot").is_none());
        assert!(record.get("v3_policy_config_payload").is_none());

        log.v3_replay_payload_schema_version = Some(1);
        log.v3_materialized_feature_snapshot = Some(serde_json::json!({
            "materialization_version": 1,
            "pool_id": "pool_payload"
        }));
        log.v3_policy_config_payload = Some(serde_json::json!({
            "policy_version": 1
        }));

        let record = serde_json::to_value(&log).unwrap();
        assert_eq!(record["v3_replay_payload_schema_version"], 1);
        assert_eq!(
            record["v3_materialized_feature_snapshot"]["pool_id"],
            "pool_payload"
        );
        assert_eq!(record["v3_policy_config_payload"]["policy_version"], 1);
    }

    #[test]
    fn test_gatekeeper_v2_replay_input_fields_serialize_only_when_populated() {
        let mut log = create_test_buy_log();
        let record = serde_json::to_value(&log).unwrap();
        assert!(record
            .get("gatekeeper_v2_replay_input_schema_version")
            .is_none());
        assert!(record
            .get("gatekeeper_v2_replay_ready_non_temporal")
            .is_none());
        assert!(record.get("gatekeeper_v2_phase_pass_vector").is_none());
        assert!(record.get("decision_eval_snapshots").is_none());

        log.gatekeeper_v2_replay_input_schema_version = Some(1);
        log.gatekeeper_v2_replay_ready_non_temporal = Some(true);
        log.gatekeeper_v2_replay_ready_temporal = Some(false);
        log.gatekeeper_v2_replay_missing_fields =
            vec!["temporal:decision_eval_snapshots".to_string()];
        log.gatekeeper_v2_phase_pass_vector = Some(serde_json::json!({
            "phase1": true,
            "phase2": true,
            "phase3": true,
            "phase4": true,
            "phase5": true,
            "phase6": true
        }));
        log.hard_reject_reason = Some("REJECT_CORE_FAIL".to_string());
        log.pdd_soft_penalty_points = Some(3);
        log.pdd_hard_fail_evaluated = Some(true);
        log.hard_fail_hhi_threshold = Some(0.20);
        log.observed_mode = Some("standard".to_string());
        log.observed_window_ms = Some(5_000);
        log.observed_stage = Some("terminal".to_string());
        log.decision_eval_snapshots = Some(serde_json::json!([
            {"elapsed_ms": 5_000, "stage": "terminal"}
        ]));

        let record = serde_json::to_value(&log).unwrap();
        assert_eq!(record["gatekeeper_v2_replay_input_schema_version"], 1);
        assert_eq!(record["gatekeeper_v2_replay_ready_non_temporal"], true);
        assert_eq!(record["gatekeeper_v2_replay_ready_temporal"], false);
        assert_eq!(
            record["gatekeeper_v2_replay_missing_fields"][0],
            "temporal:decision_eval_snapshots"
        );
        assert_eq!(record["gatekeeper_v2_phase_pass_vector"]["phase1"], true);
        assert_eq!(record["pdd_soft_penalty_points"], 3);
        assert_eq!(record["hard_fail_hhi_threshold"], 0.20);
        assert_eq!(record["observed_mode"], "standard");
        assert_eq!(record["observed_window_ms"], 5_000);
        assert_eq!(record["decision_eval_snapshots"][0]["elapsed_ms"], 5_000);
    }

    #[test]
    fn test_v3_serialized_replay_payload_hash_probe_detects_mismatch() {
        let mut log = create_test_buy_log();
        log.v3_materialization_version = Some(1);
        log.v3_replay_payload_schema_version = Some(1);
        log.v3_materialized_feature_snapshot = Some(serde_json::json!({
            "pool_id": "pool_payload",
            "session_metadata": {
                "session_id": 42,
                "observation_duration_ms": 2000
            }
        }));

        let log_value = serde_json::to_value(&log).unwrap();
        let expected_hash = v3_serialized_replay_payload_hash(&log_value, 1).unwrap();
        log.v3_feature_snapshot_hash = Some(expected_hash.clone());
        assert!(v3_serialized_replay_payload_hash_mismatch(&log).is_none());

        let mut same_payload_different_session = log.clone();
        same_payload_different_session
            .v3_materialized_feature_snapshot
            .as_mut()
            .unwrap()["session_metadata"]["session_id"] = serde_json::json!(99);
        assert!(
            v3_serialized_replay_payload_hash_mismatch(&same_payload_different_session).is_none()
        );

        log.v3_feature_snapshot_hash = Some("not-the-serialized-payload-hash".to_string());
        let mismatch = v3_serialized_replay_payload_hash_mismatch(&log)
            .expect("tampered serialized replay hash should be detected");
        assert_eq!(mismatch.0, "not-the-serialized-payload-hash");
        assert_eq!(mismatch.1, expected_hash);
    }

    #[test]
    fn test_v3_post_serialize_replay_payload_hash_probe_detects_mismatch() {
        let mut log = create_test_buy_log();
        log.v3_materialization_version = Some(1);
        log.v3_replay_payload_schema_version = Some(1);
        log.v3_materialized_feature_snapshot = Some(serde_json::json!({
            "pool_id": "pool_payload",
            "session_metadata": {
                "session_id": 42,
                "observation_duration_ms": 2000
            }
        }));

        let json = serde_json::to_string(&log).unwrap();
        let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let expected_hash = v3_serialized_replay_payload_hash(&json_value, 1).unwrap();
        log.v3_feature_snapshot_hash = Some(expected_hash.clone());
        let json = serde_json::to_string(&log).unwrap();
        assert!(v3_post_serialize_replay_payload_hash_mismatch(&log, &json).is_none());

        log.v3_feature_snapshot_hash = Some("not-the-post-serialize-payload-hash".to_string());
        let json = serde_json::to_string(&log).unwrap();
        let mismatch = v3_post_serialize_replay_payload_hash_mismatch(&log, &json)
            .expect("tampered post-serialize replay hash should be detected");
        assert_eq!(mismatch.0, "not-the-post-serialize-payload-hash");
        assert_eq!(mismatch.1, expected_hash);
    }

    #[test]
    fn test_v3_replay_stable_gatekeeper_buy_log_json_canonicalizes_hash() {
        let mut log = create_test_buy_log();
        log.v3_materialization_version = Some(1);
        log.v3_replay_payload_schema_version = Some(1);
        log.v3_materialized_feature_snapshot = Some(serde_json::json!({
            "pool_id": "pool_payload",
            "session_metadata": {
                "session_id": 42,
                "observation_duration_ms": 2000
            }
        }));
        log.v3_feature_snapshot_hash = Some("pre-serialize-payload-hash".to_string());

        let json = v3_replay_stable_gatekeeper_buy_log_json(&log).unwrap();
        let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let expected_hash = v3_serialized_replay_payload_hash(&json_value, 1).unwrap();

        assert_eq!(
            json_value["v3_feature_snapshot_hash"].as_str(),
            Some(expected_hash.as_str())
        );
        assert!(v3_post_serialize_replay_payload_hash_mismatch(&log, &json).is_some());
    }

    #[test]
    fn test_gatekeeper_buy_log_v19_without_v3_fields_deserializes() {
        let mut legacy_value = serde_json::to_value(create_test_buy_log()).unwrap();
        legacy_value["log_schema_version"] = serde_json::json!(19);

        let parsed: GatekeeperBuyLog = serde_json::from_value(legacy_value).unwrap();
        assert_eq!(parsed.log_schema_version, 19);
        assert!(parsed.v3_shadow_schema_version.is_none());
        assert!(parsed.v3_shadow_verdict.is_none());
        assert!(parsed.v3_shadow_stage.is_none());
        assert!(parsed.v3_shadow_reason_code.is_none());
        assert!(parsed.v3_shadow_reason_chain.is_none());
        assert!(parsed.v3_shadow_secondary_reason_codes.is_none());
        assert!(parsed.v3_shadow_risk_status.is_none());
        assert!(parsed.v3_shadow_risk_primary_reason.is_none());
        assert!(parsed.v3_shadow_risk_penalty.is_none());
        assert!(parsed.v3_shadow_opportunity_status.is_none());
        assert!(parsed.v3_shadow_opportunity_score.is_none());
        assert!(parsed.v3_shadow_confidence_raw.is_none());
        assert!(parsed.v3_shadow_confidence_after_risk.is_none());
        assert!(parsed.v3_shadow_confidence_after_stage.is_none());
        assert!(parsed.v3_shadow_confidence_cap.is_none());
        assert!(parsed.v3_shadow_confidence_cap_reasons.is_none());
        assert!(parsed.v3_shadow_confidence_final.is_none());
        assert!(parsed.v3_shadow_confidence.is_none());
        assert!(parsed.v3_shadow_evidence_status.is_none());
        assert!(parsed.v3_shadow_organic_broadening.is_none());
        assert!(parsed.v3_shadow_manipulation_contradictions.is_none());
        assert!(parsed.v3_evidence_status.is_none());
        assert!(parsed.v3_organic_broadening.is_none());
        assert!(parsed.v3_manipulation_contradictions.is_none());
        assert!(parsed.v3_policy_config_hash.is_none());
        assert!(parsed.v3_feature_snapshot_hash.is_none());
        assert!(parsed.v3_replay_payload_schema_version.is_none());
        assert!(parsed.v3_materialized_feature_snapshot.is_none());
        assert!(parsed.v3_policy_config_payload.is_none());
        assert!(parsed.v3_materialization_version.is_none());
        assert!(parsed.v3_policy_version.is_none());
        assert!(parsed.v3_stage_thresholds.is_none());
        assert!(parsed.v3_component_scores.is_none());
        assert!(parsed.v3_actionability.is_none());
        assert!(parsed.v3_shadow_notes.is_none());
    }

    #[test]
    fn test_v3_shadow_fields_are_additive_not_routed_plane() {
        let mut log = create_test_buy_log();
        log.pool_id = "pool_v3_shadow_sidecar".to_string();
        log.decision_reason = Some("legacy reject".to_string());
        log.decision_verdict_buy = Some(false);
        log.verdict_type = Some("REJECT_CORE_FAIL".to_string());
        log.reason_code = Some(GatekeeperReasonCode::RejectCoreFail.as_log_str());
        log.reason_code_version = GatekeeperReasonCode::version();
        log.v3_shadow_schema_version = Some(1);
        log.v3_shadow_verdict = Some("PENDING".to_string());
        log.v3_shadow_stage = Some("EVIDENCE".to_string());
        log.v3_shadow_reason_code = Some(GatekeeperReasonCode::PendingV3WaitEvidence.as_log_str());
        log.v3_shadow_reason_chain = Some(vec![
            GatekeeperReasonCode::V3EvidenceDegraded.as_log_str(),
            GatekeeperReasonCode::PendingV3WaitEvidence.as_log_str(),
        ]);
        log.v3_shadow_secondary_reason_codes = Some(vec![
            GatekeeperReasonCode::PendingV3WaitEvidence.as_log_str(),
        ]);
        log.v3_shadow_risk_status = Some("DEGRADED".to_string());
        log.v3_shadow_risk_primary_reason = None;
        log.v3_shadow_risk_penalty = Some(0.0);
        log.v3_shadow_opportunity_status = Some("DEGRADED".to_string());
        log.v3_shadow_opportunity_score = Some(0.0);
        log.v3_shadow_confidence_raw = Some(0.0);
        log.v3_shadow_confidence_after_risk = Some(0.0);
        log.v3_shadow_confidence_after_stage = Some(0.0);
        log.v3_shadow_confidence_cap = Some(0.0);
        log.v3_shadow_confidence_cap_reasons = Some(vec!["insufficient_evidence".to_string()]);
        log.v3_shadow_confidence_final = Some(0.0);
        log.v3_shadow_confidence = Some(0.0);
        log.v3_shadow_evidence_status = Some(serde_json::json!({
            "tx_intel": { "status": "DEGRADED", "reasons": ["SEGMENT_SEQUENCE_PARTIAL"] }
        }));
        log.v3_shadow_organic_broadening = Some(serde_json::json!({
            "sequence_available": false
        }));
        log.v3_shadow_manipulation_contradictions = Some(serde_json::json!({
            "dev_sold": false
        }));
        log.v3_evidence_status = log.v3_shadow_evidence_status.clone();
        log.v3_organic_broadening = log.v3_shadow_organic_broadening.clone();
        log.v3_manipulation_contradictions = log.v3_shadow_manipulation_contradictions.clone();
        log.v3_policy_config_hash = Some("v3-policy-hash".to_string());
        log.v3_feature_snapshot_hash = Some("v3-feature-hash".to_string());
        log.v3_replay_payload_schema_version = Some(1);
        log.v3_materialized_feature_snapshot = Some(serde_json::json!({
            "materialization_version": 1,
            "token_mint": "mint_v3_replay"
        }));
        log.v3_policy_config_payload = Some(serde_json::json!({
            "policy_version": 1,
            "profiles": {"normal": {"min_tx_count": 12}}
        }));
        log.v3_materialization_version = Some(1);
        log.v3_policy_version = Some(1);
        log.v3_stage_thresholds = Some(serde_json::json!({
            "evidence": {"min_tx_count": 12}
        }));
        log.v3_component_scores = Some(serde_json::json!({
            "opportunity": {"score": 0.0}
        }));
        log.v3_actionability = Some(serde_json::json!({
            "stages": {"evidence": "not_actionable"}
        }));
        log.v3_shadow_notes = Some(serde_json::json!({
            "p1": "calibrated_shadow_funnel"
        }));

        let expanded = expand_gatekeeper_plane_logs(log);
        assert_eq!(expanded.len(), 1);
        assert!(
            expanded
                .iter()
                .all(|row| row.decision_plane.as_deref() != Some("v3_shadow")),
            "V3 P0 must not create a routed v3_shadow decision plane"
        );

        let record = serde_json::to_value(&expanded[0]).unwrap();
        assert_eq!(record["decision_plane"], DECISION_PLANE_LEGACY_LIVE);
        assert_eq!(record["v3_shadow_schema_version"], 1);
        assert_eq!(record["v3_shadow_verdict"], "PENDING");
        assert_eq!(record["v3_shadow_stage"], "EVIDENCE");
        assert_eq!(
            record["v3_shadow_reason_code"],
            GatekeeperReasonCode::PendingV3WaitEvidence.as_log_str()
        );
        assert_eq!(
            record["v3_shadow_reason_chain"][0],
            GatekeeperReasonCode::V3EvidenceDegraded.as_log_str()
        );
        assert_eq!(
            record["v3_shadow_secondary_reason_codes"][0],
            GatekeeperReasonCode::PendingV3WaitEvidence.as_log_str()
        );
        assert_eq!(record["v3_shadow_risk_status"], "DEGRADED");
        assert_eq!(record["v3_shadow_opportunity_status"], "DEGRADED");
        assert_eq!(record["v3_shadow_confidence_final"], 0.0);
        assert_eq!(record["v3_shadow_confidence"], 0.0);
        assert_eq!(record["v3_policy_config_hash"], "v3-policy-hash");
        assert_eq!(record["v3_feature_snapshot_hash"], "v3-feature-hash");
        assert_eq!(record["v3_replay_payload_schema_version"], 1);
        assert_eq!(
            record["v3_materialized_feature_snapshot"]["token_mint"],
            "mint_v3_replay"
        );
        assert_eq!(
            record["v3_policy_config_payload"]["profiles"]["normal"]["min_tx_count"],
            12
        );
        assert_eq!(record["v3_materialization_version"], 1);
        assert_eq!(record["v3_policy_version"], 1);
        assert_eq!(
            record["v3_stage_thresholds"]["evidence"]["min_tx_count"],
            12
        );
        assert_eq!(record["v3_component_scores"]["opportunity"]["score"], 0.0);
        assert_eq!(
            record["v3_actionability"]["stages"]["evidence"],
            "not_actionable"
        );
        assert_eq!(
            record["v3_evidence_status"]["tx_intel"]["status"],
            "DEGRADED"
        );
        assert_eq!(
            record["v3_shadow_notes"]["p1"],
            serde_json::json!("calibrated_shadow_funnel")
        );
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

    fn test_decision_logger_config(log_dir: PathBuf) -> DecisionLoggerConfig {
        DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir,
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        }
    }

    async fn read_test_selector_shadow_score_rows(log_dir: &Path) -> Vec<serde_json::Value> {
        let sidecar_file = test_gatekeeper_route_dir(
            log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(SELECTOR_SHADOW_SCORE_JSONL);
        assert!(sidecar_file.exists(), "selector sidecar should exist");
        let content = fs::read_to_string(&sidecar_file).await.unwrap();
        content
            .trim()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn test_selector_shadow_score_sidecar_writes_additive_jsonl() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_score_sidecar".to_string();
        buy_log.base_mint = Some("mint_score_sidecar".to_string());
        buy_log.execution_candidate_id = Some("candidate_score_sidecar".to_string());
        buy_log.ab_record_id = Some("pool_score_sidecar:1000:11000:BUY".to_string());
        buy_log.decision_verdict_buy = Some(true);
        buy_log.verdict_type = Some("BUY".to_string());
        buy_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());

        logger.log_gatekeeper_buy_decision(buy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        assert!(decisions_file.exists(), "decision JSONL should still exist");

        let rows = read_test_selector_shadow_score_rows(&log_dir).await;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["schema_version"], SELECTOR_SHADOW_SCORE_SCHEMA_VERSION);
        assert_eq!(row["score_version"], SELECTOR_SHADOW_SCORE_VERSION);
        assert_eq!(
            row["score_candidate_id"],
            SELECTOR_SHADOW_SCORE_CANDIDATE_ID
        );
        assert_eq!(row["candidate_id"], "candidate_score_sidecar");
        assert_eq!(row["pool_id"], "pool_score_sidecar");
        assert_eq!(row["base_mint"], "mint_score_sidecar");
        assert_eq!(row["gatekeeper_verdict_type"], "BUY");

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_emitted_for_buy_reject_timeout() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut pass_log = create_test_buy_log();
        pass_log.pool_id = "pool_score_buy".to_string();
        pass_log.decision_verdict_buy = Some(true);
        pass_log.verdict_type = Some("BUY".to_string());
        pass_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());
        pass_log.ab_record_id = Some("pool_score_buy:1000:11000:BUY".to_string());

        let mut reject_log = create_test_buy_log();
        reject_log.pool_id = "pool_score_reject".to_string();
        reject_log.decision_verdict_buy = Some(false);
        reject_log.verdict_type = Some("REJECT_CORE_FAIL".to_string());
        reject_log.reason_code = Some(GatekeeperReasonCode::RejectCoreFail.as_log_str());
        reject_log.ab_record_id = Some("pool_score_reject:1000:11000:REJECT".to_string());

        let mut timeout_log = create_test_buy_log();
        timeout_log.pool_id = "pool_score_timeout".to_string();
        timeout_log.decision_verdict_buy = None;
        timeout_log.verdict_type = Some("TIMEOUT_PHASE1".to_string());
        timeout_log.reason_code =
            Some(GatekeeperReasonCode::TimeoutPhase1Insufficient.as_log_str());
        timeout_log.ab_record_id = Some("pool_score_timeout:1000:11000:TIMEOUT".to_string());

        logger.log_gatekeeper_buy_decision(pass_log).await;
        logger.log_gatekeeper_buy_decision(reject_log).await;
        logger.log_gatekeeper_buy_decision(timeout_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        let decisions_content = fs::read_to_string(&decisions_file).await.unwrap();
        assert_eq!(decisions_content.trim().lines().count(), 3);

        let rows = read_test_selector_shadow_score_rows(&log_dir).await;
        let pool_ids: Vec<&str> = rows
            .iter()
            .map(|row| row["pool_id"].as_str().unwrap())
            .collect();
        assert_eq!(pool_ids.len(), 3);
        assert!(pool_ids.contains(&"pool_score_buy"));
        assert!(pool_ids.contains(&"pool_score_reject"));
        assert!(pool_ids.contains(&"pool_score_timeout"));
        let verdict_types: Vec<&str> = rows
            .iter()
            .map(|row| row["gatekeeper_verdict_type"].as_str().unwrap())
            .collect();
        assert!(verdict_types.contains(&"BUY"));
        assert!(verdict_types.contains(&"REJECT_CORE_FAIL"));
        assert!(verdict_types.contains(&"TIMEOUT_PHASE1"));

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_sidecar_has_non_claim_boundaries() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_score_boundaries".to_string();
        buy_log.ab_record_id = Some("pool_score_boundaries:1000:11000:REJECT".to_string());

        logger.log_gatekeeper_buy_decision(buy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let rows = read_test_selector_shadow_score_rows(&log_dir).await;
        let boundaries = &rows[0]["claim_boundaries"];
        assert_eq!(boundaries["diagnostic_only"], true);
        assert_eq!(boundaries["shadow_only"], true);
        assert_eq!(boundaries["production_promotion_allowed"], false);
        assert_eq!(boundaries["gatekeeper_tuning_started"], false);
        assert_eq!(boundaries["changes_gatekeeper_decision"], false);
        assert_eq!(boundaries["changes_execution"], false);
        assert_eq!(boundaries["send_path_changed"], false);

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_does_not_change_gatekeeper_verdict() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut reject_log = create_test_buy_log();
        reject_log.pool_id = "pool_score_verdict".to_string();
        reject_log.decision_verdict_buy = Some(false);
        reject_log.verdict_type = Some("REJECT_CORE_FAIL".to_string());
        reject_log.reason_code = Some(GatekeeperReasonCode::RejectCoreFail.as_log_str());
        reject_log.ab_record_id = Some("pool_score_verdict:1000:11000:REJECT".to_string());

        logger.log_gatekeeper_buy_decision(reject_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        let decisions_content = fs::read_to_string(&decisions_file).await.unwrap();
        let decision: serde_json::Value =
            serde_json::from_str(decisions_content.trim().lines().next().unwrap()).unwrap();
        assert_eq!(decision["decision_verdict_buy"], false);
        assert_eq!(decision["verdict_type"], "REJECT_CORE_FAIL");
        assert!(decision.get("selector_shadow_score").is_none());

        let sidecar_rows = read_test_selector_shadow_score_rows(&log_dir).await;
        assert_eq!(
            sidecar_rows[0]["claim_boundaries"]["changes_gatekeeper_decision"],
            false
        );

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_does_not_change_execution_eligibility() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_score_execution".to_string();
        buy_log.shadow_trigger_eligible = Some(true);
        buy_log.shadow_execution_outcome = Some("shadow_dispatch_ready".to_string());
        buy_log.ab_record_id = Some("pool_score_execution:1000:11000:BUY".to_string());
        buy_log.decision_verdict_buy = Some(true);
        buy_log.verdict_type = Some("BUY".to_string());
        buy_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());

        logger.log_gatekeeper_buy_decision(buy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        let decisions_content = fs::read_to_string(&decisions_file).await.unwrap();
        let decision: serde_json::Value =
            serde_json::from_str(decisions_content.trim().lines().next().unwrap()).unwrap();
        assert_eq!(decision["shadow_trigger_eligible"], true);
        assert_eq!(
            decision["shadow_execution_outcome"],
            "shadow_dispatch_ready"
        );

        let sidecar_rows = read_test_selector_shadow_score_rows(&log_dir).await;
        assert_eq!(
            sidecar_rows[0]["claim_boundaries"]["changes_execution"],
            false
        );
        assert_eq!(
            sidecar_rows[0]["claim_boundaries"]["send_path_changed"],
            false
        );

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_numeric_when_features_available() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_score_numeric".to_string();
        buy_log.observation_end_ts_ms = Some(11_000);
        buy_log.curve_wait_elapsed_ms = Some(10_010);
        buy_log.vectors_ts_offsets_ms = Some(vec![0, 250, 1000, 1800]);
        buy_log.vectors_sol_amounts = Some(vec![0.5, 1.0, 2.0, 0.75]);
        buy_log.vectors_prices = Some(vec![0.00000003, 0.00000004, 0.00000005, 0.000000045]);
        buy_log.ab_record_id = Some("pool_score_numeric:1000:11000:BUY".to_string());

        logger.log_gatekeeper_buy_decision(buy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let rows = read_test_selector_shadow_score_rows(&log_dir).await;
        let row = &rows[0];
        let score = row["selector_shadow_score"]
            .as_f64()
            .expect("P3L-B should emit a numeric shadow score");
        assert!((0.0..=1.0).contains(&score));
        assert_eq!(row["score_validity_status"], SELECTOR_SHADOW_SCORE_VALID);
        assert_eq!(row["score_valid"], true);
        assert_eq!(row["score_degraded"], false);
        assert_eq!(
            row["feature_availability"]["runtime_score_adapter_available"],
            true
        );
        assert_eq!(
            row["feature_availability"]["feature_mapping_status"],
            "partial_runtime_mapping_missing_flow_features"
        );
        assert_eq!(row["feature_availability"]["cutoff_verified"], true);
        assert!(row["feature_missing_count"].as_u64().unwrap() > 0);
        assert_eq!(row["required_feature_missing_count"], 0);
        assert!(row["reason_vector"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("net_quote_in_15s")));

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_filters_non_finite_feature_values() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_score_non_finite".to_string();
        buy_log.observation_end_ts_ms = Some(11_000);
        buy_log.vectors_prices = Some(vec![f64::NAN, 0.00000004, 0.00000005]);
        buy_log.vectors_sol_amounts = Some(vec![0.5, 1.0, 2.0]);
        buy_log.vectors_ts_offsets_ms = Some(vec![0, 250, 1000]);
        buy_log.ab_record_id = Some("pool_score_non_finite:1000:11000:REJECT".to_string());

        logger.log_gatekeeper_buy_decision(buy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let rows = read_test_selector_shadow_score_rows(&log_dir).await;
        let row = &rows[0];
        let score = row["selector_shadow_score"]
            .as_f64()
            .expect("non-finite feature inputs must not serialize score as null");
        assert!(score.is_finite());
        let missing = row["reason_vector"]["missing"].as_array().unwrap();
        assert!(missing
            .iter()
            .any(|value| value.as_str() == Some("gk_vector_price_first")));
        assert!(missing
            .iter()
            .any(|value| value.as_str() == Some("gk_vector_price_return")));

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_degraded_when_concentration_missing() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_score_degraded_concentration".to_string();
        buy_log.observation_end_ts_ms = Some(11_000);
        buy_log.hhi = None;
        buy_log.top3_volume_pct = None;
        buy_log.ab_record_id =
            Some("pool_score_degraded_concentration:1000:11000:REJECT".to_string());

        logger.log_gatekeeper_buy_decision(buy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let rows = read_test_selector_shadow_score_rows(&log_dir).await;
        let row = &rows[0];
        assert!(row["selector_shadow_score"].as_f64().is_some());
        assert_eq!(
            row["score_validity_status"],
            SELECTOR_SHADOW_SCORE_DEGRADED_MISSING_CONCENTRATION
        );
        assert_eq!(row["score_valid"], false);
        assert_eq!(row["score_degraded"], true);
        assert_eq!(
            row["feature_availability"]["concentration_available"],
            false
        );
        assert!(row["reason_vector"]["negative"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("gk_concentration_missing")));

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_selector_shadow_score_invalid_when_core_curve_missing() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let logger = DecisionLogger::new(test_decision_logger_config(log_dir.clone()));

        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_score_invalid_core".to_string();
        buy_log.observation_end_ts_ms = Some(11_000);
        buy_log.bonding_progress_pct = None;
        buy_log.current_market_cap_sol = None;
        buy_log.price_change_ratio = None;
        buy_log.curve_data_known = Some(false);
        buy_log.hhi = None;
        buy_log.top3_volume_pct = None;
        buy_log.ab_record_id = Some("pool_score_invalid_core:1000:11000:REJECT".to_string());

        logger.log_gatekeeper_buy_decision(buy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let rows = read_test_selector_shadow_score_rows(&log_dir).await;
        let row = &rows[0];
        assert_eq!(
            row["score_validity_status"],
            SELECTOR_SHADOW_SCORE_INVALID_MISSING_CORE
        );
        assert_eq!(row["score_valid"], false);
        assert_eq!(row["score_degraded"], false);
        assert!(row["selector_shadow_score"].as_f64().is_some());
        assert_eq!(
            row["feature_availability"]["runtime_score_adapter_available"],
            true
        );
        assert_eq!(
            row["feature_availability"]["core_curve_market_available"],
            false
        );
        assert_eq!(
            row["feature_availability"]["concentration_available"],
            false
        );
        assert!(row["reason_vector"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("gk_bonding_progress_pct")));

        logger.shutdown().await;
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
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
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
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        // 1× PASS (decision_verdict_buy = Some(true))
        let mut pass_log = create_test_buy_log();
        pass_log.pool_id = "pool_pass".to_string();
        pass_log.decision_verdict_buy = Some(true);
        pass_log.verdict_type = Some("BUY".to_string());
        pass_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());
        pass_log.reason_code_version = GatekeeperReasonCode::version();
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
        reject_log.reason_code = Some(GatekeeperReasonCode::RejectCoreFail.as_log_str());
        reject_log.reason_code_version = GatekeeperReasonCode::version();
        reject_log.ab_record_id = Some("pool_reject:1000:11000:REJECT".to_string());

        // 1× TIMEOUT (decision_verdict_buy = None)
        let mut timeout_log = create_test_buy_log();
        timeout_log.pool_id = "pool_timeout".to_string();
        timeout_log.decision_verdict_buy = None;
        timeout_log.verdict_type = Some("TIMEOUT_PHASE1".to_string());
        timeout_log.reason_code =
            Some(GatekeeperReasonCode::TimeoutPhase1Insufficient.as_log_str());
        timeout_log.reason_code_version = GatekeeperReasonCode::version();
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
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        let mut mixed_log = create_test_buy_log();
        mixed_log.pool_id = "pool_mixed".to_string();
        mixed_log.decision_reason = Some("legacy_buy".to_string());
        mixed_log.decision_verdict_buy = Some(true);
        mixed_log.verdict_type = Some("BUY".to_string());
        mixed_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());
        mixed_log.reason_code_version = GatekeeperReasonCode::version();
        mixed_log.legacy_live_reason_chain = Some("legacy_buy".to_string());
        mixed_log.legacy_live_verdict_buy = Some(true);
        mixed_log.legacy_live_verdict_type = Some("BUY".to_string());
        mixed_log.v25_shadow_reason_chain = Some("shadow reject due to TAS".to_string());
        mixed_log.v25_shadow_verdict_type =
            Some(GatekeeperReasonCode::RejectLowTrajectory.as_log_str());
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
            legacy_record["reason_code"],
            GatekeeperReasonCode::BuyNormal.as_log_str()
        );
        assert_eq!(
            legacy_record["v25_shadow_verdict_type"],
            GatekeeperReasonCode::RejectLowTrajectory.as_log_str()
        );

        assert_eq!(shadow_record["decision_plane"], DECISION_PLANE_V25_SHADOW);
        assert_eq!(shadow_record["gatekeeper_version"], GATEKEEPER_VERSION);
        assert_eq!(shadow_record["decision_verdict_buy"], false);
        assert_eq!(
            shadow_record["verdict_type"],
            GatekeeperReasonCode::RejectLowTrajectory.as_log_str()
        );
        assert_eq!(shadow_record["decision_reason"], "shadow reject due to TAS");
        assert_eq!(
            shadow_record["reason_code"],
            GatekeeperReasonCode::RejectLowTrajectory.as_log_str()
        );
        assert_eq!(shadow_record["legacy_live_verdict_type"], "BUY");

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_logger_persists_v25_timeout_with_cached_confidence_only() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        let mut timeout_log = create_test_buy_log();
        timeout_log.pool_id = "pool_v25_timeout".to_string();
        timeout_log.decision_reason =
            Some("TIMEOUT_PHASE1_INSUFFICIENT: tx=1/3 signers=1/2 buys=1/2".to_string());
        timeout_log.decision_verdict_buy = None;
        timeout_log.verdict_type = Some("TIMEOUT_PHASE1_INSUFFICIENT".to_string());
        timeout_log.reason_code =
            Some(GatekeeperReasonCode::TimeoutPhase1Insufficient.as_log_str());
        timeout_log.reason_code_version = GatekeeperReasonCode::version();
        timeout_log.legacy_live_reason_chain = timeout_log.decision_reason.clone();
        timeout_log.legacy_live_verdict_buy = None;
        timeout_log.legacy_live_verdict_type = timeout_log.verdict_type.clone();
        timeout_log.v25_shadow_verdict_type = None;
        timeout_log.v25_shadow_reason_chain = None;
        timeout_log.v25_shadow_confidence = Some(0.0);
        timeout_log.v25_shadow_confidence_source = Some("assessment_cached".to_string());
        timeout_log.v25_shadow_observation_stage = Some("Extended".to_string());
        timeout_log.ab_record_id = Some("pool_v25_timeout:1000:11000:TIMEOUT".to_string());

        logger.log_gatekeeper_buy_decision(timeout_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let shadow_dir =
            test_gatekeeper_route_dir(&log_dir, GATEKEEPER_VERSION, DECISION_PLANE_V25_SHADOW);
        let shadow_decisions = shadow_dir.join(GATEKEEPER_DECISIONS_JSONL);
        let shadow_buys = shadow_dir.join(GATEKEEPER_PASSED_JSONL);

        assert!(
            shadow_decisions.exists(),
            "v25 shadow timeout decisions file should exist"
        );
        assert!(
            !shadow_buys.exists(),
            "TIMEOUT must not be written to the v25 shadow BUY file"
        );

        let shadow_lines = fs::read_to_string(&shadow_decisions).await.unwrap();
        let lines: Vec<&str> = shadow_lines.trim().lines().collect();
        assert_eq!(lines.len(), 1);
        let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();

        assert_eq!(record["decision_plane"], DECISION_PLANE_V25_SHADOW);
        assert_eq!(record["gatekeeper_version"], GATEKEEPER_VERSION);
        assert_eq!(
            record["ab_record_id"],
            "pool_v25_timeout:1000:11000:TIMEOUT"
        );
        assert_eq!(record["verdict_type"], "TIMEOUT_PHASE1_INSUFFICIENT");
        assert_eq!(
            record["reason_code"],
            GatekeeperReasonCode::TimeoutPhase1Insufficient.as_log_str()
        );
        assert!(
            record.get("decision_verdict_buy").is_none(),
            "TIMEOUT rows must keep decision_verdict_buy absent/null, not false"
        );
        assert_eq!(
            record["decision_reason"],
            "TIMEOUT_PHASE1_INSUFFICIENT: tx=1/3 signers=1/2 buys=1/2"
        );

        logger.shutdown().await;
    }

    #[test]
    fn test_expand_shadow_plane_does_not_fallback_to_main_reason_code() {
        let mut mixed_log = create_test_buy_log();
        mixed_log.pool_id = "pool_shadow_no_fallback".to_string();
        mixed_log.decision_reason = Some("legacy reject".to_string());
        mixed_log.decision_verdict_buy = Some(false);
        mixed_log.verdict_type = Some("REJECT_HARD_FAIL".to_string());
        mixed_log.reason_code = Some(GatekeeperReasonCode::HardFailPriceChange.as_log_str());
        mixed_log.reason_code_version = GatekeeperReasonCode::version();
        mixed_log.legacy_live_reason_chain = mixed_log.decision_reason.clone();
        mixed_log.legacy_live_verdict_buy = mixed_log.decision_verdict_buy;
        mixed_log.legacy_live_verdict_type = mixed_log.verdict_type.clone();
        mixed_log.v25_shadow_reason_chain = Some("shadow generic reject".to_string());
        mixed_log.v25_shadow_verdict_type = Some("REJECT_PUMP_AND_DUMP".to_string());
        mixed_log.v25_shadow_confidence = Some(0.0);
        mixed_log.v25_shadow_observation_stage = Some("Normal".to_string());

        let expanded = expand_gatekeeper_plane_logs(mixed_log);
        assert_eq!(expanded.len(), 2, "expected legacy and shadow plane rows");

        let legacy = expanded
            .iter()
            .find(|log| log.decision_plane.as_deref() == Some(DECISION_PLANE_LEGACY_LIVE))
            .expect("legacy plane row");
        assert_eq!(
            legacy.reason_code.as_deref(),
            Some(
                GatekeeperReasonCode::HardFailPriceChange
                    .as_log_str()
                    .as_str()
            )
        );

        let shadow = expanded
            .iter()
            .find(|log| log.decision_plane.as_deref() == Some(DECISION_PLANE_V25_SHADOW))
            .expect("shadow plane row");
        assert_eq!(shadow.verdict_type.as_deref(), Some("REJECT_PUMP_AND_DUMP"));
        assert_eq!(
            shadow.reason_code, None,
            "shadow plane must not inherit main reason_code"
        );
    }

    #[test]
    fn test_expand_timeout_fallback_requires_cached_shadow_assessment_evidence() {
        let mut timeout_log = create_test_buy_log();
        timeout_log.pool_id = "pool_shadow_reason_only_timeout".to_string();
        timeout_log.decision_reason =
            Some("TIMEOUT_PHASE1_INSUFFICIENT: terminal active timeout".to_string());
        timeout_log.decision_verdict_buy = None;
        timeout_log.verdict_type = Some("TIMEOUT_PHASE1_INSUFFICIENT".to_string());
        timeout_log.reason_code =
            Some(GatekeeperReasonCode::TimeoutPhase1Insufficient.as_log_str());
        timeout_log.reason_code_version = GatekeeperReasonCode::version();
        timeout_log.legacy_live_reason_chain = timeout_log.decision_reason.clone();
        timeout_log.legacy_live_verdict_buy = None;
        timeout_log.legacy_live_verdict_type = timeout_log.verdict_type.clone();
        timeout_log.v25_shadow_reason_chain = Some("shadow reason only".to_string());
        timeout_log.v25_shadow_verdict_type = None;
        timeout_log.v25_shadow_confidence = None;
        timeout_log.v25_shadow_observation_stage = None;

        let expanded = expand_gatekeeper_plane_logs(timeout_log);
        assert_eq!(expanded.len(), 2, "expected legacy and shadow plane rows");

        let shadow = expanded
            .iter()
            .find(|log| log.decision_plane.as_deref() == Some(DECISION_PLANE_V25_SHADOW))
            .expect("shadow plane row");

        assert_eq!(
            shadow.decision_reason.as_deref(),
            Some("shadow reason only")
        );
        assert_eq!(
            shadow.verdict_type, None,
            "reason-chain-only shadow evidence must not inherit terminal TIMEOUT verdict"
        );
        assert_eq!(
            shadow.reason_code, None,
            "reason-chain-only shadow evidence must not inherit terminal TIMEOUT reason_code"
        );
        assert_eq!(
            shadow.decision_verdict_buy, None,
            "TIMEOUT fallback must not synthesize a BUY/REJECT alias without cached shadow assessment evidence"
        );
    }

    #[tokio::test]
    async fn test_logger_persists_legacy_plane_with_reason_code() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        let mut legacy_log = create_test_buy_log();
        legacy_log.pool_id = "pool_reason_code_ok".to_string();
        legacy_log.decision_reason = Some("legacy buy".to_string());
        legacy_log.decision_verdict_buy = Some(true);
        legacy_log.verdict_type = Some("BUY".to_string());
        legacy_log.legacy_live_reason_chain = legacy_log.decision_reason.clone();
        legacy_log.legacy_live_verdict_buy = legacy_log.decision_verdict_buy;
        legacy_log.legacy_live_verdict_type = legacy_log.verdict_type.clone();
        legacy_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());
        legacy_log.reason_code_version = GatekeeperReasonCode::version();
        legacy_log.ab_record_id = Some("pool_reason_code_ok:1000:11000:BUY".to_string());

        logger.log_gatekeeper_buy_decision(legacy_log).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let decisions_file = test_gatekeeper_route_dir(
            &log_dir,
            LEGACY_GATEKEEPER_VERSION,
            DECISION_PLANE_LEGACY_LIVE,
        )
        .join(GATEKEEPER_DECISIONS_JSONL);
        assert!(
            decisions_file.exists(),
            "legacy decisions file should exist"
        );

        let content = fs::read_to_string(&decisions_file).await.unwrap();
        let records: Vec<serde_json::Value> = content
            .trim()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["pool_id"], "pool_reason_code_ok");
        assert_eq!(
            records[0]["reason_code"],
            GatekeeperReasonCode::BuyNormal.as_log_str()
        );

        logger.shutdown().await;
    }

    #[tokio::test]
    async fn test_logger_drops_only_plane_rows_missing_reason_code() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        let mut mixed_log = create_test_buy_log();
        mixed_log.pool_id = "pool_partial_drop".to_string();
        mixed_log.decision_reason = Some("legacy buy".to_string());
        mixed_log.decision_verdict_buy = Some(true);
        mixed_log.verdict_type = Some("BUY".to_string());
        mixed_log.legacy_live_reason_chain = mixed_log.decision_reason.clone();
        mixed_log.legacy_live_verdict_buy = mixed_log.decision_verdict_buy;
        mixed_log.legacy_live_verdict_type = mixed_log.verdict_type.clone();
        mixed_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());
        mixed_log.reason_code_version = GatekeeperReasonCode::version();
        mixed_log.v25_shadow_reason_chain = Some("shadow generic reject".to_string());
        mixed_log.v25_shadow_verdict_type = Some("REJECT_PUMP_AND_DUMP".to_string());
        mixed_log.v25_shadow_confidence = Some(0.0);
        mixed_log.v25_shadow_observation_stage = Some("Normal".to_string());
        mixed_log.ab_record_id = Some("pool_partial_drop:1000:11000:MIXED".to_string());

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

        assert!(
            legacy_decisions.exists(),
            "valid legacy plane row should be persisted"
        );
        assert!(
            !shadow_decisions.exists(),
            "shadow plane row without typed reason_code must be dropped"
        );

        let legacy_content = fs::read_to_string(&legacy_decisions).await.unwrap();
        let records: Vec<serde_json::Value> = legacy_content
            .trim()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["pool_id"], "pool_partial_drop");
        assert_eq!(records[0]["decision_plane"], DECISION_PLANE_LEGACY_LIVE);
        assert_eq!(
            records[0]["reason_code"],
            GatekeeperReasonCode::BuyNormal.as_log_str()
        );

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
            gatekeeper_run_id: None,
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);

        // Case 1: legacy/malformed fallback: verdict_type = None, decision_verdict_buy = None → TIMEOUT
        let mut timeout_log = create_test_buy_log();
        timeout_log.pool_id = "pool_vt_timeout".to_string();
        timeout_log.decision_verdict_buy = None;
        timeout_log.verdict_type = None;
        timeout_log.reason_code = Some(GatekeeperReasonCode::TimeoutDeadlineLowPhases.as_log_str());
        timeout_log.reason_code_version = GatekeeperReasonCode::version();
        timeout_log.ab_record_id = Some("pool_vt_timeout:1:2:A".to_string());

        // Case 2: verdict_type = None, decision_verdict_buy = Some(false) → REJECT_UNKNOWN
        let mut reject_log = create_test_buy_log();
        reject_log.pool_id = "pool_vt_reject".to_string();
        reject_log.decision_verdict_buy = Some(false);
        reject_log.verdict_type = None;
        reject_log.reason_code = Some(GatekeeperReasonCode::RejectCoreFail.as_log_str());
        reject_log.reason_code_version = GatekeeperReasonCode::version();
        reject_log.ab_record_id = Some("pool_vt_reject:1:2:B".to_string());

        // Case 3: verdict_type = None, decision_verdict_buy = Some(true) → BUY
        let mut buy_log = create_test_buy_log();
        buy_log.pool_id = "pool_vt_buy".to_string();
        buy_log.decision_verdict_buy = Some(true);
        buy_log.verdict_type = None;
        buy_log.reason_code = Some(GatekeeperReasonCode::BuyNormal.as_log_str());
        buy_log.reason_code_version = GatekeeperReasonCode::version();
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

    #[tokio::test]
    async fn test_coordination_risk_evidence_sidecar_logging() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path().to_path_buf();
        let config = DecisionLoggerConfig {
            log_dir: log_dir.clone(),
            gatekeeper_log_dir: log_dir.clone(),
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            gatekeeper_config_hash: "test-config-hash".to_string(),
            gatekeeper_run_id: Some("run-42".to_string()),
            gatekeeper_session_id: None,
            brain_config_path: None,
            brain_config_hash: None,
            channel_buffer_size: 10,
            enabled: true,
        };
        let logger = DecisionLogger::new(config);
        let pool_id = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let unit = CoordinationRiskEvidenceUnit {
            schema_version: 1,
            scope_id: String::new(),
            run_id: None,
            candidate_id: Some("candidate-42".to_string()),
            pool_id,
            mint,
            decision_id: Some("decision-42".to_string()),
            decision_ts_ms: 1_000,
            decision_slot: Some(10),
            snapshot_mode: CoordinationSnapshotMode::DecisionTime,
            feature_cutoff_ts_ms: 999,
            feature_cutoff_slot: Some(10),
            source_buffer_watermark_slot: Some(10),
            computed_at_recv_ts_ns: 1_000_000,
            gatekeeper_version: None,
            source_snapshot_hash: Some("snapshot-hash".to_string()),
            sample_summary: Default::default(),
            funding_visibility: FundingVisibility::Unavailable,
            features: CoordinationRiskFeatures::default(),
            metric_breakdowns: CoordinationMetricBreakdowns::default(),
            skipped_metrics: Default::default(),
            degraded_reasons: Default::default(),
        };

        logger.log_coordination_risk_evidence(unit).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let sidecar_path = log_dir
            .join("test-rollout")
            .join(GATEKEEPER_VERSION)
            .join("coordination_risk")
            .join(COORDINATION_RISK_EVIDENCE_JSONL);
        assert!(sidecar_path.exists(), "sidecar file should be created");
        let content = fs::read_to_string(&sidecar_path).await.unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["scope_id"], "test-rollout");
        assert_eq!(parsed["run_id"], "run-42");
        assert_eq!(parsed["gatekeeper_version"], GATEKEEPER_VERSION);
        assert_eq!(parsed["pool_id"], serde_json::to_value(pool_id).unwrap());
        assert_eq!(parsed["mint"], serde_json::to_value(mint).unwrap());
        assert!(parsed["features"]
            .get("total_coordination_penalty")
            .is_none());
        assert!(parsed["features"].get("interaction_penalty").is_none());

        logger.shutdown().await;
    }
}
