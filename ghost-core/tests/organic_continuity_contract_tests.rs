use ghost_core::checkpoint::{
    organic_continuity_experimental_score_contract_hash, EvidenceDegradedReason, EvidenceStatus,
    ManipulationContradictionFeatures, MaterializedFeatureSet, OrganicBroadeningFeatures,
    OrganicContinuityBucketReasonV1, OrganicContinuityExperimentalScoreStatusV1,
};

fn materialized_features(
    organic_buy_ratio_mean: f64,
    organic_buy_ratio_max: f64,
    buy_count: u64,
    sol_buy_ratio: f64,
) -> MaterializedFeatureSet {
    let mut features = MaterializedFeatureSet::default();

    features.tx_intel_features.buy_count = buy_count;
    features.tx_intel_features.buy_ratio = organic_buy_ratio_mean;
    features.tx_intel_features.sol_buy_ratio = sol_buy_ratio;
    features.tx_intel_features.burst_ratio = 0.08;
    features.tx_intel_features.same_ms_tx_ratio = 0.02;
    features.alpha_fingerprint.flipper_presence_ratio = Some(0.10);
    features.alpha_fingerprint.fixed_size_buy_ratio = Some(0.20);
    features.manipulation_contradictions = ManipulationContradictionFeatures {
        contradiction_score: 0.15,
        ..ManipulationContradictionFeatures::default()
    };
    features.organic_broadening = OrganicBroadeningFeatures {
        sequence_available: true,
        total_tx_count: 9,
        total_unique_signers: 6,
        t0_tx_count: 2,
        t1_tx_count: 3,
        t2_tx_count: 4,
        t0_unique_signers: 2,
        t1_unique_signers: 3,
        t2_unique_signers: 4,
        t1_vs_t0_unique_signer_delta: 1,
        t2_vs_t1_unique_signer_delta: 1,
        tx_count_growth_ratio: 2.0,
        unique_signer_growth_ratio: 2.0,
        buy_ratio_mean: organic_buy_ratio_mean,
        buy_ratio_min: organic_buy_ratio_mean.min(organic_buy_ratio_max),
        buy_ratio_max: organic_buy_ratio_max,
        max_segment_hhi: 0.30,
        min_segment_hhi: 0.10,
        signer_growth_t2_t0: 2,
        hhi_delta_t2_t0: -0.05,
        tx_count_growth_vs_signer_growth: 0.0,
        new_signer_ratio_t2: 0.50,
        broadening_score: 0.42,
        status: EvidenceStatus::Clean,
        degraded_reasons: vec![EvidenceDegradedReason::SegmentSequencePartial],
    };

    features
}

#[test]
fn evidence_vector_serializes_all_raw_organic_fields_and_boundaries() {
    let evidence = materialized_features(0.25, 0.60, 4, 0.5099).organic_continuity_evidence_v1();
    let value = serde_json::to_value(&evidence).expect("serialize organic continuity evidence");
    let raw = value
        .get("raw_organic_fields")
        .and_then(serde_json::Value::as_object)
        .expect("raw organic fields object");

    for key in [
        "sequence_available",
        "total_tx_count",
        "total_unique_signers",
        "t0_tx_count",
        "t1_tx_count",
        "t2_tx_count",
        "t0_unique_signers",
        "t1_unique_signers",
        "t2_unique_signers",
        "t1_vs_t0_unique_signer_delta",
        "t2_vs_t1_unique_signer_delta",
        "tx_count_growth_ratio",
        "unique_signer_growth_ratio",
        "buy_ratio_mean",
        "buy_ratio_min",
        "buy_ratio_max",
        "max_segment_hhi",
        "min_segment_hhi",
        "signer_growth_t2_t0",
        "hhi_delta_t2_t0",
        "tx_count_growth_vs_signer_growth",
        "new_signer_ratio_t2",
        "broadening_score",
        "status",
        "degraded_reasons",
    ] {
        assert!(raw.contains_key(key), "missing raw organic field {key}");
    }

    let boundaries = value
        .get("claim_boundaries")
        .and_then(serde_json::Value::as_object)
        .expect("claim boundaries object");
    assert!(value
        .get("organic_continuity_experimental_score_v1")
        .is_some());
    assert_eq!(boundaries["diagnostic_only"], true);
    assert_eq!(boundaries["shadow_only"], true);
    assert_eq!(boundaries["changes_gatekeeper_decision"], false);
    assert_eq!(boundaries["changes_execution"], false);
    assert_eq!(boundaries["production_promotion_allowed"], false);
    assert_eq!(boundaries["policy_score"], false);
    assert_eq!(boundaries["runtime_filter"], false);
}

#[test]
fn low_buy_ratio_is_diagnostic_neutral_not_rejected() {
    let evidence = materialized_features(0.20, 0.50, 4, 0.50).organic_continuity_evidence_v1();

    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5OrganicBuyRatioMeanLe025));
    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5OrganicBuyRatioMaxLe06));
    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5BuyCountLe4));
    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5SolBuyRatioLe05099));

    assert!(evidence.claim_boundaries.diagnostic_only);
    assert!(evidence.claim_boundaries.shadow_only);
    assert!(!evidence.claim_boundaries.policy_score);
    assert!(!evidence.claim_boundaries.runtime_filter);
    assert!(!evidence.claim_boundaries.production_promotion_allowed);
    assert_eq!(
        evidence.organic_continuity_experimental_score_v1.status,
        OrganicContinuityExperimentalScoreStatusV1::NotImplemented
    );
    assert!(evidence
        .organic_continuity_experimental_score_v1
        .value
        .is_none());
}

#[test]
fn high_buy_ratio_is_diagnostic_neutral_not_policy_rewarded() {
    let evidence = materialized_features(0.90, 1.00, 12, 0.90).organic_continuity_evidence_v1();

    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5OrganicBuyRatioMeanGt025));
    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5OrganicBuyRatioMaxGt06));
    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5BuyCountGt4));
    assert!(evidence
        .bucket_reasons
        .contains(&OrganicContinuityBucketReasonV1::R4R5SolBuyRatioGt05326));

    assert!(evidence.claim_boundaries.diagnostic_only);
    assert!(!evidence.claim_boundaries.changes_gatekeeper_decision);
    assert!(!evidence.claim_boundaries.policy_score);
    assert!(!evidence.claim_boundaries.runtime_filter);
    assert_eq!(
        evidence.organic_continuity_experimental_score_v1.status,
        OrganicContinuityExperimentalScoreStatusV1::NotImplemented
    );
    assert!(
        evidence
            .organic_continuity_experimental_score_v1
            .not_policy_score
    );
    assert!(
        evidence
            .organic_continuity_experimental_score_v1
            .not_promotion_candidate
    );
    assert!(
        evidence
            .organic_continuity_experimental_score_v1
            .direction_unvalidated
    );
}

#[test]
fn claim_boundaries_forbid_runtime_promotion() {
    let evidence = materialized_features(0.25, 0.60, 4, 0.5173).organic_continuity_evidence_v1();

    assert!(evidence.claim_boundaries.diagnostic_only);
    assert!(evidence.claim_boundaries.shadow_only);
    assert!(!evidence.claim_boundaries.changes_gatekeeper_decision);
    assert!(!evidence.claim_boundaries.changes_execution);
    assert!(!evidence.claim_boundaries.production_promotion_allowed);
    assert!(!evidence.claim_boundaries.policy_score);
    assert!(!evidence.claim_boundaries.runtime_filter);
    assert!(evidence.source.canonical_feature_source == "MaterializedFeatureSet");
    assert!(evidence.source.decision_time_inputs_only);
}

#[test]
fn organic_continuity_contract_excludes_outcome_lifecycle_fields() {
    let evidence = materialized_features(0.25, 0.60, 4, 0.5326).organic_continuity_evidence_v1();
    let value = serde_json::to_value(&evidence).expect("serialize organic continuity evidence");
    let mut keys = Vec::new();
    collect_json_keys(&value, &mut keys);

    for key in keys {
        let lowered = key.to_ascii_lowercase();
        for forbidden in [
            "pnl",
            "terminal",
            "exit",
            "lifecycle",
            "outcome",
            "final_",
            "shadow_lifecycle",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "forbidden outcome/lifecycle field leaked into contract: {key}"
            );
        }
    }
}

#[test]
fn experimental_score_contract_hash_changes_with_schema_or_weight_seed() {
    let base = organic_continuity_experimental_score_contract_hash(
        "organic_continuity_experimental_score_v1",
        "no_weights_v1",
    );
    let schema_changed = organic_continuity_experimental_score_contract_hash(
        "organic_continuity_experimental_score_v2",
        "no_weights_v1",
    );
    let weight_changed = organic_continuity_experimental_score_contract_hash(
        "organic_continuity_experimental_score_v1",
        "weights_v2",
    );

    assert_ne!(base, schema_changed);
    assert_ne!(base, weight_changed);
}

fn collect_json_keys(value: &serde_json::Value, keys: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, nested) in object {
                keys.push(key.clone());
                collect_json_keys(nested, keys);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_keys(item, keys);
            }
        }
        _ => {}
    }
}
