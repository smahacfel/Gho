//! Offline-only ACE Core one-day falsification probe.
//!
//! This module consumes durable evidence after capture completion. It never
//! subscribes to the event bus and never changes runtime decision behavior.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use ghost_brain::events::schema::NewPoolDetectedPayload;
use ghost_brain::events::{EventKind, ExecutionEvent, PoolTransactionPayload};
use ghost_brain::execution::backend::Lane;
use ghost_core::PumpReserveState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::rug_reality_capture::{
    RugRealityCaptureHealthEvidenceV1, RugRealityCaptureRunManifestV1,
};
use crate::rug_scalp_v2::{
    reserves_after_buy, RugScalpPumpQuoteContractV1, RUG_SCALP_ENTRY_ROUTE, RUG_SCALP_EXIT_ROUTE,
    RUG_SCALP_PUMP_PROGRAM,
};

pub const ACE_CORE_ONE_DAY_PROBE_SCHEMA: &str = "ace_core_one_day_probe_v3";
pub const ACE_CORE_CALIBRATION_SCHEMA: &str = "ace_core_one_day_calibration_v1";
pub const ACE_CORE_SUMMARY_SCHEMA: &str = "ace_core_one_day_summary_v1";
pub const ACE_CORE_BASELINE_SHA: &str = "43057b296663129ca9b4f572e793474830a5452c";

const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const POOL_TRANSACTION_PAYLOAD_SCHEMA_V1: &str = "v1";
const ACE_CORE_SIGNAL_DETECTOR: &str = "ace_core_one_day_probe_v3_observe_only";
const ACE_CAPTURE_HEALTH_SCHEMA_VERSION: u16 = 1;
const CALIBRATION_BIRTHS: usize = 250;
const CUTOFF_OFFSET_MS: u64 = 11_111;
const FEATURE_WINDOW_MS: u64 = 8_000;
const ENTRY_STATE_MAX_AGE_MS: u64 = 1_000;
const TOTAL_WALLET_DEBIT_CAP_LAMPORTS: u64 = 150_000_000;
const MAX_ENTRY_SELF_IMPACT_BPS: u32 = 500;
const MAX_IMMEDIATE_EXIT_IMPACT_BPS: u32 = 500;
const MAX_ENTRY_TO_X3_NUMERATOR: u128 = 1;
const MAX_ENTRY_TO_X3_DENOMINATOR: u128 = 10;
const PRIMARY_EXIT_LATENCY_MS: u64 = 250;
const SUSTAIN_CONFIRM_AT_MS: u64 = 1_000;
const MAX_STATE_LOOKUP_LAG_MS: u64 = 1_000;
const OUTCOME_HORIZON_MS: u64 = 120_000;

const CALIBRATION_FILE: &str = "calibration_v1.json";
const CANDIDATE_ROWS_FILE: &str = "candidate_rows_v1.jsonl";
const SUMMARY_FILE: &str = "summary_v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AceCoreProbeDayId {
    Day1,
    Day2,
}

impl AceCoreProbeDayId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day1 => "day1",
            Self::Day2 => "day2",
        }
    }
}

impl FromStr for AceCoreProbeDayId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "day1" => Ok(Self::Day1),
            "day2" => Ok(Self::Day2),
            other => bail!("--day-id must be day1 or day2, got {other}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AceCoreOneDayProbeArgs {
    pub events_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub output_dir: PathBuf,
    pub day_id: AceCoreProbeDayId,
    pub calibration_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AceCandidateStatusV3 {
    #[serde(rename = "CALIBRATION_EXCLUDED")]
    CalibrationExcluded,
    #[serde(rename = "EVALUABLE_SELECTED")]
    EvaluableSelected,
    #[serde(rename = "EVALUABLE_REST")]
    EvaluableRest,
    #[serde(rename = "NON_EVALUABLE_FEATURES")]
    NonEvaluableFeatures,
    #[serde(rename = "NON_EVALUABLE_RESERVES")]
    NonEvaluableReserves,
    #[serde(rename = "NON_EVALUABLE_CAPACITY")]
    NonEvaluableCapacity,
    #[serde(rename = "NON_EVALUABLE_SUSTAIN_COVERAGE")]
    NonEvaluableSustainCoverage,
    #[serde(rename = "INVALID_CAPTURE")]
    InvalidCapture,
}

impl AceCandidateStatusV3 {
    const fn is_evaluable(self) -> bool {
        matches!(self, Self::EvaluableSelected | Self::EvaluableRest)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceCoreCandidateRowV3 {
    pub schema: String,
    pub candidate_id: String,
    pub base_mint: String,
    pub bonding_curve: String,
    pub creator: String,
    pub birth_ts_ms: u64,
    pub cutoff_ts_ms: u64,
    pub x1_creator_buy_wallet_debit_share: Option<f64>,
    pub x2_new_buyer_intensity_log_ratio: Option<f64>,
    pub x3_first_buy_wallet_debit_lamports: Option<u64>,
    pub x4_first_buy_late_early_log_ratio: Option<f64>,
    pub x5_first_buy_wallet_debit_hhi: Option<f64>,
    pub score: Option<f64>,
    pub selected: Option<bool>,
    pub entry_state_slot: Option<u64>,
    pub entry_total_debit_lamports: Option<u64>,
    pub entry_token_amount_raw: Option<u64>,
    pub entry_impact_bps: Option<u32>,
    pub immediate_exit_impact_bps: Option<u32>,
    pub best_sustained_proxy_net_return_120s: Option<f64>,
    pub best_trigger_ts_ms: Option<u64>,
    pub landing_ts_ms: Option<u64>,
    pub confirmation_ts_ms: Option<u64>,
    pub sustained_net17_hit: Option<bool>,
    pub status: AceCandidateStatusV3,
    pub reason: Option<String>,
    pub outcome_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceCoreCalibrationV1 {
    pub schema: String,
    pub day_id: AceCoreProbeDayId,
    pub feature_contract_version: String,
    pub amount_source_label: String,
    pub source_run_id: String,
    pub source_baseline_sha: String,
    pub source_implementation_sha: String,
    pub source_code_hash: String,
    pub medians: [f64; 5],
    pub iqrs: [f64; 5],
    pub score_weights: [f64; 5],
    pub selected_threshold: f64,
    pub cutoff_offset_ms: u64,
    pub total_wallet_debit_cap_lamports: u64,
    pub max_entry_self_impact_bps: u32,
    pub max_immediate_exit_impact_bps: u32,
    pub max_entry_to_x3_numerator: u128,
    pub max_entry_to_x3_denominator: u128,
    pub primary_exit_latency_ms: u64,
    pub sustain_confirm_at_ms: u64,
    pub max_state_lookup_lag_ms: u64,
    pub outcome_horizon_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AceCoreMetricsV1 {
    pub selected_count: usize,
    pub rest_count: usize,
    pub evaluable_coverage_pct: f64,
    pub selected_mean: Option<f64>,
    pub rest_mean: Option<f64>,
    pub delta_mean: Option<f64>,
    pub selected_median: Option<f64>,
    pub rest_median: Option<f64>,
    pub delta_median: Option<f64>,
    pub selected_sustained_net17_hit_rate: Option<f64>,
    pub rest_sustained_net17_hit_rate: Option<f64>,
    pub delta_sustained_hit17: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceCoreOneDaySummaryV1 {
    pub schema: String,
    pub day_id: AceCoreProbeDayId,
    pub terminal_status: String,
    pub capture_status: String,
    pub capture_invalid_reasons: Vec<String>,
    pub baseline_sha: String,
    pub implementation_sha: String,
    pub code_hash: String,
    pub binary_hash: String,
    pub run_id: String,
    pub authority_epoch_id: String,
    pub fee_authority_evidence_hash: String,
    pub birth_count: usize,
    pub duplicate_birth_evidence_count: usize,
    pub calibration_excluded_count: usize,
    pub non_evaluable_count_by_reason: BTreeMap<String, usize>,
    pub metrics: AceCoreMetricsV1,
    pub pooled_metrics: Option<AceCoreMetricsV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct BirthKey {
    base_mint: String,
    bonding_curve: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BirthKeyResolution {
    OutsideUniverse,
    Eligible(BirthKey),
    Malformed(&'static str),
}

#[derive(Debug, Clone)]
struct TapeBirth {
    candidate_id: String,
    payload: NewPoolDetectedPayload,
    event_id: String,
    file_ordinal: usize,
    line_number: usize,
}

#[derive(Debug, Clone)]
struct TapeTrade {
    payload: PoolTransactionPayload,
    event_id: String,
    file_ordinal: usize,
    line_number: usize,
}

#[derive(Debug, Default)]
struct Tape {
    births: Vec<TapeBirth>,
    trades: Vec<TapeTrade>,
    invalid_reasons: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct CandidateWork {
    birth: TapeBirth,
    trades: Vec<TapeTrade>,
    feature_result: std::result::Result<FeatureValues, String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FeatureValues {
    x1: f64,
    x2: f64,
    x3_lamports: u64,
    x4: f64,
    x5: f64,
}

impl FeatureValues {
    const fn values(self) -> [f64; 5] {
        [self.x1, self.x2, self.x3_lamports as f64, self.x4, self.x5]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct CanonicalTradeOrder {
    slot: u64,
    tx_index: u32,
    outer_instruction_index: u32,
    inner_group_index: u32,
    event_ordinal: u32,
}

#[derive(Debug, Clone, Copy)]
struct ReserveObservation {
    event_ts_ms: u64,
    slot: u64,
    order: Option<CanonicalTradeOrder>,
    ordinal: (usize, usize),
    reserves: PumpReserveState,
}

#[derive(Debug, Clone, Copy)]
struct EconomicOutcome {
    entry_state_slot: u64,
    entry_total_debit_lamports: u64,
    entry_token_amount_raw: u64,
    entry_impact_bps: u32,
    immediate_exit_impact_bps: u32,
    best_net_return: f64,
    best_trigger_ts_ms: u64,
    landing_ts_ms: u64,
    confirmation_ts_ms: u64,
}

#[derive(Debug, Clone)]
enum EconomicFailure {
    Reserves(&'static str),
    Capacity(&'static str),
    SustainCoverage(&'static str),
}

impl EconomicFailure {
    const fn status(&self) -> AceCandidateStatusV3 {
        match self {
            Self::Reserves(_) => AceCandidateStatusV3::NonEvaluableReserves,
            Self::Capacity(_) => AceCandidateStatusV3::NonEvaluableCapacity,
            Self::SustainCoverage(_) => AceCandidateStatusV3::NonEvaluableSustainCoverage,
        }
    }

    const fn reason(&self) -> &'static str {
        match self {
            Self::Reserves(reason) | Self::Capacity(reason) | Self::SustainCoverage(reason) => {
                reason
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FullTradeDedupeKey {
    signature: String,
    slot: u64,
    tx_index: u32,
    outer_instruction_index: u32,
    inner_group_index: u32,
    event_ordinal: u32,
}

/// Run the strictly offline ACE Core one-day probe.
///
/// The function intentionally accepts only persisted evidence paths. It never
/// opens RPC connections, constructs a runtime observer, or emits an intent.
pub fn run_ace_core_one_day_probe(args: AceCoreOneDayProbeArgs) -> Result<AceCoreOneDaySummaryV1> {
    validate_cli_contract(&args)?;
    create_output_dir_new(&args.output_dir)?;

    let manifest = load_manifest(&args.manifest_path)?;
    let mut invalid_reasons = validate_manifest(&manifest);
    invalid_reasons.extend(validate_capture_health_evidence(
        &args.manifest_path,
        &manifest,
    ));
    let mut tape = read_tape(&args.events_dir, &manifest.run_id)?;

    let (births, duplicate_birth_evidence_count) = canonical_births(&mut tape);
    let canonical_birth_keys = births
        .iter()
        .filter_map(|birth| match birth_key(&birth.payload) {
            BirthKeyResolution::Eligible(key) => Some(key),
            BirthKeyResolution::OutsideUniverse | BirthKeyResolution::Malformed(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let trade_index = strict_trade_index(&mut tape, &canonical_birth_keys);
    invalid_reasons.extend(tape.invalid_reasons.iter().cloned());
    let quote_contract = if invalid_reasons.is_empty() {
        Some(
            manifest
                .pump_quote_authority
                .materialize()
                .map_err(|error| {
                    anyhow!(
                        "materialize frozen pump_quote_authority from capture manifest: {error}"
                    )
                })?,
        )
    } else {
        None
    };

    let calibration = match args.day_id {
        AceCoreProbeDayId::Day1 => None,
        AceCoreProbeDayId::Day2 => Some(load_and_validate_day2_calibration(
            args.calibration_path.as_deref().expect("validated above"),
            &manifest,
        )?),
    };

    let mut works = births
        .into_iter()
        .map(|birth| {
            let (trades, feature_result) = match birth_key(&birth.payload) {
                BirthKeyResolution::Eligible(key) => {
                    let trades = trade_index.get(&key).cloned().unwrap_or_default();
                    let feature_result = calculate_features(&birth, &trades);
                    (trades, feature_result)
                }
                BirthKeyResolution::Malformed(reason) => {
                    (Vec::new(), Err(format!("malformed_birth:{reason}")))
                }
                BirthKeyResolution::OutsideUniverse => (
                    Vec::new(),
                    Err("birth_outside_eligible_universe".to_string()),
                ),
            };
            CandidateWork {
                birth,
                trades,
                feature_result,
            }
        })
        .collect::<Vec<_>>();
    works.sort_by(compare_candidate_work);

    let (calibration, calibration_indices, calibration_error) = match args.day_id {
        AceCoreProbeDayId::Day1 => create_day1_calibration(&works, &manifest),
        AceCoreProbeDayId::Day2 => (calibration, HashSet::new(), None),
    };

    let mut rows = Vec::with_capacity(works.len());
    if !invalid_reasons.is_empty() {
        rows.extend(works.iter().map(|work| invalid_capture_row(work)));
    } else if let Some(reason) = calibration_error.as_ref() {
        rows.extend(works.iter().map(|work| {
            if work.feature_result.is_ok() {
                calibration_excluded_row(work, Some(reason.clone()))
            } else {
                feature_failure_row(work)
            }
        }));
    } else {
        let calibration = calibration
            .as_ref()
            .ok_or_else(|| anyhow!("ACE calibration unexpectedly unavailable"))?;
        let quote_contract = quote_contract
            .as_ref()
            .ok_or_else(|| anyhow!("ACE quote contract unexpectedly unavailable"))?;
        for (index, work) in works.iter().enumerate() {
            let row = match &work.feature_result {
                Err(_) => feature_failure_row(work),
                Ok(features) if calibration_indices.contains(&index) => {
                    calibration_excluded_row(work, None)
                }
                Ok(features) => scored_candidate_row(work, *features, calibration, &quote_contract),
            };
            rows.push(row);
        }
    }

    let capture_status = if invalid_reasons.is_empty() {
        "VALID_CAPTURE".to_string()
    } else {
        "INVALID_CAPTURE".to_string()
    };
    let metrics = metrics_from_rows(&rows);
    let mut summary = AceCoreOneDaySummaryV1 {
        schema: ACE_CORE_SUMMARY_SCHEMA.to_string(),
        day_id: args.day_id,
        terminal_status: "ACE_PROBE_INCONCLUSIVE".to_string(),
        capture_status,
        capture_invalid_reasons: invalid_reasons.into_iter().collect(),
        baseline_sha: ACE_CORE_BASELINE_SHA.to_string(),
        implementation_sha: manifest.implementation_sha.clone(),
        code_hash: manifest.code_hash.clone(),
        binary_hash: manifest.binary_hash.clone(),
        run_id: manifest.run_id.clone(),
        authority_epoch_id: manifest.authority_epoch_id.to_string(),
        fee_authority_evidence_hash: manifest.runtime_fee_authority.evidence_hash.clone(),
        birth_count: rows.len(),
        duplicate_birth_evidence_count,
        calibration_excluded_count: rows
            .iter()
            .filter(|row| row.status == AceCandidateStatusV3::CalibrationExcluded)
            .count(),
        non_evaluable_count_by_reason: non_evaluable_counts(&rows),
        metrics,
        pooled_metrics: None,
    };

    if summary.capture_status == "VALID_CAPTURE" && calibration_error.is_none() {
        summary.terminal_status = match args.day_id {
            AceCoreProbeDayId::Day1 => day1_terminal_status(&summary.metrics),
            AceCoreProbeDayId::Day2 => {
                let (day1_summary, day1_rows) = load_day1_probe_artifacts(
                    args.calibration_path
                        .as_deref()
                        .expect("validated day2 calibration path"),
                )?;
                let calibration = calibration
                    .as_ref()
                    .expect("validated day2 calibration is retained");
                if day1_summary.run_id != calibration.source_run_id
                    || day1_summary.baseline_sha != calibration.source_baseline_sha
                    || day1_summary.implementation_sha != calibration.source_implementation_sha
                    || day1_summary.code_hash != calibration.source_code_hash
                    || day1_summary.capture_status != "VALID_CAPTURE"
                {
                    bail!("day2 calibration does not match its day1 probe summary")
                }
                let mut pooled_rows = day1_rows;
                pooled_rows.extend(rows.iter().cloned());
                let pooled_metrics = metrics_from_rows(&pooled_rows);
                let day2_negative = day_is_negative(&summary.metrics);
                let day1_negative =
                    day1_summary.terminal_status.as_str() == "ACE_PROBE_DAY1_NEGATIVE_UNCONFIRMED";
                summary.pooled_metrics = Some(pooled_metrics.clone());
                if day1_negative && day2_negative {
                    "ACE_PROBE_DEAD".to_string()
                } else if pooled_is_promising(&pooled_metrics, 100) {
                    "ACE_PROBE_PROMISING_NOT_PROVEN".to_string()
                } else {
                    "ACE_PROBE_INCONCLUSIVE".to_string()
                }
            }
        };
    }

    write_candidate_rows_new(&args.output_dir.join(CANDIDATE_ROWS_FILE), &rows)?;
    if let Some(calibration) = calibration.as_ref() {
        write_json_new(&args.output_dir.join(CALIBRATION_FILE), calibration)?;
    }
    write_json_new(&args.output_dir.join(SUMMARY_FILE), &summary)?;
    Ok(summary)
}

fn validate_cli_contract(args: &AceCoreOneDayProbeArgs) -> Result<()> {
    match (args.day_id, args.calibration_path.as_ref()) {
        (AceCoreProbeDayId::Day1, Some(_)) => {
            bail!("--calibration is only valid with --day-id day2")
        }
        (AceCoreProbeDayId::Day2, None) => {
            bail!("--day-id day2 requires --calibration <DAY1_PROBE_DIR>/calibration_v1.json")
        }
        _ => Ok(()),
    }
}

fn create_output_dir_new(path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create ACE probe output parent {}", parent.display()))?;
    fs::create_dir(path).with_context(|| {
        format!(
            "create new ACE probe output directory {} (existing output is forbidden)",
            path.display()
        )
    })
}

fn load_manifest(path: &Path) -> Result<RugRealityCaptureRunManifestV1> {
    let bytes = fs::read(path)
        .with_context(|| format!("read ACE probe capture manifest {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("decode ACE probe capture manifest {}", path.display()))
}

fn validate_manifest(manifest: &RugRealityCaptureRunManifestV1) -> BTreeSet<String> {
    let mut reasons = BTreeSet::new();
    if manifest.schema_version < 3 {
        reasons.insert("manifest_schema_does_not_freeze_ace_provenance_and_health".to_string());
    }
    if manifest.run_id.trim().is_empty() {
        reasons.insert("manifest_run_id_missing".to_string());
    }
    if manifest.config_hash.trim().is_empty() || manifest.binary_hash.trim().is_empty() {
        reasons.insert("manifest_config_or_binary_provenance_missing".to_string());
    }
    if manifest.baseline_sha != ACE_CORE_BASELINE_SHA {
        reasons.insert("manifest_baseline_sha_mismatch".to_string());
    }
    if !is_full_git_sha(&manifest.implementation_sha) {
        reasons.insert("manifest_implementation_sha_missing_or_invalid".to_string());
    }
    if manifest.code_hash != format!("git:{}", manifest.implementation_sha) {
        reasons.insert("manifest_code_hash_does_not_match_implementation_sha".to_string());
    }
    if manifest.health_evidence_path.trim().is_empty() {
        reasons.insert("manifest_capture_health_evidence_path_missing".to_string());
    }
    if !manifest.observe_only {
        reasons.insert("manifest_not_observe_only".to_string());
    }
    if manifest.signal_detector != ACE_CORE_SIGNAL_DETECTOR {
        reasons.insert("manifest_signal_detector_not_ace_observe_only".to_string());
    }
    if manifest.entry_route_id != "buy_v2" || manifest.exit_route_id != "legacy_sell" {
        reasons.insert("manifest_route_authority_mismatch".to_string());
    }
    if manifest
        .runtime_fee_authority
        .evidence_hash
        .trim()
        .is_empty()
    {
        reasons.insert("manifest_fee_authority_evidence_missing".to_string());
    }
    if manifest.authority_epoch_id == 0 {
        reasons.insert("manifest_pr1_authority_epoch_missing".to_string());
    }
    if manifest.event_writer_run_id != manifest.run_id {
        reasons.insert("manifest_event_writer_run_id_mismatch".to_string());
    }
    if !manifest.event_writer_optional_events_enabled {
        reasons.insert("manifest_optional_pool_transaction_events_disabled".to_string());
    }
    if manifest.pump_quote_authority.schedules.is_empty() {
        reasons.insert("manifest_pump_quote_authority_missing".to_string());
    } else if manifest.pump_quote_authority.materialize().is_err() {
        reasons.insert("manifest_pump_quote_authority_unmaterializable".to_string());
    } else {
        let entry_schedule_matches = manifest.pump_quote_authority.schedules.iter().any(|entry| {
            entry.route_variant == RUG_SCALP_ENTRY_ROUTE
                && entry.schedule.fee_schedule_id
                    == manifest.runtime_fee_authority.buy_v2_fee_schedule_id
        });
        let exit_schedule_matches = manifest.pump_quote_authority.schedules.iter().any(|entry| {
            entry.route_variant == RUG_SCALP_EXIT_ROUTE
                && entry.schedule.fee_schedule_id
                    == manifest.runtime_fee_authority.legacy_sell_fee_schedule_id
        });
        if !entry_schedule_matches || !exit_schedule_matches {
            reasons.insert("manifest_fee_schedule_metadata_mismatch".to_string());
        }
    }
    reasons
}

fn is_full_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_capture_health_evidence(
    manifest_path: &Path,
    manifest: &RugRealityCaptureRunManifestV1,
) -> BTreeSet<String> {
    let mut reasons = BTreeSet::new();
    let health_path = Path::new(&manifest.health_evidence_path);
    let bytes = match fs::read(health_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            reasons.insert("capture_health_evidence_missing_or_unreadable".to_string());
            return reasons;
        }
    };
    let receipt = match serde_json::from_slice::<RugRealityCaptureHealthEvidenceV1>(&bytes) {
        Ok(receipt) => receipt,
        Err(_) => {
            reasons.insert("capture_health_evidence_decode_failed".to_string());
            return reasons;
        }
    };
    if receipt.schema_version != ACE_CAPTURE_HEALTH_SCHEMA_VERSION {
        reasons.insert("capture_health_evidence_schema_mismatch".to_string());
    }
    if receipt.run_id != manifest.run_id {
        reasons.insert("capture_health_evidence_run_id_mismatch".to_string());
    }
    match fs::read(manifest_path) {
        Ok(manifest_bytes) if receipt.manifest_sha256 == sha256_hex(&manifest_bytes) => {}
        _ => {
            reasons.insert("capture_health_evidence_manifest_hash_mismatch".to_string());
        }
    }
    if receipt.start_metrics_sha256.trim().is_empty()
        || receipt.end_metrics_sha256.trim().is_empty()
    {
        reasons.insert("capture_health_metrics_snapshot_provenance_missing".to_string());
    }
    if receipt.pr1_runtime_bypass_attempt_total != 0 {
        reasons.insert("capture_health_pr1_runtime_bypass_attempt_nonzero".to_string());
    }
    if receipt.pr1_runtime_candidate_admission_closed_total != 0 {
        reasons.insert("capture_health_candidate_admission_closed".to_string());
    }
    if receipt.pr1_runtime_primary_coverage_gap_total != 0 {
        reasons.insert("capture_health_primary_local_coverage_gap".to_string());
    }
    if receipt.event_writer_write_failure_count != 0 {
        reasons.insert("capture_health_event_writer_write_failure".to_string());
    }
    if receipt.event_writer_lock_failure_count != 0 {
        reasons.insert("capture_health_event_writer_lock_failure".to_string());
    }
    if !receipt.controlled_shutdown {
        reasons.insert("capture_health_controlled_shutdown_not_proven".to_string());
    }
    if !receipt.event_files_cleanly_flushed {
        reasons.insert("capture_health_event_writer_flush_not_proven".to_string());
    }
    if !receipt.log_evidence_clean {
        reasons.insert("capture_health_log_failure_detected".to_string());
    }
    reasons
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn collect_event_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        bail!("--events-dir is not a directory: {}", root.display());
    }
    let mut files = Vec::new();
    collect_event_files_inner(root, &mut files)?;
    files.sort();
    if files.is_empty() {
        bail!(
            "--events-dir contains no exec_*.jsonl evidence files: {}",
            root.display()
        );
    }
    Ok(files)
}

fn collect_event_files_inner(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(root).with_context(|| format!("read events directory {}", root.display()))?
    {
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

fn read_tape(events_dir: &Path, expected_run_id: &str) -> Result<Tape> {
    let files = collect_event_files(events_dir)?;
    let mut tape = Tape::default();
    for (file_ordinal, path) in files.iter().enumerate() {
        let file =
            File::open(path).with_context(|| format!("open event tape {}", path.display()))?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut line_number = 0usize;
        loop {
            line.clear();
            let count = reader
                .read_line(&mut line)
                .with_context(|| format!("read event tape {}", path.display()))?;
            if count == 0 {
                break;
            }
            line_number += 1;
            if !line.ends_with('\n') {
                tape.invalid_reasons
                    .insert("event_writer_not_cleanly_flushed".to_string());
                continue;
            }
            if line.trim().is_empty() {
                tape.invalid_reasons
                    .insert("event_jsonl_blank_line".to_string());
                continue;
            }
            let event = match serde_json::from_str::<ExecutionEvent>(&line) {
                Ok(event) => event,
                Err(_) => {
                    tape.invalid_reasons
                        .insert("event_jsonl_decode_failed".to_string());
                    continue;
                }
            };
            if event.envelope.run_id != expected_run_id {
                tape.invalid_reasons
                    .insert("manifest_run_id_does_not_match_event_tape".to_string());
                continue;
            }
            if event.envelope.lane != Lane::Shadow {
                tape.invalid_reasons
                    .insert("event_tape_lane_is_not_shadow".to_string());
                continue;
            }
            match event.kind {
                EventKind::NewPoolDetected(payload) => tape.births.push(TapeBirth {
                    candidate_id: event.envelope.candidate_id.to_string(),
                    payload,
                    event_id: event.envelope.event_id,
                    file_ordinal,
                    line_number,
                }),
                EventKind::PoolTransaction(payload) => tape.trades.push(TapeTrade {
                    payload,
                    event_id: event.envelope.event_id,
                    file_ordinal,
                    line_number,
                }),
                _ => {}
            }
        }
    }
    Ok(tape)
}

fn is_wsol_quote(quote_mint: &str) -> bool {
    matches!(quote_mint.trim(), "SOL" | "WSOL" | WSOL_MINT)
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value.trim())
}

fn birth_key(payload: &NewPoolDetectedPayload) -> BirthKeyResolution {
    if !payload.is_birth_event
        || payload.amm_program.trim() != RUG_SCALP_PUMP_PROGRAM.to_string()
        || !is_wsol_quote(&payload.quote_mint)
    {
        return BirthKeyResolution::OutsideUniverse;
    }
    let Some(base_mint) = non_empty(&payload.base_mint) else {
        return BirthKeyResolution::Malformed("pump_sol_birth_base_mint_missing");
    };
    let Some(mint_id) = non_empty(&payload.mint_id) else {
        return BirthKeyResolution::Malformed("pump_sol_birth_mint_alias_missing");
    };
    if base_mint != mint_id {
        return BirthKeyResolution::Malformed("pump_sol_birth_mint_alias_conflict");
    }
    let Some(pool_amm_id) = non_empty(&payload.pool_amm_id) else {
        return BirthKeyResolution::Malformed("pump_sol_birth_pool_amm_id_missing");
    };
    let Some(pool_id) = non_empty(&payload.pool_id) else {
        return BirthKeyResolution::Malformed("pump_sol_birth_pool_id_missing");
    };
    if pool_amm_id != pool_id {
        return BirthKeyResolution::Malformed("pump_sol_birth_pool_alias_conflict");
    }
    let Some(bonding_curve) = non_empty(&payload.bonding_curve) else {
        return BirthKeyResolution::Malformed("pump_sol_birth_bonding_curve_missing");
    };
    if pool_amm_id != bonding_curve {
        return BirthKeyResolution::Malformed("pump_sol_birth_pool_curve_conflict");
    }
    BirthKeyResolution::Eligible(BirthKey {
        base_mint: base_mint.to_string(),
        bonding_curve: bonding_curve.to_string(),
    })
}

fn compare_birth(left: &TapeBirth, right: &TapeBirth) -> std::cmp::Ordering {
    (
        left.payload.birth_ts_ms,
        left.payload.event_slot.unwrap_or(u64::MAX),
        left.payload.signature.as_str(),
        left.candidate_id.as_str(),
        left.event_id.as_str(),
        left.file_ordinal,
        left.line_number,
    )
        .cmp(&(
            right.payload.birth_ts_ms,
            right.payload.event_slot.unwrap_or(u64::MAX),
            right.payload.signature.as_str(),
            right.candidate_id.as_str(),
            right.event_id.as_str(),
            right.file_ordinal,
            right.line_number,
        ))
}

fn canonical_births(tape: &mut Tape) -> (Vec<TapeBirth>, usize) {
    let mut by_key = BTreeMap::<BirthKey, Vec<TapeBirth>>::new();
    let mut malformed = Vec::new();
    for birth in tape.births.iter().cloned() {
        match birth_key(&birth.payload) {
            BirthKeyResolution::OutsideUniverse => {}
            BirthKeyResolution::Eligible(key) => {
                by_key.entry(key).or_default().push(birth);
            }
            BirthKeyResolution::Malformed(reason) => {
                tape.invalid_reasons.insert(reason.to_string());
                // Keep malformed canonical Pump/SOL births in the terminal
                // output instead of silently shrinking the denominator.
                malformed.push(birth);
            }
        }
    }
    let mut canonical = Vec::with_capacity(by_key.len());
    let mut duplicate_count = 0usize;
    for mut candidates in by_key.into_values() {
        candidates.sort_by(compare_birth);
        duplicate_count = duplicate_count.saturating_add(candidates.len().saturating_sub(1));
        if let Some(first) = candidates.into_iter().next() {
            canonical.push(first);
        }
    }
    canonical.extend(malformed);
    canonical.sort_by(compare_birth);
    (canonical, duplicate_count)
}

fn canonical_slot(payload: &PoolTransactionPayload) -> Option<u64> {
    match (payload.event_slot, payload.slot) {
        (Some(event_slot), Some(slot)) if event_slot != slot => None,
        (Some(slot), _) | (_, Some(slot)) => Some(slot),
        (None, None) => None,
    }
}

fn canonical_trade_order(payload: &PoolTransactionPayload) -> Option<CanonicalTradeOrder> {
    Some(CanonicalTradeOrder {
        slot: canonical_slot(payload)?,
        tx_index: payload.tx_index?,
        outer_instruction_index: payload.outer_instruction_index?,
        inner_group_index: payload.inner_group_index?,
        event_ordinal: payload.event_ordinal?,
    })
}

fn strict_trade_key(
    payload: &PoolTransactionPayload,
) -> std::result::Result<BirthKey, &'static str> {
    if payload.schema_version != POOL_TRANSACTION_PAYLOAD_SCHEMA_V1 {
        return Err("pool_transaction_schema_version_mismatch");
    }
    if let Some(quote_mint) = payload.quote_mint.as_deref().and_then(non_empty) {
        if !is_wsol_quote(quote_mint) {
            return Err("pool_transaction_quote_mint_not_wsol");
        }
    }
    let mut mint_aliases = BTreeSet::new();
    for alias in [
        payload.base_mint.as_deref(),
        payload.mint_id.as_deref(),
        payload.token_mint.as_deref(),
    ] {
        if let Some(alias) = alias.and_then(non_empty) {
            mint_aliases.insert(alias.to_string());
        }
    }
    if mint_aliases.is_empty() {
        return Err("pool_transaction_mint_alias_missing");
    }
    if mint_aliases.len() != 1 {
        return Err("pool_transaction_mint_alias_conflict");
    }
    let Some(pool_amm_id) = non_empty(&payload.pool_amm_id) else {
        return Err("pool_transaction_pool_amm_id_missing");
    };
    let Some(pool_id) = non_empty(&payload.pool_id) else {
        return Err("pool_transaction_pool_id_missing");
    };
    if pool_amm_id != pool_id {
        return Err("pool_transaction_pool_alias_conflict");
    }
    let Some(bonding_curve) = non_empty(&payload.bonding_curve) else {
        return Err("pool_transaction_bonding_curve_missing");
    };
    if pool_amm_id != bonding_curve {
        return Err("pool_transaction_pool_curve_conflict");
    }
    Ok(BirthKey {
        base_mint: mint_aliases
            .into_iter()
            .next()
            .expect("non-empty singleton mint aliases"),
        bonding_curve: bonding_curve.to_string(),
    })
}

fn full_trade_dedupe_key(payload: &PoolTransactionPayload) -> Option<FullTradeDedupeKey> {
    Some(FullTradeDedupeKey {
        signature: non_empty(&payload.signature)?.to_string(),
        slot: canonical_slot(payload)?,
        tx_index: payload.tx_index?,
        outer_instruction_index: payload.outer_instruction_index?,
        inner_group_index: payload.inner_group_index?,
        event_ordinal: payload.event_ordinal?,
    })
}

fn full_trade_material_digest(payload: &PoolTransactionPayload) -> Option<String> {
    serde_json::to_vec(payload)
        .ok()
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
}

fn compare_trade(left: &TapeTrade, right: &TapeTrade) -> std::cmp::Ordering {
    (
        left.payload.event_ts_ms,
        canonical_slot(&left.payload).unwrap_or(u64::MAX),
        left.payload.tx_index.unwrap_or(u32::MAX),
        left.payload.outer_instruction_index.unwrap_or(u32::MAX),
        left.payload.inner_group_index.unwrap_or(u32::MAX),
        left.payload.event_ordinal.unwrap_or(u32::MAX),
        left.payload.signature.as_str(),
        left.event_id.as_str(),
        left.file_ordinal,
        left.line_number,
    )
        .cmp(&(
            right.payload.event_ts_ms,
            canonical_slot(&right.payload).unwrap_or(u64::MAX),
            right.payload.tx_index.unwrap_or(u32::MAX),
            right.payload.outer_instruction_index.unwrap_or(u32::MAX),
            right.payload.inner_group_index.unwrap_or(u32::MAX),
            right.payload.event_ordinal.unwrap_or(u32::MAX),
            right.payload.signature.as_str(),
            right.event_id.as_str(),
            right.file_ordinal,
            right.line_number,
        ))
}

fn strict_trade_index(
    tape: &mut Tape,
    canonical_birth_keys: &BTreeSet<BirthKey>,
) -> BTreeMap<BirthKey, Vec<TapeTrade>> {
    let mut indexed = BTreeMap::<BirthKey, Vec<TapeTrade>>::new();
    for trade in tape.trades.iter().cloned() {
        match strict_trade_key(&trade.payload) {
            Ok(key) if canonical_birth_keys.contains(&key) => {
                indexed.entry(key).or_default().push(trade);
            }
            Ok(_) => {
                tape.invalid_reasons
                    .insert("pool_transaction_has_no_canonical_birth".to_string());
            }
            Err(reason) => {
                tape.invalid_reasons.insert(reason.to_string());
            }
        }
    }
    for trades in indexed.values_mut() {
        trades.sort_by(compare_trade);
        let mut seen = HashMap::<FullTradeDedupeKey, String>::new();
        let mut retained = Vec::with_capacity(trades.len());
        for trade in std::mem::take(trades) {
            let Some(key) = full_trade_dedupe_key(&trade.payload) else {
                // A missing full order key is not merged.  The feature
                // contract continues to classify it if it enters the window.
                retained.push(trade);
                continue;
            };
            let Some(material_digest) = full_trade_material_digest(&trade.payload) else {
                tape.invalid_reasons
                    .insert("pool_transaction_material_payload_unserializable".to_string());
                continue;
            };
            match seen.get(&key) {
                None => {
                    seen.insert(key, material_digest);
                    retained.push(trade);
                }
                Some(first_digest) if first_digest == &material_digest => {
                    // Exact material duplicate is legal delivery duplication.
                }
                Some(_) => {
                    tape.invalid_reasons
                        .insert("pool_transaction_divergent_full_mutation_duplicate".to_string());
                }
            }
        }
        *trades = retained;
    }
    indexed
}

fn compare_candidate_work(left: &CandidateWork, right: &CandidateWork) -> std::cmp::Ordering {
    compare_birth(&left.birth, &right.birth)
}

fn is_successful_buy(payload: &PoolTransactionPayload) -> bool {
    payload.success && payload.is_buy && payload.side.eq_ignore_ascii_case("buy")
}

fn feature_buy_debit(
    trade: &TapeTrade,
    birth_ts_ms: u64,
    cutoff_ts_ms: u64,
) -> std::result::Result<Option<(CanonicalTradeOrder, String, u64)>, String> {
    let payload = &trade.payload;
    if payload.event_ts_ms < birth_ts_ms || payload.event_ts_ms > cutoff_ts_ms {
        return Ok(None);
    }
    if !is_successful_buy(payload) {
        return Ok(None);
    }
    if payload.is_synthetic != Some(false) {
        return Err("successful_buy_synthetic_provenance_missing_or_true".to_string());
    }
    let signer = non_empty(&payload.signer)
        .ok_or_else(|| "successful_buy_wallet_identity_missing".to_string())?;
    if let Some(wallet) = non_empty(&payload.wallet) {
        if wallet != signer {
            return Err("successful_buy_wallet_alias_conflict".to_string());
        }
    }
    let order = canonical_trade_order(payload)
        .ok_or_else(|| "successful_buy_canonical_order_missing".to_string())?;
    let pre = payload
        .signer_pre_balance_lamports
        .ok_or_else(|| "successful_buy_pre_balance_missing".to_string())?;
    let post = payload
        .signer_post_balance_lamports
        .ok_or_else(|| "successful_buy_post_balance_missing".to_string())?;
    let debit = pre
        .checked_sub(post)
        .filter(|debit| *debit > 0)
        .ok_or_else(|| "successful_buy_wallet_debit_non_positive".to_string())?;
    Ok(Some((order, signer.to_string(), debit)))
}

fn calculate_features(
    birth: &TapeBirth,
    trades: &[TapeTrade],
) -> std::result::Result<FeatureValues, String> {
    let creator = non_empty(&birth.payload.creator)
        .ok_or_else(|| "birth_creator_identity_missing".to_string())?;
    let cutoff_ts_ms = birth.payload.birth_ts_ms.saturating_add(CUTOFF_OFFSET_MS);
    let mut buys = Vec::new();
    for trade in trades {
        if let Some((order, wallet, debit)) =
            feature_buy_debit(trade, birth.payload.birth_ts_ms, cutoff_ts_ms)?
        {
            buys.push((
                trade.payload.event_ts_ms,
                order,
                wallet,
                debit,
                trade.file_ordinal,
                trade.line_number,
            ));
        }
    }
    buys.sort_by(|left, right| {
        (left.1, left.0, left.4, left.5).cmp(&(right.1, right.0, right.4, right.5))
    });
    if buys.is_empty() {
        return Err("successful_buy_wallet_debit_evidence_missing".to_string());
    }

    let total_buy_debit = buys.iter().try_fold(0u64, |sum, buy| {
        sum.checked_add(buy.3)
            .ok_or_else(|| "total_buy_wallet_debit_overflow".to_string())
    })?;
    let creator_buy_debit =
        buys.iter()
            .filter(|buy| buy.2 == creator)
            .try_fold(0u64, |sum, buy| {
                sum.checked_add(buy.3)
                    .ok_or_else(|| "creator_buy_wallet_debit_overflow".to_string())
            })?;

    let mut first_buys = BTreeMap::<String, (u64, CanonicalTradeOrder, u64)>::new();
    for (event_ts_ms, order, wallet, debit, _, _) in &buys {
        first_buys
            .entry(wallet.clone())
            .or_insert((*event_ts_ms, *order, *debit));
    }
    let recent_start = cutoff_ts_ms.saturating_sub(FEATURE_WINDOW_MS);
    let short_start = cutoff_ts_ms.saturating_sub(2_000);
    let mut recent_first_buys = first_buys
        .into_iter()
        .filter_map(|(wallet, (event_ts_ms, order, debit))| {
            (event_ts_ms >= recent_start && event_ts_ms <= cutoff_ts_ms).then_some((
                event_ts_ms,
                order,
                wallet,
                debit,
            ))
        })
        .collect::<Vec<_>>();
    recent_first_buys.sort_by(|left, right| {
        (left.1, left.0, left.2.as_str()).cmp(&(right.1, right.0, right.2.as_str()))
    });
    if recent_first_buys.len() < 4 {
        return Err("fewer_than_four_recent_first_buys".to_string());
    }

    let n_short = recent_first_buys
        .iter()
        .filter(|buy| buy.0 >= short_start)
        .count() as f64;
    let n_long = recent_first_buys.len() as f64;
    let lambda_short = (n_short + 1.0) / 3.0;
    let lambda_long = (n_long + 1.0) / 9.0;
    let x2 = (lambda_short / lambda_long).ln();
    let x3_lamports = recent_first_buys.iter().try_fold(0u64, |sum, buy| {
        sum.checked_add(buy.3)
            .ok_or_else(|| "first_buy_wallet_debit_overflow".to_string())
    })?;
    if x3_lamports == 0 {
        return Err("first_buy_wallet_debit_non_positive".to_string());
    }
    let midpoint = recent_first_buys.len() / 2;
    let early = median_u64(
        &recent_first_buys[..midpoint]
            .iter()
            .map(|buy| buy.3)
            .collect::<Vec<_>>(),
    );
    let late = median_u64(
        &recent_first_buys[midpoint..]
            .iter()
            .map(|buy| buy.3)
            .collect::<Vec<_>>(),
    );
    let x4 = ((late as f64 + 1.0) / (early as f64 + 1.0)).ln();
    let x5 = recent_first_buys.iter().fold(0.0, |sum, buy| {
        let share = buy.3 as f64 / x3_lamports as f64;
        sum + share * share
    });
    let values = FeatureValues {
        x1: creator_buy_debit as f64 / total_buy_debit as f64,
        x2,
        x3_lamports,
        x4,
        x5,
    };
    values
        .values()
        .into_iter()
        .all(f64::is_finite)
        .then_some(values)
        .ok_or_else(|| "feature_value_non_finite".to_string())
}

fn median_u64(values: &[u64]) -> u64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let midpoint = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        sorted[midpoint - 1]
            .saturating_add(sorted[midpoint])
            .saturating_div(2)
    } else {
        sorted[midpoint]
    }
}

fn create_day1_calibration(
    works: &[CandidateWork],
    manifest: &RugRealityCaptureRunManifestV1,
) -> (Option<AceCoreCalibrationV1>, HashSet<usize>, Option<String>) {
    let feature_indices = works
        .iter()
        .enumerate()
        .filter_map(|(index, work)| work.feature_result.is_ok().then_some(index))
        .collect::<Vec<_>>();
    if feature_indices.len() < CALIBRATION_BIRTHS {
        return (
            None,
            feature_indices.into_iter().collect(),
            Some("insufficient_feature_evaluable_births_for_250_row_calibration".to_string()),
        );
    }
    let calibration_indices = feature_indices
        .iter()
        .take(CALIBRATION_BIRTHS)
        .copied()
        .collect::<HashSet<_>>();
    let mut columns = [
        Vec::<f64>::with_capacity(CALIBRATION_BIRTHS),
        Vec::<f64>::with_capacity(CALIBRATION_BIRTHS),
        Vec::<f64>::with_capacity(CALIBRATION_BIRTHS),
        Vec::<f64>::with_capacity(CALIBRATION_BIRTHS),
        Vec::<f64>::with_capacity(CALIBRATION_BIRTHS),
    ];
    for index in feature_indices.iter().take(CALIBRATION_BIRTHS) {
        let values = works[*index]
            .feature_result
            .as_ref()
            .expect("feature index is evaluable")
            .values();
        for (column, value) in columns.iter_mut().zip(values) {
            column.push(value);
        }
    }
    let mut medians = [0.0; 5];
    let mut iqrs = [0.0; 5];
    for (index, column) in columns.iter_mut().enumerate() {
        column.sort_by(f64::total_cmp);
        medians[index] = quantile_linear(column, 0.5);
        iqrs[index] = quantile_linear(column, 0.75) - quantile_linear(column, 0.25);
    }
    if iqrs.iter().any(|iqr| !iqr.is_finite() || *iqr == 0.0) {
        return (
            None,
            calibration_indices,
            Some("calibration_iqr_zero_or_non_finite".to_string()),
        );
    }
    let score_weights = [-1.0, 1.0, 1.0, 1.0, -1.0];
    let mut calibration_scores = feature_indices
        .iter()
        .take(CALIBRATION_BIRTHS)
        .map(|index| {
            score_features(
                *works[*index]
                    .feature_result
                    .as_ref()
                    .expect("feature index is evaluable"),
                medians,
                iqrs,
                score_weights,
            )
        })
        .collect::<Vec<_>>();
    calibration_scores.sort_by(f64::total_cmp);
    let selected_threshold = quantile_linear(&calibration_scores, 0.8);
    if !selected_threshold.is_finite() {
        return (
            None,
            calibration_indices,
            Some("calibration_score_threshold_non_finite".to_string()),
        );
    }
    (
        Some(AceCoreCalibrationV1 {
            schema: ACE_CORE_CALIBRATION_SCHEMA.to_string(),
            day_id: AceCoreProbeDayId::Day1,
            feature_contract_version: ACE_CORE_ONE_DAY_PROBE_SCHEMA.to_string(),
            amount_source_label: "observed_buy_wallet_debit_lamports=signer_pre_balance_lamports-signer_post_balance_lamports".to_string(),
            source_run_id: manifest.run_id.clone(),
            source_baseline_sha: ACE_CORE_BASELINE_SHA.to_string(),
            source_implementation_sha: manifest.implementation_sha.clone(),
            source_code_hash: manifest.code_hash.clone(),
            medians,
            iqrs,
            score_weights,
            selected_threshold,
            cutoff_offset_ms: CUTOFF_OFFSET_MS,
            total_wallet_debit_cap_lamports: TOTAL_WALLET_DEBIT_CAP_LAMPORTS,
            max_entry_self_impact_bps: MAX_ENTRY_SELF_IMPACT_BPS,
            max_immediate_exit_impact_bps: MAX_IMMEDIATE_EXIT_IMPACT_BPS,
            max_entry_to_x3_numerator: MAX_ENTRY_TO_X3_NUMERATOR,
            max_entry_to_x3_denominator: MAX_ENTRY_TO_X3_DENOMINATOR,
            primary_exit_latency_ms: PRIMARY_EXIT_LATENCY_MS,
            sustain_confirm_at_ms: SUSTAIN_CONFIRM_AT_MS,
            max_state_lookup_lag_ms: MAX_STATE_LOOKUP_LAG_MS,
            outcome_horizon_ms: OUTCOME_HORIZON_MS,
        }),
        calibration_indices,
        None,
    )
}

fn quantile_linear(sorted: &[f64], percentile: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    let position = percentile * (sorted.len().saturating_sub(1) as f64);
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f64;
    sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
}

fn score_features(
    features: FeatureValues,
    medians: [f64; 5],
    iqrs: [f64; 5],
    weights: [f64; 5],
) -> f64 {
    features
        .values()
        .into_iter()
        .zip(medians)
        .zip(iqrs)
        .zip(weights)
        .map(|(((value, median), iqr), weight)| {
            let z = ((value - median) / iqr).clamp(-3.0, 3.0);
            weight * z
        })
        .sum()
}

fn load_and_validate_day2_calibration(
    path: &Path,
    manifest: &RugRealityCaptureRunManifestV1,
) -> Result<AceCoreCalibrationV1> {
    let bytes =
        fs::read(path).with_context(|| format!("read day2 calibration file {}", path.display()))?;
    let calibration: AceCoreCalibrationV1 = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode day2 calibration file {}", path.display()))?;
    validate_day2_calibration(&calibration, manifest)?;
    Ok(calibration)
}

fn validate_day2_calibration(
    calibration: &AceCoreCalibrationV1,
    manifest: &RugRealityCaptureRunManifestV1,
) -> Result<()> {
    if calibration.schema != ACE_CORE_CALIBRATION_SCHEMA
        || calibration.day_id != AceCoreProbeDayId::Day1
        || calibration.feature_contract_version != ACE_CORE_ONE_DAY_PROBE_SCHEMA
        || calibration.amount_source_label
            != "observed_buy_wallet_debit_lamports=signer_pre_balance_lamports-signer_post_balance_lamports"
        || calibration.source_baseline_sha != ACE_CORE_BASELINE_SHA
        || calibration.source_implementation_sha != manifest.implementation_sha
        || calibration.source_code_hash != manifest.code_hash
        || calibration.source_run_id.trim().is_empty()
        || calibration.source_run_id == manifest.run_id
        || calibration.cutoff_offset_ms != CUTOFF_OFFSET_MS
        || calibration.total_wallet_debit_cap_lamports != TOTAL_WALLET_DEBIT_CAP_LAMPORTS
        || calibration.max_entry_self_impact_bps != MAX_ENTRY_SELF_IMPACT_BPS
        || calibration.max_immediate_exit_impact_bps != MAX_IMMEDIATE_EXIT_IMPACT_BPS
        || calibration.max_entry_to_x3_numerator != MAX_ENTRY_TO_X3_NUMERATOR
        || calibration.max_entry_to_x3_denominator != MAX_ENTRY_TO_X3_DENOMINATOR
        || calibration.primary_exit_latency_ms != PRIMARY_EXIT_LATENCY_MS
        || calibration.sustain_confirm_at_ms != SUSTAIN_CONFIRM_AT_MS
        || calibration.max_state_lookup_lag_ms != MAX_STATE_LOOKUP_LAG_MS
        || calibration.outcome_horizon_ms != OUTCOME_HORIZON_MS
        || calibration.score_weights != [-1.0, 1.0, 1.0, 1.0, -1.0]
        || calibration
            .medians
            .iter()
            .chain(calibration.iqrs.iter())
            .chain(std::iter::once(&calibration.selected_threshold))
            .any(|value| !value.is_finite())
        || calibration.iqrs.iter().any(|iqr| *iqr == 0.0)
    {
        bail!("day2 calibration file does not match frozen ACE V3 contract")
    }
    Ok(())
}

fn base_row(
    work: &CandidateWork,
    status: AceCandidateStatusV3,
    reason: Option<String>,
) -> AceCoreCandidateRowV3 {
    AceCoreCandidateRowV3 {
        schema: ACE_CORE_ONE_DAY_PROBE_SCHEMA.to_string(),
        candidate_id: work.birth.candidate_id.clone(),
        base_mint: work.birth.payload.base_mint.clone(),
        bonding_curve: work.birth.payload.bonding_curve.clone(),
        creator: work.birth.payload.creator.clone(),
        birth_ts_ms: work.birth.payload.birth_ts_ms,
        cutoff_ts_ms: work
            .birth
            .payload
            .birth_ts_ms
            .saturating_add(CUTOFF_OFFSET_MS),
        x1_creator_buy_wallet_debit_share: None,
        x2_new_buyer_intensity_log_ratio: None,
        x3_first_buy_wallet_debit_lamports: None,
        x4_first_buy_late_early_log_ratio: None,
        x5_first_buy_wallet_debit_hhi: None,
        score: None,
        selected: None,
        entry_state_slot: None,
        entry_total_debit_lamports: None,
        entry_token_amount_raw: None,
        entry_impact_bps: None,
        immediate_exit_impact_bps: None,
        best_sustained_proxy_net_return_120s: None,
        best_trigger_ts_ms: None,
        landing_ts_ms: None,
        confirmation_ts_ms: None,
        sustained_net17_hit: None,
        status,
        reason,
        outcome_label: "observed_path_non_propagated_sustained_proxy".to_string(),
    }
}

fn add_feature_values(row: &mut AceCoreCandidateRowV3, features: FeatureValues) {
    row.x1_creator_buy_wallet_debit_share = Some(features.x1);
    row.x2_new_buyer_intensity_log_ratio = Some(features.x2);
    row.x3_first_buy_wallet_debit_lamports = Some(features.x3_lamports);
    row.x4_first_buy_late_early_log_ratio = Some(features.x4);
    row.x5_first_buy_wallet_debit_hhi = Some(features.x5);
}

fn invalid_capture_row(work: &CandidateWork) -> AceCoreCandidateRowV3 {
    let mut row = base_row(
        work,
        AceCandidateStatusV3::InvalidCapture,
        Some("run_level_capture_validation_failed".to_string()),
    );
    if let Ok(features) = work.feature_result.as_ref() {
        add_feature_values(&mut row, *features);
    }
    row
}

fn feature_failure_row(work: &CandidateWork) -> AceCoreCandidateRowV3 {
    base_row(
        work,
        AceCandidateStatusV3::NonEvaluableFeatures,
        Some(
            work.feature_result
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "feature_evidence_unavailable".to_string()),
        ),
    )
}

fn calibration_excluded_row(work: &CandidateWork, reason: Option<String>) -> AceCoreCandidateRowV3 {
    let mut row = base_row(work, AceCandidateStatusV3::CalibrationExcluded, reason);
    if let Ok(features) = work.feature_result.as_ref() {
        add_feature_values(&mut row, *features);
    }
    row
}

fn scored_candidate_row(
    work: &CandidateWork,
    features: FeatureValues,
    calibration: &AceCoreCalibrationV1,
    quote_contract: &RugScalpPumpQuoteContractV1,
) -> AceCoreCandidateRowV3 {
    let score = score_features(
        features,
        calibration.medians,
        calibration.iqrs,
        calibration.score_weights,
    );
    let selected = score >= calibration.selected_threshold;
    let mut row = base_row(
        work,
        if selected {
            AceCandidateStatusV3::EvaluableSelected
        } else {
            AceCandidateStatusV3::EvaluableRest
        },
        None,
    );
    add_feature_values(&mut row, features);
    row.score = Some(score);
    row.selected = Some(selected);
    match calculate_economic_outcome(work, features.x3_lamports, quote_contract) {
        Ok(outcome) => {
            row.entry_state_slot = Some(outcome.entry_state_slot);
            row.entry_total_debit_lamports = Some(outcome.entry_total_debit_lamports);
            row.entry_token_amount_raw = Some(outcome.entry_token_amount_raw);
            row.entry_impact_bps = Some(outcome.entry_impact_bps);
            row.immediate_exit_impact_bps = Some(outcome.immediate_exit_impact_bps);
            row.best_sustained_proxy_net_return_120s = Some(outcome.best_net_return);
            row.best_trigger_ts_ms = Some(outcome.best_trigger_ts_ms);
            row.landing_ts_ms = Some(outcome.landing_ts_ms);
            row.confirmation_ts_ms = Some(outcome.confirmation_ts_ms);
            row.sustained_net17_hit = Some(outcome.best_net_return >= 0.17);
        }
        Err(failure) => {
            row.status = failure.status();
            row.reason = Some(failure.reason().to_string());
        }
    }
    row
}

fn reserve_observation(trade: &TapeTrade) -> Option<ReserveObservation> {
    let payload = &trade.payload;
    if !payload.success || payload.is_synthetic != Some(false) || payload.complete != Some(false) {
        return None;
    }
    Some(ReserveObservation {
        event_ts_ms: payload.event_ts_ms,
        slot: canonical_slot(payload)?,
        order: canonical_trade_order(payload),
        ordinal: (trade.file_ordinal, trade.line_number),
        reserves: PumpReserveState {
            virtual_base_reserves: payload.virtual_token_reserves?,
            virtual_quote_reserves: payload.virtual_sol_reserves?,
            real_base_reserves: payload.real_token_reserves?,
            real_quote_reserves: payload.real_sol_reserves?,
        },
    })
}

fn compare_reserve_observation(
    left: &ReserveObservation,
    right: &ReserveObservation,
) -> std::cmp::Ordering {
    (left.event_ts_ms, left.slot, left.order, left.ordinal).cmp(&(
        right.event_ts_ms,
        right.slot,
        right.order,
        right.ordinal,
    ))
}

fn absolute_virtual_price_impact_bps(
    before: PumpReserveState,
    after: PumpReserveState,
) -> Option<u32> {
    if before.virtual_quote_reserves == 0
        || before.virtual_base_reserves == 0
        || after.virtual_quote_reserves == 0
        || after.virtual_base_reserves == 0
    {
        return None;
    }
    let before_numerator = u128::from(before.virtual_quote_reserves)
        .checked_mul(u128::from(after.virtual_base_reserves))?;
    let after_numerator = u128::from(after.virtual_quote_reserves)
        .checked_mul(u128::from(before.virtual_base_reserves))?;
    let denominator = before_numerator;
    let difference = after_numerator.abs_diff(before_numerator);
    let scaled = difference.checked_mul(10_000)?;
    let rounded_up = scaled
        .checked_add(denominator.checked_sub(1)?)?
        .checked_div(denominator)?;
    u32::try_from(rounded_up).ok()
}

fn reserve_state_after_quote(
    before: PumpReserveState,
    quote: &ghost_core::PumpQuoteV1,
) -> PumpReserveState {
    PumpReserveState {
        virtual_base_reserves: quote.reserve_transition.base_after,
        virtual_quote_reserves: quote.reserve_transition.quote_after,
        real_base_reserves: before.real_base_reserves.saturating_add(quote.token_amount),
        real_quote_reserves: before
            .real_quote_reserves
            .saturating_sub(quote.curve_quote_amount),
    }
}

fn calculate_economic_outcome(
    work: &CandidateWork,
    x3_lamports: u64,
    quote_contract: &RugScalpPumpQuoteContractV1,
) -> std::result::Result<EconomicOutcome, EconomicFailure> {
    let cutoff_ts_ms = work
        .birth
        .payload
        .birth_ts_ms
        .saturating_add(CUTOFF_OFFSET_MS);
    let mut states = work
        .trades
        .iter()
        .filter_map(reserve_observation)
        .filter(|state| state.event_ts_ms >= work.birth.payload.birth_ts_ms)
        .collect::<Vec<_>>();
    states.sort_by(compare_reserve_observation);
    let entry_state = states
        .iter()
        .filter(|state| state.event_ts_ms <= cutoff_ts_ms)
        .filter(|state| cutoff_ts_ms.saturating_sub(state.event_ts_ms) <= ENTRY_STATE_MAX_AGE_MS)
        .max_by(|left, right| compare_reserve_observation(left, right))
        .copied()
        .ok_or(EconomicFailure::Reserves("entry_state_missing_or_stale"))?;

    let entry_tx_cost = quote_contract
        .entry_transaction_cost_lamports()
        .map_err(|_| EconomicFailure::Reserves("entry_transaction_cost_unavailable"))?;
    let program_cap = TOTAL_WALLET_DEBIT_CAP_LAMPORTS
        .checked_sub(entry_tx_cost)
        .ok_or(EconomicFailure::Capacity(
            "entry_transaction_cost_exceeds_wallet_cap",
        ))?;
    let entry_quote = quote_contract
        .quote_buy_v2_under_wallet_cap(entry_state.slot, entry_state.reserves, program_cap)
        .map_err(|_| EconomicFailure::Capacity("entry_quote_unavailable_under_wallet_cap"))?;
    let entry_total_debit_lamports = entry_quote
        .program_settlement
        .wallet_debit_or_credit
        .checked_add(entry_tx_cost)
        .ok_or(EconomicFailure::Capacity("entry_total_debit_overflow"))?;
    if entry_total_debit_lamports > TOTAL_WALLET_DEBIT_CAP_LAMPORTS {
        return Err(EconomicFailure::Capacity(
            "entry_total_debit_exceeds_wallet_cap",
        ));
    }
    let reserves_after_entry = reserves_after_buy(entry_state.reserves, &entry_quote);
    let entry_impact_bps =
        absolute_virtual_price_impact_bps(entry_state.reserves, reserves_after_entry)
            .ok_or(EconomicFailure::Reserves("entry_impact_unavailable"))?;
    if entry_impact_bps > MAX_ENTRY_SELF_IMPACT_BPS {
        return Err(EconomicFailure::Capacity("entry_self_impact_exceeds_5pct"));
    }
    let (immediate_exit_quote, _) = quote_contract
        .executable_exit_value_lamports(
            entry_state.slot,
            reserves_after_entry,
            entry_quote.token_amount,
        )
        .map_err(|_| EconomicFailure::Capacity("immediate_full_position_exit_unquotable"))?;
    let reserves_after_immediate_exit =
        reserve_state_after_quote(reserves_after_entry, &immediate_exit_quote);
    let immediate_exit_impact_bps =
        absolute_virtual_price_impact_bps(reserves_after_entry, reserves_after_immediate_exit)
            .ok_or(EconomicFailure::Reserves(
                "immediate_exit_impact_unavailable",
            ))?;
    if immediate_exit_impact_bps > MAX_IMMEDIATE_EXIT_IMPACT_BPS {
        return Err(EconomicFailure::Capacity(
            "immediate_full_position_exit_impact_exceeds_5pct",
        ));
    }
    if u128::from(entry_total_debit_lamports)
        .checked_mul(MAX_ENTRY_TO_X3_DENOMINATOR)
        .ok_or(EconomicFailure::Capacity("entry_to_x3_comparison_overflow"))?
        > u128::from(x3_lamports)
            .checked_mul(MAX_ENTRY_TO_X3_NUMERATOR)
            .ok_or(EconomicFailure::Capacity("entry_to_x3_comparison_overflow"))?
    {
        return Err(EconomicFailure::Capacity("entry_to_x3_exceeds_10pct"));
    }

    let outcome_horizon = cutoff_ts_ms.saturating_add(OUTCOME_HORIZON_MS);
    let post_cutoff_states = states
        .iter()
        .filter(|state| state.event_ts_ms > cutoff_ts_ms && state.event_ts_ms <= outcome_horizon)
        .copied()
        .collect::<Vec<_>>();
    let mut best: Option<(f64, u64, u64, u64)> = None;
    for trigger in &post_cutoff_states {
        let landing = post_cutoff_states.iter().find(|state| {
            state.event_ts_ms >= trigger.event_ts_ms.saturating_add(PRIMARY_EXIT_LATENCY_MS)
                && state.event_ts_ms
                    <= trigger
                        .event_ts_ms
                        .saturating_add(PRIMARY_EXIT_LATENCY_MS)
                        .saturating_add(MAX_STATE_LOOKUP_LAG_MS)
        });
        let confirmation = post_cutoff_states.iter().find(|state| {
            state.event_ts_ms >= trigger.event_ts_ms.saturating_add(SUSTAIN_CONFIRM_AT_MS)
                && state.event_ts_ms
                    <= trigger
                        .event_ts_ms
                        .saturating_add(SUSTAIN_CONFIRM_AT_MS)
                        .saturating_add(MAX_STATE_LOOKUP_LAG_MS)
        });
        let (Some(landing), Some(confirmation)) = (landing, confirmation) else {
            continue;
        };
        if landing.slot == confirmation.slot {
            continue;
        }
        let Ok((_, landing_exit_value)) = quote_contract.executable_exit_value_lamports(
            landing.slot,
            landing.reserves,
            entry_quote.token_amount,
        ) else {
            continue;
        };
        let Ok((_, confirmation_exit_value)) = quote_contract.executable_exit_value_lamports(
            confirmation.slot,
            confirmation.reserves,
            entry_quote.token_amount,
        ) else {
            continue;
        };
        let landing_return = (landing_exit_value as f64 - entry_total_debit_lamports as f64)
            / entry_total_debit_lamports as f64;
        let confirmation_return = (confirmation_exit_value as f64
            - entry_total_debit_lamports as f64)
            / entry_total_debit_lamports as f64;
        let sustained_return = landing_return.min(confirmation_return);
        if !sustained_return.is_finite() {
            continue;
        }
        let candidate = (
            sustained_return,
            trigger.event_ts_ms,
            landing.event_ts_ms,
            confirmation.event_ts_ms,
        );
        let replace = best.as_ref().map_or(true, |current| {
            candidate.0 > current.0
                || (candidate.0 == current.0
                    && (candidate.1, candidate.2, candidate.3) < (current.1, current.2, current.3))
        });
        if replace {
            best = Some(candidate);
        }
    }
    let Some((best_net_return, best_trigger_ts_ms, landing_ts_ms, confirmation_ts_ms)) = best
    else {
        return Err(EconomicFailure::SustainCoverage(
            "no_legal_pre_migration_sustained_landing_confirmation_pair",
        ));
    };
    Ok(EconomicOutcome {
        entry_state_slot: entry_state.slot,
        entry_total_debit_lamports,
        entry_token_amount_raw: entry_quote.token_amount,
        entry_impact_bps,
        immediate_exit_impact_bps,
        best_net_return,
        best_trigger_ts_ms,
        landing_ts_ms,
        confirmation_ts_ms,
    })
}

fn metrics_from_rows(rows: &[AceCoreCandidateRowV3]) -> AceCoreMetricsV1 {
    let selected = rows
        .iter()
        .filter(|row| row.status == AceCandidateStatusV3::EvaluableSelected)
        .filter_map(|row| {
            Some((
                row.best_sustained_proxy_net_return_120s?,
                row.sustained_net17_hit?,
            ))
        })
        .collect::<Vec<_>>();
    let rest = rows
        .iter()
        .filter(|row| row.status == AceCandidateStatusV3::EvaluableRest)
        .filter_map(|row| {
            Some((
                row.best_sustained_proxy_net_return_120s?,
                row.sustained_net17_hit?,
            ))
        })
        .collect::<Vec<_>>();
    let selected_mean = mean(
        selected
            .iter()
            .map(|(outcome, _)| *outcome)
            .collect::<Vec<_>>(),
    );
    let rest_mean = mean(rest.iter().map(|(outcome, _)| *outcome).collect::<Vec<_>>());
    let selected_median = median_f64(
        selected
            .iter()
            .map(|(outcome, _)| *outcome)
            .collect::<Vec<_>>(),
    );
    let rest_median = median_f64(rest.iter().map(|(outcome, _)| *outcome).collect::<Vec<_>>());
    let selected_hit_rate = hit_rate(&selected);
    let rest_hit_rate = hit_rate(&rest);
    let selected_count = selected.len();
    let rest_count = rest.len();
    AceCoreMetricsV1 {
        selected_count,
        rest_count,
        evaluable_coverage_pct: if rows.is_empty() {
            0.0
        } else {
            (selected_count.saturating_add(rest_count) as f64 / rows.len() as f64) * 100.0
        },
        delta_mean: selected_mean
            .zip(rest_mean)
            .map(|(left, right)| left - right),
        delta_median: selected_median
            .zip(rest_median)
            .map(|(left, right)| left - right),
        delta_sustained_hit17: selected_hit_rate
            .zip(rest_hit_rate)
            .map(|(left, right)| left - right),
        selected_mean,
        rest_mean,
        selected_median,
        rest_median,
        selected_sustained_net17_hit_rate: selected_hit_rate,
        rest_sustained_net17_hit_rate: rest_hit_rate,
    }
}

fn mean(values: Vec<f64>) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn median_f64(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let midpoint = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[midpoint - 1] + values[midpoint]) / 2.0
    } else {
        values[midpoint]
    })
}

fn hit_rate(values: &[(f64, bool)]) -> Option<f64> {
    (!values.is_empty())
        .then(|| values.iter().filter(|(_, hit)| *hit).count() as f64 / values.len() as f64)
}

fn non_evaluable_counts(rows: &[AceCoreCandidateRowV3]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    // Calibration rows have their own explicit denominator/reporting field.
    // They are deliberately excluded from `non_evaluable_count_by_reason` so
    // the latter remains a breakdown of evidence/capacity/coverage failures.
    for row in rows.iter().filter(|row| {
        !row.status.is_evaluable() && row.status != AceCandidateStatusV3::CalibrationExcluded
    }) {
        if let Some(reason) = &row.reason {
            *counts.entry(reason.clone()).or_insert(0) += 1;
        } else {
            *counts.entry(format!("{:?}", row.status)).or_insert(0) += 1;
        }
    }
    counts
}

fn pooled_is_promising(metrics: &AceCoreMetricsV1, minimum_selected_count: usize) -> bool {
    metrics.delta_mean.is_some_and(|value| value > 0.0)
        && metrics.delta_median.is_some_and(|value| value > 0.0)
        && metrics.selected_mean.is_some_and(|value| value > 0.0)
        && metrics.selected_count >= minimum_selected_count
        && metrics.evaluable_coverage_pct >= 50.0
}

fn day_is_negative(metrics: &AceCoreMetricsV1) -> bool {
    metrics.delta_mean.is_some_and(|value| value <= 0.0)
        && metrics.delta_median.is_some_and(|value| value <= 0.0)
}

fn day1_terminal_status(metrics: &AceCoreMetricsV1) -> String {
    if pooled_is_promising(metrics, 50) {
        "ACE_PROBE_PROMISING_NOT_PROVEN".to_string()
    } else if day_is_negative(metrics) {
        "ACE_PROBE_DAY1_NEGATIVE_UNCONFIRMED".to_string()
    } else {
        "ACE_PROBE_DAY1_MIXED".to_string()
    }
}

fn write_candidate_rows_new(path: &Path, rows: &[AceCoreCandidateRowV3]) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create ACE candidate rows {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    for row in rows {
        serde_json::to_writer(&mut writer, row)
            .with_context(|| format!("serialize ACE candidate row {}", row.candidate_id))?;
        writer
            .write_all(b"\n")
            .with_context(|| format!("write ACE candidate rows {}", path.display()))?;
    }
    writer
        .flush()
        .with_context(|| format!("flush ACE candidate rows {}", path.display()))?;
    writer
        .into_inner()
        .map_err(|error| anyhow!("finish ACE candidate rows {}: {}", path.display(), error))?
        .sync_all()
        .with_context(|| format!("sync ACE candidate rows {}", path.display()))?;
    Ok(())
}

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .with_context(|| format!("serialize ACE output {}", path.display()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create ACE output {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write ACE output {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("terminate ACE output {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync ACE output {}", path.display()))?;
    Ok(())
}

fn load_day1_probe_artifacts(
    calibration_path: &Path,
) -> Result<(AceCoreOneDaySummaryV1, Vec<AceCoreCandidateRowV3>)> {
    let parent = calibration_path.parent().ok_or_else(|| {
        anyhow!(
            "day2 calibration path has no parent directory: {}",
            calibration_path.display()
        )
    })?;
    let summary_path = parent.join(SUMMARY_FILE);
    let summary_bytes = fs::read(&summary_path)
        .with_context(|| format!("read day1 probe summary {}", summary_path.display()))?;
    let summary: AceCoreOneDaySummaryV1 = serde_json::from_slice(&summary_bytes)
        .with_context(|| format!("decode day1 probe summary {}", summary_path.display()))?;
    if summary.schema != ACE_CORE_SUMMARY_SCHEMA
        || summary.day_id != AceCoreProbeDayId::Day1
        || !matches!(
            summary.terminal_status.as_str(),
            "ACE_PROBE_DAY1_NEGATIVE_UNCONFIRMED" | "ACE_PROBE_DAY1_MIXED"
        )
    {
        bail!("day2 requires a completed negative-or-mixed day1 probe summary")
    }
    let rows_path = parent.join(CANDIDATE_ROWS_FILE);
    let file = File::open(&rows_path)
        .with_context(|| format!("open day1 candidate rows {}", rows_path.display()))?;
    let mut rows = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("read day1 candidate row {}", index + 1))?;
        if line.trim().is_empty() {
            bail!("blank day1 candidate row at {}", index + 1);
        }
        rows.push(
            serde_json::from_str(&line)
                .with_context(|| format!("decode day1 candidate row {}", index + 1))?,
        );
    }
    Ok((summary, rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_brain::events::schema::EventEnvelope;
    use ghost_core::{
        quote_exact_base_out, FeeRounding, ProgramFeeRule, ProgramFeeSchedule,
        ProgramFeeScheduleEvidenceV1, PumpRouteVariant, TransactionCosts,
    };

    use crate::rug_reality_capture::{RugRealityCaptureConfigV1, RugRealityCostProfileV1};
    use crate::rug_scalp_v2::{
        RugScalpPumpFeeScheduleV1, RugScalpPumpQuoteAuthorityV1,
        RugScalpRuntimeFeeAuthorityManifestV1, RUG_SCALP_ENTRY_ROUTE, RUG_SCALP_EXIT_ROUTE,
    };

    fn test_birth(candidate_id: &str, birth_ts_ms: u64) -> TapeBirth {
        TapeBirth {
            candidate_id: candidate_id.to_string(),
            payload: NewPoolDetectedPayload {
                is_birth_event: true,
                pool_amm_id: "curve".to_string(),
                pool_id: "curve".to_string(),
                base_mint: "mint".to_string(),
                mint_id: "mint".to_string(),
                quote_mint: WSOL_MINT.to_string(),
                bonding_curve: "curve".to_string(),
                creator: "wallet-0".to_string(),
                amm_program: RUG_SCALP_PUMP_PROGRAM.to_string(),
                signature: format!("birth-{candidate_id}"),
                birth_ts_ms,
                timestamp_ms: birth_ts_ms,
                event_slot: Some(1),
                detected_wall_ts_ms: Some(birth_ts_ms),
                chain_event_ts_ms: Some(birth_ts_ms),
                source: "canonical_pr1e".to_string(),
            },
            event_id: format!("birth-event-{candidate_id}"),
            file_ordinal: 0,
            line_number: 1,
        }
    }

    fn test_trade(event_ts_ms: u64, wallet: &str, ordinal: u32, debit: u64) -> TapeTrade {
        TapeTrade {
            payload: PoolTransactionPayload {
                schema_version: "v1".to_string(),
                pool_amm_id: "curve".to_string(),
                pool_id: "curve".to_string(),
                source_pool_amm_id: None,
                base_mint: Some("mint".to_string()),
                mint_id: Some("mint".to_string()),
                token_mint: Some("mint".to_string()),
                quote_mint: Some(WSOL_MINT.to_string()),
                bonding_curve: "curve".to_string(),
                signature: format!("trade-signature-{ordinal}"),
                event_slot: Some(100 + u64::from(ordinal)),
                slot: Some(100 + u64::from(ordinal)),
                tx_index: Some(ordinal),
                event_ordinal: Some(ordinal),
                outer_instruction_index: Some(0),
                inner_group_index: Some(0),
                event_ts_ms,
                timestamp_ms: event_ts_ms,
                arrival_ts_ms: event_ts_ms,
                source: "canonical_primary".to_string(),
                side: "buy".to_string(),
                is_buy: true,
                success: true,
                error_code: None,
                signer: wallet.to_string(),
                wallet: wallet.to_string(),
                signer_pre_balance_lamports: Some(2_000_000_000u64.saturating_add(debit)),
                signer_post_balance_lamports: Some(2_000_000_000),
                is_synthetic: Some(false),
                quote_amount_sol: 999.0,
                volume_sol: 999.0,
                sol_amount_lamports: Some(999_999_999),
                effective_curve_quote_lamports: Some(999_999_999),
                token_amount_units: Some(1),
                virtual_sol_reserves: Some(10_000_000_000),
                virtual_token_reserves: Some(1_000_000_000_000),
                real_sol_reserves: Some(10_000_000_000),
                real_token_reserves: Some(1_000_000_000_000),
                complete: Some(false),
                reserve_base: None,
                reserve_quote: None,
                price_quote: None,
                v_tokens_in_bonding_curve: None,
                v_sol_in_bonding_curve: None,
                market_cap_sol: None,
                curve_progress_pct: None,
                curve_progress_status: "known".to_string(),
                curve_finality: "canonical".to_string(),
                curve_data_known: true,
                execution_account_contract_status: "not_used_by_ace_probe".to_string(),
                execution_account_contract_reason: None,
            },
            event_id: format!("trade-event-{ordinal}"),
            file_ordinal: 0,
            line_number: ordinal as usize + 1,
        }
    }

    fn feature_trades(birth_ts_ms: u64, seed: u64) -> Vec<TapeTrade> {
        let cutoff = birth_ts_ms + CUTOFF_OFFSET_MS;
        let late = seed % 2 == 0;
        let times = if late {
            [
                cutoff - 7_000,
                cutoff - 6_000,
                cutoff - 1_500,
                cutoff - 1_000,
            ]
        } else {
            [
                cutoff - 7_000,
                cutoff - 5_000,
                cutoff - 4_000,
                cutoff - 3_000,
            ]
        };
        times
            .into_iter()
            .enumerate()
            .map(|(index, ts)| {
                let debit = match index {
                    0 => 1_000_000 + seed,
                    1 => 2_000_000 + seed.saturating_mul(2),
                    2 => 4_000_000 + seed.saturating_mul(3),
                    _ => 8_000_000 + seed.saturating_mul(5),
                };
                test_trade(ts, &format!("wallet-{index}"), index as u32 + 1, debit)
            })
            .collect()
    }

    fn feature_work(seed: u64) -> CandidateWork {
        let birth = test_birth(&format!("candidate-{seed}"), 1_000_000 + seed * 100_000);
        let trades = feature_trades(birth.payload.birth_ts_ms, seed);
        let feature_result = calculate_features(&birth, &trades);
        CandidateWork {
            birth,
            trades,
            feature_result,
        }
    }

    fn runtime_schedule(route_variant: PumpRouteVariant, id: &str) -> RugScalpPumpFeeScheduleV1 {
        RugScalpPumpFeeScheduleV1 {
            route_variant,
            schedule: ProgramFeeSchedule {
                fee_schedule_id: id.to_string(),
                effective_slot: 0,
                evidence: ProgramFeeScheduleEvidenceV1::OnChainConfig {
                    config_pubkey: "canonical-config".to_string(),
                    owner_program: RUG_SCALP_PUMP_PROGRAM.to_string(),
                    account_data_hash: "canonical-hash".to_string(),
                    observed_slot: 0,
                },
                rules: vec![ProgramFeeRule {
                    component_id: "fee".to_string(),
                    numerator: 1,
                    denominator: 10_000,
                    rounding: FeeRounding::Ceil,
                }],
            },
        }
    }

    fn quote_authority() -> RugScalpPumpQuoteAuthorityV1 {
        RugScalpPumpQuoteAuthorityV1 {
            schedules: vec![
                runtime_schedule(RUG_SCALP_ENTRY_ROUTE, "buy-v2"),
                runtime_schedule(RUG_SCALP_EXIT_ROUTE, "legacy-sell"),
            ],
            entry_transaction_costs: TransactionCosts::default(),
            exit_transaction_costs: TransactionCosts::default(),
        }
    }

    fn manifest(run_id: &str) -> RugRealityCaptureRunManifestV1 {
        let implementation_sha = "1111111111111111111111111111111111111111".to_string();
        RugRealityCaptureRunManifestV1 {
            schema_version: 3,
            run_id: run_id.to_string(),
            observe_only: true,
            signal_detector: ACE_CORE_SIGNAL_DETECTOR.to_string(),
            entry_route_id: "buy_v2".to_string(),
            exit_route_id: "legacy_sell".to_string(),
            config_hash: "config-hash".to_string(),
            baseline_sha: ACE_CORE_BASELINE_SHA.to_string(),
            code_hash: format!("git:{implementation_sha}"),
            implementation_sha,
            binary_hash: "binary-hash".to_string(),
            health_evidence_path: "health-evidence.json".to_string(),
            authority_epoch_id: 42,
            event_writer_run_id: run_id.to_string(),
            event_writer_optional_events_enabled: true,
            cost_profile: RugRealityCostProfileV1::default(),
            runtime_fee_authority: RugScalpRuntimeFeeAuthorityManifestV1 {
                schema_version: 1,
                observed_slot: 0,
                effective_slot: 0,
                global_config_pubkey: "global".to_string(),
                global_owner_program: RUG_SCALP_PUMP_PROGRAM.to_string(),
                global_account_data_hash: "global-hash".to_string(),
                fee_config_pubkey: "fee".to_string(),
                fee_config_owner_program: "fee-owner".to_string(),
                fee_config_account_data_hash: "fee-hash".to_string(),
                evidence_hash: "frozen-fee-evidence".to_string(),
                buy_v2_fee_schedule_id: "buy-v2".to_string(),
                legacy_sell_fee_schedule_id: "legacy-sell".to_string(),
            },
            pump_quote_authority: quote_authority(),
        }
    }

    fn write_valid_health_evidence(
        manifest_path: &Path,
        manifest: &RugRealityCaptureRunManifestV1,
    ) {
        let receipt = RugRealityCaptureHealthEvidenceV1 {
            schema_version: ACE_CAPTURE_HEALTH_SCHEMA_VERSION,
            run_id: manifest.run_id.clone(),
            manifest_sha256: sha256_hex(&fs::read(manifest_path).expect("manifest bytes")),
            start_metrics_sha256: "start-metrics".to_string(),
            end_metrics_sha256: "end-metrics".to_string(),
            pr1_runtime_bypass_attempt_total: 0,
            pr1_runtime_candidate_admission_closed_total: 0,
            pr1_runtime_primary_coverage_gap_total: 0,
            event_writer_write_failure_count: 0,
            event_writer_lock_failure_count: 0,
            controlled_shutdown: true,
            event_files_cleanly_flushed: true,
            log_evidence_clean: true,
        };
        write_json_new(Path::new(&manifest.health_evidence_path), &receipt)
            .expect("write valid capture health evidence");
    }

    fn set_reserves(trade: &mut TapeTrade, base: u64, quote: u64) {
        trade.payload.virtual_token_reserves = Some(base);
        trade.payload.virtual_sol_reserves = Some(quote);
        trade.payload.real_token_reserves = Some(base);
        trade.payload.real_sol_reserves = Some(quote);
        trade.payload.complete = Some(false);
        trade.payload.is_synthetic = Some(false);
    }

    fn economic_work(
        confirmation_matches_entry: bool,
        confirmation_same_slot: bool,
    ) -> CandidateWork {
        let birth = test_birth("economic", 1_000);
        let cutoff = birth.payload.birth_ts_ms + CUTOFF_OFFSET_MS;
        let mut trades = feature_trades(birth.payload.birth_ts_ms, 1);
        let mut entry = test_trade(cutoff, "entry-wallet", 20, 10_000_000);
        set_reserves(&mut entry, 1_000_000_000_000, 10_000_000_000);
        let mut trigger = test_trade(cutoff + 100, "trigger-wallet", 21, 10_000_000);
        set_reserves(&mut trigger, 900_000_000_000, 20_000_000_000);
        let mut landing = test_trade(cutoff + 350, "landing-wallet", 22, 10_000_000);
        set_reserves(&mut landing, 700_000_000_000, 50_000_000_000);
        let mut confirmation = test_trade(cutoff + 1_100, "confirmation-wallet", 23, 10_000_000);
        if confirmation_matches_entry {
            set_reserves(&mut confirmation, 1_000_000_000_000, 10_000_000_000);
        } else {
            set_reserves(&mut confirmation, 700_000_000_000, 50_000_000_000);
        }
        if confirmation_same_slot {
            confirmation.payload.event_slot = landing.payload.event_slot;
            confirmation.payload.slot = landing.payload.slot;
        }
        trades.extend([entry, trigger, landing, confirmation]);
        CandidateWork {
            feature_result: calculate_features(&birth, &trades),
            birth,
            trades,
        }
    }

    #[test]
    fn missing_balance_never_falls_back_to_instruction_amount() {
        let birth = test_birth("candidate", 1_000);
        let mut trades = feature_trades(birth.payload.birth_ts_ms, 1);
        trades[0].payload.signer_pre_balance_lamports = None;
        trades[0].payload.sol_amount_lamports = Some(1);
        trades[0].payload.effective_curve_quote_lamports = Some(1);
        assert_eq!(
            calculate_features(&birth, &trades),
            Err("successful_buy_pre_balance_missing".to_string())
        );
    }

    #[test]
    fn pool_transaction_payload_preserves_signer_pre_and_post_balances() {
        let payload = test_trade(1_000, "wallet", 1, 420_000_000).payload;
        let encoded = serde_json::to_value(&payload).expect("serialize payload");
        assert_eq!(encoded["signer_pre_balance_lamports"], 2_420_000_000u64);
        assert_eq!(encoded["signer_post_balance_lamports"], 2_000_000_000u64);
        assert_eq!(encoded["is_synthetic"], false);
    }

    #[test]
    fn non_positive_wallet_debit_is_non_evaluable() {
        let birth = test_birth("candidate", 1_000);
        let mut trades = feature_trades(birth.payload.birth_ts_ms, 1);
        trades[0].payload.signer_post_balance_lamports =
            trades[0].payload.signer_pre_balance_lamports;
        assert_eq!(
            calculate_features(&birth, &trades),
            Err("successful_buy_wallet_debit_non_positive".to_string())
        );
    }

    #[test]
    fn effective_curve_quote_is_not_a_feature_input() {
        let birth = test_birth("candidate", 1_000);
        let trades = feature_trades(birth.payload.birth_ts_ms, 1);
        let expected = calculate_features(&birth, &trades).expect("feature evidence");
        let mut altered = trades.clone();
        for trade in &mut altered {
            trade.payload.effective_curve_quote_lamports = Some(u64::MAX);
            trade.payload.sol_amount_lamports = Some(u64::MAX);
            trade.payload.volume_sol = f64::INFINITY;
        }
        assert_eq!(calculate_features(&birth, &altered).unwrap(), expected);
    }

    #[test]
    fn post_cutoff_trade_cannot_change_features() {
        let birth = test_birth("candidate", 1_000);
        let mut trades = feature_trades(birth.payload.birth_ts_ms, 1);
        let expected = calculate_features(&birth, &trades).expect("feature evidence");
        let cutoff = birth.payload.birth_ts_ms + CUTOFF_OFFSET_MS;
        let mut after_cutoff = test_trade(cutoff + 1, "late-wallet", 99, 999_999_999);
        after_cutoff.payload.signer_pre_balance_lamports = None;
        trades.push(after_cutoff);
        assert_eq!(calculate_features(&birth, &trades).unwrap(), expected);
    }

    #[test]
    fn same_signature_multi_mutation_is_not_deduplicated_by_signature_alone() {
        let mut first = test_trade(1_000, "wallet-0", 1, 1);
        first.payload.signature = "same-signature".to_string();
        let mut second = test_trade(1_001, "wallet-1", 2, 1);
        second.payload.signature = "same-signature".to_string();
        let mut tape = Tape {
            births: Vec::new(),
            trades: vec![first, second],
            invalid_reasons: BTreeSet::new(),
        };
        let birth_keys = [BirthKey {
            base_mint: "mint".to_string(),
            bonding_curve: "curve".to_string(),
        }]
        .into_iter()
        .collect::<BTreeSet<_>>();
        assert_eq!(
            strict_trade_index(&mut tape, &birth_keys)
                .get(&BirthKey {
                    base_mint: "mint".to_string(),
                    bonding_curve: "curve".to_string(),
                })
                .expect("strict key")
                .len(),
            2
        );
    }

    #[test]
    fn failed_reserve_row_cannot_be_used_as_entry_state() {
        let mut work = economic_work(false, false);
        let cutoff = work.birth.payload.birth_ts_ms + CUTOFF_OFFSET_MS;
        let entry = work
            .trades
            .iter_mut()
            .find(|trade| trade.payload.event_ts_ms == cutoff)
            .expect("entry fixture");
        entry.payload.success = false;
        let contract = quote_authority().materialize().expect("quote contract");
        assert!(matches!(
            calculate_economic_outcome(&work, 10_000_000_000, &contract),
            Err(EconomicFailure::Reserves("entry_state_missing_or_stale"))
        ));
    }

    #[test]
    fn failed_reserve_rows_cannot_form_trigger_landing_or_confirmation() {
        let mut work = economic_work(false, false);
        let cutoff = work.birth.payload.birth_ts_ms + CUTOFF_OFFSET_MS;
        for trade in &mut work.trades {
            if trade.payload.event_ts_ms > cutoff {
                trade.payload.success = false;
            }
        }
        let contract = quote_authority().materialize().expect("quote contract");
        assert!(matches!(
            calculate_economic_outcome(&work, 10_000_000_000, &contract),
            Err(EconomicFailure::SustainCoverage(_))
        ));
    }

    #[test]
    fn malformed_pump_sol_birth_is_retained_and_invalidates_capture() {
        let mut malformed = test_birth("malformed", 1_000);
        malformed.payload.base_mint.clear();
        let mut tape = Tape {
            births: vec![malformed],
            trades: Vec::new(),
            invalid_reasons: BTreeSet::new(),
        };
        let (births, duplicates) = canonical_births(&mut tape);
        assert_eq!(births.len(), 1);
        assert_eq!(duplicates, 0);
        assert!(tape
            .invalid_reasons
            .contains("pump_sol_birth_base_mint_missing"));
        assert!(matches!(
            birth_key(&births[0].payload),
            BirthKeyResolution::Malformed("pump_sol_birth_base_mint_missing")
        ));
    }

    #[test]
    fn unjoinable_trade_alias_conflict_invalidates_capture() {
        let birth = test_birth("candidate", 1_000);
        let mut trade = test_trade(1_100, "wallet", 1, 1);
        trade.payload.token_mint = Some("conflicting-mint".to_string());
        let mut tape = Tape {
            births: vec![birth],
            trades: vec![trade],
            invalid_reasons: BTreeSet::new(),
        };
        let (births, _) = canonical_births(&mut tape);
        let keys = births
            .iter()
            .filter_map(|birth| match birth_key(&birth.payload) {
                BirthKeyResolution::Eligible(key) => Some(key),
                BirthKeyResolution::OutsideUniverse | BirthKeyResolution::Malformed(_) => None,
            })
            .collect::<BTreeSet<_>>();
        let indexed = strict_trade_index(&mut tape, &keys);
        assert!(indexed.is_empty());
        assert!(tape
            .invalid_reasons
            .contains("pool_transaction_mint_alias_conflict"));
    }

    #[test]
    fn divergent_duplicate_full_mutation_key_invalidates_capture() {
        let birth = test_birth("candidate", 1_000);
        let first = test_trade(1_100, "wallet-a", 1, 1);
        let mut conflicting = first.clone();
        conflicting.payload.signer = "wallet-b".to_string();
        conflicting.payload.wallet = "wallet-b".to_string();
        conflicting.event_id = "conflicting-delivery".to_string();
        conflicting.line_number = 99;
        let mut tape = Tape {
            births: vec![birth],
            trades: vec![first, conflicting],
            invalid_reasons: BTreeSet::new(),
        };
        let (births, _) = canonical_births(&mut tape);
        let keys = births
            .iter()
            .filter_map(|birth| match birth_key(&birth.payload) {
                BirthKeyResolution::Eligible(key) => Some(key),
                BirthKeyResolution::OutsideUniverse | BirthKeyResolution::Malformed(_) => None,
            })
            .collect::<BTreeSet<_>>();
        let indexed = strict_trade_index(&mut tape, &keys);
        assert_eq!(indexed.values().next().expect("trade group").len(), 1);
        assert!(tape
            .invalid_reasons
            .contains("pool_transaction_divergent_full_mutation_duplicate"));
    }

    #[test]
    fn unknown_pool_transaction_schema_invalidates_capture() {
        let birth = test_birth("candidate", 1_000);
        let mut trade = test_trade(1_100, "wallet", 1, 1);
        trade.payload.schema_version = "unknown".to_string();
        let mut tape = Tape {
            births: vec![birth],
            trades: vec![trade],
            invalid_reasons: BTreeSet::new(),
        };
        let (births, _) = canonical_births(&mut tape);
        let keys = births
            .iter()
            .filter_map(|birth| match birth_key(&birth.payload) {
                BirthKeyResolution::Eligible(key) => Some(key),
                BirthKeyResolution::OutsideUniverse | BirthKeyResolution::Malformed(_) => None,
            })
            .collect::<BTreeSet<_>>();
        let indexed = strict_trade_index(&mut tape, &keys);
        assert!(indexed.is_empty());
        assert!(tape
            .invalid_reasons
            .contains("pool_transaction_schema_version_mismatch"));
    }

    #[test]
    fn first_250_feature_evaluable_births_are_calibration_only() {
        let works = (0..251).map(feature_work).collect::<Vec<_>>();
        let (calibration, indices, error) = create_day1_calibration(&works, &manifest("day1"));
        assert!(error.is_none());
        assert!(calibration.is_some());
        assert_eq!(indices.len(), CALIBRATION_BIRTHS);
        assert!(indices.contains(&0));
        assert!(indices.contains(&(CALIBRATION_BIRTHS - 1)));
        assert!(!indices.contains(&CALIBRATION_BIRTHS));
    }

    #[test]
    fn calibration_excluded_rows_are_reported_separately_from_non_evaluable_reasons() {
        let work = feature_work(1);
        let calibration = calibration_excluded_row(&work, None);
        let feature_failure = feature_failure_row(&CandidateWork {
            feature_result: Err("successful_buy_pre_balance_missing".to_string()),
            ..work
        });
        let counts = non_evaluable_counts(&[calibration, feature_failure]);
        assert_eq!(counts.len(), 1);
        assert_eq!(counts["successful_buy_pre_balance_missing"], 1);
    }

    #[test]
    fn day2_rejects_missing_or_mismatched_calibration() {
        let args = AceCoreOneDayProbeArgs {
            events_dir: PathBuf::from("events"),
            manifest_path: PathBuf::from("manifest"),
            output_dir: PathBuf::from("output"),
            day_id: AceCoreProbeDayId::Day2,
            calibration_path: None,
        };
        assert!(validate_cli_contract(&args).is_err());

        let work = feature_work(1);
        let (calibration, _, _) = create_day1_calibration(
            &(0..250).map(feature_work).collect::<Vec<_>>(),
            &manifest("day1"),
        );
        let mut mismatched = calibration.expect("calibration");
        mismatched.cutoff_offset_ms += 1;
        assert!(validate_day2_calibration(&mismatched, &manifest("day2")).is_err());
        assert!(work.feature_result.is_ok());
    }

    #[test]
    fn manifest_materializes_frozen_typed_quote_authority() {
        let manifest = manifest("day1");
        assert!(validate_manifest(&manifest).is_empty());
        assert!(manifest.pump_quote_authority.materialize().is_ok());
    }

    #[test]
    fn capture_health_evidence_requires_zero_integrity_counters_and_manifest_hash() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manifest_path = temp.path().join("manifest.json");
        let mut capture_manifest = manifest("health-run");
        capture_manifest.health_evidence_path = temp
            .path()
            .join("health.json")
            .to_string_lossy()
            .into_owned();
        write_json_new(&manifest_path, &capture_manifest).expect("manifest");
        write_valid_health_evidence(&manifest_path, &capture_manifest);
        assert!(validate_capture_health_evidence(&manifest_path, &capture_manifest).is_empty());

        let health_path = Path::new(&capture_manifest.health_evidence_path);
        let mut receipt: RugRealityCaptureHealthEvidenceV1 =
            serde_json::from_slice(&fs::read(health_path).expect("health bytes"))
                .expect("health receipt");
        receipt.pr1_runtime_bypass_attempt_total = 1;
        fs::remove_file(health_path).expect("remove fixture health receipt");
        write_json_new(health_path, &receipt).expect("rewrite health receipt");
        assert!(
            validate_capture_health_evidence(&manifest_path, &capture_manifest)
                .contains("capture_health_pr1_runtime_bypass_attempt_nonzero")
        );
    }

    #[test]
    fn probe_uses_existing_buy_v2_quote_contract_exactly() {
        let authority = quote_authority();
        let contract = authority.materialize().expect("materialize authority");
        let reserves = PumpReserveState {
            virtual_base_reserves: 1_000_000_000_000,
            virtual_quote_reserves: 10_000_000_000,
            real_base_reserves: 1_000_000_000_000,
            real_quote_reserves: 10_000_000_000,
        };
        let contract_quote = contract
            .quote_buy_v2_under_wallet_cap(100, reserves, 150_000_000)
            .expect("typed contract quote");
        let direct = quote_exact_base_out(
            RUG_SCALP_ENTRY_ROUTE,
            reserves,
            contract_quote.token_amount,
            150_000_000,
            &authority.schedules[0].schedule,
        )
        .expect("canonical quote math");
        assert_eq!(contract_quote, direct);
    }

    #[test]
    fn entry_and_immediate_exit_impacts_are_separate_measurements() {
        let before = PumpReserveState {
            virtual_base_reserves: 1_000,
            virtual_quote_reserves: 1_000,
            real_base_reserves: 1_000,
            real_quote_reserves: 1_000,
        };
        let after_entry = PumpReserveState {
            virtual_base_reserves: 900,
            virtual_quote_reserves: 1_112,
            real_base_reserves: 900,
            real_quote_reserves: 1_112,
        };
        let after_exit = PumpReserveState {
            virtual_base_reserves: 1_000,
            virtual_quote_reserves: 1_000,
            real_base_reserves: 1_000,
            real_quote_reserves: 1_000,
        };
        let entry_impact = absolute_virtual_price_impact_bps(before, after_entry).unwrap();
        let exit_impact = absolute_virtual_price_impact_bps(after_entry, after_exit).unwrap();
        assert_ne!(entry_impact, exit_impact);
    }

    #[test]
    fn typed_entry_never_exceeds_total_wallet_debit_cap() {
        let work = economic_work(false, false);
        let contract = quote_authority().materialize().expect("quote contract");
        let outcome = calculate_economic_outcome(&work, 10_000_000_000, &contract)
            .expect("legal sustained outcome");
        assert!(
            outcome.entry_total_debit_lamports <= TOTAL_WALLET_DEBIT_CAP_LAMPORTS,
            "probe must never reduce or exceed the frozen notional cap"
        );
    }

    #[test]
    fn capacity_violation_is_typed_and_never_shrinks_notional() {
        let work = economic_work(false, false);
        let contract = quote_authority().materialize().expect("quote contract");
        assert!(matches!(
            calculate_economic_outcome(&work, 1, &contract),
            Err(EconomicFailure::Capacity("entry_to_x3_exceeds_10pct"))
        ));
    }

    #[test]
    fn same_slot_spike_without_confirmation_is_not_sustained() {
        let work = economic_work(false, true);
        let contract = quote_authority().materialize().expect("quote contract");
        assert!(matches!(
            calculate_economic_outcome(&work, 10_000_000_000, &contract),
            Err(EconomicFailure::SustainCoverage(_))
        ));
    }

    #[test]
    fn outcome_state_at_or_before_cutoff_is_ignored() {
        let mut work = economic_work(false, false);
        let cutoff = work.birth.payload.birth_ts_ms + CUTOFF_OFFSET_MS;
        work.trades
            .retain(|trade| trade.payload.event_ts_ms <= cutoff);
        let contract = quote_authority().materialize().expect("quote contract");
        assert!(matches!(
            calculate_economic_outcome(&work, 10_000_000_000, &contract),
            Err(EconomicFailure::SustainCoverage(_))
        ));
    }

    #[test]
    fn weak_confirmation_keeps_sustained_hit_below_seventeen_percent() {
        let work = economic_work(true, false);
        let contract = quote_authority().materialize().expect("quote contract");
        let outcome = calculate_economic_outcome(&work, 10_000_000_000, &contract)
            .expect("legal landing and confirmation pair");
        assert!(outcome.best_net_return < 0.17);
    }

    #[test]
    fn capture_preflight_rejects_optional_events_disabled() {
        let config = RugRealityCaptureConfigV1 {
            enabled: true,
            ..RugRealityCaptureConfigV1::default()
        };
        assert!(config.validate_event_writer_contract(false).is_err());
    }

    #[test]
    fn rollout_config_is_shadow_only_and_enables_optional_trade_evidence() {
        let config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../configs/rollout/ace-core-one-day-probe-r1.toml");
        let config = crate::config::LauncherConfig::from_file(&config_path)
            .expect("ACE rollout config must decode");
        assert!(matches!(
            config.execution.execution_mode,
            crate::config::ExecutionMode::Shadow
        ));
        assert!(!config.trigger.enabled);
        assert!(!config.p37_shadow_probe.enabled);
        assert!(!config.rug_scalp_v2.enabled);
        assert!(config.rug_reality_capture.enabled);
        assert!(config.execution.events.enable_optional_events);
        assert!(!config.execution.events.enable_aem_ticks);
        assert!(config
            .rug_reality_capture
            .validate_enabled_contract()
            .is_ok());
        assert!(config
            .rug_reality_capture
            .validate_event_writer_contract(config.execution.events.enable_optional_events)
            .is_ok());
    }

    #[test]
    fn typed_rows_and_summary_serialize_bit_identically() {
        let work = feature_work(1);
        let mut row = calibration_excluded_row(&work, None);
        add_feature_values(&mut row, work.feature_result.expect("feature evidence"));
        let first = serde_json::to_vec(&row).expect("serialize row");
        let second = serde_json::to_vec(&row).expect("serialize row");
        assert_eq!(first, second);
    }

    #[test]
    fn same_tape_manifest_and_calibration_produce_bit_identical_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let events_dir = temp.path().join("events");
        fs::create_dir_all(&events_dir).expect("events dir");
        let manifest_path = temp.path().join("manifest.json");
        let run_id = "deterministic-day1";
        let mut capture_manifest = manifest(run_id);
        capture_manifest.health_evidence_path = temp
            .path()
            .join("health-evidence.json")
            .to_string_lossy()
            .into_owned();
        write_json_new(&manifest_path, &capture_manifest).expect("manifest");
        write_valid_health_evidence(&manifest_path, &capture_manifest);
        let event_path = events_dir.join("exec_deterministic-day1_0000.jsonl");
        let mut event_file = File::create(&event_path).expect("event file");
        for seed in 0..251u64 {
            let mut birth = test_birth(
                &format!("candidate-{seed}"),
                1_000_000 + seed.saturating_mul(100_000),
            );
            let mint = format!("mint-{seed}");
            let curve = format!("curve-{seed}");
            birth.payload.base_mint = mint.clone();
            birth.payload.mint_id = mint.clone();
            birth.payload.pool_amm_id = curve.clone();
            birth.payload.pool_id = curve.clone();
            birth.payload.bonding_curve = curve.clone();
            let birth_event = ExecutionEvent::new(
                EventEnvelope::new(
                    run_id.to_string(),
                    Lane::Shadow,
                    birth.candidate_id.clone(),
                    birth.payload.birth_ts_ms,
                ),
                EventKind::NewPoolDetected(birth.payload.clone()),
            );
            serde_json::to_writer(&mut event_file, &birth_event).expect("birth JSON");
            event_file.write_all(b"\n").expect("birth newline");
            for mut trade in feature_trades(birth.payload.birth_ts_ms, seed) {
                trade.payload.base_mint = Some(mint.clone());
                trade.payload.mint_id = Some(mint.clone());
                trade.payload.token_mint = Some(mint.clone());
                trade.payload.pool_amm_id = curve.clone();
                trade.payload.pool_id = curve.clone();
                trade.payload.bonding_curve = curve.clone();
                let trade_event = ExecutionEvent::new(
                    EventEnvelope::new(
                        run_id.to_string(),
                        Lane::Shadow,
                        birth.candidate_id.clone(),
                        trade.payload.event_ts_ms,
                    ),
                    EventKind::PoolTransaction(trade.payload),
                );
                serde_json::to_writer(&mut event_file, &trade_event).expect("trade JSON");
                event_file.write_all(b"\n").expect("trade newline");
            }
        }
        event_file.sync_all().expect("sync tape");

        let output_one = temp.path().join("output-one");
        let output_two = temp.path().join("output-two");
        let first = run_ace_core_one_day_probe(AceCoreOneDayProbeArgs {
            events_dir: events_dir.clone(),
            manifest_path: manifest_path.clone(),
            output_dir: output_one.clone(),
            day_id: AceCoreProbeDayId::Day1,
            calibration_path: None,
        })
        .expect("first probe");
        let second = run_ace_core_one_day_probe(AceCoreOneDayProbeArgs {
            events_dir,
            manifest_path,
            output_dir: output_two.clone(),
            day_id: AceCoreProbeDayId::Day1,
            calibration_path: None,
        })
        .expect("second probe");
        assert_eq!(first.terminal_status, second.terminal_status);
        for file in [CANDIDATE_ROWS_FILE, CALIBRATION_FILE, SUMMARY_FILE] {
            assert_eq!(
                fs::read(output_one.join(file)).expect("first artifact"),
                fs::read(output_two.join(file)).expect("second artifact"),
                "artifact differs: {file}"
            );
        }
    }

    #[test]
    fn execution_event_fixture_uses_expected_shadow_envelope_shape() {
        let envelope =
            EventEnvelope::new("run".to_string(), Lane::Shadow, "candidate".to_string(), 1);
        let event = ExecutionEvent::new(
            envelope,
            EventKind::NewPoolDetected(test_birth("c", 1).payload),
        );
        assert_eq!(event.envelope.lane, Lane::Shadow);
    }
}
