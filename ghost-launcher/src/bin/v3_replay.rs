use anyhow::{anyhow, Context, Result};
use ghost_brain::config::GatekeeperV3Config;
use ghost_core::checkpoint::MaterializedFeatureSet;
use ghost_launcher::components::gatekeeper_v3::{
    evaluate_v3_from_features, v3_feature_snapshot_hash,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

const SUPPORTED_REPLAY_PAYLOAD_SCHEMA_VERSION: u64 = 1;
const FLOAT_TOLERANCE: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum RowReplayStatus {
    FullReplayOk,
    HashOnly,
    PayloadAbsent,
    PayloadSchemaUnsupported,
    PayloadDeserializeFailed,
    MaterializationVersionAbsent,
    PayloadHashMismatch,
    PolicyPayloadAbsent,
    PolicyDeserializeFailed,
    PolicyHashMismatch,
    VerdictMismatch,
    StageMismatch,
    ReasonMismatch,
    ScoreMismatch,
}

impl RowReplayStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::FullReplayOk => "full_replay_ok",
            Self::HashOnly => "hash_only",
            Self::PayloadAbsent => "payload_absent",
            Self::PayloadSchemaUnsupported => "payload_schema_unsupported",
            Self::PayloadDeserializeFailed => "payload_deserialize_failed",
            Self::MaterializationVersionAbsent => "materialization_version_absent",
            Self::PayloadHashMismatch => "payload_hash_mismatch",
            Self::PolicyPayloadAbsent => "policy_payload_absent",
            Self::PolicyDeserializeFailed => "policy_deserialize_failed",
            Self::PolicyHashMismatch => "policy_hash_mismatch",
            Self::VerdictMismatch => "verdict_mismatch",
            Self::StageMismatch => "stage_mismatch",
            Self::ReasonMismatch => "reason_mismatch",
            Self::ScoreMismatch => "score_mismatch",
        }
    }
}

#[derive(Debug, Serialize)]
struct RowReplayResult {
    line_number: usize,
    ab_record_id: Option<String>,
    status: RowReplayStatus,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReplayReport {
    status: String,
    replay_status: String,
    input: String,
    total_rows: usize,
    bad_rows: usize,
    v3_rows: usize,
    status_counts: BTreeMap<String, usize>,
    row_results: Vec<RowReplayResult>,
}

#[derive(Debug)]
struct Args {
    input: PathBuf,
    json: bool,
    strict: bool,
}

fn parse_args() -> Result<Args> {
    let mut input = None;
    let mut json = false;
    let mut strict = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--input requires a path"))?;
                input = Some(PathBuf::from(value));
            }
            "--json" => json = true,
            "--strict" => strict = true,
            "--help" | "-h" => {
                println!(
                    "Usage: v3_replay --input <decisions.jsonl> [--json] [--strict]\n\
                     Validates V3 full replay payloads fail-closed."
                );
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        input: input.ok_or_else(|| anyhow!("--input is required"))?,
        json,
        strict,
    })
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let report = build_report(&args.input)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("status={}", report.status);
        println!("replay_status={}", report.replay_status);
        println!("v3_rows={}", report.v3_rows);
        println!("status_counts={:?}", report.status_counts);
    }

    if args.strict && (report.status != "ok" || report.replay_status != "full_replay_ok") {
        std::process::exit(2);
    }
    Ok(())
}

fn build_report(input: &PathBuf) -> Result<ReplayReport> {
    let file = File::open(input).with_context(|| format!("failed to open {}", input.display()))?;
    let reader = BufReader::new(file);
    let mut total_rows = 0;
    let mut bad_rows = 0;
    let mut results = Vec::new();

    for (idx, line) in reader.lines().enumerate() {
        let line_number = idx + 1;
        let line = line.with_context(|| format!("failed to read line {line_number}"))?;
        if line.trim().is_empty() {
            continue;
        }
        total_rows += 1;
        let row: Value = match serde_json::from_str(&line) {
            Ok(row) => row,
            Err(err) => {
                bad_rows += 1;
                results.push(RowReplayResult {
                    line_number,
                    ab_record_id: None,
                    status: RowReplayStatus::PayloadDeserializeFailed,
                    detail: Some(format!("invalid json row: {err}")),
                });
                continue;
            }
        };
        if !has_v3_fields(&row) {
            continue;
        }
        results.push(validate_v3_row(line_number, &row));
    }

    let mut status_counts = BTreeMap::new();
    for result in &results {
        *status_counts
            .entry(result.status.as_str().to_string())
            .or_insert(0) += 1;
    }
    let replay_status = replay_status(&results);
    let status = if results
        .iter()
        .any(|result| is_invalid_status(result.status))
        || bad_rows > 0
    {
        "fail_closed"
    } else {
        "ok"
    };

    Ok(ReplayReport {
        status: status.to_string(),
        replay_status,
        input: input.display().to_string(),
        total_rows,
        bad_rows,
        v3_rows: results.len(),
        status_counts,
        row_results: results,
    })
}

fn replay_status(results: &[RowReplayResult]) -> String {
    if results.is_empty() {
        return "no_v3_rows".to_string();
    }
    let full = results
        .iter()
        .filter(|result| result.status == RowReplayStatus::FullReplayOk)
        .count();
    if full == results.len() {
        return "full_replay_ok".to_string();
    }
    if results
        .iter()
        .any(|result| is_invalid_status(result.status))
    {
        return "fail_closed".to_string();
    }
    if results
        .iter()
        .all(|result| result.status == RowReplayStatus::HashOnly)
    {
        return "hash_only".to_string();
    }
    if results
        .iter()
        .all(|result| result.status == RowReplayStatus::PayloadAbsent)
    {
        return "payload_absent".to_string();
    }
    "mixed_non_replay".to_string()
}

fn validate_v3_row(line_number: usize, row: &Value) -> RowReplayResult {
    let ab_record_id = string_field(row, "ab_record_id");
    match validate_v3_row_status(row) {
        Ok(status) => RowReplayResult {
            line_number,
            ab_record_id,
            status,
            detail: None,
        },
        Err((status, detail)) => RowReplayResult {
            line_number,
            ab_record_id,
            status,
            detail: Some(detail),
        },
    }
}

fn validate_v3_row_status(
    row: &Value,
) -> std::result::Result<RowReplayStatus, (RowReplayStatus, String)> {
    let snapshot_payload = row.get("v3_materialized_feature_snapshot");
    if snapshot_payload.is_none() {
        if string_field(row, "v3_feature_snapshot_hash").is_some() {
            return Ok(RowReplayStatus::HashOnly);
        }
        return Ok(RowReplayStatus::PayloadAbsent);
    }

    let schema = row
        .get("v3_replay_payload_schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            (
                RowReplayStatus::PayloadSchemaUnsupported,
                "missing v3_replay_payload_schema_version".to_string(),
            )
        })?;
    if schema != SUPPORTED_REPLAY_PAYLOAD_SCHEMA_VERSION {
        return Err((
            RowReplayStatus::PayloadSchemaUnsupported,
            format!("unsupported schema version {schema}"),
        ));
    }

    let materialization_version = row
        .get("v3_materialization_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            (
                RowReplayStatus::MaterializationVersionAbsent,
                "missing v3_materialization_version".to_string(),
            )
        })? as u32;

    let features: MaterializedFeatureSet =
        serde_json::from_value(snapshot_payload.cloned().unwrap()).map_err(|err| {
            (
                RowReplayStatus::PayloadDeserializeFailed,
                format!("MaterializedFeatureSet deserialize failed: {err}"),
            )
        })?;

    let expected_snapshot_hash =
        string_field(row, "v3_feature_snapshot_hash").ok_or_else(|| {
            (
                RowReplayStatus::PayloadHashMismatch,
                "missing v3_feature_snapshot_hash".to_string(),
            )
        })?;
    let actual_snapshot_hash = v3_feature_snapshot_hash(&features, materialization_version);
    if actual_snapshot_hash != expected_snapshot_hash {
        return Err((
            RowReplayStatus::PayloadHashMismatch,
            format!("expected {expected_snapshot_hash}, recomputed {actual_snapshot_hash}"),
        ));
    }

    let policy_payload = row.get("v3_policy_config_payload").ok_or_else(|| {
        (
            RowReplayStatus::PolicyPayloadAbsent,
            "missing v3_policy_config_payload".to_string(),
        )
    })?;
    let expected_policy_hash = string_field(row, "v3_policy_config_hash").ok_or_else(|| {
        (
            RowReplayStatus::PolicyHashMismatch,
            "missing v3_policy_config_hash".to_string(),
        )
    })?;
    let policy_bytes = serde_json::to_vec(policy_payload).map_err(|err| {
        (
            RowReplayStatus::PolicyDeserializeFailed,
            format!("policy payload serialization failed: {err}"),
        )
    })?;
    let actual_policy_hash = blake3::hash(&policy_bytes).to_hex().to_string();
    if actual_policy_hash != expected_policy_hash {
        return Err((
            RowReplayStatus::PolicyHashMismatch,
            format!("expected {expected_policy_hash}, recomputed {actual_policy_hash}"),
        ));
    }

    let config: GatekeeperV3Config =
        serde_json::from_value(policy_payload.clone()).map_err(|err| {
            (
                RowReplayStatus::PolicyDeserializeFailed,
                format!("GatekeeperV3Config deserialize failed: {err}"),
            )
        })?;

    let deadline_elapsed = row
        .get("v3_shadow_notes")
        .and_then(|notes| notes.get("deadline_elapsed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let decision = evaluate_v3_from_features(&features, &config, deadline_elapsed);

    if string_field(row, "v3_shadow_verdict").as_deref() != Some(decision.verdict.as_log_str()) {
        return Err((
            RowReplayStatus::VerdictMismatch,
            format!(
                "expected {:?}, replayed {}",
                string_field(row, "v3_shadow_verdict"),
                decision.verdict.as_log_str()
            ),
        ));
    }
    if string_field(row, "v3_shadow_stage").as_deref() != Some(decision.stage.as_log_str()) {
        return Err((
            RowReplayStatus::StageMismatch,
            format!(
                "expected {:?}, replayed {}",
                string_field(row, "v3_shadow_stage"),
                decision.stage.as_log_str()
            ),
        ));
    }
    let replayed_reason_code = decision.reason_code.as_log_str();
    if string_field(row, "v3_shadow_reason_code").as_deref() != Some(replayed_reason_code.as_str())
    {
        return Err((
            RowReplayStatus::ReasonMismatch,
            format!(
                "expected {:?}, replayed {}",
                string_field(row, "v3_shadow_reason_code"),
                replayed_reason_code
            ),
        ));
    }
    compare_f64(
        row,
        "v3_shadow_risk_penalty",
        decision.risk_penalty,
        RowReplayStatus::ScoreMismatch,
    )?;
    compare_f64(
        row,
        "v3_shadow_opportunity_score",
        decision.opportunity_score,
        RowReplayStatus::ScoreMismatch,
    )?;
    compare_f64(
        row,
        "v3_shadow_confidence",
        decision.confidence,
        RowReplayStatus::ScoreMismatch,
    )?;

    Ok(RowReplayStatus::FullReplayOk)
}

fn compare_f64(
    row: &Value,
    field: &'static str,
    actual: f64,
    status: RowReplayStatus,
) -> std::result::Result<(), (RowReplayStatus, String)> {
    let Some(expected) = row.get(field).and_then(Value::as_f64) else {
        return Ok(());
    };
    if (expected - actual).abs() > FLOAT_TOLERANCE {
        return Err((
            status,
            format!("{field} expected {expected}, replayed {actual}"),
        ));
    }
    Ok(())
}

fn has_v3_fields(row: &Value) -> bool {
    row.get("v3_shadow_schema_version").is_some()
        || row.get("v3_shadow_verdict").is_some()
        || row.get("v3_policy_config_hash").is_some()
        || row.get("v3_feature_snapshot_hash").is_some()
        || row.get("v3_materialized_feature_snapshot").is_some()
}

fn string_field(row: &Value, field: &str) -> Option<String> {
    row.get(field)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
}

fn is_invalid_status(status: RowReplayStatus) -> bool {
    !matches!(
        status,
        RowReplayStatus::FullReplayOk | RowReplayStatus::HashOnly | RowReplayStatus::PayloadAbsent
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_launcher::components::gatekeeper_v3::v3_feature_snapshot_hash;
    use serde_json::json;

    fn policy_payload_and_hash() -> (Value, String) {
        let mut config = GatekeeperV3Config::default();
        config.shadow_emit_enabled = true;
        let payload = config.v3_policy_config_payload();
        let bytes = serde_json::to_vec(&payload).unwrap();
        let hash = blake3::hash(&bytes).to_hex().to_string();
        (payload, hash)
    }

    fn full_row() -> Value {
        let features = MaterializedFeatureSet::default();
        let materialization_version = 1;
        let (policy_payload, policy_hash) = policy_payload_and_hash();
        let config: GatekeeperV3Config = serde_json::from_value(policy_payload.clone()).unwrap();
        let decision = evaluate_v3_from_features(&features, &config, false);
        json!({
            "ab_record_id": "ab-1",
            "v3_shadow_schema_version": 1,
            "v3_shadow_verdict": decision.verdict.as_log_str(),
            "v3_shadow_stage": decision.stage.as_log_str(),
            "v3_shadow_reason_code": decision.reason_code.as_log_str(),
            "v3_shadow_risk_penalty": decision.risk_penalty,
            "v3_shadow_opportunity_score": decision.opportunity_score,
            "v3_shadow_confidence": decision.confidence,
            "v3_replay_payload_schema_version": 1,
            "v3_materialized_feature_snapshot": serde_json::to_value(&features).unwrap(),
            "v3_materialization_version": materialization_version,
            "v3_feature_snapshot_hash": v3_feature_snapshot_hash(&features, materialization_version),
            "v3_policy_config_payload": policy_payload,
            "v3_policy_config_hash": policy_hash,
            "v3_shadow_notes": {"deadline_elapsed": false}
        })
    }

    #[test]
    fn validates_full_replay_ok() {
        assert_eq!(
            validate_v3_row_status(&full_row()).unwrap(),
            RowReplayStatus::FullReplayOk
        );
    }

    #[test]
    fn distinguishes_hash_only_and_payload_absent() {
        assert_eq!(
            validate_v3_row_status(&json!({"v3_feature_snapshot_hash": "hash"})).unwrap(),
            RowReplayStatus::HashOnly
        );
        assert_eq!(
            validate_v3_row_status(&json!({"v3_shadow_verdict": "PENDING"})).unwrap(),
            RowReplayStatus::PayloadAbsent
        );
    }

    #[test]
    fn rejects_unsupported_schema() {
        let mut row = full_row();
        row["v3_replay_payload_schema_version"] = json!(999);
        let err = validate_v3_row_status(&row).unwrap_err();
        assert_eq!(err.0, RowReplayStatus::PayloadSchemaUnsupported);
    }

    #[test]
    fn rejects_payload_hash_mismatch() {
        let mut row = full_row();
        row["v3_feature_snapshot_hash"] = json!("wrong");
        let err = validate_v3_row_status(&row).unwrap_err();
        assert_eq!(err.0, RowReplayStatus::PayloadHashMismatch);
    }

    #[test]
    fn rejects_stage_mismatch() {
        let mut row = full_row();
        row["v3_shadow_stage"] = json!("wrong_stage");
        let err = validate_v3_row_status(&row).unwrap_err();
        assert_eq!(err.0, RowReplayStatus::StageMismatch);
    }

    #[test]
    fn rejects_policy_hash_mismatch() {
        let mut row = full_row();
        row["v3_policy_config_hash"] = json!("wrong");
        let err = validate_v3_row_status(&row).unwrap_err();
        assert_eq!(err.0, RowReplayStatus::PolicyHashMismatch);
    }
}
