use super::{replay_metric_contract_record_v2, Pr2cCounterfactualLaneStatusV1, Pr2cReplayInputV2};
use ghost_brain::oracle::{
    GatekeeperBuyLog, MetricContractRotatedPartManifestV1, MetricContractRotationManifestV1,
    GATEKEEPER_BUY_LOG_SCHEMA_VERSION, METRIC_CONTRACT_EVIDENCE_V1_FILE,
    METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE, METRIC_CONTRACT_SUMMARY_V34_FILE,
};
use ghost_core::checkpoint::MaterializedFeatureSet;
use ghost_core::metric_contracts::{
    BurnInContractV1, CanonicalNullableV1, MetricAvailabilityV1,
    MetricContractAuditTerminalClassV1, MetricContractCutoverScopeV1,
    MetricContractDecisionSummaryV1, MetricContractEvidenceTransportV1,
    MetricEvidenceRecordIdentityV1, MetricMeasurementQualityV1, StableEventIdentityV1,
    BURN_IN_CONTRACT_V3_CANONICAL_HASH, BURN_IN_CONTRACT_VERSION_V3,
    METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1,
    METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1,
    METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34, METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
    METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1,
    METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3, PR2C_COMPARATOR_P99_MAX_US,
    PR2C_FULL_BUILD_AND_SERIALIZE_P99_MAX_US, PR2C_LOGGER_ENQUEUE_WAIT_P99_MAX_US,
    PR2C_PROJECTION_BUILD_AND_VALIDATE_P99_MAX_US,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pr2cSingleRunAuditReportV1 {
    pub terminal_class: MetricContractAuditTerminalClassV1,
    pub cutover_scope: MetricContractCutoverScopeV1,
    pub run_id: Option<String>,
    pub run_start_ms: Option<u64>,
    pub run_end_ms: Option<u64>,
    pub summary_rows: usize,
    pub evidence_rows: usize,
    pub replayed_rows: usize,
    pub duplicate_record_identities: usize,
    pub missing_pairs: usize,
    pub policy_drift_rows: usize,
    pub comparator_not_evaluable_rows: usize,
    pub counterfactual_policy_delta_observed_rows: usize,
    pub counterfactual_not_evaluable_rows: usize,
    pub stable_identity_unavailable_rows: usize,
    pub dev_known_decisions: usize,
    pub clean_flip_v2_evaluable: usize,
    pub real_dev_legacy_v2_divergences: usize,
    pub projection_wire_p95_bytes: usize,
    pub projection_wire_max_bytes: usize,
    pub sidecar_p95_bytes: usize,
    pub sidecar_p99_bytes: usize,
    pub comparator_p99_us: u32,
    pub metric_contract_serialize_p99_us: u32,
    pub metric_contract_build_and_serialize_p99_us: u64,
    pub projection_build_and_validate_p99_us: u64,
    pub logger_enqueue_wait_p99_us: u64,
    pub v34_p95_increase_bytes: i64,
    pub v34_p95_increase_ratio: f64,
    pub combined_bytes_delta_ratio: f64,
    pub writer_queue_high_water: u64,
    pub paired_record_identities: Vec<MetricEvidenceRecordIdentityV1>,
    pub paired_decision_timestamps_ms: Vec<u64>,
    pub utc_4h_buckets: Vec<u64>,
    pub counterfactual_diagnostics: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pr2cBundleAuditReportV1 {
    pub terminal_class: MetricContractAuditTerminalClassV1,
    pub cutover_scope: MetricContractCutoverScopeV1,
    pub run_reports: Vec<Pr2cSingleRunAuditReportV1>,
    pub non_overlapping_runs: bool,
    pub consistent_provenance: bool,
    pub stable_event_collisions: usize,
    pub stable_identity_collision_gate_evaluable: bool,
    pub unique_run_ids: bool,
    pub global_duplicate_record_identities: usize,
    pub reasons: Vec<String>,
}

#[derive(Debug, Error)]
pub enum Pr2cAuditErrorV1 {
    #[error("read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: String,
        source: serde_json::Error,
    },
    #[error("manifest part integrity mismatch: {0}")]
    PartIntegrity(String),
    #[error("missing decision-time MFS projection for record {0:?}")]
    MissingDecisionProjection(MetricEvidenceRecordIdentityV1),
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, Pr2cAuditErrorV1> {
    fs::read(path).map_err(|source| Pr2cAuditErrorV1::Read {
        path: path.display().to_string(),
        source,
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Pr2cAuditErrorV1> {
    serde_json::from_slice(&read_bytes(path)?).map_err(|source| Pr2cAuditErrorV1::Json {
        path: path.display().to_string(),
        source,
    })
}

fn read_jsonl<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<(Vec<T>, Vec<usize>), Pr2cAuditErrorV1> {
    let bytes = read_bytes(path)?;
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(Pr2cAuditErrorV1::PartIntegrity(format!(
            "truncated JSONL {}",
            path.display()
        )));
    }
    let mut rows = Vec::new();
    let mut sizes = Vec::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        sizes.push(line.len());
        rows.push(
            serde_json::from_slice(line).map_err(|source| Pr2cAuditErrorV1::Json {
                path: path.display().to_string(),
                source,
            })?,
        );
    }
    Ok((rows, sizes))
}

fn percentile<T: Copy + Ord>(values: &[T], percentile: usize) -> Option<T> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    let rank = ((values.len() - 1) * percentile).div_ceil(100);
    values.get(rank).copied()
}

fn resolve_manifest_part_path(
    run_dir: &Path,
    file_path: &str,
) -> Result<PathBuf, Pr2cAuditErrorV1> {
    let path = Path::new(file_path);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(Pr2cAuditErrorV1::PartIntegrity(format!(
            "manifest part path must be one relative file name: {file_path}"
        )));
    }
    Ok(run_dir.join(path))
}

fn valid_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_build_commit(value: &str) -> bool {
    valid_lower_hex(value, 40)
}

fn validate_part_rows(
    path: &Path,
    part: &MetricContractRotatedPartManifestV1,
) -> Result<(), Pr2cAuditErrorV1> {
    let identities = match part.stream.as_str() {
        "decision_v34" => read_jsonl::<MetricContractDecisionSummaryV1>(path)?
            .0
            .into_iter()
            .map(|row| row.evidence_record_id)
            .collect::<Vec<_>>(),
        "full_evidence_v1" => read_jsonl::<MetricContractEvidenceTransportV1>(path)?
            .0
            .into_iter()
            .map(|row| {
                if row.rotation_part_index != part.part_index || row.writer_timestamp_ms == 0 {
                    return Err(Pr2cAuditErrorV1::PartIntegrity(format!(
                        "evidence writer metadata mismatch in {}",
                        path.display()
                    )));
                }
                Ok(row.payload.record_identity)
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(Pr2cAuditErrorV1::PartIntegrity(format!(
                "unknown manifest stream {}",
                part.stream
            )))
        }
    };
    if identities.len() as u64 != part.row_count
        || identities.first() != part.first_record_identity.as_ref()
        || identities.last() != part.last_record_identity.as_ref()
        || identities
            .iter()
            .any(|identity| identity.run_id != part.run_id)
    {
        return Err(Pr2cAuditErrorV1::PartIntegrity(format!(
            "first/last/run identity metadata mismatch in {}",
            path.display()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
struct FrozenRunProvenanceV1 {
    run_id: String,
    build_commit: String,
    build_worktree_clean: bool,
    gatekeeper_config_hash: String,
    brain_config_hash: String,
    rollout_mode: ghost_core::metric_contracts::MetricContractRolloutMode,
    metric_contract_schema_version: u16,
    projection_wire_version: u16,
    evidence_schema_version: u16,
    decision_schema_version: u32,
    wire_schema_manifest_blake3: String,
    burn_in_contract_version: u16,
    burn_in_contract_canonical_hash: ghost_core::metric_contracts::CanonicalHashV1,
    profile_id: String,
    profile_hash: ghost_core::metric_contracts::CanonicalHashV1,
    metric_contract_effective_config_hash: ghost_core::metric_contracts::CanonicalHashV1,
    effective_config: ghost_core::metric_contracts::ResolvedMetricContractEffectiveConfigV1,
}

impl From<&MetricContractRotatedPartManifestV1> for FrozenRunProvenanceV1 {
    fn from(part: &MetricContractRotatedPartManifestV1) -> Self {
        Self {
            run_id: part.run_id.clone(),
            build_commit: part.build_commit.clone(),
            build_worktree_clean: part.build_worktree_clean,
            gatekeeper_config_hash: part.gatekeeper_config_hash.clone(),
            brain_config_hash: part.brain_config_hash.clone(),
            rollout_mode: part.rollout_mode,
            metric_contract_schema_version: part.metric_contract_schema_version,
            projection_wire_version: part.projection_wire_version,
            evidence_schema_version: part.evidence_schema_version,
            decision_schema_version: part.decision_schema_version,
            wire_schema_manifest_blake3: part.wire_schema_manifest_blake3.clone(),
            burn_in_contract_version: part.burn_in_contract_version,
            burn_in_contract_canonical_hash: part.burn_in_contract_canonical_hash.clone(),
            profile_id: part.profile_id.clone(),
            profile_hash: part.profile_hash.clone(),
            metric_contract_effective_config_hash: part
                .metric_contract_effective_config_hash
                .clone(),
            effective_config: part.effective_config.clone(),
        }
    }
}

fn validate_manifest_parts(
    run_dir: &Path,
    manifest: &MetricContractRotationManifestV1,
) -> Result<(), Pr2cAuditErrorV1> {
    if manifest.manifest_schema_version != 1
        || !manifest.writer_finalized
        || manifest.summary_parts.is_empty()
        || manifest.summary_parts.len() != manifest.evidence_parts.len()
    {
        return Err(Pr2cAuditErrorV1::PartIntegrity(
            "unsupported or incomplete rotation manifest".to_string(),
        ));
    }
    let frozen_run_provenance = manifest
        .summary_parts
        .first()
        .map(FrozenRunProvenanceV1::from)
        .ok_or_else(|| {
            Pr2cAuditErrorV1::PartIntegrity(
                "rotation manifest has no provenance anchor".to_string(),
            )
        })?;
    for (summary, evidence) in manifest.summary_parts.iter().zip(&manifest.evidence_parts) {
        if summary.stream != "decision_v34"
            || summary.schema != "metric_contract_decision_v34"
            || evidence.stream != "full_evidence_v1"
            || evidence.schema != "metric_contract_evidence_v1"
            || summary.part_index != evidence.part_index
            || summary.row_count != evidence.row_count
            || summary.first_record_identity != evidence.first_record_identity
            || summary.last_record_identity != evidence.last_record_identity
            || summary.run_id != evidence.run_id
            || summary.build_commit != evidence.build_commit
            || summary.build_worktree_clean != evidence.build_worktree_clean
            || summary.gatekeeper_config_hash != evidence.gatekeeper_config_hash
            || summary.brain_config_hash != evidence.brain_config_hash
            || summary.rollout_mode != evidence.rollout_mode
            || summary.metric_contract_schema_version != evidence.metric_contract_schema_version
            || summary.projection_wire_version != evidence.projection_wire_version
            || summary.evidence_schema_version != evidence.evidence_schema_version
            || summary.decision_schema_version != evidence.decision_schema_version
            || summary.wire_schema_manifest_blake3 != evidence.wire_schema_manifest_blake3
            || summary.burn_in_contract_version != evidence.burn_in_contract_version
            || summary.burn_in_contract_canonical_hash != evidence.burn_in_contract_canonical_hash
            || summary.profile_id != evidence.profile_id
            || summary.profile_hash != evidence.profile_hash
            || summary.metric_contract_effective_config_hash
                != evidence.metric_contract_effective_config_hash
            || summary.effective_config != evidence.effective_config
        {
            return Err(Pr2cAuditErrorV1::PartIntegrity(
                "summary/evidence rotated-part provenance mismatch".to_string(),
            ));
        }
        if FrozenRunProvenanceV1::from(summary) != frozen_run_provenance
            || FrozenRunProvenanceV1::from(evidence) != frozen_run_provenance
        {
            return Err(Pr2cAuditErrorV1::PartIntegrity(
                "rotated parts do not share one frozen run provenance".to_string(),
            ));
        }
        if !valid_build_commit(&summary.build_commit)
            || !summary.build_worktree_clean
            || !valid_lower_hex(&summary.gatekeeper_config_hash, 64)
            || !valid_lower_hex(&summary.brain_config_hash, 64)
            || summary.metric_contract_schema_version
                != METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1
            || summary.projection_wire_version
                != METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1
            || summary.evidence_schema_version != METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1
            || summary.decision_schema_version != METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34
            || summary.wire_schema_manifest_blake3
                != METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3
            || summary.burn_in_contract_version != BURN_IN_CONTRACT_VERSION_V3
            || summary.burn_in_contract_canonical_hash.as_str()
                != BURN_IN_CONTRACT_V3_CANONICAL_HASH
        {
            return Err(Pr2cAuditErrorV1::PartIntegrity(
                "unknown, dirty, or incomplete run/build/schema/BURN provenance".to_string(),
            ));
        }
    }
    if manifest.writer_stats.summary_rows_written_total
        != manifest
            .summary_parts
            .iter()
            .map(|part| part.row_count)
            .sum::<u64>()
        || manifest.writer_stats.evidence_rows_written_total
            != manifest
                .evidence_parts
                .iter()
                .map(|part| part.row_count)
                .sum::<u64>()
    {
        return Err(Pr2cAuditErrorV1::PartIntegrity(
            "writer counters disagree with rotated-part row counts".to_string(),
        ));
    }
    let expected_samples = manifest.writer_stats.paired_commands_total;
    for (name, histogram) in [
        (
            "logger_enqueue_wait_us",
            &manifest.writer_stats.logger_enqueue_wait_us,
        ),
        (
            "metric_contract_build_and_serialize_us",
            &manifest.writer_stats.metric_contract_build_and_serialize_us,
        ),
        (
            "projection_build_and_validate_us",
            &manifest.writer_stats.projection_build_and_validate_us,
        ),
    ] {
        histogram.validate(expected_samples).map_err(|error| {
            Pr2cAuditErrorV1::PartIntegrity(format!(
                "invalid or incomplete {name} histogram: {error}"
            ))
        })?;
    }
    let mut declared_file_names = BTreeSet::new();
    for parts in [&manifest.summary_parts, &manifest.evidence_parts] {
        for (expected_index, part) in parts.iter().enumerate() {
            if usize::try_from(part.part_index).ok() != Some(expected_index) {
                return Err(Pr2cAuditErrorV1::PartIntegrity(
                    "non-contiguous part numbering".to_string(),
                ));
            }
            if !declared_file_names.insert(part.file_path.clone()) {
                return Err(Pr2cAuditErrorV1::PartIntegrity(
                    "one rotated part is declared more than once".to_string(),
                ));
            }
            let path = resolve_manifest_part_path(run_dir, &part.file_path)?;
            let bytes = read_bytes(&path)?;
            if bytes.len() as u64 != part.byte_count
                || bytes.iter().filter(|byte| **byte == b'\n').count() as u64 != part.row_count
                || format!("{:x}", Sha256::digest(&bytes)) != part.part_sha256.as_str()
            {
                return Err(Pr2cAuditErrorV1::PartIntegrity(path.display().to_string()));
            }
            validate_part_rows(&path, part)?;
        }
    }
    let directory = fs::read_dir(run_dir).map_err(|source| Pr2cAuditErrorV1::Read {
        path: run_dir.display().to_string(),
        source,
    })?;
    for entry in directory {
        let entry = entry.map_err(|source| Pr2cAuditErrorV1::Read {
            path: run_dir.display().to_string(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_metric_contract_part = name == METRIC_CONTRACT_SUMMARY_V34_FILE
            || name == METRIC_CONTRACT_EVIDENCE_V1_FILE
            || (name.starts_with("metric_contract_decisions_v34.part-")
                && name.ends_with(".jsonl"))
            || (name.starts_with("metric_contract_evidence_v1.part-") && name.ends_with(".jsonl"));
        if is_metric_contract_part && !declared_file_names.contains(&name) {
            return Err(Pr2cAuditErrorV1::PartIntegrity(format!(
                "undeclared rotated part {name}"
            )));
        }
    }
    Ok(())
}

fn load_decision_projections(
    decision_v33_paths: &[PathBuf],
) -> Result<BTreeMap<MetricEvidenceRecordIdentityV1, CurrentDecisionBaselineV33>, Pr2cAuditErrorV1>
{
    let mut projections = BTreeMap::new();
    for path in decision_v33_paths {
        let (rows, sizes) = read_jsonl::<serde_json::Value>(path)?;
        for (row, serialized_size) in rows.into_iter().zip(sizes) {
            let typed: GatekeeperBuyLog =
                serde_json::from_value(row.clone()).map_err(|source| Pr2cAuditErrorV1::Json {
                    path: path.display().to_string(),
                    source,
                })?;
            if typed.log_schema_version != GATEKEEPER_BUY_LOG_SCHEMA_VERSION
                || serde_json::to_value(&typed).map_err(|source| Pr2cAuditErrorV1::Json {
                    path: path.display().to_string(),
                    source,
                })? != row
            {
                return Err(Pr2cAuditErrorV1::PartIntegrity(format!(
                    "decision baseline is not an exact current GatekeeperBuyLog v33 row: {}",
                    path.display()
                )));
            }
            let run_id = typed.run_id.as_deref().ok_or_else(|| {
                Pr2cAuditErrorV1::PartIntegrity("current v33 row lacks run_id".to_string())
            })?;
            let join_key = typed.join_key.as_deref().ok_or_else(|| {
                Pr2cAuditErrorV1::PartIntegrity("current v33 row lacks join_key".to_string())
            })?;
            let decision_plane = typed.decision_plane.as_deref().ok_or_else(|| {
                Pr2cAuditErrorV1::PartIntegrity("current v33 row lacks decision_plane".to_string())
            })?;
            let snapshot = typed.materialized_feature_snapshot.clone().ok_or_else(|| {
                Pr2cAuditErrorV1::PartIntegrity(
                    "current v33 row lacks materialized_feature_snapshot".to_string(),
                )
            })?;
            let gatekeeper_config_hash = typed.config_hash.clone().ok_or_else(|| {
                Pr2cAuditErrorV1::PartIntegrity(
                    "current v33 row lacks gatekeeper config hash".to_string(),
                )
            })?;
            let brain_config_hash = typed.brain_config_hash.clone().ok_or_else(|| {
                Pr2cAuditErrorV1::PartIntegrity(
                    "current v33 row lacks brain config hash".to_string(),
                )
            })?;
            let identity =
                MetricEvidenceRecordIdentityV1::try_new(run_id, join_key, decision_plane).map_err(
                    |_| Pr2cAuditErrorV1::PartIntegrity("invalid v33 record identity".to_string()),
                )?;
            let mfs =
                serde_json::from_value(snapshot).map_err(|source| Pr2cAuditErrorV1::Json {
                    path: path.display().to_string(),
                    source,
                })?;
            if projections
                .insert(
                    identity,
                    CurrentDecisionBaselineV33 {
                        materialized_features: mfs,
                        serialized_size,
                        gatekeeper_config_hash,
                        brain_config_hash,
                    },
                )
                .is_some()
            {
                return Err(Pr2cAuditErrorV1::PartIntegrity(
                    "duplicate v33 record identity".to_string(),
                ));
            }
        }
    }
    Ok(projections)
}

#[derive(Debug)]
struct CurrentDecisionBaselineV33 {
    materialized_features: MaterializedFeatureSet,
    serialized_size: usize,
    gatekeeper_config_hash: String,
    brain_config_hash: String,
}

pub fn audit_pr2c_single_run_v1(
    run_dir: &Path,
    decision_v33_paths: &[PathBuf],
) -> Result<Pr2cSingleRunAuditReportV1, Pr2cAuditErrorV1> {
    let manifest_path = run_dir.join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let manifest: MetricContractRotationManifestV1 = read_json(&manifest_path)?;
    validate_manifest_parts(run_dir, &manifest)?;
    let mut summaries = Vec::new();
    let mut summary_sizes = Vec::new();
    for part in &manifest.summary_parts {
        let (mut rows, mut sizes) = read_jsonl::<MetricContractDecisionSummaryV1>(
            &resolve_manifest_part_path(run_dir, &part.file_path)?,
        )?;
        summaries.append(&mut rows);
        summary_sizes.append(&mut sizes);
    }
    let mut evidence_rows = Vec::new();
    let mut sidecar_sizes = Vec::new();
    for part in &manifest.evidence_parts {
        let (mut rows, mut sizes) = read_jsonl::<MetricContractEvidenceTransportV1>(
            &resolve_manifest_part_path(run_dir, &part.file_path)?,
        )?;
        evidence_rows.append(&mut rows);
        sidecar_sizes.append(&mut sizes);
    }
    let decision_projections = load_decision_projections(decision_v33_paths)?;
    let mut duplicate_record_identities = 0usize;
    let mut summaries_by_id = BTreeMap::new();
    for (summary, size) in summaries.into_iter().zip(summary_sizes) {
        if summaries_by_id
            .insert(summary.evidence_record_id.clone(), (summary, size))
            .is_some()
        {
            duplicate_record_identities += 1;
        }
    }
    let mut evidence_by_id = BTreeMap::new();
    for (evidence, size) in evidence_rows.into_iter().zip(sidecar_sizes) {
        if evidence_by_id
            .insert(evidence.payload.record_identity.clone(), (evidence, size))
            .is_some()
        {
            duplicate_record_identities += 1;
        }
    }
    let summary_ids = summaries_by_id.keys().cloned().collect::<BTreeSet<_>>();
    let evidence_ids = evidence_by_id.keys().cloned().collect::<BTreeSet<_>>();
    let v33_ids = decision_projections
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut all_ids = summary_ids.clone();
    all_ids.extend(evidence_ids.iter().cloned());
    all_ids.extend(v33_ids.iter().cloned());
    let paired_ids = summary_ids
        .intersection(&evidence_ids)
        .filter(|identity| v33_ids.contains(*identity))
        .cloned()
        .collect::<BTreeSet<_>>();
    let missing_pairs = all_ids.len().saturating_sub(paired_ids.len());
    let evidence_row_count = evidence_by_id.len();
    let mut replayed_rows = 0usize;
    let mut policy_drift_rows = 0usize;
    let mut comparator_not_evaluable_rows = 0usize;
    let mut counterfactual_policy_delta_observed_rows = 0usize;
    let mut counterfactual_not_evaluable_rows = 0usize;
    let mut stable_identity_unavailable_rows = 0usize;
    let mut dev_known_decisions = 0usize;
    let mut clean_flip_v2_evaluable = 0usize;
    let mut real_dev_legacy_v2_divergences = 0usize;
    let mut wire_sizes = Vec::new();
    let mut paired_sidecar_sizes = Vec::new();
    let mut comparator_times = Vec::new();
    let mut serialize_times = Vec::new();
    let mut v34_increase_bytes = Vec::new();
    let mut v34_increase_ratios = Vec::new();
    let mut reasons = Vec::new();
    let mut counterfactual_diagnostics = Vec::new();
    let mut paired_record_identities = Vec::new();
    let mut paired_decision_timestamps_ms = Vec::new();
    let mut paired_v33_total_bytes = 0u64;
    let mut paired_v34_total_bytes = 0u64;
    let mut paired_sidecar_total_bytes = 0u64;
    let effective_config = manifest
        .summary_parts
        .first()
        .map(|part| part.effective_config.clone());
    for identity in &paired_ids {
        let (summary, summary_size) = summaries_by_id
            .get(identity)
            .expect("paired identity exists in summary map");
        let (evidence, sidecar_size) = evidence_by_id
            .get(identity)
            .expect("paired identity exists in evidence map");
        let baseline = decision_projections
            .get(identity)
            .expect("paired identity exists in v33 map");
        let Some(part_provenance) = manifest.summary_parts.first() else {
            reasons.push("manifest lacks summary part provenance".to_string());
            continue;
        };
        if baseline.gatekeeper_config_hash != part_provenance.gatekeeper_config_hash
            || baseline.brain_config_hash != part_provenance.brain_config_hash
        {
            reasons.push(format!(
                "v33/run-manifest config provenance mismatch for {identity:?}"
            ));
            continue;
        }
        let Some(projection) = baseline
            .materialized_features
            .metric_contract_decision_projection_v1
            .clone()
        else {
            reasons.push(format!("missing decision-time projection for {identity:?}"));
            continue;
        };
        let Some(effective_config) = effective_config.clone() else {
            reasons.push("manifest lacks effective config".to_string());
            continue;
        };
        let wire_size = projection
            .authoritative_serialized_size_bytes()
            .map_err(|error| Pr2cAuditErrorV1::PartIntegrity(error.to_string()))?;
        match replay_metric_contract_record_v2(Pr2cReplayInputV2 {
            decision_v34: summary.clone(),
            evidence: evidence.clone(),
            decision_time_projection: projection,
            effective_config,
        }) {
            Err(error) => reasons.push(error.to_string()),
            Ok(replay) => {
                paired_v33_total_bytes =
                    paired_v33_total_bytes.saturating_add(baseline.serialized_size as u64);
                paired_v34_total_bytes =
                    paired_v34_total_bytes.saturating_add(*summary_size as u64);
                paired_sidecar_total_bytes =
                    paired_sidecar_total_bytes.saturating_add(*sidecar_size as u64);
                paired_sidecar_sizes.push(*sidecar_size);
                comparator_times.push(summary.comparator_elapsed_us);
                serialize_times.push(summary.metric_contract_serialize_us);
                wire_sizes.push(wire_size);
                if summary.equivalence_deltas.has_policy_drift() {
                    policy_drift_rows += 1;
                }
                if summary.equivalence_deltas.is_not_evaluable() {
                    comparator_not_evaluable_rows += 1;
                }
                if evidence.payload.stable_event_identity.is_null() {
                    stable_identity_unavailable_rows += 1;
                }
                let increase = i64::try_from(*summary_size).unwrap_or(i64::MAX);
                v34_increase_bytes.push(increase);
                v34_increase_ratios.push(*summary_size as f64 / baseline.serialized_size as f64);
                replayed_rows += 1;
                paired_record_identities.push(identity.clone());
                paired_decision_timestamps_ms
                    .push(evidence.payload.source_cutoff.decision_timestamp_ms.get());
                if evidence
                    .payload
                    .contracts
                    .dev_buy
                    .tx_intel_first_observed
                    .creator_known
                {
                    dev_known_decisions += 1;
                }
                let flip = &evidence.payload.contracts.flip_ratio.hybrid_v2;
                if flip.envelope.availability == MetricAvailabilityV1::Available
                    && flip.envelope.measurement_quality == MetricMeasurementQualityV1::Measured
                    && flip.eligible_buyer_count > 0
                {
                    clean_flip_v2_evaluable += 1;
                }
                let dev = &evidence.payload.contracts.dev_buy;
                if matches!(
                    (&dev.tx_intel_first_observed.amount_sol, &dev.mfs_primary_v1.amount_sol),
                    (CanonicalNullableV1::Value(legacy), CanonicalNullableV1::Value(v2))
                        if legacy.to_bits() != v2.to_bits()
                ) {
                    real_dev_legacy_v2_divergences += 1;
                }
                if replay.counterfactual_evaluation.any_not_evaluable() {
                    counterfactual_not_evaluable_rows += 1;
                }
                if replay.counterfactual_evaluation.delta_present() {
                    counterfactual_policy_delta_observed_rows += 1;
                }
                for (lane, status) in [
                    ("dev_primary", replay.counterfactual_evaluation.dev_primary),
                    (
                        "corrected_ftdi_actionability",
                        replay
                            .counterfactual_evaluation
                            .corrected_ftdi_actionability,
                    ),
                ] {
                    if status == Pr2cCounterfactualLaneStatusV1::Different {
                        counterfactual_diagnostics.push(format!(
                            "COUNTERFACTUAL_POLICY_DELTA_OBSERVED:{lane}:{}:{}:{}",
                            identity.run_id, identity.join_key, identity.decision_plane
                        ));
                    }
                }
            }
        }
    }
    paired_decision_timestamps_ms.sort_unstable();
    let run_start_ms = paired_decision_timestamps_ms.first().copied();
    let run_end_ms = paired_decision_timestamps_ms.last().copied();
    let utc_4h_buckets = paired_decision_timestamps_ms
        .iter()
        .map(|timestamp| timestamp / 14_400_000)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let projection_wire_p95_bytes = percentile(&wire_sizes, 95).unwrap_or_default();
    let projection_wire_max_bytes = wire_sizes.iter().copied().max().unwrap_or_default();
    let sidecar_p95_bytes = percentile(&paired_sidecar_sizes, 95).unwrap_or_default();
    let sidecar_p99_bytes = percentile(&paired_sidecar_sizes, 99).unwrap_or_default();
    let comparator_p99_us = percentile(&comparator_times, 99).unwrap_or_default();
    let metric_contract_serialize_p99_us = percentile(&serialize_times, 99).unwrap_or_default();
    let metric_contract_build_and_serialize_p99_us = manifest
        .writer_stats
        .metric_contract_build_and_serialize_us
        .percentile_upper_bound_us(99)
        .unwrap_or(u64::MAX);
    let projection_build_and_validate_p99_us = manifest
        .writer_stats
        .projection_build_and_validate_us
        .percentile_upper_bound_us(99)
        .unwrap_or(u64::MAX);
    let logger_enqueue_wait_p99_us = manifest
        .writer_stats
        .logger_enqueue_wait_us
        .percentile_upper_bound_us(99)
        .unwrap_or(u64::MAX);
    let v34_p95_increase_bytes = percentile(&v34_increase_bytes, 95).unwrap_or_default();
    v34_increase_ratios.sort_by(f64::total_cmp);
    let v34_p95_increase_ratio = if v34_increase_ratios.is_empty() {
        0.0
    } else {
        v34_increase_ratios[((v34_increase_ratios.len() - 1) * 95).div_ceil(100)]
    };
    let queue_high_water_ratio = if manifest.writer_queue_capacity == 0 {
        1.0
    } else {
        manifest.writer_stats.writer_queue_high_water as f64 / manifest.writer_queue_capacity as f64
    };
    let combined_bytes_delta_ratio = if paired_v33_total_bytes == 0 {
        f64::INFINITY
    } else {
        paired_v34_total_bytes.saturating_add(paired_sidecar_total_bytes) as f64
            / paired_v33_total_bytes as f64
    };
    // `metric_contract_serialize_us` remains durable diagnostic telemetry.
    // The frozen BURN contract gates the continuous first-producer-to-final-
    // bytes measurement instead of imposing a second, overlapping threshold
    // on one implementation sub-step.
    let resource_failure = comparator_p99_us > PR2C_COMPARATOR_P99_MAX_US
        || metric_contract_build_and_serialize_p99_us
            > u64::from(PR2C_FULL_BUILD_AND_SERIALIZE_P99_MAX_US)
        || projection_build_and_validate_p99_us
            > u64::from(PR2C_PROJECTION_BUILD_AND_VALIDATE_P99_MAX_US)
        || logger_enqueue_wait_p99_us > u64::from(PR2C_LOGGER_ENQUEUE_WAIT_P99_MAX_US)
        || projection_wire_p95_bytes > 12 * 1024
        || projection_wire_max_bytes > METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1
        || sidecar_p95_bytes > 24 * 1024
        || sidecar_p99_bytes > 48 * 1024
        || v34_p95_increase_bytes > 8 * 1024
        || v34_p95_increase_ratio > 0.10
        || combined_bytes_delta_ratio > 0.25
        || queue_high_water_ratio >= 0.80
        || manifest.writer_stats.queue_dropped_rows_total > 0
        || manifest.writer_stats.summary_write_failures_total > 0
        || manifest.writer_stats.evidence_write_failures_total > 0
        || manifest.writer_stats.writer_disabled_total > 0
        || manifest.writer_stats.queue_send_failures_total > 0
        || manifest.writer_stats.missing_pair_total > 0
        || manifest.writer_stats.orphan_summary_total > 0
        || manifest.writer_stats.orphan_evidence_total > 0
        || manifest.writer_stats.manifest_write_failures_total > 0
        || manifest.writer_stats.finalization_failures_total > 0;
    let schema_failure = duplicate_record_identities > 0
        || missing_pairs > 0
        || !reasons.is_empty()
        || replayed_rows != paired_ids.len()
        || manifest.writer_stats.paired_commands_total != summary_ids.len() as u64
        || summary_ids != evidence_ids
        || summary_ids != v33_ids;
    let terminal_class = if schema_failure {
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    } else if policy_drift_rows > 0 {
        MetricContractAuditTerminalClassV1::FailPolicyDrift
    } else if resource_failure {
        MetricContractAuditTerminalClassV1::FailResourceBudget
    } else if stable_identity_unavailable_rows > 0 || comparator_not_evaluable_rows > 0 {
        MetricContractAuditTerminalClassV1::NotEvaluable
    } else {
        MetricContractAuditTerminalClassV1::PassCutoverReady
    };
    let run_id = manifest
        .summary_parts
        .first()
        .map(|part| part.run_id.clone());
    Ok(Pr2cSingleRunAuditReportV1 {
        terminal_class,
        cutover_scope: MetricContractCutoverScopeV1::MetricContractsV1_1ProfileAEquivalenceOnly,
        run_id,
        run_start_ms,
        run_end_ms,
        summary_rows: summary_ids.len(),
        evidence_rows: evidence_row_count,
        replayed_rows,
        duplicate_record_identities,
        missing_pairs,
        policy_drift_rows,
        comparator_not_evaluable_rows,
        counterfactual_policy_delta_observed_rows,
        counterfactual_not_evaluable_rows,
        stable_identity_unavailable_rows,
        dev_known_decisions,
        clean_flip_v2_evaluable,
        real_dev_legacy_v2_divergences,
        projection_wire_p95_bytes,
        projection_wire_max_bytes,
        sidecar_p95_bytes,
        sidecar_p99_bytes,
        comparator_p99_us,
        metric_contract_serialize_p99_us,
        metric_contract_build_and_serialize_p99_us,
        projection_build_and_validate_p99_us,
        logger_enqueue_wait_p99_us,
        v34_p95_increase_bytes,
        v34_p95_increase_ratio,
        combined_bytes_delta_ratio,
        writer_queue_high_water: manifest.writer_stats.writer_queue_high_water,
        paired_record_identities,
        paired_decision_timestamps_ms,
        utc_4h_buckets,
        counterfactual_diagnostics,
        reasons,
    })
}

pub fn audit_pr2c_bundle_v1(
    runs: &[(PathBuf, Vec<PathBuf>)],
) -> Result<Pr2cBundleAuditReportV1, Pr2cAuditErrorV1> {
    let mut reports = Vec::new();
    let mut provenance = BTreeSet::new();
    let mut stable_identities = BTreeMap::<String, String>::new();
    let mut run_ids = BTreeSet::new();
    let mut unique_run_ids = true;
    let mut global_record_identities = BTreeSet::new();
    let mut global_duplicate_record_identities = 0usize;
    let mut intervals = Vec::new();
    let mut stable_event_collisions = 0usize;
    let mut stable_identity_collision_gate_evaluable = true;
    for (run_dir, decision_paths) in runs {
        let report = audit_pr2c_single_run_v1(run_dir, decision_paths)?;
        if !report
            .run_id
            .as_ref()
            .is_some_and(|run_id| run_ids.insert(run_id.clone()))
        {
            unique_run_ids = false;
        }
        for identity in &report.paired_record_identities {
            if !global_record_identities.insert(identity.clone()) {
                global_duplicate_record_identities += 1;
            }
        }
        if report.stable_identity_unavailable_rows > 0 {
            stable_identity_collision_gate_evaluable = false;
        }
        if let (Some(start), Some(end)) = (report.run_start_ms, report.run_end_ms) {
            intervals.push((start, end));
        }
        let manifest: MetricContractRotationManifestV1 =
            read_json(&run_dir.join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE))?;
        if let Some(part) = manifest.summary_parts.first() {
            provenance.insert(
                serde_json::to_string(&serde_json::json!({
                    "build_commit": part.build_commit,
                    "build_worktree_clean": part.build_worktree_clean,
                    "schema": part.schema,
                    "gatekeeper_config_hash": part.gatekeeper_config_hash,
                    // Brain config remains durable provenance and is frozen
                    // within each run, but it is not an exact cross-run
                    // equivalence gate. Gatekeeper/profile/effective-config
                    // provenance below remains exact across the bundle.
                    "rollout_mode": part.rollout_mode,
                    "metric_contract_schema_version": part.metric_contract_schema_version,
                    "projection_wire_version": part.projection_wire_version,
                    "evidence_schema_version": part.evidence_schema_version,
                    "decision_schema_version": part.decision_schema_version,
                    "wire_schema_manifest_blake3": part.wire_schema_manifest_blake3,
                    "burn_in_contract_version": part.burn_in_contract_version,
                    "burn_in_contract_canonical_hash": part.burn_in_contract_canonical_hash,
                    "profile_id": part.profile_id,
                    "profile_hash": part.profile_hash,
                    "metric_contract_effective_config_hash":
                        part.metric_contract_effective_config_hash,
                    "effective_config_schema_version": part.effective_config.payload.schema_version,
                }))
                .map_err(|source| Pr2cAuditErrorV1::Json {
                    path: run_dir.display().to_string(),
                    source,
                })?,
            );
        }
        for part in &manifest.evidence_parts {
            let (rows, _) = read_jsonl::<MetricContractEvidenceTransportV1>(
                &resolve_manifest_part_path(run_dir, &part.file_path)?,
            )?;
            for row in rows {
                if let ghost_core::metric_contracts::CanonicalNullableV1::Value(identity) =
                    row.payload.stable_event_identity
                {
                    let key = serde_json::to_string(&identity).map_err(|source| {
                        Pr2cAuditErrorV1::Json {
                            path: part.file_path.clone(),
                            source,
                        }
                    })?;
                    let run_id = row.payload.record_identity.run_id;
                    if stable_identities
                        .insert(key, run_id.clone())
                        .is_some_and(|previous| previous != run_id)
                    {
                        stable_event_collisions += 1;
                    }
                }
            }
        }
        reports.push(report);
    }
    let consistent_provenance = provenance.len() <= 1;
    intervals.sort_unstable();
    let non_overlapping_runs = intervals.windows(2).all(|window| window[0].1 < window[1].0);
    let mut reasons = Vec::new();
    if !consistent_provenance {
        reasons.push("build/profile/effective-config mismatch".to_string());
    }
    if stable_event_collisions > 0 {
        reasons.push("stable underlying event collision".to_string());
    }
    if !non_overlapping_runs {
        reasons.push("run time ranges overlap".to_string());
    }
    if !unique_run_ids {
        reasons.push("bundle contains duplicate or missing run_id".to_string());
    }
    if global_duplicate_record_identities > 0 {
        reasons.push("bundle contains duplicate full record identity".to_string());
    }
    let terminal_class = if reports.iter().any(|report| {
        report.terminal_class == MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    }) || !consistent_provenance
        || stable_event_collisions > 0
        || !non_overlapping_runs
        || !unique_run_ids
        || global_duplicate_record_identities > 0
    {
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    } else if reports
        .iter()
        .any(|report| report.terminal_class == MetricContractAuditTerminalClassV1::FailPolicyDrift)
    {
        MetricContractAuditTerminalClassV1::FailPolicyDrift
    } else if reports.iter().any(|report| {
        report.terminal_class == MetricContractAuditTerminalClassV1::FailResourceBudget
    }) {
        MetricContractAuditTerminalClassV1::FailResourceBudget
    } else if !stable_identity_collision_gate_evaluable
        || reports
            .iter()
            .any(|report| report.terminal_class == MetricContractAuditTerminalClassV1::NotEvaluable)
    {
        MetricContractAuditTerminalClassV1::NotEvaluable
    } else {
        MetricContractAuditTerminalClassV1::PassCutoverReady
    };
    Ok(Pr2cBundleAuditReportV1 {
        terminal_class,
        cutover_scope: MetricContractCutoverScopeV1::MetricContractsV1_1ProfileAEquivalenceOnly,
        run_reports: reports,
        non_overlapping_runs,
        consistent_provenance,
        stable_event_collisions,
        stable_identity_collision_gate_evaluable,
        unique_run_ids,
        global_duplicate_record_identities,
        reasons,
    })
}

pub fn audit_pr2c_bundle_against_burn_in_contract_v2(
    runs: &[(PathBuf, Vec<PathBuf>)],
    contract: &BurnInContractV1,
) -> Result<Pr2cBundleAuditReportV1, Pr2cAuditErrorV1> {
    contract
        .validate_hash()
        .map_err(|error| Pr2cAuditErrorV1::PartIntegrity(error.to_string()))?;
    for (run_dir, _) in runs {
        let manifest: MetricContractRotationManifestV1 =
            read_json(&run_dir.join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE))?;
        if manifest
            .summary_parts
            .iter()
            .chain(&manifest.evidence_parts)
            .any(|part| {
                part.burn_in_contract_version != contract.payload.burn_in_contract_version
                    || part.burn_in_contract_canonical_hash != contract.contract_canonical_hash
                    || part.wire_schema_manifest_blake3
                        != contract.payload.wire_schema_manifest_blake3
            })
        {
            return Err(Pr2cAuditErrorV1::PartIntegrity(
                "run manifest is not bound to the supplied BURN_IN_CONTRACT_V3".to_string(),
            ));
        }
    }
    let mut report = audit_pr2c_bundle_v1(runs)?;
    let payload = &contract.payload;
    let frozen_at_ms = chrono::DateTime::parse_from_rfc3339(&payload.frozen_at)
        .map_err(|_| {
            Pr2cAuditErrorV1::PartIntegrity(
                "BURN_IN_CONTRACT_V3 frozen_at is not RFC3339".to_string(),
            )
        })?
        .timestamp_millis();
    let aggregate_duration_ms = report.run_reports.iter().try_fold(0u64, |total, run| {
        let duration = run
            .run_start_ms
            .zip(run.run_end_ms)
            .and_then(|(start, end)| end.checked_sub(start))
            .ok_or_else(|| {
                Pr2cAuditErrorV1::PartIntegrity("run has no valid paired duration".to_string())
            })?;
        total.checked_add(duration).ok_or_else(|| {
            Pr2cAuditErrorV1::PartIntegrity("aggregate run duration overflow".to_string())
        })
    })?;
    let decisions = report
        .run_reports
        .iter()
        .flat_map(|run| run.paired_record_identities.iter().cloned())
        .collect::<BTreeSet<_>>()
        .len() as u64;
    let dev_known = report
        .run_reports
        .iter()
        .map(|run| run.dev_known_decisions as u64)
        .sum::<u64>();
    let flip_evaluable = report
        .run_reports
        .iter()
        .map(|run| run.clean_flip_v2_evaluable as u64)
        .sum::<u64>();
    let dev_divergences = report
        .run_reports
        .iter()
        .map(|run| run.real_dev_legacy_v2_divergences as u64)
        .sum::<u64>();
    let buckets = report
        .run_reports
        .iter()
        .flat_map(|run| run.utc_4h_buckets.iter().copied())
        .collect::<BTreeSet<_>>()
        .len();
    let each_run_duration_pass = report.run_reports.iter().all(|run| {
        run.run_start_ms
            .zip(run.run_end_ms)
            .and_then(|(start, end)| end.checked_sub(start))
            .is_some_and(|duration| duration >= payload.minimum_run_duration_ms)
    });
    let prospective_rows_only = u64::try_from(frozen_at_ms)
        .ok()
        .is_some_and(|frozen_at_ms| {
            report.run_reports.iter().all(|run| {
                !run.paired_decision_timestamps_ms.is_empty()
                    && run
                        .paired_decision_timestamps_ms
                        .iter()
                        .all(|timestamp| *timestamp > frozen_at_ms)
            })
        });
    let every_run_passed_before_aggregation = report
        .run_reports
        .iter()
        .all(|run| run.terminal_class == MetricContractAuditTerminalClassV1::PassCutoverReady);
    let enough_runs = report.run_reports.len() >= usize::from(payload.minimum_non_overlapping_runs);
    let enough_buckets = buckets >= usize::from(payload.minimum_utc_4h_buckets);
    let enough_aggregate_duration = aggregate_duration_ms >= payload.minimum_aggregate_duration_ms;
    let enough_decisions = decisions >= payload.minimum_unique_decisions;
    let enough_dev_known = dev_known >= payload.minimum_dev_known_decisions;
    let enough_flip_evaluable = flip_evaluable >= payload.minimum_clean_flip_v2_evaluable;
    let enough_dev_divergences = dev_divergences >= payload.minimum_real_dev_legacy_v2_divergences;
    let minima_pass = every_run_passed_before_aggregation
        && report.unique_run_ids
        && report.global_duplicate_record_identities == 0
        && enough_runs
        && each_run_duration_pass
        && enough_buckets
        && enough_aggregate_duration
        && enough_decisions
        && enough_dev_known
        && enough_flip_evaluable
        && enough_dev_divergences
        && prospective_rows_only;
    if !minima_pass {
        if !enough_runs {
            report
                .reasons
                .push("BURN_IN_CONTRACT_V3 minimum non-overlapping run count not met".to_string());
        }
        if !each_run_duration_pass {
            report
                .reasons
                .push("BURN_IN_CONTRACT_V3 minimum per-run duration not met".to_string());
        }
        if !enough_buckets {
            report.reasons.push(
                "BURN_IN_CONTRACT_V3 minimum paired-decision UTC bucket count not met".to_string(),
            );
        }
        if !enough_aggregate_duration {
            report
                .reasons
                .push("BURN_IN_CONTRACT_V3 minimum aggregate duration not met".to_string());
        }
        if !enough_decisions {
            report
                .reasons
                .push("BURN_IN_CONTRACT_V3 minimum unique decisions not met".to_string());
        }
        if !enough_dev_known {
            report
                .reasons
                .push("BURN_IN_CONTRACT_V3 minimum dev-known decisions not met".to_string());
        }
        if !enough_flip_evaluable {
            report
                .reasons
                .push("BURN_IN_CONTRACT_V3 minimum clean Flip V2 evidence not met".to_string());
        }
        if !enough_dev_divergences {
            report.reasons.push(
                "BURN_IN_CONTRACT_V3 minimum real dev divergence evidence not met".to_string(),
            );
        }
        if !prospective_rows_only {
            report
                .reasons
                .push("BURN_IN_CONTRACT_V3 contains a row at or before frozen_at".to_string());
        }
        if !every_run_passed_before_aggregation {
            report.reasons.push(
                "BURN_IN_CONTRACT_V3 requires every run to pass before aggregation".to_string(),
            );
        }
        if report.terminal_class == MetricContractAuditTerminalClassV1::PassCutoverReady {
            report.terminal_class = MetricContractAuditTerminalClassV1::NotEvaluable;
        }
    }
    Ok(report)
}

#[must_use]
pub fn canonical_metric_contract_part_paths(run_dir: &Path) -> [PathBuf; 2] {
    [
        run_dir.join(METRIC_CONTRACT_SUMMARY_V34_FILE),
        run_dir.join(METRIC_CONTRACT_EVIDENCE_V1_FILE),
    ]
}

#[allow(dead_code)]
fn _stable_identity_type_contract(_: &StableEventIdentityV1) {}
