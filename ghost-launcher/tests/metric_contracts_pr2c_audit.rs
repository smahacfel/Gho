#[path = "common/metric_contracts_pr2c.rs"]
mod common;

use common::{
    current_v33_log, equal_policy, paired_fixture, paired_fixture_with_comparator,
    paired_fixture_with_degraded_flip, paired_fixture_with_dev_counterfactual,
    paired_fixture_with_dev_primary_only, paired_fixture_with_ftdi_counterfactual,
    paired_fixture_with_policies, paired_fixture_with_stable_identity,
};
use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::oracle::reason_code::GatekeeperReasonCode;
use ghost_brain::oracle::{
    MetricContractLatencyHistogramSnapshotV1, MetricContractPairedWriterConfigV1,
    MetricContractPairedWriterStatsV1, MetricContractPairedWriterV1,
    MetricContractRotationManifestV1, METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE,
};
use ghost_core::metric_contracts::{
    BurnInContractV1, MetricContractAuditTerminalClassV1, MetricContractCutoverScopeV1,
};
use ghost_launcher::components::gatekeeper_policy::{
    build_assessment_from_features, evaluate_policy_from_assessment, PolicyEvaluationContext,
};
use ghost_launcher::metric_contracts::{
    audit_pr2c_bundle_against_burn_in_contract_v2, audit_pr2c_bundle_v1, audit_pr2c_single_run_v1,
    pr2c_policy_equivalence_snapshot_v1,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn write_current_v33(
    path: &std::path::Path,
    pair: &ghost_core::metric_contracts::MetricContractPairedRecordV1,
) {
    let row = current_v33_log(pair);
    std::fs::write(path, format!("{}\n", serde_json::to_string(&row).unwrap())).unwrap();
}

fn normalize_semantic_audit_fixture_histograms(run_dir: &std::path::Path) {
    // These tests exercise replay, joins, provenance, bundle minima and the
    // audit's histogram-integrity rules in the unoptimized test profile. They
    // must not turn debug JSON serialization speed on a shared CI runner into
    // a resource-acceptance assertion. Give the semantic fixture one closed,
    // internally consistent sample per paired command; the dedicated release
    // harness below the durability suite remains the sole performance proof
    // and uses the real continuous producer-to-final-byte clock.
    let path = run_dir.join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut manifest: MetricContractRotationManifestV1 =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let sample_count = manifest.writer_stats.paired_commands_total;
    let deterministic = || {
        let mut histogram = MetricContractLatencyHistogramSnapshotV1::default();
        histogram.sample_count = sample_count;
        if sample_count > 0 {
            histogram.bucket_counts[1] = sample_count;
            histogram.max_us = 2;
        }
        histogram
    };
    manifest.writer_stats.logger_enqueue_wait_us = deterministic();
    manifest.writer_stats.metric_contract_build_and_serialize_us = deterministic();
    manifest.writer_stats.projection_build_and_validate_us = deterministic();
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

async fn write_run(run_id: &str, join_key: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let pair = paired_fixture(run_id, join_key);
    write_pair_run(pair).await
}

async fn write_pair_run(
    mut pair: ghost_core::metric_contracts::MetricContractPairedRecordV1,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    stats.record_enqueue_wait_us(1);
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    // The public constructor deliberately fails closed. Audit fixtures model
    // the production DecisionLogger boundary, which supplies an attested
    // clean-tree bit from build-time Git provenance.
    config.build_worktree_clean = true;
    let mut writer = MetricContractPairedWriterV1::open(config, stats)
        .await
        .unwrap();
    // These audit fixtures receive a prebuilt pair and therefore cannot
    // preserve a meaningful producer-start clock across fixture setup. Start
    // their synthetic one-record resource sample at the writer boundary. The
    // dedicated durability harness separately proves the real continuous
    // producer -> pair -> writer clock.
    pair.metric_contract_full_path_started = std::time::Instant::now();
    writer.write_pair(pair.clone()).await.unwrap();
    writer.finalize().await.unwrap();
    normalize_semantic_audit_fixture_histograms(temp.path());
    let v33_path = temp.path().join("gatekeeper_v2_decisions.jsonl");
    write_current_v33(&v33_path, &pair);
    (temp, v33_path)
}

#[tokio::test]
async fn single_run_audit_verifies_manifest_pair_projection_replay_and_resources() {
    let (run, v33) = write_run("run-a", "join-a").await;
    let v33_bytes = std::fs::read(&v33).unwrap();
    let summary_bytes = std::fs::read(
        run.path()
            .join(ghost_brain::oracle::METRIC_CONTRACT_SUMMARY_V34_FILE),
    )
    .unwrap();
    let sidecar_bytes = std::fs::read(
        run.path()
            .join(ghost_brain::oracle::METRIC_CONTRACT_EVIDENCE_V1_FILE),
    )
    .unwrap();
    let expected_additive_delta =
        (summary_bytes.len() + sidecar_bytes.len() - 2) as f64 / (v33_bytes.len() - 1) as f64;
    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::PassCutoverReady,
        "{report:#?}"
    );
    assert_eq!(report.replayed_rows, 1);
    assert_eq!(
        report.cutover_scope,
        MetricContractCutoverScopeV1::MetricContractsV1_1ProfileAEquivalenceOnly
    );
    assert_eq!(
        serde_json::to_value(&report).unwrap()["cutover_scope"],
        Value::String("metric_contracts_v1_1_profile_a_equivalence_only".to_string())
    );
    assert_eq!(report.missing_pairs, 0);
    assert_eq!(report.policy_drift_rows, 0);
    assert!((report.combined_bytes_delta_ratio - expected_additive_delta).abs() < f64::EPSILON);
    assert!(report.combined_bytes_delta_ratio <= 0.25);
}

#[tokio::test]
async fn audit_detects_missing_pair_and_truncated_jsonl() {
    let (run, v33) = write_run("run-a", "join-a").await;
    let summary = run
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_SUMMARY_V34_FILE);
    std::fs::write(&summary, b"{}").unwrap();
    assert!(audit_pr2c_single_run_v1(run.path(), &[v33]).is_err());
}

#[tokio::test]
async fn extra_current_v33_without_v34_or_evidence_is_a_missing_pair() {
    let (run, v33) = write_run("run-a", "join-a").await;
    let extra_pair = paired_fixture("run-a", "join-extra-v33");
    let mut bytes = std::fs::read(&v33).unwrap();
    bytes.extend_from_slice(
        format!(
            "{}\n",
            serde_json::to_string(&current_v33_log(&extra_pair)).unwrap()
        )
        .as_bytes(),
    );
    std::fs::write(&v33, bytes).unwrap();

    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.missing_pairs, 1);
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    );
}

#[tokio::test]
async fn persisted_equivalence_drift_reaches_audit_as_fail_policy_drift() {
    let mut comparator = equal_policy();
    comparator.primary_reason_code.push_str("_DRIFT");
    let pair = paired_fixture_with_comparator("run-drift", "join-drift", &comparator, true);
    assert!(pair.decision_v34.equivalence_deltas.has_policy_drift());
    let (run, v33) = write_pair_run(pair).await;

    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.policy_drift_rows, 1);
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::FailPolicyDrift
    );
}

#[tokio::test]
async fn real_second_policy_evaluation_persists_one_delta_and_audits_as_policy_drift() {
    let config = GatekeeperV2Config::default();
    let assessment = build_assessment_from_features(
        ghost_core::checkpoint::MaterializedFeatureSet::default(),
        &config,
        PolicyEvaluationContext::default(),
    );
    let comparator_decision = evaluate_policy_from_assessment(&assessment, &config);
    let comparator = pr2c_policy_equivalence_snapshot_v1(&assessment, Some(&comparator_decision));

    // Model a durable authoritative result that differs from the real second
    // evaluation in exactly one frozen lane. The pair must remain writable so
    // the audit, rather than structural validation, classifies the drift.
    let mut authoritative_decision = comparator_decision.clone();
    authoritative_decision.reason_code = Some(GatekeeperReasonCode::BuyNormal);
    let authoritative =
        pr2c_policy_equivalence_snapshot_v1(&assessment, Some(&authoritative_decision));
    let deltas = authoritative.compare(&comparator);
    assert_eq!(
        deltas.primary_reason_code,
        ghost_core::metric_contracts::ComparatorDeltaStatusV1::Different
    );
    assert_eq!(
        deltas.verdict,
        ghost_core::metric_contracts::ComparatorDeltaStatusV1::Equal
    );
    assert_eq!(
        deltas.ordered_reason_chain,
        ghost_core::metric_contracts::ComparatorDeltaStatusV1::Equal
    );

    let pair = paired_fixture_with_policies(
        "run-real-second-evaluation",
        "join-real-second-evaluation",
        &authoritative,
        &comparator,
    );
    let (run, v33) = write_pair_run(pair).await;
    let manifest: ghost_brain::oracle::MetricContractRotationManifestV1 = serde_json::from_slice(
        &std::fs::read(
            run.path()
                .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(manifest.writer_finalized);

    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.policy_drift_rows, 1);
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::FailPolicyDrift
    );
}

#[tokio::test]
async fn actual_dev_primary_counterfactual_emits_typed_diagnostic_without_policy_drift() {
    let pair = paired_fixture_with_dev_counterfactual("run-counterfactual", "join-counterfactual");
    assert!(pair.decision_v34.counterfactual_delta_present);
    assert!(pair.decision_v34.equivalence_deltas.is_zero_drift());
    let (run, v33) = write_pair_run(pair).await;

    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.counterfactual_policy_delta_observed_rows, 1);
    assert!(report
        .counterfactual_diagnostics
        .iter()
        .any(|diagnostic| diagnostic
            .starts_with("COUNTERFACTUAL_POLICY_DELTA_OBSERVED:dev_primary:")));
    assert_eq!(report.policy_drift_rows, 0);
}

#[tokio::test]
async fn actual_corrected_ftdi_counterfactual_emits_typed_diagnostic_without_policy_drift() {
    let pair = paired_fixture_with_ftdi_counterfactual(
        "run-ftdi-counterfactual",
        "join-ftdi-counterfactual",
    );
    assert!(pair.decision_v34.counterfactual_delta_present);
    assert!(pair.decision_v34.equivalence_deltas.is_zero_drift());
    let (run, v33) = write_pair_run(pair).await;

    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.counterfactual_policy_delta_observed_rows, 1);
    assert!(report.counterfactual_diagnostics.iter().any(|diagnostic| {
        diagnostic.starts_with("COUNTERFACTUAL_POLICY_DELTA_OBSERVED:corrected_ftdi_actionability:")
    }));
    assert_eq!(report.policy_drift_rows, 0);
}

#[tokio::test]
async fn degraded_available_flip_is_not_counted_as_clean_burn_evidence() {
    let pair = paired_fixture_with_degraded_flip("run-degraded-flip", "join-degraded-flip");
    assert_eq!(
        pair.evidence
            .payload
            .contracts
            .flip_ratio
            .hybrid_v2
            .envelope
            .availability,
        ghost_core::metric_contracts::MetricAvailabilityV1::Available
    );
    assert_eq!(
        pair.evidence
            .payload
            .contracts
            .flip_ratio
            .hybrid_v2
            .envelope
            .measurement_quality,
        ghost_core::metric_contracts::MetricMeasurementQualityV1::Degraded
    );
    let (run, v33) = write_pair_run(pair).await;

    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.replayed_rows, 1);
    assert_eq!(report.clean_flip_v2_evaluable, 0);
}

#[tokio::test]
async fn missing_legacy_dev_lane_is_not_counted_as_real_divergence() {
    let pair = paired_fixture_with_dev_primary_only("run-dev-missing", "join-dev-missing");
    assert!(!pair.decision_v34.counterfactual_delta_present);
    let (run, v33) = write_pair_run(pair).await;

    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.replayed_rows, 1);
    assert_eq!(report.real_dev_legacy_v2_divergences, 0);
    assert_eq!(report.counterfactual_policy_delta_observed_rows, 0);
    assert_eq!(report.counterfactual_not_evaluable_rows, 1);
}

#[tokio::test]
async fn bundle_treats_same_join_key_in_different_runs_as_distinct_record_identity() {
    let (run_a, v33_a) = write_run("run-a", "join-shared").await;
    let (run_b, v33_b) = write_run("run-b", "join-shared").await;
    let report = audit_pr2c_bundle_v1(&[
        (run_a.path().to_path_buf(), vec![v33_a]),
        (run_b.path().to_path_buf(), vec![v33_b]),
    ])
    .unwrap();
    assert_eq!(report.stable_event_collisions, 1);
    assert!(report.consistent_provenance);
    assert!(report
        .run_reports
        .iter()
        .all(|run| run.duplicate_record_identities == 0));
}

#[tokio::test]
async fn bundle_rejects_duplicate_run_id_across_distinct_directories() {
    let (run_a, v33_a) = write_run("duplicate-run", "join-a").await;
    let (run_b, v33_b) = write_run("duplicate-run", "join-b").await;
    let report = audit_pr2c_bundle_v1(&[
        (run_a.path().to_path_buf(), vec![v33_a]),
        (run_b.path().to_path_buf(), vec![v33_b]),
    ])
    .unwrap();
    assert!(!report.unique_run_ids);
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    );
}

#[tokio::test]
async fn bundle_rejects_duplicate_full_identity_across_directories() {
    let (run_a, v33_a) = write_run("duplicate-run", "duplicate-join").await;
    let (run_b, v33_b) = write_run("duplicate-run", "duplicate-join").await;
    let report = audit_pr2c_bundle_v1(&[
        (run_a.path().to_path_buf(), vec![v33_a]),
        (run_b.path().to_path_buf(), vec![v33_b]),
    ])
    .unwrap();
    assert_eq!(report.global_duplicate_record_identities, 1);
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    );
}

#[test]
fn historical_v33_materialized_features_without_projection_remain_none() {
    let mut json =
        serde_json::to_value(ghost_core::checkpoint::MaterializedFeatureSet::default()).unwrap();
    json.as_object_mut()
        .unwrap()
        .remove("metric_contract_decision_projection_v1");
    let historical: ghost_core::checkpoint::MaterializedFeatureSet =
        serde_json::from_value(json).unwrap();
    assert!(historical.metric_contract_decision_projection_v1.is_none());
}

#[tokio::test]
async fn current_v34_pair_without_decision_time_projection_is_rejected() {
    let (run, v33) = write_run("run-a", "join-a").await;
    let mut row: Value =
        serde_json::from_str(std::fs::read_to_string(&v33).unwrap().trim()).unwrap();
    row["materialized_feature_snapshot"] = serde_json::json!({});
    std::fs::write(&v33, format!("{}\n", serde_json::to_string(&row).unwrap())).unwrap();
    assert!(audit_pr2c_single_run_v1(run.path(), &[v33]).is_err());
}

#[tokio::test]
async fn arbitrary_or_unknown_current_v33_fields_are_not_a_storage_baseline() {
    let (run, v33) = write_run("run-a", "join-a").await;
    let mut row: Value =
        serde_json::from_str(std::fs::read_to_string(&v33).unwrap().trim()).unwrap();
    row.as_object_mut().unwrap().insert(
        "v33_baseline_padding".to_string(),
        Value::String("x".repeat(100_000)),
    );
    std::fs::write(&v33, format!("{}\n", serde_json::to_string(&row).unwrap())).unwrap();
    assert!(audit_pr2c_single_run_v1(run.path(), &[v33]).is_err());
}

#[tokio::test]
async fn missing_stable_identity_is_not_evaluable_never_zero_collisions() {
    let pair = paired_fixture_with_stable_identity("run-a", "join-a", None);
    let (run, v33) = write_pair_run(pair).await;
    let report = audit_pr2c_single_run_v1(run.path(), std::slice::from_ref(&v33)).unwrap();
    assert_eq!(report.stable_identity_unavailable_rows, 1);
    assert_ne!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::PassCutoverReady,
        "missing stable identity must never become a clean collision PASS: {report:#?}"
    );
    let bundle = audit_pr2c_bundle_v1(&[(run.path().to_path_buf(), vec![v33])]).unwrap();
    assert!(!bundle.stable_identity_collision_gate_evaluable);
}

#[tokio::test]
async fn audit_rejects_modified_sha_missing_and_undeclared_rotated_parts() {
    let (modified, v33_modified) = write_run("run-a", "join-a").await;
    let summary = modified
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_SUMMARY_V34_FILE);
    let mut bytes = std::fs::read(&summary).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&summary, bytes).unwrap();
    assert!(audit_pr2c_single_run_v1(modified.path(), &[v33_modified]).is_err());

    let (extra, v33_extra) = write_run("run-a", "join-a").await;
    std::fs::copy(
        extra
            .path()
            .join(ghost_brain::oracle::METRIC_CONTRACT_SUMMARY_V34_FILE),
        extra
            .path()
            .join("metric_contract_decisions_v34.part-00001.jsonl"),
    )
    .unwrap();
    assert!(audit_pr2c_single_run_v1(extra.path(), &[v33_extra]).is_err());

    let (missing, v33_missing) = write_run("run-a", "join-a").await;
    std::fs::remove_file(
        missing
            .path()
            .join(ghost_brain::oracle::METRIC_CONTRACT_EVIDENCE_V1_FILE),
    )
    .unwrap();
    assert!(audit_pr2c_single_run_v1(missing.path(), &[v33_missing]).is_err());
}

#[tokio::test]
async fn audit_rejects_unknown_or_dirty_build_and_missing_burn_binding() {
    for mutation in ["unknown-build", "dirty-build", "burn-mismatch"] {
        let (run, v33) = write_run("provenance-run", mutation).await;
        let manifest_path = run
            .path()
            .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
        let mut manifest: ghost_brain::oracle::MetricContractRotationManifestV1 =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        for part in manifest
            .summary_parts
            .iter_mut()
            .chain(manifest.evidence_parts.iter_mut())
        {
            match mutation {
                "unknown-build" => part.build_commit = "unknown_build_commit".to_string(),
                "dirty-build" => part.build_worktree_clean = false,
                "burn-mismatch" => {
                    part.burn_in_contract_canonical_hash =
                        ghost_core::metric_contracts::CanonicalHashV1::parse("0".repeat(64))
                            .unwrap();
                }
                _ => unreachable!(),
            }
        }
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(audit_pr2c_single_run_v1(run.path(), &[v33]).is_err());
    }
}

#[tokio::test]
async fn current_v33_config_provenance_must_match_paired_run_manifest() {
    for field in ["gatekeeper", "brain"] {
        let (run, v33_path) = write_run("v33-provenance-run", field).await;
        let mut row: ghost_brain::oracle::GatekeeperBuyLog =
            serde_json::from_str(std::fs::read_to_string(&v33_path).unwrap().trim_end()).unwrap();
        match field {
            "gatekeeper" => {
                row.config_hash = Some(
                    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
                );
            }
            "brain" => {
                row.brain_config_hash = Some(
                    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string(),
                );
            }
            _ => unreachable!(),
        }
        std::fs::write(
            &v33_path,
            format!("{}\n", serde_json::to_string(&row).unwrap()),
        )
        .unwrap();

        let report = audit_pr2c_single_run_v1(run.path(), &[v33_path]).unwrap();
        assert_eq!(
            report.terminal_class,
            MetricContractAuditTerminalClassV1::FailSchemaOrReplay
        );
        assert_eq!(report.replayed_rows, 0);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("v33/run-manifest config provenance mismatch")));
    }
}

#[tokio::test]
async fn audit_validates_rotation_row_metadata_and_confines_part_paths_to_run_directory() {
    let (identity_run, identity_v33) = write_run("metadata-run", "metadata-join").await;
    let identity_manifest_path = identity_run
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut identity_manifest: ghost_brain::oracle::MetricContractRotationManifestV1 =
        serde_json::from_slice(&std::fs::read(&identity_manifest_path).unwrap()).unwrap();
    identity_manifest.summary_parts[0].first_record_identity = Some(
        ghost_core::metric_contracts::MetricEvidenceRecordIdentityV1::try_new(
            "metadata-run",
            "forged-first",
            "legacy_live",
        )
        .unwrap(),
    );
    std::fs::write(
        &identity_manifest_path,
        serde_json::to_vec_pretty(&identity_manifest).unwrap(),
    )
    .unwrap();
    assert!(audit_pr2c_single_run_v1(identity_run.path(), &[identity_v33]).is_err());

    let (rotation_run, rotation_v33) = write_run("rotation-run", "rotation-join").await;
    let rotation_manifest_path = rotation_run
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut rotation_manifest: ghost_brain::oracle::MetricContractRotationManifestV1 =
        serde_json::from_slice(&std::fs::read(&rotation_manifest_path).unwrap()).unwrap();
    let evidence_path = rotation_run
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_EVIDENCE_V1_FILE);
    let mut evidence: ghost_core::metric_contracts::MetricContractEvidenceTransportV1 =
        serde_json::from_str(std::fs::read_to_string(&evidence_path).unwrap().trim()).unwrap();
    evidence.rotation_part_index = 42;
    let mut evidence_bytes = serde_json::to_vec(&evidence).unwrap();
    evidence_bytes.push(b'\n');
    std::fs::write(&evidence_path, &evidence_bytes).unwrap();
    rotation_manifest.evidence_parts[0].byte_count = evidence_bytes.len() as u64;
    rotation_manifest.evidence_parts[0].part_sha256 =
        ghost_core::metric_contracts::CanonicalHashV1::parse(format!(
            "{:x}",
            Sha256::digest(&evidence_bytes)
        ))
        .unwrap();
    std::fs::write(
        &rotation_manifest_path,
        serde_json::to_vec_pretty(&rotation_manifest).unwrap(),
    )
    .unwrap();
    assert!(audit_pr2c_single_run_v1(rotation_run.path(), &[rotation_v33]).is_err());

    let (path_run, path_v33) = write_run("path-run", "path-join").await;
    let path_manifest_path = path_run
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut path_manifest: ghost_brain::oracle::MetricContractRotationManifestV1 =
        serde_json::from_slice(&std::fs::read(&path_manifest_path).unwrap()).unwrap();
    path_manifest.summary_parts[0].file_path = "/tmp/outside-run.jsonl".to_string();
    std::fs::write(
        &path_manifest_path,
        serde_json::to_vec_pretty(&path_manifest).unwrap(),
    )
    .unwrap();
    assert!(audit_pr2c_single_run_v1(path_run.path(), &[path_v33]).is_err());
}

#[tokio::test]
async fn audit_rejects_incomplete_or_reinterpreted_resource_histograms() {
    for mutation in [
        "bounds",
        "sample-count",
        "bucket-sum",
        "max",
        "missing-sample",
    ] {
        let (run, v33) = write_run("histogram-run", mutation).await;
        let manifest_path = run
            .path()
            .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
        let mut manifest: ghost_brain::oracle::MetricContractRotationManifestV1 =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        let histogram = &mut manifest.writer_stats.metric_contract_build_and_serialize_us;
        match mutation {
            "bounds" => histogram.bucket_upper_bounds_us[0] = 3,
            "sample-count" => {
                histogram.sample_count += 1;
                histogram.bucket_counts[0] += 1;
            }
            "bucket-sum" => histogram.bucket_counts[0] += 1,
            "max" => histogram.max_us = 0,
            "missing-sample" => {
                histogram.bucket_counts = [0; 19];
                histogram.sample_count = 0;
                histogram.max_us = 0;
            }
            _ => unreachable!(),
        }
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(
            audit_pr2c_single_run_v1(run.path(), &[v33]).is_err(),
            "audit accepted mutated {mutation} histogram"
        );
    }
}

#[tokio::test]
async fn audit_rejects_mutually_consistent_part_one_with_different_run_provenance() {
    let temp = tempfile::tempdir().unwrap();
    let first = paired_fixture("two-part-run", "join-a");
    let second = paired_fixture("two-part-run", "join-b");
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    stats.record_enqueue_wait_us(1);
    stats.record_enqueue_wait_us(1);
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    config.build_worktree_clean = true;
    config.rotation_max_bytes = 1;
    let mut writer = MetricContractPairedWriterV1::open(config, stats)
        .await
        .unwrap();
    writer.write_pair(first.clone()).await.unwrap();
    writer.write_pair(second.clone()).await.unwrap();
    writer.finalize().await.unwrap();

    let v33 = temp.path().join("gatekeeper_v2_decisions.jsonl");
    std::fs::write(
        &v33,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&current_v33_log(&first)).unwrap(),
            serde_json::to_string(&current_v33_log(&second)).unwrap()
        ),
    )
    .unwrap();

    let manifest_path = temp
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut manifest: ghost_brain::oracle::MetricContractRotationManifestV1 =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.summary_parts.len(), 2);
    let different_valid_commit = "a".repeat(40);
    manifest.summary_parts[1].build_commit = different_valid_commit.clone();
    manifest.evidence_parts[1].build_commit = different_valid_commit;
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert!(audit_pr2c_single_run_v1(temp.path(), &[v33]).is_err());
}

#[tokio::test]
async fn public_burn_v3_audit_enforces_run_bucket_and_prospective_minima() {
    let contract: BurnInContractV1 = serde_json::from_str(include_str!(
        "../../reports/metric_contracts/BURN_IN_CONTRACT_V3.json"
    ))
    .unwrap();
    let (run, v33) = write_run("run-a", "join-a").await;
    let report = audit_pr2c_bundle_against_burn_in_contract_v2(
        &[(run.path().to_path_buf(), vec![v33])],
        &contract,
    )
    .unwrap();
    assert_eq!(
        report.cutover_scope,
        MetricContractCutoverScopeV1::MetricContractsV1_1ProfileAEquivalenceOnly
    );
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::NotEvaluable
    );
    for expected in [
        "minimum non-overlapping run count not met",
        "minimum paired-decision UTC bucket count not met",
        "contains a row at or before frozen_at",
    ] {
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason.contains(expected)),
            "missing typed BURN V2 failure {expected}: {report:#?}"
        );
    }
}

#[tokio::test]
async fn public_burn_v3_audit_rejects_gate_hash_change_binding() {
    let contract: BurnInContractV1 = serde_json::from_str(include_str!(
        "../../reports/metric_contracts/BURN_IN_CONTRACT_V3.json"
    ))
    .unwrap();
    let (run, v33) = write_run("burn-binding-run", "burn-binding-join").await;
    let manifest_path = run
        .path()
        .join(ghost_brain::oracle::METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut manifest: ghost_brain::oracle::MetricContractRotationManifestV1 =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    let changed_hash =
        ghost_core::metric_contracts::CanonicalHashV1::parse("0".repeat(64)).unwrap();
    for part in manifest
        .summary_parts
        .iter_mut()
        .chain(manifest.evidence_parts.iter_mut())
    {
        part.burn_in_contract_canonical_hash = changed_hash.clone();
    }
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert!(audit_pr2c_bundle_against_burn_in_contract_v2(
        &[(run.path().to_path_buf(), vec![v33])],
        &contract,
    )
    .is_err());
}

#[tokio::test]
async fn duplicate_full_record_identity_is_rejected_within_one_run() {
    let temp = tempfile::tempdir().unwrap();
    let pair = paired_fixture("run-a", "join-a");
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    stats.record_enqueue_wait_us(1);
    stats.record_enqueue_wait_us(1);
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    config.build_worktree_clean = true;
    let mut writer = MetricContractPairedWriterV1::open(config, stats)
        .await
        .unwrap();
    writer.write_pair(pair.clone()).await.unwrap();
    writer.write_pair(pair.clone()).await.unwrap();
    writer.finalize().await.unwrap();
    let v33 = temp.path().join("gatekeeper_v2_decisions.jsonl");
    write_current_v33(&v33, &pair);
    let report = audit_pr2c_single_run_v1(temp.path(), &[v33]).unwrap();
    assert!(report.duplicate_record_identities > 0);
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::FailSchemaOrReplay
    );
}

#[tokio::test]
async fn audit_rejects_a_manifest_from_a_still_mutable_writer() {
    let temp = tempfile::tempdir().unwrap();
    let pair = paired_fixture("run-a", "join-a");
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    let config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    let mut writer = MetricContractPairedWriterV1::open(config, stats)
        .await
        .unwrap();
    writer.write_pair(pair.clone()).await.unwrap();
    let v33 = temp.path().join("gatekeeper_v2_decisions.jsonl");
    write_current_v33(&v33, &pair);

    assert!(audit_pr2c_single_run_v1(temp.path(), &[v33]).is_err());
}
