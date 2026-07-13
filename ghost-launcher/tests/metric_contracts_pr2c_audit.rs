#[path = "common/metric_contracts_pr2c.rs"]
mod common;

use common::{mfs_with_projection, paired_fixture, paired_fixture_with_stable_identity};
use ghost_brain::oracle::{
    MetricContractPairedWriterConfigV1, MetricContractPairedWriterStatsV1,
    MetricContractPairedWriterV1,
};
use ghost_core::metric_contracts::MetricContractAuditTerminalClassV1;
use ghost_launcher::metric_contracts::{audit_pr2c_bundle_v1, audit_pr2c_single_run_v1};
use serde_json::Value;
use std::sync::Arc;

async fn write_run(run_id: &str, join_key: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let pair = paired_fixture(run_id, join_key);
    write_pair_run(pair).await
}

async fn write_pair_run(
    pair: ghost_core::metric_contracts::MetricContractPairedRecordV1,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let identity = pair.record_identity().clone();
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    stats.record_enqueue_wait_us(1);
    let config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    let mut writer = MetricContractPairedWriterV1::open(config, stats)
        .await
        .unwrap();
    writer.write_pair(pair.clone()).await.unwrap();
    writer.finalize().await.unwrap();
    let v33_path = temp.path().join("gatekeeper_v2_decisions.jsonl");
    let row = serde_json::json!({
        "run_id": identity.run_id,
        "join_key": identity.join_key,
        "decision_plane": identity.decision_plane,
        "materialized_feature_snapshot": mfs_with_projection(&pair),
        // Existing v33 rows carry the full Gatekeeper snapshot and are
        // approximately 83-137 KiB in the frozen historical baseline. Keep
        // this fixture representative so the combined-delta gate is tested
        // against its real denominator rather than a synthetic tiny row.
        "v33_baseline_padding": "x".repeat(100_000),
    });
    std::fs::write(
        &v33_path,
        format!("{}\n", serde_json::to_string(&row).unwrap()),
    )
    .unwrap();
    (temp, v33_path)
}

#[tokio::test]
async fn single_run_audit_verifies_manifest_pair_projection_replay_and_resources() {
    let (run, v33) = write_run("run-a", "join-a").await;
    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::PassCutoverReady,
        "{report:#?}"
    );
    assert_eq!(report.replayed_rows, 1);
    assert_eq!(report.missing_pairs, 0);
    assert_eq!(report.policy_drift_rows, 0);
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
async fn missing_stable_identity_is_not_evaluable_never_zero_collisions() {
    let pair = paired_fixture_with_stable_identity("run-a", "join-a", None);
    let (run, v33) = write_pair_run(pair).await;
    let report = audit_pr2c_single_run_v1(run.path(), &[v33]).unwrap();
    assert_eq!(report.stable_identity_unavailable_rows, 1);
    assert_eq!(
        report.terminal_class,
        MetricContractAuditTerminalClassV1::NotEvaluable,
        "{report:#?}"
    );
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
async fn duplicate_full_record_identity_is_rejected_within_one_run() {
    let temp = tempfile::tempdir().unwrap();
    let pair = paired_fixture("run-a", "join-a");
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    stats.record_enqueue_wait_us(1);
    let config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    let mut writer = MetricContractPairedWriterV1::open(config, stats)
        .await
        .unwrap();
    writer.write_pair(pair.clone()).await.unwrap();
    writer.write_pair(pair.clone()).await.unwrap();
    writer.finalize().await.unwrap();
    let v33 = temp.path().join("gatekeeper_v2_decisions.jsonl");
    let row = serde_json::json!({
        "run_id": "run-a",
        "join_key": "join-a",
        "decision_plane": "legacy_live",
        "materialized_feature_snapshot": mfs_with_projection(&pair),
        "v33_baseline_padding": "x".repeat(100_000),
    });
    std::fs::write(&v33, format!("{}\n", serde_json::to_string(&row).unwrap())).unwrap();
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
    let row = serde_json::json!({
        "run_id": "run-a",
        "join_key": "join-a",
        "decision_plane": "legacy_live",
        "materialized_feature_snapshot": mfs_with_projection(&pair),
    });
    std::fs::write(&v33, format!("{}\n", serde_json::to_string(&row).unwrap())).unwrap();

    assert!(audit_pr2c_single_run_v1(temp.path(), &[v33]).is_err());
}
