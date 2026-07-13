use ghost_core::metric_contracts::{
    metric_contract_projection_wire_v1_mapping_tables,
    metric_contract_projection_wire_v1_tuple_layouts, MetricContractId, MetricEffectiveConfigKeyV1,
};
use ghost_launcher::metric_contracts::{
    pr2b_key_boundary_set_is_closed, Pr2bEffectiveConfigValidationBoundaryV1,
    PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1,
};
use std::collections::BTreeSet;

#[test]
fn pr2b_effective_config_key_boundary_classification_is_closed_unique_and_exact() {
    assert!(pr2b_key_boundary_set_is_closed());
    let expected = ghost_core::metric_contracts::METRIC_EFFECTIVE_CONFIG_KEYS_V1
        .iter()
        .copied()
        .filter(|key| {
            matches!(
                key.contract_id(),
                Some(MetricContractId::FlipRatio)
                    | Some(MetricContractId::ManipulationContradiction)
                    | Some(MetricContractId::ReserveVelocity)
                    | Some(MetricContractId::RecentBuySell)
            )
        })
        .collect::<BTreeSet<_>>();
    let actual = PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1
        .iter()
        .map(|(key, _)| *key)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1.len());

    for frozen in [
        MetricEffectiveConfigKeyV1::FlipCandidateDustThresholdSol,
        MetricEffectiveConfigKeyV1::FlipCandidateDedupeKey,
        MetricEffectiveConfigKeyV1::FlipCandidateDedupeCapacity,
        MetricEffectiveConfigKeyV1::FlipCandidateEvictionPolicy,
        MetricEffectiveConfigKeyV1::FlipCandidateMaxWallets,
        MetricEffectiveConfigKeyV1::FlipCandidateReconnectBehavior,
    ] {
        assert_eq!(
            PR2B_EFFECTIVE_CONFIG_KEY_BOUNDARIES_V1
                .iter()
                .find(|(key, _)| *key == frozen)
                .map(|(_, boundary)| *boundary),
            Some(Pr2bEffectiveConfigValidationBoundaryV1::FrozenProducerBoundaryValidated),
            "{frozen:?}"
        );
    }
}

#[test]
fn materialized_feature_set_contains_only_the_compact_projection_contract_field() {
    let source = include_str!("../../ghost-core/src/checkpoint/types.rs");
    assert_eq!(
        source
            .matches("metric_contract_decision_projection_v1")
            .count(),
        1
    );
    assert!(source.contains("pub metric_contract_decision_projection_v1:"));
    assert!(source.contains("Option<MetricContractDecisionEvidenceProjectionV1>"));
    assert!(!source.contains("MetricContractsEvidenceSetV1"));
}

#[test]
fn active_gatekeeper_policy_does_not_consume_pr2b_projection_fields() {
    for source in [
        include_str!("../src/components/gatekeeper_policy.rs"),
        include_str!("../src/components/gatekeeper_v3.rs"),
    ] {
        assert!(!source.contains("metric_contract_decision_projection_v1"));
        assert!(!source.contains("RecentBuySellDecisionProjectionV1"));
        assert!(!source.contains("FlipDecisionProjectionV1"));
        assert!(!source.contains("ManipulationDecisionProjectionV1"));
        assert!(!source.contains("ReserveVelocityDecisionProjectionV1"));
    }
}

#[test]
fn pr2b_does_not_start_pr2c_type5_or_v34_surfaces() {
    let materialization = include_str!("../src/metric_contracts/pr2b.rs");
    for forbidden in [
        "DecisionLoggerV34",
        "paired_writer",
        "replay_v2",
        "Type5Binding",
        "DualCompute",
    ] {
        assert!(
            !materialization.contains(forbidden),
            "forbidden token {forbidden}"
        );
    }
}

#[test]
fn terminal_snapshot_has_exactly_one_canonical_producer_call_per_family() {
    let source = include_str!("../src/session/observation.rs");
    let start = source.find("pub fn try_materialize_features(").unwrap();
    let end = source[start..]
        .find("/// Compatibility facade for existing callers")
        .map(|offset| start + offset)
        .unwrap();
    let body = &source[start..end];
    for needle in [
        "self.fingerprint_metrics()",
        "compute_sybil_resistance_with_ftdi(",
        "self.materialize_v3_manipulation_contradictions(&materialized)",
        ".metric_contract_snapshot(&materialized.tx_intel_features)",
        ".metric_contract_dev_primary_compatibility_snapshot()",
        "self.metric_contract_recent_exact_timing_snapshot()",
        "self.metric_contract_recent_buy_sell_snapshot()?",
        ".flip_v2_snapshot(decision_timestamp_ms, decision_slot)",
        ".metric_contract_reserve_velocity_snapshot(&self.base_mint)",
        "build_pr2b_timed_complete_metric_contract_snapshot_v1(",
    ] {
        assert_eq!(body.matches(needle).count(), 1, "producer call {needle}");
    }
    assert_eq!(
        body.matches("let fsc = self.funding_source_index.compute_for_transactions(")
            .count(),
        1
    );
}

#[test]
fn compact_json_wire_v1_mapping_is_closed_and_mfs_field_scoped() {
    let tuples = metric_contract_projection_wire_v1_tuple_layouts();
    let enums = metric_contract_projection_wire_v1_mapping_tables();
    assert_eq!(tuples.len(), 18);
    assert_eq!(enums.len(), 28);
    for tables in [tuples, enums] {
        let names = tables
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), tables.len());
        assert!(tables.iter().all(|(_, entries)| !entries.is_empty()));
    }

    let mfs = include_str!("../../ghost-core/src/checkpoint/types.rs");
    assert!(mfs.contains(
        "serialize_with = \"crate::metric_contracts::serialize_optional_projection_wire_v1\""
    ));
    assert!(mfs.contains(
        "deserialize_with = \"crate::metric_contracts::deserialize_optional_projection_wire_v1\""
    ));

    let domain = include_str!("../../ghost-core/src/metric_contracts/projection.rs");
    let root_start = domain
        .find("pub struct MetricContractDecisionEvidenceProjectionV1")
        .unwrap();
    let root_end = domain[root_start..].find("impl ").unwrap() + root_start;
    let root = &domain[root_start..root_end];
    assert!(!root.contains("serde(rename"));
    assert!(!root.contains("serialize_with"));

    let wire = include_str!("../../ghost-core/src/metric_contracts/projection_wire.rs");
    assert!(wire.contains("pub w: u16"));
    assert!(wire.contains("pub d: Vec<Value>"));
    assert!(wire.contains("#[serde(deny_unknown_fields)]"));
}
