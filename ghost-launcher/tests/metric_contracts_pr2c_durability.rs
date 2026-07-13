#[path = "common/metric_contracts_pr2c.rs"]
mod common;

use common::{frozen_inputs_fixture, paired_fixture};
use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::oracle::decision_logger::MetricContractEnqueueErrorV1;
use ghost_brain::oracle::{
    DecisionLogger, DecisionLoggerConfig, MetricContractPairedWriterConfigV1,
    MetricContractPairedWriterStatsV1, MetricContractPairedWriterV1,
    MetricContractRotationManifestV1, MetricContractWriterFaultInjectionV1,
    METRIC_CONTRACT_EVIDENCE_V1_FILE, METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE,
    METRIC_CONTRACT_SUMMARY_V34_FILE,
};
use ghost_core::checkpoint::MaterializedFeatureSet;
use ghost_core::metric_contracts::{
    BurnInContractV1, MetricContractDecisionSummaryV1, MetricContractEvidenceTransportV1,
    MetricContractProjectionWireV1SchemaManifest,
    METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3,
    METRIC_CONTRACT_WIRE_V1_MAPPING_TABLE_COUNT, METRIC_CONTRACT_WIRE_V1_TUPLE_TABLE_COUNT,
    PR2C_COMPARATOR_P99_MAX_US, PR2C_FULL_BUILD_AND_SERIALIZE_P99_MAX_US,
    PR2C_PROJECTION_BUILD_AND_VALIDATE_P99_MAX_US, PR2C_SERIALIZE_P99_MAX_US,
};
use ghost_launcher::components::gatekeeper_policy::{
    build_assessment_from_features, evaluate_policy_from_assessment, PolicyEvaluationContext,
};
use sha2::Digest;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn ordered_wire_v1_codebook_has_all_18_layouts_and_28_mapping_tables() {
    let manifest = MetricContractProjectionWireV1SchemaManifest::current();
    assert!(manifest.has_closed_table_counts());
    assert_eq!(
        manifest.tuple_layouts.len(),
        METRIC_CONTRACT_WIRE_V1_TUPLE_TABLE_COUNT
    );
    assert_eq!(
        manifest.mapping_tables.len(),
        METRIC_CONTRACT_WIRE_V1_MAPPING_TABLE_COUNT
    );
    assert_eq!(
        manifest.blake3_hex().unwrap(),
        METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3
    );
    let checked_in: MetricContractProjectionWireV1SchemaManifest = serde_json::from_str(
        include_str!("../../reports/metric_contracts/metric_contract_wire_v1_schema_manifest.json"),
    )
    .unwrap();
    assert_eq!(checked_in, manifest);
}

#[test]
fn ordered_wire_v1_codebook_hash_is_sensitive_to_every_schema_dimension() {
    let manifest = MetricContractProjectionWireV1SchemaManifest::current();
    let frozen = manifest.blake3_hex().unwrap();
    let mut mutations = Vec::new();
    let mut changed = manifest.clone();
    changed.wire_version += 1;
    mutations.push(changed);
    let mut changed = manifest.clone();
    changed.tuple_layouts.swap(0, 1);
    mutations.push(changed);
    let mut changed = manifest.clone();
    changed.tuple_layouts[0].name.push_str("_changed");
    mutations.push(changed);
    let mut changed = manifest.clone();
    changed.tuple_layouts[0].entries[0].position += 1;
    mutations.push(changed);
    let mut changed = manifest.clone();
    changed.mapping_tables.swap(0, 1);
    mutations.push(changed);
    let mut changed = manifest.clone();
    changed.mapping_tables[0].entries[0].code += 1;
    mutations.push(changed);
    let mut changed = manifest;
    changed.mapping_tables[0].entries[0]
        .domain_value
        .push_str("_changed");
    mutations.push(changed);
    for changed in mutations {
        assert_ne!(changed.blake3_hex().unwrap(), frozen);
    }
}

#[test]
fn one_pass_projection_hash_proof_matches_the_public_validated_hash_contract() {
    let frozen = frozen_inputs_fixture();
    let timed = frozen.build_timed();
    let snapshot = timed.snapshot();
    let context = ghost_core::metric_contracts::MetricDecisionProjectionBuildContextV1 {
        rollout_mode: ghost_core::metric_contracts::MetricContractRolloutMode::Legacy,
        profile: frozen.profile(),
        effective_config: frozen.effective_config(),
        source_cutoff: snapshot
            .compact_projection
            .fee_topology_diversity_index
            .legacy_value
            .source_cutoff
            .clone(),
    };
    assert_eq!(
        timed.validated_projection_hash(),
        &snapshot
            .compact_projection
            .validated_canonical_hash(&context)
            .unwrap()
    );
}

#[test]
fn burn_in_contract_v1_is_frozen_hashed_and_fail_closed() {
    let contract: BurnInContractV1 = serde_json::from_str(include_str!(
        "../../reports/metric_contracts/BURN_IN_CONTRACT_V1.json"
    ))
    .unwrap();
    contract.validate_hash().unwrap();
    assert_eq!(contract.payload.minimum_non_overlapping_runs, 3);
    assert_eq!(contract.payload.minimum_run_duration_ms, 3_600_000);
    assert_eq!(contract.payload.minimum_utc_4h_buckets, 2);
    assert_eq!(
        contract.payload.owner_approval_identity,
        "github:smahacfel:authorized-pr2c-task:2026-07-13"
    );
    assert_eq!(
        contract.contract_canonical_hash.as_str(),
        "40872b8c1ab8fcd8ecb4b1612e35fcf9dc157cbb1109546c7490c7d006f00ffd"
    );

    let mut value = serde_json::to_value(&contract).unwrap();
    value["payload"]["minimum_unique_decisions"] = serde_json::json!(1);
    assert!(serde_json::from_value::<BurnInContractV1>(value).is_err());
}

#[test]
fn build_provenance_is_git_derived_clean_sensitive_and_not_environment_overrideable() {
    let build_script = include_str!("../../ghost-brain/build.rs");
    assert!(build_script.contains("git_output(&[\"rev-parse\", \"HEAD\"])"));
    assert!(build_script.contains("status"));
    assert!(build_script.contains("--untracked-files=all"));
    assert!(build_script.contains("unwrap_or(false)"));
    assert!(build_script.contains("GIT_WORKTREE_CLEAN"));
    assert!(!build_script.contains("std::env::var(\"GIT_COMMIT\")"));
    assert!(!build_script.contains("rerun-if-env-changed=GIT_COMMIT"));
}

#[test]
fn v34_summary_has_exact_frozen_compact_field_set() {
    let pair = paired_fixture("run-a", "join-a");
    let value = serde_json::to_value(&pair.decision_v34).unwrap();
    let actual = value
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected = [
        "metric_contract_schema_version",
        "rollout_mode",
        "profile_id",
        "profile_hash",
        "metric_contract_effective_config_hash",
        "evidence_record_id",
        "evidence_sha256",
        "evidence_schema_version",
        "authoritative_contracts",
        "comparator_contracts",
        "equivalence_deltas",
        "counterfactual_delta_present",
        "comparator_elapsed_us",
        "metric_contract_serialize_us",
        "measured_fields_mask",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    let roundtrip: MetricContractDecisionSummaryV1 = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, pair.decision_v34);
}

#[tokio::test]
async fn paired_writer_emits_summary_evidence_and_rotation_manifest_from_one_command() {
    let temp = tempfile::tempdir().unwrap();
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    config.rotation_max_bytes = 16 * 1024 * 1024;
    let mut writer = MetricContractPairedWriterV1::open(config, Arc::clone(&stats))
        .await
        .unwrap();
    let pair = paired_fixture("run-a", "join-a");
    assert_eq!(pair.evidence.writer_timestamp_ms, 0);
    writer.write_pair(pair).await.unwrap();

    let summary =
        std::fs::read_to_string(temp.path().join(METRIC_CONTRACT_SUMMARY_V34_FILE)).unwrap();
    let evidence =
        std::fs::read_to_string(temp.path().join(METRIC_CONTRACT_EVIDENCE_V1_FILE)).unwrap();
    assert_eq!(summary.lines().count(), 1);
    assert_eq!(evidence.lines().count(), 1);
    serde_json::from_str::<MetricContractDecisionSummaryV1>(summary.trim()).unwrap();
    let persisted_evidence =
        serde_json::from_str::<MetricContractEvidenceTransportV1>(evidence.trim()).unwrap();
    assert!(persisted_evidence.writer_timestamp_ms > 0);
    assert_eq!(persisted_evidence.rotation_part_index, 0);

    let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
        &std::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.summary_parts.len(), 1);
    assert_eq!(manifest.evidence_parts.len(), 1);
    assert_eq!(manifest.summary_parts[0].row_count, 1);
    assert_eq!(manifest.evidence_parts[0].row_count, 1);
    assert_eq!(manifest.writer_stats.paired_commands_total, 1);
    assert_eq!(manifest.writer_stats.orphan_summary_total, 0);
    assert_eq!(manifest.writer_stats.orphan_evidence_total, 0);
}

#[tokio::test]
async fn paired_writer_rotates_both_streams_with_one_contiguous_part_index() {
    let temp = tempfile::tempdir().unwrap();
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    config.rotation_max_bytes = 1;
    let mut writer = MetricContractPairedWriterV1::open(config, stats)
        .await
        .unwrap();
    writer
        .write_pair(paired_fixture("run-a", "join-a"))
        .await
        .unwrap();
    writer
        .write_pair(paired_fixture("run-a", "join-b"))
        .await
        .unwrap();
    let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
        &std::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest
            .summary_parts
            .iter()
            .map(|part| part.part_index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        manifest
            .evidence_parts
            .iter()
            .map(|part| part.part_index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn evidence_transport_hash_mismatch_and_unknown_fields_fail_deserialization() {
    let pair = paired_fixture("run-a", "join-a");
    let mut value = serde_json::to_value(&pair.evidence).unwrap();
    value["evidence_sha256"] = serde_json::Value::String("0".repeat(64));
    assert!(serde_json::from_value::<MetricContractEvidenceTransportV1>(value).is_err());

    let mut value = serde_json::to_value(&pair.evidence).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_string(), serde_json::json!(true));
    assert!(serde_json::from_value::<MetricContractEvidenceTransportV1>(value).is_err());

    let pair = paired_fixture("run-a", "join-a");
    let mut value = serde_json::to_value(&pair.decision_v34).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_string(), serde_json::json!(true));
    assert!(serde_json::from_value::<MetricContractDecisionSummaryV1>(value).is_err());

    let pair = paired_fixture("run-a", "join-a");
    let mut value = serde_json::to_value(&pair.evidence).unwrap();
    value["payload"]["evidence_schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<MetricContractEvidenceTransportV1>(value).is_err());

    let pair = paired_fixture("run-a", "join-a");
    let mut value = serde_json::to_value(&pair.evidence).unwrap();
    value["payload"]["contracts"]
        .as_object_mut()
        .unwrap()
        .remove("recent_buy_sell");
    assert!(serde_json::from_value::<MetricContractEvidenceTransportV1>(value).is_err());
}

#[tokio::test]
async fn paired_writer_enospc_orphans_are_fail_closed_and_counted_in_both_directions() {
    for (
        fault,
        expected_summary_rows,
        expected_evidence_rows,
        expected_orphan_summary,
        expected_orphan_evidence,
    ) in [
        (
            MetricContractWriterFaultInjectionV1::SummaryEnospc,
            0,
            0,
            0,
            0,
        ),
        (
            MetricContractWriterFaultInjectionV1::EvidenceEnospcAfterSummary,
            1,
            0,
            1,
            0,
        ),
        (
            MetricContractWriterFaultInjectionV1::SummaryEnospcAfterEvidence,
            0,
            1,
            0,
            1,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
        let mut config = MetricContractPairedWriterConfigV1::new(
            temp.path().to_path_buf(),
            "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
        );
        config.fault_injection = Some(fault);
        let mut writer = MetricContractPairedWriterV1::open(config, Arc::clone(&stats))
            .await
            .unwrap();
        let error = writer
            .write_pair(paired_fixture("run-a", "join-a"))
            .await
            .unwrap_err();
        assert!(error
            .chain()
            .filter_map(|source| source.downcast_ref::<std::io::Error>())
            .any(|source| source.raw_os_error() == Some(28)));
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.missing_pair_total, 1);
        assert_eq!(snapshot.orphan_summary_total, expected_orphan_summary);
        assert_eq!(snapshot.orphan_evidence_total, expected_orphan_evidence);
        assert_eq!(
            snapshot.summary_write_failures_total + snapshot.evidence_write_failures_total,
            1
        );
        writer.finalize().await.unwrap();
        let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
            &tokio::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.writer_stats.missing_pair_total, 1);
        assert_eq!(
            manifest.writer_stats.orphan_summary_total,
            expected_orphan_summary
        );
        assert_eq!(
            manifest.writer_stats.orphan_evidence_total,
            expected_orphan_evidence
        );
        assert_eq!(manifest.summary_parts.len(), 1);
        assert_eq!(manifest.evidence_parts.len(), 1);
        assert_eq!(manifest.summary_parts[0].row_count, expected_summary_rows);
        assert_eq!(manifest.evidence_parts[0].row_count, expected_evidence_rows);
    }
}

#[tokio::test]
async fn paired_writer_records_actual_mid_row_short_writes_as_durable_truncated_parts() {
    const PREFIX_BYTES: usize = 31;
    for (fault, truncated_stream) in [
        (
            MetricContractWriterFaultInjectionV1::SummaryShortWriteAfterBytes(PREFIX_BYTES),
            "summary",
        ),
        (
            MetricContractWriterFaultInjectionV1::EvidenceShortWriteAfterSummaryBytes(PREFIX_BYTES),
            "evidence",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
        let mut config = MetricContractPairedWriterConfigV1::new(
            temp.path().to_path_buf(),
            "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
        );
        config.fault_injection = Some(fault);
        let mut writer = MetricContractPairedWriterV1::open(config, stats)
            .await
            .unwrap();
        writer
            .write_pair(paired_fixture("short-write-run", "short-write-join"))
            .await
            .unwrap_err();
        writer.finalize().await.unwrap();

        let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
            &tokio::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE))
                .await
                .unwrap(),
        )
        .unwrap();
        let (part, path) = if truncated_stream == "summary" {
            (
                &manifest.summary_parts[0],
                temp.path().join(METRIC_CONTRACT_SUMMARY_V34_FILE),
            )
        } else {
            (
                &manifest.evidence_parts[0],
                temp.path().join(METRIC_CONTRACT_EVIDENCE_V1_FILE),
            )
        };
        let persisted_prefix = tokio::fs::read(path).await.unwrap();
        assert_eq!(persisted_prefix.len(), PREFIX_BYTES);
        assert!(!persisted_prefix.ends_with(b"\n"));
        assert_eq!(part.byte_count, PREFIX_BYTES as u64);
        assert_eq!(part.row_count, 0);
        assert_eq!(
            part.part_sha256.as_str(),
            format!("{:x}", sha2::Sha256::digest(&persisted_prefix))
        );
        assert_eq!(manifest.writer_stats.missing_pair_total, 1);
    }
}

#[tokio::test]
async fn final_manifest_failure_is_counted_and_cannot_claim_an_immutable_run() {
    let temp = tempfile::tempdir().unwrap();
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    config.fault_injection = Some(MetricContractWriterFaultInjectionV1::FinalManifestEnospc);
    let mut writer = MetricContractPairedWriterV1::open(config, Arc::clone(&stats))
        .await
        .unwrap();
    writer
        .write_pair(paired_fixture(
            "finalize-failure-run",
            "finalize-failure-join",
        ))
        .await
        .unwrap();
    writer.finalize().await.unwrap_err();

    let snapshot = stats.snapshot();
    assert_eq!(snapshot.manifest_write_failures_total, 1);
    assert_eq!(snapshot.finalization_failures_total, 1);
    let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
        &tokio::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(!manifest.writer_finalized);
    assert_eq!(manifest.writer_stats.manifest_write_failures_total, 1);
    assert_eq!(manifest.writer_stats.finalization_failures_total, 1);
}

#[tokio::test]
async fn paired_queue_reports_disabled_channel_close_and_bounded_high_water() {
    let pair = paired_fixture("run-a", "join-a");
    let disabled = DecisionLogger::new(DecisionLoggerConfig {
        enabled: false,
        ..DecisionLoggerConfig::default()
    });
    assert!(matches!(
        disabled.log_metric_contract_pair(pair.clone()).await,
        Err(MetricContractEnqueueErrorV1::WriterDisabled)
    ));
    assert_eq!(
        disabled
            .metric_contract_writer_stats()
            .writer_disabled_total,
        1
    );

    let temp = tempfile::tempdir().unwrap();
    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        channel_buffer_size: 1,
        enabled: true,
        ..DecisionLoggerConfig::default()
    });
    logger.log_metric_contract_pair(pair.clone()).await.unwrap();
    logger.shutdown().await;
    let mut closed = false;
    for _ in 0..100 {
        if matches!(
            logger.log_metric_contract_pair(pair.clone()).await,
            Err(MetricContractEnqueueErrorV1::ChannelClosed)
        ) {
            closed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(
        closed,
        "logger receiver did not close after bounded shutdown wait"
    );
    let stats = logger.metric_contract_writer_stats();
    assert!(stats.writer_queue_high_water <= 1);
    assert!(stats.queue_send_failures_total >= 1);
    assert!(stats.queue_dropped_rows_total >= 1);
}

#[tokio::test]
async fn logger_enqueue_wait_and_queue_high_water_pass_the_frozen_resource_gate() {
    const SAMPLE_COUNT: u64 = 128;
    const QUEUE_CAPACITY: usize = 1_000;
    let temp = tempfile::tempdir().unwrap();
    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        channel_buffer_size: QUEUE_CAPACITY,
        enabled: true,
        ..DecisionLoggerConfig::default()
    });
    let pair = paired_fixture("enqueue-resource-run", "enqueue-resource-join");
    for _ in 0..SAMPLE_COUNT {
        logger.log_metric_contract_pair(pair.clone()).await.unwrap();
    }
    let enqueued = logger.metric_contract_writer_stats();
    assert_eq!(enqueued.logger_enqueue_wait_us.sample_count, SAMPLE_COUNT);
    let enqueue_p99_us = enqueued
        .logger_enqueue_wait_us
        .percentile_upper_bound_us(99)
        .unwrap();
    eprintln!(
        "PR2C bounded queue resource gate: logger_enqueue_wait_us_p99={enqueue_p99_us} writer_queue_high_water={} queue_capacity={QUEUE_CAPACITY}",
        enqueued.writer_queue_high_water
    );
    assert!(enqueue_p99_us <= 1_000);
    assert!(enqueued.writer_queue_high_water < (QUEUE_CAPACITY as u64 * 8 / 10));

    logger.shutdown().await;
    for _ in 0..1_000 {
        if logger
            .metric_contract_writer_stats()
            .evidence_rows_written_total
            == SAMPLE_COUNT
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let manifest_path = temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut finalized = false;
    for _ in 0..1_000 {
        finalized = tokio::fs::read(&manifest_path)
            .await
            .ok()
            .and_then(|bytes| {
                serde_json::from_slice::<MetricContractRotationManifestV1>(&bytes).ok()
            })
            .is_some_and(|manifest| manifest.writer_finalized);
        if finalized {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(finalized, "paired writer did not finalize its run manifest");
    let completed = logger.metric_contract_writer_stats();
    assert_eq!(completed.summary_rows_written_total, SAMPLE_COUNT);
    assert_eq!(completed.evidence_rows_written_total, SAMPLE_COUNT);
    assert_eq!(completed.queue_dropped_rows_total, 0);
    assert_eq!(completed.summary_write_failures_total, 0);
    assert_eq!(completed.evidence_write_failures_total, 0);
}

#[tokio::test]
async fn pr2c_release_resource_harness_reports_full_path_percentiles() {
    fn percentiles<T: Copy + Ord>(mut values: Vec<T>) -> (T, T, T) {
        values.sort_unstable();
        (
            values[values.len() * 50 / 100],
            values[values.len() * 95 / 100],
            values[values.len() * 99 / 100],
        )
    }

    let (warmup_samples, measured_samples) = if cfg!(debug_assertions) {
        (4, 16)
    } else {
        (16, 128)
    };
    let frozen = frozen_inputs_fixture();
    let policy = common::equal_policy();
    let comparator_config = GatekeeperV2Config::default();
    let comparator_assessment = build_assessment_from_features(
        MaterializedFeatureSet::default(),
        &comparator_config,
        PolicyEvaluationContext::default(),
    );
    let expected_comparator_decision =
        evaluate_policy_from_assessment(&comparator_assessment, &comparator_config);

    // Warm exactly the same producer -> full evidence -> projection -> pair
    // construction path. Warmup samples are deliberately excluded from the
    // measured writer histogram.
    for index in 0..warmup_samples {
        let timed = frozen.build_timed();
        let pair = ghost_launcher::metric_contracts::build_pr2c_timed_paired_record_from_validated_snapshot_v1(
            &timed,
            &ghost_launcher::metric_contracts::Pr2cDecisionRecordContextV1 {
                record_identity: ghost_core::metric_contracts::MetricEvidenceRecordIdentityV1::try_new(
                    "resource-warmup",
                    format!("join-{index}"),
                    "legacy_live",
                )
                .unwrap(),
                stable_event_identity: None,
                rollout_mode: ghost_core::metric_contracts::MetricContractRolloutMode::Legacy,
                profile: frozen.profile(),
                effective_config: frozen.effective_config(),
                authoritative_policy: &policy,
                comparator_policy: &policy,
                comparator_evaluable: true,
                comparator_elapsed_us: 0,
                metric_contract_serialize_us: 0,
                metric_contract_build_and_serialize_us: 0,
                projection_build_and_validate_us: 0,
                gatekeeper_config_hash: common::TEST_GATEKEEPER_CONFIG_HASH,
                brain_config_hash: Some(common::TEST_BRAIN_CONFIG_HASH),
            },
        )
        .unwrap();
        serde_json::to_vec(&pair.decision_v34).unwrap();
        serde_json::to_vec(&pair.evidence).unwrap();
    }

    let temp = tempfile::tempdir().unwrap();
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    let config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    let mut writer = MetricContractPairedWriterV1::open(config, Arc::clone(&stats))
        .await
        .unwrap();
    let mut comparator = Vec::with_capacity(measured_samples);
    let mut wire_bytes = Vec::with_capacity(measured_samples);
    let mut complete_snapshot_build = Vec::with_capacity(measured_samples);
    let mut context_validation = Vec::with_capacity(measured_samples);
    let mut evidence_build = Vec::with_capacity(measured_samples);
    let mut evidence_validation = Vec::with_capacity(measured_samples);
    let mut projection_build = Vec::with_capacity(measured_samples);
    let mut pair_construction = Vec::with_capacity(measured_samples);
    for index in 0..measured_samples {
        // This is the canonical PR2B timer: every family producer is invoked
        // once, then full evidence, semantic validation, compact projection,
        // Wire hard gate and the semantic projection hash are built.
        let rebuilt = frozen.build_timed();
        complete_snapshot_build.push(u128::from(
            rebuilt.timings().metric_contract_build_and_validate_us,
        ));
        context_validation.push(u128::from(rebuilt.timings().context_validation_us));
        evidence_build.push(u128::from(rebuilt.timings().evidence_build_us));
        evidence_validation.push(u128::from(rebuilt.timings().evidence_validation_us));
        projection_build.push(u128::from(
            rebuilt.timings().projection_build_and_validate_us,
        ));
        let started = Instant::now();
        let comparator_decision =
            evaluate_policy_from_assessment(&comparator_assessment, &comparator_config);
        assert_eq!(
            comparator_decision.verdict_buy,
            expected_comparator_decision.verdict_buy
        );
        assert_eq!(
            comparator_decision.reason_code,
            expected_comparator_decision.reason_code
        );
        assert!(policy.compare(&policy).is_zero_drift());
        let comparator_us = u32::try_from(started.elapsed().as_micros()).unwrap();
        comparator.push(u128::from(comparator_us));

        let join_key = format!("join-{index}");
        let pair = ghost_launcher::metric_contracts::build_pr2c_timed_paired_record_from_validated_snapshot_v1(
            &rebuilt,
            &ghost_launcher::metric_contracts::Pr2cDecisionRecordContextV1 {
                record_identity: ghost_core::metric_contracts::MetricEvidenceRecordIdentityV1::try_new(
                    "resource-run",
                    &join_key,
                    "legacy_live",
                )
                .unwrap(),
                stable_event_identity: Some(
                    ghost_core::metric_contracts::StableEventIdentityV1::try_from_signature(
                        "resource_harness",
                        format!("sig-{join_key}"),
                    )
                    .unwrap(),
                ),
                rollout_mode: ghost_core::metric_contracts::MetricContractRolloutMode::Legacy,
                profile: frozen.profile(),
                effective_config: frozen.effective_config(),
                authoritative_policy: &policy,
                comparator_policy: &policy,
                comparator_evaluable: true,
                comparator_elapsed_us: comparator_us,
                metric_contract_serialize_us: 0,
                metric_contract_build_and_serialize_us: 0,
                projection_build_and_validate_us: 0,
                gatekeeper_config_hash: common::TEST_GATEKEEPER_CONFIG_HASH,
                brain_config_hash: Some(common::TEST_BRAIN_CONFIG_HASH),
            },
        )
        .unwrap();
        pair_construction.push(u128::from(
            pair.metric_contract_build_and_serialize_us
                .checked_sub(rebuilt.timings().metric_contract_build_and_validate_us)
                .unwrap(),
        ));
        wire_bytes.push(
            pair.decision_time_projection
                .authoritative_serialized_size_bytes()
                .unwrap(),
        );
        writer.write_pair(pair).await.unwrap();
    }
    writer.finalize().await.unwrap();
    let writer_stats = stats.snapshot();
    let build_p50 = writer_stats
        .metric_contract_build_and_serialize_us
        .percentile_upper_bound_us(50)
        .unwrap();
    let build_p95 = writer_stats
        .metric_contract_build_and_serialize_us
        .percentile_upper_bound_us(95)
        .unwrap();
    let build_p99 = writer_stats
        .metric_contract_build_and_serialize_us
        .percentile_upper_bound_us(99)
        .unwrap();
    let (comparator_p50, comparator_p95, comparator_p99) = percentiles(comparator);
    let (snapshot_p50, snapshot_p95, snapshot_p99) = percentiles(complete_snapshot_build);
    let (context_p50, context_p95, context_p99) = percentiles(context_validation);
    let (evidence_build_p50, evidence_build_p95, evidence_build_p99) = percentiles(evidence_build);
    let (evidence_validation_p50, evidence_validation_p95, evidence_validation_p99) =
        percentiles(evidence_validation);
    let (projection_p50, projection_p95, projection_p99) = percentiles(projection_build);
    let (pair_p50, pair_p95, pair_p99) = percentiles(pair_construction);
    let summary_bytes = std::fs::read(temp.path().join(METRIC_CONTRACT_SUMMARY_V34_FILE)).unwrap();
    let evidence_bytes = std::fs::read(temp.path().join(METRIC_CONTRACT_EVIDENCE_V1_FILE)).unwrap();
    let mut v34_bytes = summary_bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| line.len())
        .collect::<Vec<_>>();
    let mut sidecar_bytes = evidence_bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| line.len())
        .collect::<Vec<_>>();
    let serialize_samples = summary_bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_slice::<MetricContractDecisionSummaryV1>(line)
                .unwrap()
                .metric_contract_serialize_us
        })
        .collect::<Vec<_>>();
    let (serialize_p50, serialize_p95, serialize_p99) = percentiles(serialize_samples);
    wire_bytes.sort_unstable();
    sidecar_bytes.sort_unstable();
    v34_bytes.sort_unstable();
    let wire_p95 = wire_bytes[wire_bytes.len() * 95 / 100];
    let wire_max = *wire_bytes.last().unwrap();
    let sidecar_p95 = sidecar_bytes[sidecar_bytes.len() * 95 / 100];
    let sidecar_p99 = sidecar_bytes[sidecar_bytes.len() * 99 / 100];
    let v34_p95 = v34_bytes[v34_bytes.len() * 95 / 100];
    eprintln!(
        "PR2C release resource harness: metric_contract_build_and_serialize_us_p50={build_p50} metric_contract_build_and_serialize_us_p95={build_p95} metric_contract_build_and_serialize_us_p99={build_p99} complete_snapshot_build_validate_us_p50={snapshot_p50} complete_snapshot_build_validate_us_p95={snapshot_p95} complete_snapshot_build_validate_us_p99={snapshot_p99} context_validation_us_p50={context_p50} context_validation_us_p95={context_p95} context_validation_us_p99={context_p99} evidence_build_us_p50={evidence_build_p50} evidence_build_us_p95={evidence_build_p95} evidence_build_us_p99={evidence_build_p99} evidence_validation_us_p50={evidence_validation_p50} evidence_validation_us_p95={evidence_validation_p95} evidence_validation_us_p99={evidence_validation_p99} projection_build_validate_us_p50={projection_p50} projection_build_validate_us_p95={projection_p95} projection_build_validate_us_p99={projection_p99} terminal_pair_construction_us_p50={pair_p50} terminal_pair_construction_us_p95={pair_p95} terminal_pair_construction_us_p99={pair_p99} metric_contract_serialize_us_p50={serialize_p50} metric_contract_serialize_us_p95={serialize_p95} metric_contract_serialize_us_p99={serialize_p99} comparator_elapsed_us_p50={comparator_p50} comparator_elapsed_us_p95={comparator_p95} comparator_elapsed_us_p99={comparator_p99} projection_wire_json_bytes_p95={wire_p95} projection_wire_json_bytes_max={wire_max} sidecar_json_bytes_p95={sidecar_p95} sidecar_json_bytes_p99={sidecar_p99} v34_json_bytes_p95={v34_p95}"
    );
    assert_eq!(
        writer_stats.summary_rows_written_total,
        measured_samples as u64
    );
    assert_eq!(
        writer_stats.evidence_rows_written_total,
        measured_samples as u64
    );
    assert!(wire_p95 <= 12 * 1024);
    assert!(wire_max <= 16 * 1024);
    assert!(sidecar_p95 <= 24 * 1024);
    assert!(sidecar_p99 <= 48 * 1024);
    if !cfg!(debug_assertions) {
        assert!(build_p99 <= u64::from(PR2C_FULL_BUILD_AND_SERIALIZE_P99_MAX_US));
        assert!(projection_p99 <= u128::from(PR2C_PROJECTION_BUILD_AND_VALIDATE_P99_MAX_US));
        assert!(serialize_p99 <= PR2C_SERIALIZE_P99_MAX_US);
        assert!(comparator_p99 <= u128::from(PR2C_COMPARATOR_P99_MAX_US));
    }
}
