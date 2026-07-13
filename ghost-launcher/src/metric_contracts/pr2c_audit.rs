use super::{replay_metric_contract_record_v2, Pr2cReplayInputV2};
use ghost_brain::oracle::{
    MetricContractRotationManifestV1, METRIC_CONTRACT_EVIDENCE_V1_FILE,
    METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE, METRIC_CONTRACT_SUMMARY_V34_FILE,
};
use ghost_core::checkpoint::MaterializedFeatureSet;
use ghost_core::metric_contracts::{
    BurnInContractV1, MetricAvailabilityV1, MetricContractAuditTerminalClassV1,
    MetricContractDecisionSummaryV1, MetricContractEvidenceTransportV1,
    MetricEvidenceRecordIdentityV1, StableEventIdentityV1,
    METRIC_CONTRACT_PROJECTION_SERIALIZED_HARD_MAX_BYTES_V1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pr2cSingleRunAuditReportV1 {
    pub terminal_class: MetricContractAuditTerminalClassV1,
    pub run_id: Option<String>,
    pub run_start_ms: Option<u64>,
    pub run_end_ms: Option<u64>,
    pub summary_rows: usize,
    pub evidence_rows: usize,
    pub replayed_rows: usize,
    pub duplicate_record_identities: usize,
    pub missing_pairs: usize,
    pub policy_drift_rows: usize,
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
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pr2cBundleAuditReportV1 {
    pub terminal_class: MetricContractAuditTerminalClassV1,
    pub run_reports: Vec<Pr2cSingleRunAuditReportV1>,
    pub non_overlapping_runs: bool,
    pub consistent_provenance: bool,
    pub stable_event_collisions: usize,
    pub stable_identity_collision_gate_evaluable: bool,
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
    let rank = ((values.len() - 1) * percentile + 99) / 100;
    values.get(rank).copied()
}

fn resolve_manifest_part_path(run_dir: &Path, file_path: &str) -> PathBuf {
    let path = Path::new(file_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        run_dir.join(path)
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
            || summary.gatekeeper_config_hash != evidence.gatekeeper_config_hash
            || summary.brain_config_hash != evidence.brain_config_hash
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
            let path = resolve_manifest_part_path(run_dir, &part.file_path);
            let bytes = read_bytes(&path)?;
            if bytes.len() as u64 != part.byte_count
                || bytes.iter().filter(|byte| **byte == b'\n').count() as u64 != part.row_count
                || format!("{:x}", Sha256::digest(&bytes)) != part.part_sha256.as_str()
            {
                return Err(Pr2cAuditErrorV1::PartIntegrity(path.display().to_string()));
            }
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
) -> Result<
    BTreeMap<MetricEvidenceRecordIdentityV1, (MaterializedFeatureSet, usize)>,
    Pr2cAuditErrorV1,
> {
    let mut projections = BTreeMap::new();
    for path in decision_v33_paths {
        let (rows, sizes) = read_jsonl::<serde_json::Value>(path)?;
        for (row, serialized_size) in rows.into_iter().zip(sizes) {
            let Some(run_id) = row.get("run_id").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(join_key) = row.get("join_key").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(decision_plane) = row
                .get("decision_plane")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Some(snapshot) = row.get("materialized_feature_snapshot") else {
                continue;
            };
            let identity =
                MetricEvidenceRecordIdentityV1::try_new(run_id, join_key, decision_plane).map_err(
                    |_| Pr2cAuditErrorV1::PartIntegrity("invalid v33 record identity".to_string()),
                )?;
            let mfs = serde_json::from_value(snapshot.clone()).map_err(|source| {
                Pr2cAuditErrorV1::Json {
                    path: path.display().to_string(),
                    source,
                }
            })?;
            if projections
                .insert(identity, (mfs, serialized_size))
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
            &resolve_manifest_part_path(run_dir, &part.file_path),
        )?;
        summaries.append(&mut rows);
        summary_sizes.append(&mut sizes);
    }
    let mut evidence_rows = Vec::new();
    let mut sidecar_sizes = Vec::new();
    for part in &manifest.evidence_parts {
        let (mut rows, mut sizes) = read_jsonl::<MetricContractEvidenceTransportV1>(
            &resolve_manifest_part_path(run_dir, &part.file_path),
        )?;
        evidence_rows.append(&mut rows);
        sidecar_sizes.append(&mut sizes);
    }
    let decision_projections = load_decision_projections(decision_v33_paths)?;
    let paired_v33_total_bytes = decision_projections
        .values()
        .map(|(_, bytes)| *bytes as u64)
        .sum::<u64>();
    let paired_v34_total_bytes = summary_sizes.iter().map(|bytes| *bytes as u64).sum::<u64>();
    let paired_sidecar_total_bytes = sidecar_sizes.iter().map(|bytes| *bytes as u64).sum::<u64>();
    let decision_timestamps = decision_projections
        .values()
        .filter_map(|(mfs, _)| {
            mfs.metric_contract_decision_projection_v1
                .as_ref()
                .map(|projection| {
                    projection
                        .fee_topology_diversity_index
                        .legacy_value
                        .source_cutoff
                        .decision_timestamp_ms
                        .get()
                })
        })
        .collect::<Vec<_>>();
    let run_start_ms = decision_timestamps.iter().copied().min();
    let run_end_ms = decision_timestamps.iter().copied().max();
    let mut evidence_by_id = BTreeMap::new();
    let dev_known_decisions = evidence_rows
        .iter()
        .filter(|row| {
            row.payload
                .contracts
                .dev_buy
                .tx_intel_first_observed
                .creator_known
        })
        .count();
    let clean_flip_v2_evaluable = evidence_rows
        .iter()
        .filter(|row| {
            let flip = &row.payload.contracts.flip_ratio.hybrid_v2;
            flip.envelope.availability == MetricAvailabilityV1::Available
                && flip.eligible_buyer_count > 0
        })
        .count();
    let real_dev_legacy_v2_divergences = evidence_rows
        .iter()
        .filter(|row| {
            let dev = &row.payload.contracts.dev_buy;
            dev.tx_intel_first_observed.amount_sol != dev.mfs_primary_v1.amount_sol
        })
        .count();
    let mut duplicate_record_identities = 0usize;
    for evidence in evidence_rows {
        if evidence_by_id
            .insert(evidence.payload.record_identity.clone(), evidence)
            .is_some()
        {
            duplicate_record_identities += 1;
        }
    }
    let mut summary_ids = BTreeSet::new();
    let evidence_row_count = evidence_by_id.len();
    let mut replayed_rows = 0usize;
    let mut missing_pairs = 0usize;
    let mut policy_drift_rows = 0usize;
    let mut stable_identity_unavailable_rows = 0usize;
    let mut wire_sizes = Vec::new();
    let mut comparator_times = Vec::new();
    let mut serialize_times = Vec::new();
    let mut v34_increase_bytes = Vec::new();
    let mut v34_increase_ratios = Vec::new();
    let mut reasons = Vec::new();
    let effective_config = manifest
        .summary_parts
        .first()
        .map(|part| part.effective_config.clone());
    for (summary, summary_size) in summaries.into_iter().zip(summary_sizes) {
        if !summary_ids.insert(summary.evidence_record_id.clone()) {
            duplicate_record_identities += 1;
            continue;
        }
        comparator_times.push(summary.comparator_elapsed_us);
        serialize_times.push(summary.metric_contract_serialize_us);
        if !summary.equivalence_deltas.is_zero_drift() {
            policy_drift_rows += 1;
        }
        let Some(evidence) = evidence_by_id.remove(&summary.evidence_record_id) else {
            missing_pairs += 1;
            continue;
        };
        if evidence.payload.stable_event_identity.is_null() {
            stable_identity_unavailable_rows += 1;
        }
        let Some((mfs, v33_size)) = decision_projections.get(&summary.evidence_record_id) else {
            missing_pairs += 1;
            continue;
        };
        let increase = i64::try_from(summary_size).unwrap_or(i64::MAX)
            - i64::try_from(*v33_size).unwrap_or(i64::MAX);
        v34_increase_bytes.push(increase);
        v34_increase_ratios.push(summary_size as f64 / *v33_size as f64 - 1.0);
        let Some(projection) = mfs.metric_contract_decision_projection_v1.clone() else {
            return Err(Pr2cAuditErrorV1::MissingDecisionProjection(
                summary.evidence_record_id,
            ));
        };
        wire_sizes.push(
            projection
                .authoritative_serialized_size_bytes()
                .map_err(|error| Pr2cAuditErrorV1::PartIntegrity(error.to_string()))?,
        );
        let Some(effective_config) = effective_config.clone() else {
            missing_pairs += 1;
            continue;
        };
        if let Err(error) = replay_metric_contract_record_v2(Pr2cReplayInputV2 {
            decision_v34: summary,
            evidence,
            decision_time_projection: projection,
            effective_config,
        }) {
            reasons.push(error.to_string());
        } else {
            replayed_rows += 1;
        }
    }
    missing_pairs += evidence_by_id.len();
    let projection_wire_p95_bytes = percentile(&wire_sizes, 95).unwrap_or_default();
    let projection_wire_max_bytes = wire_sizes.iter().copied().max().unwrap_or_default();
    let sidecar_p95_bytes = percentile(&sidecar_sizes, 95).unwrap_or_default();
    let sidecar_p99_bytes = percentile(&sidecar_sizes, 99).unwrap_or_default();
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
        v34_increase_ratios[((v34_increase_ratios.len() - 1) * 95 + 99) / 100]
    };
    let queue_high_water_ratio = if manifest.writer_queue_capacity == 0 {
        1.0
    } else {
        manifest.writer_stats.writer_queue_high_water as f64 / manifest.writer_queue_capacity as f64
    };
    let combined_bytes_delta_ratio = if paired_v33_total_bytes == 0 {
        f64::INFINITY
    } else {
        (paired_v34_total_bytes.saturating_add(paired_sidecar_total_bytes) as f64
            / paired_v33_total_bytes as f64)
            - 1.0
    };
    let resource_failure = comparator_p99_us > 1_000
        || metric_contract_serialize_p99_us > 1_000
        || metric_contract_build_and_serialize_p99_us > 1_000
        || projection_build_and_validate_p99_us > 1_000
        || logger_enqueue_wait_p99_us > 1_000
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
        || manifest.writer_stats.orphan_evidence_total > 0;
    let schema_failure = duplicate_record_identities > 0
        || missing_pairs > 0
        || !reasons.is_empty()
        || replayed_rows != summary_ids.len();
    let terminal_class = if schema_failure {
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    } else if policy_drift_rows > 0 {
        MetricContractAuditTerminalClassV1::FailPolicyDrift
    } else if resource_failure {
        MetricContractAuditTerminalClassV1::FailResourceBudget
    } else if stable_identity_unavailable_rows > 0 {
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
        run_id,
        run_start_ms,
        run_end_ms,
        summary_rows: summary_ids.len(),
        evidence_rows: evidence_row_count,
        replayed_rows,
        duplicate_record_identities,
        missing_pairs,
        policy_drift_rows,
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
        reasons,
    })
}

pub fn audit_pr2c_bundle_v1(
    runs: &[(PathBuf, Vec<PathBuf>)],
) -> Result<Pr2cBundleAuditReportV1, Pr2cAuditErrorV1> {
    let mut reports = Vec::new();
    let mut provenance = BTreeSet::new();
    let mut stable_identities = BTreeMap::<String, String>::new();
    let mut intervals = Vec::new();
    let mut stable_event_collisions = 0usize;
    let mut stable_identity_collision_gate_evaluable = true;
    for (run_dir, decision_paths) in runs {
        let report = audit_pr2c_single_run_v1(run_dir, decision_paths)?;
        if report.stable_identity_unavailable_rows > 0 {
            stable_identity_collision_gate_evaluable = false;
        }
        if let (Some(start), Some(end)) = (report.run_start_ms, report.run_end_ms) {
            intervals.push((start, end));
        }
        let manifest: MetricContractRotationManifestV1 =
            read_json(&run_dir.join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE))?;
        if let Some(part) = manifest.summary_parts.first() {
            provenance.insert((
                part.build_commit.clone(),
                part.schema.clone(),
                part.gatekeeper_config_hash.clone(),
                part.profile_id.clone(),
                part.profile_hash.clone(),
                part.metric_contract_effective_config_hash.clone(),
                part.effective_config.payload.schema_version,
            ));
        }
        for part in &manifest.evidence_parts {
            let (rows, _) = read_jsonl::<MetricContractEvidenceTransportV1>(
                &resolve_manifest_part_path(run_dir, &part.file_path),
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
    let terminal_class = if reports.iter().any(|report| {
        report.terminal_class == MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    }) {
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
    } else if !consistent_provenance || stable_event_collisions > 0 || !non_overlapping_runs {
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
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
        run_reports: reports,
        non_overlapping_runs,
        consistent_provenance,
        stable_event_collisions,
        stable_identity_collision_gate_evaluable,
        reasons,
    })
}

pub fn audit_pr2c_bundle_against_burn_in_contract_v1(
    runs: &[(PathBuf, Vec<PathBuf>)],
    contract: &BurnInContractV1,
) -> Result<Pr2cBundleAuditReportV1, Pr2cAuditErrorV1> {
    contract
        .validate_hash()
        .map_err(|error| Pr2cAuditErrorV1::PartIntegrity(error.to_string()))?;
    let mut report = audit_pr2c_bundle_v1(runs)?;
    let payload = &contract.payload;
    let frozen_at_ms = chrono::DateTime::parse_from_rfc3339(&payload.frozen_at)
        .map_err(|_| {
            Pr2cAuditErrorV1::PartIntegrity(
                "BURN_IN_CONTRACT_V1 frozen_at is not RFC3339".to_string(),
            )
        })?
        .timestamp_millis();
    let aggregate_duration_ms = report
        .run_reports
        .iter()
        .filter_map(|run| Some(run.run_end_ms?.checked_sub(run.run_start_ms?)?))
        .sum::<u64>();
    let decisions = report
        .run_reports
        .iter()
        .map(|run| run.summary_rows as u64)
        .sum::<u64>();
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
        .filter_map(|run| run.run_start_ms.map(|start| start / 14_400_000))
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
            report
                .run_reports
                .iter()
                .all(|run| run.run_start_ms.is_some_and(|start| start > frozen_at_ms))
        });
    let minima_pass = report.run_reports.len() >= usize::from(payload.minimum_non_overlapping_runs)
        && each_run_duration_pass
        && buckets >= usize::from(payload.minimum_utc_4h_buckets)
        && aggregate_duration_ms >= payload.minimum_aggregate_duration_ms
        && decisions >= payload.minimum_unique_decisions
        && dev_known >= payload.minimum_dev_known_decisions
        && flip_evaluable >= payload.minimum_clean_flip_v2_evaluable
        && dev_divergences >= payload.minimum_real_dev_legacy_v2_divergences
        && prospective_rows_only;
    if !minima_pass {
        report
            .reasons
            .push("BURN_IN_CONTRACT_V1 prospective minima not met".to_string());
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
