#[path = "common/metric_contracts_pr2c.rs"]
mod common;

use common::{equal_policy, paired_fixture, paired_fixture_with_comparator};
use ghost_core::metric_contracts::{
    CanonicalNullableV1, MetricContractDecisionEvidenceProjectionV1,
    MetricContractDecisionProjectionWireV1, MetricContractDecisionSourceCutoffV1,
    MetricContractEvidenceTransportV1,
};
use ghost_launcher::metric_contracts::{
    replay_metric_contract_record_v2, Pr2cReplayErrorV2, Pr2cReplayInputV2,
};

fn replay_input() -> Pr2cReplayInputV2 {
    let pair = paired_fixture("run-a", "join-a");
    Pr2cReplayInputV2 {
        decision_v34: pair.decision_v34,
        evidence: pair.evidence,
        decision_time_projection: pair.decision_time_projection,
        effective_config: pair.effective_config,
    }
}

#[test]
fn replay_v2_rebuilds_exact_domain_projection_hash_and_wire_roundtrip() {
    let input = replay_input();
    let original = input.decision_time_projection.clone();
    let result = replay_metric_contract_record_v2(input).unwrap();
    assert_eq!(result.rebuilt_projection, original);
    assert_eq!(result.wire_version, 1);
    let wire = MetricContractDecisionProjectionWireV1::try_from_domain(&original).unwrap();
    assert_eq!(wire.try_into_domain().unwrap(), original);
}

#[test]
fn replay_v2_rejects_full_evidence_hash_mismatch_after_serde() {
    let input = replay_input();
    let mut value = serde_json::to_value(&input.evidence).unwrap();
    value["evidence_sha256"] = serde_json::Value::String("f".repeat(64));
    assert!(serde_json::from_value::<MetricContractEvidenceTransportV1>(value).is_err());
}

#[test]
fn replay_v2_rejects_decision_time_projection_vs_rebuilt_projection_mismatch() {
    let mut input = replay_input();
    input.decision_time_projection.dev_buy.mfs_primary_v1.value = CanonicalNullableV1::Value(1.0);
    assert!(matches!(
        replay_metric_contract_record_v2(input),
        Err(Pr2cReplayErrorV2::Projection(_))
            | Err(Pr2cReplayErrorV2::ProjectionFullEvidenceMismatch)
    ));
}

#[test]
fn replay_v2_rejects_global_projection_cutoff_tamper_against_durable_evidence_cutoff() {
    fn replace_all_cutoffs(value: &mut serde_json::Value, cutoff: &serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("source_cutoff") {
                    object.insert("source_cutoff".to_string(), cutoff.clone());
                }
                for child in object.values_mut() {
                    replace_all_cutoffs(child, cutoff);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    replace_all_cutoffs(child, cutoff);
                }
            }
            _ => {}
        }
    }

    let mut input = replay_input();
    let forged_cutoff = MetricContractDecisionSourceCutoffV1::try_new(99_999, Some(999)).unwrap();
    let mut projection = serde_json::to_value(&input.decision_time_projection).unwrap();
    replace_all_cutoffs(
        &mut projection,
        &serde_json::to_value(forged_cutoff).unwrap(),
    );
    input.decision_time_projection =
        serde_json::from_value::<MetricContractDecisionEvidenceProjectionV1>(projection).unwrap();
    assert!(matches!(
        replay_metric_contract_record_v2(input),
        Err(Pr2cReplayErrorV2::Projection(_))
    ));
}

#[test]
fn replay_v2_rejects_rehashed_evidence_cutoff_drift_from_decision_time_projection() {
    let mut input = replay_input();
    input.evidence.payload.source_cutoff =
        MetricContractDecisionSourceCutoffV1::try_new(99_999, Some(999)).unwrap();
    input.evidence = MetricContractEvidenceTransportV1::try_new(
        input.evidence.payload,
        input.evidence.writer_timestamp_ms,
        input.evidence.rotation_part_index,
    )
    .unwrap();
    input.decision_v34.evidence_sha256 = input.evidence.evidence_sha256.clone();
    assert!(matches!(
        replay_metric_contract_record_v2(input),
        Err(Pr2cReplayErrorV2::Projection(_))
    ));
}

#[test]
fn replay_v2_rejects_profile_config_and_pair_mismatch() {
    let mut input = replay_input();
    input.decision_v34.evidence_sha256 =
        ghost_core::metric_contracts::CanonicalHashV1::parse("a".repeat(64)).unwrap();
    assert!(matches!(
        replay_metric_contract_record_v2(input),
        Err(Pr2cReplayErrorV2::PairMismatch)
    ));
}

#[test]
fn replay_v2_rejects_unknown_projection_schema_even_with_a_valid_pair() {
    let mut input = replay_input();
    input.decision_time_projection.schema_version += 1;
    input.decision_v34.metric_contract_schema_version =
        input.decision_time_projection.schema_version;
    assert!(matches!(
        replay_metric_contract_record_v2(input),
        Err(Pr2cReplayErrorV2::Projection(_))
    ));
}

#[test]
fn replay_v2_recomputes_every_projection_derived_v34_summary_field() {
    let mut changed_mask = replay_input();
    changed_mask.decision_v34.measured_fields_mask ^= 1;
    assert!(matches!(
        replay_metric_contract_record_v2(changed_mask),
        Err(Pr2cReplayErrorV2::SummarySemanticMismatch)
    ));

    let mut changed_authoritative = replay_input();
    changed_authoritative
        .decision_v34
        .authoritative_contracts
        .pop();
    assert!(matches!(
        replay_metric_contract_record_v2(changed_authoritative),
        Err(Pr2cReplayErrorV2::SummarySemanticMismatch)
    ));

    let mut changed_comparator = replay_input();
    changed_comparator.decision_v34.comparator_contracts.clear();
    assert!(matches!(
        replay_metric_contract_record_v2(changed_comparator),
        Err(Pr2cReplayErrorV2::SummarySemanticMismatch)
    ));

    let mut changed_counterfactual = replay_input();
    changed_counterfactual
        .decision_v34
        .counterfactual_delta_present ^= true;
    assert!(matches!(
        replay_metric_contract_record_v2(changed_counterfactual),
        Err(Pr2cReplayErrorV2::SummarySemanticMismatch)
    ));
}

#[test]
fn replay_v2_rejects_equivalence_delta_tamper_against_hashed_policy_snapshots() {
    let mut comparator = equal_policy();
    comparator.primary_reason_code.push_str("_DRIFT");
    let pair = paired_fixture_with_comparator(
        "run-policy-evidence",
        "join-policy-evidence",
        &comparator,
        true,
    );
    assert!(pair.decision_v34.equivalence_deltas.has_policy_drift());
    assert!(pair
        .evidence
        .payload
        .policy_equivalence
        .recompute_deltas()
        .has_policy_drift());

    let equal = equal_policy();
    let mut input = Pr2cReplayInputV2 {
        decision_v34: pair.decision_v34,
        evidence: pair.evidence,
        decision_time_projection: pair.decision_time_projection,
        effective_config: pair.effective_config,
    };
    input.decision_v34.equivalence_deltas = equal.compare(&equal);

    assert!(matches!(
        replay_metric_contract_record_v2(input),
        Err(Pr2cReplayErrorV2::SummarySemanticMismatch)
    ));
}

#[test]
fn wire_roundtrip_never_reconstructs_full_evidence_or_owner_event_sidecars() {
    let input = replay_input();
    let wire =
        MetricContractDecisionProjectionWireV1::try_from_domain(&input.decision_time_projection)
            .unwrap();
    let json = serde_json::to_string(&wire).unwrap();
    for forbidden in [
        "owner_states",
        "event_identities",
        "known_non_neutral_buyer_count",
        "contracts",
    ] {
        assert!(!json.contains(forbidden));
    }
}
