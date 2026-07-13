#[path = "common/metric_contracts_pr2c.rs"]
mod common;

use common::{complete_snapshot_fixture, equal_policy};
use ghost_core::metric_contracts::{
    ComparatorDeltaStatusV1, MetricContractPolicyEquivalenceSnapshotV1, MetricContractRolloutMode,
    MetricEvidenceRecordIdentityV1,
};
use ghost_launcher::metric_contracts::{
    build_pr2c_paired_record_v1, Pr2cDecisionRecordContextV1, Pr2cRecordBuildErrorV1,
};

#[test]
fn equivalence_comparator_reports_exact_zero_drift() {
    let policy = equal_policy();
    let deltas = policy.compare(&policy);
    assert!(deltas.is_zero_drift());
}

#[test]
fn decision_record_builder_fails_closed_on_any_equivalence_policy_drift() {
    let fixture = complete_snapshot_fixture();
    let authoritative = equal_policy();
    let mut candidate = authoritative.clone();
    candidate.primary_reason_code.push_str("_DRIFT");
    let result = build_pr2c_paired_record_v1(
        &fixture.complete,
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                "run-a",
                "join-a",
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: None,
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &fixture.profile,
            effective_config: &fixture.effective,
            authoritative_policy: &authoritative,
            comparator_policy: &candidate,
            counterfactual_delta_present: false,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 20,
            metric_contract_build_and_serialize_us: 30,
            projection_build_and_validate_us: 20,
            gatekeeper_config_hash: "gatekeeper-config-a",
            brain_config_hash: Some("brain-config-a"),
            writer_timestamp_ms: 20_000,
        },
    );
    assert!(matches!(result, Err(Pr2cRecordBuildErrorV1::PolicyDrift)));
}

#[test]
fn counterfactual_delta_is_diagnostic_when_equivalence_lane_has_zero_drift() {
    let fixture = complete_snapshot_fixture();
    let policy = equal_policy();
    let pair = build_pr2c_paired_record_v1(
        &fixture.complete,
        &Pr2cDecisionRecordContextV1 {
            record_identity: MetricEvidenceRecordIdentityV1::try_new(
                "run-counterfactual",
                "join-counterfactual",
                "legacy_live",
            )
            .unwrap(),
            stable_event_identity: None,
            rollout_mode: MetricContractRolloutMode::Legacy,
            profile: &fixture.profile,
            effective_config: &fixture.effective,
            authoritative_policy: &policy,
            comparator_policy: &policy,
            counterfactual_delta_present: true,
            comparator_elapsed_us: 10,
            metric_contract_serialize_us: 20,
            metric_contract_build_and_serialize_us: 30,
            projection_build_and_validate_us: 20,
            gatekeeper_config_hash: "gatekeeper-config-a",
            brain_config_hash: Some("brain-config-a"),
            writer_timestamp_ms: 20_000,
        },
    )
    .unwrap();

    assert!(pair.decision_v34.counterfactual_delta_present);
    assert!(pair.decision_v34.equivalence_deltas.is_zero_drift());
}

#[test]
fn runtime_comparator_is_a_real_pure_policy_evaluation_without_live_or_execution_reads() {
    let source = include_str!("../src/oracle_runtime.rs");
    let start = source
        .find("// The equivalence comparator performs a real second")
        .unwrap();
    let end = source[start..]
        .find("let comparator_elapsed_us")
        .map(|offset| start + offset)
        .unwrap();
    let comparator = &source[start..end];
    assert!(comparator.contains("evaluate_policy_from_assessment"));
    for forbidden in [
        "session.read",
        ".await",
        "run_iwim",
        "execute_",
        "live_state",
    ] {
        assert!(
            !comparator.contains(forbidden),
            "comparator unexpectedly contains {forbidden}"
        );
    }
}

#[test]
fn terminal_snapshot_lock_scope_ends_before_async_queue_and_filesystem_work() {
    let source = include_str!("../src/oracle_runtime.rs");
    let start = source.find("fn build_pr2c_terminal_pair(").unwrap();
    let end = source[start..]
        .find("fn freeze_coordination_decision_snapshot_for_runtime")
        .map(|offset| start + offset)
        .unwrap();
    let boundary = &source[start..end];
    assert!(boundary.contains("let session = session.read();"));
    assert!(!boundary.contains(".await"));
    assert!(boundary.contains("DecisionSnapshotMismatch"));
    assert!(boundary.contains("metric_contract_decision_projection_v1"));
    assert!(boundary.contains("snapshot.compact_projection"));

    let logger = include_str!("../../ghost-brain/src/oracle/decision_logger.rs");
    let start = logger
        .find("pub async fn log_metric_contract_pair(")
        .unwrap();
    let end = logger[start..]
        .find("pub fn metric_contract_writer_stats")
        .map(|offset| start + offset)
        .unwrap();
    let enqueue = &logger[start..end];
    assert!(!enqueue.contains("session.read"));
    assert!(!enqueue.contains("parking_lot"));
}

#[test]
fn equivalence_comparator_detects_every_frozen_drift_class() {
    let authoritative = equal_policy();
    let cases: Vec<(
        fn(&mut MetricContractPolicyEquivalenceSnapshotV1),
        fn(
            &ghost_core::metric_contracts::MetricContractComparatorSummaryV1,
        ) -> ComparatorDeltaStatusV1,
    )> = vec![
        (|v| v.verdict.push('x'), |d| d.verdict),
        (
            |v| v.primary_reason_code.push('x'),
            |d| d.primary_reason_code,
        ),
        (
            |v| v.ordered_reason_chain.push("x".to_string()),
            |d| d.ordered_reason_chain,
        ),
        (|v| v.phase_pass_vector[0] = true, |d| d.phase_pass_vector),
        (|v| v.soft_points += 1, |d| d.soft_points),
        (
            |v| v.selector_soft_score_bits += 1,
            |d| d.selector_soft_score,
        ),
        (
            |v| v.hard_fail_classification.push('x'),
            |d| d.hard_fail_classification,
        ),
    ];
    for (mutate, select) in cases {
        let mut candidate = authoritative.clone();
        mutate(&mut candidate);
        let deltas = authoritative.compare(&candidate);
        assert_eq!(select(&deltas), ComparatorDeltaStatusV1::Different);
        assert!(!deltas.is_zero_drift());
    }
}
