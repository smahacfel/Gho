//! Offline-only ACE-EV V2 evaluator.
//!
//! This module is deliberately outside the live decision path.  It consumes
//! immutable EventWriter JSONL plus the capture-start manifest after a
//! controlled shutdown; it never subscribes to the Event Bus, opens RPC, or
//! emits an execution intent.  The V2 estimand is explicitly limited to the
//! typed Pump bonding-curve routes frozen in that manifest.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use ghost_brain::events::schema::{
    NewPoolDetectedPayload, PoolReserveStatePayload, PoolTransactionPayload,
};
use ghost_core::PumpReserveState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ace_core_one_day_probe as tape;
use crate::rug_reality_capture::{
    RugRealityCaptureHealthEvidenceV1, RugRealityCaptureRunManifestV1,
};
use crate::rug_scalp_v2::{reserves_after_buy, RugScalpPumpQuoteContractV1};

pub const ACE_EV_V2_CONTRACT_SCHEMA: &str = "ace_ev_v2_contract_v1";
pub const ACE_EV_V2_FEATURE_SCALE_SCHEMA: &str = "ace_ev_v2_feature_scale_v1";
pub const ACE_EV_V2_OUTCOME_SCHEMA: &str = "ace_ev_v2_candidate_outcome_v1";
pub const ACE_EV_V2_SCREENING_SCHEMA: &str = "ace_ev_v2_screening_v1";
pub const ACE_EV_V2_SUMMARY_SCHEMA: &str = "ace_ev_v2_summary_v1";
pub const ACE_EV_V2_PROSPECTIVE_AMENDMENT_SCHEMA: &str = "ace_ev_v2_prospective_1000_amendment_v1";
pub const ACE_EV_V2_PROSPECTIVE_STOP_EVIDENCE_SCHEMA: &str =
    "ace_ev_v2_prospective_stop_evidence_v1";
pub const ACE_EV_V2_ESTIMAND_ID: &str = "pump_bonding_curve_only_zero_recovery_floor_v1";
pub const ACE_EV_V2_FEATURE_ORDER: [&str; 7] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7"];

const FEATURE_CUTOFF_MS: u64 = 11_111;
const ENTRY_LATENCY_MS: u64 = 250;
const ENTRY_LANDING_MAX_LAG_MS: u64 = 1_000;
const EXIT_LATENCY_MS: u64 = 250;
const EXIT_LANDING_MAX_LAG_MS: u64 = 1_000;
const MAX_HOLD_MS: u64 = 120_000;
const ENTRY_TOTAL_WALLET_DEBIT_CAP_LAMPORTS: u64 = 150_000_000;
const MAX_ENTRY_IMPACT_BPS: u32 = 500;
const MAX_IMMEDIATE_EXIT_IMPACT_BPS: u32 = 500;
const HARD_LOSS_BPS: i64 = -3_000;
const TAKE_PROFIT_BPS: u64 = 1_700;
const MAX_TAKE_PROFIT_ATTEMPTS: u8 = 3;
const QUALIFICATION_ENROLLMENT_MS: u64 = 3_600_000;
const OUTCOME_DRAIN_MS: u64 = 150_000;
const PROSPECTIVE_MAX_ENROLLMENT_MS: u64 = 21_600_000;
const QUALIFICATION_MIN_TERMINAL_OUTCOMES: usize = 12;
const QUALIFICATION_MIN_SUCCESSFUL_ENTRIES_WITH_TERMINAL_EXIT: usize = 6;
const QUALIFICATION_ENROLLMENT_LIMIT: usize = 250;
const PROSPECTIVE_ENROLLMENT_LIMIT: usize = 1_000;
const TRAIN_ROWS: usize = 100;
const THRESHOLD_CALIBRATION_ROWS: usize = 50;
const UNTOUCHED_TEST_ROWS: usize = 100;
const PROSPECTIVE_TRAIN_ROWS: usize = 400;
const PROSPECTIVE_THRESHOLD_ROWS: usize = 200;
const PROSPECTIVE_UNTOUCHED_TEST_ROWS: usize = 400;
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

const FEATURE_SCALE_FILE: &str = "feature_scale_v1.json";
const SCREENING_FILE: &str = "candidate_screening_v2.jsonl";
const OUTCOMES_FILE: &str = "candidate_outcomes_v2.jsonl";
const SUMMARY_FILE: &str = "summary_v2.json";

#[derive(Debug, Clone)]
pub struct AceEvV2FreezeScaleArgs {
    pub events_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub checkpoint_manifest_path: PathBuf,
    pub contract_path: PathBuf,
    pub output_dir: PathBuf,
    /// Explicit functional source revision of this offline evaluator. The
    /// binary intentionally does not inspect mutable workspace Git state.
    pub offline_evaluator_source_sha: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AceEvV2CaptureKind {
    YieldQualification,
    #[serde(rename = "prospective_1000")]
    Prospective1000,
}

impl std::str::FromStr for AceEvV2CaptureKind {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "yield_qualification" => Ok(Self::YieldQualification),
            "prospective_1000" => Ok(Self::Prospective1000),
            other => {
                bail!("--capture-kind must be yield_qualification or prospective_1000, got {other}")
            }
        }
    }
}

impl AceEvV2CaptureKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::YieldQualification => "yield_qualification",
            Self::Prospective1000 => "prospective_1000",
        }
    }

    const fn health_capture_kind(self) -> &'static str {
        match self {
            Self::YieldQualification => "yield_qualification",
            Self::Prospective1000 => "prospective",
        }
    }

    const fn enrollment_window_ms(self) -> u64 {
        match self {
            Self::YieldQualification => QUALIFICATION_ENROLLMENT_MS,
            Self::Prospective1000 => PROSPECTIVE_MAX_ENROLLMENT_MS,
        }
    }

    const fn enrollment_limit(self) -> usize {
        match self {
            Self::YieldQualification => QUALIFICATION_ENROLLMENT_LIMIT,
            Self::Prospective1000 => PROSPECTIVE_ENROLLMENT_LIMIT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AceEvV2EvaluateArgs {
    pub events_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub contract_path: PathBuf,
    pub feature_scale_path: PathBuf,
    pub output_dir: PathBuf,
    pub capture_kind: AceEvV2CaptureKind,
    /// Required only for `prospective_1000`; it is a separately frozen
    /// outcome-blind amendment and is never inferred from mutable CLI state.
    pub amendment_path: Option<PathBuf>,
    /// Required only for a target-reached prospective final evaluation.
    pub stop_evidence_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AceEvV2MonitorArgs {
    pub events_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub contract_path: PathBuf,
    pub amendment_path: PathBuf,
    pub feature_scale_path: PathBuf,
    /// Immutable, manifest-bound start snapshot written by the supervisor.
    pub start_metrics_path: PathBuf,
    pub stop_evidence_path: PathBuf,
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceEvV2ProspectiveAmendmentV1 {
    pub schema: String,
    pub amendment_id: String,
    pub base_contract_sha256: String,
    pub target_terminal_outcomes: usize,
    pub train_rows: usize,
    pub threshold_calibration_rows: usize,
    pub untouched_test_rows: usize,
    pub max_enrollment_ms: u64,
    pub outcome_drain_ms: u64,
    pub test_min_entry_filled: usize,
    pub test_min_exit_filled: usize,
    pub positive_min_selected: usize,
    pub positive_min_selected_entry_filled: usize,
    pub positive_min_selected_exit_filled: usize,
    pub positive_max_top1_positive_pnl_share: f64,
    pub positive_max_top3_positive_pnl_share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AceEvV2ProspectiveStopEvidenceV1 {
    pub schema: String,
    pub run_id: String,
    pub manifest_sha256: String,
    pub base_contract_sha256: String,
    pub amendment_sha256: String,
    pub feature_scale_sha256: String,
    pub implementation_sha: String,
    pub target_terminal_outcomes: usize,
    pub terminal_outcome_count: usize,
    pub cohort_candidate_order_sha256: String,
    pub complete_file_prefixes: Vec<tape::TapeCompletePrefixV1>,
    pub stop_captured_at_unix_ms: u64,
    pub monitor_binary_blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceEvV2FeatureScaleFeatureV1 {
    pub id: String,
    pub winsor_p01: f64,
    pub winsor_p99: f64,
    pub median: f64,
    pub iqr: f64,
    pub missing_count: usize,
    pub missing_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceEvV2FeatureScaleV1 {
    pub schema: String,
    pub feature_order: [String; 7],
    pub estimand_id: String,
    pub raw_transform_order: String,
    pub log_base: String,
    pub quantile_method: String,
    pub iqr_zero_policy: String,
    pub non_finite_policy: String,
    pub source_run_id: String,
    pub source_manifest_sha256: String,
    pub source_checkpoint_manifest_sha256: String,
    pub source_checkpoint_wall_clock_utc: String,
    pub source_capture_head: String,
    pub source_implementation_sha: String,
    pub offline_evaluator_source_sha: String,
    pub contract_sha256: String,
    pub population_count: usize,
    pub source_candidate_count: usize,
    pub excluded_by_reason: BTreeMap<String, usize>,
    pub features: [AceEvV2FeatureScaleFeatureV1; 7],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AceEvV2FeatureVectorV1 {
    pub f1_log_unique_first_buyers: f64,
    pub f2_log_total_first_buy_flow: f64,
    pub f3_buyer_acceleration: f64,
    pub f4_creator_buy_share: f64,
    pub f5_first_buy_flow_hhi: f64,
    pub f6_same_slot_first_buy_flow_share: f64,
    pub f7_pre_cutoff_sell_buy_log_ratio: f64,
}

impl AceEvV2FeatureVectorV1 {
    pub const fn values(self) -> [f64; 7] {
        [
            self.f1_log_unique_first_buyers,
            self.f2_log_total_first_buy_flow,
            self.f3_buyer_acceleration,
            self.f4_creator_buy_share,
            self.f5_first_buy_flow_hhi,
            self.f6_same_slot_first_buy_flow_share,
            self.f7_pre_cutoff_sell_buy_log_ratio,
        ]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AceEvV2ScreeningStatus {
    PreEntryEligible,
    PreEntryNonEvaluable,
    PreEntryCreatorSellReject,
    NotEnrolledCohortClosed,
    InvalidCapture,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceEvV2ScreeningRowV1 {
    pub schema: String,
    pub candidate_id: String,
    pub base_mint: String,
    pub bonding_curve: String,
    pub creator: String,
    pub candidate_order: Option<CandidateOrderV1>,
    pub feature_vector: Option<AceEvV2FeatureVectorV1>,
    pub normalized_features: Option<[f64; 7]>,
    pub status: AceEvV2ScreeningStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct CandidateOrderV1 {
    pub decision_ingress_cutoff_ms: u64,
    pub birth_ts_ms: u64,
    pub event_slot: u64,
    pub bonding_curve: String,
    pub base_mint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalTerminalStatus {
    EntryFilledExitFilled,
    EntryFailedPriceProtection,
    EntryFailedNoLandingState,
    PostEntryValidityBoundLossFloor,
    PostEntryUnsupportedRouteLossFloor,
    ExitStateUnavailableLossFloor,
}

impl InternalTerminalStatus {
    /// Whether the fixed BuyV2 instruction reached a landed state and passed
    /// its frozen price-protection contract.  This is recorded separately from
    /// the terminal exit status so downstream analysis never has to infer an
    /// entry fill from a loss-floor outcome.
    const fn entry_status(self) -> &'static str {
        match self {
            Self::EntryFailedPriceProtection => "ENTRY_FAILED_PRICE_PROTECTION",
            Self::EntryFailedNoLandingState => "ENTRY_FAILED_NO_LANDING_STATE",
            Self::EntryFilledExitFilled
            | Self::PostEntryValidityBoundLossFloor
            | Self::PostEntryUnsupportedRouteLossFloor
            | Self::ExitStateUnavailableLossFloor => "ENTRY_FILLED",
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::EntryFilledExitFilled => "EXIT_FILLED",
            Self::EntryFailedPriceProtection => "ENTRY_FAILED_PRICE_PROTECTION",
            Self::EntryFailedNoLandingState => "ENTRY_FAILED_NO_LANDING_STATE",
            Self::PostEntryValidityBoundLossFloor => "POST_ENTRY_VALIDITY_BOUND_LOSS_FLOOR",
            Self::PostEntryUnsupportedRouteLossFloor => "POST_ENTRY_UNSUPPORTED_ROUTE_LOSS_FLOOR",
            Self::ExitStateUnavailableLossFloor => "EXIT_STATE_UNAVAILABLE_LOSS_FLOOR",
        }
    }

    const fn successful_entry(self) -> bool {
        matches!(
            self,
            Self::EntryFilledExitFilled
                | Self::PostEntryValidityBoundLossFloor
                | Self::PostEntryUnsupportedRouteLossFloor
                | Self::ExitStateUnavailableLossFloor
        )
    }

    const fn terminal_exit_filled(self) -> bool {
        matches!(self, Self::EntryFilledExitFilled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceEvV2StressOutcomeV1 {
    pub entry_latency_ms: u64,
    pub exit_latency_ms: u64,
    pub terminal_status: String,
    pub terminal_net_pnl_lamports: i128,
    pub terminal_net_pnl_sol: f64,
    pub profit17_hit: bool,
    pub exit_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceEvV2CandidateOutcomeV1 {
    pub schema: String,
    pub estimand_id: String,
    pub enrollment_index: usize,
    pub split: String,
    pub candidate_id: String,
    pub base_mint: String,
    pub bonding_curve: String,
    pub creator: String,
    pub candidate_order: CandidateOrderV1,
    pub birth_ts_ms: u64,
    pub decision_event_cutoff_ms: u64,
    pub decision_ingress_cutoff_ms: u64,
    pub feature_vector: AceEvV2FeatureVectorV1,
    pub normalized_features: [f64; 7],
    pub decision_state_slot: u64,
    pub entry_landing_state_slot: Option<u64>,
    pub entry_status: String,
    pub fixed_token_amount_raw: u64,
    pub fixed_max_sol_cost_lamports: u64,
    pub entry_total_debit_lamports: Option<u64>,
    pub entry_impact_bps: Option<u32>,
    pub immediate_exit_impact_bps: Option<u32>,
    pub entry_landed_arrival_ts_ms: Option<u64>,
    pub terminal_status: String,
    pub terminal_status_subtype: Option<String>,
    pub terminal_net_pnl_lamports: i128,
    pub terminal_net_pnl_sol: f64,
    pub terminal_net_return: Option<f64>,
    pub profit17_hit: bool,
    pub exit_reason: String,
    pub exit_trigger_event_ts_ms: Option<u64>,
    pub exit_trigger_arrival_ts_ms: Option<u64>,
    pub exit_landing_state_slot: Option<u64>,
    pub failed_take_profit_attempts: u8,
    pub cumulative_failed_exit_cost_lamports: u64,
    pub post_entry_route_loss: bool,
    pub stress_latency_1s: AceEvV2StressOutcomeV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceEvV2SummaryV1 {
    pub schema: String,
    pub capture_kind: AceEvV2CaptureKind,
    pub capture_status: String,
    pub capture_invalid_reasons: Vec<String>,
    pub terminal_status: String,
    pub run_id: String,
    pub baseline_sha: String,
    pub implementation_sha: String,
    pub code_hash: String,
    pub binary_hash: String,
    pub feature_scale_sha256: String,
    pub contract_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prospective_amendment_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prospective_stop_evidence_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohort_candidate_order_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prospective_terminalization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_outcomes_sha256: Option<String>,
    pub total_canonical_births: usize,
    pub pre_entry_eligible_count: usize,
    pub enrollment_closed: bool,
    pub enrolled_count: usize,
    pub terminal_outcome_count: usize,
    pub successful_entry_count: usize,
    pub successful_entry_with_terminal_exit_count: usize,
    pub direct_post_120s_state_count: usize,
    pub terminal_status_counts: BTreeMap<String, usize>,
    pub screening_reason_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct FeatureBuy {
    event_ts_ms: u64,
    arrival_ts_ms: u64,
    order: tape::CanonicalTradeOrder,
    signer: String,
    debit_lamports: u64,
    file_ordinal: usize,
    line_number: usize,
}

#[derive(Debug, Clone, Copy)]
struct DirectState {
    event_ts_ms: u64,
    arrival_ts_ms: u64,
    slot: u64,
    write_version: Option<u64>,
    sequence_number: u64,
    ordinal: (usize, usize),
    complete: bool,
    reserves: PumpReserveState,
}

#[derive(Debug, Clone)]
struct PreEntryPlan {
    birth: tape::TapeBirth,
    trades: Vec<tape::TapeTrade>,
    states: Vec<DirectState>,
    cutoffs: tape::DecisionCutoffs,
    features: AceEvV2FeatureVectorV1,
    normalized_features: [f64; 7],
    decision_state: DirectState,
    fixed_token_amount_raw: u64,
    fixed_max_sol_cost_lamports: u64,
    decision_entry_total_debit_lamports: u64,
    decision_entry_impact_bps: u32,
    decision_immediate_exit_impact_bps: u32,
}

#[derive(Debug, Clone, Copy)]
struct TerminalOutcome {
    status: InternalTerminalStatus,
    subtype: Option<&'static str>,
    terminal_net_pnl_lamports: i128,
    terminal_net_return: Option<f64>,
    profit17_hit: bool,
    entry_landing: Option<DirectState>,
    entry_total_debit_lamports: Option<u64>,
    entry_impact_bps: Option<u32>,
    immediate_exit_impact_bps: Option<u32>,
    exit_reason: &'static str,
    exit_trigger_event_ts_ms: Option<u64>,
    exit_trigger_arrival_ts_ms: Option<u64>,
    exit_landing_state_slot: Option<u64>,
    failed_take_profit_attempts: u8,
    cumulative_failed_exit_cost_lamports: u64,
    post_entry_route_loss: bool,
}

/// Freeze an outcome-blind F1–F7 scale from a valid checkpoint's frozen tape.
/// The scale reader intentionally decodes only births and PoolTransaction rows;
/// it never materializes reserve states, quotes, entries, exits, PnL, returns,
/// or hit-rate fields.
pub fn freeze_feature_scale(args: AceEvV2FreezeScaleArgs) -> Result<AceEvV2FeatureScaleV1> {
    create_output_dir_new(&args.output_dir)?;
    if args.offline_evaluator_source_sha.len() != 40
        || !args
            .offline_evaluator_source_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("offline_evaluator_source_sha must be a 40-character Git SHA")
    }
    let contract_bytes = load_contract_bytes(&args.contract_path)?;
    let contract_sha256 = sha256_hex(&contract_bytes);
    let manifest_bytes = fs::read(&args.manifest_path)
        .with_context(|| format!("read capture manifest {}", args.manifest_path.display()))?;
    let manifest: RugRealityCaptureRunManifestV1 = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("decode capture manifest {}", args.manifest_path.display()))?;
    validate_checkpoint_for_scale(
        &args.checkpoint_manifest_path,
        &args.events_dir,
        &manifest,
        &manifest_bytes,
    )?;

    let mut tape = read_feature_scale_tape(&args.events_dir, &manifest.run_id)?;
    let (births, _) = tape::canonical_births(&mut tape);
    let canonical_birth_keys = births
        .iter()
        .filter_map(|birth| match tape::birth_key(&birth.payload) {
            tape::BirthKeyResolution::Eligible(key) => Some(key),
            tape::BirthKeyResolution::OutsideUniverse | tape::BirthKeyResolution::Malformed(_) => {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    let trades = tape::strict_trade_index(&mut tape, &canonical_birth_keys);
    if !tape.invalid_reasons.is_empty() {
        bail!(
            "feature-scale checkpoint tape is invalid: {}",
            tape.invalid_reasons
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let mut rows = Vec::<AceEvV2FeatureVectorV1>::new();
    let mut excluded_by_reason = BTreeMap::<String, usize>::new();
    for birth in births {
        let tape::BirthKeyResolution::Eligible(key) = tape::birth_key(&birth.payload) else {
            *excluded_by_reason
                .entry("birth_not_feature_scale_universe".to_string())
                .or_default() += 1;
            continue;
        };
        let candidate_trades = trades.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        match calculate_features_v2(&birth, candidate_trades) {
            Ok((features, _)) => match has_pre_entry_creator_sell_veto(&birth, candidate_trades)
                .map_err(|reason| anyhow!(reason))?
            {
                false => rows.push(features),
                true => {
                    *excluded_by_reason
                        .entry("pre_entry_creator_sell_veto".to_string())
                        .or_default() += 1;
                }
            },
            Err(reason) => *excluded_by_reason.entry(reason).or_default() += 1,
        }
    }
    if rows.is_empty() {
        bail!("feature-scale population is empty")
    }
    let scale = build_feature_scale(
        rows,
        births_count_for_scale(&args.events_dir, &manifest.run_id)?,
        excluded_by_reason,
        &manifest,
        &manifest_bytes,
        &args.checkpoint_manifest_path,
        args.offline_evaluator_source_sha,
        contract_sha256,
    )?;
    write_json_new(&args.output_dir.join(FEATURE_SCALE_FILE), &scale)?;
    Ok(scale)
}

fn create_output_dir_new(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create ACE-EV V2 output parent {}", parent.display()))?;
    fs::create_dir(path).with_context(|| {
        format!(
            "create new ACE-EV V2 output directory {} (existing output is forbidden)",
            path.display()
        )
    })
}

fn load_contract_bytes(path: &Path) -> Result<Vec<u8>> {
    let bytes =
        fs::read(path).with_context(|| format!("read ACE-EV V2 contract {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode ACE-EV V2 contract {}", path.display()))?;
    if value.get("schema").and_then(Value::as_str) != Some(ACE_EV_V2_CONTRACT_SCHEMA) {
        bail!("ACE-EV V2 contract schema mismatch")
    }
    if value.get("estimand_id").and_then(Value::as_str) != Some(ACE_EV_V2_ESTIMAND_ID) {
        bail!("ACE-EV V2 contract estimand_id mismatch")
    }
    validate_frozen_contract_semantics(&value)?;
    Ok(bytes)
}

fn load_prospective_amendment(
    path: &Path,
    base_contract_sha256: &str,
) -> Result<(AceEvV2ProspectiveAmendmentV1, String)> {
    let bytes = fs::read(path)
        .with_context(|| format!("read ACE-EV V2 prospective amendment {}", path.display()))?;
    let amendment: AceEvV2ProspectiveAmendmentV1 = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode ACE-EV V2 prospective amendment {}", path.display()))?;
    if amendment.schema != ACE_EV_V2_PROSPECTIVE_AMENDMENT_SCHEMA
        || amendment.amendment_id != "ACE_EV_V2_PROSPECTIVE_1000"
        || amendment.base_contract_sha256 != base_contract_sha256
        || amendment.target_terminal_outcomes != PROSPECTIVE_ENROLLMENT_LIMIT
        || amendment.train_rows != PROSPECTIVE_TRAIN_ROWS
        || amendment.threshold_calibration_rows != PROSPECTIVE_THRESHOLD_ROWS
        || amendment.untouched_test_rows != PROSPECTIVE_UNTOUCHED_TEST_ROWS
        || amendment.train_rows
            + amendment.threshold_calibration_rows
            + amendment.untouched_test_rows
            != amendment.target_terminal_outcomes
        || amendment.max_enrollment_ms != PROSPECTIVE_MAX_ENROLLMENT_MS
        || amendment.outcome_drain_ms != OUTCOME_DRAIN_MS
        || amendment.test_min_entry_filled != 60
        || amendment.test_min_exit_filled != 25
        || amendment.positive_min_selected != 80
        || amendment.positive_min_selected_entry_filled != 20
        || amendment.positive_min_selected_exit_filled != 10
        || amendment.positive_max_top1_positive_pnl_share != 0.25
        || amendment.positive_max_top3_positive_pnl_share != 0.50
    {
        bail!("ACE-EV V2 prospective amendment contract mismatch")
    }
    Ok((amendment, sha256_hex(&bytes)))
}

fn load_monitor_start_snapshot(
    path: &Path,
    manifest_path: &Path,
    manifest: &RugRealityCaptureRunManifestV1,
) -> Result<u64> {
    let bytes = fs::read(path)
        .with_context(|| format!("read prospective start snapshot {}", path.display()))?;
    let snapshot: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode prospective start snapshot {}", path.display()))?;
    let manifest_sha256 = sha256_hex(
        &fs::read(manifest_path)
            .with_context(|| format!("read prospective manifest {}", manifest_path.display()))?,
    );
    if snapshot.pointer("/schema_version").and_then(Value::as_u64) != Some(1)
        || snapshot.pointer("/capture_kind").and_then(Value::as_str) != Some("prospective")
        || snapshot.pointer("/run_id").and_then(Value::as_str) != Some(manifest.run_id.as_str())
        || snapshot.pointer("/manifest_sha256").and_then(Value::as_str)
            != Some(manifest_sha256.as_str())
    {
        bail!("prospective start snapshot provenance mismatch")
    }
    snapshot
        .pointer("/captured_at_unix_ms")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow!("prospective start snapshot timestamp missing"))
}

fn validate_frozen_contract_semantics(contract: &Value) -> Result<()> {
    let required = [
        (
            "/mode",
            serde_json::json!("offline_observed_path_non_propagated_executable_proxy"),
        ),
        ("/live_execution", serde_json::json!(false)),
        ("/typed_pumpswap_route", serde_json::json!(false)),
        (
            "/feature_contract/cutoff_ms",
            serde_json::json!(FEATURE_CUTOFF_MS),
        ),
        (
            "/feature_contract/feature_order",
            serde_json::json!(ACE_EV_V2_FEATURE_ORDER),
        ),
        (
            "/feature_scale/quantile_method",
            serde_json::json!("quantile_linear"),
        ),
        ("/feature_scale/epsilon_fallback", serde_json::json!(false)),
        ("/pre_entry/required_route", serde_json::json!("BuyV2")),
        (
            "/pre_entry/required_state",
            serde_json::json!("direct PrimaryAuthority PoolReserveState"),
        ),
        (
            "/pre_entry/wallet_debit_cap_lamports",
            serde_json::json!(ENTRY_TOTAL_WALLET_DEBIT_CAP_LAMPORTS),
        ),
        (
            "/pre_entry/entry_impact_bps_max",
            serde_json::json!(MAX_ENTRY_IMPACT_BPS),
        ),
        (
            "/pre_entry/immediate_exit_impact_bps_max",
            serde_json::json!(MAX_IMMEDIATE_EXIT_IMPACT_BPS),
        ),
        ("/entry/latency_ms", serde_json::json!(ENTRY_LATENCY_MS)),
        ("/exit/latency_ms", serde_json::json!(EXIT_LATENCY_MS)),
        ("/exit/max_hold_ms", serde_json::json!(MAX_HOLD_MS)),
        ("/exit/hard_loss_net_return_lte", serde_json::json!(-0.30)),
        ("/exit/take_profit_net_return_gte", serde_json::json!(0.17)),
        (
            "/exit/take_profit_max_attempts",
            serde_json::json!(MAX_TAKE_PROFIT_ATTEMPTS),
        ),
        (
            "/enrollment/limit",
            serde_json::json!(QUALIFICATION_ENROLLMENT_LIMIT),
        ),
        (
            "/enrollment/outcome_drain_ms",
            serde_json::json!(OUTCOME_DRAIN_MS),
        ),
        (
            "/qualification/enrollment_duration_ms",
            serde_json::json!(QUALIFICATION_ENROLLMENT_MS),
        ),
        (
            "/qualification/outcome_drain_ms",
            serde_json::json!(OUTCOME_DRAIN_MS),
        ),
        ("/model/kind", serde_json::json!("HuberRegressor")),
        ("/model/target", serde_json::json!("terminal_net_pnl_sol")),
        (
            "/model/prediction",
            serde_json::json!("predicted_robust_net_pnl_sol"),
        ),
        ("/model/epsilon", serde_json::json!(1.35)),
        ("/model/alpha", serde_json::json!(1.0)),
        ("/model/fit_intercept", serde_json::json!(true)),
        ("/model/tol", serde_json::json!(0.00001)),
        ("/model/max_iter", serde_json::json!(1000)),
        ("/model/warm_start", serde_json::json!(false)),
        ("/model/blas_threads", serde_json::json!(1)),
        ("/stress/entry_latency_ms", serde_json::json!(1000)),
        ("/stress/exit_latency_ms", serde_json::json!(1000)),
    ];
    for (pointer, expected) in required {
        if contract.pointer(pointer) != Some(&expected) {
            bail!("ACE-EV V2 frozen contract mismatch at {pointer}")
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn collect_event_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        bail!("events directory is not a directory: {}", root.display())
    }
    let mut files = Vec::new();
    collect_event_files_inner(root, &mut files)?;
    files.sort();
    if files.is_empty() {
        bail!(
            "events directory contains no exec_*.jsonl files: {}",
            root.display()
        )
    }
    Ok(files)
}

fn collect_event_files_inner(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry.with_context(|| format!("read entry below {}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_event_files_inner(&path, files)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("exec_") && name.ends_with(".jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

/// Reads a narrow, outcome-blind tape projection.  In particular, a
/// `PoolReserveState` payload is never deserialized or retained here.
fn read_feature_scale_tape(events_dir: &Path, expected_run_id: &str) -> Result<tape::Tape> {
    let files = collect_event_files(events_dir)?;
    let mut tape_rows = tape::Tape::default();
    for (file_ordinal, path) in files.iter().enumerate() {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut line_number = 0usize;
        loop {
            line.clear();
            let byte_count = reader
                .read_line(&mut line)
                .with_context(|| format!("read {}", path.display()))?;
            if byte_count == 0 {
                break;
            }
            line_number += 1;
            if !line.ends_with('\n') {
                tape_rows
                    .invalid_reasons
                    .insert("event_writer_not_cleanly_flushed".to_string());
                continue;
            }
            if line.trim().is_empty() {
                tape_rows
                    .invalid_reasons
                    .insert("event_jsonl_blank_line".to_string());
                continue;
            }
            let value: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => {
                    tape_rows
                        .invalid_reasons
                        .insert("event_jsonl_decode_failed".to_string());
                    continue;
                }
            };
            let Some(envelope) = value.get("envelope") else {
                tape_rows
                    .invalid_reasons
                    .insert("event_envelope_missing".to_string());
                continue;
            };
            let run_id = envelope.get("run_id").and_then(Value::as_str);
            let lane = envelope.get("lane").and_then(Value::as_str);
            if run_id != Some(expected_run_id) {
                tape_rows
                    .invalid_reasons
                    .insert("manifest_run_id_does_not_match_event_tape".to_string());
                continue;
            }
            if lane != Some("shadow") {
                tape_rows
                    .invalid_reasons
                    .insert("event_tape_lane_is_not_shadow".to_string());
                continue;
            }
            let Some(kind) = value.get("kind") else {
                tape_rows
                    .invalid_reasons
                    .insert("event_kind_missing".to_string());
                continue;
            };
            let Some(kind_name) = kind.get("type").and_then(Value::as_str) else {
                tape_rows
                    .invalid_reasons
                    .insert("event_kind_type_missing".to_string());
                continue;
            };
            let candidate_id = envelope
                .get("candidate_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let event_id = envelope
                .get("event_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match kind_name {
                "NewPoolDetected" => match kind.get("payload").cloned() {
                    Some(payload) => {
                        match serde_json::from_value::<NewPoolDetectedPayload>(payload) {
                            Ok(payload) => tape_rows.births.push(tape::TapeBirth {
                                candidate_id,
                                payload,
                                event_id,
                                file_ordinal,
                                line_number,
                            }),
                            Err(_) => {
                                tape_rows
                                    .invalid_reasons
                                    .insert("new_pool_detected_payload_decode_failed".to_string());
                            }
                        }
                    }
                    None => {
                        tape_rows
                            .invalid_reasons
                            .insert("new_pool_detected_payload_missing".to_string());
                    }
                },
                "PoolTransaction" => match kind.get("payload").cloned() {
                    Some(payload) => {
                        match serde_json::from_value::<PoolTransactionPayload>(payload) {
                            Ok(payload) => tape_rows.trades.push(tape::TapeTrade {
                                payload,
                                event_id,
                                file_ordinal,
                                line_number,
                            }),
                            Err(_) => {
                                tape_rows
                                    .invalid_reasons
                                    .insert("pool_transaction_payload_decode_failed".to_string());
                            }
                        }
                    }
                    None => {
                        tape_rows
                            .invalid_reasons
                            .insert("pool_transaction_payload_missing".to_string());
                    }
                },
                // Deliberately ignore all other event types without
                // deserializing their payloads.  This enforces the
                // outcome-blind feature-scale boundary.
                _ => {}
            }
        }
    }
    Ok(tape_rows)
}

fn validate_checkpoint_for_scale(
    checkpoint_manifest_path: &Path,
    events_dir: &Path,
    capture_manifest: &RugRealityCaptureRunManifestV1,
    capture_manifest_bytes: &[u8],
) -> Result<()> {
    let checkpoint_bytes = fs::read(checkpoint_manifest_path).with_context(|| {
        format!(
            "read outcome-blind checkpoint manifest {}",
            checkpoint_manifest_path.display()
        )
    })?;
    let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes).with_context(|| {
        format!(
            "decode outcome-blind checkpoint manifest {}",
            checkpoint_manifest_path.display()
        )
    })?;
    if checkpoint.get("schema").and_then(Value::as_str) != Some("ace_14h_checkpoint_manifest_v1") {
        bail!("feature-scale source is not an ace_14h_checkpoint_manifest_v1")
    }
    if checkpoint
        .get("capture_status_at_checkpoint")
        .and_then(Value::as_str)
        != Some("VALID_CAPTURE")
    {
        bail!("feature-scale source checkpoint was not valid")
    }
    let Some(provenance) = checkpoint.get("capture_provenance") else {
        bail!("feature-scale checkpoint capture_provenance missing")
    };
    if provenance
        .get("event_writer_run_id")
        .and_then(Value::as_str)
        != Some(&capture_manifest.run_id)
    {
        bail!("feature-scale checkpoint run_id does not match capture manifest")
    }
    let expected_manifest_hash = sha256_hex(capture_manifest_bytes);
    let recorded_manifest_hash = checkpoint
        .pointer("/source_artifacts/source_manifest_sha256")
        .and_then(Value::as_str);
    if recorded_manifest_hash != Some(expected_manifest_hash.as_str()) {
        bail!("feature-scale checkpoint source manifest hash mismatch")
    }
    for name in [
        "pr1_runtime_bypass_attempt_total",
        "pr1_runtime_candidate_admission_closed_total",
        "pr1_runtime_primary_coverage_gap_total",
        "ace_capture_segment_invalid_total",
    ] {
        if checkpoint
            .pointer(&format!("/critical_counters/{name}"))
            .and_then(Value::as_u64)
            != Some(0)
        {
            bail!("feature-scale checkpoint critical counter is non-zero or absent: {name}")
        }
    }
    let index_name = checkpoint
        .pointer("/source_artifacts/maximum_offsets_index")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("feature-scale checkpoint file index is missing"))?;
    let index_path = checkpoint_manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(index_name);
    let index_bytes = fs::read(&index_path)
        .with_context(|| format!("read frozen checkpoint file index {}", index_path.display()))?;
    let index: Value = serde_json::from_slice(&index_bytes).with_context(|| {
        format!(
            "decode frozen checkpoint file index {}",
            index_path.display()
        )
    })?;
    let files = index
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("feature-scale checkpoint file list missing"))?;
    let actual_files = collect_event_files(events_dir)?;
    let actual = actual_files
        .iter()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
        .collect::<BTreeSet<_>>();
    let mut indexed = BTreeSet::new();
    for file in files {
        let frozen_path = file
            .get("frozen_path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("checkpoint file has no frozen_path"))?;
        let expected_hash = file
            .get("frozen_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("checkpoint file has no frozen_sha256"))?;
        let path = PathBuf::from(frozen_path);
        let canonical = path.canonicalize().unwrap_or(path.clone());
        indexed.insert(canonical.clone());
        let bytes = fs::read(&path)
            .with_context(|| format!("read frozen checkpoint tape {}", path.display()))?;
        if sha256_hex(&bytes) != expected_hash {
            bail!(
                "feature-scale checkpoint frozen tape hash mismatch: {}",
                path.display()
            )
        }
    }
    if actual != indexed {
        bail!("feature-scale events-dir is not exactly the checkpoint frozen tape set")
    }
    Ok(())
}

fn births_count_for_scale(events_dir: &Path, expected_run_id: &str) -> Result<usize> {
    // This intentionally uses the narrow reader again.  It only supports the
    // source-population denominator in the scale artifact and cannot expose
    // reserve/outcome information to the scale calculation.
    let mut tape_rows = read_feature_scale_tape(events_dir, expected_run_id)?;
    let (births, _) = tape::canonical_births(&mut tape_rows);
    Ok(births.len())
}

fn build_feature_scale(
    rows: Vec<AceEvV2FeatureVectorV1>,
    source_candidate_count: usize,
    excluded_by_reason: BTreeMap<String, usize>,
    manifest: &RugRealityCaptureRunManifestV1,
    manifest_bytes: &[u8],
    checkpoint_manifest_path: &Path,
    offline_evaluator_source_sha: String,
    contract_sha256: String,
) -> Result<AceEvV2FeatureScaleV1> {
    let population_count = rows.len();
    let mut columns: [Vec<f64>; 7] =
        std::array::from_fn(|_| Vec::<f64>::with_capacity(population_count));
    for row in rows {
        for (index, value) in row.values().into_iter().enumerate() {
            if !value.is_finite() {
                bail!(
                    "FEATURE_SCALE_INVALID: non-finite raw feature {}",
                    ACE_EV_V2_FEATURE_ORDER[index]
                )
            }
            columns[index].push(value);
        }
    }
    let features: [Result<AceEvV2FeatureScaleFeatureV1>; 7] =
        std::array::from_fn(|index| -> Result<AceEvV2FeatureScaleFeatureV1> {
            let mut sorted = columns[index].clone();
            sorted.sort_by(f64::total_cmp);
            let winsor_p01 = quantile_linear(&sorted, 0.01)?;
            let winsor_p99 = quantile_linear(&sorted, 0.99)?;
            if !winsor_p01.is_finite() || !winsor_p99.is_finite() || winsor_p01 > winsor_p99 {
                bail!(
                    "FEATURE_SCALE_INVALID: invalid winsor bounds for {}",
                    ACE_EV_V2_FEATURE_ORDER[index]
                )
            }
            let mut winsorized = sorted
                .iter()
                .map(|value| value.clamp(winsor_p01, winsor_p99))
                .collect::<Vec<_>>();
            winsorized.sort_by(f64::total_cmp);
            let median = quantile_linear(&winsorized, 0.5)?;
            let q25 = quantile_linear(&winsorized, 0.25)?;
            let q75 = quantile_linear(&winsorized, 0.75)?;
            let iqr = q75 - q25;
            if !median.is_finite() || !iqr.is_finite() || iqr == 0.0 {
                bail!(
                    "FEATURE_SCALE_INVALID: IQR == 0 or non-finite for {}",
                    ACE_EV_V2_FEATURE_ORDER[index]
                )
            }
            Ok(AceEvV2FeatureScaleFeatureV1 {
                id: ACE_EV_V2_FEATURE_ORDER[index].to_string(),
                winsor_p01,
                winsor_p99,
                median,
                iqr,
                missing_count: source_candidate_count.saturating_sub(population_count),
                missing_rate: (source_candidate_count.saturating_sub(population_count)) as f64
                    / source_candidate_count.max(1) as f64,
            })
        });
    let features: [AceEvV2FeatureScaleFeatureV1; 7] = features
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow!("FEATURE_SCALE_INVALID: feature count mismatch"))?;
    let checkpoint_bytes = fs::read(checkpoint_manifest_path)?;
    let checkpoint: Value = serde_json::from_slice(&checkpoint_bytes)?;
    let source_checkpoint_wall_clock_utc = checkpoint
        .get("checkpoint_wall_clock_utc")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("checkpoint timestamp missing"))?
        .to_string();
    let source_capture_head = checkpoint
        .pointer("/capture_provenance/capture_head")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("checkpoint capture head missing"))?
        .to_string();
    Ok(AceEvV2FeatureScaleV1 {
        schema: ACE_EV_V2_FEATURE_SCALE_SCHEMA.to_string(),
        feature_order: ACE_EV_V2_FEATURE_ORDER.map(str::to_string),
        estimand_id: ACE_EV_V2_ESTIMAND_ID.to_string(),
        raw_transform_order: "raw_feature -> clamp[p01,p99] -> (value-median)/IQR".to_string(),
        log_base: "natural".to_string(),
        quantile_method: "quantile_linear".to_string(),
        iqr_zero_policy: "FEATURE_SCALE_INVALID".to_string(),
        non_finite_policy: "FEATURE_SCALE_INVALID".to_string(),
        source_run_id: manifest.run_id.clone(),
        source_manifest_sha256: sha256_hex(manifest_bytes),
        source_checkpoint_manifest_sha256: sha256_hex(&checkpoint_bytes),
        source_checkpoint_wall_clock_utc,
        source_capture_head,
        source_implementation_sha: manifest.implementation_sha.clone(),
        offline_evaluator_source_sha,
        contract_sha256,
        population_count,
        source_candidate_count,
        excluded_by_reason,
        features,
    })
}

fn quantile_linear(sorted: &[f64], percentile: f64) -> Result<f64> {
    if sorted.is_empty() || !(0.0..=1.0).contains(&percentile) {
        bail!("invalid quantile input")
    }
    if sorted.len() == 1 {
        return Ok(sorted[0]);
    }
    let position = percentile * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f64;
    Ok(sorted[lower] + (sorted[upper] - sorted[lower]) * fraction)
}

fn calculate_features_v2(
    birth: &tape::TapeBirth,
    trades: &[tape::TapeTrade],
) -> std::result::Result<(AceEvV2FeatureVectorV1, tape::DecisionCutoffs), String> {
    let creator = tape::non_empty(&birth.payload.creator)
        .ok_or_else(|| "birth_creator_identity_missing".to_string())?;
    let cutoffs = tape::decision_cutoffs(birth)?;
    let mut buys = Vec::<FeatureBuy>::new();
    let mut successful_sell_count = 0u64;
    for trade in trades {
        let payload = &trade.payload;
        if payload.event_ts_ms < birth.payload.birth_ts_ms
            || !available_before_cutoff(payload.event_ts_ms, payload.arrival_ts_ms, cutoffs)
            || !payload.success
            || payload.is_synthetic != Some(false)
        {
            continue;
        }
        let declares_buy = payload.is_buy || payload.side.eq_ignore_ascii_case("buy");
        if declares_buy {
            // A feature vector represents the complete successful BUY flow
            // known at decision time.  Keeping one well-formed BUY while
            // silently omitting another successful BUY would make F1-F6 and
            // F7 describe different populations.  Fail this candidate before
            // enrollment instead of manufacturing a partial-flow vector.
            let feature_evidence_incomplete =
                || "successful_buy_feature_evidence_incomplete".to_string();
            if !payload.is_buy || !payload.side.eq_ignore_ascii_case("buy") {
                return Err(feature_evidence_incomplete());
            }
            let order =
                tape::canonical_trade_order(payload).ok_or_else(feature_evidence_incomplete)?;
            let signer =
                tape::non_empty(&payload.signer).ok_or_else(feature_evidence_incomplete)?;
            if let Some(wallet) = tape::non_empty(&payload.wallet) {
                if wallet != signer {
                    return Err(feature_evidence_incomplete());
                }
            }
            let pre = payload
                .signer_pre_balance_lamports
                .ok_or_else(feature_evidence_incomplete)?;
            let post = payload
                .signer_post_balance_lamports
                .ok_or_else(feature_evidence_incomplete)?;
            let debit_lamports = pre
                .checked_sub(post)
                .filter(|debit| *debit > 0)
                .ok_or_else(feature_evidence_incomplete)?;
            buys.push(FeatureBuy {
                event_ts_ms: payload.event_ts_ms,
                arrival_ts_ms: payload.arrival_ts_ms,
                order,
                signer: signer.to_string(),
                debit_lamports,
                file_ordinal: trade.file_ordinal,
                line_number: trade.line_number,
            });
        } else if !payload.is_buy && payload.side.eq_ignore_ascii_case("sell") {
            successful_sell_count = successful_sell_count.saturating_add(1);
        }
    }
    if buys.is_empty() {
        return Err("successful_buy_wallet_debit_evidence_missing".to_string());
    }
    // Every retained BUY has just passed the same evidence contract used by
    // F1-F6, so F7's buy denominator is now provably drawn from that exact
    // same population.
    let successful_buy_count = buys.len() as u64;
    buys.sort_by(compare_feature_buy);
    let total_buy_debit = buys.iter().try_fold(0u64, |sum, buy| {
        sum.checked_add(buy.debit_lamports)
            .ok_or_else(|| "total_buy_wallet_debit_overflow".to_string())
    })?;
    let creator_buy_debit =
        buys.iter()
            .filter(|buy| buy.signer == creator)
            .try_fold(0u64, |sum, buy| {
                sum.checked_add(buy.debit_lamports)
                    .ok_or_else(|| "creator_buy_wallet_debit_overflow".to_string())
            })?;
    let mut first_buys = BTreeMap::<String, FeatureBuy>::new();
    for buy in buys {
        first_buys.entry(buy.signer.clone()).or_insert(buy);
    }
    let first_buys = first_buys.into_values().collect::<Vec<_>>();
    let total_first_buy_flow = first_buys.iter().try_fold(0u64, |sum, buy| {
        sum.checked_add(buy.debit_lamports)
            .ok_or_else(|| "total_first_buy_flow_overflow".to_string())
    })?;
    if total_first_buy_flow == 0 {
        return Err("total_first_buy_flow_non_positive".to_string());
    }
    let cutoff = cutoffs.event_cutoff_ts_ms;
    let buyers_last_3s = first_buys
        .iter()
        .filter(|buy| buy.event_ts_ms >= cutoff.saturating_sub(3_000))
        .count() as f64;
    let buyers_full_window = first_buys.len() as f64;
    let f3 = (((buyers_last_3s + 1.0) / (3.0 + 1.0))
        / ((buyers_full_window + 1.0) / (11.111 + 1.0)))
        .ln();
    let mut flow_by_slot = BTreeMap::<u64, u64>::new();
    for buy in &first_buys {
        *flow_by_slot.entry(buy.order.slot).or_default() = flow_by_slot
            .get(&buy.order.slot)
            .copied()
            .unwrap_or(0)
            .checked_add(buy.debit_lamports)
            .ok_or_else(|| "same_slot_first_buy_flow_overflow".to_string())?;
    }
    let max_same_slot_flow = flow_by_slot.values().copied().max().unwrap_or(0);
    let f5 = first_buys.iter().fold(0.0, |sum, buy| {
        let share = buy.debit_lamports as f64 / total_first_buy_flow as f64;
        sum + share * share
    });
    let features = AceEvV2FeatureVectorV1 {
        f1_log_unique_first_buyers: (1.0 + first_buys.len() as f64).ln(),
        f2_log_total_first_buy_flow: (1.0 + total_first_buy_flow as f64).ln(),
        f3_buyer_acceleration: f3,
        f4_creator_buy_share: creator_buy_debit as f64 / total_buy_debit as f64,
        f5_first_buy_flow_hhi: f5,
        f6_same_slot_first_buy_flow_share: max_same_slot_flow as f64 / total_first_buy_flow as f64,
        f7_pre_cutoff_sell_buy_log_ratio: ((successful_sell_count + 1) as f64
            / (successful_buy_count + 1) as f64)
            .ln(),
    };
    features
        .values()
        .into_iter()
        .all(f64::is_finite)
        .then_some((features, cutoffs))
        .ok_or_else(|| "feature_value_non_finite".to_string())
}

fn compare_feature_buy(left: &FeatureBuy, right: &FeatureBuy) -> Ordering {
    (
        left.order,
        left.event_ts_ms,
        left.arrival_ts_ms,
        left.file_ordinal,
        left.line_number,
    )
        .cmp(&(
            right.order,
            right.event_ts_ms,
            right.arrival_ts_ms,
            right.file_ordinal,
            right.line_number,
        ))
}

fn available_before_cutoff(
    event_ts_ms: u64,
    arrival_ts_ms: u64,
    cutoffs: tape::DecisionCutoffs,
) -> bool {
    arrival_ts_ms > 0
        && event_ts_ms <= cutoffs.event_cutoff_ts_ms
        && arrival_ts_ms <= cutoffs.ingress_cutoff_ts_ms
}

fn has_pre_entry_creator_sell_veto(
    birth: &tape::TapeBirth,
    trades: &[tape::TapeTrade],
) -> std::result::Result<bool, String> {
    let creator = tape::non_empty(&birth.payload.creator)
        .ok_or_else(|| "birth_creator_identity_missing".to_string())?;
    let cutoffs = tape::decision_cutoffs(birth)?;
    Ok(trades.iter().any(|trade| {
        let payload = &trade.payload;
        payload.success
            && !payload.is_buy
            && payload.side.eq_ignore_ascii_case("sell")
            && payload.is_synthetic == Some(false)
            && payload.event_ts_ms >= birth.payload.birth_ts_ms
            && available_before_cutoff(payload.event_ts_ms, payload.arrival_ts_ms, cutoffs)
            && tape::non_empty(&payload.signer) == Some(creator)
            && tape::canonical_trade_order(payload).is_some()
    }))
}

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .with_context(|| format!("serialize ACE-EV V2 artifact {}", path.display()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create ACE-EV V2 artifact {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write ACE-EV V2 artifact {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("terminate ACE-EV V2 artifact {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync ACE-EV V2 artifact {}", path.display()))?;
    Ok(())
}

/// Evaluate the frozen, predeclared V2 state machine from a completed capture.
/// The evaluator never changes the capture and enrollment is based exclusively
/// on pre-entry evidence sorted by `CandidateOrderV1`, never terminalization
/// speed.
pub fn run_ace_ev_v2_evaluator(args: AceEvV2EvaluateArgs) -> Result<AceEvV2SummaryV1> {
    create_output_dir_new(&args.output_dir)?;
    let contract_bytes = load_contract_bytes(&args.contract_path)?;
    let contract_sha256 = sha256_hex(&contract_bytes);
    let prospective_amendment = match args.capture_kind {
        AceEvV2CaptureKind::YieldQualification => {
            if args.amendment_path.is_some() || args.stop_evidence_path.is_some() {
                bail!("yield qualification must not receive prospective amendment or stop evidence")
            }
            None
        }
        AceEvV2CaptureKind::Prospective1000 => {
            let path = args
                .amendment_path
                .as_deref()
                .ok_or_else(|| anyhow!("prospective_1000 requires --amendment"))?;
            Some(load_prospective_amendment(path, &contract_sha256)?)
        }
    };
    let scale_bytes = fs::read(&args.feature_scale_path).with_context(|| {
        format!(
            "read ACE-EV V2 feature scale {}",
            args.feature_scale_path.display()
        )
    })?;
    let feature_scale: AceEvV2FeatureScaleV1 =
        serde_json::from_slice(&scale_bytes).with_context(|| {
            format!(
                "decode ACE-EV V2 feature scale {}",
                args.feature_scale_path.display()
            )
        })?;
    validate_feature_scale(&feature_scale, &contract_sha256)?;
    let feature_scale_sha256 = sha256_hex(&scale_bytes);
    let manifest_bytes = fs::read(&args.manifest_path)
        .with_context(|| format!("read capture manifest {}", args.manifest_path.display()))?;
    let manifest: RugRealityCaptureRunManifestV1 = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("decode capture manifest {}", args.manifest_path.display()))?;
    let mut invalid_reasons = validate_v2_capture_manifest(&args.manifest_path, &manifest);
    let health_receipt = match load_capture_health_receipt(&manifest) {
        Ok(receipt) => {
            if receipt.capture_kind != args.capture_kind.health_capture_kind() {
                invalid_reasons.insert(format!(
                    "capture_health_kind_mismatch:expected_{}:actual_{}",
                    args.capture_kind.health_capture_kind(),
                    receipt.capture_kind
                ));
            }
            Some(receipt)
        }
        Err(error) => {
            invalid_reasons.insert(format!("capture_health_receipt_unreadable:{error}"));
            None
        }
    };
    let mut tape_rows = tape::read_tape(&args.events_dir, &manifest.run_id)?;
    let (births, _) = tape::canonical_births(&mut tape_rows);
    let canonical_birth_keys = births
        .iter()
        .filter_map(|birth| match tape::birth_key(&birth.payload) {
            tape::BirthKeyResolution::Eligible(key) => Some(key),
            tape::BirthKeyResolution::OutsideUniverse | tape::BirthKeyResolution::Malformed(_) => {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    let trade_index = tape::strict_trade_index(&mut tape_rows, &canonical_birth_keys);
    let reserve_index = tape::strict_reserve_state_index(&mut tape_rows, &canonical_birth_keys);
    invalid_reasons.extend(tape_rows.invalid_reasons.iter().cloned());

    let quote_contract = match manifest.pump_quote_authority.materialize() {
        Ok(contract) => Some(contract),
        Err(error) => {
            invalid_reasons.insert(format!(
                "pump_quote_authority_materialization_failed:{error}"
            ));
            None
        }
    };

    if !invalid_reasons.is_empty() {
        let summary = invalid_capture_summary(
            &args,
            &manifest,
            &feature_scale_sha256,
            &contract_sha256,
            births.len(),
            invalid_reasons,
        );
        write_candidate_rows_new::<AceEvV2ScreeningRowV1>(
            &args.output_dir.join(SCREENING_FILE),
            &[],
        )?;
        write_candidate_rows_new::<AceEvV2CandidateOutcomeV1>(
            &args.output_dir.join(OUTCOMES_FILE),
            &[],
        )?;
        write_json_new(&args.output_dir.join(SUMMARY_FILE), &summary)?;
        return Ok(summary);
    }
    let quote_contract = quote_contract.expect("validated quote authority is present");
    let health_receipt = health_receipt.expect("validated capture health receipt is present");

    let mut screening_rows = Vec::<AceEvV2ScreeningRowV1>::new();
    let mut plans = Vec::<PreEntryPlan>::new();
    let mut screening_reason_counts = BTreeMap::<String, usize>::new();
    let mut direct_post_120s_state_count = 0usize;
    for birth in births.iter().cloned() {
        let tape::BirthKeyResolution::Eligible(key) = tape::birth_key(&birth.payload) else {
            let row = screening_row(
                &birth,
                None,
                None,
                AceEvV2ScreeningStatus::PreEntryNonEvaluable,
                "birth_not_v2_eligible_universe",
            );
            *screening_reason_counts
                .entry("birth_not_v2_eligible_universe".to_string())
                .or_default() += 1;
            screening_rows.push(row);
            continue;
        };
        let trades = trade_index.get(&key).cloned().unwrap_or_default();
        let raw_states = reserve_index.get(&key).cloned().unwrap_or_default();
        match direct_states_from_tape(&raw_states) {
            Ok(states) => {
                if has_direct_state_after_birth_plus(&birth, &states, 120_000) {
                    direct_post_120s_state_count = direct_post_120s_state_count.saturating_add(1);
                }
                match make_pre_entry_plan(&birth, trades, states, &feature_scale, &quote_contract) {
                    Ok(plan) => {
                        plans.push(plan);
                    }
                    Err(PreEntryFailure::CreatorSellVeto { features, order }) => {
                        *screening_reason_counts
                            .entry("creator_sell_before_entry".to_string())
                            .or_default() += 1;
                        screening_rows.push(screening_row(
                            &birth,
                            Some(features),
                            Some(order),
                            AceEvV2ScreeningStatus::PreEntryCreatorSellReject,
                            "creator_sell_before_entry",
                        ));
                    }
                    Err(PreEntryFailure::NonEvaluable {
                        reason,
                        features,
                        order,
                    }) => {
                        *screening_reason_counts.entry(reason.clone()).or_default() += 1;
                        screening_rows.push(screening_row(
                            &birth,
                            features,
                            order,
                            AceEvV2ScreeningStatus::PreEntryNonEvaluable,
                            &reason,
                        ));
                    }
                }
            }
            Err(reason) => {
                invalid_reasons.insert(reason);
            }
        }
    }

    if !invalid_reasons.is_empty() {
        let summary = invalid_capture_summary(
            &args,
            &manifest,
            &feature_scale_sha256,
            &contract_sha256,
            births.len(),
            invalid_reasons,
        );
        write_candidate_rows_new(&args.output_dir.join(SCREENING_FILE), &screening_rows)?;
        write_candidate_rows_new::<AceEvV2CandidateOutcomeV1>(
            &args.output_dir.join(OUTCOMES_FILE),
            &[],
        )?;
        write_json_new(&args.output_dir.join(SUMMARY_FILE), &summary)?;
        return Ok(summary);
    }

    plans.sort_by(|left, right| {
        candidate_order_for(&left.birth).cmp(&candidate_order_for(&right.birth))
    });
    let enrollment_deadline_ms = health_receipt
        .start_captured_at_unix_ms
        .saturating_add(args.capture_kind.enrollment_window_ms());
    let mut enrollment_eligible_plans = Vec::with_capacity(plans.len());
    for plan in plans {
        if candidate_order_for(&plan.birth).decision_ingress_cutoff_ms <= enrollment_deadline_ms {
            enrollment_eligible_plans.push(plan);
        } else {
            *screening_reason_counts
                .entry("enrollment_closed_before_candidate_decision".to_string())
                .or_default() += 1;
            screening_rows.push(screening_row(
                &plan.birth,
                Some(plan.features),
                Some(candidate_order_for(&plan.birth)),
                AceEvV2ScreeningStatus::NotEnrolledCohortClosed,
                "enrollment_closed_before_candidate_decision",
            ));
        }
    }
    let plans = enrollment_eligible_plans;
    let enrollment_limit = args.capture_kind.enrollment_limit();
    let enrollment_closed = plans.len() >= enrollment_limit
        || health_receipt.end_captured_at_unix_ms >= enrollment_deadline_ms;
    let enrolled_plans = plans.iter().take(enrollment_limit).collect::<Vec<_>>();
    let outcome_drain_deadline = enrolled_plans
        .iter()
        .map(|plan| required_outcome_drain_deadline(plan))
        .max();
    if let Some(deadline) = outcome_drain_deadline {
        if health_receipt.end_captured_at_unix_ms < deadline {
            invalid_reasons.insert(format!(
                "outcome_drain_incomplete:end_ms={}:required_ms={deadline}",
                health_receipt.end_captured_at_unix_ms
            ));
        }
    }
    if !invalid_reasons.is_empty() {
        let summary = invalid_capture_summary(
            &args,
            &manifest,
            &feature_scale_sha256,
            &contract_sha256,
            births.len(),
            invalid_reasons,
        );
        write_candidate_rows_new(&args.output_dir.join(SCREENING_FILE), &screening_rows)?;
        write_candidate_rows_new::<AceEvV2CandidateOutcomeV1>(
            &args.output_dir.join(OUTCOMES_FILE),
            &[],
        )?;
        write_json_new(&args.output_dir.join(SUMMARY_FILE), &summary)?;
        return Ok(summary);
    }
    let mut outcomes = Vec::new();
    for (index, plan) in plans.iter().take(enrollment_limit).enumerate() {
        let outcome =
            simulate_terminal_outcome(plan, &quote_contract, ENTRY_LATENCY_MS, EXIT_LATENCY_MS);
        let stress = simulate_terminal_outcome(plan, &quote_contract, 1_000, 1_000);
        outcomes.push(outcome_row(
            plan,
            index + 1,
            outcome,
            stress,
            args.capture_kind,
        ));
    }
    for plan in plans.iter().skip(enrollment_limit) {
        *screening_reason_counts
            .entry(format!(
                "cohort_closed_after_{enrollment_limit}_pre_entry_candidates"
            ))
            .or_default() += 1;
        screening_rows.push(screening_row(
            &plan.birth,
            Some(plan.features),
            Some(candidate_order_for(&plan.birth)),
            AceEvV2ScreeningStatus::NotEnrolledCohortClosed,
            &format!("cohort_closed_after_{enrollment_limit}_pre_entry_candidates"),
        ));
    }
    for plan in plans.iter().take(enrollment_limit) {
        screening_rows.push(screening_row(
            &plan.birth,
            Some(plan.features),
            Some(candidate_order_for(&plan.birth)),
            AceEvV2ScreeningStatus::PreEntryEligible,
            "enrolled",
        ));
    }
    screening_rows.sort_by(|left, right| {
        (
            left.candidate_order.as_ref(),
            left.base_mint.as_str(),
            left.bonding_curve.as_str(),
            left.candidate_id.as_str(),
        )
            .cmp(&(
                right.candidate_order.as_ref(),
                right.base_mint.as_str(),
                right.bonding_curve.as_str(),
                right.candidate_id.as_str(),
            ))
    });
    let terminal_status_counts = count_terminal_statuses(&outcomes);
    let outcomes_sha256 = candidate_rows_sha256(&outcomes)?;
    let successful_entry_count = outcomes
        .iter()
        .filter(|row| {
            matches!(
                row.terminal_status.as_str(),
                "EXIT_FILLED"
                    | "POST_ENTRY_VALIDITY_BOUND_LOSS_FLOOR"
                    | "POST_ENTRY_UNSUPPORTED_ROUTE_LOSS_FLOOR"
                    | "EXIT_STATE_UNAVAILABLE_LOSS_FLOOR"
            )
        })
        .count();
    let successful_entry_with_terminal_exit_count = outcomes
        .iter()
        .filter(|row| row.terminal_status == InternalTerminalStatus::EntryFilledExitFilled.as_str())
        .count();
    let terminal_outcome_count = outcomes.len();
    let terminal_status = match args.capture_kind {
        AceEvV2CaptureKind::YieldQualification => {
            if terminal_outcome_count >= QUALIFICATION_MIN_TERMINAL_OUTCOMES
                && successful_entry_with_terminal_exit_count
                    >= QUALIFICATION_MIN_SUCCESSFUL_ENTRIES_WITH_TERMINAL_EXIT
                && direct_post_120s_state_count > 0
            {
                "ACE_EV_V2_YIELD_QUALIFICATION_PASS".to_string()
            } else {
                "ACE_EV_V2_YIELD_QUALIFICATION_FAIL".to_string()
            }
        }
        AceEvV2CaptureKind::Prospective1000 => prospective_terminal_status(
            &args,
            &manifest,
            &health_receipt,
            &contract_sha256,
            &feature_scale_sha256,
            prospective_amendment
                .as_ref()
                .expect("prospective amendment was validated"),
            terminal_outcome_count,
            &outcomes,
        )?,
    };
    let summary = AceEvV2SummaryV1 {
        schema: ACE_EV_V2_SUMMARY_SCHEMA.to_string(),
        capture_kind: args.capture_kind,
        capture_status: "VALID_CAPTURE".to_string(),
        capture_invalid_reasons: Vec::new(),
        terminal_status,
        run_id: manifest.run_id.clone(),
        baseline_sha: manifest.baseline_sha.clone(),
        implementation_sha: manifest.implementation_sha.clone(),
        code_hash: manifest.code_hash.clone(),
        binary_hash: manifest.binary_hash.clone(),
        feature_scale_sha256: feature_scale_sha256.clone(),
        contract_sha256: contract_sha256.clone(),
        prospective_amendment_sha256: prospective_amendment.as_ref().map(|(_, sha)| sha.clone()),
        prospective_stop_evidence_sha256: prospective_stop_evidence_sha256(
            &args,
            prospective_amendment.as_ref().map(|(_, sha)| sha.as_str()),
            &manifest,
            &contract_sha256,
            &feature_scale_sha256,
            &outcomes,
        )?,
        cohort_candidate_order_sha256: (args.capture_kind == AceEvV2CaptureKind::Prospective1000)
            .then(|| candidate_cohort_sha256(&outcomes)),
        prospective_terminalization: prospective_amendment.as_ref().map(|_| {
            health_receipt
                .prospective_terminalization
                .clone()
                .unwrap_or_default()
        }),
        candidate_outcomes_sha256: Some(outcomes_sha256),
        total_canonical_births: births.len(),
        pre_entry_eligible_count: plans.len(),
        enrollment_closed,
        enrolled_count: outcomes.len(),
        terminal_outcome_count,
        successful_entry_count,
        successful_entry_with_terminal_exit_count,
        direct_post_120s_state_count,
        terminal_status_counts,
        screening_reason_counts,
    };
    write_candidate_rows_new(&args.output_dir.join(SCREENING_FILE), &screening_rows)?;
    write_candidate_rows_new(&args.output_dir.join(OUTCOMES_FILE), &outcomes)?;
    write_json_new(&args.output_dir.join(SUMMARY_FILE), &summary)?;
    Ok(summary)
}

/// Observe a growing, durable shadow tape and write immutable target evidence
/// once the *first* 1,000 candidate-order rows all have terminal outcomes.
///
/// This is deliberately an offline reader: it has no RPC, Event Bus, or
/// runtime mutation capability.  It shares the strict indexes, feature
/// eligibility, candidate ordering, pre-entry plan construction, and terminal
/// state machine with the final evaluator.  The only relaxed I/O rule is that
/// a currently-being-written final JSONL row is ignored until its newline is
/// durable.
pub fn run_ace_ev_v2_monitor(args: AceEvV2MonitorArgs) -> Result<()> {
    if args.poll_interval_ms == 0 {
        bail!("prospective monitor poll interval must be positive")
    }
    if args.stop_evidence_path.exists() {
        bail!(
            "prospective stop evidence already exists and is immutable: {}",
            args.stop_evidence_path.display()
        )
    }
    let contract_bytes = load_contract_bytes(&args.contract_path)?;
    let contract_sha256 = sha256_hex(&contract_bytes);
    let (amendment, amendment_sha256) =
        load_prospective_amendment(&args.amendment_path, &contract_sha256)?;
    let scale_bytes = fs::read(&args.feature_scale_path).with_context(|| {
        format!(
            "read ACE-EV V2 feature scale {}",
            args.feature_scale_path.display()
        )
    })?;
    let feature_scale: AceEvV2FeatureScaleV1 = serde_json::from_slice(&scale_bytes)
        .with_context(|| format!("decode feature scale {}", args.feature_scale_path.display()))?;
    validate_feature_scale(&feature_scale, &contract_sha256)?;
    let feature_scale_sha256 = sha256_hex(&scale_bytes);
    let manifest_bytes = fs::read(&args.manifest_path)
        .with_context(|| format!("read capture manifest {}", args.manifest_path.display()))?;
    let manifest: RugRealityCaptureRunManifestV1 = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("decode capture manifest {}", args.manifest_path.display()))?;
    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let manifest_reasons = validate_v2_capture_manifest_base(&manifest);
    if !manifest_reasons.is_empty() {
        bail!(
            "prospective monitor manifest invalid: {}",
            manifest_reasons.into_iter().collect::<Vec<_>>().join(", ")
        )
    }
    let capture_start_ms =
        load_monitor_start_snapshot(&args.start_metrics_path, &args.manifest_path, &manifest)?;
    let quote_contract = manifest
        .pump_quote_authority
        .materialize()
        .context("materialize capture-time typed Pump quote authority for prospective monitor")?;
    let monitor_binary_blake3 = blake3::hash(
        &fs::read(std::env::current_exe().context("resolve monitor executable")?)
            .context("read monitor executable for BLAKE3 provenance")?,
    )
    .to_hex()
    .to_string();

    loop {
        let (mut tape_rows, complete_file_prefixes) =
            tape::read_tape_complete_prefix(&args.events_dir, &manifest.run_id)?;
        let latest_arrival_ms = latest_tape_arrival_ms(&tape_rows);
        let plans = build_monitor_pre_entry_plans(&mut tape_rows, &feature_scale, &quote_contract)?;
        let enrollment_deadline_ms = capture_start_ms.saturating_add(amendment.max_enrollment_ms);
        let cohort = plans
            .iter()
            .filter(|plan| {
                candidate_order_for(&plan.birth).decision_ingress_cutoff_ms
                    <= enrollment_deadline_ms
            })
            .take(amendment.target_terminal_outcomes)
            .collect::<Vec<_>>();
        if let Some(required_drain) =
            prospective_cohort_drain_deadline(&cohort, amendment.target_terminal_outcomes)
        {
            // The monitor cannot turn an unknown future path into a loss
            // floor.  An earlier pending candidate therefore blocks a later,
            // fast terminal candidate from owning the stop decision.
            if latest_arrival_ms >= required_drain {
                let outcomes = cohort
                    .iter()
                    .enumerate()
                    .map(|(index, plan)| {
                        outcome_row(
                            plan,
                            index + 1,
                            simulate_terminal_outcome(
                                plan,
                                &quote_contract,
                                ENTRY_LATENCY_MS,
                                EXIT_LATENCY_MS,
                            ),
                            simulate_terminal_outcome(plan, &quote_contract, 1_000, 1_000),
                            AceEvV2CaptureKind::Prospective1000,
                        )
                    })
                    .collect::<Vec<_>>();
                let evidence = prospective_stop_evidence_for_target(
                    &manifest.run_id,
                    &manifest.implementation_sha,
                    &manifest_sha256,
                    &contract_sha256,
                    &amendment_sha256,
                    &feature_scale_sha256,
                    amendment.target_terminal_outcomes,
                    &outcomes,
                    complete_file_prefixes,
                    unix_now_ms()?,
                    &monitor_binary_blake3,
                )
                .expect("target-sized prospective cohort was just constructed");
                write_json_new(&args.stop_evidence_path, &evidence)?;
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(args.poll_interval_ms));
    }
}

fn unix_now_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock predates Unix epoch while writing prospective stop evidence")?
        .as_millis()
        .try_into()
        .context("prospective stop timestamp does not fit u64")?)
}

fn build_monitor_pre_entry_plans(
    tape_rows: &mut tape::Tape,
    feature_scale: &AceEvV2FeatureScaleV1,
    quote_contract: &RugScalpPumpQuoteContractV1,
) -> Result<Vec<PreEntryPlan>> {
    let (births, _) = tape::canonical_births(tape_rows);
    let canonical_birth_keys = births
        .iter()
        .filter_map(|birth| match tape::birth_key(&birth.payload) {
            tape::BirthKeyResolution::Eligible(key) => Some(key),
            tape::BirthKeyResolution::OutsideUniverse | tape::BirthKeyResolution::Malformed(_) => {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    let trade_index = tape::strict_trade_index(tape_rows, &canonical_birth_keys);
    let reserve_index = tape::strict_reserve_state_index(tape_rows, &canonical_birth_keys);
    if !tape_rows.invalid_reasons.is_empty() {
        bail!(
            "prospective monitor tape invalid: {}",
            tape_rows
                .invalid_reasons
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
    let mut plans = Vec::new();
    for birth in births {
        let tape::BirthKeyResolution::Eligible(key) = tape::birth_key(&birth.payload) else {
            continue;
        };
        let states =
            direct_states_from_tape(reserve_index.get(&key).map(Vec::as_slice).unwrap_or(&[]))
                .map_err(|reason| {
                    anyhow!("prospective monitor direct-state contract failure: {reason}")
                })?;
        if let Ok(plan) = make_pre_entry_plan(
            &birth,
            trade_index.get(&key).cloned().unwrap_or_default(),
            states,
            feature_scale,
            quote_contract,
        ) {
            plans.push(plan);
        }
    }
    plans.sort_by(|left, right| {
        candidate_order_for(&left.birth).cmp(&candidate_order_for(&right.birth))
    });
    Ok(plans)
}

fn latest_tape_arrival_ms(tape_rows: &tape::Tape) -> u64 {
    let birth_arrival = tape_rows
        .births
        .iter()
        .filter_map(|birth| birth.payload.detected_wall_ts_ms)
        .max();
    let trade_arrival = tape_rows
        .trades
        .iter()
        .map(|trade| trade.payload.arrival_ts_ms)
        .max();
    let state_arrival = tape_rows
        .reserve_states
        .iter()
        .map(|state| state.payload.arrival_ts_ms)
        .max();
    birth_arrival
        .into_iter()
        .chain(trade_arrival)
        .chain(state_arrival)
        .max()
        .unwrap_or(0)
}

fn validate_feature_scale(scale: &AceEvV2FeatureScaleV1, contract_sha256: &str) -> Result<()> {
    if scale.schema != ACE_EV_V2_FEATURE_SCALE_SCHEMA
        || scale.estimand_id != ACE_EV_V2_ESTIMAND_ID
        || scale.feature_order != ACE_EV_V2_FEATURE_ORDER.map(str::to_string)
        || scale.contract_sha256 != contract_sha256
    {
        bail!("ACE-EV V2 feature scale contract mismatch")
    }
    if scale.population_count == 0 {
        bail!("FEATURE_SCALE_INVALID: source population is empty")
    }
    for feature in &scale.features {
        if !feature.winsor_p01.is_finite()
            || !feature.winsor_p99.is_finite()
            || !feature.median.is_finite()
            || !feature.iqr.is_finite()
            || feature.iqr == 0.0
            || feature.winsor_p01 > feature.winsor_p99
        {
            bail!("FEATURE_SCALE_INVALID: invalid scale for {}", feature.id)
        }
    }
    Ok(())
}

fn validate_v2_capture_manifest(
    manifest_path: &Path,
    manifest: &RugRealityCaptureRunManifestV1,
) -> BTreeSet<String> {
    let mut reasons = validate_v2_capture_manifest_base(manifest);
    reasons.extend(tape::validate_capture_health_evidence(
        manifest_path,
        manifest,
    ));
    reasons
}

fn validate_v2_capture_manifest_base(
    manifest: &RugRealityCaptureRunManifestV1,
) -> BTreeSet<String> {
    let mut reasons = BTreeSet::new();
    if manifest.schema_version < 3 {
        reasons.insert("manifest_schema_too_old".to_string());
    }
    if manifest.run_id.trim().is_empty()
        || manifest.config_hash.trim().is_empty()
        || manifest.binary_hash.trim().is_empty()
        || manifest.health_evidence_path.trim().is_empty()
    {
        reasons.insert("manifest_capture_provenance_missing".to_string());
    }
    if manifest.baseline_sha != tape::ACE_CORE_BASELINE_SHA {
        reasons.insert("manifest_baseline_sha_mismatch".to_string());
    }
    if manifest.implementation_sha.len() != 40
        || !manifest
            .implementation_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || manifest.code_hash != format!("git:{}", manifest.implementation_sha)
    {
        reasons.insert("manifest_implementation_provenance_invalid".to_string());
    }
    if !manifest.observe_only {
        reasons.insert("manifest_not_observe_only".to_string());
    }
    if manifest.entry_route_id != "buy_v2" || manifest.exit_route_id != "legacy_sell" {
        reasons.insert("manifest_route_authority_mismatch".to_string());
    }
    if manifest.event_writer_run_id != manifest.run_id
        || !manifest.event_writer_optional_events_enabled
    {
        reasons.insert("manifest_event_writer_contract_invalid".to_string());
    }
    if manifest.authority_epoch_id == 0
        || manifest
            .runtime_fee_authority
            .evidence_hash
            .trim()
            .is_empty()
    {
        reasons.insert("manifest_primary_or_fee_authority_missing".to_string());
    }
    if manifest.pump_quote_authority.materialize().is_err() {
        reasons.insert("manifest_pump_quote_authority_unmaterializable".to_string());
    }
    reasons
}

fn load_capture_health_receipt(
    manifest: &RugRealityCaptureRunManifestV1,
) -> Result<RugRealityCaptureHealthEvidenceV1> {
    let path = Path::new(&manifest.health_evidence_path);
    let bytes = fs::read(path)
        .with_context(|| format!("read capture health receipt {}", path.display()))?;
    let receipt: RugRealityCaptureHealthEvidenceV1 = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode capture health receipt {}", path.display()))?;
    if receipt.run_id != manifest.run_id {
        bail!("capture health receipt run_id mismatch")
    }
    Ok(receipt)
}

fn candidate_cohort_sha256(rows: &[AceEvV2CandidateOutcomeV1]) -> String {
    let mut hasher = Sha256::new();
    for row in rows {
        hasher.update((row.enrollment_index as u64).to_be_bytes());
        for value in [
            row.candidate_id.as_str(),
            row.base_mint.as_str(),
            row.bonding_curve.as_str(),
            row.candidate_order.bonding_curve.as_str(),
            row.candidate_order.base_mint.as_str(),
        ] {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value.as_bytes());
        }
        hasher.update(row.candidate_order.decision_ingress_cutoff_ms.to_be_bytes());
        hasher.update(row.candidate_order.birth_ts_ms.to_be_bytes());
        hasher.update(row.candidate_order.event_slot.to_be_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Return the latest outcome horizon that must be observed before all rows in
/// the target candidate-order cohort are terminal.  The cohort is deliberately
/// selected before this check: a later fast candidate must not bypass an
/// earlier candidate that is still pending its bounded outcome window.
fn prospective_cohort_drain_deadline(cohort: &[&PreEntryPlan], target: usize) -> Option<u64> {
    (cohort.len() == target).then(|| {
        cohort
            .iter()
            .map(|plan| required_outcome_drain_deadline(plan))
            .max()
            .unwrap_or(0)
    })
}

/// Construct, but never overwrite, the immutable monitor-to-supervisor
/// receipt.  Returning `None` below the target makes the 999-row case a
/// non-event rather than an ambiguous partial stop request.  When more rows
/// are supplied, only the first candidate-order target participates, which is
/// the same boundary the final evaluator reconstitutes after shutdown.
fn prospective_stop_evidence_for_target(
    run_id: &str,
    implementation_sha: &str,
    manifest_sha256: &str,
    contract_sha256: &str,
    amendment_sha256: &str,
    feature_scale_sha256: &str,
    target: usize,
    outcomes: &[AceEvV2CandidateOutcomeV1],
    complete_file_prefixes: Vec<tape::TapeCompletePrefixV1>,
    stop_captured_at_unix_ms: u64,
    monitor_binary_blake3: &str,
) -> Option<AceEvV2ProspectiveStopEvidenceV1> {
    let target_rows = outcomes.get(..target)?;
    Some(AceEvV2ProspectiveStopEvidenceV1 {
        schema: ACE_EV_V2_PROSPECTIVE_STOP_EVIDENCE_SCHEMA.to_string(),
        run_id: run_id.to_string(),
        manifest_sha256: manifest_sha256.to_string(),
        base_contract_sha256: contract_sha256.to_string(),
        amendment_sha256: amendment_sha256.to_string(),
        feature_scale_sha256: feature_scale_sha256.to_string(),
        implementation_sha: implementation_sha.to_string(),
        target_terminal_outcomes: target,
        terminal_outcome_count: target_rows.len(),
        cohort_candidate_order_sha256: candidate_cohort_sha256(target_rows),
        complete_file_prefixes,
        stop_captured_at_unix_ms,
        monitor_binary_blake3: monitor_binary_blake3.to_string(),
    })
}

fn read_prospective_stop_evidence(
    path: &Path,
) -> Result<(AceEvV2ProspectiveStopEvidenceV1, String)> {
    let bytes = fs::read(path)
        .with_context(|| format!("read prospective stop evidence {}", path.display()))?;
    let evidence: AceEvV2ProspectiveStopEvidenceV1 = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode prospective stop evidence {}", path.display()))?;
    Ok((evidence, sha256_hex(&bytes)))
}

fn validate_prospective_stop_evidence(
    evidence: &AceEvV2ProspectiveStopEvidenceV1,
    manifest: &RugRealityCaptureRunManifestV1,
    manifest_sha256: &str,
    contract_sha256: &str,
    amendment_sha256: &str,
    feature_scale_sha256: &str,
    expected_cohort_sha256: &str,
) -> Result<()> {
    if evidence.schema != ACE_EV_V2_PROSPECTIVE_STOP_EVIDENCE_SCHEMA
        || evidence.run_id != manifest.run_id
        || evidence.manifest_sha256 != manifest_sha256
        || evidence.base_contract_sha256 != contract_sha256
        || evidence.amendment_sha256 != amendment_sha256
        || evidence.feature_scale_sha256 != feature_scale_sha256
        || evidence.implementation_sha != manifest.implementation_sha
        || evidence.target_terminal_outcomes != PROSPECTIVE_ENROLLMENT_LIMIT
        || evidence.terminal_outcome_count != PROSPECTIVE_ENROLLMENT_LIMIT
        || evidence.stop_captured_at_unix_ms == 0
        || evidence.monitor_binary_blake3.trim().is_empty()
        || evidence.complete_file_prefixes.is_empty()
    {
        bail!("prospective stop evidence contract mismatch")
    }
    ensure_prospective_cohort_hash(
        &evidence.cohort_candidate_order_sha256,
        expected_cohort_sha256,
    )?;
    let mut last = None::<&str>;
    for prefix in &evidence.complete_file_prefixes {
        if prefix.relative_path.trim().is_empty()
            || last.is_some_and(|previous| previous >= prefix.relative_path.as_str())
        {
            bail!("prospective stop evidence file-prefix ordering invalid")
        }
        last = Some(&prefix.relative_path);
    }
    Ok(())
}

fn ensure_prospective_cohort_hash(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("prospective stop evidence cohort hash mismatch")
    }
    Ok(())
}

fn prospective_terminal_status(
    args: &AceEvV2EvaluateArgs,
    manifest: &RugRealityCaptureRunManifestV1,
    health_receipt: &RugRealityCaptureHealthEvidenceV1,
    contract_sha256: &str,
    feature_scale_sha256: &str,
    amendment: &(AceEvV2ProspectiveAmendmentV1, String),
    terminal_outcome_count: usize,
    outcomes: &[AceEvV2CandidateOutcomeV1],
) -> Result<String> {
    match health_receipt.prospective_terminalization.as_deref() {
        Some("TARGET_REACHED") => {
            if terminal_outcome_count != amendment.0.target_terminal_outcomes {
                bail!("prospective target receipt does not have exactly 1000 terminal rows")
            }
            let path = args.stop_evidence_path.as_deref().ok_or_else(|| {
                anyhow!("TARGET_REACHED prospective capture requires --stop-evidence")
            })?;
            let (evidence, _) = read_prospective_stop_evidence(path)?;
            let manifest_sha256 = sha256_hex(&fs::read(&args.manifest_path)?);
            validate_prospective_stop_evidence(
                &evidence,
                manifest,
                &manifest_sha256,
                contract_sha256,
                &amendment.1,
                feature_scale_sha256,
                &candidate_cohort_sha256(outcomes),
            )?;
            let receipt_hash = health_receipt
                .prospective_stop_evidence_sha256
                .as_deref()
                .ok_or_else(|| anyhow!("prospective health receipt lacks stop evidence hash"))?;
            let (_, evidence_hash) = read_prospective_stop_evidence(path)?;
            if receipt_hash != evidence_hash {
                bail!("prospective health receipt stop evidence hash mismatch")
            }
            Ok("ACE_EV_V2_OUTCOMES_READY_FOR_FIT".to_string())
        }
        Some("MAX_DURATION_INSUFFICIENT_YIELD") => {
            if terminal_outcome_count >= amendment.0.target_terminal_outcomes {
                bail!("prospective max-duration receipt conflicts with reached target")
            }
            if args.stop_evidence_path.is_some() {
                bail!("max-duration prospective capture must not supply target stop evidence")
            }
            Ok("ACE_EV_V2_INSUFFICIENT_YIELD".to_string())
        }
        _ => bail!("prospective health receipt terminalization is missing or invalid"),
    }
}

fn prospective_stop_evidence_sha256(
    args: &AceEvV2EvaluateArgs,
    amendment_sha256: Option<&str>,
    manifest: &RugRealityCaptureRunManifestV1,
    contract_sha256: &str,
    feature_scale_sha256: &str,
    outcomes: &[AceEvV2CandidateOutcomeV1],
) -> Result<Option<String>> {
    if args.capture_kind != AceEvV2CaptureKind::Prospective1000 {
        return Ok(None);
    }
    let receipt = load_capture_health_receipt(manifest)?;
    if receipt.prospective_terminalization.as_deref() != Some("TARGET_REACHED") {
        return Ok(None);
    }
    let path = args
        .stop_evidence_path
        .as_deref()
        .ok_or_else(|| anyhow!("TARGET_REACHED prospective capture requires --stop-evidence"))?;
    let (evidence, hash) = read_prospective_stop_evidence(path)?;
    let manifest_sha256 = sha256_hex(&fs::read(&args.manifest_path)?);
    validate_prospective_stop_evidence(
        &evidence,
        manifest,
        &manifest_sha256,
        contract_sha256,
        amendment_sha256.ok_or_else(|| anyhow!("prospective amendment hash missing"))?,
        feature_scale_sha256,
        &candidate_cohort_sha256(outcomes),
    )?;
    Ok(Some(hash))
}

fn required_outcome_drain_deadline(plan: &PreEntryPlan) -> u64 {
    // Worst legal path: entry can land at the end of its 1,000 ms lookup lag,
    // max-hold begins from that landed arrival, and exit can land at the end
    // of its own 1,000 ms lookup lag.  If the capture ended earlier, a missing
    // future state is censoring, not the contractual zero-recovery loss floor.
    plan.cutoffs
        .ingress_cutoff_ts_ms
        .saturating_add(ENTRY_LATENCY_MS)
        .saturating_add(ENTRY_LANDING_MAX_LAG_MS)
        .saturating_add(MAX_HOLD_MS)
        .saturating_add(EXIT_LATENCY_MS)
        .saturating_add(EXIT_LANDING_MAX_LAG_MS)
}

fn invalid_capture_summary(
    args: &AceEvV2EvaluateArgs,
    manifest: &RugRealityCaptureRunManifestV1,
    feature_scale_sha256: &str,
    contract_sha256: &str,
    total_canonical_births: usize,
    invalid_reasons: BTreeSet<String>,
) -> AceEvV2SummaryV1 {
    AceEvV2SummaryV1 {
        schema: ACE_EV_V2_SUMMARY_SCHEMA.to_string(),
        capture_kind: args.capture_kind,
        capture_status: "INVALID_CAPTURE".to_string(),
        capture_invalid_reasons: invalid_reasons.into_iter().collect(),
        terminal_status: "ACE_EV_V2_CAPTURE_INVALID".to_string(),
        run_id: manifest.run_id.clone(),
        baseline_sha: manifest.baseline_sha.clone(),
        implementation_sha: manifest.implementation_sha.clone(),
        code_hash: manifest.code_hash.clone(),
        binary_hash: manifest.binary_hash.clone(),
        feature_scale_sha256: feature_scale_sha256.to_string(),
        contract_sha256: contract_sha256.to_string(),
        prospective_amendment_sha256: None,
        prospective_stop_evidence_sha256: None,
        cohort_candidate_order_sha256: None,
        prospective_terminalization: None,
        candidate_outcomes_sha256: None,
        total_canonical_births,
        pre_entry_eligible_count: 0,
        enrollment_closed: false,
        enrolled_count: 0,
        terminal_outcome_count: 0,
        successful_entry_count: 0,
        successful_entry_with_terminal_exit_count: 0,
        direct_post_120s_state_count: 0,
        terminal_status_counts: BTreeMap::new(),
        screening_reason_counts: BTreeMap::new(),
    }
}

fn direct_states_from_tape(
    rows: &[tape::TapeReserveState],
) -> std::result::Result<Vec<DirectState>, String> {
    let mut states = Vec::with_capacity(rows.len());
    for row in rows {
        let payload: &PoolReserveStatePayload = &row.payload;
        if payload.provider_role.as_deref() != Some("PrimaryAuthority") {
            return Err("pool_reserve_state_provider_not_primary_authority".to_string());
        }
        if payload.event_ts_ms == 0 || payload.arrival_ts_ms == 0 {
            return Err("pool_reserve_state_timestamp_missing".to_string());
        }
        states.push(DirectState {
            event_ts_ms: payload.event_ts_ms,
            arrival_ts_ms: payload.arrival_ts_ms,
            slot: payload.slot,
            write_version: payload.write_version,
            sequence_number: payload.sequence_number,
            ordinal: (row.file_ordinal, row.line_number),
            complete: payload.complete,
            reserves: PumpReserveState {
                virtual_base_reserves: payload.virtual_token_reserves,
                virtual_quote_reserves: payload.virtual_sol_reserves,
                real_base_reserves: payload.real_token_reserves,
                real_quote_reserves: payload.real_sol_reserves,
            },
        });
    }
    states.sort_by(compare_state_chain);
    Ok(states)
}

fn compare_state_chain(left: &DirectState, right: &DirectState) -> Ordering {
    (
        left.event_ts_ms,
        left.slot,
        left.write_version,
        left.sequence_number,
        left.ordinal,
    )
        .cmp(&(
            right.event_ts_ms,
            right.slot,
            right.write_version,
            right.sequence_number,
            right.ordinal,
        ))
}

fn compare_state_arrival(left: &DirectState, right: &DirectState) -> Ordering {
    (
        left.arrival_ts_ms,
        left.event_ts_ms,
        left.slot,
        left.write_version,
        left.sequence_number,
        left.ordinal,
    )
        .cmp(&(
            right.arrival_ts_ms,
            right.event_ts_ms,
            right.slot,
            right.write_version,
            right.sequence_number,
            right.ordinal,
        ))
}

fn has_direct_state_after_birth_plus(
    birth: &tape::TapeBirth,
    states: &[DirectState],
    offset_ms: u64,
) -> bool {
    let Some(birth_ingress) = birth.payload.detected_wall_ts_ms else {
        return false;
    };
    states.iter().any(|state| {
        state.event_ts_ms >= birth.payload.birth_ts_ms.saturating_add(offset_ms)
            && state.arrival_ts_ms >= birth_ingress.saturating_add(offset_ms)
    })
}

#[derive(Debug, Clone)]
enum PreEntryFailure {
    CreatorSellVeto {
        features: AceEvV2FeatureVectorV1,
        order: CandidateOrderV1,
    },
    NonEvaluable {
        reason: String,
        features: Option<AceEvV2FeatureVectorV1>,
        order: Option<CandidateOrderV1>,
    },
}

fn make_pre_entry_plan(
    birth: &tape::TapeBirth,
    trades: Vec<tape::TapeTrade>,
    states: Vec<DirectState>,
    scale: &AceEvV2FeatureScaleV1,
    quote_contract: &RugScalpPumpQuoteContractV1,
) -> std::result::Result<PreEntryPlan, PreEntryFailure> {
    let (features, cutoffs) =
        calculate_features_v2(birth, &trades).map_err(|reason| PreEntryFailure::NonEvaluable {
            reason,
            features: None,
            order: None,
        })?;
    let order = candidate_order_for(birth);
    if has_pre_entry_creator_sell_veto(birth, &trades).map_err(|reason| {
        PreEntryFailure::NonEvaluable {
            reason,
            features: Some(features),
            order: Some(order.clone()),
        }
    })? {
        return Err(PreEntryFailure::CreatorSellVeto { features, order });
    }
    let normalized_features =
        normalize_features(features, scale).map_err(|reason| PreEntryFailure::NonEvaluable {
            reason,
            features: Some(features),
            order: Some(order.clone()),
        })?;
    let decision_state = states
        .iter()
        .filter(|state| {
            state.event_ts_ms <= cutoffs.event_cutoff_ts_ms
                && state.arrival_ts_ms <= cutoffs.ingress_cutoff_ts_ms
        })
        .max_by(|left, right| compare_state_chain(left, right))
        .copied()
        .ok_or_else(|| PreEntryFailure::NonEvaluable {
            reason: "decision_time_direct_reserve_state_missing".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        })?;
    if decision_state.complete {
        return Err(PreEntryFailure::NonEvaluable {
            reason: "pre_entry_complete_or_migrated_route".to_string(),
            features: Some(features),
            order: Some(order),
        });
    }
    let entry_tx_cost = quote_contract
        .entry_transaction_cost_lamports()
        .map_err(|_| PreEntryFailure::NonEvaluable {
            reason: "decision_entry_transaction_cost_unavailable".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        })?;
    let program_cap = ENTRY_TOTAL_WALLET_DEBIT_CAP_LAMPORTS
        .checked_sub(entry_tx_cost)
        .ok_or_else(|| PreEntryFailure::NonEvaluable {
            reason: "decision_entry_transaction_cost_exceeds_wallet_cap".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        })?;
    let entry_quote = quote_contract
        .quote_buy_v2_under_wallet_cap(decision_state.slot, decision_state.reserves, program_cap)
        .map_err(|_| PreEntryFailure::NonEvaluable {
            reason: "decision_buy_v2_quote_unavailable".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        })?;
    let entry_total_debit_lamports = entry_quote
        .program_settlement
        .wallet_debit_or_credit
        .checked_add(entry_tx_cost)
        .ok_or_else(|| PreEntryFailure::NonEvaluable {
            reason: "decision_entry_total_debit_overflow".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        })?;
    if !entry_quote.instruction_limit_check.passed
        || entry_total_debit_lamports > ENTRY_TOTAL_WALLET_DEBIT_CAP_LAMPORTS
    {
        return Err(PreEntryFailure::NonEvaluable {
            reason: "decision_entry_wallet_cap_or_instruction_protection_failed".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        });
    }
    let reserves_after_entry = reserves_after_buy(decision_state.reserves, &entry_quote);
    let entry_impact_bps =
        tape::absolute_virtual_price_impact_bps(decision_state.reserves, reserves_after_entry)
            .ok_or_else(|| PreEntryFailure::NonEvaluable {
                reason: "decision_entry_impact_unavailable".to_string(),
                features: Some(features),
                order: Some(order.clone()),
            })?;
    if entry_impact_bps > MAX_ENTRY_IMPACT_BPS {
        return Err(PreEntryFailure::NonEvaluable {
            reason: "decision_entry_impact_exceeds_5pct".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        });
    }
    let (immediate_exit_quote, _) = quote_contract
        .executable_exit_value_lamports(
            decision_state.slot,
            reserves_after_entry,
            entry_quote.token_amount,
        )
        .map_err(|_| PreEntryFailure::NonEvaluable {
            reason: "decision_immediate_full_exit_unquotable".to_string(),
            features: Some(features),
            order: Some(order.clone()),
        })?;
    let reserves_after_exit =
        tape::reserve_state_after_quote(reserves_after_entry, &immediate_exit_quote);
    let immediate_exit_impact_bps =
        tape::absolute_virtual_price_impact_bps(reserves_after_entry, reserves_after_exit)
            .ok_or_else(|| PreEntryFailure::NonEvaluable {
                reason: "decision_immediate_exit_impact_unavailable".to_string(),
                features: Some(features),
                order: Some(order.clone()),
            })?;
    if immediate_exit_impact_bps > MAX_IMMEDIATE_EXIT_IMPACT_BPS {
        return Err(PreEntryFailure::NonEvaluable {
            reason: "decision_immediate_full_exit_impact_exceeds_5pct".to_string(),
            features: Some(features),
            order: Some(order),
        });
    }
    Ok(PreEntryPlan {
        birth: birth.clone(),
        trades,
        states,
        cutoffs,
        features,
        normalized_features,
        decision_state,
        fixed_token_amount_raw: entry_quote.token_amount,
        fixed_max_sol_cost_lamports: entry_quote.instruction_limit_check.limit,
        decision_entry_total_debit_lamports: entry_total_debit_lamports,
        decision_entry_impact_bps: entry_impact_bps,
        decision_immediate_exit_impact_bps: immediate_exit_impact_bps,
    })
}

fn normalize_features(
    features: AceEvV2FeatureVectorV1,
    scale: &AceEvV2FeatureScaleV1,
) -> std::result::Result<[f64; 7], String> {
    let raw = features.values();
    let mut normalized = [0.0; 7];
    for index in 0..7 {
        let scale_feature = &scale.features[index];
        if scale_feature.iqr == 0.0 || !scale_feature.iqr.is_finite() {
            return Err("FEATURE_SCALE_INVALID".to_string());
        }
        let winsorized = raw[index].clamp(scale_feature.winsor_p01, scale_feature.winsor_p99);
        let value = (winsorized - scale_feature.median) / scale_feature.iqr;
        if !value.is_finite() {
            return Err("FEATURE_SCALE_INVALID".to_string());
        }
        normalized[index] = value;
    }
    Ok(normalized)
}

fn candidate_order_for(birth: &tape::TapeBirth) -> CandidateOrderV1 {
    CandidateOrderV1 {
        decision_ingress_cutoff_ms: birth
            .payload
            .detected_wall_ts_ms
            .unwrap_or(u64::MAX)
            .saturating_add(FEATURE_CUTOFF_MS),
        birth_ts_ms: birth.payload.birth_ts_ms,
        event_slot: birth.payload.event_slot.unwrap_or(u64::MAX),
        bonding_curve: birth.payload.bonding_curve.clone(),
        base_mint: birth.payload.base_mint.clone(),
    }
}

fn screening_row(
    birth: &tape::TapeBirth,
    features: Option<AceEvV2FeatureVectorV1>,
    order: Option<CandidateOrderV1>,
    status: AceEvV2ScreeningStatus,
    reason: &str,
) -> AceEvV2ScreeningRowV1 {
    AceEvV2ScreeningRowV1 {
        schema: ACE_EV_V2_SCREENING_SCHEMA.to_string(),
        candidate_id: birth.candidate_id.clone(),
        base_mint: birth.payload.base_mint.clone(),
        bonding_curve: birth.payload.bonding_curve.clone(),
        creator: birth.payload.creator.clone(),
        candidate_order: order,
        feature_vector: features,
        normalized_features: None,
        status,
        reason: Some(reason.to_string()),
    }
}

fn write_candidate_rows_new<T: Serialize>(path: &Path, rows: &[T]) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create ACE-EV V2 row file {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    for row in rows {
        serde_json::to_writer(&mut writer, row)
            .with_context(|| format!("encode ACE-EV V2 row {}", path.display()))?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer
        .into_inner()
        .map_err(|error| anyhow!("flush ACE-EV V2 row file {}: {error}", path.display()))?
        .sync_all()
        .with_context(|| format!("sync ACE-EV V2 row file {}", path.display()))?;
    Ok(())
}

fn candidate_rows_sha256<T: Serialize>(rows: &[T]) -> Result<String> {
    let mut hasher = Sha256::new();
    for row in rows {
        let bytes = serde_json::to_vec(row).context("serialize candidate row for output hash")?;
        hasher.update(bytes);
        hasher.update(b"\n");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitTriggerKind {
    CreatorSell,
    HardLoss,
    TakeProfit,
    MaxHold,
}

impl ExitTriggerKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CreatorSell => "creator_sell",
            Self::HardLoss => "hard_loss",
            Self::TakeProfit => "take_profit",
            Self::MaxHold => "max_hold",
        }
    }

    const fn priority(self) -> u8 {
        match self {
            Self::CreatorSell => 1,
            Self::HardLoss => 2,
            Self::TakeProfit => 3,
            Self::MaxHold => 4,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ExitTrigger {
    kind: ExitTriggerKind,
    event_ts_ms: u64,
    arrival_ts_ms: u64,
    slot: u64,
    order: Option<tape::CanonicalTradeOrder>,
}

#[derive(Debug, Clone, Copy)]
enum LandingResult {
    Landed(DirectState),
    RouteLost,
    Unavailable,
}

#[derive(Debug, Clone, Copy)]
enum ExitAttemptResult {
    Filled {
        net_pnl_lamports: i128,
        net_return: f64,
        landing: DirectState,
        profit17_hit: bool,
    },
    FailedTakeProfit,
    RouteLost,
    StateUnavailable,
}

fn simulate_terminal_outcome(
    plan: &PreEntryPlan,
    quote_contract: &RugScalpPumpQuoteContractV1,
    entry_latency_ms: u64,
    exit_latency_ms: u64,
) -> TerminalOutcome {
    let entry_attempt_cost = match quote_contract.entry_transaction_cost_lamports() {
        Ok(cost) => cost,
        Err(_) => {
            return entry_failed_price_protection(None, None, None, 0);
        }
    };
    let entry_landing = match find_landed_state(
        &plan.states,
        plan.cutoffs.event_cutoff_ts_ms,
        plan.cutoffs.ingress_cutoff_ts_ms,
        entry_latency_ms,
        ENTRY_LANDING_MAX_LAG_MS,
    ) {
        LandingResult::Landed(state) if !state.complete => state,
        LandingResult::Landed(_) | LandingResult::RouteLost | LandingResult::Unavailable => {
            return entry_failed_no_landing(entry_attempt_cost);
        }
    };
    let entry_quote = match quote_contract.quote_buy_v2_exact_base_out_with_max_sol_cost(
        entry_landing.slot,
        entry_landing.reserves,
        plan.fixed_token_amount_raw,
        plan.fixed_max_sol_cost_lamports,
    ) {
        Ok(quote) if quote.instruction_limit_check.passed => quote,
        _ => {
            return entry_failed_price_protection(
                Some(entry_landing),
                None,
                None,
                entry_attempt_cost,
            )
        }
    };
    let entry_total_debit_lamports = match entry_quote
        .program_settlement
        .wallet_debit_or_credit
        .checked_add(entry_attempt_cost)
    {
        Some(total) if total <= ENTRY_TOTAL_WALLET_DEBIT_CAP_LAMPORTS => total,
        _ => {
            return entry_failed_price_protection(
                Some(entry_landing),
                None,
                None,
                entry_attempt_cost,
            )
        }
    };
    // The landed BuyV2 has now passed its instruction protection and the
    // frozen wallet-debit cap.  It is an irrevocable simulated fill.  Any
    // later validity or route failure must remain in the enrolled cohort as a
    // conservative post-entry loss floor; it must never be rewritten as a
    // cheaper ENTRY_FAILED attempt.
    let reserves_after_entry = reserves_after_buy(entry_landing.reserves, &entry_quote);
    let entry_impact_bps =
        match tape::absolute_virtual_price_impact_bps(entry_landing.reserves, reserves_after_entry)
        {
            Some(value) if value <= MAX_ENTRY_IMPACT_BPS => value,
            Some(value) => {
                return post_entry_loss_floor(
                    InternalTerminalStatus::PostEntryValidityBoundLossFloor,
                    Some("landed_entry_impact_exceeds_5pct"),
                    entry_landing,
                    entry_total_debit_lamports,
                    Some(value),
                    None,
                    0,
                    0,
                )
            }
            None => {
                return post_entry_loss_floor(
                    InternalTerminalStatus::PostEntryValidityBoundLossFloor,
                    Some("landed_entry_impact_unavailable"),
                    entry_landing,
                    entry_total_debit_lamports,
                    None,
                    None,
                    0,
                    0,
                )
            }
        };
    let (immediate_exit_quote, _) = match quote_contract.executable_exit_value_lamports(
        entry_landing.slot,
        reserves_after_entry,
        entry_quote.token_amount,
    ) {
        Ok(value) => value,
        Err(_) => {
            return post_entry_loss_floor(
                InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor,
                Some("immediate_exit_quote_unavailable"),
                entry_landing,
                entry_total_debit_lamports,
                Some(entry_impact_bps),
                None,
                0,
                0,
            )
        }
    };
    let immediate_exit_impact_bps = match tape::absolute_virtual_price_impact_bps(
        reserves_after_entry,
        tape::reserve_state_after_quote(reserves_after_entry, &immediate_exit_quote),
    ) {
        Some(value) if value <= MAX_IMMEDIATE_EXIT_IMPACT_BPS => value,
        Some(value) => {
            return post_entry_loss_floor(
                InternalTerminalStatus::PostEntryValidityBoundLossFloor,
                Some("immediate_exit_impact_exceeds_5pct"),
                entry_landing,
                entry_total_debit_lamports,
                Some(entry_impact_bps),
                Some(value),
                0,
                0,
            )
        }
        None => {
            return post_entry_loss_floor(
                InternalTerminalStatus::PostEntryValidityBoundLossFloor,
                Some("immediate_exit_impact_unavailable"),
                entry_landing,
                entry_total_debit_lamports,
                Some(entry_impact_bps),
                None,
                0,
                0,
            )
        }
    };

    let creator = plan.birth.payload.creator.as_str();
    let mut creator_sells = plan
        .trades
        .iter()
        .filter_map(|trade| post_entry_creator_sell(trade, creator, entry_landing))
        .collect::<Vec<_>>();
    creator_sells.sort_by(compare_exit_trigger);
    let mut states = plan
        .states
        .iter()
        .copied()
        .filter(|state| compare_state_after_entry(*state, entry_landing))
        .collect::<Vec<_>>();
    states.sort_by(compare_state_arrival);
    let max_hold_trigger = ExitTrigger {
        kind: ExitTriggerKind::MaxHold,
        event_ts_ms: entry_landing.event_ts_ms.saturating_add(MAX_HOLD_MS),
        arrival_ts_ms: entry_landing.arrival_ts_ms.saturating_add(MAX_HOLD_MS),
        slot: entry_landing.slot,
        order: None,
    };
    let mut state_index = 0usize;
    let mut creator_sell_index = 0usize;
    let mut failed_take_profit_attempts = 0u8;
    let mut cumulative_failed_exit_cost_lamports = 0u64;

    loop {
        let next_creator = creator_sells
            .get(creator_sell_index)
            .copied()
            .filter(|trigger| trigger.arrival_ts_ms <= max_hold_trigger.arrival_ts_ms);
        // Consume neutral reserve observations before choosing a later action.
        // Otherwise a single non-triggering state would make max-hold fire
        // early and silently bypass a later hard-loss or take-profit trigger.
        let mut state_trigger = None;
        loop {
            let next_state = states
                .get(state_index)
                .copied()
                .filter(|state| state.arrival_ts_ms <= max_hold_trigger.arrival_ts_ms);
            let Some(state) = next_state else {
                break;
            };

            if state.complete {
                if next_creator.map_or(true, |creator_trigger| {
                    state.arrival_ts_ms <= creator_trigger.arrival_ts_ms
                }) {
                    return post_entry_loss_floor(
                        InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor,
                        Some("unsupported_route"),
                        entry_landing,
                        entry_total_debit_lamports,
                        Some(entry_impact_bps),
                        Some(immediate_exit_impact_bps),
                        failed_take_profit_attempts,
                        cumulative_failed_exit_cost_lamports,
                    );
                }
                break;
            }

            match hypothetical_exit_return(
                quote_contract,
                state,
                entry_quote.token_amount,
                entry_total_debit_lamports,
                cumulative_failed_exit_cost_lamports,
            ) {
                Ok(return_value) if return_value <= HARD_LOSS_BPS as f64 / 10_000.0 => {
                    state_trigger = Some(ExitTrigger {
                        kind: ExitTriggerKind::HardLoss,
                        event_ts_ms: state.event_ts_ms,
                        arrival_ts_ms: state.arrival_ts_ms,
                        slot: state.slot,
                        order: None,
                    });
                    break;
                }
                Ok(return_value)
                    if failed_take_profit_attempts < MAX_TAKE_PROFIT_ATTEMPTS
                        && return_value >= TAKE_PROFIT_BPS as f64 / 10_000.0 =>
                {
                    state_trigger = Some(ExitTrigger {
                        kind: ExitTriggerKind::TakeProfit,
                        event_ts_ms: state.event_ts_ms,
                        arrival_ts_ms: state.arrival_ts_ms,
                        slot: state.slot,
                        order: None,
                    });
                    break;
                }
                Ok(_) => state_index = state_index.saturating_add(1),
                Err(_) => {
                    if next_creator.map_or(true, |creator_trigger| {
                        state.arrival_ts_ms <= creator_trigger.arrival_ts_ms
                    }) {
                        return post_entry_loss_floor(
                            InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor,
                            Some("typed_exit_route_unavailable"),
                            entry_landing,
                            entry_total_debit_lamports,
                            Some(entry_impact_bps),
                            Some(immediate_exit_impact_bps),
                            failed_take_profit_attempts,
                            cumulative_failed_exit_cost_lamports,
                        );
                    }
                    break;
                }
            }
        }

        let selected = match [next_creator, state_trigger]
            .into_iter()
            .flatten()
            .min_by(compare_exit_trigger)
        {
            Some(trigger)
                if compare_exit_trigger(&trigger, &max_hold_trigger) != Ordering::Greater =>
            {
                trigger
            }
            _ => max_hold_trigger,
        };
        match selected.kind {
            ExitTriggerKind::CreatorSell => {
                creator_sell_index = creator_sell_index.saturating_add(1)
            }
            ExitTriggerKind::HardLoss | ExitTriggerKind::TakeProfit => {
                // Advance exactly the observed state that supplied this
                // trigger.  A failed TP does not get retried on the same
                // state; a later state can produce the next attempt.
                if states.get(state_index).is_some_and(|state| {
                    state.event_ts_ms == selected.event_ts_ms
                        && state.arrival_ts_ms == selected.arrival_ts_ms
                        && state.slot == selected.slot
                }) {
                    state_index = state_index.saturating_add(1);
                }
            }
            ExitTriggerKind::MaxHold => {}
        }
        match execute_exit_attempt(
            plan,
            quote_contract,
            entry_quote.token_amount,
            entry_total_debit_lamports,
            cumulative_failed_exit_cost_lamports,
            selected,
            exit_latency_ms,
        ) {
            ExitAttemptResult::Filled {
                net_pnl_lamports,
                net_return,
                landing,
                profit17_hit,
            } => {
                return TerminalOutcome {
                    status: InternalTerminalStatus::EntryFilledExitFilled,
                    subtype: None,
                    terminal_net_pnl_lamports: net_pnl_lamports,
                    terminal_net_return: Some(net_return),
                    profit17_hit,
                    entry_landing: Some(entry_landing),
                    entry_total_debit_lamports: Some(entry_total_debit_lamports),
                    entry_impact_bps: Some(entry_impact_bps),
                    immediate_exit_impact_bps: Some(immediate_exit_impact_bps),
                    exit_reason: selected.kind.as_str(),
                    exit_trigger_event_ts_ms: Some(selected.event_ts_ms),
                    exit_trigger_arrival_ts_ms: Some(selected.arrival_ts_ms),
                    exit_landing_state_slot: Some(landing.slot),
                    failed_take_profit_attempts,
                    cumulative_failed_exit_cost_lamports,
                    post_entry_route_loss: false,
                };
            }
            ExitAttemptResult::FailedTakeProfit => {
                let exit_cost = match quote_contract.exit_transaction_cost_lamports() {
                    Ok(cost) => cost,
                    Err(_) => {
                        return post_entry_loss_floor(
                            InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor,
                            Some("exit_attempt_cost_unavailable"),
                            entry_landing,
                            entry_total_debit_lamports,
                            Some(entry_impact_bps),
                            Some(immediate_exit_impact_bps),
                            failed_take_profit_attempts,
                            cumulative_failed_exit_cost_lamports,
                        )
                    }
                };
                failed_take_profit_attempts = failed_take_profit_attempts.saturating_add(1);
                cumulative_failed_exit_cost_lamports =
                    cumulative_failed_exit_cost_lamports.saturating_add(exit_cost);
            }
            ExitAttemptResult::RouteLost => {
                return post_entry_loss_floor(
                    InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor,
                    Some("unsupported_route"),
                    entry_landing,
                    entry_total_debit_lamports,
                    Some(entry_impact_bps),
                    Some(immediate_exit_impact_bps),
                    failed_take_profit_attempts,
                    cumulative_failed_exit_cost_lamports,
                );
            }
            ExitAttemptResult::StateUnavailable => {
                return post_entry_loss_floor(
                    InternalTerminalStatus::ExitStateUnavailableLossFloor,
                    None,
                    entry_landing,
                    entry_total_debit_lamports,
                    Some(entry_impact_bps),
                    Some(immediate_exit_impact_bps),
                    failed_take_profit_attempts,
                    cumulative_failed_exit_cost_lamports,
                );
            }
        }
    }
}

fn entry_failed_no_landing(entry_attempt_cost: u64) -> TerminalOutcome {
    TerminalOutcome {
        status: InternalTerminalStatus::EntryFailedNoLandingState,
        subtype: None,
        terminal_net_pnl_lamports: -(entry_attempt_cost as i128),
        terminal_net_return: None,
        profit17_hit: false,
        entry_landing: None,
        entry_total_debit_lamports: None,
        entry_impact_bps: None,
        immediate_exit_impact_bps: None,
        exit_reason: "entry_no_landing_state",
        exit_trigger_event_ts_ms: None,
        exit_trigger_arrival_ts_ms: None,
        exit_landing_state_slot: None,
        failed_take_profit_attempts: 0,
        cumulative_failed_exit_cost_lamports: 0,
        post_entry_route_loss: false,
    }
}

fn entry_failed_price_protection(
    entry_landing: Option<DirectState>,
    entry_total_debit_lamports: Option<u64>,
    entry_impact_bps: Option<u32>,
    entry_attempt_cost: u64,
) -> TerminalOutcome {
    TerminalOutcome {
        status: InternalTerminalStatus::EntryFailedPriceProtection,
        subtype: None,
        terminal_net_pnl_lamports: -(entry_attempt_cost as i128),
        terminal_net_return: None,
        profit17_hit: false,
        entry_landing,
        entry_total_debit_lamports,
        entry_impact_bps,
        immediate_exit_impact_bps: None,
        exit_reason: "entry_price_protection",
        exit_trigger_event_ts_ms: None,
        exit_trigger_arrival_ts_ms: None,
        exit_landing_state_slot: None,
        failed_take_profit_attempts: 0,
        cumulative_failed_exit_cost_lamports: 0,
        post_entry_route_loss: false,
    }
}

fn post_entry_loss_floor(
    status: InternalTerminalStatus,
    subtype: Option<&'static str>,
    entry_landing: DirectState,
    entry_total_debit_lamports: u64,
    entry_impact_bps: Option<u32>,
    immediate_exit_impact_bps: Option<u32>,
    failed_take_profit_attempts: u8,
    cumulative_failed_exit_cost_lamports: u64,
) -> TerminalOutcome {
    let loss = entry_total_debit_lamports.saturating_add(cumulative_failed_exit_cost_lamports);
    TerminalOutcome {
        status,
        subtype,
        terminal_net_pnl_lamports: -(loss as i128),
        terminal_net_return: Some(-(loss as f64) / entry_total_debit_lamports as f64),
        profit17_hit: false,
        entry_landing: Some(entry_landing),
        entry_total_debit_lamports: Some(entry_total_debit_lamports),
        entry_impact_bps,
        immediate_exit_impact_bps,
        exit_reason: match status {
            InternalTerminalStatus::PostEntryValidityBoundLossFloor => {
                "post_entry_validity_bound_loss_floor"
            }
            InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor => {
                "unsupported_route_loss_floor"
            }
            InternalTerminalStatus::ExitStateUnavailableLossFloor => {
                "exit_state_unavailable_loss_floor"
            }
            InternalTerminalStatus::EntryFilledExitFilled
            | InternalTerminalStatus::EntryFailedPriceProtection
            | InternalTerminalStatus::EntryFailedNoLandingState => {
                unreachable!("post_entry_loss_floor requires a post-entry terminal status")
            }
        },
        exit_trigger_event_ts_ms: None,
        exit_trigger_arrival_ts_ms: None,
        exit_landing_state_slot: None,
        failed_take_profit_attempts,
        cumulative_failed_exit_cost_lamports,
        post_entry_route_loss: status == InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor,
    }
}

fn compare_state_after_entry(state: DirectState, entry: DirectState) -> bool {
    // Arrival after entry means the evaluator could observe this state only
    // after the fill.  It is insufficient on its own: a delayed historical
    // account write must never be quoted as the post-entry curve.  The state
    // also has to advance the canonical chain-order axis.
    state.arrival_ts_ms > entry.arrival_ts_ms
        && compare_state_chain(&state, &entry) == Ordering::Greater
}

fn post_entry_creator_sell(
    trade: &tape::TapeTrade,
    creator: &str,
    entry: DirectState,
) -> Option<ExitTrigger> {
    let payload = &trade.payload;
    if !payload.success
        || payload.is_buy
        || !payload.side.eq_ignore_ascii_case("sell")
        || payload.is_synthetic != Some(false)
        || tape::non_empty(&payload.signer)? != creator
        || payload.arrival_ts_ms <= entry.arrival_ts_ms
    {
        return None;
    }
    let order = tape::canonical_trade_order(payload)?;
    Some(ExitTrigger {
        kind: ExitTriggerKind::CreatorSell,
        event_ts_ms: payload.event_ts_ms,
        arrival_ts_ms: payload.arrival_ts_ms,
        slot: order.slot,
        order: Some(order),
    })
}

fn compare_exit_trigger(left: &ExitTrigger, right: &ExitTrigger) -> Ordering {
    (
        left.arrival_ts_ms,
        left.kind.priority(),
        left.event_ts_ms,
        left.slot,
        left.order,
    )
        .cmp(&(
            right.arrival_ts_ms,
            right.kind.priority(),
            right.event_ts_ms,
            right.slot,
            right.order,
        ))
}

fn find_landed_state(
    states: &[DirectState],
    trigger_event_ts_ms: u64,
    trigger_arrival_ts_ms: u64,
    latency_ms: u64,
    max_lag_ms: u64,
) -> LandingResult {
    let event_lower = trigger_event_ts_ms.saturating_add(latency_ms);
    let event_upper = event_lower.saturating_add(max_lag_ms);
    let arrival_lower = trigger_arrival_ts_ms.saturating_add(latency_ms);
    let arrival_upper = arrival_lower.saturating_add(max_lag_ms);
    let mut candidates = states
        .iter()
        .copied()
        .filter(|state| {
            (event_lower..=event_upper).contains(&state.event_ts_ms)
                && (arrival_lower..=arrival_upper).contains(&state.arrival_ts_ms)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(compare_state_arrival);
    match candidates.first().copied() {
        Some(state) if state.complete => LandingResult::RouteLost,
        Some(state) => LandingResult::Landed(state),
        None => {
            let later_complete = states.iter().any(|state| {
                state.complete
                    && state.event_ts_ms >= trigger_event_ts_ms
                    && state.arrival_ts_ms >= trigger_arrival_ts_ms
            });
            if later_complete {
                LandingResult::RouteLost
            } else {
                LandingResult::Unavailable
            }
        }
    }
}

fn hypothetical_exit_return(
    quote_contract: &RugScalpPumpQuoteContractV1,
    state: DirectState,
    token_amount: u64,
    entry_total_debit_lamports: u64,
    cumulative_failed_exit_cost_lamports: u64,
) -> Result<f64> {
    let quote = quote_contract.quote_full_position_exit_with_min_program_credit(
        state.slot,
        state.reserves,
        token_amount,
        0,
    )?;
    if !quote.instruction_limit_check.passed {
        bail!("unprotected full-position exit quote failed")
    }
    let exit_cost = quote_contract.exit_transaction_cost_lamports()?;
    let credit = quote.program_settlement.wallet_debit_or_credit;
    let pnl = i128::from(credit)
        - i128::from(exit_cost)
        - i128::from(cumulative_failed_exit_cost_lamports)
        - i128::from(entry_total_debit_lamports);
    Ok(pnl as f64 / entry_total_debit_lamports as f64)
}

fn execute_exit_attempt(
    plan: &PreEntryPlan,
    quote_contract: &RugScalpPumpQuoteContractV1,
    token_amount: u64,
    entry_total_debit_lamports: u64,
    cumulative_failed_exit_cost_lamports: u64,
    trigger: ExitTrigger,
    exit_latency_ms: u64,
) -> ExitAttemptResult {
    let landing = match find_landed_state(
        &plan.states,
        trigger.event_ts_ms,
        trigger.arrival_ts_ms,
        exit_latency_ms,
        EXIT_LANDING_MAX_LAG_MS,
    ) {
        LandingResult::Landed(state) => state,
        LandingResult::RouteLost => return ExitAttemptResult::RouteLost,
        LandingResult::Unavailable => return ExitAttemptResult::StateUnavailable,
    };
    let exit_cost = match quote_contract.exit_transaction_cost_lamports() {
        Ok(cost) => cost,
        Err(_) => return ExitAttemptResult::RouteLost,
    };
    let min_program_credit = if trigger.kind == ExitTriggerKind::TakeProfit {
        let required_net = match ceil_mul_bps(entry_total_debit_lamports, 10_000 + TAKE_PROFIT_BPS)
        {
            Some(value) => value,
            None => return ExitAttemptResult::RouteLost,
        };
        match required_net
            .checked_add(cumulative_failed_exit_cost_lamports)
            .and_then(|value| value.checked_add(exit_cost))
        {
            Some(value) => value,
            None => return ExitAttemptResult::RouteLost,
        }
    } else {
        0
    };
    let quote = match quote_contract.quote_full_position_exit_with_min_program_credit(
        landing.slot,
        landing.reserves,
        token_amount,
        min_program_credit,
    ) {
        Ok(quote) => quote,
        Err(_) => return ExitAttemptResult::RouteLost,
    };
    if !quote.instruction_limit_check.passed {
        return if trigger.kind == ExitTriggerKind::TakeProfit {
            ExitAttemptResult::FailedTakeProfit
        } else {
            ExitAttemptResult::RouteLost
        };
    }
    let credit = quote.program_settlement.wallet_debit_or_credit;
    let net_pnl_lamports = i128::from(credit)
        - i128::from(exit_cost)
        - i128::from(cumulative_failed_exit_cost_lamports)
        - i128::from(entry_total_debit_lamports);
    let net_return = net_pnl_lamports as f64 / entry_total_debit_lamports as f64;
    ExitAttemptResult::Filled {
        net_pnl_lamports,
        net_return,
        landing,
        profit17_hit: trigger.kind == ExitTriggerKind::TakeProfit
            && net_return >= TAKE_PROFIT_BPS as f64 / 10_000.0,
    }
}

fn ceil_mul_bps(value: u64, bps: u64) -> Option<u64> {
    let numerator = u128::from(value).checked_mul(u128::from(bps))?;
    let rounded = numerator.checked_add(9_999)?.checked_div(10_000)?;
    u64::try_from(rounded).ok()
}

fn outcome_row(
    plan: &PreEntryPlan,
    enrollment_index: usize,
    outcome: TerminalOutcome,
    stress: TerminalOutcome,
    capture_kind: AceEvV2CaptureKind,
) -> AceEvV2CandidateOutcomeV1 {
    let split = split_for_capture_kind(capture_kind, enrollment_index);
    let stress_latency_1s = AceEvV2StressOutcomeV1 {
        entry_latency_ms: 1_000,
        exit_latency_ms: 1_000,
        terminal_status: stress.status.as_str().to_string(),
        terminal_net_pnl_lamports: stress.terminal_net_pnl_lamports,
        terminal_net_pnl_sol: stress.terminal_net_pnl_lamports as f64 / LAMPORTS_PER_SOL,
        profit17_hit: stress.profit17_hit,
        exit_reason: stress.exit_reason.to_string(),
    };
    AceEvV2CandidateOutcomeV1 {
        schema: ACE_EV_V2_OUTCOME_SCHEMA.to_string(),
        estimand_id: ACE_EV_V2_ESTIMAND_ID.to_string(),
        enrollment_index,
        split: split.to_string(),
        candidate_id: plan.birth.candidate_id.clone(),
        base_mint: plan.birth.payload.base_mint.clone(),
        bonding_curve: plan.birth.payload.bonding_curve.clone(),
        creator: plan.birth.payload.creator.clone(),
        candidate_order: candidate_order_for(&plan.birth),
        birth_ts_ms: plan.birth.payload.birth_ts_ms,
        decision_event_cutoff_ms: plan.cutoffs.event_cutoff_ts_ms,
        decision_ingress_cutoff_ms: plan.cutoffs.ingress_cutoff_ts_ms,
        feature_vector: plan.features,
        normalized_features: plan.normalized_features,
        decision_state_slot: plan.decision_state.slot,
        entry_landing_state_slot: outcome.entry_landing.map(|state| state.slot),
        entry_status: outcome.status.entry_status().to_string(),
        fixed_token_amount_raw: plan.fixed_token_amount_raw,
        fixed_max_sol_cost_lamports: plan.fixed_max_sol_cost_lamports,
        entry_total_debit_lamports: outcome.entry_total_debit_lamports,
        entry_impact_bps: outcome.entry_impact_bps,
        immediate_exit_impact_bps: outcome.immediate_exit_impact_bps,
        entry_landed_arrival_ts_ms: outcome.entry_landing.map(|state| state.arrival_ts_ms),
        terminal_status: outcome.status.as_str().to_string(),
        terminal_status_subtype: outcome.subtype.map(str::to_string),
        terminal_net_pnl_lamports: outcome.terminal_net_pnl_lamports,
        terminal_net_pnl_sol: outcome.terminal_net_pnl_lamports as f64 / LAMPORTS_PER_SOL,
        terminal_net_return: outcome.terminal_net_return,
        profit17_hit: outcome.profit17_hit,
        exit_reason: outcome.exit_reason.to_string(),
        exit_trigger_event_ts_ms: outcome.exit_trigger_event_ts_ms,
        exit_trigger_arrival_ts_ms: outcome.exit_trigger_arrival_ts_ms,
        exit_landing_state_slot: outcome.exit_landing_state_slot,
        failed_take_profit_attempts: outcome.failed_take_profit_attempts,
        cumulative_failed_exit_cost_lamports: outcome.cumulative_failed_exit_cost_lamports,
        post_entry_route_loss: outcome.post_entry_route_loss,
        stress_latency_1s,
    }
}

fn split_for_capture_kind(
    capture_kind: AceEvV2CaptureKind,
    enrollment_index: usize,
) -> &'static str {
    let (train_rows, threshold_rows) = match capture_kind {
        AceEvV2CaptureKind::YieldQualification => (TRAIN_ROWS, THRESHOLD_CALIBRATION_ROWS),
        AceEvV2CaptureKind::Prospective1000 => (PROSPECTIVE_TRAIN_ROWS, PROSPECTIVE_THRESHOLD_ROWS),
    };
    if enrollment_index <= train_rows {
        "TRAIN"
    } else if enrollment_index <= train_rows + threshold_rows {
        "THRESHOLD_CALIBRATION"
    } else {
        "UNTOUCHED_TEST"
    }
}

fn count_terminal_statuses(rows: &[AceEvV2CandidateOutcomeV1]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(row.terminal_status.clone()).or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::{
        FeeRounding, ProgramFeeRule, ProgramFeeSchedule, ProgramFeeScheduleEvidenceV1,
        PumpRouteVariant, TransactionCosts,
    };

    use crate::rug_scalp_v2::{
        RugScalpPumpFeeScheduleV1, RugScalpPumpQuoteAuthorityV1, RUG_SCALP_ENTRY_ROUTE,
        RUG_SCALP_EXIT_ROUTE,
    };

    fn birth() -> tape::TapeBirth {
        tape::TapeBirth {
            candidate_id: "candidate".to_string(),
            payload: NewPoolDetectedPayload {
                is_birth_event: true,
                pool_amm_id: "curve".to_string(),
                pool_id: "curve".to_string(),
                base_mint: "mint".to_string(),
                mint_id: "mint".to_string(),
                quote_mint: tape::WSOL_MINT.to_string(),
                bonding_curve: "curve".to_string(),
                creator: "creator".to_string(),
                amm_program: crate::rug_scalp_v2::RUG_SCALP_PUMP_PROGRAM.to_string(),
                signature: "birth-signature".to_string(),
                birth_ts_ms: 1_000,
                timestamp_ms: 1_000,
                event_slot: Some(1),
                detected_wall_ts_ms: Some(1_000),
                chain_event_ts_ms: Some(1_000),
                source: "primary".to_string(),
            },
            event_id: "birth-event".to_string(),
            file_ordinal: 0,
            line_number: 1,
        }
    }

    fn trade(
        event_ts_ms: u64,
        arrival_ts_ms: u64,
        signer: &str,
        slot: u64,
        ordinal: u32,
        debit_lamports: u64,
    ) -> tape::TapeTrade {
        tape::TapeTrade {
            payload: PoolTransactionPayload {
                schema_version: tape::POOL_TRANSACTION_PAYLOAD_SCHEMA_V1.to_string(),
                pool_amm_id: "curve".to_string(),
                pool_id: "curve".to_string(),
                source_pool_amm_id: None,
                base_mint: Some("mint".to_string()),
                mint_id: Some("mint".to_string()),
                token_mint: Some("mint".to_string()),
                quote_mint: Some(tape::WSOL_MINT.to_string()),
                bonding_curve: "curve".to_string(),
                signature: format!("trade-{ordinal}"),
                event_slot: Some(slot),
                slot: Some(slot),
                tx_index: Some(ordinal),
                event_ordinal: Some(ordinal),
                outer_instruction_index: Some(0),
                inner_group_index: Some(0),
                event_ts_ms,
                timestamp_ms: event_ts_ms,
                arrival_ts_ms,
                source: "primary".to_string(),
                side: "buy".to_string(),
                is_buy: true,
                success: true,
                error_code: None,
                signer: signer.to_string(),
                wallet: signer.to_string(),
                signer_pre_balance_lamports: Some(2_000_000_000 + debit_lamports),
                signer_post_balance_lamports: Some(2_000_000_000),
                is_synthetic: Some(false),
                quote_amount_sol: 0.0,
                volume_sol: 0.0,
                sol_amount_lamports: None,
                effective_curve_quote_lamports: None,
                token_amount_units: None,
                virtual_sol_reserves: None,
                virtual_token_reserves: None,
                real_sol_reserves: None,
                real_token_reserves: None,
                complete: None,
                reserve_base: None,
                reserve_quote: None,
                price_quote: None,
                v_tokens_in_bonding_curve: None,
                v_sol_in_bonding_curve: None,
                market_cap_sol: None,
                curve_progress_pct: None,
                curve_progress_status: "unknown".to_string(),
                curve_finality: "canonical".to_string(),
                curve_data_known: false,
                execution_account_contract_status: "not_used_by_ace_ev_v2".to_string(),
                execution_account_contract_reason: None,
            },
            event_id: format!("trade-event-{ordinal}"),
            file_ordinal: 0,
            line_number: ordinal as usize + 2,
        }
    }

    fn direct_state(event_ts_ms: u64, arrival_ts_ms: u64, slot: u64) -> DirectState {
        DirectState {
            event_ts_ms,
            arrival_ts_ms,
            slot,
            write_version: Some(slot),
            sequence_number: slot,
            ordinal: (0, slot as usize),
            complete: false,
            reserves: PumpReserveState {
                virtual_base_reserves: 1_000_000_000_000,
                virtual_quote_reserves: 10_000_000_000,
                real_base_reserves: 1_000_000_000_000,
                real_quote_reserves: 10_000_000_000,
            },
        }
    }

    fn direct_state_with_reserves(
        event_ts_ms: u64,
        arrival_ts_ms: u64,
        slot: u64,
        virtual_base_reserves: u64,
        virtual_quote_reserves: u64,
    ) -> DirectState {
        DirectState {
            reserves: PumpReserveState {
                virtual_base_reserves,
                virtual_quote_reserves,
                real_base_reserves: virtual_base_reserves,
                real_quote_reserves: virtual_quote_reserves,
            },
            ..direct_state(event_ts_ms, arrival_ts_ms, slot)
        }
    }

    fn test_quote_schedule(route_variant: PumpRouteVariant, id: &str) -> RugScalpPumpFeeScheduleV1 {
        RugScalpPumpFeeScheduleV1 {
            route_variant,
            schedule: ProgramFeeSchedule {
                fee_schedule_id: id.to_string(),
                effective_slot: 0,
                evidence: ProgramFeeScheduleEvidenceV1::OnChainConfig {
                    config_pubkey: format!("fixture-{id}"),
                    owner_program: crate::rug_scalp_v2::RUG_SCALP_PUMP_PROGRAM.to_string(),
                    account_data_hash: format!("fixture-hash-{id}"),
                    observed_slot: 0,
                },
                rules: vec![ProgramFeeRule {
                    component_id: "fixture-fee".to_string(),
                    numerator: 1,
                    denominator: 10_000,
                    rounding: FeeRounding::Ceil,
                }],
            },
        }
    }

    fn test_quote_contract() -> RugScalpPumpQuoteContractV1 {
        test_quote_contract_with_exit_effective_slot(0)
    }

    fn test_quote_contract_with_exit_effective_slot(
        exit_effective_slot: u64,
    ) -> RugScalpPumpQuoteContractV1 {
        let mut exit_schedule = test_quote_schedule(RUG_SCALP_EXIT_ROUTE, "legacy-sell");
        exit_schedule.schedule.effective_slot = exit_effective_slot;
        if let ProgramFeeScheduleEvidenceV1::OnChainConfig { observed_slot, .. } =
            &mut exit_schedule.schedule.evidence
        {
            *observed_slot = exit_effective_slot;
        }
        RugScalpPumpQuoteAuthorityV1 {
            schedules: vec![
                test_quote_schedule(RUG_SCALP_ENTRY_ROUTE, "buy-v2"),
                exit_schedule,
            ],
            entry_transaction_costs: TransactionCosts::default(),
            exit_transaction_costs: TransactionCosts::default(),
        }
        .materialize()
        .expect("fixture quote authority materializes")
    }

    fn plan_for_landed_entry(
        entry_landing: DirectState,
        fixed_token_amount_raw: u64,
        fixed_max_sol_cost_lamports: u64,
    ) -> PreEntryPlan {
        PreEntryPlan {
            birth: birth(),
            trades: Vec::new(),
            states: vec![entry_landing],
            cutoffs: tape::DecisionCutoffs {
                event_cutoff_ts_ms: 11_111,
                ingress_cutoff_ts_ms: 11_111,
            },
            features: AceEvV2FeatureVectorV1 {
                f1_log_unique_first_buyers: 0.0,
                f2_log_total_first_buy_flow: 0.0,
                f3_buyer_acceleration: 0.0,
                f4_creator_buy_share: 0.0,
                f5_first_buy_flow_hhi: 0.0,
                f6_same_slot_first_buy_flow_share: 0.0,
                f7_pre_cutoff_sell_buy_log_ratio: 0.0,
            },
            normalized_features: [0.0; 7],
            decision_state: entry_landing,
            fixed_token_amount_raw,
            fixed_max_sol_cost_lamports,
            decision_entry_total_debit_lamports: 0,
            decision_entry_impact_bps: 0,
            decision_immediate_exit_impact_bps: 0,
        }
    }

    fn terminal_candidate(index: usize) -> AceEvV2CandidateOutcomeV1 {
        let candidate_id = format!("candidate-{index:04}");
        let base_mint = format!("mint-{index:04}");
        let bonding_curve = format!("curve-{index:04}");
        AceEvV2CandidateOutcomeV1 {
            schema: ACE_EV_V2_OUTCOME_SCHEMA.to_string(),
            estimand_id: ACE_EV_V2_ESTIMAND_ID.to_string(),
            enrollment_index: index,
            split: split_for_capture_kind(AceEvV2CaptureKind::Prospective1000, index).to_string(),
            candidate_id,
            base_mint: base_mint.clone(),
            bonding_curve: bonding_curve.clone(),
            creator: "creator".to_string(),
            candidate_order: CandidateOrderV1 {
                decision_ingress_cutoff_ms: index as u64,
                birth_ts_ms: index as u64,
                event_slot: index as u64,
                bonding_curve,
                base_mint,
            },
            birth_ts_ms: index as u64,
            decision_event_cutoff_ms: index as u64,
            decision_ingress_cutoff_ms: index as u64,
            feature_vector: AceEvV2FeatureVectorV1 {
                f1_log_unique_first_buyers: 0.0,
                f2_log_total_first_buy_flow: 0.0,
                f3_buyer_acceleration: 0.0,
                f4_creator_buy_share: 0.0,
                f5_first_buy_flow_hhi: 0.0,
                f6_same_slot_first_buy_flow_share: 0.0,
                f7_pre_cutoff_sell_buy_log_ratio: 0.0,
            },
            normalized_features: [0.0; 7],
            decision_state_slot: index as u64,
            entry_landing_state_slot: None,
            entry_status: "ENTRY_FAILED_NO_LANDING_STATE".to_string(),
            fixed_token_amount_raw: 0,
            fixed_max_sol_cost_lamports: 0,
            entry_total_debit_lamports: None,
            entry_impact_bps: None,
            immediate_exit_impact_bps: None,
            entry_landed_arrival_ts_ms: None,
            terminal_status: "ENTRY_FAILED_NO_LANDING_STATE".to_string(),
            terminal_status_subtype: None,
            terminal_net_pnl_lamports: 0,
            terminal_net_pnl_sol: 0.0,
            terminal_net_return: None,
            profit17_hit: false,
            exit_reason: "entry_failed_no_landing_state".to_string(),
            exit_trigger_event_ts_ms: None,
            exit_trigger_arrival_ts_ms: None,
            exit_landing_state_slot: None,
            failed_take_profit_attempts: 0,
            cumulative_failed_exit_cost_lamports: 0,
            post_entry_route_loss: false,
            stress_latency_1s: AceEvV2StressOutcomeV1 {
                entry_latency_ms: 1_000,
                exit_latency_ms: 1_000,
                terminal_status: "ENTRY_FAILED_NO_LANDING_STATE".to_string(),
                terminal_net_pnl_lamports: 0,
                terminal_net_pnl_sol: 0.0,
                profit17_hit: false,
                exit_reason: "entry_failed_no_landing_state".to_string(),
            },
        }
    }

    #[test]
    fn prospective_capture_kind_serializes_to_the_frozen_model_contract_name() {
        assert_eq!(
            serde_json::to_string(&AceEvV2CaptureKind::Prospective1000)
                .expect("serialize prospective capture kind"),
            "\"prospective_1000\""
        );
        assert_eq!(
            "prospective_1000"
                .parse::<AceEvV2CaptureKind>()
                .expect("parse frozen prospective capture kind"),
            AceEvV2CaptureKind::Prospective1000
        );
    }

    #[test]
    fn one_and_two_first_buy_wallets_produce_full_f1_through_f7() {
        let birth = birth();
        let cutoff = birth.payload.birth_ts_ms + FEATURE_CUTOFF_MS;
        let one = vec![trade(
            cutoff - 500,
            cutoff - 500,
            "wallet-a",
            10,
            1,
            4_000_000,
        )];
        let (one_features, _) = calculate_features_v2(&birth, &one).expect("one buyer is eligible");
        assert!((one_features.f1_log_unique_first_buyers - 2.0_f64.ln()).abs() < 1e-12);
        assert_eq!(one_features.f5_first_buy_flow_hhi, 1.0);
        assert_eq!(one_features.f6_same_slot_first_buy_flow_share, 1.0);
        assert!(one_features.values().into_iter().all(f64::is_finite));

        let two = vec![
            trade(cutoff - 2_000, cutoff - 2_000, "wallet-a", 10, 1, 4_000_000),
            trade(cutoff - 500, cutoff - 500, "wallet-b", 11, 2, 2_000_000),
        ];
        let (two_features, _) =
            calculate_features_v2(&birth, &two).expect("two buyers are eligible");
        assert!((two_features.f1_log_unique_first_buyers - 3.0_f64.ln()).abs() < 1e-12);
        assert!(two_features.f5_first_buy_flow_hhi < 1.0);
        assert!(two_features.f6_same_slot_first_buy_flow_share < 1.0);
        assert!(two_features.values().into_iter().all(f64::is_finite));
    }

    #[test]
    fn creator_sell_before_dual_cutoff_is_hard_pre_entry_reject() {
        let birth = birth();
        let cutoff = birth.payload.birth_ts_ms + FEATURE_CUTOFF_MS;
        let mut creator_sell = trade(cutoff - 1, cutoff - 1, "creator", 10, 1, 1);
        creator_sell.payload.side = "sell".to_string();
        creator_sell.payload.is_buy = false;
        assert!(has_pre_entry_creator_sell_veto(&birth, &[creator_sell]).expect("veto lookup"));
    }

    #[test]
    fn delayed_pre_cutoff_trade_never_changes_v2_features() {
        let birth = birth();
        let cutoff = birth.payload.birth_ts_ms + FEATURE_CUTOFF_MS;
        let mut trades = vec![trade(
            cutoff - 1_000,
            cutoff - 1_000,
            "wallet-a",
            10,
            1,
            3_000_000,
        )];
        let expected = calculate_features_v2(&birth, &trades).expect("baseline features");
        let ingress_cutoff = birth.payload.detected_wall_ts_ms.unwrap() + FEATURE_CUTOFF_MS;
        trades.push(trade(
            cutoff - 10,
            ingress_cutoff + 1,
            "wallet-b",
            11,
            2,
            9_000_000,
        ));
        assert_eq!(
            calculate_features_v2(&birth, &trades)
                .expect("delayed row is ignored")
                .0
                .values(),
            expected.0.values()
        );
    }

    #[test]
    fn landed_entry_requires_both_clock_axes_after_250ms() {
        let states = vec![
            direct_state(11_360, 11_360, 2),
            direct_state(11_361, 11_361, 3),
        ];
        assert!(matches!(
            find_landed_state(&states, 11_111, 11_111, 250, 1_000),
            LandingResult::Landed(state) if state.slot == 3
        ));

        let arrival_late = vec![direct_state(11_361, 11_360, 4)];
        assert!(matches!(
            find_landed_state(&arrival_late, 11_111, 11_111, 250, 1_000),
            LandingResult::Unavailable
        ));
    }

    #[test]
    fn landed_buy_v2_over_5pct_impact_is_filled_validity_loss_floor_not_entry_failure() {
        let quote_contract = test_quote_contract();
        // A 0.15 SOL capped BuyV2 against a 5 SOL virtual quote reserve is a
        // legal instruction, but its landed self-impact is deliberately above
        // the frozen 5% validity bound.
        let entry_landing =
            direct_state_with_reserves(11_361, 11_361, 10, 1_000_000_000_000, 5_000_000_000);
        let entry_cost = quote_contract
            .entry_transaction_cost_lamports()
            .expect("fixture entry transaction cost");
        let quote = quote_contract
            .quote_buy_v2_under_wallet_cap(
                entry_landing.slot,
                entry_landing.reserves,
                ENTRY_TOTAL_WALLET_DEBIT_CAP_LAMPORTS - entry_cost,
            )
            .expect("landed BuyV2 quote");
        assert!(quote.instruction_limit_check.passed);
        let plan = plan_for_landed_entry(
            entry_landing,
            quote.token_amount,
            quote.instruction_limit_check.limit,
        );

        let outcome =
            simulate_terminal_outcome(&plan, &quote_contract, ENTRY_LATENCY_MS, EXIT_LATENCY_MS);

        assert_eq!(
            outcome.status,
            InternalTerminalStatus::PostEntryValidityBoundLossFloor
        );
        assert_eq!(outcome.status.entry_status(), "ENTRY_FILLED");
        assert_eq!(outcome.subtype, Some("landed_entry_impact_exceeds_5pct"));
        assert_eq!(
            outcome.terminal_net_pnl_lamports,
            -(outcome.entry_total_debit_lamports.expect("landed fill") as i128)
        );
    }

    #[test]
    fn landed_buy_v2_with_unavailable_immediate_sell_is_route_loss_floor() {
        // BuyV2 authority exists at slot 10, but LegacySell authority starts
        // only at a future slot. The landed BuyV2 must remain filled; the
        // immediate exit-route failure is post-entry, not a rewritable entry
        // attempt failure.
        let quote_contract = test_quote_contract_with_exit_effective_slot(11);
        let entry_landing = direct_state(11_361, 11_361, 10);
        let entry_cost = quote_contract
            .entry_transaction_cost_lamports()
            .expect("fixture entry transaction cost");
        let quote = quote_contract
            .quote_buy_v2_under_wallet_cap(
                entry_landing.slot,
                entry_landing.reserves,
                ENTRY_TOTAL_WALLET_DEBIT_CAP_LAMPORTS - entry_cost,
            )
            .expect("landed BuyV2 quote");
        assert!(quote.instruction_limit_check.passed);
        let plan = plan_for_landed_entry(
            entry_landing,
            quote.token_amount,
            quote.instruction_limit_check.limit,
        );
        let outcome =
            simulate_terminal_outcome(&plan, &quote_contract, ENTRY_LATENCY_MS, EXIT_LATENCY_MS);

        assert_eq!(
            outcome.status,
            InternalTerminalStatus::PostEntryUnsupportedRouteLossFloor
        );
        assert_eq!(outcome.status.entry_status(), "ENTRY_FILLED");
        assert_eq!(outcome.subtype, Some("immediate_exit_quote_unavailable"));
        assert_eq!(
            outcome.terminal_net_pnl_lamports,
            -(outcome.entry_total_debit_lamports.expect("landed fill") as i128)
        );
    }

    #[test]
    fn delayed_historical_reserve_state_cannot_trigger_post_entry_exit() {
        let entry = direct_state(20_000, 20_000, 20);
        // It arrived later, but its canonical state is older in both event
        // time and slot. It must never be quoted as a post-entry curve state.
        let delayed_historical = direct_state(19_999, 20_001, 19);
        assert!(!compare_state_after_entry(delayed_historical, entry));

        let truly_post_entry = direct_state(20_001, 20_001, 21);
        assert!(compare_state_after_entry(truly_post_entry, entry));
    }

    #[test]
    fn incomplete_successful_buy_makes_entire_pre_entry_feature_set_non_evaluable() {
        let birth = birth();
        let cutoff = birth.payload.birth_ts_ms + FEATURE_CUTOFF_MS;
        let complete = trade(cutoff - 2_000, cutoff - 2_000, "wallet-a", 10, 1, 4_000_000);
        let mut incomplete = trade(cutoff - 1_000, cutoff - 1_000, "wallet-b", 11, 2, 2_000_000);
        incomplete.payload.signer_pre_balance_lamports = None;

        assert!(matches!(
            calculate_features_v2(&birth, &[complete, incomplete]),
            Err(reason) if reason == "successful_buy_feature_evidence_incomplete"
        ));
    }

    #[test]
    fn max_hold_uses_first_legal_landed_direct_state() {
        let states = vec![
            direct_state(131_360, 131_360, 4),
            direct_state(131_361, 131_361, 5),
        ];
        assert!(matches!(
            find_landed_state(&states, 131_111, 131_111, 250, 1_000),
            LandingResult::Landed(state) if state.slot == 5
        ));
    }

    #[test]
    fn trigger_order_is_arrival_first_with_creator_sell_priority_at_a_tie() {
        let creator = ExitTrigger {
            kind: ExitTriggerKind::CreatorSell,
            event_ts_ms: 20,
            arrival_ts_ms: 30,
            slot: 2,
            order: None,
        };
        let hard_loss = ExitTrigger {
            kind: ExitTriggerKind::HardLoss,
            event_ts_ms: 19,
            arrival_ts_ms: 30,
            slot: 1,
            order: None,
        };
        let later_take_profit = ExitTrigger {
            kind: ExitTriggerKind::TakeProfit,
            event_ts_ms: 10,
            arrival_ts_ms: 31,
            slot: 1,
            order: None,
        };
        assert_eq!(compare_exit_trigger(&creator, &hard_loss), Ordering::Less);
        assert_eq!(
            compare_exit_trigger(&hard_loss, &later_take_profit),
            Ordering::Less
        );
    }

    #[test]
    fn enrollment_split_is_fixed_by_candidate_order_not_terminal_speed() {
        let mut first = birth();
        first.payload.detected_wall_ts_ms = Some(10);
        first.payload.birth_ts_ms = 100;
        let mut second = birth();
        second.payload.detected_wall_ts_ms = Some(11);
        second.payload.birth_ts_ms = 99;
        assert!(candidate_order_for(&first) < candidate_order_for(&second));
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::YieldQualification, 100),
            "TRAIN"
        );
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::YieldQualification, 101),
            "THRESHOLD_CALIBRATION"
        );
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::YieldQualification, 150),
            "THRESHOLD_CALIBRATION"
        );
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::YieldQualification, 151),
            "UNTOUCHED_TEST"
        );
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::Prospective1000, 400),
            "TRAIN"
        );
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::Prospective1000, 401),
            "THRESHOLD_CALIBRATION"
        );
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::Prospective1000, 600),
            "THRESHOLD_CALIBRATION"
        );
        assert_eq!(
            split_for_capture_kind(AceEvV2CaptureKind::Prospective1000, 601),
            "UNTOUCHED_TEST"
        );
    }

    #[test]
    fn prospective_monitor_writes_no_target_evidence_at_999_and_exact_evidence_at_1000() {
        let rows_999 = (1..=999).map(terminal_candidate).collect::<Vec<_>>();
        assert!(prospective_stop_evidence_for_target(
            "run",
            "implementation",
            "manifest",
            "contract",
            "amendment",
            "scale",
            PROSPECTIVE_ENROLLMENT_LIMIT,
            &rows_999,
            vec![tape::TapeCompletePrefixV1 {
                relative_path: "exec_run_0000.jsonl".to_string(),
                complete_offset: 1,
            }],
            1,
            "monitor",
        )
        .is_none());

        let rows_1000 = (1..=1_000).map(terminal_candidate).collect::<Vec<_>>();
        let evidence = prospective_stop_evidence_for_target(
            "run",
            "implementation",
            "manifest",
            "contract",
            "amendment",
            "scale",
            PROSPECTIVE_ENROLLMENT_LIMIT,
            &rows_1000,
            vec![tape::TapeCompletePrefixV1 {
                relative_path: "exec_run_0000.jsonl".to_string(),
                complete_offset: 1,
            }],
            1,
            "monitor",
        )
        .expect("the 1000th terminal row produces immutable stop evidence");
        assert_eq!(evidence.target_terminal_outcomes, 1_000);
        assert_eq!(evidence.terminal_outcome_count, 1_000);
        assert_eq!(
            evidence.cohort_candidate_order_sha256,
            candidate_cohort_sha256(&rows_1000)
        );
    }

    #[test]
    fn prospective_1001st_terminal_row_cannot_change_first_1000_cohort_hash() {
        let rows_1000 = (1..=1_000).map(terminal_candidate).collect::<Vec<_>>();
        let rows_1001 = (1..=1_001).map(terminal_candidate).collect::<Vec<_>>();
        let from_1000 = prospective_stop_evidence_for_target(
            "run",
            "implementation",
            "manifest",
            "contract",
            "amendment",
            "scale",
            1_000,
            &rows_1000,
            vec![tape::TapeCompletePrefixV1 {
                relative_path: "exec_run_0000.jsonl".to_string(),
                complete_offset: 1,
            }],
            1,
            "monitor",
        )
        .expect("1000 rows");
        let from_1001 = prospective_stop_evidence_for_target(
            "run",
            "implementation",
            "manifest",
            "contract",
            "amendment",
            "scale",
            1_000,
            &rows_1001,
            vec![tape::TapeCompletePrefixV1 {
                relative_path: "exec_run_0000.jsonl".to_string(),
                complete_offset: 2,
            }],
            2,
            "monitor",
        )
        .expect("first 1000 rows remain the target cohort");
        assert_eq!(
            from_1000.cohort_candidate_order_sha256,
            from_1001.cohort_candidate_order_sha256
        );
        assert_ne!(
            candidate_cohort_sha256(&rows_1000),
            candidate_cohort_sha256(&rows_1001)
        );
    }

    #[test]
    fn earlier_pending_candidate_blocks_later_fast_terminal_candidate_from_monitor_stop() {
        let pending = plan_for_landed_entry(direct_state(12_000, 12_000, 1), 1, 1);
        let mut later_fast = pending.clone();
        later_fast.cutoffs.ingress_cutoff_ts_ms = 1;
        let mut plans = vec![&pending];
        plans.extend(std::iter::repeat(&later_fast).take(PROSPECTIVE_ENROLLMENT_LIMIT - 1));
        let required = prospective_cohort_drain_deadline(&plans, PROSPECTIVE_ENROLLMENT_LIMIT)
            .expect("full candidate-order cohort");
        assert!(required > required_outcome_drain_deadline(&later_fast));
        assert!(required > 1);
        // A tape advanced only past the fast later candidate is not allowed to
        // produce target evidence; the earliest pending candidate owns the
        // cohort watermark.
        assert!(1 < required);
    }

    #[test]
    fn final_reconciliation_rejects_tampered_cohort_hash() {
        let rows = (1..=1_000).map(terminal_candidate).collect::<Vec<_>>();
        let evidence = prospective_stop_evidence_for_target(
            "run",
            "implementation",
            "manifest",
            "contract",
            "amendment",
            "scale",
            1_000,
            &rows,
            vec![tape::TapeCompletePrefixV1 {
                relative_path: "exec_run_0000.jsonl".to_string(),
                complete_offset: 1,
            }],
            1,
            "monitor",
        )
        .expect("target evidence");
        let mut tampered = evidence.clone();
        tampered.cohort_candidate_order_sha256 = "tampered".to_string();
        assert!(ensure_prospective_cohort_hash(
            &tampered.cohort_candidate_order_sha256,
            &candidate_cohort_sha256(&rows),
        )
        .is_err());
    }

    #[test]
    fn outcome_drain_requirement_prevents_future_censoring_as_loss_floor() {
        let birth = birth();
        let plan = PreEntryPlan {
            birth,
            trades: Vec::new(),
            states: Vec::new(),
            cutoffs: tape::DecisionCutoffs {
                event_cutoff_ts_ms: 12_111,
                ingress_cutoff_ts_ms: 12_111,
            },
            features: AceEvV2FeatureVectorV1 {
                f1_log_unique_first_buyers: 0.0,
                f2_log_total_first_buy_flow: 0.0,
                f3_buyer_acceleration: 0.0,
                f4_creator_buy_share: 0.0,
                f5_first_buy_flow_hhi: 0.0,
                f6_same_slot_first_buy_flow_share: 0.0,
                f7_pre_cutoff_sell_buy_log_ratio: 0.0,
            },
            normalized_features: [0.0; 7],
            decision_state: direct_state(12_000, 12_000, 1),
            fixed_token_amount_raw: 1,
            fixed_max_sol_cost_lamports: 1,
            decision_entry_total_debit_lamports: 1,
            decision_entry_impact_bps: 0,
            decision_immediate_exit_impact_bps: 0,
        };
        assert_eq!(
            required_outcome_drain_deadline(&plan),
            12_111
                + ENTRY_LATENCY_MS
                + ENTRY_LANDING_MAX_LAG_MS
                + MAX_HOLD_MS
                + EXIT_LATENCY_MS
                + EXIT_LANDING_MAX_LAG_MS
        );
    }
}
