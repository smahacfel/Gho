#[path = "common/metric_contracts_pr2c.rs"]
mod common;

use common::{
    current_v33_unrouted_fixture, current_v33_unrouted_log, frozen_inputs_fixture, paired_fixture,
};
use ghost_brain::oracle::decision_logger::MetricContractEnqueueErrorV1;
use ghost_brain::oracle::{
    DecisionLogger, DecisionLoggerConfig, MetricContractLatencyHistogramErrorV1,
    MetricContractLatencyHistogramSnapshotV1, MetricContractPairedWriterConfigV1,
    MetricContractPairedWriterStatsV1, MetricContractPairedWriterV1,
    MetricContractRotationManifestV1, MetricContractWriterFaultInjectionV1,
    GATEKEEPER_DECISIONS_JSONL, GATEKEEPER_VERSION, LEGACY_GATEKEEPER_VERSION,
    METRIC_CONTRACT_COMPLETION_PROOF_V1_FILE, METRIC_CONTRACT_EVIDENCE_V1_FILE,
    METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V1, METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE,
    METRIC_CONTRACT_SUMMARY_V34_FILE,
};
use ghost_core::metric_contracts::{
    MetricContractAuditTerminalClassV1, MetricContractDecisionSummaryV1,
    MetricContractEvidenceTransportV1, MetricContractProjectionWireV1SchemaManifest,
    METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3,
    METRIC_CONTRACT_WIRE_V1_MAPPING_TABLE_COUNT, METRIC_CONTRACT_WIRE_V1_TUPLE_TABLE_COUNT,
};
use ghost_launcher::components::gatekeeper_policy::evaluate_policy_from_assessment;
use ghost_launcher::metric_contracts::{
    audit_pr2c_single_run_v1, pr2c_policy_equivalence_snapshot_v1,
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
fn prospective_burn_in_contract_v2_is_withdrawn() {
    let core = include_str!("../../ghost-core/src/metric_contracts/pr2c.rs");
    let audit = include_str!("../src/metric_contracts/pr2c_audit.rs");
    assert!(!core.contains("BURN_IN_CONTRACT_V2_CANONICAL_HASH"));
    assert!(!core.contains("BurnInContractV1"));
    assert!(!audit.contains("audit_pr2c_bundle_against_burn_in_contract_v2"));
    assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../reports/metric_contracts/BURN_IN_CONTRACT_V2.json")
        .exists());
}

#[test]
fn canonical_hash_uses_the_reference_jcs_implementation_without_benchmark_shortcuts() {
    let source = include_str!("../../ghost-core/src/metric_contracts/canonical_hash.rs");
    let cargo = include_str!("../../ghost-core/Cargo.toml");
    assert!(source.contains("serde_json_canonicalizer::to_vec(payload)"));
    assert!(!source.contains("CanonicalJcsSerializerV1"));
    assert!(!source.contains("ryu_js"));
    assert!(!cargo
        .lines()
        .any(|line| line.trim_start().starts_with("ryu-js")));

    let writer = include_str!("../../ghost-brain/src/oracle/metric_contract_writer.rs");
    assert!(!writer.contains("TELEMETRY_FIELD_WITH_SENTINEL"));
    assert!(!writer.contains("SENTINEL_BYTES"));
}

#[test]
fn resource_histogram_validation_is_closed_over_codebook_counts_samples_and_max() {
    let valid = MetricContractLatencyHistogramSnapshotV1 {
        bucket_upper_bounds_us: METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V1,
        bucket_counts: [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sample_count: 1,
        max_us: 1,
    };
    valid.validate(1).unwrap();

    let mut wrong_bounds = valid.clone();
    wrong_bounds.bucket_upper_bounds_us[0] = 3;
    assert_eq!(
        wrong_bounds.validate(1),
        Err(MetricContractLatencyHistogramErrorV1::BucketCodebookMismatch)
    );

    let mut wrong_bucket_sum = valid.clone();
    wrong_bucket_sum.bucket_counts[0] = 2;
    assert_eq!(
        wrong_bucket_sum.validate(1),
        Err(MetricContractLatencyHistogramErrorV1::BucketSumMismatch)
    );

    let mut wrong_sample_count = valid.clone();
    wrong_sample_count.sample_count = 2;
    wrong_sample_count.bucket_counts[0] = 2;
    assert_eq!(
        wrong_sample_count.validate(1),
        Err(MetricContractLatencyHistogramErrorV1::SampleCountMismatch)
    );

    let mut wrong_max = valid.clone();
    wrong_max.max_us = 3_000;
    assert_eq!(
        wrong_max.validate(1),
        Err(MetricContractLatencyHistogramErrorV1::MaxBucketMismatch)
    );

    let missing_sample = MetricContractLatencyHistogramSnapshotV1::default();
    assert_eq!(
        missing_sample.validate(1),
        Err(MetricContractLatencyHistogramErrorV1::SampleCountMismatch)
    );
}

#[tokio::test]
async fn unrouted_terminal_v33_uses_one_logger_route_for_v33_pair_and_single_run_audit() {
    let temp = tempfile::tempdir().unwrap();
    let full_path_started = Instant::now();
    let frozen = frozen_inputs_fixture();
    let timed = frozen.build_timed_from(full_path_started);
    let mut terminal = current_v33_unrouted_fixture(&timed.snapshot().compact_projection);

    // `GatekeeperAssessment::to_buy_log()` intentionally does not own file
    // routing. Mirror the real runtime's observation-identity enrichment by
    // adding only the join key; DecisionLogger must supply run, plane and
    // config provenance before the PR2C snapshot is consumed.
    assert!(terminal.log.run_id.is_none());
    assert!(terminal.log.join_key.is_none());
    assert!(terminal.log.decision_plane.is_none());
    assert!(terminal.log.config_hash.is_none());
    assert!(terminal.log.brain_config_hash.is_none());
    terminal.log.join_key = Some("terminal-route-join".to_string());
    terminal.log.v25_shadow_verdict_type = Some("REJECT_LOW_CONFIDENCE".to_string());
    terminal.log.v25_shadow_reason_chain = Some("shadow-only regression".to_string());

    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        gatekeeper_rollout_profile: "profile-a".to_string(),
        gatekeeper_config_hash: common::TEST_GATEKEEPER_CONFIG_HASH.to_string(),
        gatekeeper_run_id: Some("terminal-route-run".to_string()),
        gatekeeper_session_id: Some("terminal-route-session".to_string()),
        brain_config_path: Some("ghost-brain/config/ghost_brain_config.toml".to_string()),
        brain_config_hash: Some(common::TEST_BRAIN_CONFIG_HASH.to_string()),
        channel_buffer_size: 16,
        metric_contract_pr2c_enabled: true,
        enabled: true,
    });
    let routed_context = logger.pr2c_legacy_live_context(&terminal.log).unwrap();
    assert_eq!(
        routed_context.record_identity().run_id,
        "terminal-route-run"
    );
    assert_eq!(
        routed_context.record_identity().join_key,
        "terminal-route-join"
    );
    assert_eq!(
        routed_context.record_identity().decision_plane,
        "legacy_live"
    );

    let authoritative = pr2c_policy_equivalence_snapshot_v1(
        &terminal.assessment,
        terminal.assessment.decision.as_ref(),
    );
    let comparator_decision =
        evaluate_policy_from_assessment(&terminal.assessment, &terminal.config);
    let comparator =
        pr2c_policy_equivalence_snapshot_v1(&terminal.assessment, Some(&comparator_decision));
    let pair = ghost_launcher::metric_contracts::build_pr2c_timed_paired_record_v1(
        &timed,
        &ghost_launcher::metric_contracts::Pr2cDecisionRecordContextV1 {
            record_identity: routed_context.record_identity().clone(),
            stable_event_identity: None,
            rollout_mode: ghost_core::metric_contracts::MetricContractRolloutMode::Legacy,
            profile: frozen.profile(),
            effective_config: frozen.effective_config(),
            authoritative_policy: &authoritative,
            comparator_policy: &comparator,
            comparator_evaluable: terminal.assessment.decision.is_some(),
            comparator_elapsed_us: 0,
            metric_contract_serialize_us: 0,
            metric_contract_build_and_serialize_us: 0,
            projection_build_and_validate_us: 0,
            gatekeeper_config_hash: routed_context.gatekeeper_config_hash().as_str(),
            brain_config_hash: Some(routed_context.brain_config_hash().as_str()),
        },
    )
    .unwrap();

    logger.log_gatekeeper_buy_decision(terminal.log).await;
    logger.log_metric_contract_pair(pair).unwrap();
    logger.shutdown().await.unwrap();

    let legacy_v33_path = temp
        .path()
        .join("profile-a")
        .join(LEGACY_GATEKEEPER_VERSION)
        .join("legacy_live")
        .join(common::TEST_GATEKEEPER_CONFIG_HASH)
        .join(GATEKEEPER_DECISIONS_JSONL);
    let legacy_rows = std::fs::read_to_string(&legacy_v33_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<ghost_brain::oracle::GatekeeperBuyLog>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(legacy_rows.len(), 1);

    let summaries = std::fs::read_to_string(temp.path().join(METRIC_CONTRACT_SUMMARY_V34_FILE))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<MetricContractDecisionSummaryV1>(line).unwrap())
        .collect::<Vec<_>>();
    let evidence = std::fs::read_to_string(temp.path().join(METRIC_CONTRACT_EVIDENCE_V1_FILE))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<MetricContractEvidenceTransportV1>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(summaries.len(), 1);
    assert_eq!(evidence.len(), 1);
    let expected_identity = routed_context.record_identity();
    assert_eq!(summaries[0].evidence_record_id, *expected_identity);
    assert_eq!(evidence[0].payload.record_identity, *expected_identity);
    assert_eq!(
        legacy_rows[0].run_id.as_deref(),
        Some(expected_identity.run_id.as_str())
    );
    assert_eq!(
        legacy_rows[0].join_key.as_deref(),
        Some(expected_identity.join_key.as_str())
    );
    assert_eq!(
        legacy_rows[0].decision_plane.as_deref(),
        Some(expected_identity.decision_plane.as_str())
    );

    let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
        &std::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE)).unwrap(),
    )
    .unwrap();
    assert!(manifest.writer_finalized);
    let audit = audit_pr2c_single_run_v1(temp.path(), &[legacy_v33_path]);
    if manifest.summary_parts[0].build_worktree_clean {
        assert_ne!(
            audit.unwrap().terminal_class,
            MetricContractAuditTerminalClassV1::FailSchemaOrReplay
        );
    } else {
        let error = audit.unwrap_err().to_string();
        assert!(
            error.contains("unknown, dirty, or incomplete run/build/schema provenance"),
            "dirty build provenance must fail closed with the exact provenance error: {error}"
        );
    }
}

#[test]
fn runtime_terminal_source_keeps_raw_v33_and_builds_pr2c_only_after_the_off_gate() {
    let source = include_str!("../src/oracle_runtime.rs");
    let helper_start = source
        .find("fn build_pr2c_pair_if_enabled(")
        .expect("runtime owns one opt-in pair helper");
    let helper = &source[helper_start
        ..source[helper_start..]
            .find("\nfn freeze_coordination_decision_snapshot_for_runtime(")
            .map(|offset| helper_start + offset)
            .expect("pair helper has a bounded source region")];
    let disabled_gate = helper
        .find("if !ctx.decision_logger.metric_contract_pr2c_enabled()")
        .expect("PR2C OFF exits before context extraction and second compute");
    let context = helper
        .find("pr2c_legacy_live_context(&buy_log)")
        .expect("pair identity comes from lightweight logger-owned provenance");
    let build = helper
        .find("build_pr2c_terminal_pair(session, assessment, &routed_context")
        .expect("pair builder consumes only the typed routed context");
    assert!(disabled_gate < context && context < build);

    let spawn_start = source
        .find("fn spawn_gatekeeper_decision_logs(")
        .expect("terminal logger helper exists");
    let spawn = &source[spawn_start
        ..source[spawn_start..]
            .find("\n#[derive(Debug, Error)]")
            .map(|offset| spawn_start + offset)
            .expect("terminal logger helper has a bounded source region")];
    let v33 = spawn
        .find("log_gatekeeper_buy_decision(buy_log)")
        .expect("the unchanged raw v33 payload is enqueued");
    let pair = spawn
        .find("log_metric_contract_pair(metric_contract_pair)")
        .expect("pair is enqueued after v33");
    assert!(v33 < pair);

    let logger = include_str!("../../ghost-brain/src/oracle/decision_logger.rs");
    assert!(logger.contains("WriteGatekeeperBuy(GatekeeperBuyLog)"));
    assert!(logger.contains("for mut plane_log in expand_gatekeeper_plane_logs(log)"));
    assert!(!logger.contains("struct RoutedGatekeeperDecisionV1"));
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

#[tokio::test]
async fn full_path_timer_is_continuous_across_snapshot_pair_and_writer_boundaries() {
    let full_path_started = Instant::now();
    let frozen = frozen_inputs_fixture();
    let timed = frozen.build_timed_from(full_path_started);
    let policy = common::equal_policy();
    let pair = ghost_launcher::metric_contracts::build_pr2c_timed_paired_record_v1(
        &timed,
        &ghost_launcher::metric_contracts::Pr2cDecisionRecordContextV1 {
            record_identity: ghost_core::metric_contracts::MetricEvidenceRecordIdentityV1::try_new(
                "continuous-timer-run",
                "continuous-timer-join",
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
    let at_pair_boundary_us = pair.metric_contract_build_and_serialize_us;

    // A segmented-sum implementation would omit this gap. The writer must
    // observe it through the single carried monotonic origin.
    tokio::time::sleep(std::time::Duration::from_millis(6)).await;
    let temp = tempfile::tempdir().unwrap();
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    stats.record_enqueue_wait_us(0);
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    config.build_worktree_clean = true;
    let mut writer = MetricContractPairedWriterV1::open(config, Arc::clone(&stats))
        .await
        .unwrap();
    writer.write_pair(pair).await.unwrap();
    writer.finalize().await.unwrap();

    let measured = stats.snapshot().metric_contract_build_and_serialize_us;
    assert_eq!(measured.sample_count, 1);
    assert!(measured.max_us >= 6_000, "{measured:#?}");
    assert!(measured.max_us > u64::from(at_pair_boundary_us));
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
async fn post_rename_directory_sync_failure_cannot_leave_a_valid_completion_proof() {
    let temp = tempfile::tempdir().unwrap();
    let pair = paired_fixture("post-rename-failure-run", "post-rename-failure-join");
    let stats = Arc::new(MetricContractPairedWriterStatsV1::default());
    let mut config = MetricContractPairedWriterConfigV1::new(
        temp.path().to_path_buf(),
        "fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9",
    );
    config.build_worktree_clean = true;
    config.fault_injection = Some(MetricContractWriterFaultInjectionV1::FinalManifestDirectorySync);
    let mut writer = MetricContractPairedWriterV1::open(config, Arc::clone(&stats))
        .await
        .unwrap();
    writer.write_pair(pair.clone()).await.unwrap();
    writer.finalize().await.unwrap_err();

    assert!(stats.snapshot().evidence_run_invalid);
    assert!(!temp
        .path()
        .join(METRIC_CONTRACT_COMPLETION_PROOF_V1_FILE)
        .exists());
    let manifest_path = temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
    let mut manifest: MetricContractRotationManifestV1 =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    // Even if the pre-sync rename left a true manifest visible and recovery
    // also failed, the independent completion proof is absent and audit must
    // reject the run.
    manifest.writer_finalized = true;
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let v33 = temp.path().join("gatekeeper_v2_decisions.jsonl");
    std::fs::write(
        &v33,
        format!(
            "{}\n",
            serde_json::to_string(&common::current_v33_log(&pair)).unwrap()
        ),
    )
    .unwrap();
    assert!(audit_pr2c_single_run_v1(temp.path(), &[v33]).is_err());
}

#[tokio::test]
async fn pr2c_disabled_preserves_exact_v33_bytes_and_opens_no_pr2c_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let pair = paired_fixture("disabled-run", "disabled-join");
    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        gatekeeper_rollout_profile: "profile-a".to_string(),
        gatekeeper_config_hash: common::TEST_GATEKEEPER_CONFIG_HASH.to_string(),
        gatekeeper_run_id: Some("disabled-run".to_string()),
        gatekeeper_session_id: Some("disabled-session".to_string()),
        brain_config_path: Some("ghost-brain/config/ghost_brain_config.toml".to_string()),
        brain_config_hash: Some(common::TEST_BRAIN_CONFIG_HASH.to_string()),
        channel_buffer_size: 4,
        metric_contract_pr2c_enabled: false,
        enabled: true,
    });
    assert!(!logger.metric_contract_pr2c_enabled());

    let mut raw = current_v33_unrouted_log(&pair);
    raw.join_key = Some("disabled-join".to_string());
    raw.v25_shadow_verdict_type = Some("REJECT_LOW_TRAJECTORY".to_string());
    raw.v25_shadow_reason_chain = Some("shadow-only regression".to_string());
    logger.log_gatekeeper_buy_decision(raw).await;
    assert_eq!(
        logger.log_metric_contract_pair(pair),
        Err(MetricContractEnqueueErrorV1::WriterDisabled)
    );
    logger.shutdown().await.unwrap();

    for (plane, version) in [
        ("legacy_live", LEGACY_GATEKEEPER_VERSION),
        ("v25_shadow", GATEKEEPER_VERSION),
    ] {
        let path = temp
            .path()
            .join("profile-a")
            .join(version)
            .join(plane)
            .join(common::TEST_GATEKEEPER_CONFIG_HASH)
            .join(GATEKEEPER_DECISIONS_JSONL);
        let bytes = std::fs::read_to_string(path).unwrap();
        let logged: ghost_brain::oracle::GatekeeperBuyLog =
            serde_json::from_str(bytes.trim_end()).unwrap();
        assert_eq!(logged.decision_plane.as_deref(), Some(plane));
        assert_eq!(logged.run_id.as_deref(), Some("disabled-run"));
        assert_eq!(logged.join_key.as_deref(), Some("disabled-join"));
        assert_eq!(
            logged.config_hash.as_deref(),
            Some(common::TEST_GATEKEEPER_CONFIG_HASH)
        );
        assert_eq!(
            logged.brain_config_hash.as_deref(),
            Some(common::TEST_BRAIN_CONFIG_HASH)
        );
    }
    for file_name in [
        METRIC_CONTRACT_SUMMARY_V34_FILE,
        METRIC_CONTRACT_EVIDENCE_V1_FILE,
        METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE,
        METRIC_CONTRACT_COMPLETION_PROOF_V1_FILE,
    ] {
        assert!(!temp.path().join(file_name).exists());
    }
    assert_eq!(
        logger.metric_contract_writer_stats(),
        MetricContractPairedWriterStatsV1::default().snapshot()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn saturated_pr2c_queue_is_non_blocking_keeps_v33_writable_and_invalidates_run() {
    let temp = tempfile::tempdir().unwrap();
    let pair = paired_fixture("saturation-run", "saturation-join");
    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        gatekeeper_rollout_profile: "profile-a".to_string(),
        gatekeeper_config_hash: common::TEST_GATEKEEPER_CONFIG_HASH.to_string(),
        gatekeeper_run_id: Some("saturation-run".to_string()),
        gatekeeper_session_id: Some("saturation-session".to_string()),
        brain_config_path: Some("ghost-brain/config/ghost_brain_config.toml".to_string()),
        brain_config_hash: Some(common::TEST_BRAIN_CONFIG_HASH.to_string()),
        channel_buffer_size: 1,
        metric_contract_pr2c_enabled: true,
        enabled: true,
    });

    let mut raw = current_v33_unrouted_log(&pair);
    raw.join_key = Some("saturation-join".to_string());

    // A current-thread runtime cannot schedule the spawned PR2C worker until
    // this task yields. The first synchronous try_send therefore fills the
    // one-slot queue and the second deterministically proves typed Full.
    logger.log_metric_contract_pair(pair.clone()).unwrap();
    let started = Instant::now();
    assert_eq!(
        logger.log_metric_contract_pair(pair),
        Err(MetricContractEnqueueErrorV1::QueueFull)
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(10));

    // v33 uses a different queue and worker, so it remains writable while the
    // PR2C branch is saturated.
    logger.log_gatekeeper_buy_decision(raw).await;
    logger.shutdown().await.unwrap();

    let v33_path = temp
        .path()
        .join("profile-a")
        .join(LEGACY_GATEKEEPER_VERSION)
        .join("legacy_live")
        .join(common::TEST_GATEKEEPER_CONFIG_HASH)
        .join(GATEKEEPER_DECISIONS_JSONL);
    assert_eq!(
        std::fs::read_to_string(&v33_path).unwrap().lines().count(),
        1
    );
    let stats = logger.metric_contract_writer_stats();
    assert_eq!(stats.writer_queue_high_water, 1);
    assert_eq!(stats.queue_full_total, 1);
    assert_eq!(stats.queue_closed_total, 0);
    assert_eq!(stats.queue_send_failures_total, 1);
    assert_eq!(stats.queue_dropped_rows_total, 1);
    assert_eq!(stats.missing_pair_total, 1);
    assert!(stats.evidence_run_invalid);

    let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
        &std::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE)).unwrap(),
    )
    .unwrap();
    assert!(manifest.writer_finalized);
    assert!(manifest.writer_stats.evidence_run_invalid);
    let audit = audit_pr2c_single_run_v1(temp.path(), &[v33_path]);
    if manifest.summary_parts[0].build_worktree_clean {
        assert_eq!(
            audit.unwrap().terminal_class,
            MetricContractAuditTerminalClassV1::FailResourceBudget
        );
    } else {
        let error = audit.unwrap_err().to_string();
        assert!(
            error.contains("unknown, dirty, or incomplete run/build/schema provenance"),
            "dirty build provenance must fail closed with the exact provenance error: {error}"
        );
    }
}

#[tokio::test]
async fn closed_pr2c_queue_returns_typed_error_and_marks_in_memory_run_invalid() {
    let temp = tempfile::tempdir().unwrap();
    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        channel_buffer_size: 1,
        metric_contract_pr2c_enabled: true,
        enabled: true,
        ..DecisionLoggerConfig::default()
    });
    logger.shutdown().await.unwrap();

    assert_eq!(
        logger.log_metric_contract_pair(paired_fixture("closed-run", "closed-join")),
        Err(MetricContractEnqueueErrorV1::ChannelClosed)
    );
    let stats = logger.metric_contract_writer_stats();
    assert_eq!(stats.queue_closed_total, 1);
    assert_eq!(stats.queue_send_failures_total, 1);
    assert_eq!(stats.queue_dropped_rows_total, 1);
    assert!(stats.evidence_run_invalid);
}

#[tokio::test]
async fn logger_try_send_and_queue_high_water_are_recorded_without_drops() {
    const SAMPLE_COUNT: u64 = 128;
    const QUEUE_CAPACITY: usize = 1_000;
    let temp = tempfile::tempdir().unwrap();
    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        channel_buffer_size: QUEUE_CAPACITY,
        metric_contract_pr2c_enabled: true,
        enabled: true,
        ..DecisionLoggerConfig::default()
    });
    let pair = paired_fixture("enqueue-resource-run", "enqueue-resource-join");
    for _ in 0..SAMPLE_COUNT {
        logger.log_metric_contract_pair(pair.clone()).unwrap();
    }
    let enqueued = logger.metric_contract_writer_stats();
    assert_eq!(enqueued.logger_enqueue_wait_us.sample_count, SAMPLE_COUNT);
    let enqueue_p99_us = enqueued
        .logger_enqueue_wait_us
        .percentile_upper_bound_us(99)
        .unwrap();
    eprintln!(
        "PR2C isolated queue diagnostic: logger_try_send_us_p99={enqueue_p99_us} writer_queue_high_water={} queue_capacity={QUEUE_CAPACITY}",
        enqueued.writer_queue_high_water
    );
    assert!(enqueued.writer_queue_high_water < (QUEUE_CAPACITY as u64 * 8 / 10));

    logger.shutdown().await.unwrap();
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
    assert_eq!(completed.queue_full_total, 0);
    assert_eq!(completed.queue_closed_total, 0);
    assert_eq!(completed.summary_write_failures_total, 0);
    assert_eq!(completed.evidence_write_failures_total, 0);
    assert!(!completed.evidence_run_invalid);
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
        (16, 200)
    };
    let policy = common::equal_policy();
    // Prime the producer/evidence/projection/pair code before the diagnostic
    // logger run. Latency percentiles are reported, not used as merge gates.
    for index in 0..warmup_samples {
        let full_path_started = Instant::now();
        let frozen = frozen_inputs_fixture();
        let timed = frozen.build_timed_from(full_path_started);
        let pair = ghost_launcher::metric_contracts::build_pr2c_timed_paired_record_v1(
            &timed,
            &ghost_launcher::metric_contracts::Pr2cDecisionRecordContextV1 {
                record_identity:
                    ghost_core::metric_contracts::MetricEvidenceRecordIdentityV1::try_new(
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
    let logger = DecisionLogger::new(DecisionLoggerConfig {
        log_dir: temp.path().to_path_buf(),
        gatekeeper_log_dir: temp.path().to_path_buf(),
        gatekeeper_rollout_profile: "profile-a".to_string(),
        gatekeeper_config_hash: common::TEST_GATEKEEPER_CONFIG_HASH.to_string(),
        gatekeeper_run_id: Some("resource-run".to_string()),
        gatekeeper_session_id: Some("resource-session".to_string()),
        brain_config_path: Some("ghost-brain/config/ghost_brain_config.toml".to_string()),
        brain_config_hash: Some(common::TEST_BRAIN_CONFIG_HASH.to_string()),
        channel_buffer_size: 32,
        metric_contract_pr2c_enabled: true,
        enabled: true,
    });
    let mut comparator = Vec::with_capacity(measured_samples);
    let mut wire_bytes = Vec::with_capacity(measured_samples);
    let mut complete_snapshot_build = Vec::with_capacity(measured_samples);
    let mut context_validation = Vec::with_capacity(measured_samples);
    let mut evidence_build = Vec::with_capacity(measured_samples);
    let mut evidence_validation = Vec::with_capacity(measured_samples);
    let mut projection_build = Vec::with_capacity(measured_samples);
    let mut pair_construction = Vec::with_capacity(measured_samples);
    for index in 0..measured_samples {
        // The timer begins before the fixture invokes the canonical family
        // producers, then crosses the full evidence/projection build. The
        // paired sample below additionally includes the real comparator and
        // writer-owned final-byte serialization.
        let full_path_started = Instant::now();
        let frozen = frozen_inputs_fixture();
        let rebuilt = frozen.build_timed_from(full_path_started);
        complete_snapshot_build.push(u128::from(
            rebuilt.timings().metric_contract_build_and_validate_us,
        ));
        context_validation.push(u128::from(rebuilt.timings().context_validation_us));
        evidence_build.push(u128::from(rebuilt.timings().evidence_build_us));
        evidence_validation.push(u128::from(rebuilt.timings().evidence_validation_us));
        projection_build.push(u128::from(
            rebuilt.timings().projection_build_and_validate_us,
        ));
        let join_key = format!("join-{index}");
        let mut terminal = current_v33_unrouted_fixture(&rebuilt.snapshot().compact_projection);
        terminal.log.join_key = Some(join_key.clone());
        let routed_context = logger.pr2c_legacy_live_context(&terminal.log).unwrap();
        let authoritative = pr2c_policy_equivalence_snapshot_v1(
            &terminal.assessment,
            terminal.assessment.decision.as_ref(),
        );
        let started = Instant::now();
        let comparator_decision =
            evaluate_policy_from_assessment(&terminal.assessment, &terminal.config);
        let comparator_policy =
            pr2c_policy_equivalence_snapshot_v1(&terminal.assessment, Some(&comparator_decision));
        assert!(authoritative.compare(&comparator_policy).is_zero_drift());
        let comparator_us = u32::try_from(started.elapsed().as_micros()).unwrap();
        comparator.push(u128::from(comparator_us));

        let pair = ghost_launcher::metric_contracts::build_pr2c_timed_paired_record_v1(
            &rebuilt,
            &ghost_launcher::metric_contracts::Pr2cDecisionRecordContextV1 {
                record_identity: routed_context.record_identity().clone(),
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
                authoritative_policy: &authoritative,
                comparator_policy: &comparator_policy,
                comparator_evaluable: terminal.assessment.decision.is_some(),
                comparator_elapsed_us: comparator_us,
                metric_contract_serialize_us: 0,
                metric_contract_build_and_serialize_us: 0,
                projection_build_and_validate_us: 0,
                gatekeeper_config_hash: routed_context.gatekeeper_config_hash().as_str(),
                brain_config_hash: Some(routed_context.brain_config_hash().as_str()),
            },
        )
        .unwrap();
        pair_construction.push(u128::from(
            pair.metric_contract_build_and_serialize_us
                .checked_sub(
                    rebuilt
                        .timings()
                        .metric_contract_build_and_validate_us
                        .checked_add(comparator_us)
                        .unwrap(),
                )
                .unwrap(),
        ));
        wire_bytes.push(
            pair.decision_time_projection
                .authoritative_serialized_size_bytes()
                .unwrap(),
        );
        // Exercise both production queues. Their independent workers preserve
        // routing identity without making v33 wait for PR2C fsync/manifest I/O.
        logger.log_gatekeeper_buy_decision(terminal.log).await;
        logger.log_metric_contract_pair(pair).unwrap();
        let expected_rows = u64::try_from(index + 1).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if logger
                    .metric_contract_writer_stats()
                    .evidence_rows_written_total
                    >= expected_rows
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("production DecisionLogger did not persist the measured pair in time");
    }
    logger.shutdown().await.unwrap();
    let manifest: MetricContractRotationManifestV1 = serde_json::from_slice(
        &std::fs::read(temp.path().join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE)).unwrap(),
    )
    .unwrap();
    assert!(manifest.writer_finalized);
    let writer_stats = manifest.writer_stats;
    writer_stats
        .logger_enqueue_wait_us
        .validate(measured_samples as u64)
        .unwrap();
    writer_stats
        .metric_contract_build_and_serialize_us
        .validate(measured_samples as u64)
        .unwrap();
    writer_stats
        .projection_build_and_validate_us
        .validate(measured_samples as u64)
        .unwrap();
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
    let build_max = writer_stats.metric_contract_build_and_serialize_us.max_us;
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
        "PR2C durable latency diagnostic: metric_contract_build_and_serialize_us_p50={build_p50} metric_contract_build_and_serialize_us_p95={build_p95} metric_contract_build_and_serialize_us_p99={build_p99} metric_contract_build_and_serialize_us_max={build_max} complete_snapshot_build_validate_us_p50={snapshot_p50} complete_snapshot_build_validate_us_p95={snapshot_p95} complete_snapshot_build_validate_us_p99={snapshot_p99} context_validation_us_p50={context_p50} context_validation_us_p95={context_p95} context_validation_us_p99={context_p99} evidence_build_us_p50={evidence_build_p50} evidence_build_us_p95={evidence_build_p95} evidence_build_us_p99={evidence_build_p99} evidence_validation_us_p50={evidence_validation_p50} evidence_validation_us_p95={evidence_validation_p95} evidence_validation_us_p99={evidence_validation_p99} projection_build_validate_us_p50={projection_p50} projection_build_validate_us_p95={projection_p95} projection_build_validate_us_p99={projection_p99} terminal_pair_construction_us_p50={pair_p50} terminal_pair_construction_us_p95={pair_p95} terminal_pair_construction_us_p99={pair_p99} metric_contract_serialize_us_p50={serialize_p50} metric_contract_serialize_us_p95={serialize_p95} metric_contract_serialize_us_p99={serialize_p99} comparator_elapsed_us_p50={comparator_p50} comparator_elapsed_us_p95={comparator_p95} comparator_elapsed_us_p99={comparator_p99} projection_wire_json_bytes_p95={wire_p95} projection_wire_json_bytes_max={wire_max} sidecar_json_bytes_p95={sidecar_p95} sidecar_json_bytes_p99={sidecar_p99} v34_json_bytes_p95={v34_p95}"
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
}
