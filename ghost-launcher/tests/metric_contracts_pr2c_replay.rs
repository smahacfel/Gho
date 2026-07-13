#[path = "common/metric_contracts_pr2c.rs"]
mod common;

use common::paired_fixture;
use ghost_core::metric_contracts::{
    CanonicalNullableV1, MetricContractDecisionProjectionWireV1, MetricContractEvidenceTransportV1,
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
